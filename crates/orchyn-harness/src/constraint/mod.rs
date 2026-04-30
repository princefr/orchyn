//! Constraint Mode — fast, deterministic execution of pre-written manifests.
//!
//! See `ARCHITECTURE.md` and the hybrid spec: this module drives the 0.8B
//! Qwen model through a fixed list of test steps, validates each model
//! response against the on-screen element map, and emits pass/fail records.

pub mod recovery;
pub mod runner;
pub mod scheduler;
pub mod telemetry;
pub mod validation;

pub use recovery::{RecoveryAction, RecoveryPolicy, StuckDetector};
pub use runner::{ConstraintRunResult, TestRunner};
pub use scheduler::TurnScheduler;
pub use telemetry::compress_telemetry;
pub use validation::{execute_with_retry, parse_and_validate};
