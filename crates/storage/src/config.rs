use orchyn_core::OrchynError;

#[derive(Debug)]
pub struct StorageConfig {
    pub database_url: String,
    pub artifact_root: std::path::PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://localhost/orchyn".to_string()),
            artifact_root: std::env::var("ARTIFACT_ROOT")
                .unwrap_or_else(|_| "./storage/artifacts".to_string())
                .into(),
        }
    }
}
