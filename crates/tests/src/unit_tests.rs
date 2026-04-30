/// Supplemental unit tests for types/modules that either have no inline tests
/// or whose inline coverage is thin. Tests here import from crate roots so
/// they compile alongside the real code without modifying source files.
///
/// Constraint Mode modules (telemetry, validation, recovery, scheduler) have
/// been removed — those modules no longer exist in the codebase.

// ── core::action ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod action_tests {
    use orchyn_core::action::{Action, AnnotatedElement, AssertionType, BoundingBox, LlmDecision};

    #[test]
    fn action_navigate_round_trips_serde() {
        let a = Action::Navigate { value: "https://example.com".into() };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("NAVIGATE"), "tag serialised correctly");
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn action_click_round_trips_serde() {
        let a = Action::Click { target: 3 };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("CLICK"));
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn action_type_round_trips_serde() {
        let a = Action::Type { target: 2, value: "hello".into() };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("TYPE"));
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn action_assert_success_round_trips_serde() {
        let a = Action::AssertSuccess {
            assertion_type: AssertionType::TextContains,
            expected: "Welcome".into(),
        };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("ASSERT_SUCCESS"));
        assert!(json.contains("TEXT_CONTAINS"));
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn llm_decision_deserialises_flatten_click() {
        let raw = r#"{
            "actor_id": "alice",
            "manifest_step": 2,
            "thought": "click submit",
            "action": "CLICK",
            "target": 7
        }"#;
        let d: LlmDecision = serde_json::from_str(raw).unwrap();
        assert_eq!(d.actor_id, "alice");
        assert_eq!(d.manifest_step, 2);
        assert!(matches!(d.action, Action::Click { target: 7 }));
    }

    #[test]
    fn llm_decision_deserialises_navigate() {
        let raw = r#"{"actor_id":"bob","manifest_step":0,"thought":"","action":"NAVIGATE","value":"http://localhost/"}"#;
        let d: LlmDecision = serde_json::from_str(raw).unwrap();
        assert!(matches!(d.action, Action::Navigate { ref value } if value == "http://localhost/"));
    }

    #[test]
    fn annotated_element_id_is_u8() {
        let el = AnnotatedElement {
            id: 42,
            element_type: "button".into(),
            text: Some("Submit".into()),
            bounds: BoundingBox { x: 0, y: 0, width: 80, height: 32 },
        };
        assert_eq!(el.id, 42u8);
    }
}

// ── core::types ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod types_tests {
    use orchyn_core::{FailPolicy, ManifestStep, RawTelemetry};

    #[test]
    fn manifest_step_on_fail_defaults_to_halt() {
        let json = r#"{"step":0,"actor":"alice","instruction":"do it"}"#;
        let step: ManifestStep = serde_json::from_str(json).unwrap();
        assert_eq!(step.on_fail, FailPolicy::Halt);
    }

    #[test]
    fn manifest_step_action_hint_defaults_to_none() {
        let json = r#"{"step":0,"actor":"alice","instruction":"do it"}"#;
        let step: ManifestStep = serde_json::from_str(json).unwrap();
        assert!(step.action_hint.is_none());
    }

    #[test]
    fn raw_telemetry_default_is_empty() {
        let t = RawTelemetry::default();
        assert!(t.console_logs.is_empty());
        assert!(t.network_requests.is_empty());
        assert!(t.ws_messages.is_empty());
        assert!(t.current_url.is_empty());
    }
}

// ── pixel_observer::annotator ────────────────────────────────────────────────

#[cfg(test)]
mod annotator_tests {
    use image::{Rgba, RgbaImage};
    use orchyn_core::action::BoundingBox;
    use orchyn_core::UiElement;
    use orchyn_pixel_observer::annotate_screenshot;

    fn blank_png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    fn el(x: u32, y: u32, w: u32, h: u32, text: Option<&str>) -> UiElement {
        UiElement {
            element_type: "button".into(),
            text: text.map(str::to_string),
            bounds: BoundingBox { x, y, width: w, height: h },
        }
    }

    #[test]
    fn empty_element_list_returns_unchanged_png() {
        let png = blank_png(50, 50);
        let (out, map) = annotate_screenshot(&png, &[]).unwrap();
        assert!(map.is_empty());
        image::load_from_memory(&out).unwrap();
    }

    #[test]
    fn output_is_valid_png_and_differs_from_input() {
        let png = blank_png(200, 200);
        let elements = vec![el(10, 10, 80, 40, Some("Login"))];
        let (out, _) = annotate_screenshot(&png, &elements).unwrap();
        assert_ne!(out, png, "annotated PNG should differ from input");
        image::load_from_memory(&out).unwrap();
    }

