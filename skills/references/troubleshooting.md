# MemX Troubleshooting

This file records common memx problems and a recommended troubleshooting order.

## 1. First Run Exits Immediately

Symptoms:

- The process exits after the first `cargo run --release`

This is usually not a failure. The current behavior is:

1. Auto-create `~/.memx/config.toml`
2. Prompt for the embedding API key
3. Exit immediately

Resolution:

1. Open `~/.memx/config.toml`
2. Fill in the `[embedding]` configuration
3. Start memx again

## 2. Missing Embedding API Key

Symptoms:

- Startup fails with `embedding.api_key is required`

Resolution:

- Fill in `embedding.api_key` in `~/.memx/config.toml`
- Or set the `EMBEDDING_API_KEY` environment variable

## 3. Model And Dimension Do Not Match

Symptoms:

- Embedding requests fail
- Retrieval results look wrong
- An existing database behaves inconsistently after switching models

Key constraints:

- `dimension` must match the actual output dimension of the embedding model
- It must also match the vector column definition in the database

Resolution:

1. Confirm the real dimension of the current model
2. Check `dimension` in `~/.memx/config.toml`
3. Do not directly reuse the old database after changing the model dimension
4. Use a new database file or perform an offline migration first

## 4. Service Not Running

Symptoms:

- `curl http://127.0.0.1:7878/...` fails to connect

Resolution:

1. Start memx: `cargo run --release`
2. Or run the compiled binary: `./target/release/memx`
3. If host or port was changed, check `~/.memx/config.toml`

## 5. Search Returns Empty

Symptoms:

- There are clearly relevant memories, but `/memories/search` returns an empty array

Possible causes:

- The query text is too short or too weak
- Keyword retrieval did not hit
- The vector score is below the rejection threshold
- The embedding configuration is broken

Resolution order:

1. Confirm the memory actually exists using `/memories/{id}` or `/memories`
2. Retry with a more specific query
3. Check whether the embedding service is healthy
4. Check `no_keyword_min_vector_score` in the search config
5. Adjust search weights if needed

## 6. Update Did Not Change Tags Or Type

Symptoms:

- The request includes `tags` or `type`, but the returned result does not change

Cause:

- The current update implementation only supports `content` and `importance`

Resolution:

- Do not treat this as a failed call
- If `tags`, `type`, or `metadata` must be changed, the service implementation needs to be extended

## 7. Search Count And Access Count Look Different

Symptoms:

- Users are confused about why a memory's `retrieval_count` and `access_count` are different

Current behavior:

- `/memories/search` increments `retrieval_count` when a result is matched
- `GET /memories/{id}` increments `access_count`

These two fields mean different things and should not be treated as equivalent.

## 8. Local Ollama Setup Fails

Symptoms:

- Requests fail when using local embeddings

Recommended checks:

1. Whether Ollama is running
2. Whether the model has been pulled, for example `nomic-embed-text`
3. Whether `base_url` is `http://localhost:11434/v1`
4. Whether `api_key` is set to `local`
5. Whether `dimension` matches the model

## 9. Recommended Verification Flow

During troubleshooting, do not rely only on startup logs. Verify in this order:

1. The config file exists and the fields are complete
2. The service can start
3. Creating one minimal memory succeeds
4. Search can retrieve that memory
5. Reading by ID succeeds
6. If graph capabilities are involved, verify the link APIs as well
