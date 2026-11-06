# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Orchyn is a Rust harness for vibecoded web apps. It has three parts: a deterministic replay runner (manifests run against real browsers with no model), static code-intent extraction reconciled against observed network traffic to surface "reality gaps", and an exploratory agent mode driven by a local Qwen 3.5 0.8B model. `ARCHITECTURE.md` is the canonical design document — read it before touching the crate graph.

## Workspace layout

Cargo workspace (`resolver = "2"`, Rust 2024 edition, `rust-toolchain.toml` channel `stable`). Members:

- `crates/*` — 16 crates (including `crates/tests`), described in `ARCHITECTURE.md` ("Workspace layout"). Internal deps are wired through `[workspace.dependencies]` as `orchyn-<name> = { path = "crates/<name>" }`; add new crates to both places.
- `bin/*` — four binaries: `orchyn-cli` (agent-mode explorer, flag-based, no subcommands), `orchyn-server` (axum + GraphQL API on `127.0.0.1:3000`), `orchyn-worker` (scheduler), `orchyn-bench` (replay manifests scored against `bench/`).
- `crates/tests` (package `orchyn-tests`) — cross-crate unit tests, integrated as a regular member. It is listed *explicitly* in `[workspace].members`; the explicit entry documents intent, since the glob already covers it. The same is true of `crates/orchyn-harness` and `crates/artifact-storage`.
- `tests/integration` (package `orchyn-integration-tests`) — end-to-end tests at the repo root, outside `crates/`.
- `bench/` — benchmark suite, not a Cargo member: `suite.json`, `fixtures/<name>/` (each with `server.js`, `FEATURE.md`, `public/`; run clean and with `BUG=1`), and reference replay manifests in `manifests/`. Needs `npm install` in `bench/`.

## Build, run, test

```bash
cargo build --workspace
cargo clippy --workspace --all-targets

cargo test -p orchyn-harness --lib                           # harness unit tests (replay, intent, agent)
cargo test -p orchyn-tests                                   # cross-crate tests
cargo test -p orchyn-tests agent_completes_on_first_done_response   # single test
cargo test -p orchyn-integration-tests                       # e2e: needs node, Chrome, fixture npm install

cargo run -p orchyn-cli -- --app <url-or-path> [--max-turns 40] [--project-root .] [--diff FROM TO]
cargo run -p orchyn-bench -- --runs 3 [--suite bench/suite.json] [--manifests DIR] [--label L] [--out FILE] [--only NAME]
cargo run -p orchyn-server                                   # serves 127.0.0.1:3000
cargo run -p orchyn-worker
```

- `cargo test -p orchyn-harness` without `--lib` does not compile: `crates/orchyn-harness/tests/harness_tests.rs` builds a `HarnessConfig` without the `phases` field. Use `--lib` until that is fixed.
- `orchyn-cli` has no `run`, `agent`, `validate` or `diff` subcommands. Replay is only reachable through `orchyn-bench` today.
- Prefer `-p <crate>` over workspace-wide builds; the workspace is heavy.

## Runtime requirements

- **Model.** `QwenClient` (`crates/model-client`) serves Qwen 3.5 0.8B: Hugging Face repo `bartowski/Qwen_Qwen3.5-0.8B-GGUF`, file `Qwen_Qwen3.5-0.8B-Q4_K_M.gguf`, run through `llama-cpp-2`. It is loaded lazily on the first model call (`LlamaEngine::load` via `hf-hub`), not at construction. The model is text-only: `QwenClient::observe` returns an error. All model calls route through this one client; do not add alternate model backends (see `ARCHITECTURE.md`, "Single Model Policy"). Replay and reconciliation need no model.
- **Browser.** `WebAdapter` (`crates/platform-web`) uses `headless_chrome`, not Playwright. It uses the binary named by `CHROME=<path>` when set, otherwise one `headless_chrome` finds. Set `ORCHYN_NO_SANDBOX=1` when running as root or in a container.
- **PostgreSQL** is required by `orchyn-server` and `orchyn-worker`; both read `DATABASE_URL` and fail without it. `Storage::new` creates its tables on startup (`CREATE TABLE IF NOT EXISTS`); `migrations/0001_init.sql` is not run by the code. The storage config defaults `DATABASE_URL` to `postgres://localhost/orchyn`. The CLI and bench do not use storage.
- **orchyn-server** also requires `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `JWT_SECRET`, `FRONTEND_URL` and `STRIPE_WEBHOOK_SECRET`. `ARTIFACT_ROOT` (default `./storage/artifacts`) only sets where artifacts are stored; no route in this repo serves it.
- **Benchmark.** `bench/` needs `npm install` (express). `orchyn-bench` starts each fixture with `node server.js`, setting `PORT` to a free port and `BUG` to `0` or `1`.
- **Integration tests.** `tests/integration` starts Node fixtures from `tests/fixtures/` (`broken-counter`, `broken-login`, `qa-target`) on fixed ports such as 18091. Run `npm install` in `tests/fixtures/` if `node_modules` is missing, and keep the ports free. Most tests use `MockClient`; the real-model tests load the 0.8B model and are serialised with a lock.

## Conventions baked into the code

- **Native async traits** for traits that are not used as `dyn` (`async fn`, Rust 1.75+). Traits used as `dyn` (`ModelClient`, `PlatformAdapter`, `Tool`, `AdapterFactory`) return `BoxFuture`. Do not add `#[async_trait]`.
- **Errors flow through `orchyn_core::OrchynError`** via the alias `orchyn_core::Result<T>`. Convert crate-local errors into one of its variants (`Internal`, `Model`, `Storage`, `Platform`, `Auth`, `Validation`, `Stuck`, `RecoveryExhausted`) rather than adding a new top-level error type.
- **Replay and reconciliation stay model-free.** Code under `crates/orchyn-harness/src/replay/` and `src/intent/` must be deterministic and must not call `ModelClient`.
- **Navigator discipline** (see `skills/codebase-navigation/SKILL.md`): every claim needs a `SourceRef`, and file reads are range-bounded (max 200 lines). That skill still describes Qwen 3.5 4B; the model in use is 0.8B. The sibling directories under `skills/` (`actor-generation`, `gap-detection`, `platform-adapters`, `spec-inference`, `vlm-prompting`) hold constraints for the other agent areas. Consult the relevant `SKILL.md` before changing agent behaviour, and note that vision is not available (see `ARCHITECTURE.md`, "Pixel-level evidence today").
- **Prompts live in `prompts/`** (`system.md`, `agent_system.md`, `action_planner.md`, `gap_reporter.md`, `spec_writer.md`), loaded with `include_str!`. Keep model-facing prompts there, not inline in `.rs` files.
- **Keep the docs in step with the code.** Update `ARCHITECTURE.md` in the same change when the crate graph or runtime model changes.
