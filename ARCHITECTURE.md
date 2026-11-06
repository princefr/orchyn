# Orchyn — Technical Architecture

Orchyn is a Rust harness for vibecoded web apps. It has two halves that
deliberately do different jobs:

- **Replay** runs a concrete, pre-written manifest against real browsers. No
  model is involved, so the verdict is deterministic.
- **Agent** lets a small local model explore an app and report what it finds.
  It is exploratory and currently unreliable (see below).

Reality-gap reconciliation sits between them: it compares what the code calls
and serves (static intent) with what the browser actually sent and received
(observed traffic) and classifies the disagreements.

This document is the source of truth for the crate graph and runtime model.
If something here disagrees with the code, the code wins; please update this
file in the same PR.

---

## Modes and the intended workflow

| | **Replay** | **Agent** |
|---|---|---|
| Purpose | Run a manifest step by step and give a verdict | Explore an app, record gaps, emit a report |
| Code | `crates/orchyn-harness/src/replay/` | `crates/orchyn-harness/src/agent/` |
| Model | None | Qwen 3.5 0.8B through `QwenClient` |
| Determinism | Same manifest + same app state gives the same verdict | Low; the model's choices vary between runs |
| Entry point | `orchyn-bench` (there is no replay subcommand in `orchyn-cli`) | `orchyn-cli --app ...` |
| Output | `ReplayReport` (per-step results, observed requests, console errors) | `AgentReport` (gap, test and fix counts, final report JSON) |

**Intended workflow: explore once, replay forever.** An explorer (a human, a
frontier model, or agent mode) writes a replay manifest from the product spec
and the running app. Replay runs that manifest on every commit with no model.
Reality-gap reconciliation explains failures in terms of code intent versus
observed traffic. The local model is meant for authoring and triage, not for
clicking in CI.

Today the pieces exist but the loop is not closed: agent mode does not emit a
replay manifest, and nothing in the CLI runs replay. Manifests are currently
written by hand or by an external explorer, and replay is driven by
`orchyn-bench`.

**Reconciliation** (`crates/orchyn-harness/src/intent/`) is pure and has no
model. It is used by `orchyn-bench` only; neither `orchyn-cli` nor the agent
calls it yet.

---

## Single Model Policy

All model calls go through `crates/model-client`'s `QwenClient`.

- The model is **Qwen 3.5 0.8B**, quantised to Q4_K_M. It is fetched from the
  Hugging Face repo `bartowski/Qwen_Qwen3.5-0.8B-GGUF` (file
  `Qwen_Qwen3.5-0.8B-Q4_K_M.gguf`) and run with `llama-cpp-2`.
- It is downloaded lazily, on the first model call, not at construction.
- It is **text-only**. `QwenClient::observe` returns
  `OrchynError::Model` ("vision observation unavailable"); no mmproj is wired.
  Nothing in the codebase currently sends pixels to a model.
- `QwenClient::new` takes a `ModelConfig`, but the engine does not read its
  fields. Generation length comes from each `CompletionRequest`, with a default
  of 512 tokens.
- `crates/model-client/src/remote.rs` (an HTTP `observe` client) is not declared
  in `lib.rs`, so it is not compiled.

Do not add alternate model backends without updating this section.

---

## Pixel-level evidence today

There is no vision model. The pixel-level checks that exist are:

1. **Occlusion in replay.** Replay `click` and `expect_visible` scroll the
   element into view, then probe `elementFromPoint` at its centre. If another
   element covers that point, the step fails with "covered by ...". This is the
   "DOM says clickable, pixels say not" check. `click` then sends a real mouse
   event at the centre.
2. **Screenshot annotation.** `pixel-observer::annotate_screenshot` draws
   numbered boxes on a PNG and returns the matching element map. No current
   code path feeds the annotated image to a model.

---

## Workspace layout

