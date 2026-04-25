use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use orchyn_storage::Storage;
use std::env;
use std::sync::Arc;
use stripe::Webhook;
use tracing::{error, info};

pub async fn stripe_webhook(
    State(_storage): State<Arc<Storage>>,
    headers: HeaderMap,
    payload: Bytes,
) -> impl IntoResponse {
    let sig = headers
        .get("stripe-signature")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();

    let secret = env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_else(|_| "whsec_test".to_string());

    let event = Webhook::construct_event(
        std::str::from_utf8(&payload).unwrap_or_default(),
        sig,
        &secret,
    );

    let event = match event {
        Ok(e) => e,
        Err(e) => {
            error!("Webhook signature verification failed: {}", e);
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    match event.type_ {
        stripe::EventType::CheckoutSessionCompleted => {
            info!("Checkout session completed");
        }
        stripe::EventType::CustomerSubscriptionUpdated => {
            info!("Subscription updated");
        }
        stripe::EventType::CustomerSubscriptionDeleted => {
            info!("Subscription deleted");
        }
        _ => {
            info!("Unhandled event type: {:?}", event.type_);
        }
    }

    StatusCode::OK.into_response()
}
