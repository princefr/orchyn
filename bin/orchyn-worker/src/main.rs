use orchyn_scheduler::Scheduler;
use orchyn_storage::Storage;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("Starting Orchyn Worker...");

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    let storage = Arc::new(Storage::new(Arc::new(pool)));
    let scheduler = Scheduler::new(storage);

    if let Err(e) = scheduler.run_worker_loop().await {
        tracing::error!("Worker failed: {}", e);
    }
}
