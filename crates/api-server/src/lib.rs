pub mod analytics;
pub mod auth;
pub mod routes;
pub mod schema;
pub mod state;
pub mod webhooks;

// Re-export main types for convenience
pub use analytics::PostHogClient;
pub use auth::middleware::{AuthMiddleware, AuthUser, RequireAuth};
pub use auth::{JwtAuth, OAuthState};
pub use schema::*;
pub use state::AppState;

use async_graphql::{EmptySubscription, Schema};
use axum::{Router, routing::post};
use tower_http::cors::{Any, CorsLayer};

pub fn build_schema() -> OrchynSchema {
    Schema::build(Query, Mutation, EmptySubscription).finish()
}

pub fn build_router(state: AppState, oauth_state: std::sync::Arc<auth::OAuthState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/graphql", post(routes::handler::graphql_handler))
        .nest("/auth", routes::auth_routes::router(oauth_state))
        .nest("/api", routes::sse_routes::router())
        .layer(cors)
        .with_state(state)
}

pub fn build_router(state: AppState, oauth_state: std::sync::Arc<OAuthState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/graphql", post(routes::handler::graphql_handler))
        .nest("/auth", routes::auth_routes::router(oauth_state))
        .layer(cors)
        .with_state(state)
}
