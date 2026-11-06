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
    /// A JavaScript dialog (alert/confirm/prompt) whose message contains
    /// `contains` was shown to this actor since the run started (or the last
    /// `reset_telemetry`). Dialogs are accepted automatically.
    ExpectDialog { contains: String },
    /// Every visible, enabled interactive element (`button, a[href], input`,
    /// `select`, `textarea`, `[role=button]`) must be the hit target at its
    /// centre. Each is scrolled into view first.
    ExpectAllClickable,
    /// `location.reload()`, then wait for the page to load and settle.
    Reload,
    /// Store this actor's normalised `document.body.innerText` under `key`.
    SnapshotText { key: String },
    /// This actor's current normalised text must equal the snapshot under `key`.
    ExpectTextUnchanged { key: String },
    /// Monkey pass: really click each visible, enabled element matching
    /// `selector` (default `button, input[type=submit], [role=button]`), in DOM
    /// order, at most 20. Fails if a click produces a console error or a failed
    /// request. Navigation caused by a click is undone before the next click.
    ClickEach {
        #[serde(default)]
        selector: Option<String>,
    },
    /// Locates `selector` like `click` (scroll into view; fails if missing,
    /// zero-size or covered), then dispatches `pointerover`, `pointerenter`,
    /// `mouseover` and `mouseenter` on it. This is a JS event dispatch, not a
    /// real pointer move: CSS-only `:hover` menus are NOT supported, but JS
    /// hover handlers are triggered. Settles 250 ms.
    Hover { selector: String },
    /// HTML5 drag-and-drop from `selector` onto `target` via synthetic
    /// `DragEvent`s (`dragstart`, `dragenter`, `dragover`, `drop`, `dragend`).
    /// `position` is `"before"` (top quarter of the target), `"after"` (bottom
    /// quarter) or anything else for the centre. Both elements must exist and
    /// have non-zero size. Settles 300 ms.
    Drag {
        selector: String,
        target: String,
        #[serde(default)]
        position: Option<String>,
    },
    /// The element's `.value` equals `value`. A checkbox or radio reports
    /// `"true"`/`"false"` for `checked`. When `selector` matches several
    /// radios or checkboxes (a group), the value of the checked one is used,
    /// or `""` if none is checked.
    ExpectValue { selector: String, value: String },
    /// `document.querySelectorAll(selector).length == count`.
    ExpectCount { selector: String, count: usize },
    /// `localStorage` has `key`, and its value contains `contains` if given.
    ExpectStorage {
        key: String,
        #[serde(default)]
        contains: Option<String>,
    },
    /// `localStorage.setItem(key, value)`.
    SetStorage { key: String, value: String },
    /// This actor's current normalised text equals the text that `other_actor`
    /// stored under `key` with `snapshot_text`. Failure names the first
    /// differing line.
    ExpectSameText { key: String, other_actor: String },
    /// The element has attribute `name` (and, if `value` is given, its value
    /// equals `value`). Covers `required`, `contenteditable`, and so on.
    ExpectAttribute {
        selector: String,
        name: String,
        #[serde(default)]
        value: Option<String>,
    },
    /// Waits up to `timeout_ms` for a new window opened by this actor's pages
    /// (for example `window.open('preview.html')`), then makes it the active
    /// tab: later steps act on it. Fails with "no new window was opened" if
    /// none appears in time. Settles 300 ms.
    SwitchToPopup {
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    /// Makes tab `index` the active tab for this actor: 0 is the original tab,
    /// 1.. are the new windows in opening order. Settles 300 ms.
    SwitchTab { index: usize },
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
            StepOp::ExpectDialog { .. } => "expect_dialog",
            StepOp::ExpectAllClickable => "expect_all_clickable",
            StepOp::Reload => "reload",
            StepOp::SnapshotText { .. } => "snapshot_text",
            StepOp::ExpectTextUnchanged { .. } => "expect_text_unchanged",
            StepOp::ClickEach { .. } => "click_each",
            StepOp::Hover { .. } => "hover",
            StepOp::Drag { .. } => "drag",
            StepOp::ExpectValue { .. } => "expect_value",
            StepOp::ExpectCount { .. } => "expect_count",
            StepOp::ExpectStorage { .. } => "expect_storage",
            StepOp::SetStorage { .. } => "set_storage",
            StepOp::ExpectSameText { .. } => "expect_same_text",
            StepOp::ExpectAttribute { .. } => "expect_attribute",
            StepOp::SwitchToPopup { .. } => "switch_to_popup",
            StepOp::SwitchTab { .. } => "switch_tab",
        }
    }
}

impl StepOp {
    /// True for steps that only assert (or only record a snapshot to assert
    /// against later) and never change the app. Exhaustive on purpose: a new
    /// op must be classified here.
    pub fn is_assertion(&self) -> bool {
        match self {
            StepOp::ExpectText { .. }
            | StepOp::ExpectNoText { .. }
            | StepOp::ExpectUrl { .. }
            | StepOp::ExpectVisible { .. }
            | StepOp::ExpectConsoleClean
            | StepOp::ExpectRequest { .. }
            | StepOp::ExpectNoFailedRequests
            | StepOp::ExpectDialog { .. }
            | StepOp::ExpectAllClickable
            | StepOp::SnapshotText { .. }
            | StepOp::ExpectTextUnchanged { .. }
            | StepOp::ExpectValue { .. }
            | StepOp::ExpectCount { .. }
            | StepOp::ExpectStorage { .. }
            | StepOp::ExpectSameText { .. }
            | StepOp::ExpectAttribute { .. } => true,
            StepOp::Goto { .. }
            | StepOp::Click { .. }
            | StepOp::ClickText { .. }
            | StepOp::Fill { .. }
            | StepOp::WaitMs { .. }
            | StepOp::WaitForText { .. }
            | StepOp::ResetTelemetry
            | StepOp::Reload
            | StepOp::ClickEach { .. }
            | StepOp::Hover { .. }
            | StepOp::Drag { .. }
            | StepOp::SetStorage { .. }
            | StepOp::SwitchToPopup { .. }
            | StepOp::SwitchTab { .. } => false,
        }
    }
}

