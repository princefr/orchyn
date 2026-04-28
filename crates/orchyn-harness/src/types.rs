use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

// --- Core Types ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Phase {
    Ingest,           // Parse repo, build call graph, identify entry points
    VisionBaseline,   // Screenshot every route, establish "golden" images
    SingleActorFlow,  // One user: signup → core action → logout
    MultiActorFlow,   // N users: causal chains, race conditions
    ErrorConstraint,  // Map all failures to (file, line, error_type)
    FixLoop,          // Iterative: generate patch → apply → re-verify
    Harden,           // Security, lint, typecheck (deterministic)
    Ship,             // Final artifact validation
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HarnessEvent {
    PhaseStart(Phase),
    PhaseEnd(Evidence),
    Failure(Failure),
    PatchApplied(SuggestedPatch),
    Diagnostic(orchyn_core::DiagnosticEvent),
    Network(orchyn_core::NetworkEvent),
    Notice(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub phase: Phase,
    pub artifacts: Vec<Artifact>,
    pub constraints: Vec<Constraint>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Artifact {
    Screenshot(Bytes),
    DomSnapshot(String),
    ConsoleLogs(Vec<String>),
    NetworkLogs(Vec<String>),
    Patch(SuggestedPatch),
    TestResult(TestResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedPatch {
    pub file_path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub original_content: String,
    pub suggested_content: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub test_name: String,
    pub passed: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Constraint {
    MustCompile,
    MustPassTest(String),
    NoLayoutShift,
    NoErrorLogs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail(String),
    Ambiguous(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Failure {
    pub id: Uuid,
    pub phase: Phase,
    pub error_type: String,
    pub message: String,
    pub location: Option<Location>,
    pub context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessConfig {
    pub target_url: String,
    pub max_fix_iterations: usize,
}
