//! In-memory task tracker tools that mirror the OpenHarness `task_*` and
//! `todo_write` surface. All state lives in [`ToolContext::memory`] so it
//! survives across tool calls within a single agent run.

use crate::context::ToolContext;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::{OrchynError, Result};
use serde_json::json;
use uuid::Uuid;

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| OrchynError::Validation(format!("missing string field `{key}`")))
}

// === task_create ===

pub struct TaskCreateTool;

impl Tool for TaskCreateTool {
    fn name(&self) -> &str { "task_create" }
    fn description(&self) -> &str {
        "Create a new task in working memory. Returns the task id (UUID)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"description": {"type": "string"}},
            "required": ["description"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let description = require_str(&args, "description")?.to_string();
            let id = Uuid::new_v4().to_string();
            let entry = json!({
                "id": id,
                "description": description,
                "status": "pending",
            });
            let mut mem = ctx.memory.lock().await;
            mem.store(format!("task_{id}"), entry.clone());
            Ok(ToolResult::ok(json!({"id": id, "status": "pending"})))
        }
        .boxed()
    }
}

// === task_get ===

pub struct TaskGetTool;

impl Tool for TaskGetTool {
    fn name(&self) -> &str { "task_get" }
    fn description(&self) -> &str { "Fetch a task by id from working memory." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let id = require_str(&args, "id")?.to_string();
            let key = format!("task_{id}");
            let mem = ctx.memory.lock().await;
            let entry = mem
                .recall(&key)
                .cloned()
                .ok_or_else(|| OrchynError::Validation(format!("no task with id {id}")))?;
            Ok(ToolResult::ok(entry))
        }
        .boxed()
    }
}

// === task_list ===

pub struct TaskListTool;

impl Tool for TaskListTool {
    fn name(&self) -> &str { "task_list" }
    fn description(&self) -> &str {
        "List every task in working memory, optionally filtered by status."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"status": {"type": "string"}}
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let filter = args.get("status").and_then(|v| v.as_str()).map(str::to_string);
            let mem = ctx.memory.lock().await;
            let mut tasks: Vec<serde_json::Value> = Vec::new();
            for (key, value) in mem.entries() {
                if !key.starts_with("task_") {
                    continue;
                }
                if let Some(ref status) = filter {
                    let matches = value
                        .get("status")
                        .and_then(|v| v.as_str())
                        .map(|s| s == status)
                        .unwrap_or(false);
                    if !matches {
                        continue;
                    }
                }
                tasks.push(value.clone());
            }
            Ok(ToolResult::ok(json!({"count": tasks.len(), "tasks": tasks})))
        }
        .boxed()
    }
}

// === task_update ===

pub struct TaskUpdateTool;

impl Tool for TaskUpdateTool {
    fn name(&self) -> &str { "task_update" }
    fn description(&self) -> &str {
        "Mutate a task's description and/or status (in place)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "description": {"type": "string"},
                "status": {"type": "string"}
            },
            "required": ["id"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let id = require_str(&args, "id")?.to_string();
            let key = format!("task_{id}");
            let mut mem = ctx.memory.lock().await;
            let mut entry = mem
                .recall(&key)
                .cloned()
                .ok_or_else(|| OrchynError::Validation(format!("no task with id {id}")))?;
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                entry["description"] = json!(desc);
            }
            if let Some(status) = args.get("status").and_then(|v| v.as_str()) {
                entry["status"] = json!(status);
            }
            mem.store(key, entry.clone());
            Ok(ToolResult::ok(entry))
        }
        .boxed()
    }
}

// === task_stop ===

pub struct TaskStopTool;

impl Tool for TaskStopTool {
    fn name(&self) -> &str { "task_stop" }
    fn description(&self) -> &str { "Mark a task as stopped." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let id = require_str(&args, "id")?.to_string();
            let key = format!("task_{id}");
            let mut mem = ctx.memory.lock().await;
            let mut entry = mem
                .recall(&key)
                .cloned()
                .ok_or_else(|| OrchynError::Validation(format!("no task with id {id}")))?;
            entry["status"] = json!("stopped");
            mem.store(key, entry.clone());
            Ok(ToolResult::ok(entry))
        }
        .boxed()
    }
}

// === task_output ===

pub struct TaskOutputTool;

