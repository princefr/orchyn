use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::error::Result;
use futures::future::BoxFuture;
use std::fmt::Debug;

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

pub trait ArtifactStorage: Send + Sync + Debug {
    fn save(
        &self,
        run_id: Uuid,
        kind: ArtifactKind,
        data: Vec<u8>,
        content_type: String,
    ) -> BoxFuture<'_, Result<ArtifactMetadata>>;
    
    fn get(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<Vec<u8>>>;
    
    fn get_metadata(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<ArtifactMetadata>>;
    
    fn list_for_run(&self, run_id: Uuid) -> BoxFuture<'_, Result<Vec<ArtifactMetadata>>>;
    
    fn delete(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<()>>;
}
