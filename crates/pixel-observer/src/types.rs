use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub elements: Vec<UiElement>,
    pub anomalies: Vec<Anomaly>,
    pub keyboard_visible: bool,
}
