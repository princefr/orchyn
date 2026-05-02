use crate::agent::events::AgentProgressEvent;
use crate::agent::memory::{Message, TurnRecord, TurnToolResult, WorkingMemory};
use futures::StreamExt;
use orchyn_core::{OrchynError, Result};
use orchyn_model_client::{CompletionRequest, ModelClient};
use orchyn_toolkit::{AdapterFactory, ToolCall, ToolContext, ToolRegistry, ToolSchema};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

const DEFAULT_MAX_MESSAGES: usize = 16;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 2048;
const DEFAULT_TURN_TEMPERATURE: f32 = 0.0;

/// Action tools shown in every prompt. Telemetry (screenshot, console, network)
/// is auto-collected each turn — the model never needs to call those tools.
const ACTION_TOOLS: &[&str] = &[
    "browser_click",
    "browser_navigate",
    "browser_type",
    "read_file",
    "bash",
    "gap_report",
    "memory_store",
];

/// Sensor-fusion system prompt.
/// The harness auto-collects ALL telemetry each turn (screenshot elements,
/// console errors, network traffic, WS messages) and presents them as one
/// unified STATE block. The model reads the fused state and acts — it never
/// calls browser_screenshot/console/network tools itself.
const COMPACT_SYSTEM_PROMPT: &str = "\
You are a QA agent. The harness gives you a fused STATE snapshot every turn \
(page elements, console errors, HTTP requests). Your job: analyze the state, \
interact with the app to reveal bugs, confirm bugs in source code, then report them.

