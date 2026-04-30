use crate::agent::memory::{Message, TurnRecord, TurnToolResult, WorkingMemory};
use futures::StreamExt;
use orchyn_core::{OrchynError, Result};
use orchyn_model_client::{CompletionRequest, ModelClient};
use orchyn_toolkit::{AdapterFactory, ToolCall, ToolContext, ToolRegistry, ToolSchema};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("../../../../prompts/agent_system.md");
const DEFAULT_MAX_MESSAGES: usize = 20;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 768;
const DEFAULT_TURN_TEMPERATURE: f32 = 0.0;

fn collect_env_info(app_url: &str, project_root: &std::path::Path) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".into());
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let project = project_root.display();
    format!(
        "# Environment\n\
         - OS: {os} ({arch})\n\
         - Shell: {shell}\n\
         - Working directory: {cwd}\n\
         - Project root: {project}\n\
         - Date: {date}\n\
         - App under test: {app_url}"
    )
}

fn build_system_prompt(app_url: &str, project_root: &std::path::Path) -> String {
    format!("{}\n\n{}", SYSTEM_PROMPT_TEMPLATE, collect_env_info(app_url, project_root))
}

/// Drives Agent Mode: the model picks tools each turn, the harness runs
/// them, and the resulting observations feed back into the next prompt.
pub struct AgentHarness {
    llm: Arc<dyn ModelClient>,
    tools: ToolRegistry,
    ctx: ToolContext,
    memory: WorkingMemory,
    session_id: Uuid,
    max_turns: usize,
    /// Built once at `run()` time from the prompt template + env info.
    system_prompt: String,
}

#[derive(Debug, Clone)]
pub struct AgentReport {
    pub session_id: Uuid,
    pub turns_used: usize,
    pub gaps_found: usize,
    pub tests_executed: usize,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub fixes_proposed: usize,
    pub working_memory: serde_json::Value,
    pub completed: bool,
    /// Structured report emitted by the model in its final `done: true` turn.
    pub final_report: Option<serde_json::Value>,
}

