use orchyn_action_executor::Action;
use orchyn_core::{OrchynError, Result};
use orchyn_model_client::{CompletionRequest, ModelClient};
use orchyn_pixel_observer::UiState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    pub criteria: Vec<String>,
}
