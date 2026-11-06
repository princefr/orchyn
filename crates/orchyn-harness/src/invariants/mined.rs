//! Invariants mined from known-good runs (Daikon-style).
//!
//! `mine` reads API exchanges recorded while the app behaved correctly and keeps
//! only the facts that held in every observation of an endpoint. `check` replays
//! those facts against a new trace and reports what no longer holds. Pure: no IO,
//! no model.

use crate::invariants::{Evidence, InvariantCandidate, InvariantSource};
use orchyn_core::Exchange;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Deepest JSON pointer (in segments) a shape describes, so `/items/0/name` is the limit.
const MAX_SHAPE_DEPTH: usize = 3;
/// Object levels expanded below the root: top-level keys plus one level deeper.
const MAX_OBJECT_LEVEL: usize = 2;
const DELTA_EPS: f64 = 1e-9;

/// Normalised `METHOD /path` key. Origin and query are dropped; all-digit and
/// uuid-looking segments become `:n`. Example: `POST /api/todos/:n`.
pub fn endpoint_key(method: &str, url: &str) -> String {
    let path = path_of(url);
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| if is_numeric(s) || is_uuid(s) { ":n" } else { s })
        .collect();
    format!("{} /{}", method.to_ascii_uppercase(), segments.join("/"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mined {
    /// Every observation of the endpoint returned this status.
    StatusAlways { endpoint: String, status: u16 },
    /// Every JSON response had these pointer -> type entries.
    ShapeAlways {
        endpoint: String,
        shape: BTreeMap<String, String>,
    },
    /// The array at `pointer` was non-empty in every observation.
    NonEmptyArray { endpoint: String, pointer: String },
    /// The value at `pointer` was never null.
    NonNull { endpoint: String, pointer: String },
    /// Within a trace, consecutive responses differ at `pointer` by exactly `delta`.
    NumericStep {
        endpoint: String,
        pointer: String,
        delta: f64,
    },
}

/// A mined invariant that a trace broke. `observed` is a short human description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    pub invariant: Mined,
    pub observed: String,
}

/// Mine invariants from known-good traces. Each trace is the exchanges of one run.
///
/// An invariant is kept only when its endpoint appears in at least two traces and
/// it held in every observation across all traces. Only exchanges whose response
/// body parses as JSON contribute to shape, non-null, non-empty and step invariants.
pub fn mine(traces: &[Vec<Exchange>]) -> Vec<Mined> {
    let mut endpoints: BTreeMap<String, Endpoint> = BTreeMap::new();
    for (t, trace) in traces.iter().enumerate() {
        for e in trace {
            let ep = endpoints
                .entry(endpoint_key(&e.method, &e.url))
                .or_default();
            ep.traces.insert(t);
            ep.statuses.push(e.status);
            if let Some(v) = json_body(e) {
                ep.shapes.push(flatten(&v));
                ep.sequences.entry(t).or_default().push(v);
            }
        }
    }

    let mut out = Vec::new();
    for (endpoint, ep) in &endpoints {
        if ep.traces.len() < 2 {
            continue;
        }
        if ep.statuses.len() >= 2 && ep.statuses.iter().all(|s| *s == ep.statuses[0]) {
            out.push(Mined::StatusAlways {
                endpoint: endpoint.clone(),
                status: ep.statuses[0],
            });
        }
        if ep.shapes.is_empty() {
            continue;
        }

        let common = common_entries(&ep.shapes);
        if !common.is_empty() {
            out.push(Mined::ShapeAlways {
                endpoint: endpoint.clone(),
                shape: common
                    .iter()
                    .map(|(p, ty)| (p.clone(), (*ty).to_string()))
                    .collect(),
            });
        }
        for (p, ty) in &common {
            if *ty == "array"
                && ep
                    .shapes
                    .iter()
                    .all(|s| s.get(p).is_some_and(|(_, n)| *n > 0))
            {
                out.push(Mined::NonEmptyArray {
                    endpoint: endpoint.clone(),
                    pointer: p.clone(),
                });
            }
            if *ty != "null" && !p.is_empty() {
                out.push(Mined::NonNull {
                    endpoint: endpoint.clone(),
                    pointer: p.clone(),
                });
            }
            if let Some(delta) = (*ty == "number")
                .then(|| numeric_step(&ep.sequences, p))
                .flatten()
            {
                out.push(Mined::NumericStep {
                    endpoint: endpoint.clone(),
                    pointer: p.clone(),
                    delta,
                });
            }
        }
    }

    out.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    out
}

/// Replay mined invariants against one trace. Reports every broken observation, and
/// one "endpoint never called" per endpoint that the trace does not contain.
pub fn check(invariants: &[Mined], trace: &[Exchange]) -> Vec<Violation> {
    let mut by_endpoint: BTreeMap<String, Vec<&Exchange>> = BTreeMap::new();
    for e in trace {
        by_endpoint
            .entry(endpoint_key(&e.method, &e.url))
            .or_default()
            .push(e);
    }

    // For each absent endpoint, report once: on its StatusAlways if mined, else the first.
    let mut representative: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, inv) in invariants.iter().enumerate() {
        let ep = endpoint_of(inv);
        if by_endpoint.contains_key(ep) {
            continue;
        }
        match representative.get(ep) {
            None => {
                representative.insert(ep, i);
            }
            Some(&j)
                if matches!(inv, Mined::StatusAlways { .. })
                    && !matches!(invariants[j], Mined::StatusAlways { .. }) =>
            {
                representative.insert(ep, i);
            }
            Some(_) => {}
        }
    }

    let mut out = Vec::new();
    for (i, inv) in invariants.iter().enumerate() {
        let ep = endpoint_of(inv);
        let Some(exchanges) = by_endpoint.get(ep) else {
            if representative.get(ep) == Some(&i) {
                out.push(Violation {
                    invariant: inv.clone(),
                    observed: "endpoint never called".to_string(),
                });
            }
            continue;
        };
        for observed in observations(inv, exchanges) {
            out.push(Violation {
                invariant: inv.clone(),
                observed,
            });
        }
    }
    out
}

