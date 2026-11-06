//! Deterministic replay runner ("Constraint Mode").
//!
//! Executes a [`ReplayManifest`] against platform adapters with no model in the
//! loop. Each actor gets its own adapter, created lazily on its first step.
//! Clicks and typing go through `execute_harness` so they are real pointer and
//! keyboard input. Element lookup and assertions use `eval_js` probes.
//!
//! Failures are recorded in the report and halt the run. Only a factory failure
//! makes [`ReplayRunner::run`] return `Err`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use orchyn_core::action::{Action as HarnessAction, AnnotatedElement, BoundingBox};
use orchyn_core::{ConsoleLog, NetworkRequest, PlatformAdapter, RawTelemetry};
use serde::{Deserialize, Serialize};

use super::manifest::{ReplayManifest, StepOp};
use super::snapshot::PageSnapshot;

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
    /// API (fetch/XHR) exchanges with bodies, all actors, in order. Used to
    /// mine invariants from known-good runs.
    #[serde(default)]
    pub observed_exchanges: Vec<orchyn_core::Exchange>,
    /// Page state after each successful step that can change the page, one per
    /// actor-step, in run order. Snapshot failures are skipped silently.
    #[serde(default)]
    pub page_snapshots: Vec<PageSnapshot>,
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
            observed_exchanges: Vec::new(),
            page_snapshots: Vec::new(),
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
            if passed && records_page_snapshot(&step.op) {
                let snap = take_page_snapshot(
                    &mut *session.actors[i].adapter,
                    &step.actor,
                    index,
                    step.op.name(),
                )
                .await;
                session.page_snapshots.extend(snap);
            }
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
            observed_exchanges: session.observed_exchanges,
            page_snapshots: session.page_snapshots,
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
    /// Normalised page text saved by `snapshot_text`, keyed by its `key`.
    snapshots: HashMap<String, String>,
    /// Messages of JavaScript dialogs shown since the start or the last reset.
    dialogs: Vec<String>,
}

/// Run-wide state. Lives for one `run` call.
struct Session {
    factory: AdapterFactoryFn,
    base_url: String,
    actors: Vec<Actor>,
    observed_requests: Vec<NetworkRequest>,
    console_errors: Vec<String>,
    observed_exchanges: Vec<orchyn_core::Exchange>,
    page_snapshots: Vec<PageSnapshot>,
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
            dialogs: Vec::new(),
            snapshots: HashMap::new(),
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
                self.actors[i].dialogs.clear();
                Ok("telemetry reset".to_string())
            }
            StepOp::ExpectDialog { contains } => {
                self.pull(i).await?;
                let a = &self.actors[i];
                if a.dialogs.iter().any(|d| d.contains(contains.as_str())) {
                    Ok(format!("dialog containing {contains:?} was shown"))
                } else if a.dialogs.is_empty() {
                    Err(format!("no dialog was shown to actor '{}'", a.name))
                } else {
                    Err(format!(
                        "no dialog containing {contains:?}; shown: {}",
                        a.dialogs.join(" | ")
                    ))
                }
            }
            StepOp::ExpectAllClickable => expect_all_clickable(&mut *self.actors[i].adapter).await,
            StepOp::Reload => reload(&mut *self.actors[i].adapter).await,
            StepOp::SnapshotText { key } => {
                let text = read_body_text(&mut *self.actors[i].adapter).await?;
                let lines = text.lines().count();
                self.actors[i].snapshots.insert(key.clone(), text);
                Ok(format!("stored {lines} line(s) of text under '{key}'"))
            }
            StepOp::ExpectTextUnchanged { key } => {
                let before = self.actors[i].snapshots.get(key).cloned();
                expect_text_unchanged(&mut *self.actors[i].adapter, before, key).await
            }
            StepOp::ClickEach { selector } => self.click_each(i, selector.as_deref()).await,
            StepOp::Hover { selector } => hover(&mut *self.actors[i].adapter, selector).await,
            StepOp::Drag {
                selector,
                target,
                position,
            } => {
                drag(
                    &mut *self.actors[i].adapter,
                    selector,
                    target,
                    position.as_deref(),
                )
                .await
            }
            StepOp::ExpectValue { selector, value } => {
                expect_value(&mut *self.actors[i].adapter, selector, value).await
            }
            StepOp::ExpectCount { selector, count } => {
                expect_count(&mut *self.actors[i].adapter, selector, *count).await
            }
            StepOp::ExpectStorage { key, contains } => {
                expect_storage(&mut *self.actors[i].adapter, key, contains.as_deref()).await
            }
            StepOp::SetStorage { key, value } => {
                set_storage(&mut *self.actors[i].adapter, key, value).await
            }
            StepOp::ExpectSameText { key, other_actor } => {
                match lookup_snapshot(&self.actors, other_actor, key) {
                    Ok(expected) => {
                        expect_same_text(&mut *self.actors[i].adapter, &expected, other_actor, key)
                            .await
                    }
                    Err(detail) => Err(detail),
                }
            }
            StepOp::ExpectAttribute {
                selector,
                name,
                value,
            } => {
                expect_attribute(
                    &mut *self.actors[i].adapter,
                    selector,
                    name,
                    value.as_deref(),
                )
                .await
            }
        }
    }

    /// Monkey pass: really clicks each candidate in DOM order. Each candidate is
    /// re-located by index before its click, so a page that changed under us is
    /// handled. Telemetry is pulled after every click, and only what that click
    /// produced is judged. A click that moved the page is undone before the next.
    async fn click_each(&mut self, i: usize, selector: Option<&str>) -> StepOutcome {
        let selector = selector.unwrap_or(DEFAULT_CLICK_EACH_SELECTOR);
        let origin = current_url(&mut *self.actors[i].adapter).await?;
        let mut total = 0;
        let mut clicked = 0;
        let mut skipped: Vec<String> = Vec::new();
        let mut problems: Vec<String> = Vec::new();

        for k in 0..MAX_CLICK_EACH {
            let mark = mark_nth(&mut *self.actors[i].adapter, selector, k).await?;
            if k == 0 {
                total = mark.total;
            }
            if !mark.found {
                break;
            }
            let p = probe(&mut *self.actors[i].adapter, MARKER_SELECTOR).await?;
            if !p.found || !p.visible {
                skipped.push(format!("{} (not visible)", mark.label));
                continue;
            }
            if p.covered {
                let by = p
                    .coverer
                    .as_deref()
                    .unwrap_or("something outside the viewport");
                skipped.push(format!("{} covered by {by}", mark.label));
                continue;
            }

            let requests_before = self.actors[i].requests.len();
            let errors_before = self.actors[i].errors.len();
            self.actors[i]
                .adapter
                .execute_harness(&HarnessAction::Click { target: 0 }, &[annotate(&p)])
                .await
                .map_err(|e| format!("mouse click on {} failed: {e}", mark.label))?;
            sleep_ms(CLICK_EACH_SETTLE_MS).await;
            self.pull(i).await?;
            clicked += 1;

            let actor = &self.actors[i];
            for message in &actor.errors[errors_before..] {
                problems.push(format!("{} logged a console error: {message}", mark.label));
            }
            for request in &actor.requests[requests_before..] {
                if is_failed_request(request) {
                    problems.push(format!(
                        "{} caused {} {} {}",
                        mark.label, request.method, request.url, request.status
                    ));
                }
            }

            let now = current_url(&mut *self.actors[i].adapter).await?;
            if now != origin {
                goto(&mut *self.actors[i].adapter, &origin).await?;
            }
        }
        // Best effort: the marker is only a way to address the element.
        let _ = self.actors[i].adapter.eval_js(CLEAR_MARKER_JS).await;

        if !problems.is_empty() {
            let listed: Vec<&str> = problems
                .iter()
                .take(MAX_LISTED)
                .map(String::as_str)
                .collect();
            return Err(format!(
                "{} problem(s) while clicking each element on {origin}: {}",
                problems.len(),
                listed.join(" | ")
            ));
        }
        let considered = total.min(MAX_CLICK_EACH);
        let mut detail = format!("clicked {clicked} of {considered} element(s) on {origin}");
        if !skipped.is_empty() {
            detail.push_str(&format!("; skipped: {}", skipped.join(", ")));
        }
        Ok(detail)
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
            if let Some(d) = message.strip_prefix("[dialog] ") {
                actor.dialogs.push(d.to_string());
                continue;
            }
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
        // Exchanges are best-effort: adapters without body capture return none.
        if let Ok(ex) = actor.adapter.take_exchanges().await {
            self.observed_exchanges.extend(ex);
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
    // Chrome logs every 4xx/5xx response as "Failed to load resource". Those
    // are network outcomes, judged by `expect_no_failed_requests` (which knows
    // to ignore the browser's automatic /favicon.ico probe), not script errors.
    if message.starts_with("Failed to load resource") {
        return false;
    }
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

// ---------------------------------------------------------------------------
// Universal checks: clickability, reload stability, monkey clicking
// ---------------------------------------------------------------------------

const DEFAULT_CLICK_EACH_SELECTOR: &str = "button, input[type=submit], [role=button]";
const CLICKABLE_SELECTOR: &str =
    "button, a[href], input:not([type=hidden]), select, textarea, [role=button]";
const MAX_CLICK_EACH: usize = 20;
const CLICK_EACH_SETTLE_MS: u64 = 300;
const RELOAD_START_MS: u64 = 100;
const RELOAD_TIMEOUT_MS: u64 = 5000;
const RELOAD_SETTLE_MS: u64 = 300;
const MAX_OFFENDERS: usize = 5;
const TEXT_LINE_MAX_CHARS: usize = 120;
const BODY_TEXT_JS: &str = "(document.body ? document.body.innerText : '')";

/// Shared JS helpers, spliced into probe scripts. Plain strings: no format braces.
const VISIBLE_JS: &str = r#"const visible = (e) => {
    const r = e.getBoundingClientRect();
    const cs = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && cs.display !== 'none' && cs.visibility !== 'hidden';
  };"#;
const DESCRIBE_JS: &str = r#"const describe = (n) => {
    let s = n.tagName;
    if (n.id) s += '#' + n.id;
    const cls = (typeof n.className === 'string' ? n.className : '').trim().split(/\s+/).filter(Boolean);
    if (cls.length) s += '.' + cls.join('.');
    return s;
  };"#;

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClickableReport {
    checked: usize,
    offenders: Vec<String>,
}

