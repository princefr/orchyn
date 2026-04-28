use orchyn_core::{DiagnosticEvent, OrchynError, Result};
use std::future::Future;
use uuid::Uuid;
use crate::types::*;

pub trait StorageProvider: Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;
}

pub trait StorageProviderInner: Send + Sync {
    // User Methods
    fn ensure_user(&self, email: &str) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_>;
    
    // Project Methods
    fn create_project(&self, owner_id: Uuid, name: &str, source_url: &str) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_>;
    fn add_project_member(&self, project_id: Uuid, user_id: Uuid, role: &str) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_>;
    fn get_user_projects(&self, user_id: Uuid) -> Box<dyn Future<Output = Result<Vec<ProjectRow>, OrchynError>> + Send + '_>;
    
    // Invitation Methods
    fn create_invitation(&self, project_id: Uuid, inviter_id: Uuid, invitee_email: &str) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_>;
    fn accept_invitation(&self, invitation_id: Uuid, user_id: Uuid) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_>;
    
    // Run & Artifact Methods
    fn start_run(&self, project_id: Uuid) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_>;
    fn update_run_status(&self, run_id: Uuid, status: &str) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_>;
    fn get_project_runs(&self, project_id: Uuid) -> Box<dyn Future<Output = Result<Vec<RunRow>, OrchynError>> + Send + '_>;
    fn get_run(&self, run_id: Uuid) -> Box<dyn Future<Output = Result<RunRow, OrchynError>> + Send + '_>;
    fn update_run_diagnostics(&self, run_id: Uuid, events: Vec<DiagnosticEvent>) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_>;
    fn save_screenshot(&self, run_id: Uuid, data: &[u8]) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_>;
    fn artifact_dir(&self) -> &std::path::Path;
}
