use crate::OrchynSchema;
use crate::analytics::PostHogClient;
use crate::auth::JwtAuth;
use orchyn_harness::events::HarnessEvent;
use orchyn_storage::Storage;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub schema: OrchynSchema,
    pub auth: Arc<JwtAuth>,
    pub storage: Arc<Storage>,
    pub analytics: Arc<PostHogClient>,
    pub event_tx: broadcast::Sender<HarnessEvent>, // For SSE streaming
}

impl axum::extract::FromRef<AppState> for OrchynSchema {
    fn from_ref(state: &AppState) -> Self {
        state.schema.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<JwtAuth> {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<Storage> {
    fn from_ref(state: &AppState) -> Self {
        state.storage.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<PostHogClient> {
    fn from_ref(state: &AppState) -> Self {
        state.analytics.clone()
    }
}

impl axum::extract::FromRef<AppState> for broadcast::Sender<HarnessEvent> {
    fn from_ref(state: &AppState) -> Self {
        state.event_tx.clone()
    }
}
