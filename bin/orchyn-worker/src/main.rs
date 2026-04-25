use orchyn_scheduler::Scheduler;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("Starting Orchyn Worker...");
    if let Err(e) = Scheduler::run_smoke_test().await {
        tracing::error!("Worker failed: {}", e);
    }
}
