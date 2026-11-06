//! `orchyn-check`: give a coding agent (or a human) grounded feedback on a
//! running web app, with no model in the loop.
//!
//! ```text
//! orchyn-check --url http://localhost:3000 --project ./app \
//!     [--manifest checks.json ...] [--universal] [--contracts]
//! orchyn-check --lint-spec TASKS.md [--lint-spec MORE.md]
//! ```
//!
//! * `--lint-spec`: model-free lint of a task spec for phrases that admit more
//!   than one reading. Runs first, needs no `--url`, never changes the exit code.
//!
//! * `--manifest`: replay each manifest in real browsers; print every step.
//! * `--universal`: replay the spec-free universal invariants for each page.
//! * `--contracts`: list invariants the code declares (inputs, validation,
//!   statuses, auth, schemas) so they can be turned into manifests.
//! * Always: reconcile the project's client calls / server routes with the
//!   network traffic observed during the run (reality gaps).
//!
//! Exit code 1 when any manifest fails or a universal check with confidence
//! >= 0.9 fails; lower-confidence universal failures are printed as advisory.
//! Exit code 0 otherwise.

use anyhow::{Context, Result};
use clap::Parser;
use futures::FutureExt;
use orchyn_core::PlatformAdapter;
use orchyn_harness::intent::{self, GapKind};
use orchyn_harness::invariants;
use orchyn_harness::replay::{AdapterFactoryFn, ReplayManifest, ReplayReport, ReplayRunner};
use orchyn_platform_web::WebAdapter;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "orchyn-check", about = "Grounded checks for a running web app (no model)")]
struct Args {
    /// Base URL of the running app (required unless only `--lint-spec` is given).
    #[arg(long)]
    url: Option<String>,
    /// Task spec to lint for ambiguous phrases (repeatable).
    #[arg(long = "lint-spec", value_name = "FILE")]
    lint_spec: Vec<PathBuf>,
    /// Project source directory (for code intent and contracts).
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Replay manifest(s) to run.
    #[arg(long)]
    manifest: Vec<PathBuf>,
    /// Run universal invariants (console clean, no failed requests, all
    /// controls clickable, reload stable, interactions clean) on each page.
    #[arg(long)]
    universal: bool,
    /// Print the invariants the code declares.
    #[arg(long)]
    contracts: bool,
}

fn factory() -> AdapterFactoryFn {
    Arc::new(|_actor: String| {
        async move {
            let a = tokio::task::spawn_blocking(|| {
                tokio::runtime::Handle::current().block_on(WebAdapter::new("about:blank"))
            })
            .await
            .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))??;
            Ok(Box::new(a) as Box<dyn PlatformAdapter>)
        }
        .boxed()
    })
}

fn print_report(r: &ReplayReport) {
    println!(
        "{} {} ({}/{} steps, {} ms)",
        if r.passed { "PASS" } else { "FAIL" },
        r.name,
        r.steps.len(),
        r.total_steps,
        r.duration_ms
    );
    for s in &r.steps {
        if !s.passed || !r.passed {
            println!(
                "  {} #{} [{}] {}: {}",
                if s.passed { "ok " } else { "ERR" },
                s.index,
                s.actor,
                s.op,
                s.detail
            );
        }
    }
    if !r.passed && r.first_failure.map(|i| i + 1 < r.total_steps).unwrap_or(false) {
        println!("  (halted; later steps not run)");
    }
}

/// Print the ambiguities of one spec file. Informational only.
fn print_lint(path: &PathBuf) -> Result<()> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    println!("== spec ambiguities ({})", path.display());
    let found = invariants::ambiguity::lint(&text);
    if found.is_empty() {
        println!("none");
    }
    for a in &found {
        println!("L{} [{}] {}", a.line, a.kind, a.excerpt);
        println!("    ? {}", a.question);
        println!("    readings: {}", a.readings.join(" | "));
    }
    println!();
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    for path in &args.lint_spec {
        print_lint(path)?;
    }
    let Some(url) = args.url.clone() else {
        if !args.manifest.is_empty() || args.universal || args.contracts {
            anyhow::bail!("--url is required to run manifests, --universal or --contracts");
        }
        return Ok(());
    };
    let runner = || ReplayRunner::new(factory()).with_base_url(url.clone());
    let code = intent::extract_intent(&args.project);
    let mut all_ok = true;
    let mut observed = Vec::new();

    for path in &args.manifest {
        let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let m = ReplayManifest::from_json(&text)
            .with_context(|| format!("{} is not a valid replay manifest", path.display()))?;
        let r = runner().run(&m).await?;
        print_report(&r);
        all_ok &= r.passed;
        observed.extend(r.observed_requests);
    }

    if args.universal {
        println!("\n== universal invariants");
        for (cand, m) in invariants::universal::manifests(&code) {
            let r = runner().run(&m).await?;
            println!("{} {}", if r.passed { "PASS" } else { "FAIL" }, cand.statement);
            if !r.passed {
                if let Some(s) = r.first_failure.and_then(|i| r.steps.iter().find(|s| s.index == i)) {
                    println!("  {}: {}", s.op, s.detail);
                }
                // Lower-confidence universal rules are advisory.
                if cand.confidence >= 0.9 {
                    all_ok = false;
                } else {
                    println!("  (advisory: confidence {:.1})", cand.confidence);
                }
            }
            observed.extend(r.observed_requests);
        }
    }

    let gaps: Vec<_> = intent::reconcile(&code, &observed)
        .into_iter()
        .filter(|g| g.kind != GapKind::UnexercisedClientCall)
        .collect();
    println!("\n== reality gaps (code intent vs observed traffic)");
    if gaps.is_empty() {
        println!("none");
    }
    for g in &gaps {
        println!("{:?}: {}", g.kind, g.message);
    }

    if args.contracts {
        println!("\n== invariants declared by the code");
        for c in invariants::contracts::extract(&args.project) {
            let at = c
                .evidence
                .first()
                .and_then(|e| e.file.as_ref().map(|f| format!(" ({}:{})", f.display(), e.line.unwrap_or(0))))
                .unwrap_or_default();
            println!("- {}{}", c.statement, at);
        }
    }

    std::process::exit(if all_ok { 0 } else { 1 });
}
