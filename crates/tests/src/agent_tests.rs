/// Integration tests for Agent Mode: AgentHarness driven by a scripted model
/// and scripted adapter, no real browser or LLM required.

use super::mocks::{agent_done_response, agent_tool_call_response, scripted_model};
use orchyn_harness::agent::{AgentHarness, AgentReport};
use orchyn_toolkit::{default_registry, ToolContext};

fn harness_with_model(responses: Vec<String>, max_turns: usize) -> AgentHarness {
    let model = scripted_model(responses);
    let ctx = ToolContext::new(".");
    let registry = default_registry();
    AgentHarness::new(model, registry, ctx, max_turns)
}

// ── done on first turn ────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_completes_on_first_done_response() {
    let responses = vec![agent_done_response("No issues found.")];
    let mut harness = harness_with_model(responses, 10);
    let report = harness.run("http://localhost/").await.unwrap();

    assert!(report.completed, "agent should be marked completed");
    assert_eq!(report.turns_used, 1);
}

// ── final report is captured ──────────────────────────────────────────────────

#[tokio::test]
async fn agent_captures_final_report_on_done() {
    let response = serde_json::json!({
        "reasoning": "All done.",
        "tool_calls": [],
        "done": true,
        "report": {
            "app_type": "todo app",
            "features_found": ["login", "todo list", "counter"],
            "gaps_found": 2,
            "tests_created": 4,
            "summary": "Functional but has two critical bugs in the counter."
        }
    })
    .to_string();

    let mut harness = harness_with_model(vec![response], 10);
    let report = harness.run("http://localhost/").await.unwrap();

    let final_report = report.final_report.expect("final_report should be Some");
    assert_eq!(final_report["app_type"], "todo app");
    assert_eq!(final_report["gaps_found"], 2);
    assert_eq!(final_report["features_found"].as_array().unwrap().len(), 3);
}

// ── result counters via working memory ───────────────────────────────────────

#[tokio::test]
async fn agent_result_counters_driven_by_memory_keys() {
    // The agent runs two tests inline, stores results, then finishes.
    let responses = vec![
        agent_tool_call_response(
            "memory_store",
            serde_json::json!({ "key": "gap_empty_email", "value": { "description": "empty email accepted" } }),
        ),
        agent_tool_call_response(
            "memory_store",
            serde_json::json!({ "key": "result_pass_login_happy", "value": { "ok": true } }),
        ),
        agent_tool_call_response(
            "memory_store",
            serde_json::json!({ "key": "result_fail_counter_nan", "value": { "ok": false, "reason": "NaN on load" } }),
        ),
        agent_done_response("found 1 gap, 1 pass, 1 fail"),
    ];

    let mut harness = harness_with_model(responses, 10);
    let report = harness.run("http://localhost/").await.unwrap();

    assert!(report.completed);
    assert_eq!(report.gaps_found, 1);
    assert_eq!(report.tests_executed, 2, "both result_ keys count");
    assert_eq!(report.tests_passed, 1);
    assert_eq!(report.tests_failed, 1);
}

// ── max turns guard ───────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_stops_at_max_turns_even_without_done() {
    // Return 100 non-done responses; max_turns = 3
    let responses: Vec<String> = (0..100)
        .map(|_| {
            serde_json::json!({
                "reasoning": "still working",
                "tool_calls": [],
                "done": false,
            })
            .to_string()
        })
        .collect();

    let mut harness = harness_with_model(responses, 3);
    let report = harness.run("http://localhost/").await.unwrap();

    assert!(!report.completed, "not completed because done was never true");
    assert!(report.turns_used <= 3, "turns_used must respect max_turns");
}

// ── malformed response recovery ───────────────────────────────────────────────

#[tokio::test]
async fn agent_recovers_from_one_malformed_response() {
    let responses = vec![
        "this is not json at all".to_string(),
        // The harness sends an error-correction message and retries;
        // second response is valid and done.
        agent_done_response("recovered"),
    ];

    let mut harness = harness_with_model(responses, 5);
    let report = harness.run("http://localhost/").await.unwrap();

    assert!(report.completed);
}

// ── report structure ──────────────────────────────────────────────────────────

#[test]
fn agent_report_defaults_are_sensible() {
    // Construct a minimal report as the harness would after 0 turns.
    let report = AgentReport {
        session_id: uuid::Uuid::new_v4(),
        turns_used: 0,
        gaps_found: 0,
        tests_executed: 0,
        tests_passed: 0,
        tests_failed: 0,
        fixes_proposed: 0,
        working_memory: serde_json::Value::Object(Default::default()),
        completed: false,
        final_report: None,
        turn_observations: vec![],
    };
    assert!(!report.completed);
    assert!(report.final_report.is_none());
    assert_eq!(report.gaps_found, 0);
}

// ── system prompt includes env info ──────────────────────────────────────────

#[tokio::test]
async fn agent_system_prompt_contains_env_section() {
    // We can't inspect the prompt directly, but we can verify the harness
    // doesn't panic or error when building it. A single done response is
    // sufficient to confirm that build_system_prompt ran without error.
    let mut harness = harness_with_model(vec![agent_done_response("ok")], 1);
    harness.run("http://localhost:5174/").await.unwrap();
}

// ── done flag on last tool_call list ─────────────────────────────────────────

#[tokio::test]
async fn agent_completes_when_tool_calls_include_done_pseudo_tool() {
    // Some model outputs may include {"name":"done"} as the last tool call
    // rather than setting done:true at the top level.
    let response = serde_json::json!({
        "reasoning": "wrapping up",
        "tool_calls": [{"name": "done", "arguments": {"summary": "finished"}}],
        "done": false,
    })
    .to_string();

    let mut harness = harness_with_model(vec![response], 5);
    let report = harness.run("http://localhost/").await.unwrap();
    assert!(report.completed, "saw_done should trigger completion");
}
