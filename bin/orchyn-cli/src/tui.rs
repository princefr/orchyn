//! Streaming terminal renderer — displays AgentProgressEvents as they arrive,
//! similar to how Claude Code shows tool calls, spinner, and results live.

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use orchyn_harness::AgentProgressEvent;
use std::time::Duration;
use tokio::sync::broadcast;

/// Spawn a background task that renders progress events to stdout.
/// Returns a JoinHandle that resolves when `RunComplete` is received or
/// the channel closes.
pub fn start(
    mut rx: broadcast::Receiver<AgentProgressEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
        );

        loop {
            match rx.recv().await {
                Ok(event) => {
                    if !handle_event(&spinner, event) {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    spinner.println(format!("{} {n} events dropped", "[TUI lag]".yellow()));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }

        spinner.finish_and_clear();
    })
}

/// Returns false when the run is complete (caller should stop listening).
fn handle_event(sp: &ProgressBar, event: AgentProgressEvent) -> bool {
    match event {
        AgentProgressEvent::TurnStart { turn, url } => {
            sp.finish_and_clear();
            let divider = "─".repeat(50);
            sp.println(format!(
                "\n{} {} {}",
                format!("── Turn {turn}").cyan().bold(),
                divider.dimmed(),
                ""
            ));
            if !url.is_empty() && url != "about:blank" {
                sp.println(format!("   {} {}", "url".dimmed(), url.cyan()));
            }
        }

        AgentProgressEvent::LlmThinking { .. } => {
            sp.set_message("Thinking...");
            sp.enable_steady_tick(Duration::from_millis(80));
        }

        AgentProgressEvent::LlmResponse { reasoning, tool_names, .. } => {
            sp.finish_and_clear();
            if !reasoning.is_empty() {
                sp.println(format!("   {}", reasoning.italic().truecolor(150, 150, 150)));
            }
            if !tool_names.is_empty() {
                sp.println(format!(
                    "   {} {}",
                    "→".blue().bold(),
                    tool_names.join(", ").bold()
                ));
            }
        }

        AgentProgressEvent::ToolCalling { name, args, .. } => {
            let compact = compact_args(&args);
            sp.println(format!(
                "   {} {}  {}",
                "⟶".blue(),
                name.bold(),
                compact.truecolor(120, 120, 120)
            ));
        }

        AgentProgressEvent::ToolResult { success, preview, .. } => {
            let preview = preview.chars().take(100).collect::<String>();
            if success {
                sp.println(format!(
                    "   {} {}",
                    "✓".green(),
                    preview.truecolor(130, 180, 130)
                ));
            } else {
                sp.println(format!("   {} {}", "✗".red(), preview.red()));
            }
        }

        AgentProgressEvent::GapFound { description, severity, .. } => {
            sp.finish_and_clear();
            sp.println(format!(
                "   {} {}  [{}]",
                "⚠ gap".yellow().bold(),
                description.yellow(),
                severity.truecolor(200, 150, 50)
            ));
        }

        AgentProgressEvent::TestRecorded { key, passed, .. } => {
            let icon = if passed { "✓".green() } else { "✗".red() };
            sp.println(format!("   {} {}", icon, key.dimmed()));
        }

        AgentProgressEvent::RunComplete { completed, turns_used, gaps_found } => {
            sp.finish_and_clear();
            let status = if completed {
                "completed".green().bold()
            } else {
                "max turns reached".yellow().bold()
            };
            println!(
                "\n{}  {}  turns: {}  gaps: {}",
                "──────────────────────────────────".dimmed(),
                status,
                turns_used.to_string().bold(),
                gaps_found.to_string().bold()
            );
            return false;
        }
    }
    true
}

/// Compact JSON args for single-line display — skip large or binary values.
fn compact_args(args: &serde_json::Value) -> String {
    let obj = match args.as_object() {
        Some(o) => o,
        None => return args.to_string().chars().take(80).collect(),
    };
    let parts: Vec<String> = obj
        .iter()
        .map(|(k, v)| {
            let val = match v {
                serde_json::Value::String(s) if s.len() > 60 => {
                    format!("\"{}…\"", &s[..57])
                }
                serde_json::Value::String(s) => format!("\"{s}\""),
                other => other.to_string().chars().take(40).collect(),
            };
            format!("{k}:{val}")
        })
        .collect();
    format!("{{ {} }}", parts.join("  "))
}

/// Print a formatted diff report to stdout.
pub fn print_diff_report(report: &orchyn_harness::DiffReport) {
    println!("\n{}", "══════════════════════════════════════════════════".cyan().bold());
    println!("{}", "  ORCHYN DIFF REPORT".cyan().bold());
    println!("{}", "══════════════════════════════════════════════════".cyan().bold());
    println!(
        "  {} {}  →  {} {}",
        report.ref_a.bold(),
        format!("({})", &report.commit_a[..7]).dimmed(),
        report.ref_b.bold(),
        format!("({})", &report.commit_b[..7]).dimmed(),
    );

    // Code diff summary
    if !report.code_diff.is_empty() {
        let lines: usize = report.code_diff.lines().count();
        let added = report.code_diff.lines().filter(|l| l.starts_with('+')).count();
        let removed = report.code_diff.lines().filter(|l| l.starts_with('-')).count();
        println!(
            "\n  {} {} lines  (+{} -{}):",
            "code diff".bold(),
            lines,
            added.to_string().green(),
            removed.to_string().red()
        );
        for line in report.code_diff.lines().take(30) {
            if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
                println!("  {}", line.truecolor(100, 100, 180));
            } else if line.starts_with('+') {
                println!("  {}", line.green());
            } else if line.starts_with('-') {
                println!("  {}", line.red());
            } else {
                println!("  {}", line.dimmed());
            }
        }
        if report.code_diff.lines().count() > 30 {
            println!("  {}", "  … (truncated)".dimmed());
        }
    } else {
        println!("\n  {} (no code changes between refs)", "code diff:".bold());
    }

    // Gap sections
    print_gap_section(
        "✗  REGRESSIONS",
        &report.new_gaps,
        "red",
    );
    print_gap_section(
        "✓  IMPROVEMENTS",
        &report.fixed_gaps,
        "green",
    );
    print_gap_section(
        "~  UNCHANGED",
        &report.unchanged_gaps,
        "yellow",
    );

    // Summary verdict
    println!();
    if report.regression {
        println!(
            "  {}  {} new regression(s) introduced by this commit.",
            "REGRESSION DETECTED".red().bold(),
            report.new_gaps.len()
        );
    } else {
        println!("  {}", "NO REGRESSION — safe to ship.".green().bold());
    }
    println!("{}", "══════════════════════════════════════════════════".cyan().bold());
}

fn print_gap_section(label: &str, gaps: &[orchyn_harness::diff_engine::GapEntry], color: &str) {
    if gaps.is_empty() {
        return;
    }
    let header = match color {
        "red" => format!("\n  {} ({}):", label, gaps.len()).red().bold().to_string(),
        "green" => format!("\n  {} ({}):", label, gaps.len()).green().bold().to_string(),
        _ => format!("\n  {} ({}):", label, gaps.len()).yellow().bold().to_string(),
    };
    println!("{header}");
    for gap in gaps {
        println!("    •  {}", gap.description);
    }
}
