pub mod middleware;
mod oauth;

pub use middleware::{AuthMiddleware, AuthUser, AuthError, JwtAuth, RequireAuth};

// Re-export for backward compatibility
pub use oauth::*;
