pub mod agent;
pub mod diff_engine;
pub mod events;
pub mod harness;
pub mod intent;
pub mod replay;
pub mod types;

pub use agent::AgentProgressEvent;
pub use diff_engine::{DiffEngine, DiffReport, GapEntry};
pub use events::*;
pub use harness::HarnessRunResult;
pub use harness::OrchynHarness;
pub use types::*;

use orchyn_model_client::ModelClient;
use std::sync::Arc;

pub trait LlmClient: Send + Sync {
    fn client(&self) -> Arc<dyn ModelClient>;
}

pub struct RealLlm {
    pub client: Arc<dyn ModelClient>,
}

impl LlmClient for RealLlm {
    fn client(&self) -> Arc<dyn ModelClient> {
        self.client.clone()
    }
}

pub struct MockLlm {
    pub client: Arc<dyn ModelClient>,
}

impl LlmClient for MockLlm {
    fn client(&self) -> Arc<dyn ModelClient> {
        self.client.clone()
    }
}
