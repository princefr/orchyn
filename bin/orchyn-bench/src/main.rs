//! `orchyn-bench`: score replay manifests against bug-seeded fixtures.
//!
//! For every fixture in the suite it starts the app twice — clean and with the
//! planted bug (`BUG=1`) — runs the manifest `--runs` times against each, and
//! records:
//!
//! * **detected**: the manifest failed on every buggy run,
//! * **false positive**: the manifest failed on any clean run,
//! * **flaky**: verdicts differ across runs of the same variant,
//! * the reality gaps found by reconciling static code intent with the network
//!   traffic actually observed.
//!
//! No model is involved at run time: whatever wrote the manifest (a human, a
//! frontier model, or Orchyn's agent mode), the verdict is deterministic.

use anyhow::{Context, Result, bail};
use clap::Parser;
use futures::FutureExt;
use orchyn_core::PlatformAdapter;
use orchyn_harness::intent::{self, CodeIntent, RealityGap};
use orchyn_harness::replay::{AdapterFactoryFn, ReplayManifest, ReplayReport, ReplayRunner};
use orchyn_platform_web::WebAdapter;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

mod modes;

#[derive(Parser, Debug)]
#[command(name = "orchyn-bench", about = "Score replay manifests against bug-seeded fixtures")]
pub(crate) struct Args {
    /// Suite file (JSON array of fixtures).
    #[arg(long, default_value = "bench/suite.json")]
    suite: PathBuf,
    /// Directory holding `<fixture>.json` manifests. Defaults to each suite
    /// entry's own `manifest` path (the hand-written reference set).
    #[arg(long)]
    manifests: Option<PathBuf>,
    /// Runs per (fixture, variant).
    #[arg(long, default_value_t = 3)]
    runs: usize,
    /// Only run fixtures whose name contains this substring.
    #[arg(long)]
    only: Option<String>,
    /// Label for this condition (e.g. "reference", "haiku-blind").
    #[arg(long, default_value = "reference")]
    label: String,
    /// Write the full JSON results here.
    #[arg(long)]
    out: Option<PathBuf>,
    /// manifest: replay manifests (default). universal: spec-free universal
    /// invariants generated from the code. mined: mine invariants from clean
    /// runs of `--manifests` (the driver) and check buggy runs against them.
    /// invariants: write every invariant candidate per fixture to `--out` dir.
    #[arg(long, default_value = "manifest")]
    mode: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Fixture {
    name: String,
    dir: String,
    bug_class: String,
    bug_description: String,
    manifest: String,
}

#[derive(Debug, Clone, Serialize)]
struct RunRecord {
    variant: &'static str,
    run: usize,
    passed: bool,
    first_failure: Option<String>,
    duration_ms: u64,
    gaps: Vec<RealityGap>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct FixtureResult {
    name: String,
    bug_class: String,
    bug_description: String,
    manifest: Option<String>,
    steps: usize,
    detected: bool,
    false_positive: bool,
    flaky: bool,
    gap_detected: bool,
    runs: Vec<RunRecord>,
}

#[derive(Debug, Serialize)]
struct Summary {
    label: String,
    fixtures: usize,
    runs_per_variant: usize,
    detected: usize,
    false_positives: usize,
    flaky: usize,
    gap_detected: usize,
    mean_run_ms: u64,
    results: Vec<FixtureResult>,
}

fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(l.local_addr()?.port())
}

pub(crate) async fn start_fixture(dir: &Path, bug: bool) -> Result<(Child, String)> {
    let port = free_port()?;
    let mut child = Command::new("node")
        .arg("server.js")
        .current_dir(dir)
        .env("PORT", port.to_string())
        .env("BUG", if bug { "1" } else { "0" })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawn node in {}", dir.display()))?;
    let stdout = child.stdout.take().context("no stdout")?;
    let mut lines = BufReader::new(stdout).lines();
    let ready = tokio::time::timeout(Duration::from_secs(10), async {
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains("listening") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    if !ready {
        bail!("fixture {} did not report 'listening' within 10s", dir.display());
    }
    // Keep draining stdout so the child never blocks on a full pipe.
    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
    Ok((child, format!("http://127.0.0.1:{port}")))
}

pub(crate) fn web_factory() -> AdapterFactoryFn {
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

async fn run_once(
    fixture_dir: &Path,
    manifest: &ReplayManifest,
    intent: &CodeIntent,
    bug: bool,
    run: usize,
) -> RunRecord {
    let variant = if bug { "bug" } else { "clean" };
    let started = Instant::now();
    let outcome: Result<ReplayReport> = async {
        let (mut child, base) = start_fixture(fixture_dir, bug).await?;
        let report = ReplayRunner::new(web_factory())
            .with_base_url(base)
            .run(manifest)
            .await;
        let _ = child.kill().await;
        Ok(report?)
    }
    .await;
    match outcome {
        Ok(report) => {
            let first_failure = report.first_failure.and_then(|i| {
                report
                    .steps
                    .iter()
                    .find(|s| s.index == i)
                    .map(|s| format!("#{} {} [{}]: {}", s.index, s.op, s.actor, s.detail))
            });
            RunRecord {
                variant,
                run,
                passed: report.passed,
                first_failure,
                duration_ms: report.duration_ms,
                gaps: intent::reconcile(intent, &report.observed_requests)
                    .into_iter()
                    .filter(|g| g.kind != intent::GapKind::UnexercisedClientCall)
                    .collect(),
                error: None,
            }
        }
        Err(e) => RunRecord {
            variant,
            run,
            passed: false,
            first_failure: None,
            duration_ms: started.elapsed().as_millis() as u64,
            gaps: vec![],
            error: Some(format!("{e:#}")),
        },
    }
}

fn verdicts_differ(runs: &[&RunRecord]) -> bool {
    runs.windows(2).any(|w| w[0].passed != w[1].passed)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let suite_dir = args
        .suite
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let fixtures: Vec<Fixture> = serde_json::from_str(
        &std::fs::read_to_string(&args.suite).with_context(|| format!("read {}", args.suite.display()))?,
    )?;

    match args.mode.as_str() {
        "manifest" => {}
        "universal" => return modes::universal(&args, &suite_dir, &fixtures).await,
        "mined" => return modes::mined(&args, &suite_dir, &fixtures).await,
        "invariants" => return modes::invariants(&args, &suite_dir, &fixtures).await,
        other => bail!("unknown --mode {other}"),
    }

    let mut results = Vec::new();
    for fx in fixtures {
        if let Some(only) = &args.only {
            if !fx.name.contains(only.as_str()) {
                continue;
            }
        }
        let fixture_dir = suite_dir.join(&fx.dir);
        let manifest_path = match &args.manifests {
            Some(d) => d.join(format!("{}.json", fx.name)),
            None => suite_dir.join(&fx.manifest),
        };
        let intent = intent::extract_intent(&fixture_dir);
        let manifest = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| ReplayManifest::from_json(&s).ok());

        let Some(manifest) = manifest else {
            eprintln!("[{}] no valid manifest at {}", fx.name, manifest_path.display());
            results.push(FixtureResult {
                name: fx.name,
                bug_class: fx.bug_class,
                bug_description: fx.bug_description,
                manifest: None,
                steps: 0,
                detected: false,
                false_positive: false,
                flaky: false,
                gap_detected: false,
                runs: vec![],
            });
            continue;
        };

        let mut runs = Vec::new();
        for r in 0..args.runs {
            for bug in [false, true] {
                let rec = run_once(&fixture_dir, &manifest, &intent, bug, r).await;
                eprintln!(
                    "[{}] {:5} run {}: {} {}ms {}",
                    fx.name,
                    rec.variant,
                    r,
                    if rec.passed { "PASS" } else { "FAIL" },
                    rec.duration_ms,
                    rec.first_failure.as_deref().or(rec.error.as_deref()).unwrap_or("")
                );
                runs.push(rec);
            }
        }
        let clean: Vec<&RunRecord> = runs.iter().filter(|r| r.variant == "clean").collect();
        let bug: Vec<&RunRecord> = runs.iter().filter(|r| r.variant == "bug").collect();
        let clean_gaps = clean.iter().map(|r| r.gaps.len()).max().unwrap_or(0);
        results.push(FixtureResult {
            name: fx.name,
            bug_class: fx.bug_class,
            bug_description: fx.bug_description,
            manifest: Some(manifest_path.display().to_string()),
            steps: manifest.steps.len(),
            detected: bug.iter().all(|r| !r.passed && r.error.is_none()),
            false_positive: clean.iter().any(|r| !r.passed),
            flaky: verdicts_differ(&clean) || verdicts_differ(&bug),
            gap_detected: bug.iter().all(|r| r.gaps.len() > clean_gaps),
            runs,
        });
    }

    let all_runs: Vec<&RunRecord> = results.iter().flat_map(|f| f.runs.iter()).collect();
    let summary = Summary {
        label: args.label.clone(),
        fixtures: results.len(),
        runs_per_variant: args.runs,
        detected: results.iter().filter(|f| f.detected).count(),
        false_positives: results.iter().filter(|f| f.false_positive).count(),
        flaky: results.iter().filter(|f| f.flaky).count(),
        gap_detected: results.iter().filter(|f| f.gap_detected).count(),
        mean_run_ms: if all_runs.is_empty() {
            0
        } else {
            all_runs.iter().map(|r| r.duration_ms).sum::<u64>() / all_runs.len() as u64
        },
        results,
    };

    println!("\n## {} ({} runs per variant)\n", summary.label, summary.runs_per_variant);
    println!("| fixture | class | steps | bug caught | false positive | flaky | network gap | failing step on bug |");
    println!("|---|---|---|---|---|---|---|---|");
    for f in &summary.results {
        let why = f
            .runs
            .iter()
            .find(|r| r.variant == "bug")
            .and_then(|r| r.first_failure.clone().or(r.error.clone()))
            .unwrap_or_else(|| "-".into())
            .replace('|', "\\|");
        let why: String = why.chars().take(90).collect();
        let yn = |b: bool| if b { "yes" } else { "no" };
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            f.name,
            f.bug_class,
            f.steps,
            if f.manifest.is_none() { "n/a" } else { yn(f.detected) },
            yn(f.false_positive),
            yn(f.flaky),
            yn(f.gap_detected),
            why
        );
    }
    println!(
        "\n**Caught {}/{}** · false positives {} · flaky {} · network-gap signal {} · mean run {} ms",
        summary.detected,
        summary.fixtures,
        summary.false_positives,
        summary.flaky,
        summary.gap_detected,
        summary.mean_run_ms
    );

    if let Some(out) = &args.out {
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(out, serde_json::to_string_pretty(&summary)?)?;
        eprintln!("wrote {}", out.display());
    }
    Ok(())
}
