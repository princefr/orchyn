mod tui;

use orchyn_core::{OrchynError, Result};
use chrono::Utc;
use clap::Parser;
use git2::Repository;
use orchyn_harness::{HarnessEvent, OrchynHarness};
use orchyn_model_client::{ModelClient, QwenClient};
use orchyn_storage::Storage;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tracing::{info, warn, error};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Local path or Git repository URL to analyze.
    #[arg(default_value = ".")]
    source: String,

    /// Optional debugging goal handed to the fixer harness.
    #[arg(long, short)]
    goal: Option<String>,

    /// Local directory to store artifacts.
    #[arg(long, default_value = "./orchyn_artifacts")]
    artifact_dir: PathBuf,

    /// Launch the interactive TUI (action feed + evidence panel).
    #[arg(long)]
    tui: bool,

    /// Launch the autonomous testing and fixing harness.
    #[arg(long)]
    harness: bool,

    /// Cap the fixer loop.
    #[arg(long, default_value_t = 30)]
    max_steps: usize,

    /// Orchyn Server URL (VPS) to push results to.
    #[arg(long)]
    server: Option<String>,

    /// API Key for the Orchyn Server.
    #[arg(long, env = "ORCHYN_API_KEY")]
    api_key: Option<String>,

    /// Auto-approve all tool calls (non-interactive mode).
    #[arg(long, short = 'y')]
    yes: bool,
}

