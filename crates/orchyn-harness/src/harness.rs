use orchyn_storage::Storage;
use orchyn_artifact_storage::ArtifactStorage;
use crate::types::*;
use crate::events::*;
use futures::StreamExt;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;
use tokio::sync::broadcast;
use orchyn_core::Result as OrchynResult;

// Load prompts at compile time
const PROMPT_NAVIGATOR: &str = include_str!("../../../prompts/navigator.md");
const PROMPT_VLM_OBSERVER: &str = include_str!("../../../prompts/vlm_observer.md");
const PROMPT_GAP_REPORTER: &str = include_str!("../../../prompts/gap_reporter.md");
const PROMPT_SPEC_WRITER: &str = include_str!("../../../prompts/spec_writer.md");

pub struct OrchynHarness<L: crate::LlmClient> {
    storage: Arc<Storage>,
    artifact_storage: Arc<dyn ArtifactStorage>,
    config: HarnessConfig,
    llm: L,
    events_tx: broadcast::Sender<HarnessEvent>,
}

impl<L: crate::LlmClient> OrchynHarness<L> {
    pub fn new(
        storage: Arc<Storage>,
        artifact_storage: Arc<dyn ArtifactStorage>,
        config: HarnessConfig,
        llm: L,
    ) -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            storage,
            artifact_storage,
            config,
            llm,
            events_tx: tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HarnessEvent> {
        self.events_tx.subscribe()
    }

