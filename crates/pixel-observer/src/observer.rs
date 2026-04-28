use orchyn_core::{OrchynError, Result};
use orchyn_model_client::{ModelClient, CompletionRequest};
use crate::UiState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

pub struct PixelObserver<M: ModelClient> {
    model: Arc<M>,
}

impl<M: ModelClient> PixelObserver<M> {
    pub fn new(model: Arc<M>) -> Self {
        Self { model }
    }
    
    pub async fn observe(&self, screenshot: &[u8], state: &UiState) -> Result<UiState> {
        let prompt = format!(
            "Observe the UI state. Current state: {:?}. Describe what you see.",
            state
        );
        let req = CompletionRequest {
            prompt,
            max_tokens: Some(512),
            temperature: Some(0.0),
        };
        
        // For now, just return the state as-is (streaming has compilation issues)
        info!("Observe called (streaming WIP)");
        Ok(state.clone())
    }
}
