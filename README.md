# Orchyn

> **Explore once, replay forever.**
>
> Vibecoded apps look finished long before they behave. Orchyn turns a one-off
> exploration into a deterministic test that runs on every commit, and explains
> failures by comparing what the code intends with what the browser observed.

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange)](https://www.rust-lang.org)

---

## The problem

For fifty years, developers shipped production software with far less than
today's models know. They were embedded in the machine: they watched the browser
console, the network tab and the UI on real devices, and they felt the lag.

Vibecoding agents generate code at lightspeed but never see any of that. They
never watch the WebSocket drop or the button hidden behind a z-index bug. The
0→1 phase is fast. The 1→n phase is a minefield.

## The insight

**0→1 is generative. 1→n is invariant-seeking.**

Building an MVP explores what the app could be. Productionizing shrinks that
space to what must always be true: every API returns what the UI expects, every
visible button can be clicked, every state transition is valid.

Language models are good at generation and poor at invariant-seeking, and small
local models are poorer still. So Orchyn does not ask a model to police the app
on every run. It captures the checks once, as concrete steps, and runs them
without a model.

---

## The idea: explore once, replay forever

1. **Explore once.** An explorer writes a *replay manifest*: a JSON list of
   concrete steps such as `goto`, `click`, `fill`, `expect_text` and
   `expect_request`. The explorer can be a person, a frontier model, or Orchyn's
   agent mode. Today manifests are written by hand.
2. **Replay forever.** On every commit, the manifest runs in headless Chrome, one
   browser per actor. No model runs, so the verdict is deterministic and cheap.
   The run halts at the first failing step.
3. **Explain failures with intent.** Static analysis lists the HTTP calls the
   client makes and the routes the server serves. Reconciliation compares them
   with the traffic the browser actually saw and reports **reality gaps**: a call
   with no matching route, a failed or 4xx/5xx request, an API request the code
   never makes, or a call that never fired.
4. **Use the model where it helps.** The local model is meant for authoring and
   triage, not for clicking through CI. Agent mode is the first step in that
   direction, and it is not reliable yet (see below).

Pixel reality is checked too. A replay `click` fails when another element covers
the point it would click, so "the DOM says it's clickable but a user cannot click
it" becomes a failing step.

---

## What works today

- **Replay** (`crates/orchyn-harness/src/replay/`). Fourteen ops, one browser per
  actor, occlusion-aware clicks, console and network assertions. Driven by
  `orchyn-bench`. There is no replay command in `orchyn-cli` yet.
- **Reality gaps** (`crates/orchyn-harness/src/intent/`). Static extraction for
  `fetch`, `axios`, jQuery `$.get`/`$.post`, `EventSource`, HTML forms and
  Express-style routes. Four gap kinds. Used by `orchyn-bench`.
- **Benchmark** (`bench/`, `orchyn-bench`). Eight bug-seeded fixtures, each with a
  clean and a `BUG=1` variant, scored for detection, false positives and
  flakiness.
- **Agent mode** (`orchyn-cli --app URL`). An exploratory tool-using loop with
  Qwen 3.5 0.8B. See the caveat below.

Not built yet: vision (the local model is text-only and nothing sends pixels to a
model), manifest output from agent mode, real per-ref checkouts for `--diff`,
performance budgets, and time-travel debugging.

### Agent mode: read this before trusting it

- The model is Qwen 3.5 0.8B (Q4_K_M GGUF) run through llama.cpp. It is text-only.
- When the model's reply cannot be parsed, or it returns no tool calls without
  finishing, the harness substitutes a scripted action. The script clicks the
  first button, reads `server.js`, and files the fixed gap "Logic bug found in
  source code — see server.js", whatever the app is. **Gap reports from agent
  mode are not evidence until this fallback is removed.**
- `--diff FROM TO` does not check out the two refs. It runs the agent twice
  against the same URL and compares the gap descriptions, so it is not a
  regression test yet.

---

## Quick start

```bash
cargo build --workspace

# Benchmark: needs Node.js, a Chrome/Chromium binary and npm packages in bench/
(cd bench && npm install)
CHROME=/path/to/chrome ORCHYN_NO_SANDBOX=1 \
  cargo run -p orchyn-bench -- --runs 3 --out results.json

# Agent mode: downloads the GGUF from Hugging Face on first model use
cargo run -p orchyn-cli -- --app http://localhost:3000

# Harness unit tests (the harness integration-test target does not compile yet)
cargo test -p orchyn-harness --lib
```

Set `CHROME` if `headless_chrome` cannot find a browser. Set
`ORCHYN_NO_SANDBOX=1` when running as root or in a container.

---

## A replay manifest

From `crates/orchyn-harness/src/replay/manifest.rs`:

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

| Group | Ops |
|---|---|
| Act | `goto`, `click`, `click_text`, `fill`, `wait_ms`, `wait_for_text` |
| Expect | `expect_text`, `expect_no_text`, `expect_url`, `expect_visible`, `expect_console_clean`, `expect_request`, `expect_no_failed_requests` |
| Telemetry | `reset_telemetry` |

Each step may set `actor` (default `"default"`). Each actor gets its own browser.
`ARCHITECTURE.md` documents every op and the runner's behaviour.

---

## Reality gaps

Reconciliation returns one of four kinds:

- **`ClientCallWithoutRoute`** (static). The code calls an endpoint that no
  server route matches. Checked only when routes were found.
- **`ObservedFailure`** (runtime). A request returned status 0 or 400+.
- **`UnexpectedRequest`** (runtime). An observed `/api/...` request matches no
  client call and no route.
- **`UnexercisedClientCall`** (runtime, informational). A client call never fired
  during the run.

Each gap records the method, the path, the status and, where known, the file and
line of the call.

---

## Architecture

```
bin/orchyn-cli         agent mode: exploration, --diff
bin/orchyn-bench       replay manifests scored against bug-seeded fixtures
bin/orchyn-server      GraphQL API on :3000 (PostgreSQL, Google OAuth, Stripe webhooks)
bin/orchyn-worker      scheduler worker

crates/orchyn-harness  replay/, intent/, agent/, diff engine, legacy phase pipeline
crates/platform-web    WebAdapter over headless_chrome
crates/model-client    QwenClient: Qwen 3.5 0.8B via llama.cpp (single model policy)
crates/toolkit         tool registry for agent mode
crates/core            shared types, errors, PlatformAdapter
```

Full crate graph, data flow and status of each component: [ARCHITECTURE.md](ARCHITECTURE.md).

### Configuration

| Variable | Used by | Purpose |
|---|---|---|
| `CHROME` | `orchyn-cli`, `orchyn-bench` | Path to the Chrome binary |
| `ORCHYN_NO_SANDBOX=1` | same | Disable the Chrome sandbox (root or containers) |
| `DATABASE_URL` | `orchyn-server`, `orchyn-worker` | PostgreSQL connection (required) |
| `ARTIFACT_ROOT` | storage | Artifact directory (default `./storage/artifacts`) |
| `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `JWT_SECRET`, `FRONTEND_URL` | `orchyn-server` | Sign-in and sessions |
| `STRIPE_WEBHOOK_SECRET` | `orchyn-server` | Webhook signature check |

The CLI and the benchmark do not need PostgreSQL.

---

## Benchmark

Eight tiny apps in `bench/fixtures/`, each with one planted bug that is only
active with `BUG=1` (logic, state, HTTP 500, client/server route mismatch,
console crash, a button covered by an invisible overlay, multi-user sync, an
error message that is set but hidden). A manifest scores a **valid catch**
only if it fails on every buggy run *and* passes on every clean run.

Conditions:

- **reference**: manifests hand-written next to the fixtures (knowing the bug).
- **haiku-spec-N**: a small model (Claude Haiku) wrote the manifests from
  `FEATURE.md` alone — it never saw the code, the bug, or the running app.
- **haiku-explore-N**: same, but it could also probe the *clean* running app
  over HTTP before writing them (it never saw the buggy version).

| fixture | class | reference | haiku-spec-1 | haiku-spec-2 | haiku-explore-1 | haiku-explore-2 |
|---|---|---|---|---|---|---|
| login-session | logic | ✓ | ✓ | ✓ | ✓ | ✓ |
| counter | logic/state | ✓ | ✓ | ✓ | ✓ | ✓ |
| create-500 | network | ✓ | ✓ | ✓ | ✓ | ✓ |
| route-mismatch | network/static | ✓ | ✓ | ✓ | ✓ | ✓ |
| console-crash | console | ✓ | ✗ clean fails | ✗ clean fails | ✓ | ✓ |
| covered-button | visual/pixel | ✓ | ✓ | ✓ | ✓ | ✓ |
| realtime-sync | multi_actor | ✓ | ✓ | ✓ | ✓ | ✓ |
| hidden-error | visual | ✓ | ✓ | ✓ | ✓ | ✓ |
| **valid catches** | | **8/8** | **7/8** | **7/8** | **8/8** | **8/8** |
| false positives | | 0 | 1 | 1 | 0 | 0 |
| flaky fixtures | | 0 | 0 | 0 | 0 | 0 |
| runs per variant | | 3 | 3 | 3 | 3 | 3 |
| mean run (ms) | | 7995 | 10633 | 10330 | 10846 | 10174 |

What this shows, and what it doesn't:

- Deterministic replay is stable: 0 flaky verdicts across 240 browser runs.
- A small model that never saw the bug wrote tests that caught all 8 once it
  could look at the real app. From the spec alone it caught 7: both spec-only
  authors asserted the heading "Tags", but CSS renders it as "TAGS". The spec
  is not what the user sees; observing the running app fixes that.
- The pixel-level check matters: `covered-button` passes any DOM-level test
  (`button.click()` works) and is only caught by a real click at the
  element's centre.
- Static intent vs. observed traffic flags the two network bugs
  (`create-500`, `route-mismatch`) with no manifest assertion at all.
- Limits: 8 toy fixtures, bugs planted by the same team that wrote the
  harness, one bug per app, 3 runs per variant. This is a sanity check of the
  pipeline, not evidence about real-world recall.

Reproduce: `(cd bench && npm install)`, then
`CHROME=/path/to/chrome ORCHYN_NO_SANDBOX=1 cargo run -p orchyn-bench -- --runs 3 [--manifests bench/authored/explore-1 --label haiku-explore-1 --out bench/results/haiku-explore-1.json]`,
then `python3 bench/summarize.py`.

---

## Philosophy

The explorer is slow, creative and expensive. The replay is literal, cheap and
the same every time. Spend the expensive part once, when the app changes, and
run the cheap part on every commit.

---

## Contributing

Read [CLAUDE.md](CLAUDE.md) for build commands and conventions, and
[ARCHITECTURE.md](ARCHITECTURE.md) before changing the crate graph. Use
`cargo test -p orchyn-harness --lib` for harness tests until the integration
target is fixed.

---

## License

Cargo metadata declares MIT. The repository does not yet include a LICENSE file.