```
orchyn/
├── Cargo.toml                 # workspace root: "crates/*" glob plus explicit members
├── rust-toolchain.toml        # channel = "stable"
├── crates/
│   ├── core/                  # orchyn-core: OrchynError, Action enums, types, PlatformAdapter trait
│   ├── model-client/          # orchyn-model-client: ModelClient trait, QwenClient (0.8B), MockClient
│   ├── orchyn-harness/        # replay/, intent/, agent/, DiffEngine, legacy OrchynHarness
│   ├── platform-web/          # orchyn-platform-web: WebAdapter over headless_chrome
│   ├── action-executor/       # re-exports core::{Action, DeviceInfo, PlatformAdapter, Result}
│   ├── pixel-observer/        # annotate_screenshot(); legacy PixelObserver<M>
│   ├── toolkit/               # orchyn-toolkit: tool registry used by agent mode
│   ├── actor-engine/          # orchyn-actor-engine: legacy phase-based actor
│   ├── runtime-tracer/        # telemetry record types
│   ├── pattern-library/       # PatternId and related types
│   ├── report-generator/      # report prose through ModelClient (gap_reporter prompt)
│   ├── scheduler/             # run scheduling over storage (used by orchyn-worker)
│   ├── storage/               # orchyn-storage: PostgreSQL via sqlx, schema creation
│   ├── artifact-storage/      # orchyn-artifact-storage: artifact traits and error type
│   ├── api-server/            # orchyn-api-server: axum routes, GraphQL schema, Stripe webhooks, auth
│   └── tests/                 # orchyn-tests: cross-crate unit tests
├── bin/
│   ├── orchyn-cli/            # explorer: agent mode, optional --diff
│   ├── orchyn-server/         # axum + GraphQL API on 127.0.0.1:3000
│   ├── orchyn-worker/         # scheduler worker
│   └── orchyn-bench/          # scores replay manifests against bug-seeded fixtures
├── bench/                     # suite.json, fixtures/<name>/ (clean and BUG=1), manifests/, FEATURE.md specs
├── tests/
│   ├── fixtures/              # Node apps: broken-counter, broken-login, qa-target, todo-app
│   └── integration/           # orchyn-integration-tests (end-to-end)
├── skills/                    # per-area guidance (SKILL.md) for agent behaviour
├── prompts/                   # model-facing prompts, loaded with include_str!
└── migrations/                # 0001_init.sql (not executed by the code)
```

The crates `navigator-agent`, `action-planner`, `spec-engine`, `gap-detector`,
`fixer-agent` and `codebase-indexer` do not exist in this repository. The
legacy phase pipeline described below (`OrchynHarness`) does not depend on them.

---

## Crate responsibilities

### `core`

- `OrchynError` variants: `Internal`, `Model`, `Storage`, `Platform`, `Auth`,
  `Validation`, `Stuck`, `RecoveryExhausted`. The alias is
  `orchyn_core::Result<T>`.
- Two action vocabularies coexist:
  - `core::traits::Action`: coordinate-based (`Tap`, `Type`, `Wait`, `Reset`),
    used by the legacy phase pipeline.
  - `core::action::Action`: element-id-based (`Navigate`, `Click { target }`,
    `Type { target, value }`, `Clear`, `Drag`, `Wait`, `AssertSuccess`,
    `AssertFailure`, `WsWait`), used by agent mode and by the replay adapter
    contract.
- `core::PlatformAdapter` has default implementations of `scrape_elements`,
  `current_url`, `collect_telemetry`, `execute_harness` and `shutdown`.

### `model-client`

```rust
pub trait ModelClient: Send + Sync + Debug {
    fn complete(&self, req: CompletionRequest) -> BoxFuture<'_, Result<CompletionStream>>;
    fn tool_call(&self, req: ToolCallRequest) -> BoxFuture<'_, Result<ToolCallResult>>;
    fn observe(&self, image: &[u8], prompt: &str) -> BoxFuture<'_, Result<serde_json::Value>>;
    fn model_id(&self) -> &str;
}
```

