use chrono::Utc;
use futures::future::{BoxFuture, FutureExt};
use headless_chrome::protocol::cdp::types::Event;
use headless_chrome::protocol::cdp::{Emulation, Log, Network};
use headless_chrome::{Browser, LaunchOptions};
use orchyn_action_executor::{Action, DeviceInfo, PlatformAdapter};
use orchyn_core::action::{Action as HarnessAction, AnnotatedElement, BoundingBox};
use orchyn_core::{
    ConsoleLog, DiagnosticEvent, Exchange, LogLevel, NetworkEvent, NetworkRequest, OrchynError,
    Platform, RawTelemetry, Result, UiElement,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::info;

pub struct WebAdapter {
    _browser: Browser,
    tab: Arc<headless_chrome::Tab>,
    device_info: DeviceInfo,
    diagnostics: Arc<Mutex<Vec<DiagnosticEvent>>>,
    network_events: Arc<Mutex<Vec<NetworkEvent>>>,
    ws_messages: Arc<Mutex<Vec<String>>>,
    /// XHR/fetch exchanges with bodies, filled by the response handler and
    /// drained by `take_exchanges`. Separate from `network_events`.
    exchanges: Arc<Mutex<Vec<Exchange>>>,
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
                // `CHROME` overrides the browser binary (e.g. a Playwright-managed
                // Chromium); `ORCHYN_NO_SANDBOX=1` is needed when running as root
                // inside containers / CI.
                .path(std::env::var_os("CHROME").map(std::path::PathBuf::from))
                .sandbox(std::env::var("ORCHYN_NO_SANDBOX").map(|v| v != "1").unwrap_or(true))
                // Default idle_browser_timeout is 30s — too short when LLM inference
                // takes 15-40s per turn. Set to 10 minutes to outlast any turn.
                .idle_browser_timeout(std::time::Duration::from_secs(600))
                .build()
                .map_err(|e| OrchynError::Internal(e.to_string()))?,
        )
        .map_err(|e| OrchynError::Internal(e.to_string()))?;

        let tab = browser
            .new_tab()
            .map_err(|e| OrchynError::Internal(e.to_string()))?;

        let diagnostics: Arc<Mutex<Vec<DiagnosticEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let network_events: Arc<Mutex<Vec<NetworkEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let exchanges: Arc<Mutex<Vec<Exchange>>> = Arc::new(Mutex::new(Vec::new()));
        let ws_messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        // Enable Log + Runtime domains before registering the listener.
        tab.enable_log()
            .map_err(|e| OrchynError::Internal(e.to_string()))?
            .enable_runtime()
            .map_err(|e| OrchynError::Internal(e.to_string()))?;

        // Enable Network domain (idempotent; also called internally by register_response_handling).
        tab.call_method(Network::Enable {
            max_total_buffer_size: None,
            max_resource_buffer_size: None,
            max_post_data_size: None,
            report_direct_socket_traffic: None,
            enable_durable_messages: None,
        })
        .map_err(|e| OrchynError::Internal(format!("Network.enable: {e}")))?;

        // request_id → (method, post_data) from RequestWillBeSent so we can attach them to the
        // response. Shared between the event listener and the response handler.
        let pending: Arc<Mutex<HashMap<String, (String, Option<String>)>>> =
            Arc::new(Mutex::new(HashMap::new()));
        // request_id → index into `network_events` once response headers arrived.
        // Recording at ResponseReceived (not LoadingFinished) means requests whose
        // body never finishes loading — e.g. the page navigates away right after
        // `await fetch()` resolves — are still observed with their real status.
        let recorded: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));

        // ── event listener: console, WS frames, request capture, network failures ──────────────
        {
            let diag_l = Arc::clone(&diagnostics);
            let rec_l = Arc::clone(&recorded);
            let net_l = Arc::clone(&network_events);
            let ws_l = Arc::clone(&ws_messages);
            let pending_l = Arc::clone(&pending);

            tab.add_event_listener(Arc::new(move |event: &Event| {
                match event {
                    // Log-domain console messages (console.log/warn/error → Log.entryAdded).
                    Event::LogEntryAdded(ev) => {
                        let level = match ev.params.entry.level {
                            Log::LogEntryLevel::Error => LogLevel::Error,
                            Log::LogEntryLevel::Warning => LogLevel::Warn,
                            _ => LogLevel::Info,
                        };
                        diag_l.lock().unwrap().push(DiagnosticEvent::Console {
                            level,
                            message: ev.params.entry.text.clone(),
                            timestamp: Utc::now(),
                        });
                    }

                    // Uncaught JS exceptions — include file + line + first stack frame.
                    Event::RuntimeExceptionThrown(ev) => {
                        let details = &ev.params.exception_details;
                        let base_msg = details
                            .exception
                            .as_ref()
                            .and_then(|e| e.description.clone())
                            .unwrap_or_else(|| details.text.clone());

                        // Attach file:line if available.
                        let location = if let Some(url) = &details.url {
                            format!(" ({}:{}:{})", url, details.line_number, details.column_number)
                        } else {
                            String::new()
                        };

                        // Append first few call frames so the model knows where to look.
                        let frames = details
                            .stack_trace
                            .as_ref()
                            .map(|st| {
                                st.call_frames
                                    .iter()
                                    .take(3)
                                    .map(|f| {
                                        format!(
                                            "\n    at {}{}:{}:{}",
                                            if f.function_name.is_empty() {
                                                "<anonymous>".to_string()
                                            } else {
                                                f.function_name.clone()
                                            },
                                            if f.url.is_empty() {
                                                String::new()
                                            } else {
                                                format!(" ({})", f.url)
                                            },
                                            f.line_number,
                                            f.column_number,
                                        )
                                    })
                                    .collect::<String>()
                            })
                            .unwrap_or_default();

                        diag_l.lock().unwrap().push(DiagnosticEvent::Console {
                            level: LogLevel::Error,
                            message: format!("Uncaught: {base_msg}{location}{frames}"),
                            timestamp: Utc::now(),
                        });
                    }

                    // Direct console API calls via the Runtime domain (distinct from Log domain).
                    // Only capture warn/error to avoid noise.
                    Event::RuntimeConsoleAPICalled(ev) => {
                        use headless_chrome::protocol::cdp::Runtime::ConsoleAPICalledEventTypeOption;
                        let level = match ev.params.Type {
                            ConsoleAPICalledEventTypeOption::Error => LogLevel::Error,
                            ConsoleAPICalledEventTypeOption::Warning => LogLevel::Warn,
                            _ => return,
                        };
                        let msg = ev
                            .params
                            .args
                            .iter()
                            .filter_map(|a| {
                                a.value
                                    .as_ref()
                                    .map(|v| v.to_string())
                                    .or_else(|| a.description.clone())
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        diag_l.lock().unwrap().push(DiagnosticEvent::Console {
                            level,
                            message: msg,
                            timestamp: Utc::now(),
                        });
                    }

                    // Store request metadata so the response handler can join it.
                    Event::NetworkRequestWillBeSent(ev) => {
                        pending_l.lock().unwrap().insert(
                            ev.params.request_id.clone(),
                            (
                                ev.params.request.method.clone(),
                                ev.params.request.post_data.clone(),
                            ),
                        );
                    }

                    Event::NetworkResponseReceived(ev) => {
                        let (method, req_body) = pending_l
                            .lock()
                            .unwrap()
                            .get(&ev.params.request_id)
                            .cloned()
                            .unwrap_or_default();
                        let mut net = net_l.lock().unwrap();
                        net.push(NetworkEvent {
                            method,
                            url: ev.params.response.url.clone(),
                            status: ev.params.response.status as u16,
                            request_body: req_body,
                            response_body: None,
                            timestamp: Utc::now(),
                        });
                        rec_l
                            .lock()
                            .unwrap()
                            .insert(ev.params.request_id.clone(), net.len() - 1);
                    }

                    // Network-level failures (DNS, CORS, connection refused) — no HTTP status.
                    // A request that already got a response and was then cancelled
                    // (navigation) is not a failure.
                    Event::NetworkLoadingFailed(ev)
                        if rec_l.lock().unwrap().contains_key(&ev.params.request_id) => {}
                    Event::NetworkLoadingFailed(ev) => {
                        let (method, req_body) = pending_l
                            .lock()
                            .unwrap()
                            .remove(&ev.params.request_id)
                            .unwrap_or_default();
                        net_l.lock().unwrap().push(NetworkEvent {
                            method,
                            url: String::new(),
                            status: 0,
                            request_body: req_body,
                            response_body: Some(ev.params.error_text.clone()),
                            timestamp: Utc::now(),
                        });
                    }

                    // WebSocket frames (text frames only; opcode 1 = text, 2 = binary).
                    Event::NetworkWebSocketFrameReceived(ev) => {
                        if ev.params.response.opcode as u8 == 1 {
                            let payload = ev.params.response.payload_data.clone();
                            if payload.len() <= 2048 {
                                ws_l.lock().unwrap().push(format!("← {payload}"));
                            } else {
                                ws_l.lock().unwrap().push(format!("← {}…", &payload[..2048]));
                            }
                        }
                    }
                    Event::NetworkWebSocketFrameSent(ev) => {
                        if ev.params.response.opcode as u8 == 1 {
                            let payload = ev.params.response.payload_data.clone();
                            if payload.len() <= 2048 {
                                ws_l.lock().unwrap().push(format!("→ {payload}"));
                            } else {
                                ws_l.lock().unwrap().push(format!("→ {}…", &payload[..2048]));
                            }
                        }
                    }

                    _ => {}
                }
            }))
            .map_err(|e| OrchynError::Internal(format!("event_listener: {e}")))?;
        }

        // ── dialogs: alert/confirm/prompt block the page (and every CDP call that
        // waits on it) until handled. Accept them immediately, like a user
        // clicking OK, and record them so manifests can assert on them.
        {
            let diag_d = Arc::clone(&diagnostics);
            let dialog = tab.get_dialog();
            tab.add_event_listener(Arc::new(move |event: &Event| {
                if let Event::PageJavascriptDialogOpening(ev) = event {
                    diag_d.lock().unwrap().push(DiagnosticEvent::Console {
                        level: LogLevel::Info,
                        message: format!("[dialog] {}", ev.params.message),
                        timestamp: Utc::now(),
                    });
                    let _ = dialog.accept(None);
                }
            }))
            .map_err(|e| OrchynError::Internal(format!("dialog_listener: {e}")))?;
        }

        // ── response handler: fires on loadingFinished, gives us status + optional body ────────
        {
            let net_l = Arc::clone(&network_events);
            let pending_l = Arc::clone(&pending);
            let rec_h = Arc::clone(&recorded);
            let exch_h = Arc::clone(&exchanges);

            tab.register_response_handling(
                "orchyn",
                Box::new(move |params, fetch_body| {
                    let (method, req_body) = pending_l
                        .lock()
                        .unwrap()
                        .remove(&params.request_id)
                        .unwrap_or_default();

                    // Only fetch body for API calls; skip images, fonts, CSS, scripts.
                    let resp_body = match params.Type {
                        Network::ResourceType::Xhr
                        | Network::ResourceType::Fetch
                        | Network::ResourceType::Document => fetch_body()
                            .ok()
                            .filter(|r| !r.base_64_encoded)
                            .map(|r| r.body)
                            .filter(|b| !b.is_empty() && b.len() <= 16_384),
                        _ => None,
                    };

                    if matches!(
                        params.Type,
                        Network::ResourceType::Xhr | Network::ResourceType::Fetch
                    ) {
                        exch_h.lock().unwrap().push(Exchange {
                            method: method.clone(),
                            url: params.response.url.clone(),
                            status: params.response.status as u16,
                            request_body: req_body.clone(),
                            response_body: resp_body.clone(),
                        });
                    }

                    let idx = rec_h.lock().unwrap().get(&params.request_id).copied();
                    let mut net = net_l.lock().unwrap();
                    // The buffer is drained by collect_telemetry, so the stored
                    // index may be stale: only attach when it still points at
                    // the same request.
                    match idx.and_then(|i| net.get_mut(i)) {
                        Some(ev) if ev.url == params.response.url => {
                            ev.response_body = resp_body;
                        }
                        Some(_) => {}
                        None if idx.is_none() => net.push(NetworkEvent {
                            method,
                            url: params.response.url.clone(),
                            status: params.response.status as u16,
                            request_body: req_body,
                            response_body: resp_body,
                            timestamp: Utc::now(),
                        }),
                        None => {}
                    }
                }),
            )
            .map_err(|e| OrchynError::Internal(format!("response_handler: {e}")))?;
        }

        tab.navigate_to(url)
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        tab.wait_until_navigated()
            .map_err(|e| OrchynError::Internal(e.to_string()))?;

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
            ws_messages,
            exchanges,
        })
    }

    /// Change the browser viewport. Affects both screenshot dimensions and responsive CSS.
    pub fn set_viewport(&self, width: u32, height: u32) -> Result<()> {
        self.tab
            .call_method(Emulation::SetDeviceMetricsOverride {
                width,
                height,
                device_scale_factor: 1.0,
                mobile: false,
                scale: None,
                screen_width: None,
                screen_height: None,
                position_x: None,
                position_y: None,
                dont_set_visible_size: None,
                screen_orientation: None,
                viewport: None,
                display_feature: None,
                device_posture: None,
            })
            .map_err(|e| OrchynError::Platform(format!("set_viewport: {e}")))?;
        Ok(())
    }

    /// Evaluate an arbitrary JS expression and return the JSON result string.
    pub fn eval_json(&self, js: &str) -> Result<String> {
        let remote = self
            .tab
            .evaluate(js, false)
            .map_err(|e| OrchynError::Platform(format!("eval: {e}")))?;
        Ok(remote
            .value
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string()))
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
                .map_err(|e| OrchynError::Internal(e.to_string()))?;
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
                        .map_err(|e| OrchynError::Internal(e.to_string()))?;
                }
                Action::Type { text } => {
                    tab.type_str(&text)
                        .map_err(|e| OrchynError::Internal(e.to_string()))?;
                }
                Action::Wait { ms } => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(ms as u64)).await;
                }
                Action::Reset => {
                    tab.navigate_to("about:blank")
                        .map_err(|e| OrchynError::Internal(e.to_string()))?;
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

    fn take_exchanges(&mut self) -> BoxFuture<'_, Result<Vec<Exchange>>> {
        let exch = self.exchanges.clone();
        async move { Ok(std::mem::take(&mut *exch.lock().unwrap())) }.boxed()
    }

    fn get_elements(&mut self) -> BoxFuture<'_, Result<Vec<serde_json::Value>>> {
        async move { Ok(vec![]) }.boxed()
    }

    fn reset(&mut self) -> BoxFuture<'_, Result<()>> {
        let tab = self.tab.clone();
        async move {
            tab.navigate_to("about:blank")
                .map_err(|e| OrchynError::Internal(e.to_string()))?;
            Ok(())
        }
        .boxed()
    }

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
        let ws = self.ws_messages.clone();
        async move {
            let console_logs: Vec<ConsoleLog> = {
                let mut lock = diags.lock().unwrap();
                lock.drain(..).filter_map(diag_to_console).collect()
            };
            let network_requests: Vec<NetworkRequest> = {
                let mut lock = nets.lock().unwrap();
                lock.drain(..).map(net_to_request).collect()
            };
            let ws_messages: Vec<String> = {
                let mut lock = ws.lock().unwrap();
                lock.drain(..).collect()
            };
            Ok(RawTelemetry {
                console_logs,
                network_requests,
                ws_messages,
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
                | HarnessAction::WsWait { .. } => {}
            }
            Ok(())
        }
        .boxed()
    }

    fn shutdown(&mut self) -> BoxFuture<'_, Result<()>> {
        async move { Ok(()) }.boxed()
    }

    fn eval_js(&mut self, js: &str) -> BoxFuture<'_, Result<String>> {
        let tab = self.tab.clone();
        let js = js.to_string();
        async move {
            let remote = tab
                .evaluate(&js, false)
                .map_err(|e| OrchynError::Platform(format!("eval_js: {e}")))?;
            Ok(remote
                .value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string()))
        }
        .boxed()
    }

    fn set_viewport(&mut self, width: u32, height: u32) -> BoxFuture<'_, Result<()>> {
        let tab = self.tab.clone();
        async move {
            tab.call_method(Emulation::SetDeviceMetricsOverride {
                width,
                height,
                device_scale_factor: 1.0,
                mobile: false,
                scale: None,
                screen_width: None,
                screen_height: None,
                position_x: None,
                position_y: None,
                dont_set_visible_size: None,
                screen_orientation: None,
                viewport: None,
                display_feature: None,
                device_posture: None,
            })
            .map_err(|e| OrchynError::Platform(format!("set_viewport: {e}")))?;
            Ok(())
        }
        .boxed()
    }
}

