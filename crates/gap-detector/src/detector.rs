use crate::types::*;
use orchyn_core::{RealityContext, Result};
use orchyn_model_client::{ModelClient, CompletionRequest};
use orchyn_spec_engine::Spec;
use std::sync::Arc;
use tracing::info;
use futures::StreamExt;

pub struct GapDetector<M: ModelClient> {
    model: Arc<M>,
}

impl<M: ModelClient> GapDetector<M> {
    pub fn new(model: Arc<M>) -> Self {
        Self { model }
    }

    pub async fn detect_gaps(&self, spec: &Spec, reality: &RealityContext) -> Result<Vec<Gap>> {
        info!("Detecting gaps between Spec (version {}) and current Reality...", spec.version);

        let spec_json = serde_json::to_string(spec).unwrap_or_default();
        let reality_json = serde_json::to_string(reality).unwrap_or_default();

        let prompt = format!(
            "You are an intent-vs-reality reconciliation engine.\n\
            Compare the intended SPEC against the observed REALITY.\n\n\
            SPEC:\n{}\n\n\
            REALITY:\n{}\n\n\
            Identify any discrepancies (gaps) where the reality fails to meet the spec.\n\
            Return ONLY a JSON array of objects with 'description' (string) and 'severity' (float 0.0-1.0).",
            spec_json, reality_json
        );

        let req = CompletionRequest {
            prompt,
            max_tokens: Some(1024),
            temperature: Some(0.0),
        };

        let mut stream = self.model.complete(req).await?;
        let mut response = String::new();
        while let Some(chunk) = stream.next().await {
            response.push_str(&chunk?);
        }

        // Try to parse the response as a JSON array of Gap objects
        // We look for '[' instead of '{' for an array
        let start = response.find('[').unwrap_or(0);
        let json_str = &response[start..];
        
        let mut gaps: Vec<Gap> = Vec::new();
        
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(arr) = val.as_array() {
                for item in arr {
                    if let (Some(desc), Some(sev)) = (
                        item.get("description").and_then(|v| v.as_str()),
                        item.get("severity").and_then(|v| v.as_f64())
                    ) {
                        gaps.push(Gap {
                            description: desc.to_string(),
                            severity: sev as f32,
                        });
                    }
                }
            }
        }

        info!("Gap detector found {} gaps.", gaps.len());
        Ok(gaps)
    }
}
