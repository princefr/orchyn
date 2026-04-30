use serde::{Deserialize, Serialize};

/// JSON-serialisable description of a tool, suitable for sending to the
/// model as part of its system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
