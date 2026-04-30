use futures::future::{BoxFuture, FutureExt};
use headless_chrome::{Browser, LaunchOptions};
use orchyn_action_executor::{Action, DeviceInfo, PlatformAdapter};
use orchyn_core::action::{Action as HarnessAction, AnnotatedElement, BoundingBox};
use orchyn_core::{
    ConsoleLog, DiagnosticEvent, LogLevel, NetworkEvent, NetworkRequest, OrchynError, Platform,
    RawTelemetry, Result, UiElement,
};
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

    // === Hybrid harness extensions ===

    /// Enumerates interactive UI elements via in-page JS and returns them in
    /// document order. Elements with zero area or off-screen are filtered out
    /// in JS so the caller only sees elements a real user could click.
    fn scrape_elements(&mut self) -> BoxFuture<'_, Result<Vec<UiElement>>> {
        let tab = self.tab.clone();
        async move {
            let remote = tab
                .evaluate(SCRAPE_ELEMENTS_JS, false)
                .map_err(|e| OrchynError::Platform(format!("scrape_elements: {e}")))?;
            let raw = remote
                .value
                .ok_or_else(|| OrchynError::Platform("scrape_elements: empty value".into()))?;
            let json_text = raw.as_str().ok_or_else(|| {
                OrchynError::Platform("scrape_elements: expected JSON string".into())
            })?;
            let parsed: Vec<RawElement> = serde_json::from_str(json_text)
                .map_err(|e| OrchynError::Platform(format!("scrape_elements parse: {e}")))?;
            Ok(parsed.into_iter().map(RawElement::into_ui).collect())
        }
        .boxed()
    }

    fn current_url(&mut self) -> BoxFuture<'_, Result<String>> {
        let tab = self.tab.clone();
        async move { Ok(tab.get_url()) }.boxed()
    }

    fn collect_telemetry(&mut self) -> BoxFuture<'_, Result<RawTelemetry>> {
        let tab = self.tab.clone();
        let diags = self.diagnostics.clone();
        let nets = self.network_events.clone();
        async move {
            let console_logs: Vec<ConsoleLog> = {
                let mut lock = diags.lock().unwrap();
                let drained: Vec<_> = lock.drain(..).collect();
                drained.into_iter().filter_map(diag_to_console).collect()
            };
            let network_requests: Vec<NetworkRequest> = {
                let mut lock = nets.lock().unwrap();
                let drained: Vec<_> = lock.drain(..).collect();
                drained.into_iter().map(net_to_request).collect()
            };
            Ok(RawTelemetry {
                console_logs,
                network_requests,
                ws_messages: vec![],
                current_url: tab.get_url(),
            })
        }
        .boxed()
    }

    fn execute_harness(
        &mut self,
        action: &HarnessAction,
        element_map: &[AnnotatedElement],
    ) -> BoxFuture<'_, Result<()>> {
        let tab = self.tab.clone();
        let action = action.clone();
        let element_map: Vec<AnnotatedElement> = element_map.to_vec();
        async move {
            match action {
                HarnessAction::Navigate { value } => {
                    tab.navigate_to(&value)
                        .map_err(|e| OrchynError::Platform(format!("navigate: {e}")))?;
                    tab.wait_until_navigated()
                        .map_err(|e| OrchynError::Platform(format!("wait_navigated: {e}")))?;
                }
                HarnessAction::Click { target } => {
                    let bb = lookup(&element_map, target)?;
                    click_center(&tab, &bb)?;
                }
                HarnessAction::Type { target, value } => {
                    let bb = lookup(&element_map, target)?;
                    click_center(&tab, &bb)?;
                    tab.type_str(&value)
                        .map_err(|e| OrchynError::Platform(format!("type: {e}")))?;
                }
                HarnessAction::Clear { target } => {
                    let bb = lookup(&element_map, target)?;
                    click_center(&tab, &bb)?;
                    // Select-all + delete: reasonable cross-platform clear.
                    tab.evaluate(
                        "document.activeElement && (document.activeElement.value = '')",
                        false,
                    )
                    .map_err(|e| OrchynError::Platform(format!("clear: {e}")))?;
                }
                HarnessAction::Drag { .. } => {
                    return Err(OrchynError::Platform(
                        "DRAG action not yet implemented on web adapter".into(),
                    ));
                }
                HarnessAction::Wait { .. } => {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                HarnessAction::AssertSuccess { .. }
                | HarnessAction::AssertFailure { .. }
                | HarnessAction::WsWait { .. } => {
                    // Assertions are evaluated by the harness, not the adapter.
                }
            }
            Ok(())
        }
        .boxed()
    }

    fn shutdown(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }
}

