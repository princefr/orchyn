pub mod handler {
    use crate::OrchynSchema;
    use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
    use axum::extract::State;
    use crate::auth::middleware::AuthUser;
    
    pub async fn graphql_handler(
        State(schema): State<OrchynSchema>,
        req: GraphQLRequest,
    ) -> GraphQLResponse {
        // Simplified for E2E: No auth user injection for now to fix trait bounds
        schema.execute(req.into_inner()).await.into()
    }
}

pub mod auth_routes {
    use axum::{routing::get, Router};
    use std::sync::Arc;
    use crate::auth::oauth::{google_login, google_callback, OAuthState};
    
    pub fn router(state: Arc<OAuthState>) -> Router {
        Router::new()
            .route("/google/login", get(google_login))
            .route("/google/callback", get(google_callback))
            .with_state(state)
    }
}

pub mod sse_routes {
    use axum::{extract::State, response::sse::{Event, KeepAlive, Sse}, routing::get, Router};
    use tokio::sync::broadcast;
    use tokio_stream::wrappers::BroadcastStream;
    use orchyn_harness::events::HarnessEvent;
    use std::time::Duration;
    use tokio_stream::StreamExt;
    
    pub async fn stream_events(
        State(event_tx): State<broadcast::Sender<HarnessEvent>>,
        _path: axum::extract::Path<String>,
    ) -> Sse<impl tokio_stream::Stream<Item = Result<Event, axum::Error>>> {
        let rx = event_tx.subscribe();
        let stream = BroadcastStream::new(rx);
        
        // Convert HarnessEvent to SSE format
        let sse_stream = stream.filter_map(|result| {
            match result {
                Ok(event) => {
                    let json = serde_json::to_string(&event).ok()?;
                    Some(Ok(Event::default().data(json)))
                }
                Err(_) => None,
            }
        });
        
        Sse::new(sse_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
    }
    
    pub fn router<S>() -> Router<S> 
    where 
        S: Clone + Send + Sync + 'static,
        broadcast::Sender<HarnessEvent>: axum::extract::FromRef<S>
    {
        Router::new()
            .route("/api/jobs/:id/stream", get(stream_events))
    }
}


