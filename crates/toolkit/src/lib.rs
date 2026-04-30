//! Tool registry mirroring OpenHarness tools.
//! Each tool is a small, side-effecting capability the model can invoke by name with JSON arguments.

pub mod browser_tools;
pub mod file_tools;
pub mod context;
pub mod meta_tools;
pub mod registry;
pub mod schema;
pub mod shell_tools;
pub mod web_tools;
pub mod task_tools;
pub mod report_tools;

pub use browser_tools::{
    BrowserClickTool, BrowserGetConsoleTool, BrowserGetNetworkTool, BrowserNavigateTool,
    BrowserScreenshotTool, BrowserTypeTool,
};
pub use file_tools::{
    ReadFileTool, WriteFileTool, EditFileTool, GrepTool, GlobTool,
};
pub use shell_tools::{BashTool, EnterWorktreeTool, ExitWorktreeTool};
pub use web_tools::{WebFetchTool, WebSearchTool};
pub use task_tools::{
    TaskCreateTool, TaskGetTool, TaskListTool, TaskUpdateTool, TaskStopTool, TaskOutputTool,
    TodoWriteTool, TeamCreateTool, TeamDeleteTool, AgentTool, SendMessageTool,
    CronCreateTool, CronListTool,
};
pub use meta_tools::{
    ActorSpawnTool, MemoryRecallTool, MemoryStoreTool, DoneTool,
    EnterPlanModeTool, ExitPlanModeTool, AskUserQuestionTool, SleepTool,
    ToolSearchTool, SkillTool, BriefTool, ConfigTool,
};
pub use report_tools::{GapReportTool, TestResultTool, FixProposeTool};

pub use context::{ActorMap, AdapterFactory, ToolContext, WorkingMemory};
pub use registry::{Tool, ToolCall, ToolRegistry, ToolResult};
pub use schema::ToolSchema;

/// Convenience helper: registers every built-in tool on a fresh registry.
pub fn default_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    
    // File / Code tools
    reg.register(Box::new(ReadFileTool));
    reg.register(Box::new(WriteFileTool));
    reg.register(Box::new(EditFileTool));
    reg.register(Box::new(GrepTool));
    reg.register(Box::new(GlobTool));
    
    // Shell tools
    reg.register(Box::new(BashTool));
    reg.register(Box::new(EnterWorktreeTool));
    reg.register(Box::new(ExitWorktreeTool));
    
    // Web tools
    reg.register(Box::new(WebFetchTool));
    reg.register(Box::new(WebSearchTool));
    
    // Task tools
    reg.register(Box::new(TaskCreateTool));
    reg.register(Box::new(TaskGetTool));
    reg.register(Box::new(TaskListTool));
    reg.register(Box::new(TaskUpdateTool));
    reg.register(Box::new(TaskStopTool));
    reg.register(Box::new(TaskOutputTool));
    reg.register(Box::new(TodoWriteTool));
    reg.register(Box::new(TeamCreateTool));
    reg.register(Box::new(TeamDeleteTool));
    reg.register(Box::new(AgentTool));
    reg.register(Box::new(SendMessageTool));
    reg.register(Box::new(CronCreateTool));
    reg.register(Box::new(CronListTool));
    
    // Reporting tools (Orchyn specific)
    reg.register(Box::new(GapReportTool));
    reg.register(Box::new(TestResultTool));
    reg.register(Box::new(FixProposeTool));
    
    // Meta / Utility tools
    reg.register(Box::new(EnterPlanModeTool));
    reg.register(Box::new(ExitPlanModeTool));
    reg.register(Box::new(AskUserQuestionTool));
    reg.register(Box::new(SleepTool));
    reg.register(Box::new(ToolSearchTool));
    reg.register(Box::new(SkillTool));
    reg.register(Box::new(BriefTool));
    reg.register(Box::new(ConfigTool));
    reg.register(Box::new(MemoryStoreTool));
    reg.register(Box::new(MemoryRecallTool));
    reg.register(Box::new(DoneTool));
    
    // Browser tools
    reg.register(Box::new(BrowserScreenshotTool));
    reg.register(Box::new(BrowserNavigateTool));
    reg.register(Box::new(BrowserClickTool));
    reg.register(Box::new(BrowserTypeTool));
    reg.register(Box::new(BrowserGetConsoleTool));
    reg.register(Box::new(BrowserGetNetworkTool));
    
    // Additional actors
    reg.register(Box::new(ActorSpawnTool));
    
    reg
}
