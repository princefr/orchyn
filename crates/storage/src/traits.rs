use crate::types::*;
use std::future::Future;
use uuid::Uuid;

pub trait StorageProvider: Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;
}

pub trait StorageProviderInner: Send + Sync {
    fn ensure_user(
        &self,
        email: &str,
    ) -> impl Future<Output = std::result::Result<Uuid, orchyn_core::OrchynError>> + Send;

    fn create_project(
        &self,
        owner_id: Uuid,
        name: &str,
        source_url: &str,
    ) -> impl Future<Output = std::result::Result<Uuid, orchyn_core::OrchynError>> + Send;

    fn add_project_member(
        &self,
        project_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> impl Future<Output = std::result::Result<(), orchyn_core::OrchynError>> + Send;

    fn get_user_projects(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = std::result::Result<Vec<ProjectRow>, orchyn_core::OrchynError>> + Send;

    fn create_invitation(
        &self,
        project_id: Uuid,
        inviter_id: Uuid,
        invitee_email: &str,
    ) -> impl Future<Output = std::result::Result<Uuid, orchyn_core::OrchynError>> + Send;

    fn accept_invitation(
        &self,
        invitation_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = std::result::Result<(), orchyn_core::OrchynError>> + Send;

    fn start_run(
        &self,
        project_id: Uuid,
    ) -> impl Future<Output = std::result::Result<Uuid, orchyn_core::OrchynError>> + Send;

    fn update_run_status(
        &self,
        run_id: Uuid,
        status: &str,
    ) -> impl Future<Output = std::result::Result<(), orchyn_core::OrchynError>> + Send;

    fn get_project_runs(
        &self,
        project_id: Uuid,
    ) -> impl Future<
        Output = std::result::Result<Vec<crate::types::RunRow>, orchyn_core::OrchynError>,
    > + Send;

    fn get_run(
        &self,
        run_id: Uuid,
    ) -> impl Future<Output = std::result::Result<crate::types::RunRow, orchyn_core::OrchynError>> + Send;

    fn update_run_diagnostics(
        &self,
        run_id: Uuid,
        events: Vec<orchyn_core::DiagnosticEvent>,
    ) -> impl Future<Output = std::result::Result<(), orchyn_core::OrchynError>> + Send;

    fn save_screenshot(
        &self,
        run_id: Uuid,
        data: &[u8],
    ) -> impl Future<Output = std::result::Result<Uuid, orchyn_core::OrchynError>> + Send;

    fn artifact_dir(&self) -> &std::path::Path;
}
