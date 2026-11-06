//! Path and method matching shared by static intent and runtime reconciliation.

use super::Endpoint;

/// Static asset extensions that are never API traffic.
const ASSET_EXTENSIONS: &[&str] = &[
    ".js", ".css", ".map", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".ico", ".woff", ".woff2",
    ".ttf", ".webp",
];

/// Returns true when `actual` matches `pattern`.
///
/// Query strings and trailing slashes are ignored. Segments must match one
/// for one, so lengths must be equal. A pattern segment starting with `:` or
/// equal to `*` matches any single non-empty segment; other segments must be equal.
pub fn path_matches(pattern: &str, actual: &str) -> bool {
    let pattern = normalize_route_path(pattern);
    let actual = normalize_route_path(actual);
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let actual_segments: Vec<&str> = actual.split('/').collect();
    pattern_segments.len() == actual_segments.len()
        && pattern_segments
            .iter()
            .zip(&actual_segments)
            .all(|(p, a)| if is_param(p) { !a.is_empty() } else { p == a })
}

/// Like [`path_matches`], but parameters on either side match any non-empty
/// segment. Used to compare two static paths, where both may contain `:param`.
pub(crate) fn paths_compatible(a: &str, b: &str) -> bool {
    let a = normalize_route_path(a);
    let b = normalize_route_path(b);
    let a_segments: Vec<&str> = a.split('/').collect();
    let b_segments: Vec<&str> = b.split('/').collect();
    a_segments.len() == b_segments.len()
        && a_segments.iter().zip(&b_segments).all(|(x, y)| {
            if is_param(x) || is_param(y) {
                !x.is_empty() && !y.is_empty()
            } else {
                x == y
            }
        })
}

/// Case-insensitive method comparison where "ANY" matches every method.
pub(crate) fn method_matches(a: &str, b: &str) -> bool {
    a == "ANY" || b == "ANY" || a.eq_ignore_ascii_case(b)
}

/// True when `endpoint` covers the given method and concrete path.
pub(crate) fn endpoint_matches(endpoint: &Endpoint, method: &str, path: &str) -> bool {
    method_matches(&endpoint.method, method) && path_matches(&endpoint.path, path)
}

/// Strips the query, fragment and trailing slash. An empty result becomes "/".
pub(crate) fn normalize_route_path(raw: &str) -> String {
    let path = raw.split(['?', '#']).next().unwrap_or_default();
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Converts an observed URL into a normalised path.
///
/// Absolute URLs of any host are reduced to their path. Relative URLs must
/// start with a single `/`. Returns `None` for anything else.
pub(crate) fn url_path(url: &str) -> Option<String> {
    let rest = url.trim();
    let path = match strip_scheme(rest) {
        Some(after) => match after.find(['/', '?', '#']) {
            Some(i) if after[i..].starts_with('/') => &after[i..],
            _ => "/",
        },
        None if rest.starts_with('/') && !rest.starts_with("//") => rest,
        None => return None,
    };
    Some(normalize_route_path(path))
}

/// True for static asset paths and `/favicon.ico`.
pub(crate) fn is_asset_path(path: &str) -> bool {
    if path == "/favicon.ico" {
        return true;
    }
    let lower = path.to_ascii_lowercase();
    ASSET_EXTENSIONS.iter().any(|ext| lower.ends_with(*ext))
}

fn strip_scheme(url: &str) -> Option<&str> {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
}

fn is_param(segment: &str) -> bool {
    segment.starts_with(':') || segment == "*"
}