/// Turn mined invariants into candidates for the shared invariant pipeline.
pub fn to_candidates(invariants: &[Mined]) -> Vec<InvariantCandidate> {
    invariants
        .iter()
        .map(|inv| InvariantCandidate {
            id: candidate_id(inv),
            source: InvariantSource::Mined,
            statement: statement(inv),
            evidence: vec![Evidence {
                file: None,
                line: None,
                note: format!("mined from known-good traces: {}", endpoint_of(inv)),
            }],
            confidence: 0.8,
            check: None,
        })
        .collect()
}

// ── checking ─────────────────────────────────────────────────────────────────

/// Broken observations of `inv` in the exchanges of one endpoint, in trace order.
fn observations(inv: &Mined, exchanges: &[&Exchange]) -> Vec<String> {
    let mut obs = Vec::new();
    match inv {
        Mined::StatusAlways { status, .. } => {
            for e in exchanges.iter().filter(|e| e.status != *status) {
                obs.push(format!("status {}", e.status));
            }
        }
        Mined::ShapeAlways { shape, .. } => {
            for v in exchanges.iter().copied().filter_map(json_body) {
                for (p, ty) in shape {
                    match v.pointer(p) {
                        None if !under_empty_array(&v, p) => {
                            obs.push(format!("{} missing (expected {ty})", label(p)));
                        }
                        Some(actual) if type_name(actual) != ty.as_str() => {
                            obs.push(format!(
                                "{} is {} (expected {ty})",
                                label(p),
                                type_name(actual)
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
        Mined::NonNull { pointer, .. } => {
            for v in exchanges.iter().copied().filter_map(json_body) {
                match v.pointer(pointer) {
                    None if !under_empty_array(&v, pointer) => {
                        obs.push(format!("{} missing", label(pointer)));
                    }
                    Some(Value::Null) => obs.push(format!("{} is null", label(pointer))),
                    _ => {}
                }
            }
        }
        Mined::NonEmptyArray { pointer, .. } => {
            for v in exchanges.iter().copied().filter_map(json_body) {
                match v.pointer(pointer) {
                    None if !under_empty_array(&v, pointer) => {
                        obs.push(format!("{} missing", label(pointer)));
                    }
                    Some(Value::Array(a)) if a.is_empty() => {
                        obs.push(format!("{} is empty", label(pointer)));
                    }
                    Some(Value::Array(_)) | None => {}
                    Some(other) => obs.push(format!(
                        "{} is {} (expected array)",
                        label(pointer),
                        type_name(other)
                    )),
                }
            }
        }
        Mined::NumericStep { pointer, delta, .. } => {
            if exchanges.len() >= 2 {
                let nums: Vec<Option<f64>> = exchanges
                    .iter()
                    .copied()
                    .filter_map(json_body)
                    .map(|v| v.pointer(pointer).and_then(Value::as_f64))
                    .collect();
                for w in nums.windows(2) {
                    if let [Some(a), Some(b)] = w {
                        let d = *b - *a;
                        if (d - delta).abs() > DELTA_EPS {
                            obs.push(format!(
                                "delta {} (expected {})",
                                fmt_num(d),
                                fmt_num(*delta)
                            ));
                        }
                    }
                }
            }
        }
    }
    obs
}

// ── mining ───────────────────────────────────────────────────────────────────

/// Flattened JSON shape: pointer -> (type name, array length or 0).
type Shape = BTreeMap<String, (&'static str, usize)>;

#[derive(Default)]
struct Endpoint {
    /// Distinct trace indices in which the endpoint appeared at all.
    traces: BTreeSet<usize>,
    statuses: Vec<u16>,
    /// One flattened shape per JSON response, across all traces.
    shapes: Vec<Shape>,
    /// Per trace index: JSON responses in call order.
    sequences: BTreeMap<usize, Vec<Value>>,
}

/// Pointer -> type for every entry present with the same type in all shapes.
fn common_entries(shapes: &[Shape]) -> BTreeMap<String, &'static str> {
    let Some((first, rest)) = shapes.split_first() else {
        return BTreeMap::new();
    };
    first
        .iter()
        .filter(|(p, (ty, _))| rest.iter().all(|s| s.get(*p).is_some_and(|(t, _)| t == ty)))
        .map(|(p, (ty, _))| (p.clone(), *ty))
        .collect()
}

/// The per-trace delta of a numeric pointer, if every trace with at least two
/// responses steps by the same non-zero amount and there are at least two pairs.
fn numeric_step(sequences: &BTreeMap<usize, Vec<Value>>, pointer: &str) -> Option<f64> {
    let mut delta: Option<f64> = None;
    let mut pairs = 0usize;
    for seq in sequences.values().filter(|s| s.len() >= 2) {
        let vals: Vec<f64> = seq
            .iter()
            .map(|v| v.pointer(pointer).and_then(Value::as_f64))
            .collect::<Option<_>>()?;
        for w in vals.windows(2) {
            let d = w[1] - w[0];
            match delta {
                None => delta = Some(d),
                Some(prev) if (prev - d).abs() > DELTA_EPS => return None,
                Some(_) => {}
            }
            pairs += 1;
        }
    }
    let d = delta?;
    (pairs >= 2 && d.abs() > DELTA_EPS).then_some(d)
}

/// Shape of a JSON value: root plus top-level keys, one level deeper into objects,
/// and the first element of arrays, limited to `MAX_SHAPE_DEPTH` segments.
fn flatten(v: &Value) -> Shape {
    let mut out = Shape::new();
    walk(v, String::new(), 0, 0, &mut out);
    out
}

fn walk(v: &Value, ptr: String, segs: usize, level: usize, out: &mut Shape) {
    let len = match v {
        Value::Array(a) => a.len(),
        _ => 0,
    };
    out.insert(ptr.clone(), (type_name(v), len));
    match v {
        Value::Object(map) if level < MAX_OBJECT_LEVEL && segs < MAX_SHAPE_DEPTH => {
            for (k, child) in map {
                let child_ptr = format!("{ptr}/{}", escape(k));
                walk(child, child_ptr, segs + 1, level + 1, out);
            }
        }
        Value::Array(items) if segs < MAX_SHAPE_DEPTH => {
            if let Some(first) = items.first() {
                walk(first, format!("{ptr}/0"), segs + 1, level, out);
            }
        }
        _ => {}
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn path_of(url: &str) -> &str {
    let authority_rest = match url.find("://") {
        Some(i) => Some(&url[i + 3..]),
        None => url.strip_prefix("//"),
    };
    match authority_rest {
        Some(rest) => rest.find(['/', '?', '#']).map_or("", |i| &rest[i..]),
        None => url,
    }
}

fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn json_body(e: &Exchange) -> Option<Value> {
    e.response_body
        .as_deref()
        .and_then(|b| serde_json::from_str(b).ok())
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// JSON pointer token escaping (RFC 6901).
fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// True when an ancestor of `pointer` is an empty array, so a missing entry under it
/// is expected rather than a change.
fn under_empty_array(v: &Value, pointer: &str) -> bool {
    pointer
        .match_indices('/')
        .any(|(i, _)| matches!(v.pointer(&pointer[..i]), Some(Value::Array(a)) if a.is_empty()))
}

fn label(pointer: &str) -> &str {
    if pointer.is_empty() {
        "response"
    } else {
        pointer
    }
}

fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

fn endpoint_of(inv: &Mined) -> &str {
    match inv {
        Mined::StatusAlways { endpoint, .. }
        | Mined::ShapeAlways { endpoint, .. }
        | Mined::NonEmptyArray { endpoint, .. }
        | Mined::NonNull { endpoint, .. }
        | Mined::NumericStep { endpoint, .. } => endpoint.as_str(),
    }
}

fn sort_key(m: &Mined) -> (u8, &str, &str) {
    match m {
        Mined::StatusAlways { endpoint, .. } => (0, endpoint.as_str(), ""),
        Mined::ShapeAlways { endpoint, .. } => (1, endpoint.as_str(), ""),
        Mined::NonEmptyArray { endpoint, pointer } => (2, endpoint.as_str(), pointer.as_str()),
        Mined::NonNull { endpoint, pointer } => (3, endpoint.as_str(), pointer.as_str()),
        Mined::NumericStep {
            endpoint, pointer, ..
        } => (4, endpoint.as_str(), pointer.as_str()),
    }
}

fn candidate_id(inv: &Mined) -> String {
    match inv {
        Mined::StatusAlways { endpoint, .. } => format!("mined:status_always:{endpoint}"),
        Mined::ShapeAlways { endpoint, .. } => format!("mined:shape_always:{endpoint}"),
        Mined::NonEmptyArray { endpoint, pointer } => {
            format!("mined:non_empty_array:{endpoint}:{pointer}")
        }
        Mined::NonNull { endpoint, pointer } => format!("mined:non_null:{endpoint}:{pointer}"),
        Mined::NumericStep {
            endpoint, pointer, ..
        } => format!("mined:numeric_step:{endpoint}:{pointer}"),
    }
}

fn statement(inv: &Mined) -> String {
    match inv {
        Mined::StatusAlways { endpoint, status } => {
            format!("{endpoint}: status is always {status}")
        }
        Mined::ShapeAlways { endpoint, shape } => {
            let parts: Vec<String> = shape
                .iter()
                .map(|(p, t)| format!("{} is {t}", label(p)))
                .collect();
            format!("{endpoint}: response shape: {}", parts.join(", "))
        }
        Mined::NonEmptyArray { endpoint, pointer } => {
            format!("{endpoint}: {} is never empty", label(pointer))
        }
        Mined::NonNull { endpoint, pointer } => {
            format!("{endpoint}: {} is never null", label(pointer))
        }
        Mined::NumericStep {
            endpoint,
            pointer,
            delta,
        } => {
            if *delta > 0.0 {
                format!(
                    "{endpoint}: {} increases by exactly {} each call",
                    label(pointer),
                    fmt_num(*delta)
                )
            } else {
                format!(
                    "{endpoint}: {} decreases by exactly {} each call",
                    label(pointer),
                    fmt_num(-*delta)
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(method: &str, url: &str, status: u16, body: Option<&str>) -> Exchange {
        Exchange {
            method: method.to_string(),
            url: url.to_string(),
            status,
            request_body: None,
            response_body: body.map(str::to_string),
        }
    }

    fn counter_trace(values: &[i64]) -> Vec<Exchange> {
        values
            .iter()
            .map(|v| {
                ex(
                    "POST",
                    "http://localhost:8080/api/counter/inc",
                    200,
                    Some(&format!(r#"{{"value":{v}}}"#)),
                )
            })
            .collect()
    }

    const PROFILE: &str = r#"{"name":"A","tags":["x"]}"#;

    fn profile_trace(body: &str) -> Vec<Exchange> {
        vec![ex(
            "GET",
            "http://localhost:8080/api/profile",
            200,
            Some(body),
        )]
    }

    const COUNTER: &str = "POST /api/counter/inc";

    #[test]
    fn endpoint_key_normalises_ids_origin_and_query() {
        assert_eq!(
            endpoint_key("post", "http://localhost:8080/api/todos/42?x=1"),
            "POST /api/todos/:n"
        );
        assert_eq!(
            endpoint_key(
                "GET",
                "https://h.example/api/users/3f2504e0-4f89-11d3-9a0c-0305e82c3301/tags/"
            ),
            "GET /api/users/:n/tags"
        );
        assert_eq!(endpoint_key("GET", "/api/items#top"), "GET /api/items");
        assert_eq!(endpoint_key("GET", "http://localhost:8080"), "GET /");
        assert_eq!(
            endpoint_key("GET", "http://localhost:8080/v1/2024/items"),
            "GET /v1/:n/items"
        );
    }

    #[test]
    fn mines_counter_step_and_status() {
        let inv = mine(&[counter_trace(&[1, 2, 3]), counter_trace(&[1, 2, 3])]);
        assert!(inv.contains(&Mined::NumericStep {
            endpoint: COUNTER.into(),
            pointer: "/value".into(),
            delta: 1.0,
        }));
        assert!(inv.contains(&Mined::StatusAlways {
            endpoint: COUNTER.into(),
            status: 200,
        }));
    }

    #[test]
    fn check_flags_wrong_step_and_accepts_correct_one() {
        let inv = mine(&[counter_trace(&[1, 2, 3]), counter_trace(&[1, 2, 3])]);
        let v = check(&inv, &counter_trace(&[1, 3]));
        assert!(v.iter().any(|x| {
            matches!(x.invariant, Mined::NumericStep { .. }) && x.observed == "delta 2 (expected 1)"
        }));
        assert!(check(&inv, &counter_trace(&[5, 6])).is_empty());
    }

    #[test]
    fn null_tags_flags_non_null_and_shape() {
        let inv = mine(&[profile_trace(PROFILE), profile_trace(PROFILE)]);
        assert!(inv.contains(&Mined::NonNull {
            endpoint: "GET /api/profile".into(),
            pointer: "/tags".into(),
        }));
        let v = check(&inv, &profile_trace(r#"{"name":"A","tags":null}"#));
        assert!(
            v.iter()
                .any(|x| matches!(x.invariant, Mined::NonNull { .. })
                    && x.observed == "/tags is null")
        );
        assert!(
            v.iter()
                .any(|x| matches!(x.invariant, Mined::ShapeAlways { .. })
                    && x.observed == "/tags is null (expected array)")
        );
    }

    #[test]
    fn status_500_flagged_against_mined_200() {
        let inv = mine(&[profile_trace(PROFILE), profile_trace(PROFILE)]);
        let bad = vec![ex("GET", "http://localhost:8080/api/profile", 500, None)];
        let v = check(&inv, &bad);
        assert!(v.iter().any(|x| {
            matches!(x.invariant, Mined::StatusAlways { .. }) && x.observed == "status 500"
        }));
    }

    #[test]
    fn renamed_endpoint_reports_never_called_once() {
        let inv = mine(&[profile_trace(PROFILE), profile_trace(PROFILE)]);
        let renamed = vec![ex("GET", "http://localhost:8080/api/me", 404, None)];
        let v = check(&inv, &renamed);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].observed, "endpoint never called");
        assert!(matches!(v[0].invariant, Mined::StatusAlways { .. }));
    }

    #[test]
    fn invariants_seen_in_one_trace_are_dropped() {
        assert!(mine(&[counter_trace(&[1, 2, 3])]).is_empty());
    }

    #[test]
    fn candidates_carry_mined_source_and_statement() {
        let inv = mine(&[counter_trace(&[1, 2, 3]), counter_trace(&[1, 2, 3])]);
        let cands = to_candidates(&inv);
        let step = cands
            .iter()
            .find(|c| c.id == "mined:numeric_step:POST /api/counter/inc:/value")
            .expect("numeric step candidate");
        assert_eq!(step.source, InvariantSource::Mined);
        assert_eq!(
            step.statement,
            "POST /api/counter/inc: /value increases by exactly 1 each call"
        );
        assert!((step.confidence - 0.8).abs() < 1e-6);
        assert!(step.check.is_none());
    }
}
