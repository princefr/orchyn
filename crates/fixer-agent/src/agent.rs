use crate::tools::FixerTool;
use crate::traits::{Agent, AgentEvent};
use orchyn_core::{RealityContext, Result, EvidenceBundle};
use orchyn_model_client::{CompletionRequest, ModelClient, ToolDefinition};
use orchyn_pixel_observer::UiState;
use tracing::{info, warn};
use tokio::sync::broadcast;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use futures::StreamExt;

pub const SKILL_CODEBASE_NAVIGATION: &str =
    include_str!("../../../skills/codebase-navigation/SKILL.md");
pub const SKILL_GAP_DETECTION: &str =
    include_str!("../../../skills/gap-detection/SKILL.md");

const REQUIRED_BEFORE_FINISH: &[&str] = &["run_command", "observe_ui"];

pub struct FixerAgent<M: ModelClient> {
    model: Arc<M>,
    tools: Vec<Box<dyn FixerTool>>,
    events_tx: broadcast::Sender<AgentEvent>,
    max_steps: usize,
    evidence: Arc<Mutex<EvidenceBundle>>,
    initial_sha: Option<String>,
}

impl<M: ModelClient> FixerAgent<M> {
    pub fn new(
        model: Arc<M>,
        tools: Vec<Box<dyn FixerTool>>,
        initial_sha: Option<String>,
    ) -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            model,
            tools,
            events_tx: tx,
            max_steps: 30,
            evidence: Arc::new(Mutex::new(EvidenceBundle::default())),
            initial_sha,
        }
    }

    pub fn set_max_steps(&mut self, max: usize) {
        self.max_steps = max;
    }
}

