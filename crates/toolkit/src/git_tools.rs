//! Git tools: git_log, git_diff, git_show, git_status.

use crate::context::ToolContext;
use crate::file_tools::get_opt_str;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::{OrchynError, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn open_repo(root: &PathBuf) -> std::result::Result<git2::Repository, git2::Error> {
    git2::Repository::open(root)
}

/// Convert a git2::Time into an RFC 3339 string.
fn time_to_rfc3339(t: git2::Time) -> String {
    let offset_secs = t.offset_minutes() * 60;
    let utc_secs = t.seconds() + offset_secs as i64;
    // Build a naive UTC timestamp then apply the fixed offset.
    // We avoid pulling in chrono; simple manual formatting is sufficient.
    let utc = time_secs_to_parts(utc_secs);
    let offset_h = t.offset_minutes() / 60;
    let offset_m = (t.offset_minutes() % 60).abs();
    let sign = if t.offset_minutes() >= 0 { '+' } else { '-' };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
        utc.0, utc.1, utc.2, utc.3, utc.4, utc.5, sign,
        offset_h.unsigned_abs(),
        offset_m.unsigned_abs()
    )
}

/// Returns (year, month, day, hour, min, sec) from a Unix timestamp (UTC).
fn time_secs_to_parts(ts: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Days since epoch
    let (ts, neg) = if ts < 0 { (-ts, true) } else { (ts, false) };
    let _ = neg; // we assume positive timestamps for git commits
    let secs_per_day: i64 = 86400;
    let days = ts / secs_per_day;
    let rem = (ts % secs_per_day) as u32;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;

    // Gregorian calendar conversion from Julian Day Number.
    // epoch (1970-01-01) = JDN 2440588
    let jdn = days + 2440588;
    let a = jdn + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day = (e - (153 * m + 2) / 5 + 1) as u32;
    let month = (m + 3 - 12 * (m / 10)) as u32;
    let year = (100 * b + d - 4800 + m / 10) as i32;
    (year, month, day, hour, min, sec)
}

fn commit_to_json(commit: &git2::Commit) -> Value {
    let hash = commit.id().to_string();
    let short = &hash[..7];
    let message = commit
        .message()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    let author_name = commit.author().name().unwrap_or("").to_string();
    let author_email = commit.author().email().unwrap_or("").to_string();
    let author = format!("{} <{}>", author_name, author_email);
    let date = time_to_rfc3339(commit.author().when());
    json!({
        "hash": hash,
        "short": short,
        "message": message,
        "author": author,
        "date": date,
    })
}

// ---------------------------------------------------------------------------
// git_log
// ---------------------------------------------------------------------------

pub struct GitLogTool;

impl Tool for GitLogTool {
    fn name(&self) -> &str { "git_log" }

    fn description(&self) -> &str {
        "Show recent git commit history. Returns an array of commits with hash, short message, author, and date."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "n": {
                    "type": "integer",
                    "description": "Number of commits to return (default 10, max 50)",
                    "default": 10,
                    "minimum": 1,
                    "maximum": 50
                },
                "path": {
                    "type": "string",
                    "description": "Optional path filter — return only commits that touched this path"
                }
            },
            "required": []
        })
    }

    fn execute<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let n = args.get("n").and_then(|v| v.as_u64()).unwrap_or(10).min(50) as usize;
            let path_filter = get_opt_str(&args, "path").map(|s| s.to_string());
            let root: Arc<PathBuf> = Arc::clone(&ctx.project_root);

            let commits = tokio::task::spawn_blocking(move || -> std::result::Result<Vec<Value>, String> {
                let repo = open_repo(&root).map_err(|e| format!("not a git repo: {e}"))?;
                let mut revwalk = repo.revwalk().map_err(|e| e.to_string())?;
                revwalk.push_head().map_err(|e| format!("no HEAD: {e}"))?;
                revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
                    .map_err(|e| e.to_string())?;

                let mut result = Vec::new();
                for oid in revwalk {
                    if result.len() >= n {
                        break;
                    }
                    let oid = oid.map_err(|e| e.to_string())?;
                    let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;

                    // Apply path filter if requested.
                    if let Some(ref filter_path) = path_filter {
                        let parent_tree = if commit.parent_count() > 0 {
                            let parent = commit.parent(0).map_err(|e| e.to_string())?;
                            Some(parent.tree().map_err(|e| e.to_string())?)
                        } else {
                            None
                        };
                        let commit_tree = commit.tree().map_err(|e| e.to_string())?;
                        let mut diff_opts = git2::DiffOptions::new();
                        diff_opts.pathspec(filter_path);
                        let diff = repo.diff_tree_to_tree(
                            parent_tree.as_ref(),
                            Some(&commit_tree),
                            Some(&mut diff_opts),
                        ).map_err(|e| e.to_string())?;
                        if diff.deltas().count() == 0 {
                            continue;
                        }
                    }

                    result.push(commit_to_json(&commit));
                }
                Ok(result)
            })
            .await
            .map_err(|e| OrchynError::Internal(format!("git_log panic: {e}")))?
            .map_err(|e| OrchynError::Internal(format!("git_log: {e}")))?;

            Ok(ToolResult::ok(json!(commits)))
        }
        .boxed()
    }
}

