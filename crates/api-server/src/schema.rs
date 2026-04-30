use async_graphql::{Context, EmptySubscription, ID, Object, Schema, SimpleObject};
use chrono::{DateTime, Utc};
use orchyn_storage::{ProjectRow, RunRow, Storage};
use crate::auth::middleware::AuthUser;
use std::sync::Arc;
use uuid::Uuid;

// --- GraphQL Object Types ---

#[derive(SimpleObject)]
pub struct User {
    pub id: ID,
    pub email: String,
    pub subscription_tier: String,
}

#[derive(SimpleObject)]
pub struct Project {
    pub id: ID,
    pub name: String,
    pub source_url: String,
    pub created_at: DateTime<Utc>,
}

impl From<ProjectRow> for Project {
    fn from(row: ProjectRow) -> Self {
        Self {
            id: ID::from(row.id),
            name: row.name,
            source_url: row.source_url,
            created_at: row.created_at.unwrap_or_else(Utc::now),
        }
    }
}

#[derive(SimpleObject)]
pub struct Run {
    pub id: ID,
    pub project_id: ID,
    pub status: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub screenshots: Vec<Artifact>,
    pub diagnostics: Vec<DiagnosticLog>,
}

impl From<RunRow> for Run {
    fn from(row: RunRow) -> Self {
        Self {
            id: ID::from(row.id),
            project_id: ID::from(row.project_id),
            status: row.status,
            started_at: row.started_at,
            finished_at: row.finished_at,
            screenshots: vec![],
            diagnostics: vec![],
        }
    }
}

#[derive(SimpleObject)]
pub struct DiagnosticLog {
    pub level: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(SimpleObject)]
pub struct Artifact {
    pub id: ID,
    pub kind: String,
    pub local_url: String,
}

#[derive(SimpleObject)]
pub struct CheckoutSession {
    pub url: String,
}

#[derive(SimpleObject)]
pub struct Invitation {
    pub id: ID,
    pub project_name: String,
    pub inviter_email: String,
    pub status: String,
}

#[derive(SimpleObject)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
}

// --- Query ---

pub struct Query;

#[Object]
impl Query {
    async fn health(&self) -> HealthStatus {
        HealthStatus {
            status: "ok".to_string(),
            version: "0.1.0".to_string(),
        }
    }

    async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<User> {
        let auth_user = ctx.data::<AuthUser>().map_err(|_| "Unauthorized")?;
        Ok(User {
            id: ID::from(auth_user.user_id),
            email: auth_user.email.clone(),
            subscription_tier: "pro".to_string(),
        })
    }

    async fn projects(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Project>> {
        let auth = ctx.data::<AuthUser>().map_err(|_| "Unauthorized")?;
        let storage = ctx.data::<Arc<Storage>>().map_err(|_| "Storage not available")?;
        
        let rows = storage.get_user_projects(auth.user_id).await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        
        Ok(rows.into_iter().map(Project::from).collect())
    }

    async fn job(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<Option<Run>> {
        let _auth = ctx.data::<AuthUser>().map_err(|_| "Unauthorized")?;
        let storage = ctx.data::<Arc<Storage>>().map_err(|_| "Storage not available")?;
        
        let uuid = Uuid::parse_str(&id.to_string()).map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let run = storage.get_run(uuid).await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        
        Ok(Some(Run::from(run)))
    }
}

// --- Mutation ---

pub struct Mutation;

#[Object]
impl Mutation {
    async fn create_project(
        &self,
        ctx: &Context<'_>,
        name: String,
        source_url: String,
        _framework: Option<String>,
    ) -> async_graphql::Result<ID> {
        let auth = ctx.data::<AuthUser>().map_err(|_| "Unauthorized")?;
        let storage = ctx.data::<Arc<Storage>>().map_err(|_| "Storage not available")?;
        
        let project_id = storage.create_project(auth.user_id, &name, &source_url).await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        
        Ok(ID::from(project_id))
    }

    async fn scan_repo(
        &self,
        ctx: &Context<'_>,
        project_id: ID,
        _skill: Option<String>,
    ) -> async_graphql::Result<ID> {
        let _auth = ctx.data::<AuthUser>().map_err(|_| "Unauthorized")?;
        let storage = ctx.data::<Arc<Storage>>().map_err(|_| "Storage not available")?;
        
        let uuid = Uuid::parse_str(&project_id.to_string()).map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let run_id = storage.start_run(uuid).await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        
        // TODO: Actually trigger the harness to run
        
        Ok(ID::from(run_id))
    }

    async fn invite_team_member(
        &self,
        ctx: &Context<'_>,
        project_id: ID,
        email: String,
        role: String,
    ) -> async_graphql::Result<String> {
        let auth = ctx.data::<AuthUser>().map_err(|_| "Unauthorized")?;
        let storage = ctx.data::<Arc<Storage>>().map_err(|_| "Storage not available")?;
        
        let project_uuid = Uuid::parse_str(&project_id.to_string()).map_err(|e| async_graphql::Error::new(e.to_string()))?;
        
        // Use create_invitation instead of invite_member
        storage.create_invitation(project_uuid, auth.user_id, &email).await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        
        Ok("invited".to_string())
    }

    async fn create_checkout_session(
        &self,
        ctx: &Context<'_>,
        _plan: String,
    ) -> async_graphql::Result<CheckoutSession> {
        let _auth = ctx.data::<AuthUser>().map_err(|_| "Unauthorized")?;
        
        // TODO: Implement Stripe checkout
        Ok(CheckoutSession {
            url: "https://checkout.stripe.com/dummy".to_string(),
        })
    }
}

pub type OrchynSchema = Schema<Query, Mutation, EmptySubscription>;