/// One candidate for `click_each`, as located by [`mark_nth_js`].
#[derive(Deserialize, Default)]
#[serde(default)]
struct Marked {
    total: usize,
    found: bool,
    label: String,
}

/// Scrolls each visible, enabled interactive element into view and checks that
/// `elementFromPoint` at its centre is the element or one of its descendants.
fn all_clickable_js() -> String {
    let sel = js_str(CLICKABLE_SELECTOR);
    let visible = VISIBLE_JS;
    let describe = DESCRIBE_JS;
    format!(
        r#"(() => {{
  const selector = {sel};
  {visible}
  {describe}
  const elements = Array.from(document.querySelectorAll(selector)).filter((e) => visible(e) && !e.disabled);
  const offenders = [];
  for (const el of elements) {{
    el.scrollIntoView({{ block: 'center', inline: 'center' }});
    const r = el.getBoundingClientRect();
    const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    if (top === null || !el.contains(top)) {{
      offenders.push(describe(el) + ' covered by ' + (top ? describe(top) : 'nothing (outside the viewport)'));
    }}
  }}
  return JSON.stringify({{ checked: elements.length, offenders }});
}})()"#
    )
}

/// Marks the `k`th visible, enabled element matching `selector` with the marker
/// attribute and reports how many there are in total.
fn mark_nth_js(selector: &str, k: usize) -> String {
    let attr = MARKER_ATTR;
    let sel = js_str(selector);
    let visible = VISIBLE_JS;
    let describe = DESCRIBE_JS;
    format!(
        r#"(() => {{
  const attr = '{attr}';
  document.querySelectorAll('[' + attr + ']').forEach((e) => e.removeAttribute(attr));
  {visible}
  {describe}
  const candidates = Array.from(document.querySelectorAll({sel})).filter((e) => visible(e) && !e.disabled);
  const hit = candidates[{k}];
  if (!hit) return JSON.stringify({{ total: candidates.length, found: false }});
  hit.setAttribute(attr, '');
  return JSON.stringify({{ total: candidates.length, found: true, label: describe(hit) }});
}})()"#
    )
}

async fn mark_nth(adapter: &mut dyn PlatformAdapter, selector: &str, k: usize) -> Fallible<Marked> {
    let raw = adapter
        .eval_js(&mark_nth_js(selector, k))
        .await
        .map_err(|e| format!("locating {selector} failed: {e}"))?;
    serde_json::from_str(&js_text(&raw))
        .map_err(|e| format!("unreadable element list for {selector}: {e}"))
}

async fn current_url(adapter: &mut dyn PlatformAdapter) -> Fallible<String> {
    let raw = adapter
        .eval_js("location.href")
        .await
        .map_err(|e| format!("reading location failed: {e}"))?;
    Ok(js_text(&raw))
}

async fn expect_all_clickable(adapter: &mut dyn PlatformAdapter) -> StepOutcome {
    let raw = adapter
        .eval_js(&all_clickable_js())
        .await
        .map_err(|e| format!("checking clickable elements failed: {e}"))?;
    let report: ClickableReport = serde_json::from_str(&js_text(&raw))
        .map_err(|e| format!("unreadable clickability report: {e}"))?;
    if report.offenders.is_empty() {
        return Ok(format!(
            "all {} interactive element(s) can be clicked",
            report.checked
        ));
    }
    let listed: Vec<&str> = report
        .offenders
        .iter()
        .take(MAX_OFFENDERS)
        .map(String::as_str)
        .collect();
    Err(format!(
        "{} of {} interactive element(s) cannot be clicked: {}",
        report.offenders.len(),
        report.checked,
        listed.join("; ")
    ))
}

