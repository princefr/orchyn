use anyhow::Result;
use hf_hub::{api::tokio::Api, Repo, RepoType};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone)]
pub struct ModelConfig {
    // Single Unified Model (Qwen 3.5 9B for GPU)
    pub repo: String,
    pub filename: String,
    pub tokenizer_repo: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            // Using Qwen 3.5 9B Q4_K_M for native multimodal + reasoning on GPU
            repo: "bartowski/Qwen3.5-9B-Instruct-GGUF".to_string(),
            filename: "Qwen3.5-9B-Instruct-Q4_K_M.gguf".to_string(),
            tokenizer_repo: "Qwen/Qwen3.5-9B-Instruct".to_string(),
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
        
        info!("Syncing unified Qwen 3.5 9B model: {}", self.config.repo);
        let repo = api.repo(Repo::with_revision(
            self.config.repo.clone(),
            RepoType::Model,
            "main".to_string(),
        ));
        let weights = repo.get(&self.config.filename).await?;

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
