use crate::error::Result;
use crate::traits::{ArtifactKind, ArtifactMetadata, ArtifactStorage};
use std::path::PathBuf;
use uuid::Uuid;
use chrono::Utc;
use tokio::fs;
use futures::future::{BoxFuture, FutureExt};

#[derive(Debug)]
pub struct LocalArtifactStorage {
    root: PathBuf,
}

impl LocalArtifactStorage {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.exists() {
            std::fs::create_dir_all(&root)?;
        }
        Ok(Self { root })
    }

    fn get_path(&self, id: Uuid) -> PathBuf {
        self.root.join(format!("{}.artifact", id))
    }

    fn get_meta_path(&self, id: Uuid) -> PathBuf {
        self.root.join(format!("{}.meta", id))
    }
}

impl ArtifactStorage for LocalArtifactStorage {
    fn save(
        &self,
        run_id: Uuid,
        kind: ArtifactKind,
        data: Vec<u8>,
        content_type: String,
    ) -> BoxFuture<'_, Result<ArtifactMetadata>> {
        let id = Uuid::new_v4();
        let metadata = ArtifactMetadata {
            id,
            run_id,
            kind,
            created_at: Utc::now(),
            size_bytes: data.len() as u64,
            content_type: content_type.to_string(),
        };

        let path = self.get_path(id);
        let meta_path = self.get_meta_path(id);

        async move {
            fs::write(path, data).await?;
            let meta_json = serde_json::to_string(&metadata)?;
            fs::write(meta_path, meta_json).await?;
            Ok(metadata)
        }.boxed()
    }

    fn get(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<Vec<u8>>> {
        let path = self.get_path(artifact_id);
        async move {
            let data = fs::read(path).await?;
            Ok(data)
        }.boxed()
    }

    fn get_metadata(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<ArtifactMetadata>> {
        let meta_path = self.get_meta_path(artifact_id);
        async move {
            let meta_json = fs::read_to_string(meta_path).await?;
            let metadata: ArtifactMetadata = serde_json::from_str(&meta_json)?;
            Ok(metadata)
        }.boxed()
    }

    fn list_for_run(&self, _run_id: Uuid) -> BoxFuture<'_, Result<Vec<ArtifactMetadata>>> {
        async move {
            // Placeholder: This would normally scan the directory or use a DB
            Ok(vec![])
        }.boxed()
    }

    fn delete(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<()>> {
        let path = self.get_path(artifact_id);
        let meta_path = self.get_meta_path(artifact_id);
        async move {
            let _ = fs::remove_file(path).await;
            let _ = fs::remove_file(meta_path).await;
            Ok(())
        }.boxed()
    }
}
