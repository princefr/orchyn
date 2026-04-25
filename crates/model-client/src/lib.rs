pub mod loader;
pub mod inference;

use orchyn_core::{Result, OrchynError};
pub use crate::loader::{LocalModelPaths, ModelConfig, ModelManager};
use crate::inference::UnifiedEngine;
use serde::{Deserialize, Serialize};
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use once_cell::sync::Lazy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub prompt: String,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub name: String,
    pub arguments: serde_json::Value,
}

pub type CompletionStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

// Global singleton for unified engine
static UNIFIED_ENGINE: Lazy<Mutex<Option<Arc<UnifiedEngine>>>> = Lazy::new(|| Mutex::new(None));

#[derive(Clone)]
pub struct QwenClient {
    config: ModelConfig,
}

impl QwenClient {
    pub fn new(config: Option<ModelConfig>) -> Self {
        Self {
            config: config.unwrap_or_default(),
        }
    }

    async fn get_engine(&self) -> Result<Arc<UnifiedEngine>> {
        let mut lock = UNIFIED_ENGINE.lock().await;
        if let Some(engine) = &*lock {
            return Ok(engine.clone());
        }

        info!("Initializing Unified GPU Engine (Qwen 3.5 9B)...");
        let manager = ModelManager::new(self.config.clone());
        let paths = manager.download_models().await
            .map_err(|e| OrchynError::Model(format!("Download failed: {}", e)))?;

        let engine = Arc::new(UnifiedEngine::load(&paths)
            .map_err(|e| OrchynError::Model(format!("Unified load failed: {}", e)))?);

        *lock = Some(engine.clone());
        Ok(engine)
    }

    pub async fn complete(&self, req: CompletionRequest) -> Result<CompletionStream> {
        let engine = self.get_engine().await?;
        engine.generate_stream(req)
    }

    pub async fn tool_call(&self, req: ToolCallRequest) -> Result<ToolCallResult> {
        let tool_desc = serde_json::to_string(&req.tools).unwrap();
        let prompt = format!(
            "<|im_start|>system\nYou are a tool-calling agent. Output ONLY JSON matching a tool name and arguments.<|im_end|>\n\
            <|im_start|>user\nTools: {}\n\nTask: {}\nDecide the next tool to call.<|im_end|>\n<|im_start|>assistant\n",
            tool_desc, req.prompt
        );

        let completion_req = CompletionRequest {
            prompt,
            max_tokens: Some(512),
            temperature: Some(0.0),
        };

        let mut stream = self.complete(completion_req).await?;
        use futures::StreamExt;
        let mut response = String::new();
        while let Some(chunk) = stream.next().await {
            response.push_str(&chunk?);
        }

        let json_val: serde_json::Value = if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                serde_json::from_str(&response[start..=end])
                    .map_err(|e| OrchynError::Model(format!("Invalid tool JSON: {}. Response: {}", e, response)))?
            } else {
                return Err(OrchynError::Model(format!("Malformed tool response: {}", response)));
            }
        } else {
            return Err(OrchynError::Model(format!("Model failed tool JSON: {}", response)));
        };

        Ok(ToolCallResult {
            name: json_val.get("name").and_then(|v| v.as_str()).unwrap_or("finish").to_string(),
            arguments: json_val.get("arguments").cloned().unwrap_or(serde_json::Value::Object(Default::default())),
        })
    }

    pub async fn observe(&self, image: &[u8], prompt: &str) -> Result<serde_json::Value> {
        let engine = self.get_engine().await?;
        engine.observe(image, prompt).await
    }
    
    pub fn model_id(&self) -> &str {
        &self.config.repo
    }
}
