pub mod agent;
pub mod tools;
pub mod traits;

pub use agent::FixerAgent;
pub use traits::{Agent, AgentEvent};
pub use tools::FixerTool;

use orchyn_model_client::ModelClient;
use std::sync::Arc;
use std::path::PathBuf;

pub fn create_default_tools<M: ModelClient + 'static>(model: Arc<M>, cwd: PathBuf) -> Vec<Box<dyn FixerTool>> {
    vec![
        Box::new(tools::ListFilesTool::new(cwd.clone())),
        Box::new(tools::GrepTool::new(cwd.clone())),
        Box::new(tools::ReadFileTool),
        Box::new(tools::WriteFileTool),
        Box::new(tools::ReplaceTextTool),
        Box::new(tools::RunCommandTool::new(cwd)),
        Box::new(tools::ObserveTool::new(model)),
    ]
}

pub fn create_fixer_agent<M: ModelClient + 'static>(model: Arc<M>, cwd: PathBuf) -> FixerAgent<M> {
    let tools = create_default_tools(model.clone(), cwd.clone());
    
    let initial_sha = if let Ok(repo) = git2::Repository::open(&cwd) {
        repo.head().ok().and_then(|h| h.target()).map(|t| t.to_string())
    } else {
        None
    };

    FixerAgent::new(model, tools, initial_sha)
}
