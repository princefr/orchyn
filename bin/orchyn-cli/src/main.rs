mod tui;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use git2::Repository;
use orchyn_codebase_indexer::Indexer;
use orchyn_core::RealityContext;
use orchyn_fixer_agent::FixerAgent;
use orchyn_model_client::QwenClient;
use orchyn_pixel_observer::UiState;
use orchyn_storage::Storage;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tracing::{info, warn, Level};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Local path or Git repository URL to analyze.
    source: String,

    /// Local directory to store artifacts.
    #[arg(long, default_value = "./orchyn_artifacts")]
    artifact_dir: PathBuf,

    /// Launch the interactive TUI (action feed + evidence panel).
    #[arg(long)]
    tui: bool,

    /// Cap the fixer loop. 0 = skip the fixer entirely (just index).
    #[arg(long, default_value_t = 30)]
    max_steps: usize,

    /// Optional debugging goal handed to the fixer harness.
    #[arg(long)]
    goal: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.tui {
        tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    }

    info!("Orchyn — local codebase intelligence starting...");

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/orchyn".to_string());
    let _storage = match Storage::new(&database_url, args.artifact_dir.clone()).await {
        Ok(s) => Some(Arc::new(s)),
        Err(e) => {
            warn!("Storage unavailable ({}); continuing without it.", e);
            None
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

    let model = QwenClient::new(None);
    let model_id = model.model_id().to_string();
    info!("Loaded model client: {}", model_id);

    info!("Indexing codebase at {}...", target_path.display());
    let indexer = Indexer::new(&target_path);
    let app_map = indexer.index()?;
    
    if args.max_steps == 0 {
        info!("max_steps=0 — index-only run, skipping fixer harness.");
        return Ok(());
    }

    let mut agent = FixerAgent::new(model, target_path.clone());
    agent.set_max_steps(args.max_steps);

    if let Some(goal) = &args.goal {
        if let Ok(mut bundle) = agent.evidence().lock() {
            bundle.goal = Some(goal.clone());
        }
    }

    let initial_sha = agent.initial_sha().map(|s| s.to_string());
    let events = agent.subscribe();

    let reality = RealityContext {
        screenshot_path: PathBuf::new(),
        dom_elements: Vec::new(),
        diagnostics: Vec::new(),
        timestamp: Utc::now(),
    };
    let visual_state = UiState {
        elements: Vec::new(),
        anomalies: Vec::new(),
        keyboard_visible: false,
    };

    // Prepare initial context for the agent
    let file_list: Vec<String> = app_map.files.iter().map(|f| f.path.to_string_lossy().to_string()).collect();
    let initial_context = format!(
        "PROJECT STRUCTURE:\nFiles in project: {:?}\n\n\
        GOAL: Explore the codebase, run tests, observe the UI, and ensure everything works correctly. \
        DO NOT finish until you have used 'run_command' or 'observe_ui' to verify reality.",
        file_list
    );

    if args.tui {
        run_with_tui(agent, events, reality, visual_state, model_id, initial_sha, Some(initial_context)).await
    } else {
        run_plain(agent, events, reality, visual_state, Some(initial_context)).await
    }
}

async fn run_with_tui(
    mut agent: FixerAgent,
    events: broadcast::Receiver<orchyn_fixer_agent::AgentEvent>,
    reality: RealityContext,
    visual_state: UiState,
    model_id: String,
    initial_sha: Option<String>,
    initial_history: Option<String>,
) -> Result<()> {
    let agent_handle = tokio::spawn(async move {
        agent.propose_fix_with_history(&reality, &visual_state, initial_history).await
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

async fn run_plain(
    mut agent: FixerAgent,
    mut events: broadcast::Receiver<orchyn_fixer_agent::AgentEvent>,
    reality: RealityContext,
    visual_state: UiState,
    initial_history: Option<String>,
) -> Result<()> {
    let printer = tokio::spawn(async move {
        use orchyn_fixer_agent::AgentEvent::*;
        use colored::*;
        loop {
            match events.recv().await {
                Ok(Step { n, max }) => println!("\n{}", format!("── step {}/{} ──", n, max).bold().cyan()),
                Ok(ToolCall { name, args }) => {
                    let s = args.to_string();
                    let trimmed: String = s.chars().take(120).collect();
                    println!("{} {} {}", "→".yellow().bold(), name.bold().yellow(), trimmed.dimmed());
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

    let result = agent.propose_fix_with_history(&reality, &visual_state, initial_history).await;
    let _ = printer.await;
    match result {
        Ok(_summary) => {
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
