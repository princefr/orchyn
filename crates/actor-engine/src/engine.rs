use orchyn_action_executor::{Action, PlatformAdapter};
use orchyn_action_planner::{ActionPlanner, Goal};
use orchyn_core::{ActorId, OrchynError, RealityContext, Result};
use orchyn_pixel_observer::{PixelObserver, UiState};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorPersona {
    Rational,
    Confused,
    Adversarial,
    Chaos,
    Accessibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorBudget {
    pub max_steps: usize,
    pub max_cost_usd: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorEngine {
    id: ActorId,
    persona: ActorPersona,
    planner: ActionPlanner,
    observer: PixelObserver,
    adapter: Arc<Mutex<dyn PlatformAdapter>>,
    history: Vec<(UiState, Action)>,
    budget: ActorBudget,
}

impl ActorEngine {
    pub fn new(
        persona: ActorPersona,
        planner: ActionPlanner,
        observer: PixelObserver,
        adapter: Box<dyn PlatformAdapter>,
        budget: ActorBudget,
    ) -> Self {
        Self {
            id: ActorId(Uuid::new_v4()),
            persona,
            planner,
            observer,
            adapter: Arc::new(Mutex::new(adapter)),
            history: vec![],
            budget,
        }
    }

    pub async fn run(&mut self, goal: &Goal) -> Result<()> {
        info!("Actor {:?} starting with goal: {}", self.persona, goal.description);
        // TODO: Implement actor loop
        Ok(())
    }
}
