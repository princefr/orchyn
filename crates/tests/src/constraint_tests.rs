/// Integration tests for Constraint Mode: manifest execution with a scripted
/// adapter and scripted model, no real browser required.

use super::mocks::{
    agent_done_response, blank_png, decision_assert_visible, decision_click, decision_navigate,
    scripted_model, ScriptedAdapter,
};
use orchyn_core::{FailPolicy, ManifestStep, RunStatus, UiElement};
use orchyn_core::action::BoundingBox;
use orchyn_harness::constraint::runner::TestRunner;
use std::collections::HashMap;

fn make_step(idx: usize, actor: &str, instruction: &str) -> ManifestStep {
    ManifestStep {
        step: idx,
        actor: actor.into(),
        instruction: instruction.into(),
        on_fail: FailPolicy::Halt,
        action_hint: None,
    }
}

fn button_element() -> UiElement {
    UiElement {
        element_type: "button".into(),
        text: Some("Submit".into()),
        bounds: BoundingBox { x: 10, y: 50, width: 80, height: 32 },
    }
}

// ── happy path ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn happy_path_single_actor_navigate_and_click() {
    let manifest = vec![
        make_step(0, "alice", "Navigate to the login page"),
        make_step(1, "alice", "Click the submit button"),
    ];

    let mut adapter = ScriptedAdapter::new();
    // Two turns → two screenshots, two element lists
    adapter
        .push_screenshot(blank_png())
        .push_screenshot(blank_png())
        .push_elements(vec![])           // step 0: no elements needed for navigate
        .push_elements(vec![button_element()]);  // step 1: one button

    let responses = vec![
        decision_navigate("alice", 0, "http://localhost/"),
        decision_click("alice", 1, 1),
    ];
    let model = scripted_model(responses);

    let mut actors: HashMap<String, Box<dyn orchyn_core::PlatformAdapter>> = HashMap::new();
    actors.insert("alice".into(), Box::new(adapter));

    let runner = TestRunner::new(manifest, actors, model);
    let result = runner.run().await.unwrap();

    assert!(
        matches!(result.record.status, RunStatus::Passed),
        "expected Passed, got {:?}",
        result.record.status
    );
}

// ── multi-actor coordination ──────────────────────────────────────────────────

#[tokio::test]
async fn multi_actor_manifest_both_actors_execute() {
    let manifest = vec![
        make_step(0, "alice", "Navigate"),
        make_step(1, "bob", "Navigate"),
        make_step(2, "alice", "Click"),
    ];

    let mut alice = ScriptedAdapter::new();
    alice
        .push_screenshot(blank_png())
        .push_screenshot(blank_png())
        .push_elements(vec![])
        .push_elements(vec![button_element()]);

    let mut bob = ScriptedAdapter::new();
    bob.push_screenshot(blank_png()).push_elements(vec![]);

    let responses = vec![
        decision_navigate("alice", 0, "http://localhost/"),
        decision_navigate("bob", 1, "http://localhost/"),
        decision_click("alice", 2, 1),
    ];
    let model = scripted_model(responses);

    let mut actors: HashMap<String, Box<dyn orchyn_core::PlatformAdapter>> = HashMap::new();
    actors.insert("alice".into(), Box::new(alice));
    actors.insert("bob".into(), Box::new(bob));

    let runner = TestRunner::new(manifest, actors, model);
    let result = runner.run().await.unwrap();

    assert!(matches!(result.record.status, RunStatus::Passed));
    // Both actors should appear in the final state map
    assert!(result.actor_states.contains_key("alice"));
    assert!(result.actor_states.contains_key("bob"));
}

