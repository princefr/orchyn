//! Invariant-source benchmark modes.
//!
//! * `universal`: no manifest, no spec. Candidates come from
//!   `invariants::universal` (derived from the server's page routes); each is
//!   replayed on clean and buggy builds. A candidate is *valid* if it holds on
//!   every clean run; a fixture is caught if some valid candidate fails on
//!   every buggy run.
//! * `mined`: drive the clean app with a manifest (`--manifests`), mine
//!   invariants from the API exchanges of all but the last clean run, then
//!   check the held-out clean run (false positives) and the buggy runs.
//! * `invariants`: write every candidate from every source for each fixture.

use super::{Args, Fixture, start_fixture, web_factory};
use anyhow::{Context, Result};
use orchyn_core::Exchange;
use orchyn_harness::replay::PageSnapshot;
use orchyn_harness::intent;
use orchyn_harness::invariants::{self, InvariantCandidate, mined, pages};
use orchyn_harness::replay::{ReplayManifest, ReplayReport, ReplayRunner};
use serde::Serialize;
use std::path::Path;

async fn replay(dir: &Path, manifest: &ReplayManifest, bug: bool) -> Result<ReplayReport> {
    let (mut child, base) = start_fixture(dir, bug).await?;
    let report = ReplayRunner::new(web_factory())
        .with_base_url(base)
        .run(manifest)
        .await;
    let _ = child.kill().await;
    Ok(report?)
}

fn selected<'a>(args: &'a Args, fixtures: &'a [Fixture]) -> impl Iterator<Item = &'a Fixture> {
    fixtures
        .iter()
        .filter(move |f| args.only.as_ref().is_none_or(|o| f.name.contains(o.as_str())))
}

fn write_out<T: Serialize>(args: &Args, value: &T) -> Result<()> {
    if let Some(out) = &args.out {
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(out, serde_json::to_string_pretty(value)?)?;
        eprintln!("wrote {}", out.display());
    }
    Ok(())
}

fn first_failure(r: &ReplayReport) -> String {
    r.first_failure
        .and_then(|i| r.steps.iter().find(|s| s.index == i))
        .map(|s| format!("#{} {}: {}", s.index, s.op, s.detail))
        .unwrap_or_default()
}

// ── universal ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CandidateOutcome {
    id: String,
    statement: String,
    clean_passed: usize,
    bug_failed: usize,
    runs: usize,
    valid: bool,
    fired: bool,
    bug_failure: String,
    clean_failure: String,
}

#[derive(Serialize)]
struct UniversalFixture {
    name: String,
    bug_class: String,
    caught: bool,
    fired_by: Vec<String>,
    invalid: Vec<String>,
    candidates: Vec<CandidateOutcome>,
}

