use std::future::Future;
use axum::{
    body::Body,
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde_json::Value;
use std::sync::Arc;

// --- Trait for Auth Middleware (testable) ---

pub trait AuthMiddleware: Send + Sync {
    fn authenticate(&self, token: &str) -> impl Future<Output = Result<AuthUser, AuthError>> + Send + '_;
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: uuid::Uuid,
    pub email: String,
}

#[derive(Debug)]
pub enum AuthError {
    MissingToken,
    InvalidToken,
    ExpiredToken,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::MissingToken => (StatusCode::UNAUTHORIZED, "Missing token"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid token"),
            AuthError::ExpiredToken => (StatusCode::UNAUTHORIZED, "Expired token"),
        };
        (status, message).into_response()
    }
}

// --- JWT Auth Implementation ---

pub struct JwtAuth {
    secret: String,
}

impl JwtAuth {
    pub fn new(secret: &str) -> Self {
        Self {
            secret: secret.to_string(),
        }
    }
}

impl AuthMiddleware for JwtAuth {
    fn authenticate(&self, token: &str) -> impl Future<Output = Result<AuthUser, AuthError>> + Send + '_ {
        async move {
            let mut validation = Validation::default();
            validation.validate_exp = true;

            match decode::<Value>(token, &DecodingKey::from_secret(self.secret.as_bytes()), &validation) {
                Ok(token_data) => {
                    let sub = token_data.claims.get("sub")
                        .and_then(|v| v.as_str())
                        .ok_or(AuthError::InvalidToken)?;
                    
                    let user_id = uuid::Uuid::parse_str(sub).map_err(|_| AuthError::InvalidToken)?;
                    
                    let email = token_data.claims.get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    
                    Ok(AuthUser { user_id, email })
                }
                Err(jsonwebtoken::errors::ErrorKind::ExpiredSignature) => Err(AuthError::ExpiredToken),
                Err(_) => Err(AuthError::InvalidToken),
            }
        }
    }
}

// --- Axum Extractor for AuthUser ---

pub struct RequireAuth;

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    fn from_request_parts(parts: &mut Parts, state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> + Send + 'static {
        async move {
            let auth_state: &Arc<JwtAuth> = State::from_request_parts(parts, state)
                .await
                .map_err(|_| AuthError::MissingToken)?;
            
            let token = extract_token_from_headers(parts)
                .ok_or(AuthError::MissingToken)?;
            
            auth_state.authenticate(&token).await
        }
    }
}

fn extract_token_from_headers(parts: &Parts) -> Option<String> {
    parts.headers.get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

// --- Mock for Testing ---

#[cfg(test)]
pub struct MockAuthMiddleware {
    pub should_succeed: bool,
    pub user_id: uuid::Uuid,
}

#[cfg(test)]
impl AuthMiddleware for MockAuthMiddleware {
    fn authenticate(&self, _token: &str) -> impl Future<Output = Result<AuthUser, AuthError>> + Send + '_ {
        async move {
            if self.should_succeed {
                Ok(AuthUser {
                    user_id: self.user_id,
                    email: "test@example.com".to_string(),
                })
            } else {
                Err(AuthError::InvalidToken)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_jwt_auth_valid_token() {
        let auth = JwtAuth::new("test-secret");
        let user_id = uuid::Uuid::new_v4();
        
        let claims = serde_json::json!({
            "sub": user_id.to_string(),
            "email": "test@example.com",
            "exp": (chrono::Utc::now().timestamp() + 3600),
            "iat": chrono::Utc::now().timestamp(),
        });
        
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret("test-secret".as_bytes()),
        ).unwrap();
        
        let result = auth.authenticate(&token).await;
        assert!(result.is_ok());
        let user = result.unwrap();
        assert_eq!(user.user_id, user_id);
        assert_eq!(user.email, "test@example.com");
    }

    #[tokio::test]
    async fn test_jwt_auth_expired_token() {
        let auth = JwtAuth::new("test-secret");
        
        let claims = serde_json::json!({
            "sub": uuid::Uuid::new_v4().to_string(),
            "exp": (chrono::Utc::now().timestamp() - 3600), // Expired
            "iat": chrono::Utc::now().timestamp() - 7200,
        });
        
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret("test-secret".as_bytes()),
        ).unwrap();
        
        let result = auth.authenticate(&token).await;
        assert!(matches!(result, Err(AuthError::ExpiredToken)));
    }

    #[tokio::test]
    async fn test_mock_auth_success() {
        let user_id = uuid::Uuid::new_v4();
        let auth = MockAuthMiddleware {
            should_succeed: true,
            user_id,
        };
        
        let result = auth.authenticate("any-token").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().user_id, user_id);
    }

    #[tokio::test]
    async fn test_mock_auth_failure() {
        let auth = MockAuthMiddleware {
            should_succeed: false,
            user_id: uuid::Uuid::new_v4(),
        };
        
        let result = auth.authenticate("any-token").await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }
}
