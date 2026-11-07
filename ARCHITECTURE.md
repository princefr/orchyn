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
| Entry point | `orchyn-check` (one app, one run) or `orchyn-bench` (scored fixtures); there is no replay subcommand in `orchyn-cli` | `orchyn-cli --app ...` |
| Output | `ReplayReport` (per-step results, observed requests and API exchanges, console errors) | `AgentReport` (gap, test and fix counts, final report JSON) |

**Intended workflow: explore once, replay forever.** An explorer (a human, a
frontier model, or agent mode) writes a replay manifest from the product spec
and the running app. Replay runs that manifest on every commit with no model.
Reality-gap reconciliation explains failures in terms of code intent versus
observed traffic. The local model is meant for authoring and triage, not for
clicking in CI.

Today the pieces exist but the loop is not closed: agent mode does not emit a
replay manifest, and nothing in the CLI runs replay. Manifests are currently
written by hand or by an external explorer, and replay is driven by
`orchyn-check` and `orchyn-bench`.

**Reconciliation** (`crates/orchyn-harness/src/intent/`) is pure and has no
model. It is used by `orchyn-check` and `orchyn-bench`; neither `orchyn-cli` nor
the agent calls it yet.

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
   event at the centre. The `expect_all_clickable` op applies the same test to
   every visible, enabled control, and `click_each` applies it to each control
   it clicks (a covered control is skipped and reported).
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
│   ├── orchyn-harness/        # replay/, intent/, invariants/, agent/, DiffEngine, legacy OrchynHarness
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
│   ├── orchyn-check/          # grounded checks of a running app: manifests, --universal, --contracts, --lint-spec, --record, --baseline
│   └── orchyn-bench/          # scores replay manifests against bug-seeded fixtures (--mode)
├── bench/                     # suite.json, fixtures/<name>/ (clean and BUG=1, PROMPTS.md), manifests/, results/, webbench/
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
  `current_url`, `collect_telemetry`, `execute_harness`, `shutdown` and
  `take_exchanges` (which returns none by default).
- `core::Exchange { method, url, status, request_body, response_body }` is one
  API exchange. Bodies are optional.

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
- JavaScript dialogs (`alert`, `confirm`, `prompt`) are accepted as soon as they
  open, and recorded as console entries prefixed `[dialog] `. `expect_dialog`
  reads them.
- **Shared browser.** By default one Chromium serves every `WebAdapter` in the
  process, and each adapter gets its own incognito context in it, so cookies and
  storage stay isolated. The shared browser is relaunched after 8 adapters (its
  incognito contexts cannot be disposed through the public API) and is killed by
  PID at process exit. `ORCHYN_BROWSER_PER_ADAPTER=1` restores one Chromium per
  adapter.
- `mouse_move(x, y)` moves the real pointer. Adapters that do not implement it
  return an error.
- **Popups.** Every tab, including popups, gets the same instrumentation.
  `switch_to_new_tab(timeout)` waits for a tab whose opener is one of this
  adapter's tabs and makes it active. `switch_to_tab(index)` activates a tab the
  adapter already has (0 is the original tab). A same-origin popup that
  calls `alert()` while loading freezes its opener too (shared event loop).
  This is handled by two patches to a vendored headless_chrome
  (`vendor/headless_chrome/ORCHYN_PATCHES.md`: new-tab handles are built off
  the browser event thread, and pending dialogs on a new tab are accepted while
  it is being enabled) together with the default browser choice below; with
  full Chrome (`CHROME=.../chrome`) that case can still hang.
- **Browser binary.** `CHROME` if set; otherwise Playwright's
  `chrome-headless-shell` when installed (under `PLAYWRIGHT_BROWSERS_PATH` or
  `~/.cache/ms-playwright`); otherwise headless_chrome's own lookup.
- `take_exchanges` returns XHR and fetch exchanges only. A response body is kept
  when it is non-empty, not base64 and at most 16 KiB.

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

