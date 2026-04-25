# Orchyn — Technical Architecture

A Rust-based harness that takes vibecoded apps to production by reconciling
inferred code intent against observed pixel reality.

## Core thesis

The product detects **reality gaps**: the codebase claims feature X exists,
the pixel reality shows feature Y. Every bug in vibecoded apps lives in that
delta. Orchyn makes the delta visible, reproducible, and fixable.

---

## Workspace layout

```
orchyn/
├── Cargo.toml                      # workspace root
├── rust-toolchain.toml             # pin to stable
├── crates/
│   ├── core/                       # shared types, errors, config
│   ├── storage/                    # SQLite + S3 persistence layer
│   ├── model-client/               # Unified Qwen3.5 4B interface (Code + Vision)
│   ├── codebase-indexer/           # AST, symbol graph, file tree
│   ├── navigator-agent/            # tool-using LLM code navigator
│   ├── spec-engine/                # inferred spec model + versioning
│   ├── runtime-tracer/             # network + console instrumentation
│   ├── actor-engine/               # multi-actor goal-driven test runner
│   ├── pixel-observer/             # screenshot → VLM → structured state
│   ├── action-planner/             # LLM plans next gesture from state
│   ├── action-executor/            # platform-specific execution
│   ├── platform-web/               # Playwright adapter
│   ├── platform-android/           # ADB adapter
│   ├── platform-ios/               # Appium/XCUITest adapter
│   ├── platform-flutter/           # Flutter driver adapter
│   ├── gap-detector/               # reconciles intent vs reality
│   ├── pattern-library/            # cross-app bug pattern store
│   ├── report-generator/           # video + narration + patch proposal
│   ├── scheduler/                  # test run orchestration, cost governor
│   └── api-server/                 # HTTP API, webhooks, PR bot
├── bin/
│   ├── orchyn-cli/                 # local CLI for dev usage
│   ├── orchyn-server/              # hosted API server
│   └── orchyn-worker/              # test execution worker
└── tests/
    ├── fixtures/                   # sample vibecoded apps
    └── integration/
```

---

## Crate responsibilities (Updated for Native Async Traits & Single Model)

### `core`

Shared types used across all crates. Using Rust 2024 edition features.

### `model-client`

Unified async interface to the local Qwen3.5 4B model, handling both text/code completion and vision tasks.

```rust
pub trait ModelClient: Send + Sync {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionStream, ModelError>;
    async fn tool_call(&self, req: ToolCallRequest) -> Result<ToolCallResult, ModelError>;
    async fn observe(&self, image: &[u8], prompt: &str) -> Result<UiState, ModelError>;
    
    fn model_id(&self) -> &str { "qwen3.5-4b" }
    fn context_window(&self) -> usize { 32768 }
}
```

### `codebase-indexer`

Static analysis layer using tree-sitter to produce the map for the navigator agent.

### `navigator-agent`

Tool-using agent that reads code strategically. Uses native async traits for tools.

```rust
pub trait NavigatorTool: Send + Sync {
    fn name(&self) -> &str;
    async fn call(&self, args: serde_json::Value) -> Result<ToolResult, ToolError>;
}
```

### `actor-engine`

Multi-actor goal-driven test runner. Each actor is a finite-state agent.

### `pixel-observer`

Wraps `model-client` vision capabilities to return structured `UiState`.

### `action-executor` + platform crates

```rust
pub trait PlatformAdapter: Send + Sync {
    async fn screenshot(&mut self) -> Result<Vec<u8>, PlatformError>;
    async fn execute(&mut self, action: &Action) -> Result<(), PlatformError>;
    async fn reset(&mut self) -> Result<(), PlatformError>;
    fn device_info(&self) -> DeviceInfo;
}
```

---

## Concurrency & Implementation Notes

- **Native Async Traits:** We use Rust's native `async fn` in traits (available since 1.75). No `#[async_trait]` macro is needed.
- **Single Model Policy:** All LLM/VLM tasks are routed to the local Qwen3.5 4B instance. This simplifies the architecture and ensures privacy/latency consistency.
- **2024 Edition:** All crates target the Rust 2024 edition.
