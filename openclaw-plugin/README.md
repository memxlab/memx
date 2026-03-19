# MemX OpenClaw Plugin

MemX OpenClaw Plugin connects OpenClaw's `context-engine` lifecycle to MemX so memory can be recalled before each reply and saved after each turn.

## What it does

- Searches MemX before each reply and injects relevant memories as context
- Saves each conversation turn back into MemX as an `episodic` memory by default
- Keeps the integration local-first by talking to your MemX HTTP service
- Avoids manual memory tool calls during normal conversation

## Requirements

- Node.js 18+
- OpenClaw with plugin loading enabled
- A running MemX service, typically at `http://127.0.0.1:7878`

## Quick start

From this repository:

```bash
cd openclaw-plugin
node ./bin/install.js
```

The installer:

- copies the plugin into `~/.openclaw/plugins/memx-openclaw`
- adds that path to `plugins.load.paths`
- adds `memx-openclaw` to `plugins.allow`
- sets `plugins.slots.contextEngine = "memx-openclaw"`
- sets `plugins.slots.memory = "none"` to avoid slot conflicts
- creates or updates `plugins.entries["memx-openclaw"]`

After install:

```bash
openclaw gateway restart
```

Then verify with natural language:

```text
Remember: I like espresso.
What coffee do I like?
```

## Default config

```json
{
  "plugins": {
    "allow": ["memx-openclaw"],
    "slots": {
      "memory": "none",
      "contextEngine": "memx-openclaw"
    },
    "entries": {
      "memx-openclaw": {
        "enabled": true,
        "config": {
          "baseUrl": "http://127.0.0.1:7878",
          "topK": 5,
          "memoryType": "episodic",
          "saveAssistantMessages": false,
          "minContentChars": 8
        }
      }
    }
  }
}
```

## Config fields

| Field | Default | Description |
| --- | --- | --- |
| `baseUrl` | `http://127.0.0.1:7878` | MemX HTTP base URL |
| `topK` | `5` | Maximum number of memories to inject |
| `memoryType` | `episodic` | Memory type used for saved turns |
| `saveAssistantMessages` | `false` | Whether assistant replies are stored |
| `minContentChars` | `8` | Skip messages shorter than this threshold |

## How saving works

- The plugin stores one MemX memory per turn
- By default it stores user content only
- If `saveAssistantMessages` is enabled, assistant text is appended to the same memory
- Metadata records the session key, turn number, plugin id, and roles

## Notes

- MemX currently has no dedicated `/health` endpoint, so the installer probes `GET /memories?limit=1&offset=0`
- Retrieval quality depends on your MemX embedding setup and search config
- If you change MemX embedding dimensions, keep `model` and `dimension` consistent in `~/.memx/config.toml`
