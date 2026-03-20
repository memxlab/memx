<div align="center">

<img src="assets/logo-light.png" alt="MemX logo" width="220" />

# MemX

**Local-first long-term memory system for AI assistants**

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![libSQL](https://img.shields.io/badge/Storage-libSQL-blue.svg)](https://github.com/tursodatabase/libsql)

</div>

---

## What is MemX?

Large language models forget everything between sessions. MemX gives your AI assistant a **persistent, searchable memory** that lives entirely on your own machine — no cloud, no subscriptions, no privacy concerns.

Every preference, project note, and past conversation can be stored once and retrieved accurately later. When the AI doesn't know the answer, it says so instead of making something up.

```
You:   "What's my preferred code style?"
AI:    [searches MemX] → "You prefer 4-space indentation with explicit error handling."

You:   "What's my pet's name?"
AI:    [searches MemX] → "I don't have that information stored."  ← won't hallucinate
```

---

## Features

- **Unified binary** — `memx setup/doctor/serve` cover bootstrap, validation, and service mode; `memx add/search/list` operate the DB directly from the CLI
- **Local-first** — all data stored in a single libSQL file on your machine, forever yours
- **Single-file portability** — moving to a new computer is just copying one file
- **Native vector search** — libSQL built-in vector functions + DiskANN index
- **Hybrid retrieval** — vector search + keyword search fused via Reciprocal Rank Fusion (RRF)
- **Four-factor re-ranking** — semantic similarity, recency, frequency, and importance
- **Smart rejection** — low-confidence queries return empty rather than wrong results
- **Memory graph** — 7 link types (similar, related, contradicts, extends, supersedes, caused_by, temporal) with graph traversal
- **Multi-app sharing** — one HTTP service, multiple AI clients share the same memory store
- **MVCC concurrency** — libSQL multi-write support, outperforms SQLite
- **Privacy-first** — works with any local embedding model (no data ever leaves your machine)
- **Fast** — keyword search under 3 ms; total latency dominated only by embedding API call

---

## Quick Start

### One-line install

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/memxlab/memx/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/memxlab/memx/main/install.ps1 | iex
```

The installer downloads the latest GitHub release, installs `memx`, launches `memx setup`, and then offers to install/start the background service.

One-line uninstall:

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/memxlab/memx/main/install.sh | sh -s -- uninstall
```

Windows PowerShell:

```powershell
$env:MEMX_UNINSTALL="1"; irm https://raw.githubusercontent.com/memxlab/memx/main/install.ps1 | iex
```

The uninstall flow requires confirmation and warns that `~/.memx` data will be deleted.

### Prerequisites

- Rust 1.70+
- An OpenAI-compatible embedding API (DeepInfra / OpenAI / Ollama / LM Studio / SiliconFlow)

### 1. First run — create config

```bash
cargo run --release -- setup
```

`memx setup` prompts for your embedding provider, model, base URL, API key, and auto-detects the vector dimension when possible.

### 2. Validate config

```bash
cargo run --release -- doctor
```

You should see checks for config presence, API connectivity, and vector dimension consistency.

### 3. Edit config manually if needed

```bash
vim ~/.memx/config.toml
```

The generated config stores these fields:

```toml
[embedding]
api_key = "your-api-key-here"
base_url = "https://api.deepinfra.com/v1/openai"
model = "Qwen/Qwen3-Embedding-0.6B"
dimension = 1024
```

Or use a fully local model via Ollama (no API key needed):

```toml
[embedding]
api_key = "local"
base_url = "http://localhost:11434/v1"
model = "nomic-embed-text"
dimension = 768
```

### 4. Start the service

```bash
cargo run --release -- serve
# → Listening on http://127.0.0.1:7878
```

### 5. Try it — CLI

MemX has a built-in CLI for direct database operations without starting the server:

```bash
# Add a memory
memx add "I prefer dark mode in all editors" --type emotional --tags "preferences" --importance 0.8
# ✓ Memory created [a1b2c3d4]

# Search memories
memx search "editor preferences" --limit 5
# Found 1 results for "editor preferences":
#
#   [a1b2c3d4] (score: 0.91) 2025-01-15
#   I prefer dark mode in all editors

# List recent memories
memx list --limit 10
```

All CLI commands share the same database as the server — no server process needed.

### 5. Try it — REST API

```bash
# Store a memory
curl -X POST http://127.0.0.1:7878/memories \
  -H "Content-Type: application/json" \
  -d '{"content": "I prefer dark mode in all editors", "type": "emotional", "tags": ["preferences"], "importance": 0.8}'

# Search memories
curl "http://127.0.0.1:7878/memories/search?q=editor+preferences&limit=5"
```

## CLI Reference

```
memx <COMMAND>

Commands:
  setup   Interactively create or update the config
  doctor  Check config and embedding connectivity
  version  Show the MemX version
  service  Manage the background MemX service
  uninstall  Remove the installed binary and local MemX data
  serve   Start the HTTP server
  add     Add a new memory
  search  Search memories
  list    List recent memories
  help    Print help
```

### `memx setup`

```bash
memx setup [OPTIONS]

Options:
  --provider <PROVIDER>    Provider preset used to prefill defaults
  --base-url <BASE_URL>    Embedding API base URL
  --model <MODEL>          Embedding model name
  --api-key <API_KEY>      Embedding API key
  --dimension <DIMENSION>  Embedding vector dimension
  --non-interactive        Disable interactive prompts
  --skip-validate          Skip remote validation
  --force                  Overwrite existing config without an extra prompt
  --yes                    Accept default confirmations
```

### `memx doctor`

```bash
memx doctor
```

### `memx version`

```bash
memx version
```

### `memx service`

```bash
memx service <install|start|stop|status|remove>
```

### `memx uninstall`

```bash
memx uninstall [--yes]
```

### `memx add`

```bash
memx add <CONTENT> [OPTIONS]

Options:
  --type <TYPE>              Memory type: semantic | episodic | procedural | emotional | reflective
  --tags <TAGS>              Comma-separated tags  (e.g. "rust,programming")
  --importance <IMPORTANCE>  Importance score 0.0–1.0
```

### `memx search`

```bash
memx search <QUERY> [OPTIONS]

Options:
  --limit <N>   Maximum results to return  [default: 10]
```

### `memx list`

```bash
memx list [OPTIONS]

Options:
  --limit <N>    Maximum results to return  [default: 20]
  --offset <N>   Pagination offset          [default: 0]
```

---

## API Overview

| Method   | Path                       | Description                    |
| -------- | -------------------------- | ------------------------------ |
| `POST`   | `/memories`                | Create a memory                |
| `GET`    | `/memories`                | List memories                  |
| `GET`    | `/memories/:id`            | Get a memory by ID             |
| `PUT`    | `/memories/:id`            | Update a memory                |
| `DELETE` | `/memories/:id`            | Delete a memory                |
| `GET`    | `/memories/search?q=...`   | Hybrid search                  |
| `POST`   | `/memories/:id/links`      | Create a link between memories |
| `GET`    | `/memories/:id/links`      | Get all links for a memory     |
| `DELETE` | `/memories/links/:link_id` | Delete a link                  |

Full API reference: [README.md](README.md)

## MCP Tools

| Tool            | Description                  |
| --------------- | ---------------------------- |
| `memory_add`    | Store a new memory           |
| `memory_search` | Search related memories      |
| `memory_list`   | List recent memories         |

---

## Memory Types

| Type         | Use case                            |
| ------------ | ----------------------------------- |
| `episodic`   | Events, experiences, conversations  |
| `semantic`   | Facts, knowledge, notes             |
| `procedural` | Skills, workflows, how-tos          |
| `emotional`  | Preferences, attitudes, feelings    |
| `reflective` | Summaries, insights, meta-cognition |

---

## Embedding Providers

MemX works with any OpenAI-compatible embedding API:

| Provider          | base_url                              | Example model                         | Dimension |
| ----------------- | ------------------------------------- | ------------------------------------- | --------- |
| DeepInfra         | `https://api.deepinfra.com/v1/openai` | `Qwen/Qwen3-Embedding-0.6B`           | 1024      |
| OpenAI            | `https://api.openai.com/v1`           | `text-embedding-3-small`              | 1536      |
| SiliconFlow       | `https://api.siliconflow.cn/v1`       | `BAAI/bge-large-zh-v1.5`              | 1024      |
| Ollama (local)    | `http://localhost:11434/v1`           | `nomic-embed-text`                    | 768       |
| LM Studio (local) | `http://localhost:1234/v1`            | `nomic-ai/nomic-embed-text-v1.5-GGUF` | 768       |

> **Note:** `dimension` must match the model's output dimension. Changing dimension on an existing database requires a new DB file or an offline migration.

---

## Configuration

`~/.memx/config.toml` (auto-generated on first run):

```toml
[embedding]
api_key    = "your-key"
base_url   = "https://api.deepinfra.com/v1/openai"
model      = "Qwen/Qwen3-Embedding-0.6B"
dimension  = 1024

[server]
host = "127.0.0.1"
port = 7878

# [database]
# path = "~/.memx/memory.db"

# [search]
# semantic_weight             = 0.45
# recency_weight              = 0.25
# frequency_weight            = 0.05
# importance_weight           = 0.10
# no_keyword_min_vector_score = 0.48
```

All fields can be overridden via environment variables (`EMBEDDING_API_KEY`, `SERVER_PORT`, `DATABASE_PATH`, etc.).

---

## Tech Stack

| Component        | Technology                                                                    |
| ---------------- | ----------------------------------------------------------------------------- |
| HTTP framework   | [axum](https://github.com/tokio-rs/axum) 0.8                                  |
| Database         | [libSQL](https://github.com/tursodatabase/libsql) 0.9 (native vector support) |
| Async runtime    | [tokio](https://tokio.rs/) 1.x                                                |
| Embedding client | [reqwest](https://github.com/seanmonstar/reqwest) (OpenAI-compatible)         |
| CLI              | [clap](https://github.com/clap-rs/clap) 4 (derive API)                       |
| Serialization    | serde + serde_json                                                            |

---

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.

---

<div align="center">
  <sub>Built with Rust · Your memory, your machine</sub>
</div>
