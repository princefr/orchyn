use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

pub type AppId = Uuid;
pub type UserId = Uuid;
pub type RunId = Uuid;
pub type ActorId = Uuid;
pub type ScreenId = Uuid;
pub type SpecId = Uuid;
pub type ClaimId = Uuid;
pub type GapId = Uuid;
pub type PatternId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubscriptionTier {
    Free,
    Pro,
    Enterprise,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticEvent {
    Console {
        level: LogLevel,
        message: String,
        timestamp: DateTime<Utc>,
    },
    NetworkError {
        url: String,
        method: String,
        status: u16,
        error: String,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealityContext {
    pub screenshot_path: PathBuf,
    pub dom_elements: Vec<serde_json::Value>,
    pub diagnostics: Vec<DiagnosticEvent>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub email: String,
    pub stripe_customer_id: Option<String>,
    pub subscription_tier: SubscriptionTier,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialOrd, PartialEq)]
pub struct Confidence(f32);

impl Confidence {
    pub fn new(val: f32) -> Self {
        Self(val.clamp(0.0, 1.0))
    }
    pub fn value(&self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub file: PathBuf,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Platform {
    Web,
    Android,
    Ios,
    Flutter,
    ReactNative,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source: SourceRef,
    pub excerpt: String,
    pub reason: String,
}

/// Single line of output from a static-analysis tool (cargo check, clippy, eslint, tsc, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticAnalysisFinding {
    /// Emitting tool: "cargo", "clippy", "eslint", "tsc", ...
    pub tool: String,
    pub level: LogLevel,
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
    pub message: String,
}

/// Line captured from a test runner (Playwright, `cargo test`, `npm test`, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunnerLog {
    /// "playwright", "cargo-test", "jest", ...
    pub runner: String,
    pub suite: Option<String>,
    pub test: Option<String>,
    pub passed: Option<bool>,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

/// Free-form runtime output captured from a long-running process (the app under
/// test, the dev server, a worker). Distinct from `DiagnosticEvent` which is
/// in-browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeOutput {
    /// "stdout" | "stderr" | "panic" | custom
    pub stream: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

/// Structured VLM observation of a screenshot: kept as opaque JSON so the
/// `pixel-observer` crate is free to evolve its schema without churning core.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VisualObservation {
    pub screenshot_path: Option<PathBuf>,
    pub description: Option<String>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// Self-managed task entry the agent writes via the `todo_write` tool. Visible
/// in the TUI so a human spectator can follow the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: u32,
    pub status: TodoStatus,
    pub description: String,
}

/// Everything the fixer harness has gathered about the current failure, in one
/// place. This is the human-like "evidence pile" — visuals, console, network,
/// runtime output, tests, and static analysis — that gets fed to Gemma so it
/// can propose tool calls that actually fix the underlying issue.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceBundle {
    pub goal: Option<String>,
    pub visual: Option<VisualObservation>,
    pub dom_elements: Vec<serde_json::Value>,
    pub console: Vec<DiagnosticEvent>,
    pub runtime: Vec<RuntimeOutput>,
    pub tests: Vec<TestRunnerLog>,
    pub static_analysis: Vec<StaticAnalysisFinding>,
    pub notes: Vec<String>,
    pub todos: Vec<Todo>,
    /// Number of `static_analysis` runs the agent has performed. The
    /// convergence check requires this to be ≥ 1 — an empty bundle is not
    /// "clean", it's "unevaluated".
    pub static_analysis_runs: u32,
    /// Number of `run_tests` runs the agent has performed.
    pub test_runs: u32,
}

impl EvidenceBundle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_goal(mut self, goal: impl Into<String>) -> Self {
        self.goal = Some(goal.into());
        self
    }

    pub fn push_console(&mut self, event: DiagnosticEvent) {
        self.console.push(event);
    }

    pub fn push_runtime(&mut self, output: RuntimeOutput) {
        self.runtime.push(output);
    }

    pub fn push_test(&mut self, log: TestRunnerLog) {
        self.tests.push(log);
    }

    pub fn push_finding(&mut self, finding: StaticAnalysisFinding) {
        self.static_analysis.push(finding);
    }

    pub fn extend_from_reality(&mut self, reality: &RealityContext) {
        self.dom_elements
            .extend(reality.dom_elements.iter().cloned());
        self.console.extend(reality.diagnostics.iter().cloned());
        if self.visual.is_none() {
            self.visual = Some(VisualObservation {
                screenshot_path: Some(reality.screenshot_path.clone()),
                ..Default::default()
            });
        }
    }

    /// Replace prior findings produced by `tool` with `new_findings`, then
    /// bump the run counter. Each `static_analysis` run is a fresh snapshot,
    /// so old findings from the same tool must not linger.
    pub fn replace_findings_for_tool(
        &mut self,
        tool: &str,
        new_findings: Vec<StaticAnalysisFinding>,
    ) {
        self.static_analysis.retain(|f| f.tool != tool);
        self.static_analysis.extend(new_findings);
        self.static_analysis_runs = self.static_analysis_runs.saturating_add(1);
    }

    /// Replace prior test logs from `runner` with `new_logs`, then bump the
    /// run counter.
    pub fn replace_tests_for_runner(
        &mut self,
        runner: &str,
        new_logs: Vec<TestRunnerLog>,
    ) {
        self.tests.retain(|t| t.runner != runner);
        self.tests.extend(new_logs);
        self.test_runs = self.test_runs.saturating_add(1);
    }

    /// True when the bundle reflects a fully-verified clean state. Requires:
    /// (a) at least one `static_analysis` run has happened — empty bundles are
    /// "unevaluated", not "clean"; (b) no error-level static findings;
    /// (c) zero failing tests *if* any test run has happened (no test run is
    /// permissible — projects without tests still need to converge);
    /// (d) no console / network errors; (e) no panics.
    pub fn is_clean(&self) -> bool {
        if self.static_analysis_runs == 0 {
            return false;
        }
        let no_errors = !self.console.iter().any(|e| {
            matches!(
                e,
                DiagnosticEvent::Console {
                    level: LogLevel::Error,
                    ..
                } | DiagnosticEvent::NetworkError { .. }
            )
        });
        let no_findings = self
            .static_analysis
            .iter()
            .all(|f| !matches!(f.level, LogLevel::Error));
        let no_failed_tests = self.tests.iter().all(|t| t.passed.unwrap_or(true));
        let no_panics = self.runtime.iter().all(|r| r.stream != "panic");
        no_errors && no_findings && no_failed_tests && no_panics
    }
}

