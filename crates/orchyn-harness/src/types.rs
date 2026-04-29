use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

// --- Core Types ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Phase {
    Ingest,       // Codebase indexing + Spec inference
    Observation,  // Test run + Trace collection
    GapDetection, // Spec vs Reality reconciliation
    FixLoop,      // LLM-led patch generation
    Ship,         // Validation + Final Report
    VisionBaseline,
    SingleActorFlow,
    MultiActorFlow,
    ErrorConstraint,
    Harden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: Uuid,
    pub phase: Phase,
    pub kind: EvidenceKind,
    pub data: EvidenceData,
    pub timestamp: DateTime<Utc>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvidenceKind {
    Screenshot,
    Log,
    NetworkTrace,
    SpecDelta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvidenceData {
    Text(String),
    Binary(Vec<u8>),
    Screenshot(Bytes),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail(String),
    Inconclusive(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: Uuid,
    pub kind: String,
    pub local_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessConfig {
    pub target_url: String,
    pub max_fix_iterations: usize,
    pub phases: Option<Vec<Phase>>, // None = use defaults
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            target_url: "http://localhost:3000".to_string(),
            max_fix_iterations: 30,
            phases: None, // Will use default phases
        }
    }
}
