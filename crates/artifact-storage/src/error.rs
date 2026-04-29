use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum ArtifactError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Artifact not found: {0}")]
    NotFound(Uuid),

    #[error("Internal storage error: {0}")]
    Internal(String),

    #[error("Invalid metadata: {0}")]
    InvalidMetadata(String),
}

pub type Result<T> = std::result::Result<T, ArtifactError>;
