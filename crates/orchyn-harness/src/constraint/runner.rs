use crate::constraint::recovery::{RecoveryAction, RecoveryPolicy, StuckDetector};
use crate::constraint::scheduler::TurnScheduler;
use crate::constraint::telemetry::compress_telemetry;
use crate::constraint::validation::{execute_with_retry, parse_and_validate};
use chrono::Utc;
use orchyn_core::action::{Action, AnnotatedElement, AssertionType};
use orchyn_core::{
    ActorState, ManifestStep, OrchynError, PlatformAdapter, Result, RunStatus, StepRecord,
    TelemetrySnapshot, TestRunRecord,
};
use orchyn_model_client::ModelClient;
use orchyn_pixel_observer::annotate_screenshot;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

const SYSTEM_PROMPT: &str = include_str!("../../../../prompts/system.md");

/// Drives Constraint Mode: a fixed manifest is replayed step-by-step against
/// one or more actors, with the local 0.8B model picking the action for each
/// turn from the on-screen element map.
pub struct TestRunner {
    manifest: Vec<ManifestStep>,
    actors: HashMap<String, Arc<Mutex<Box<dyn PlatformAdapter>>>>,
    actor_states: HashMap<String, ActorState>,
    llm: Arc<dyn ModelClient>,
    scheduler: TurnScheduler,
    stuck_detector: StuckDetector,
    recovery: RecoveryPolicy,
    project_id: String,
}

#[derive(Debug)]
pub struct ConstraintRunResult {
    pub record: TestRunRecord,
    pub actor_states: HashMap<String, ActorState>,
}

