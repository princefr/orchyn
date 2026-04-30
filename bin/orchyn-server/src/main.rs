use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl, basic::BasicClient};
use orchyn_api_server::{
    AppState, analytics::PostHogClient, auth::JwtAuth, build_router, build_schema,
};
use orchyn_core::OrchynError;
use orchyn_harness::events::HarnessEvent;
use orchyn_storage::Storage;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

#[tokio::main]
async fn main() -> std::result::Result<(), OrchynError> {
    tracing_subscriber::fmt::init();

    info!("Starting Orchyn API server...");

    let database_url = env::var("DATABASE_URL").map_err(|e| OrchynError::Internal(e.to_string()))?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .map_err(|e| OrchynError::Storage(e.to_string()))?;

    let storage = Arc::new(Storage::new(Arc::new(pool)));

    let analytics = Arc::new(PostHogClient::new().await);
    info!("PostHog analytics initialized.");

    let google_client_id =
        ClientId::new(env::var("GOOGLE_CLIENT_ID").map_err(|e| OrchynError::Auth(e.to_string()))?);
    let google_client_secret = ClientSecret::new(
        env::var("GOOGLE_CLIENT_SECRET").map_err(|e| OrchynError::Auth(e.to_string()))?,
    );
    let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
        .map_err(|e| OrchynError::Auth(e.to_string()))?;
    let token_url = TokenUrl::new("https://www.googleapis.com/oauth2/v4/token".to_string())
        .map_err(|e| OrchynError::Auth(e.to_string()))?;

    let client = BasicClient::new(
        google_client_id,
        Some(google_client_secret),
        auth_url,
        Some(token_url),
    )
    .set_redirect_uri(
        RedirectUrl::new("http://localhost:3000/auth/google/callback".to_string())
            .map_err(|e| OrchynError::Auth(e.to_string()))?,
    );

    let jwt_secret =
        env::var("JWT_SECRET").unwrap_or_else(|_| "orchyn-secret-key-12345".to_string());

    let auth = Arc::new(JwtAuth::new(jwt_secret));

    let oauth_state = Arc::new(orchyn_api_server::OAuthState {
        client,
        storage: storage.clone(),
        jwt_auth: auth.clone(),
    });

    let schema = build_schema();

    // Create broadcast channel for SSE streaming
    let (event_tx, _) = broadcast::channel::<HarnessEvent>(100);

    let state = AppState {
        schema,
        auth,
        storage,
        analytics,
        event_tx,
    };

    let app = build_router(state, oauth_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await
        .map_err(|e| OrchynError::Internal(e.to_string()))?;
    info!("Server listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.map_err(|e| OrchynError::Internal(e.to_string()))?;

    Ok(())
}