// === JS snippets ============================================================

/// Interactive element scraper. Includes CSS anomaly flags so the model can
/// detect invisible-but-present elements and invalid form fields.
const SCRAPE_ELEMENTS_JS: &str = r#"
(() => {
    const SELECTORS = [
        'button', 'a[href]', 'input:not([type="hidden"])',
        'textarea', 'select',
        '[role="button"]', '[role="link"]', '[role="checkbox"]',
        '[role="radio"]', '[role="textbox"]', '[role="combobox"]',
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
            if (r.bottom < 0 || r.right < 0) continue;
            const style = window.getComputedStyle(el);
            if (style.visibility === 'hidden' || style.display === 'none') continue;
            const tag = el.tagName.toLowerCase();
            const role = el.getAttribute('role');
            const type = role || tag;
            let text = (el.innerText || el.value || el.placeholder || el.getAttribute('aria-label') || '').trim();
            if (text.length > 80) text = text.slice(0, 80);
            // CSS anomaly flags
            const opacity = parseFloat(style.opacity);
            const flags = [];
            if (opacity < 0.1) flags.push('opacity:' + opacity.toFixed(2));
            if (style.pointerEvents === 'none') flags.push('pointer-events:none');
            if (el.matches(':invalid')) flags.push('invalid');
            if (el.getAttribute('aria-invalid') === 'true') flags.push('aria-invalid');
            if (style.color === style.backgroundColor && style.color !== '') flags.push('invisible-text');
            out.push({
                type,
                text: text || null,
                x: Math.max(0, Math.round(r.left)),
                y: Math.max(0, Math.round(r.top)),
                w: Math.round(r.width),
                h: Math.round(r.height),
                flags: flags.length ? flags : null,
            });
        }
    }
    return JSON.stringify(out);
})()
"#;

