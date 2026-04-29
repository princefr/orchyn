use futures::Stream;
use futures::future::BoxFuture;
use orchyn_core::Result as OrchynResult;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

pub trait ModelClient: Send + Sync + Debug {
    fn complete(&self, req: CompletionRequest) -> BoxFuture<'_, OrchynResult<CompletionStream>>;
    fn tool_call(&self, req: ToolCallRequest) -> BoxFuture<'_, OrchynResult<ToolCallResult>>;
    fn observe(&self, image: &[u8], prompt: &str)
    -> BoxFuture<'_, OrchynResult<serde_json::Value>>;
    fn model_id(&self) -> &str;
}

pub type CompletionStream = Box<dyn Stream<Item = OrchynResult<String>> + Unpin + Send>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub prompt: String,
    pub tools: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

pub type Result<T> = OrchynResult<T>;
