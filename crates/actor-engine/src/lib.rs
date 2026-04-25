use chrono::Utc;
use orchyn_action_executor::PlatformAdapter;
use orchyn_action_planner::{ActionPlanner, Goal};
use orchyn_core::{ActorId, OrchynError, RealityContext, Result};
use orchyn_pixel_observer::PixelObserver;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

/// Skill embedded at compile time. The `ActionPlanner` also embeds it, but we
/// keep a copy here so callers querying the actor-engine crate can introspect
/// which guidance the loop is enforcing.
pub const SKILL_ACTOR_GENERATION: &str =
    include_str!("../../../skills/actor-generation/SKILL.md");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActorPersona {
    Rational,
    Confused,
    Adversarial,
    Chaos,
    Accessibility,
}

impl ActorPersona {
    /// Stable string name passed to the planner. Must match the keys in
    /// `action-planner::persona_heuristic`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ActorPersona::Rational => "Rational",
            ActorPersona::Confused => "Confused",
            ActorPersona::Adversarial => "Adversarial",
            ActorPersona::Chaos => "Chaos",
            ActorPersona::Accessibility => "Accessibility",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorBudget {
    pub max_steps: u32,
    pub max_tokens: u32,
    pub max_seconds: u32,
}

pub struct Actor {
    pub id: ActorId,
    pub persona: ActorPersona,
    pub goal: Goal,
    pub budget: ActorBudget,
}

pub struct ActorEngine {
    planner: Arc<ActionPlanner>,
    observer: Arc<PixelObserver>,
}

impl ActorEngine {
    pub fn new(planner: ActionPlanner, observer: PixelObserver) -> Self {
        Self {
            planner: Arc::new(planner),
            observer: Arc::new(observer),
        }
    }

    pub async fn run_actor<P: PlatformAdapter>(
        &self,
        actor: &Actor,
        platform: &mut P,
    ) -> Result<Vec<RealityContext>> {
        info!(
            "Starting actor {} with persona {:?}",
            actor.id, actor.persona
        );

        let mut steps = 0;
        let mut traces = Vec::new();

        while steps < actor.budget.max_steps {
            let screenshot = platform.screenshot().await?;
            let dom_elements = platform.get_elements().await.unwrap_or_default();
            let diagnostics = platform.diagnostics().await.unwrap_or_default();

            // Mixed Reality Context
            let reality = RealityContext {
                screenshot_path: std::path::PathBuf::from("pending"), // In a real run, this is saved to disk
                dom_elements: dom_elements.clone(),
                diagnostics: diagnostics.clone(),
                timestamp: Utc::now(),
            };

            let state = self
                .observer
                .observe_hybrid(&screenshot, &dom_elements)
                .await?;

            traces.push(reality);

            let action = self
                .planner
                .plan_for_persona(&state, &actor.goal, actor.persona.as_str())
                .await?;
            platform.execute(&action).await?;

            steps += 1;
        }

        warn!("Actor {} exhausted step budget.", actor.id);
        Ok(traces)
    }
}

pub struct MultiActorManager {
    engine: Arc<ActorEngine>,
}

impl MultiActorManager {
    pub fn new(engine: ActorEngine) -> Self {
        Self {
            engine: Arc::new(engine),
        }
    }

    pub async fn run_multi_actor<P: PlatformAdapter + 'static>(
        &self,
        actors: Vec<(Actor, P)>,
    ) -> Result<Vec<Vec<RealityContext>>> {
        let mut handles = vec![];

        for (actor, mut platform) in actors {
            let engine = self.engine.clone();
            let handle = tokio::spawn(async move { engine.run_actor(&actor, &mut platform).await });
            handles.push(handle);
        }

        let mut all_traces = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => all_traces.push(result?),
                Err(e) => return Err(OrchynError::Internal(format!("Actor task panicked: {}", e))),
            }
        }

        Ok(all_traces)
    }
}
