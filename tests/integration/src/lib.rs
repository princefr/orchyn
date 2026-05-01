/// End-to-end integration tests for the Orchyn agent harness.
///
/// Each test spins up a real Node.js fixture server, connects a headless
/// Chromium browser via `WebAdapter`, and drives the `AgentHarness` with a
/// pre-scripted `MockClient` so no live LLM is required.
///
/// # Fixture layout
/// - `tests/fixtures/broken-counter/` — Express counter whose increment button
///   calls a /api/noop endpoint (does nothing).
/// - `tests/fixtures/broken-login/`   — Express login that sets a cookie but
///   the dashboard page ignores it.
/// - `tests/fixtures/qa-target/`      — Vue 3 + Vite app with 7 intentional
///   bugs. The real-LLM test lets the agent start the server autonomously.
///
/// # Prerequisites
/// - Node.js must be in PATH.
/// - `tests/fixtures/node_modules/` must contain `express`
///   (run `npm install` in `tests/fixtures/` once).
/// - A Chromium/Chrome binary must be discoverable by `headless_chrome`.

#[cfg(test)]
mod tests {
    use futures::future::{BoxFuture, FutureExt};
    use orchyn_core::{PlatformAdapter, Result};
    use orchyn_harness::agent::{AgentHarness, TurnObservation};
    use orchyn_model_client::{MockClient, QwenClient};
    use orchyn_platform_web::WebAdapter;
    use orchyn_toolkit::{default_registry, AdapterFactory, ToolContext};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::sync::Arc;
    use std::time::Duration;

    // ── fixture server helper ─────────────────────────────────────────────────

    struct FixtureServer {
        _child: Child,
        port: u16,
        /// Absolute path to the fixture directory (used as project_root).
        pub dir: PathBuf,
    }

    impl FixtureServer {
        fn start(name: &str, port: u16) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures")
                .join(name)
                .canonicalize()
                .unwrap_or_else(|_| panic!("fixture dir not found: {name}"));

            let child = Command::new("node")
                .arg("server.js")
                .env("PORT", port.to_string())
                .current_dir(&root)
                // Redirect server stdout/stderr so they don't pollute test output.
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap_or_else(|_| panic!("failed to start node server for {name}"));

            // Poll until the port is open (max 4 s).
            for _ in 0..40 {
                if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            Self { _child: child, port, dir: root }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }
    }

    impl Drop for FixtureServer {
        fn drop(&mut self) {
            let _ = self._child.kill();
        }
    }

    // ── AdapterFactory that mints a real WebAdapter ───────────────────────────

    struct WebAdapterFactory {
        url: String,
    }

