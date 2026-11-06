//! Source scanning: walks a project and extracts client calls and server routes.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};
use walkdir::{DirEntry, WalkDir};

use super::matching::normalize_route_path;
use super::{CodeIntent, Endpoint};

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "build",
    ".next",
    "target",
    "coverage",
];
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "jsx", "ts", "tsx", "html", "vue", "svelte",
];
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// How far past a `fetch(` URL to look for its `method` option.
const FETCH_OPTIONS_WINDOW: usize = 300;

/// A JS/TS string literal: '...', "..." or `...`. Content is captured in groups
/// 1, 2 and 3, one per quote style. See [`literal`].
const LITERAL: &str = r#"(?:'([^'\\\n]*)'|"([^"\\\n]*)"|`([^`\\]*)`)"#;

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static intent regex must compile")
}

static FETCH_CALL: LazyLock<Regex> = LazyLock::new(|| compile(&format!(r"\bfetch\(\s*{LITERAL}")));
static AXIOS_METHOD_CALL: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(
        r"\baxios\.(get|post|put|patch|delete)\(\s*{LITERAL}"
    ))
});
static AXIOS_CALL: LazyLock<Regex> = LazyLock::new(|| compile(&format!(r"\baxios\(\s*{LITERAL}")));
static JQUERY_CALL: LazyLock<Regex> =
    LazyLock::new(|| compile(&format!(r"\$\.(get|post)\(\s*{LITERAL}")));
static EVENT_SOURCE: LazyLock<Regex> =
    LazyLock::new(|| compile(&format!(r"\bnew\s+EventSource\(\s*{LITERAL}")));
static ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(
        r"\b(app|router|server|api)\.(get|post|put|patch|delete|all)\(\s*{LITERAL}"
    ))
});
static FORM_TAG: LazyLock<Regex> = LazyLock::new(|| compile(r"(?i)<form\b([^>]*)>"));
static FORM_ACTION: LazyLock<Regex> =
    LazyLock::new(|| compile(r#"(?i)(?:^|\s)action\s*=\s*(?:"([^"]*)"|'([^']*)')"#));
static FORM_METHOD: LazyLock<Regex> =
    LazyLock::new(|| compile(r#"(?i)(?:^|\s)method\s*=\s*["']?([A-Za-z]+)"#));
static METHOD_OPTION: LazyLock<Regex> =
    LazyLock::new(|| compile(r#"(?i)\bmethod\s*:\s*["'`]([A-Za-z]+)["'`]"#));
static TEMPLATE_EXPR: LazyLock<Regex> = LazyLock::new(|| compile(r"\$\{[^}]*\}"));

/// Walks `root` and collects every client call and server route.
pub(super) fn extract(root: &Path) -> CodeIntent {
    let mut client_calls = Vec::new();
    let mut server_routes = Vec::new();
    for entry in source_files(root) {
        let path = entry.path();
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let file = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        scan_source(&text, &file, &mut client_calls, &mut server_routes);
    }
    CodeIntent {
        client_calls: finish(client_calls),
        server_routes: finish(server_routes),
    }
}

fn source_files(root: &Path) -> impl Iterator<Item = DirEntry> + '_ {
    WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !(entry.file_type().is_dir()
                    && entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| SKIP_DIRS.contains(&name)))
        })
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && has_source_extension(entry.path())
                && entry
                    .metadata()
                    .is_ok_and(|meta| meta.len() <= MAX_FILE_BYTES)
        })
}

fn has_source_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext.as_str()))
}

fn scan_source(text: &str, file: &Path, clients: &mut Vec<Endpoint>, routes: &mut Vec<Endpoint>) {
    let lines = LineIndex::new(text);

    for caps in FETCH_CALL.captures_iter(text) {
        let (Some(whole), Some(raw)) = (caps.get(0), literal(&caps, 1)) else {
            continue;
        };
        let method = fetch_method(&text[whole.end()..]);
        push(clients, method, raw, file, lines.line_of(whole.start()));
    }

    for caps in AXIOS_METHOD_CALL.captures_iter(text) {
        let (Some(whole), Some(raw)) = (caps.get(0), literal(&caps, 2)) else {
            continue;
        };
        let method = caps.get(1).and_then(|m| canonical_method(m.as_str()));
        push(clients, method, raw, file, lines.line_of(whole.start()));
    }

    for caps in AXIOS_CALL.captures_iter(text) {
        let (Some(whole), Some(raw)) = (caps.get(0), literal(&caps, 1)) else {
            continue;
        };
        push(
            clients,
            Some("GET"),
            raw,
            file,
            lines.line_of(whole.start()),
        );
    }

    for caps in JQUERY_CALL.captures_iter(text) {
        let (Some(whole), Some(raw)) = (caps.get(0), literal(&caps, 2)) else {
            continue;
        };
        let method = caps.get(1).and_then(|m| canonical_method(m.as_str()));
        push(clients, method, raw, file, lines.line_of(whole.start()));
    }

    for caps in EVENT_SOURCE.captures_iter(text) {
        let (Some(whole), Some(raw)) = (caps.get(0), literal(&caps, 1)) else {
            continue;
        };
        push(
            clients,
            Some("GET"),
            raw,
            file,
            lines.line_of(whole.start()),
        );
    }

    for tag in FORM_TAG.captures_iter(text) {
        let Some(whole) = tag.get(0) else {
            continue;
        };
        let attrs = tag.get(1).map_or("", |m| m.as_str());
        let Some(action) = FORM_ACTION
            .captures(attrs)
            .and_then(|c| c.get(1).or_else(|| c.get(2)))
        else {
            continue;
        };
        let method = FORM_METHOD
            .captures(attrs)
            .and_then(|c| c.get(1))
            .map_or(Some("GET"), |m| canonical_method(m.as_str()));
        push(
            clients,
            method,
            action.as_str(),
            file,
            lines.line_of(whole.start()),
        );
    }

    for caps in ROUTE.captures_iter(text) {
        let (Some(whole), Some(raw)) = (caps.get(0), literal(&caps, 3)) else {
            continue;
        };
        let method = caps.get(2).and_then(|m| {
            if m.as_str().eq_ignore_ascii_case("all") {
                Some("ANY")
            } else {
                canonical_method(m.as_str())
            }
        });
        push(routes, method, raw, file, lines.line_of(whole.start()));
    }
}

