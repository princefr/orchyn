# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Orchyn is a Rust harness for vibecoded web apps. It has three parts: a deterministic replay runner (manifests run against real browsers with no model), static code-intent extraction reconciled against observed network traffic to surface "reality gaps", and an exploratory agent mode driven by a local Qwen 3.5 0.8B model. Invariant candidates come from universal page rules, code contracts, transcripts, production logs and mined traces (`crates/orchyn-harness/src/invariants/`). `ARCHITECTURE.md` is the canonical design document — read it before touching the crate graph.

## Workspace layout

Cargo workspace (`resolver = "2"`, Rust 2024 edition, `rust-toolchain.toml` channel `stable`). Members:

- `crates/*` — 16 crates (including `crates/tests`), described in `ARCHITECTURE.md` ("Workspace layout"). Internal deps are wired through `[workspace.dependencies]` as `orchyn-<name> = { path = "crates/<name>" }`; add new crates to both places.
- `bin/*` — five binaries: `orchyn-cli` (agent-mode explorer, flag-based, no subcommands), `orchyn-server` (axum + GraphQL API on `127.0.0.1:3000`), `orchyn-worker` (scheduler), `orchyn-check` (grounded checks of a running app: manifests, universal invariants, contracts; no model), `orchyn-bench` (replay manifests scored against `bench/`, selected with `--mode`).
- `crates/tests` (package `orchyn-tests`) — cross-crate unit tests, integrated as a regular member. It is listed *explicitly* in `[workspace].members`; the explicit entry documents intent, since the glob already covers it. The same is true of `crates/orchyn-harness` and `crates/artifact-storage`.
- `tests/integration` (package `orchyn-integration-tests`) — end-to-end tests at the repo root, outside `crates/`.
- `bench/` — benchmark suite, not a Cargo member: `suite.json`, `fixtures/<name>/` (each with `server.js`, `FEATURE.md`, `PROMPTS.md`, `public/`; run clean and with `BUG=1`), reference replay manifests in `manifests/`, and `results/`. `webbench/` holds an external with/without-Orchyn experiment (see its `RESULTS.md`). Needs `npm install` in `bench/`.

## Build, run, test

```bash
cargo build --workspace
cargo clippy --workspace --all-targets

cargo test -p orchyn-harness --lib                           # harness unit tests (replay, intent, agent)
cargo test -p orchyn-tests                                   # cross-crate tests
cargo test -p orchyn-tests agent_completes_on_first_done_response   # single test
cargo test -p orchyn-integration-tests                       # e2e: needs node, Chrome, fixture npm install

cargo run -p orchyn-cli -- --app <url-or-path> [--max-turns 40] [--project-root .] [--diff FROM TO]
cargo run -p orchyn-check -- --url <http://...> [--project .] [--manifest FILE ...] [--universal] [--contracts]
cargo run -p orchyn-check -- --lint-spec TASKS.md [--lint-spec MORE.md]    # no --url needed
cargo run -p orchyn-check -- --url <http://...> --manifest FILE ... --record DIR [--runs 3]
cargo run -p orchyn-check -- --url <http://...> --manifest FILE ... --baseline DIR
cargo run -p orchyn-bench -- --runs 3 [--mode manifest|universal|mined|invariants] [--suite bench/suite.json] [--manifests DIR] [--label L] [--out FILE] [--only NAME]
cargo run -p orchyn-server                                   # serves 127.0.0.1:3000
cargo run -p orchyn-worker
```

- `orchyn-cli` has no `run`, `agent`, `validate` or `diff` subcommands. Replay is reached through `orchyn-check` (one app, one run) and `orchyn-bench` (scored fixtures).
- `orchyn-check` exits 0 only if every `--manifest` passes, no `--universal` candidate with confidence >= 0.9 fails, and (with `--baseline`) no recorded invariant is broken. Reality gaps and `--lint-spec` findings do not change the exit code. `--baseline` replays each manifest's actions only, so its assertions are not checked in that mode.
- `orchyn-bench --mode mined` needs `--manifests DIR` (driver manifests). In `--mode invariants`, `--out` is a directory.
- Prefer `-p <crate>` over workspace-wide builds; the workspace is heavy.

## Runtime requirements

