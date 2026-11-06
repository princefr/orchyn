//! Invariant sources.
//!
//! Productionizing is invariant-seeking: find what must always be true, then
//! check it on every change. A candidate invariant can come from several
//! places, none of which needs a model at check time:
//!
//! | source          | module         | example                                             |
//! |-----------------|----------------|-----------------------------------------------------|
//! | universal rules | `universal`    | every visible button is clickable; reload keeps state |
//! | code contracts  | `contracts`    | `<input required>`; `NOT NULL`; auth middleware      |
//! | transcript      | `sources`      | "the count must persist after refresh" (vibecoding chat) |
//! | known-good runs | `mined`        | `POST /api/counter/inc` always returns `value = prev + 1` |
//! | production      | `sources`      | `GET /api/items` served 2xx 10k times — keep it so  |
//! | spec            | (FEATURE.md)   | used by manifest authors                             |
//!
//! Every candidate carries evidence (where it came from) and, when it can be
//! checked mechanically, the replay steps that check it.

use crate::replay::ReplayStep;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvariantSource {
    Universal,
    CodeContract,
    Transcript,
    Mined,
    Production,
    Spec,
}

/// Where a candidate came from: a file location and/or a short note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Evidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// Quoted source text or a short explanation (keep under ~200 chars).
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantCandidate {
    /// Stable id, e.g. `contract:input:email:required` or `mined:status_always:POST /api/todos`.
    pub id: String,
    pub source: InvariantSource,
    /// One plain-English sentence: what must always hold.
    pub statement: String,
    pub evidence: Vec<Evidence>,
    /// 0.0–1.0: how sure the extractor is that this is intended behaviour.
    pub confidence: f32,
    /// Replay steps that check it, when it can be checked mechanically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<Vec<ReplayStep>>,
}

pub mod ambiguity;
pub mod contracts;
pub mod mined;
pub mod pages;
pub mod sources;
pub mod universal;