/// Accessibility violation scanner. Returns an array of plain-English issues.
pub const ARIA_VIOLATIONS_JS: &str = r#"
(() => {
    const issues = [];
    // Images without alt text
    for (const img of document.querySelectorAll('img:not([alt])')) {
        const src = img.src.split('/').pop() || 'img';
        issues.push({ type: 'missing-alt', element: 'img', detail: src });
    }
    // Inputs without labels
    for (const inp of document.querySelectorAll('input:not([type="hidden"]):not([type="submit"]):not([type="button"])')) {
        const id = inp.id;
        const hasLabel = (id && document.querySelector('label[for="' + id + '"]'))
            || inp.closest('label')
            || inp.getAttribute('aria-label')
            || inp.getAttribute('aria-labelledby');
        if (!hasLabel) {
            issues.push({ type: 'missing-label', element: 'input', detail: inp.type || 'text', name: inp.name });
        }
    }
    // Buttons without accessible names
    for (const btn of document.querySelectorAll('button, [role="button"]')) {
        const name = (btn.innerText || btn.getAttribute('aria-label') || btn.getAttribute('title') || '').trim();
        if (!name) issues.push({ type: 'unnamed-button', element: btn.tagName.toLowerCase() });
    }
    // Links without text
    for (const a of document.querySelectorAll('a[href]')) {
        const name = (a.innerText || a.getAttribute('aria-label') || a.getAttribute('title') || '').trim();
        if (!name) issues.push({ type: 'unnamed-link', href: a.href });
    }
    // Focusable elements with tabIndex < 0 that look interactive
    for (const el of document.querySelectorAll('button, a, input, select, textarea')) {
        if (el.tabIndex < 0) {
            issues.push({ type: 'unreachable-by-keyboard', element: el.tagName.toLowerCase(),
                text: (el.innerText || el.value || '').trim().slice(0, 40) });
        }
    }
    // form :invalid fields
    for (const el of document.querySelectorAll(':invalid')) {
        issues.push({ type: 'invalid-field', element: el.tagName.toLowerCase(),
            name: el.name || el.id || null });
    }
    return JSON.stringify(issues);
})()
"#;

