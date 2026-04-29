use crate::auth::JwtAuth;
use axum::{
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use serde_json::Value;
use std::future::Future;
use std::sync::Arc;

// --- Trait for Auth Middleware (testable) ---

pub trait AuthMiddleware: Send + Sync {
    fn authenticate(
        &self,
        token: String,
    ) -> impl Future<Output = Result<AuthUser, AuthError>> + Send + '_;
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

impl AuthMiddleware for JwtAuth {
    fn authenticate(
        &self,
        token: String,
    ) -> impl Future<Output = Result<AuthUser, AuthError>> + Send + '_ {
        async move {
            let mut validation = Validation::default();
            validation.validate_exp = true;

            match decode::<Value>(
                &token,
                &DecodingKey::from_secret(self.secret.as_bytes()),
                &validation,
            ) {
                Ok(token_data) => {
                    let sub = token_data
                        .claims
                        .get("sub")
                        .and_then(|v| v.as_str())
                        .ok_or(AuthError::InvalidToken)?;

                    let user_id =
                        uuid::Uuid::parse_str(sub).map_err(|_| AuthError::InvalidToken)?;

                    let email = token_data
                        .claims
                        .get("email")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    Ok(AuthUser { user_id, email })
                }
                Err(e) => {
                    if matches!(e.kind(), jsonwebtoken::errors::ErrorKind::ExpiredSignature) {
                        Err(AuthError::ExpiredToken)
                    } else {
                        Err(AuthError::InvalidToken)
                    }
                }
            }
        }
    }
}

// --- Axum Extractor for AuthUser ---

pub struct RequireAuth;

impl<S> FromRequestParts<S> for AuthUser
where
    Arc<JwtAuth>: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AuthError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let state = state;
        let token = extract_token_from_headers(parts);
        let auth_state_future = State::<Arc<JwtAuth>>::from_request_parts(parts, &state);

        async move {
            let token = token.ok_or(AuthError::MissingToken)?;
            let State(auth_state) = auth_state_future
                .await
                .map_err(|_| AuthError::MissingToken)?;
            auth_state.authenticate(token).await
        }
    }
}

fn extract_token_from_headers(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}
