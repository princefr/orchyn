//! Modular system prompt assembly, mirroring claurst's
//! `core::system_prompt` architecture but adapted to the orchyn convergence
//! loop (visual + console + runtime + static analysis + tests).
//!
//! The prompt is composed from named sections so each one can be edited /
//! tested independently. The cacheable / dynamic split is preserved (Gemma
//! doesn't currently honor it, but it keeps us API-portable).

use orchyn_model_client::ToolDefinition;

/// Marker between cacheable and per-turn sections. We keep it for
/// API-portability — a future API-backed model can prompt-cache the prefix.
pub const SYSTEM_PROMPT_DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

/// Per-turn dynamic context fed into the prompt.
pub struct DynamicContext<'a> {
    pub step: usize,
    pub max_steps: usize,
    pub initial_sha: Option<&'a str>,
    pub modified_files: &'a [String],
    pub evidence_snapshot: &'a str,
    pub working_directory: Option<&'a str>,
    pub goal: Option<&'a str>,
}

/// Build the full system prompt + tool catalog + per-turn block. Returns the
/// finished Gemma 4 chat-template string ready to feed `complete()`.
pub fn build_prompt(tools: &[ToolDefinition], ctx: &DynamicContext<'_>) -> String {
    let mut parts: Vec<String> = Vec::new();

    // ── cacheable sections ──
    parts.push(ROLE.to_string());
    parts.push(CAPABILITIES.to_string());
    parts.push(TOOL_USE_GUIDELINES.to_string());
    parts.push(CONVERGENCE_RULES.to_string());
    parts.push(SAFETY_GUIDELINES.to_string());

    // Boundary
    parts.push(SYSTEM_PROMPT_DYNAMIC_BOUNDARY.to_string());

    // ── dynamic / per-turn sections ──
    parts.push(env_info_section());
    if let Some(cwd) = ctx.working_directory {
        parts.push(format!("Working directory: {}", cwd));
    }
    if let Some(goal) = ctx.goal {
        parts.push(format!("Goal: {}", goal));
    }
    parts.push(format!(
        "Step {}/{}. Initial HEAD: {}. Files you've edited so far: {}.",
        ctx.step + 1,
        ctx.max_steps,
        ctx.initial_sha.unwrap_or("<n/a>"),
        if ctx.modified_files.is_empty() {
            "<none>".to_string()
        } else {
            ctx.modified_files.join(", ")
        }
    ));
    parts.push(format!("Current evidence:\n{}", ctx.evidence_snapshot));

    let system = parts.join("\n\n");
    let tool_json = serde_json::to_string(tools).unwrap_or_default();

    // Gemma 4 chat template wrapping. The tool catalog and the "respond with
    // JSON" instruction live in the user turn so the system prefix stays cache-
    // stable.
    format!(
        "<start_of_turn>user\n{}\n\nAvailable tools:\n{}\n\nPick the single most useful next tool call. Return ONLY JSON like `{{\"name\": \"...\", \"arguments\": {{...}}}}`.<end_of_turn>\n<start_of_turn>model\n",
        system, tool_json
    )
}

/// Build env info — OS / shell / date. Mirrors claurst `build_env_info_section`.
fn env_info_section() -> String {
    let os = std::env::consts::OS;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
    let date = chrono::Utc::now().format("%Y-%m-%d");
    format!("Environment:\n  OS: {}\n  Shell: {}\n  Date (UTC): {}", os, shell, date)
}

// ---------------------------------------------------------------------------
// Cacheable section bodies
// ---------------------------------------------------------------------------

const ROLE: &str = "\
You are Orchyn's autonomous fixer agent. Your job is to converge a vibecoded \
application onto a clean evidence state — no static-analysis errors, no \
failing tests, no console / runtime errors, no visual anomalies — by \
selecting one tool call at a time. You debug like an experienced human: you \
look at visuals, console, runtime output, static analysis, and tests \
together, then pick the action that makes the *evidence* cleaner, not just \
the symptom.";

