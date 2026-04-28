use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::Utc;
use std::collections::HashMap;
use crate::traits::*;
use crate::error::*;
use crate::ArtifactKind;

pub struct MockArtifactStorage {
    artifacts: Arc<Mutex<HashMap<Uuid, (ArtifactKind, Vec<u8>, Uuid)>>>,
}

impl MockArtifactStorage {
    pub fn new() -> Self {
        Self {
            artifacts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ArtifactStorage for MockArtifactStorage {
    fn save(
        &self,
        run_id: Uuid,
        kind: ArtifactKind,
        data: &[u8],
        _content_type: &str,
    ) -> impl Future<Output = Result<ArtifactMetadata, ArtifactError>> + Send + '_ {
        let artifacts = self.artifacts.clone();
        let data = data.to_vec();
        async move {
            let id = Uuid::new_v4();
            let mut artifacts = artifacts.lock().await;
            
            artifacts.insert(id, (kind.clone(), data.clone(), run_id));
            
            Ok(ArtifactMetadata {
                id,
                run_id,
                kind,
                created_at: Utc::now(),
                size_bytes: data.len() as u64,
                content_type: "application/octet-stream".to_string(),
            })
        }
    }

    fn get(&self, artifact_id: Uuid) -> impl Future<Output = Result<Vec<u8>, ArtifactError>> + Send + '_ {
        let artifacts = self.artifacts.clone();
        async move {
            let artifacts = artifacts.lock().await;
            
            match artifacts.get(&artifact_id) {
                Some((_, data, _)) => Ok(data.clone()),
                None => Err(ArtifactError::NotFound(format!("Artifact {} not found", artifact_id))),
            }
        }
    }

    fn get_metadata(&self, artifact_id: Uuid) -> impl Future<Output = Result<ArtifactMetadata, ArtifactError>> + Send + '_ {
        let artifacts = self.artifacts.clone();
        async move {
            let artifacts = artifacts.lock().await;
            
            match artifacts.get(&artifact_id) {
                Some((kind, data, run_id)) => Ok(ArtifactMetadata {
                    id: artifact_id,
                    run_id: *run_id,
                    kind: kind.clone(),
                    created_at: Utc::now(),
                    size_bytes: data.len() as u64,
                    content_type: "application/octet-stream".to_string(),
                }),
                None => Err(ArtifactError::NotFound(format!("Artifact {} not found", artifact_id))),
            }
        }
    }