#[tokio::main]
async fn main() -> std::result::Result<(), OrchynError> {
    let args = Args::parse();

    if !args.tui {
        tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    }

    info!("Orchyn (Qwen 3.5 Edition) — starting...");

    // Initialize Storage
    let storage = if let Some(server_url) = args.server {
        let api_key = args.api_key.unwrap_or_else(|| "default-key".to_string());
        info!("Remote mode enabled: Pushing results to {}", server_url);
        // Would need a RemoteStorage impl
        None
    } else {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/orchyn".to_string());
        match sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
        {
            Ok(pool) => Some(Arc::new(Storage::new(Arc::new(pool)))),
            Err(e) => {
                warn!("Local storage unavailable ({}); running in ephemeral mode.", e);
                None
            }
        }
    };

    // Clone or use local path
    let mut _temp_dir_guard = None;
    let target_path = if args.source.starts_with("http") || args.source.starts_with("git@") {
        info!("Cloning repository {}...", args.source);
        let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
        let target_path = temp_dir.path().to_path_buf();
        Repository::clone(&args.source, &target_path)
            .map_err(|e| OrchynError::Internal(e.to_string()))?;
        _temp_dir_guard = Some(temp_dir);
        target_path
    } else {
        PathBuf::from(&args.source).canonicalize()
            .map_err(|e| OrchynError::Internal(e.to_string()))?
    };

    // Initialize model
    let qwen_client = QwenClient::new(None);
    let model: Arc<dyn ModelClient> = qwen_client.clone();
    let model_id = model.model_id().to_string();
    info!("Loaded local model: {}", model_id);

    // Create harness and run all phases
    let config = orchyn_harness::HarnessConfig {
        target_url: format!("file://{}", target_path.display()),
        max_fix_iterations: args.max_steps,
        phases: None, // Use defaults
    };

    let artifact_storage: Arc<dyn orchyn_artifact_storage::ArtifactStorage> = if storage.is_some() {
        Arc::new(orchyn_artifact_storage::LocalArtifactStorage::new(&args.artifact_dir)
            .map_err(|e| OrchynError::Storage(e.to_string()))?)
    } else {
        Arc::new(orchyn_artifact_storage::MockArtifactStorage::new())
    };

    let llm = orchyn_harness::RealLlm { client: model.clone() };
    
    if let Some(storage) = storage {
        let harness = OrchynHarness::new(
            storage,
            artifact_storage.clone(),
            config,
            llm,
        );

        let events = harness.subscribe();
        
        // Run with or without TUI
        if args.tui {
            let harness_handle = tokio::spawn(async move {
                harness.run_all().await
            });
            
            let tui_result = tui::run_tui(events, model_id, None).await;
            let harness_result = harness_handle.await;
            
            if let Err(e) = tui_result {
                eprintln!("TUI error: {}", e);
            }
            match harness_result {
                Ok(Ok(results)) => {
                    println!("\n✓ Harness completed with {} phases", results.len());
                    for evidence in &results {
                        println!("  - {:?}: {}", evidence.phase, evidence.verdict);
                    }
                }
                Ok(Err(e)) => eprintln!("Harness error: {}", e),
                Err(e) => eprintln!("Harness task panicked: {}", e),
            }
        } else {
            // Plain output
            let printer = tokio::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(HarnessEvent::PhaseStart(phase)) => {
                            println!("{}", format!("── Phase {:?} ──", phase).bold().yellow());
                        }
                        Ok(HarnessEvent::PhaseEnd(evidence)) => {
                            println!("  {} Phase {:?} finished: {}", "✓".green().bold(), evidence.phase, evidence.verdict);
                        }
                        Ok(HarnessEvent::Error(msg)) => {
                            println!("  {} Error: {}", "✗".red().bold(), msg.red());
                        }
                        Err(_) => break,
                        _ => {}
                    }
                }
            });
            
            let result = harness.run_all().await;
            let _ = printer.await;
            
            match result {
                Ok(results) => {
                    println!("\n✓ Harness completed with {} phases", results.len());
                    for evidence in &results {
                        println!("  - {:?}: {}", evidence.phase, evidence.verdict);
                    }
                }
                Err(e) => eprintln!("Harness error: {}", e),
            }
        }
    } else {
        // Ephemeral mode
        warn!("Running in ephemeral mode (no storage)");
        let harness = OrchynHarness::new(
            Arc::new(Storage::new_mock()),
            artifact_storage,
            config,
            llm,
        );
        
        let _ = harness.run_all().await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.tui {
        tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    }

    info!("Orchyn (Qwen 3.5 Edition) — starting...");

    // Initialize Storage (Remote Push or Local Postgres fallback)
    let storage = if let Some(server_url) = args.server {
        let api_key = args.api_key.unwrap_or_else(|| "default-key".to_string());
        info!("Remote mode enabled: Pushing results to {}", server_url);
        // Use a mock or trait-compliant wrapper for RemoteStorage
        // For now, let's assume Storage can be constructed from a RemoteStorage impl
        None // Placeholder until traits are aligned
    } else {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/orchyn".to_string());
        match sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
        {
            Ok(pool) => Some(Arc::new(Storage::new(Arc::new(pool))),
            Err(e) => {
                warn!("Local storage unavailable ({}); running in ephemeral mode.", e);
                None
            }
        }
    };

    // Clone or use local path
    let mut _temp_dir_guard = None;
    let target_path = if args.source.starts_with("http") || args.source.starts_with("git@") {
        info!("Cloning repository {}...", args.source);
        let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
        let target_path = temp_dir.path().to_path_buf();
        Repository::clone(&args.source, &target_path)?;
        _temp_dir_guard = Some(temp_dir);
        target_path
    } else {
        PathBuf::from(&args.source).canonicalize()?
    };

    // Initialize model
    let qwen_client = QwenClient::new(None);
    let model: Arc<dyn ModelClient> = qwen_client.clone();
    let model_id = model.model_id().to_string();
    info!("Loaded local model: {}", model_id);

    // Create harness and run all phases
    let config = orchyn_harness::HarnessConfig {
        target_url: format!("file://{}", target_path.display()),
        max_fix_iterations: args.max_steps,
    };

    let artifact_storage: Arc<dyn orchyn_artifact_storage::ArtifactStorage> = if storage.is_some() {
        // Use real artifact storage if we have storage
        Arc::new(orchyn_artifact_storage::LocalArtifactStorage::new(&args.artifact_dir)?)
    } else {
        Arc::new(orchyn_artifact_storage::MockArtifactStorage::new())
    };

    let llm = orchyn_harness::RealLlm { client: model.clone() };
    
    if let Some(storage) = storage {
        let harness = OrchynHarness::new(
            storage,
            artifact_storage,
            config,
            llm,
        );

        let events = harness.subscribe();
        
        // Run with or without TUI
        if args.tui {
            let harness_handle = tokio::spawn(async move {
                harness.run_all().await
            });
            
            let tui_result = tui::run_tui(events, model_id, None).await;
            let harness_result = harness_handle.await;
            
            if let Err(e) = tui_result {
                eprintln!("TUI error: {}", e);
            }
            match harness_result {
                Ok(Ok(results)) => {
                    println!("\n✓ Harness completed with {} phases", results.len());
                    for evidence in &results {
                        println!("  - {:?}: {}", evidence.phase, evidence.verdict);
                    }
                }
                Ok(Err(e)) => eprintln!("Harness error: {}", e),
                Err(e) => eprintln!("Harness task panicked: {}", e),
            }
        } else {
            // Plain output
            let printer = tokio::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(HarnessEvent::PhaseStart(phase)) => {
                            println!("{}", format!("── Phase {:?} ──", phase).bold().yellow());
                        }
                        Ok(HarnessEvent::PhaseEnd(evidence)) => {
                            println!("  {} Phase {:?} finished: {}", "✓".green().bold(), evidence.phase, evidence.verdict);
                        }
                        Ok(HarnessEvent::Error(msg)) => {
                            println!("  {} Error: {}", "✗".red().bold(), msg.red());
                        }
                        Err(_) => break,
                        _ => {}
                    }
                }
            });
            
            let result = harness.run_all().await;
            let _ = printer.await;
            
            match result {
                Ok(results) => {
                    println!("\n✓ Harness completed with {} phases", results.len());
                    for evidence in &results {
                        println!("  - {:?}: {}", evidence.phase, evidence.verdict);
                    }
                }
                Err(e) => eprintln!("Harness error: {}", e),
            }
        }
    } else {
        // Ephemeral mode - still run harness but without storage
        warn!("Running in ephemeral mode (no storage)");
        let harness = OrchynHarness::new(
            Arc::new(Storage::new_mock()), // Would need a mock storage
            artifact_storage,
            config,
            llm,
        );
        
        let _ = harness.run_all().await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.tui {
        tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    }

    info!("Orchyn (Qwen 3.5 Edition) — starting...");

    // Initialize Storage (Remote Push or Local Postgres fallback)
    let storage = if let Some(server_url) = args.server {
        let api_key = args.api_key.unwrap_or_else(|| "default-key".to_string());
        info!("Remote mode enabled: Pushing results to {}", server_url);
        // Use a mock or trait-compliant wrapper for RemoteStorage
        // For now, let's assume Storage can be constructed from a RemoteStorage impl
        None // Placeholder until traits are aligned
    } else {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/orchyn".to_string());
        match sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
        {
            Ok(pool) => Some(Arc::new(Storage::new(Arc::new(pool)))),
            Err(e) => {
                warn!("Local storage unavailable ({}); running in ephemeral mode.", e);
                None
            }
        }
    };

    // Clone or use local path
    let mut _temp_dir_guard = None;
    let target_path = if args.source.starts_with("http") || args.source.starts_with("git@") {
        info!("Cloning repository {}...", args.source);
        let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
        let target_path = temp_dir.path().to_path_buf();
        Repository::clone(&args.source, &target_path)?;
        _temp_dir_guard = Some(temp_dir);
        target_path
    } else {
        PathBuf::from(&args.source).canonicalize()?
    };

    // Initialize model
    let qwen_client = QwenClient::new(None);
    let model: Arc<dyn ModelClient> = qwen_client.clone();
    let model_id = model.model_id().to_string();
    info!("Loaded local model: {}", model_id);

    // Create harness and run all phases
    let config = orchyn_harness::HarnessConfig {
        target_url: format!("file://{}", target_path.display()),
        max_fix_iterations: args.max_steps,
    };

    let artifact_storage: Arc<dyn ArtifactStorage> = if storage.is_some() {
        // Use real artifact storage if we have storage
        Arc::new(orchyn_artifact_storage::LocalArtifactStorage::new(&args.artifact_dir)?)
    } else {
        Arc::new(orchyn_artifact_storage::MockArtifactStorage::new())
    };

    let llm = orchyn_harness::RealLlm { client: model.clone() };
    
    if let Some(storage) = storage {
        let harness = OrchynHarness::new(
            storage,
            artifact_storage,
            config,
            llm,
        );

        let events = harness.subscribe();
        
        // Run with or without TUI
        if args.tui {
            let harness_handle = tokio::spawn(async move {
                harness.run_all().await
            });
            
            let tui_result = tui::run_tui(events, model_id, None).await;
            let harness_result = harness_handle.await;
            
            if let Err(e) = tui_result {
                eprintln!("TUI error: {}", e);
            }
            match harness_result {
                Ok(Ok(results)) => {
                    println!("\n✓ Harness completed with {} phases", results.len());
                    for evidence in &results {
                        println!("  - {:?}: {}", evidence.phase, evidence.verdict);
                    }
                }
                Ok(Err(e)) => eprintln!("Harness error: {}", e),
                Err(e) => eprintln!("Harness task panicked: {}", e),
            }
        } else {
            // Plain output
            let printer = tokio::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(HarnessEvent::PhaseStart(phase)) => {
                            println!("{}", format!("── Phase {:?} ──", phase).bold().yellow());
                        }
                        Ok(HarnessEvent::PhaseEnd(evidence)) => {
                            println!("  {} Phase {:?} finished: {}", "✓".green().bold(), evidence.phase, evidence.verdict);
                        }
                        Ok(HarnessEvent::Error(msg)) => {
                            println!("  {} Error: {}", "✗".red().bold(), msg.red());
                        }
                        Err(_) => break,
                        _ => {}
                    }
                }
            });
            
            let result = harness.run_all().await;
            let _ = printer.await;
            
            match result {
                Ok(results) => {
                    println!("\n✓ Harness completed with {} phases", results.len());
                    for evidence in &results {
                        println!("  - {:?}: {}", evidence.phase, evidence.verdict);
                    }
                }
                Err(e) => eprintln!("Harness error: {}", e),
            }
        }
    } else {
        // Ephemeral mode - still run harness but without storage
        warn!("Running in ephemeral mode (no storage)");
        let harness = OrchynHarness::new(
            Arc::new(Storage::new_mock()), // Would need a mock storage
            artifact_storage,
            config,
            llm,
        );
        
        let _ = harness.run_all().await?;
    }

    Ok(())
}

    info!("Orchyn (Heavy Client Edition) — starting...");

    // Initialize Intelligence (Local Model)
    let qwen_client = QwenClient::new(None);
    let model: Arc<dyn ModelClient> = qwen_client.clone();
    info!("Loaded local model via llama.cpp: {}", model.model_id());

    // Initialize Storage (Remote Push or Local Postgres fallback)
    let storage = if let Some(server_url) = args.server {
        let api_key = args.api_key.unwrap_or_else(|| "default-key".to_string());
        info!("Remote mode enabled: Pushing results to {}", server_url);
        // Use a mock or trait-compliant wrapper for RemoteStorage
        // For now, let's assume Storage can be constructed from a RemoteStorage impl
        None // Placeholder until traits are aligned
    } else {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/orchyn".to_string());
        match sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
        {
            Ok(pool) => Some(Arc::new(Storage::new(Arc::new(pool)))),
            Err(e) => {
                warn!("Local storage unavailable ({}); running in ephemeral mode.", e);
                None
            }
        }
    };

    let mut _temp_dir_guard = None;
    let target_path = if args.source.starts_with("http") || args.source.starts_with("git@") {
        info!("Cloning repository {}...", args.source);
        let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
        let target_path = temp_dir.path().to_path_buf();
        Repository::clone(&args.source, &target_path)?;
        _temp_dir_guard = Some(temp_dir);
        target_path
    } else {
        PathBuf::from(&args.source).canonicalize()?
    };

    // Use llama.cpp with Qwen 3.5 0.8B
    let qwen_client = QwenClient::new(None);
    let model: Arc<dyn ModelClient> = qwen_client.clone();
    let model_id = model.model_id().to_string();
    info!("Loaded local model via llama.cpp: {}", model_id);

    if args.harness {
        let config = orchyn_harness::HarnessConfig {
            target_url: "http://localhost:3000".to_string(),
            max_fix_iterations: args.max_steps,
        };

        let storage = storage.expect("Harness requires storage");
        let artifact_storage = Arc::new(orchyn_artifact_storage::MockArtifactStorage::new());
        let llm = orchyn_harness::RealLlm { client: model.clone() };
        let harness = orchyn_harness::OrchynHarness::<orchyn_harness::RealLlm>::new(
            storage,
            artifact_storage,
            config,
            llm,
        );

        let (tx, _rx) = broadcast::channel(100);
        return run_harness_plain(harness, tx.subscribe()).await;
    }

    info!("Indexing codebase at {}...", target_path.display());
    let indexer = Indexer::new(&target_path);
    let app_map = indexer.index()?;

    let mut agent = FixerAgent::new(qwen_client, vec![], None);
    agent.set_max_steps(args.max_steps);

    let goal = args
        .goal
        .clone()
        .unwrap_or_else(|| "Find and fix any bugs in this project.".to_string());

    if let Ok(mut bundle) = agent.evidence().lock() {
        bundle.goal = Some(goal.clone());
    }

    let initial_sha = agent.initial_sha().map(|s| s.to_string());
    let events = agent.subscribe();

    let reality = RealityContext {
        screenshot_path: PathBuf::new(),
        dom_elements: Vec::new(),
        diagnostics: Vec::new(),
        network_events: Vec::new(),
        timestamp: Utc::now(),
    };
    let visual_state = UiState {
        elements: Vec::new(),
        anomalies: Vec::new(),
        keyboard_visible: false,
    };

    let file_list: Vec<String> = app_map
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().to_string())
        .collect();
    let initial_context = format!(
        "GOAL: {}\n\nPROJECT STRUCTURE:\nFiles: {:?}\n\n\
        You are in an autonomous loop. Use 'observe_ui' and 'run_command' to verify all fixes.",
        goal, file_list
    );

    if args.tui {
        run_with_tui(
            agent,
            events,
            reality,
            visual_state,
            model_id,
            initial_sha,
            Some(initial_context),
        )
        .await
    } else {
        run_plain(agent, events, reality, visual_state, Some(initial_context)).await
    }
}

