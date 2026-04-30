//! File-system tools mirroring OpenHarness 1:1
//! (`read_file`, `write_file`, `edit_file`, `grep`, `glob`).

use crate::context::ToolContext;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::{OrchynError, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use regex::Regex;
use ignore::WalkBuilder;
use globset::{Glob, GlobSetBuilder};

/// Resolve `rel` against the project root and reject paths that escape it.
pub(crate) async fn resolve_in_root(ctx: &ToolContext, rel: &str) -> Result<PathBuf> {
    let candidate = ctx.project_root.join(rel);
    // Note: OpenHarness uses path.resolve() which handles .. etc.
    // We should ensure the resolved path is inside project_root.
    let canonical_root = tokio::fs::canonicalize(&*ctx.project_root)
        .await
        .map_err(|e| OrchynError::Internal(format!("canonicalize root: {e}")))?;
    
    // For non-existent files (write), we check the parent.
    let to_check = if candidate.exists() {
        candidate.clone()
    } else {
        first_existing_ancestor(&candidate)
    };

    let canonical_target = tokio::fs::canonicalize(&to_check)
        .await
        .map_err(|e| OrchynError::Internal(format!("canonicalize {}: {e}", to_check.display())))?;

    if !canonical_target.starts_with(&canonical_root) {
        return Err(OrchynError::Validation(format!(
            "path escapes project root: {rel}"
        )));
    }
    Ok(candidate)
}

fn first_existing_ancestor(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    while !current.exists() {
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }
    current
}

pub(crate) fn get_opt_u64(args: &Value, key: &str, default: u64) -> u64 {
    args.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

pub(crate) fn get_opt_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

pub(crate) fn get_opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

pub(crate) fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| OrchynError::Validation(format!("missing string field `{key}`")))
}

// === read_file ===

pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str {
        "Read a text file from the local repository."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path of the file to read"},
                "offset": {"type": "integer", "default": 0, "minimum": 0, "description": "Zero-based starting line"},
                "limit": {"type": "integer", "default": 200, "minimum": 1, "maximum": 2000, "description": "Number of lines to return"}
            },
            "required": ["path"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let rel = require_str(&args, "path")?.to_string();
            let offset = get_opt_u64(&args, "offset", 0) as usize;
            let limit = get_opt_u64(&args, "limit", 200) as usize;
            
            let path = resolve_in_root(ctx, &rel).await?;
            
            if !path.exists() {
                return Ok(ToolResult::err(format!("File not found: {}", rel)));
            }
            if path.is_dir() {
                return Ok(ToolResult::err(format!("Cannot read directory: {}", rel)));
            }

            let raw = tokio::fs::read(&path)
                .await
                .map_err(|e| OrchynError::Internal(format!("read {rel}: {e}")))?;
            
            if raw.contains(&0) {
                return Ok(ToolResult::err(format!("Binary file cannot be read as text: {}", rel)));
            }

            let text = String::from_utf8_lossy(&raw);
            let lines: Vec<&str> = text.lines().collect();
            
            let end = (offset + limit).min(lines.len());
            if offset >= lines.len() {
                return Ok(ToolResult::ok(json!({
                    "output": format!("(no content in selected range for {})", rel)
                })));
            }

            let selected = &lines[offset..end];
            let mut output = String::new();
            for (i, line) in selected.iter().enumerate() {
                output.push_str(&format!("{:>6}\t{}\n", offset + i + 1, line));
            }

            Ok(ToolResult::ok(json!(output.trim_end())))
        }
        .boxed()
    }
}

// === write_file ===

pub struct WriteFileTool;

impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }
    fn description(&self) -> &str {
        "Create or overwrite a text file in the local repository."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path of the file to write"},
                "content": {"type": "string", "description": "Full file contents"},
                "create_directories": {"type": "boolean", "default": true}
            },
            "required": ["path", "content"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let rel = require_str(&args, "path")?.to_string();
            let content = require_str(&args, "content")?;
            let create_dirs = get_opt_bool(&args, "create_directories", true);
            
            let path = resolve_in_root(ctx, &rel).await?;
            
            if create_dirs {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| OrchynError::Internal(format!("mkdir {}: {e}", parent.display())))?;
                }
            }

            tokio::fs::write(&path, content.as_bytes())
                .await
                .map_err(|e| OrchynError::Internal(format!("write {rel}: {e}")))?;

            Ok(ToolResult::ok(json!(format!("Wrote {}", rel))))
        }
        .boxed()
    }
}

// === edit_file ===

pub struct EditFileTool;

impl Tool for EditFileTool {
    fn name(&self) -> &str { "edit_file" }
    fn description(&self) -> &str {
        "Edit an existing file by replacing a string."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path of the file to edit"},
                "old_str": {"type": "string", "description": "Existing text to replace"},
                "new_str": {"type": "string", "description": "Replacement text"},
                "replace_all": {"type": "boolean", "default": false}
            },
            "required": ["path", "old_str", "new_str"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let rel = require_str(&args, "path")?.to_string();
            let old_str = require_str(&args, "old_str")?;
            let new_str = require_str(&args, "new_str")?;
            let replace_all = get_opt_bool(&args, "replace_all", false);
            
            let path = resolve_in_root(ctx, &rel).await?;
            
            if !path.exists() {
                return Ok(ToolResult::err(format!("File not found: {}", rel)));
            }

            let original = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| OrchynError::Internal(format!("read {rel}: {e}")))?;
            