async fn reload(adapter: &mut dyn PlatformAdapter) -> StepOutcome {
    // Reload on a later tick so this eval returns before the page unloads.
    adapter
        .eval_js("setTimeout(() => location.reload(), 0); 'reloading'")
        .await
        .map_err(|e| format!("reload failed: {e}"))?;
    sleep_ms(RELOAD_START_MS).await;
    let started = Instant::now();
    loop {
        // Evals can fail while the page is unloading; that just means not ready.
        let ready = adapter
            .eval_js("document.readyState === 'complete'")
            .await
            .map(|raw| js_text(&raw) == "true")
            .unwrap_or(false);
        if ready {
            break;
        }
        if started.elapsed() >= Duration::from_millis(RELOAD_TIMEOUT_MS) {
            return Err(format!(
                "page did not finish loading within {RELOAD_TIMEOUT_MS} ms of reload"
            ));
        }
        sleep_ms(POLL_MS).await;
    }
    sleep_ms(RELOAD_SETTLE_MS).await;
    Ok("reloaded and loaded".to_string())
}

/// Trims each line, collapses whitespace runs inside it and drops blank lines.
fn normalise_text(raw: &str) -> String {
    raw.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

async fn read_body_text(adapter: &mut dyn PlatformAdapter) -> Fallible<String> {
    let raw = adapter
        .eval_js(BODY_TEXT_JS)
        .await
        .map_err(|e| format!("reading page text failed: {e}"))?;
    Ok(normalise_text(&js_text(&raw)))
}

async fn expect_text_unchanged(
    adapter: &mut dyn PlatformAdapter,
    before: Option<String>,
    key: &str,
) -> StepOutcome {
    let Some(before) = before else {
        return Err(format!(
            "no snapshot '{key}' was taken for this actor (snapshot_text must come first)"
        ));
    };
    let after = read_body_text(adapter).await?;
    if after == before {
        return Ok(format!("text unchanged since snapshot '{key}'"));
    }
    Err(text_change_detail(key, &before, &after))
}

/// Names the first line that differs in each text, truncated for the report.
fn text_change_detail(key: &str, before: &str, after: &str) -> String {
    format!(
        "text changed since snapshot '{key}' {}",
        describe_first_difference(before, after, "before", "after")
    )
}

/// Names the first line at which two texts differ, with each side's line
/// (truncated) under the given label.
fn describe_first_difference(
    first: &str,
    second: &str,
    first_label: &str,
    second_label: &str,
) -> String {
    let first_lines: Vec<&str> = first.lines().collect();
    let second_lines: Vec<&str> = second.lines().collect();
    let at = (0..first_lines.len().max(second_lines.len()))
        .find(|&n| first_lines.get(n) != second_lines.get(n))
        .unwrap_or(0);
    let show = |line: Option<&&str>| match line {
        Some(line) => truncate_chars(line, TEXT_LINE_MAX_CHARS),
        None => "<end of text>".to_string(),
    };
    format!(
        "at line {}: {first_label}: {} | {second_label}: {}",
        at + 1,
        show(first_lines.get(at)),
        show(second_lines.get(at)),
    )
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max - 3).collect();
    format!("{head}...")
}

// ---------------------------------------------------------------------------
// Pointer, drag, form, storage and attribute steps
// ---------------------------------------------------------------------------

const HOVER_SETTLE_MS: u64 = 250;
const DRAG_SETTLE_MS: u64 = 300;
const SNAPSHOT_VISIBLE_CAP: usize = 500;

/// Fails unless the element exists and has non-zero size. Covering is allowed.
fn check_present_and_sized(label: &str, p: &Probe) -> Fallible<()> {
    if !p.found {
        return Err(format!("element {label} not found on the page"));
    }
    if !p.visible {
        return Err(format!(
            "element {label} is not visible (zero size, display:none or visibility:hidden)"
        ));
    }
    Ok(())
}

/// Dispatches the pointer and mouse enter/over events on the element at its
/// centre. Returns `missing` if the element has gone.
fn hover_js(selector: &str) -> String {
    format!(
        r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return 'missing';
  const r = el.getBoundingClientRect();
  const opts = {{
    bubbles: true, cancelable: true, view: window,
    clientX: r.left + r.width / 2, clientY: r.top + r.height / 2,
  }};
  const Pointer = typeof PointerEvent === 'function' ? PointerEvent : MouseEvent;
  el.dispatchEvent(new Pointer('pointerover', opts));
  el.dispatchEvent(new Pointer('pointerenter', {{ ...opts, bubbles: false }}));
  el.dispatchEvent(new MouseEvent('mouseover', opts));
  el.dispatchEvent(new MouseEvent('mouseenter', {{ ...opts, bubbles: false }}));
  return 'ok';
}})()"#,
        sel = js_str(selector)
    )
}

async fn hover(adapter: &mut dyn PlatformAdapter, selector: &str) -> StepOutcome {
    let p = probe(adapter, selector).await?;
    check_actionable(selector, &p)?;
    let raw = adapter
        .eval_js(&hover_js(selector))
        .await
        .map_err(|e| format!("hovering {selector} failed: {e}"))?;
    if js_text(&raw) != "ok" {
        return Err(format!("element {selector} disappeared before hovering"));
    }
    sleep_ms(HOVER_SETTLE_MS).await;
    Ok(format!("hovered {selector} (JS hover events only)"))
}

/// Fraction of the target's height at which the drop lands.
fn drop_fraction(position: Option<&str>) -> (f64, &'static str) {
    match position {
        Some("before") => (0.25, "before"),
        Some("after") => (0.75, "after"),
        _ => (0.5, "centre"),
    }
}

/// Synthesises an HTML5 drag from `src` to `dst` with one shared DataTransfer.
fn drag_js(selector: &str, target: &str, fraction: f64) -> String {
    format!(
        r#"(() => {{
  const src = document.querySelector({src});
  const dst = document.querySelector({dst});
  if (!src || !dst) return 'missing';
  const s = src.getBoundingClientRect();
  const t = dst.getBoundingClientRect();
  const sx = s.left + s.width / 2, sy = s.top + s.height / 2;
  const tx = t.left + t.width / 2, ty = t.top + t.height * {fraction};
  const dt = new DataTransfer();
  const ev = (x, y) => ({{ bubbles: true, cancelable: true, view: window, clientX: x, clientY: y, dataTransfer: dt }});
  src.dispatchEvent(new DragEvent('dragstart', ev(sx, sy)));
  dst.dispatchEvent(new DragEvent('dragenter', ev(tx, ty)));
  dst.dispatchEvent(new DragEvent('dragover', ev(tx, ty)));
  dst.dispatchEvent(new DragEvent('drop', ev(tx, ty)));
  src.dispatchEvent(new DragEvent('dragend', ev(sx, sy)));
  return 'ok';
}})()"#,
        src = js_str(selector),
        dst = js_str(target),
    )
}

async fn drag(
    adapter: &mut dyn PlatformAdapter,
    selector: &str,
    target: &str,
    position: Option<&str>,
) -> StepOutcome {
    let src = probe(adapter, selector).await?;
    check_present_and_sized(&format!("drag source {selector}"), &src)?;
    let dst = probe(adapter, target).await?;
    check_present_and_sized(&format!("drag target {target}"), &dst)?;
    let (fraction, mode) = drop_fraction(position);
    let raw = adapter
        .eval_js(&drag_js(selector, target, fraction))
        .await
        .map_err(|e| format!("dragging {selector} onto {target} failed: {e}"))?;
    if js_text(&raw) != "ok" {
        return Err("drag source or target disappeared before dropping".to_string());
    }
    sleep_ms(DRAG_SETTLE_MS).await;
    Ok(format!("dragged {selector} onto {target} ({mode})"))
}