// ---------------------------------------------------------------------------
// git_diff
// ---------------------------------------------------------------------------

pub struct GitDiffTool;

impl Tool for GitDiffTool {
    fn name(&self) -> &str { "git_diff" }

    fn description(&self) -> &str {
        "Show unified diff between two git refs. Returns the diff as a text string."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from": {
                    "type": "string",
                    "description": "Base ref (e.g. 'HEAD~1', a commit hash, or branch name)"
                },
                "to": {
                    "type": "string",
                    "description": "Target ref (default: 'HEAD')"
                },
                "path": {
                    "type": "string",
                    "description": "Optional path filter"
                }
            },
            "required": ["from"]
        })
    }

    fn execute<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let from = match args.get("from").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return Ok(ToolResult::err("missing required field `from`")),
            };
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("HEAD").to_string();
            let path_filter = get_opt_str(&args, "path").map(|s| s.to_string());
            let root: Arc<PathBuf> = Arc::clone(&ctx.project_root);

            let diff_text = tokio::task::spawn_blocking(move || -> std::result::Result<String, String> {
                let repo = open_repo(&root).map_err(|e| format!("not a git repo: {e}"))?;

                let resolve_ref = |refspec: &str| -> std::result::Result<git2::Tree, String> {
                    let obj = repo.revparse_single(refspec)
                        .map_err(|e| format!("cannot resolve ref '{refspec}': {e}"))?;
                    let commit = obj.peel_to_commit()
                        .map_err(|e| format!("ref '{refspec}' is not a commit: {e}"))?;
                    commit.tree().map_err(|e| e.to_string())
                };

                if from == to {
                    return Ok("(no changes)".to_string());
                }

                let from_tree = resolve_ref(&from)?;
                let to_tree = resolve_ref(&to)?;

                let mut diff_opts = git2::DiffOptions::new();
                if let Some(ref p) = path_filter {
                    diff_opts.pathspec(p);
                }

                let diff = repo.diff_tree_to_tree(
                    Some(&from_tree),
                    Some(&to_tree),
                    Some(&mut diff_opts),
                ).map_err(|e| format!("diff failed: {e}"))?;

                let mut buf = Vec::<u8>::new();
                diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
                    use git2::DiffLineType::*;
                    let prefix = match line.origin_value() {
                        Addition => "+",
                        Deletion => "-",
                        Context => " ",
                        _ => "",
                    };
                    let content = std::str::from_utf8(line.content()).unwrap_or("");
                    buf.extend_from_slice(prefix.as_bytes());
                    buf.extend_from_slice(content.as_bytes());
                    true
                }).map_err(|e| format!("diff.print: {e}"))?;

                let text = String::from_utf8_lossy(&buf).into_owned();
                // Cap at 20 000 chars
                if text.len() > 20_000 {
                    Ok(format!("{}\n... (truncated at 20000 chars)", &text[..20_000]))
                } else if text.is_empty() {
                    Ok("(no changes)".to_string())
                } else {
                    Ok(text)
                }
            })
            .await
            .map_err(|e| OrchynError::Internal(format!("git_diff panic: {e}")))?
            .map_err(|e| OrchynError::Internal(format!("git_diff: {e}")))?;

            Ok(ToolResult::ok(json!(diff_text)))
        }
        .boxed()
    }
}

// ---------------------------------------------------------------------------
// git_show
// ---------------------------------------------------------------------------

pub struct GitShowTool;

impl Tool for GitShowTool {
    fn name(&self) -> &str { "git_show" }

