//! Universal invariants: rules every web app should satisfy, with no spec.
//!
//! Every server route that serves a page gets the same battery of checks: no
//! console errors, no failed requests, every control clickable, reloads that
//! keep the text, and a monkey pass that clicks every button. Each check is a
//! list of replay steps, so it runs deterministically through
//! [`crate::replay`] like any hand-written manifest. No model is involved.

use crate::intent::CodeIntent;
use crate::invariants::{Evidence, InvariantCandidate, InvariantSource};
use crate::replay::{DEFAULT_ACTOR, ReplayManifest, ReplayStep, StepOp};

/// Paths to check as pages: `/` first, then every GET or ANY route that serves
/// a page, in route order and without duplicates.
///
/// Excluded: API routes (`/api` and `/api/...`), parameterised or wildcard
/// routes, and static files (a last segment with an extension other than
/// `.html`).
pub fn page_paths(intent: &CodeIntent) -> Vec<String> {
    let mut paths = vec!["/".to_string()];
    for route in intent
        .server_routes
        .iter()
        .filter(|r| serves_pages(&r.method))
    {
        let path = normalise_path(&route.path);
        if is_page_path(&path) && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// Every universal candidate for every page path, in page order.
pub fn candidates(intent: &CodeIntent) -> Vec<InvariantCandidate> {
    page_paths(intent)
        .iter()
        .flat_map(|page| page_candidates(intent, page))
        .collect()
}

/// One manifest that runs every candidate's steps in order.
///
/// The runner halts at the first failure, so a failure hides the candidates
/// after it. Use [`manifests`] to get one manifest per candidate.
pub fn manifest(intent: &CodeIntent, name: &str) -> ReplayManifest {
    ReplayManifest {
        name: name.to_string(),
        base_url: String::new(),
        steps: candidates(intent)
            .into_iter()
            .flat_map(|c| c.check.unwrap_or_default())
            .collect(),
    }
}

/// One manifest per candidate, named by the candidate's id.
pub fn manifests(intent: &CodeIntent) -> Vec<(InvariantCandidate, ReplayManifest)> {
    candidates(intent)
        .into_iter()
        .map(|candidate| {
            let manifest = ReplayManifest {
                name: candidate.id.clone(),
                base_url: String::new(),
                steps: candidate.check.clone().unwrap_or_default(),
            };
            (candidate, manifest)
        })
        .collect()
}

fn page_candidates(intent: &CodeIntent, page: &str) -> Vec<InvariantCandidate> {
    let evidence = vec![page_evidence(intent, page)];
    let build = |kind: &str, statement: String, confidence: f32, steps: Vec<ReplayStep>| {
        InvariantCandidate {
            id: format!("universal:{kind}:{page}"),
            source: InvariantSource::Universal,
            statement,
            evidence: evidence.clone(),
            confidence,
            check: Some(steps),
        }
    };

    vec![
        build(
            "console-clean",
            format!("Loading {page} logs no console errors or uncaught exceptions."),
            0.9,
            vec![step(goto(page)), step(StepOp::ExpectConsoleClean)],
        ),
        build(
            "no-failed-requests",
            format!("Every network request made while loading {page} succeeds."),
            0.9,
            vec![step(goto(page)), step(StepOp::ExpectNoFailedRequests)],
        ),
        build(
            "all-clickable",
            format!(
                "Every visible, enabled control on {page} can be clicked by a real pointer, \
                 not covered by another element."
            ),
            0.9,
            vec![step(goto(page)), step(StepOp::ExpectAllClickable)],
        ),
        build(
            "reload-stable",
            format!("Reloading {page} leaves its visible text unchanged."),
            0.6,
            vec![
                step(goto(page)),
                step(StepOp::WaitMs { ms: 500 }),
                step(snapshot("before")),
                step(StepOp::Reload),
                step(StepOp::WaitMs { ms: 500 }),
                step(unchanged("before")),
            ],
        ),
        build(
            "interactions-clean",
            format!(
                "Clicking each button on {page} produces no console errors and no failed requests."
            ),
            0.7,
            vec![
                step(goto(page)),
                step(StepOp::ResetTelemetry),
                step(StepOp::ClickEach { selector: None }),
                step(StepOp::ExpectConsoleClean),
                step(StepOp::ExpectNoFailedRequests),
            ],
        ),
        build(
            "actors-agree",
            format!(
                "What one user changes on {page} stays visible to another user across reloads."
            ),
            0.5,
            vec![
                step_as("a", goto(page)),
                step_as("a", StepOp::ClickEach { selector: None }),
                step_as("b", goto(page)),
                step_as("b", StepOp::WaitMs { ms: 1500 }),
                step_as("b", snapshot("b")),
                step_as("b", StepOp::Reload),
                step_as("b", StepOp::WaitMs { ms: 500 }),
                step_as("b", unchanged("b")),
            ],
        ),
    ]
}

fn page_evidence(intent: &CodeIntent, page: &str) -> Evidence {
    let route = intent
        .server_routes
        .iter()
        .find(|r| serves_pages(&r.method) && normalise_path(&r.path) == page);
    match route {
        Some(route) => Evidence {
            file: Some(route.file.clone()),
            line: Some(route.line),
            note: format!(
                "applies to every page; route from {}:{}",
                route.file.display(),
                route.line
            ),
        },
        None => Evidence {
            file: None,
            line: None,
            note: "applies to every page; no server route serves it".to_string(),
        },
    }
}

fn serves_pages(method: &str) -> bool {
    method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("ANY")
}

fn normalise_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn is_page_path(path: &str) -> bool {
    if path == "/api" || path.starts_with("/api/") {
        return false;
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments
        .iter()
        .any(|s| s.starts_with(':') || s.contains(['*', '{']))
    {
        return false;
    }
    match segments.last() {
        Some(last) => !last.contains('.') || last.ends_with(".html"),
        None => true,
    }
}

fn goto(page: &str) -> StepOp {
    StepOp::Goto {
        path: page.to_string(),
    }
}

fn snapshot(key: &str) -> StepOp {
    StepOp::SnapshotText {
        key: key.to_string(),
    }
}

fn unchanged(key: &str) -> StepOp {
    StepOp::ExpectTextUnchanged {
        key: key.to_string(),
    }
}

fn step(op: StepOp) -> ReplayStep {
    step_as(DEFAULT_ACTOR, op)
}

fn step_as(actor: &str, op: StepOp) -> ReplayStep {
    ReplayStep {
        actor: actor.to_string(),
        op,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::Endpoint;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn route(method: &str, path: &str, line: usize) -> Endpoint {
        Endpoint {
            method: method.to_string(),
            path: path.to_string(),
            file: PathBuf::from("server.js"),
            line,
        }
    }

    fn intent(routes: Vec<Endpoint>) -> CodeIntent {
        CodeIntent {
            client_calls: Vec::new(),
            server_routes: routes,
        }
    }

    /// Two pages: `/` (line 3) and `/about` (line 9).
    fn two_page_intent() -> CodeIntent {
        intent(vec![route("GET", "/", 3), route("GET", "/about", 9)])
    }

    #[test]
    fn page_paths_keeps_root_first_and_drops_non_pages() {
        let intent = intent(vec![
            route("POST", "/submit", 3),
            route("GET", "/about", 4),
            route("ANY", "/", 5),
            route("GET", "/about", 6),
            route("GET", "/api/items", 7),
            route("GET", "/api", 8),
            route("GET", "/apiary", 9),
            route("GET", "/users/:id", 10),
            route("GET", "/static/app.js", 11),
            route("GET", "/contact.html", 12),
        ]);
        assert_eq!(
            page_paths(&intent),
            vec!["/", "/about", "/apiary", "/contact.html"]
        );
    }

    #[test]
    fn page_paths_is_root_when_there_are_no_routes() {
        assert_eq!(page_paths(&CodeIntent::default()), vec!["/".to_string()]);
    }

    #[test]
    fn candidates_cover_each_page_with_stable_ids() {
        let cands = candidates(&two_page_intent());
        assert_eq!(cands.len(), 12);

        let ids: HashSet<&str> = cands.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids.len(), 12);
        assert_eq!(
            cands[..6].iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec![
                "universal:console-clean:/",
                "universal:no-failed-requests:/",
                "universal:all-clickable:/",
                "universal:reload-stable:/",
                "universal:interactions-clean:/",
                "universal:actors-agree:/",
            ]
        );
        assert!(ids.contains("universal:actors-agree:/about"));

        let by_id = |id: &str| cands.iter().find(|c| c.id == id).unwrap();
        assert_eq!(by_id("universal:reload-stable:/").confidence, 0.6);
        assert_eq!(by_id("universal:interactions-clean:/about").confidence, 0.7);
        assert_eq!(by_id("universal:actors-agree:/about").confidence, 0.5);
        assert_eq!(by_id("universal:console-clean:/about").confidence, 0.9);
        assert!(cands.iter().all(|c| c.source == InvariantSource::Universal));
    }

    #[test]
    fn every_candidate_has_a_statement_and_a_non_empty_check() {
        for c in candidates(&two_page_intent()) {
            assert!(!c.statement.is_empty(), "{}", c.id);
            assert!(
                c.check.as_ref().is_some_and(|steps| !steps.is_empty()),
                "{}",
                c.id
            );
        }
    }

    #[test]
    fn evidence_points_at_the_route_that_serves_the_page() {
        let cands = candidates(&two_page_intent());
        let about = cands
            .iter()
            .find(|c| c.id == "universal:console-clean:/about")
            .unwrap();
        assert_eq!(about.evidence[0].file, Some(PathBuf::from("server.js")));
        assert_eq!(about.evidence[0].line, Some(9));
        assert!(about.evidence[0].note.contains("server.js:9"));
    }

    #[test]
    fn manifest_concatenates_steps_and_round_trips_through_json() {
        let intent = two_page_intent();
        let m = manifest(&intent, "universal");
        let expected: usize = candidates(&intent)
            .iter()
            .map(|c| c.check.as_ref().map_or(0, Vec::len))
            .sum();
        assert_eq!(m.steps.len(), expected);
        assert_eq!(m.actors(), vec!["default", "a", "b"]);

        let back = ReplayManifest::from_json(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn manifests_gives_one_round_tripping_manifest_per_candidate() {
        let intent = two_page_intent();
        let pairs = manifests(&intent);
        assert_eq!(pairs.len(), candidates(&intent).len());
        for (candidate, per) in pairs {
            assert_eq!(per.name, candidate.id);
            assert_eq!(per.base_url, "");
            assert_eq!(Some(per.steps.clone()), candidate.check.clone());
            let back = ReplayManifest::from_json(&serde_json::to_string(&per).unwrap()).unwrap();
            assert_eq!(back, per);
        }
    }
}
