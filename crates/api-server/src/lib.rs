pub mod analytics;
pub mod auth;
pub mod webhooks;
pub mod schema;
pub mod routes;
pub mod state;

// Re-export main types for convenience
pub use schema::*;
pub use state::AppState;
pub use analytics::PostHogClient;
pub use auth::{OAuthState, JwtAuth};
pub use auth::middleware::{AuthUser, RequireAuth, AuthMiddleware};