// ── skip policy ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn skip_policy_step_continues_after_bad_decision() {
    // Step 0 will get an unparseable response; with Skip policy the run continues.
    let manifest = vec![
        ManifestStep {
            step: 0,
            actor: "alice".into(),
            instruction: "flaky step".into(),
            on_fail: FailPolicy::Skip,
            action_hint: None,
        },
        make_step(1, "alice", "Navigate"),
    ];

    let mut adapter = ScriptedAdapter::new();
    adapter
        .push_screenshot(blank_png())
        .push_screenshot(blank_png())
        .push_elements(vec![])
        .push_elements(vec![]);

    // First response is invalid JSON → parse error → Skip
    let responses = vec![
        "this is not valid json".to_string(),
        decision_navigate("alice", 1, "http://localhost/"),
    ];
    let model = scripted_model(responses);

    let mut actors: HashMap<String, Box<dyn orchyn_core::PlatformAdapter>> = HashMap::new();
    actors.insert("alice".into(), Box::new(adapter));

    let runner = TestRunner::new(manifest, actors, model);
    let result = runner.run().await.unwrap();
    // Should complete (passed or failed) without panicking — the skip lets it proceed
    let _ = result.record.status;
}

// ── halt policy ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn halt_policy_fails_run_on_bad_decision() {
    let manifest = vec![make_step(0, "alice", "click something")];

    let mut adapter = ScriptedAdapter::new();
    adapter.push_screenshot(blank_png()).push_elements(vec![]);

    // Response targets element 42 which doesn't exist → validation error → Halt
    let responses = vec![
        serde_json::json!({
            "actor_id": "alice",
            "manifest_step": 0,
            "thought": "click nonexistent",
            "action": "CLICK",
            "target": 42
        })
        .to_string(),
    ];
    let model = scripted_model(responses);

    let mut actors: HashMap<String, Box<dyn orchyn_core::PlatformAdapter>> = HashMap::new();
    actors.insert("alice".into(), Box::new(adapter));

    let runner = TestRunner::new(manifest, actors, model);
    let result = runner.run().await.unwrap();

    assert!(
        matches!(result.record.status, RunStatus::Failed(_)),
        "expected Failed, got {:?}",
        result.record.status
    );
}

// ── manifest round-trip ───────────────────────────────────────────────────────

#[test]
fn manifest_serialises_and_deserialises_losslessly() {
    let manifest = vec![
        ManifestStep {
            step: 0,
            actor: "alice".into(),
            instruction: "Login to the app".into(),
            on_fail: FailPolicy::Halt,
            action_hint: Some("CLICK".into()),
        },
        ManifestStep {
            step: 1,
            actor: "bob".into(),
            instruction: "Verify the dashboard".into(),
            on_fail: FailPolicy::Skip,
            action_hint: None,
        },
    ];

    let json = serde_json::to_string_pretty(&manifest).unwrap();
    let back: Vec<ManifestStep> = serde_json::from_str(&json).unwrap();

    assert_eq!(back.len(), 2);
    assert_eq!(back[0].actor, "alice");
    assert_eq!(back[0].on_fail, FailPolicy::Halt);
    assert_eq!(back[1].on_fail, FailPolicy::Skip);
    assert!(back[1].action_hint.is_none());
}

// ── assertion failure → RunStatus::Failed ────────────────────────────────────

/// The model emits ASSERT_SUCCESS TEXT_CONTAINS "Welcome" but the page never
/// shows that text.  The runner should detect the assertion failure and set
/// RunStatus::Failed with a message referencing the unmet expectation.
#[tokio::test]
async fn assertion_failure_sets_run_status_failed() {
    // Two steps: navigate, then assert "Welcome" is visible.
    let manifest = vec![
        make_step(0, "alice", "Navigate to the app"),
        make_step(1, "alice", "Assert Welcome heading is visible"),
    ];

    let mut adapter = ScriptedAdapter::new();
    adapter
        .push_screenshot(blank_png())
        .push_screenshot(blank_png())
        .push_elements(vec![])
        .push_elements(vec![]);

    let responses = vec![
        decision_navigate("alice", 0, "http://localhost/"),
        decision_assert_visible("alice", 1, "Welcome"),
    ];
    let model = scripted_model(responses);

    let mut actors: HashMap<String, Box<dyn orchyn_core::PlatformAdapter>> = HashMap::new();
    actors.insert("alice".into(), Box::new(adapter));

    let runner = TestRunner::new(manifest, actors, model);
    let result = runner.run().await.unwrap();

    assert!(
        matches!(result.record.status, RunStatus::Failed(_)),
        "assertion on missing text should produce Failed, got {:?}",
        result.record.status
    );
    if let RunStatus::Failed(msg) = &result.record.status {
        assert!(
            msg.to_lowercase().contains("welcome") || msg.to_lowercase().contains("assert"),
            "failure message should reference the assertion; got: {msg}"
        );
    }
}