// === scrape_elements helpers ===

const SCRAPE_ELEMENTS_JS: &str = r#"
(() => {
    const SELECTORS = [
        'button',
        'a[href]',
        'input:not([type="hidden"])',
        'textarea',
        'select',
        '[role="button"]',
        '[role="link"]',
        '[role="checkbox"]',
        '[role="radio"]',
        '[role="textbox"]',
        '[role="combobox"]',
        '[contenteditable="true"]',
    ];
    const seen = new Set();
    const out = [];
    for (const sel of SELECTORS) {
        for (const el of document.querySelectorAll(sel)) {
            if (seen.has(el)) continue;
            seen.add(el);
            const r = el.getBoundingClientRect();
            if (r.width <= 0 || r.height <= 0) continue;
            // Skip elements scrolled fully off-screen.
            if (r.bottom < 0 || r.right < 0) continue;
            const style = window.getComputedStyle(el);
            if (style.visibility === 'hidden' || style.display === 'none') continue;
            const tag = el.tagName.toLowerCase();
            const role = el.getAttribute('role');
            const type = role || tag;
            let text = (el.innerText || el.value || el.placeholder || el.getAttribute('aria-label') || '').trim();
            if (text.length > 80) text = text.slice(0, 80);
            out.push({
                type,
                text: text || null,
                x: Math.max(0, Math.round(r.left)),
                y: Math.max(0, Math.round(r.top)),
                w: Math.round(r.width),
                h: Math.round(r.height),
            });
        }
    }
    return JSON.stringify(out);
})()
"#;

#[derive(serde::Deserialize)]
struct RawElement {
    #[serde(rename = "type")]
    ty: String,
    text: Option<String>,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl RawElement {
    fn into_ui(self) -> UiElement {
        UiElement {
            element_type: self.ty,
            text: self.text,
            bounds: BoundingBox {
                x: self.x,
                y: self.y,
                width: self.w,
                height: self.h,
            },
        }
    }
}

fn lookup(map: &[AnnotatedElement], target: u8) -> Result<BoundingBox> {
    map.iter()
        .find(|e| e.id == target)
        .map(|e| e.bounds)
        .ok_or_else(|| OrchynError::Validation(format!("unknown target id: {target}")))
}

fn click_center(tab: &headless_chrome::Tab, bb: &BoundingBox) -> Result<()> {
    let point = headless_chrome::browser::tab::point::Point {
        x: bb.x as f64 + bb.width as f64 / 2.0,
        y: bb.y as f64 + bb.height as f64 / 2.0,
    };
    tab.click_point(point)
        .map_err(|e| OrchynError::Platform(format!("click_point: {e}")))?;
    Ok(())
}

fn diag_to_console(d: DiagnosticEvent) -> Option<ConsoleLog> {
    match d {
        DiagnosticEvent::Console { level, message, .. } => Some(ConsoleLog {
            level: log_level_str(&level).to_string(),
            message,
        }),
        DiagnosticEvent::Anomaly { kind, details } => Some(ConsoleLog {
            level: "warn".to_string(),
            message: format!("{kind}: {details}"),
        }),
    }
}

fn log_level_str(l: &LogLevel) -> &'static str {
    match l {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

fn net_to_request(n: NetworkEvent) -> NetworkRequest {
    NetworkRequest {
        method: n.method,
        url: n.url,
        status: n.status,
    }
}