/// Returns the content of the string literal whose groups start at `first_group`.
fn literal<'t>(caps: &Captures<'t>, first_group: usize) -> Option<&'t str> {
    (first_group..first_group + 3)
        .find_map(|i| caps.get(i))
        .map(|m| m.as_str())
}

/// Determines the method of a `fetch(` call from the text after its URL.
///
/// Looks for `method: '...'` inside the call's parentheses, within the first
/// [`FETCH_OPTIONS_WINDOW`] characters. Defaults to GET. Returns `None` for an
/// unsupported method, so the call is dropped.
fn fetch_method(rest: &str) -> Option<&'static str> {
    let limit = rest
        .char_indices()
        .nth(FETCH_OPTIONS_WINDOW)
        .map_or(rest.len(), |(i, _)| i);
    let window = &rest[..limit];

    let mut depth = 1usize;
    let mut end = window.len();
    for (i, ch) in window.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }

    match METHOD_OPTION.captures(&window[..end]) {
        Some(caps) => canonical_method(caps.get(1)?.as_str()),
        None => Some("GET"),
    }
}

fn canonical_method(raw: &str) -> Option<&'static str> {
    match raw.to_ascii_uppercase().as_str() {
        "GET" => Some("GET"),
        "POST" => Some("POST"),
        "PUT" => Some("PUT"),
        "PATCH" => Some("PATCH"),
        "DELETE" => Some("DELETE"),
        _ => None,
    }
}

/// Adds an endpoint when both the method and the URL are usable.
fn push(out: &mut Vec<Endpoint>, method: Option<&str>, raw: &str, file: &Path, line: usize) {
    let (Some(method), Some(path)) = (method, endpoint_path(raw)) else {
        return;
    };
    out.push(Endpoint {
        method: method.to_string(),
        path,
        file: file.to_path_buf(),
        line,
    });
}

/// Turns a literal URL into an endpoint path, or `None` if it is not one we track.
///
/// Keeps paths starting with `/`, and `http(s)://localhost` or `127.0.0.1`
/// URLs reduced to their path. Template `${...}` expressions become `:param`.
fn endpoint_path(raw: &str) -> Option<String> {
    let replaced = TEMPLATE_EXPR.replace_all(raw.trim(), ":param");
    let text: &str = &replaced;
    let path = if let Some(local) = local_origin_path(text) {
        local
    } else if text.starts_with('/') && !text.starts_with("//") {
        text
    } else {
        return None;
    };
    Some(normalize_route_path(path))
}

fn local_origin_path(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..host_end].split(':').next().unwrap_or("");
    if !matches!(host, "localhost" | "127.0.0.1") {
        return None;
    }
    let tail = &rest[host_end..];
    Some(if tail.starts_with('/') { tail } else { "/" })
}

/// Sorts endpoints by location and drops duplicates of (method, path, file).
fn finish(mut endpoints: Vec<Endpoint>) -> Vec<Endpoint> {
    endpoints.sort_by(|a, b| {
        (&a.file, a.line, &a.method, &a.path).cmp(&(&b.file, b.line, &b.method, &b.path))
    });
    let mut seen: HashSet<(String, String, PathBuf)> = HashSet::new();
    endpoints.retain(|e| seen.insert((e.method.clone(), e.path.clone(), e.file.clone())));
    endpoints
}

/// Maps byte offsets to 1-based line numbers.
struct LineIndex {
    newlines: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        Self {
            newlines: text.match_indices('\n').map(|(i, _)| i).collect(),
        }
    }

    fn line_of(&self, offset: usize) -> usize {
        self.newlines.partition_point(|&nl| nl < offset) + 1
    }
}
