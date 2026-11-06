//! Deterministic replay runner ("Constraint Mode").
//!
//! Executes a [`ReplayManifest`] against platform adapters with no model in the
//! loop. Each actor gets its own adapter, created lazily on its first step.
//! Clicks and typing go through `execute_harness` so they are real pointer and
//! keyboard input. Element lookup and assertions use `eval_js` probes.
//!
//! Failures are recorded in the report and halt the run. Only a factory failure
//! makes [`ReplayRunner::run`] return `Err`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use orchyn_core::action::{Action as HarnessAction, AnnotatedElement, BoundingBox};
use orchyn_core::{ConsoleLog, NetworkRequest, PlatformAdapter, RawTelemetry};
use serde::{Deserialize, Serialize};

use super::manifest::{ReplayManifest, StepOp};

/// Creates the adapter for one actor. Called once per distinct actor name.
pub type AdapterFactoryFn = Arc<
    dyn Fn(String) -> BoxFuture<'static, orchyn_core::Result<Box<dyn PlatformAdapter>>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub index: usize,
    pub actor: String,
    pub op: String,
    pub passed: bool,
    pub detail: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayReport {
    pub name: String,
    pub base_url: String,
    pub passed: bool,
    /// Executed steps only. The run halts at the first failure.
    pub steps: Vec<StepResult>,
    pub total_steps: usize,
    /// Index of the failing step, if any.
    pub first_failure: Option<usize>,
    pub duration_ms: u64,
    /// All actors, whole run, in order.
    pub observed_requests: Vec<NetworkRequest>,
    /// All actors, formatted as `"actor: message"`.
    pub console_errors: Vec<String>,
}

pub struct ReplayRunner {
    factory: AdapterFactoryFn,
    base_url_override: Option<String>,
}

impl ReplayRunner {
    pub fn new(factory: AdapterFactoryFn) -> Self {
        Self {
            factory,
            base_url_override: None,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url_override = Some(url.into());
        self
    }

    pub async fn run(&self, manifest: &ReplayManifest) -> orchyn_core::Result<ReplayReport> {
        let started = Instant::now();
        let base_url = self
            .base_url_override
            .clone()
            .unwrap_or_else(|| manifest.base_url.clone());
        let mut session = Session {
            factory: self.factory.clone(),
            base_url: base_url.clone(),
            actors: Vec::new(),
            observed_requests: Vec::new(),
            console_errors: Vec::new(),
        };

        let mut steps = Vec::with_capacity(manifest.steps.len());
        let mut first_failure = None;
        for (index, step) in manifest.steps.iter().enumerate() {
            let t0 = Instant::now();
            let i = match session.actor_index(&step.actor).await {
                Ok(i) => i,
                Err(e) => {
                    session.shutdown().await;
                    return Err(e);
                }
            };
            let outcome = match session.exec(i, &step.op).await {
                // Pull telemetry produced by the step so the next assertion sees it.
                Ok(detail) => session.pull(i).await.map(|()| detail),
                Err(detail) => Err(detail),
            };
            let (passed, detail) = match outcome {
                Ok(detail) => (true, detail),
                Err(detail) => (false, detail),
            };
            steps.push(StepResult {
                index,
                actor: step.actor.clone(),
                op: step.op.name().to_string(),
                passed,
                detail,
                duration_ms: elapsed_ms(t0),
            });
            if !passed {
                first_failure = Some(index);
                break;
            }
        }

        session.drain_all().await;
        session.shutdown().await;

        Ok(ReplayReport {
            name: manifest.name.clone(),
            base_url,
            passed: first_failure.is_none(),
            steps,
            total_steps: manifest.steps.len(),
            first_failure,
            duration_ms: elapsed_ms(started),
            observed_requests: session.observed_requests,
            console_errors: session.console_errors,
        })
    }
}

/// Step-level outcome: `Ok` carries a short description, `Err` a failure detail.
type StepOutcome = Fallible<String>;
type Fallible<T> = std::result::Result<T, String>;

/// Per-actor state: the adapter plus telemetry accumulated since the last reset.
struct Actor {
    name: String,
    adapter: Box<dyn PlatformAdapter>,
    requests: Vec<NetworkRequest>,
    /// Console error messages since the start of the run or the last reset.
    errors: Vec<String>,
}

/// Run-wide state. Lives for one `run` call.
struct Session {
    factory: AdapterFactoryFn,
    base_url: String,
    actors: Vec<Actor>,
    observed_requests: Vec<NetworkRequest>,
    console_errors: Vec<String>,
}

impl Session {
    /// Index of the actor, creating its adapter on first use.
    async fn actor_index(&mut self, name: &str) -> orchyn_core::Result<usize> {
        if let Some(i) = self.actors.iter().position(|a| a.name == name) {
            return Ok(i);
        }
        let adapter = (self.factory)(name.to_string()).await?;
        self.actors.push(Actor {
            name: name.to_string(),
            adapter,
            requests: Vec::new(),
            errors: Vec::new(),
        });
        Ok(self.actors.len() - 1)
    }

