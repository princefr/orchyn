use crate::types::{ProjectRow, RunRow};
use orchyn_core::{OrchynError, Result};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct MockStorage {
    _projects: Arc<Mutex<HashMap<Uuid, (Uuid, String, String)>>>,
    _runs: Arc<Mutex<HashMap<Uuid, (Uuid, String)>>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            _projects: Arc::new(Mutex::new(HashMap::new())),
            _runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn ensure_user(&self, _email: &str) -> Result<Uuid> {
        Ok(Uuid::new_v4())
    }

    pub async fn create_project(
        &self,
        owner_id: Uuid,
        name: &str,
        source_url: &str,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let mut projects = self._projects.lock().unwrap();
        projects.insert(id, (owner_id, name.to_string(), source_url.to_string()));
        Ok(id)
    }

    pub async fn get_user_projects(&self, _user_id: Uuid) -> Result<Vec<ProjectRow>> {
        Ok(vec![])
    }

    pub async fn start_run(&self, project_id: Uuid) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let mut runs = self._runs.lock().unwrap();
        runs.insert(id, (project_id, "pending".to_string()));
        Ok(id)
    }

    pub async fn get_run(&self, run_id: Uuid) -> Result<RunRow> {
        let runs = self._runs.lock().unwrap();
        if let Some((project_id, status)) = runs.get(&run_id) {
            Ok(RunRow {
                id: run_id,
                project_id: *project_id,
                status: status.clone(),
                started_at: None,
                finished_at: None,
                diagnostics: serde_json::json!({}),
            })
        } else {
            Err(OrchynError::Internal("Run not found".to_string()))
        }
    }
}

impl Default for MockStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_storage_projects() {
        let storage = MockStorage::new();
        let user_id = Uuid::new_v4();
        
        let proj_id = storage.create_project(user_id, "Test Project", "http://test.com").await.unwrap();
        
        let run_id = storage.start_run(proj_id).await.unwrap();
        let run = storage.get_run(run_id).await.unwrap();
        assert_eq!(run.id, run_id);
        assert_eq!(run.project_id, proj_id);
        assert_eq!(run.status, "pending");
    }
}
