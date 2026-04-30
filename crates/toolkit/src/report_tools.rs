//! Orchyn-specific reporting tools (gap_report, test_propose, fix_propose).
//! These are NOT part of the OpenHarness surface — they back the agent's
//! findings that get flattened into a Constraint Mode manifest by the CLI.

use crate::context::ToolContext;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::Result;
use serde_json::json;
use uuid::Uuid;

fn store_in_memory(ctx: &ToolContext, key: String, value: serde_json::Value) -> BoxFuture<'_, ()> {
    async move {
        let mut mem = ctx.memory.lock().await;
        mem.store(key, value);
    }
    .boxed()
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
