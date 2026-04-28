use orchyn_core::OrchynError;

#[derive(Debug)]
pub enum StorageError {
    ConnectionFailed(String),
    QueryFailed(String),
    NotFound,
}

impl From<sqlx::Error> for StorageError {
    fn from(e: sqlx::Error) -> Self {
        StorageError::QueryFailed(e.to_string())
    }
}

impl From<StorageError> for OrchynError {
    fn from(e: StorageError) -> Self {
        OrchynError::Storage(e.to_string())
    }
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::ConnectionFailed(e) => write!(f, "Connection failed: {}", e),
            StorageError::QueryFailed(e) => write!(f, "Query failed: {}", e),
            StorageError::NotFound => write!(f, "Record not found"),
        }
    }
}

impl std::error::Error for StorageError {}