impl ReplayManifest {
    /// The same flow with every assertion removed and text waits turned into
    /// plain waits, so a build is driven through the whole flow (to observe
    /// its traffic and pages) instead of halting at the first failed check.
    pub fn actions_only(&self) -> ReplayManifest {
        let steps = self
            .steps
            .iter()
            .filter(|s| !s.op.is_assertion())
            .map(|s| ReplayStep {
                actor: s.actor.clone(),
                op: match &s.op {
                    StepOp::WaitForText { timeout_ms, .. } => StepOp::WaitMs {
                        ms: (*timeout_ms).min(1500),
                    },
                    other => other.clone(),
                },
            })
            .collect();
        ReplayManifest {
            name: format!("{} (actions only)", self.name),
            base_url: self.base_url.clone(),
            steps,
        }
    }

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
        assert!(matches!(
            m.steps[1].op,
            StepOp::ExpectText {
                timeout_ms: 3000,
                ..
            }
        ));
    }

    #[test]
    fn parses_universal_ops() {
        let m = ReplayManifest::from_json(
            r#"{"name":"u","steps":[
                {"op":"expect_all_clickable"},
                {"op":"reload"},
                {"op":"snapshot_text","key":"before"},
                {"op":"expect_text_unchanged","key":"before"},
                {"op":"click_each"},
                {"op":"click_each","selector":"a.nav"}]}"#,
        )
        .unwrap();
        let names: Vec<&str> = m.steps.iter().map(|s| s.op.name()).collect();
        assert_eq!(
            names,
            vec![
                "expect_all_clickable",
                "reload",
                "snapshot_text",
                "expect_text_unchanged",
                "click_each",
                "click_each",
            ]
        );
        assert_eq!(m.steps[4].op, StepOp::ClickEach { selector: None });
        assert_eq!(
            m.steps[5].op,
            StepOp::ClickEach {
                selector: Some("a.nav".to_string())
            }
        );
    }

    #[test]
    fn parses_tab_switching_ops() {
        let m = ReplayManifest::from_json(
            r#"{"name":"t","steps":[
                {"op":"switch_to_popup"},
                {"op":"switch_to_popup","timeout_ms":500},
                {"op":"switch_tab","index":0}]}"#,
        )
        .unwrap();
        assert_eq!(m.steps[0].op, StepOp::SwitchToPopup { timeout_ms: 3000 });
        assert_eq!(m.steps[1].op, StepOp::SwitchToPopup { timeout_ms: 500 });
        assert_eq!(m.steps[2].op, StepOp::SwitchTab { index: 0 });
        let names: Vec<&str> = m.steps.iter().map(|s| s.op.name()).collect();
        assert_eq!(
            names,
            vec!["switch_to_popup", "switch_to_popup", "switch_tab"]
        );
        // Tab switches change which page later steps act on, so they are actions.
        assert!(m.steps.iter().all(|s| !s.op.is_assertion()));
    }

    #[test]
    fn parses_new_ops() {
        let m = ReplayManifest::from_json(
            r##"{"name":"n","steps":[
                {"op":"hover","selector":"#menu"},
                {"op":"drag","selector":"#a","target":"#b"},
                {"op":"drag","selector":"#a","target":"#b","position":"before"},
                {"op":"expect_value","selector":"#e","value":"x"},
                {"op":"expect_count","selector":"li","count":3},
                {"op":"expect_storage","key":"k"},
                {"op":"expect_storage","key":"k","contains":"v"},
                {"op":"set_storage","key":"k","value":"v"},
                {"actor":"bob","op":"expect_same_text","key":"feed","other_actor":"alice"},
                {"op":"expect_attribute","selector":"#e","name":"required"},
                {"op":"expect_attribute","selector":"#e","name":"aria-label","value":"Go"}]}"##,
        )
        .unwrap();
        let names: Vec<&str> = m.steps.iter().map(|s| s.op.name()).collect();
        assert_eq!(
            names,
            vec![
                "hover",
                "drag",
                "drag",
                "expect_value",
                "expect_count",
                "expect_storage",
                "expect_storage",
                "set_storage",
                "expect_same_text",
                "expect_attribute",
                "expect_attribute",
            ]
        );
        assert_eq!(
            m.steps[1].op,
            StepOp::Drag {
                selector: "#a".into(),
                target: "#b".into(),
                position: None
            }
        );
        assert_eq!(
            m.steps[6].op,
            StepOp::ExpectStorage {
                key: "k".into(),
                contains: Some("v".into())
            }
        );
        assert_eq!(
            m.steps[10].op,
            StepOp::ExpectAttribute {
                selector: "#e".into(),
                name: "aria-label".into(),
                value: Some("Go".into())
            }
        );
    }
}
