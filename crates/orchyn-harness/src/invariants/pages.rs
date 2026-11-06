//! Page-level invariants mined from replay snapshots.
//!
//! `mine` reads the [`PageSnapshot`]s of several known-good runs of the same
//! driver manifest and keeps what held at each `(actor, step)` in every run:
//! text that was shown, elements that were visible, paths that were reached.
//! `check` replays those facts against a new run. Pure: no IO, no model.
//!
//! These catch what API mining cannot: a greeting that stops showing, a post
//! one actor never sees, an error message that is set but hidden.

use crate::invariants::{Evidence, InvariantCandidate, InvariantSource};
use crate::replay::PageSnapshot;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Lines longer than this are not mined: they are usually generated content.
const MAX_LINE_CHARS: usize = 200;
/// Characters of the value kept in a candidate id.
const ID_VALUE_CHARS: usize = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PageInvariant {
    /// `line` was shown to `actor` after `step` in every known-good run.
    TextAlways {
        actor: String,
        step: usize,
        line: String,
    },
    /// `element` was visible to `actor` after `step` in every known-good run.
    VisibleAlways {
        actor: String,
        step: usize,
        element: String,
    },
    /// `actor` was on `url_path` (origin stripped) after `step` in every known-good run.
    UrlAlways {
        actor: String,
        step: usize,
        url_path: String,
    },
}

/// A mined page invariant that a trace broke. `observed` is a short description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageViolation {
    pub invariant: PageInvariant,
    pub observed: String,
}

/// `(actor, step)`: the key snapshots are matched on across runs.
type Key = (String, usize);

/// What one `(actor, step)` looked like in the previous common step of the same actor.
struct Seen {
    url: Option<String>,
    text: BTreeSet<String>,
    visible: BTreeSet<String>,
}

/// Mine page invariants from known-good traces. Each trace is the snapshots of one run.
///
/// Needs at least two traces. A key is used only if it is present in every trace.
/// Text and visible elements must be shared by every trace at that key. Text is
/// reduced to first appearances: a line already shown on the same page at the
/// previous step of the same actor is dropped, except after a `reload` or `goto`,
/// which restate the page. The same reduction applies to visible elements. A URL
/// path is kept when every trace at the key shares it.
pub fn mine(traces: &[Vec<PageSnapshot>]) -> Vec<PageInvariant> {
    if traces.len() < 2 {
        return Vec::new();
    }
    let indexed: Vec<BTreeMap<Key, &PageSnapshot>> = traces.iter().map(|t| index(t)).collect();
    let mut keys: BTreeSet<Key> = indexed[0].keys().cloned().collect();
    for idx in &indexed[1..] {
        keys.retain(|k| idx.contains_key(k));
    }

    let mut out = Vec::new();
    // Last common key per actor. Keys iterate in (actor, step) order, so this is
    // the previous step of the same actor.
    let mut previous: BTreeMap<String, Seen> = BTreeMap::new();
    for key in &keys {
        let (actor, step) = (&key.0, key.1);
        let snaps: Vec<&PageSnapshot> = indexed.iter().map(|idx| idx[key]).collect();
        let url = shared_url(&snaps);
        let text = shared(&snaps, |s| {
            s.lines
                .iter()
                .filter(|l| l.chars().count() <= MAX_LINE_CHARS)
                .cloned()
                .collect()
        });
        let visible = shared(&snaps, |s| s.visible.iter().cloned().collect());

        let restated = snaps
            .iter()
            .any(|s| matches!(s.op.as_str(), "reload" | "goto"));
        let before = previous
            .get(actor)
            .filter(|p| !restated && url.is_some() && p.url == url);

        for line in &text {
            if before.is_some_and(|p| p.text.contains(line)) {
                continue;
            }
            out.push(PageInvariant::TextAlways {
                actor: actor.clone(),
                step,
                line: line.clone(),
            });
        }
        for element in &visible {
            if before.is_some_and(|p| p.visible.contains(element)) {
                continue;
            }
            out.push(PageInvariant::VisibleAlways {
                actor: actor.clone(),
                step,
                element: element.clone(),
            });
        }
        if let Some(path) = &url {
            out.push(PageInvariant::UrlAlways {
                actor: actor.clone(),
                step,
                url_path: path.clone(),
            });
        }
        previous.insert(actor.clone(), Seen { url, text, visible });
    }

    out.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    out
}