/// Reads `.value` of the element, or of the checked member when the selector
/// matches a radio/checkbox group. Checkboxes and radios with no group report
/// `checked` as "true"/"false".
fn value_js(selector: &str) -> String {
    format!(
        r#"(() => {{
  const els = Array.from(document.querySelectorAll({sel}));
  if (els.length === 0) return JSON.stringify({{ found: false }});
  const isChoice = (e) => e.tagName === 'INPUT' && (e.type === 'radio' || e.type === 'checkbox');
  let value = null;
  if (els.length > 1 && els.every(isChoice)) {{
    const on = els.find((e) => e.checked);
    value = on ? on.value : '';
  }} else if (isChoice(els[0])) {{
    value = String(els[0].checked);
  }} else if ('value' in els[0]) {{
    value = String(els[0].value);
  }}
  return JSON.stringify({{ found: true, value }});
}})()"#,
        sel = js_str(selector)
    )
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ValueRead {
    found: bool,
    value: Option<String>,
}

async fn expect_value(
    adapter: &mut dyn PlatformAdapter,
    selector: &str,
    expected: &str,
) -> StepOutcome {
    let raw = adapter
        .eval_js(&value_js(selector))
        .await
        .map_err(|e| format!("reading the value of {selector} failed: {e}"))?;
    let read: ValueRead = serde_json::from_str(&js_text(&raw))
        .map_err(|e| format!("unreadable value for {selector}: {e}"))?;
    if !read.found {
        return Err(format!("element {selector} not found on the page"));
    }
    let Some(actual) = read.value else {
        return Err(format!("element {selector} has no value property"));
    };
    if actual == expected {
        Ok(format!("{selector} has value {expected:?}"))
    } else {
        Err(format!(
            "{selector} has value {actual:?}, expected {expected:?}"
        ))
    }
}

async fn expect_count(
    adapter: &mut dyn PlatformAdapter,
    selector: &str,
    expected: usize,
) -> StepOutcome {
    let js = format!("document.querySelectorAll({}).length", js_str(selector));
    let raw = adapter
        .eval_js(&js)
        .await
        .map_err(|e| format!("counting {selector} failed: {e}"))?;
    let actual: usize = js_text(&raw)
        .trim()
        .parse()
        .map_err(|_| format!("unreadable element count for {selector}: {}", js_text(&raw)))?;
    if actual == expected {
        Ok(format!("{actual} element(s) match {selector}"))
    } else {
        Err(format!(
            "{actual} element(s) match {selector}, expected {expected}"
        ))
    }
}

fn storage_get_js(key: &str) -> String {
    format!(
        "(() => {{ try {{ const v = localStorage.getItem({k}); \
         return JSON.stringify({{ found: v !== null, value: v }}); }} \
         catch (e) {{ return JSON.stringify({{ found: false, error: String(e) }}); }} }})()",
        k = js_str(key)
    )
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StorageRead {
    found: bool,
    value: Option<String>,
    error: Option<String>,
}

async fn expect_storage(
    adapter: &mut dyn PlatformAdapter,
    key: &str,
    contains: Option<&str>,
) -> StepOutcome {
    let raw = adapter
        .eval_js(&storage_get_js(key))
        .await
        .map_err(|e| format!("reading localStorage failed: {e}"))?;
    let read: StorageRead = serde_json::from_str(&js_text(&raw))
        .map_err(|e| format!("unreadable localStorage result: {e}"))?;
    if let Some(error) = read.error {
        return Err(format!("localStorage is not available: {error}"));
    }
    if !read.found {
        return Err(format!("localStorage has no key {key:?}"));
    }
    let value = read.value.unwrap_or_default();
    match contains {
        Some(needle) if !value.contains(needle) => Err(format!(
            "localStorage[{key:?}] is {}, expected it to contain {needle:?}",
            truncate_chars(&value, TEXT_LINE_MAX_CHARS)
        )),
        _ => Ok(format!("localStorage has key {key:?}")),
    }
}

async fn set_storage(adapter: &mut dyn PlatformAdapter, key: &str, value: &str) -> StepOutcome {
    let js = format!(
        "(() => {{ try {{ localStorage.setItem({k}, {v}); return 'ok'; }} \
         catch (e) {{ return 'error: ' + String(e); }} }})()",
        k = js_str(key),
        v = js_str(value),
    );
    let raw = adapter
        .eval_js(&js)
        .await
        .map_err(|e| format!("setting localStorage {key:?} failed: {e}"))?;
    let reply = js_text(&raw);
    if reply == "ok" {
        Ok(format!("stored {key:?} in localStorage"))
    } else {
        Err(format!("could not set localStorage {key:?}: {reply}"))
    }
}

/// The normalised text `owner` stored under `key`. Snapshots are kept per actor.
fn lookup_snapshot(actors: &[Actor], owner: &str, key: &str) -> Fallible<String> {
    let actor = actors.iter().find(|a| a.name == owner).ok_or_else(|| {
        format!("no actor named '{owner}' has run a step, so there is no snapshot '{key}'")
    })?;
    actor.snapshots.get(key).cloned().ok_or_else(|| {
        format!("actor '{owner}' took no snapshot '{key}' (its snapshot_text must come first)")
    })
}

async fn expect_same_text(
    adapter: &mut dyn PlatformAdapter,
    expected: &str,
    owner: &str,
    key: &str,
) -> StepOutcome {
    let actual = read_body_text(adapter).await?;
    if actual == expected {
        return Ok(format!("text matches actor '{owner}'s snapshot '{key}'"));
    }
    Err(format!(
        "text differs from actor '{owner}'s snapshot '{key}' {}",
        describe_first_difference(expected, &actual, "expected", "actual")
    ))
}

/// Reads the attribute's presence and value. `has` is false when absent.
fn attribute_js(selector: &str, name: &str) -> String {
    format!(
        r#"(() => {{
  const el = document.querySelector({sel});
  if (!el) return JSON.stringify({{ found: false }});
  const n = {name};
  return JSON.stringify({{ found: true, has: el.hasAttribute(n), value: el.getAttribute(n) }});
}})()"#,
        sel = js_str(selector),
        name = js_str(name),
    )
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AttributeRead {
    found: bool,
    has: bool,
    value: Option<String>,
}

