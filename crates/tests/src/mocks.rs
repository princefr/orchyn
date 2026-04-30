#![allow(dead_code)]

use futures::future::{BoxFuture, FutureExt};
use image::{Rgba, RgbaImage};
use orchyn_core::{
    action::{Action as HarnessAction, AnnotatedElement},
    Action, DeviceInfo, DiagnosticEvent, NetworkEvent, PlatformAdapter, Platform,
    RawTelemetry, Result, UiElement,
};
use orchyn_model_client::MockClient;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

// ── PNG helpers ───────────────────────────────────────────────────────────────

/// A 100×100 grey RGBA PNG — valid input for annotate_screenshot and dhash.
pub fn blank_png() -> Vec<u8> {
    let img = RgbaImage::from_pixel(100, 100, Rgba([200, 200, 200, 255]));
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

/// A 100×100 PNG filled with `color`.
pub fn colored_png(color: [u8; 4]) -> Vec<u8> {
    let img = RgbaImage::from_pixel(100, 100, Rgba(color));
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

// ── ScriptedAdapter ───────────────────────────────────────────────────────────

/// Records every `execute_harness` call so tests can assert on it.
pub type CallLog = Arc<Mutex<Vec<(HarnessAction, Vec<AnnotatedElement>)>>>;

#[derive(Debug)]
pub struct ScriptedAdapter {
    screenshots: VecDeque<Vec<u8>>,
    elements: VecDeque<Vec<UiElement>>,
    current_url: String,
    pub call_log: CallLog,
}

impl ScriptedAdapter {
    pub fn new() -> Self {
        Self {
            screenshots: VecDeque::new(),
            elements: VecDeque::new(),
            current_url: "http://localhost/".to_string(),
            call_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn push_screenshot(&mut self, png: Vec<u8>) -> &mut Self {
        self.screenshots.push_back(png);
        self
    }

    pub fn push_elements(&mut self, els: Vec<UiElement>) -> &mut Self {
        self.elements.push_back(els);
        self
    }

    pub fn set_url(&mut self, url: &str) -> &mut Self {
        self.current_url = url.to_string();
        self
    }

    pub fn calls(&self) -> Vec<(HarnessAction, Vec<AnnotatedElement>)> {
        self.call_log.lock().unwrap().clone()
    }
}

impl Default for ScriptedAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformAdapter for ScriptedAdapter {
    fn device_info(&self) -> DeviceInfo {
        DeviceInfo { platform: Platform::Web, screen_width: 1280, screen_height: 800 }
    }

    fn screenshot(&mut self) -> BoxFuture<'_, Result<Vec<u8>>> {
        let bytes = self.screenshots.pop_front().unwrap_or_else(blank_png);
        async move { Ok(bytes) }.boxed()
    }

    fn execute(&mut self, _action: &Action) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }

    fn reset(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }

    fn diagnostics(&mut self) -> BoxFuture<'_, Result<Vec<DiagnosticEvent>>> {
        async move { Ok(vec![]) }.boxed()
    }

    fn network_events(&mut self) -> BoxFuture<'_, Result<Vec<NetworkEvent>>> {
        async move { Ok(vec![]) }.boxed()
    }

    fn get_elements(&mut self) -> BoxFuture<'_, Result<Vec<serde_json::Value>>> {
        async move { Ok(vec![]) }.boxed()
    }

    fn scrape_elements(&mut self) -> BoxFuture<'_, Result<Vec<UiElement>>> {
        let els = self.elements.pop_front().unwrap_or_default();
        async move { Ok(els) }.boxed()
    }

    fn current_url(&mut self) -> BoxFuture<'_, Result<String>> {
        let url = self.current_url.clone();
        async move { Ok(url) }.boxed()
    }

    fn collect_telemetry(&mut self) -> BoxFuture<'_, Result<RawTelemetry>> {
        async move { Ok(RawTelemetry::default()) }.boxed()
    }

    fn execute_harness(
        &mut self,
        action: &HarnessAction,
        element_map: &[AnnotatedElement],
    ) -> BoxFuture<'_, Result<()>> {
        self.call_log
            .lock()
            .unwrap()
            .push((action.clone(), element_map.to_vec()));
        async move { Ok(()) }.boxed()
    }
}

// ── Model JSON builders ───────────────────────────────────────────────────────

/// Returns a MockClient pre-loaded with the given JSON strings.
pub fn scripted_model(responses: Vec<String>) -> Arc<MockClient> {
    Arc::new(MockClient::new(responses))
}

/// Valid LlmDecision JSON for NAVIGATE.
pub fn decision_navigate(actor: &str, step: usize, url: &str) -> String {
    serde_json::json!({
        "actor_id": actor,
        "manifest_step": step,
        "thought": "navigate to the app",
        "action": "NAVIGATE",
        "value": url,
    })
    .to_string()
}

/// Valid LlmDecision JSON for CLICK.
pub fn decision_click(actor: &str, step: usize, target: u8) -> String {
    serde_json::json!({
        "actor_id": actor,
        "manifest_step": step,
        "thought": "click the element",
        "action": "CLICK",
        "target": target,
    })
    .to_string()
}

/// Valid LlmDecision JSON for TYPE.
pub fn decision_type(actor: &str, step: usize, target: u8, value: &str) -> String {
    serde_json::json!({
        "actor_id": actor,
        "manifest_step": step,
        "thought": "type into input",
        "action": "TYPE",
        "target": target,
        "value": value,
    })
    .to_string()
}

/// Valid LlmDecision JSON for ASSERT_SUCCESS ELEMENT_VISIBLE.
pub fn decision_assert_visible(actor: &str, step: usize, expected: &str) -> String {
    serde_json::json!({
        "actor_id": actor,
        "manifest_step": step,
        "thought": "assert element visible",
        "action": "ASSERT_SUCCESS",
        "assertion_type": "ELEMENT_VISIBLE",
        "expected": expected,
    })
    .to_string()
}

/// Valid agent-turn JSON: no tool calls, done = true (immediate finish).
pub fn agent_done_response(summary: &str) -> String {
    serde_json::json!({
        "reasoning": "Exploration complete.",
        "tool_calls": [],
        "done": true,
        "report": {
            "app_type": "test fixture",
            "features_found": ["login", "dashboard"],
            "gaps_found": 0,
            "tests_executed": 1,
            "tests_passed": 1,
            "tests_failed": 0,
            "summary": summary
        }
    })
    .to_string()
}

/// Valid agent-turn JSON: one tool call, done = false.
pub fn agent_tool_call_response(tool: &str, args: serde_json::Value) -> String {
    serde_json::json!({
        "reasoning": "Taking an action.",
        "tool_calls": [{ "name": tool, "arguments": args }],
        "done": false,
    })
    .to_string()
}
