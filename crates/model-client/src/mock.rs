use crate::traits::{ModelClient, CompletionRequest, ToolCallRequest, ToolCallResult, CompletionStream, Result};
use std::sync::{Arc, Mutex};
use futures::{StreamExt, future::{BoxFuture, FutureExt}};
use orchyn_core::OrchynError;

#[derive(Debug, Clone)]
pub struct MockClient {
    responses: Arc<Mutex<Vec<String>>>,
}

impl MockClient {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
        }
    }
}

impl ModelClient for MockClient {
    fn complete(&self, _req: CompletionRequest) -> BoxFuture<'_, Result<CompletionStream>> {
        let response = {
            let mut resps = self.responses.lock().unwrap();
            if resps.is_empty() {
                "Mock response".to_string()
            } else {
                resps.remove(0)
            }
        };

        async move {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            let _ = tx.send(Ok(response)).await;
            
            use tokio_stream::wrappers::ReceiverStream;
            let stream: CompletionStream = Box::new(ReceiverStream::new(rx));
            Ok(stream)
        }.boxed()
    }

    fn tool_call(&self, _req: ToolCallRequest) -> BoxFuture<'_, Result<ToolCallResult>> {
        let response = {
            let mut resps = self.responses.lock().unwrap();
            if resps.is_empty() {
                r#"{"name": "unknown", "arguments": {}}"#.to_string()
            } else {
                resps.remove(0)
            }
        };

        async move {
            let val: serde_json::Value = serde_json::from_str(&response)
                .map_err(|e| OrchynError::Model(e.to_string()))?;
            
            Ok(ToolCallResult {
                tool_name: val["name"].as_str().unwrap_or("unknown").to_string(),
                arguments: val["arguments"].clone(),
            })
        }.boxed()
    }

    fn observe(&self, _image: &[u8], _prompt: &str) -> BoxFuture<'_, Result<serde_json::Value>> {
        async move {
            Ok(serde_json::json!({"status": "observed"}))
        }.boxed()
    }

    fn model_id(&self) -> &str {
        "mock-model"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{CompletionRequest, ToolCallRequest};

    #[tokio::test]
    async fn test_mock_client_complete() {
        let client = MockClient::new(vec!["Hello".to_string(), "World".to_string()]);
        
        let mut stream = client.complete(CompletionRequest {
            prompt: "Hi".to_string(),
            max_tokens: None,
            temperature: None,
        }).await.unwrap();
        
        let chunk = stream.next().await.unwrap().unwrap();
        assert_eq!(chunk, "Hello");
        
        let mut stream2 = client.complete(CompletionRequest {
            prompt: "Hi".to_string(),
            max_tokens: None,
            temperature: None,
        }).await.unwrap();
        
        let chunk2 = stream2.next().await.unwrap().unwrap();
        assert_eq!(chunk2, "World");
    }

    #[tokio::test]
    async fn test_mock_client_tool_call() {
        let response = r#"{"name": "test_tool", "arguments": {"foo": "bar"}}"#.to_string();
        let client = MockClient::new(vec![response]);
        
        let result = client.tool_call(ToolCallRequest {
            prompt: "Call test tool".to_string(),
            tools: vec![],
        }).await.unwrap();
        
        assert_eq!(result.tool_name, "test_tool");
        assert_eq!(result.arguments["foo"], "bar");
    }
}
