# Orchyn

> **Constraint-Powered Testing for Small LLMs**
>
> *What LLMs lack is not intelligence — it's constraints.*

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## The Problem

For 50 years, human developers built production software with far less knowledge than today's LLMs possess. We didn't have 4 trillion parameters of training data. We couldn't hold entire codebases in working memory. We reasoned slowly, forgot details, and made mistakes constantly.

Yet we shipped reliable software. Why?

**Because we were constrained.**

We stared at the browser console until our eyes hurt. We watched the network tab for failed requests. We felt UI lag on real devices. We sat in rooms with other humans who broke our app in ways we never imagined. We ran linters that yelled at us. We searched Stack Overflow when stuck. We tested on different browsers, different screen sizes, different network conditions. We knew the system RAM was running low because the fan spun up.

**We were embedded in the machine.**

Today's vibecoding agents — Cursor, Bolt, Claude Code — are disembodied brains. They see your codebase, maybe a screenshot, and generate code at lightspeed. They never feel the console error. Never watch the WebSocket drop. Never see the button hidden behind a z-index bug on mobile Safari. They are coding blindfolded.

The result? Apps that look finished but break in production. The 0→1 phase is magical. The 1→n phase is a minefield.

---

## The Insight

**0→1 is generative. 1→n is invariant-seeking.**

Building an MVP is about exploring possibility space: *"What could this app be?"* This is what frontier LLMs excel at.

Productionizing is the opposite. It's about *shrinking* possibility space: *"What must always be true?"* Every pixel in the right place. Every API returning 200. Every state transition valid. Every race condition eliminated.

LLMs are naturally good at generation and naturally bad at invariant-seeking — **unless you give them the sensory feedback loop to detect violations.**

---

## The Solution

Orchyn is a **constraint architecture** that wraps small LLMs (0.8B–4B parameters) in a cage of reality. The model doesn't need to be smarter. It needs to be **more constrained**.

### What Orchyn Provides

| Constraint | What It Does | Why It Matters |
|-----------|-------------|----------------|
| **Visual Grounding** | Screenshots annotated with numbered element overlays (1–99). LLM references elements by ID, never raw coordinates. | Eliminates coordinate hallucination. Works on any framework: React, Vue, Flutter, Unity. |
| **Telemetry Fusion** | Console errors, network HAR, WebSocket traffic, performance timelines — fused into a single evidence snapshot with pre-computed correlations. | LLM receives *"login failed: button disabled (visual) + POST 500 (network) + TypeError (console)"* instead of parsing raw dumps. |
| **Multi-Actor Orchestration** | Alice and Bob run in separate browsers, coordinated by the harness. Test real-time sync, race conditions, permission collisions. | Catches bugs impossible to find with single-actor testing: *"Bob deletes a card while Alice is editing it."* |
| **Differential Testing** | Compare test results between versions. Report only what changed. | Eliminates noise. Answers: *"Did this commit break anything?"* |
| **Structured Output Enforcement** | Strict JSON schema validation before any browser action. Invalid output = retry or halt. | Prevents destructive actions from malformed LLM responses. |
| **Stuck Detection & Recovery** | Perceptual hash comparison across screenshots. Automatic retry, skip, or halt policies. | Handles flaky UI states without human intervention. |
| **Performance Budgets** | Enforce FCP, TTI, API latency thresholds at CI time. | Catches performance regressions before users complain. |
| **Time-Travel Debugging** | Record every screenshot, telemetry snapshot, and LLM decision. Replay any turn. | Debug flaky tests with exact state reconstruction. |

### The Fusion Engine

Instead of feeding evidence to the LLM one modality at a time, Orchyn's **Fusion Engine** pre-correlates all sources:

```
Screenshot ──┐
DOM tree ────┼──→ Visual Analyzer ──┐
              │                       │
Console ─────┼──→ Telemetry Parser ──┼──→ Correlation Engine ──→ FusedEvidence (JSON)
              │                       │
Network ─────┘                       │
                                     │
WebSocket ───────────────────────────┘
```

The LLM receives a single `FusedEvidence` payload per turn:

```json
{
  "fused_state": {
    "visual_summary": { "element_count": 7, "anomalies": ["button 3 is disabled despite valid inputs"] },
    "console_state": { "error_count": 1, "severity": "Critical", "correlated_element_ids": [3] },
    "network_state": { "failed_requests": [{"method": "POST", "url": "/api/login", "status": 500}] },
    "invariant_violations": [
      { "invariant": "login button enabled when inputs valid", "violated": true, "severity": "Critical" }
    ],
    "correlations": [
      { "description": "Click on login button triggered POST 500, causing console error and leaving button disabled",
        "confidence": 0.97 }
    ]
  }
}
```