            if !original.contains(old_str) {
                return Ok(ToolResult::err("old_str was not found in the file".to_string()));
            }

            let updated = if replace_all {
                original.replace(old_str, new_str)
            } else {
                original.replacen(old_str, new_str, 1)
            };

            tokio::fs::write(&path, updated.as_bytes())
                .await
                .map_err(|e| OrchynError::Internal(format!("write {rel}: {e}")))?;

            Ok(ToolResult::ok(json!(format!("Updated {}", rel))))
        }
        .boxed()
    }
}

// === grep ===

pub struct GrepTool;

impl Tool for GrepTool {
    fn name(&self) -> &str { "grep" }
    fn description(&self) -> &str {
        "Search file contents with a regular expression."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regular expression to search for"},
                "root": {"type": "string", "description": "Search root directory"},
                "file_glob": {"type": "string", "default": "**/*"},
                "case_sensitive": {"type": "boolean", "default": true},
                "limit": {"type": "integer", "default": 200, "minimum": 1, "maximum": 2000},
                "timeout_seconds": {"type": "integer", "default": 20, "minimum": 1, "maximum": 120}
            },
            "required": ["pattern"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let pattern = require_str(&args, "pattern")?.to_string();
            let root_rel = get_opt_str(&args, "root").unwrap_or(".");
            let file_glob = get_opt_str(&args, "file_glob").unwrap_or("**/*");
            let case_sensitive = get_opt_bool(&args, "case_sensitive", true);
            let limit = get_opt_u64(&args, "limit", 200) as usize;
            
            let root_path = resolve_in_root(ctx, root_rel).await?;
            let project_root = (*ctx.project_root).clone();

            let regex = if case_sensitive {
                Regex::new(&pattern)
            } else {
                Regex::new(&format!("(?i){}", pattern))
            }.map_err(|e| OrchynError::Validation(format!("invalid regex: {e}")))?;

            let glob = Glob::new(file_glob)
                .map_err(|e| OrchynError::Validation(format!("invalid glob: {e}")))?;
            let mut builder = GlobSetBuilder::new();
            builder.add(glob);
            let glob_set = builder.build()
                .map_err(|e| OrchynError::Internal(format!("globset build: {e}")))?;

            let hits = tokio::task::spawn_blocking(move || {
                let mut hits = Vec::new();
                let walker = WalkBuilder::new(&root_path).build();
                for entry in walker {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        continue;
                    }
                    let path = entry.path();
                    let rel_path = path.strip_prefix(&project_root).unwrap_or(path);
                    
                    if !glob_set.is_match(rel_path) && !glob_set.is_match(path) {
                        continue;
                    }

                    let content = match std::fs::read_to_string(path) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    for (i, line) in content.lines().enumerate() {
                        if regex.is_match(line) {
                            hits.push(format!("{}:{}:{}", rel_path.display(), i + 1, line));
                            if hits.len() >= limit {
                                return hits;
                            }
                        }
                    }
                }
                hits
            })
            .await
            .map_err(|e| OrchynError::Internal(format!("grep panic: {e}")))?;

            if hits.is_empty() {
                Ok(ToolResult::ok(json!("(no matches)")))
            } else {
                Ok(ToolResult::ok(json!(hits.join("\n"))))
            }
        }
        .boxed()
    }
}

// === glob ===

pub struct GlobTool;

impl Tool for GlobTool {
    fn name(&self) -> &str { "glob" }
    fn description(&self) -> &str {
        "List files matching a glob pattern."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern relative to the working directory"},
                "root": {"type": "string", "description": "Optional search root"},
                "limit": {"type": "integer", "default": 200, "minimum": 1, "maximum": 5000}
            },
            "required": ["pattern"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let pattern = require_str(&args, "pattern")?.to_string();
            let root_rel = get_opt_str(&args, "root").unwrap_or(".");
            let limit = get_opt_u64(&args, "limit", 200) as usize;
            
            let root_path = resolve_in_root(ctx, root_rel).await?;
            let project_root = (*ctx.project_root).clone();

            let glob = Glob::new(&pattern)
                .map_err(|e| OrchynError::Validation(format!("invalid glob: {e}")))?;
            let mut builder = GlobSetBuilder::new();
            builder.add(glob);
            let glob_set = builder.build()
                .map_err(|e| OrchynError::Internal(format!("globset build: {e}")))?;

            let files = tokio::task::spawn_blocking(move || {
                let mut files = Vec::new();
                let walker = WalkBuilder::new(&root_path).build();
                for entry in walker {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        continue;
                    }
                    let path = entry.path();
                    let rel_path = path.strip_prefix(&project_root).unwrap_or(path);
                    
                    if glob_set.is_match(rel_path) || glob_set.is_match(path) {
                        files.push(rel_path.to_string_lossy().to_string());
                        if files.len() >= limit {
                            break;
                        }
                    }
                }
                files.sort();
                files
            })
            .await
            .map_err(|e| OrchynError::Internal(format!("glob panic: {e}")))?;

            if files.is_empty() {
                Ok(ToolResult::ok(json!("(no matches)")))
            } else {
                Ok(ToolResult::ok(json!(files.join("\n"))))
            }
        }
        .boxed()
    }
}
