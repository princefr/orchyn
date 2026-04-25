use ignore::WalkBuilder;
use orchyn_core::{OrchynError, Result, SourceRef};
use regex::Regex;
use serde_json::Value;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait NavigatorTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>>;
}

pub struct ListFilesTool {
    root: PathBuf,
}

impl ListFilesTool {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl NavigatorTool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }
    fn description(&self) -> &str {
        "List files in the codebase."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" }
            }
        })
    }
    fn call<'a>(&'a self, _args: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let mut files = Vec::new();
            let walker = WalkBuilder::new(&self.root).build();
            for entry in walker.flatten() {
                if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    if let Ok(path) = entry.path().strip_prefix(&self.root) {
                        files.push(path.to_string_lossy().to_string());
                    }
                }
            }
            Ok(Value::Array(files.into_iter().map(Value::String).collect()))
        })
    }
}

pub struct ReadFileTool;

impl NavigatorTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a specific line range from a file."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "line_start": { "type": "integer" },
                "line_end": { "type": "integer" }
            },
            "required": ["path"]
        })
    }
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let path_str = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let start = args.get("line_start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let end = args
                .get("line_end")
                .and_then(|v| v.as_u64())
                .unwrap_or(10000) as usize;

            let content = tokio::fs::read_to_string(path_str)
                .await
                .map_err(|e| OrchynError::Internal(e.to_string()))?;

            let lines: Vec<&str> = content.lines().collect();
            let subset = lines
                .iter()
                .skip(start)
                .take(end - start + 1)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");

            Ok(Value::String(subset))
        })
    }
}

pub struct GrepTool {
    root: PathBuf,
}

impl GrepTool {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl NavigatorTool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Search for symbols or text across the repository."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" }
            },
            "required": ["pattern"]
        })
    }
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let re = Regex::new(pattern).map_err(|e| OrchynError::Internal(e.to_string()))?;
            let mut matches = Vec::new();
            let walker = WalkBuilder::new(&self.root).build();
            for entry in walker.flatten() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if re.is_match(&content) {
                        matches.push(entry.path().to_string_lossy().to_string());
                    }
                }
            }
            Ok(Value::Array(
                matches.into_iter().map(Value::String).collect(),
            ))
        })
    }
}
