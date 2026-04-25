pub mod analytics;
pub mod auth;
pub mod webhooks;

use async_graphql::{Context, EmptySubscription, ID, Object, Schema, SimpleObject};
use axum::extract::FromRef;
use chrono::{DateTime, Utc};
use orchyn_storage::{ProjectRow, Storage};
use std::sync::Arc;
use uuid::Uuid;

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

pub struct Query;

#[Object]
impl Query {
    async fn me(&self, _ctx: &Context<'_>) -> async_graphql::Result<User> {
        Ok(User {
            id: ID::from(Uuid::nil()),
            email: "test@example.com".to_string(),
            subscription_tier: "Pro".to_string(),
        })
    }

    async fn projects(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Project>> {
        let storage = ctx.data::<Arc<Storage>>()?;
        // For demo, use a fixed user id or get from auth context
        let user_id = Uuid::nil();
        let projects = storage.get_user_projects(user_id).await?;
        Ok(projects.into_iter().map(Project::from).collect())
    }

    async fn runs(&self, ctx: &Context<'_>, _project_id: ID) -> async_graphql::Result<Vec<Run>> {
        let _storage = ctx.data::<Arc<Storage>>()?;
        Ok(vec![])
    }
}

#[derive(SimpleObject)]
pub struct CheckoutSession {
    pub url: String,
}

pub struct Mutation;

#[Object]
impl Mutation {
    async fn create_project(
        &self,
        ctx: &Context<'_>,
        name: String,
        source_url: String,
    ) -> async_graphql::Result<Project> {
        let storage = ctx.data::<Arc<Storage>>()?;
        let user_id = Uuid::nil(); // Placeholder
        let id = storage.create_project(user_id, &name, &source_url).await?;

        Ok(Project {
            id: ID::from(id),
            name,
            source_url,
            created_at: Utc::now(),
        })
    }

    async fn invite_user(
        &self,
        ctx: &Context<'_>,
        project_id: ID,
        email: String,
    ) -> async_graphql::Result<bool> {
        let storage = ctx.data::<Arc<Storage>>()?;
        let user_id = Uuid::nil(); // Placeholder
        let pid = Uuid::parse_str(project_id.as_str())?;
        storage.create_invitation(pid, user_id, &email).await?;
        Ok(true)
    }

    async fn accept_invitation(
        &self,
        ctx: &Context<'_>,
        invitation_id: ID,
    ) -> async_graphql::Result<bool> {
        let storage = ctx.data::<Arc<Storage>>()?;
        let user_id = Uuid::nil(); // Placeholder
        let iid = Uuid::parse_str(invitation_id.as_str())?;
        storage.accept_invitation(iid, user_id).await?;
        Ok(true)
    }
}

pub type OrchynSchema = Schema<Query, Mutation, EmptySubscription>;

pub fn build_schema(
    storage: Arc<Storage>,
    analytics: Arc<analytics::PostHogClient>,
) -> OrchynSchema {
    Schema::build(Query, Mutation, EmptySubscription)
        .data(storage)
        .data(analytics)
        .finish()
}

#[derive(Clone)]
pub struct AppState {
    pub schema: OrchynSchema,
    pub auth: Arc<auth::AuthState>,
    pub storage: Arc<Storage>,
    pub analytics: Arc<analytics::PostHogClient>,
}

impl FromRef<AppState> for OrchynSchema {
    fn from_ref(state: &AppState) -> Self {
        state.schema.clone()
    }
}

impl FromRef<AppState> for Arc<auth::AuthState> {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}

impl FromRef<AppState> for Arc<Storage> {
    fn from_ref(state: &AppState) -> Self {
        state.storage.clone()
    }
}

impl FromRef<AppState> for Arc<analytics::PostHogClient> {
    fn from_ref(state: &AppState) -> Self {
        state.analytics.clone()
    }
}

pub mod handler {
    use crate::OrchynSchema;
    use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
    use axum::extract::State;

    pub async fn graphql_handler(
        State(schema): State<OrchynSchema>,
        req: GraphQLRequest,
    ) -> GraphQLResponse {
        schema.execute(req.into_inner()).await.into()
    }
}

pub fn build_router(state: AppState) -> axum::Router {
    use axum::http::Method;
    use axum::routing::{get, post};
    use tower_http::cors::{Any, CorsLayer};
    use tower_http::services::ServeDir;

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_origin(Any)
        .allow_headers(Any);

    let artifact_dir = state.storage.artifact_dir().to_path_buf();

    axum::Router::new()
        .route("/graphql", post(handler::graphql_handler))
        .route("/auth/google", get(auth::google_login))
        .route("/auth/google/callback", get(auth::google_callback))
        .route("/webhooks/stripe", post(webhooks::stripe_webhook))
        .nest_service("/artifacts", ServeDir::new(artifact_dir))
        .route("/", get(|| async { "Orchyn GraphQL API" }))
        .with_state(state)
        .layer(cors)
}
