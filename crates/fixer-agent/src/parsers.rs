//! Parsers that turn raw tool output (cargo, clippy, eslint, tsc, playwright)
//! into structured `StaticAnalysisFinding` / `TestRunnerLog` entries the model
//! can reason over without re-reading the noise.

use chrono::Utc;
use orchyn_core::{LogLevel, StaticAnalysisFinding, TestRunnerLog};
use regex::Regex;
use std::path::PathBuf;
use std::sync::OnceLock;

fn rustc_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Matches lines like: `error[E0107]: ...` or `warning: ...` with a following `--> path:line:col`
    R.get_or_init(|| Regex::new(r"^(error|warning|note)(?:\[[^\]]+\])?:\s*(.+)$").unwrap())
}

fn location_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"-->\s*([^:\s]+):(\d+):(\d+)").unwrap())
}

fn eslint_tsc_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // `path:line:col - error TS2345: ...` (tsc) or `path:line:col error  rule` (eslint stylish)
    R.get_or_init(|| {
        Regex::new(r"^([^:\s]+):(\d+):(\d+)\s*-?\s*(error|warning)\s+\S*\s*(.+)$").unwrap()
    })
}

/// Parse `cargo check` / `cargo clippy` combined stdout+stderr into findings.
/// This is deliberately tolerant — any line we can't classify is dropped rather
/// than crashing the loop.
pub fn parse_cargo(tool: &str, output: &str) -> Vec<StaticAnalysisFinding> {
    let mut findings = Vec::new();
    let mut pending: Option<(LogLevel, String)> = None;

    for line in output.lines() {
        let trimmed = line.trim_start();
        if let Some(caps) = rustc_regex().captures(trimmed) {
            let level = match &caps[1] {
                "error" => LogLevel::Error,
                "warning" => LogLevel::Warning,
                _ => LogLevel::Info,
            };
            pending = Some((level, caps[2].to_string()));
            continue;
        }
        if let Some(caps) = location_regex().captures(trimmed) {
            if let Some((level, message)) = pending.take() {
                findings.push(StaticAnalysisFinding {
                    tool: tool.to_string(),
                    level,
                    file: Some(PathBuf::from(&caps[1])),
                    line: caps[2].parse().ok(),
                    message,
                });
            }
        }
    }
    // Flush any dangling diagnostic without a location line.
    if let Some((level, message)) = pending {
        findings.push(StaticAnalysisFinding {
            tool: tool.to_string(),
            level,
            file: None,
            line: None,
            message,
        });
    }
    findings
}

/// Parse eslint/tsc style `path:line:col - level CODE message`.
pub fn parse_eslint_tsc(tool: &str, output: &str) -> Vec<StaticAnalysisFinding> {
    let mut findings = Vec::new();
    for line in output.lines() {
        if let Some(caps) = eslint_tsc_regex().captures(line.trim()) {
            let level = match &caps[4] {
                "error" => LogLevel::Error,
                "warning" => LogLevel::Warning,
                _ => LogLevel::Info,
            };
            findings.push(StaticAnalysisFinding {
                tool: tool.to_string(),
                level,
                file: Some(PathBuf::from(&caps[1])),
                line: caps[2].parse().ok(),
                message: caps[5].to_string(),
            });
        }
    }
    findings
}

/// Parse Playwright reporter lines (`✓`, `✘`). Best-effort.
pub fn parse_playwright(output: &str) -> Vec<TestRunnerLog> {
    let mut logs = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        let (passed, test) = if let Some(rest) = trimmed.strip_prefix("✓") {
            (Some(true), Some(rest.trim().to_string()))
        } else if let Some(rest) = trimmed.strip_prefix("✘") {
            (Some(false), Some(rest.trim().to_string()))
        } else {
            continue;
        };
        logs.push(TestRunnerLog {
            runner: "playwright".to_string(),
            suite: None,
            test,
            passed,
            message: trimmed.to_string(),
            timestamp: Utc::now(),
        });
    }
    logs
}

fn cargo_test_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // `test mod::foo ... ok` / `test mod::foo ... FAILED` / `test mod::foo ... ignored`
    R.get_or_init(|| Regex::new(r"^test\s+(\S+)\s+\.\.\.\s+(ok|FAILED|ignored)").unwrap())
}

/// Parse `cargo test` / `cargo nextest` style output.
pub fn parse_cargo_test(output: &str) -> Vec<TestRunnerLog> {
    let mut logs = Vec::new();
    for line in output.lines() {
        if let Some(caps) = cargo_test_regex().captures(line.trim()) {
            let passed = match &caps[2] {
                "ok" => Some(true),
                "FAILED" => Some(false),
                _ => None,
            };
            logs.push(TestRunnerLog {
                runner: "cargo-test".to_string(),
                suite: None,
                test: Some(caps[1].to_string()),
                passed,
                message: line.trim().to_string(),
                timestamp: Utc::now(),
            });
        }
    }
    logs
}

/// Parse Jest/Vitest minimal text reporter (`PASS path/spec.test.ts`, `FAIL ...`).
pub fn parse_jest(output: &str) -> Vec<TestRunnerLog> {
    let mut logs = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        let (passed, rest) = if let Some(rest) = trimmed.strip_prefix("PASS ") {
            (Some(true), rest)
        } else if let Some(rest) = trimmed.strip_prefix("FAIL ") {
            (Some(false), rest)
        } else {
            continue;
        };
        logs.push(TestRunnerLog {
            runner: "jest".to_string(),
            suite: Some(rest.trim().to_string()),
            test: None,
            passed,
            message: trimmed.to_string(),
            timestamp: Utc::now(),
        });
    }
    logs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cargo_error_with_location() {
        let out = "error[E0107]: expected 2 arguments, found 3\n  --> inference.rs:110:57\n";
        let findings = parse_cargo("cargo", out);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].level, LogLevel::Error));
        assert_eq!(findings[0].line, Some(110));
    }

    #[test]
    fn parses_eslint_line() {
        let out = "src/app.ts:12:5 - error TS2345 Argument of type 'string' is not assignable";
        let findings = parse_eslint_tsc("tsc", out);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("Argument"));
    }

    #[test]
    fn parses_cargo_test_lines() {
        let out = "running 3 tests\n\
            test foo::bar ... ok\n\
            test foo::baz ... FAILED\n\
            test foo::qux ... ignored\n";
        let logs = parse_cargo_test(out);
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[1].passed, Some(false));
        assert_eq!(logs[2].passed, None);
    }

    #[test]
    fn parses_jest_summary() {
        let out = "PASS src/a.test.ts\nFAIL src/b.test.ts\n";
        let logs = parse_jest(out);
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].passed, Some(true));
        assert_eq!(logs[1].passed, Some(false));
    }
}
