pub mod tools;

use crate::tools::NavigatorTool;
use orchyn_codebase_indexer::AppMap;
use orchyn_core::Result;
use orchyn_model_client::{QwenClient, ToolDefinition, CompletionRequest};
use serde_json::Value;
use tracing::{debug, info, warn};
use futures::StreamExt;

/// System prompt and skills embedded at compile time. Edits to these files
/// rebuild the crate.
pub const PROMPT_NAVIGATOR: &str = include_str!("../../../prompts/navigator.md");
pub const SKILL_CODEBASE_NAVIGATION: &str =
    include_str!("../../../skills/codebase-navigation/SKILL.md");
pub const SKILL_SPEC_INFERENCE: &str =
    include_str!("../../../skills/spec-inference/SKILL.md");

pub struct NavigatorAgent {
    model: QwenClient,
    tools: Vec<Box<dyn NavigatorTool>>,
    app_map: AppMap,
    claims: Vec<Value>, // Inferred claims
}

impl NavigatorAgent {
    pub fn new(model: QwenClient, app_map: AppMap) -> Self {
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
                })
            });

            tool_definitions.push(ToolDefinition {
                name: "finish".to_string(),
                description: "Finish navigation and return findings.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "summary": { "type": "string" }
                    }
                })
            });

            let tool_json = serde_json::to_string(&tool_definitions).unwrap();
            // Embed the repo's navigator prompt + skills so the agent's
            // behavior tracks `prompts/navigator.md` and the SKILL.md files.
            // Use Qwen chat tokens (the engine is Qwen).
            let prompt = format!(
                "<|im_start|>system\n{}\n\n\
                === SKILL: CODEBASE NAVIGATION ===\n{}\n\n\
                === SKILL: SPEC INFERENCE ===\n{}\n\n\
                === OUTPUT FORMAT ===\n\
                Return ONLY a single JSON object: {{\"name\":\"<tool>\",\"arguments\":{{...}}}}.\n\
                No prose, no markdown fences, no commentary.<|im_end|>\n\
                <|im_start|>user\nGoal: {}\n\nHistory:\n{}\n\nAvailable tools: {}\n\nNext action (JSON only):<|im_end|>\n<|im_start|>assistant\n",
                PROMPT_NAVIGATOR, SKILL_CODEBASE_NAVIGATION, SKILL_SPEC_INFERENCE,
                goal, history, tool_json
            );

            let req = CompletionRequest {
                prompt,
                max_tokens: Some(512),
                temperature: Some(0.0),
            };

            let mut stream = self.model.complete(req).await?;
            let mut response = String::new();
            while let Some(chunk) = stream.next().await {
                let text = chunk?;
                response.push_str(&text);
            }

            let json_val: Value = match extract_json(&response) {
                Ok(v) => v,
                Err(reason) => {
                    let notice = format!(
                        "Model output was not parseable JSON ({}). Raw: {}. Reply with ONLY {{\"name\":\"<tool>\",\"arguments\":{{...}}}}.",
                        reason,
                        response.chars().take(400).collect::<String>()
                    );
                    warn!("{}", notice);
                    history.push_str(&format!("\nREJECTED: {}", notice));
                    max_steps -= 1;
                    continue;
                }
            };

            let action_name = match json_val.get("name").and_then(|v| v.as_str()) {
                Some(n) if !n.is_empty() => n.to_string(),
                _ => {
                    let notice = format!(
                        "Tool call missing 'name' field. Got: {}. Reply with {{\"name\":\"<tool>\",\"arguments\":{{...}}}}.",
                        json_val
                    );
                    warn!("{}", notice);
                    history.push_str(&format!("\nREJECTED: {}", notice));
                    max_steps -= 1;
                    continue;
                }
            };

            if action_name == "finish" {
                info!("Agent signaled finish.");
                break;
            }

            if action_name == "propose_claim" {
                let args = json_val.get("arguments").cloned().unwrap_or(Value::Object(Default::default()));
                info!("Claim proposed: {:?}", args);
                self.claims.push(args);
                history.push_str("\nAction: propose_claim acknowledged.");
            } else {
                let tool = match self.tools.iter().find(|t| t.name() == action_name) {
                    Some(t) => t,
                    None => {
                        let available: Vec<&str> = self
                            .tools
                            .iter()
                            .map(|t| t.name())
                            .chain(["propose_claim", "finish"])
                            .collect();
                        let notice = format!(
                            "Unknown tool '{}'. Available: {}.",
                            action_name,
                            available.join(", ")
                        );
                        warn!("{}", notice);
                        history.push_str(&format!("\nREJECTED: {}", notice));
                        max_steps -= 1;
                        continue;
                    }
                };

                let args = json_val.get("arguments").cloned().unwrap_or(Value::Object(Default::default()));
                let result = tool.call(args).await?;
                debug!("Tool {} result received.", action_name);
                history.push_str(&format!("\nAction: {}, Result: {}", action_name, result));
            }

            max_steps -= 1;
        }

        if max_steps == 0 {
            warn!("Navigator Agent exhausted safety limit.");
        }

        Ok(self.claims.clone())
    }
}

/// Extract the outermost JSON object from the model's response. Brace-balanced
/// with string/escape awareness so prose or markdown fences around the object
/// don't break parsing.
fn extract_json(response: &str) -> std::result::Result<Value, String> {
    let start = response.find('{').ok_or_else(|| "no '{' found".to_string())?;
    let bytes = response.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&response[start..=i])
                        .map_err(|e| format!("invalid JSON: {}", e));
                }
            }
            _ => {}
        }
    }
    Err("unbalanced braces".to_string())
}
