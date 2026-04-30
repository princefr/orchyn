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
///   bugs. Requires `npm run dev` on port 5174 — marked `#[ignore]`.
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
    use orchyn_harness::agent::AgentHarness;
    use orchyn_model_client::MockClient;
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
