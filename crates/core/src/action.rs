//! Hybrid harness action vocabulary used by both Constraint Mode (manifest
//! execution) and Agent Mode (tool-using exploration).
//!
//! Distinct from the legacy `crate::traits::Action` enum (Tap/Type/Wait/Reset)
//! which the older phase-based harness still uses for coordinate-based input.
//! New code targets [`Action`] in this module.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Action {
    Navigate { value: String },
    Click { target: u8 },
    Type { target: u8, value: String },
    Clear { target: u8 },
    Drag { target: u8, destination: u8 },
    Wait { reason: String },
    AssertSuccess { assertion_type: AssertionType, expected: String },
    AssertFailure { assertion_type: AssertionType, expected: String },
    WsWait { substring: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssertionType {
    ElementVisible,
    ElementHidden,
    TextContains,
    UrlIs,
    ConsoleClean,
    NetworkOk,
    WsReceived,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnnotatedElement {
    pub id: u8,
    pub element_type: String,
    pub text: Option<String>,
    pub bounds: BoundingBox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmDecision {
    pub actor_id: String,
    pub manifest_step: usize,
    #[serde(default)]
    pub thought: String,
    #[serde(flatten)]
    pub action: Action,
    #[serde(default)]
    pub recovery: Option<RecoverySuggestion>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoverySuggestion {
    RetryStep,
    SkipAssert,
    Halt,
}
