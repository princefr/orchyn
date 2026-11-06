//! Replay manifest: a list of *concrete* steps that run with no LLM.
//!
//! This is the deterministic half of Orchyn ("Constraint Mode"). An explorer
//! (human, frontier model, or agent mode) writes the manifest once; the replay
//! runner executes it on every commit and gives the same answer every time.
//!
//! JSON shape:
//! ```json
//! {
//!   "name": "login",
//!   "base_url": "http://localhost:8092",
//!   "steps": [
//!     {"op": "goto", "path": "/"},
//!     {"op": "fill", "selector": "#email", "value": "alice@test.com"},
//!     {"op": "click", "selector": "button[type=submit]"},
//!     {"op": "expect_text", "text": "Welcome"},
//!     {"op": "expect_request", "method": "POST", "path_contains": "/api/login", "status": 200},
//!     {"op": "expect_console_clean"},
//!     {"actor": "bob", "op": "goto", "path": "/"}
//!   ]
//! }
//! ```
//! Every step may carry `"actor"` (default `"default"`). Each distinct actor
//! gets its own isolated browser.

use serde::{Deserialize, Serialize};

pub const DEFAULT_ACTOR: &str = "default";

fn default_actor() -> String {
    DEFAULT_ACTOR.to_string()
}

fn default_timeout_ms() -> u64 {
    3000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayManifest {
    pub name: String,
    /// Base URL; `goto.path` is joined onto it. May be overridden at run time.
    #[serde(default)]
    pub base_url: String,
    pub steps: Vec<ReplayStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayStep {
    #[serde(default = "default_actor")]
    pub actor: String,
    #[serde(flatten)]
    pub op: StepOp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StepOp {
    /// Navigate to `base_url + path` (or `path` if it is absolute).
    Goto { path: String },
    /// Real mouse click at the element's centre. Fails if the element is
    /// missing, has zero size, or is covered by another element at that point
    /// (i.e. what the *pixels* show differs from what the DOM claims).
    Click { selector: String },
    /// Like `click`, but finds the first visible button/link/[role=button]
    /// whose trimmed innerText equals `text`.
    ClickText { text: String },
    /// Focus the element by real click, clear it, then type `value`.
    Fill { selector: String, value: String },
    /// Sleep.
    WaitMs { ms: u64 },
    /// Poll until `text` is visible in document.body.innerText (or timeout).
    WaitForText {
        text: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// Polls up to `timeout_ms` for `text` in document.body.innerText.
    ExpectText {
        text: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// `text` must NOT be in document.body.innerText (checked once).
    ExpectNoText { text: String },
    /// Current URL must contain `contains`.
    ExpectUrl { contains: String },
    /// Element exists, has non-zero size, is not `display:none` /
    /// `visibility:hidden`, and is not covered at its centre point.
    ExpectVisible { selector: String },
    /// No console `error`-level messages and no uncaught exceptions seen by
    /// this actor since the run started (or since the last `reset_telemetry`).
    ExpectConsoleClean,
    /// At least one observed request matches method + path substring (+ status
    /// if given).
    ExpectRequest {
        method: String,
        path_contains: String,
        #[serde(default)]
        status: Option<u16>,
    },
    /// No observed request returned status >= 400 (or failed at network level).
    ExpectNoFailedRequests,
    /// Forget telemetry collected so far for this actor.
    ResetTelemetry,
}

impl StepOp {
    /// Short machine name, e.g. `"expect_text"`.
    pub fn name(&self) -> &'static str {
        match self {
            StepOp::Goto { .. } => "goto",
            StepOp::Click { .. } => "click",
            StepOp::ClickText { .. } => "click_text",
            StepOp::Fill { .. } => "fill",
            StepOp::WaitMs { .. } => "wait_ms",
            StepOp::WaitForText { .. } => "wait_for_text",
            StepOp::ExpectText { .. } => "expect_text",
            StepOp::ExpectNoText { .. } => "expect_no_text",
            StepOp::ExpectUrl { .. } => "expect_url",
            StepOp::ExpectVisible { .. } => "expect_visible",
            StepOp::ExpectConsoleClean => "expect_console_clean",
            StepOp::ExpectRequest { .. } => "expect_request",
            StepOp::ExpectNoFailedRequests => "expect_no_failed_requests",
            StepOp::ResetTelemetry => "reset_telemetry",
        }
    }
}

impl ReplayManifest {
    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }

    /// Distinct actors in first-appearance order.
    pub fn actors(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in &self.steps {
            if !out.contains(&s.actor) {
                out.push(s.actor.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example() {
        let m = ReplayManifest::from_json(
            r#"{"name":"x","base_url":"http://h","steps":[
                {"op":"goto","path":"/"},
                {"op":"expect_text","text":"Hi"},
                {"actor":"bob","op":"expect_request","method":"GET","path_contains":"/api"},
                {"op":"expect_console_clean"}]}"#,
        )
        .unwrap();
        assert_eq!(m.steps.len(), 4);
        assert_eq!(m.steps[0].actor, DEFAULT_ACTOR);
        assert_eq!(m.actors(), vec!["default".to_string(), "bob".to_string()]);
        assert!(matches!(m.steps[1].op, StepOp::ExpectText { timeout_ms: 3000, .. }));
    }
}