    async fn exec(&mut self, i: usize, op: &StepOp) -> StepOutcome {
        match op {
            StepOp::Goto { path } => {
                let url = join_url(&self.base_url, path);
                goto(&mut *self.actors[i].adapter, &url).await
            }
            StepOp::Click { selector } => click(&mut *self.actors[i].adapter, selector).await,
            StepOp::ClickText { text } => click_text(&mut *self.actors[i].adapter, text).await,
            StepOp::Fill { selector, value } => {
                fill(&mut *self.actors[i].adapter, selector, value).await
            }
            StepOp::WaitMs { ms } => {
                sleep_ms(*ms).await;
                Ok(format!("waited {ms} ms"))
            }
            StepOp::WaitForText { text, timeout_ms } | StepOp::ExpectText { text, timeout_ms } => {
                wait_for_text(&mut *self.actors[i].adapter, text, *timeout_ms).await
            }
            StepOp::ExpectNoText { text } => {
                expect_no_text(&mut *self.actors[i].adapter, text).await
            }
            StepOp::ExpectUrl { contains } => {
                expect_url(&mut *self.actors[i].adapter, contains).await
            }
            StepOp::ExpectVisible { selector } => {
                expect_visible(&mut *self.actors[i].adapter, selector).await
            }
            StepOp::ExpectConsoleClean => {
                self.pull(i).await?;
                expect_console_clean(&self.actors[i])
            }
            StepOp::ExpectRequest {
                method,
                path_contains,
                status,
            } => {
                self.pull(i).await?;
                expect_request(&self.actors[i], method, path_contains, *status)
            }
            StepOp::ExpectNoFailedRequests => {
                self.pull(i).await?;
                expect_no_failed_requests(&self.actors[i])
            }
            StepOp::ResetTelemetry => {
                // Drain first so events from before the reset do not reappear.
                self.pull(i).await?;
                self.actors[i].requests.clear();
                self.actors[i].errors.clear();
                Ok("telemetry reset".to_string())
            }
        }
    }

    /// Drains the adapter's buffered telemetry into the actor's buffer and the
    /// run-wide report lists.
    async fn pull(&mut self, i: usize) -> Fallible<()> {
        let raw: RawTelemetry = self.actors[i]
            .adapter
            .collect_telemetry()
            .await
            .map_err(|e| format!("collecting telemetry failed: {e}"))?;
        let actor = &mut self.actors[i];
        for ConsoleLog { level, message } in raw.console_logs {
            if is_console_error(&level, &message) {
                self.console_errors
                    .push(format!("{}: {message}", actor.name));
                actor.errors.push(message);
            }
        }
        for req in raw.network_requests {
            self.observed_requests.push(req.clone());
            actor.requests.push(req);
        }
        Ok(())
    }

    /// Best-effort final drain so the report includes late telemetry.
    async fn drain_all(&mut self) {
        for i in 0..self.actors.len() {
            let _ = self.pull(i).await;
        }
    }