- `complete` streams tokens from llama.cpp (greedy sampling with a repetition
  penalty over the last 128 tokens).
- `tool_call` wraps `complete` with a prompt that asks for
  `{"name", "arguments"}` JSON and parses the reply.
- `observe` always errors today (see above).
- `MockClient` returns scripted responses and is used by tests.

### `pixel-observer`

- `annotator::annotate_screenshot(raw_png, &[UiElement])` draws numbered red
  boxes and returns the `AnnotatedElement` map. Digits come from a hand-coded
  5×7 bitmap.
- `observer::PixelObserver<M>` is the legacy VLM-driven state extractor. It
  calls `observe`, so it fails at runtime with the current model client.

### `platform-web`

`WebAdapter` implements `core::PlatformAdapter` on top of **`headless_chrome`**
(not Playwright).

- `WebAdapter::new(url)` launches Chrome. The binary comes from the `CHROME`
  environment variable when set; otherwise `headless_chrome` finds one.
  Sandboxing is on unless `ORCHYN_NO_SANDBOX=1`, which is needed when running as
  root or in a container.
- `execute_harness` performs navigation, clicks (centre of the element's bounds)
  and typing.
- `collect_telemetry` drains console messages and network requests for the
  replay runner.

### `toolkit`

The tool registry used by agent mode. Each tool implements:

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;
    fn execute<'a>(&'a self, args: serde_json::Value, ctx: &'a ToolContext)
        -> BoxFuture<'a, Result<ToolResult>>;
}
```

`default_registry()` registers, among others:

- **Files and shell**: `read_file`, `write_file`, `edit_file`, `grep`, `glob`,
  `bash` (gated), `enter_worktree`, `exit_worktree`.
- **Git and web**: `git_log`, `git_diff`, `git_show`, `git_status`,
  `web_fetch`, `web_search`.
- **Browser**: `browser_navigate`, `browser_click`, `browser_type`,
  `browser_screenshot`, `browser_get_console`, `browser_get_network`,
  `browser_get_ws_messages`, `browser_get_storage`, `browser_get_performance`,
  `browser_check_accessibility`, `browser_set_viewport`.
- **Reporting**: `gap_report`, `test_result`, `fix_propose`, `memory_store`,
  `memory_recall`, `done`.
- **Other**: `agent` and `send_message` (sub-agents), `task_*`, `team_*`,
  `cron_*`, `todo_write`, `tool_search`, `skill`, `sleep`, `brief`, `config`,
  plan-mode and `ask_user_question`.

`ToolContext` carries the shared `ActorMap`, a `WorkingMemory`, an optional
`AdapterFactory`, the project root, `allow_shell` (off by default; `orchyn-cli`
turns it on) and the tool index used by `tool_search`.

### `orchyn-harness`

**`replay/`: deterministic runner (no model).** See
[Replay manifests](#replay-manifests) below.

**`intent/`: static intent and reconciliation (no model).** See
[Reality gaps](#reality-gaps) below.

**`agent/`: `AgentHarness`.**

- `AgentHarness::new(llm, registry, ctx, max_turns)`, `subscribe()` for a
  `broadcast` of `AgentProgressEvent`s, and `run(app_url)` returning an
  `AgentReport`.
- Each turn builds a prompt (system text, tool schemas, recent messages,
  observations) and calls `QwenClient::complete`. The reply is parsed as
  `{reasoning, tool_calls, done}` and the tool calls are dispatched through the
  registry. The loop stops on `done` or at `max_turns` (the CLI default is 40).
- **Model idling is overridden by script.** If the reply cannot be parsed, or it
  has no tool calls and `done` is false, `derive_action_from_state` replaces it
  with a fixed action chosen from the observed state. This is not a model
  decision. The fallback:
  - starts `npm run dev` through `bash` when the page is `about:blank`;
  - otherwise clicks the first button, input or submit element;
  - after an interaction, calls `read_file("server.js")`;
  - after reading that file, files the canned gap "Logic bug found in source code
    — see server.js" with severity critical and returns `done`.

  The canned gap is filed whatever the app is, so **agent-mode gap reports are
  not evidence until this fallback is removed or made to depend on observed
  data.** The fallback is also hard-wired to `server.js`, `/tmp` paths and the
  actor name `explorer`.

**`diff_engine.rs`: `DiffEngine`.** See [Differential mode](#differential-mode).

**`harness.rs`: `OrchynHarness`, legacy.** A phase pipeline over the `Phase`
enum (`Ingest`, `Observation`, `GapDetection`, `FixLoop`, `Ship` and the
`VisionBaseline`, `SingleActorFlow`, `MultiActorFlow`, `ErrorConstraint`,
`Harden` phases). `FixLoop` is a stub.

**`lib.rs`** also exposes `LlmClient`, `RealLlm` and `MockLlm`, thin wrappers
around `Arc<dyn ModelClient>`.

**Known build break.** `crates/orchyn-harness/tests/harness_tests.rs` builds a
`HarnessConfig` without the `phases` field, so that integration-test target
does not compile. Run harness unit tests with `cargo test -p orchyn-harness --lib`.

### `bin/orchyn-cli`

A single command with flags, no subcommands. The clap name is `orchyn`; the
Cargo package is `orchyn-cli`.

```text
orchyn-cli --app <URL-or-path> [--max-turns 40] [--project-root .]
orchyn-cli --app <URL-or-path> --diff <FROM_REF> <TO_REF>
```

- Explore mode builds a `QwenClient`, a `WebAdapter` factory, the default
  registry with shell enabled, and runs `AgentHarness` with a TUI
  (`bin/orchyn-cli/src/tui.rs`). It prints a report at the end.
- `--app` is a URL or a local path. With a local path, the agent is expected to
  start the app itself through its `bash` tool.

There is no `run`, `agent`, `validate` or `diff` subcommand, and no manifest
input or output.

### `bin/orchyn-bench`

See [Benchmark](#benchmark) below.

### `bin/orchyn-server` and `bin/orchyn-worker`

- `orchyn-server` binds `127.0.0.1:3000` and serves the GraphQL schema from
  `orchyn_api_server::build_schema`, Google OAuth (callback
  `http://localhost:3000/auth/google/callback`), and Stripe webhooks. It needs
  `DATABASE_URL`, `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `JWT_SECRET`,
  `FRONTEND_URL` and `STRIPE_WEBHOOK_SECRET`.
- `orchyn-worker` connects to PostgreSQL (`DATABASE_URL` is required) and runs
  the scheduler.

---

## Replay manifests

A manifest is a JSON object. Each step names an op, and may name an `actor`
(default `"default"`).

```json
{
  "name": "login",
  "base_url": "http://localhost:8092",
  "steps": [
    {"op": "goto", "path": "/"},
    {"op": "fill", "selector": "#email", "value": "alice@test.com"},
    {"op": "click", "selector": "button[type=submit]"},
    {"op": "expect_text", "text": "Welcome"},
    {"op": "expect_request", "method": "POST", "path_contains": "/api/login", "status": 200},
    {"op": "expect_console_clean"},
    {"actor": "bob", "op": "goto", "path": "/"}
  ]
}
```

Ops, from `crates/orchyn-harness/src/replay/manifest.rs`:

| Op | Fields | Behaviour |
|---|---|---|
| `goto` | `path` | Navigates to `base_url` + `path` (or `path` if absolute) |
| `click` | `selector` | Probe, then real mouse click at centre. Fails if missing, zero-size, or covered |
| `click_text` | `text` | As `click`, on the first visible button, link or `[role=button]` with that trimmed text |
| `fill` | `selector`, `value` | Real click to focus, clear, type |
| `wait_ms` | `ms` | Sleep |
| `wait_for_text` | `text`, `timeout_ms` (3000) | Poll `document.body.innerText` |
| `expect_text` | `text`, `timeout_ms` (3000) | Poll until the text appears |
| `expect_no_text` | `text` | Text must be absent (checked once) |
| `expect_url` | `contains` | Current URL must contain the string |
| `expect_visible` | `selector` | Exists, non-zero size, not hidden, not covered at centre |
| `expect_console_clean` | | No console errors or uncaught exceptions for this actor since start or last reset |
| `expect_request` | `method`, `path_contains`, `status?` | At least one matching request for this actor |
| `expect_no_failed_requests` | | No response status >= 400 and no network failures for this actor |
| `reset_telemetry` | | Forget this actor's telemetry so far |

Runner behaviour (`replay/runner.rs`):

- Each distinct actor gets its own browser (`WebAdapter`), created on first use
  by an `AdapterFactoryFn`.
- The run halts at the first failing step. `ReplayReport` holds the executed
  steps (`StepResult` with actor, op, passed, detail, duration), `first_failure`,
  all observed requests, and all console errors formatted as `actor: message`.
- Actors run sequentially in manifest order; the runner does not interleave them.

---

## Reality gaps

`crates/orchyn-harness/src/intent/` answers: where does the code disagree with
what the running app did?

**Static extraction**, `extract_intent(root)`:

- Client calls: `fetch(...)` (method read from a `method:` option within 300
  characters, default GET), `axios(...)`, `axios.get|post|put|patch|delete`,
  `$.get` and `$.post`, `new EventSource(...)`, and `<form action method>`.
- Server routes: `app|router|server|api` `.get|post|put|patch|delete|all(...)`
  (`all` maps to method `ANY`).
- Scans `js mjs cjs jsx ts tsx html vue svelte`. Skips `node_modules`, `.git`,
  `dist`, `build`, `.next`, `target` and `coverage`, and files over 1 MB.

**Reconciliation**, `reconcile(intent, observed) -> Vec<RealityGap>`:
static asset requests are ignored, and the result is sorted and de-duplicated.

| `GapKind` | Source | Meaning |
|---|---|---|
| `ClientCallWithoutRoute` | static | A client call matches no server route. Only checked when routes were found |
| `ObservedFailure` | runtime | A request returned status 0 or >= 400 |
| `UnexpectedRequest` | runtime | An observed `/api/...` request matches no client call and no route |
| `UnexercisedClientCall` | runtime | A client call never fired during the run (informational). Only checked when there was traffic |

Each `RealityGap` carries method, path, observed status, the source call
(file and line) when known, and a message.

Reconciliation is used by `orchyn-bench` only. The agent does not call it.

---

## Benchmark

`orchyn-bench` scores replay manifests against bug-seeded fixtures.

```bash
npm install --prefix bench          # or: cd bench && npm install
cargo run -p orchyn-bench -- --runs 3 [--suite bench/suite.json] [--manifests DIR] [--label L] [--out FILE] [--only NAME]
```

- `suite.json` is a JSON array of `{name, dir, bug_class, bug_description,
  manifest}`. `dir` and `manifest` resolve relative to the suite file's
  directory (`bench/`).
- Each fixture is `bench/fixtures/<name>/` with `server.js`, a `FEATURE.md` spec
  and a `public/` front end. It is started twice per run: `BUG=0` (clean) and `BUG=1`
  (planted bug). `PORT` is set to a free port, and startup waits up to 10 s for a
  line containing `listening`.
- Each variant's manifest runs `--runs` times.
- Per fixture:
  - **detected**: every buggy run failed and did not error;
  - **false_positive**: any clean run failed;
  - **flaky**: verdicts differ between runs of the same variant;
  - **gap_detected**: every buggy run has more reality gaps than the largest
    gap count among the clean runs;
  - **mean_run_ms**: average run time across the suite.
- `--manifests DIR` looks for `<fixture>.json` there instead of each suite
  entry's `manifest`, which lets you score other authors' manifests. `--out`
  writes full JSON results.

The suite currently has eight fixtures: login-session, counter, create-500,
route-mismatch, console-crash, covered-button, realtime-sync and hidden-error.
The reference manifests are in `bench/manifests/`. Results are written to
`bench/results/` when `--out` is given.

---

## Differential mode

`orchyn-cli --app URL --diff FROM TO` resolves both refs with `git2` and
computes the code diff. It then runs the agent **twice against the same `--app`
URL** and compares gap descriptions. A regression is a gap description that
appears only in the second run. The exit status is 1 on regression.

It does **not** check out either ref. The worktree step described in the
`DiffEngine::compare` doc comment is not implemented, so the two runs see the
same code and differ only through model variation. Treat the result as noise
until real per-ref checkouts exist.

---

## Storage

`orchyn-storage` uses PostgreSQL through sqlx.

- `StorageConfig` reads `DATABASE_URL` (default `postgres://localhost/orchyn`)
  and `ARTIFACT_ROOT` (default `./storage/artifacts`).
