use crate::context::ToolContext;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::action::Action;
use orchyn_core::{OrchynError, Result};
use orchyn_pixel_observer::annotate_screenshot;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

async fn locate_actor(
    ctx: &ToolContext,
    actor_id: &str,
) -> Result<Arc<Mutex<Box<dyn orchyn_core::PlatformAdapter>>>> {
    let actors = ctx.actors.lock().await;
    actors
        .get(actor_id)
        .cloned()
        .ok_or_else(|| OrchynError::Validation(format!("unknown actor: {actor_id}")))
}

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| OrchynError::Validation(format!("missing string field `{key}`")))
}

fn require_u8(args: &serde_json::Value, key: &str) -> Result<u8> {
    args.get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u8)
        .ok_or_else(|| OrchynError::Validation(format!("missing integer field `{key}`")))
}

// === browser_screenshot ===

pub struct BrowserScreenshotTool;

impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str { "browser_screenshot" }
    fn description(&self) -> &str {
        "Capture a numbered screenshot of an actor's current page and return the element map."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "actor": {"type": "string"}
            },
            "required": ["actor"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            let raw = guard.screenshot().await?;
            let elements = guard.scrape_elements().await?;
            drop(guard);
            let (annotated, map) = annotate_screenshot(&raw, &elements)
                .map_err(|e| OrchynError::Internal(format!("annotate: {e}")))?;
            let path = std::env::temp_dir().join(format!("orchyn_{}.png", Uuid::new_v4()));
            tokio::fs::write(&path, &annotated)
                .await
                .map_err(|e| OrchynError::Internal(format!("save screenshot: {e}")))?;
            Ok(ToolResult::ok(json!({
                "screenshot_path": path,
                "element_count": map.len(),
                "elements": map,
            })))
        }
        .boxed()
    }
}

// === browser_navigate ===

pub struct BrowserNavigateTool;

impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str { "browser_navigate" }
    fn description(&self) -> &str { "Navigate an actor to a URL." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "actor": {"type": "string"},
                "url": {"type": "string"}
            },
            "required": ["actor", "url"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let url = require_str(&args, "url")?.to_string();
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            guard
                .execute_harness(&Action::Navigate { value: url.clone() }, &[])
                .await?;
            Ok(ToolResult::ok(json!({"actor": actor_id, "url": url})))
        }
        .boxed()
    }
}

// === browser_click ===

pub struct BrowserClickTool;

impl Tool for BrowserClickTool {
    fn name(&self) -> &str { "browser_click" }
    fn description(&self) -> &str {
        "Click an element by its numbered overlay id (call browser_screenshot first)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "actor": {"type": "string"},
                "element_id": {"type": "integer", "minimum": 1, "maximum": 99}
            },
            "required": ["actor", "element_id"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let target = require_u8(&args, "element_id")?;
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            // Re-scrape so we have a fresh element_map for click resolution.
            let raw = guard.screenshot().await?;
            let elements = guard.scrape_elements().await?;
            let (_, map) = annotate_screenshot(&raw, &elements)
                .map_err(|e| OrchynError::Internal(format!("annotate: {e}")))?;
            guard
                .execute_harness(&Action::Click { target }, &map)
                .await?;
            Ok(ToolResult::ok(json!({"actor": actor_id, "clicked": target})))
        }
        .boxed()
    }
}

// === browser_type ===

pub struct BrowserTypeTool;

impl Tool for BrowserTypeTool {
    fn name(&self) -> &str { "browser_type" }
    fn description(&self) -> &str { "Type text into an input element by id." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "actor": {"type": "string"},
                "element_id": {"type": "integer", "minimum": 1, "maximum": 99},
                "text": {"type": "string"}
            },
            "required": ["actor", "element_id", "text"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let target = require_u8(&args, "element_id")?;
            let text = require_str(&args, "text")?.to_string();
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            let raw = guard.screenshot().await?;
            let elements = guard.scrape_elements().await?;
            let (_, map) = annotate_screenshot(&raw, &elements)
                .map_err(|e| OrchynError::Internal(format!("annotate: {e}")))?;
            guard
                .execute_harness(
                    &Action::Type { target, value: text.clone() },
                    &map,
                )
                .await?;
            Ok(ToolResult::ok(json!({"actor": actor_id, "typed_into": target})))
        }
        .boxed()
    }
}

// === browser_get_network ===

pub struct BrowserGetNetworkTool;

