# Orchyn — Technical Architecture

A Rust harness that takes vibecoded apps to production by reconciling
*inferred code intent* against *observed pixel reality*. Orchyn detects
**reality gaps** — the delta between what the codebase claims and what the
running pixels show — and makes those gaps visible, reproducible, and
fixable.

This document is the source of truth for the crate graph and runtime model.
If something here disagrees with the code, the code wins; please update this
file in the same PR.

---

## Two modes, one runtime

Orchyn operates in two modes that share infrastructure but serve different
purposes:

|              | **Constraint Mode**                          | **Agent Mode**                                |
|--------------|----------------------------------------------|-----------------------------------------------|
| Purpose      | Replay a pre-written manifest deterministically | Explore an app, find gaps, propose tests/fixes |
| Driver       | `crates/orchyn-harness/src/constraint`       | `crates/orchyn-harness/src/agent`             |
| Brain        | Local Qwen 3.5 0.8B (single model policy)    | Local Qwen 3.5 0.8B (single model policy)     |
| Determinism  | High — same manifest, same result            | Low — different exploration each run          |
| Use case     | CI / regression / smoke tests                | Onboarding new apps, generating manifests     |
| Output       | Pass/Fail run record + actor states          | `manifest.json` + gaps + fixes in working memory |

**Hybrid bridge.** Agent Mode discovers an app and emits a `manifest.json`.
Constraint Mode replays that manifest on every commit. Manifests are JSON,
read/written by `orchyn-cli`.

---

## Single Model Policy

All LLM and VLM calls in the codebase route through `crates/model-client`'s
`QwenClient`, which serves Qwen 3.5 0.8B locally. Both modes use the same
client; they differ in *how* they prompt it (Constraint Mode sends a
screenshot + element map, Agent Mode sends a tool-using JSON dialogue).

Do not add alternate model backends without updating this section.

---

## Workspace layout

```
orchyn/
├── Cargo.toml                      # workspace root
├── rust-toolchain.toml             # pin to stable
├── crates/
│   ├── core/                       # shared types, errors, traits
│   ├── model-client/               # QwenClient (local 0.8B; text + vision)
│   ├── pixel-observer/             # screenshot annotator + legacy VLM observer
│   ├── platform-web/               # headless_chrome web adapter (PlatformAdapter impl)
│   ├── action-executor/            # re-export shim around core::traits
│   ├── toolkit/                    # tool registry: browser/code/meta tools (Agent Mode)
│   ├── orchyn-harness/             # constraint::TestRunner + agent::AgentHarness
│   ├── actor-engine/               # legacy phase-based actor (slated for migration)
│   ├── codebase-indexer/           # static analysis (used by legacy ingest phase)
│   ├── runtime-tracer/             # network/console instrumentation
│   ├── navigator-agent/            # legacy code navigator
│   ├── action-planner/             # legacy planner
│   ├── spec-engine/                # legacy spec model
│   ├── gap-detector/               # legacy reality-gap reporter
│   ├── fixer-agent/                # legacy fix loop
│   ├── pattern-library/            # cross-app bug pattern store
│   ├── report-generator/           # video + narration + patch proposal
│   ├── scheduler/                  # test run orchestration
│   ├── storage/                    # PostgreSQL persistence (sqlx)
│   ├── artifact-storage/           # screenshot/video object store
│   ├── api-server/                 # HTTP API, webhooks
│   └── tests/                      # cross-crate unit tests
├── bin/
│   ├── orchyn-cli/                 # `orchyn agent | run | validate`
│   ├── orchyn-server/              # axum + GraphQL on :3000
│   └── orchyn-worker/              # scheduler / test executor
└── tests/
    ├── fixtures/                   # sample apps (e.g. todo-app)
    └── integration/                # end-to-end tests
```

The legacy crates (`navigator-agent`, `action-planner`, `spec-engine`,
`gap-detector`, `fixer-agent`, `codebase-indexer`) wire into the older
phase-based `orchyn-harness::OrchynHarness`. The new hybrid runtime does not
require them, but they are kept while the legacy entrypoint is migrated.

---

## Crate responsibilities

### `core`

Shared types used across all crates. Two action vocabularies coexist:

- `core::traits::Action` — coordinate-based legacy actions (`Tap`, `Type`,
  `Wait`, `Reset`) used by the legacy phase-based harness.
- `core::action::Action` — element-id-based hybrid actions (`Navigate`,
  `Click { target }`, `Type { target, value }`, `Clear`, `Drag`, `Wait`,
  `AssertSuccess`, `AssertFailure`, `WsWait`) used by Constraint and Agent
  modes.

Hybrid types (`AnnotatedElement`, `BoundingBox`, `LlmDecision`,
`TelemetrySnapshot`, `ActorState`, `ManifestStep`, `RunStatus`,
`TestRunRecord`, …) live in `core::action` and `core::types`.

`core::PlatformAdapter` carries default impls for the new methods
(`scrape_elements`, `current_url`, `collect_telemetry`, `execute_harness`,
`shutdown`) so legacy adapters that haven't been migrated still compile.

### `model-client`

Single async interface to local Qwen 3.5 0.8B (text + vision). Trait:

```rust
pub trait ModelClient: Send + Sync + Debug {
    fn complete(&self, req: CompletionRequest) -> BoxFuture<'_, Result<CompletionStream>>;
    fn tool_call(&self, req: ToolCallRequest) -> BoxFuture<'_, Result<ToolCallResult>>;
    fn observe(&self, image: &[u8], prompt: &str) -> BoxFuture<'_, Result<serde_json::Value>>;
    fn model_id(&self) -> &str;
}
```

