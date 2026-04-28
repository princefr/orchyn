use std::future::Future;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::ArtifactError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ArtifactKind {
    Screenshot,
    Video,
    DomSnapshot,
    ConsoleLog,
    NetworkLog,
    Patch,
    TestResult,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactMetadata {
    pub id: Uuid,
    pub run_id: Uuid,
    pub kind: ArtifactKind,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
    pub content_type: String,
}

pub trait ArtifactStorage: Send + Sync {
    fn save(
        &self,
        run_id: Uuid,
        kind: ArtifactKind,
        data: &[u8],
        content_type: &str,
    ) -> impl Future<Output = Result<ArtifactMetadata, ArtifactError>> + Send + '_;
    
    fn get(&self, artifact_id: Uuid) -> impl Future<Output = Result<Vec<u8>, ArtifactError>> + Send + '_;
    
    fn get_metadata(&self, artifact_id: Uuid) -> impl Future<Output = Result<ArtifactMetadata, ArtifactError>> + Send + '_;
    
    fn list_for_run(&self, run_id: Uuid) -> impl Future<Output = Result<Vec<ArtifactMetadata>, ArtifactError>> + Send + '_;
    
    fn delete(&self, artifact_id: Uuid) -> impl Future<Output = Result<(), ArtifactError>> + Send + '_;
}