    async fn shutdown(&mut self) {
        for actor in &mut self.actors {
            if let Err(e) = actor.adapter.shutdown().await {
                tracing::debug!(actor = %actor.name, error = %e, "adapter shutdown failed");
            }
        }
    }
}

fn is_console_error(level: &str, message: &str) -> bool {
    level.eq_ignore_ascii_case("error") || message.contains("Uncaught")
}

/// Joins `path` onto `base`. Absolute `http...` paths are used as-is.
fn join_url(base: &str, path: &str) -> String {
    if path.starts_with("http") {
        return path.to_string();
    }
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

// ---------------------------------------------------------------------------
// Step implementations
// ---------------------------------------------------------------------------

const NAV_SETTLE_MS: u64 = 300;
const ACTION_SETTLE_MS: u64 = 250;
const POLL_MS: u64 = 100;
const MAX_LISTED: usize = 10;
const MARKER_ATTR: &str = "data-orchyn-replay-target";
const MARKER_SELECTOR: &str = "[data-orchyn-replay-target]";
const CLEAR_MARKER_JS: &str = "(() => { document.querySelectorAll('[data-orchyn-replay-target]')\
    .forEach((e) => e.removeAttribute('data-orchyn-replay-target')); return 'ok'; })()";

async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

fn elapsed_ms(t: Instant) -> u64 {
    u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// JS string literal for `s`. JSON string syntax is valid JS.
fn js_str(s: &str) -> String {
    serde_json::to_string(s).expect("serialising a string cannot fail")
}

/// `eval_js` returns the JS value JSON-encoded, so a JS string arrives quoted.
/// This returns the JS value as text: the string itself, or the JSON of other
/// values such as `true` or `null`.
fn js_text(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => raw.to_string(),
    }
}

/// Geometry and state of one element, as measured by [`probe_js`].
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Probe {
    found: bool,
    visible: bool,
    covered: bool,
    tag: String,
    coverer: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Scrolls the element into view, then measures it and checks what is at its
/// centre point. `covered` is true when `elementFromPoint` returns neither the
/// element nor a descendant.
fn probe_js(selector: &str) -> String {
    format!(
        r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ found: false }});
  el.scrollIntoView({{ block: 'center', inline: 'center' }});
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
  const covered = top === null || !el.contains(top);
  const describe = (n) => {{
    let s = n.tagName;
    if (n.id) s += '#' + n.id;
    const cls = (typeof n.className === 'string' ? n.className : '').trim().split(/\s+/).filter(Boolean);
    if (cls.length) s += '.' + cls.join('.');
    return s;
  }};
  return JSON.stringify({{
    found: true,
    visible: r.width > 0 && r.height > 0 && cs.display !== 'none' && cs.visibility !== 'hidden',
    covered,
    tag: el.tagName,
    coverer: top ? describe(top) : null,
    x: r.left, y: r.top, w: r.width, h: r.height,
  }});
}})()"#,
        sel = js_str(selector)
    )
}

/// Marks the first visible button, link or role=button whose text matches with
/// the marker attribute, so the probe can address it.
fn tag_text_js(text: &str) -> String {
    format!(
        r#"(() => {{
  const want = {want};
  const attr = '{attr}';
  document.querySelectorAll('[' + attr + ']').forEach((e) => e.removeAttribute(attr));
  const visible = (e) => {{
    const r = e.getBoundingClientRect();
    const cs = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && cs.display !== 'none' && cs.visibility !== 'hidden';
  }};
  const label = (e) => (e.tagName === 'INPUT' ? (e.value || '') : (e.innerText || '')).trim();
  const cands = Array.from(document.querySelectorAll(
    'button, a, [role=button], input[type=submit], input[type=button]'));
  const hit = cands.find((e) => label(e) === want && visible(e));
  if (!hit) return JSON.stringify({{ found: false }});
  hit.setAttribute(attr, '');
  return JSON.stringify({{ found: true }});
}})()"#,
        want = js_str(text),
        attr = MARKER_ATTR,
    )
}

fn clear_value_js(selector: &str) -> String {
    format!(
        "(() => {{ const el = document.querySelector({sel}); \
         if (el && 'value' in el) {{ el.value = ''; \
         el.dispatchEvent(new Event('input', {{ bubbles: true }})); }} return 'ok'; }})()",
        sel = js_str(selector)
    )
}

fn body_contains_js(text: &str) -> String {
    format!(
        "(document.body ? document.body.innerText : '').includes({})",
        js_str(text)
    )
}

