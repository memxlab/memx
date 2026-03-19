#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { exec } from "node:child_process";
import { fileURLToPath } from "node:url";
import readline from "node:readline";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const HOME_DIR = process.env.HOME || process.env.USERPROFILE;
const CONFIG_PATH = path.join(HOME_DIR, ".openclaw", "openclaw.json");
const PLUGIN_ID = "memx-openclaw";
const PLUGIN_DIR = path.join(__dirname, "..");
const STABLE_PLUGIN_DIR = path.join(HOME_DIR, ".openclaw", "plugins", PLUGIN_ID);
const DEFAULT_CONFIG = {
  baseUrl: "http://127.0.0.1:7878",
  topK: 5,
  memoryType: "episodic",
  saveAssistantMessages: false,
  minContentChars: 8,
};

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
});

function log(message) {
  console.log(message);
}

function info(message) {
  log(`[info] ${message}`);
}

function ok(message) {
  log(`[ok] ${message}`);
}

function warn(message) {
  log(`[warn] ${message}`);
}

function fail(message) {
  log(`[error] ${message}`);
}

async function prompt(question) {
  return new Promise((resolve) => {
    rl.question(question, (answer) => resolve(answer.trim()));
  });
}

async function promptWithDefault(label, defaultValue) {
  const answer = await prompt(`${label} (default: ${defaultValue}): `);
  return answer || String(defaultValue);
}

function closeAndExit(code) {
  rl.close();
  process.exit(code);
}

async function pingMemx(baseUrl) {
  const url = `${String(baseUrl).replace(/\/+$/, "")}/memories?limit=1&offset=0`;
  const response = await fetch(url, { signal: AbortSignal.timeout(5000) });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  await response.json();
}

function ensureStablePluginPath() {
  if (path.resolve(PLUGIN_DIR) === path.resolve(STABLE_PLUGIN_DIR)) {
    return STABLE_PLUGIN_DIR;
  }

  fs.mkdirSync(path.dirname(STABLE_PLUGIN_DIR), { recursive: true });
  fs.rmSync(STABLE_PLUGIN_DIR, { recursive: true, force: true });
  fs.cpSync(PLUGIN_DIR, STABLE_PLUGIN_DIR, { recursive: true, force: true });

  const installerPath = path.join(STABLE_PLUGIN_DIR, "bin", "install.js");
  if (fs.existsSync(installerPath)) {
    fs.chmodSync(installerPath, 0o755);
  }

  ok(`Installed plugin files to ${STABLE_PLUGIN_DIR}`);
  return STABLE_PLUGIN_DIR;
}

function loadConfig() {
  if (!fs.existsSync(CONFIG_PATH)) {
    return { exists: false, data: null };
  }

  try {
    return {
      exists: true,
      data: JSON.parse(fs.readFileSync(CONFIG_PATH, "utf-8")),
    };
  } catch (error) {
    return { exists: true, error };
  }
}

function saveConfig(config) {
  fs.mkdirSync(path.dirname(CONFIG_PATH), { recursive: true });
  if (fs.existsSync(CONFIG_PATH)) {
    fs.copyFileSync(CONFIG_PATH, `${CONFIG_PATH}.bak`);
  }
  fs.writeFileSync(CONFIG_PATH, `${JSON.stringify(config, null, 2)}\n`, "utf-8");
}

function ensureConfigShape(config) {
  config.plugins = config.plugins || {};
  config.plugins.load = config.plugins.load || {};
  config.plugins.load.paths = Array.isArray(config.plugins.load.paths)
    ? config.plugins.load.paths
    : [];
  config.plugins.allow = Array.isArray(config.plugins.allow) ? config.plugins.allow : [];
  config.plugins.slots = config.plugins.slots || {};
  config.plugins.entries = config.plugins.entries || {};
}

function mergePluginConfig(existingConfig, overrides = {}) {
  return {
    ...DEFAULT_CONFIG,
    ...(existingConfig || {}),
    ...overrides,
  };
}

function printNextSteps(config) {
  log("");
  log("Next steps:");
  log(`1. Make sure MemX is running at ${config.baseUrl}`);
  log("2. Restart OpenClaw if it was not restarted automatically");
  log("3. Verify with natural language:");
  log('   - "Remember: I like espresso."');
  log('   - "What coffee do I like?"');
  log("");
}

