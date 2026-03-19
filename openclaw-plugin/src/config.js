const VALID_MEMORY_TYPES = new Set([
  "episodic",
  "semantic",
  "procedural",
  "emotional",
  "reflective",
]);

const DEFAULTS = {
  baseUrl: "http://127.0.0.1:7878",
  topK: 5,
  memoryType: "episodic",
  saveAssistantMessages: false,
  minContentChars: 8,
};

function clampInteger(value, min, max, fallback) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, Math.floor(parsed)));
}

function normalizeBaseUrl(baseUrl) {
  const raw = String(baseUrl || DEFAULTS.baseUrl).trim();
  return raw.replace(/\/+$/, "") || DEFAULTS.baseUrl;
}

export function resolveConfig(pluginConfig = {}) {
  const memoryType = VALID_MEMORY_TYPES.has(pluginConfig.memoryType)
    ? pluginConfig.memoryType
    : DEFAULTS.memoryType;

  return {
    baseUrl: normalizeBaseUrl(pluginConfig.baseUrl),
    topK: clampInteger(pluginConfig.topK, 1, 20, DEFAULTS.topK),
    memoryType,
    saveAssistantMessages: pluginConfig.saveAssistantMessages ?? DEFAULTS.saveAssistantMessages,
    minContentChars: clampInteger(
      pluginConfig.minContentChars,
      1,
      200,
      DEFAULTS.minContentChars,
    ),
  };
}

export { DEFAULTS };
