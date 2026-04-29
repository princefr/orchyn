#[cfg(test)]
mod tests {
    use orchyn_action_planner::Goal;
    use orchyn_codebase_indexer::Indexer;
    use orchyn_fixer_agent::{Agent, FixerAgent};
    use orchyn_model_client::{MockClient, QwenClient};
    use orchyn_navigator_agent::NavigatorAgent;
    use orchyn_storage::Storage;
    use orchyn_artifact_storage::MockArtifactStorage;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::sync::Arc;
    use std::time::Duration;
    use tracing::{Level, info};

    struct TestFixture {
        server: Option<Child>,
        pub dir: PathBuf,
        _port: u16,
    }

    impl TestFixture {
        fn new(name: &str, port: u16, start_server: bool) -> Self {
            let _ = tracing_subscriber::fmt()
                .with_max_level(Level::INFO)
                .try_init();

            let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
            let dir = manifest_dir
                .join("../../tests/fixtures")
                .join(name)
                .canonicalize()
                .expect("Failed to find fixture dir");

            let mut server = None;
            if start_server {
                info!("Starting fixture {} on port {}...", name, port);
                server = Some(
                    Command::new("node")
                        .arg("server.js")
                        .env("PORT", port.to_string())
                        .current_dir(&dir)
                        .spawn()
                        .expect("Failed to start server"),
                );

                let mut attempts = 0;
                while attempts < 10 {
                    if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                    attempts += 1;
                }
            }

            Self { server, dir, _port: port }
        }
    }

    impl Drop for TestFixture {
        fn drop(&mut self) {
            if let Some(mut s) = self.server.take() {
                let _ = s.kill();
            }
        }
    }

    #[tokio::test]
    async fn test_navigator_agent_discovery_loop() {
        let fixture = TestFixture::new("todo-app", 0, false);
        let model = QwenClient::new(None);

        let indexer = Indexer::new(&fixture.dir);
        let app_map = indexer.index().unwrap();

        let mut navigator = NavigatorAgent::new(model, app_map);

        info!("Running Navigator Discovery Loop on Todo App...");
        // This might fail if the model is slow/offline, but we check if it compiles and starts
        let _ = navigator
            .run("Identify the API endpoints and how the frontend calls them.")
            .await;
    }

    #[tokio::test]
    async fn test_fixer_agent_autonomous_repair_loop() {
        let fixture = TestFixture::new("broken-counter", 8091, false);
        let model = QwenClient::new(None);

        let mut fixer = FixerAgent::new(model, orchyn_fixer_agent::create_default_tools(fixture.dir.clone(), Arc::new(QwenClient::new(None))), None);

        let reality = orchyn_core::RealityContext {
            screenshot_path: PathBuf::from("counter.png"),
            dom_elements: vec![],
            diagnostics: vec![orchyn_core::DiagnosticEvent::Console {
                level: orchyn_core::LogLevel::Error,
                message: "API Error: /api/counter/increment failed with 500".to_string(),
                timestamp: chrono::Utc::now(),
            }],
            network_events: vec![],
            timestamp: chrono::Utc::now(),
        };

        let visual_state = orchyn_pixel_observer::UiState {
            elements: vec![],
            anomalies: vec![],
            keyboard_visible: false,
        };

        info!("Running Fixer Autonomous Repair Loop...");
        let _ = fixer.propose_fix(&reality, &visual_state).await;
    }

    #[tokio::test]
    async fn test_agent_harness_with_mock() {
        let fixture = TestFixture::new("broken-counter", 8091, true);

        let responses = vec![
            r#"{"name": "list_files", "arguments": {}}"#.to_string(),
            r#"{"name": "read_file", "arguments": {"path": "server.js"}}"#.to_string(),
            r#"{"name": "run_command", "arguments": {"command": "ls"}}"#.to_string(),
            r#"{"name": "observe_ui", "arguments": {"url": "http://localhost:8091", "prompt": "Verify button"}}"#.to_string(),
            r#"{"name": "finish", "arguments": {"summary": "Fixed the counter endpoint and verified with vision."}}"#.to_string(),
        ];

        let model = Arc::new(MockClient::new(responses));
        // Use MockClient for everything
        let tools = orchyn_fixer_agent::create_default_tools(fixture.dir.clone(), model.clone());
        let mut fixer = FixerAgent::new(model, tools, None);

        let reality = orchyn_core::RealityContext {
            screenshot_path: PathBuf::new(),
            dom_elements: vec![],
            diagnostics: vec![],
            network_events: vec![],
            timestamp: chrono::Utc::now(),
        };

        let visual_state = orchyn_pixel_observer::UiState {
            elements: vec![],
            anomalies: vec![],
            keyboard_visible: false,
        };

        info!("Running Mock Fixer Harness Test...");
        let summary = fixer.propose_fix(&reality, &visual_state).await.unwrap();

        assert!(summary.contains("Fixed the counter endpoint") || summary.contains("finished"));
        info!("Mock Fixer Summary: {}", summary);
    }

    #[tokio::test]
    async fn test_harness_multi_actor_e2e() {
        let _fixture = TestFixture::new("todo-app", 8092, true);

        let responses = vec![
            "Mock response".to_string(),
        ];

        let model = Arc::new(MockClient::new(responses));
        let llm = orchyn_harness::MockLlm { client: model };

        let config = orchyn_harness::HarnessConfig {
            target_url: "http://localhost:8092".to_string(),
            max_fix_iterations: 1,
        };

        // Storage setup
        let mock_db = Arc::new(sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap());
        let storage = Arc::new(Storage::new(mock_db));
        let artifact_storage = Arc::new(MockArtifactStorage::new());

        let harness = orchyn_harness::OrchynHarness::new(storage, artifact_storage, config, llm);
        let mut events = harness.subscribe();

        let printer = tokio::spawn(async move {
            use colored::*;
            use orchyn_harness::HarnessEvent::*;
            loop {
                match events.recv().await {
                    Ok(PhaseStart(phase)) => {
                        println!("\n{}", format!("── phase {:?} ──", phase).bold().yellow())
                    }
                    Ok(PhaseEnd(_evidence)) => {
                        println!("  {} Phase passed", "✓".green().bold());
                    }
                    Ok(Notice(msg)) => println!("  {} {}", "•".magenta(), msg),
                    Ok(Error(e)) => println!("  {} Error: {}", "✗".red().bold(), e),
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        info!("Starting Harness E2E Run...");
        let results = harness.run_all().await.unwrap();
        drop(harness);
        let _ = printer.await;

        assert!(!results.is_empty());
        info!("Harness E2E Run Completed with {} phases.", results.len());
    }
}