#[derive(Debug, Error)]
pub enum OrchynError {
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Model error: {0}")]
    Model(String),
    #[error("Platform error: {0}")]
    Platform(String),
    #[error("Storage error: {0}")]
    Storage(String),
}

pub type Result<T, E = OrchynError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    fn err_finding(tool: &str) -> StaticAnalysisFinding {
        StaticAnalysisFinding {
            tool: tool.to_string(),
            level: LogLevel::Error,
            file: Some(PathBuf::from("a.rs")),
            line: Some(1),
            message: "boom".to_string(),
        }
    }

    #[test]
    fn empty_bundle_is_not_clean() {
        let b = EvidenceBundle::new();
        assert!(
            !b.is_clean(),
            "empty bundle must be unevaluated, not clean"
        );
    }

    #[test]
    fn bundle_clean_after_zero_error_static_run() {
        let mut b = EvidenceBundle::new();
        b.replace_findings_for_tool("cargo", vec![]); // analyzer ran, nothing found
        assert_eq!(b.static_analysis_runs, 1);
        assert!(b.is_clean());
    }

    #[test]
    fn bundle_unclean_when_static_has_errors() {
        let mut b = EvidenceBundle::new();
        b.replace_findings_for_tool("cargo", vec![err_finding("cargo")]);
        assert!(!b.is_clean());
    }

    #[test]
    fn second_static_run_replaces_first() {
        let mut b = EvidenceBundle::new();
        b.replace_findings_for_tool("cargo", vec![err_finding("cargo"), err_finding("cargo")]);
        assert_eq!(b.static_analysis.len(), 2);
        // Re-run with no errors — old findings must be evicted.
        b.replace_findings_for_tool("cargo", vec![]);
        assert_eq!(b.static_analysis.len(), 0);
        assert_eq!(b.static_analysis_runs, 2);
        assert!(b.is_clean());
    }

    #[test]
    fn second_static_run_keeps_other_tool_findings() {
        let mut b = EvidenceBundle::new();
        b.replace_findings_for_tool("cargo", vec![err_finding("cargo")]);
        b.replace_findings_for_tool("eslint", vec![err_finding("eslint")]);
        // Cargo re-run with no errors — eslint's still there.
        b.replace_findings_for_tool("cargo", vec![]);
        assert_eq!(b.static_analysis.len(), 1);
        assert_eq!(b.static_analysis[0].tool, "eslint");
        assert!(!b.is_clean());
    }

    #[test]
    fn failing_tests_block_clean() {
        let mut b = EvidenceBundle::new();
        b.replace_findings_for_tool("cargo", vec![]);
        b.replace_tests_for_runner(
            "cargo-test",
            vec![TestRunnerLog {
                runner: "cargo-test".to_string(),
                suite: None,
                test: Some("foo::bar".to_string()),
                passed: Some(false),
                message: "FAIL".to_string(),
                timestamp: Utc::now(),
            }],
        );
        assert!(!b.is_clean());
    }

    #[test]
    fn passing_tests_then_clean_static_is_clean() {
        let mut b = EvidenceBundle::new();
        b.replace_findings_for_tool("cargo", vec![]);
        b.replace_tests_for_runner(
            "cargo-test",
            vec![TestRunnerLog {
                runner: "cargo-test".to_string(),
                suite: None,
                test: Some("foo::bar".to_string()),
                passed: Some(true),
                message: "ok".to_string(),
                timestamp: Utc::now(),
            }],
        );
        assert!(b.is_clean());
    }
}