/// Replay mined page invariants against one run. Reports every broken observation,
/// and one "step not reached" per `(actor, step)` the run does not contain.
pub fn check(invariants: &[PageInvariant], trace: &[PageSnapshot]) -> Vec<PageViolation> {
    let seen = index(trace);
    let mut unreached: BTreeSet<Key> = BTreeSet::new();
    let mut out = Vec::new();
    for inv in invariants {
        let (actor, step, _, _) = parts(inv);
        let key = (actor.to_string(), step);
        let Some(snap) = seen.get(&key) else {
            if unreached.insert(key) {
                out.push(PageViolation {
                    invariant: inv.clone(),
                    observed: "step not reached".to_string(),
                });
            }
            continue;
        };
        if let Some(observed) = observe(inv, snap) {
            out.push(PageViolation {
                invariant: inv.clone(),
                observed,
            });
        }
    }
    out
}

/// Turn mined page invariants into candidates for the shared invariant pipeline.
pub fn to_candidates(invariants: &[PageInvariant]) -> Vec<InvariantCandidate> {
    invariants
        .iter()
        .map(|inv| {
            let (actor, step, kind, value) = parts(inv);
            InvariantCandidate {
                id: format!(
                    "page:{kind}:{actor}:{step}:{}",
                    value.chars().take(ID_VALUE_CHARS).collect::<String>()
                ),
                source: InvariantSource::Mined,
                statement: statement(inv),
                evidence: vec![Evidence {
                    file: None,
                    line: None,
                    note: format!(
                        "page snapshots of {actor} after step {step}, same in every known-good run"
                    ),
                }],
                confidence: 0.75,
                check: None,
            }
        })
        .collect()
}

// ── checking ─────────────────────────────────────────────────────────────────

/// The broken observation of `inv` in `snap`, if any.
fn observe(inv: &PageInvariant, snap: &PageSnapshot) -> Option<String> {
    match inv {
        PageInvariant::TextAlways { line, .. } => {
            (!snap.lines.contains(line)).then_some("text not shown".to_string())
        }
        PageInvariant::VisibleAlways { element, .. } => {
            (!snap.visible.contains(element)).then_some("not visible".to_string())
        }
        PageInvariant::UrlAlways {
            url_path: expected, ..
        } => {
            let got = url_path(&snap.url);
            (got != *expected).then(|| format!("url was {got}"))
        }
    }
}

// ── mining ───────────────────────────────────────────────────────────────────

/// First snapshot per `(actor, step)`.
fn index(trace: &[PageSnapshot]) -> BTreeMap<Key, &PageSnapshot> {
    let mut map = BTreeMap::new();
    for s in trace {
        map.entry((s.actor.clone(), s.step)).or_insert(s);
    }
    map
}

/// Items present in every snapshot, as produced by `items`.
fn shared<F>(snaps: &[&PageSnapshot], items: F) -> BTreeSet<String>
where
    F: Fn(&PageSnapshot) -> BTreeSet<String>,
{
    let mut sets = snaps.iter().map(|s| items(s));
    let Some(first) = sets.next() else {
        return BTreeSet::new();
    };
    sets.fold(first, |acc, set| acc.intersection(&set).cloned().collect())
}

/// The path shared by every snapshot at a key, or `None` if they disagree.
fn shared_url(snaps: &[&PageSnapshot]) -> Option<String> {
    let first = url_path(&snaps[0].url);
    snaps
        .iter()
        .all(|s| url_path(&s.url) == first)
        .then_some(first)
}