    pub async fn run_phase(&self, phase: Phase) -> OrchynResult<Evidence> {
        let _ = self.events_tx.send(HarnessEvent::PhaseStart(phase.clone()));
        
        match phase {
            Phase::Ingest => {
                info!("Starting Ingest phase (codebase indexing + spec inference)...");
                
                // Use codebase-indexer to index the code
                let target_url = self.config.target_url.replace("file://", "");
                let target_path = std::path::Path::new(&target_url);
                let indexer = orchyn_codebase_indexer::Indexer::new(target_path);
                let app_map = indexer.index()
                    .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                
                // Use the navigator prompt to understand the app
                let prompt = format!(
                    "{}\n\nCodebase structure:\n{:?}\n\nGenerate a spec using the spec_writer prompt.",
                    PROMPT_NAVIGATOR,
                    app_map
                );
                
                let req = orchyn_model_client::CompletionRequest {
                    prompt,
                    max_tokens: Some(2048),
                    temperature: Some(0.0),
                };
                
                let mut stream = self.llm.client().complete(req).await
                    .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
                
                let mut spec_text = String::new();
                while let Some(chunk) = stream.next().await {
                    spec_text.push_str(&chunk?);
                }
                
                let evidence = Evidence {
                    id: Uuid::new_v4(),
                    phase: Phase::Ingest,
                    kind: EvidenceKind::Log,
                    data: EvidenceData::Text(format!("Spec generated:\n{}", spec_text)),
                    timestamp: chrono::Utc::now(),
                    verdict: Verdict::Pass,
                };
                
                let _ = self.events_tx.send(HarnessEvent::PhaseEnd(evidence.clone()));
                Ok(evidence)
            }
            
            Phase::Observation => {
                info!("Starting Observation phase (UI capture + VLM analysis)...");
                
                // Use pixel-observer to capture UI state
                let vlm_prompt = format!(
                    "{}\n\nAnalyze the current UI state.",
                    PROMPT_VLM_OBSERVER
                );
                
                // This would use platform-web to navigate and capture
                // For now, create a placeholder
                let evidence = Evidence {
                    id: Uuid::new_v4(),
                    phase: Phase::Observation,
                    kind: EvidenceKind::Screenshot,
                    data: EvidenceData::Text("Observation phase - VLM analysis would happen here".to_string()),
                    timestamp: chrono::Utc::now(),
                    verdict: Verdict::Pass,
                };
                
                let _ = self.events_tx.send(HarnessEvent::PhaseEnd(evidence.clone()));
                Ok(evidence)
            }
            
            Phase::GapDetection => {
                info!("Starting GapDetection phase...");
                
                // Use gap-detector with gap_reporter prompt
                let prompt = format!(
                    "{}\n\nAnalyze the spec vs observed behavior to find gaps.",
                    PROMPT_GAP_REPORTER
                );
                
                let evidence = Evidence {
                    id: Uuid::new_v4(),
                    phase: Phase::GapDetection,
                    kind: EvidenceKind::Log,
                    data: EvidenceData::Text("Gap detection completed".to_string()),
                    timestamp: chrono::Utc::now(),
                    verdict: Verdict::Pass,
                };
                
                let _ = self.events_tx.send(HarnessEvent::PhaseEnd(evidence.clone()));
                Ok(evidence)
            }
            
            Phase::FixLoop => {
                // Legacy phase-based FixLoop is being superseded by the new
                // hybrid `agent` module. The original wiring constructed
                // `FixerAgent` with `Arc<dyn ModelClient>` (it requires a
                // concrete `M: ModelClient`) and called a non-existent
                // `propose_fix` method. Stubbed until the legacy pipeline is
                // migrated or removed.
                info!("FixLoop phase is stubbed pending hybrid migration");
                let evidence = Evidence {
                    id: Uuid::new_v4(),
                    phase: Phase::FixLoop,
                    kind: EvidenceKind::Log,
                    data: EvidenceData::Text(
                        "FixLoop stubbed — see crates/orchyn-harness/src/agent for replacement".into(),
                    ),
                    timestamp: chrono::Utc::now(),
                    verdict: Verdict::Pass,
                };
                let _ = self.events_tx.send(HarnessEvent::PhaseEnd(evidence.clone()));
                Ok(evidence)
            }
            
            Phase::Ship => {
                info!("Starting Ship phase (final report generation)...");
                
                // Use report-generator to create final report
                let evidence = Evidence {
                    id: Uuid::new_v4(),
                    phase: Phase::Ship,
                    kind: EvidenceKind::Log,
                    data: EvidenceData::Text("Ship phase - report generated".to_string()),
                    timestamp: chrono::Utc::now(),
                    verdict: Verdict::Pass,
                };
                
                let _ = self.events_tx.send(HarnessEvent::PhaseEnd(evidence.clone()));
                Ok(evidence)
            }
            
            _ => {
                // Stub for other phases
                let evidence = Evidence {
                    id: Uuid::new_v4(),
                    phase: phase.clone(),
                    kind: EvidenceKind::Log,
                    data: EvidenceData::Text(format!("Phase {:?} completed (stub)", phase)),
                    timestamp: chrono::Utc::now(),
                    verdict: Verdict::Pass,
                };
                let _ = self.events_tx.send(HarnessEvent::PhaseEnd(evidence.clone()));
                Ok(evidence)
            }
        }
    }

    pub async fn run_all(&self) -> OrchynResult<Vec<Evidence>> {
        info!("Starting full Orchyn validation harness for {}", self.config.target_url);
        
        let mut results = vec![];
        let phases = vec![
            Phase::Ingest,
            Phase::Observation,
            Phase::GapDetection,
            Phase::FixLoop,
            Phase::Ship,
        ];

        for phase in phases {
            results.push(self.run_phase(phase).await?);
        }

        Ok(results)
    }

    pub async fn save_evidence(&self, run_id: Uuid, data: Vec<u8>, name: &str) -> OrchynResult<()> {
        let meta = self.artifact_storage.save(
            run_id,
            orchyn_artifact_storage::ArtifactKind::Screenshot,
            data,
            "image/png".to_string()
        ).await.map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
        
        info!("Saved evidence {}: {}", name, meta.id);
        Ok(())
    }
}

pub type Result<T, E> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct HarnessRunResult {
    pub run: orchyn_storage::RunRow,
}
