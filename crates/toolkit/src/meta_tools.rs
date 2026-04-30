use crate::context::ToolContext;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::{OrchynError, Result};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| OrchynError::Validation(format!("missing string field `{key}`")))
}

// === actor_spawn ===

pub struct ActorSpawnTool;

impl Tool for ActorSpawnTool {
    fn name(&self) -> &str { "actor_spawn" }
    fn description(&self) -> &str {
        "Spawn a new browser actor with the given name. Requires an AdapterFactory in the context."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let name = require_str(&args, "name")?.to_string();
            let factory = ctx
                .adapter_factory
                .clone()
                .ok_or_else(|| OrchynError::Validation("no AdapterFactory installed".into()))?;
            let adapter = factory.spawn(&name).await?;
            let mut actors = ctx.actors.lock().await;
            if actors.contains_key(&name) {
                return Err(OrchynError::Validation(format!(
                    "actor `{name}` already exists"
                )));
            }
            actors.insert(name.clone(), Arc::new(Mutex::new(adapter)));
            Ok(ToolResult::ok(json!({"actor": name, "status": "spawned"})))
        }
        .boxed()
    }
}

// === memory_store ===

pub struct MemoryStoreTool;

impl Tool for MemoryStoreTool {
    fn name(&self) -> &str { "memory_store" }
    fn description(&self) -> &str { "Persist a (key, value) pair into working memory." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {"type": "string"},
                "value": {}
            },
            "required": ["key", "value"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let key = require_str(&args, "key")?.to_string();
            let value = args.get("value").cloned().unwrap_or(serde_json::Value::Null);
            let mut mem = ctx.memory.lock().await;
            mem.store(key.clone(), value);
            Ok(ToolResult::ok(json!({"stored": key})))
        }
        .boxed()
    }
}

// === memory_recall ===

pub struct MemoryRecallTool;

impl Tool for MemoryRecallTool {
    fn name(&self) -> &str { "memory_recall" }
    fn description(&self) -> &str { "Read a key from working memory." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"key": {"type": "string"}},
            "required": ["key"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let key = require_str(&args, "key")?.to_string();
            let mem = ctx.memory.lock().await;
            let value = mem.recall(&key).cloned().unwrap_or(serde_json::Value::Null);
            Ok(ToolResult::ok(json!({"key": key, "value": value})))
        }
        .boxed()
    }
}

// === done ===

pub struct DoneTool;

impl Tool for DoneTool {
    fn name(&self) -> &str { "done" }
    fn description(&self) -> &str { "Signal that the agent has completed its goal." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"summary": {"type": "string"}},
            "required": ["summary"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let summary = args
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("(no summary)")
                .to_string();
            Ok(ToolResult::ok(json!({"done": true, "summary": summary})))
        }
        .boxed()
    }
}