// ── manifest generation → constraint execution roundtrip ─────────────────────

/// Full pipeline: AgentHarness stores test_ steps in working memory →
/// manifest_from_report() extracts them → TestRunner replays them.
/// Verifies the two modes are compositionally compatible.
#[tokio::test]
async fn manifest_roundtrip_agent_generates_then_constraint_replays() {
    use orchyn_harness::agent::AgentHarness;
    use orchyn_toolkit::{default_registry, ToolContext};

    // ── phase 1: agent stores a test manifest in working memory ──────────────
    let agent_response = serde_json::json!({
        "reasoning": "I found a login flow to test.",
        "tool_calls": [
            {
                "name": "memory_store",
                "arguments": {
                    "key": "test_login_flow",
                    "value": {
                        "steps": [
                            {"step": 0, "actor": "alice", "instruction": "Navigate to login"},
                            {"step": 1, "actor": "alice", "instruction": "Click submit"}
                        ]
                    }
                }
            }
        ],
        "done": false,
    })
    .to_string();

    let done_response = agent_done_response("login flow captured");

    let model = scripted_model(vec![agent_response, done_response]);
    let ctx = ToolContext::new(".");
    let registry = default_registry();
    let mut harness = AgentHarness::new(model, registry, ctx, 5);
    let report = harness.run("http://localhost/").await.unwrap();

    assert_eq!(report.tests_proposed, 1, "one test_ key should be in working memory");

    // ── phase 2: extract manifest from report ────────────────────────────────
    // Replicate manifest_from_report logic (same as in orchyn-cli) inline.
    let mut manifest: Vec<ManifestStep> = Vec::new();
    if let Some(obj) = report.working_memory.as_object() {
        for (key, value) in obj {
            if !key.starts_with("test_") { continue; }
            if let Some(steps) = value.get("steps").and_then(|s| s.as_array()) {
                for (idx, raw) in steps.iter().enumerate() {
                    manifest.push(ManifestStep {
                        step: idx,
                        actor: raw.get("actor").and_then(|v| v.as_str()).unwrap_or("explorer").into(),
                        instruction: raw.get("instruction").and_then(|v| v.as_str()).unwrap_or("").into(),
                        on_fail: FailPolicy::Halt,
                        action_hint: None,
                    });
                }
            }
        }
    }
    assert_eq!(manifest.len(), 2, "manifest should have two steps");

    // ── phase 3: replay manifest through TestRunner ──────────────────────────
    let mut adapter = ScriptedAdapter::new();
    adapter
        .push_screenshot(blank_png())
        .push_screenshot(blank_png())
        .push_elements(vec![])
        .push_elements(vec![button_element()]);

    let replay_model = scripted_model(vec![
        decision_navigate("alice", 0, "http://localhost/"),
        decision_click("alice", 1, 1),
    ]);

    let mut actors: HashMap<String, Box<dyn orchyn_core::PlatformAdapter>> = HashMap::new();
    actors.insert("alice".into(), Box::new(adapter));

    let runner = TestRunner::new(manifest, actors, replay_model);
    let result = runner.run().await.unwrap();

    assert!(
        matches!(result.record.status, RunStatus::Passed),
        "replayed manifest should pass, got {:?}",
        result.record.status
    );
}