- On startup, `Storage::new` creates these tables with `CREATE TABLE IF NOT
  EXISTS`: `users`, `projects`, `project_members`, `invitations`, `runs`,
  `artifacts`.
- `migrations/0001_init.sql` is not run by the code.
- `ARTIFACT_ROOT` is configuration only. No route in this repository serves it.

The CLI does not use storage, so `orchyn-cli` runs without PostgreSQL.

---

## Concurrency and implementation notes

- **Trait-object async**: traits used as `dyn` (`PlatformAdapter`, `ModelClient`,
  `Tool`, `AdapterFactory`) return `BoxFuture<'_, T>`. Native `async fn` in traits
  is used where `dyn` is not required.
- **Errors**: crates return `orchyn_core::Result<T>`, which uses `OrchynError`.
- **Edition**: Rust 2024. The toolchain is `stable`.
- **Build hygiene**: prefer `cargo check -p <crate>` or `cargo test -p <crate> --lib`
  over workspace-wide builds. A full workspace build is heavy and can thrash `target/`.

---

## Prompts and skills

- `prompts/` holds model-facing prompts, loaded with `include_str!`:
  `system.md` (pixel observer), `gap_reporter.md` (report-generator), and
  `agent_system.md`, `action_planner.md` and `spec_writer.md`, which are only
  referenced by `_PROMPT_*` constants that nothing uses.
