use orchyn_spec_engine::Spec;
use orchyn_core::RealityContext;
use serde_json::Value;
use tracing::info;

pub trait GapTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn call(&self, args: Value) -> String;
}

pub struct AnalyzeSpecTool;

impl GapTool for AnalyzeSpecTool {
    fn name(&self) -> &str { "analyze_spec" }
    fn description(&self) -> &str {
        "Analyze the spec and extract claims that should be checked against reality."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "focus": { "type": "string", "description": "Aspect to focus on (features, constraints, flows)" }
            }
        })
    }
    fn call(&self, args: Value) -> String {
        let focus = args.get("focus").and_then(|v| v.as_str()).unwrap_or("all");
        info!("Analyzing spec with focus: {}", focus);
        format!("Spec analyzed. Focus: {}", focus)
    }
}

pub struct CompareRealityTool;

impl GapTool for CompareRealityTool {
    fn name(&self) -> &str { "compare_reality" }
    fn description(&self) -> &str {
        "Compare a specific claim against observed reality (diagnostics, network, DOM)."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "claim_id": { "type": "string" },
                "reality_summary": { "type": "string" }
            },
            "required": ["claim_id", "reality_summary"]
        })
    }
    fn call(&self, args: Value) -> String {
        let claim_id = args.get("claim_id").and_then(|v| v.as_str()).unwrap_or("");
        let summary = args.get("reality_summary").and_then(|v| v.as_str()).unwrap_or("");
        info!("Comparing claim {} with reality: {}", claim_id, summary);
        format!("Comparison done for claim {}. Reality: {}", claim_id, summary)
    }
}