impl TestRunner {
    pub fn new(
        manifest: Vec<ManifestStep>,
        actors: HashMap<String, Box<dyn PlatformAdapter>>,
        llm: Arc<dyn ModelClient>,
    ) -> Self {
        let actor_states = actors
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    ActorState {
                        actor_id: id.clone(),
                        ..Default::default()
                    },
                )
            })
            .collect();

        let actors = actors
            .into_iter()
            .map(|(id, adapter)| (id, Arc::new(Mutex::new(adapter))))
            .collect();

        Self {
            scheduler: TurnScheduler::new(manifest.clone()),
            manifest,
            actors,
            actor_states,
            llm,
            stuck_detector: StuckDetector::new(3),
            recovery: RecoveryPolicy::new(3, 3),
            project_id: "orchyn".to_string(),
        }
    }

    pub fn with_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = project_id.into();
        self
    }

    pub async fn run(mut self) -> Result<ConstraintRunResult> {
        let run_id = Uuid::new_v4();
        let mut record = TestRunRecord {
            id: run_id,
            project_id: self.project_id.clone(),
            started_at: Utc::now(),
            finished_at: None,
            status: RunStatus::Running,
            manifest: self.manifest.clone(),
        };

        info!(
            "constraint::TestRunner starting run {} ({} steps, {} actors)",
            run_id,
            self.manifest.len(),
            self.actors.len()
        );

        let mut attempts: HashMap<usize, usize> = HashMap::new();
        let mut waits: HashMap<usize, usize> = HashMap::new();

        while let Some((actor_id, step_idx)) = self.scheduler.next_turn() {
            match self.execute_step(&actor_id, step_idx).await {
                Ok(StepOutcome::Advance) => {
                    self.scheduler.advance();
                    self.stuck_detector.reset();
                }
                Ok(StepOutcome::WaitAgain) => {
                    let counter = waits.entry(step_idx).or_insert(0);
                    *counter += 1;
                    if *counter >= self.recovery.max_waits {
                        record.status = RunStatus::Failed(format!(
                            "step {step_idx} stuck on WAIT after {} attempts",
                            counter
                        ));
                        record.finished_at = Some(Utc::now());
                        return Ok(ConstraintRunResult {
                            record,
                            actor_states: self.actor_states,
                        });
                    }
                }
                Ok(StepOutcome::Stuck) => {
                    record.status = RunStatus::Stuck;
                    record.finished_at = Some(Utc::now());
                    return Ok(ConstraintRunResult {
                        record,
                        actor_states: self.actor_states,
                    });
                }
                Err(e) => {
                    let attempt = attempts.entry(step_idx).or_insert(0);
                    *attempt += 1;
                    let step = self.manifest[step_idx].clone();
                    let action = self
                        .recovery
                        .decide(&step, *attempt, *waits.get(&step_idx).unwrap_or(&0));
                    warn!(
                        "step {} failed (attempt {}): {} -> {:?}",
                        step_idx, attempt, e, action
                    );
                    match action {
                        RecoveryAction::Retry => self.scheduler.retry_current(),
                        RecoveryAction::Skip => self.scheduler.skip_current(),
                        RecoveryAction::Halt => {
                            record.status = RunStatus::Failed(e.to_string());
                            record.finished_at = Some(Utc::now());
                            return Ok(ConstraintRunResult {
                                record,
                                actor_states: self.actor_states,
                            });
                        }
                    }
                }
            }
        }

        record.status = RunStatus::Passed;
        record.finished_at = Some(Utc::now());
        Ok(ConstraintRunResult {
            record,
            actor_states: self.actor_states,
        })
    }

    async fn execute_step(&mut self, actor_id: &str, step_idx: usize) -> Result<StepOutcome> {
        let adapter = self
            .actors
            .get(actor_id)
            .ok_or_else(|| OrchynError::Validation(format!("unknown actor: {actor_id}")))?
            .clone();

        let (annotated_png, element_map, telemetry, pre_url) = {
            let mut guard = adapter.lock().await;
            let raw_png = guard.screenshot().await?;
            let elements = guard.scrape_elements().await?;
            let (annotated, map) = annotate_screenshot(&raw_png, &elements)
                .map_err(|e| OrchynError::Internal(format!("annotate: {e}")))?;
            let raw_telemetry = guard.collect_telemetry().await?;
            let url = guard.current_url().await.unwrap_or_default();
            (annotated, map, compress_telemetry(raw_telemetry), url)
        };

        let payload = self.build_payload(actor_id, step_idx, &element_map, &telemetry);
        let prompt = format!("{SYSTEM_PROMPT}\n\nINPUT:\n{payload}");

        let llm = self.llm.clone();
        let raw_decision = execute_with_retry(
            || {
                let llm = llm.clone();
                let prompt = prompt.clone();
                let png = annotated_png.clone();
                async move {
                    let value = llm.observe(&png, &prompt).await?;
                    Ok(serde_json::to_string(&value)
                        .map_err(|e| OrchynError::Internal(format!("re-serialize: {e}")))?)
                }
            },
            3,
        )
        .await?;

        let decision = parse_and_validate(&raw_decision, &element_map, actor_id, step_idx)?;
        let action_label = action_label(&decision.action);

        let outcome = {
            let mut guard = adapter.lock().await;
            match &decision.action {
                Action::AssertSuccess { assertion_type, expected } => {
                    let pass = evaluate_assertion(
                        &mut **guard,
                        assertion_type,
                        expected,
                        &element_map,
                        &telemetry,
                    )
                    .await?;
                    if !pass {
                        return Err(OrchynError::Validation(format!(
                            "ASSERT_SUCCESS failed: {:?} {}",
                            assertion_type, expected
                        )));
                    }
                    StepOutcome::Advance
                }
                Action::AssertFailure { assertion_type, expected } => {
                    let pass = evaluate_assertion(
                        &mut **guard,
                        assertion_type,
                        expected,
                        &element_map,
                        &telemetry,
                    )
                    .await?;
                    if pass {
                        return Err(OrchynError::Validation(format!(
                            "ASSERT_FAILURE failed: {:?} should not be {}",
                            assertion_type, expected
                        )));
                    }
                    StepOutcome::Advance
                }
                Action::Wait { .. } => StepOutcome::WaitAgain,
                _ => {
                    guard.execute_harness(&decision.action, &element_map).await?;
                    StepOutcome::Advance
                }
            }
        };

        // Post-action: snapshot state + stuck check.
        let post_url = {
            let mut guard = adapter.lock().await;
            guard.current_url().await.unwrap_or_default()
        };
        let state = self
            .actor_states
            .get_mut(actor_id)
            .ok_or_else(|| OrchynError::Internal(format!("missing state for {actor_id}")))?;
        state.step_history.push(StepRecord {
            step_idx,
            action: action_label.clone(),
            success: matches!(outcome, StepOutcome::Advance),
            timestamp: Utc::now(),
        });
        state.current_url = Some(post_url);
        state.last_action = Some(action_label);
        state.last_target = match decision.action {
            Action::Click { target }
            | Action::Type { target, .. }
            | Action::Clear { target } => Some(target),
            _ => None,
        };

        let _ = pre_url;

        if matches!(outcome, StepOutcome::Advance) {
            let post_png = {
                let mut guard = adapter.lock().await;
                guard.screenshot().await?
            };
            if self.stuck_detector.is_stuck(&post_png)? {
                return Ok(StepOutcome::Stuck);
            }
        }

        Ok(outcome)
    }

    fn build_payload(
        &self,
        actor_id: &str,
        step_idx: usize,
        element_map: &[AnnotatedElement],
        telemetry: &TelemetrySnapshot,
    ) -> String {
        let actor_memory = self
            .actor_states
            .get(actor_id)
            .map(memory_summary)
            .unwrap_or(serde_json::Value::Null);

        let payload = serde_json::json!({
            "meta": {
                "active_actor": actor_id,
                "manifest_step": step_idx,
                "turn_timeout_ms": 5000,
            },
            "manifest": self.manifest.iter().map(|s| serde_json::json!({
                "step": s.step,
                "actor": s.actor,
                "instruction": s.instruction,
            })).collect::<Vec<_>>(),
            "actor_memory": actor_memory,
            "element_map": element_map,
            "telemetry": telemetry,
        });
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
    }
}

