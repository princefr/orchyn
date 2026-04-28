use crate::OrchynSchema;
use crate::auth::AuthState;
use crate::analytics::PostHogClient;
use orchyn_storage::Storage;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub schema: OrchynSchema,
    pub auth: Arc<AuthState>,
    pub storage: Arc<Storage>,
    pub analytics: Arc<PostHogClient>,
    pub event_sender: tokio::sync::broadcast::Sender<crate::HarnessEvent>,
}

impl axum::extract::FromRef<AppState> for OrchynSchema {
    fn from_ref(state: &AppState) -> Self {
        state.schema.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<AuthState> {
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

impl axum::extract::FromRef<AppState> for tokio::sync::broadcast::Sender<crate::HarnessEvent> {
    fn from_ref(state: &AppState) -> Self {
        state.event_sender.clone()
    }
}
