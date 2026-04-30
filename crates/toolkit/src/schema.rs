use serde::{Deserialize, Serialize};

/// JSON-serialisable description of a tool, suitable for sending to the
/// model as part of its system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Lightweight `(name, description)` pair used by the in-process
/// `tool_search` tool. We deliberately keep the parameter schema out so the
/// snapshot stays cheap to clone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
}
