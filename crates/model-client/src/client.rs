use tokio::sync::OnceCell;
use crate::traits::{ModelClient, CompletionRequest, ToolCallRequest, ToolCallResult, CompletionStream, Result};
use crate::config::ModelConfig;
use crate::inference::MistralEngine;
use std::sync::Arc;
use futures::StreamExt;

pub struct QwenClient {
    engine: OnceCell<Arc<MistralEngine>>,
    config: ModelConfig,
}

impl QwenClient {
    pub fn new(config: Option<ModelConfig>) -> Arc<Self> {
        Arc::new(Self {
            engine: OnceCell::new(),
            config: config.unwrap_or_default(),
        })
    }

    fn get_engine(&self) -> impl Future<Output = Result<Arc<MistralEngine>>> + Send + '_ {
        async move {
            self.engine.get_or_try_init(|| async {
                let engine = MistralEngine::load().await
                    .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                Ok(Arc::new(engine))
            }).await.cloned()
        }
    }
}

impl ModelClient for QwenClient {
    fn complete(&self, req: CompletionRequest) -> impl Future<Output = Result<CompletionStream>> + Send {
        let engine_future = self.get_engine();
        async move {
            let engine = engine_future.await?;
            engine.generate_stream(req)
        }
    }

    fn tool_call(&self, req: ToolCallRequest) -> impl Future<Output = Result<ToolCallResult>> + Send {
        async move {
            // Simple implementation: use complete and parse JSON
            let prompt = format!(
                "Available tools: {:?}\nUser request: {}\nReturn JSON only.",
                req.tools, req.prompt
            );
            let completion_req = CompletionRequest {
                prompt,
                max_tokens: Some(512),
                temperature: Some(0.0),
            };
            
            let mut stream = self.complete(completion_req).await?;
            let mut response = String::new();
            while let Some(chunk) = stream.next().await {
                response.push_str(&chunk?);
            }
            
            // Dummy parsing for now, should use the actual logic from fixer-agent if needed
            Ok(ToolCallResult {
                tool_name: "unknown".to_string(),
                arguments: serde_json::Value::Null,
            })
        }
    }

    fn observe(&self, image: &[u8], prompt: &str) -> impl Future<Output = Result<serde_json::Value>> + Send {
        let engine_future = self.get_engine();
        let image = image.to_vec();
        let prompt = prompt.to_string();
        async move {
            let engine = engine_future.await?;
            engine.observe(&image, &prompt).await
        }
    }

    fn model_id(&self) -> &str {
        "qwen-3.5-0.8b"
    }
}