impl Tool for BrowserGetNetworkTool {
    fn name(&self) -> &str { "browser_get_network" }
    fn description(&self) -> &str { "Return recent HTTP requests with method, URL, status, and bodies." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "actor": {"type": "string"},
                "only_errors": {
                    "type": "boolean",
                    "default": false,
                    "description": "When true, return only 4xx/5xx responses"
                }
            },
            "required": ["actor"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let only_errors = args.get("only_errors").and_then(|v| v.as_bool()).unwrap_or(false);
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            let telemetry = guard.collect_telemetry().await?;
            let requests: Vec<_> = telemetry
                .network_requests
                .into_iter()
                .filter(|r| !only_errors || r.status >= 400)
                .map(|r| json!({"method": r.method, "url": r.url, "status": r.status}))
                .collect();
            Ok(ToolResult::ok(json!({"actor": actor_id, "requests": requests})))
        }
        .boxed()
    }
}

// JS snippets used by the diagnostic tools below.
const ARIA_VIOLATIONS_JS: &str = r#"
(() => {
    const issues = [];
    for (const img of document.querySelectorAll('img:not([alt])')) {
        issues.push({ type: 'missing-alt', src: img.src.split('/').pop() });
    }
    for (const inp of document.querySelectorAll('input:not([type="hidden"]):not([type="submit"]):not([type="button"])')) {
        const hasLabel = (inp.id && document.querySelector('label[for="' + inp.id + '"]'))
            || inp.closest('label')
            || inp.getAttribute('aria-label')
            || inp.getAttribute('aria-labelledby');
        if (!hasLabel) issues.push({ type: 'missing-label', inputType: inp.type || 'text', name: inp.name });
    }
    for (const btn of document.querySelectorAll('button, [role="button"]')) {
        const name = (btn.innerText || btn.getAttribute('aria-label') || btn.getAttribute('title') || '').trim();
        if (!name) issues.push({ type: 'unnamed-button', tag: btn.tagName.toLowerCase() });
    }
    for (const a of document.querySelectorAll('a[href]')) {
        const name = (a.innerText || a.getAttribute('aria-label') || '').trim();
        if (!name) issues.push({ type: 'unnamed-link', href: a.href });
    }
    for (const el of document.querySelectorAll('button, a, input, select, textarea')) {
        if (el.tabIndex < 0) issues.push({ type: 'unreachable-by-keyboard', tag: el.tagName.toLowerCase() });
    }
    for (const el of document.querySelectorAll(':invalid')) {
        issues.push({ type: 'invalid-field', tag: el.tagName.toLowerCase(), name: el.name || el.id });
    }
    return JSON.stringify(issues);
})()
"#;

const STORAGE_SNAPSHOT_JS: &str = r#"
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
    return JSON.stringify({ localStorage: dump(localStorage), sessionStorage: dump(sessionStorage) });
})()
"#;

const PERFORMANCE_JS: &str = r#"
(() => {
    const nav = performance.getEntriesByType('navigation')[0] || {};
    const resources = performance.getEntriesByType('resource');
    const slow = resources.filter(r => r.duration > 500)
        .map(r => ({ name: r.name.split('/').pop(), ms: Math.round(r.duration), type: r.initiatorType }))
        .slice(0, 10);
    const lcp = (() => { const e = performance.getEntriesByType('largest-contentful-paint'); return e.length ? Math.round(e[e.length-1].startTime) : null; })();
    const cls = performance.getEntriesByType('layout-shift').reduce((s, e) => s + e.value, 0);
    return JSON.stringify({
        domContentLoaded: Math.round(nav.domContentLoadedEventEnd - nav.startTime) || null,
        loadComplete: Math.round(nav.loadEventEnd - nav.startTime) || null,
        ttfb: Math.round(nav.responseStart - nav.startTime) || null,
        lcp,
        cls: parseFloat(cls.toFixed(4)),
        slowResources: slow,
        totalResources: resources.length,
    });
})()
"#;

// === browser_get_console ===

pub struct BrowserGetConsoleTool;

impl Tool for BrowserGetConsoleTool {
    fn name(&self) -> &str { "browser_get_console" }
    fn description(&self) -> &str { "Read recent console errors and warnings." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"actor": {"type": "string"}},
            "required": ["actor"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            let telemetry = guard.collect_telemetry().await?;
            let errors: Vec<_> = telemetry
                .console_logs
                .into_iter()
                .filter(|l| matches!(l.level.as_str(), "error" | "warn"))
                .map(|l| json!({"level": l.level, "message": l.message}))
                .collect();
            Ok(ToolResult::ok(json!({"actor": actor_id, "errors": errors})))
        }
        .boxed()
    }
}