function restartGateway() {
  return new Promise((resolve) => {
    exec("openclaw gateway restart", (error) => {
      if (error) {
        warn(`Could not restart OpenClaw automatically: ${error.message}`);
        warn("Restart manually with: openclaw gateway restart");
      } else {
        ok("OpenClaw gateway restarted.");
      }
      resolve();
    });
  });
}

async function install() {
  info("Installing MemX OpenClaw plugin");

  const configResult = loadConfig();
  if (configResult.error) {
    fail(`Failed to parse ${CONFIG_PATH}: ${configResult.error.message}`);
    closeAndExit(1);
  }

  let config;
  if (!configResult.exists) {
    warn(`OpenClaw config not found at ${CONFIG_PATH}`);
    const answer = await prompt("Create a new OpenClaw config now? (Y/n): ");
    if (answer.toLowerCase() === "n") {
      info("Installation cancelled.");
      closeAndExit(0);
    }
    config = {};
  } else {
    config = configResult.data;
  }

  ensureConfigShape(config);

  const loadPath = ensureStablePluginPath();
  if (!config.plugins.load.paths.includes(loadPath)) {
    config.plugins.load.paths.push(loadPath);
    ok(`Added plugin path: ${loadPath}`);
  } else {
    info(`Plugin path already configured: ${loadPath}`);
  }

  if (!config.plugins.allow.includes(PLUGIN_ID)) {
    config.plugins.allow.push(PLUGIN_ID);
    ok(`Added to allow list: ${PLUGIN_ID}`);
  } else {
    info(`Plugin already allowed: ${PLUGIN_ID}`);
  }

  if (config.plugins.slots.contextEngine !== PLUGIN_ID) {
    const previous = config.plugins.slots.contextEngine || "none";
    config.plugins.slots.contextEngine = PLUGIN_ID;
    ok(`Set contextEngine slot: ${previous} -> ${PLUGIN_ID}`);
  } else {
    info(`contextEngine slot already points to ${PLUGIN_ID}`);
  }

  if (config.plugins.slots.memory !== "none") {
    const previous = config.plugins.slots.memory || "unset";
    config.plugins.slots.memory = "none";
    warn(`Set memory slot to none to avoid conflicts (was: ${previous})`);
  } else {
    info("memory slot already set to none");
  }

  const existingEntry = config.plugins.entries[PLUGIN_ID] || {};
  const hadExistingConfig = !!existingEntry.config;
  existingEntry.enabled = true;

  if (hadExistingConfig) {
    existingEntry.config = mergePluginConfig(existingEntry.config);
    info("Reusing existing plugin config and filling missing defaults.");
  } else {
    const baseUrl = await promptWithDefault("MemX base URL", DEFAULT_CONFIG.baseUrl);
    try {
      await pingMemx(baseUrl);
      ok(`MemX probe succeeded at ${baseUrl}`);
    } catch (error) {
      warn(`MemX probe failed at ${baseUrl}: ${error.message}`);
      warn("You can continue, but automatic recall/save will not work until MemX is running.");
    }

    const topK = Number(await promptWithDefault("Recall topK", DEFAULT_CONFIG.topK));
    const saveAssistantRaw = await promptWithDefault(
      "Save assistant messages too? (true/false)",
      DEFAULT_CONFIG.saveAssistantMessages,
    );
    existingEntry.config = mergePluginConfig(null, {
      baseUrl,
      topK: Number.isFinite(topK) ? topK : DEFAULT_CONFIG.topK,
      saveAssistantMessages: String(saveAssistantRaw).toLowerCase() === "true",
    });
    ok("Created plugin config.");
  }

  config.plugins.entries[PLUGIN_ID] = existingEntry;
  saveConfig(config);
  ok(`Saved OpenClaw config: ${CONFIG_PATH}`);

  const restartNow = await prompt("Restart OpenClaw now? (Y/n): ");
  if (restartNow.toLowerCase() !== "n") {
    await restartGateway();
  } else {
    warn("Remember to restart OpenClaw manually before verifying.");
  }

  printNextSteps(existingEntry.config);
  closeAndExit(0);
}

install().catch((error) => {
  fail(error.stack || error.message);
  closeAndExit(1);
});