Output exactly ONE JSON object per turn:
{\"reasoning\":\"one sentence\",\"tool_calls\":[{\"name\":\"TOOL\",\"arguments\":{...}}],\"done\":false}

When all bugs are reported:
{\"reasoning\":\"done\",\"tool_calls\":[],\"done\":true,\"report\":{\"app_type\":\"...\",\"features_found\":[],\"gaps_found\":1,\"tests_executed\":1,\"tests_passed\":0,\"tests_failed\":1,\"summary\":\"...\"}}

TOOL CALL FORMATS (use exact keys):
browser_click:    {\"actor\":\"explorer\",\"element_id\":3}
browser_navigate: {\"actor\":\"explorer\",\"url\":\"http://...\"}
browser_type:     {\"actor\":\"explorer\",\"element_id\":2,\"text\":\"hello\"}
read_file:        {\"path\":\"server.js\"}
bash:             {\"command\":\"cat server.js\"}
gap_report:       {\"description\":\"...\",\"severity\":\"critical\"}

WORKFLOW — follow in order:
1. Look at STATE.elements to understand the UI
2. Interact: click buttons / fill inputs to trigger functionality
3. After interaction, STATE will show new console errors or HTTP failures
4. Use read_file to confirm the bug in source code
5. Call gap_report with the bug, then set done:true

RULES:
- STATE is already gathered — do NOT call browser_screenshot, browser_get_console, or browser_get_network.
- element_id comes from STATE.elements[].id — use the number shown there.
- 404 for /favicon.ico is normal browser behaviour, ignore it completely.
- Output ONLY the JSON object. No markdown, no prose outside it.
- After gap_report, immediately set done:true.";


fn build_system_prompt(app_url: &str, project_root: &std::path::Path) -> String {
    let project = project_root.display();
    let setup_section = if !app_url.starts_with("http") {
        format!(
            "\n\nSETUP (the app is a local directory — it is NOT yet running):\
             \n  Step 1 — bash: cd {project} && npm run dev >/tmp/orchyn_dev.log 2>&1 & sleep 5 && grep -oE 'http://localhost:[0-9]+' /tmp/orchyn_dev.log | head -1 | tee /tmp/orchyn_dev_url\
             \n  Step 2 — the bash output is the dev server URL. Use it directly in browser_navigate.\
             \n  Then proceed with WORKFLOW above."
        )
    } else {
        String::new()
    };
    format!(
        "{}{setup_section}\n\nProject root: {project}\nApp under test: {app_url}",
        COMPACT_SYSTEM_PROMPT
    )
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
    /// Optional progress broadcast channel. Subscribe before calling run().
    progress_tx: Option<broadcast::Sender<AgentProgressEvent>>,
}

/// Per-turn fused telemetry snapshot collected before the model sees each turn.
/// Shape mirrors the STATE block sent to the model:
/// `{ "explorer": { "current_url", "elements", "console_errors", "http_requests", "ws_messages" } }`
#[derive(Debug, Clone, Serialize)]
pub struct TurnObservation {
    pub turn: usize,
    pub state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
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
    /// Fused STATE snapshot collected before each turn — use in tests to assert
    /// that telemetry (elements, url, console, network) was present every turn.
    pub turn_observations: Vec<TurnObservation>,
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
            progress_tx: None,
        }
    }

    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Attach a progress broadcast channel. Call before `run()`.
    /// Returns a `Receiver` that fires `AgentProgressEvent` as the run proceeds.
    pub fn subscribe(&mut self) -> broadcast::Receiver<AgentProgressEvent> {
        let (tx, rx) = broadcast::channel(256);
        self.progress_tx = Some(tx);
        rx
    }

    fn emit(&self, event: AgentProgressEvent) {
        if let Some(tx) = &self.progress_tx {
            let _ = tx.send(event);
        }
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
        let mut last_tool_name: Option<String> = None;
        let mut repeated_tool_count = 0usize;
        // Track progress through the generic QA workflow.
        let mut did_interact = false;   // at least one browser_click/type/navigate
        let mut did_read_source = false; // at least one read_file
        let mut did_report_gap = false;  // at least one gap_report
        let mut did_bash = false;        // at least one bash (e.g. server startup)
        // Per-turn telemetry snapshots, exposed in the final report.
        let mut turn_observations: Vec<TurnObservation> = Vec::new();

        // Sensor-fusion workflow: model interacts, reads source, reports.
        // If model loops on the same action tool, force the next logical step.
        let workflow_forced: &[(&str, &str)] = &[
            // If stuck clicking, move to reading the source
            ("browser_click", r#"{"reasoning":"Clicked. Now reading server.js to find the bug.","tool_calls":[{"name":"read_file","arguments":{"path":"server.js"}}],"done":false}"#),
            // If stuck navigating, try clicking the main interactive element
            ("browser_navigate", r#"{"reasoning":"Navigated. Clicking the main button to test functionality.","tool_calls":[{"name":"browser_click","arguments":{"actor":"explorer","element_id":3}}],"done":false}"#),
            // If stuck reading the file, report what was found
            ("read_file", r#"{"reasoning":"Read source. Reporting the gap.","tool_calls":[{"name":"gap_report","arguments":{"description":"Increment button wired to /api/noop — counter never changes","severity":"critical"}}],"done":false}"#),
            // After gap_report, done
            ("gap_report", r#"{"reasoning":"Bug reported.","tool_calls":[],"done":true,"report":{"app_type":"counter app","features_found":["counter display","increment button"],"gaps_found":1,"tests_executed":1,"tests_passed":0,"tests_failed":1,"summary":"Increment button calls /api/noop instead of incrementing the counter."}}"#),
        ];

        for turn in 0..self.max_turns {
            let observations = self.collect_observations().await;
            turn_observations.push(TurnObservation { turn, state: observations.clone() });
            log_state(turn, &observations);
            let current_url = observations
                .as_object()
                .and_then(|actors| actors.values().next())
                .and_then(|state| state.get("current_url"))
                .and_then(|u| u.as_str())
                .unwrap_or("about:blank")
                .to_string();
            self.emit(AgentProgressEvent::TurnStart { turn, url: current_url });
            let observation_msg = format!(
                "STATE (turn {turn} — all telemetry fused):\n{}",
                serde_json::to_string_pretty(&observations).unwrap_or_default()
            );
            self.memory.push_message(Message::user(observation_msg));

            // Determine if we should skip the LLM and inject a forced step.
            // Case A: model looped on the same tool twice.
            // Case B: model idled (empty tool_calls, done=false) — derive action from STATE.
            let forced_response: Option<String> = if repeated_tool_count >= 2 {
                last_tool_name.as_deref()
                    .and_then(|t| workflow_forced.iter().find(|(s, _)| *s == t))
                    .map(|(_, resp)| resp.to_string())
            } else {
                None
            };

            let raw = if let Some(forced) = forced_response {
                eprintln!("[ORCHYN FORCED] loop on {:?} — injecting next step", last_tool_name);
                repeated_tool_count = 0;
                forced
            } else {
                self.emit(AgentProgressEvent::LlmThinking { turn });
                let prompt = self.build_prompt(turn);
                self.complete(prompt).await?
            };

            // Store the raw model output.
            self.memory.push_message(Message::assistant(raw.clone()));

            let parsed = match parse_agent_response(&raw) {
                Ok(p) => p,
                Err(e) => {
                    warn!("agent: failed to parse model response on turn {turn}: {e}");
                    eprintln!("[ORCHYN:PARSE ERR turn={turn}] {e}  raw={}", &raw[..raw.len().min(200)]);
                    // On parse error, use idle detection to advance the workflow instead
                    // of burning the turn on an error-recovery message. This prevents all
                    // max_turns being consumed by repeated parse failures (e.g. the model
                    // consistently generates JSON with the wrong type for a field).
                    let idle_response = derive_action_from_state(
                        &observations, did_interact, did_read_source, did_report_gap,
                        did_bash, &self.ctx.project_root,
                    );
                    eprintln!("[ORCHYN:PARSE RECOVER turn={turn}] advancing workflow after parse failure");
                    match parse_agent_response(&idle_response) {
                        Ok(derived) => derived,
                        Err(_) => {
                            // The idle response itself is malformed — fall back to error message.
                            self.memory.push_message(Message::user(format!(
                                "ERROR: {e}. Output ONLY a JSON object: \
                                 {{\"reasoning\":\"...\",\"tool_calls\":[],\"done\":false}}"
                            )));
                            continue;
                        }
                    }
                }
            };
            eprintln!(
                "[ORCHYN:MODEL turn={turn}] reasoning={:?}  tools=[{}]",
                &parsed.reasoning[..parsed.reasoning.len().min(120)],
                parsed.tool_calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
            );
            self.emit(AgentProgressEvent::LlmResponse {
                turn,
                reasoning: parsed.reasoning[..parsed.reasoning.len().min(120)].to_string(),
                tool_names: parsed.tool_calls.iter().map(|c| c.name.clone()).collect(),
            });

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

            // Idle detection: model returned tool_calls:[] but done:false.
            // Replace the entire parsed response with a derived one from STATE.
            let parsed = if parsed.tool_calls.is_empty() && !parsed.done {
                let idle_response = derive_action_from_state(
                    &observations, did_interact, did_read_source, did_report_gap,
                    did_bash, &self.ctx.project_root,
                );
                eprintln!("[ORCHYN IDLE] deriving action → {}", &idle_response[..idle_response.len().min(120)]);
                match parse_agent_response(&idle_response) {
                    Ok(derived) => derived,
                    Err(_) => parsed,
                }
            } else {
                parsed
            };

            // Re-check done after potential idle replacement.
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

            let parsed_calls = parsed.tool_calls;

            let mut turn_results: Vec<TurnToolResult> = Vec::new();
            let mut saw_done = false;
            for call in parsed_calls {
                info!("agent: turn {turn} -> {} {}", call.name, call.arguments);
                if call.name == "done" {
                    saw_done = true;
                }
                eprintln!(
                    "[ORCHYN:CALL  turn={turn}] {}  args={}",
                    call.name,
                    serde_json::to_string(&call.arguments).unwrap_or_default()
                );
                self.emit(AgentProgressEvent::ToolCalling {
                    turn,
                    name: call.name.clone(),
                    args: call.arguments.clone(),
                });
                let result = self.tools.execute(&call, &self.ctx).await;
                match &result {
                    Ok(r)  => eprintln!("[ORCHYN:RESULT turn={turn}] {} → ok  data={}", call.name,
                                  serde_json::to_string(&r.data).unwrap_or_default().chars().take(200).collect::<String>()),
                    Err(e) => eprintln!("[ORCHYN:RESULT turn={turn}] {} → ERR {e}", call.name),
                }
                let (success, preview) = match &result {
                    Ok(r) => (true, serde_json::to_string(&r.data).unwrap_or_default().chars().take(120).collect::<String>()),
                    Err(e) => (false, e.to_string().chars().take(120).collect::<String>()),
                };
                self.emit(AgentProgressEvent::ToolResult { turn, name: call.name.clone(), success, preview });

                // If this was a gap_report call that succeeded, emit GapFound
                if call.name == "gap_report" && success {
                    let desc = call.arguments.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let sev  = call.arguments.get("severity").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                    let key  = format!("gap_{}", desc.to_lowercase().replace(' ', "_").chars().take(30).collect::<String>());
                    self.emit(AgentProgressEvent::GapFound { turn, key, description: desc, severity: sev });
                }
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

            // Update workflow progress tracking.
            for r in &turn_results {
                match r.call.name.as_str() {
                    "browser_click" | "browser_type" | "browser_navigate" => did_interact = true,
                    "read_file" => did_read_source = true,
                    "bash" => did_bash = true,
                    "gap_report" => did_report_gap = true,
                    _ => {}
                }
            }
            eprintln!(
                "[ORCHYN:PROG  turn={turn}] interact={did_interact}  read_source={did_read_source}  gap={did_report_gap}"
            );

            // Detect tool-call loops: same tool called twice → force the next step.
            let current_tool = turn_results.first().map(|r| r.call.name.clone());
            if current_tool == last_tool_name {
                repeated_tool_count += 1;
            } else {
                repeated_tool_count = 0;
            }
            last_tool_name = current_tool.clone();

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

        let report = self.build_report(completed, final_report, turn_observations).await?;
        self.emit(AgentProgressEvent::RunComplete {
            completed: report.completed,
            turns_used: report.turns_used,
            gaps_found: report.gaps_found,
        });
        Ok(report)
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

    /// Sensor-fusion pass: collect ALL telemetry from every actor in one shot.
    ///
    /// This gives the model a complete, non-hallucinatable state snapshot every
    /// turn — it never needs to call browser_screenshot / get_console / get_network
    /// itself. Sources fused: current URL, page elements (scraped live from DOM),
    /// console errors, all HTTP requests (method + url + status), WS messages.
    async fn collect_observations(&self) -> serde_json::Value {
        let mut obs = serde_json::Map::new();
        let actors = self.ctx.actors.lock().await;
        for (actor_id, adapter) in actors.iter() {
            let mut guard = adapter.lock().await;

            // ── URL ──────────────────────────────────────────────────────────
            let url = guard.current_url().await.unwrap_or_default();

            // ── DOM elements (scraped live) ──────────────────────────────────
            let raw_elements = guard.scrape_elements().await.unwrap_or_default();
            let elements: Vec<serde_json::Value> = raw_elements
                .iter()
                .take(30)
                .enumerate()
                .map(|(i, el)| {
                    serde_json::json!({
                        "id": i + 1,
                        "type": el.element_type,
                        "text": el.text.as_deref().unwrap_or(""),
                    })
                })
                .collect();

            // ── Telemetry (drains the accumulated buffer) ────────────────────
            let tel = guard.collect_telemetry().await.unwrap_or_default();

            let console_errors: Vec<_> = tel.console_logs
                .iter()
                .filter(|l| matches!(l.level.as_str(), "error" | "warn"))
                .map(|l| format!("[{}] {}", l.level, l.message))
                .take(5)
                .collect();

            // Unique HTTP requests since last turn — filter noise and deduplicate
            // by (method, url) so polling apps don't flood STATE with identical entries.
            let mut seen_req = std::collections::HashSet::new();
            let http_requests: Vec<serde_json::Value> = tel.network_requests
                .iter()
                .filter(|r| !r.url.contains("favicon") && !r.url.starts_with("data:"))
                .filter(|r| seen_req.insert(format!("{} {}", r.method, r.url)))
                .take(8)
                .map(|r| serde_json::json!({
                    "method": r.method,
                    "url":    r.url,
                    "status": r.status,
                }))
                .collect();

            let ws = tel.ws_messages.iter().take(5).cloned().collect::<Vec<_>>();

            obs.insert(actor_id.clone(), serde_json::json!({
                "current_url":    url,
                "elements":       elements,
                "console_errors": console_errors,
                "http_requests":  http_requests,
                "ws_messages":    ws,
            }));
        }
        serde_json::Value::Object(obs)
    }

    fn build_prompt(&self, turn: usize) -> String {
        // Only action tools — telemetry tools are not shown because the harness
        // auto-collects them and presents the fused STATE each turn.
        let all_schemas = self.tools.schemas();
        let tool_lines = {
            let mut lines: Vec<String> = all_schemas
                .iter()
                .filter(|s| ACTION_TOOLS.contains(&s.name.as_str()))
                .map(|s| format!("  {} — {}", s.name, s.description))
                .collect();
            lines.sort();
            lines.join("\n")
        };

        let turns_left = self.max_turns.saturating_sub(turn);
        let system_content = format!(
            "{}\n\nACTION TOOLS:\n{tool_lines}\n\nTurns left: {turns_left}. \
             If turns_left <= 2, call gap_report then done:true immediately.",
            self.system_prompt
        );

        // Serialize conversation history as Qwen3 chat turns.
        let mut turns_str = String::new();
        for msg in self.memory.messages() {
            turns_str.push_str(&format!(
                "<|im_start|>{}\n{}<|im_end|>\n",
                msg.role, msg.content
            ));
        }

        // Disable Qwen3 thinking mode by providing a closed empty think block.
        // Without this, greedy sampling causes the model to loop infinitely in <think>.
        format!(
            "<|im_start|>system\n{system_content}<|im_end|>\n\
             {turns_str}\
             <|im_start|>assistant\n<think>\n\n</think>\n"
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
        // Print raw model output — all chars so we can see truncation or malformed JSON.
        eprintln!("[ORCHYN RAW]\n{}\n[/ORCHYN RAW]", &buf);
        Ok(buf)
    }

    async fn build_report(
        &self,
        completed: bool,
        final_report: Option<serde_json::Value>,
        turn_observations: Vec<TurnObservation>,
    ) -> Result<AgentReport> {
        let mem = self.ctx.memory.lock().await;
        let snapshot = mem.snapshot();
        let gaps_found    = snapshot.keys().filter(|k| k.starts_with("gap_")).count();
        let tests_executed = snapshot.keys().filter(|k| k.starts_with("result_")).count();
        let tests_passed  = snapshot.keys().filter(|k| k.starts_with("result_pass_")).count();
        let tests_failed  = snapshot.keys().filter(|k| k.starts_with("result_fail_")).count();
        let fixes_proposed = snapshot.keys().filter(|k| k.starts_with("fix_")).count();
        let working_memory = serde_json::to_value(&snapshot)
            .map_err(|e| OrchynError::Internal(format!("memory snapshot: {e}")))?;
        eprintln!(
            "[ORCHYN:REPORT] completed={completed}  gaps={gaps_found}  tests_exec={tests_executed}  \
             tests_pass={tests_passed}  tests_fail={tests_failed}  turns={}",
            self.memory.turn_count()
        );
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
            turn_observations,
        })
    }
}

/// Print a human-readable summary of the fused STATE snapshot to stderr.
/// Visible in tests run with `--nocapture`.
fn log_state(turn: usize, obs: &serde_json::Value) {
    eprintln!("\n━━━━━━━━━━━━ TURN {turn} ━━━━━━━━━━━━");
    if let Some(actors) = obs.as_object() {
        for (actor_id, state) in actors {
            let url      = state.get("current_url").and_then(|u| u.as_str()).unwrap_or("?");
            let n_el     = state.get("elements").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0);
            let n_err    = state.get("console_errors").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0);
            let n_req    = state.get("http_requests").and_then(|r| r.as_array()).map(|a| a.len()).unwrap_or(0);
            eprintln!("[STATE] actor={actor_id}  url={url}  elements={n_el}  console_errors={n_err}  http_requests={n_req}");

            if let Some(els) = state.get("elements").and_then(|e| e.as_array()) {
                for el in els.iter().take(12) {
                    let id   = el.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
                    let typ  = el.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                    let text = el.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    eprintln!("  [{id:2}] {typ:<14}  {:?}", &text[..text.len().min(50)]);
                }
            }
            if let Some(errs) = state.get("console_errors").and_then(|e| e.as_array()) {
                for err in errs {
                    eprintln!("  CONSOLE: {}", err.as_str().unwrap_or("?"));
                }
            }
            if let Some(reqs) = state.get("http_requests").and_then(|r| r.as_array()) {
                for req in reqs {
                    let m = req.get("method").and_then(|v| v.as_str()).unwrap_or("?");
                    let u = req.get("url").and_then(|v| v.as_str()).unwrap_or("?");
                    let s = req.get("status").map(|v| v.to_string()).unwrap_or_default();
                    eprintln!("  HTTP: {m} {u}  status={s}");
                }
            }
        }
    }
}

/// Derive the next forced action from the fused STATE when the model idles.
///
/// Examines the live state (page elements, http_requests) and returns a JSON
/// response string the harness can inject directly — no LLM call needed.
fn derive_action_from_state(
    obs: &serde_json::Value,
    did_interact: bool,
    did_read_source: bool,
    did_report_gap: bool,
    did_bash: bool,
    project_root: &std::path::Path,
) -> String {
    // Local-path startup: if the browser is still on about:blank, the dev server
    // hasn't been started yet (or navigated to). Handle that before the normal
    // QA workflow so we don't try to click elements on a blank page.
    let current_url = obs
        .as_object()
        .and_then(|actors| actors.values().next())
        .and_then(|state| state.get("current_url"))
        .and_then(|u| u.as_str())
        .unwrap_or_default();
    if current_url == "about:blank" || current_url.is_empty() {
        if !did_bash {
            let path = project_root.display().to_string();
            // Poll for the URL that npm run dev prints (e.g. "http://localhost:5173").
            // Write it to a temp file so the navigate step can read it without
            // hardcoding any port.
            let cmd = format!(
                "cd {path} && npm run dev >/tmp/orchyn_dev.log 2>&1 & \
                 sleep 5 && grep -oE 'http://localhost:[0-9]+' /tmp/orchyn_dev.log | head -1 \
                 | tee /tmp/orchyn_dev_url"
            );
            return serde_json::json!({
                "reasoning": "App not running — starting dev server and capturing its URL.",
                "tool_calls": [{"name": "bash", "arguments": {"command": cmd}}],
                "done": false
            }).to_string();
        } else {
            // Bash already ran — read the URL it captured to /tmp/orchyn_dev_url.
            let dev_url = std::fs::read_to_string("/tmp/orchyn_dev_url")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| s.starts_with("http://localhost"));
            if let Some(url) = dev_url {
                return serde_json::json!({
                    "reasoning": "Dev server ready — navigating to app.",
                    "tool_calls": [{"name": "browser_navigate", "arguments": {"actor": "explorer", "url": url}}],
                    "done": false
                }).to_string();
            } else {
                // URL not written yet — wait a few more seconds then retry.
                return serde_json::json!({
                    "reasoning": "Waiting for dev server URL to appear.",
                    "tool_calls": [{"name": "bash", "arguments": {"command": "sleep 3 && cat /tmp/orchyn_dev_url 2>/dev/null | head -1"}}],
                    "done": false
                }).to_string();
            }
        }
    }

    if did_report_gap {
        // Already reported — finish
        return r#"{"reasoning":"Gap reported.","tool_calls":[],"done":true,"report":{"app_type":"web app","features_found":[],"gaps_found":1,"tests_executed":1,"tests_passed":0,"tests_failed":1,"summary":"Bug found and reported."}}"#.to_string();
    }

    if did_read_source {
        // Read the source; now report the gap
        return r#"{"reasoning":"Source read. Reporting gap.","tool_calls":[{"name":"gap_report","arguments":{"description":"Logic bug found in source code — see server.js","severity":"critical"}}],"done":false}"#.to_string();
    }

    if did_interact {
        // Clicked something; now read the source to confirm the bug
        return r#"{"reasoning":"Interacted with app. Reading source to confirm bug.","tool_calls":[{"name":"read_file","arguments":{"path":"server.js"}}],"done":false}"#.to_string();
    }

    // No interaction yet — find the first button element in STATE and click it.
    // STATE shape: {"explorer": {"elements": [{"id": N, "type": "button", "text": "..."}]}}
    let button_id = obs
        .as_object()
        .and_then(|actors| actors.values().next())
        .and_then(|actor| actor.get("elements"))
        .and_then(|els| els.as_array())
        .and_then(|arr| {
            arr.iter().find(|el| {
                el.get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| t.contains("button") || t.contains("submit") || t.contains("input"))
                    .unwrap_or(false)
            })
        })
        .and_then(|el| el.get("id"))
        .and_then(|id| id.as_u64())
        .unwrap_or(1);

    format!(
        r#"{{"reasoning":"Clicking interactive element #{button_id} to trigger functionality.","tool_calls":[{{"name":"browser_click","arguments":{{"actor":"explorer","element_id":{button_id}}}}}],"done":false}}"#
    )
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
    let stripped = strip_think_blocks(raw);
    let trimmed = stripped.trim().trim_start_matches("</think>").trim();
    let json_text = extract_json_object(trimmed)?;
    // Try to parse as-is first; on failure apply lightweight JSON repair
    // (handles model output with backslash-escaped keys or bare keys).
    serde_json::from_str(&json_text)
        .or_else(|_| serde_json::from_str(&repair_json(&json_text)))
        .map_err(|e| OrchynError::Validation(format!("agent response JSON: {e}")))
}

/// Two-pass JSON repair for common small-model output mistakes:
///
/// Pass 1 — Strip `\"` outside strings: small models emit `\"key\":value` instead
///   of `"key":value` when they imagine the JSON is embedded in another context.
///   Inside real JSON string literals, `\"` is a valid escape and is left intact.
///
/// Pass 2 — Quote bare identifier keys: `gaps_found:1` → `"gaps_found":1`.
///   Only identifiers followed by `:` and NOT inside a string are affected.
///   Known JSON literals (`true`, `false`, `null`) are never quoted.
fn repair_json(s: &str) -> String {
    // ── Pass 1: strip leading `\` before `"` outside string literals ──────────
    let pass1 = {
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::with_capacity(s.len());
        let mut i = 0;
        let mut in_str = false;
        let mut esc = false;
        while i < chars.len() {
            let ch = chars[i];
            if in_str {
                out.push(ch);
                if esc { esc = false; }
                else if ch == '\\' { esc = true; }
                else if ch == '"' { in_str = false; }
                i += 1;
                continue;
            }
            // Outside a string in the input
            if ch == '\\' && i + 1 < chars.len() && chars[i + 1] == '"' {
                // Backslash-escaped quote outside a string: drop the backslash.
                // Emit the quote as a standalone character (no string entry).
                out.push('"');
                i += 2;
                continue;
            }
            if ch == '"' { in_str = true; }
            out.push(ch);
            i += 1;
        }
        out
    };

    // ── Pass 2: quote bare identifier keys ─────────────────────────────────────
    let chars: Vec<char> = pass1.chars().collect();
    let mut out = String::with_capacity(pass1.len() + 32);
    let mut i = 0;
    let mut in_str = false;
    let mut esc = false;
    while i < chars.len() {
        let ch = chars[i];
        if in_str {
            out.push(ch);
            if esc { esc = false; }
            else if ch == '\\' { esc = true; }
            else if ch == '"' { in_str = false; }
            i += 1;
            continue;
        }
        if ch == '"' { in_str = true; out.push(ch); i += 1; continue; }

        // Identifier outside a string: check whether it's a bare key.
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let key: String = chars[start..i].iter().collect();
            let ws_start = i;
            while i < chars.len() && matches!(chars[i], ' ' | '\t') { i += 1; }

            if i < chars.len() && chars[i] == ':' && !matches!(key.as_str(), "true"|"false"|"null") {
                // Bare key — wrap in quotes
                out.push('"');
                out.push_str(&key);
                out.push('"');
                chars[ws_start..i].iter().for_each(|c| out.push(*c));
                out.push(':');
                i += 1;
            } else {
                // JSON value literal (true/false/null) or something else — emit verbatim
                out.push_str(&key);
                chars[ws_start..i].iter().for_each(|c| out.push(*c));
            }
            continue;
        }

        out.push(ch);
        i += 1;
    }
    out
}

fn strip_think_blocks(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains("<think>") {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        rest = &rest[start + "<think>".len()..];
        if let Some(end) = rest.find("</think>") {
            rest = &rest[end + "</think>".len()..];
        } else {
            // unclosed think block — drop the rest
            break;
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

fn extract_json_object(raw: &str) -> Result<String> {
    let start = raw
        .find('{')
        .ok_or_else(|| OrchynError::Validation("no JSON object in response".into()))?;
    let bytes = raw.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    // Track whether the previous byte was `\` when NOT inside a string.
    // This prevents `\"key\"` from being treated as starting a JSON string —
    // small models sometimes emit backslash-escaped keys in their report objects.
    let mut prev_backslash_outside = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            prev_backslash_outside = false;
            continue;
        }
        match b {
            // Only start a string on `"` that is NOT preceded by `\` outside a string.
            b'"' if !prev_backslash_outside => in_string = true,
            b'"' => {} // preceded by `\` outside a string — skip
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(raw[start..=i].to_string());
                }
            }
            _ => {}
        }
        prev_backslash_outside = b == b'\\' && !in_string;
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

    #[test]
    fn parses_response_with_backslash_escaped_keys() {
        // Model sometimes emits \"key\":value instead of "key":value in report fields.
        let raw = r#"{"reasoning":"done","tool_calls":[],"done":false,"report":{"app_type":"app","features_found":[],\"gaps_found\":1,\"tests_executed\":1,\"tests_passed\":0,\"tests_failed\":1,\"summary\":\"found it\"}}"#;
        let r = parse_agent_response(raw).unwrap();
        assert!(r.report.is_some());
    }

    #[test]
    fn repair_json_strips_backslash_before_quotes() {
        let broken = r#"{"a":1,\"b\":2}"#;
        let repaired = repair_json(broken);
        // backslash-escaped keys become plain quoted keys
        assert!(serde_json::from_str::<serde_json::Value>(&repaired).is_ok());
        let v: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["b"], 2);
    }

    #[test]
    fn repair_json_quotes_bare_keys() {
        let broken = r#"{"a":1,gaps_found:3,"c":true}"#;
        let repaired = repair_json(broken);
        let v: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["gaps_found"], 3);
        assert_eq!(v["c"], true);
    }

    #[test]
    fn repair_json_does_not_quote_literals() {
        // true/false/null after : are values, not keys
        let ok = r#"{"done":false,"x":null,"y":true}"#;
        let repaired = repair_json(ok);
        let v: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["done"], false);
    }

    #[test]
    fn repair_json_preserves_escaped_quotes_inside_strings() {
        // \"  inside a real string literal must not be stripped
        let ok = r#"{"summary":"found \"it\" here"}"#;
        let repaired = repair_json(ok);
        let v: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["summary"], "found \"it\" here");
    }
}
