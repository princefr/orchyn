//! Deterministic replay ("Constraint Mode"): run a [`ReplayManifest`] against
//! real browsers with no model in the loop.
pub mod manifest;
pub mod runner;

pub use manifest::{ReplayManifest, ReplayStep, StepOp, DEFAULT_ACTOR};
pub use runner::*;
