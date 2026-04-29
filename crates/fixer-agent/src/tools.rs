use futures::future::{BoxFuture, FutureExt};
use orchyn_core::Result;
use orchyn_model_client::ModelClient;
use serde_json::Value;
use std::fmt::Debug;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

pub trait FixerTool: Send + Sync + Debug {
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

impl FixerTool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }
    fn description(&self) -> &str {
        "Recursively list files"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
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
pub struct GrepTool {
    root: PathBuf,
}

impl GrepTool {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl FixerTool for GrepTool {
    fn name(&self) -> &str {
        "grep_search"
    }
    fn description(&self) -> &str {
        "Search pattern in files"
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
        async move {
            let _pattern = args["pattern"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing pattern".to_string(),
                ))?;
            Ok(serde_json::json!([]))
        }
        .boxed()
    }
}

#[derive(Debug)]
pub struct ReadFileTool;

impl FixerTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read file content"
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
            Ok(Value::String(content))
        }
        .boxed()
    }
}

#[derive(Debug)]
pub struct WriteFileTool;

impl FixerTool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write file content"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }
    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        async move {
            let path = args["path"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing path".to_string(),
                ))?;
            let content = args["content"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing content".to_string(),
                ))?;
            std::fs::write(path, content)
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
            Ok(Value::String("Success".to_string()))
        }
        .boxed()
    }
}

#[derive(Debug)]
pub struct ReplaceTextTool;

impl FixerTool for ReplaceTextTool {
    fn name(&self) -> &str {
        "replace_text"
    }
    fn description(&self) -> &str {
        "Replace text in a file"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_text": { "type": "string" },
                "new_text": { "type": "string" }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }
    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        async move {
            let path = args["path"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing path".to_string(),
                ))?;
            let old_text = args["old_text"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing old_text".to_string(),
                ))?;
            let new_text = args["new_text"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing new_text".to_string(),
                ))?;

            let content = std::fs::read_to_string(path)
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;
            let new_content = content.replace(old_text, new_text);
            std::fs::write(path, new_content)
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;

            Ok(Value::String("Success".to_string()))
        }
        .boxed()
    }
}

#[derive(Debug)]
pub struct RunCommandTool {
    _cwd: PathBuf,
}

impl RunCommandTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { _cwd: cwd }
    }
}

impl FixerTool for RunCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }
    fn description(&self) -> &str {
        "Run shell command"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" }
            },
            "required": ["command"]
        })
    }
    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        async move {
            let command = args["command"]
                .as_str()
                .ok_or(orchyn_core::OrchynError::Internal(
                    "Missing command".to_string(),
                ))?;
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .output()
                .map_err(|e| orchyn_core::OrchynError::Internal(e.to_string()))?;

            Ok(serde_json::json!({
                "status": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            }))
        }
        .boxed()
    }
}

#[derive(Debug)]
pub struct ObserveTool<M: ModelClient> {
    model: Arc<M>,
}

impl<M: ModelClient> ObserveTool<M> {
    pub fn new(model: Arc<M>) -> Self {
        Self { model }
    }
}

impl<M: ModelClient> FixerTool for ObserveTool<M> {
    fn name(&self) -> &str {
        "observe_ui"
    }
    fn description(&self) -> &str {
        "Observe UI with vision"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "prompt": { "type": "string" }
            },
            "required": ["url", "prompt"]
        })
    }
    fn call(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        async move { Ok(serde_json::json!({})) }.boxed()
    }
}
