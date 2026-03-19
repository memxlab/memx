import { createRequire } from "node:module";
import { createContextEngine } from "./src/engine.js";

const require = createRequire(import.meta.url);
const pluginMeta = require("./openclaw.plugin.json");

export default function register(api) {
  const log = api.logger || {
    info: (...args) => console.log(...args),
    warn: (...args) => console.warn(...args),
  };

  log.info(`[${pluginMeta.id}] registering MemX OpenClaw plugin`);

  api.registerContextEngine(pluginMeta.id, (pluginConfig) => {
    return createContextEngine(pluginMeta, pluginConfig, api.logger);
  });
}
