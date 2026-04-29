use crate::auth::JwtAuth;
use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Redirect},
};
use jsonwebtoken::{EncodingKey, Header, encode};
use oauth2::{
    AuthorizationCode, Scope, TokenResponse, basic::BasicClient, reqwest::async_http_client,
};
use orchyn_storage::Storage;
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

#[derive(Deserialize)]
pub struct AuthRequest {
    pub code: String,
    pub state: String,
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    email: String,
    name: Option<String>,
    picture: Option<String>,
}

#[derive(Clone)]
pub struct OAuthState {
    pub client: BasicClient,
    pub storage: Arc<Storage>,
    pub jwt_auth: Arc<JwtAuth>,
}

pub async fn google_login(State(state): State<Arc<OAuthState>>) -> impl IntoResponse {
    let (auth_url, _csrf_token) = state
        .client
        .authorize_url(oauth2::CsrfToken::new_random)
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .url();

    Redirect::to(auth_url.as_str())
}

pub async fn google_callback(
    State(state): State<Arc<OAuthState>>,
    Query(query): Query<AuthRequest>,
) -> impl IntoResponse {
    let token_result = state
        .client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(async_http_client)
        .await;

    match token_result {
        Ok(token) => {
            // Get user info from Google
            let client = reqwest::Client::new();
            let user_info: GoogleUserInfo = client
                .get("https://www.googleapis.com/oauth2/v3/userinfo")
                .bearer_auth(token.access_token().secret())
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();

            info!("Authenticated user: {}", user_info.email);

            // Create or find user in database
            let user_id = match state.storage.ensure_user(&user_info.email).await {
                Ok(id) => id,
                Err(e) => {
                    tracing::error!("Failed to ensure user: {}", e);
                    return Json(serde_json::json!({"error": "internal_error"})).into_response();
                }
            };

            // Create JWT token using JwtAuth
            let now = chrono::Utc::now().timestamp();
            let claims = serde_json::json!({
                "sub": user_id.to_string(),
                "email": user_info.email,
                "exp": now + 24 * 3600, // 24 hours
                "iat": now,
            });

            let header = Header::default();
            let jwt = match encode(
                &header,
                &claims,
                &EncodingKey::from_secret(state.jwt_auth.secret.as_bytes()),
            ) {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("JWT encoding failed: {}", e);
                    return Json(serde_json::json!({"error": "internal_error"})).into_response();
                }
            };

            // Redirect to frontend with token
            let frontend_url = std::env::var("FRONTEND_URL")
                .unwrap_or_else(|_| "http://localhost:5173".to_string());
            let redirect_url = format!("{}?token={}", frontend_url, jwt);
            Redirect::to(&redirect_url).into_response()
        }
        Err(e) => {
            tracing::error!("OAuth token exchange failed: {}", e);
            Json(serde_json::json!({"error": "auth_failed"})).into_response()
        }
    }
}
