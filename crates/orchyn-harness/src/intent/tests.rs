use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use orchyn_core::NetworkRequest;

use super::*;

const SERVER_JS: &str = "\
const express = require('express');
const app = express();
app.get('/api/todos', (req, res) => res.json([]));
app.post(\"/api/todos\", (req, res) => res.status(201).end());
router.delete(`/api/todos/${req.params.id}`, handler);
app.all('/api/health', (req, res) => res.send('ok'));
app.get('/', (req, res) => res.send('home'));
app.use(express.json());
app.get('env');
";

const APP_JS: &str = "\
fetch('/api/todos');
fetch('/api/todos', { method: 'POST', body: JSON.stringify({ title: 'x' }) });
axios.put(`/api/todos/${id}`, { done: true });
axios('/api/config');
$.post(\"/api/login\", data);
const feed = new EventSource('/api/stream');
fetch('http://localhost:3000/api/status?verbose=1');
fetch('https://cdn.example.com/lib.js');
fetch(`${BASE}/api/never`);
";

const INDEX_HTML: &str = "\
<html>
<body>
  <form action=\"/api/search\" method=\"post\">
  <form action=\"/api/feedback\">
  <form action=\"#\" method=\"post\">
</body>
</html>
";

const VENDOR_JS: &str = "\
app.get('/api/vendor', f);
fetch('/api/vendor-call');
";

struct TempProject(PathBuf);

impl TempProject {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "orchyn-intent-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, contents: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ep(method: &str, path: &str, file: &str, line: usize) -> Endpoint {
    Endpoint {
        method: method.to_string(),
        path: path.to_string(),
        file: PathBuf::from(file),
        line,
    }
}

fn req(method: &str, url: &str, status: u16) -> NetworkRequest {
    NetworkRequest {
        method: method.to_string(),
        url: url.to_string(),
        status,
    }
}

fn pairs(endpoints: &[Endpoint]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = endpoints
        .iter()
        .map(|e| (e.method.clone(), e.path.clone()))
        .collect();
    out.sort();
    out
}

fn expected(items: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = items
        .iter()
        .map(|(m, p)| (m.to_string(), p.to_string()))
        .collect();
    out.sort();
    out
}

#[test]
fn path_matches_params_and_wildcards() {
    assert!(path_matches("/api/todos/:id", "/api/todos/42"));
    assert!(path_matches("/api/todos/:param", "/api/todos/7"));
    assert!(path_matches("/api/*/x", "/api/anything/x"));
    assert!(!path_matches("/api/:id", "/api/"));
    assert!(!path_matches("/api/:id", "/api/a/b"));
}

#[test]
fn path_matches_ignores_query_and_trailing_slash() {
    assert!(path_matches("/api/todos/", "/api/todos"));
    assert!(path_matches("/api/todos", "/api/todos/"));
    assert!(path_matches("/api/todos", "/api/todos?page=2"));
    assert!(path_matches("/", "/"));
    assert!(!path_matches("/api/todos/:id", "/api/todos/"));
    assert!(!path_matches("/api/todos/:id", "/api/todos?x=1"));
}

#[test]
fn path_matches_requires_equal_length_and_exact_literals() {
    assert!(!path_matches("/api/todos", "/api/todo"));
    assert!(!path_matches("/api/todos", "/api/todos/extra"));
    assert!(!path_matches("/api/todos/:id", "/api/todos"));
}

#[test]
fn method_matching_is_case_insensitive_and_any_matches_all() {
    assert!(method_matches("GET", "get"));
    assert!(method_matches("ANY", "DELETE"));
    assert!(method_matches("POST", "ANY"));
    assert!(!method_matches("GET", "POST"));
}

#[test]
fn url_path_reduces_observed_urls() {
    assert_eq!(
        url_path("http://localhost:3000/api/x?y=1").as_deref(),
        Some("/api/x")
    );
    assert_eq!(url_path("https://example.com").as_deref(), Some("/"));
    assert_eq!(url_path("/rel/path/").as_deref(), Some("/rel/path"));
    assert_eq!(url_path("ftp://x/y"), None);
    assert_eq!(url_path("//cdn.example.com/x"), None);
}

#[test]
fn extract_finds_routes_and_client_calls() {
    let project = TempProject::new("extract");
    project.write("server.js", SERVER_JS);
    project.write("public/app.js", APP_JS);
    project.write("public/index.html", INDEX_HTML);
    project.write("node_modules/lib/index.js", VENDOR_JS);
    project.write("public/notes.txt", "fetch('/api/ignored');");

    let intent = extract_intent(&project.0);

    assert_eq!(
        pairs(&intent.server_routes),
        expected(&[
            ("ANY", "/api/health"),
            ("DELETE", "/api/todos/:param"),
            ("GET", "/"),
            ("GET", "/api/todos"),
            ("POST", "/api/todos"),
        ])
    );

    assert_eq!(
        pairs(&intent.client_calls),
        expected(&[
            ("GET", "/api/config"),
            ("GET", "/api/feedback"),
            ("GET", "/api/status"),
            ("GET", "/api/stream"),
            ("GET", "/api/todos"),
            ("POST", "/api/login"),
            ("POST", "/api/search"),
            ("POST", "/api/todos"),
            ("PUT", "/api/todos/:param"),
        ])
    );

    let post_todos = intent
        .client_calls
        .iter()
        .find(|e| e.method == "POST" && e.path == "/api/todos")
        .unwrap();
    assert_eq!(post_todos, &ep("POST", "/api/todos", "public/app.js", 2));

    let search = intent
        .client_calls
        .iter()
        .find(|e| e.path == "/api/search")
        .unwrap();
    assert_eq!(search, &ep("POST", "/api/search", "public/index.html", 3));

    let route = intent
        .server_routes
        .iter()
        .find(|e| e.method == "GET" && e.path == "/api/todos")
        .unwrap();
    assert_eq!(route, &ep("GET", "/api/todos", "server.js", 3));
}

#[test]
fn extract_skips_node_modules_and_large_or_missing_roots() {
    let project = TempProject::new("skip");
    project.write("node_modules/lib/index.js", VENDOR_JS);
    let intent = extract_intent(&project.0);
    assert!(intent.client_calls.is_empty());
    assert!(intent.server_routes.is_empty());

    let missing = project.0.join("does-not-exist");
    let intent = extract_intent(&missing);
    assert!(intent.client_calls.is_empty());
}

#[test]
fn reconcile_emits_every_gap_kind() {
    let intent = CodeIntent {
        client_calls: vec![
            ep("GET", "/api/todos", "public/app.js", 1),
            ep("POST", "/api/todos", "public/app.js", 2),
            ep("GET", "/api/ghost", "public/app.js", 9),
        ],
        server_routes: vec![
            ep("GET", "/api/todos", "server.js", 3),
            ep("POST", "/api/todos", "server.js", 4),
            ep("DELETE", "/api/todos/:id", "server.js", 5),
        ],
    };
    let observed = vec![
        req("GET", "http://localhost:3000/api/todos", 200),
        req("POST", "http://localhost:3000/api/todos", 500),
        req("GET", "http://localhost:3000/api/mystery", 200),
        req("GET", "http://localhost:3000/app.js", 200),
        req("GET", "http://localhost:3000/about", 200),
        req("GET", "http://localhost:3000/api/todos", 0),
    ];

    let gaps = reconcile(&intent, &observed);

    let count = |kind: GapKind| gaps.iter().filter(|g| g.kind == kind).count();
    assert_eq!(count(GapKind::ClientCallWithoutRoute), 1);
    assert_eq!(count(GapKind::ObservedFailure), 2);
    assert_eq!(count(GapKind::UnexpectedRequest), 1);
    assert_eq!(count(GapKind::UnexercisedClientCall), 1);
    assert_eq!(gaps.len(), 5);

    let ghost = gaps
        .iter()
        .find(|g| g.kind == GapKind::ClientCallWithoutRoute)
        .unwrap();
    assert_eq!(ghost.path, "/api/ghost");
    assert_eq!(ghost.source.as_ref(), Some(&intent.client_calls[2]));

    let failure = gaps
        .iter()
        .find(|g| g.kind == GapKind::ObservedFailure && g.observed_status == Some(500))
        .unwrap();
    assert_eq!(failure.method, "POST");
    assert_eq!(failure.source.as_ref(), Some(&intent.client_calls[1]));
    assert_eq!(
        failure.message,
        "POST /api/todos returned 500 (called from public/app.js:2)"
    );

    let zero = gaps
        .iter()
        .find(|g| g.kind == GapKind::ObservedFailure && g.observed_status == Some(0))
        .unwrap();
    assert_eq!(zero.source.as_ref(), Some(&intent.client_calls[0]));

    let unexpected = gaps
        .iter()
        .find(|g| g.kind == GapKind::UnexpectedRequest)
        .unwrap();
    assert_eq!(unexpected.path, "/api/mystery");
    assert!(unexpected.source.is_none());

    let unexercised = gaps
        .iter()
        .find(|g| g.kind == GapKind::UnexercisedClientCall)
        .unwrap();
    assert_eq!(unexercised.path, "/api/ghost");
}

#[test]
fn reconcile_is_deterministic() {
    let intent = CodeIntent {
        client_calls: vec![
            ep("GET", "/api/b", "a.js", 1),
            ep("GET", "/api/a", "a.js", 2),
        ],
        server_routes: vec![ep("GET", "/api/x", "s.js", 1)],
    };
    let observed = vec![
        req("POST", "http://h/api/z", 404),
        req("GET", "http://h/api/q", 200),
    ];
    let first = reconcile(&intent, &observed);
    let second = reconcile(&intent, &observed);
    let key = |gaps: &[RealityGap]| {
        gaps.iter()
            .map(|g| {
                (
                    g.kind as u8,
                    g.path.clone(),
                    g.method.clone(),
                    g.message.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(key(&first), key(&second));
    let paths: Vec<&str> = first.iter().map(|g| g.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["/api/a", "/api/b", "/api/z", "/api/q", "/api/a", "/api/b"]
    );
}

#[test]
fn reconcile_without_routes_skips_static_gap_and_without_observations_skips_unexercised() {
    let intent = CodeIntent {
        client_calls: vec![ep("GET", "/api/todos", "public/app.js", 1)],
        server_routes: vec![],
    };
    assert!(reconcile(&intent, &[]).is_empty());

    let asset_only = vec![req("GET", "http://localhost/app.js", 200)];
    let gaps = reconcile(&intent, &asset_only);
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].kind, GapKind::UnexercisedClientCall);
}

#[test]
fn reconcile_matches_parameterised_routes_at_runtime() {
    let intent = CodeIntent {
        client_calls: vec![ep("DELETE", "/api/todos/:param", "public/app.js", 4)],
        server_routes: vec![ep("DELETE", "/api/todos/:id", "server.js", 5)],
    };
    let observed = vec![req("DELETE", "http://localhost:3000/api/todos/42", 204)];
    assert!(reconcile(&intent, &observed).is_empty());
}
