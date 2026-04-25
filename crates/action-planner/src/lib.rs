use orchyn_action_executor::Action;
use orchyn_core::{OrchynError, Result};
use orchyn_model_client::{CompletionRequest, QwenClient};
use orchyn_pixel_observer::UiState;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Skills embedded at compile time so the planner's behavior tracks the repo's
/// `actor-generation` and `platform-adapters` SKILL.md files.
pub const SKILL_ACTOR_GENERATION: &str =
    include_str!("../../../skills/actor-generation/SKILL.md");
pub const SKILL_PLATFORM_ADAPTERS: &str =
    include_str!("../../../skills/platform-adapters/SKILL.md");

/// Persona-specific heuristic injected into the planner prompt. Mirrors the
/// table in `skills/actor-generation/SKILL.md`. We accept it as a string so
/// `action-planner` doesn't have to depend on `actor-engine` (which would be a
/// dependency cycle — `actor-engine` depends on us).
fn persona_heuristic(persona: &str) -> &'static str {
    match persona {
        "Rational" => "Read all labels carefully. Take the shortest viable path to the goal. Prefer enabled, well-labeled controls.",
        "Confused" => "Prioritize visually large elements over logically correct ones. Ignore small text. Misread similar labels.",
        "Adversarial" => "Probe boundaries: paste 10k chars, use unicode homoglyphs, double-click, submit empty forms, escape strings.",
        "Chaos" => "Trigger orientation changes, background the app, throttle the network, race-click multiple controls.",
        "Accessibility" => "Simulate a screen reader. Check contrast and touch targets. Treat sub-44dp targets as unreachable.",
        _ => "Take the shortest viable path to the goal.",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    pub criteria: Vec<String>,
}

pub struct ActionPlanner {
    model: QwenClient,
}

impl ActionPlanner {
    pub fn new(model: QwenClient) -> Self {
        Self { model }
    }

    /// Backwards-compatible entry point that defaults to a Rational persona.
    pub async fn plan(&self, state: &UiState, goal: &Goal) -> Result<Action> {
        self.plan_for_persona(state, goal, "Rational").await
    }

    /// Plan an action with persona-aware guidance. The persona name should
    /// match `ActorPersona`'s Debug repr (e.g. "Rational", "Confused",
    /// "Adversarial", "Chaos", "Accessibility").
    pub async fn plan_for_persona(
        &self,
        state: &UiState,
        goal: &Goal,
        persona: &str,
    ) -> Result<Action> {
        info!(
            "Planning next action for goal '{}' (persona: {})",
            goal.description, persona
        );

        let state_json = serde_json::to_string(state).unwrap_or_default();
        let goal_json = serde_json::to_string(goal).unwrap_or_default();
        let heuristic = persona_heuristic(persona);

        let prompt = format!(
            "<|im_start|>system\nYou are Orchyn's action planner. Follow the repo's expert skills.\n\n\
            === SKILL: ACTOR GENERATION ===\n{}\n\n\
            === SKILL: PLATFORM ADAPTERS ===\n{}\n\n\
            === PERSONA ({}) ===\n{}\n\n\
            === ACTION SCHEMA ===\n\
            Available Action JSON formats:\n\
            - {{\"Tap\": {{\"x\": 10, \"y\": 20}}}}\n\
            - {{\"Type\": {{\"text\": \"hello\"}}}}\n\
            - {{\"Wait\": {{\"ms\": 1000}}}}\n\n\
            === OUTPUT FORMAT ===\n\
            Return ONLY a single JSON object describing exactly ONE action. \
            Tap coordinates are in DP. No prose, no markdown, no commentary.<|im_end|>\n\
            <|im_start|>user\nGoal: {}\n\nCurrent UI state: {}\n\nDecide the next action.<|im_end|>\n<|im_start|>assistant\n",
            SKILL_ACTOR_GENERATION, SKILL_PLATFORM_ADAPTERS, persona, heuristic,
            goal_json, state_json
        );

        let req = CompletionRequest {
            prompt,
            max_tokens: Some(256),
            temperature: Some(0.0),
        };

        let stream = self.model.complete(req).await?;

        use futures::StreamExt;
        let mut full_response = String::new();
        let mut stream = stream;
        while let Some(chunk) = stream.next().await {
            full_response.push_str(&chunk?);
        }

        debug!("Planner raw response: {}", full_response);

        let text = full_response.trim();
        let json_str = if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                &text[start..=end]
            } else {
                text
            }
        } else {
            text
        };

        let action: Action = serde_json::from_str(json_str).map_err(|e| {
            OrchynError::Model(format!(
                "Failed to parse planned action: {}. Response was: {}",
                e, full_response
            ))
        })?;

        info!("Planned action: {:?}", action);
        Ok(action)
    }
}
