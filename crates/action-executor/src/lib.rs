use orchyn_core::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Tap { x: u32, y: u32 },
    Type { text: String },
    Wait { ms: u32 },
    // ... other actions
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub platform: orchyn_core::Platform,
    pub screen_width: u32,
    pub screen_height: u32,
}

pub trait PlatformAdapter: Send + Sync {
    fn screenshot(&mut self) -> BoxFuture<'_, Result<Vec<u8>>>;
    fn execute(&mut self, action: &Action) -> BoxFuture<'_, Result<()>>;
    fn reset(&mut self) -> BoxFuture<'_, Result<()>>;
    fn diagnostics(&mut self) -> BoxFuture<'_, Result<Vec<orchyn_core::DiagnosticEvent>>>;
    fn get_elements(&mut self) -> BoxFuture<'_, Result<Vec<serde_json::Value>>>;
    fn device_info(&self) -> DeviceInfo;
}
