use orchyn_core::{OrchynError, Result};
use serde_json::Value;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::process::Command;
use ignore::WalkBuilder;
use regex::Regex;
use orchyn_model_client::QwenClient;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait FixerTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>>;
}

pub struct ListFilesTool {
    cwd: PathBuf,
}

impl ListFilesTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

impl FixerTool for ListFilesTool {
    fn name(&self) -> &str { "list_files" }
    fn description(&self) -> &str { "List all files in the project, respecting gitignore." }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Optional glob pattern" }
            }
        })
    }
    fn call<'a>(&'a self, _args: Value) -> BoxFuture<'a, Result<Value>> {
        let cwd = self.cwd.clone();
        Box::pin(async move {
            let mut files = Vec::new();
            let walker = WalkBuilder::new(&cwd).build();
            for entry in walker.flatten() {
                if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    if let Ok(path) = entry.path().strip_prefix(&cwd) {
                        files.push(path.to_string_lossy().to_string());
                    }
                }
            }
            Ok(Value::Array(files.into_iter().map(Value::String).collect()))
        })
    }
}

pub struct GrepTool {
    cwd: PathBuf,
}

impl GrepTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

impl FixerTool for GrepTool {
    fn name(&self) -> &str { "grep" }
    fn description(&self) -> &str { "Search for a regex pattern in all files." }
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
        let cwd = self.cwd.clone();
        Box::pin(async move {
            let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let re = Regex::new(pattern).map_err(|e| OrchynError::Internal(e.to_string()))?;
            
            let mut matches = Vec::new();
            let walker = WalkBuilder::new(&cwd).build();
            for entry in walker.flatten() {
                if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if re.is_match(&content) {
                            if let Ok(path) = entry.path().strip_prefix(&cwd) {
                                matches.push(path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
            Ok(Value::Array(matches.into_iter().map(Value::String).collect()))
        })
    }
}

pub struct ReadFileTool;

impl FixerTool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str { "Read file content" }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        })
    }
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let content = tokio::fs::read_to_string(path)
                .await
                .map_err(|e| OrchynError::Internal(e.to_string()))?;
            Ok(Value::String(content))
        })
    }
}

pub struct WriteFileTool;

impl FixerTool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }
    fn description(&self) -> &str { "Write content to file (overwrite)" }
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
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            tokio::fs::write(path, content)
                .await
                .map_err(|e| OrchynError::Internal(e.to_string()))?;
            Ok(Value::String("File written".to_string()))
        })
    }
}

pub struct ReplaceTextTool;

impl FixerTool for ReplaceTextTool {
    fn name(&self) -> &str { "replace_text" }
    fn description(&self) -> &str { "Replace text in file" }
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
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let old_text = args.get("old_text").and_then(|v| v.as_str()).unwrap_or("");
            let new_text = args.get("new_text").and_then(|v| v.as_str()).unwrap_or("");

            let content = tokio::fs::read_to_string(path)
                .await
                .map_err(|e| OrchynError::Internal(e.to_string()))?;

            if !content.contains(old_text) {
                return Err(OrchynError::Internal("old_text not found".to_string()));
            }

            let new_content = content.replace(old_text, new_text);
            tokio::fs::write(path, new_content)
                .await
                .map_err(|e| OrchynError::Internal(e.to_string()))?;
            Ok(Value::String("Text replaced".to_string()))
        })
    }
}

pub struct RunCommandTool {
    cwd: PathBuf,
}

impl RunCommandTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

impl FixerTool for RunCommandTool {
    fn name(&self) -> &str { "run_command" }
    fn description(&self) -> &str { "Run a shell command (e.g., npm run build, cargo check). Always run this to verify fixes." }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" }
            },
            "required": ["command"]
        })
    }
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>> {
        let cwd = self.cwd.clone();
        Box::pin(async move {
            let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let output = Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&cwd)
                .output()
                .await
                .map_err(|e| OrchynError::Internal(e.to_string()))?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            let result = format!(
                "Exit status: {}\nStdout: {}\nStderr: {}",
                output.status, stdout, stderr
            );
            Ok(Value::String(result))
        })
    }
}

pub struct ObserveTool {
    model: QwenClient,
}

impl ObserveTool {
    pub fn new(model: QwenClient) -> Self {
        Self { model }
    }
}

impl FixerTool for ObserveTool {
    fn name(&self) -> &str { "observe_ui" }
    fn description(&self) -> &str { "Open a URL in a headless browser, take a screenshot, and use Vision AI to describe it." }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to probe (e.g. http://localhost:3000)" },
                "prompt": { "type": "string", "description": "What to look for in the UI" }
            },
            "required": ["url", "prompt"]
        })
    }
    fn call<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value>> {
        let model = self.model.clone();
        Box::pin(async move {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("What is on the screen?");
            
            use orchyn_platform_web::WebAdapter;
            use orchyn_action_executor::PlatformAdapter;
            
            let mut adapter = WebAdapter::new(url).await?;
            let screenshot: Vec<u8> = adapter.screenshot().await?;
            
            let analysis = model.observe(&screenshot, prompt).await?;
            Ok(analysis)
        })
    }
}
