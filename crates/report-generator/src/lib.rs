use orchyn_model_client::{ModelClient, CompletionRequest};
use std::sync::Arc;
use futures::StreamExt;

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/gap_reporter.md");

pub struct Gap {
    pub description: String,
    pub severity: String,
}

pub struct ReportGenerator<M: ModelClient> {
    model: Arc<M>,
}

impl<M: ModelClient> ReportGenerator<M> {
    pub fn new(model: Arc<M>) -> Self {
        Self { model }
    }

    pub async fn generate_report(&self, gaps: &[Gap]) -> anyhow::Result<String> {
        let mut reports = Vec::new();

        for gap in gaps {
            let prompt = format!(
                "{}\n\nGAP DATA:\nDescription: {}\nSeverity: {}\n\nGenerate the markdown report segment for this gap:",
                SYSTEM_PROMPT, gap.description, gap.severity
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
            reports.push(response);
        }

        Ok(reports.join("\n\n---\n\n"))
    }
}
