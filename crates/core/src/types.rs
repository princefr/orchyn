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
