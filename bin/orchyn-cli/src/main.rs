mod tui;

use clap::Parser;
use colored::Colorize;
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::{OrchynError, PlatformAdapter, Result};
use orchyn_harness::{
    agent::{AgentHarness, AgentReport},
    diff_engine::DiffEngine,
};
use orchyn_model_client::{ModelClient, QwenClient};
use orchyn_platform_web::WebAdapter;
use orchyn_toolkit::{default_registry, AdapterFactory, ToolContext};
use std::sync::Arc;
use tracing::{info, Level};

#[derive(Parser, Debug)]
#[command(
    name = "orchyn",
    author,
    version,
    about = "Orchyn — autonomous QA agent",
    long_about = "Orchyn explores your app, finds reality gaps, and can compare two git revisions to detect regressions."
)]
struct Cli {
    /// GitHub URL (https://github.com/owner/repo) or path to a local npm project.
    /// The agent will start the app via its shell tools.
    #[arg(long)]
    app: String,

    /// Cap on agent turns per run.
    #[arg(long, default_value_t = 40)]
    max_turns: usize,

    /// Project root the agent is allowed to read files from.
    #[arg(long, default_value = ".")]
    project_root: std::path::PathBuf,

    /// Differential mode: compare two git refs.
    /// Example: --diff HEAD~1 HEAD
    /// The agent runs against the app URL at each ref and reports new/fixed gaps.
    #[arg(long, num_args = 2, value_names = ["FROM_REF", "TO_REF"])]
    diff: Option<Vec<String>>,
}

#[tokio::main]
async fn main() -> std::result::Result<(), OrchynError> {
    tracing_subscriber::fmt()
        .with_max_level(Level::WARN) // suppress info noise — TUI shows progress
        .init();
    let cli = Cli::parse();

    let diff_refs = cli.diff.clone();
    if let Some(refs) = diff_refs {
        run_diff(cli, refs[0].clone(), refs[1].clone()).await
    } else {
        run_explore(cli).await
    }
}

// ── Single-run explore mode ───────────────────────────────────────────────────

async fn run_explore(args: Cli) -> Result<()> {
    info!("orchyn — app: {}", args.app);
    let model: Arc<dyn ModelClient> = QwenClient::new(None);

    let factory: Arc<dyn AdapterFactory> =
        Arc::new(WebAdapterFactory { initial_url: args.app.clone() });
    let registry = default_registry();
    let tool_index = registry
        .schemas()
        .into_iter()
        .map(|s| orchyn_toolkit::schema::ToolDescriptor {
            name: s.name,
            description: s.description,
        })
        .collect();
    let ctx = ToolContext::new(&args.project_root)
        .with_factory(factory)
        .with_shell(true)
        .with_tool_index(tool_index);

    let mut harness = AgentHarness::new(model, registry, ctx, args.max_turns);
    let rx = harness.subscribe();
    let render_task = tui::start(rx);

    let report = harness.run(&args.app).await?;
    render_task.await.ok();

    print_report(&report);
    Ok(())
}

// ── Differential mode ─────────────────────────────────────────────────────────

async fn run_diff(args: Cli, ref_a: String, ref_b: String) -> Result<()> {
    let repo_path = args.project_root.clone();
    let app = args.app.clone();
    let model: Arc<dyn ModelClient> = QwenClient::new(None);

    println!(
        "\n{}  {} {} → {}",
        "ORCHYN DIFF".cyan().bold(),
        "comparing".dimmed(),
        ref_a.bold(),
        ref_b.bold()
    );

    // For each run we create a fresh browser pointing at the app URL.
    // If the app is a local path the agent starts the server via bash (SETUP prompt).
    let factory_a: Arc<dyn AdapterFactory> =
        Arc::new(WebAdapterFactory { initial_url: app.clone() });
    let factory_b: Arc<dyn AdapterFactory> =
        Arc::new(WebAdapterFactory { initial_url: app.clone() });

    let engine = DiffEngine::new(model, args.max_turns);

    // Wire TUI for run A — subscribe *before* compare() spawns the harness
    // (compare() creates the harness internally, so we set up a separate channel
    //  here via a pair of AgentHarness runs with injected progress channels).
    //
    // Since DiffEngine::compare() creates harnesses internally without exposing
    // the broadcast channel, we print section headers manually and let the
    // harness stderr ([ORCHYN:*]) carry the detail.
    println!(
        "\n  {} {} ({})",
        "Run A →".bold(),
        ref_a.dimmed(),
        app.dimmed()
    );
    let diff_report = engine
        .compare(
            &repo_path,
            &ref_a,
            &ref_b,
            &app,
            &app,
            Some(factory_a),
            Some(factory_b),
        )
        .await?;

    tui::print_diff_report(&diff_report);

    std::process::exit(if diff_report.regression { 1 } else { 0 });
}

// ── Report pretty-printer ─────────────────────────────────────────────────────

fn print_report(r: &AgentReport) {
    println!("\n{}", "── Orchyn Report ──────────────────────────".bold().yellow());
    println!("  Session        : {}", r.session_id);
    println!("  Turns used     : {}", r.turns_used);
    println!("  Gaps found     : {}", r.gaps_found);
    println!("  Tests executed : {}", r.tests_executed);
    println!(
        "  Tests passed   : {}",
        r.tests_passed.to_string().green()
    );
    println!(
        "  Tests failed   : {}",
        if r.tests_failed > 0 {
            r.tests_failed.to_string().red()
        } else {
            r.tests_failed.to_string().normal()
        }
    );
    println!("  Fixes proposed : {}", r.fixes_proposed);
    println!(
        "  Completed      : {}",
        if r.completed {
            "yes".green()
        } else {
            "no".red()
        }
    );
    if let Some(rep) = &r.final_report {
        println!("\n{}", "── Final Report ───────────────────────────".bold().cyan());
        println!(
            "{}",
            serde_json::to_string_pretty(rep).unwrap_or_default()
        );
    }
}

// ── AdapterFactory ────────────────────────────────────────────────────────────

struct WebAdapterFactory {
    initial_url: String,
}

impl AdapterFactory for WebAdapterFactory {
    fn spawn(&self, _name: &str) -> BoxFuture<'_, Result<Box<dyn PlatformAdapter>>> {
        let url = self.initial_url.clone();
        async move {
            let adapter = WebAdapter::new(&url).await?;
            Ok(Box::new(adapter) as Box<dyn PlatformAdapter>)
        }
        .boxed()
    }
}
