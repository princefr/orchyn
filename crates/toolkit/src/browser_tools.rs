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
