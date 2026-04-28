use std::future::Future;
use std::path::PathBuf;
use std::fs;
use uuid::Uuid;
use chrono::Utc;
use crate::traits::*;
use crate::error::*;
use crate::ArtifactKind;

pub struct LocalArtifactStorage {
    root: PathBuf,
}

impl LocalArtifactStorage {
    pub fn new(root: PathBuf) -> Result<Self, ArtifactError> {
        if !root.exists() {
            fs::create_dir_all(&root).map_err(|e| ArtifactError::Io(e))?;
        }
        Ok(Self { root })
    }
    
    fn get_artifact_path(&self, artifact_id: Uuid, kind: &ArtifactKind) -> PathBuf {
        let extension = match kind {
            ArtifactKind::Screenshot => "png",
            ArtifactKind::Video => "mp4",
            ArtifactKind::DomSnapshot => "html",
            ArtifactKind::ConsoleLog => "txt",
            ArtifactKind::NetworkLog => "txt",
            ArtifactKind::Patch => "patch",
            ArtifactKind::TestResult => "json",
        };
        self.root.join(format!("{}.{}", artifact_id, extension))
    }
}

impl ArtifactStorage for LocalArtifactStorage {
    fn save(
        &self,
        run_id: Uuid,
        kind: ArtifactKind,
        data: &[u8],
        content_type: &str,
    ) -> impl Future<Output = Result<ArtifactMetadata, ArtifactError>> + Send + '_ {
        let root = self.root.clone();
        let data = data.to_vec();
        let content_type = content_type.to_string();
        async move {
            let id = Uuid::new_v4();
            let extension = match kind {
                ArtifactKind::Screenshot => "png",
                ArtifactKind::Video => "mp4",
                ArtifactKind::DomSnapshot => "html",
                ArtifactKind::ConsoleLog => "txt",
                ArtifactKind::NetworkLog => "txt",
                ArtifactKind::Patch => "patch",
                ArtifactKind::TestResult => "json",
            };
            let path = root.join(format!("{}.{}", id, extension));
            
            fs::write(&path, &data).map_err(|e| ArtifactError::Io(e))?;
            
            let metadata = ArtifactMetadata {
                id,
                run_id,
                kind,
                created_at: Utc::now(),
                size_bytes: data.len() as u64,
                content_type,
            };
            
            Ok(metadata)
        }
    }

    fn get(&self, artifact_id: Uuid) -> impl Future<Output = Result<Vec<u8>, ArtifactError>> + Send + '_ {
        let root = self.root.clone();
        async move {
            let extensions = ["png", "mp4", "html", "txt", "patch", "json"];
            
            for ext in &extensions {
                let path = root.join(format!("{}.{}", artifact_id, ext));
                if path.exists() {
                    return fs::read(&path).map_err(|e| ArtifactError::Io(e));
                }
            }
            
            Err(ArtifactError::NotFound(format!("Artifact {} not found", artifact_id)))
        }
    }

    fn get_metadata(&self, artifact_id: Uuid) -> impl Future<Output = Result<ArtifactMetadata, ArtifactError>> + Send + '_ {
        async move {
            // In a real implementation, we'd store metadata separately
            // For now, we'll return an error since we don't have metadata storage
            Err(ArtifactError::NotFound(format!("Metadata for artifact {} not found", artifact_id)))
        }
    }

    fn list_for_run(&self, _run_id: Uuid) -> impl Future<Output = Result<Vec<ArtifactMetadata>, ArtifactError>> + Send + '_ {
        async move {
            // In a real implementation, we'd query stored metadata
            // For now, return empty vec
            Ok(vec![])
        }
    }

    fn delete(&self, artifact_id: Uuid) -> impl Future<Output = Result<(), ArtifactError>> + Send + '_ {
        let root = self.root.clone();
        async move {
            let extensions = ["png", "mp4", "html", "txt", "patch", "json"];
            
            for ext in &extensions {
                let path = root.join(format!("{}.{}", artifact_id, ext));
                if path.exists() {
                    fs::remove_file(&path).map_err(|e| ArtifactError::Io(e))?;
                    return Ok(());
                }
            }
            
            Err(ArtifactError::NotFound(format!("Artifact {} not found", artifact_id)))
        }
    }
}
