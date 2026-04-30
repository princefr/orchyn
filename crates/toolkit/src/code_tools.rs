use crate::context::ToolContext;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use ignore::WalkBuilder;
use orchyn_core::{OrchynError, Result};
use regex::Regex;
use serde_json::json;
use uuid::Uuid;

const READ_LIMIT_BYTES: usize = 32_000;
const SEARCH_HIT_LIMIT: usize = 50;

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| OrchynError::Validation(format!("missing string field `{key}`")))
}

fn store_in_memory(ctx: &ToolContext, key: String, value: serde_json::Value) -> BoxFuture<'_, ()> {
    async move {
        let mut mem = ctx.memory.lock().await;
        mem.store(key, value);
    }
    .boxed()
}

// === code_read ===

pub struct CodeReadTool;

impl Tool for CodeReadTool {
    fn name(&self) -> &str { "code_read" }
    fn description(&self) -> &str {
        "Read a file from the project root. Path is resolved relative to the project root."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let rel = require_str(&args, "path")?;
            let path = ctx.project_root.join(rel);
            // Refuse to escape project_root via `..`.
            let canonical = tokio::fs::canonicalize(&path)
                .await
                .map_err(|e| OrchynError::Internal(format!("read {rel}: {e}")))?;
            let canonical_root = tokio::fs::canonicalize(&*ctx.project_root)
                .await
                .map_err(|e| OrchynError::Internal(format!("canonicalize root: {e}")))?;
            if !canonical.starts_with(&canonical_root) {
                return Err(OrchynError::Validation(format!(
                    "path escapes project root: {rel}"
                )));
            }
            let bytes = tokio::fs::read(&canonical)
                .await
                .map_err(|e| OrchynError::Internal(format!("read {rel}: {e}")))?;
            let truncated = bytes.len() > READ_LIMIT_BYTES;
            let text = String::from_utf8_lossy(
                &bytes[..bytes.len().min(READ_LIMIT_BYTES)],
            )
            .into_owned();
            Ok(ToolResult::ok(json!({
                "path": rel,
                "bytes": bytes.len(),
                "truncated": truncated,
                "content": text,
            })))
        }
        .boxed()
    }
}

// === code_search ===

pub struct CodeSearchTool;

impl Tool for CodeSearchTool {
    fn name(&self) -> &str { "code_search" }
    fn description(&self) -> &str {
        "Search for a regex pattern across the project (respects .gitignore)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Optional subdirectory under project root."}
            },
            "required": ["pattern"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let pattern = require_str(&args, "pattern")?;
            let regex = Regex::new(pattern)
                .map_err(|e| OrchynError::Validation(format!("invalid regex: {e}")))?;
            let sub = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let root = ctx.project_root.join(sub);

            let regex_clone = regex.clone();
            let root_clone = root.clone();
            let project_root = (*ctx.project_root).clone();
            let hits = tokio::task::spawn_blocking(move || {
                let mut hits: Vec<serde_json::Value> = Vec::new();
                for entry in WalkBuilder::new(&root_clone).build() {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        continue;
                    }
                    let path = entry.path().to_path_buf();
                    let Ok(content) = std::fs::read_to_string(&path) else { continue };
                    for (i, line) in content.lines().enumerate() {
                        if regex_clone.is_match(line) {
                            let display = path
                                .strip_prefix(&project_root)
                                .unwrap_or(&path)
                                .to_string_lossy()
                                .to_string();
                            hits.push(json!({
                                "file": display,
                                "line": i + 1,
                                "text": line.trim().chars().take(200).collect::<String>(),
                            }));
                            if hits.len() >= SEARCH_HIT_LIMIT {
                                return hits;
                            }
                        }
                    }
                }
                hits
            })
            .await
            .map_err(|e| OrchynError::Internal(format!("search join: {e}")))?;
            Ok(ToolResult::ok(json!({"pattern": pattern, "hits": hits})))
        }
        .boxed()
    }
}

// === gap_report ===

pub struct GapReportTool;

impl Tool for GapReportTool {
    fn name(&self) -> &str { "gap_report" }
    fn description(&self) -> &str {
        "Record a reality gap (a discrepancy between expected and observed behaviour)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "description": {"type": "string"},
                "severity": {"type": "string", "enum": ["low", "medium", "high", "critical"]},
                "reproduction": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["description", "severity"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let id = Uuid::new_v4();
            let entry = json!({
                "gap_id": id,
                "description": args.get("description").cloned().unwrap_or_default(),
                "severity": args.get("severity").cloned().unwrap_or_default(),
                "reproduction": args.get("reproduction").cloned().unwrap_or(json!([])),
            });
            store_in_memory(ctx, format!("gap_{id}"), entry.clone()).await;
            Ok(ToolResult::ok(json!({"gap_id": id, "status": "recorded"})))
        }
        .boxed()
    }
}

// === test_propose ===

pub struct TestProposeTool;

impl Tool for TestProposeTool {
    fn name(&self) -> &str { "test_propose" }
    fn description(&self) -> &str {
        "Propose a manifest entry for Constraint Mode (a multi-step test scenario)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "steps": {"type": "array", "items": {"type": "object"}}
            },
            "required": ["name", "steps"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let id = Uuid::new_v4();
            let entry = json!({
                "test_id": id,
                "name": args.get("name").cloned().unwrap_or_default(),
                "steps": args.get("steps").cloned().unwrap_or(json!([])),
            });
            store_in_memory(ctx, format!("test_{id}"), entry.clone()).await;
            Ok(ToolResult::ok(json!({"test_id": id, "status": "proposed"})))
        }
        .boxed()
    }
}

// === fix_propose ===

pub struct FixProposeTool;

impl Tool for FixProposeTool {
    fn name(&self) -> &str { "fix_propose" }
    fn description(&self) -> &str {
        "Propose a code fix as a unified diff or descriptive patch."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file": {"type": "string"},
                "description": {"type": "string"},
                "patch": {"type": "string"}
            },
            "required": ["file", "description"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let id = Uuid::new_v4();
            let entry = json!({
                "fix_id": id,
                "file": args.get("file").cloned().unwrap_or_default(),
                "description": args.get("description").cloned().unwrap_or_default(),
                "patch": args.get("patch").cloned().unwrap_or_default(),
            });
            store_in_memory(ctx, format!("fix_{id}"), entry.clone()).await;
            Ok(ToolResult::ok(json!({"fix_id": id, "status": "proposed"})))
        }
        .boxed()
    }
}
