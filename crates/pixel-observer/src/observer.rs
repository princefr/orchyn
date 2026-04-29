use crate::UiState;
use orchyn_core::{OrchynError, Result};
use orchyn_model_client::{CompletionRequest, ModelClient};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/vlm_observer.md");

#[derive(Clone)]
pub struct PixelObserver<M: ModelClient> {
    model: Arc<M>,
}

impl<M: ModelClient> std::fmt::Debug for PixelObserver<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PixelObserver")
            .field("model", &self.model)
            .finish()
    }
}

impl<M: ModelClient> PixelObserver<M> {
    pub fn new(model: Arc<M>) -> Self {
        Self { model }
    }

    pub async fn observe(&self, screenshot: &[u8], _state: &UiState) -> Result<UiState> {
        info!("PixelObserver: analyzing screenshot ({} bytes)...", screenshot.len());

        let val = self.model.observe(screenshot, SYSTEM_PROMPT).await?;
        
        // Parse the structured state from the VLM response
        let state: UiState = serde_json::from_value(val)
            .map_err(|e| OrchynError::Internal(format!("Failed to parse UI state from VLM: {}", e)))?;

        info!("PixelObserver: detected {} elements and {} anomalies.", 
            state.elements.len(), 
            state.anomalies.len()
        );

        Ok(state)
    }
}
