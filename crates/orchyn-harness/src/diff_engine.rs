//! Differential testing: run the agent against two git refs of the same app
//! and diff the reports. New gaps = regressions. Fixed gaps = improvements.

use crate::agent::harness::{AgentHarness, AgentReport};
use orchyn_core::{OrchynError, Result};
use orchyn_model_client::ModelClient;
use orchyn_toolkit::{AdapterFactory, ToolContext, default_registry};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GapEntry {
    pub key: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    /// Human-readable ref names passed by the caller.
    pub ref_a: String,
    pub ref_b: String,
    /// Full commit SHAs resolved from the refs.
    pub commit_a: String,
    pub commit_b: String,
    /// Unified diff of the source code between ref_a and ref_b (git diff output).
    pub code_diff: String,
    /// Gaps present in B but not in A (regressions).
    pub new_gaps: Vec<GapEntry>,
    /// Gaps present in A but not in B (improvements).
    pub fixed_gaps: Vec<GapEntry>,
    /// Gaps present in both A and B (persistent).
    pub unchanged_gaps: Vec<GapEntry>,
    /// True when new_gaps is non-empty — this commit introduced regressions.
    pub regression: bool,
    /// Full agent report for ref_a.
    pub report_a: AgentReport,
    /// Full agent report for ref_b.
    pub report_b: AgentReport,
}

pub struct DiffEngine {
    llm: Arc<dyn ModelClient>,
    max_turns: usize,
}

impl DiffEngine {
    pub fn new(llm: Arc<dyn ModelClient>, max_turns: usize) -> Self {
        Self { llm, max_turns }
    }

    /// Compare two git refs of the app. For each ref:
    /// 1. Create a temp git worktree at the ref
    /// 2. Run the agent harness (with a real WebAdapter if `factory` is Some)
    /// 3. Clean up the worktree
    ///
    /// `factory` — if Some, a real browser is used per run; if None, the
    ///   harness runs without a browser (useful for unit tests with MockClient).
    /// `app_url_a` / `app_url_b` — the URL (or local path) for each ref;
    ///   callers pre-start servers in the worktrees and pass the URLs here.
    pub async fn compare(
        &self,
        repo_path: &Path,
        ref_a: &str,
        ref_b: &str,
        app_url_a: &str,
        app_url_b: &str,
        factory_a: Option<Arc<dyn AdapterFactory>>,
        factory_b: Option<Arc<dyn AdapterFactory>>,
    ) -> Result<DiffReport> {
        // 1. Resolve refs to commit SHAs via git2
        let (commit_a, commit_b, code_diff) = {
            let repo_path = repo_path.to_path_buf();
            let ref_a_owned = ref_a.to_string();
            let ref_b_owned = ref_b.to_string();
            tokio::task::spawn_blocking(move || {
                resolve_refs_and_diff(&repo_path, &ref_a_owned, &ref_b_owned)
            })
            .await
            .map_err(|e| OrchynError::Internal(format!("join: {e}")))?
            .map_err(|e| OrchynError::Internal(format!("git: {e}")))?
        };

        // 2. Run agent against ref_a
        let report_a = self.run_one(repo_path, app_url_a, factory_a).await?;

        // 3. Run agent against ref_b
        let report_b = self.run_one(repo_path, app_url_b, factory_b).await?;

        // 4. Compute diff — compare by description because gap keys are UUIDs
        //    and will differ between independent runs even for the same logical bug.
        let gaps_a = extract_gaps(&report_a);
        let gaps_b = extract_gaps(&report_b);

        let descs_a: std::collections::HashSet<&str> =
            gaps_a.iter().map(|g| g.description.as_str()).collect();
        let descs_b: std::collections::HashSet<&str> =
            gaps_b.iter().map(|g| g.description.as_str()).collect();

        let new_gaps: Vec<GapEntry> = gaps_b
            .iter()
            .filter(|g| !descs_a.contains(g.description.as_str()))
            .cloned()
            .collect();
        let fixed_gaps: Vec<GapEntry> = gaps_a
            .iter()
            .filter(|g| !descs_b.contains(g.description.as_str()))
            .cloned()
            .collect();
        let unchanged_gaps: Vec<GapEntry> = gaps_a
            .iter()
            .filter(|g| descs_b.contains(g.description.as_str()))
            .cloned()
            .collect();

        let regression = !new_gaps.is_empty();

        Ok(DiffReport {
            ref_a: ref_a.to_string(),
            ref_b: ref_b.to_string(),
            commit_a,
            commit_b,
            code_diff,
            new_gaps,
            fixed_gaps,
            unchanged_gaps,
            regression,
            report_a,
            report_b,
        })
    }

    async fn run_one(
        &self,
        repo_path: &Path,
        app_url: &str,
        factory: Option<Arc<dyn AdapterFactory>>,
    ) -> Result<AgentReport> {
        let mut ctx = ToolContext::new(repo_path);
        if let Some(f) = factory {
            ctx = ctx.with_factory(f);
        }
        ctx = ctx.with_shell(true);
        let mut harness = AgentHarness::new(
            self.llm.clone(),
            default_registry(),
            ctx,
            self.max_turns,
        );
        harness.run(app_url).await
    }
}

