use serde::{Deserialize, Serialize};
use crate::types::{Phase, Evidence};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HarnessEvent {
    PhaseStart(Phase),
    PhaseEnd(Evidence),
    PatchApplied(Patch),
    Diagnostic(orchyn_core::DiagnosticEvent),
    Network(orchyn_core::NetworkEvent),
    Notice(String),
    Failure(HarnessFailure),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub file_path: PathBuf,
    pub original_content: String,
    pub suggested_content: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessFailure {
    pub message: String,
    pub phase: Phase,
}