async fn probe(adapter: &mut dyn PlatformAdapter, selector: &str) -> Fallible<Probe> {
    let raw = adapter
        .eval_js(&probe_js(selector))
        .await
        .map_err(|e| format!("probing {selector} failed: {e}"))?;
    serde_json::from_str(&js_text(&raw))
        .map_err(|e| format!("unreadable probe result for {selector}: {e}"))
}

/// Fails with a user-facing reason unless a real click would land on `label`.
fn check_actionable(label: &str, p: &Probe) -> Fallible<()> {
    if !p.found {
        return Err(format!("element {label} not found on the page"));
    }
    if !p.visible {
        return Err(format!(
            "element {label} is not visible (zero size, display:none or visibility:hidden)"
        ));
    }
    if p.covered {
        return Err(match &p.coverer {
            Some(by) => format!(
                "element {label} is covered by {by} at its centre — a real user cannot click it"
            ),
            None => format!(
                "element {label}'s centre is outside the viewport — a real user cannot click it"
            ),
        });
    }
    Ok(())
}

/// Builds the element handed to `execute_harness`. Its bounds put the click at
/// the element's centre, as `click_center` computes `x + width / 2`.
fn annotate(p: &Probe) -> AnnotatedElement {
    let bounds = if p.x >= 0.0 && p.y >= 0.0 {
        BoundingBox {
            x: p.x.round() as u32,
            y: p.y.round() as u32,
            width: p.w.round() as u32,
            height: p.h.round() as u32,
        }
    } else {
        // Top-left is off-screen: a 2px box centred on the element keeps the
        // click point exact.
        let cx = (p.x + p.w / 2.0).max(1.0).round() as u32;
        let cy = (p.y + p.h / 2.0).max(1.0).round() as u32;
        BoundingBox {
            x: cx - 1,
            y: cy - 1,
            width: 2,
            height: 2,
        }
    };
    AnnotatedElement {
        id: 0,
        element_type: p.tag.to_lowercase(),
        text: None,
        bounds,
    }
}

async fn goto(adapter: &mut dyn PlatformAdapter, url: &str) -> StepOutcome {
    adapter
        .execute_harness(
            &HarnessAction::Navigate {
                value: url.to_string(),
            },
            &[],
        )
        .await
        .map_err(|e| format!("navigate to {url} failed: {e}"))?;
    sleep_ms(NAV_SETTLE_MS).await;
    Ok(format!("navigated to {url}"))
}

/// Probes `selector`, checks it is clickable, then performs a real mouse click.
async fn click_resolved(
    adapter: &mut dyn PlatformAdapter,
    label: &str,
    selector: &str,
) -> StepOutcome {
    let p = probe(adapter, selector).await?;
    check_actionable(label, &p)?;
    adapter
        .execute_harness(&HarnessAction::Click { target: 0 }, &[annotate(&p)])
        .await
        .map_err(|e| format!("mouse click on {label} failed: {e}"))?;
    sleep_ms(ACTION_SETTLE_MS).await;
    Ok(format!("clicked {label}"))
}

