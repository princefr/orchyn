use crate::errors::OrchynError;
use crate::types::*;

use futures::future::BoxFuture;

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
}

pub type Result<T> = std::result::Result<T, OrchynError>;
