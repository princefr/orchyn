use thiserror::Error;

#[derive(Error, Debug)]
pub enum OrchynError {
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Model error: {0}")]
    Model(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Platform error: {0}")]
    Platform(String),

    #[error("Auth error: {0}")]
    Auth(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, OrchynError>;

impl From<anyhow::Error> for OrchynError {
    fn from(e: anyhow::Error) -> Self {
        OrchynError::Internal(e.to_string())
    }
}
