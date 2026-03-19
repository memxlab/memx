---
name: memx-openclaw
version: 0.1.0
description: |
  Install and configure the MemX OpenClaw context-engine plugin.

  Use when users say:
  - "install memx openclaw plugin"
  - "set up memx for openclaw"
  - "enable memx memory in openclaw"
  - "connect openclaw to memx"
author: MemX
keywords:
  - memx
  - openclaw
  - context-engine
  - memory
  - plugin
---

# MemX OpenClaw Plugin

This plugin uses the OpenClaw `context-engine` lifecycle to integrate MemX automatic recall and save into normal conversation.

## Trigger phrases

Use this skill when the user wants to:

- install the MemX OpenClaw plugin
- connect OpenClaw to a running MemX service
- enable automatic memory recall in OpenClaw
- enable automatic conversation save into MemX

## When to use

Use this skill when the user wants:

- persistent memory through MemX during normal conversation
- automatic recall before reply
- automatic save after reply
- a local-first OpenClaw memory integration

## When not to use

Do not use this skill for:

- direct MemX API troubleshooting unrelated to OpenClaw
- temporary per-session context only
- cloud memory products that do not use MemX

## Definition of done

This task is not complete until all of the following are true:

1. the MemX service is reachable or the user has been told it still needs to be started
2. the plugin files are installed into OpenClaw's plugin path
3. `openclaw.json` points `plugins.slots.contextEngine` to `memx-openclaw`
4. `plugins.slots.memory` is set to `none`
5. OpenClaw has been restarted
6. the user has been given a natural-language verification step

## Verification

Recommended test:

> Say: "Remember: I like espresso."
>
> Then ask: "What coffee do I like?"

This checks the real conversation path instead of only checking config.
