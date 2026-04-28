use async_graphql::{Context, EmptySubscription, ID, Object, Schema, SimpleObject};
use chrono::{DateTime, Utc};
use orchyn_storage::{ProjectRow, RunRow, Storage};
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
pub struct Invitation {
    pub id: ID,
    pub project_name: String,
    pub inviter_email: String,
    pub status: String,
}

#[derive(SimpleObject)]
pub struct Run {
    pub id: ID,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub screenshots: Vec<Artifact>,
    pub diagnostics: Vec<DiagnosticLog>,
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
pub struct ChatMessage {
    pub id: ID,
    pub content: String,
    pub role: String,
}

#[derive(SimpleObject)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
}

// --- GraphQL Subscription Types ---

#[derive(SimpleObject)]
pub struct HarnessEvent {
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub data: serde_json::Value,
}
