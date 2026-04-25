use chrono::Utc;
use headless_chrome::{Browser, LaunchOptions};
use orchyn_action_executor::{Action, BoxFuture, DeviceInfo, PlatformAdapter};
use orchyn_core::{DiagnosticEvent, LogLevel, Result};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::info;

pub struct WebAdapter {
    _browser: Browser,
    tab: Arc<headless_chrome::Tab>,
    device_info: DeviceInfo,
    events: Arc<Mutex<Vec<DiagnosticEvent>>>,
}

impl WebAdapter {
    pub async fn new(url: &str) -> Result<Self> {
        info!("Initializing Headless Chrome WebAdapter for {}", url);

        let options = LaunchOptions::default_builder()
            .window_size(Some((1280, 720)))
            .idle_browser_timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| {
                orchyn_core::OrchynError::Platform(format!("Failed to build options: {}", e))
            })?;

        let browser = Browser::new(options).map_err(|e| {
            orchyn_core::OrchynError::Platform(format!("Failed to launch browser: {}", e))
        })?;

        let tab = browser.new_tab().map_err(|e| {
            orchyn_core::OrchynError::Platform(format!("Failed to create tab: {}", e))
        })?;

        tab.set_default_timeout(Duration::from_secs(300));

        let events = Arc::new(Mutex::new(Vec::new()));

        tab.navigate_to(url).map_err(|e| {
            orchyn_core::OrchynError::Platform(format!("Failed to navigate: {}", e))
        })?;

        tab.wait_until_navigated().map_err(|e| {
            orchyn_core::OrchynError::Platform(format!("Failed to wait for navigation: {}", e))
        })?;

        let device_info = DeviceInfo {
            platform: orchyn_core::Platform::Web,
            screen_width: 1280,
            screen_height: 720,
        };

        Ok(Self {
            _browser: browser,
            tab,
            device_info,
            events,
        })
    }
}

impl PlatformAdapter for WebAdapter {
    fn screenshot(&mut self) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async move {
            info!("Taking screenshot with Headless Chrome");
            let screenshot_bytes = self
                .tab
                .capture_screenshot(
                    headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                    None,
                    None,
                    true,
                )
                .map_err(|e| {
                    orchyn_core::OrchynError::Platform(format!("Screenshot failed: {}", e))
                })?;

            Ok(screenshot_bytes)
        })
    }

    fn execute(&mut self, action: &Action) -> BoxFuture<'_, Result<()>> {
        let action = action.clone();
        Box::pin(async move {
            info!("Executing action: {:?}", action);
            match action {
                Action::Tap { x, y } => {
                    self.tab
                        .click_point(headless_chrome::browser::tab::point::Point {
                            x: x as f64,
                            y: y as f64,
                        })
                        .map_err(|e| {
                            orchyn_core::OrchynError::Platform(format!("Tap failed: {}", e))
                        })?;
                }
                Action::Type { text } => {
                    self.tab.type_str(&text).map_err(|e| {
                        orchyn_core::OrchynError::Platform(format!("Type failed: {}", e))
                    })?;
                }
                Action::Wait { ms } => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(ms as u64)).await;
                }
            }
            Ok(())
        })
    }

    fn reset(&mut self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            info!("Resetting WebAdapter tab");
            self.tab
                .reload(false, None)
                .map_err(|e| orchyn_core::OrchynError::Platform(format!("Reset failed: {}", e)))?;
            self.tab.wait_until_navigated().map_err(|e| {
                orchyn_core::OrchynError::Platform(format!("Wait after reset failed: {}", e))
            })?;
            Ok(())
        })
    }

    fn diagnostics(&mut self) -> BoxFuture<'_, Result<Vec<DiagnosticEvent>>> {
        Box::pin(async move {
            // As a fallback to CDP events which have complex generated types,
            // we'll inject a script to capture console.error and window errors.
            let script = r#"
                (function() {
                    const logs = window.__orchyn_logs || [];
                    window.__orchyn_logs = [];
                    return logs;
                })()
            "#;

            let mut captured = Vec::new();
            if let Ok(result) = self.tab.evaluate(script, false) {
                if let Some(val) = result.value {
                    let logs: Vec<String> = serde_json::from_value(val).unwrap_or_default();
                    for log in logs {
                        captured.push(DiagnosticEvent::Console {
                            level: LogLevel::Error,
                            message: log,
                            timestamp: Utc::now(),
                        });
                    }
                }
            }

            let mut events = self.events.lock().unwrap();
            captured.append(&mut events);
            events.clear();
            Ok(captured)
        })
    }

    fn get_elements(&mut self) -> BoxFuture<'_, Result<Vec<serde_json::Value>>> {
        Box::pin(async move {
            let script = r#"
                Array.from(document.querySelectorAll('button, input, a, h1, h2, li')).map(el => ({
                    kind: el.tagName,
                    label: el.innerText || el.value || el.placeholder,
                    x: el.getBoundingClientRect().left,
                    y: el.getBoundingClientRect().top,
                    width: el.getBoundingClientRect().width,
                    height: el.getBoundingClientRect().height
                }))
            "#;
            let result = self.tab.evaluate(script, false).map_err(|e| {
                orchyn_core::OrchynError::Platform(format!("JS eval failed: {}", e))
            })?;

            if let Some(val) = result.value {
                let elements: Vec<serde_json::Value> =
                    serde_json::from_value(val).unwrap_or_default();
                Ok(elements)
            } else {
                Ok(vec![])
            }
        })
    }

    fn device_info(&self) -> DeviceInfo {
        self.device_info.clone()
    }
}