    fn description(&self) -> &str {
        "Show metadata and diff for a single commit."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ref": {
                    "type": "string",
                    "description": "Commit ref to show (default: 'HEAD')"
                }
            },
            "required": []
        })
    }

    fn execute<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let refspec = args.get("ref").and_then(|v| v.as_str()).unwrap_or("HEAD").to_string();
            let root: Arc<PathBuf> = Arc::clone(&ctx.project_root);

            let result = tokio::task::spawn_blocking(move || -> std::result::Result<Value, String> {
                let repo = open_repo(&root).map_err(|e| format!("not a git repo: {e}"))?;
                let obj = repo.revparse_single(&refspec)
                    .map_err(|e| format!("cannot resolve ref '{refspec}': {e}"))?;
                let commit = obj.peel_to_commit()
                    .map_err(|e| format!("ref '{refspec}' is not a commit: {e}"))?;

                let parent_tree = if commit.parent_count() > 0 {
                    let parent = commit.parent(0).map_err(|e| e.to_string())?;
                    Some(parent.tree().map_err(|e| e.to_string())?)
                } else {
                    None
                };
                let commit_tree = commit.tree().map_err(|e| e.to_string())?;

                let diff = repo.diff_tree_to_tree(
                    parent_tree.as_ref(),
                    Some(&commit_tree),
                    None,
                ).map_err(|e| format!("diff failed: {e}"))?;

                let mut buf = Vec::<u8>::new();
                diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
                    use git2::DiffLineType::*;
                    let prefix = match line.origin_value() {
                        Addition => "+",
                        Deletion => "-",
                        Context => " ",
                        _ => "",
                    };
                    let content = std::str::from_utf8(line.content()).unwrap_or("");
                    buf.extend_from_slice(prefix.as_bytes());
                    buf.extend_from_slice(content.as_bytes());
                    true
                }).map_err(|e| format!("diff.print: {e}"))?;

                let mut diff_text = String::from_utf8_lossy(&buf).into_owned();
                // Cap at 10 000 chars
                if diff_text.len() > 10_000 {
                    diff_text = format!("{}\n... (truncated at 10000 chars)", &diff_text[..10_000]);
                }

                let mut meta = commit_to_json(&commit);
                meta["diff"] = json!(diff_text);
                // Add full message (not just first line)
                meta["full_message"] = json!(commit.message().unwrap_or("").to_string());
                Ok(meta)
            })
            .await
            .map_err(|e| OrchynError::Internal(format!("git_show panic: {e}")))?
            .map_err(|e| OrchynError::Internal(format!("git_show: {e}")))?;

            Ok(ToolResult::ok(result))
        }
        .boxed()
    }
}

// ---------------------------------------------------------------------------
// git_status
// ---------------------------------------------------------------------------

pub struct GitStatusTool;

impl Tool for GitStatusTool {
    fn name(&self) -> &str { "git_status" }

    fn description(&self) -> &str {
        "Show working tree status (modified, added, deleted, untracked files)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn execute<'a>(&'a self, _args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let root: Arc<PathBuf> = Arc::clone(&ctx.project_root);

            let result = tokio::task::spawn_blocking(move || -> std::result::Result<Value, String> {
                let repo = open_repo(&root).map_err(|e| format!("not a git repo: {e}"))?;

                let mut opts = git2::StatusOptions::new();
                opts.include_untracked(true)
                    .recurse_untracked_dirs(true)
                    .include_ignored(false);

                let statuses = repo.statuses(Some(&mut opts))
                    .map_err(|e| format!("statuses: {e}"))?;

                let mut modified: Vec<String> = Vec::new();
                let mut added: Vec<String> = Vec::new();
                let mut deleted: Vec<String> = Vec::new();
                let mut renamed: Vec<[String; 2]> = Vec::new();
                let mut untracked: Vec<String> = Vec::new();

                for entry in statuses.iter() {
                    let flags = entry.status();
                    let path = entry.path().unwrap_or("").to_string();

                    // Untracked
                    if flags.contains(git2::Status::WT_NEW) && !flags.contains(git2::Status::INDEX_NEW) {
                        untracked.push(path);
                        continue;
                    }

                    // Renamed (index or worktree)
                    if flags.contains(git2::Status::INDEX_RENAMED) || flags.contains(git2::Status::WT_RENAMED) {
                        if let Some(hd) = entry.head_to_index() {
                            let old = hd.old_file().path()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let new = hd.new_file().path()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            renamed.push([old, new]);
                        } else if let Some(iw) = entry.index_to_workdir() {
                            let old = iw.old_file().path()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let new = iw.new_file().path()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            renamed.push([old, new]);
                        }
                        continue;
                    }

                    // Added (new in index)
                    if flags.contains(git2::Status::INDEX_NEW) {
                        added.push(path);
                        continue;
                    }

                    // Deleted
                    if flags.contains(git2::Status::INDEX_DELETED) || flags.contains(git2::Status::WT_DELETED) {
                        deleted.push(path);
                        continue;
                    }

                    // Modified (index or worktree)
                    if flags.contains(git2::Status::INDEX_MODIFIED)
                        || flags.contains(git2::Status::WT_MODIFIED)
                    {
                        modified.push(path);
                    }
                }

                let clean = modified.is_empty()
                    && added.is_empty()
                    && deleted.is_empty()
                    && renamed.is_empty()
                    && untracked.is_empty();

                let renamed_json: Vec<Value> = renamed
                    .into_iter()
                    .map(|[old, new]| json!([old, new]))
                    .collect();

                Ok(json!({
                    "modified": modified,
                    "added": added,
                    "deleted": deleted,
                    "renamed": renamed_json,
                    "untracked": untracked,
                    "clean": clean,
                }))
            })
            .await
            .map_err(|e| OrchynError::Internal(format!("git_status panic: {e}")))?
            .map_err(|e| OrchynError::Internal(format!("git_status: {e}")))?;

            Ok(ToolResult::ok(result))
        }
        .boxed()
    }
}
