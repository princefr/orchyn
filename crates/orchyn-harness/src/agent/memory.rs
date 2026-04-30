use orchyn_toolkit::{ToolCall, ToolResult};
use serde::Serialize;

/// Sliding-window of conversation messages and per-turn tool-call records
/// the agent harness keeps in RAM. Distinct from
/// [`orchyn_toolkit::WorkingMemory`], which holds long-lived findings
/// (`gap_*`, `test_*`, `fix_*`) for the report.
pub struct WorkingMemory {
    messages: Vec<Message>,
    turns: Vec<TurnRecord>,
    max_messages: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
    }
}

#[derive(Debug, Clone)]
pub struct TurnRecord {
    pub turn: usize,
    pub reasoning: String,
    pub tool_results: Vec<TurnToolResult>,
}

#[derive(Debug, Clone)]
pub struct TurnToolResult {
    pub call: ToolCall,
    pub result: Result<ToolResult, String>,
}

impl WorkingMemory {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::new(),
            turns: Vec::new(),
            max_messages,
        }
    }

    pub fn push_message(&mut self, msg: Message) {
        if self.messages.len() >= self.max_messages {
            self.messages.remove(0);
        }
        self.messages.push(msg);
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn record_turn(&mut self, record: TurnRecord) {
        self.turns.push(record);
    }

    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }
}