/// Path of a URL with origin, query and fragment removed. `/` when empty.
fn url_path(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or("");
    let path = match url.find("://") {
        Some(i) => {
            let rest = &url[i + 3..];
            rest.find('/').map_or("/", |j| &rest[j..])
        }
        None => url,
    };
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

// ── presentation ─────────────────────────────────────────────────────────────

/// `(actor, step, kind, value)` of an invariant. Also the sort key.
fn parts(inv: &PageInvariant) -> (&str, usize, &'static str, &str) {
    match inv {
        PageInvariant::TextAlways { actor, step, line } => {
            (actor.as_str(), *step, "text_always", line.as_str())
        }
        PageInvariant::VisibleAlways {
            actor,
            step,
            element,
        } => (actor.as_str(), *step, "visible_always", element.as_str()),
        PageInvariant::UrlAlways {
            actor,
            step,
            url_path,
        } => (actor.as_str(), *step, "url_always", url_path.as_str()),
    }
}

fn sort_key(inv: &PageInvariant) -> (&str, usize, &'static str, &str) {
    parts(inv)
}

fn statement(inv: &PageInvariant) -> String {
    match inv {
        PageInvariant::TextAlways { actor, step, line } => {
            format!("After step {step}, {actor} sees “{line}”.")
        }
        PageInvariant::VisibleAlways {
            actor,
            step,
            element,
        } => format!("After step {step}, {element} is visible to {actor}."),
        PageInvariant::UrlAlways {
            actor,
            step,
            url_path,
        } => format!("After step {step}, {actor} is on {url_path}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        actor: &str,
        step: usize,
        op: &str,
        url: &str,
        lines: &[&str],
        visible: &[&str],
    ) -> PageSnapshot {
        PageSnapshot {
            actor: actor.to_string(),
            step,
            op: op.to_string(),
            url: url.to_string(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            visible: visible.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn text(actor: &str, step: usize, line: &str) -> PageInvariant {
        PageInvariant::TextAlways {
            actor: actor.to_string(),
            step,
            line: line.to_string(),
        }
    }

    fn visible(actor: &str, step: usize, element: &str) -> PageInvariant {
        PageInvariant::VisibleAlways {
            actor: actor.to_string(),
            step,
            element: element.to_string(),
        }
    }

    fn url(actor: &str, step: usize, url_path: &str) -> PageInvariant {
        PageInvariant::UrlAlways {
            actor: actor.to_string(),
            step,
            url_path: url_path.to_string(),
        }
    }

    /// Login flow on `origin`. `greeting` is the line shown after the submit.
    fn login_run(origin: &str, after_submit: &str, greeting: &str) -> Vec<PageSnapshot> {
        let login = format!("{origin}/login");
        vec![
            snap("default", 0, "goto", &login, &["Sign in"], &["form#login"]),
            snap("default", 1, "type", &login, &["Sign in"], &["form#login"]),
            snap("default", 2, "click", &login, &["Sign in"], &["form#login"]),
            snap(
                "default",
                3,
                "click",
                &format!("{origin}{after_submit}"),
                &[greeting, "Logout"],
                &["div#home"],
            ),
        ]
    }

    #[test]
    fn login_greeting_and_url_are_mined_and_checked() {
        let good = vec![
            login_run("http://127.0.0.1:4001", "/dashboard", "Welcome, alice"),
            login_run("http://127.0.0.1:4002", "/dashboard", "Welcome, alice"),
        ];
        let inv = mine(&good);
        assert!(inv.contains(&text("default", 3, "Welcome, alice")));
        assert!(inv.contains(&url("default", 3, "/dashboard")));

        // Bug: the greeting is replaced by a sign-in prompt, but the URL is still /dashboard.
        let bad = login_run("http://127.0.0.1:4003", "/dashboard", "Please sign in");
        let v = check(&inv, &bad);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].invariant, text("default", 3, "Welcome, alice"));
        assert_eq!(v[0].observed, "text not shown");
        assert!(
            !v.iter()
                .any(|v| matches!(v.invariant, PageInvariant::UrlAlways { .. }))
        );

        // A failed login that never leaves /login breaks the URL invariant.
        let stuck = login_run("http://127.0.0.1:4004", "/login", "Please sign in");
        let v = check(&inv, &stuck);
        assert!(
            v.iter()
                .any(|v| v.invariant == url("default", 3, "/dashboard")
                    && v.observed == "url was /login")
        );
    }

    #[test]
    fn text_seen_by_one_actor_is_checked_for_that_actor() {
        let run = |bob_step6: &[&str]| {
            vec![
                snap("alice", 5, "click", "http://h/list", &["Buy milk"], &[]),
                snap("bob", 2, "goto", "http://h/list", &["Nothing yet"], &[]),
                snap("bob", 6, "click", "http://h/list", bob_step6, &[]),
            ]
        };
        let good = vec![
            run(&["Nothing yet", "Buy milk"]),
            run(&["Nothing yet", "Buy milk"]),
        ];
        let inv = mine(&good);
        assert!(inv.contains(&text("bob", 6, "Buy milk")));
        assert!(inv.contains(&text("alice", 5, "Buy milk")));

        let bad = run(&["Nothing yet"]);
        let v = check(&inv, &bad);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].invariant, text("bob", 6, "Buy milk"));
        assert_eq!(v[0].observed, "text not shown");
    }

    #[test]
    fn hidden_element_is_flagged() {
        let run = |visible: &[&str]| {
            vec![
                snap(
                    "default",
                    0,
                    "goto",
                    "http://h/signup",
                    &["Join"],
                    &["form#signup"],
                ),
                snap("default", 1, "click", "http://h/signup", &["Join"], visible),
            ]
        };
        let good = vec![
            run(&["form#signup", "div#email-error"]),
            run(&["form#signup", "div#email-error"]),
        ];
        let inv = mine(&good);
        assert!(inv.contains(&visible("default", 1, "div#email-error")));

        let v = check(&inv, &run(&["form#signup"]));
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].invariant, visible("default", 1, "div#email-error"));
        assert_eq!(v[0].observed, "not visible");
    }

    #[test]
    fn varying_text_is_not_mined() {
        let run = |session: &str| {
            vec![
                snap(
                    "default",
                    0,
                    "goto",
                    "http://h/",
                    &["Hello", &format!("Session {session}")],
                    &[],
                ),
                snap("default", 1, "click", "http://h/", &["Hello"], &[]),
            ]
        };
        let inv = mine(&[run("1a2b"), run("9f8e")]);
        assert!(inv.contains(&text("default", 0, "Hello")));
        assert!(!inv.iter().any(|i| matches!(
            i,
            PageInvariant::TextAlways { line, .. } if line.starts_with("Session")
        )));
    }

    #[test]
    fn repeated_header_is_mined_once_and_kept_after_reload() {
        let run = || {
            vec![
                snap(
                    "default",
                    0,
                    "goto",
                    "http://h/dashboard",
                    &["Dashboard"],
                    &[],
                ),
                snap(
                    "default",
                    1,
                    "click",
                    "http://h/dashboard",
                    &["Dashboard"],
                    &[],
                ),
                snap(
                    "default",
                    2,
                    "click",
                    "http://h/dashboard",
                    &["Dashboard", "Saved"],
                    &[],
                ),
                snap(
                    "default",
                    3,
                    "click",
                    "http://h/dashboard",
                    &["Dashboard"],
                    &[],
                ),
                snap(
                    "default",
                    4,
                    "reload",
                    "http://h/dashboard",
                    &["Dashboard"],
                    &[],
                ),
            ]
        };
        let inv = mine(&[run(), run()]);
        let headers: Vec<&PageInvariant> = inv
            .iter()
            .filter(|i| matches!(i, PageInvariant::TextAlways { line, .. } if line == "Dashboard"))
            .collect();
        let steps: Vec<usize> = headers
            .iter()
            .map(|i| match i {
                PageInvariant::TextAlways { step, .. } => *step,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(steps, vec![0, 4], "{inv:?}");
        assert!(inv.contains(&text("default", 2, "Saved")));
    }

    #[test]
    fn step_not_reached_is_reported_once_per_key() {
        let inv = vec![
            text("default", 3, "Welcome"),
            url("default", 3, "/dashboard"),
            visible("default", 3, "div#home"),
            text("default", 4, "x"),
        ];
        let trace = vec![snap("default", 0, "goto", "http://h/", &[], &[])];
        let v = check(&inv, &trace);
        let unreached: Vec<&PageViolation> = v
            .iter()
            .filter(|v| v.observed == "step not reached")
            .collect();
        assert_eq!(unreached.len(), 2, "{v:?}");
        assert_eq!(unreached[0].invariant, text("default", 3, "Welcome"));
        assert_eq!(unreached[1].invariant, text("default", 4, "x"));
    }

    #[test]
    fn single_trace_mines_nothing() {
        assert!(mine(&[]).is_empty());
        let one = vec![login_run("http://h", "/dashboard", "Welcome")];
        assert!(mine(&one).is_empty());
    }

    #[test]
    fn url_path_strips_origin_query_and_fragment() {
        assert_eq!(
            url_path("http://127.0.0.1:4001/dashboard?x=1#top"),
            "/dashboard"
        );
        assert_eq!(url_path("https://example.test"), "/");
        assert_eq!(url_path("/login"), "/login");
    }

    #[test]
    fn candidates_have_stable_ids_and_statements() {
        let inv = vec![
            text("bob", 5, "Buy milk"),
            visible("default", 2, "#email-error"),
            url("default", 3, "/dashboard"),
        ];
        let c = to_candidates(&inv);
        assert_eq!(c[0].id, "page:text_always:bob:5:Buy milk");
        assert_eq!(c[0].statement, "After step 5, bob sees “Buy milk”.");
        assert_eq!(
            c[1].statement,
            "After step 2, #email-error is visible to default."
        );
        assert_eq!(c[2].statement, "After step 3, default is on /dashboard.");
        assert!(c.iter().all(|c| c.source == InvariantSource::Mined
            && c.check.is_none()
            && (c.confidence - 0.75).abs() < f32::EPSILON));
    }

    #[test]
    fn invariants_round_trip_through_json() {
        let inv = text("bob", 5, "Buy milk");
        let json = serde_json::to_string(&inv).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"text_always","actor":"bob","step":5,"line":"Buy milk"}"#
        );
        assert_eq!(serde_json::from_str::<PageInvariant>(&json).unwrap(), inv);
    }
}
