use futures::future::{BoxFuture, FutureExt};
use orchyn_core::Result;
use serde_json::Value;
use std::fmt::Debug;
use std::future::Future;
use std::path::PathBuf;

pub trait NavigatorTool: Send + Sync + Debug {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>>;
}

#[derive(Debug)]
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
        "Recursively list files in a directory"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        })
    }
    fn call(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        let root = self.root.clone();
        async move {
            let mut files = Vec::new();
            for entry in ignore::Walk::new(root) {
                if let Ok(entry) = entry {
                    if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        files.push(entry.path().to_string_lossy().to_string());
                    }
                }
            }
            Ok(serde_json::Value::from(files))
        }
        .boxed()
    }
}

#[derive(Debug)]
pub struct ReadFileTool;

impl NavigatorTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read the contents of a file"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        })
    }
    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        async move {
            let path = args["path"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing path".to_string(),
                ))?;
            let content = std::fs::read_to_string(path)
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
            Ok(serde_json::Value::String(content))
        }
        .boxed()
    }
}

#[derive(Debug)]
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
        "grep_search"
    }
    fn description(&self) -> &str {
        "Search for a pattern in files"
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
    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let _root = self.root.clone();
        async move {
            let _pattern = args["pattern"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing pattern".to_string(),
                ))?;
            // Simple placeholder grep
            Ok(serde_json::json!([]))
        }
        .boxed()
    }
}
