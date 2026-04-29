use chrono::{DateTime, Utc};
use orchyn_core::{DiagnosticEvent, OrchynError, Result};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::path::PathBuf;
use tracing::info;
use uuid::Uuid;
use crate::traits::StorageProvider;
use crate::types::ProjectRow;

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
        
        let tables = vec![
            ("users", "id UUID PRIMARY KEY, email TEXT UNIQUE NOT NULL, stripe_customer_id TEXT, subscription_tier TEXT NOT NULL, is_active BOOLEAN DEFAULT TRUE, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP"),
            ("projects", "id UUID PRIMARY KEY, owner_id UUID NOT NULL REFERENCES users(id), name TEXT NOT NULL, source_url TEXT NOT NULL, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP"),
            ("project_members", "project_id UUID NOT NULL REFERENCES projects(id), user_id UUID NOT NULL REFERENCES users(id), role TEXT NOT NULL, joined_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, PRIMARY KEY(project_id, user_id)"),
            ("invitations", "id UUID PRIMARY KEY, project_id UUID NOT NULL REFERENCES projects(id), inviter_id UUID NOT NULL REFERENCES users(id), invitee_email TEXT NOT NULL, status TEXT NOT NULL, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP"),
            ("runs", "id UUID PRIMARY KEY, project_id UUID NOT NULL REFERENCES projects(id), status TEXT NOT NULL, started_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, finished_at TIMESTAMPTZ, diagnostics JSONB DEFAULT '[]'"),
            ("artifacts", "id UUID PRIMARY KEY, run_id UUID NOT NULL REFERENCES runs(id), kind TEXT NOT NULL, local_path TEXT NOT NULL, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP"),
        ];
        
        for (table_name, definition) in tables {
            info!("Creating table {} if not exists...", table_name);
            let query = format!("CREATE TABLE IF NOT EXISTS {} ({})", table_name, definition);
            
            match sqlx::query(&query).execute(&self.pool).await {
                Ok(_) => info!("Table {} created/exists", table_name),
                Err(e) => {
                    warn!("Failed to create table {}: {}", table_name, e);
                    return Err(OrchynError::Storage(e.to_string()));
                }
            }
        }
        
        info!("Schema initialization complete.");
        Ok(())
    }
    
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
    
    pub fn artifact_dir(&self) -> &std::path::Path {
        &self.artifact_root
    }
}

impl StorageProvider for PostgresStorage {
    // User Methods
    fn ensure_user(&self, email: &str) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        let email = email.to_string();
        Box::new(async move {
            let row = sqlx::query("SELECT id FROM users WHERE email = $1")
                .bind(&email)
                .fetch_optional(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            if let Some(r) = row {
                Ok(r.get("id"))
            } else {
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO users (id, email, subscription_tier) VALUES ($1, $2, $3)")
                    .bind(id)
                    .bind(&email)
                    .bind("Free")
                    .execute(&pool)
                    .await
                    .map_err(|e| OrchynError::Storage(e.to_string()))?;
                Ok(id)
            }
        })
    }
    
