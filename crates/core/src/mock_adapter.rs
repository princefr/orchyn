use crate::traits::{Action, DeviceInfo, PlatformAdapter, Result};
use crate::types::{DiagnosticEvent, NetworkEvent, Platform};
use futures::future::{BoxFuture, FutureExt};

#[derive(Debug)]
pub struct MockAdapter {
    pub device_info: DeviceInfo,
}

impl MockAdapter {
    pub fn new() -> Self {
        Self {
            device_info: DeviceInfo {
                platform: Platform::Web,
                screen_width: 1920,
                screen_height: 1080,
            },
        }
    }
}

impl PlatformAdapter for MockAdapter {
    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }

    fn screenshot(&mut self) -> BoxFuture<'_, Result<Vec<u8>>> {
        async move { Ok(vec![0, 1, 2, 3]) }.boxed()
    }

    fn execute(&mut self, _action: &Action) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }

    fn reset(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }

    fn diagnostics(&mut self) -> BoxFuture<'_, Result<Vec<DiagnosticEvent>>> {
        async move { Ok(vec![]) }.boxed()
    }

    fn network_events(&mut self) -> BoxFuture<'_, Result<Vec<NetworkEvent>>> {
        async move { Ok(vec![]) }.boxed()
    }

    fn get_elements(&mut self) -> BoxFuture<'_, Result<Vec<serde_json::Value>>> {
        async move { Ok(vec![]) }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_adapter_screenshot() {
        let mut adapter = MockAdapter::new();
        let screenshot = adapter.screenshot().await.unwrap();
        assert_eq!(screenshot, vec![0, 1, 2, 3]);
    }
}
