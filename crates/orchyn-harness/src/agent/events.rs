use serde::{Deserialize, Serialize};

/// Events emitted by AgentHarness during a run. Subscribe via
/// `AgentHarness::subscribe()` before calling `run()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentProgressEvent {
    /// A new turn is starting; url is the current browser URL (may be "about:blank").
    TurnStart { turn: usize, url: String },
    /// The LLM is being called (spinner cue).
    LlmThinking { turn: usize },
    /// The LLM responded with a reasoning string and a list of tool names.
    LlmResponse { turn: usize, reasoning: String, tool_names: Vec<String> },
    /// A tool is about to be called.
    ToolCalling { turn: usize, name: String, args: serde_json::Value },
    /// A tool returned.
    ToolResult { turn: usize, name: String, success: bool, preview: String },
    /// A gap was reported by the agent (gap_report tool call succeeded).
    GapFound { turn: usize, key: String, description: String, severity: String },
    /// A test was recorded (memory_store with a result_ key).
    TestRecorded { turn: usize, key: String, passed: bool },
    /// The run finished (done:true or max_turns reached).
    RunComplete { completed: bool, turns_used: usize, gaps_found: usize },
}
