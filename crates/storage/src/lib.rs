use chrono::{DateTime, Utc};
use orchyn_core::{DiagnosticEvent, OrchynError, Result};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::path::PathBuf;
use tracing::info;
use uuid::Uuid;

pub struct Storage {
    pool: PgPool,
    artifact_root: PathBuf,
}

impl Storage {
    pub async fn new(database_url: &str, artifact_root: PathBuf) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(database_url)
            .await
            .map_err(|e| OrchynError::Storage(e.to_string()))?;

        let storage = Self {
            pool,
            artifact_root,
        };
        storage.init_schema().await?;

        if !storage.artifact_root.exists() {
            std::fs::create_dir_all(&storage.artifact_root).map_err(|e| {
                OrchynError::Storage(format!("Failed to create artifact dir: {}", e))
            })?;
        }

        Ok(storage)
    }

    async fn init_schema(&self) -> Result<()> {
        info!("Initializing PostgreSQL schema...");

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                id UUID PRIMARY KEY,
                email TEXT UNIQUE NOT NULL,
                stripe_customer_id TEXT,
                subscription_tier TEXT NOT NULL,
                is_active BOOLEAN DEFAULT TRUE,
                created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS projects (
                id UUID PRIMARY KEY,
                owner_id UUID NOT NULL REFERENCES users(id),
                name TEXT NOT NULL,
                source_url TEXT NOT NULL,
                created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS project_members (
                project_id UUID NOT NULL REFERENCES projects(id),
                user_id UUID NOT NULL REFERENCES users(id),
                role TEXT NOT NULL,
                joined_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY(project_id, user_id)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS invitations (
                id UUID PRIMARY KEY,
                project_id UUID NOT NULL REFERENCES projects(id),
                inviter_id UUID NOT NULL REFERENCES users(id),
                invitee_email TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS runs (
                id UUID PRIMARY KEY,
                project_id UUID NOT NULL REFERENCES projects(id),
                status TEXT NOT NULL,
                started_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
                diagnostics JSONB DEFAULT '[]'
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS artifacts (
                id UUID PRIMARY KEY,
                run_id UUID NOT NULL REFERENCES runs(id),
                kind TEXT NOT NULL,
                local_path TEXT NOT NULL,
                created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

        Ok(())
    }

    // User Methods
    pub async fn ensure_user(&self, email: &str) -> Result<Uuid> {
        let row = sqlx::query("SELECT id FROM users WHERE email = $1")
            .bind(email)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| OrchynError::Storage(e.to_string()))?;

        if let Some(r) = row {
            Ok(r.get("id"))
        } else {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO users (id, email, subscription_tier) VALUES ($1, $2, $3)")
                .bind(id)
                .bind(email)
                .bind("Free")
                .execute(&self.pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(id)
        }
    }

    // Project Methods
    pub async fn create_project(
        &self,
        owner_id: Uuid,
        name: &str,
        source_url: &str,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects (id, owner_id, name, source_url) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(owner_id)
        .bind(name)
        .bind(source_url)
        .execute(&self.pool)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

        self.add_project_member(id, owner_id, "Owner").await?;

        Ok(id)
    }

    pub async fn add_project_member(
        &self,
        project_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
        )
        .bind(project_id)
        .bind(user_id)
        .bind(role)
        .execute(&self.pool).await.map_err(|e| OrchynError::Storage(e.to_string()))?;
        Ok(())
    }

    pub async fn get_user_projects(&self, user_id: Uuid) -> Result<Vec<ProjectRow>> {
        let rows = sqlx::query(
            "SELECT p.id, p.name, p.source_url, p.created_at 
             FROM projects p 
             JOIN project_members pm ON p.id = pm.project_id 
             WHERE pm.user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

        let projects = rows
            .into_iter()
            .map(|r| ProjectRow {
                id: r.get("id"),
                name: r.get("name"),
                source_url: r.get("source_url"),
                created_at: r.get("created_at"),
            })
            .collect();

        Ok(projects)
    }

    // Invitation Methods
    pub async fn create_invitation(
        &self,
        project_id: Uuid,
        inviter_id: Uuid,
        invitee_email: &str,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO invitations (id, project_id, inviter_id, invitee_email, status) VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(id)
        .bind(project_id)
        .bind(inviter_id)
        .bind(invitee_email)
        .bind("Pending")
        .execute(&self.pool).await.map_err(|e| OrchynError::Storage(e.to_string()))?;
        Ok(id)
    }

    pub async fn accept_invitation(&self, invitation_id: Uuid, user_id: Uuid) -> Result<()> {
        let row =
            sqlx::query("SELECT project_id FROM invitations WHERE id = $1 AND status = 'Pending'")
                .bind(invitation_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;

        let project_id: Uuid = row.get("project_id");

        self.add_project_member(project_id, user_id, "Member")
            .await?;

        sqlx::query("UPDATE invitations SET status = 'Accepted' WHERE id = $1")
            .bind(invitation_id)
            .execute(&self.pool)
            .await
            .map_err(|e| OrchynError::Storage(e.to_string()))?;

        Ok(())
    }

    // Run & Artifact Methods
    pub async fn start_run(&self, project_id: Uuid) -> Result<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO runs (id, project_id, status) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(project_id)
            .bind("Running")
            .execute(&self.pool)
            .await
            .map_err(|e| OrchynError::Storage(e.to_string()))?;
        Ok(id)
    }

    pub async fn update_run_diagnostics(
        &self,
        run_id: Uuid,
        events: Vec<DiagnosticEvent>,
    ) -> Result<()> {
        let json_events =
            serde_json::to_value(events).map_err(|e| OrchynError::Internal(e.to_string()))?;
        sqlx::query("UPDATE runs SET diagnostics = diagnostics || $2 WHERE id = $1")
            .bind(run_id)
            .bind(json_events)
            .execute(&self.pool)
            .await
            .map_err(|e| OrchynError::Storage(e.to_string()))?;
        Ok(())
    }

    pub async fn save_screenshot(&self, run_id: Uuid, data: &[u8]) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let filename = format!("{}.png", id);
        let path = self.artifact_root.join(&filename);

        std::fs::write(&path, data).map_err(|e| OrchynError::Storage(e.to_string()))?;

        sqlx::query("INSERT INTO artifacts (id, run_id, kind, local_path) VALUES ($1, $2, $3, $4)")
            .bind(id)
            .bind(run_id)
            .bind("Screenshot")
            .bind(path.to_string_lossy().to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| OrchynError::Storage(e.to_string()))?;

        Ok(id)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn artifact_dir(&self) -> &std::path::Path {
        &self.artifact_root
    }
}

pub struct ProjectRow {
    pub id: Uuid,
    pub name: String,
    pub source_url: String,
    pub created_at: Option<DateTime<Utc>>,
}