/// localStorage + sessionStorage snapshot (keys and values, capped at 4 KB each).
pub const STORAGE_SNAPSHOT_JS: &str = r#"
(() => {
    const dump = (store) => {
        const out = {};
        for (let i = 0; i < store.length; i++) {
            const k = store.key(i);
            let v = store.getItem(k);
            if (v && v.length > 4096) v = v.slice(0, 4096) + '…';
            out[k] = v;
        }
        return out;
    };
    return JSON.stringify({
        localStorage: dump(localStorage),
        sessionStorage: dump(sessionStorage),
    });
})()
"#;

/// Performance metrics via the Web Performance API.
pub const PERFORMANCE_JS: &str = r#"
(() => {
    const nav = performance.getEntriesByType('navigation')[0] || {};
    const resources = performance.getEntriesByType('resource');
    const slowResources = resources
        .filter(r => r.duration > 500)
        .map(r => ({ name: r.name.split('/').pop(), duration: Math.round(r.duration), type: r.initiatorType }))
        .slice(0, 10);
    // LCP via PerformanceObserver buffer (only available if page loaded after observer)
    const lcpEntries = performance.getEntriesByType('largest-contentful-paint');
    const lcp = lcpEntries.length ? Math.round(lcpEntries[lcpEntries.length - 1].startTime) : null;
    const cls = (() => {
        let score = 0;
        for (const e of performance.getEntriesByType('layout-shift')) score += e.value;
        return parseFloat(score.toFixed(4));
    })();
    return JSON.stringify({
        domContentLoaded: Math.round(nav.domContentLoadedEventEnd - nav.startTime) || null,
        loadComplete: Math.round(nav.loadEventEnd - nav.startTime) || null,
        ttfb: Math.round(nav.responseStart - nav.startTime) || null,
        lcp,
        cls,
        slowResources,
        totalResources: resources.length,
    });
})()
"#;

// === helpers ================================================================

#[derive(serde::Deserialize)]
struct RawElement {
    #[serde(rename = "type")]
    ty: String,
    text: Option<String>,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    flags: Option<Vec<String>>,
}

impl RawElement {
    fn into_ui(self) -> UiElement {
        let mut label = self.text;
        if let Some(flags) = self.flags {
            if !flags.is_empty() {
                let flag_str = format!("[{}]", flags.join(", "));
                label = Some(match label {
                    Some(t) => format!("{t} {flag_str}"),
                    None => flag_str,
                });
            }
        }
        UiElement {
            element_type: self.ty,
            text: label,
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
