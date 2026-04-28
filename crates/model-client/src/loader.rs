use anyhow::Result;
use hf_hub::{api::tokio::Api, Repo, RepoType};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone)]
pub struct ModelConfig {
    // Single Unified Model (Qwen 3.5 0.8B for fast, unified text+vision)
    pub repo: String,
    pub filename: String,
    pub tokenizer_repo: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            // Using bartowski's GGUF of the official Qwen 3.5 0.8B
            // The official repo only has Safetensors, which requires a different loader.
            repo: "bartowski/Qwen_Qwen3.5-0.8B-GGUF".to_string(),
            filename: "Qwen_Qwen3.5-0.8B-Q4_K_M.gguf".to_string(),
            tokenizer_repo: "Qwen/Qwen3.5-0.8B".to_string(),
        }
    }
}

pub struct ModelManager {
    config: ModelConfig,
}

impl ModelManager {
    pub fn new(config: ModelConfig) -> Self {
        Self { config }
    }

    pub async fn download_models(&self) -> Result<LocalModelPaths> {
        let api = Api::new()?;
        
        info!("Syncing unified Qwen 3.5 0.8B model: {}", self.config.repo);
        let repo = api.repo(Repo::with_revision(
            self.config.repo.clone(),
            RepoType::Model,
            "main".to_string(),
        ));
        let weights = repo.get(&self.config.filename).await?;

        // Tokenizer is always from the official repo
        let t_repo = api.repo(Repo::with_revision(
            self.config.tokenizer_repo.clone(),
            RepoType::Model,
            "main".to_string(),
        ));
        let tokenizer = t_repo.get("tokenizer.json").await?;

        Ok(LocalModelPaths {
            weights,
            tokenizer,
        })
    }
}

#[derive(Debug, Clone)]
pub struct LocalModelPaths {
    pub weights: PathBuf,
    pub tokenizer: PathBuf,
}
