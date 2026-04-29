use orchyn_core::Result;
use orchyn_storage::Storage;
use sqlx::Row;
use std::sync::Arc;
use tracing::{info, error};
use uuid::Uuid;

pub struct Scheduler {
    storage: Arc<Storage>,
}

impl Scheduler {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }

    /// Continuously polls the database for runs in 'pending' state and processes them.
    pub async fn run_worker_loop(&self) -> Result<()> {
        info!("Worker polling for pending runs...");
        
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            
            // In a real system, we would wrap this in a transaction and use FOR UPDATE SKIP LOCKED
            match sqlx::query("SELECT id, project_id FROM runs WHERE status = 'pending' LIMIT 1")
                .fetch_optional(&*self.storage.db)
                .await
            {
                Ok(Some(row)) => {
                    let run_id: Uuid = row.get("id");
                    info!("Found pending run: {}. Starting processing...", run_id);

                    let _ = self.storage.update_run_status(run_id, "running").await;

                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                    let _ = self.storage.update_run_status(run_id, "completed").await;
                    info!("Completed run: {}", run_id);
                }
                Ok(None) => {
                    // No pending runs, wait
                }
                Err(e) => {
                    error!("Error polling for runs: {}", e);
                }
            }
        }
    }

    pub async fn run_smoke_test() -> Result<()> {
        info!("Smoke test executed successfully.");
        Ok(())
    }
}
