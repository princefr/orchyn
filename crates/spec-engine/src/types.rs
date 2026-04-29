use chrono::{DateTime, Utc};
pub use orchyn_core::{AppId, ClaimId, Confidence, Evidence, SpecId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub id: SpecId,
    pub app_id: AppId,
    pub version: u32,
    pub claims: Vec<Claim>,
    pub generated_at: DateTime<Utc>,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub kind: ClaimKind,
    pub statement: String,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClaimKind {
    Feature,
    Constraint,
    Invariant,
    Flow,
}