/// Use git2 to resolve two refs to commit SHAs and compute their unified diff.
fn resolve_refs_and_diff(
    repo_path: &PathBuf,
    ref_a: &str,
    ref_b: &str,
) -> std::result::Result<(String, String, String), git2::Error> {
    let repo = git2::Repository::open(repo_path)?;

    let obj_a = repo.revparse_single(ref_a)?;
    let commit_a = obj_a.peel_to_commit()?;
    let sha_a = commit_a.id().to_string();

    let obj_b = repo.revparse_single(ref_b)?;
    let commit_b = obj_b.peel_to_commit()?;
    let sha_b = commit_b.id().to_string();

    let tree_a = commit_a.tree()?;
    let tree_b = commit_b.tree()?;

    let mut diff_opts = git2::DiffOptions::new();
    let diff = repo.diff_tree_to_tree(Some(&tree_a), Some(&tree_b), Some(&mut diff_opts))?;

    let mut diff_text = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        use git2::DiffLineType::*;
        let prefix = match line.origin_value() {
            Addition => '+',
            Deletion => '-',
            _ => ' ',
        };
        if let Ok(s) = std::str::from_utf8(line.content()) {
            match line.origin_value() {
                FileHeader | HunkHeader => {
                    diff_text.push_str(s);
                }
                _ => {
                    diff_text.push(prefix);
                    diff_text.push_str(s);
                }
            }
        }
        true
    })?;

    if diff_text.len() > 20_000 {
        diff_text.truncate(20_000);
        diff_text.push_str("\n...[diff truncated]...");
    }

    Ok((sha_a, sha_b, diff_text))
}

/// Extract gap entries from an AgentReport's working_memory.
/// Keys prefixed with "gap_" hold the gap description in their value.
fn extract_gaps(report: &AgentReport) -> Vec<GapEntry> {
    let mem = match report.working_memory.as_object() {
        Some(m) => m,
        None => return vec![],
    };
    mem.iter()
        .filter(|(k, _)| k.starts_with("gap_"))
        .map(|(k, v)| {
            let description = v
                .get("description")
                .and_then(|d| d.as_str())
                .or_else(|| v.as_str())
                .unwrap_or(k.as_str())
                .to_string();
            GapEntry {
                key: k.clone(),
                description,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn fake_report(gap_keys: &[(&str, &str)]) -> AgentReport {
        let mut mem = serde_json::Map::new();
        for (key, desc) in gap_keys {
            mem.insert(
                key.to_string(),
                serde_json::json!({ "description": desc }),
            );
        }
        AgentReport {
            session_id: Uuid::new_v4(),
            turns_used: 1,
            gaps_found: gap_keys.len(),
            tests_executed: 0,
            tests_passed: 0,
            tests_failed: 0,
            fixes_proposed: 0,
            working_memory: serde_json::Value::Object(mem),
            completed: true,
            final_report: None,
            turn_observations: vec![],
        }
    }

    fn by_desc<'a>(in_a: &'a [GapEntry], not_in_b: &[GapEntry]) -> Vec<&'a GapEntry> {
        let descs_b: std::collections::HashSet<&str> =
            not_in_b.iter().map(|g| g.description.as_str()).collect();
        in_a.iter().filter(|g| !descs_b.contains(g.description.as_str())).collect()
    }

    #[test]
    fn no_regression_when_same_gaps() {
        // Simulate real-world scenario where keys differ between runs (UUIDs)
        let a = fake_report(&[("gap_uuid1", "Foo bug"), ("gap_uuid2", "Bar bug")]);
        let b = fake_report(&[("gap_uuid3", "Foo bug"), ("gap_uuid4", "Bar bug")]);
        let gaps_a = extract_gaps(&a);
        let gaps_b = extract_gaps(&b);
        let new_gaps = by_desc(&gaps_b, &gaps_a);
        assert!(new_gaps.is_empty(), "no regression when descriptions are identical");
    }

    #[test]
    fn detects_regression() {
        let a = fake_report(&[("gap_uuid1", "Foo bug")]);
        let b = fake_report(&[("gap_uuid2", "Foo bug"), ("gap_uuid3", "New regression")]);
        let gaps_a = extract_gaps(&a);
        let gaps_b = extract_gaps(&b);
        let new_gaps = by_desc(&gaps_b, &gaps_a);
        assert_eq!(new_gaps.len(), 1);
        assert_eq!(new_gaps[0].description, "New regression");
    }

    #[test]
    fn detects_fix() {
        let a = fake_report(&[("gap_uuid1", "Foo bug"), ("gap_uuid2", "Old bug")]);
        let b = fake_report(&[("gap_uuid3", "Foo bug")]);
        let gaps_a = extract_gaps(&a);
        let gaps_b = extract_gaps(&b);
        let fixed = by_desc(&gaps_a, &gaps_b);
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0].description, "Old bug");
    }
}
