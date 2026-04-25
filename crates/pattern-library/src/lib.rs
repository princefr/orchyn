use orchyn_core::PatternId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub id: PatternId,
    pub signature: String,
    pub description: String,
    pub framework_tags: Vec<String>,
    pub prevalence: f32,
    pub detection_hint: String,
}
