use crate::context::ToolContext;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::{OrchynError, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::file_tools::require_str;

// === actor_spawn ===

pub struct ActorSpawnTool;

impl Tool for ActorSpawnTool {
    fn name(&self) -> &str { "actor_spawn" }
    fn description(&self) -> &str {
        "Spawn a new browser actor with the given name. Requires an AdapterFactory in the context."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
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
    fn schema(&self) -> Value {
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
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let key = require_str(&args, "key")?.to_string();
            let value = args.get("value").cloned().unwrap_or(Value::Null);
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
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"key": {"type": "string"}},
            "required": ["key"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let key = require_str(&args, "key")?.to_string();
            let mem = ctx.memory.lock().await;
            let value = mem.recall(&key).cloned().unwrap_or(Value::Null);
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
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"summary": {"type": "string"}},
            "required": ["summary"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
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

// === enter_plan_mode ===

pub struct EnterPlanModeTool;

impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str { "enter_plan_mode" }
    fn description(&self) -> &str { "Switches the application's permission mode to 'plan'." }
    fn schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn execute<'a>(
        &'a self,
        _args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let mut mode = ctx.permission_mode.lock().await;
            *mode = "plan".to_string();
            Ok(ToolResult::ok(json!("Permission mode set to plan")))
        }
        .boxed()
    }
}

// === exit_plan_mode ===

pub struct ExitPlanModeTool;

impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str { "exit_plan_mode" }
    fn description(&self) -> &str { "Reverts the permission mode to 'default'." }
    fn schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn execute<'a>(
        &'a self,
        _args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let mut mode = ctx.permission_mode.lock().await;
            *mode = "default".to_string();
            Ok(ToolResult::ok(json!("Permission mode set to default")))
        }
        .boxed()
    }
}

// === ask_user_question ===

pub struct AskUserQuestionTool;

impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str { "ask_user_question" }
    fn description(&self) -> &str { "Interactively asks the user a question and returns their response." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"question": {"type": "string"}},
            "required": ["question"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let question = require_str(&args, "question")?.to_string();
            if let Some(ref cb) = ctx.ask_user {
                let response = cb(question).await;
                let trimmed = response.trim();
                Ok(ToolResult::ok(json!(if trimmed.is_empty() { "(no response)" } else { trimmed })))
            } else {
                Ok(ToolResult::err("User interaction not available in this context".to_string()))
            }
        }
        .boxed()
    }
}

// === sleep ===

pub struct SleepTool;

impl Tool for SleepTool {
    fn name(&self) -> &str { "sleep" }
    fn description(&self) -> &str { "Pauses execution for a specified duration." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"seconds": {"type": "number", "default": 1.0, "minimum": 0.1, "maximum": 30.0}}
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let seconds = args.get("seconds").and_then(|v| v.as_f64()).unwrap_or(1.0);
            tokio::time::sleep(std::time::Duration::from_secs_f64(seconds)).await;
            Ok(ToolResult::ok(json!(format!("Slept for {} seconds", seconds))))
        }
        .boxed()
    }
}

// === tool_search ===

pub struct ToolSearchTool;

impl Tool for ToolSearchTool {
    fn name(&self) -> &str { "tool_search" }
    fn description(&self) -> &str { "Search the available tool list by name or description." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"query": {"type": "string", "description": "Substring to search in tool names and descriptions"}},
            "required": ["query"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let query = require_str(&args, "query")?.to_lowercase();
            let mut matches = Vec::new();
            for tool in ctx.tool_index.iter() {
                if tool.name.to_lowercase().contains(&query) || tool.description.to_lowercase().contains(&query) {
                    matches.push(format!("{}: {}", tool.name, tool.description));
                }
            }
            if matches.is_empty() {
                Ok(ToolResult::ok(json!("(no matches)")))
            } else {
                Ok(ToolResult::ok(json!(matches.join("\n"))))
            }
        }
        .boxed()
    }
}

// === skill ===

pub struct SkillTool;

impl Tool for SkillTool {
    fn name(&self) -> &str { "skill" }
    fn description(&self) -> &str { "Read the content of a specific skill." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"name": {"type": "string", "description": "Name of the skill to read"}},
            "required": ["name"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let name = require_str(&args, "name")?;
            // Skills are in project_root/skills/<name>/SKILL.md
            let skill_path = ctx.project_root.join("skills").join(name).join("SKILL.md");
            
            if !skill_path.exists() {
                // Try fuzzy match or case-insensitive if needed, but for now exact
                return Ok(ToolResult::err(format!("Skill not found: {}", name)));
            }

            let content = tokio::fs::read_to_string(&skill_path)
                .await
                .map_err(|e| OrchynError::Internal(format!("read skill {name}: {e}")))?;

            Ok(ToolResult::ok(json!(content)))
        }
        .boxed()
    }
}

// === brief ===

pub struct BriefTool;

impl Tool for BriefTool {
    fn name(&self) -> &str { "brief" }
    fn description(&self) -> &str { "Shorten a piece of text for compact display." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {"type": "string", "description": "Text to shorten"},
                "max_chars": {"type": "integer", "default": 200, "minimum": 20, "maximum": 2000}
            },
            "required": ["text"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let text = require_str(&args, "text")?.trim();
            let max_chars = args.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
            
            if text.len() <= max_chars {
                Ok(ToolResult::ok(json!(text)))
            } else {
                let shortened = format!("{}...", text[..max_chars].trim_end());
                Ok(ToolResult::ok(json!(shortened)))
            }
        }
        .boxed()
    }
}

// === config ===

const CONFIG_FILE: &str = ".orchyn.json";

pub struct ConfigTool;

impl Tool for ConfigTool {
    fn name(&self) -> &str { "config" }
    fn description(&self) -> &str {
        "Read or update application settings in .orchyn.json."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["show", "set"], "default": "show"},
                "key": {"type": "string"},
                "value": {"type": "string"}
            }
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("show");
            let path = ctx.project_root.join(CONFIG_FILE);

            let mut config = if path.exists() {
                let text = tokio::fs::read_to_string(&path).await
                    .map_err(|e| OrchynError::Internal(format!("read config: {e}")))?;
                serde_json::from_str::<Value>(&text).unwrap_or(json!({}))
            } else {
                json!({})
            };

            if action == "show" {
                return Ok(ToolResult::ok(config));
            }

            if action == "set" {
                let key = require_str(&args, "key")?;
                let value = require_str(&args, "value")?;
                
                if let Some(obj) = config.as_object_mut() {
                    obj.insert(key.to_string(), json!(value));
                } else {
                    config = json!({key: value});
                }

                let text = serde_json::to_string_pretty(&config)
                    .map_err(|e| OrchynError::Internal(format!("encode config: {e}")))?;
                tokio::fs::write(&path, text.as_bytes()).await
                    .map_err(|e| OrchynError::Internal(format!("write config: {e}")))?;
                
                return Ok(ToolResult::ok(json!(format!("Updated {key}"))));
            }

            Ok(ToolResult::err("Usage: action=show or action=set with key/value".to_string()))
        }
        .boxed()
    }
}
