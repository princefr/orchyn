use orchyn_actor_engine::ActorPersona;
use orchyn_core::GapId;
use orchyn_spec_engine::Claim;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub id: GapId,
    pub kind: GapKind,
    pub claim: Claim,
    pub expected: String,
    pub observed: String,
    pub actor_persona: ActorPersona,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GapKind {
    MissingFeature,
    SilentFailure,
    WrongOutcome,
    UnintendedFeature,
    DataLoss,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}