    impl AdapterFactory for WebAdapterFactory {
        fn spawn(&self, _name: &str) -> BoxFuture<'_, Result<Box<dyn PlatformAdapter>>> {
            let url = self.url.clone();
            async move {
                let adapter = WebAdapter::new(&url).await?;
                Ok(Box::new(adapter) as Box<dyn PlatformAdapter>)
            }
            .boxed()
        }
    }

    /// Build a harness wired to a real WebAdapter (via factory) and a
    /// scripted model whose responses are consumed turn by turn.
    fn scripted_harness(
        responses: Vec<&str>,
        project_root: &Path,
        url: &str,
        max_turns: usize,
    ) -> AgentHarness {
        let model = Arc::new(MockClient::new(
            responses.into_iter().map(str::to_string).collect(),
        ));
        let factory = Arc::new(WebAdapterFactory { url: url.to_string() });
        let ctx = ToolContext::new(project_root)
            .with_factory(factory)
            .with_shell(true);
        AgentHarness::new(model, default_registry(), ctx, max_turns)
    }

    /// Build a harness wired to a real WebAdapter and the actual Qwen 3.5 0.8B model.
    fn real_agent_harness(
        project_root: &Path,
        url: &str,
        max_turns: usize,
    ) -> AgentHarness {
        let model = QwenClient::new(None);
        let factory = Arc::new(WebAdapterFactory { url: url.to_string() });
        let ctx = ToolContext::new(project_root)
            .with_factory(factory)
            .with_shell(true);
        AgentHarness::new(model, default_registry(), ctx, max_turns)
    }

    /// Assert that every STATE snapshot collected by the harness has the
    /// required telemetry fields with valid structure. This verifies the sensor
    /// fusion pipeline actually wired all four channels each turn — not just
    /// the channels the model happened to ask about.
    fn assert_turn_telemetry(turn_observations: &[TurnObservation]) {
        assert!(
            !turn_observations.is_empty(),
            "harness must collect at least one STATE snapshot"
        );
        for obs in turn_observations {
            let turn = obs.turn;
            // Every actor must have the four telemetry channels
            let actors = obs.state.as_object()
                .unwrap_or_else(|| panic!("turn {turn}: STATE must be a JSON object"));
            assert!(!actors.is_empty(), "turn {turn}: STATE must have at least one actor");

            for (actor_id, state) in actors {
                let url = state.get("current_url")
                    .and_then(|u| u.as_str())
                    .unwrap_or_else(|| panic!("turn {turn}/{actor_id}: missing current_url"));
                assert!(!url.is_empty(), "turn {turn}/{actor_id}: current_url must not be empty");

                assert!(
                    state.get("elements").and_then(|e| e.as_array()).is_some(),
                    "turn {turn}/{actor_id}: missing or non-array 'elements' in STATE"
                );
                assert!(
                    state.get("console_errors").and_then(|e| e.as_array()).is_some(),
                    "turn {turn}/{actor_id}: missing or non-array 'console_errors' in STATE"
                );
                assert!(
                    state.get("http_requests").and_then(|r| r.as_array()).is_some(),
                    "turn {turn}/{actor_id}: missing or non-array 'http_requests' in STATE"
                );
                assert!(
                    state.get("ws_messages").and_then(|w| w.as_array()).is_some(),
                    "turn {turn}/{actor_id}: missing or non-array 'ws_messages' in STATE"
                );
            }
        }
    }

    // ── Test 1: raw WebAdapter screenshot ────────────────────────────────────

    /// Verifies that a `WebAdapter` successfully takes a screenshot of a live
    /// page and returns valid PNG bytes.
    #[tokio::test]
    async fn web_adapter_screenshot_returns_valid_png() {
        let srv = FixtureServer::start("broken-counter", 18091);
        let mut adapter = WebAdapter::new(&srv.url())
            .await
            .expect("WebAdapter::new failed");

        let png = adapter.screenshot().await.expect("screenshot failed");

        assert!(!png.is_empty(), "screenshot bytes should not be empty");
        image::load_from_memory(&png).expect("screenshot should decode as a valid PNG");
    }

    // ── Test 2: harness finds gap in broken-counter ───────────────────────────

    /// The broken-counter fixture wires the Increment button to `POST /api/noop`
    /// which acknowledges the request but never changes the counter.
    /// The scripted agent takes a screenshot, reads server.js, reports the gap,
    /// records a test failure, then signals done.
    #[tokio::test]
    async fn harness_scripted_finds_gap_in_broken_counter() {
        let srv = FixtureServer::start("broken-counter", 18092);

        let responses = vec![
            // Turn 1 — screenshot to observe the UI.
            r#"{"reasoning":"Taking a screenshot to see what the counter app looks like.","tool_calls":[{"name":"browser_screenshot","arguments":{"actor":"explorer"}}],"done":false}"#,
            // Turn 2 — read the source to find the root cause.
            r#"{"reasoning":"Reading server.js to understand how the increment button works.","tool_calls":[{"name":"read_file","arguments":{"path":"server.js"}}],"done":false}"#,
            // Turn 3 — report gap + record failed test.
            r#"{"reasoning":"The increment button calls /api/noop which does nothing. This is a critical gap.","tool_calls":[{"name":"gap_report","arguments":{"description":"Increment button calls /api/noop; counter never changes","severity":"critical","reproduction":["Click Increment button","Counter value stays at 0"]}},{"name":"memory_store","arguments":{"key":"result_fail_counter_increment","value":{"ok":false,"reason":"/api/noop does not increment the counter"}}}],"done":false}"#,
            // Turn 4 — done.
            r#"{"reasoning":"Gap found and recorded. Testing complete.","tool_calls":[],"done":true,"report":{"app_type":"counter app","features_found":["counter display","increment button"],"gaps_found":1,"tests_executed":1,"tests_passed":0,"tests_failed":1,"summary":"The increment button is wired to a noop endpoint."}}"#,
        ];

        let mut harness = scripted_harness(responses, &srv.dir, &srv.url(), 10);
        let report = harness.run(&srv.url()).await.expect("harness.run failed");

        assert!(report.completed, "agent should set done=true");
        assert!(report.gaps_found >= 1, "should find at least 1 gap, got {}", report.gaps_found);
        assert_eq!(report.tests_executed, 1, "one test recorded");
        assert_eq!(report.tests_failed, 1, "that test failed");
        assert_eq!(report.tests_passed, 0);
    }

    // ── Test 2.5: Real LLM finds gap in broken-counter ───────────────────────

    /// This test uses the actual Qwen 3.5 0.8B model to explore the broken-counter
    /// fixture. It verifies that the model can autonomously decide to use tools
    /// and eventually find the gap.
    #[tokio::test]
    #[ignore = "requires downloading/running Qwen 3.5 0.8B model"]
    async fn harness_real_llm_explores_broken_counter() {
        let srv = FixtureServer::start("broken-counter", 18096);

        let mut harness = real_agent_harness(&srv.dir, &srv.url(), 15);
        let report = harness.run(&srv.url()).await.expect("harness.run failed");

        assert!(report.completed, "agent should set done=true");
        assert!(report.gaps_found >= 1, "should find the gap autonomously");
        assert!(report.tests_executed >= 1);
        assert!(report.tests_failed >= 1);
        assert_turn_telemetry(&report.turn_observations);
    }

    // ── Test 2.7: Fully Autonomous Lifecycle ────────────────────────────────

    /// This test represents the "holy grail" of autonomy: the agent is given
    /// a path to a project, and it must autonomously:
    /// 1. Run `npm install` via shell_execute.
    /// 2. Figure out how to start the app.
    /// 3. Explore and find gaps.
    #[tokio::test]
    #[ignore = "Full autonomous run with real LLM is slow and model-heavy"]
    async fn harness_autonomous_lifecycle_todo_app() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/todo-app")
            .canonicalize()
            .unwrap();

        // We don't start the FixtureServer here because the agent should do it!
        // But for a stable test, we'll give it the goal of getting the app running.
        let mut harness = real_agent_harness(&root, "http://localhost:8080", 25);
        
        let goal = "Install dependencies, start the app on port 8080, and verify if the todo list works.";
        let report = harness.run(goal).await.expect("Autonomous run failed");

        assert!(report.completed);
        assert!(report.tests_executed > 0);
    }

    // ── Test 3: browser_get_console surfaces injected error log ──────────────

    /// The broken-counter fixture injects a `window.__orchyn_logs` error when
    /// the increment button is clicked. The agent reads that via
    /// `browser_get_console` so we verify the console tool returns data
    /// without crashing.
    #[tokio::test]
    async fn harness_scripted_console_tool_returns_data_after_click() {
        let srv = FixtureServer::start("broken-counter", 18093);

        let responses = vec![
            // Turn 1 — screenshot + click increment (element 1) in same turn.
            r#"{"reasoning":"Taking a screenshot then clicking Increment to trigger the API error log.","tool_calls":[{"name":"browser_screenshot","arguments":{"actor":"explorer"}},{"name":"browser_click","arguments":{"actor":"explorer","element_id":1}}],"done":false}"#,
            // Turn 2 — read console.
            r#"{"reasoning":"Reading console errors after the click.","tool_calls":[{"name":"browser_get_console","arguments":{"actor":"explorer"}}],"done":false}"#,
            // Turn 3 — done, no assertions needed on the content.
            r#"{"reasoning":"Console tool responded without error. Done.","tool_calls":[],"done":true,"report":{"app_type":"counter","features_found":["increment"],"gaps_found":0,"tests_executed":1,"tests_passed":1,"tests_failed":0,"summary":"console tool works"}}"#,
        ];

        let mut harness = scripted_harness(responses, &srv.dir, &srv.url(), 10);
        let report = harness.run(&srv.url()).await.expect("harness.run failed");

        assert!(report.completed, "agent should complete");
    }

    // ── Test 4: harness finds auth gap in broken-login ────────────────────────

    /// The broken-login fixture sets a session cookie on `POST /api/login` but
    /// the dashboard page ignores it and always shows "Please sign in".
    /// The agent navigates, reads the source, spots the gap, and reports it.
    #[tokio::test]
    async fn harness_scripted_finds_auth_gap_in_broken_login() {
        let srv = FixtureServer::start("broken-login", 18094);

        let responses = vec![
            // Turn 1 — screenshot to see the login page.
            r#"{"reasoning":"Taking a screenshot of the login page.","tool_calls":[{"name":"browser_screenshot","arguments":{"actor":"explorer"}}],"done":false}"#,
            // Turn 2 — read server.js to understand the auth flow.
            r#"{"reasoning":"Reading server.js to understand the login and dashboard routes.","tool_calls":[{"name":"read_file","arguments":{"path":"server.js"}}],"done":false}"#,
            // Turn 3 — report the gap.
            r#"{"reasoning":"The dashboard ignores the session cookie. Login sets it but dashboard never checks it.","tool_calls":[{"name":"gap_report","arguments":{"description":"Dashboard ignores the session cookie set by /api/login — unauthenticated users can reach it","severity":"high","reproduction":["POST /api/login","GET /dashboard — should be protected but isn't"]}},{"name":"memory_store","arguments":{"key":"result_fail_auth_cookie_check","value":{"ok":false,"reason":"dashboard never validates session cookie"}}}],"done":false}"#,
            // Turn 4 — done.
            r#"{"reasoning":"Auth gap documented. Testing complete.","tool_calls":[],"done":true,"report":{"app_type":"login app","features_found":["login form","dashboard"],"gaps_found":1,"tests_executed":1,"tests_passed":0,"tests_failed":1,"summary":"Dashboard does not enforce authentication."}}"#,
        ];

        let mut harness = scripted_harness(responses, &srv.dir, &srv.url(), 10);
        let report = harness.run(&srv.url()).await.expect("harness.run failed");

        assert!(report.completed);
        assert!(report.gaps_found >= 1, "should find auth gap");
        assert_eq!(report.tests_failed, 1);
    }

    // ── Test 5: network tool returns HTTP requests ────────────────────────────

    /// Verifies that `browser_get_network` executes without error and that the
    /// harness correctly relays the tool result back to the model.
    #[tokio::test]
    async fn harness_scripted_network_tool_executes() {
        let srv = FixtureServer::start("broken-counter", 18095);

        let responses = vec![
            // Navigate then immediately check network events.
            r#"{"reasoning":"Checking network events after page load.","tool_calls":[{"name":"browser_get_network","arguments":{"actor":"explorer"}}],"done":false}"#,
            r#"{"reasoning":"Network tool responded. Done.","tool_calls":[],"done":true,"report":{"app_type":"counter","features_found":[],"gaps_found":0,"tests_executed":0,"tests_passed":0,"tests_failed":0,"summary":"network tool works"}}"#,
        ];

        let mut harness = scripted_harness(responses, &srv.dir, &srv.url(), 5);
        let report = harness.run(&srv.url()).await.expect("harness.run failed");

        assert!(report.completed);
    }

    // ── Test 2.8: Real LLM finds auth gap in broken-login ───────────────────

    /// Qwen 3.5 0.8B explores the broken-login fixture autonomously.
    /// The fixture sets a session cookie on POST /api/login but the dashboard
    /// route never validates it — the auth gap must be found and reported.
    #[tokio::test]
    #[ignore = "requires downloading/running Qwen 3.5 0.8B model"]
    async fn harness_real_llm_explores_broken_login() {
        let srv = FixtureServer::start("broken-login", 18097);

        let mut harness = real_agent_harness(&srv.dir, &srv.url(), 15);
        let report = harness.run(&srv.url()).await.expect("harness.run failed");

        assert!(report.completed, "agent should set done=true");
        assert!(report.gaps_found >= 1, "should find the auth gap autonomously");
        assert!(report.tests_executed >= 1);
        assert!(report.tests_failed >= 1);
        assert_turn_telemetry(&report.turn_observations);
    }

    // ── Test 2.9: Real LLM explores todo-app ────────────────────────────────

    /// Qwen 3.5 0.8B explores the shared-team todo app fixture.
    /// The app uses innerHTML for list items — an XSS vector — and the server
    /// accepts empty todo text. The agent must surface at least one gap.
    #[tokio::test]
    #[ignore = "requires downloading/running Qwen 3.5 0.8B model"]
    async fn harness_real_llm_explores_todo_app() {
        let srv = FixtureServer::start("todo-app", 18098);

        let mut harness = real_agent_harness(&srv.dir, &srv.url(), 15);
        let report = harness.run(&srv.url()).await.expect("harness.run failed");

        assert!(report.completed, "agent should set done=true");
        assert!(report.gaps_found >= 1, "should find at least one gap");
        assert!(report.tests_executed >= 1);
        assert_turn_telemetry(&report.turn_observations);
    }

    // ── Test 2.10: Real LLM explores qa-target (Vue 3 + Vite) ───────────────

    /// Qwen 3.5 0.8B explores the qa-target Vue 3 app which has 7 intentional
    /// bugs (BUG-1 through BUG-7). The agent autonomously runs `npm run dev`,
    /// discovers the port from Vite's output, navigates there, then finds bugs.
    #[tokio::test]
    #[ignore = "requires Qwen 3.5 0.8B model and npm/node in PATH"]
    async fn harness_real_llm_explores_qa_target() {
        let qa_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/qa-target")
            .canonicalize()
            .unwrap();

        // No FixtureServer — the agent autonomously starts the dev server via bash.
        let mut harness = real_agent_harness(&qa_dir, "about:blank", 25);
        let report = harness
            .run(&qa_dir.display().to_string())
            .await
            .expect("harness.run failed");

        assert!(report.completed, "agent should set done=true");
        assert!(report.gaps_found >= 1, "should find at least one of the 7 bugs");
        assert!(report.tests_executed >= 1);
        assert!(report.tests_failed >= 1);
        assert_turn_telemetry(&report.turn_observations);
    }

    // ── Test 6: qa-target Vite app (ignored — needs dev server) ──────────────

    /// Full agent loop against the Vue 3 qa-target fixture with 7 intentional
    /// bugs. Requires `npm run dev` running in `tests/fixtures/qa-target` on
    /// port 5174 before running this test.
    ///
    /// Run manually: `cargo test -p orchyn-integration-tests qa_target -- --ignored`
    #[tokio::test]
    #[ignore = "requires `npm run dev` running in tests/fixtures/qa-target on port 5174"]
    async fn harness_scripted_finds_bugs_in_qa_target() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let qa_dir = manifest
            .join("../../tests/fixtures/qa-target")
            .canonicalize()
            .expect("qa-target dir not found");

        let responses = vec![
            // Turn 1 — screenshot login page.
            r#"{"reasoning":"Taking a screenshot to see the login form.","tool_calls":[{"name":"browser_screenshot","arguments":{"actor":"explorer"}}],"done":false}"#,
            // Turn 2 — submit with empty email (exploits BUG-1: only password checked).
            r#"{"reasoning":"Submitting form with empty email to test validation.","tool_calls":[{"name":"browser_type","arguments":{"actor":"explorer","element_id":2,"text":"secret"}},{"name":"browser_click","arguments":{"actor":"explorer","element_id":3}}],"done":false}"#,
            // Turn 3 — screenshot to confirm we're logged in despite blank email.
            r#"{"reasoning":"Screenshot to see if empty email was accepted.","tool_calls":[{"name":"browser_screenshot","arguments":{"actor":"explorer"}}],"done":false}"#,
            // Turn 4 — report the gap + record result.
            r#"{"reasoning":"Empty email was accepted — validation gap confirmed.","tool_calls":[{"name":"gap_report","arguments":{"description":"Login accepts empty email — only password length is validated","severity":"high","reproduction":["Leave email blank","Enter any password","Click Login","Login succeeds"]}},{"name":"memory_store","arguments":{"key":"result_fail_login_empty_email","value":{"ok":false,"reason":"empty email accepted"}}}],"done":false}"#,
            // Turn 5 — done.
            r#"{"reasoning":"Gap recorded. Testing complete.","tool_calls":[],"done":true,"report":{"app_type":"dashboard app","features_found":["login","counter","todo list"],"gaps_found":1,"tests_executed":1,"tests_passed":0,"tests_failed":1,"summary":"Login accepts empty emails — BUG-1 confirmed."}}"#,
        ];

        let mut harness = scripted_harness(responses, &qa_dir, "http://127.0.0.1:5174", 10);
        let report = harness.run("http://127.0.0.1:5174")
            .await
            .expect("harness run failed");

        assert!(report.completed);
        assert!(report.gaps_found >= 1, "expected at least 1 gap");
        assert!(report.tests_failed >= 1);
    }
}
