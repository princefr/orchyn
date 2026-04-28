use futures::Stream;
use serde::{Deserialize, Serialize};
use orchyn_core::{OrchynError, Result as OrchynResult};
use std::future::Future;

pub trait ModelClient: Send + Sync {
    fn complete(&self, req: CompletionRequest) -> impl Future<Output = OrchynResult<CompletionStream>> + Send;
    fn tool_call(&self, req: ToolCallRequest) -> impl Future<Output = OrchynResult<ToolCallResult>> + Send;
    fn observe(&self, image: &[u8], prompt: &str) -> impl Future<Output = OrchynResult<serde_json::Value>> + Send;
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
