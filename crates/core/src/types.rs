use crate::action::BoundingBox;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpecId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClaimId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PatternId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GapId(pub Uuid);

pub type Confidence = f32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    Web,
    Android,
    IOS,
    Flutter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub file: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
}

impl From<(PathBuf, usize, usize)> for SourceRef {
    fn from((f, s, e): (PathBuf, usize, usize)) -> Self {
        Self {
            file: f,
            line_start: s,
            line_end: e,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticEvent {
    Console {
        level: LogLevel,
        message: String,
        timestamp: DateTime<Utc>,
    },
    Anomaly {
        kind: String,
        details: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEvent {
    pub method: String,
    pub url: String,
    pub status: u16,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealityContext {
    pub screenshot_path: PathBuf,
    pub dom_elements: Vec<serde_json::Value>,
    pub diagnostics: Vec<DiagnosticEvent>,
    pub network_events: Vec<NetworkEvent>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub description: String,
    pub source: String, // e.g., "code", "screenshot", "dom"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceBundle {
    pub goal: Option<String>,
    pub diagnostics: Vec<DiagnosticEvent>,
    pub network_events: Vec<NetworkEvent>,
    pub screenshot_paths: Vec<PathBuf>,
    pub findings: Vec<Evidence>,
}

// === Hybrid harness types ===
//
// These types power Constraint Mode (pre-written manifest execution) and
// Agent Mode (tool-using autonomous exploration). They are separate from the
// older phase-based pipeline that lives alongside in `RealityContext` etc.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiElement {
    pub element_type: String,
    pub text: Option<String>,
    pub bounds: BoundingBox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleLog {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRequest {
    pub method: String,
    pub url: String,
    pub status: u16,
}

/// A completed XHR/fetch exchange with its bodies, as observed by the platform
/// adapter. Bodies are best-effort: `None` when not captured (binary, too large,
/// or not available).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Exchange {
    pub method: String,
    pub url: String,
    pub status: u16,
    #[serde(default)]
    pub request_body: Option<String>,
    #[serde(default)]
    pub response_body: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawTelemetry {
    pub console_logs: Vec<ConsoleLog>,
    pub network_requests: Vec<NetworkRequest>,
    pub ws_messages: Vec<String>,
    pub current_url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub current_url: String,
    pub console_errors: Vec<String>,
    pub network_4xx_5xx: Vec<String>,
    pub ws_messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub step_idx: usize,
    pub action: String,
    pub success: bool,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActorState {
    pub actor_id: String,
    pub logged_in: bool,
    pub current_url: Option<String>,
    pub last_action: Option<String>,
    pub last_target: Option<u8>,
    pub seen_elements: Vec<String>,
    pub step_history: Vec<StepRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailPolicy {
    #[default]
    Halt,
    Skip,
    Retry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestStep {
    pub step: usize,
    pub actor: String,
    pub instruction: String,
    #[serde(default)]
    pub on_fail: FailPolicy,
    #[serde(default)]
    pub action_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RunStatus {
    Running,
    Passed,
    Failed(String),
    Stuck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunRecord {
    pub id: Uuid,
    pub project_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    pub manifest: Vec<ManifestStep>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_id_creation() {
        let uuid = Uuid::new_v4();
        let app_id = AppId(uuid);
        assert_eq!(app_id.0, uuid);
    }

    #[test]
    fn test_reality_context_serialization() {
        let ctx = RealityContext {
            screenshot_path: PathBuf::from("test.png"),
            dom_elements: vec![],
            diagnostics: vec![],
            network_events: vec![],
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        assert!(json.contains("test.png"));
    }
}
