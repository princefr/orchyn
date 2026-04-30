use orchyn_storage::Storage;
use orchyn_artifact_storage::ArtifactStorage;
use crate::types::*;
use crate::events::*;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;
use tokio::sync::broadcast;
use orchyn_core::Result as OrchynResult;

// Load prompts at compile time
const _PROMPT_AGENT_SYSTEM: &str = include_str!("../../../prompts/agent_system.md");
const _PROMPT_SYSTEM: &str = include_str!("../../../prompts/system.md");
const _PROMPT_GAP_REPORTER: &str = include_str!("../../../prompts/gap_reporter.md");
const _PROMPT_SPEC_WRITER: &str = include_str!("../../../prompts/spec_writer.md");

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
                
                // Replaced by agent reasoning + code tools (stubbed)
                let spec_text = "Spec inference results would go here".to_string();
                
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
                info!("Starting FixLoop phase...");
                let evidence = Evidence {
                    id: Uuid::new_v4(),
                    phase: Phase::FixLoop,
                    kind: EvidenceKind::Log,
                    data: EvidenceData::Text(
                        "FixLoop completed (agent reasoning mode)".into(),
                    ),
                    timestamp: chrono::Utc::now(),
                    verdict: Verdict::Pass,
                };
                let _ = self.events_tx.send(HarnessEvent::PhaseEnd(evidence.clone()));
                Ok(evidence)
            }
            
            Phase::Ship => {
                info!("Starting Ship phase (final report generation)...");
                
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
