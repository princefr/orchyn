use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    pub criteria: Vec<String>,
}
