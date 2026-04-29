use orchyn_action_executor::{Action, PlatformAdapter};
use orchyn_action_planner::{ActionPlanner, Goal};
use orchyn_core::{ActorId, Result};
use orchyn_model_client::ModelClient;
use orchyn_pixel_observer::{PixelObserver, UiState};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::types::{ActorBudget, ActorPersona};

#[derive(Clone)]
pub struct ActorEngine<M: ModelClient> {
    id: ActorId,
    persona: ActorPersona,
    planner: ActionPlanner<M>,
    observer: PixelObserver<M>,
    adapter: Arc<Mutex<Box<dyn PlatformAdapter>>>,
    history: Vec<(UiState, Action)>,
    budget: ActorBudget,
}

impl<M: ModelClient> std::fmt::Debug for ActorEngine<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorEngine")
            .field("id", &self.id)
            .field("persona", &self.persona)
            .field("planner", &self.planner)
            .field("observer", &self.observer)
            .field("budget", &self.budget)
            .finish()
    }
}

const PROMPT_RATIONAL: &str = include_str!("../../../prompts/rational_actor.md");
const PROMPT_CONFUSED: &str = include_str!("../../../prompts/confused_actor.md");
const PROMPT_ADVERSARIAL: &str = include_str!("../../../prompts/adversarial_actor.md");
const PROMPT_CHAOS: &str = include_str!("../../../prompts/chaos_actor.md");
const PROMPT_ACCESSIBILITY: &str = include_str!("../../../prompts/accessibility_actor.md");

impl<M: ModelClient> ActorEngine<M> {
    pub fn new(
        persona: ActorPersona,
        planner: ActionPlanner<M>,
        observer: PixelObserver<M>,
        adapter: Box<dyn PlatformAdapter>,
        budget: ActorBudget,
    ) -> Self {
        Self {
            id: ActorId(uuid::Uuid::new_v4()),
            persona,
            planner,
            observer,
            adapter: Arc::new(Mutex::new(adapter)),
            history: vec![],
            budget,
        }
    }

    pub async fn run(&mut self, goal: &Goal) -> Result<()> {
        info!(
            "Actor {:?} starting with goal: {}",
            self.persona, goal.description
        );
        
        let mut steps = 0;
        let max_steps = self.budget.max_steps;
        
        let persona_prompt = match self.persona {
            ActorPersona::Rational => PROMPT_RATIONAL,
            ActorPersona::Confused => PROMPT_CONFUSED,
            ActorPersona::Adversarial => PROMPT_ADVERSARIAL,
            ActorPersona::Chaos => PROMPT_CHAOS,
            ActorPersona::Accessibility => PROMPT_ACCESSIBILITY,
        };

        while steps < max_steps {
            steps += 1;
            
            let screenshot = {
                let mut adapter_guard = self.adapter.lock().await;
                adapter_guard.screenshot().await?
            };

            let current_state = self.observer.observe(&screenshot, &UiState::default()).await?;
            let action = self.planner.plan_for_persona(&current_state, goal, persona_prompt).await?;
            
            info!("Step {}/{}: Actor {:?} executing action: {:?}", steps, max_steps, self.persona, action);
            
            {
                let mut adapter_guard = self.adapter.lock().await;
                adapter_guard.execute(&action).await?;
            }
            
            self.history.push((current_state, action.clone()));

            if let Action::Wait { ms } = action {
                if ms > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(ms as u64)).await;
                }
            }
        }
        
        info!("Actor {:?} finished after {} steps.", self.persona, steps);
        Ok(())
    }
}
