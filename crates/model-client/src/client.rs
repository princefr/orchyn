use crate::config::ModelConfig;
use crate::inference::LlamaEngine;
use crate::traits::{
    CompletionRequest, CompletionStream, ModelClient, Result, ToolCallRequest, ToolCallResult,
};
use futures::{
    StreamExt,
    future::{BoxFuture, FutureExt},
};
use std::sync::Arc;
use tokio::sync::OnceCell;

#[derive(Debug)]
pub struct QwenClient {
    engine: OnceCell<Arc<LlamaEngine>>,
    config: ModelConfig,
}

impl QwenClient {
    pub fn new(config: Option<ModelConfig>) -> Arc<Self> {
        Arc::new(Self {
            engine: OnceCell::new(),
            config: config.unwrap_or_default(),
        })
    }

    fn get_engine(
        &self,
    ) -> impl std::future::Future<Output = Result<Arc<LlamaEngine>>> + Send + '_ {
        async move {
            self.engine
                .get_or_try_init(|| async {
                    let engine = LlamaEngine::load()
                        .await
                        .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                    Ok(Arc::new(engine))
                })
                .await
                .cloned()
        }
    }
}

impl ModelClient for QwenClient {
    fn complete(&self, req: CompletionRequest) -> BoxFuture<'_, Result<CompletionStream>> {
        let engine_future = self.get_engine();
        async move {
            let engine = engine_future.await?;
            engine.generate_stream(req)
        }
        .boxed()
    }

    fn tool_call(&self, req: ToolCallRequest) -> BoxFuture<'_, Result<ToolCallResult>> {
        let engine_future = self.get_engine();
        async move {
            let engine = engine_future.await?;
            let prompt = format!(
                "SYSTEM: You are a tool-calling assistant. Available tools: {:?}\nUSER: {}\n\
                Return ONLY a JSON object: {{\"name\":\"<tool>\",\"arguments\":{{...}}}}.",
                req.tools, req.prompt
            );
            let completion_req = CompletionRequest {
                prompt,
                max_tokens: Some(512),
                temperature: Some(0.0),
            };

            let mut stream = engine.generate_stream(completion_req)?;
            let mut response = String::new();
            while let Some(chunk) = stream.next().await {
                response.push_str(&chunk?);
            }

            let json_val = orchyn_core::utils::extract_json(&response)
                .map_err(|e| orchyn_core::OrchynError::Model(format!("Failed to parse tool call JSON: {}", e)))?;

            let tool_name = json_val.get("name").and_then(|v| v.as_str())
                .ok_or_else(|| orchyn_core::OrchynError::Model("Tool call missing 'name'".to_string()))?
                .to_string();
            let arguments = json_val.get("arguments").cloned()
                .unwrap_or(serde_json::Value::Object(Default::default()));

            Ok(ToolCallResult {
                tool_name,
                arguments,
            })
        }
        .boxed()
    }

    fn observe(&self, image: &[u8], prompt: &str) -> BoxFuture<'_, Result<serde_json::Value>> {
        let engine_future = self.get_engine();
        let image = image.to_vec();
        let prompt = prompt.to_string();
        async move {
            let engine = engine_future.await?;
            engine.observe(&image, &prompt).await
        }
        .boxed()
    }

    fn model_id(&self) -> &str {
        "qwen-3.5-0.8b"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::CompletionRequest;
    use futures::StreamExt;

    #[tokio::test]
    #[ignore] // Ignore by default as it downloads 600MB model
    async fn test_qwen_client_completion() {
        let client = QwenClient::new(None);
        let req = CompletionRequest {
            prompt: "<|im_start|>user\nWhat is 2+2?<|im_end|>\n<|im_start|>assistant\n".to_string(),
            max_tokens: Some(128),
            temperature: Some(0.0),
        };

        let mut stream = client.complete(req).await.unwrap();
        let mut response = String::new();
        while let Some(chunk) = stream.next().await {
            response.push_str(&chunk.unwrap());
        }

        println!("Model response: {}", response);
        assert!(response.contains("4"));
    }

    #[tokio::test]
    #[ignore] // downloads the model
    async fn test_qwen_client_vision_is_honestly_unavailable() {
        // The text-only GGUF cannot see pixels; observe() must say so rather
        // than fabricate a description.
        let client = QwenClient::new(None);
        let image_bytes = std::fs::read("../../tests/fixtures/dog.jpg").expect("fixture");
        let err = client.observe(&image_bytes, "Describe this image.").await;
        assert!(err.is_err());
    }
}
