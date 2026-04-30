//! Shell-execution tools mirroring OpenHarness 1:1
//! (`bash`).

use crate::context::ToolContext;
use crate::registry::{Tool, ToolResult};
use futures::future::{BoxFuture, FutureExt};
use orchyn_core::{OrchynError, Result};
use serde_json::{json, Value};
use crate::file_tools::{require_str, get_opt_u64, get_opt_str, get_opt_bool};

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &str { "bash" }
    fn description(&self) -> &str {
        "Run a shell command in the local repository."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command to execute"},
                "cwd": {"type": "string", "description": "Working directory override"},
                "timeout_seconds": {"type": "integer", "default": 600, "minimum": 1, "maximum": 600}
            },
            "required": ["command"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            if !ctx.allow_shell {
                return Err(OrchynError::Validation(
                    "bash is disabled (set ToolContext::allow_shell to opt in)".into(),
                ));
            }

            let command = require_str(&args, "command")?;
            let timeout_secs = get_opt_u64(&args, "timeout_seconds", 600);
            let cwd_rel = get_opt_str(&args, "cwd");
            
            let cwd = if let Some(rel) = cwd_rel {
                let path = ctx.project_root.join(rel);
                path.canonicalize().unwrap_or(path)
            } else {
                (*ctx.project_root).clone()
            };

            if let Some(error) = preflight_interactive_command(command) {
                return Ok(ToolResult::err(error));
            }

            let cmd_str = command.to_string();
            let mut child = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd_str)
                .current_dir(cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| OrchynError::Internal(format!("bash spawn failed: {e}")))?;

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                child.wait_with_output()
            ).await;

            match result {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let combined = format!("{}{}", stdout, stderr);
                    let text = format_output(&combined);
                    
                    if output.status.success() {
                        Ok(ToolResult::ok(json!(text)))
                    } else {
                        Ok(ToolResult::err(text))
                    }
                }
                Ok(Err(e)) => {
                    Ok(ToolResult::err(format!("bash wait failed: {e}")))
                }
                Err(_) => {
                    Ok(ToolResult::err(format!("Command timed out after {} seconds.", timeout_secs)))
                }
            }
        }
        .boxed()
    }
}

fn format_output(output: &str) -> String {
    let text = output.replace("\r\n", "\n").trim().to_string();
    if text.is_empty() {
        return "(no output)".to_string();
    }
    if text.len() > 12000 {
        return format!("{}\n...[truncated]...", &text[..12000]);
    }
    text
}

fn preflight_interactive_command(command: &str) -> Option<String> {
    let lowered = command.to_lowercase();
    if looks_like_interactive_scaffold(&lowered) {
        return Some(
            "This command appears to require interactive input before it can continue. \
            The bash tool is non-interactive, so it cannot answer installer/scaffold prompts live. \
            Prefer non-interactive flags (for example --yes, -y, --skip-install, --defaults, --non-interactive), \
            or run the scaffolding step once in an external terminal before asking the agent to continue.".to_string()
        );
    }
    None
}

fn looks_like_interactive_scaffold(lowered: &str) -> bool {
    let markers = [
        "create-next-app",
        "npm create ",
        "pnpm create ",
        "yarn create ",
        "bun create ",
        "pnpm dlx ",
        "npm init ",
        "pnpm init ",
        "yarn init ",
        "bunx create-",
        "npx create-",
    ];
    let non_interactive = [
        "--yes",
        " -y",
        "--skip-install",
        "--defaults",
        "--non-interactive",
        "--ci",
    ];
    
    markers.iter().any(|m| lowered.contains(m)) && !non_interactive.iter().any(|m| lowered.contains(m))
}

// === enter_worktree ===

pub struct EnterWorktreeTool;

impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str { "enter_worktree" }
    fn description(&self) -> &str {
        "Create a git worktree and return its path."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "branch": {"type": "string", "description": "Target branch name for the worktree"},
                "path": {"type": "string", "description": "Optional worktree path"},
                "create_branch": {"type": "boolean", "default": true},
                "base_ref": {"type": "string", "default": "HEAD", "description": "Base ref when creating a new branch"}
            },
            "required": ["branch"]
        })
    }
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, Result<ToolResult>> {
        async move {
            let branch = require_str(&args, "branch")?;
            let path_opt = get_opt_str(&args, "path");
            let create_branch = get_opt_bool(&args, "create_branch", true);
            let base_ref = get_opt_str(&args, "base_ref").unwrap_or("HEAD");
            
            let repo_root = (*ctx.project_root).clone();
            
            let worktree_path = if let Some(p) = path_opt {
                repo_root.join(p)
            } else {
                let slug = branch.chars().map(|c| if c.is_alphanumeric() || "._-".contains(c) { c } else { '-' }).collect::<String>();
                repo_root.join(".openharness").join("worktrees").join(slug)
            };

            if let Some(parent) = worktree_path.parent() {
                tokio::fs::create_dir_all(parent).await
                    .map_err(|e| OrchynError::Internal(format!("mkdir: {e}")))?;
            }

            let mut cmd = tokio::process::Command::new("git");
            cmd.arg("worktree").arg("add").current_dir(&repo_root);
            
            if create_branch {
                cmd.arg("-b").arg(branch).arg(&worktree_path).arg(base_ref);
            } else {
                cmd.arg(&worktree_path).arg(branch);
            }

            let output = cmd.output().await
                .map_err(|e| OrchynError::Internal(format!("git worktree add failed: {e}")))?;
            
            let out_text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            
            if !output.status.success() {
                Ok(ToolResult::err(out_text))
            } else {
                Ok(ToolResult::ok(json!(format!("{}\nPath: {}", out_text.trim(), worktree_path.display()))))
            }
        }
        .boxed()
    }
}

// === exit_worktree ===

pub struct ExitWorktreeTool;

impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str { "exit_worktree" }
    fn description(&self) -> &str {
        "Remove a git worktree by path."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Worktree path to remove"}
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
            let path_str = require_str(&args, "path")?;
            let path = ctx.project_root.join(path_str);
            
            let output = tokio::process::Command::new("git")
                .arg("worktree").arg("remove").arg("--force").arg(&path)
                .current_dir(&*ctx.project_root)
                .output().await
                .map_err(|e| OrchynError::Internal(format!("git worktree remove failed: {e}")))?;
            
            let out_text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            
            if !output.status.success() {
                Ok(ToolResult::err(out_text))
            } else {
                Ok(ToolResult::ok(json!(format!("Removed worktree {}", path.display()))))
            }
        }
        .boxed()
    }
}
