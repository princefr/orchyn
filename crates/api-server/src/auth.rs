use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect},
};
use oauth2::{AuthorizationCode, Scope, basic::BasicClient, reqwest::async_http_client};
use orchyn_storage::Storage;
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

#[derive(Deserialize)]
pub struct AuthRequest {
    code: String,
    state: String,
}

pub struct AuthState {
    pub client: BasicClient,
    pub storage: Arc<Storage>,
}

pub async fn google_login(State(state): State<Arc<AuthState>>) -> impl IntoResponse {
    let (auth_url, _csrf_token) = state
        .client
        .authorize_url(oauth2::CsrfToken::new_random)
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .url();

    Redirect::to(auth_url.as_str())
}

pub async fn google_callback(
    State(state): State<Arc<AuthState>>,
    Query(query): Query<AuthRequest>,
) -> impl IntoResponse {
    let _token_result = state
        .client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(async_http_client)
        .await
        .unwrap();

    // In a real app, you'd use the access token to get user info from Google
    // and then create or find the user in your database.
    info!("Successfully authenticated user with Google");

    // Placeholder redirect to dashboard
    Redirect::to("http://localhost:5173/dashboard")
}