**`invariants/`: invariant candidates (no model).** Universal page rules, code
contracts, transcripts, production logs and mined traces. See
[Invariants](#invariants) below.

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

### `bin/orchyn-check`

Grounded checks for a running app, for a coding agent or a person. No model.

```text
orchyn-check --url <base-url> [--project .] [--manifest FILE ...] [--universal] [--contracts]
orchyn-check --lint-spec FILE ...                        # no --url needed
orchyn-check --url <base-url> --manifest FILE ... --record DIR [--runs 3]
orchyn-check --url <base-url> --manifest FILE ... --baseline DIR
```

- `--url`: the running app. A manifest's relative `goto` paths resolve against it.
  Required unless only `--lint-spec` is given.
- `--project` (default `.`): the source tree for code intent and contracts.
- `--manifest` (repeatable): each manifest is replayed, and each failing step is
  printed. A halted run says so.
- `--universal`: each universal candidate (see [Invariants](#invariants)) is
  replayed as its own manifest. A failing candidate with confidence 0.9 or more
  fails the run. Lower-confidence failures print as advisory.
- `--contracts`: prints the statements of the invariants the code declares. It
  does not run them.
- `--lint-spec FILE` (repeatable): runs first. Prints the spec ambiguities found
  by the linter (see [Spec ambiguity lint](#spec-ambiguity-lint-ambiguityrs)).
  Informational only.
- `--record DIR`: for each `--manifest`, replays its actions only (see
  `ReplayManifest::actions_only`) `--runs` times (default 3, at least 2). It
  writes `DIR/<manifest-stem>.baseline.json` with the API invariants (`mined`)
  and page invariants (`pages`) that held in every run. It returns before the
  manifest, universal and reconciliation output, and exits 0 when it succeeds.
  Conflicts with `--baseline`.
- `--baseline DIR`: for each `--manifest`, loads its baseline and replays the
  manifest's actions once. Each recorded invariant that no longer holds is
  printed. In this mode the manifest's own assertions are not evaluated, and
  the manifest is not run as a manifest. `--universal` and reconciliation still
  run afterwards.
- Always: prints reality gaps (`reconcile`), leaving out `UnexercisedClientCall`.
  Gaps do not affect the exit status.

Exit status is 0 when every manifest passes, no universal candidate with
confidence 0.9 or more fails, and (with `--baseline`) no recorded invariant is
broken. It is 1 otherwise, including when a manifest or baseline file cannot be
read or parsed. `--record` exits 0 when it writes its baselines.

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
| `click` | `selector` | Probe, then real mouse click at centre. Fails if missing, zero-size, or covered. Waits (up to 5 s) for a navigation the click starts |
| `click_text` | `text` | As `click`, on the first visible button, link or `[role=button]` with that trimmed text |
| `fill` | `selector`, `value` | Real click to focus, clear, type |
| `wait_ms` | `ms` | Sleep |
| `wait_for_text` | `text`, `timeout_ms` (3000) | Poll `document.body.innerText` |
| `expect_text` | `text`, `timeout_ms` (3000) | Poll until the text appears |
| `expect_no_text` | `text` | Text must be absent (checked once) |
| `expect_url` | `contains` | Current URL must contain the string |
| `expect_visible` | `selector` | Exists, non-zero size, not hidden, not covered at centre |
| `expect_console_clean` | | No console errors or uncaught exceptions for this actor since start or last reset. Chrome's `Failed to load resource` lines are not counted; `expect_no_failed_requests` judges those |
| `expect_request` | `method`, `path_contains`, `status?` | At least one matching request for this actor |
| `expect_no_failed_requests` | | No response status >= 400 and no network failures for this actor. `/favicon.ico` is ignored |
| `reset_telemetry` | | Forget this actor's telemetry so far |
| `expect_dialog` | `contains` | A JS dialog whose message contains the string was shown to this actor since start or last reset. Dialogs are accepted automatically |
| `expect_all_clickable` | | Every visible, enabled `button, a[href], input, select, textarea, [role=button]` is the hit target at its centre. Each is scrolled into view first; at most five offenders are listed |
| `reload` | | `location.reload()`, then wait for `document.readyState` to be `complete` (5 s limit), then settle for 300 ms |
| `snapshot_text` | `key` | Store this actor's normalised `document.body.innerText` under `key` |
| `expect_text_unchanged` | `key` | This actor's normalised text must equal the snapshot under `key`. Fails if no snapshot was taken |
| `click_each` | `selector?` (default `button, input[type=submit], [role=button]`) | Really click each visible, enabled match in DOM order, at most 20. A covered match is skipped. Fails on a console error or failed request caused by a click. A navigation caused by a click is undone before the next click |
| `hover` | `selector` | Probe as `click` (fails if missing or covered). Dispatches `pointerover`, `pointerenter`, `mouseover` and `mouseenter` at the centre, then moves the real pointer there with `mouse_move`, so CSS `:hover` applies. Adapters without `mouse_move` get the JS events only. Settles 250 ms |
| `drag` | `selector`, `target`, `position?` | HTML5 drag from `selector` onto `target` by synthetic `DragEvent`s (`dragstart`, `dragenter`, `dragover`, `drop`, `dragend`). `position` is `before` (top quarter of the target), `after` (bottom quarter) or the centre. Both elements must exist with non-zero size. Settles 300 ms |
| `expect_value` | `selector`, `value` | The element's `.value` equals `value`. A checkbox or radio reports `"true"`/`"false"` for `checked`. For a radio or checkbox group, the checked member's value, or `""` if none is checked |
| `expect_count` | `selector`, `count` | `document.querySelectorAll(selector).length` equals `count` |
| `expect_storage` | `key`, `contains?` | `localStorage` has `key`, and its value contains `contains` when given |
| `set_storage` | `key`, `value` | `localStorage.setItem(key, value)` |
| `expect_same_text` | `key`, `other_actor` | This actor's normalised text equals the text `other_actor` stored under `key` with `snapshot_text`. Fails, naming the first differing line, if the text differs. Fails if that actor has no such snapshot |
| `expect_attribute` | `selector`, `name`, `value?` | The element has attribute `name`. If `value` is given, the attribute equals it |
| `switch_to_popup` | `timeout_ms` (3000) | Waits for a new window opened by this actor's pages, then makes it the active tab, so later steps act on it. Fails with "no new window was opened" if none appears in time. Settles 300 ms |
| `switch_tab` | `index` | Makes tab `index` active for this actor: 0 is the original tab, 1.. are new windows in opening order. Settles 300 ms |

`StepOp::is_assertion()` classifies every op, and the match is exhaustive, so a
new op must be classified. `ReplayManifest::actions_only()` drops the assertions
and turns `wait_for_text` into a plain wait of at most 1.5 s. The `orchyn-check`
baseline modes and `orchyn-bench --mode mined` use it.

Runner behaviour (`replay/runner.rs`):

- Each distinct actor gets its own browser (`WebAdapter`), created on first use
  by an `AdapterFactoryFn`.
- The run halts at the first failing step. `ReplayReport` holds the executed
  steps (`StepResult` with actor, op, passed, detail, duration), `first_failure`,
  all observed requests, and all console errors formatted as `actor: message`.
- Actors run sequentially in manifest order; the runner does not interleave them.
- `ReplayReport.observed_exchanges` holds the XHR and fetch exchanges with bodies,
  all actors, in order. The runner drains them whenever it collects telemetry and
  at the end of the run. The `mined` bench mode reads them.
- `ReplayReport.page_snapshots` holds a `PageSnapshot` (`replay/snapshot.rs`) per
  actor after each step that passed and can change the page: `goto`, `click`,
  `click_text`, `fill`, `wait_ms`, `wait_for_text`, `reload`, `click_each`,
  `drag`, `hover`, `set_storage`, `switch_to_popup` and `switch_tab`. Assertions
  never record one. A snapshot holds the URL without query or fragment, the
  visible text lines, and the visible elements (`tag#id`, or `tag.class...` when
  there is no id). A snapshot that cannot be read is skipped without failing the
  step.

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
- Mounts: `app|router|server|api.use('/prefix', x)`, where `x` is an identifier
  bound by `require` or `import`, or an inline `require('./x')`. Each route in
  the mounted file is emitted once per prefix. Mounts chain up to five levels. A
  file reachable only through a cycle, or through a deeper chain, keeps its
  literal paths. Only relative specifiers (`./`, `../`) are followed. A specifier
  resolves as written, then with `.js`, `.mjs`, `.cjs` or `.ts`, then as
  `index.js` or `index.ts`. A `.js` specifier also tries `.ts`.
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

Reconciliation is used by `orchyn-check` and `orchyn-bench`. The agent does not
call it.

---

## Invariants

`crates/orchyn-harness/src/invariants/` collects candidate invariants, meaning
things that should always hold, from several sources. Every candidate has the
same shape (`InvariantCandidate`, in `invariants/mod.rs`):

- `id`: stable, for example `universal:console-clean:/`,
  `contract:input:email:required` or `mined:status_always:POST /api/todos`.
- `source`: `Universal`, `CodeContract`, `Transcript`, `Mined`, `Production` or
  `Spec`. Nothing produces `Spec` yet.
- `statement`: one plain-English sentence.
- `evidence`: file, line, and a quote or note.
- `confidence`: 0.0 to 1.0, the extractor's estimate that the behaviour is
  intended.
- `check`: replay steps that test the statement, when it can be checked
  mechanically. Only universal candidates set it today. The other sources yield
  statements that a person or an explorer can turn into manifests.

Nothing in this module calls a model.

### Universal rules (`universal.rs`)

Spec-free rules that apply to every page. `page_paths` returns `/` plus each GET
or ANY route that serves a page. It leaves out `/api` and `/api/...`, routes with
`:param`, `*` or `{`, and paths whose last segment has an extension other than
`.html`. Each page gets six candidates, and each is a replay check:

| kind | confidence | check |
|---|---|---|
| `console-clean` | 0.9 | Load the page. No console errors or uncaught exceptions |
| `no-failed-requests` | 0.9 | Load the page. No failed requests |
| `all-clickable` | 0.9 | Every visible, enabled control is the hit target at its centre |
| `reload-stable` | 0.6 | Reloading the page leaves its visible text unchanged |
| `interactions-clean` | 0.7 | Clicking each button causes no console error and no failed request |
| `actors-agree` | 0.5 | Actor `a` clicks each button. Actor `b` loads the page, then reloads it. `b`'s text must not change across the reload |

`actors-agree` does not compare the two actors' text. Its statement ("what one
user changes stays visible to another") says more than its check does.

`manifests()` gives one manifest per candidate, so a failure does not hide the
candidates after it. The runner halts at the first failure within a manifest.
Each candidate's evidence cites the route that serves its page.

### Code contracts (`contracts.rs`)

Regex and line heuristics over the source tree, with the same skip rules as
intent scanning. Ids are grouped by prefix:

| id | found by | confidence |
|---|---|---|
| `contract:input:<name>:<constraint>` | `required`, `type="email"`, `minlength`, `maxlength` or `pattern` on an `<input>` in html, htm, jsx, tsx, vue or svelte | 0.8 |
| `contract:validate:<METHOD path>:<field>` | `if (!req.body.x …)` with `res.status(4xx)` or `sendStatus(4xx)` within the next three lines | 0.7 |
| `contract:status:<METHOD path>` | `res.status(2xx)` or `sendStatus(2xx)` in a route handler. Codes are merged per route | 0.6 |
| `contract:auth:<METHOD path>` | Route middleware named like `auth`, `login`, `session`, `requireUser` or `protect` (0.8). An `if` that tests `req.session` or `req.cookies.<name>` inside a handler (0.6) | 0.8 or 0.6 |
| `contract:schema:<field>:<zod type>` | Fields of a zod `z.object({...})`. The statement notes `.email()` and `.min(n)` | 0.7 |
| `contract:sql:<table>.<column>:not_null` / `:unique` | `CREATE TABLE` in JavaScript or TypeScript source, or in `.sql` files | 0.9 |
| `contract:ts:<Interface>.<member>` | Members of a TypeScript interface that are not optional | 0.4 |

When two sites give the same id, their evidence is merged and the highest
confidence is kept.

### Transcripts and production logs (`sources.rs`)

- `from_transcript(path)` reads a vibecoding chat, as Markdown or as a JSON array
  of `{role, content}` turns. **Only user turns count.** A sentence becomes a
  candidate (`transcript:<turn>:<sentence>`) when it has a requirement marker
  such as `must`, `should`, `always`, `never`, `make sure`, `only`, `every`,
  `don't`, `so that`, or a `when … then`. Confidence is 0.7 with a strong marker
  (`must`, `never`, `always`, `ensure*`), otherwise 0.5.
- `from_production_log(path)` reads NDJSON events that carry `method`, a
  `route`, `url` or `path`, and a `status`. Optionally they carry an `error` or
  `message`. A route (numeric segments become `:n`) becomes
  `production:status:<METHOD path>` at confidence 0.8, if it has at least 20
  events with a status and at least 99% of them are below 400. An error message
  seen at least three times becomes `production:error:<message>` at 0.6.

Each bench fixture has a `PROMPTS.md`, which `orchyn-bench --mode invariants`
reads as a transcript.

### Mined from known-good traces (`mined.rs`)

Daikon-style mining. `mine(traces)` takes the API exchanges of several known-good
runs, one trace per run. Endpoints are keyed by method and path, with numeric and
UUID segments as `:n`. A fact is kept only when its endpoint appears in at least
two traces and the fact held for every observation:

- `StatusAlways`: every exchange returned the same status (at least two).
- `ShapeAlways`: every JSON response has the same type at each JSON pointer, up to
  three segments deep.
- `NonNull`: the value at a pointer is never null.
- `NonEmptyArray`: the array at a pointer is never empty.
- `NumericStep`: within each trace, consecutive responses differ at a pointer by
  the same non-zero delta, with at least two pairs.

`check(invariants, trace)` returns a `Violation` for each broken observation. It
also reports "endpoint never called" once for each mined endpoint that is absent
from the trace. `to_candidates` turns the mined facts into candidates at
confidence 0.8, with no `check`.

### Page invariants from replay snapshots (`pages.rs`)

Mined from the `PageSnapshot`s of known-good runs of one driver manifest (see
[Replay manifests](#replay-manifests)). `mine(traces)` needs at least two runs
and keeps a key `(actor, step)` only when every run has it. It keeps:

- `TextAlways`: a line the actor saw after that step in every run. Lines over
  200 characters are skipped.
- `VisibleAlways`: an element that was visible in every run.
- `UrlAlways`: the URL path, when every run shares it.

Text and elements are reduced to first appearances. One already shown on the same
page at the actor's previous common step is dropped, unless the step was a
`reload` or `goto`, which restate the page.

`check(invariants, trace)` reports each broken observation, and one "step not
reached" for each `(actor, step)` the run lacks. `to_candidates` gives `Mined`
candidates at confidence 0.75, with no `check`. These cover what API mining does
not, such as a greeting that stops showing or a post one actor never sees.

### Spec ambiguity lint (`ambiguity.rs`)

Not an invariant source. `lint(text)` is a regex-based, model-free linter for task
specs. It returns an `Ambiguity` for each phrase a coding agent could read in more
than one way: its line, an excerpt, one question to settle it, and two to four
readings. It has ten rule kinds: `value_range`, `removal`, `visibility_scope`,
`vague_quality`, `unspecified_value`, `persistence`, `error_feedback`,
`status_code`, `position` and `naming_variant`. A line that already settles the
question (a unit, a storage place, a status code, a visibility qualifier or a
default) is not flagged. It is used by `orchyn-check --lint-spec`.

### Where the sources are used

- `orchyn-check --universal` runs the universal candidates. `--contracts` prints
  the contract candidates. `--record` and `--baseline` mine and check API and page
  invariants for one manifest's actions. `--lint-spec` runs the ambiguity lint.
- `orchyn-bench --mode universal` runs the universal candidates against the clean
  and buggy fixtures. `--mode mined` mines and checks traces and reports API, page
  and combined detection. `--mode invariants` writes every source per fixture,
  reading `PROMPTS.md` and `production.ndjson` where they exist. No fixture has a
  `production.ndjson` yet, so the production source has no benchmark coverage.

---

## Benchmark

`orchyn-bench` scores replay manifests against bug-seeded fixtures.

```bash
npm install --prefix bench          # or: cd bench && npm install
cargo run -p orchyn-bench -- --runs 3 [--suite bench/suite.json] [--manifests DIR] [--label L] [--out FILE] [--only NAME] [--mode manifest|universal|mined|invariants]
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

The suite has eight fixtures under neutral names: login, counter, todos, shop,
profile, newsletter, notes and signup. `bench/results/NAMES.md` maps the older
names (for example `create-500` is now `todos`) for result files written before
the rename. The reference manifests are in `bench/manifests/`. Results are written
to `bench/results/` when `--out` is given.

`--mode` selects what is scored (`manifest` is the default and the one described
above):

- `universal`: no manifest and no spec. Each universal candidate is replayed
  `--runs` times on the clean and buggy builds. A candidate is *valid* if it
  passes every clean run, and it *fires* on a fixture if it also fails every
  buggy run. Results go to `--out` as JSON.
- `mined`: needs `--manifests DIR`, holding a driver manifest per fixture. The
  driver is replayed with its assertions, snapshots and text-expectations
  removed, so the app is driven through the same actions without verdicts. Its
  `max(--runs, 2)` clean traces are mined, and one further clean trace is held
  out. The held-out trace is checked for false positives, and each of the
  `--runs` buggy traces is checked against the mined invariants. API and page
  invariants (see [Invariants](#invariants)) are reported separately and combined.
- `invariants`: writes every candidate from every source for each fixture. `--out`
  is a directory here (default `bench/results/invariants/`), and each fixture gets
  `<fixture>.json`.

`bench/summarize.py` combines the manifest-mode JSON files in `bench/results/`
into one markdown table of valid catches, false positives and flaky fixtures.

`bench/webbench/` is a separate experiment: Web-Bench projects built by Claude
Haiku 5.5, with and without `orchyn-check`. It found no measurable gain. With
Orchyn the agent passed 71 of 90 task tests, and without it 75 of 90, a gap within
noise for two runs per arm, at 3 to 5 times the agent time. The write-up
attributes this to checks an agent writes for itself, which share its reading of
the spec. It argues that the value must come from invariants with an independent
source, such as the transcript, code contracts, or mined or production traffic.
See [RESULTS.md](bench/webbench/RESULTS.md).

`bench/mutation/` generates single-edit mutants of Web-Bench reference projects
(`mutate.js`; its README lists the operators). `score.sh` scores three detectors
on each mutant: independent checks, invariants recorded on the unmutated control,
and universal rules. A mutant is relevant only when the official Web-Bench tests
fail on it. `bench/webbench/indep-checks/` holds checks written from the task
text, with validation against the reference solutions. `bench/webbench/lint/`
holds `--lint-spec` output for the task specs.

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
REPLAY (orchyn-check, orchyn-bench; per commit in CI once wired)
    - one browser per actor, no model
    - halts at the first failing step
    - ReplayReport: steps, observed requests and API exchanges, console errors
    |
    v
RECONCILIATION (intent/)
    - static client calls and server routes vs observed requests
    - ObservedFailure / UnexpectedRequest / UnexercisedClientCall / ClientCallWithoutRoute

INVARIANTS (invariants/, no model)
    - universal page rules (orchyn-check --universal) and code contracts (--contracts)
    - transcript, production-log and mined candidates (orchyn-bench --mode invariants)

AGENT MODE (orchyn-cli --app URL) is separate and exploratory; see the
caveat under orchyn-harness. It does not feed replay yet.
```
