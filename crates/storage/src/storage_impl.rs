use crate::types::{ProjectRow, RunRow};
use orchyn_core::{OrchynError, Result};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

pub struct Storage {
    pub db: Arc<sqlx::PgPool>,
}

impl Storage {
    pub fn new(db: Arc<sqlx::PgPool>) -> Self {
        Self { db }
    }

    pub async fn ensure_user(&self, email: &str) -> Result<Uuid> {
        let user_id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO users (id, email) VALUES ($1, $2) ON CONFLICT (email) DO UPDATE SET email = EXCLUDED.email RETURNING id",
        )
        .bind(user_id)
        .bind(email)
        .fetch_one(&*self.db)
        .await
        .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(row.get::<Uuid, _>("id"))
    }

    pub async fn create_project(
        &self,
        owner_id: Uuid,
        name: &str,
        source_url: &str,
    ) -> Result<Uuid> {
        let project_id = Uuid::new_v4();
        sqlx::query("INSERT INTO projects (id, owner_id, name, source_url) VALUES ($1, $2, $3, $4)")
            .bind(project_id)
            .bind(owner_id)
            .bind(name)
            .bind(source_url)
            .execute(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(project_id)
    }

    pub async fn add_project_member(
        &self,
        project_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Result<()> {
        sqlx::query("INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3)")
            .bind(project_id)
            .bind(user_id)
            .bind(role)
            .execute(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(())
    }

    pub async fn get_user_projects(&self, user_id: Uuid) -> Result<Vec<ProjectRow>> {
        let rows = sqlx::query("SELECT id, name, source_url, created_at FROM projects WHERE owner_id = $1")
            .bind(user_id)
            .fetch_all(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| ProjectRow {
                id: r.get("id"),
                name: r.get("name"),
                source_url: r.get("source_url"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn create_invitation(
        &self,
        project_id: Uuid,
        inviter_id: Uuid,
        invitee_email: &str,
    ) -> Result<Uuid> {
        let invitation_id = Uuid::new_v4();
        sqlx::query("INSERT INTO invitations (id, project_id, inviter_id, invitee_email) VALUES ($1, $2, $3, $4)")
            .bind(invitation_id)
            .bind(project_id)
            .bind(inviter_id)
            .bind(invitee_email)
            .execute(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(invitation_id)
    }

    pub async fn accept_invitation(&self, invitation_id: Uuid, user_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE invitations SET status = 'accepted', accepted_by = $1 WHERE id = $2")
            .bind(user_id)
            .bind(invitation_id)
            .execute(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(())
    }

    pub async fn start_run(&self, project_id: Uuid) -> Result<Uuid> {
        let run_id = Uuid::new_v4();
        sqlx::query("INSERT INTO runs (id, project_id, status) VALUES ($1, $2, 'pending')")
            .bind(run_id)
            .bind(project_id)
            .execute(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(run_id)
    }

    pub async fn update_run_status(&self, run_id: Uuid, status: &str) -> Result<()> {
        sqlx::query("UPDATE runs SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(run_id)
            .execute(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(())
    }

    pub async fn get_project_runs(&self, project_id: Uuid) -> Result<Vec<RunRow>> {
        let rows = sqlx::query("SELECT id, project_id, status, started_at, finished_at, diagnostics FROM runs WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|r| RunRow {
                id: r.get("id"),
                project_id: r.get("project_id"),
                status: r.get("status"),
                started_at: r.get("started_at"),
                finished_at: r.get("finished_at"),
                diagnostics: r.get("diagnostics"),
            })
            .collect())
    }

    pub async fn get_run(&self, run_id: Uuid) -> Result<RunRow> {
        let r = sqlx::query("SELECT id, project_id, status, started_at, finished_at, diagnostics FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(RunRow {
            id: r.get("id"),
            project_id: r.get("project_id"),
            status: r.get("status"),
            started_at: r.get("started_at"),
            finished_at: r.get("finished_at"),
            diagnostics: r.get("diagnostics"),
        })
    }

    pub async fn update_run_diagnostics(
        &self,
        run_id: Uuid,
        events: Vec<orchyn_core::DiagnosticEvent>,
    ) -> Result<()> {
        let events_json = serde_json::to_value(events).unwrap_or(serde_json::json!([]));
        sqlx::query("UPDATE runs SET diagnostics = $1 WHERE id = $2")
            .bind(events_json)
            .bind(run_id)
            .execute(&*self.db)
            .await
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        Ok(())
    }

    pub async fn save_screenshot(&self, _run_id: Uuid, _data: &[u8]) -> Result<Uuid> {
        Ok(Uuid::new_v4())
    }
}