async fn click(adapter: &mut dyn PlatformAdapter, selector: &str) -> StepOutcome {
    click_resolved(adapter, selector, selector).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Tagged {
    found: bool,
}

async fn click_text(adapter: &mut dyn PlatformAdapter, text: &str) -> StepOutcome {
    let raw = adapter
        .eval_js(&tag_text_js(text))
        .await
        .map_err(|e| format!("locating text {text:?} failed: {e}"))?;
    let tagged: Tagged = serde_json::from_str(&js_text(&raw)).unwrap_or_default();
    if !tagged.found {
        return Err(format!(
            "no visible button, link or [role=button] has the text {text:?}"
        ));
    }
    let outcome = click_resolved(
        adapter,
        &format!("button/link with text {text:?}"),
        MARKER_SELECTOR,
    )
    .await;
    // Best effort: remove the marker whether or not the click succeeded.
    let _ = adapter.eval_js(CLEAR_MARKER_JS).await;
    outcome
}

async fn fill(adapter: &mut dyn PlatformAdapter, selector: &str, value: &str) -> StepOutcome {
    let p = probe(adapter, selector).await?;
    check_actionable(selector, &p)?;
    adapter
        .eval_js(&clear_value_js(selector))
        .await
        .map_err(|e| format!("clearing {selector} failed: {e}"))?;
    adapter
        .execute_harness(
            &HarnessAction::Type {
                target: 0,
                value: value.to_string(),
            },
            &[annotate(&p)],
        )
        .await
        .map_err(|e| format!("typing into {selector} failed: {e}"))?;
    sleep_ms(ACTION_SETTLE_MS).await;
    Ok(format!("filled {selector}"))
}

async fn body_contains(adapter: &mut dyn PlatformAdapter, text: &str) -> Fallible<bool> {
    let raw = adapter
        .eval_js(&body_contains_js(text))
        .await
        .map_err(|e| format!("reading page text failed: {e}"))?;
    Ok(js_text(&raw) == "true")
}

async fn wait_for_text(
    adapter: &mut dyn PlatformAdapter,
    text: &str,
    timeout_ms: u64,
) -> StepOutcome {
    let started = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);
    loop {
        if body_contains(adapter, text).await? {
            return Ok(format!("found text {text:?}"));
        }
        if started.elapsed() >= timeout {
            return Err(format!(
                "text {text:?} not found in document.body.innerText after {timeout_ms} ms"
            ));
        }
        sleep_ms(POLL_MS).await;
    }
}

async fn expect_no_text(adapter: &mut dyn PlatformAdapter, text: &str) -> StepOutcome {
    if body_contains(adapter, text).await? {
        Err(format!(
            "text {text:?} is present in document.body.innerText"
        ))
    } else {
        Ok(format!("text {text:?} is absent"))
    }
}

async fn expect_url(adapter: &mut dyn PlatformAdapter, contains: &str) -> StepOutcome {
    let raw = adapter
        .eval_js("location.href")
        .await
        .map_err(|e| format!("reading location failed: {e}"))?;
    let href = js_text(&raw);
    if href.contains(contains) {
        Ok(format!("url {href} contains {contains:?}"))
    } else {
        Err(format!(
            "url is {href}, expected it to contain {contains:?}"
        ))
    }
}

async fn expect_visible(adapter: &mut dyn PlatformAdapter, selector: &str) -> StepOutcome {
    let p = probe(adapter, selector).await?;
    check_actionable(selector, &p)?;
    Ok(format!("{selector} is visible"))
}

fn expect_console_clean(actor: &Actor) -> StepOutcome {
    if actor.errors.is_empty() {
        return Ok("no console errors".to_string());
    }
    let listed: Vec<&str> = actor
        .errors
        .iter()
        .take(MAX_LISTED)
        .map(String::as_str)
        .collect();
    Err(format!(
        "actor '{}' has {} console error(s): {}",
        actor.name,
        actor.errors.len(),
        listed.join(" | ")
    ))
}

fn expect_request(
    actor: &Actor,
    method: &str,
    path_contains: &str,
    status: Option<u16>,
) -> StepOutcome {
    let hit = actor.requests.iter().any(|r| {
        r.method.eq_ignore_ascii_case(method)
            && r.url.contains(path_contains)
            && status.is_none_or(|s| r.status == s)
    });
    if hit {
        return Ok(format!("{method} *{path_contains}* observed"));
    }
    let want_status = status
        .map(|s| format!(" with status {s}"))
        .unwrap_or_default();
    Err(format!(
        "no {method} request to a URL containing {path_contains:?}{want_status} for actor '{}'; observed: {}",
        actor.name,
        describe_requests(&actor.requests)
    ))
}

fn is_failed_request(r: &NetworkRequest) -> bool {
    (r.status >= 400 || r.status == 0) && !r.url.contains("/favicon.ico")
}

fn expect_no_failed_requests(actor: &Actor) -> StepOutcome {
    let failed: Vec<NetworkRequest> = actor
        .requests
        .iter()
        .filter(|r| is_failed_request(r))
        .cloned()
        .collect();
    if failed.is_empty() {
        return Ok("no failed requests".to_string());
    }
    Err(format!(
        "actor '{}' has {} failed request(s): {}",
        actor.name,
        failed.len(),
        describe_requests(&failed)
    ))
}