pub(crate) async fn universal(args: &Args, suite_dir: &Path, fixtures: &[Fixture]) -> Result<()> {
    let mut out = Vec::new();
    for fx in selected(args, fixtures) {
        let dir = suite_dir.join(&fx.dir);
        let code = intent::extract_intent(&dir);
        let mut outcomes = Vec::new();
        for (cand, manifest) in invariants::universal::manifests(&code) {
            let (mut clean_passed, mut bug_failed) = (0, 0);
            let (mut bug_failure, mut clean_failure) = (String::new(), String::new());
            for _ in 0..args.runs {
                match replay(&dir, &manifest, false).await {
                    Ok(r) if r.passed => clean_passed += 1,
                    Ok(r) => clean_failure = first_failure(&r),
                    Err(e) => clean_failure = format!("{e:#}"),
                }
                match replay(&dir, &manifest, true).await {
                    Ok(r) if !r.passed => {
                        bug_failed += 1;
                        bug_failure = first_failure(&r);
                    }
                    Ok(_) => {}
                    Err(e) => bug_failure = format!("error: {e:#}"),
                }
            }
            let valid = clean_passed == args.runs;
            let fired = valid && bug_failed == args.runs;
            eprintln!(
                "[{}] {:45} clean {}/{} bug-fail {}/{} {}",
                fx.name,
                cand.id,
                clean_passed,
                args.runs,
                bug_failed,
                args.runs,
                if fired { "FIRED" } else if !valid { "invalid" } else { "" }
            );
            outcomes.push(CandidateOutcome {
                id: cand.id,
                statement: cand.statement,
                clean_passed,
                bug_failed,
                runs: args.runs,
                valid,
                fired,
                bug_failure,
                clean_failure,
            });
        }
        out.push(UniversalFixture {
            name: fx.name.clone(),
            bug_class: fx.bug_class.clone(),
            caught: outcomes.iter().any(|c| c.fired),
            fired_by: outcomes.iter().filter(|c| c.fired).map(|c| c.id.clone()).collect(),
            invalid: outcomes.iter().filter(|c| !c.valid).map(|c| c.id.clone()).collect(),
            candidates: outcomes,
        });
    }

    println!("\n## universal invariants, no spec ({} runs per variant)\n", args.runs);
    println!("| fixture | class | caught | fired by | invalid on clean app |");
    println!("|---|---|---|---|---|");
    for f in &out {
        let short = |v: &[String]| {
            if v.is_empty() {
                "-".to_string()
            } else {
                v.iter()
                    .map(|id| id.split(':').nth(1).unwrap_or(id).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        println!(
            "| {} | {} | {} | {} | {} |",
            f.name,
            f.bug_class,
            if f.caught { "✓" } else { "missed" },
            short(&f.fired_by),
            short(&f.invalid)
        );
    }
    println!(
        "\n**Caught {}/{}** with no spec and no hand-written test.",
        out.iter().filter(|f| f.caught).count(),
        out.len()
    );
    write_out(args, &out)
}

// ── mined ────────────────────────────────────────────────────────────────────


#[derive(Serialize)]
struct MinedFixture {
    name: String,
    bug_class: String,
    invariants: Vec<mined::Mined>,
    clean_violations: Vec<mined::Violation>,
    bug_violations: Vec<Vec<mined::Violation>>,
    caught: bool,
    false_positive: bool,
    page_invariants: Vec<pages::PageInvariant>,
    page_clean_violations: Vec<pages::PageViolation>,
    page_bug_violations: Vec<Vec<pages::PageViolation>>,
    page_caught: bool,
    page_false_positive: bool,
    /// Union of API and page invariants.
    any_caught: bool,
    any_false_positive: bool,
}

pub(crate) async fn mined(args: &Args, suite_dir: &Path, fixtures: &[Fixture]) -> Result<()> {
    let driver_dir = args
        .manifests
        .clone()
        .context("--mode mined needs --manifests <driver manifest dir>")?;
    let train = args.runs.max(2);
    let mut out = Vec::new();
    for fx in selected(args, fixtures) {
        let dir = suite_dir.join(&fx.dir);
        let path = driver_dir.join(format!("{}.json", fx.name));
        let Some(manifest) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| ReplayManifest::from_json(&s).ok())
        else {
            eprintln!("[{}] no driver manifest at {}", fx.name, path.display());
            continue;
        };
        let manifest = manifest.actions_only();
        // train + 1 held-out clean trace
        let mut clean: Vec<Vec<Exchange>> = Vec::new();
        let mut clean_pages: Vec<Vec<PageSnapshot>> = Vec::new();
        for _ in 0..=train {
            let r = replay(&dir, &manifest, false).await?;
            clean.push(r.observed_exchanges);
            clean_pages.push(r.page_snapshots);
        }
        let held_out = clean.pop().unwrap_or_default();
        let held_out_pages = clean_pages.pop().unwrap_or_default();
        let invs = mined::mine(&clean);
        let page_invs = pages::mine(&clean_pages);
        let clean_violations = mined::check(&invs, &held_out);
        let page_clean_violations = pages::check(&page_invs, &held_out_pages);
        let mut bug_violations = Vec::new();
        let mut page_bug_violations = Vec::new();
        for _ in 0..args.runs {
            let r = replay(&dir, &manifest, true).await?;
            bug_violations.push(mined::check(&invs, &r.observed_exchanges));
            page_bug_violations.push(pages::check(&page_invs, &r.page_snapshots));
        }
        let caught = !invs.is_empty() && bug_violations.iter().all(|v| !v.is_empty());
        let page_caught =
            !page_invs.is_empty() && page_bug_violations.iter().all(|v| !v.is_empty());
        let any_caught = bug_violations
            .iter()
            .zip(&page_bug_violations)
            .all(|(a, p)| !a.is_empty() || !p.is_empty());
        let any_false_positive = !clean_violations.is_empty() || !page_clean_violations.is_empty();
        eprintln!(
            "[{}] api: {} invariants, clean violations {}, bug {:?} | pages: {} invariants, clean violations {}, bug {:?}",
            fx.name,
            invs.len(),
            clean_violations.len(),
            bug_violations.iter().map(Vec::len).collect::<Vec<_>>(),
            page_invs.len(),
            page_clean_violations.len(),
            page_bug_violations.iter().map(Vec::len).collect::<Vec<_>>()
        );
        out.push(MinedFixture {
            name: fx.name.clone(),
            bug_class: fx.bug_class.clone(),
            false_positive: !clean_violations.is_empty(),
            caught,
            invariants: invs,
            clean_violations,
            bug_violations,
            page_false_positive: !page_clean_violations.is_empty(),
            page_caught,
            page_invariants: page_invs,
            page_clean_violations,
            page_bug_violations,
            any_caught,
            any_false_positive,
        });
    }

    println!(
        "\n## invariants mined from {} clean runs, checked on buggy runs ({} runs)\n",
        train, args.runs
    );
    println!("| fixture | class | API mined | API | page mined | page | combined | first page violation on bug |");
    println!("|---|---|---|---|---|---|---|---|");
    let verdict = |caught: bool, fp: bool, n: usize| {
        if n == 0 {
            "n/a"
        } else if fp {
            "✗ clean violates"
        } else if caught {
            "✓"
        } else {
            "missed"
        }
    };
    for f in &out {
        let first = f
            .page_bug_violations
            .first()
            .and_then(|v| v.first())
            .map(|v| {
                let inv = serde_json::to_value(&v.invariant).unwrap_or_default();
                let what = inv
                    .get("line")
                    .or_else(|| inv.get("element"))
                    .or_else(|| inv.get("url_path"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("?")
                    .to_string();
                let what: String = what.chars().take(40).collect();
                format!(
                    "{} [{}@{}] {} → {}",
                    inv["kind"].as_str().unwrap_or("?"),
                    inv["actor"].as_str().unwrap_or("?"),
                    inv["step"],
                    what,
                    v.observed
                )
            })
            .unwrap_or_else(|| "-".into())
            .replace('|', "\\|");
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            f.name,
            f.bug_class,
            f.invariants.len(),
            verdict(f.caught, f.false_positive, f.invariants.len()),
            f.page_invariants.len(),
            verdict(f.page_caught, f.page_false_positive, f.page_invariants.len()),
            verdict(f.any_caught, f.any_false_positive, f.invariants.len() + f.page_invariants.len()),
            first
        );
    }
    let count = |pred: &dyn Fn(&MinedFixture) -> bool| out.iter().filter(|f| pred(f)).count();
    println!(
        "\n**Valid catches** — API {}/{} · pages {}/{} · combined {}/{} (false positives: API {}, pages {}).",
        count(&|f| f.caught && !f.false_positive),
        out.len(),
        count(&|f| f.page_caught && !f.page_false_positive),
        out.len(),
        count(&|f| f.any_caught && !f.any_false_positive),
        out.len(),
        count(&|f| f.false_positive),
        count(&|f| f.page_false_positive)
    );
    write_out(args, &out)
}

// ── invariants (dump all sources) ───────────────────────────────────────────

#[derive(Serialize)]
struct FixtureInvariants {
    name: String,
    counts: std::collections::BTreeMap<String, usize>,
    candidates: Vec<InvariantCandidate>,
}

pub(crate) async fn invariants(args: &Args, suite_dir: &Path, fixtures: &[Fixture]) -> Result<()> {
    let out_dir = args
        .out
        .clone()
        .unwrap_or_else(|| suite_dir.join("results/invariants"));
    std::fs::create_dir_all(&out_dir)?;
    for fx in selected(args, fixtures) {
        let dir = suite_dir.join(&fx.dir);
        let code = intent::extract_intent(&dir);
        let mut all = invariants::universal::candidates(&code);
        all.extend(invariants::contracts::extract(&dir));
        let transcript = dir.join("PROMPTS.md");
        if transcript.exists() {
            all.extend(invariants::sources::from_transcript(&transcript)?);
        }
        let prod = dir.join("production.ndjson");
        if prod.exists() {
            all.extend(invariants::sources::from_production_log(&prod)?);
        }
        if let Some(driver_dir) = &args.manifests {
            let path = driver_dir.join(format!("{}.json", fx.name));
            if let Some(m) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| ReplayManifest::from_json(&s).ok())
            {
                let mut traces = Vec::new();
                for _ in 0..args.runs.max(2) {
                    traces.push(replay(&dir, &m, false).await?.observed_exchanges);
                }
                all.extend(mined::to_candidates(&mined::mine(&traces)));
            }
        }
        let mut counts = std::collections::BTreeMap::new();
        for c in &all {
            *counts
                .entry(serde_json::to_value(c.source)?.as_str().unwrap_or("?").to_string())
                .or_insert(0) += 1;
        }
        eprintln!("[{}] {:?}", fx.name, counts);
        let path = out_dir.join(format!("{}.json", fx.name));
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&FixtureInvariants {
                name: fx.name.clone(),
                counts,
                candidates: all,
            })?,
        )?;
    }
    eprintln!("wrote {}", out_dir.display());
    Ok(())
}