async fn expect_attribute(
    adapter: &mut dyn PlatformAdapter,
    selector: &str,
    name: &str,
    expected: Option<&str>,
) -> StepOutcome {
    let raw = adapter
        .eval_js(&attribute_js(selector, name))
        .await
        .map_err(|e| format!("reading attributes of {selector} failed: {e}"))?;
    let read: AttributeRead = serde_json::from_str(&js_text(&raw))
        .map_err(|e| format!("unreadable attributes for {selector}: {e}"))?;
    if !read.found {
        return Err(format!("element {selector} not found on the page"));
    }
    if !read.has {
        return Err(format!("element {selector} has no {name} attribute"));
    }
    match expected {
        None => Ok(format!("{selector} has attribute {name}")),
        Some(want) => {
            let actual = read.value.unwrap_or_default();
            if actual == want {
                Ok(format!("{selector} has {name}={want:?}"))
            } else {
                Err(format!(
                    "{selector} has {name}={actual:?}, expected {want:?}"
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Page snapshots
// ---------------------------------------------------------------------------

/// Steps after which a page snapshot is recorded. Assertions never are.
fn records_page_snapshot(op: &StepOp) -> bool {
    matches!(
        op,
        StepOp::Goto { .. }
            | StepOp::Click { .. }
            | StepOp::ClickText { .. }
            | StepOp::Fill { .. }
            | StepOp::WaitMs { .. }
            | StepOp::WaitForText { .. }
            | StepOp::Reload
            | StepOp::ClickEach { .. }
            | StepOp::Drag { .. }
            | StepOp::Hover { .. }
            | StepOp::SetStorage { .. }
    )
}

/// One round-trip: URL, visible text lines, and stable descriptions of the
/// visible elements that have an id or a class. Plain string, no format braces.
const PAGE_SNAPSHOT_JS: &str = r#"(() => {
  const body = document.body ? document.body.innerText : '';
  const lines = body.split('\n').map((s) => s.trim()).filter((s) => s.length > 0);
  const seen = new Set();
  for (const el of Array.from(document.querySelectorAll('[id], [class]'))) {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    const shown = r.width > 0 && r.height > 0 && cs.display !== 'none'
      && cs.visibility !== 'hidden' && cs.opacity !== '0';
    if (!shown) continue;
    const tag = el.tagName.toLowerCase();
    if (el.id) {
      seen.add(tag + '#' + el.id);
    } else {
      const classes = Array.from(el.classList).sort();
      if (classes.length > 0) seen.add(tag + '.' + classes.join('.'));
    }
  }
  const visible = Array.from(seen).sort().slice(0, 500);
  return JSON.stringify({
    url: location.origin + location.pathname,
    lines,
    visible,
  });
})()"#;

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawPageSnapshot {
    url: String,
    lines: Vec<String>,
    visible: Vec<String>,
}

/// Best effort: any failure yields `None` and the step is unaffected.
async fn take_page_snapshot(
    adapter: &mut dyn PlatformAdapter,
    actor: &str,
    step: usize,
    op: &str,
) -> Option<PageSnapshot> {
    let raw = adapter.eval_js(PAGE_SNAPSHOT_JS).await.ok()?;
    let parsed: RawPageSnapshot = serde_json::from_str(&js_text(&raw)).ok()?;
    let mut visible = parsed.visible;
    visible.truncate(SNAPSHOT_VISIBLE_CAP);
    Some(PageSnapshot {
        actor: actor.to_string(),
        step,
        op: op.to_string(),
        url: parsed.url,
        lines: parsed.lines,
        visible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_load_failures_are_not_console_errors() {
        assert!(!is_console_error(
            "error",
            "Failed to load resource: the server responded with a status of 404 (Not Found)"
        ));
        assert!(is_console_error("error", "TypeError: x is null"));
        assert!(is_console_error("log", "Uncaught (in promise) TypeError"));
    }
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
        /// `(substring, replies)`: while replies remain, each matching eval
        /// gets the next one. Checked before `eval`, so page state can change
        /// between reads.
        sequences: Vec<(String, Vec<String>)>,
    }

    #[derive(Default, Debug)]
    struct Record {
        created: Vec<String>,
        /// `"actor: <action>"` for each `execute_harness` call.
        actions: Vec<String>,
        shutdowns: Vec<String>,
        /// Every JS string passed to `eval_js`, in order.
        evals: Vec<String>,
    }

    #[derive(Debug)]
    struct FakeAdapter {
        actor: String,
        config: FakeConfig,
        telemetry: VecDeque<RawTelemetry>,
        sequences: Vec<(String, VecDeque<String>)>,
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
            self.record.lock().unwrap().evals.push(js.to_string());
            if let Some((_, replies)) = self
                .sequences
                .iter_mut()
                .find(|(needle, replies)| !replies.is_empty() && js.contains(needle.as_str()))
            {
                let reply = replies.pop_front().unwrap_or_default();
                return async move { Ok(reply) }.boxed();
            }
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
                sequences: config
                    .sequences
                    .iter()
                    .map(|(needle, replies)| (needle.clone(), replies.iter().cloned().collect()))
                    .collect(),
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

    #[tokio::test]
    async fn expect_all_clickable_passes_when_nothing_is_covered() {
        let config = FakeConfig {
            eval: vec![(
                "offenders".to_string(),
                js_string_reply(serde_json::json!({ "checked": 3, "offenders": [] })),
            )],
            ..Default::default()
        };
        let (runner, _) = setup(config);
        let report = runner
            .run(&manifest(r#"{"op":"expect_all_clickable"}"#))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);
        assert!(report.steps[0].detail.contains("3 interactive"));
    }

    #[tokio::test]
    async fn expect_all_clickable_lists_at_most_five_offenders() {
        let offenders: Vec<String> = (0..6)
            .map(|i| format!("BUTTON#b{i}.btn covered by DIV.overlay"))
            .collect();
        let config = FakeConfig {
            eval: vec![(
                "offenders".to_string(),
                js_string_reply(serde_json::json!({ "checked": 9, "offenders": offenders })),
            )],
            ..Default::default()
        };
        let (runner, _) = setup(config);
        let report = runner
            .run(&manifest(r#"{"op":"expect_all_clickable"}"#))
            .await
            .unwrap();
        assert!(!report.passed);
        let detail = &report.steps[0].detail;
        assert!(detail.contains("6 of 9"), "{detail}");
        assert!(
            detail.contains("BUTTON#b4.btn covered by DIV.overlay"),
            "{detail}"
        );
        assert!(!detail.contains("BUTTON#b5"), "{detail}");
    }

    #[tokio::test]
    async fn reload_triggers_reload_and_waits_for_load() {
        let config = FakeConfig {
            eval: vec![
                (
                    "location.reload".to_string(),
                    js_string_reply(serde_json::json!("reloading")),
                ),
                ("readyState".to_string(), "true".to_string()),
            ],
            ..Default::default()
        };
        let (runner, _) = setup(config);
        let report = runner.run(&manifest(r#"{"op":"reload"}"#)).await.unwrap();
        assert!(report.passed, "{:?}", report.steps);
    }

    #[tokio::test]
    async fn snapshot_then_expect_text_unchanged_passes_on_same_text() {
        let config = FakeConfig {
            eval: vec![(
                "innerText".to_string(),
                js_string_reply(serde_json::json!("  Count:   0 \n\n  Add  ")),
            )],
            ..Default::default()
        };
        let (runner, _) = setup(config);
        let m = manifest(
            r#"{"op":"snapshot_text","key":"before"},
               {"op":"expect_text_unchanged","key":"before"}"#,
        );
        let report = runner.run(&m).await.unwrap();
        assert!(report.passed, "{:?}", report.steps);
    }

    #[tokio::test]
    async fn expect_text_unchanged_reports_the_first_changed_line() {
        let config = FakeConfig {
            sequences: vec![(
                "innerText".to_string(),
                vec![
                    js_string_reply(serde_json::json!("Count: 0\nAdd")),
                    js_string_reply(serde_json::json!("Count: 1\nAdd")),
                ],
            )],
            ..Default::default()
        };
        let (runner, _) = setup(config);
        let m = manifest(
            r#"{"op":"snapshot_text","key":"before"},
               {"op":"expect_text_unchanged","key":"before"}"#,
        );
        let report = runner.run(&m).await.unwrap();
        assert!(!report.passed);
        let detail = &report.steps[1].detail;
        assert!(
            detail.contains("Count: 0") && detail.contains("Count: 1"),
            "{detail}"
        );
    }

    #[tokio::test]
    async fn expect_text_unchanged_needs_a_snapshot_first() {
        let (runner, _) = setup(FakeConfig::default());
        let m = manifest(r#"{"op":"expect_text_unchanged","key":"before"}"#);
        let report = runner.run(&m).await.unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("no snapshot 'before'"));
    }

    /// Two candidates, `#a` then `#b`, neither covered. `hrefs` gives the page
    /// URL read at the start and after each click.
    fn two_buttons(telemetry: Vec<RawTelemetry>, hrefs: &[&str]) -> FakeConfig {
        let mark = |label: &str| {
            js_string_reply(serde_json::json!({ "total": 2, "found": true, "label": label }))
        };
        FakeConfig {
            eval: vec![
                ("candidates[0]".to_string(), mark("BUTTON#a")),
                ("candidates[1]".to_string(), mark("BUTTON#b")),
                (
                    "candidates[2]".to_string(),
                    js_string_reply(serde_json::json!({ "total": 2, "found": false })),
                ),
                (
                    "elementFromPoint".to_string(),
                    js_string_reply(serde_json::json!({
                        "found": true, "visible": true, "covered": false, "tag": "BUTTON",
                        "x": 10.0, "y": 10.0, "w": 80.0, "h": 30.0,
                    })),
                ),
            ],
            sequences: vec![(
                "location.href".to_string(),
                hrefs
                    .iter()
                    .map(|h| serde_json::to_string(*h).unwrap())
                    .collect(),
            )],
            telemetry,
        }
    }

    #[tokio::test]
    async fn click_each_clicks_every_candidate_when_telemetry_is_clean() {
        let config = two_buttons(
            vec![RawTelemetry::default(), RawTelemetry::default()],
            &["http://h/", "http://h/", "http://h/"],
        );
        let (runner, record) = setup(config);
        let report = runner
            .run(&manifest(r#"{"op":"click_each"}"#))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);
        let clicks = record
            .lock()
            .unwrap()
            .actions
            .iter()
            .filter(|a| a.contains("Click"))
            .count();
        assert_eq!(clicks, 2);
        assert!(report.steps[0].detail.contains("clicked 2 of 2"));
    }

    #[tokio::test]
    async fn click_each_fails_naming_the_element_that_logged_an_error() {
        let boom = RawTelemetry {
            console_logs: vec![ConsoleLog {
                level: "error".to_string(),
                message: "boom".to_string(),
            }],
            ..Default::default()
        };
        let config = two_buttons(
            vec![RawTelemetry::default(), boom],
            &["http://h/", "http://h/", "http://h/"],
        );
        let (runner, _) = setup(config);
        let report = runner
            .run(&manifest(r#"{"op":"click_each"}"#))
            .await
            .unwrap();
        assert!(!report.passed);
        let detail = &report.steps[0].detail;
        assert!(
            detail.contains("BUTTON#b") && detail.contains("boom"),
            "{detail}"
        );
        assert!(!detail.contains("BUTTON#a"), "{detail}");
    }

    #[tokio::test]
    async fn click_each_fails_on_a_failed_request_caused_by_a_click() {
        let failing = RawTelemetry {
            network_requests: vec![request("POST", "http://h/api/save", 500)],
            ..Default::default()
        };
        let config = two_buttons(vec![failing], &["http://h/", "http://h/", "http://h/"]);
        let (runner, _) = setup(config);
        let report = runner
            .run(&manifest(r#"{"op":"click_each"}"#))
            .await
            .unwrap();
        assert!(!report.passed);
        let detail = &report.steps[0].detail;
        assert!(
            detail.contains("BUTTON#a") && detail.contains("/api/save"),
            "{detail}"
        );
    }

    #[tokio::test]
    async fn click_each_navigates_back_when_a_click_moves_the_page() {
        let config = two_buttons(vec![], &["http://h/", "http://h/other", "http://h/"]);
        let (runner, record) = setup(config);
        let report = runner
            .run(&manifest(r#"{"op":"click_each"}"#))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);
        let actions = record.lock().unwrap().actions.clone();
        assert!(
            actions
                .iter()
                .any(|a| a.contains("Navigate") && a.contains("http://h/\"")),
            "{actions:?}"
        );
        assert_eq!(actions.iter().filter(|a| a.contains("Click")).count(), 2);
    }

    #[test]
    fn normalise_text_collapses_whitespace_and_drops_blank_lines() {
        assert_eq!(normalise_text("  Count:   0 \n\n\t Add  "), "Count: 0\nAdd");
    }

    #[test]
    fn text_change_detail_truncates_long_lines() {
        let long = "x".repeat(200);
        let detail = text_change_detail("before", "Count: 0\nHello", &format!("Count: 1\n{long}"));
        assert!(detail.contains("line 1"), "{detail}");
        assert!(
            detail.contains("Count: 0") && detail.contains("Count: 1"),
            "{detail}"
        );
        assert!(!detail.contains(&long), "{detail}");
        assert_eq!(truncate_chars("abcdef", 4), "a...");
    }

    /// Raw `eval_js` reply for a JS string result (JSON-encoded, as the adapter returns it).
    fn js_plain_reply(text: &str) -> String {
        serde_json::to_string(text).unwrap()
    }

    /// Raw `eval_js` reply for a probe of an element.
    fn probe_reply(found: bool, visible: bool, covered: bool) -> String {
        js_string_reply(serde_json::json!({
            "found": found, "visible": visible, "covered": covered, "tag": "DIV",
            "coverer": if covered { serde_json::json!("DIV.overlay") } else { serde_json::Value::Null },
            "x": 10.0, "y": 10.0, "w": 80.0, "h": 30.0,
        }))
    }

    /// Config whose probes all report the given state and whose `needle` JS
    /// gets `reply`.
    fn with_probe_and(needle: &str, reply: String, probe: String) -> FakeConfig {
        FakeConfig {
            eval: vec![
                (needle.to_string(), reply),
                ("elementFromPoint".to_string(), probe),
            ],
            ..Default::default()
        }
    }

    fn evals_containing(record: &Arc<Mutex<Record>>, needle: &str) -> usize {
        record
            .lock()
            .unwrap()
            .evals
            .iter()
            .filter(|js| js.contains(needle))
            .count()
    }

    #[tokio::test]
    async fn hover_dispatches_enter_and_over_events_on_a_visible_element() {
        let config = with_probe_and(
            "mouseenter",
            js_plain_reply("ok"),
            probe_reply(true, true, false),
        );
        let (runner, record) = setup(config);
        let report = runner
            .run(&manifest(r##"{"op":"hover","selector":"#menu"}"##))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);
        assert!(report.steps[0].detail.contains("JS hover events only"));
        let rec = record.lock().unwrap();
        let dispatch = rec
            .evals
            .iter()
            .find(|js| js.contains("mouseenter"))
            .expect("hover script ran");
        for event in ["pointerover", "pointerenter", "mouseover"] {
            assert!(dispatch.contains(event), "{event} missing from {dispatch}");
        }
        // No real pointer input is sent for hover.
        assert!(rec.actions.is_empty());
    }

    #[tokio::test]
    async fn hover_fails_when_the_element_is_covered() {
        let config = with_probe_and(
            "mouseenter",
            js_plain_reply("ok"),
            probe_reply(true, true, true),
        );
        let (runner, record) = setup(config);
        let report = runner
            .run(&manifest(r##"{"op":"hover","selector":"#menu"}"##))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("covered by DIV.overlay"));
        assert_eq!(evals_containing(&record, "mouseenter"), 0);
    }

    #[tokio::test]
    async fn hover_fails_when_the_element_is_missing() {
        let config = with_probe_and(
            "mouseenter",
            js_plain_reply("ok"),
            probe_reply(false, false, false),
        );
        let (runner, _) = setup(config);
        let report = runner
            .run(&manifest(r##"{"op":"hover","selector":"#menu"}"##))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("not found"));
    }

    #[tokio::test]
    async fn drag_dispatches_the_html5_sequence_between_two_elements() {
        let config = with_probe_and(
            "DataTransfer",
            js_plain_reply("ok"),
            probe_reply(true, true, false),
        );
        let (runner, record) = setup(config);
        let report = runner
            .run(&manifest(
                r##"{"op":"drag","selector":"#card","target":"#col"}"##,
            ))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);
        assert!(report.steps[0].detail.contains("centre"));
        let rec = record.lock().unwrap();
        let script = rec
            .evals
            .iter()
            .find(|js| js.contains("DataTransfer"))
            .expect("drag script ran");
        let order: Vec<usize> = ["dragstart", "dragenter", "dragover", "'drop'", "dragend"]
            .iter()
            .map(|e| script.find(e).unwrap_or_else(|| panic!("{e} missing")))
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
        assert!(script.contains("* 0.5;"), "centre drop expected");
        assert!(rec.actions.is_empty());
    }

    #[tokio::test]
    async fn drag_before_and_after_aim_at_the_target_quarters() {
        for (position, fraction) in [("before", "* 0.25;"), ("after", "* 0.75;")] {
            let config = with_probe_and(
                "DataTransfer",
                js_plain_reply("ok"),
                probe_reply(true, true, false),
            );
            let (runner, record) = setup(config);
            let m = manifest(&format!(
                r##"{{"op":"drag","selector":"#a","target":"#b","position":"{position}"}}"##
            ));
            let report = runner.run(&m).await.unwrap();
            assert!(report.passed, "{position}: {:?}", report.steps);
            let rec = record.lock().unwrap();
            let script = rec
                .evals
                .iter()
                .find(|js| js.contains("DataTransfer"))
                .unwrap();
            assert!(script.contains(fraction), "{position}: {script}");
        }
    }

    #[tokio::test]
    async fn drag_fails_when_source_or_target_is_missing_or_zero_size() {
        let (runner, record) = setup(with_probe_and(
            "DataTransfer",
            js_plain_reply("ok"),
            probe_reply(false, false, false),
        ));
        let report = runner
            .run(&manifest(
                r##"{"op":"drag","selector":"#a","target":"#b"}"##,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("drag source #a not found"));
        assert_eq!(evals_containing(&record, "DataTransfer"), 0);

        let (runner, _) = setup(with_probe_and(
            "DataTransfer",
            js_plain_reply("ok"),
            // Found, but zero-size: visible is false.
            probe_reply(true, false, false),
        ));
        let report = runner
            .run(&manifest(
                r##"{"op":"drag","selector":"#a","target":"#b"}"##,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(
            report.steps[0]
                .detail
                .contains("drag source #a is not visible")
        );
    }

    fn value_config(reply: serde_json::Value) -> FakeConfig {
        FakeConfig {
            eval: vec![("isChoice".to_string(), js_string_reply(reply))],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn expect_value_compares_the_element_value() {
        let pass = value_config(serde_json::json!({ "found": true, "value": "alice" }));
        let (runner, _) = setup(pass);
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_value","selector":"#name","value":"alice"}"##,
            ))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);

        let fail = value_config(serde_json::json!({ "found": true, "value": "bob" }));
        let (runner, _) = setup(fail);
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_value","selector":"#name","value":"alice"}"##,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        let detail = &report.steps[0].detail;
        assert!(
            detail.contains("\"bob\"") && detail.contains("\"alice\""),
            "{detail}"
        );
    }

    #[tokio::test]
    async fn expect_value_reads_checked_state_and_group_values() {
        // Single checkbox: "true" / "false".
        let checked = value_config(serde_json::json!({ "found": true, "value": "true" }));
        let (runner, _) = setup(checked);
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_value","selector":"#agree","value":"true"}"##,
            ))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);

        // Radio group with nothing checked: "".
        let none = value_config(serde_json::json!({ "found": true, "value": "" }));
        let (runner, _) = setup(none);
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_value","selector":"input[name=size]","value":""}"##,
            ))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);
    }

    #[tokio::test]
    async fn expect_value_fails_for_missing_or_valueless_elements() {
        let missing = value_config(serde_json::json!({ "found": false }));
        let (runner, _) = setup(missing);
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_value","selector":"#x","value":""}"##,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("not found"));

        let valueless = value_config(serde_json::json!({ "found": true, "value": null }));
        let (runner, _) = setup(valueless);
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_value","selector":"#x","value":""}"##,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("no value property"));
    }

    #[tokio::test]
    async fn expect_count_matches_query_selector_all_length() {
        let config = |n: &str| FakeConfig {
            eval: vec![("querySelectorAll(".to_string(), n.to_string())],
            ..Default::default()
        };
        let (runner, _) = setup(config("3"));
        let report = runner
            .run(&manifest(
                r#"{"op":"expect_count","selector":"li","count":3}"#,
            ))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);

        let (runner, _) = setup(config("3"));
        let report = runner
            .run(&manifest(
                r#"{"op":"expect_count","selector":"li","count":2}"#,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(
            report.steps[0]
                .detail
                .contains("3 element(s) match li, expected 2")
        );
    }

    #[tokio::test]
    async fn expect_storage_checks_key_and_contained_value() {
        let config = FakeConfig {
            eval: vec![(
                "localStorage.getItem".to_string(),
                js_string_reply(serde_json::json!({ "found": true, "value": "hello world" })),
            )],
            ..Default::default()
        };
        let (runner, _) = setup(config.clone());
        let report = runner
            .run(&manifest(
                r#"{"op":"expect_storage","key":"token"},
                   {"op":"expect_storage","key":"token","contains":"world"}"#,
            ))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);

        let (runner, _) = setup(config);
        let report = runner
            .run(&manifest(
                r#"{"op":"expect_storage","key":"token","contains":"xyz"}"#,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("hello world"));

        let absent = FakeConfig {
            eval: vec![(
                "localStorage.getItem".to_string(),
                js_string_reply(serde_json::json!({ "found": false, "value": null })),
            )],
            ..Default::default()
        };
        let (runner, _) = setup(absent);
        let report = runner
            .run(&manifest(r#"{"op":"expect_storage","key":"token"}"#))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("no key \"token\""));
    }

    #[tokio::test]
    async fn set_storage_writes_the_key_and_reports_failures() {
        let ok = FakeConfig {
            eval: vec![("localStorage.setItem".to_string(), js_plain_reply("ok"))],
            ..Default::default()
        };
        let (runner, record) = setup(ok);
        let report = runner
            .run(&manifest(
                r#"{"op":"set_storage","key":"theme","value":"dark"}"#,
            ))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);
        let rec = record.lock().unwrap();
        let js = rec.evals.iter().find(|j| j.contains("setItem")).unwrap();
        assert!(js.contains("\"theme\"") && js.contains("\"dark\""), "{js}");
        drop(rec);

        let broken = FakeConfig {
            eval: vec![(
                "localStorage.setItem".to_string(),
                js_plain_reply("error: QuotaExceededError"),
            )],
            ..Default::default()
        };
        let (runner, _) = setup(broken);
        let report = runner
            .run(&manifest(
                r#"{"op":"set_storage","key":"theme","value":"dark"}"#,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("QuotaExceededError"));
    }

    fn text_steps_config(alice_then_bob: Vec<&str>) -> FakeConfig {
        FakeConfig {
            sequences: vec![(
                "innerText".to_string(),
                alice_then_bob.iter().map(|t| js_plain_reply(t)).collect(),
            )],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn expect_same_text_compares_against_another_actors_snapshot() {
        // Each actor's adapter gets its own copy of the sequence, so both see
        // the same text on their first read.
        let config = text_steps_config(vec!["Hi\nFeed item", "unused"]);
        let (runner, _) = setup(config);
        let m = manifest(
            r#"{"actor":"alice","op":"snapshot_text","key":"feed"},
               {"actor":"bob","op":"expect_same_text","key":"feed","other_actor":"alice"}"#,
        );
        let report = runner.run(&m).await.unwrap();
        assert!(report.passed, "{:?}", report.steps);
        assert!(report.steps[1].detail.contains("matches actor 'alice'"));
    }

    #[tokio::test]
    async fn expect_same_text_names_the_first_differing_line() {
        // Bob's first read is the second entry, after `expect_no_text` has
        // consumed the first.
        let config = text_steps_config(vec!["Hi\nFeed item", "Hi\nEmpty"]);
        let (runner, _) = setup(config);
        let m = manifest(
            r#"{"actor":"alice","op":"snapshot_text","key":"feed"},
               {"actor":"bob","op":"expect_no_text","text":"zzz"},
               {"actor":"bob","op":"expect_same_text","key":"feed","other_actor":"alice"}"#,
        );
        let report = runner.run(&m).await.unwrap();
        assert!(!report.passed);
        let detail = &report.steps[2].detail;
        assert!(detail.contains("line 2"), "{detail}");
        assert!(
            detail.contains("Feed item") && detail.contains("Empty"),
            "{detail}"
        );
    }

    #[tokio::test]
    async fn expect_same_text_fails_without_the_other_actor_or_snapshot() {
        let (runner, _) = setup(FakeConfig::default());
        let report = runner
            .run(&manifest(
                r#"{"op":"expect_same_text","key":"feed","other_actor":"nobody"}"#,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("no actor named 'nobody'"));

        let (runner, _) = setup(FakeConfig::default());
        let report = runner
            .run(&manifest(
                r#"{"actor":"alice","op":"expect_no_text","text":"x"},
                   {"op":"expect_same_text","key":"feed","other_actor":"alice"}"#,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[1].detail.contains("took no snapshot 'feed'"));
    }

    #[tokio::test]
    async fn expect_attribute_checks_presence_and_value() {
        let config = FakeConfig {
            eval: vec![(
                "hasAttribute".to_string(),
                js_string_reply(serde_json::json!({ "found": true, "has": true, "value": "" })),
            )],
            ..Default::default()
        };
        let (runner, _) = setup(config.clone());
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_attribute","selector":"#email","name":"required"}"##,
            ))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);

        let (runner, _) = setup(config);
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_attribute","selector":"#email","name":"required","value":"yes"}"##,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("expected \"yes\""));

        let absent = FakeConfig {
            eval: vec![(
                "hasAttribute".to_string(),
                js_string_reply(serde_json::json!({ "found": true, "has": false, "value": null })),
            )],
            ..Default::default()
        };
        let (runner, _) = setup(absent);
        let report = runner
            .run(&manifest(
                r##"{"op":"expect_attribute","selector":"#email","name":"required"}"##,
            ))
            .await
            .unwrap();
        assert!(!report.passed);
        assert!(report.steps[0].detail.contains("no required attribute"));
    }

    fn page_snapshot_config() -> FakeConfig {
        FakeConfig {
            eval: vec![
                (
                    "location.origin".to_string(),
                    js_string_reply(serde_json::json!({
                        "url": "http://h/",
                        "lines": ["Hello", "World"],
                        "visible": ["div#app", "button.primary"],
                    })),
                ),
                (
                    "elementFromPoint".to_string(),
                    probe_reply(true, true, false),
                ),
            ],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn page_snapshots_follow_actions_and_not_assertions() {
        let (runner, _) = setup(page_snapshot_config());
        let m = manifest(
            r##"{"op":"goto","path":"/"},
               {"op":"expect_no_text","text":"zzz"},
               {"op":"click","selector":"#go"},
               {"actor":"bob","op":"expect_no_text","text":"zzz"}"##,
        );
        let report = runner.run(&m).await.unwrap();
        assert!(report.passed, "{:?}", report.steps);
        let snaps = &report.page_snapshots;
        let shape: Vec<(&str, usize, &str)> = snaps
            .iter()
            .map(|s| (s.actor.as_str(), s.step, s.op.as_str()))
            .collect();
        assert_eq!(shape, vec![("default", 0, "goto"), ("default", 2, "click")]);
        assert_eq!(snaps[1].url, "http://h/");
        assert_eq!(
            snaps[1].lines,
            vec!["Hello".to_string(), "World".to_string()]
        );
        assert_eq!(
            snaps[1].visible,
            vec!["div#app".to_string(), "button.primary".to_string()]
        );
    }

    #[tokio::test]
    async fn page_snapshot_failure_does_not_fail_the_step() {
        // No snapshot reply configured: the JS returns `null`, which is unreadable.
        let (runner, _) = setup(FakeConfig::default());
        let report = runner
            .run(&manifest(r#"{"op":"goto","path":"/"}"#))
            .await
            .unwrap();
        assert!(report.passed, "{:?}", report.steps);
        assert!(report.page_snapshots.is_empty());
    }
}
