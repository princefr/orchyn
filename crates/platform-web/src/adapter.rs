use futures::future::{BoxFuture, FutureExt};
use headless_chrome::{Browser, LaunchOptions};
use orchyn_action_executor::{Action, DeviceInfo, PlatformAdapter};
use orchyn_core::{DiagnosticEvent, NetworkEvent, Platform, Result};
use std::sync::{Arc, Mutex};
use tracing::info;

pub struct WebAdapter {
    _browser: Browser,
    tab: Arc<headless_chrome::Tab>,
    device_info: DeviceInfo,
    diagnostics: Arc<Mutex<Vec<DiagnosticEvent>>>,
    network_events: Arc<Mutex<Vec<NetworkEvent>>>,
    _response_bodies: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl std::fmt::Debug for WebAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebAdapter")
            .field("device_info", &self.device_info)
            .finish()
    }
}

impl WebAdapter {
    pub async fn new(url: &str) -> Result<Self> {
        info!("Initializing Headless Chrome WebAdapter for {}", url);

        let browser = Browser::new(
            LaunchOptions::default_builder()
                .headless(true)
                .build()
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?,
        )
        .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;

        let tab = browser
            .new_tab()
            .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;

        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let network_events = Arc::new(Mutex::new(Vec::new()));
        let response_bodies = Arc::new(Mutex::new(std::collections::HashMap::new()));

        tab.enable_log()
            .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;

        tab.navigate_to(url)
            .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
        tab.wait_until_navigated()
            .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;

        Ok(Self {
            _browser: browser,
            tab,
            device_info: DeviceInfo {
                platform: Platform::Web,
                screen_width: 1280,
                screen_height: 800,
            },
            diagnostics,
            network_events,
            _response_bodies: response_bodies,
        })
    }
}

impl PlatformAdapter for WebAdapter {
    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }

    #[tracing::instrument(skip(self))]
    fn screenshot(&mut self) -> BoxFuture<'_, Result<Vec<u8>>> {
        let tab = self.tab.clone();
        async move {
            use headless_chrome::protocol::cdp::Page;
            let png = tab
                .capture_screenshot(Page::CaptureScreenshotFormatOption::Png, None, None, true)
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
            Ok(png)
        }
        .boxed()
    }

    #[tracing::instrument(skip(self))]
    fn execute(&mut self, action: &Action) -> BoxFuture<'_, Result<()>> {
        let tab = self.tab.clone();
        let action = action.clone();
        async move {
            info!("Executing web action: {:?}", action);
            match action {
                Action::Tap { x, y } => {
                    let point = headless_chrome::browser::tab::point::Point {
                        x: x as f64,
                        y: y as f64,
                    };
                    tab.click_point(point)
                        .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                }
                Action::Type { text } => {
                    tab.type_str(&text)
                        .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                }
                Action::Wait { ms } => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(ms as u64)).await;
                }
                Action::Reset => {
                    tab.navigate_to("about:blank")
                        .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                }
            }
            Ok(())
        }
        .boxed()
    }

    fn diagnostics(&mut self) -> BoxFuture<'_, Result<Vec<DiagnosticEvent>>> {
        let diags = self.diagnostics.clone();
        async move {
            let mut lock = diags.lock().unwrap();
            let current = lock.clone();
            lock.clear();
            Ok(current)
        }
        .boxed()
    }

    fn network_events(&mut self) -> BoxFuture<'_, Result<Vec<NetworkEvent>>> {
        let net = self.network_events.clone();
        async move {
            let mut lock = net.lock().unwrap();
            let current = lock.clone();
            lock.clear();
            Ok(current)
        }
        .boxed()
    }

    fn get_elements(&mut self) -> BoxFuture<'_, Result<Vec<serde_json::Value>>> {
        async move { Ok(vec![]) }.boxed()
    }

    fn reset(&mut self) -> BoxFuture<'_, Result<()>> {
        let tab = self.tab.clone();
        async move {
            tab.navigate_to("about:blank")
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
            Ok(())
        }
        .boxed()
    }
}
