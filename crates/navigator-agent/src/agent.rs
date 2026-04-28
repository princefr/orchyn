use crate::tools::NavigatorTool;
use orchyn_codebase_indexer::AppMap;
use orchyn_core::Result;
use orchyn_model_client::{ModelClient, ToolDefinition, CompletionRequest};
use serde_json::Value;
use tracing::{debug, info, warn};
use futures::StreamExt;
use std::sync::Arc;

/// System prompt and skills embedded at compile time
pub const PROMPT_NAVIGATOR: &str = include_str!("../../../prompts/navigator.md");
pub const SKILL_CODEBASE_NAVIGATION: &str = include_str!("../../../skills/codebase-navigation/SKILL.md");
pub const SKILL_SPEC_INFERENCE: &str = include_str!("../../../skills/spec-inference/SKILL.md");

pub struct NavigatorAgent {
    model: Arc<dyn ModelClient>,
    tools: Vec<Box<dyn NavigatorTool>>,
    app_map: AppMap,
    claims: Vec<Value>,
}

impl NavigatorAgent {
    pub fn new(model: Arc<dyn ModelClient>, app_map: AppMap) -> Self {
        let root = app_map.root.clone();
        Self {
            model,
            tools: vec![
                Box::new(tools::ListFilesTool::new(root.clone())),
                Box::new(tools::ReadFileTool),
                Box::new(tools::GrepTool::new(root)),
            ],
            app_map,
            claims: Vec::new(),
        }
    }

    pub async fn run(&mut self, goal: &str) -> Result<Vec<Value>> {
        info!("Navigator Agent starting with goal: {}", goal);
        
        let mut max_steps = 40;
        let mut history = String::new();
        
        while max_steps > 0 {
            debug!("Steps remaining: {}", max_steps);
            
            let mut tool_definitions: Vec<ToolDefinition> = self
                .tools
                .iter()
                .map(|t| ToolDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters(),
                })
                .collect();
            
            tool_definitions.push(ToolDefinition {
                name: "propose_claim".to_string(),
                description: "Propose a new intent claim for the spec engine.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "statement": { "type": "string" },
                        "kind": { "type": "string" },
                        "confidence": { "type": "number" },
                        "evidence": { "type": "array", "items": { "type": "object" } }
                    },
                    "required": ["statement", "kind", "confidence", "evidence"]
                }),
            });
            
            tool_definitions.push(ToolDefinition {
                name: "finish".to_string(),
                description: "Finish navigation and return findings.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "summary": { "type": "string" }
                    }
                }),
            });
            
            let prompt = format!(
                "{}\n\n=== SKILL: CODEBASE NAVIGATION ===\n{}\n\n=== SKILL: SPEC INFERENCE ===\n{}\n\nGoal: {}\n\nHistory: {}\n\nDecide next tool to call.",
                PROMPT_NAVIGATOR, SKILL_CODEBASE_NAVIGATION, SKILL_SPEC_INFERENCE, goal, history
            );
            
            let req = CompletionRequest {
                prompt,
                max_tokens: Some(1024),
                temperature: Some(0.0),
            };
            
            let mut stream = self.model.complete(req).await?;
            let mut response = String::new();
            while let Some(chunk) = stream.next().await {
                response.push_str(&chunk?);
            }
            
            // Parse JSON response
            if let Some(start) = response.find('{') {
                if let Some(end) = response.rfind('}') {
                    let json_str = &response[start..=end];
                    if let Ok(val) = serde_json::from_str::<Value>(json_str) {
                        let tool_name = val.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let args = val.get("arguments").cloned().unwrap_or_default();
                        
                        if tool_name == "propose_claim" {
                            self.claims.push(args);
                            history.push_str(&format!("\nProposed claim: {}", serde_json::to_string(&args).unwrap_or_default()));
                        } else if tool_name == "finish" {
                            info!("Navigator finished: {:?}", args.get("summary"));
                            break;
                        } else {
                            // Call tool
                            let tool = match self.tools.iter().find(|t| t.name() == tool_name) {
                                Some(t) => t,
                                None => {
                                    warn!("Unknown tool: {}", tool_name);
                                    continue;
                                }
                            };
                            
                            let result = tool.call(args).await;
                            history.push_str(&format!("\nTool {} result: {}", tool_name, result));
                        }
                    }
                }
            }
            
            max_steps -= 1;
        }
        
        Ok(self.claims.clone())
    }
}
