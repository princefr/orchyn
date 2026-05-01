# Orchyn — Project Instructions

Orchyn is a Rust-based autonomous QA harness that reconciles *inferred code intent* with *observed pixel reality* to detect "reality gaps." This file provides foundational guidance for working within this codebase.

## Project Overview

- **Goal:** Automate the detection and fixing of UI/UX discrepancies by comparing codebase "intent" against running application "pixels."
- **Core Modes:**
    - **Agent Mode:** Autonomous exploration and gap detection.
    - **Constraint Mode:** Deterministic replay of generated manifests.
- **Architecture:** Modular crate-based workspace. See `ARCHITECTURE.md` for the canonical design.

## Technical Stack

- **Language:** Rust (2024 Edition, stable).
- **Async:** Native async traits (Rust 1.75+). Do **not** use `#[async_trait]`.
- **Database:** PostgreSQL (via `sqlx`). Schema is auto-initialized by `Storage::new`.
- **Model:** Qwen 3.5 (0.8B or 4B variants) run locally via `hf-hub` (Single Model Policy).
- **Web Platform:** Headless Chrome via `headless_chrome` (implemented in `platform-web`).
- **API Server:** Axum + GraphQL (`async-graphql`).

## Workspace Layout

- `bin/`: Entry points for `orchyn-cli`, `orchyn-server`, and `orchyn-worker`.
- `crates/`: Library crates (e.g., `core`, `model-client`, `storage`, `orchyn-harness`).
- `prompts/`: Model-facing prompts (keep them here, not inline in `.rs` files).
- `tests/integration/`: E2E tests using Playwright and fixture apps.

## Building and Running

### Prerequisites
- **PostgreSQL:** Required for storage. `DATABASE_URL` must be set.
- **Node.js:** Required for running integration test fixtures.
- **Model:** Qwen 3.5 will be downloaded on first use.

### Commands
- **Build:** `cargo build --workspace`
- **Clippy:** `cargo clippy --workspace --all-targets`
- **Unit Tests:** `cargo test -p orchyn-tests`
- **E2E Tests:** `cargo test -p integration` (run `npm install` in `tests/fixtures/todo-app` first).
- **Run CLI:** `cargo run -p orchyn-cli -- --app <url-or-path>`
- **Run Server:** `cargo run -p orchyn-server`

## Development Conventions

- **Error Handling:** Use `orchyn_core::OrchynError`. Crate-local errors should convert to its variants (`Model`, `Storage`, `Platform`, `Internal`).
- **Traits & Mocks:** Follow the pattern: `Trait` -> `Implementation` + `Mock`. Ensure every new module is testable in isolation.
- **Agent Discipline:** Every claim by an agent needs a `SourceRef`. `read_file` should be range-bounded.
- **Single Model Policy:** All LLM/VLM calls must route through `QwenClient` in `crates/model-client`.