    fn list_for_run(&self, run_id: Uuid) -> impl Future<Output = Result<Vec<ArtifactMetadata>, ArtifactError>> + Send + '_ {
        let artifacts = self.artifacts.clone();
        async move {
            let artifacts = artifacts.lock().await;
            
            let mut result = vec![];
            for (id, (kind, data, rid)) in artifacts.iter() {
                if *rid == run_id {
                    result.push(ArtifactMetadata {
                        id: *id,
                        run_id: *rid,
                        kind: kind.clone(),
                        created_at: Utc::now(),
                        size_bytes: data.len() as u64,
                        content_type: "application/octet-stream".to_string(),
                    });
                }
            }
            
            Ok(result)
        }
    }

    fn delete(&self, artifact_id: Uuid) -> impl Future<Output = Result<(), ArtifactError>> + Send + '_ {
        let artifacts = self.artifacts.clone();
        async move {
            let mut artifacts = artifacts.lock().await;
            
            if artifacts.remove(&artifact_id).is_some() {
                Ok(())
            } else {
                Err(ArtifactError::NotFound(format!("Artifact {} not found", artifact_id)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArtifactKind;

    #[tokio::test]
    async fn test_save_and_get() {
        let storage = MockArtifactStorage::new();
        let run_id = Uuid::new_v4();
        let data = b"test data";
        
        let metadata = storage.save(run_id, ArtifactKind::Screenshot, data, "image/png").await.unwrap();
        
        let retrieved = storage.get(metadata.id).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_delete() {
        let storage = MockArtifactStorage::new();
        let run_id = Uuid::new_v4();
        let data = b"test data";
        
        let metadata = storage.save(run_id, ArtifactKind::Screenshot, data, "image/png").await.unwrap();
        
        storage.delete(metadata.id).await.unwrap();
        
        let result = storage.get(metadata.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_for_run() {
        let storage = MockArtifactStorage::new();
        let run_id = Uuid::new_v4();
        
        storage.save(run_id, ArtifactKind::Screenshot, b"data1", "image/png").await.unwrap();
        storage.save(run_id, ArtifactKind::Video, b"data2", "video/mp4").await.unwrap();
        
        let artifacts = storage.list_for_run(run_id).await.unwrap();
        assert_eq!(artifacts.len(), 2);
    }
}


impl MockArtifactStorage {
    pub fn new() -> Self {
        Self {
            artifacts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ArtifactStorage for MockArtifactStorage {
    async fn save(
        &self,
        run_id: Uuid,
        kind: ArtifactKind,
        data: &[u8],
        _content_type: &str,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        let id = Uuid::new_v4();
        let mut artifacts = self.artifacts.lock().await;
        
        artifacts.insert(id, (kind.clone(), data.to_vec(), run_id));
        
        Ok(ArtifactMetadata {
            id,
            run_id,
            kind,
            created_at: chrono::Utc::now(),
            size_bytes: data.len() as u64,
            content_type: "application/octet-stream".to_string(),
        })
    }

    async fn get(&self, artifact_id: Uuid) -> Result<Vec<u8>, ArtifactError> {
        let artifacts = self.artifacts.lock().await;
        
        match artifacts.get(&artifact_id) {
            Some((_, data, _)) => Ok(data.clone()),
            None => Err(ArtifactError::NotFound(format!("Artifact {} not found", artifact_id))),
        }
    }

    async fn get_metadata(&self, artifact_id: Uuid) -> Result<ArtifactMetadata, ArtifactError> {
        let artifacts = self.artifacts.lock().await;
        
        match artifacts.get(&artifact_id) {
            Some((kind, data, run_id)) => Ok(ArtifactMetadata {
                id: artifact_id,
                run_id: *run_id,
                kind: kind.clone(),
                created_at: chrono::Utc::now(),
                size_bytes: data.len() as u64,
                content_type: "application/octet-stream".to_string(),
            }),
            None => Err(ArtifactError::NotFound(format!("Artifact {} not found", artifact_id))),
        }
    }

    async fn list_for_run(&self, run_id: Uuid) -> Result<Vec<ArtifactMetadata>, ArtifactError> {
        let artifacts = self.artifacts.lock().await;
        
        let mut result = vec![];
        for (id, (kind, data, rid)) in artifacts.iter() {
            if *rid == run_id {
                result.push(ArtifactMetadata {
                    id: *id,
                    run_id: *rid,
                    kind: kind.clone(),
                    created_at: chrono::Utc::now(),
                    size_bytes: data.len() as u64,
                    content_type: "application/octet-stream".to_string(),
                });
            }
        }
        
        Ok(result)
    }

    async fn delete(&self, artifact_id: Uuid) -> Result<(), ArtifactError> {
        let mut artifacts = self.artifacts.lock().await;
        
        if artifacts.remove(&artifact_id).is_some() {
            Ok(())
        } else {
            Err(ArtifactError::NotFound(format!("Artifact {} not found", artifact_id)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArtifactKind;

    #[tokio::test]
    async fn test_save_and_get() {
        let storage = MockArtifactStorage::new();
        let run_id = Uuid::new_v4();
        let data = b"test data";
        
        let metadata = storage.save(run_id, ArtifactKind::Screenshot, data, "image/png").await.unwrap();
        
        let retrieved = storage.get(metadata.id).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_delete() {
        let storage = MockArtifactStorage::new();
        let run_id = Uuid::new_v4();
        let data = b"test data";
        
        let metadata = storage.save(run_id, ArtifactKind::Screenshot, data, "image/png").await.unwrap();
        
        storage.delete(metadata.id).await.unwrap();
        
        let result = storage.get(metadata.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_for_run() {
        let storage = MockArtifactStorage::new();
        let run_id = Uuid::new_v4();
        
        storage.save(run_id, ArtifactKind::Screenshot, b"data1", "image/png").await.unwrap();
        storage.save(run_id, ArtifactKind::Video, b"data2", "video/mp4").await.unwrap();
        
        let artifacts = storage.list_for_run(run_id).await.unwrap();
        assert_eq!(artifacts.len(), 2);
    }
}
