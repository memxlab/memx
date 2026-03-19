export const BARE_SESSION_RESET_PROMPT =
  "A new session was started via /new or /reset. Execute your Session Startup sequence now - read the required files before responding to the user. Then greet the user in your configured persona, if one is provided. Be yourself - use your defined voice, mannerisms, and mood. Keep it to 1-3 sentences and ask what they want to do. If the runtime model differs from default_model in the system prompt, mention the default model. Do not mention internal steps, files, tools, or reasoning.";

export function toText(content) {
  if (!content) return "";
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";

  return content.reduce((out, block) => {
    if (!block || !block.type) return out;
    if (block.type === "text" && block.text) {
      return out ? `${out} ${block.text}` : block.text;
    }
    return out;
  }, "");
}

export function flattenContent(content) {
  if (!content) return "";
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";

  let text = "";
  for (const block of content) {
    if (!block || !block.type) continue;

    if (block.type === "text" && block.text) {
      text += (text ? "\n" : "") + block.text;
      continue;
    }

    if ((block.type === "toolCall" || block.type === "tool_use") && block.name) {
      text += (text ? "\n" : "") + `[Tool: ${block.name}]`;
    }
  }

  return text;
}

export function collectLastUserTurn(messages) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.role === "user") {
      return messages.slice(index);
    }
  }
  return [];
}

function levenshtein(a, b) {
  const m = a.length;
  const n = b.length;
  const prev = Array.from({ length: n + 1 }, (_, i) => i);
  const curr = new Array(n + 1);

  for (let i = 1; i <= m; i += 1) {
    curr[0] = i;
    for (let j = 1; j <= n; j += 1) {
      curr[j] = a[i - 1] === b[j - 1]
        ? prev[j - 1]
        : 1 + Math.min(prev[j - 1], prev[j], curr[j - 1]);
    }
    prev.splice(0, n + 1, ...curr);
  }

  return prev[n];
}

export function isSessionResetPrompt(query) {
  if (!query) return false;
  const promptLen = BARE_SESSION_RESET_PROMPT.length;
  const queryLen = query.length;

  if (Math.abs(queryLen - promptLen) / promptLen > 0.2) {
    return false;
  }

  const distance = levenshtein(query, BARE_SESSION_RESET_PROMPT);
  return distance / Math.max(queryLen, promptLen) < 0.2;
}