Constraint Mode uses `observe(image, prompt)` (annotated screenshot in,
JSON decision out). Agent Mode uses `complete(req)` with a flat text
prompt that includes tool schemas.

### `pixel-observer`

- `annotator::annotate_screenshot(raw_png, &[UiElement])` — draws red
  numbered overlays and emits the matching `AnnotatedElement` map. Used by
  both modes. No font asset bundled; digits are a 5×7 hand-coded bitmap.
- `observer::PixelObserver<M>` — legacy VLM-driven `UiState` extractor
  (still used by the phase-based pipeline).

### `platform-web`

`WebAdapter` (headless_chrome) implements `core::PlatformAdapter`,
including the hybrid extensions:

- `scrape_elements()` — runs an in-page JS snippet that enumerates
  buttons / inputs / links / textareas / `[role=...]` elements with their
  bounding rects (filtering out 0-area or invisible elements).
- `execute_harness(action, element_map)` — translates `core::action::Action`
  variants into headless_chrome operations (centroid clicks for
  element-id-based gestures).
- `current_url()`, `collect_telemetry()` — browser state for the harness.

### `toolkit`

Tool registry for Agent Mode. Tools follow the `BoxFuture` pattern:

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;
    fn execute<'a>(&'a self, args: serde_json::Value, ctx: &'a ToolContext)
        -> BoxFuture<'a, Result<ToolResult>>;
}
```

Built-in tools:

- **Browser**: `browser_screenshot`, `browser_navigate`, `browser_click`,
  `browser_type`, `browser_get_console`.
- **Code**: `code_read`, `code_search` (regex via `ignore` walker;
  enforces project-root containment).
- **Meta**: `actor_spawn` (uses an injected `AdapterFactory`),
  `memory_store`, `memory_recall`, `gap_report`, `test_propose`,
  `fix_propose`, `done`.

`ToolContext` holds a shared `ActorMap`, a `WorkingMemory` scratchpad, an
`AdapterFactory`, and the project root.

### `orchyn-harness`

- `constraint::TestRunner` — replays manifests step-by-step. Per turn it
  screenshots, scrapes the element map, annotates, sends `(image, prompt)`
  to the model, validates the JSON decision against the element map, and
  executes via `PlatformAdapter::execute_harness`. Stuck detection uses a
  64-bit perceptual difference hash with a sliding window.
- `agent::AgentHarness` — runs an autonomous tool-using loop. Each turn
  builds a flat prompt (system + tool schemas + recent messages +
  observations), parses the model's `{reasoning, tool_calls, done}` JSON,
  dispatches calls through the toolkit, and stops on `done` or
  `max_turns`.
- `harness::OrchynHarness` — legacy phase-based pipeline (`Ingest →
  Observation → GapDetection → FixLoop → Ship`). Currently stubbed at the
  FixLoop step pending migration.

### `bin/orchyn-cli`

```text
orchyn agent  --app-url URL --goal "..." [--max-turns N] [--output manifest.json]
orchyn run    --manifest manifest.json --app-url URL [--actors a,b,c]
orchyn validate --manifest manifest.json
```

Agent Mode emits a manifest derived from `test_*` entries in the agent's
working memory. Constraint Mode reads that manifest, mints one
`WebAdapter` per distinct actor, and runs the deterministic loop.

---

## Concurrency & implementation notes

- **Trait-object async**: traits that are used as `dyn Trait` (e.g.
  `PlatformAdapter`, `ModelClient`, `Tool`, `AdapterFactory`) return
  `futures::future::BoxFuture<'_, T>`. Native `async fn` in traits is used
  only where `dyn` is not required.
- **Errors**: every crate flows errors through `core::OrchynError`
  (`Internal`, `Model`, `Storage`, `Platform`, `Auth`, `Validation`,
  `Stuck`, `RecoveryExhausted`).
- **Edition**: Rust 2024.
- **Build hygiene**: prefer `cargo check -p <crate>` over workspace-wide
  builds — the workspace is heavy and concurrent rust-analyzer checks can
  thrash `target/`.

---

## Manifest format (Constraint Mode)

A manifest is a JSON array of `ManifestStep`:

```json
[
  {
    "step": 0,
    "actor": "alice",
    "instruction": "Sign in with the demo account",
    "on_fail": "halt",
    "action_hint": null
  },
  {
    "step": 1,
    "actor": "alice",
    "instruction": "Verify the dashboard greets the user",
    "on_fail": "retry"
  }
]
```

`on_fail` is one of `halt | skip | retry` (defaults to `halt`).
`action_hint` is optional free text passed through to the model.

---

## End-to-end flow

```
HUMAN -> "Test my app"
    |
    v
AGENT MODE  (orchyn agent --app-url http://localhost:3456 --goal "...")
    - explores via browser_*, code_*, memory_*
    - records gaps with gap_report, tests with test_propose
    - emits manifest.json
    |
    v
HUMAN REVIEW
    - edits manifest.json (adds edge cases, fixes copy)
    |
    v
CONSTRAINT MODE  (orchyn run --manifest manifest.json --app-url ...)
    - runs once per commit in CI
    - same model (0.8B), deterministic prompts
    - pass/fail + actor states + run record
```
