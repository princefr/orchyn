use orchyn_core::{ActorId, Result};
use serde::{Deserialize, Serialize};
use crate::engine::ActorEngine;
use orchyn_model_client::ModelClient;
use orchyn_action_planner::Goal;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub max_tokens: usize,
    pub max_seconds: u64,
}

pub struct Actor {
    pub id: uuid::Uuid,
    pub persona: ActorPersona,
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