The 0.8B model doesn't infer causation. **The harness already did.** The model just decides: `CLICK 5`, `TYPE 3`, or `ASSERT_FAILURE`.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  ORCHYN CLI                                                  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ Agent Mode  │  │ Constraint  │  │ Differential        │ │
│  │ (4B/9B)     │  │ Mode (0.8B) │  │ Testing             │ │
│  │ Explore     │  │ Execute     │  │ Compare versions    │ │
│  │ Generate    │  │ Validate    │  │ Detect regressions  │ │
│  │ manifests   │  │ Assert      │  │                     │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────────┐
│  HARNESS LAYER (Rust)                                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ Turn        │  │ Fusion      │  │ Stuck Detection &   │ │
│  │ Scheduler   │  │ Engine      │  │ Recovery Policy     │ │
│  │ Multi-actor │  │ Sensor      │  │ Retry / Skip / Halt │ │
│  │ coordination│  │ fusion      │  │                     │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ Screenshot  │  │ Telemetry   │  │ Schema Validation   │ │
│  │ Annotator   │  │ Compression │  │ JSON enforcement    │ │
│  │ Red boxes + │  │ Last-3      │  │ before execution    │ │
│  │ numbers     │  │ truncation  │  │                     │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────────┐
│  BROWSER LAYER (Playwright)                                  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ Screenshot  │  │ DOM Scraper │  │ Console / Network / │ │
│  │ capture     │  │ Interactive │  │ WebSocket listeners │ │
│  │             │  │ elements    │  │                     │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Crates

| Crate | Responsibility |
|-------|---------------|
| `orchyn-core` | `Action` enum, `AnnotatedElement`, `FusedEvidence`, `ManifestStep`, validation |
| `orchyn-model-client` | `ModelClient` (constraint) and `ToolUsingModel` (agent) traits |
| `orchyn-harness` | `TestRunner` (constraint), `AgentHarness` (agent), `FusionEngine`, scheduler, recovery |
| `orchyn-pixel-observer` | `annotate_screenshot()` — draws red numbered boxes on PNGs |
| `orchyn-action-executor` | Resolves element IDs to coordinates, executes via `PlatformAdapter` |
| `orchyn-platform-web` | Playwright adapter: screenshot, execute, scrape, telemetry |
| `orchyn-toolkit` | Tool registry, browser tools, code tools, meta tools |
| `orchyn-actor-engine` | `ActorState`, `ActorMemorySummary` — externalized state containers |
| `orchyn-storage` | PostgreSQL persistence for runs, artifacts, reports |
| `orchyn-artifact-storage` | Screenshot/PNG persistence |
| `orchyn-report-generator` | Final test reports, diff reports, gap summaries |

---

## Usage

### Constraint Mode — Fast, Deterministic, Local

Run pre-written test manifests with a 0.8B model. CI/CD ready.

```bash
# Write a manifest (YAML)
cat > tests/my-app.yml << 'EOF'
fixture:
  name: MyApp
  url: http://localhost:3000
  accounts:
    alice: {email: alice@test.com, password: alice123}
    bob:   {email: bob@test.com, password: bob123}

manifest:
  - step: 0
    actor: alice
    instruction: "Navigate to login"
    on_fail: halt
  - step: 1
    actor: alice
    instruction: "Log in as alice@test.com"
    on_fail: halt
  - step: 2
    actor: alice
    instruction: "Create card 'Buy milk' in Todo"
    on_fail: halt
  - step: 3
    actor: bob
    instruction: "Log in as bob@test.com"
    on_fail: halt
  - step: 4
    actor: bob
    instruction: "Assert 'Buy milk' is visible in Todo"
    action: ASSERT_SUCCESS
    assertion: {type: TEXT_CONTAINS, expected: "Buy milk"}
    on_fail: halt
  - step: 5
    actor: bob
    instruction: "Drag 'Buy milk' to In Progress"
    on_fail: halt
  - step: 6
    actor: alice
    instruction: "Assert 'Buy milk' is in In Progress without refresh"
    action: ASSERT_SUCCESS
    assertion: {type: TEXT_CONTAINS, expected: "Buy milk"}
    on_fail: halt
EOF

# Run it
orchyn-cli run --manifest tests/my-app.yml --model qwen-0.8b

# Output:
# [PASS] Step 0: alice navigates to login
# [PASS] Step 1: alice logs in
# [PASS] Step 2: alice creates card
# [PASS] Step 3: bob logs in
# [PASS] Step 4: bob sees card
# [PASS] Step 5: bob drags card
# [PASS] Step 6: alice sees update (real-time sync confirmed)
# ─────────────────────────────
# Result: PASSED (7/7 steps)
# Artifacts: ./orchyn-artifacts/run-2024-...
```