const CAPABILITIES: &str = "\
## Capabilities
- Read & edit source code (read_file, write_file, replace_text, batch_edit, apply_patch).
- Search the codebase (glob, grep, find_symbol).
- Run static analysis (static_analysis: cargo / clippy / eslint / tsc) — output is parsed into structured findings.
- Run tests (run_tests: cargo / playwright / jest / vitest) — pass/fail counts captured automatically.
- Drive the live app (probe_ui: screenshot+VLM, console, dom, click, type, reset) — only when an adapter is wired.
- Inspect git state (git_diff), apply unified diffs (apply_patch, supports dry_run), or reset to the initial sha (git_reset, destructive).
- Track your own plan (todo_write).
- Fetch documentation (web_fetch).
- Inspect what you already know (read_evidence) — cheaper than re-running probes.";

const TOOL_USE_GUIDELINES: &str = "\
## Tool use guidelines
- Prefer dedicated tools over `run_command`: use `static_analysis`, `run_tests`, `glob`, `grep`, `git_diff` rather than shelling out manually.
- Read before you write: call `read_file` (range-bounded) before any `replace_text` / `apply_patch`.
- For `replace_text`, `old_text` must match exactly once unless you set `replace_all=true` — preserve whitespace/indentation.
- For multi-file structural changes, prefer `apply_patch` (with `dry_run=true` first) or `batch_edit` over a sequence of `replace_text` calls.
- Use `read_evidence` when you've drifted; it returns the structured pile cheaper than re-probing.
- Update `todo_write` when your plan changes — it's visible to the human watching the TUI.";

const CONVERGENCE_RULES: &str = "\
## Convergence rules
- The loop ends when (a) `is_clean()` is true: zero static-analysis errors, zero failing tests, zero console/network errors, no panics; or (b) you call `finish`; or (c) the step budget is exhausted.
- Only call `finish` when those conditions are *demonstrably* met: run `static_analysis` and `run_tests` to verify before finishing.
- After every fix, re-run the analyzer that surfaced the error. The loop tracks error count and will flag REGRESSION notes when fixes make things worse.
- If you see a REGRESSION note, consider `git_reset` to the initial sha and try a different angle.";

const SAFETY_GUIDELINES: &str = "\
## Safety
- `git_reset` is destructive — only use it on a clear regression, never speculatively.
- Don't write secrets, credentials, or API keys to disk.
- Don't modify files outside the working directory (`.gitconfig`, `.bashrc`, `.zshrc`, host-level configs).
- For ambiguous destructive shell commands (rm -rf, DROP TABLE, force pushes), prefer asking the user — but in autonomous mode, refuse them silently rather than guess.";

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> DynamicContext<'static> {
        DynamicContext {
            step: 0,
            max_steps: 30,
            initial_sha: Some("abcd1234"),
            modified_files: &[],
            evidence_snapshot: "(none)",
            working_directory: None,
            goal: None,
        }
    }

    #[test]
    fn prompt_contains_all_cacheable_sections() {
        let p = build_prompt(&[], &ctx());
        assert!(p.contains("Orchyn's autonomous fixer agent"));
        assert!(p.contains("## Capabilities"));
        assert!(p.contains("## Tool use guidelines"));
        assert!(p.contains("## Convergence rules"));
        assert!(p.contains("## Safety"));
    }

    #[test]
    fn prompt_contains_dynamic_boundary() {
        let p = build_prompt(&[], &ctx());
        assert!(p.contains(SYSTEM_PROMPT_DYNAMIC_BOUNDARY));
    }

    #[test]
    fn prompt_includes_step_budget_and_sha() {
        let p = build_prompt(&[], &ctx());
        assert!(p.contains("Step 1/30"));
        assert!(p.contains("abcd1234"));
    }

    #[test]
    fn prompt_includes_env_info() {
        let p = build_prompt(&[], &ctx());
        assert!(p.contains("Environment:"));
        assert!(p.contains("OS:"));
    }

    #[test]
    fn prompt_ends_with_model_turn_marker() {
        let p = build_prompt(&[], &ctx());
        assert!(p.contains("<start_of_turn>model"));
    }
}
