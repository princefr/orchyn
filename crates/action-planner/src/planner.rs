use crate::types::*;
use futures::StreamExt;
use orchyn_action_executor::Action;
use orchyn_core::{OrchynError, Result};
use orchyn_model_client::{CompletionRequest, ModelClient};
use orchyn_pixel_observer::UiState;
use std::sync::Arc;
use tracing::info;

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/action_planner.md");

#[derive(Clone)]
pub struct ActionPlanner<M: ModelClient> {
    model: Arc<M>,
}

impl<M: ModelClient> std::fmt::Debug for ActionPlanner<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionPlanner")
            .field("model", &self.model)
            .finish()
    }
}

impl<M: ModelClient> ActionPlanner<M> {
    pub fn new(model: Arc<M>) -> Self {
        Self { model }
    }

    /// Backwards-compatible entry point
    pub async fn plan(&self, state: &UiState, goal: &Goal) -> Result<Action> {
        self.plan_for_persona(state, goal, "Competent, attentive user.").await
    }

    pub async fn plan_for_persona(
        &self,
        state: &UiState,
        goal: &Goal,
        persona_prompt: &str,
    ) -> Result<Action> {
        info!(
            "ActionPlanner: planning with goal: {}",
            goal.description
        );

        let prompt = format!(
            "{}\n\n{}\n\nGOAL: {}\n\nUI STATE: {:?}\n\nNext Action (JSON):",
            persona_prompt, SYSTEM_PROMPT, goal.description, state
        );

        let req = CompletionRequest {
            prompt,
            max_tokens: Some(512),
            temperature: Some(0.0),
        };

        let mut stream = self.model.complete(req).await?;
        let mut response = String::new();

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => response.push_str(&chunk),
                Err(e) => return Err(e),
            }
        }

        let val = orchyn_core::utils::extract_json(&response)
            .map_err(|e| OrchynError::Internal(format!("Failed to parse action JSON: {}", e)))?;

        self.parse_action(&val)
    }

    fn parse_action(&self, val: &serde_json::Value) -> Result<Action> {
        if let Some(obj) = val.as_object() {
            let action_type = obj.get("action").and_then(|v| v.as_str()).unwrap_or("Wait");
            match action_type {
                "Tap" => {
                    let x = obj.get("x").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let y = obj.get("y").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    Ok(Action::Tap { x, y })
                }
                "Type" => {
                    let text = obj.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    Ok(Action::Type { text })
                }
                "Wait" => {
                    let ms = obj.get("ms").and_then(|v| v.as_u64()).unwrap_or(1000) as u32;
                    Ok(Action::Wait { ms })
                }
                "GoalReached" => Ok(Action::Wait { ms: 0 }),
                "GoalUnreachable" => Err(OrchynError::Internal("Goal unreachable".to_string())),
                _ => {
                    tracing::warn!("Unknown action type: {}. Defaulting to Wait.", action_type);
                    Ok(Action::Wait { ms: 1000 })
                }
            }
        } else {
            Err(OrchynError::Internal("Expected JSON object for action".to_string()))
        }
    }
}