fn describe_requests(reqs: &[NetworkRequest]) -> String {
    if reqs.is_empty() {
        return "none".to_string();
    }
    let mut parts: Vec<String> = reqs
        .iter()
        .take(MAX_LISTED)
        .map(|r| format!("{} {} {}", r.method, r.url, r.status))
        .collect();
    if reqs.len() > MAX_LISTED {
        parts.push(format!("... and {} more", reqs.len() - MAX_LISTED));
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::FutureExt;
    use orchyn_core::OrchynError;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use orchyn_core::{
        Action as LegacyAction, DeviceInfo, DiagnosticEvent, NetworkEvent, Platform,
    };

    /// Scripted replies for one fake adapter.
    #[derive(Clone, Default, Debug)]
    struct FakeConfig {
        /// `(substring of the JS, raw eval_js reply)`. First match wins.
        /// Unmatched JS replies `null`.
        eval: Vec<(String, String)>,
        /// One batch handed out per `collect_telemetry` call.
        telemetry: Vec<RawTelemetry>,
    }

    #[derive(Default, Debug)]
    struct Record {
        created: Vec<String>,
        /// `"actor: <action>"` for each `execute_harness` call.
        actions: Vec<String>,
        shutdowns: Vec<String>,
    }

    #[derive(Debug)]
    struct FakeAdapter {
        actor: String,
        config: FakeConfig,
        telemetry: VecDeque<RawTelemetry>,
        record: Arc<Mutex<Record>>,
    }

    impl PlatformAdapter for FakeAdapter {
        fn device_info(&self) -> DeviceInfo {
            DeviceInfo {
                platform: Platform::Web,
                screen_width: 1280,
                screen_height: 800,
            }
        }
        fn screenshot(&mut self) -> BoxFuture<'_, orchyn_core::Result<Vec<u8>>> {
            async { Ok(Vec::new()) }.boxed()
        }
        fn execute(&mut self, _action: &LegacyAction) -> BoxFuture<'_, orchyn_core::Result<()>> {
            async { Ok(()) }.boxed()
        }
        fn reset(&mut self) -> BoxFuture<'_, orchyn_core::Result<()>> {
            async { Ok(()) }.boxed()
        }
        fn diagnostics(&mut self) -> BoxFuture<'_, orchyn_core::Result<Vec<DiagnosticEvent>>> {
            async { Ok(Vec::new()) }.boxed()
        }
        fn network_events(&mut self) -> BoxFuture<'_, orchyn_core::Result<Vec<NetworkEvent>>> {
            async { Ok(Vec::new()) }.boxed()
        }
        fn get_elements(&mut self) -> BoxFuture<'_, orchyn_core::Result<Vec<serde_json::Value>>> {
            async { Ok(Vec::new()) }.boxed()
        }
        fn collect_telemetry(&mut self) -> BoxFuture<'_, orchyn_core::Result<RawTelemetry>> {
            let next = self.telemetry.pop_front().unwrap_or_default();
            async move { Ok(next) }.boxed()
        }
        fn execute_harness(
            &mut self,
            action: &HarnessAction,
            _element_map: &[AnnotatedElement],
        ) -> BoxFuture<'_, orchyn_core::Result<()>> {
            self.record
                .lock()
                .unwrap()
                .actions
                .push(format!("{}: {action:?}", self.actor));
            async { Ok(()) }.boxed()
        }
        fn shutdown(&mut self) -> BoxFuture<'_, orchyn_core::Result<()>> {
            self.record
                .lock()
                .unwrap()
                .shutdowns
                .push(self.actor.clone());
            async { Ok(()) }.boxed()
        }
        fn eval_js(&mut self, js: &str) -> BoxFuture<'_, orchyn_core::Result<String>> {
            let reply = self
                .config
                .eval
                .iter()
                .find(|(needle, _)| js.contains(needle.as_str()))
                .map(|(_, reply)| reply.clone())
                .unwrap_or_else(|| "null".to_string());
            async move { Ok(reply) }.boxed()
        }
    }

    fn fake_factory(config: FakeConfig, record: Arc<Mutex<Record>>) -> AdapterFactoryFn {
        Arc::new(move |actor: String| {
            record.lock().unwrap().created.push(actor.clone());
            let adapter: Box<dyn PlatformAdapter> = Box::new(FakeAdapter {
                actor,
                config: config.clone(),
                telemetry: config.telemetry.iter().cloned().collect(),
                record: record.clone(),
            });
            async move { Ok::<_, OrchynError>(adapter) }.boxed()
        })
    }

    /// Raw `eval_js` reply for a JS string result (quoted, as the web adapter returns it).
    fn js_string_reply(v: serde_json::Value) -> String {
        serde_json::Value::String(v.to_string()).to_string()
    }

    fn manifest(steps_json: &str) -> ReplayManifest {
        ReplayManifest::from_json(&format!(
            r#"{{"name":"t","base_url":"http://h","steps":[{steps_json}]}}"#
        ))
        .unwrap()
    }

    fn request(method: &str, url: &str, status: u16) -> NetworkRequest {
        NetworkRequest {
            method: method.to_string(),
            url: url.to_string(),
            status,
        }
    }

    fn setup(config: FakeConfig) -> (ReplayRunner, Arc<Mutex<Record>>) {
        let record = Arc::new(Mutex::new(Record::default()));
        (
            ReplayRunner::new(fake_factory(config, record.clone())),
            record,
        )
    }

    #[test]
    fn join_url_handles_slashes_and_absolute_paths() {
        assert_eq!(join_url("http://h:8092/", "/login"), "http://h:8092/login");
        assert_eq!(join_url("http://h:8092", "login"), "http://h:8092/login");
        assert_eq!(join_url("http://h", "/"), "http://h/");
        assert_eq!(join_url("http://h", "https://other/x"), "https://other/x");
    }

    #[tokio::test]
    async fn expect_request_matches_method_path_and_status() {
        let config = FakeConfig {
            telemetry: vec![RawTelemetry {
                network_requests: vec![
                    request("POST", "http://h/api/login", 200),
                    request("GET", "http://h/api/data", 404),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let (runner, _) = setup(config);
        let m = manifest(
            r#"{"op":"expect_request","method":"post","path_contains":"/api/login","status":200},
               {"op":"expect_request","method":"GET","path_contains":"/api/data","status":200},
               {"op":"expect_text","text":"never"}"#,
        );
        let report = runner.run(&m).await.unwrap();
        assert!(report.steps[0].passed, "{:?}", report.steps[0]);
        assert!(!report.steps[1].passed);
        assert!(report.steps[1].detail.contains("GET http://h/api/data 404"));
        assert!(
            report.steps[1]
                .detail
                .contains("POST http://h/api/login 200")
        );
        assert_eq!(report.first_failure, Some(1));
        assert_eq!(report.steps.len(), 2);
    }

    #[tokio::test]
    async fn expect_no_failed_requests_flags_errors_and_ignores_favicon() {
        let ok = FakeConfig {
            telemetry: vec![RawTelemetry {
                network_requests: vec![
                    request("GET", "http://h/favicon.ico", 404),
                    request("GET", "http://h/api", 200),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let (runner, _) = setup(ok);
        let m = manifest(r#"{"op":"expect_no_failed_requests"}"#);
        assert!(runner.run(&m).await.unwrap().passed);

        let bad = FakeConfig {
            telemetry: vec![RawTelemetry {
                network_requests: vec![request("GET", "http://h/api", 500)],
                ..Default::default()
            }],
            ..Default::default()
        };
        let (runner, _) = setup(bad);
        let report = runner.run(&m).await.unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("GET http://h/api 500"));
    }

    #[tokio::test]
    async fn console_clean_fails_after_error_and_passes_after_reset() {
        let config = FakeConfig {
            telemetry: vec![RawTelemetry {
                console_logs: vec![ConsoleLog {
                    level: "error".to_string(),
                    message: "boom".to_string(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let (runner, _) = setup(config.clone());
        let failing = manifest(r#"{"op":"expect_console_clean"}"#);
        let report = runner.run(&failing).await.unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("boom"));

        let (runner, _) = setup(config);
        let reset = manifest(r#"{"op":"reset_telemetry"},{"op":"expect_console_clean"}"#);
        let report = runner.run(&reset).await.unwrap();
        assert!(report.passed, "{:?}", report.steps);
        // The run-wide report keeps errors even after a reset.
        assert_eq!(report.console_errors, vec!["default: boom".to_string()]);
    }

    #[tokio::test]
    async fn failure_halts_run_and_later_steps_are_not_executed() {
        let (runner, record) = setup(FakeConfig::default());
        let m = manifest(
            r##"{"op":"expect_no_text","text":"Secret"},
               {"op":"expect_text","text":"Missing","timeout_ms":150},
               {"op":"click","selector":"#later"}"##,
        );
        let report = runner.run(&m).await.unwrap();
        assert!(!report.passed);
        assert_eq!(report.total_steps, 3);
        assert_eq!(report.steps.len(), 2);
        assert_eq!(report.first_failure, Some(1));
        assert!(report.steps[1].detail.contains("Missing"));
        assert!(record.lock().unwrap().actions.is_empty());
    }

    #[tokio::test]
    async fn covered_element_click_fails_with_covered() {
        let config = FakeConfig {
            eval: vec![(
                "elementFromPoint".to_string(),
                js_string_reply(serde_json::json!({
                    "found": true,
                    "visible": true,
                    "covered": true,
                    "tag": "BUTTON",
                    "coverer": "DIV.overlay",
                    "x": 10.0, "y": 10.0, "w": 80.0, "h": 30.0,
                })),
            )],
            ..Default::default()
        };
        let (runner, record) = setup(config);
        let m = manifest(r##"{"op":"click","selector":"#submit"}"##);
        let report = runner.run(&m).await.unwrap();
        assert!(!report.passed);
        let detail = &report.steps[0].detail;
        assert!(detail.contains("covered"), "{detail}");
        assert!(detail.contains("DIV.overlay"), "{detail}");
        assert!(record.lock().unwrap().actions.is_empty());
    }

    #[tokio::test]
    async fn visible_click_performs_real_click() {
        let config = FakeConfig {
            eval: vec![(
                "elementFromPoint".to_string(),
                js_string_reply(serde_json::json!({
                    "found": true, "visible": true, "covered": false, "tag": "BUTTON",
                    "x": 10.0, "y": 10.0, "w": 80.0, "h": 30.0,
                })),
            )],
            ..Default::default()
        };
        let (runner, record) = setup(config);
        let m = manifest(r##"{"op":"click","selector":"#submit"}"##);
        let report = runner.run(&m).await.unwrap();
        assert!(report.passed, "{:?}", report.steps);
        let actions = record.lock().unwrap().actions.clone();
        assert_eq!(actions, vec!["default: Click { target: 0 }".to_string()]);
    }

    #[tokio::test]
    async fn per_actor_adapters_are_created_once_each() {
        let (runner, record) = setup(FakeConfig::default());
        let m = manifest(
            r#"{"op":"expect_no_text","text":"x"},
               {"actor":"bob","op":"expect_no_text","text":"x"},
               {"op":"expect_no_text","text":"x"},
               {"actor":"bob","op":"expect_no_text","text":"x"}"#,
        );
        let report = runner.run(&m).await.unwrap();
        assert!(report.passed);
        let rec = record.lock().unwrap();
        assert_eq!(rec.created, vec!["default".to_string(), "bob".to_string()]);
        let mut shutdowns = rec.shutdowns.clone();
        shutdowns.sort();
        assert_eq!(shutdowns, vec!["bob".to_string(), "default".to_string()]);
    }

    #[tokio::test]
    async fn factory_failure_is_the_only_run_error() {
        let factory: AdapterFactoryFn = Arc::new(|_actor: String| {
            async { Err::<Box<dyn PlatformAdapter>, _>(OrchynError::Platform("no browser".into())) }
                .boxed()
        });
        let m = manifest(r#"{"op":"expect_no_text","text":"x"}"#);
        assert!(ReplayRunner::new(factory).run(&m).await.is_err());
    }
}
