use crate::errors::OrchynError;
use crate::types::*;

use futures::future::{BoxFuture, FutureExt};

pub trait StorageProvider: Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;
}

#[derive(Debug, Clone)]
pub enum Action {
    Tap { x: u32, y: u32 },
    Type { text: String },
    Wait { ms: u32 },
    Reset,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub platform: Platform,
    pub screen_width: u32,
    pub screen_height: u32,
}

use std::fmt::Debug;

pub trait PlatformAdapter: Send + Sync + Debug {
    fn device_info(&self) -> DeviceInfo;
    fn screenshot(&mut self) -> BoxFuture<'_, Result<Vec<u8>>>;
    fn execute(&mut self, action: &Action) -> BoxFuture<'_, Result<()>>;
    fn reset(&mut self) -> BoxFuture<'_, Result<()>>;
    fn diagnostics(&mut self) -> BoxFuture<'_, Result<Vec<DiagnosticEvent>>>;
    fn network_events(&mut self) -> BoxFuture<'_, Result<Vec<NetworkEvent>>>;
    fn get_elements(&mut self) -> BoxFuture<'_, Result<Vec<serde_json::Value>>>;

    // === Hybrid harness extensions (Constraint + Agent modes) ===
    //
    // Default impls return safe empty values so existing adapters that haven't
    // been migrated still compile. Real adapters (e.g. WebAdapter) override.

    fn scrape_elements(&mut self) -> BoxFuture<'_, Result<Vec<UiElement>>> {
        async move { Ok(vec![]) }.boxed()
    }

    fn current_url(&mut self) -> BoxFuture<'_, Result<String>> {
        async move { Ok(String::new()) }.boxed()
    }

    fn collect_telemetry(&mut self) -> BoxFuture<'_, Result<RawTelemetry>> {
        async move { Ok(RawTelemetry::default()) }.boxed()
    }

    fn execute_harness(
        &mut self,
        action: &crate::action::Action,
        element_map: &[crate::action::AnnotatedElement],
    ) -> BoxFuture<'_, Result<()>> {
        let _ = (action, element_map);
        async move {
            Err(OrchynError::Platform(
                "execute_harness not implemented for this adapter".to_string(),
            ))
        }
        .boxed()
    }

    fn shutdown(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }

    /// Evaluate a JavaScript expression and return the result as a JSON string.
    fn eval_js(&mut self, js: &str) -> BoxFuture<'_, Result<String>> {
        let _ = js;
        async move {
            Err(OrchynError::Platform(
                "eval_js not implemented for this adapter".to_string(),
            ))
        }
        .boxed()
    }

    /// Resize the browser viewport (for responsive testing).
    fn set_viewport(&mut self, width: u32, height: u32) -> BoxFuture<'_, Result<()>> {
        let _ = (width, height);
        async move {
            Err(OrchynError::Platform(
                "set_viewport not implemented for this adapter".to_string(),
            ))
        }
        .boxed()
    }
}

pub type Result<T> = std::result::Result<T, OrchynError>;