### Agent Mode — Intelligent Exploration

Let a 4B+ model explore your app, discover features, and generate manifests automatically.

```bash
orchyn-cli agent   --app-url http://localhost:3000   --codebase ./src   --model qwen-4b   --output tests/generated.yml

# The agent:
# 1. Navigates to your app, takes screenshots
# 2. Reads your codebase to understand data models
# 3. Discovers user flows (login, CRUD, real-time features)
# 4. Spawns multiple actors to test collaboration
# 5. Reports gaps with severity and reproduction steps
# 6. Generates a structured manifest for constraint mode
```

### Differential Testing — Regression Detection

Compare two versions of your app automatically.

```bash
# Test baseline (main branch)
git checkout main
orchyn-cli run --manifest tests/my-app.yml --model qwen-0.8b --tag baseline

# Test candidate (feature branch)
git checkout feature/new-auth
git checkout main -- tests/my-app.yml  # same manifest
orchyn-cli run --manifest tests/my-app.yml --model qwen-0.8b --tag candidate

# Compare
orchyn-cli diff --baseline baseline --candidate candidate

# Output:
# [REGRESSION] Step 4: bob no longer sees 'Buy milk' in Todo
#   Baseline:  Element 7 visible with text "Buy milk"
#   Candidate: Element 7 missing (layout shift detected)
#   Console:   New error "Cannot read property 'cards' of undefined"
#   Network:   GET /api/cards -> 500 (was 200 in baseline)
```

---

## Telemetry Coverage

Orchyn incorporates the full sensory stack that human developers use:

| Source | What We Capture | How We Constrain |
|--------|----------------|------------------|
| **Visual** | Screenshot → VLM → element bounding boxes + anomalies | Annotated overlays with integer IDs. LLM never outputs raw coordinates. |
| **Console** | Error/warn/log levels, stack traces, source maps | Compressed to last-3 errors. Correlated with visual elements. |
| **Network** | HAR: requests, responses, status codes, timing, headers | Filtered to 4xx/5xx + slow requests. Correlated with clicks. |
| **WebSocket** | Message payloads, connection status, frame timing | Last-3 messages. Detects sync failures in multi-actor tests. |
| **Performance** | FCP, LCP, TTI, CLS, memory usage, JS heap | Budget enforcement. Fail tests on regression. |
| **Accessibility** | ARIA violations, keyboard navigation, screen reader output | Detects invisible interactive elements. |
| **DOM** | Interactive element tree, visibility, enabled state, z-index | Validates clickability before execution. |
| **System** | CPU, RAM, GPU usage of browser process | Detects memory leaks that crash tabs. |

**Is this enough?** We believe so. The fusion engine pre-computes correlations across all modalities, so the LLM receives a single, coherent state description rather than raw dumps. The constraint is in the architecture, not the model size.

---

## Why Local LLMs?

| | Cloud (GPT-4o) | Local (4B) | Local (0.8B) |
|---|---|---|---|
| **Cost per test** | $1.50–3.00 | ~$0.01 | ~$0.001 |
| **Latency per step** | 2–5s | 300–600ms | 100–200ms |
| **Data privacy** | Leaves your machine | Stays local | Stays local |
| **Offline capable** | No | Yes | Yes |
| **Determinism** | Low (temperature, API changes) | High | Very high |
| **Required hardware** | None | M4 Mac / RTX 4080 | M3 / RTX 3060 |

The constraint architecture compensates for smaller model capability. A 0.8B model with full sensor fusion and strict schema validation outperforms a 70B model coding blindfolded.

---

## Philosophy

> **"Vibecoding builds the MVP. Orchyn gives the AI the eyes, the console, and the chaos of real users — so even a small model can brute-force your app to production readiness."**

The frontier model is your creative partner. The local model, wrapped in constraints, is your production engineer. You need both. But only one of them runs on your desk, costs nothing per inference, and gets smarter every time you tighten a constraint.

---

## Contributing

Orchyn is built in Rust because reliability is a property of architecture, not models. We welcome contributions in:

- **Fusion engine rules** — new invariant detectors, correlation heuristics
- **Platform adapters** — Flutter, React Native, Unity, embedded WebViews
- **Telemetry sources** — additional browser APIs, OS-level metrics
- **Persona models** — learned user behavior for fuzzing

See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

MIT — see [LICENSE](LICENSE).

---

## Acknowledgments

Inspired by the pre-LLM developer who shipped reliable software not because they were smarter, but because they were **embedded in the machine**.