async fn run_with_tui<M: ModelClient + 'static>(
    mut agent: FixerAgent<M>,
    events: broadcast::Receiver<orchyn_fixer_agent::AgentEvent>,
    reality: RealityContext,
    visual_state: UiState,
    model_id: String,
    initial_sha: Option<String>,
    initial_history: Option<String>,
) -> Result<()> {
    let agent_handle = tokio::spawn(async move {
        agent
            .propose_fix_with_history(&reality, &visual_state, initial_history)
            .await
    });

    let tui_result = tui::run_tui(events, model_id, initial_sha).await;
    let agent_result = agent_handle.await;

    if let Err(e) = tui_result {
        eprintln!("TUI error: {}", e);
    }
    match agent_result {
        Ok(Ok(summary)) => println!("\n{}", summary),
        Ok(Err(e)) => eprintln!("agent error: {}", e),
        Err(e) => eprintln!("agent task panicked: {}", e),
    }
    Ok(())
}

async fn run_plain<M: ModelClient + 'static>(
    mut agent: FixerAgent<M>,
    mut events: broadcast::Receiver<orchyn_fixer_agent::AgentEvent>,
    reality: RealityContext,
    visual_state: UiState,
    initial_history: Option<String>,
) -> Result<()> {
    let printer = tokio::spawn(async move {
        use colored::*;
        use orchyn_fixer_agent::AgentEvent::*;
        loop {
            match events.recv().await {
                Ok(Step { n, max }) => {
                    println!("\n{}", format!("── step {}/{} ──", n, max).bold().cyan())
                }
                Ok(ToolCall { name, args }) => {
                    let s = args.to_string();
                    let trimmed: String = s.chars().take(120).collect();
                    println!(
                        "{} {} {}",
                        "→".yellow().bold(),
                        name.bold().yellow(),
                        trimmed.dimmed()
                    );
                }
                Ok(ToolResult { name, ok, summary }) => {
                    if ok {
                        println!("  {} {}: {}", "✓".green().bold(), name.green(), summary);
                    } else {
                        println!("  {} {}: {}", "✗".red().bold(), name.red(), summary.red());
                    }
                }
                Ok(Notice(msg)) => println!("{} {}", "•".magenta(), msg),
                Ok(Finish { summary }) => {
                    println!("\n{} {}", "◆".bright_blue().bold(), summary.bold());
                    break;
                }
                Ok(Evidence(_)) => {}
                Err(_) => break,
            }
        }
    });

    let result = agent
        .propose_fix_with_history(&reality, &visual_state, initial_history)
        .await;
    let _ = printer.await;
    match result {
        Ok(_summary) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn run_harness_plain<L: orchyn_harness::LlmClient + Send + 'static>(
    harness: OrchynHarness<L>,
    mut events: broadcast::Receiver<HarnessEvent>,
) -> Result<()> {
    let printer = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(HarnessEvent::PhaseStart(phase)) => {
                    println!("\n{}", format!("── phase {:?} ──", phase).bold().yellow());
                }
                Ok(HarnessEvent::PhaseEnd(evidence)) => {
                    println!("  {} Phase {:?} finished", "✓".green().bold(), evidence.phase);
                }
                Ok(HarnessEvent::Error(msg)) => {
                    println!("  {} Error: {}", "✗".red().bold(), msg.red());
                }
                Err(_) => break,
                _ => {}
            }
        }
    });

    let _ = harness.run_all().await?;
    let _ = printer.await;
    Ok(())
}