    // Project Methods
    fn create_project(
        &self,
        owner_id: Uuid,
        name: &str,
        source_url: &str,
    ) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        let name = name.to_string();
        let source_url = source_url.to_string();
        Box::new(async move {
            // Check subscription tier and project count
            let user_pools = PostgresStorage { pool: pool.clone(), artifact_root: std::path::PathBuf::new() };
            let tier = user_pools.get_subscription_tier(owner_id).await?;
            let project_count = user_pools.get_user_project_count(owner_id).await?;
            
            if tier == "Free" && project_count >= 3 {
                return Err(OrchynError::Storage(
                    "Free tier limited to 3 projects. Please upgrade to Pro.".to_string()
                ));
            }
            
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO projects (id, owner_id, name, source_url) VALUES ($1, $2, $3, $4)",
            )
                .bind(id)
                .bind(owner_id)
                .bind(&name)
                .bind(&source_url)
                .execute(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;

            let add_member = PostgresStorage { pool: pool.clone(), artifact_root: std::path::PathBuf::new() };
            add_member.add_project_member(id, owner_id, "Owner").await?;

            Ok(id)
        })
    }

    fn add_project_member(
        &self,
        project_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        let role = role.to_string();
        Box::new(async move {
            // Check subscription tier and team member count
            let tier_row = sqlx::query("SELECT subscription_tier FROM users WHERE id = (SELECT owner_id FROM projects WHERE id = $1)")
                .bind(project_id)
                .fetch_one(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            let tier: String = tier_row.get("subscription_tier");
            
            if tier == "Pro" {
                let member_count_row = sqlx::query("SELECT COUNT(*) as count FROM project_members WHERE project_id = $1")
                    .bind(project_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|e| OrchynError::Storage(e.to_string()))?;
                
                let count: i64 = member_count_row.get("count");
                // Pro tier with $9/member - no hardcoded limit, but check if we should enforce billing
                // For now, just log
                tracing::info!("Pro tier team member count: {}", count);
            }
            
            sqlx::query(
                "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
                .bind(project_id)
                .bind(user_id)
                .bind(&role)
                .execute(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(())
        })
    }
    
    fn add_project_member(
        &self,
        project_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        let role = role.to_string();
        Box::new(async move {
            sqlx::query(
                "INSERT INTO project_members (project_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
            )
            .bind(project_id)
            .bind(user_id)
            .bind(&role)
            .execute(&pool).await.map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(())
        })
    }
    
    fn get_user_projects(&self, user_id: Uuid) -> Box<dyn Future<Output = Result<Vec<ProjectRow>, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let rows = sqlx::query(
                "SELECT p.id, p.name, p.source_url, p.created_at 
                 FROM projects p 
                 JOIN project_members pm ON p.id = pm.project_id 
                 WHERE pm.user_id = $1",
            )
            .bind(user_id)
            .fetch_all(&pool)
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
        })
    }
    
    // Invitation Methods
    fn create_invitation(
        &self,
        project_id: Uuid,
        inviter_id: Uuid,
        invitee_email: &str,
    ) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        let invitee_email = invitee_email.to_string();
        Box::new(async move {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO invitations (id, project_id, inviter_id, invitee_email, status) VALUES ($1, $2, $3, $4, $5)"
            )
            .bind(id)
            .bind(project_id)
            .bind(inviter_id)
            .bind(&invitee_email)
            .bind("Pending")
            .execute(&pool).await.map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(id)
        })
    }
    
    fn accept_invitation(&self, invitation_id: Uuid, user_id: Uuid) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let row =
                sqlx::query("SELECT project_id FROM invitations WHERE id = $1 AND status = 'Pending'")
                    .bind(invitation_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            let project_id: Uuid = row.get("project_id");
            
            let add_member = PostgresStorage { pool: pool.clone(), artifact_root: std::path::PathBuf::new() };
            add_member.add_project_member(project_id, user_id, "Member")
                .await?;
            
            sqlx::query("UPDATE invitations SET status = 'Accepted' WHERE id = $1")
                .bind(invitation_id)
                .execute(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            Ok(())
        })
    }
    
    fn get_user_project_count(
        &self,
        user_id: Uuid,
    ) -> Box<dyn Future<Output = std::result::Result<i64, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let row = sqlx::query!("SELECT COUNT(*) as count FROM projects WHERE owner_id = $1", user_id)
                .fetch_one(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            Ok(row.get::<i64, _>("count").unwrap_or(0))
        })
    }

    fn get_subscription_tier(
        &self,
        user_id: Uuid,
    ) -> Box<dyn Future<Output = std::result::Result<String>, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let row = sqlx::query!("SELECT subscription_tier FROM users WHERE id = $1", user_id)
                .fetch_one(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            Ok(row.get::<String, _>("subscription_tier").unwrap_or_else(|_| "Free".to_string()))
        })
    }

    fn get_team_member_count(
        &self,
        project_id: Uuid,
    ) -> Box<dyn Future<Output = std::result::Result<i64, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let row = sqlx::query!(
                "SELECT COUNT(*) as count FROM project_members WHERE project_id = $1",
                project_id
            )
                .fetch_one(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            Ok(row.get::<i64, _>("count").unwrap_or(0))
        })
    }

    // Run & Artifact Methods
    fn start_run(&self, project_id: Uuid) -> Box<dyn Future<Output = std::result::Result<Uuid, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO runs (id, project_id, status) VALUES ($1, $2, 'Running')")
                .bind(id)
                .bind(project_id)
                .execute(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(id)
        })
    }
    
    fn update_run_status(&self, run_id: Uuid, status: &str) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        let status = status.to_string();
        Box::new(async move {
            sqlx::query("UPDATE runs SET status = $2 WHERE id = $1")
                .bind(run_id)
                .bind(&status)
                .execute(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(())
        })
    }
    
    fn get_project_runs(&self, project_id: Uuid) -> Box<dyn Future<Output = Result<Vec<crate::types::RunRow>, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let rows = sqlx::query("SELECT id, project_id, status, started_at, finished_at, diagnostics FROM runs WHERE project_id = $1")
                .bind(project_id)
                .fetch_all(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            let runs = rows
                .into_iter()
                .map(|r| crate::types::RunRow {
                    id: r.get("id"),
                    project_id: r.get("project_id"),
                    status: r.get("status"),
                    started_at: r.get("started_at"),
                    finished_at: r.get("finished_at"),
                    diagnostics: r.get("diagnostics"),
                })
                .collect();
            
            Ok(runs)
        })
    }
    
    fn get_run(&self, run_id: Uuid) -> Box<dyn Future<Output = Result<crate::types::RunRow, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let row = sqlx::query("SELECT id, project_id, status, started_at, finished_at, diagnostics FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            Ok(crate::types::RunRow {
                id: row.get("id"),
                project_id: row.get("project_id"),
                status: row.get("status"),
                started_at: row.get("started_at"),
                finished_at: row.get("finished_at"),
                diagnostics: row.get("diagnostics"),
            })
        })
    }
    
    fn update_run_diagnostics(&self, run_id: Uuid, events: Vec<DiagnosticEvent>) -> Box<dyn Future<Output = Result<(), OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        Box::new(async move {
            let json_events =
                serde_json::to_value(events).map_err(|e| OrchynError::Internal(e.to_string()))?;
            sqlx::query("UPDATE runs SET diagnostics = diagnostics || $2 WHERE id = $1")
                .bind(run_id)
                .bind(json_events)
                .execute(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            Ok(())
        })
    }
    
    fn save_screenshot(&self, run_id: Uuid, data: &[u8]) -> Box<dyn Future<Output = Result<Uuid, OrchynError>> + Send + '_> {
        let pool = self.pool.clone();
        let artifact_root = self.artifact_root.clone();
        let data = data.to_vec();
        Box::new(async move {
            let id = Uuid::new_v4();
            let filename = format!("{}.png", id);
            let path = artifact_root.join(&filename);
            
            std::fs::write(&path, &data).map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            sqlx::query("INSERT INTO artifacts (id, run_id, kind, local_path) VALUES ($1, $2, $3, $4)")
                .bind(id)
                .bind(run_id)
                .bind("Screenshot")
                .bind(path.to_string_lossy().to_string())
                .execute(&pool)
                .await
                .map_err(|e| OrchynError::Storage(e.to_string()))?;
            
            Ok(id)
        })
    }
    
    fn artifact_dir(&self) -> &std::path::Path {
        &self.artifact_root
    }
}