// === browser_check_accessibility ===

pub struct BrowserCheckAccessibilityTool;

impl Tool for BrowserCheckAccessibilityTool {
    fn name(&self) -> &str { "browser_check_accessibility" }
    fn description(&self) -> &str {
        "Scan the page for ARIA violations, missing labels, unnamed buttons, keyboard traps, \
         and invalid form fields. Returns a list of issues."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"actor": {"type": "string"}},
            "required": ["actor"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            let raw = guard.eval_js(ARIA_VIOLATIONS_JS).await?;
            let issues: serde_json::Value = serde_json::from_str(&raw)
                .unwrap_or(serde_json::Value::Array(vec![]));
            let count = issues.as_array().map(|a| a.len()).unwrap_or(0);
            Ok(ToolResult::ok(json!({
                "actor": actor_id,
                "issue_count": count,
                "issues": issues,
            })))
        }
        .boxed()
    }
}

// === browser_get_storage ===

pub struct BrowserGetStorageTool;

impl Tool for BrowserGetStorageTool {
    fn name(&self) -> &str { "browser_get_storage" }
    fn description(&self) -> &str {
        "Return the full contents of localStorage and sessionStorage as key-value maps. \
         Use this to verify state persistence after page reloads."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"actor": {"type": "string"}},
            "required": ["actor"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            let raw = guard.eval_js(STORAGE_SNAPSHOT_JS).await?;
            let storage: serde_json::Value = serde_json::from_str(&raw)
                .unwrap_or(serde_json::json!({"localStorage": {}, "sessionStorage": {}}));
            Ok(ToolResult::ok(json!({"actor": actor_id, "storage": storage})))
        }
        .boxed()
    }
}

// === browser_get_performance ===

pub struct BrowserGetPerformanceTool;

impl Tool for BrowserGetPerformanceTool {
    fn name(&self) -> &str { "browser_get_performance" }
    fn description(&self) -> &str {
        "Return Web Performance API metrics: TTFB, DOMContentLoaded, load time, LCP, CLS, \
         and a list of resources that took over 500ms. Use to diagnose slow pages."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"actor": {"type": "string"}},
            "required": ["actor"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            let raw = guard.eval_js(PERFORMANCE_JS).await?;
            let metrics: serde_json::Value = serde_json::from_str(&raw)
                .unwrap_or(serde_json::Value::Null);
            Ok(ToolResult::ok(json!({"actor": actor_id, "metrics": metrics})))
        }
        .boxed()
    }
}

// === browser_set_viewport ===

pub struct BrowserSetViewportTool;

impl Tool for BrowserSetViewportTool {
    fn name(&self) -> &str { "browser_set_viewport" }
    fn description(&self) -> &str {
        "Resize the browser viewport to test responsive layouts. \
         Common presets: desktop 1280×800, tablet 768×1024, mobile 390×844."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "actor": {"type": "string"},
                "width": {"type": "integer", "minimum": 320, "maximum": 3840},
                "height": {"type": "integer", "minimum": 240, "maximum": 2160}
            },
            "required": ["actor", "width", "height"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let width = args.get("width").and_then(|v| v.as_u64())
                .ok_or_else(|| OrchynError::Validation("missing integer field `width`".into()))? as u32;
            let height = args.get("height").and_then(|v| v.as_u64())
                .ok_or_else(|| OrchynError::Validation("missing integer field `height`".into()))? as u32;
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            guard.set_viewport(width, height).await?;
            Ok(ToolResult::ok(json!({"actor": actor_id, "width": width, "height": height})))
        }
        .boxed()
    }
}

// === browser_get_ws_messages ===

pub struct BrowserGetWsMessagesTool;

impl Tool for BrowserGetWsMessagesTool {
    fn name(&self) -> &str { "browser_get_ws_messages" }
    fn description(&self) -> &str {
        "Return buffered WebSocket frames (text only). Prefix '←' = received, '→' = sent. \
         Use to debug real-time features (notifications, live updates, chat)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"actor": {"type": "string"}},
            "required": ["actor"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let actor_id = require_str(&args, "actor")?.to_string();
            let adapter = locate_actor(ctx, &actor_id).await?;
            let mut guard = adapter.lock().await;
            // collect_telemetry drains ws_messages from the adapter
            let telemetry = guard.collect_telemetry().await?;
            let messages = telemetry.ws_messages;
            Ok(ToolResult::ok(json!({"actor": actor_id, "count": messages.len(), "messages": messages})))
        }
        .boxed()
    }
}
