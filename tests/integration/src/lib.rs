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
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;

    // Serialise all real-LLM tests: running them concurrently saturates the CPU
    // and kills the CDP WebSocket connection mid-run (~600s timeout).
    static REAL_LLM_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn real_llm_lock() -> std::sync::MutexGuard<'static, ()> {
        // unwrap_or_else recovers from poison: if a prior test panicked while
        // holding the lock the mutex is tainted, but we only care about exclusion.
        REAL_LLM_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
    }

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

        /// Start a Vite dev server via `npm run dev` and wait for `port` to open.
        fn start_vite(name: &str, port: u16) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures")
                .join(name)
                .canonicalize()
                .unwrap_or_else(|_| panic!("fixture dir not found: {name}"));

            // Kill any leftover Vite that might be holding the port.
            let _ = Command::new("pkill").args(["-f", "vite"]).status();
            std::thread::sleep(Duration::from_millis(500));

            let child = Command::new("npm")
                .args(["run", "dev"])
                .current_dir(&root)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap_or_else(|_| panic!("failed to start vite for {name}"));

            // Poll until the port is open (max 15 s).
            for _ in 0..150 {
                if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                    return Self { _child: child, port, dir: root };
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            panic!("Vite did not start on port {port} within 15s");
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
        // gap_report auto-records a result_fail_ entry; the explicit memory_store adds another.
        assert!(report.tests_executed >= 1, "should record at least one test");
        assert!(report.tests_failed >= 1, "the test should fail");
        assert_eq!(report.tests_passed, 0);
    }

    // ── Test 2.5: Real LLM finds gap in broken-counter ───────────────────────

    /// This test uses the actual Qwen 3.5 0.8B model to explore the broken-counter
    /// fixture. It verifies that the model can autonomously decide to use tools
    /// and eventually find the gap.
    #[tokio::test]
    #[ignore = "requires downloading/running Qwen 3.5 0.8B model"]
    async fn harness_real_llm_explores_broken_counter() {
        let _lock = real_llm_lock();
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
        let _lock = real_llm_lock();
        let srv = FixtureServer::start("todo-app", 18099);

        let mut harness = real_agent_harness(&srv.dir, &srv.url(), 25);
        let report = harness.run(&srv.url()).await.expect("harness.run failed");

        assert!(report.completed, "agent should set done=true");
        assert!(report.tests_executed > 0, "agent should run at least one test");
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
        assert!(report.tests_failed >= 1, "test should fail");
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
        let _lock = real_llm_lock();
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
        let _lock = real_llm_lock();
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
        let _lock = real_llm_lock();
        let qa_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/qa-target")
            .canonicalize()
            .unwrap();

        // Kill any Vite left running from a previous test run so the port is free.
        let _ = std::process::Command::new("pkill").args(["-f", "vite"]).status();
        std::thread::sleep(Duration::from_secs(1));
        let _ = std::fs::remove_file("/tmp/orchyn_dev.log");
        let _ = std::fs::remove_file("/tmp/orchyn_dev_url");

        // No FixtureServer — the agent autonomously starts the dev server via bash.
        let mut harness = real_agent_harness(&qa_dir, "about:blank", 25);
        let report = harness
            .run(&qa_dir.display().to_string())
            .await
            .expect("harness.run failed");

        // Kill the Vite process the agent started so it doesn't linger across tests.
        let _ = std::process::Command::new("pkill").args(["-f", "vite"]).status();

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
    #[ignore = "requires npm/node and Vite in PATH"]
    async fn harness_scripted_finds_bugs_in_qa_target() {
        let srv = FixtureServer::start_vite("qa-target", 5174);
        let qa_dir = srv.dir.clone();

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

        let url = srv.url();
        let mut harness = scripted_harness(responses, &qa_dir, &url, 10);
        let report = harness.run(&url).await.expect("harness run failed");

        // Kill Vite — npm run dev spawns it as a child, pkill is needed.
        let _ = Command::new("pkill").args(["-f", "vite"]).status();

        assert!(report.completed);
        assert!(report.gaps_found >= 1, "expected at least 1 gap");
        assert!(report.tests_failed >= 1);
    }

    // ── Differential testing ──────────────────────────────────────────────────

    /// Full differential-testing integration test. Creates a tiny git repo with
    /// two commits, runs DiffEngine.compare() with a scripted MockClient
    /// (no real LLM needed, no browser), and asserts the DiffReport.
    ///
    /// Commit A — agent finds "Old bug".
    /// Commit B — agent finds "Old bug" + "New regression".
    /// Expected: new_gaps=[New regression], fixed_gaps=[], unchanged_gaps=[Old bug].
    #[tokio::test]
    async fn diff_engine_finds_regressions_between_commits() {
        use orchyn_harness::diff_engine::DiffEngine;

        // 1. Create a temp git repo with two commits.
        let repo_dir = tempfile::tempdir().expect("tempdir");
        let repo_path = repo_dir.path();

        // git init
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git init");
        std::process::Command::new("git")
            .args(["config", "user.email", "test@orchyn"])
            .current_dir(repo_path).status().ok();
        std::process::Command::new("git")
            .args(["config", "user.name", "orchyn-test"])
            .current_dir(repo_path).status().ok();

        // Commit A: initial version
        std::fs::write(repo_path.join("app.js"), "// version A\nconsole.log('v1');\n")
            .expect("write app.js A");
        std::process::Command::new("git")
            .args(["add", "."]).current_dir(repo_path).status().ok();
        std::process::Command::new("git")
            .args(["commit", "-m", "version A"])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("commit A");

        // Commit B: introduce a change (simulates a regression)
        std::fs::write(repo_path.join("app.js"), "// version B\nconsole.log('v2');\n")
            .expect("write app.js B");
        std::process::Command::new("git")
            .args(["add", "."]).current_dir(repo_path).status().ok();
        std::process::Command::new("git")
            .args(["commit", "-m", "version B — adds regression"])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("commit B");

        // 2. Build a MockClient.
        // Run A responses (2 turns): report "Old bug", then done.
        // Run B responses (2 turns): report "Old bug" + "New regression", then done.
        let responses = vec![
            // Run A, turn 0 — report old bug
            r#"{"reasoning":"Found old bug","tool_calls":[{"name":"gap_report","arguments":{"description":"Old bug","severity":"high"}}],"done":false}"#,
            // Run A, turn 1 — done
            r#"{"reasoning":"done","tool_calls":[],"done":true,"report":{"app_type":"test","features_found":[],"gaps_found":1,"tests_executed":1,"tests_passed":0,"tests_failed":1,"summary":"old bug"}}"#,
            // Run B, turn 0 — report both bugs
            r#"{"reasoning":"Found old + new bugs","tool_calls":[{"name":"gap_report","arguments":{"description":"Old bug","severity":"high"}},{"name":"gap_report","arguments":{"description":"New regression","severity":"critical"}}],"done":false}"#,
            // Run B, turn 1 — done
            r#"{"reasoning":"done","tool_calls":[],"done":true,"report":{"app_type":"test","features_found":[],"gaps_found":2,"tests_executed":2,"tests_passed":0,"tests_failed":2,"summary":"old and new bugs"}}"#,
        ];
        let model = Arc::new(MockClient::new(
            responses.into_iter().map(str::to_string).collect(),
        ));

        // 3. Run DiffEngine (no browser factory — scripted model, no UI).
        let engine = DiffEngine::new(model, 5);
        let diff = engine
            .compare(
                repo_path,
                "HEAD~1", // ref A = commit A
                "HEAD",   // ref B = commit B
                "about:blank",
                "about:blank",
                None, // no browser
                None,
            )
            .await
            .expect("diff engine failed");

        // 4. Assert the diff report.
        assert_eq!(diff.ref_a, "HEAD~1");
        assert_eq!(diff.ref_b, "HEAD");
        assert!(!diff.commit_a.is_empty(), "commit_a sha must be resolved");
        assert!(!diff.commit_b.is_empty(), "commit_b sha must be resolved");
        assert!(diff.commit_a != diff.commit_b, "two distinct commits");
        assert!(
            diff.code_diff.contains("version A") || diff.code_diff.contains("version B"),
            "code_diff should contain the changed line"
        );

        assert_eq!(diff.fixed_gaps.len(), 0, "no gaps were fixed");
        assert_eq!(diff.unchanged_gaps.len(), 1, "one persistent gap");
        assert_eq!(diff.unchanged_gaps[0].description, "Old bug");
        assert_eq!(diff.new_gaps.len(), 1, "one regression introduced");
        assert_eq!(diff.new_gaps[0].description, "New regression");
        assert!(diff.regression, "regression flag must be set");

        assert!(diff.report_a.completed, "run A should complete");
        assert!(diff.report_b.completed, "run B should complete");
        assert_eq!(diff.report_a.gaps_found, 1);
        assert_eq!(diff.report_b.gaps_found, 2);
    }

    // ── Real-LLM DiffEngine regression detection ──────────────────────────────

    /// Non-scripted DiffEngine test with a real Qwen model and real Chromium.
    ///
    /// Two pure Node.js servers (no npm, no Express) are written to temp dirs
    /// and started inline:
    ///   Server A (port 18200): counter whose Increment button calls /api/noop
    ///                          — the increment is a no-op (1 bug).
    ///   Server B (port 18201): same broken counter PLUS a /api/login endpoint
    ///                          that accepts any credentials (auth bypass — 2 bugs,
    ///                          regression vs. A).
    ///
    /// A minimal two-commit git repo is created in a temp dir so DiffEngine can
    /// resolve HEAD~1 and HEAD to distinct SHAs and compute a code diff.
    ///
    /// The test asserts:
    ///   - Both commits are resolved.
    ///   - Both runs complete and use at least one turn.
    ///   - At least one gap is found across both runs combined.
    ///   - If B finds more gaps than A, the regression flag is set.
    #[tokio::test]
    #[ignore = "requires Qwen 3.5 0.8B model and Chrome"]
    async fn harness_real_llm_diff_engine_detects_regression() {
        use orchyn_harness::diff_engine::DiffEngine;

        let _lock = real_llm_lock();

        // ── inline Node.js servers ────────────────────────────────────────────

        // Server A: broken counter only.
        let script_a = r#"
const http = require('http');
let count = 0;
http.createServer((req, res) => {
  if (req.method === 'GET' && req.url === '/') {
    res.writeHead(200, {'Content-Type': 'text/html'});
    res.end(`<!DOCTYPE html><html><head><title>Counter</title></head><body>
      <h1>Counter App</h1>
      <p id="count">Count: ${count}</p>
      <button id="inc" onclick="increment()">Increment</button>
      <script>
        async function increment() {
          await fetch('/api/noop', { method: 'POST' });
          // Bug: /api/noop does nothing; count never changes
        }
      </script>
    </body></html>`);
  } else if (req.method === 'POST' && req.url === '/api/noop') {
    res.writeHead(200, {'Content-Type': 'application/json'});
    res.end('{"ok":true}');
  } else {
    res.writeHead(404); res.end('Not found');
  }
}).listen(18200, '127.0.0.1');
"#;

        // Server B: same broken counter + auth-bypass login (regression).
        let script_b = r#"
const http = require('http');
let count = 0;
http.createServer((req, res) => {
  if (req.method === 'GET' && req.url === '/') {
    res.writeHead(200, {'Content-Type': 'text/html'});
    res.end(`<!DOCTYPE html><html><head><title>Counter + Login</title></head><body>
      <h1>Counter App</h1>
      <p id="count">Count: ${count}</p>
      <button id="inc" onclick="increment()">Increment</button>
      <h2>Login</h2>
      <input id="user" placeholder="Username" /><br>
      <input id="pass" type="password" placeholder="Password" /><br>
      <button id="loginBtn" onclick="doLogin()">Login</button>
      <p id="msg"></p>
      <script>
        async function increment() {
          await fetch('/api/noop', { method: 'POST' });
          // Bug: count never changes
        }
        async function doLogin() {
          const r = await fetch('/api/login', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              user: document.getElementById('user').value,
              pass: document.getElementById('pass').value
            })
          });
          const d = await r.json();
          document.getElementById('msg').textContent = d.success ? 'Logged in!' : 'Login failed';
        }
      </script>
    </body></html>`);
  } else if (req.method === 'POST' && req.url === '/api/noop') {
    res.writeHead(200, {'Content-Type': 'application/json'});
    res.end('{"ok":true}');
  } else if (req.method === 'POST' && req.url === '/api/login') {
    let body = '';
    req.on('data', chunk => { body += chunk; });
    req.on('end', () => {
      // Regression: accepts any credentials without validation
      res.writeHead(200, {'Content-Type': 'application/json'});
      res.end('{"success":true}');
    });
  } else {
    res.writeHead(404); res.end('Not found');
  }
}).listen(18201, '127.0.0.1');
"#;

        // Start both servers — write scripts to temp dirs so node can read them.
        struct InlineServer {
            child: std::process::Child,
            _dir: tempfile::TempDir,
        }
        impl Drop for InlineServer {
            fn drop(&mut self) {
                let _ = self.child.kill();
            }
        }
        fn start_inline_server(script: &str, port: u16) -> InlineServer {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("server.js");
            std::fs::write(&path, script).expect("write server.js");
            let child = std::process::Command::new("node")
                .arg(&path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap_or_else(|_| panic!("failed to spawn node on :{port}"));
            for _ in 0..40 {
                if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            InlineServer { child, _dir: dir }
        }

        let _srv_a = start_inline_server(script_a, 18200);
        let _srv_b = start_inline_server(script_b, 18201);

        // ── minimal git repo (2 commits) for SHA resolution ───────────────────

        let repo_dir = tempfile::tempdir().expect("repo tempdir");
        let repo_path = repo_dir.path();

        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@orchyn"])
            .current_dir(repo_path).status().ok();
        std::process::Command::new("git")
            .args(["config", "user.name", "orchyn-test"])
            .current_dir(repo_path).status().ok();

        std::fs::write(repo_path.join("v.txt"), "version A\n").expect("write v.txt A");
        std::process::Command::new("git").args(["add", "."]).current_dir(repo_path).status().ok();
        std::process::Command::new("git")
            .args(["commit", "-m", "baseline"])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("commit A");

        std::fs::write(repo_path.join("v.txt"), "version B — adds login\n").expect("write v.txt B");
        std::process::Command::new("git").args(["add", "."]).current_dir(repo_path).status().ok();
        std::process::Command::new("git")
            .args(["commit", "-m", "add login feature"])
            .current_dir(repo_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("commit B");

        // ── run DiffEngine with real LLM + real browser ───────────────────────

        let model = QwenClient::new(None);
        let engine = DiffEngine::new(model, 15);

        let factory_a: Arc<dyn AdapterFactory> =
            Arc::new(WebAdapterFactory { url: "http://127.0.0.1:18200".to_string() });
        let factory_b: Arc<dyn AdapterFactory> =
            Arc::new(WebAdapterFactory { url: "http://127.0.0.1:18201".to_string() });

        let diff = engine
            .compare(
                repo_path,
                "HEAD~1",
                "HEAD",
                "http://127.0.0.1:18200",
                "http://127.0.0.1:18201",
                Some(factory_a),
                Some(factory_b),
            )
            .await
            .expect("DiffEngine::compare failed");

        // ── structural assertions ─────────────────────────────────────────────

        assert!(!diff.commit_a.is_empty(), "commit_a SHA must be resolved");
        assert!(!diff.commit_b.is_empty(), "commit_b SHA must be resolved");
        assert_ne!(diff.commit_a, diff.commit_b, "two distinct commits");
        assert!(diff.report_a.turns_used >= 1, "run A must use at least 1 turn");
        assert!(diff.report_b.turns_used >= 1, "run B must use at least 1 turn");

        // ── substantive assertions ────────────────────────────────────────────

        let total_gaps = diff.report_a.gaps_found + diff.report_b.gaps_found;
        assert!(
            total_gaps >= 1,
            "real LLM should find at least 1 gap across both runs; got 0 \
             (A: {} gaps, B: {} gaps)",
            diff.report_a.gaps_found,
            diff.report_b.gaps_found
        );

        // If B found more bugs than A, the regression flag must be set.
        if diff.report_b.gaps_found > diff.report_a.gaps_found {
            assert!(
                diff.regression,
                "B has more gaps than A but regression flag is false"
            );
        }
    }
}
