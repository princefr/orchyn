# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Orchyn is a Rust harness that reconciles a vibecoded app's *inferred code intent* with its *observed pixel reality* to surface "reality gaps." `ARCHITECTURE.md` is the canonical design document — read it before touching the crate graph.

## Workspace layout

Cargo workspace (`resolver = "2"`, Rust 2024 edition, pinned to stable via `rust-toolchain.toml`). Members:

- `crates/*` — library crates listed in `ARCHITECTURE.md`. Internal deps are wired through `[workspace.dependencies]` as `orchyn-<name> = { path = "crates/<name>" }`; add new crates to both places.
- `bin/*` — three binaries: `orchyn-cli` (local codebase indexer entrypoint), `orchyn-server` (axum + GraphQL API on `:3000`), `orchyn-worker` (scheduler/test executor).
- `crates/tests` — cross-crate unit tests (integrated into the workspace as a regular member, not `[dev-dependencies]`).
- `tests/integration` — end-to-end tests that spin up the Node fixture in `tests/fixtures/todo-app` and drive it through `WebAdapter` (Playwright).

Two non-obvious structural points: `crates/tests` is listed *explicitly* in `[workspace].members` alongside `crates/*` (not redundant — the glob picks it up, but the explicit entry documents intent), and `tests/integration` lives at the repo root outside `crates/`.

## Build, run, test

```bash
cargo build --workspace
cargo clippy --workspace --all-targets
cargo test -p orchyn-tests                       # cross-crate tests
cargo test -p orchyn-tests test_storage_multi_user_flow    # single test
cargo test -p integration                        # e2e (needs fixture prereqs — see below)

cargo run -p orchyn-cli -- <path-or-git-url> [--artifact-dir ./orchyn_artifacts]
cargo run -p orchyn-server                       # serves :3000
cargo run -p orchyn-worker
```

## Runtime requirements

- **PostgreSQL is mandatory.** `Storage::new` connects via sqlx and auto-initializes the schema (`users`, `projects`, `project_members`, `invitations`, `runs`, ...) on startup — there is no separate migration step. `DATABASE_URL` defaults to `postgres://localhost/orchyn` in the CLI and is *required* (panics on missing) for `orchyn-server` and the storage test in `crates/tests`. Use `.env` as a template; real secrets are not committed.
- **Qwen3.5 4B model** is downloaded on first use via `hf-hub` inside `QwenClient::ensure_loaded` (lazy — not at construction). All LLM *and* VLM calls in the codebase route through this single client; do not add alternate model backends (see `ARCHITECTURE.md` "Single Model Policy").
- **orchyn-server** additionally requires `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, and (for webhooks) `STRIPE_*`. `ARTIFACT_ROOT` defaults to `./storage/artifacts` and is served as static files at `/artifacts`.
- **`tests/integration`** spawns `node server.js` from `tests/fixtures/todo-app` (expects `express`, `node_modules/` already present) and drives Chromium via `WebAdapter`. Run `npm install` in the fixture dir if it's missing, and ensure port `8080` is free.

## Conventions baked into the code

- **Native async traits only** (`async fn` in traits, Rust 1.75+). Do not add `#[async_trait]`.
- **Errors flow through `orchyn_core::OrchynError`** with the `Result<T, E = OrchynError>` alias. Crate-local errors should convert into one of its variants (`Model`, `Storage`, `Platform`, `Internal`) rather than leaking a new top-level error type.
- **Navigator agent discipline** (see `skills/codebase-navigation/SKILL.md`): every claim needs a `SourceRef`; `read_file` is range-bounded (max 200 lines); budget defaults to 40 tool calls. The sibling directories under `skills/` (`actor-generation`, `gap-detection`, `platform-adapters`, `spec-inference`, `vlm-prompting`) encode the same kind of constraints for the other agent crates — consult the relevant `SKILL.md` before changing agent behavior.
- **Prompts live in `prompts/`** (`navigator.md`, `vlm_observer.md`). Keep model-facing prompts there, not inline in `.rs` files.