    #[test]
    fn ids_are_1_indexed_and_sequential() {
        let png = blank_png(300, 300);
        let elements = vec![
            el(0, 0, 50, 20, None),
            el(60, 0, 50, 20, None),
            el(120, 0, 50, 20, None),
        ];
        let (_, map) = annotate_screenshot(&png, &elements).unwrap();
        let ids: Vec<u8> = map.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn element_text_and_type_are_forwarded() {
        let png = blank_png(200, 200);
        let elements = vec![el(10, 10, 80, 30, Some("Submit"))];
        let (_, map) = annotate_screenshot(&png, &elements).unwrap();
        assert_eq!(map[0].text.as_deref(), Some("Submit"));
        assert_eq!(map[0].element_type, "button");
    }
}

// ── toolkit::registry ────────────────────────────────────────────────────────

#[cfg(test)]
mod registry_tests {
    use futures::future::BoxFuture;
    use futures::FutureExt;
    use orchyn_core::OrchynError;
    use orchyn_toolkit::registry::Tool;
    use orchyn_toolkit::{ToolCall, ToolContext, ToolRegistry, ToolResult};

    struct EchoTool;
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "echoes args" }
        fn schema(&self) -> serde_json::Value { serde_json::json!({"type":"object"}) }
        fn execute<'a>(&'a self, args: serde_json::Value, _ctx: &'a ToolContext) -> BoxFuture<'a, orchyn_core::Result<ToolResult>> {
            async move { Ok(ToolResult::ok(args)) }.boxed()
        }
    }

    struct FailTool;
    impl Tool for FailTool {
        fn name(&self) -> &str { "fail" }
        fn description(&self) -> &str { "always errors" }
        fn schema(&self) -> serde_json::Value { serde_json::json!({"type":"object"}) }
        fn execute<'a>(&'a self, _args: serde_json::Value, _ctx: &'a ToolContext) -> BoxFuture<'a, orchyn_core::Result<ToolResult>> {
            async move { Err(OrchynError::Internal("tool error".into())) }.boxed()
        }
    }

    fn ctx() -> ToolContext { ToolContext::new(".") }

    #[test]
    fn register_and_schema_round_trip() {
        let mut r = ToolRegistry::new();
        r.register(Box::new(EchoTool));
        let schemas = r.schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "echo");
    }

    #[tokio::test]
    async fn execute_known_tool_returns_ok() {
        let mut r = ToolRegistry::new();
        r.register(Box::new(EchoTool));
        let call = ToolCall { name: "echo".into(), arguments: serde_json::json!({"msg":"hi"}) };
        let result = r.execute(&call, &ctx()).await.unwrap();
        assert!(result.success);
        assert_eq!(result.data["msg"], "hi");
    }

    #[tokio::test]
    async fn execute_unknown_tool_returns_validation_error() {
        let r = ToolRegistry::new();
        let call = ToolCall { name: "ghost".into(), arguments: serde_json::Value::Null };
        let err = r.execute(&call, &ctx()).await.unwrap_err();
        assert!(matches!(err, OrchynError::Validation(_)));
        assert!(err.to_string().contains("unknown tool: ghost"));
    }

    #[tokio::test]
    async fn execute_failing_tool_propagates_error() {
        let mut r = ToolRegistry::new();
        r.register(Box::new(FailTool));
        let call = ToolCall { name: "fail".into(), arguments: serde_json::Value::Null };
        let err = r.execute(&call, &ctx()).await.unwrap_err();
        assert!(matches!(err, OrchynError::Internal(_)));
    }
}

// ── agent working memory ──────────────────────────────────────────────────────

#[cfg(test)]
mod working_memory_tests {
    use orchyn_harness::agent::memory::{Message, TurnRecord, WorkingMemory};

    #[test]
    fn sliding_window_evicts_oldest_messages() {
        let mut mem = WorkingMemory::new(3);
        mem.push_message(Message::user("first"));
        mem.push_message(Message::user("second"));
        mem.push_message(Message::user("third"));
        mem.push_message(Message::user("fourth"));
        let msgs = mem.messages();
        assert_eq!(msgs.len(), 3);
        assert!(msgs[0].content.contains("second"));
        assert!(msgs[2].content.contains("fourth"));
    }

    #[test]
    fn messages_have_correct_roles() {
        let mut mem = WorkingMemory::new(10);
        mem.push_message(Message::system("sys"));
        mem.push_message(Message::user("usr"));
        mem.push_message(Message::assistant("asst"));
        let msgs = mem.messages();
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[2].role, "assistant");
    }

    #[test]
    fn turn_count_increments_per_record() {
        let mut mem = WorkingMemory::new(10);
        assert_eq!(mem.turn_count(), 0);
        mem.record_turn(TurnRecord { turn: 0, reasoning: "first".into(), tool_results: vec![] });
        mem.record_turn(TurnRecord { turn: 1, reasoning: "second".into(), tool_results: vec![] });
        assert_eq!(mem.turn_count(), 2);
    }
}
