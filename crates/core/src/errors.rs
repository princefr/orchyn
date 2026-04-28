#[derive(Debug)]
pub enum OrchynError {
    Internal(String),
    Model(String),
    Storage(String),
    Platform(String),
}

impl From<&str> for OrchynError {
    fn from(s: &str) -> Self {
        OrchynError::Internal(s.to_string())
    }
}

impl std::fmt::Display for OrchynError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchynError::Internal(e) => write!(f, "Internal error: {}", e),
            OrchynError::Model(e) => write!(f, "Model error: {}", e),
            OrchynError::Storage(e) => write!(f, "Storage error: {}", e),
            OrchynError::Platform(e) => write!(f, "Platform error: {}", e),
        }
    }
}

impl std::error::Error for OrchynError {}