impl AgentHarness {
    pub fn new(
        llm: Arc<dyn ModelClient>,
        tools: ToolRegistry,
        ctx: ToolContext,
        max_turns: usize,
    ) -> Self {
        Self {
            llm,
            tools,
            ctx,
            memory: WorkingMemory::new(DEFAULT_MAX_MESSAGES),
            session_id: Uuid::new_v4(),
            max_turns,
            system_prompt: String::new(),
        }
    }

    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Run the agent against an app. The harness spawns a default `explorer`
    /// actor (if a factory is installed) and hands control to the model.
    pub async fn run(&mut self, app_url: &str) -> Result<AgentReport> {
        info!(
            "agent::AgentHarness session {} starting (app_url: {app_url})",
            self.session_id
        );

        self.system_prompt = build_system_prompt(app_url, &self.ctx.project_root);

        // Try to spawn the default actor. If no factory is installed, the
        // model can still operate on whatever actors the caller pre-populated.
        if let Some(factory) = self.ctx.adapter_factory.clone() {
            self.spawn_actor("explorer", factory.as_ref()).await?;
        }

        // Seed the conversation with the app source — the system prompt
        // instructs the agent how to handle URLs, GitHub repos, or local paths.
        self.memory.push_message(Message::user(format!(
            "APP: {app_url}\n\nBegin. If the app is not yet running, set it up first using shell tools, then take a screenshot."
        )));

        let mut completed = false;
        let mut final_report: Option<serde_json::Value> = None;

        for turn in 0..self.max_turns {
            let observations = self.collect_observations().await;
            let observation_msg = format!(
                "TURN {turn} OBSERVATIONS:\n{}",
                serde_json::to_string_pretty(&observations).unwrap_or_default()
            );
            self.memory.push_message(Message::user(observation_msg));

            let prompt = self.build_prompt();
            let raw = self.complete(prompt).await?;

            self.memory.push_message(Message::assistant(raw.clone()));

            let parsed = match parse_agent_response(&raw) {
                Ok(p) => p,
                Err(e) => {
                    warn!("agent: failed to parse model response on turn {turn}: {e}");
                    self.memory.push_message(Message::user(format!(
                        "ERROR parsing your last response: {e}. Output ONLY a JSON object with \
                         keys reasoning, tool_calls, done."
                    )));
                    continue;
                }
            };

            if parsed.tool_calls.is_empty() && parsed.done {
                final_report = parsed.report.clone();
                self.memory.record_turn(TurnRecord {
                    turn,
                    reasoning: parsed.reasoning.clone(),
                    tool_results: vec![],
                });
                completed = true;
                break;
            }

            let mut turn_results: Vec<TurnToolResult> = Vec::new();
            let mut saw_done = false;
            for call in parsed.tool_calls {
                info!("agent: turn {turn} -> {} {}", call.name, call.arguments);
                if call.name == "done" {
                    saw_done = true;
                }
                let result = self.tools.execute(&call, &self.ctx).await;
                turn_results.push(TurnToolResult {
                    call,
                    result: result
                        .as_ref()
                        .map(|r| r.clone())
                        .map_err(|e| e.to_string()),
                });
            }

            // Echo tool results back into the conversation so the model has
            // them on its next turn.
            let result_summary = serde_json::to_string(
                &turn_results
                    .iter()
                    .map(|r| match &r.result {
                        Ok(ok) => serde_json::json!({
                            "tool": r.call.name,
                            "ok": true,
                            "data": ok.data,
                        }),
                        Err(err) => serde_json::json!({
                            "tool": r.call.name,
                            "ok": false,
                            "error": err,
                        }),
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default();
            self.memory.push_message(Message::user(format!(
                "TOOL_RESULTS:\n{result_summary}"
            )));

            self.memory.record_turn(TurnRecord {
                turn,
                reasoning: parsed.reasoning,
                tool_results: turn_results,
            });

            if saw_done || parsed.done {
                final_report = parsed.report.clone();
                completed = true;
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        self.build_report(completed, final_report).await
    }

    async fn spawn_actor(&self, name: &str, factory: &dyn AdapterFactory) -> Result<()> {
        let adapter = factory.spawn(name).await?;
        let mut actors = self.ctx.actors.lock().await;
        actors.insert(
            name.to_string(),
            Arc::new(tokio::sync::Mutex::new(adapter)),
        );
        Ok(())
    }

    async fn collect_observations(&self) -> serde_json::Value {
        let mut obs = serde_json::Map::new();
        let actors = self.ctx.actors.lock().await;
        for (id, adapter) in actors.iter() {
            let mut guard = adapter.lock().await;
            let url = guard.current_url().await.unwrap_or_default();
            let telemetry = guard.collect_telemetry().await.unwrap_or_default();
            let console_errors: Vec<_> = telemetry
                .console_logs
                .iter()
                .filter(|l| matches!(l.level.as_str(), "error" | "warn"))
                .map(|l| format!("[{}] {}", l.level, l.message))
                .take(3)
                .collect();
            let net_failures: Vec<_> = telemetry
                .network_requests
                .iter()
                .filter(|r| r.status >= 400)
                .map(|r| format!("{} {} -> {}", r.method, r.url, r.status))
                .take(3)
                .collect();
            obs.insert(
                id.clone(),
                serde_json::json!({
                    "current_url": url,
                    "console_errors": console_errors,
                    "network_failures": net_failures,
                }),
            );
        }
        serde_json::Value::Object(obs)
    }

    fn build_prompt(&self) -> String {
        let schemas = self.tools.schemas();
        let tools_json = serde_json::to_string_pretty(&schemas).unwrap_or_default();
        let history = self
            .memory
            .messages()
            .iter()
            .map(|m| format!("[{}] {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        format!(
            "{}\n\nTOOLS:\n{tools_json}\n\nCONVERSATION:\n{history}\n\nRespond now with the JSON object.",
            self.system_prompt
        )
    }

    async fn complete(&self, prompt: String) -> Result<String> {
        let req = CompletionRequest {
            prompt,
            max_tokens: Some(DEFAULT_MAX_OUTPUT_TOKENS),
            temperature: Some(DEFAULT_TURN_TEMPERATURE),
        };
        let mut stream = self.llm.complete(req).await?;
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            buf.push_str(&chunk?);
        }
        Ok(buf)
    }

    async fn build_report(&self, completed: bool, final_report: Option<serde_json::Value>) -> Result<AgentReport> {
        let mem = self.ctx.memory.lock().await;
        let snapshot = mem.snapshot();
        let gaps_found    = snapshot.keys().filter(|k| k.starts_with("gap_")).count();
        let tests_executed = snapshot.keys().filter(|k| k.starts_with("result_")).count();
        let tests_passed  = snapshot.keys().filter(|k| k.starts_with("result_pass_")).count();
        let tests_failed  = snapshot.keys().filter(|k| k.starts_with("result_fail_")).count();
        let fixes_proposed = snapshot.keys().filter(|k| k.starts_with("fix_")).count();
        let working_memory = serde_json::to_value(&snapshot)
            .map_err(|e| OrchynError::Internal(format!("memory snapshot: {e}")))?;
        Ok(AgentReport {
            session_id: self.session_id,
            turns_used: self.memory.turn_count(),
            gaps_found,
            tests_executed,
            tests_passed,
            tests_failed,
            fixes_proposed,
            working_memory,
            completed,
            final_report,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ParsedResponse {
    #[serde(default)]
    reasoning: String,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    report: Option<serde_json::Value>,
}

fn parse_agent_response(raw: &str) -> Result<ParsedResponse> {
    let json_text = extract_json_object(raw)?;
    serde_json::from_str(&json_text)
        .map_err(|e| OrchynError::Validation(format!("agent response JSON: {e}")))
}

fn extract_json_object(raw: &str) -> Result<String> {
    let start = raw
        .find('{')
        .ok_or_else(|| OrchynError::Validation("no JSON object in response".into()))?;
    let bytes = raw.as_bytes();
    let mut depth = 0i32;
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
                    return Ok(raw[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    Err(OrchynError::Validation("unbalanced braces in response".into()))
}

#[allow(dead_code)]
fn schema_names(schemas: &[ToolSchema]) -> Vec<&str> {
    schemas.iter().map(|s| s.name.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_response_with_done_true() {
        let raw = r#"{"reasoning":"finished","tool_calls":[],"done":true}"#;
        let r = parse_agent_response(raw).unwrap();
        assert!(r.done);
        assert!(r.tool_calls.is_empty());
    }

    #[test]
    fn parses_response_with_tool_call() {
        let raw = r#"
        Some chatter the model leaked.
        {"reasoning":"take a screenshot",
         "tool_calls":[{"name":"browser_screenshot","arguments":{"actor":"explorer"}}],
         "done":false}
        "#;
        let r = parse_agent_response(raw).unwrap();
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.tool_calls[0].name, "browser_screenshot");
    }

    #[test]
    fn rejects_malformed() {
        let raw = "not json";
        assert!(parse_agent_response(raw).is_err());
    }
}
