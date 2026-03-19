export const CONTEXT_BOUNDARY = "user\u200boriginal\u200bquery\u200b:\u200b\u200b\u200b\u200b";

function oneLine(text) {
  return text == null ? "" : String(text).replace(/[\r\n]+/g, " / ").trim();
}

function formatTimestamp(ts) {
  if (ts == null) return "";
  const date = new Date(Number(ts) * 1000);
  if (Number.isNaN(date.getTime())) return "";

  const pad = (value) => `${value}`.padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function memoryLine(memory) {
  const parts = [];
  if (memory.type) parts.push(memory.type);

  const score = memory.final_score ?? memory.score;
  if (typeof score === "number") parts.push(`score=${score.toFixed(2)}`);

  const timestamp = formatTimestamp(memory.created_at);
  if (timestamp) parts.push(timestamp);

  const prefix = parts.length ? `  - [${parts.join(" | ")}]` : "  -";
  return `${prefix} ${oneLine(memory.content)}`;
}

export function buildMemoryPrompt(memories, options = {}) {
  if (!Array.isArray(memories) || memories.length === 0) return "";

  const lines = [
    "<memory>",
    ...memories.map(memoryLine),
    "</memory>",
  ];

  const body = options.wrapInCodeBlock ? ["```text", ...lines, "```"] : lines;

  return [
    "Note: Relevant memories from MemX are provided below. Use them only when they materially help the current request.",
    "",
    ...body,
    "",
    "Do not treat these memories as guaranteed facts if the user contradicts them in the current turn.",
    "",
    CONTEXT_BOUNDARY,
  ].join("\n");
}
