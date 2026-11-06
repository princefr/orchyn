//! What a page looked like after a replay step.
//!
//! The runner records one [`PageSnapshot`] per actor after every step that can
//! change the page (navigation, clicks, typing, reloads, waits). Snapshots
//! from known-good runs are mined into page-level invariants
//! (`invariants::pages`): text that is always there, elements that are always
//! visible, URLs that are always reached — per actor and step, so "what Bob
//! sees after Alice posts" becomes checkable without a spec.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageSnapshot {
    /// Actor whose browser this is.
    pub actor: String,
    /// Index of the manifest step after which it was taken.
    pub step: usize,
    /// Op name of that step (e.g. "click").
    pub op: String,
    /// `location.href` without query string or fragment.
    pub url: String,
    /// `document.body.innerText`, lines trimmed, blank lines dropped.
    pub lines: Vec<String>,
    /// Visible elements (non-zero size, not hidden, on screen or scrollable
    /// to), described stably: `tag#id` when the element has an id, otherwise
    /// `tag.class1.class2` (sorted classes) — elements with neither are
    /// skipped. Deduplicated, sorted.
    pub visible: Vec<String>,
}