- `skills/` holds per-area guidance for agent behaviour: `codebase-navigation`,
  `actor-generation`, `gap-detection`, `platform-adapters`, `spec-inference`
  and `vlm-prompting`. Some of these still describe older model sizes and the
  VLM path; treat them as guidance, not as current fact.

---

## Legacy paths

These still compile and are kept while the phase pipeline is migrated:

- `harness::OrchynHarness` and its `Phase` enum (see above).
- `actor-engine` (phase-based actor loop). It calls `observe` and fails at
  runtime.
- `pixel-observer::PixelObserver`.
- `core::traits::Action` (coordinate-based).
- `core::ManifestStep`, which belongs to the phase pipeline: a JSON array of
  `{step, actor, instruction, on_fail, action_hint}`. It is not the replay format.

---

## End-to-end flow (today)

```
EXPLORER (human, frontier model, or agent mode)
    - writes a replay manifest for the app (JSON, by hand today)
    |
    v
REPLAY (orchyn-bench; per commit in CI once wired)
    - one browser per actor, no model
    - halts at the first failing step
    - ReplayReport: steps, observed requests, console errors
    |
    v
RECONCILIATION (intent/)
    - static client calls and server routes vs observed requests
    - ObservedFailure / UnexpectedRequest / UnexercisedClientCall / ClientCallWithoutRoute

AGENT MODE (orchyn-cli --app URL) is separate and exploratory; see the
caveat under orchyn-harness. It does not feed replay yet.
```
