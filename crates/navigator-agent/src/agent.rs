use futures::StreamExt;
use orchyn_codebase_indexer::AppMap;
use orchyn_core::Result;
use orchyn_model_client::{CompletionRequest, ModelClient};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::tools::{GrepTool, ListFilesTool, NavigatorTool, ReadFileTool};

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/navigator.md");

pub struct NavigatorAgent {
    model: Arc<dyn ModelClient>,
    app_map: AppMap,
    tools: Vec<Box<dyn NavigatorTool>>,
}

impl NavigatorAgent {
    pub fn new(model: Arc<dyn ModelClient>, app_map: AppMap) -> Self {
        let root = app_map.root.clone();
        Self {
            model,
            app_map,
            tools: vec![
                Box::new(ListFilesTool::new(root.clone())),
                Box::new(ReadFileTool),
                Box::new(GrepTool::new(root)),
            ],
        }
    }

    pub async fn run(&mut self, goal: &str) -> Result<Vec<String>> {
        info!("Navigator: starting discovery loop for goal: {}", goal);

        let mut history = format!(
            "GOAL: {}\n\nPROJECT MAP:\nFiles: {:?}\n\n", 
            goal, 
            self.app_map.files.iter().map(|f| f.path.display().to_string()).collect::<Vec<_>>()
        );

        let mut claims = Vec::new();

        for step in 1..=40 {
            debug!("Navigator step {}", step);

            let prompt = format!(
                "<|im_start|>system\n{}\n<|im_end|>\n<|im_start|>user\nHistory:\n{}\n\nNext action (JSON):<|im_end|>\n<|im_start|>assistant\n",
                SYSTEM_PROMPT, history
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

            let json_val = match orchyn_core::utils::extract_json(&response) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Failed to parse navigator response: {}. Raw: {}", e, response);
                    history.push_str(&format!("\nStep {}: REJECTED - Invalid JSON", step));
                    continue;
                }
            };

            let action_name = json_val.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let args = json_val.get("arguments").cloned().unwrap_or(serde_json::json!({}));

            if action_name == "finish" {
                info!("Navigator finished: {}", args.get("summary").and_then(|v| v.as_str()).unwrap_or("No summary"));
                break;
            }

            if action_name == "propose_claim" {
                if let Some(stmt) = args.get("statement").and_then(|v| v.as_str()) {
                    claims.push(stmt.to_string());
                    history.push_str(&format!("\nStep {}: CLAIM PROPOSED: {}", step, stmt));
                }
                continue;
            }

            // Find and execute tool
            let mut tool_found = false;
            for tool in &self.tools {
                if tool.name() == action_name {
                    match tool.call(args.clone()).await {
                        Ok(res) => {
                            history.push_str(&format!("\nStep {}: Tool {} result: {}", step, action_name, res));
                        }
                        Err(e) => {
                            history.push_str(&format!("\nStep {}: Tool {} error: {}", step, action_name, e));
                        }
                    }
                    tool_found = true;
                    break;
                }
            }

            if !tool_found {
                history.push_str(&format!("\nStep {}: REJECTED - Unknown tool {}", step, action_name));
            }
        }

        Ok(claims)
    }
}
