//! Static code-intent extraction and reconciliation against observed traffic.
//!
//! [`extract_intent`] reads a JS/TS/HTML project and lists the HTTP endpoints
//! it calls and serves. [`reconcile`] compares that intent with network
//! requests observed during a real browser run and reports [`RealityGap`]s.
//! Both are pure and deterministic; no model is involved.

mod matching;
mod scan;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use orchyn_core::NetworkRequest;

pub use matching::path_matches;
use matching::{endpoint_matches, is_asset_path, method_matches, paths_compatible, url_path};

/// One HTTP endpoint, either called from client code or served by a route.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Endpoint {
    /// Uppercase: GET, POST, PUT, PATCH, DELETE, or "ANY" (matches every method).
    pub method: String,
    /// Path only (no query, no trailing slash). Parameters appear as `:name`.
    pub path: String,
    /// Source file, relative to the scanned root.
    pub file: PathBuf,
    /// 1-based line of the call or route declaration.
    pub line: usize,
}

/// Endpoints statically extracted from a project.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CodeIntent {
    pub client_calls: Vec<Endpoint>,
    pub server_routes: Vec<Endpoint>,
}

/// The kind of disagreement between code intent and observed behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    /// Static: a client call matches no server route (only checked when routes were found).
    ClientCallWithoutRoute,
    /// Runtime: a request made during the run returned >= 400 or 0.
    ObservedFailure,
    /// Runtime: an observed API request matches no client call and no route.
    UnexpectedRequest,
    /// Runtime: a client call never fired during the run (informational).
    UnexercisedClientCall,
}

/// A single disagreement between the code and the running app.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RealityGap {
    pub kind: GapKind,
    pub method: String,
    pub path: String,
    pub observed_status: Option<u16>,
    /// The client call in the code this gap relates to, when there is one.
    pub source: Option<Endpoint>,
    pub message: String,
}

/// Extracts client calls and server routes from every source file under `root`.
///
/// Routes in a file mounted with `app.use('/prefix', router)` (or `router.use`
/// inside another router) carry the mount prefix, one route per prefix. The
/// module may be bound with `require` or `import`, and is resolved relative to
/// the mounting file. Routes in unmounted files keep their literal path.
///
/// Skips `node_modules`, `.git`, `dist`, `build`, `.next`, `target` and `coverage`
/// directories, and files larger than 1 MB.
pub fn extract_intent(root: &Path) -> CodeIntent {
    scan::extract(root)
}

/// Compares static intent with observed network requests.
///
/// Static asset requests (scripts, styles, images, fonts, `/favicon.ico`) are
/// ignored. The result is sorted by kind, path, then method, and contains no
/// duplicates for the same kind, method, path, status and source location.
pub fn reconcile(intent: &CodeIntent, observed: &[NetworkRequest]) -> Vec<RealityGap> {
    let mut gaps = Vec::new();
    let traffic: Vec<Traffic> = observed.iter().filter_map(Traffic::from_request).collect();

    if !intent.server_routes.is_empty() {
        for call in &intent.client_calls {
            let served = intent.server_routes.iter().any(|route| {
                method_matches(&route.method, &call.method)
                    && paths_compatible(&route.path, &call.path)
            });
            if !served {
                gaps.push(RealityGap {
                    kind: GapKind::ClientCallWithoutRoute,
                    method: call.method.clone(),
                    path: call.path.clone(),
                    observed_status: None,
                    source: Some(call.clone()),
                    message: format!(
                        "{} {} is called from {} but no server route matches it",
                        call.method,
                        call.path,
                        location(call)
                    ),
                });
            }
        }
    }

    for request in &traffic {
        if request.status == 0 || request.status >= 400 {
            let source = intent
                .client_calls
                .iter()
                .find(|call| endpoint_matches(call, &request.method, &request.path))
                .cloned();
            let outcome = if request.status == 0 {
                "failed with status 0 (no response)".to_string()
            } else {
                format!("returned {}", request.status)
            };
            let message = match &source {
                Some(call) => format!(
                    "{} {} {outcome} (called from {})",
                    request.method,
                    request.path,
                    location(call)
                ),
                None => format!("{} {} {outcome}", request.method, request.path),
            };
            gaps.push(RealityGap {
                kind: GapKind::ObservedFailure,
                method: request.method.clone(),
                path: request.path.clone(),
                observed_status: Some(request.status),
                source,
                message,
            });
        } else if is_api_path(&request.path) {
            let known = intent
                .client_calls
                .iter()
                .chain(intent.server_routes.iter())
                .any(|ep| endpoint_matches(ep, &request.method, &request.path));
            if !known {
                gaps.push(RealityGap {
                    kind: GapKind::UnexpectedRequest,
                    method: request.method.clone(),
                    path: request.path.clone(),
                    observed_status: Some(request.status),
                    source: None,
                    message: format!(
                        "{} {} was observed at runtime but matches no client call or server route",
                        request.method, request.path
                    ),
                });
            }
        }
    }

    if !observed.is_empty() {
        for call in &intent.client_calls {
            let exercised = traffic
                .iter()
                .any(|request| endpoint_matches(call, &request.method, &request.path));
            if !exercised {
                gaps.push(RealityGap {
                    kind: GapKind::UnexercisedClientCall,
                    method: call.method.clone(),
                    path: call.path.clone(),
                    observed_status: None,
                    source: Some(call.clone()),
                    message: format!(
                        "{} {} is called from {} but never fired during the run",
                        call.method,
                        call.path,
                        location(call)
                    ),
                });
            }
        }
    }

    gaps.sort_by(|a, b| {
        kind_rank(a.kind)
            .cmp(&kind_rank(b.kind))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.method.cmp(&b.method))
            .then_with(|| a.observed_status.cmp(&b.observed_status))
            .then_with(|| a.message.cmp(&b.message))
    });
    gaps.dedup_by(|b, a| {
        a.kind == b.kind
            && a.method == b.method
            && a.path == b.path
            && a.observed_status == b.observed_status
            && source_key(a.source.as_ref()) == source_key(b.source.as_ref())
    });
    gaps
}

/// A non-asset request from the browser run, with its path normalised.
struct Traffic {
    method: String,
    path: String,
    status: u16,
}

impl Traffic {
    fn from_request(request: &NetworkRequest) -> Option<Self> {
        let path = url_path(&request.url)?;
        if is_asset_path(&path) {
            return None;
        }
        Some(Self {
            method: request.method.to_ascii_uppercase(),
            path,
            status: request.status,
        })
    }
}

fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

fn location(endpoint: &Endpoint) -> String {
    format!("{}:{}", endpoint.file.display(), endpoint.line)
}

fn source_key(source: Option<&Endpoint>) -> Option<(&Path, usize)> {
    source.map(|ep| (ep.file.as_path(), ep.line))
}

fn kind_rank(kind: GapKind) -> u8 {
    match kind {
        GapKind::ClientCallWithoutRoute => 0,
        GapKind::ObservedFailure => 1,
        GapKind::UnexpectedRequest => 2,
        GapKind::UnexercisedClientCall => 3,
    }
}
