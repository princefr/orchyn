use posthog_rs::{Client, Event, client};
use std::env;

pub struct PostHogClient {
    client: Client,
}

impl PostHogClient {
    pub async fn new() -> Self {
        let api_key = env::var("POSTHOG_API_KEY").unwrap_or_else(|_| "placeholder".to_string());

        Self {
            client: client(api_key.as_str()).await,
        }
    }

    pub fn capture_user_login(&self, user_id: &str, email: &str) {
        let mut event = Event::new("user_login", user_id);
        let _ = event.insert_prop("email", email);
        let _ = self.client.capture(event);
    }

    pub fn capture_checkout_completed(&self, user_id: &str, plan: &str) {
        let mut event = Event::new("checkout_completed", user_id);
        let _ = event.insert_prop("plan", plan);
        let _ = self.client.capture(event);
    }

    pub fn capture_scan_triggered(&self, user_id: &str, project_name: &str) {
        let mut event = Event::new("scan_triggered", user_id);
        let _ = event.insert_prop("project", project_name);
        let _ = self.client.capture(event);
    }
}
