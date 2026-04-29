use orchyn_storage::traits::StorageProvider;
use orchyn_artifact_storage::ArtifactStorage;
use orchyn_harness::OrchynHarness;
use orchyn_harness::types::HarnessConfig;
use std::sync::Arc;

// Mock Storage for testing
struct MockStorage;

impl StorageProvider for MockStorage {
    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl orchyn_storage::traits::StorageProviderInner for MockStorage {
    fn ensure_user(&self, _email: &str) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        Box::new(async { Ok(Uuid::new_v4()) })
    }
    fn create_project(&self, _owner_id: Uuid, _name: &str, _source_url: &str) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        Box::new(async { Ok(Uuid::new_v4()) })
    }
    fn add_project_member(&self, _project_id: Uuid, _user_id: Uuid, _role: &str) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        Box::new(async { Ok(()) })
    }
    fn get_user_projects(&self, _user_id: Uuid) -> Box<dyn Future<Output = Result<Vec<ProjectRow>, OrchynError>> + Send + '_> {
        Box::new(async { Ok(vec![]) })
    }
    fn create_invitation(&self, _project_id: Uuid, _inviter_id: Uuid, _invitee_email: &str) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        Box::new(async { Ok(Uuid::new_v4()) })
    }
    fn accept_invitation(&self, _invitation_id: Uuid, _user_id: Uuid) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        Box::new(async { Ok(()) })
    }
    fn start_run(&self, _project_id: Uuid) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        Box::new(async { Ok(Uuid::new_v4()) })
    }
    fn update_run_status(&self, _run_id: Uuid, _status: &str) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        Box::new(async { Ok(()) })
    }
    fn get_project_runs(&self, _project_id: Uuid) -> Box<dyn Future<Output = Result<Vec<RunRow>, OrchynError>> + Send + '_> {
        Box::new(async { Ok(vec![]) })
    }
    fn get_run(&self, _run_id: Uuid) -> Box<dyn Future<Output = Result<RunRow, OrchynError>> + Send + '_> {
        Box::new(async { Ok(RunRow {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            status: "Running".to_string(),
            started_at: None,
            finished_at: None,
            diagnostics: serde_json::json!([]),
        })})
    }
    fn update_run_diagnostics(&self, _run_id: Uuid, _events: Vec<DiagnosticEvent>) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        Box::new(async { Ok(()) })
    }
    fn save_screenshot(&self, _run_id: Uuid, _data: &[u8]) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        Box::new(async { Ok(Uuid::new_v4()) })
    }
    fn artifact_dir(&self) -> &std::path::Path {
        std::path::Path::new("/tmp")
    }
}

// Mock Artifact Storage for testing  
struct MockArtifactStorage;

impl ArtifactStorage for MockArtifactStorage {
    fn save(&self, _run_id: Uuid, _kind: ArtifactKind, _data: &[u8], _content_type: &str) -> Box<dyn Future<Output = Result<ArtifactMetadata, crate::ArtifactError>> + Send + '_> {
        Box::new(async { Ok(ArtifactMetadata {
            id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            kind: ArtifactKind::Screenshot,
            created_at: chrono::Utc::now(),
            size_bytes: 0,
            content_type: "image/png".to_string(),
        })})
    }
    fn get(&self, _artifact_id: Uuid) -> Box<dyn Future<Output = Result<Vec<u8>, crate::ArtifactError>> + Send + '_> {
        Box::new(async { Ok(vec![]) })
    }
    fn get_metadata(&self, _artifact_id: Uuid) -> Box<dyn Future<Output = Result<ArtifactMetadata, crate::ArtifactError>> + Send + '_> {
        Box::new(async { Ok(ArtifactMetadata {
            id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            kind: ArtifactKind::Screenshot,
            created_at: chrono::Utc::now(),
            size_bytes: 0,
            content_type: "image/png".to_string(),
        })})
    }
    fn list_for_run(&self, _run_id: Uuid) -> Box<dyn Future<Output = Result<Vec<ArtifactMetadata>, crate::ArtifactError>> + Send + '_> {
        Box::new(async { Ok(vec![]) })
    }
    fn delete(&self, _artifact_id: Uuid) -> Box<dyn Future<Output = Result<(), crate::ArtifactError>> + Send + '_> {
        Box::new(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_harness_with_mocks() {
        // This demonstrates the MODULAR + TESTABLE design
        // We can test the harness WITHOUT PostgreSQL, S3, or any external dependencies
        
        let storage = Arc::new(MockStorage);
        let artifact_storage = Arc::new(MockArtifactStorage);
        let config = HarnessConfig {
            target_url: "http://example.com".to_string(),
            max_fix_iterations: 5,
        };
        
        let harness = OrchynHarness::new(storage, artifact_storage, config);
        
        // Test that the harness can run with mocks
        let project_id = Uuid::new_v4();
        let result = harness.run(project_id).await;
        
        assert!(result.is_ok());
        println!("✓ Harness ran successfully with mocks!");
    }
}
