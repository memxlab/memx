import { createMemory, pingMemx, searchMemories } from "./api.js";
import { resolveConfig } from "./config.js";
import {
  collectLastUserTurn,
  flattenContent,
  isSessionResetPrompt,
  toText,
} from "./messages.js";
import { buildMemoryPrompt, CONTEXT_BOUNDARY } from "./prompt.js";

const SESSION_TTL_MS = 2 * 60 * 60 * 1000;
const MAX_CONTENT_CHARS = 8000;

function cap(text) {
  if (!text) return "";
  return text.length > MAX_CONTENT_CHARS ? `${text.slice(0, MAX_CONTENT_CHARS)}...` : text;
}

function stripInjectedContext(text) {
  if (!text) return "";
  const index = text.lastIndexOf(CONTEXT_BOUNDARY);
  if (index < 0) return text;
  return text.slice(index + CONTEXT_BOUNDARY.length).replace(/^\s+/, "");
}

function normalizeText(text) {
  return String(text || "").replace(/\r/g, "").replace(/[ \t]+\n/g, "\n").trim();
}

function roleLabel(role) {
  return role === "assistant" ? "Assistant" : "User";
}

function initState() {
  return {
    turnCount: 0,
    savedUpTo: 0,
    lastActiveTime: Date.now(),
    skipSaveOnce: false,
  };
}

function buildTurnMemory(messages, cfg, context) {
  const lines = [];
  let hasUserContent = false;

  for (const message of messages) {
    if (!message || (message.role !== "user" && message.role !== "assistant")) {
      continue;
    }

    if (message.role === "assistant" && !cfg.saveAssistantMessages) {
      continue;
    }

    let text = normalizeText(flattenContent(message.content));
    if (!text) continue;

    if (message.role === "user") {
      text = normalizeText(stripInjectedContext(text));
      if (isSessionResetPrompt(text)) continue;
    }

    if (!text || text.length < cfg.minContentChars) {
      continue;
    }

    if (message.role === "user") {
      hasUserContent = true;
    }

    lines.push(`${roleLabel(message.role)}: ${cap(text)}`);
  }

  if (lines.length === 0 || !hasUserContent) {
    return null;
  }

  return {
    content: lines.join("\n"),
    type: cfg.memoryType,
    importance: cfg.saveAssistantMessages ? 0.65 : 0.6,
    metadata: {
      source: "openclaw",
      plugin: "memx-openclaw",
      session_key: context.sessionKey,
      turn: context.turnCount,
      roles: cfg.saveAssistantMessages ? ["user", "assistant"] : ["user"],
    },
  };
}

