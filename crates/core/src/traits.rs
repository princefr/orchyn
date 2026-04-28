use crate::errors::OrchynError;
use crate::types::*;
use std::future::Future;

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

pub trait PlatformAdapter: Send + Sync {
    fn device_info(&self) -> DeviceInfo;
    fn screenshot(&mut self) -> impl Future<Output = Result<Vec<u8>>> + Send + '_;
    fn execute(&mut self, action: &Action) -> impl Future<Output = Result<()>> + Send + '_;
    fn diagnostics(&mut self) -> impl Future<Output = Result<Vec<DiagnosticEvent>>> + Send + '_;
    fn network_events(&mut self) -> impl Future<Output = Result<Vec<NetworkEvent>>> + Send + '_;
    fn get_elements(&mut self) -> impl Future<Output = Result<Vec<serde_json::Value>>> + Send + '_;
}

pub type Result<T> = std::result::Result<T, OrchynError>;

