# MemX API Reference

This file documents only the APIs and behaviors that are currently implemented in this repository, so the skill can reference them during actual execution.

## Base URL

Default:

```text
http://127.0.0.1:7878
```

## Memory Types

- `episodic`
- `semantic`
- `procedural`
- `emotional`
- `reflective`

If `type` is omitted during creation, the service defaults it to `semantic`.

## Link Types

- `similar`
- `related`
- `contradicts`
- `extends`
- `supersedes`
- `caused_by`
- `temporal`

## Create Memory

```bash
curl -s -X POST "http://127.0.0.1:7878/memories" \
  -H "Content-Type: application/json" \
  -d '{
    "content": "The user likes coffee",
    "type": "emotional",
    "tags": ["beverage", "preference"],
    "importance": 0.8,
    "metadata": {"source":"chat"}
  }'
```

Request fields:

- `content`: required
- `type`: optional
- `tags`: optional
- `metadata`: optional
- `importance`: optional, defaults to `0.5`, and is clamped to `0.0..1.0`

Response fields:

- `id`
- `content`
- `type`
- `tags`
- `importance`
- `created_at`

Notes:

- An embedding is generated before creation completes
- The create response does not include the full statistics fields

## List Memories

```bash
curl -s "http://127.0.0.1:7878/memories?limit=20&offset=0"
```

Parameters:

- `limit`: optional, default `50`, maximum `200`
- `offset`: optional, default `0`

Good for:

- Browsing recent memories
- Debugging or manual inspection

Not good for:

- Semantic recall
- Topic retrieval

## Get Memory

```bash
curl -s "http://127.0.0.1:7878/memories/<memory-id>"
```

Behavior:

- Returns the full memory object
- Increments `access_count`
- Writes `last_accessed_at`

## Search Memories

```bash
curl -s "http://127.0.0.1:7878/memories/search?q=coffee&limit=5"
```

Parameters:

- `q`: required
- `limit`: optional, default `10`, maximum `50`

Behavior:

- Generates an embedding for the query text
- Performs vector retrieval
- Runs FTS retrieval if keyword search is enabled
- Uses RRF for fusion
- Applies multi-factor reranking
- Increments `retrieval_count` for matched results
- Writes `last_retrieved_at`

Important details in the search pipeline:

- If there is no keyword hit, results can be rejected directly when the vector score is too low
- Results are deduplicated by content and by tag signature
- Search statistics and read-by-ID statistics have different meanings

## Update Memory

```bash
curl -s -X PUT "http://127.0.0.1:7878/memories/<memory-id>" \
  -H "Content-Type: application/json" \
  -d '{
    "content": "The user really likes coffee",
    "importance": 0.95
  }'
```

Current actual behavior:

- If a non-empty `content` is provided, the embedding is recomputed
- If `importance` is provided, the importance is updated
- The current implementation does not update `type`
- The current implementation does not update `tags`
- The current implementation does not update `metadata`

Do not claim that "any field can be updated" because the current implementation does not support that.

## Delete Memory

```bash
curl -s -X DELETE "http://127.0.0.1:7878/memories/<memory-id>"
```

Behavior:

- Returns `204 No Content` on success
- Returns not found when the memory does not exist

## Create Link

```bash
curl -s -X POST "http://127.0.0.1:7878/memories/<source-id>/links" \
  -H "Content-Type: application/json" \
  -d '{
    "target_id": "<target-id>",
    "link_type": "related",
    "strength": 0.7,
    "bidirectional": true,
    "metadata": {"reason":"same topic"}
  }'
```

Request fields:

- `target_id`: required
- `link_type`: required
- `strength`: optional, default `0.7`
- `bidirectional`: optional, default `true`
- `metadata`: optional

Notes:

- The `<source-id>` in the path is the actual `source_id` that will be used
- Any `source_id` in the request body is overwritten by the server

## Get Links

```bash
curl -s "http://127.0.0.1:7878/memories/<memory-id>/links"
```

Behavior:

- Returns links originating from the memory
- Also includes bidirectional links that can be read in reverse

## Get Link Count

```bash
curl -s "http://127.0.0.1:7878/memories/<memory-id>/link-count"
```

Response:

```json
{"count": 3}
```

## Delete Link

```bash
curl -s -X DELETE "http://127.0.0.1:7878/memories/links/<link-id>"
```

Behavior:

- Returns `204 No Content` on success

## Config Paths

- Config directory: `~/.memx`
- Config file: `~/.memx/config.toml`
- Default database: `~/.memx/memory.db`

## Environment Overrides

Common environment variables:

- `EMBEDDING_API_KEY`
- `EMBEDDING_BASE_URL`
- `EMBEDDING_MODEL`
- `EMBEDDING_DIMENSION`
- `DATABASE_PATH`
- `SERVER_HOST`
- `SERVER_PORT`

## Smoke Test Pattern

During integration or troubleshooting, start with this minimal flow:

1. Create one short memory
2. Search for it using a keyword
3. Read it by ID
4. Create a link if needed
