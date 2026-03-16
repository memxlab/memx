# Repository Guidelines

## Project Structure And Module Organization

This is a Rust single-binary service. The entry point is `src/main.rs`; configuration parsing is in `src/config.rs`; error types are in `src/error.rs`. Database-related logic lives in `src/db/`, including connection handling (`connection.rs`), schema (`schema.rs`), memory CRUD (`memory.rs`), search (`search.rs`), links (`links.rs`), type definitions (`types.rs`), and test utilities (`test_utils.rs`). The embedding client is in `src/embed/`; HTTP routes are in `src/routes/`. The main usage documents at the repository root are `README.md` and `QUICKSTART.md`, and `test.sh` is used for manual API smoke testing.

The configuration file is located at `~/.memx/config.toml` and is auto-generated on first run; the default database is `~/.memx/memory.db`.

## Build, Test, And Development Commands

- `cargo build --release`: Build the production binary.
- `cargo run --release`: Start the service, listening on `127.0.0.1:7878` by default.
- `RUST_LOG=debug cargo run`: Run with debug logging to help inspect routes and request flow.
- `cargo test`: Run Rust unit tests; modules such as `schema`, `memory`, and `search` already have coverage.
- `cargo clippy`: Run static analysis; the target is 0 warnings.
- `./test.sh`: Run end-to-end API verification against a running service; requires `curl` and `jq`.

## Code Style And Naming Conventions

Follow Rust 2021 and the default `rustfmt` style, with 4-space indentation. Use `snake_case` for modules and file names, `PascalCase` for type names, `snake_case` for functions and fields, and `SCREAMING_SNAKE_CASE` for constants. Prefer short functions and explicit return paths, and avoid introducing extra abstractions or heavy dependencies for small features. Run `cargo fmt` and `cargo clippy` before submitting changes.

## Testing Guidelines

Keep tests inside the corresponding module under `#[cfg(test)]`. Shared test helpers such as `open_test_db` and `cleanup` are centralized in `src/db/test_utils.rs` to avoid duplication. Test names should describe behavior directly, such as `track_access_does_not_touch_retrieval_stats`. When changing APIs, schema, or search weights, add at least one reproducible test.

## Security And Configuration Notes

Do not commit `*.db`, logs, or real API keys. The config file is located at `~/.memx/config.toml`, and environment variables can override the corresponding fields. When changing the embedding model or vector dimension, make sure `model` and `dimension` stay consistent.

**Please keep the conversation in Chinese.**