impl<M: ModelClient> Agent for FixerAgent<M> {
    fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.events_tx.subscribe()
    }

    fn evidence(&self) -> Arc<Mutex<EvidenceBundle>> {
        self.evidence.clone()
    }

    async fn propose_fix(
        &mut self,
        reality: &RealityContext,
        visual_state: &UiState,
    ) -> Result<String> {
        info!("Fixer Agent starting convergence loop (Expert Skills Edition)...");

        let reality_json = serde_json::to_string_pretty(reality).unwrap_or_default();
        let visual_json = serde_json::to_string_pretty(visual_state).unwrap_or_default();

        let mut history = format!(
            "CONTEXT:\n- Visual state of the app: {}\n- System diagnostics/logs: {}\n",
            visual_json, reality_json
        );

        let mut current_step = 0;
        let mut tools_used: HashSet<String> = HashSet::new();

        while current_step < self.max_steps {
            current_step += 1;
            let _ = self.events_tx.send(AgentEvent::Step { n: current_step, max: self.max_steps });

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
                name: "finish".to_string(),
                description: format!(
                    "End the loop. BLOCKED until you have successfully called BOTH {} at least once. \
                    Calling finish before that will be rejected and the loop will continue.",
                    REQUIRED_BEFORE_FINISH.join(" AND ")
                ),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "summary": {
                            "type": "string",
                            "description": "Non-empty: what was fixed and how it was verified (cite the run_command output and the observe_ui observation)."
                        }
                    },
                    "required": ["summary"]
                }),
            });

            let tool_json = serde_json::to_string(&tool_definitions).unwrap();

            let verification_status = REQUIRED_BEFORE_FINISH
                .iter()
                .map(|t| format!("  - {}: {}", t, if tools_used.contains(*t) { "DONE" } else { "MISSING" }))
                .collect::<Vec<_>>()
                .join("\n");

            let prompt = format!(
                "<|im_start|>system\nYou are Orchyn's autonomous fixer agent. Follow the repo's expert skills below verbatim.\n\n\
                === SKILL: CODEBASE NAVIGATION ===\n{}\n\n\
                === SKILL: GAP DETECTION ===\n{}\n\n\
                === REPAIR STRATEGY ===\n\
                - DISCOVER: map the entry point with list_files / grep / read_file (range-bounded).\n\
                - REPRODUCE: call run_command (e.g. 'npm test', 'cargo check') to confirm the failure.\n\
                - PATCH: apply minimal, surgical changes using replace_text.\n\
                - VERIFY: call run_command AND observe_ui to prove it's fixed before finishing.\n\n\
                === OUTPUT FORMAT ===\n\
                Return ONLY a single JSON object: {{\"name\":\"<tool>\",\"arguments\":{{...}}}}.\n\
                No prose, no markdown fences, no commentary.\n\n\
                Available tools: {}\n<|im_end|>\n\
                <|im_start|>user\nVerification progress (finish is blocked until both are DONE):\n{}\n\n\
                History:\n{}\n\nNext action (JSON only):<|im_end|>\n<|im_start|>assistant\n",
                SKILL_CODEBASE_NAVIGATION, SKILL_GAP_DETECTION,
                tool_json, verification_status, history
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

            let json_val: serde_json::Value = match extract_json(&response) {
                Ok(v) => v,
                Err(reason) => {
                    let notice = format!(
                        "Model output was not a parseable JSON tool call ({}). Raw response: {}. \
                        Reply with ONLY a JSON object like {{\"name\":\"<tool>\",\"arguments\":{{...}}}}.",
                        reason,
                        response.chars().take(400).collect::<String>()
                    );
                    warn!("Step {}: {}", current_step, notice);
                    let _ = self.events_tx.send(AgentEvent::Notice(notice.clone()));
                    history.push_str(&format!("\nStep {}: REJECTED — {}", current_step, notice));
                    continue;
                }
            };

            let action_name = match json_val.get("name").and_then(|v| v.as_str()) {
                Some(n) if !n.is_empty() => n.to_string(),
                _ => {
                    let notice = format!(
                        "Tool call missing required 'name' field. Got: {}. Reply with {{\"name\":\"<tool>\",\"arguments\":{{...}}}}.",
                        json_val
                    );
                    warn!("Step {}: {}", current_step, notice);
                    let _ = self.events_tx.send(AgentEvent::Notice(notice.clone()));
                    history.push_str(&format!("\nStep {}: REJECTED — {}", current_step, notice));
                    continue;
                }
            };
            let args = json_val.get("arguments").cloned().unwrap_or(serde_json::Value::Object(Default::default()));

            if action_name == "finish" {
                let missing: Vec<&str> = REQUIRED_BEFORE_FINISH
                    .iter()
                    .filter(|t| !tools_used.contains(**t))
                    .copied()
                    .collect();
                if !missing.is_empty() {
                    let notice = format!(
                        "finish rejected — you have not yet called: {}. Verify reality before finishing.",
                        missing.join(", ")
                    );
                    warn!("Step {}: {}", current_step, notice);
                    let _ = self.events_tx.send(AgentEvent::Notice(notice.clone()));
                    history.push_str(&format!("\nStep {}: REJECTED finish — {}", current_step, notice));
                    continue;
                }
                let summary = match args.get("summary").and_then(|v| v.as_str()) {
                    Some(s) if !s.trim().is_empty() => s.trim().to_string(),
                    _ => {
                        let notice = "finish rejected — 'summary' must be a non-empty string describing what was fixed and how it was verified.".to_string();
                        warn!("Step {}: {}", current_step, notice);
                        let _ = self.events_tx.send(AgentEvent::Notice(notice.clone()));
                        history.push_str(&format!("\nStep {}: REJECTED finish — {}", current_step, notice));
                        continue;
                    }
                };
                info!("Fixer Agent finished: {}", summary);
                let _ = self.events_tx.send(AgentEvent::Finish { summary: summary.clone() });
                return Ok(summary);
            }

            let _ = self.events_tx.send(AgentEvent::ToolCall { name: action_name.clone(), args: args.clone() });

            let tool = match self.tools.iter().find(|t| t.name() == action_name) {
                Some(t) => t,
                None => {
                    let available: Vec<&str> = self.tools.iter().map(|t| t.name()).chain(std::iter::once("finish")).collect();
                    let notice = format!(
                        "Unknown tool '{}'. Available tools: {}.",
                        action_name,
                        available.join(", ")
                    );
                    warn!("Step {}: {}", current_step, notice);
                    let _ = self.events_tx.send(AgentEvent::Notice(notice.clone()));
                    history.push_str(&format!("\nStep {}: REJECTED — {}", current_step, notice));
                    continue;
                }
            };

            let result = tool.call(args).await;

            match result {
                Ok(val) => {
                    tools_used.insert(action_name.clone());
                    let summary = format!("{}", val);
                    let _ = self.events_tx.send(AgentEvent::ToolResult {
                        name: action_name.clone(),
                        ok: true,
                        summary: summary.chars().take(200).collect()
                    });
                    history.push_str(&format!("\nStep {}: Action {}, Result: {}", current_step, action_name, val));
                }
                Err(e) => {
                    let _ = self.events_tx.send(AgentEvent::ToolResult {
                        name: action_name.clone(),
                        ok: false,
                        summary: e.to_string()
                    });
                    history.push_str(&format!("\nStep {}: Action {}, Error: {}", current_step, action_name, e));
                }
            }

            let bundle = self.evidence.lock().unwrap().clone();
            let _ = self.events_tx.send(AgentEvent::Evidence(Box::new(bundle)));
        }

        let msg = format!(
            "Fixer agent reached safety limit ({} steps) without verifying fix. Tools used: [{}]. Required: [{}].",
            self.max_steps,
            tools_used.iter().cloned().collect::<Vec<_>>().join(", "),
            REQUIRED_BEFORE_FINISH.join(", ")
        );
        warn!("{}", msg);
        let _ = self.events_tx.send(AgentEvent::Finish { summary: msg.clone() });
        Ok(msg)
    }
}

fn extract_json(response: &str) -> std::result::Result<serde_json::Value, String> {
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
                    let slice = &response[start..=i];
                    return serde_json::from_str(slice)
                        .map_err(|e| format!("invalid JSON: {}", e));
                }
            }
            _ => {}
        }
    }
    Err("unbalanced braces".to_string())
}
