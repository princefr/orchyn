//! Orchyn-specific reporting tools (gap_report, test_result, fix_propose).
//! gap_report stores keys with prefix `gap_`.
//! test_result stores keys with prefix `result_pass_` or `result_fail_`.
//! fix_propose stores keys with prefix `fix_`.
//! The AgentReport counters are derived from these key prefixes.

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

// === test_result ===

pub struct TestResultTool;

impl Tool for TestResultTool {
    fn name(&self) -> &str { "test_result" }
    fn description(&self) -> &str {
        "Record a test execution result. Stores result_pass_<name> or result_fail_<name> in memory."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Short slug for the test, e.g. login_happy_path"},
                "passed": {"type": "boolean"},
                "reason": {"type": "string", "description": "Required when passed=false"}
            },
            "required": ["name", "passed"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let passed = args.get("passed").and_then(|v| v.as_bool()).unwrap_or(false);
            let reason = args.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let key = if passed {
                format!("result_pass_{name}")
            } else {
                format!("result_fail_{name}")
            };
            let value = if passed {
                json!({"ok": true})
            } else {
                json!({"ok": false, "reason": reason})
            };
            store_in_memory(ctx, key.clone(), value).await;
            Ok(ToolResult::ok(json!({"key": key, "recorded": true})))
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
