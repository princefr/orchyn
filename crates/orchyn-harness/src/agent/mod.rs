//! Agent Mode — autonomous tool-using exploration of an app.
//!
//! This module is the inverse of `constraint`: instead of replaying a fixed
//! manifest, it lets the local 0.8B model drive itself through an app via the
//! [`orchyn_toolkit`] tool registry, accumulating findings (gaps, proposed
//! tests, fixes) in working memory until it emits `done`.

pub mod harness;
pub mod memory;

pub use harness::{AgentHarness, AgentReport, TurnObservation};
pub use memory::{TurnRecord, WorkingMemory};
