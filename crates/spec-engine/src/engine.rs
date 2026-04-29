use crate::types::*;
use orchyn_codebase_indexer::AppMap;
use orchyn_model_client::{ModelClient, CompletionRequest};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;
use futures::StreamExt;

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/spec_writer.md");

pub struct SpecEngine<M: ModelClient> {
    model: Arc<M>,
}

impl<M: ModelClient> SpecEngine<M> {
    pub fn new(model: Arc<M>) -> Self {
        Self { model }
    }

    /// Generate specs from codebase analysis
    pub async fn generate_spec(&self, app_map: &AppMap) -> anyhow::Result<Spec> {
        info!("Generating spec from codebase analysis...");

        // Summarize app map for context
        let files_summary: Vec<String> = app_map.files.iter().map(|f| f.path.display().to_string()).collect();

        let prompt = format!(
            "{}\n\nAnalyze the following list of files in the project and infer the intended features and invariants.\n\
            Files: {:?}\n\n\
            Return ONLY a JSON array of objects with 'kind', 'statement', and 'confidence'.",
            SYSTEM_PROMPT, files_summary
        );

        let req = CompletionRequest {
            prompt,
            max_tokens: Some(1024),
            temperature: Some(0.0),
        };

        let mut stream = self.model.complete(req).await.map_err(|e| anyhow::anyhow!(e))?;
        let mut response = String::new();
        while let Some(chunk) = stream.next().await {
            response.push_str(&chunk.map_err(|e| anyhow::anyhow!(e))?);
        }

        let mut claims = Vec::new();
        
        let start = response.find('[').unwrap_or(0);
        let json_str = &response[start..];
        
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(arr) = val.as_array() {
                for item in arr {
                    let kind_str = item.get("kind").and_then(|v| v.as_str()).unwrap_or("Feature");
                    let kind = match kind_str {
                        "Constraint" => ClaimKind::Constraint,
                        "Invariant" => ClaimKind::Invariant,
                        "Flow" => ClaimKind::Flow,
                        _ => ClaimKind::Feature,
                    };
                    
                    if let (Some(stmt), Some(conf)) = (
                        item.get("statement").and_then(|v| v.as_str()),
                        item.get("confidence").and_then(|v| v.as_f64())
                    ) {
                        claims.push(Claim {
                            id: ClaimId(Uuid::new_v4()),
                            kind,
                            statement: stmt.to_string(),
                            confidence: conf as f32,
                            evidence: vec![],
                            tags: vec![],
                        });
                    }
                }
            }
        } else {
            warn!("Failed to parse spec engine response as JSON array.");
        }

        let spec = Spec {
            id: SpecId(Uuid::new_v4()),
            app_id: AppId(Uuid::new_v4()),
            version: 1,
            claims,
            generated_at: chrono::Utc::now(),
            model_id: self.model.model_id().to_string(),
        };

        info!("Generated Spec with {} claims.", spec.claims.len());

        Ok(spec)
    }
}
