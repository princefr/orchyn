use orchyn_core::{ActorId, Result, Goal};
use serde::{Deserialize, Serialize};
use crate::engine::ActorEngine;
use orchyn_model_client::ModelClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorBudget {
    pub max_steps: usize,
    pub max_cost_usd: f32,
    pub max_tokens: usize,
    pub max_seconds: u64,
}

pub struct Actor {
    pub id: uuid::Uuid,
    pub goal: Goal,
    pub budget: ActorBudget,
}

pub struct MultiActorManager<M: ModelClient> {
    engine: ActorEngine<M>,
}

impl<M: ModelClient> MultiActorManager<M> {
    pub fn new(engine: ActorEngine<M>) -> Self {
        Self { engine }
    }
}