#[derive(Debug, Clone, Copy)]
enum StepOutcome {
    Advance,
    WaitAgain,
    Stuck,
}

fn memory_summary(state: &ActorState) -> serde_json::Value {
    serde_json::json!({
        "actor_id": state.actor_id,
        "logged_in": state.logged_in,
        "current_url": state.current_url,
        "last_action": state.last_action,
        "last_target": state.last_target,
        "total_steps": state.step_history.len(),
        "last_success": state.step_history.last().map(|s| s.success).unwrap_or(true),
    })
}

fn action_label(action: &Action) -> String {
    match action {
        Action::Navigate { .. } => "NAVIGATE",
        Action::Click { .. } => "CLICK",
        Action::Type { .. } => "TYPE",
        Action::Clear { .. } => "CLEAR",
        Action::Drag { .. } => "DRAG",
        Action::Wait { .. } => "WAIT",
        Action::AssertSuccess { .. } => "ASSERT_SUCCESS",
        Action::AssertFailure { .. } => "ASSERT_FAILURE",
        Action::WsWait { .. } => "WS_WAIT",
    }
    .to_string()
}

async fn evaluate_assertion(
    adapter: &mut dyn PlatformAdapter,
    assertion_type: &AssertionType,
    expected: &str,
    element_map: &[AnnotatedElement],
    telemetry: &TelemetrySnapshot,
) -> Result<bool> {
    let visible_text = || {
        element_map
            .iter()
            .filter_map(|e| e.text.as_deref())
            .any(|t| t.contains(expected))
    };

    Ok(match assertion_type {
        AssertionType::ElementVisible | AssertionType::TextContains => visible_text(),
        AssertionType::ElementHidden => !visible_text(),
        AssertionType::UrlIs => adapter.current_url().await? == expected,
        AssertionType::ConsoleClean => telemetry.console_errors.is_empty(),
        AssertionType::NetworkOk => telemetry.network_4xx_5xx.is_empty(),
        AssertionType::WsReceived => telemetry.ws_messages.iter().any(|m| m.contains(expected)),
    })
}
