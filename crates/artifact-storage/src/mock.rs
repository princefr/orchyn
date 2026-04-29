use crate::error::Result;
use crate::traits::{ArtifactKind, ArtifactMetadata, ArtifactStorage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use chrono::Utc;
use futures::future::{BoxFuture, FutureExt};

#[derive(Debug)]
pub struct MockArtifactStorage {
    data: Arc<Mutex<HashMap<Uuid, Vec<u8>>>>,
    metadata: Arc<Mutex<HashMap<Uuid, ArtifactMetadata>>>,
}

impl MockArtifactStorage {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            metadata: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ArtifactStorage for MockArtifactStorage {
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
            content_type,
        };

        let mut data_lock = self.data.lock().unwrap();
        let mut meta_lock = self.metadata.lock().unwrap();
        
        data_lock.insert(id, data);
        meta_lock.insert(id, metadata.clone());

        async move {
            Ok(metadata)
        }.boxed()
    }

    fn get(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<Vec<u8>>> {
        let data_lock = self.data.lock().unwrap();
        let res = data_lock.get(&artifact_id).cloned().ok_or(crate::ArtifactError::NotFound(artifact_id));
        async move { res }.boxed()
    }

    fn get_metadata(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<ArtifactMetadata>> {
        let meta_lock = self.metadata.lock().unwrap();
        let res = meta_lock.get(&artifact_id).cloned().ok_or(crate::ArtifactError::NotFound(artifact_id));
        async move { res }.boxed()
    }

    fn list_for_run(&self, run_id: Uuid) -> BoxFuture<'_, Result<Vec<ArtifactMetadata>>> {
        let meta_lock = self.metadata.lock().unwrap();
        let list: Vec<ArtifactMetadata> = meta_lock.values()
            .filter(|m| m.run_id == run_id)
            .cloned()
            .collect();
        async move { Ok(list) }.boxed()
    }

    fn delete(&self, artifact_id: Uuid) -> BoxFuture<'_, Result<()>> {
        let mut data_lock = self.data.lock().unwrap();
        let mut meta_lock = self.metadata.lock().unwrap();
        data_lock.remove(&artifact_id);
        meta_lock.remove(&artifact_id);
        async move { Ok(()) }.boxed()
    }
}

impl Default for MockArtifactStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ArtifactKind;

    #[tokio::test]
    async fn test_mock_storage_save_get() {
        let storage = MockArtifactStorage::new();
        let run_id = Uuid::new_v4();
        let data = vec![1, 2, 3, 4];
        
        let meta = storage.save(run_id, ArtifactKind::Screenshot, data.clone(), "image/png".to_string()).await.unwrap();
        assert_eq!(meta.run_id, run_id);
        assert_eq!(meta.size_bytes, 4);
        
        let retrieved = storage.get(meta.id).await.unwrap();
        assert_eq!(retrieved, data);
        
        let list = storage.list_for_run(run_id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, meta.id);
        
        storage.delete(meta.id).await.unwrap();
        let result = storage.get(meta.id).await;
        assert!(result.is_err());
    }
}
