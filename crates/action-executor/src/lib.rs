use orchyn_core::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;

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
    fn screenshot(&mut self) -> impl Future<Output = Result<Vec<u8>>> + Send + '_;
    fn execute(&mut self, action: &Action) -> impl Future<Output = Result<()>> + Send + '_;
    fn reset(&mut self) -> impl Future<Output = Result<()>> + Send + '_;
    fn diagnostics(&mut self) -> impl Future<Output = Result<Vec<orchyn_core::DiagnosticEvent>>> + Send + '_;
    fn network_events(&mut self) -> impl Future<Output = Result<Vec<orchyn_core::NetworkEvent>>> + Send + '_;
    fn get_elements(&mut self) -> impl Future<Output = Result<Vec<serde_json::Value>>> + Send + '_;
    fn device_info(&self) -> DeviceInfo;
}
