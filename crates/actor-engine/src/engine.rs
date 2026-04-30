use orchyn_action_executor::{Action, PlatformAdapter};
use orchyn_core::{ActorId, Result, Goal};
use orchyn_model_client::ModelClient;
use orchyn_pixel_observer::{PixelObserver, UiState};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::types::ActorBudget;

#[derive(Clone)]
pub struct ActorEngine<M: ModelClient> {
    id: ActorId,
    model: Arc<M>,
    observer: PixelObserver<M>,
    adapter: Arc<Mutex<Box<dyn PlatformAdapter>>>,
    history: Vec<(UiState, Action)>,
    budget: ActorBudget,
}

impl<M: ModelClient> std::fmt::Debug for ActorEngine<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorEngine")
            .field("id", &self.id)
            .field("observer", &self.observer)
            .field("budget", &self.budget)
            .finish()
    }
}

// System prompts are loaded at compile time
const _PROMPT_PLANNER: &str = include_str!("../../../prompts/action_planner.md");

impl<M: ModelClient> ActorEngine<M> {
    pub fn new(
        model: Arc<M>,
        observer: PixelObserver<M>,
        adapter: Box<dyn PlatformAdapter>,
        budget: ActorBudget,
    ) -> Self {
        Self {
            id: ActorId(uuid::Uuid::new_v4()),
            model,
            observer,
            adapter: Arc::new(Mutex::new(adapter)),
            history: vec![],
            budget,
        }
    }

    pub async fn run(&mut self, goal: &Goal) -> Result<()> {
        info!("Actor starting with goal: {}", goal.description);
        
        let mut steps = 0;
        let max_steps = self.budget.max_steps;
        
        while steps < max_steps {
            steps += 1;
            
            let screenshot = {
                let mut adapter_guard = self.adapter.lock().await;
                adapter_guard.screenshot().await?
            };

            let current_state = self.observer.observe(&screenshot, &UiState::default()).await?;
            
            // Plan next action (stubbed to use agent reasoning in future)
            let action = Action::Wait { ms: 1000 };
            
            info!("Step {}/{}: Executing action: {:?}", steps, max_steps, action);
            
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
        
        info!("Actor finished after {} steps.", steps);
        Ok(())
    }
}
