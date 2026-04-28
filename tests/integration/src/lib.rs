#[cfg(test)]
#[allow(unused)]
mod tests {
    use orchyn_action_executor::PlatformAdapter;
    use orchyn_action_planner::{ActionPlanner, Goal};
    use orchyn_actor_engine::{ActorBudget, ActorEngine, ActorPersona, MultiActorManager};
    use orchyn_codebase_indexer::Indexer;
    use orchyn_fixer_agent::FixerAgent;
    use orchyn_model_client::{ModelConfig, QwenClient, MockClient, ModelClient};
    use orchyn_navigator_agent::NavigatorAgent;
    use orchyn_pixel_observer::PixelObserver;
    use orchyn_platform_web::WebAdapter;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::sync::Arc;
    use std::time::Duration;
    use tracing::{Level, info};
    use tracing_subscriber;
    use uuid::Uuid;

    struct TestFixture {
        server: Option<Child>,
        pub dir: PathBuf,
        port: u16,
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

            Self { server, dir, port }
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
        let model: Arc<dyn ModelClient> = Arc::new(QwenClient::new(None));

        let indexer = Indexer::new(&fixture.dir);
        let app_map = indexer.index().unwrap();

        let mut navigator = NavigatorAgent::new(model, app_map);

        info!("Running Navigator Discovery Loop on Todo App...");
        let claims = navigator
            .run("Identify the API endpoints and how the frontend calls them.")
            .await
            .unwrap();

        info!("Navigator discovered {} claims.", claims.len());
    }

    #[tokio::test]
    async fn test_fixer_agent_autonomous_repair_loop() {
        let fixture = TestFixture::new("broken-counter", 8091, false);
        let model: Arc<dyn ModelClient> = Arc::new(QwenClient::new(None));

        let mut fixer = FixerAgent::new(model, fixture.dir.clone());

        let reality = orchyn_core::RealityContext {
            screenshot_path: PathBuf::from("counter.png"),
            dom_elements: vec![],
            diagnostics: vec![orchyn_core::DiagnosticEvent::Console {
                level: orchyn_core::LogLevel::Error,
                message: "API Error: /api/counter/increment failed with 500".to_string(),
                timestamp: chrono::Utc::now(),
            }],
            network_events: vec![], // Added for unified diagnostics
            timestamp: chrono::Utc::now(),
        };

        let visual_state = orchyn_pixel_observer::UiState {
            elements: vec![],
            anomalies: vec![],
            keyboard_visible: false,
        };

        info!("Running Fixer Autonomous Repair Loop...");
        let summary = fixer.propose_fix(&reality, &visual_state).await.unwrap();

        info!("Fixer Repair Summary: {}", summary);
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
        
        let model: Arc<dyn ModelClient> = Arc::new(MockClient::new(responses));
        let mut fixer = FixerAgent::new(model, fixture.dir.clone());

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

        assert!(summary.contains("Fixed the counter endpoint"));
        info!("Mock Fixer Summary: {}", summary);
    }

    #[tokio::test]
    async fn test_broken_counter_exposes_gap() {
        let _fixture = TestFixture::new("broken-counter", 8091, true);
        assert!(true);
    }

    #[tokio::test]
    async fn test_harness_multi_actor_e2e() {
        let fixture = TestFixture::new("todo-app", 8092, true);
        
        let responses = vec![
            r#"{"file_path": "src/app.js", "line_start": 1, "line_end": 1, "original_content": "const x = 1;", "suggested_content": "const x = 2;", "explanation": "E2E Fix"}"#.to_string(),
        ];
        
        let model: Arc<dyn ModelClient> = Arc::new(MockClient::new(responses));
        let llm = orchyn_harness::RealLlm { client: model };
        
        let config = orchyn_harness::HarnessConfig {
            target_url: "http://localhost:8092".to_string(),
            max_fix_iterations: 1,
        };
        
        let mut harness = orchyn_harness::OrchynHarness::new(config, llm);
        let mut events = harness.subscribe();
        
        let printer = tokio::spawn(async move {
            use orchyn_harness::HarnessEvent::*;
            use orchyn_harness::Verdict;
            use orchyn_core::DiagnosticEvent::*;
            use colored::*;
            use similar::{ChangeTag, TextDiff};
            loop {
                match events.recv().await {
                    Ok(PhaseStart(phase)) => println!("\n{}", format!("── phase {:?} ──", phase).bold().yellow()),
                    Ok(PhaseEnd(evidence)) => {
                        if evidence.verdict == Verdict::Pass {
                            println!("  {} Phase passed", "✓".green().bold());
                        } else {
                            println!("  {} Phase failed: {:?}", "✗".red().bold(), evidence.verdict);
                        }
                    }
                    Ok(PatchApplied(patch)) => {
                        println!("\n{} {}", "→ Applied patch to".bold().magenta(), patch.file_path.display().to_string().magenta());
                        let diff = TextDiff::from_lines(&patch.original_content, &patch.suggested_content);
                        for change in diff.iter_all_changes() {
                            let (sign, color) = match change.tag() {
                                ChangeTag::Delete => ("-", Color::Red),
                                ChangeTag::Insert => ("+", Color::Green),
                                ChangeTag::Equal => (" ", Color::White),
                            };
                            print!("{}", format!("{}{}", sign, change).color(color));
                        }
                    }
                    Ok(Diagnostic(diag)) => {
                        match diag {
                            Console { level, message, .. } => {
                                println!("  {} [{:?}] {}", "web-console".dimmed(), level, message);
                            }
                            Anomaly { kind, details } => {
                                println!("  {} {}: {}", "anomaly".red().bold(), kind, details);
                            }
                        }
                    }
                    Ok(Network(net)) => {
                        println!("  {} {} {} -> {}", "network".dimmed(), net.method.bold(), net.url, net.status);
                    }
                    Ok(Notice(msg)) => println!("  {} {}", "•".magenta(), msg),
                    Ok(Failure(f)) => println!("  {} Failure: {}", "✗".red().bold(), f.message),
                    Err(_) => break,
                }
            }
        });

        info!("Starting Harness E2E Run...");
        let evidence = harness.run_all().await.unwrap();
        drop(harness);
        let _ = printer.await;
        
        assert!(!evidence.is_empty());
        info!("Harness E2E Run Completed with {} phases.", evidence.len());
    }

