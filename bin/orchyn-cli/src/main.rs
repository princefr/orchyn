use clap::Parser;
use colored::*;
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::{OrchynError, PlatformAdapter, Result};
use orchyn_harness::agent::{AgentHarness, AgentReport};
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
    long_about = None
)]
struct Cli {
    /// GitHub URL (https://github.com/owner/repo) or path to a local npm project directory.
    /// The agent will clone/install/start the app using its shell tools.
    #[arg(long)]
    app: String,

    /// Cap on agent turns.
    #[arg(long, default_value_t = 40)]
    max_turns: usize,

    /// Project root the agent is allowed to read files from.
    #[arg(long, default_value = ".")]
    project_root: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> std::result::Result<(), OrchynError> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    let cli = Cli::parse();
    run_agent(cli).await
}

async fn run_agent(args: Cli) -> Result<()> {
    info!("orchyn — app: {}", args.app);
    let model: Arc<dyn ModelClient> = QwenClient::new(None);

    let factory: Arc<dyn AdapterFactory> =
        Arc::new(WebAdapterFactory { initial_url: args.app.clone() });
    let registry = default_registry();
    let tool_index = registry.schemas().into_iter()
        .map(|s| orchyn_toolkit::schema::ToolDescriptor { name: s.name, description: s.description })
        .collect();
    let ctx = ToolContext::new(args.project_root)
        .with_factory(factory)
        .with_shell(true)
        .with_tool_index(tool_index);

    let mut harness = AgentHarness::new(model, registry, ctx, args.max_turns);
    let report = harness.run(&args.app).await?;

    print_report(&report);
    Ok(())
}

fn print_report(r: &AgentReport) {
    println!("\n{}", "── Orchyn Report ──".bold().yellow());
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
        if r.completed { "yes".green() } else { "no".red() }
    );
    if let Some(rep) = &r.final_report {
        println!("\n{}", "── Final Report ──".bold().cyan());
        println!("{}", serde_json::to_string_pretty(rep).unwrap_or_default());
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
