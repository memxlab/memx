---
name: memx
description: |
  Local-first persistent memory service for agents.

  Use when users say:
  - "Remember this"
  - "Help me recall what we mentioned before"
  - "Search my memories"
  - "Update this memory"
  - "Delete this memory"
  - "Create a link between these two memories"
  - "memx is not working"
---

# memx

memx is a local-first long-term memory service for storing reusable preferences, facts, project context, and learned experience.

Work primarily through the existing HTTP APIs:

- Create memory: `POST /memories`
- Search memories: `GET /memories/search`
- Read a memory: `GET /memories/{id}`
- Update a memory: `PUT /memories/{id}`
- Delete a memory: `DELETE /memories/{id}`
- Create a link: `POST /memories/{id}/links`
- View links: `GET /memories/{id}/links`
- Delete a link: `DELETE /memories/links/{link_id}`

See [references/api.md](references/api.md) for request fields, example requests, and current limitations.
If the task involves startup failure, config issues, embedding connectivity, or broken search, read [references/troubleshooting.md](references/troubleshooting.md).

## Trigger Phrases

Use this skill in these situations:

- "Remember this"
- "Don't forget this later"
- "Help me find a preference I mentioned before"
- "Search for memories related to a topic"
- "Update this memory"
- "Delete this incorrect memory"
- "Link these two memories together"
- "memx won't start"
- "memx can't find anything"

## When To Use

Good fit for:

- Long-term preferences such as food, work habits, or expression style
- Stable facts such as identity details, project facts, or technical choices
- Procedural knowledge such as repeatable workflows, SOPs, or common commands
- Reflective summaries such as retrospectives, conclusions, or lessons learned
- Memory graphs that need explicit links between items

## When NOT To Use

Not a good fit for:

- Temporary context that only matters in the current session
- Sensitive data such as passwords, tokens, or API keys
- Large raw logs or large file contents
- Scenarios that require automatic cross-machine synchronization

## What Should Be Remembered

Recommended mappings:

- `emotional`: user preferences, attitudes, tendencies
- `semantic`: facts, knowledge, conclusions
- `procedural`: steps, SOPs, operational experience
- `episodic`: concrete events, conversations, experiences
- `reflective`: retrospectives, insights, strategies

Default rules:

- If the user does not specify a type, memx currently defaults to `semantic`
- If the user does not specify `importance`, the default is `0.5`
- `importance` should stay within `0.0` to `1.0`

## Operating Rules

Follow these rules during execution:

1. For recall or retrieval, use `/memories/search` first. Do not brute-force through the list endpoint.
2. Use `/memories/{id}` to read a single memory. This increments `access_count`; search hits increment `retrieval_count`.
3. The current update API only reliably supports `content` and `importance`. Do not claim it can update `tags`, `type`, or `metadata`, because the current implementation does not do that.
4. When creating a link, use the `{id}` in the path as the `source_id`; any `source_id` in the request body will be overwritten.
5. In a new environment or on first use, confirm the service is available before writing or searching.

## Workflow

### Step 0 - Check Readiness

Confirm the basics first:

- Whether `~/.memx/config.toml` exists
- Whether `embedding.api_key` is configured
- Whether `model` and `dimension` match
- Whether the memx service is running and using the default address `http://127.0.0.1:7878`

If this is the user's first run, explicitly explain:

- The first memx run auto-generates `~/.memx/config.toml`
- The process exits immediately after generation, and that is not a failure
- memx must be started again after the config is filled in

### Step 1 - Verify Minimal Functionality

If the user is installing, integrating, or troubleshooting:

1. Start memx
2. Create one minimal memory
3. Search for it through the search endpoint
4. Continue with the real task only after the result looks correct

Do not treat "the service process started" as done; there must be at least one write and one search verification.

### Step 2 - Perform The Requested Operation

Choose the action based on user intent:

- Remember new content: create a memory
- Recall related content: search memories
- View details for one item: read by ID
- Fix wording or importance: update the memory
- Remove incorrect content: delete the memory
- Build contextual structure: create a link

If the user speaks in natural language without specifying a memory type, use the default mapping above. If unsure, prefer `semantic` or `emotional` and avoid inventing overly complex categories.

### Step 3 - Explain The Result

After completing the action, explain:

- What was actually executed
- Which memories were matched or which memory was created
- Whether links were created
- If it failed, whether the failure was in config, service availability, embedding, or request parameters

If this was first-time setup or troubleshooting, also make these points explicit:

- Config file location: `~/.memx/config.toml`
- Default database location: `~/.memx/memory.db`
- You cannot directly reuse the old database after changing embedding dimensions

## Common Failure Modes

- The first run generates config and exits, but gets misread as a crash
- `embedding.api_key` is empty
- `model` and `dimension` do not match
- Requests are sent before the service is started
- The update API is incorrectly assumed to support all fields
- The list API is incorrectly assumed to be equivalent to semantic search

See [references/troubleshooting.md](references/troubleshooting.md) for detailed troubleshooting steps.

## Definition Of Done

The task is complete only when all of the following are true:

1. For integration or troubleshooting work, memx can start and the APIs are reachable
2. For first-time setup, the config file and embedding parameters have been confirmed correct
3. At least one minimal write and one minimal search verification have been completed
4. The requested memory operation has actually been executed, or the current API limitation has been stated clearly
5. The user has the next-step information they need, including the result, limitations, and any necessary recovery or troubleshooting guidance
