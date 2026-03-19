import { request } from "./http.js";

const noop = { info() {}, warn() {} };
const TAG = "[memx-openclaw]";

export async function pingMemx(cfg, log = noop) {
  log.info(`${TAG} probing MemX at ${cfg.baseUrl}`);
  await request(cfg, "GET", "/memories", {
    query: { limit: 1, offset: 0 },
    timeoutMs: 5000,
  });
}

export async function searchMemories(cfg, query, limit, log = noop) {
  log.info(`${TAG} GET /memories/search q=${JSON.stringify(query)} limit=${limit}`);
  return request(cfg, "GET", "/memories/search", {
    query: {
      q: query,
      limit,
    },
  });
}

export async function createMemory(cfg, payload, log = noop) {
  log.info(
    `${TAG} POST /memories type=${payload.type} importance=${payload.importance} chars=${payload.content.length}`,
  );
  return request(cfg, "POST", "/memories", { body: payload });
}
