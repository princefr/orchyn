//! Tool registry used by Agent Mode (and exposed to any future cron-driven
//! exploration). Each tool is a small, side-effecting capability the model
//! can invoke by name with JSON arguments.
//!
//! Browser tools talk to one of the actors in the [`ToolContext`]; code
//! tools read/grep the local filesystem; meta tools mutate working memory
//! or spawn additional actors.

pub mod browser_tools;
pub mod code_tools;
pub mod context;
pub mod meta_tools;
pub mod registry;
pub mod schema;

pub use browser_tools::{
    BrowserClickTool, BrowserGetConsoleTool, BrowserNavigateTool, BrowserScreenshotTool,
    BrowserTypeTool,
};
pub use code_tools::{CodeReadTool, CodeSearchTool, FixProposeTool, GapReportTool, TestProposeTool};
pub use context::{ActorMap, AdapterFactory, ToolContext, WorkingMemory};
pub use meta_tools::{ActorSpawnTool, DoneTool, MemoryRecallTool, MemoryStoreTool};
pub use registry::{Tool, ToolCall, ToolRegistry, ToolResult};
pub use schema::ToolSchema;

/// Convenience helper: registers every built-in tool on a fresh registry.
pub fn default_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(BrowserScreenshotTool));
    reg.register(Box::new(BrowserNavigateTool));
    reg.register(Box::new(BrowserClickTool));
    reg.register(Box::new(BrowserTypeTool));
    reg.register(Box::new(BrowserGetConsoleTool));
    reg.register(Box::new(CodeReadTool));
    reg.register(Box::new(CodeSearchTool));
    reg.register(Box::new(GapReportTool));
    reg.register(Box::new(TestProposeTool));
    reg.register(Box::new(FixProposeTool));
    reg.register(Box::new(ActorSpawnTool));
    reg.register(Box::new(MemoryStoreTool));
    reg.register(Box::new(MemoryRecallTool));
    reg.register(Box::new(DoneTool));
    reg
}
