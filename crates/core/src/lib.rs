// Re-exports (specific items to avoid ambiguous glob imports)
pub use errors::OrchynError;
pub use traits::{StorageProvider, PlatformAdapter, Action, DeviceInfo};
pub use types::{
    AppId, SpecId, ClaimId, ActorId, PatternId, GapId,
    Confidence, Platform, LogLevel, SourceRef,
    DiagnosticEvent, NetworkEvent, RealityContext,
    Evidence, EvidenceBundle,
};
pub use mock_adapter::MockAdapter;

// Modules
pub mod errors;
pub mod traits;
pub mod types;
pub mod mock_adapter;
pub mod utils;

// Define Result alias here to avoid ambiguity
pub type Result<T> = std::result::Result<T, OrchynError>;
