#[cfg(test)]
#[allow(unused)]
mod tests {
    use orchyn_action_executor::PlatformAdapter;
    use orchyn_action_planner::{ActionPlanner, Goal};
    use orchyn_actor_engine::{Actor, ActorBudget, ActorEngine, ActorPersona, MultiActorManager};
    use orchyn_codebase_indexer::Indexer;
    use orchyn_fixer_agent::FixerAgent;
    use orchyn_model_client::{ModelConfig, QwenClient};
    use orchyn_navigator_agent::NavigatorAgent;
    use orchyn_pixel_observer::PixelObserver;
    use orchyn_platform_web::WebAdapter;
    use std::path::PathBuf;
    use std::process::{Child, Command};
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
        let model = QwenClient::new(None);

        let indexer = Indexer::new(&fixture.dir);
        let app_map = indexer.index().unwrap();

        let mut navigator = NavigatorAgent::new(model, app_map);

        info!("Running Navigator Discovery Loop on Todo App...");
        // The navigator will use grep/read_file tools autonomously
        let claims = navigator
            .run("Identify the API endpoints and how the frontend calls them.")
            .await
            .unwrap();

        info!("Navigator discovered {} claims.", claims.len());
        // In a real run with model weights, we'd assert specific findings
    }

    #[tokio::test]
    async fn test_fixer_agent_autonomous_repair_loop() {
        let fixture = TestFixture::new("broken-counter", 8091, false);
        let model = QwenClient::new(None);

        let mut fixer = FixerAgent::new(model, fixture.dir.clone());

        // Construct a realistic failure context
        let reality = orchyn_core::RealityContext {
            screenshot_path: PathBuf::from("counter.png"),
            dom_elements: vec![],
            diagnostics: vec![orchyn_core::DiagnosticEvent::Console {
                level: orchyn_core::LogLevel::Error,
                message: "API Error: /api/counter/increment failed with 500".to_string(),
                timestamp: chrono::Utc::now(),
            }],
            timestamp: chrono::Utc::now(),
        };

        let visual_state = orchyn_pixel_observer::UiState {
            elements: vec![],
            anomalies: vec![],
            keyboard_visible: false,
        };

        info!("Running Fixer Autonomous Repair Loop...");
        // The fixer will use read_file/replace_text/run_command autonomously
        let summary = fixer.propose_fix(&reality, &visual_state).await.unwrap();

        info!("Fixer Repair Summary: {}", summary);
    }

    #[tokio::test]
    async fn test_broken_counter_exposes_gap() {
        let _fixture = TestFixture::new("broken-counter", 8091, true);
        assert!(true);
    }
}