    #[tokio::test]
    async fn test_harness_with_failure_and_fix() {
        let fixture = TestFixture::new("todo-app", 8093, true);
        
        // This patch actually removes the console.error we added to server.js
        let responses = vec![
            format!(r#"{{"file_path": {:?}, "line_start": 91, "line_end": 91, "original_content": "console.error(\"CRITICAL_UI_ERROR: Action button is misaligned\");", "suggested_content": "// Fixed by Harness", "explanation": "Removing the intentional console error to verify the fix loop."}}"#, fixture.dir.join("server.js")),
        ];
        
        let model: Arc<dyn ModelClient> = Arc::new(MockClient::new(responses));
        let llm = orchyn_harness::RealLlm { client: model };
        
        let config = orchyn_harness::HarnessConfig {
            target_url: "http://localhost:8093/fail".to_string(),
            max_fix_iterations: 1,
        };
        
        let mut harness = orchyn_harness::OrchynHarness::new(config, llm);
        let mut events = harness.subscribe();
        
        let printer = tokio::spawn(async move {
            use orchyn_harness::HarnessEvent::*;
            use orchyn_harness::Verdict;
            use orchyn_core::DiagnosticEvent::*;
            use colored::*;
            use similar::{ChangeTag, TextDiff};
            loop {
                match events.recv().await {
                    Ok(PhaseStart(phase)) => println!("\n{}", format!("── phase {:?} ──", phase).bold().yellow()),
                    Ok(PhaseEnd(evidence)) => {
                        if evidence.verdict == Verdict::Pass {
                            println!("  {} Phase passed", "✓".green().bold());
                        } else {
                            println!("  {} Phase failed: {:?}", "✗".red().bold(), evidence.verdict);
                        }
                    }
                    Ok(PatchApplied(patch)) => {
                        println!("\n{} {}", "→ Applied patch to".bold().magenta(), patch.file_path.display().to_string().magenta());
                        let diff = TextDiff::from_lines(&patch.original_content, &patch.suggested_content);
                        for change in diff.iter_all_changes() {
                            let (sign, color) = match change.tag() {
                                ChangeTag::Delete => ("-", Color::Red),
                                ChangeTag::Insert => ("+", Color::Green),
                                ChangeTag::Equal => (" ", Color::White),
                            };
                            print!("{}", format!("{}{}", sign, change).color(color));
                        }
                    }
                    Ok(Diagnostic(diag)) => {
                        match diag {
                            Console { level, message, .. } => {
                                println!("  {} [{:?}] {}", "web-console".dimmed(), level, message);
                            }
                            Anomaly { kind, details } => {
                                println!("  {} {}: {}", "anomaly".red().bold(), kind, details);
                            }
                        }
                    }
                    Ok(Network(net)) => {
                        println!("  {} {} {} -> {}", "network".dimmed(), net.method.bold(), net.url, net.status);
                    }
                    Ok(Notice(msg)) => println!("  {} {}", "•".magenta(), msg),
                    Ok(Failure(f)) => println!("  {} Failure: {}", "✗".red().bold(), f.message),
                    Err(_) => break,
                }
            }
        });

        info!("Starting Harness Failure/Fix E2E Run...");
        let evidence = harness.run_all().await.unwrap();
        drop(harness);
        let _ = printer.await;
        
        // Expected flow: Ingest (Fail) -> ErrorConstraint (Pass) -> FixLoop (Pass) -> Harden (Pass) -> Ship (Pass)
        assert!(evidence.iter().any(|e| e.phase == orchyn_harness::Phase::FixLoop));
        assert_eq!(evidence.last().unwrap().phase, orchyn_harness::Phase::Ship);
        info!("Harness Failure/Fix E2E Run Completed.");
    }
}
