use orchyn_core::{Result, RealityContext, EvidenceBundle};
use orchyn_pixel_observer::UiState;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Step { n: usize, max: usize },
    ToolCall { name: String, args: Value },
    ToolResult { name: String, ok: bool, summary: String },
    Notice(String),
    Finish { summary: String },
    Evidence(Box<EvidenceBundle>),
}

pub trait Agent: Send + Sync {
    async fn propose_fix(
        &mut self,
        reality: &RealityContext,
        visual_state: &UiState,
    ) -> Result<String>;

    fn subscribe(&self) -> broadcast::Receiver<AgentEvent>;
    fn evidence(&self) -> Arc<Mutex<EvidenceBundle>>;
}
