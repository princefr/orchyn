use headless_chrome::{Browser, LaunchOptions};
use orchyn_action_executor::{Action, DeviceInfo, PlatformAdapter};
use orchyn_core::{DiagnosticEvent, NetworkEvent, Platform, Result};
use std::future::Future;
use std::sync::{Arc, Mutex};
use tracing::info;

pub struct WebAdapter {
    browser: Browser,
    tab: Arc<headless_chrome::Tab>,
    device_info: DeviceInfo,
    diagnostics: Arc<Mutex<Vec<DiagnosticEvent>>>,
    network_events: Arc<Mutex<Vec<NetworkEvent>>>,
    response_bodies: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl WebAdapter {
    pub async fn new(url: &str) -> Result<Self> {
        info!("Initializing Headless Chrome WebAdapter for {}", url);
        
        let browser = Browser::new(
            LaunchOptions::default_builder()
                .headless(true)
                .build()
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?
        ).map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
        
        let tab = browser.new_tab()
            .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
        
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let network_events = Arc::new(Mutex::new(Vec::new()));
        let response_bodies = Arc::new(Mutex::new(std::collections::HashMap::new()));
        
        tab.enable_log().map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
        
        tab.navigate_to(url).map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
        tab.wait_until_navigated().map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
        
        Ok(Self {
            browser,
            tab,
            device_info: DeviceInfo {
                platform: Platform::Web,
                screen_width: 1280,
                screen_height: 800,
            },
            diagnostics,
            network_events,
            response_bodies,
        })
    }
}

impl PlatformAdapter for WebAdapter {
    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }
    
    fn screenshot(&mut self) -> impl Future<Output = Result<Vec<u8>>> + Send + '_ {
        let tab = self.tab.clone();
        async move {
            use headless_chrome::protocol::cdp::Page;
            let png = tab.capture_screenshot(Page::CaptureScreenshotFormatOption::Png, None, None, true)
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
            Ok(png)
        }
    }
    
    fn execute(&mut self, action: &Action) -> impl Future<Output = Result<()>> + Send + '_ {
        let tab = self.tab.clone();
        let action = action.clone();
        async move {
            match action {
                Action::Tap { x, y } => {
                    let point = headless_chrome::browser::tab::point::Point { x: x as f64, y: y as f64 };
                    tab.click_point(point)
                        .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                }
                Action::Type { text } => {
                    tab.type_str(&text)
                        .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                }
                _ => {}
            }
            Ok(())
        }
    }
    
    fn diagnostics(&mut self) -> impl Future<Output = Result<Vec<DiagnosticEvent>>> + Send + '_ {
        let diags = self.diagnostics.clone();
        async move {
            let mut lock = diags.lock().unwrap();
            let current = lock.clone();
            lock.clear();
            Ok(current)
        }
    }
    
    fn network_events(&mut self) -> impl Future<Output = Result<Vec<NetworkEvent>>> + Send + '_ {
        let net = self.network_events.clone();
        async move {
            let mut lock = net.lock().unwrap();
            let current = lock.clone();
            lock.clear();
            Ok(current)
        }
    }
    
    fn get_elements(&mut self) -> impl Future<Output = Result<Vec<serde_json::Value>>> + Send + '_ {
        async move {
            Ok(vec![])
        }
    }

    fn reset(&mut self) -> impl Future<Output = Result<()>> + Send + '_ {
        let tab = self.tab.clone();
        async move {
            tab.navigate_to("about:blank").map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
            Ok(())
        }
    }
}