impl Tool for TaskOutputTool {
    fn name(&self) -> &str { "task_output" }
    fn description(&self) -> &str {
        "Return the `output` field from a task entry (null if unset)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let id = require_str(&args, "id")?.to_string();
            let key = format!("task_{id}");
            let mem = ctx.memory.lock().await;
            let entry = mem
                .recall(&key)
                .cloned()
                .ok_or_else(|| OrchynError::Validation(format!("no task with id {id}")))?;
            let output = entry.get("output").cloned().unwrap_or(serde_json::Value::Null);
            Ok(ToolResult::ok(json!({"id": id, "output": output})))
        }
        .boxed()
    }
}

// === todo_write ===

pub struct TodoWriteTool;

impl Tool for TodoWriteTool {
    fn name(&self) -> &str { "todo_write" }
    fn description(&self) -> &str {
        "Replace the agent's todo list in working memory with a fresh array of strings."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["todos"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let todos = args
                .get("todos")
                .and_then(|v| v.as_array())
                .ok_or_else(|| OrchynError::Validation("missing array field `todos`".into()))?
                .clone();
            let mut mem = ctx.memory.lock().await;
            mem.store("todo_list".to_string(), serde_json::Value::Array(todos.clone()));
            Ok(ToolResult::ok(json!({"count": todos.len(), "todos": todos})))
        }
        .boxed()
    }
}

// === team_create ===

pub struct TeamCreateTool;

impl Tool for TeamCreateTool {
    fn name(&self) -> &str { "team_create" }
    fn description(&self) -> &str { "Create a new agent team (stub)." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        })
    }
    fn execute<'a>(
        &'a self,
        _args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            Ok(ToolResult::ok(json!("Team creation not yet supported in Rust backend.")))
        }
        .boxed()
    }
}

// === team_delete ===

pub struct TeamDeleteTool;

impl Tool for TeamDeleteTool {
    fn name(&self) -> &str { "team_delete" }
    fn description(&self) -> &str { "Delete an agent team (stub)." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        })
    }
    fn execute<'a>(
        &'a self,
        _args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            Ok(ToolResult::ok(json!("Team deletion not yet supported in Rust backend.")))
        }
        .boxed()
    }
}

// === agent ===

pub struct AgentTool;

impl Tool for AgentTool {
    fn name(&self) -> &str { "agent" }
    fn description(&self) -> &str { "Spawn a local background agent task (stub)." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "description": {"type": "string", "description": "Short description of the delegated work"},
                "prompt": {"type": "string", "description": "Full prompt for the local agent"}
            },
            "required": ["description", "prompt"]
        })
    }
    fn execute<'a>(
        &'a self,
        _args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            Ok(ToolResult::ok(json!("Agent spawning not yet supported in Rust backend.")))
        }
        .boxed()
    }
}

// === send_message ===

pub struct SendMessageTool;

impl Tool for SendMessageTool {
    fn name(&self) -> &str { "send_message" }
    fn description(&self) -> &str { "Send a follow-up message to a running local agent task (stub)." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {"type": "string", "description": "Target local agent task id"},
                "message": {"type": "string", "description": "Message to write to the task"}
            },
            "required": ["task_id", "message"]
        })
    }
    fn execute<'a>(
        &'a self,
        _args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            Ok(ToolResult::ok(json!("Message sending not yet supported in Rust backend.")))
        }
        .boxed()
    }
}

// === cron_create ===

pub struct CronCreateTool;

impl Tool for CronCreateTool {
    fn name(&self) -> &str { "cron_create" }
    fn description(&self) -> &str { "Create or replace a local cron job (stub)." }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "schedule": {"type": "string"},
                "command": {"type": "string"}
            },
            "required": ["name", "schedule", "command"]
        })
    }
    fn execute<'a>(
        &'a self,
        _args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            Ok(ToolResult::ok(json!("Cron jobs not yet supported in Rust backend.")))
        }
        .boxed()
    }
}

// === cron_list ===

pub struct CronListTool;

impl Tool for CronListTool {
    fn name(&self) -> &str { "cron_list" }
    fn description(&self) -> &str { "List configured local cron jobs (stub)." }
    fn schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }
    fn execute<'a>(
        &'a self,
        _args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            Ok(ToolResult::ok(json!("No cron jobs configured (backend not implemented).")))
        }
        .boxed()
    }
}
