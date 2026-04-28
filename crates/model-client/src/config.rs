use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    pub model_path: Option<String>,
    pub context_size: usize,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            context_size: 2048,
            temperature: 0.0,
            max_tokens: 1024,
        }
    }
}