export function createContextEngine(pluginMeta, pluginConfig, logger) {
  const cfg = resolveConfig(pluginConfig);
  const log = logger || {
    info: (...args) => console.log(...args),
    warn: (...args) => console.warn(...args),
  };
  const tag = `[${pluginMeta.id}]`;
  const sessionState = new Map();

  function pruneStaleSessionState() {
    const now = Date.now();
    for (const [sessionKey, state] of sessionState) {
      if (now - (state.lastActiveTime || 0) > SESSION_TTL_MS) {
        sessionState.delete(sessionKey);
        log.info(`${tag} pruned stale session state: ${sessionKey}`);
      }
    }
  }

  function ensureState(sessionKey) {
    if (!sessionState.has(sessionKey)) {
      sessionState.set(sessionKey, initState());
    }
    return sessionState.get(sessionKey);
  }

  return {
    info: {
      id: pluginMeta.id,
      name: pluginMeta.name,
      version: pluginMeta.version,
      ownsCompaction: false,
    },

    async bootstrap({ sessionId, sessionKey }) {
      log.info(`${tag} bootstrap: session=${sessionId}, key=${sessionKey}`);

      try {
        await pingMemx(cfg, log);
        log.info(`${tag} bootstrap: MemX is reachable at ${cfg.baseUrl}`);
      } catch (error) {
        log.warn(`${tag} bootstrap: MemX probe failed: ${error.message}`);
      }

      ensureState(sessionKey);
      return { bootstrapped: true };
    },

    async ingest({ sessionKey, message }) {
      if (message?.isHeartbeat) {
        return { ingested: false };
      }

      ensureState(sessionKey);
      return { ingested: true };
    },

    async ingestBatch({ sessionKey, messages, isHeartbeat }) {
      if (isHeartbeat) {
        return { ingestedCount: 0 };
      }

      ensureState(sessionKey);
      return { ingestedCount: messages?.length || 0 };
    },

    async assemble({ sessionKey, messages }) {
      pruneStaleSessionState();
      const state = ensureState(sessionKey);
      state.lastActiveTime = Date.now();

      const lastUserMessage = [...messages].reverse().find((message) => message.role === "user");
      const query = normalizeText(toText(lastUserMessage?.content));

      if (!query || query.length < 3) {
        return { messages, estimatedTokens: 0 };
      }

      if (isSessionResetPrompt(query)) {
        state.skipSaveOnce = true;
        return { messages, estimatedTokens: 0 };
      }

      try {
        const memories = await searchMemories(cfg, query, cfg.topK, log);
        if (!Array.isArray(memories) || memories.length === 0) {
          return { messages, estimatedTokens: 0 };
        }

        const context = buildMemoryPrompt(memories, { wrapInCodeBlock: true });
        if (!context) {
          return { messages, estimatedTokens: 0 };
        }

        return {
          messages: [
            {
              role: "system",
              content: `[Relevant Memory]\n${context}`,
              _memory: true,
            },
            ...messages,
          ],
          estimatedTokens: Math.floor(context.length / 4),
        };
      } catch (error) {
        log.warn(`${tag} assemble failed: ${error.message}`);
        return { messages, estimatedTokens: 0 };
      }
    },

    async afterTurn({ sessionKey, messages, prePromptMessageCount }) {
      const state = ensureState(sessionKey);
      state.turnCount += 1;
      state.lastActiveTime = Date.now();

      if (state.savedUpTo > messages.length) {
        state.savedUpTo = 0;
      }

      const sliceStart = prePromptMessageCount !== undefined
        ? Math.max(prePromptMessageCount, state.savedUpTo)
        : state.savedUpTo || 0;

      const newMessages = sliceStart > 0
        ? messages.slice(sliceStart)
        : collectLastUserTurn(messages);

      if (state.skipSaveOnce) {
        state.skipSaveOnce = false;
        state.savedUpTo = messages.length;
        return;
      }

      if (newMessages.length === 0) {
        state.savedUpTo = messages.length;
        return;
      }

      const payload = buildTurnMemory(newMessages, cfg, {
        sessionKey,
        turnCount: state.turnCount,
      });

      if (!payload) {
        state.savedUpTo = messages.length;
        return;
      }

      try {
        await createMemory(cfg, payload, log);
        state.savedUpTo = messages.length;
      } catch (error) {
        log.warn(`${tag} afterTurn save failed: ${error.message}`);
      }
    },

    async prepareSubagentSpawn({ sessionKey, subagentId, prompt }) {
      ensureState(sessionKey);
      const query = normalizeText(toText(prompt));
      if (!query || query.length < 3) {
        return {
          prependContext: "",
          metadata: { subagentId },
        };
      }

      try {
        const memories = await searchMemories(cfg, query, Math.min(cfg.topK, 3), log);
        return {
          prependContext: buildMemoryPrompt(memories, { wrapInCodeBlock: false }),
          metadata: { subagentId },
        };
      } catch (error) {
        log.warn(`${tag} prepareSubagentSpawn failed: ${error.message}`);
        return {
          prependContext: "",
          metadata: { subagentId },
        };
      }
    },

    async onSubagentEnded() {},

    async compact({ sessionKey, tokenBudget, currentTokenCount }) {
      const state = sessionState.get(sessionKey);
      if (!state) {
        return { ok: true, compacted: false, reason: "no session state" };
      }

      state.savedUpTo = 0;
      const threshold = tokenBudget ? tokenBudget * 0.8 : 8000;
      const overBudget = currentTokenCount && currentTokenCount > threshold;

      return {
        ok: true,
        compacted: false,
        reason: overBudget
          ? `token count (${currentTokenCount}) exceeds 80% of budget (${tokenBudget})`
          : "within threshold",
      };
    },

    async dispose({ sessionKey } = {}) {
      if (sessionKey) {
        sessionState.delete(sessionKey);
        return;
      }

      sessionState.clear();
    },
  };
}
