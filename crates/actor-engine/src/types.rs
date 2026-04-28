use orchyn_core::{ActorId, OrchynError, Result};
use serde::{Deserialize, Serialize};

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
pub struct RealityContext {
    pub screenshot_path: std::path::PathBuf,
    pub dom_elements: Vec<serde_json::Value>,
    pub diagnostics: Vec<orchyn_core::DiagnosticEvent>,
    pub network_events: Vec<orchyn_core::NetworkEvent>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
