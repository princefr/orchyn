use crate::schema::ToolDescriptor;
use futures::future::BoxFuture;
use orchyn_core::PlatformAdapter;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared map of named browser actors that tools mutate concurrently.
pub type ActorMap = Arc<Mutex<HashMap<String, Arc<Mutex<Box<dyn PlatformAdapter>>>>>>;

/// Factory used by [`ActorSpawnTool`](crate::ActorSpawnTool) to mint new
/// browser adapters on demand. The harness binds a concrete adapter
/// implementation (e.g. `WebAdapter`) when constructing the [`ToolContext`].
pub trait AdapterFactory: Send + Sync {
    fn spawn(&self, name: &str) -> BoxFuture<'_, orchyn_core::Result<Box<dyn PlatformAdapter>>>;
}

/// Small in-process scratchpad shared between turns and tools. Agent Mode
/// uses it as the agent's "working memory" (recall/store across tool calls
/// without round-tripping through the model context).
#[derive(Debug, Default)]
pub struct WorkingMemoryStore {
    facts: HashMap<String, Value>,
}

impl WorkingMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store(&mut self, key: impl Into<String>, value: Value) {
        self.facts.insert(key.into(), value);
    }

    pub fn recall(&self, key: &str) -> Option<&Value> {
        self.facts.get(key)
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.facts.remove(key)
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.facts.iter()
    }

    pub fn snapshot(&self) -> HashMap<String, Value> {
        self.facts.clone()
    }
}

pub type WorkingMemory = Arc<Mutex<WorkingMemoryStore>>;

/// Bag of dependencies that tools share. Cheap to clone — every field is an
/// `Arc`/`Option<Arc>`.
#[derive(Clone)]
pub struct ToolContext {
    pub actors: ActorMap,
    pub memory: WorkingMemory,
    pub adapter_factory: Option<Arc<dyn AdapterFactory>>,
    pub project_root: Arc<std::path::PathBuf>,
    /// Gate for shell-execution tools. Default off — opt in explicitly.
    pub allow_shell: bool,
    /// Snapshot of registered tools (name + description), used by
    /// `tool_search`. Empty when the registry was built without
    /// [`crate::default_registry_with_context`].
    pub tool_index: Arc<Vec<ToolDescriptor>>,
    /// Current permission mode (e.g. "plan", "default").
    pub permission_mode: Arc<Mutex<String>>,
    /// Callback to ask the user a question.
    pub ask_user: Option<Arc<dyn Fn(String) -> BoxFuture<'static, String> + Send + Sync>>,
}

impl ToolContext {
    pub fn new(project_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            actors: Arc::new(Mutex::new(HashMap::new())),
            memory: Arc::new(Mutex::new(WorkingMemoryStore::new())),
            adapter_factory: None,
            project_root: Arc::new(project_root.into()),
            allow_shell: false,
            tool_index: Arc::new(Vec::new()),
            permission_mode: Arc::new(Mutex::new("default".to_string())),
            ask_user: None,
        }
    }

    pub fn with_factory(mut self, factory: Arc<dyn AdapterFactory>) -> Self {
        self.adapter_factory = Some(factory);
        self
    }

    pub fn with_shell(mut self, allow: bool) -> Self {
        self.allow_shell = allow;
        self
    }

    pub fn with_tool_index(mut self, index: Vec<ToolDescriptor>) -> Self {
        self.tool_index = Arc::new(index);
        self
    }

    pub fn with_ask_user(mut self, cb: Arc<dyn Fn(String) -> BoxFuture<'static, String> + Send + Sync>) -> Self {
        self.ask_user = Some(cb);
        self
    }
}
