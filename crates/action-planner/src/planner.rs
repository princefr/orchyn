use orchyn_action_executor::Action;
use orchyn_core::{OrchynError, Result};
use orchyn_model_client::{ModelClient, CompletionRequest};
use orchyn_pixel_observer::UiState;
use crate::types::*;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use std::sync::Arc;
use futures::StreamExt;

/// Persona-specific heuristic injected into the planner prompt
fn persona_heuristic(persona: &str) -> &'static str {
    match persona {
        "Rational" => "Read all labels carefully. Take the shortest viable path to the goal. Prefer enabled, well-labeled controls.",
        "Confused" => "Prioritize visually large elements over logically correct ones. Misread similar labels.",
        "Adversarial" => "Probe boundaries: paste 10k chars, use unicode homoglyphs, double-click, submit empty forms, escape strings.",
        "Chaos" => "Trigger orientation changes, background the app, throttle the network, race-click multiple controls.",
        "Accessibility" => "Simulate a screen reader. Check contrast and touch targets. Treat sub-44dp targets as unreachable.",
        _ => "Take the shortest viable path to the goal.",
    }
}

pub struct ActionPlanner<M: ModelClient> {
    model: Arc<M>,
}

impl<M: ModelClient> ActionPlanner<M> {
    pub fn new(model: Arc<M>) -> Self {
        Self { model }
    }
    
    /// Backwards-compatible entry point
    pub async fn plan(&self, state: &UiState, goal: &Goal) -> Result<Action> {
        self.plan_for_persona(state, goal, "Rational").await
    }
    
    pub async fn plan_for_persona(
        &self,
        state: &UiState,
        goal: &Goal,
        persona: &str,
    ) -> Result<Action> {
        info!("ActionPlanner: planning for persona '{}' with goal: {}", persona, goal.description);
        
        let persona_heuristic = persona_heuristic(persona);
        let prompt = format!(
            "You are Orchyn's action planner.\nPersona: {}\n\nGoal: {}\n\nUI State: {:?}\n\nDecide the next action.",
            persona_heuristic, goal.description, state
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
        
        // Parse JSON response
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                let json_str = &response[start..=end];
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    // Parse action from JSON
                    return self.parse_action(&val);
                }
            }
        }
        
        Err(OrchynError::Internal("Failed to parse action from model response".to_string()))
    }
    
    fn parse_action(&self, val: &serde_json::Value) -> Result<Action> {
        // Placeholder - parse action from JSON
        Ok(orchyn_action_executor::Action::Wait { ms: 1000 })
    }
}
