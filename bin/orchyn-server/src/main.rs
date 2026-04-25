use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl, basic::BasicClient};
use orchyn_api_server::{
    AppState, analytics::PostHogClient, auth::AuthState, build_router, build_schema,
};
use orchyn_storage::Storage;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting Orchyn API server...");

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let artifact_root = PathBuf::from(
        env::var("ARTIFACT_ROOT").unwrap_or_else(|_| "./storage/artifacts".to_string()),
    );

    let storage = Arc::new(
        Storage::new(&database_url, artifact_root)
            .await
            .expect("Failed to connect to PostgreSQL"),
    );

    let analytics = Arc::new(PostHogClient::new().await);
    info!("PostHog analytics initialized.");

    let google_client_id =
        ClientId::new(env::var("GOOGLE_CLIENT_ID").expect("GOOGLE_CLIENT_ID must be set"));
    let google_client_secret = ClientSecret::new(
        env::var("GOOGLE_CLIENT_SECRET").expect("GOOGLE_CLIENT_SECRET must be set"),
    );
    let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
        .expect("Invalid auth URL");
    let token_url = TokenUrl::new("https://www.googleapis.com/oauth2/v4/token".to_string())
        .expect("Invalid token URL");

    let client = BasicClient::new(
        google_client_id,
        Some(google_client_secret),
        auth_url,
        Some(token_url),
    )
    .set_redirect_uri(
        RedirectUrl::new("http://localhost:3000/auth/google/callback".to_string())
            .expect("Invalid redirect URL"),
    );

    let auth_state = Arc::new(AuthState {
        client,
        storage: storage.clone(),
    });

    let schema = build_schema(storage.clone(), analytics.clone());

    let state = AppState {
        schema,
        auth: auth_state,
        storage,
        analytics,
    };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    info!("Server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
