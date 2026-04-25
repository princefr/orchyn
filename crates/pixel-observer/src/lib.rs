use orchyn_core::{OrchynError, Result};
use orchyn_model_client::QwenClient;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// VLM system prompt embedded at compile time. Editing the markdown rebuilds
/// the crate.
pub const PROMPT_VLM_OBSERVER: &str =
    include_str!("../../../prompts/vlm_observer.md");
pub const SKILL_VLM_PROMPTING: &str =
    include_str!("../../../skills/vlm-prompting/SKILL.md");

/// Build the question handed to the Moondream VLM. The model template wraps
/// it as `Question: ... Answer:` (see `model-client/src/inference.rs`), so we
/// inline the system prompt + skill + observation request as the question.
fn vlm_question(observation_focus: &str) -> String {
    format!(
        "{}\n\n=== SKILL: VLM PROMPTING ===\n{}\n\n=== TASK ===\n{}",
        PROMPT_VLM_OBSERVER, SKILL_VLM_PROMPTING, observation_focus
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub elements: Vec<UiElement>,
    pub anomalies: Vec<Anomaly>,
    pub keyboard_visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiElement {
    pub kind: String,
    pub label: Option<String>,
    pub bounding_box: BoundingBox,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub kind: String,
    pub details: String,
}

#[derive(Clone)]
pub struct PixelObserver {
    model: QwenClient,
}

impl PixelObserver {
    pub fn new(model: QwenClient) -> Self {
        Self { model }
    }

    pub async fn observe(&self, screenshot_bytes: &[u8]) -> Result<UiState> {
        self.observe_hybrid(screenshot_bytes, &[]).await
    }

    pub async fn observe_hybrid(
        &self,
        screenshot_bytes: &[u8],
        dom_elements: &[serde_json::Value],
    ) -> Result<UiState> {
        info!(
            "Observing UI state (Hybrid mode, {} DOM hints)",
            dom_elements.len()
        );

        // --- Fast Path: If we have DOM elements, ask the VLM to reconcile ---
        let observation_focus = if dom_elements.is_empty() {
            "Describe the UI you see. Emit one JSON object matching the schema above. No prose.".to_string()
        } else {
            let dom_json = serde_json::to_string(dom_elements).unwrap_or_default();
            format!(
                "DOM hints from the platform adapter: {}.\n\
                Return one JSON object matching the schema above. For every element, \
                cross-check the DOM hint against the pixels: bounding box, label, \
                enabled/focused state, and any anomaly. Flag mismatches in `anomalies`.",
                dom_json
            )
        };

        let question = vlm_question(&observation_focus);

        match tokio::time::timeout(
            std::time::Duration::from_secs(300),
            self.model.observe(screenshot_bytes, &question),
        )
        .await
        {
            Ok(Ok(result)) => {
                let state: UiState = serde_json::from_value(result).map_err(|e| {
                    OrchynError::Model(format!("Failed to parse VLM output: {}", e))
                })?;
                Ok(state)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                warn!("VLM observation timed out. Falling back to DOM-only state.");
                // Fallback: Construct a UiState from DOM if VLM is too slow
                self.construct_from_dom(dom_elements)
            }
        }
    }

    fn construct_from_dom(&self, dom_elements: &[serde_json::Value]) -> Result<UiState> {
        let mut elements = Vec::new();
        for el in dom_elements {
            elements.push(UiElement {
                kind: el
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string(),
                label: el
                    .get("label")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                bounding_box: BoundingBox {
                    x: el.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as i32,
                    y: el.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as i32,
                    width: el.get("width").and_then(|v| v.as_f64()).unwrap_or(0.0) as i32,
                    height: el.get("height").and_then(|v| v.as_f64()).unwrap_or(0.0) as i32,
                },
                confidence: 0.8, // Reduced confidence for fallback
            });
        }
        Ok(UiState {
            elements,
            anomalies: vec![],
            keyboard_visible: false,
        })
    }
}
