use crate::traits::StorageProvider;
use crate::types::*;
use orchyn_core::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sqlx::postgres::{PgPool, PgPoolOptions};

pub struct PostgresStorage {
    pool: PgPool,
    artifact_root: PathBuf,
}

impl PostgresStorage {
    pub async fn new(database_url: &str, artifact_root: PathBuf) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(database_url)
            .await
            .map_err(|e| orchyn_core::OrchynError::Storage(e.to_string()))?;
        
        Ok(Self { pool, artifact_root })
    }
}

pub struct Storage {
    inner: Arc<dyn StorageProvider>,
}

impl Storage {
    pub async fn new(database_url: &str, artifact_root: PathBuf) -> Result<Self> {
        let pg = PostgresStorage::new(database_url, artifact_root).await?;
        Ok(Self {
            inner: Arc::new(pg),
        })
    }
}

// Delegate methods to inner
impl Storage {
    pub async fn ensure_user(&self, email: &str) -> Result<Uuid> {
        self.inner.ensure_user(email).await
    }
    
    pub async fn create_project(&self, owner_id: Uuid, name: &str, source_url: &str) -> Result<Uuid> {
        self.inner.create_project(owner_id, name, source_url).await
    }
    
    pub async fn add_project_member(&self, project_id: Uuid, user_id: Uuid, role: &str) -> Result<()> {
        self.inner.add_project_member(project_id, user_id, role).await
    }
    
    pub async fn get_user_projects(&self, user_id: Uuid) -> Result<Vec<ProjectRow>> {
        self.inner.get_user_projects(user_id).await
    }
    
    pub async fn create_invitation(&self, project_id: Uuid, inviter_id: Uuid, invitee_email: &str) -> Result<Uuid> {
        self.inner.create_invitation(project_id, inviter_id, invitee_email).await
    }
    
    pub async fn accept_invitation(&self, invitation_id: Uuid, user_id: Uuid) -> Result<()> {
        self.inner.accept_invitation(invitation_id, user_id).await
    }
    
    pub async fn start_run(&self, project_id: Uuid) -> Result<Uuid> {
        self.inner.start_run(project_id).await
    }
    
    pub async fn update_run_status(&self, run_id: Uuid, status: &str) -> Result<()> {
        self.inner.update_run_status(run_id, status).await
    }
    
    pub async fn get_project_runs(&self, project_id: Uuid) -> Result<Vec<RunRow>> {
        self.inner.get_project_runs(project_id).await
    }
    
    pub async fn get_run(&self, run_id: Uuid) -> Result<RunRow> {
        self.inner.get_run(run_id).await
    }
    
    pub async fn update_run_diagnostics(&self, run_id: Uuid, events: Vec<orchyn_core::DiagnosticEvent>) -> Result<()> {
        self.inner.update_run_diagnostics(run_id, events).await
    }
    
    pub async fn save_screenshot(&self, run_id: Uuid, data: &[u8]) -> Result<Uuid> {
        self.inner.save_screenshot(run_id, data).await
    }
    
    pub fn artifact_dir(&self) -> &std::path::Path {
        self.inner.artifact_dir()
    }
}
