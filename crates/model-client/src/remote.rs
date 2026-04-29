use crate::traits::{CompletionRequest, CompletionStream, ModelClient, Result, ToolCallRequest, ToolCallResult};
use futures::{StreamExt, future::BoxFuture, FutureExt};
use orchyn_core::OrchynError;
use std::sync::Arc;

pub struct RemoteModelClient {
    base_url: String,
    client: reqwest::Client,
}

impl RemoteModelClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }
}

impl ModelClient for RemoteModelClient {
    fn complete(&self, req: CompletionRequest) -> BoxFuture<'_, Result<CompletionStream>> {
        let url = format!("{}/api/v1/complete", self.base_url);
        let client = self.client.clone();
        
        async move {
            let res = client.post(url)
                .json(&req)
                .send()
                .await
                .map_err(|e| OrchynError::Model(e.to_string()))?;
            
            let stream = res.bytes_stream().map(|item| {
                item.map(|b| String::from_utf8_lossy(&b).to_string())
                    .map_err(|e| OrchynError::Model(e.to_string()))
            });
            
            let boxed_stream: CompletionStream = Box::new(tokio_stream::wrappers::ReceiverStream::new(
                // In a real impl, we'd pipe the reqwest stream to an mpsc
                todo!("Implement stream piping")
            ));
            Ok(boxed_stream)
        }.boxed()
    }

    fn tool_call(&self, _req: ToolCallRequest) -> BoxFuture<'_, Result<ToolCallResult>> {
        todo!("Remote tool calls via API")
    }

    fn observe(&self, image_bytes: &[u8], prompt: &str) -> BoxFuture<'_, Result<serde_json::Value>> {
        let url = format!("{}/api/v1/observe", self.base_url);
        let client = self.client.clone();
        let prompt = prompt.to_string();
        let image = image_bytes.to_vec();

        async move {
            let res = client.post(url)
                .multipart(reqwest::multipart::Form::new()
                    .text("prompt", prompt)
                    .part("image", reqwest::multipart::Part::bytes(image).file_name("screenshot.png")))
                .send()
                .await
                .map_err(|e| OrchynError::Model(e.to_string()))?;
            
            res.json().await.map_err(|e| OrchynError::Model(e.to_string()))
        }.boxed()
    }

    fn model_id(&self) -> &str {
        "remote-vps-qwen"
    }
}