- **Model.** `QwenClient` (`crates/model-client`) serves Qwen 3.5 0.8B: Hugging Face repo `bartowski/Qwen_Qwen3.5-0.8B-GGUF`, file `Qwen_Qwen3.5-0.8B-Q4_K_M.gguf`, run through `llama-cpp-2`. It is loaded lazily on the first model call (`LlamaEngine::load` via `hf-hub`), not at construction. The model is text-only: `QwenClient::observe` returns an error. All model calls route through this one client; do not add alternate model backends (see `ARCHITECTURE.md`, "Single Model Policy"). Replay, reconciliation and invariants need no model.
- **Browser.** `WebAdapter` (`crates/platform-web`) uses `headless_chrome`, not Playwright. It uses the binary named by `CHROME=<path>` when set, otherwise Playwright's `chrome-headless-shell` if installed, otherwise one `headless_chrome` finds. `headless_chrome` is vendored in `vendor/headless_chrome` via `[patch.crates-io]` (see its `ORCHYN_PATCHES.md`). Set `ORCHYN_NO_SANDBOX=1` when running as root or in a container. `ORCHYN_BROWSER_PER_ADAPTER=1` restores one Chromium per adapter (the default is one shared Chromium, with an incognito context per adapter).
- **PostgreSQL** is required by `orchyn-server` and `orchyn-worker`; both read `DATABASE_URL` and fail without it. `Storage::new` creates its tables on startup (`CREATE TABLE IF NOT EXISTS`); `migrations/0001_init.sql` is not run by the code. The storage config defaults `DATABASE_URL` to `postgres://localhost/orchyn`. The CLI and bench do not use storage.
- **orchyn-server** also requires `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `JWT_SECRET`, `FRONTEND_URL` and `STRIPE_WEBHOOK_SECRET`. `ARTIFACT_ROOT` (default `./storage/artifacts`) only sets where artifacts are stored; no route in this repo serves it.
- **Benchmark.** `bench/` needs `npm install` (express). `orchyn-bench` starts each fixture with `node server.js`, setting `PORT` to a free port and `BUG` to `0` or `1`.
- **Integration tests.** `tests/integration` starts Node fixtures from `tests/fixtures/` (`broken-counter`, `broken-login`, `qa-target`) on fixed ports such as 18091. Run `npm install` in `tests/fixtures/` if `node_modules` is missing, and keep the ports free. Most tests use `MockClient`; the real-model tests load the 0.8B model and are serialised with a lock.

## Conventions baked into the code

- **Native async traits** for traits that are not used as `dyn` (`async fn`, Rust 1.75+). Traits used as `dyn` (`ModelClient`, `PlatformAdapter`, `Tool`, `AdapterFactory`) return `BoxFuture`. Do not add `#[async_trait]`.
- **Errors flow through `orchyn_core::OrchynError`** via the alias `orchyn_core::Result<T>`. Convert crate-local errors into one of its variants (`Internal`, `Model`, `Storage`, `Platform`, `Auth`, `Validation`, `Stuck`, `RecoveryExhausted`) rather than adding a new top-level error type.
- **Replay, intent and invariants are model-free; never add model calls to them.** Code under `crates/orchyn-harness/src/replay/`, `src/intent/` and `src/invariants/` must be deterministic and must not call `ModelClient`.
- **Navigator discipline** (see `skills/codebase-navigation/SKILL.md`): every claim needs a `SourceRef`, and file reads are range-bounded (max 200 lines). That skill still describes Qwen 3.5 4B; the model in use is 0.8B. The sibling directories under `skills/` (`actor-generation`, `gap-detection`, `platform-adapters`, `spec-inference`, `vlm-prompting`) hold constraints for the other agent areas. Consult the relevant `SKILL.md` before changing agent behaviour, and note that vision is not available (see `ARCHITECTURE.md`, "Pixel-level evidence today").
- **Prompts live in `prompts/`** (`system.md`, `agent_system.md`, `action_planner.md`, `gap_reporter.md`, `spec_writer.md`), loaded with `include_str!`. Keep model-facing prompts there, not inline in `.rs` files.
- **Keep the docs in step with the code.** Update `ARCHITECTURE.md` in the same change when the crate graph or runtime model changes.
