//! Invariants the code declares about itself.
//!
//! Deterministic regex and line heuristics, no model:
//!
//! * HTML form constraints (`required`, `type="email"`, `minlength`,
//!   `maxlength`, `pattern`) in `.html`, `.jsx`, `.tsx`, `.vue`, `.svelte`.
//! * Server-side validation (`if (!req.body.x) ... res.status(4xx)`) in
//!   `.js`/`.ts`, attributed to the nearest `app|router.<method>('<path>'` line.
//! * Explicit success statuses (`res.status(201)`, `res.sendStatus(204)`).
//! * Auth gates: route middleware whose name looks like auth, and session or
//!   cookie checks inside a handler.
//! * Schemas: zod `z.object({...})` fields, SQL `CREATE TABLE` `NOT NULL` and
//!   `UNIQUE` columns, and TypeScript interface members that are not optional.
//!
//! Files are walked with the same rules as `intent::scan`: `node_modules`,
//! `.git`, `dist`, `build`, `.next`, `target` and `coverage` are skipped, and
//! files over 1 MiB or not valid UTF-8 are ignored.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use walkdir::WalkDir;

use super::{Evidence, InvariantCandidate, InvariantSource};

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "build",
    ".next",
    "target",
    "coverage",
];
const MARKUP_EXTENSIONS: &[&str] = &["html", "htm", "jsx", "tsx", "vue", "svelte"];
const CODE_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx"];
const TYPESCRIPT_EXTENSIONS: &[&str] = &["ts", "mts", "cts", "tsx"];
const SQL_EXTENSIONS: &[&str] = &["sql"];
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Characters of source kept inside an evidence quote.
const QUOTE_CHARS: usize = 150;
/// Lines after an `if (!x)` searched for the 4xx it returns.
const REJECT_AFTER: usize = 3;
/// Lines before an `if (!x)` searched for a `req.body` reference.
const BODY_BEFORE: usize = 12;
/// Keywords that open a table-level constraint rather than a column definition.
const TABLE_CONSTRAINT_WORDS: &[&str] = &[
    "CONSTRAINT",
    "PRIMARY",
    "UNIQUE",
    "FOREIGN",
    "CHECK",
    "EXCLUDE",
];

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static contracts regex must compile")
}

/// Matches one HTML attribute: group 1-4 hold the value (double-quoted,
/// single-quoted, JSX `{...}`, or bare). A bare attribute matches with no groups.
fn attr_regex(name: &str) -> Regex {
    compile(&format!(
        r#"(?i)(?:^|\s){name}(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|\{{([^}}]*)\}}|([^\s"'>]+))|[\s/]|$)"#
    ))
}

static INPUT_TAG: LazyLock<Regex> = LazyLock::new(|| compile(r"(?is)<input\b([^>]*)>"));
static ATTR_ID: LazyLock<Regex> = LazyLock::new(|| attr_regex("id"));
static ATTR_NAME: LazyLock<Regex> = LazyLock::new(|| attr_regex("name"));
static ATTR_REQUIRED: LazyLock<Regex> = LazyLock::new(|| attr_regex("required"));
static ATTR_TYPE: LazyLock<Regex> = LazyLock::new(|| attr_regex("type"));
static ATTR_MINLENGTH: LazyLock<Regex> = LazyLock::new(|| attr_regex("minlength"));
static ATTR_MAXLENGTH: LazyLock<Regex> = LazyLock::new(|| attr_regex("maxlength"));
static ATTR_PATTERN: LazyLock<Regex> = LazyLock::new(|| attr_regex("pattern"));

static ROUTE_CALL: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r#"\b(?:app|router)\.(get|post|put|patch|delete|all)\(\s*(?:'([^']*)'|"([^"]*)"|`([^`]*)`)"#,
    )
});
/// A further argument after the route path: `, name`.
static ROUTE_ARG: LazyLock<Regex> = LazyLock::new(|| compile(r",\s*([A-Za-z_$][\w$.]*)"));
static AUTH_NAME: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?i)auth|login|session|requireUser|protect"));
static IF_CONDITION: LazyLock<Regex> = LazyLock::new(|| compile(r"\bif\s*\((.*)$"));
static BODY_NOT: LazyLock<Regex> = LazyLock::new(|| compile(r"!\s*req\.body\.([A-Za-z_]\w*)"));
/// `!name` that is not a member access or call (`!user.id` is skipped).
static BARE_NOT: LazyLock<Regex> = LazyLock::new(|| compile(r"!\s*([A-Za-z_$][\w$]*)([.(\[]?)"));
static BODY_REF: LazyLock<Regex> = LazyLock::new(|| compile(r"\breq\.body\b"));
static REJECT: LazyLock<Regex> =
    LazyLock::new(|| compile(r"\bres\.(?:status|sendStatus)\(\s*(4\d\d)\s*\)"));
static SUCCESS: LazyLock<Regex> =
    LazyLock::new(|| compile(r"\bres\.(?:status|sendStatus)\(\s*(2\d\d)\s*\)"));
static SESSION_CHECK: LazyLock<Regex> =
    LazyLock::new(|| compile(r"\bif\s*\(.*\breq\.(?:cookies\.\w+|session\b)"));

static ZOD_OBJECT: LazyLock<Regex> = LazyLock::new(|| compile(r"\bz\.object\(\s*\{"));
static ZOD_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    compile(r#"(?m)^[ \t]*(?:([A-Za-z_]\w*)|'([^']+)'|"([^"]+)")[ \t]*:[ \t]*z\.(\w+)\b"#)
});
static ZOD_MIN: LazyLock<Regex> = LazyLock::new(|| compile(r"\.min\(\s*(\d+)\s*\)"));

static CREATE_TABLE: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r#"(?i)\bCREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?((?:"[^"]+"|`[^`]+`|\[[^\]]+\]|[\w.]+))\s*\("#,
    )
});
static SQL_TABLE_UNIQUE: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r#"(?is)^\s*(?:CONSTRAINT\s+\S+\s+)?UNIQUE\s*\(\s*("[^"]+"|`[^`]+`|\[[^\]]+\]|\w+)\s*\)"#,
    )
});
static SQL_COLUMN: LazyLock<Regex> =
    LazyLock::new(|| compile(r#"(?s)^\s*("[^"]+"|`[^`]+`|\[[^\]]+\]|\w+)\s+(.*)$"#));
static SQL_NOT_NULL: LazyLock<Regex> = LazyLock::new(|| compile(r"(?i)\bNOT\s+NULL\b"));
static SQL_UNIQUE: LazyLock<Regex> = LazyLock::new(|| compile(r"(?i)\bUNIQUE\b"));

static TS_INTERFACE: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"\binterface\s+([A-Za-z_$][\w$]*)\s*(?:<[^>{]*>)?\s*(?:extends\s+[^{]+)?\{")
});
static TS_MEMBER: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^([A-Za-z_$][\w$]*)(\?)?\s*:\s*(.+)$"));

/// Collects candidates keyed by id. A repeated id merges its evidence and keeps
/// the highest confidence and the first statement.
#[derive(Default)]
struct Found {
    by_id: BTreeMap<String, InvariantCandidate>,
}

impl Found {
    fn add(
        &mut self,
        id: String,
        statement: String,
        confidence: f32,
        evidence: impl IntoIterator<Item = Evidence>,
    ) {
        let entry = self
            .by_id
            .entry(id.clone())
            .or_insert_with(|| InvariantCandidate {
                id,
                source: InvariantSource::CodeContract,
                statement,
                evidence: Vec::new(),
                confidence,
                check: None,
            });
        for ev in evidence {
            if !entry.evidence.contains(&ev) {
                entry.evidence.push(ev);
            }
        }
        entry.confidence = entry.confidence.max(confidence);
    }

    fn finish(self) -> Vec<InvariantCandidate> {
        self.by_id.into_values().collect()
    }
}

/// All evidence for one route, gathered across files before it becomes a candidate.
struct StatusGroup {
    method: String,
    path: String,
    codes: BTreeSet<u16>,
    evidence: Vec<Evidence>,
}

struct SourceFile {
    rel: PathBuf,
    ext: String,
    text: String,
}

/// A route registration found on one line.
struct Route {
    line: usize,
    method: String,
    path: String,
    /// Arguments between the path and the final handler.
    middleware: Vec<String>,
    text: String,
}

impl Route {
    fn key(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

/// Line starts of a text, for byte offset to 1-based line lookups.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        );
        Self { starts }
    }

    fn line_at(&self, offset: usize) -> usize {
        self.starts.partition_point(|&start| start <= offset)
    }
}

/// Invariants declared by the code under `root`, sorted by id.
pub fn extract(root: &Path) -> Vec<InvariantCandidate> {
    let mut found = Found::default();
    let mut statuses: BTreeMap<String, StatusGroup> = BTreeMap::new();
    for file in walk(root) {
        let ext = file.ext.as_str();
        if MARKUP_EXTENSIONS.contains(&ext) {
            scan_markup(&file, &mut found);
        }
        if CODE_EXTENSIONS.contains(&ext) {
            scan_routes(&file, &mut found, &mut statuses);
            scan_zod(&file, &mut found);
            scan_sql(&file, &mut found);
        }
        if SQL_EXTENSIONS.contains(&ext) {
            scan_sql(&file, &mut found);
        }
        if TYPESCRIPT_EXTENSIONS.contains(&ext) {
            scan_interfaces(&file, &mut found);
        }
    }
    for group in statuses.into_values() {
        let key = format!("{} {}", group.method, group.path);
        let codes = group
            .codes
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(" or ");
        found.add(
            format!("contract:status:{key}"),
            format!("{key} responds {codes} on success"),
            0.6,
            group.evidence,
        );
    }
    found.finish()
}

fn walk(root: &Path) -> Vec<SourceFile> {
    let entries = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !(entry.file_type().is_dir()
                    && entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| SKIP_DIRS.contains(&name)))
        });
    let mut files = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(ext) = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };
        let scanned = MARKUP_EXTENSIONS.contains(&ext.as_str())
            || CODE_EXTENSIONS.contains(&ext.as_str())
            || SQL_EXTENSIONS.contains(&ext.as_str());
        if !scanned {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_path_buf();
        files.push(SourceFile { rel, ext, text });
    }
    files
}

fn evidence(file: &Path, line: usize, text: &str) -> Evidence {
    Evidence {
        file: Some(file.to_path_buf()),
        line: Some(line),
        note: quote(text),
    }
}

/// Whitespace-collapsed source text in double quotes, cut to `QUOTE_CHARS`.
fn quote(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > QUOTE_CHARS {
        let head: String = collapsed.chars().take(QUOTE_CHARS).collect();
        format!("\"{head}...\"")
    } else {
        format!("\"{collapsed}\"")
    }
}

/// Attribute value: `None` when absent, `Some(None)` when bare, `Some(Some(v))` when valued.
fn attr_value(re: &Regex, attrs: &str) -> Option<Option<String>> {
    let caps = re.captures(attrs)?;
    let value = caps
        .iter()
        .skip(1)
        .flatten()
        .next()
        .map(|m| m.as_str().trim().to_string());
    Some(value)
}

fn scan_markup(file: &SourceFile, found: &mut Found) {
    let lines = LineIndex::new(&file.text);
    for caps in INPUT_TAG.captures_iter(&file.text) {
        let Some(whole) = caps.get(0) else {
            continue;
        };
        let attrs = caps.get(1).map_or("", |m| m.as_str());
        let line = lines.line_at(whole.start());
        let id = attr_value(&ATTR_ID, attrs)
            .flatten()
            .filter(|v| !v.is_empty());
        let name = attr_value(&ATTR_NAME, attrs)
            .flatten()
            .filter(|v| !v.is_empty());
        let (subject, label) = match (&id, &name) {
            (Some(id), _) => (format!("#{id}"), id.clone()),
            (None, Some(name)) => (format!("input[name=\"{name}\"]"), name.clone()),
            (None, None) => (format!("the input at line {line}"), format!("line{line}")),
        };
        let mut push = |constraint: &str, statement: String| {
            found.add(
                format!("contract:input:{label}:{constraint}"),
                statement,
                0.8,
                [evidence(&file.rel, line, whole.as_str())],
            );
        };

        if attr_value(&ATTR_REQUIRED, attrs).is_some_and(|v| v.as_deref() != Some("false")) {
            push(
                "required",
                format!("{subject} is required: submitting without it must not succeed"),
            );
        }
        if attr_value(&ATTR_TYPE, attrs)
            .flatten()
            .is_some_and(|v| v.eq_ignore_ascii_case("email"))
        {
            push(
                "email",
                format!("{subject} must be an email address: a non-email value must not succeed"),
            );
        }
        if let Some(n) = attr_value(&ATTR_MINLENGTH, attrs)
            .flatten()
            .and_then(|v| v.parse::<usize>().ok())
        {
            push(
                "minlength",
                format!(
                    "{subject} must be at least {n} characters long: a shorter value must not succeed"
                ),
            );
        }
        if let Some(n) = attr_value(&ATTR_MAXLENGTH, attrs)
            .flatten()
            .and_then(|v| v.parse::<usize>().ok())
        {
            push(
                "maxlength",
                format!(
                    "{subject} must be at most {n} characters long: a longer value must not be accepted"
                ),
            );
        }
        if let Some(pattern) = attr_value(&ATTR_PATTERN, attrs).flatten() {
            push(
                "pattern",
                format!(
                    "{subject} must match the pattern {pattern}: a non-matching value must not succeed"
                ),
            );
        }
    }
}

/// Routes registered on each line, in file order.
fn parse_routes(text: &str) -> Vec<Route> {
    let mut routes = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for caps in ROUTE_CALL.captures_iter(line) {
            let (Some(whole), Some(path)) =
                (caps.get(0), caps.get(2).or(caps.get(3)).or(caps.get(4)))
            else {
                continue;
            };
            let verb = caps[1].to_ascii_lowercase();
            let method = if verb == "all" {
                "ANY".to_string()
            } else {
                verb.to_ascii_uppercase()
            };
            let args: Vec<&str> = ROUTE_ARG
                .captures_iter(&line[whole.end()..])
                .filter_map(|c| c.get(1))
                .map(|m| m.as_str())
                .collect();
            let middleware = if args.len() > 1 {
                args[..args.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            } else {
                Vec::new()
            };
            routes.push(Route {
                line: index + 1,
                method,
                path: path.as_str().to_string(),
                middleware,
                text: line.trim().to_string(),
            });
        }
    }
    routes
}

/// Nearest route registered at or above `line` (1-based).
fn enclosing_route(routes: &[Route], line: usize) -> Option<&Route> {
    routes.iter().rev().find(|r| r.line <= line)
}

fn auth_statement(key: &str) -> String {
    format!("{key} requires a session: an anonymous visitor must not see its content")
}

fn scan_routes(file: &SourceFile, found: &mut Found, statuses: &mut BTreeMap<String, StatusGroup>) {
    let routes = parse_routes(&file.text);
    for route in &routes {
        if route.middleware.iter().any(|arg| AUTH_NAME.is_match(arg)) {
            let key = route.key();
            found.add(
                format!("contract:auth:{key}"),
                auth_statement(&key),
                0.8,
                [evidence(&file.rel, route.line, &route.text)],
            );
        }
    }

    let lines: Vec<&str> = file.text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let line_no = index + 1;
        let Some(route) = enclosing_route(&routes, line_no) else {
            continue;
        };
        let key = route.key();

        if let Some(caps) = IF_CONDITION.captures(line) {
            let condition = &caps[1];
            let window_end = (index + REJECT_AFTER + 1).min(lines.len());
            let rejects = lines[index..window_end].iter().any(|l| REJECT.is_match(l));
            if rejects {
                let body_nearby = lines[index.saturating_sub(BODY_BEFORE)..window_end]
                    .iter()
                    .any(|l| BODY_REF.is_match(l));
                let mut fields: Vec<String> = BODY_NOT
                    .captures_iter(condition)
                    .map(|c| c[1].to_string())
                    .collect();
                if body_nearby {
                    fields.extend(
                        BARE_NOT
                            .captures_iter(condition)
                            .filter(|c| c[2].is_empty())
                            .map(|c| c[1].to_string()),
                    );
                }
                for field in fields {
                    found.add(
                        format!("contract:validate:{key}:{field}"),
                        format!("{key}: missing {field} is rejected with 4xx"),
                        0.7,
                        [evidence(&file.rel, line_no, line)],
                    );
                }
            }
        }

        let codes: Vec<u16> = SUCCESS
            .captures_iter(line)
            .filter_map(|c| c[1].parse().ok())
            .collect();
        if !codes.is_empty() {
            let group = statuses.entry(key.clone()).or_insert_with(|| StatusGroup {
                method: route.method.clone(),
                path: route.path.clone(),
                codes: BTreeSet::new(),
                evidence: Vec::new(),
            });
            group.codes.extend(codes);
            group.evidence.push(evidence(&file.rel, line_no, line));
        }

        if SESSION_CHECK.is_match(line) {
            found.add(
                format!("contract:auth:{key}"),
                auth_statement(&key),
                0.6,
                [evidence(&file.rel, line_no, line)],
            );
        }
    }
}

/// Byte index of the delimiter closing the one at `open`, or `None`.
fn matching(text: &str, open: usize, open_byte: u8, close_byte: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in text.bytes().enumerate().skip(open) {
        if b == open_byte {
            depth += 1;
        } else if b == close_byte {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Splits on `separators` that sit outside any `()`, `{}` or `[]`. Returns each
/// non-blank piece with its byte offset inside `body`.
fn split_top_level<'a>(body: &'a str, separators: &[u8]) -> Vec<(usize, &'a str)> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, b) in body.bytes().enumerate() {
        match b {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            _ if depth == 0 && separators.contains(&b) => {
                out.push((start, &body[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push((start, &body[start..]));
    out.retain(|(_, piece)| !piece.trim().is_empty());
    out
}

fn article(word: &str) -> &'static str {
    match word.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

fn scan_zod(file: &SourceFile, found: &mut Found) {
    let lines = LineIndex::new(&file.text);
    for object in ZOD_OBJECT.find_iter(&file.text) {
        let open = object.end() - 1;
        let Some(close) = matching(&file.text, open, b'{', b'}') else {
            continue;
        };
        let body_start = open + 1;
        let body = &file.text[body_start..close];
        let fields: Vec<(usize, &str, &str)> = ZOD_FIELD
            .captures_iter(body)
            .filter_map(|c| {
                let start = c.get(0)?.start();
                let name = c.get(1).or(c.get(2)).or(c.get(3))?.as_str();
                Some((start, name, c.get(4)?.as_str()))
            })
            .collect();
        for (i, &(start, name, ty)) in fields.iter().enumerate() {
            let end = fields.get(i + 1).map_or(body.len(), |next| next.0);
            let chunk = &body[start..end];
            let mut statement = format!("field {name} must be {} {ty}", article(ty));
            if chunk.contains(".email(") {
                statement.push_str(", email");
            }
            if let Some(min) = ZOD_MIN.captures(chunk) {
                statement.push_str(&format!(", min {}", &min[1]));
            }
            let line = lines.line_at(body_start + start);
            found.add(
                format!("contract:schema:{name}:{ty}"),
                statement,
                0.7,
                [evidence(&file.rel, line, chunk.trim())],
            );
        }
    }
}

/// Strips SQL identifier quoting and any schema prefix is left to the caller.
fn unquote(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '"' | '`' | '[' | ']'))
        .collect()
}

fn sql_unique(found: &mut Found, file: &Path, table: &str, column: &str, line: usize, text: &str) {
    found.add(
        format!("contract:sql:{table}.{column}:unique"),
        format!("{table}.{column} is unique"),
        0.9,
        [evidence(file, line, text)],
    );
}

fn scan_sql(file: &SourceFile, found: &mut Found) {
    let lines = LineIndex::new(&file.text);
    for caps in CREATE_TABLE.captures_iter(&file.text) {
        let (Some(whole), Some(name)) = (caps.get(0), caps.get(1)) else {
            continue;
        };
        let open = whole.end() - 1;
        let Some(close) = matching(&file.text, open, b'(', b')') else {
            continue;
        };
        let cleaned = unquote(name.as_str());
        let table = cleaned.rsplit('.').next().unwrap_or_default().to_string();
        let body_start = open + 1;
        let body = &file.text[body_start..close];
        for (offset, item) in split_top_level(body, b",") {
            let item_text = item.trim();
            let leading = item.len() - item.trim_start().len();
            let line = lines.line_at(body_start + offset + leading);

            if let Some(unique) = SQL_TABLE_UNIQUE.captures(item_text) {
                let column = unquote(&unique[1]);
                sql_unique(found, &file.rel, &table, &column, line, item_text);
                continue;
            }
            let first_word = item_text
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            if TABLE_CONSTRAINT_WORDS.contains(&first_word.as_str()) {
                continue;
            }
            let Some(column_caps) = SQL_COLUMN.captures(item_text) else {
                continue;
            };
            let column = unquote(&column_caps[1]);
            let rest = &column_caps[2];
            if SQL_NOT_NULL.is_match(rest) {
                found.add(
                    format!("contract:sql:{table}.{column}:not_null"),
                    format!("{table}.{column} is never null"),
                    0.9,
                    [evidence(&file.rel, line, item_text)],
                );
            }
            if SQL_UNIQUE.is_match(rest) {
                sql_unique(found, &file.rel, &table, &column, line, item_text);
            }
        }
    }
}

fn scan_interfaces(file: &SourceFile, found: &mut Found) {
    let lines = LineIndex::new(&file.text);
    for caps in TS_INTERFACE.captures_iter(&file.text) {
        let (Some(whole), Some(iface)) = (caps.get(0), caps.get(1)) else {
            continue;
        };
        let open = whole.end() - 1;
        let Some(close) = matching(&file.text, open, b'{', b'}') else {
            continue;
        };
        let body_start = open + 1;
        let body = &file.text[body_start..close];
        let iface = iface.as_str();
        for (offset, piece) in split_top_level(body, b";\n,") {
            let member_text = piece.trim();
            let Some(member) = TS_MEMBER.captures(member_text) else {
                continue;
            };
            if member.get(2).is_some() {
                continue;
            }
            let ty = member[3]
                .split("//")
                .next()
                .unwrap_or_default()
                .trim()
                .trim_end_matches([',', ';'])
                .trim();
            if ty.is_empty() || ty.starts_with('{') {
                continue;
            }
            let name = &member[1];
            let leading = piece.len() - piece.trim_start().len();
            let line = lines.line_at(body_start + offset + leading);
            found.add(
                format!("contract:ts:{iface}.{name}"),
                format!("{iface}.{name} is always present ({ty})"),
                0.4,
                [evidence(&file.rel, line, member_text)],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "orchyn-contracts-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn find<'a>(cands: &'a [InvariantCandidate], id: &str) -> &'a InvariantCandidate {
        cands.iter().find(|c| c.id == id).unwrap_or_else(|| {
            let ids: Vec<&String> = cands.iter().map(|c| &c.id).collect();
            panic!("missing {id}; have {ids:?}")
        })
    }

    fn has(cands: &[InvariantCandidate], id: &str) -> bool {
        cands.iter().any(|c| c.id == id)
    }

    #[test]
    fn html_form_constraints() {
        let dir = TempDir::new("html");
        dir.write(
            "index.html",
            "<form>\n  <input id=\"email\" type=\"email\" required>\n  <input name=\"username\" minlength=\"3\" maxlength=\"20\" pattern=\"[a-z]+\">\n</form>\n",
        );
        let cands = extract(&dir.0);

        let required = find(&cands, "contract:input:email:required");
        assert_eq!(
            required.statement,
            "#email is required: submitting without it must not succeed"
        );
        assert_eq!(required.source, InvariantSource::CodeContract);
        assert!((required.confidence - 0.8).abs() < 1e-6);
        assert_eq!(required.evidence[0].line, Some(2));
        assert_eq!(
            required.evidence[0].file.as_deref(),
            Some(Path::new("index.html"))
        );
        assert!(required.check.is_none());

        find(&cands, "contract:input:email:email");
        let min = find(&cands, "contract:input:username:minlength");
        assert!(
            min.statement
                .starts_with("input[name=\"username\"] must be at least 3")
        );
        find(&cands, "contract:input:username:maxlength");
        find(&cands, "contract:input:username:pattern");
    }

    #[test]
    fn express_validation_status_and_auth() {
        let dir = TempDir::new("express");
        dir.write(
            "server.js",
            r#"const app = express();
app.post('/api/todos', (req, res) => {
  if (!req.body.title) return res.status(400).json({ error: 'title' });
  res.status(201).json({ title: req.body.title });
});
app.post('/api/login', (req, res) => {
  const { email } = req.body;
  if (!email) {
    return res.status(422).send();
  }
  res.sendStatus(204);
});
app.get('/dashboard', requireAuth, (req, res) => {
  res.send('secret');
});
app.get('/account', (req, res) => {
  if (!req.session.user) return res.status(401).end();
  res.sendStatus(200);
});
"#,
        );
        let cands = extract(&dir.0);

        let title = find(&cands, "contract:validate:POST /api/todos:title");
        assert_eq!(
            title.statement,
            "POST /api/todos: missing title is rejected with 4xx"
        );
        assert_eq!(title.evidence[0].line, Some(3));
        assert!((title.confidence - 0.7).abs() < 1e-6);

        let email = find(&cands, "contract:validate:POST /api/login:email");
        assert_eq!(email.evidence[0].line, Some(8));

        let created = find(&cands, "contract:status:POST /api/todos");
        assert_eq!(created.statement, "POST /api/todos responds 201 on success");
        assert!((created.confidence - 0.6).abs() < 1e-6);
        assert_eq!(
            find(&cands, "contract:status:POST /api/login").statement,
            "POST /api/login responds 204 on success"
        );
        assert_eq!(
            find(&cands, "contract:status:GET /account").statement,
            "GET /account responds 200 on success"
        );

        let dash = find(&cands, "contract:auth:GET /dashboard");
        assert_eq!(
            dash.statement,
            "GET /dashboard requires a session: an anonymous visitor must not see its content"
        );
        assert!((dash.confidence - 0.8).abs() < 1e-6);
        assert_eq!(dash.evidence[0].line, Some(13));

        let account = find(&cands, "contract:auth:GET /account");
        assert!((account.confidence - 0.6).abs() < 1e-6);
        assert!(!has(&cands, "contract:auth:POST /api/todos"));
    }

    #[test]
    fn zod_object_fields() {
        let dir = TempDir::new("zod");
        dir.write(
            "schema.ts",
            r#"import { z } from 'zod';
export const Signup = z.object({
  email: z.string().email(),
  name: z.string().min(3),
});
"#,
        );
        let cands = extract(&dir.0);
        assert_eq!(
            find(&cands, "contract:schema:email:string").statement,
            "field email must be a string, email"
        );
        assert_eq!(
            find(&cands, "contract:schema:name:string").statement,
            "field name must be a string, min 3"
        );
    }

    #[test]
    fn sql_not_null_and_unique() {
        let dir = TempDir::new("sql");
        dir.write(
            "db/schema.sql",
            r#"CREATE TABLE users (
  id INTEGER PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  nickname TEXT,
  score NUMERIC(10, 2) NOT NULL,
  UNIQUE (nickname)
);
"#,
        );
        let cands = extract(&dir.0);
        let not_null = find(&cands, "contract:sql:users.email:not_null");
        assert_eq!(not_null.statement, "users.email is never null");
        assert!((not_null.confidence - 0.9).abs() < 1e-6);
        assert_eq!(
            find(&cands, "contract:sql:users.email:unique").statement,
            "users.email is unique"
        );
        find(&cands, "contract:sql:users.score:not_null");
        find(&cands, "contract:sql:users.nickname:unique");
        assert!(!has(&cands, "contract:sql:users.nickname:not_null"));
        assert!(!has(&cands, "contract:sql:users.id:not_null"));
    }

    #[test]
    fn typescript_interface_members() {
        let dir = TempDir::new("ts");
        dir.write(
            "types.ts",
            r#"export interface Todo {
  id: number;
  title: string;
  done?: boolean;
  tags: string[];
  toggle(): void;
}
"#,
        );
        let cands = extract(&dir.0);
        let id = find(&cands, "contract:ts:Todo.id");
        assert_eq!(id.statement, "Todo.id is always present (number)");
        assert!((id.confidence - 0.4).abs() < 1e-6);
        assert_eq!(
            find(&cands, "contract:ts:Todo.tags").statement,
            "Todo.tags is always present (string[])"
        );
        assert!(!has(&cands, "contract:ts:Todo.done"));
        assert!(!has(&cands, "contract:ts:Todo.toggle"));
    }

    #[test]
    fn skips_dependency_and_build_dirs() {
        let dir = TempDir::new("skip");
        dir.write(
            "node_modules/lib/index.html",
            "<input id=\"ghost\" required>",
        );
        dir.write(
            "node_modules/lib/server.js",
            "app.get('/ghost', requireAuth, (req, res) => {});",
        );
        dir.write("dist/bundle.html", "<input id=\"built\" required>");
        dir.write("src/app.html", "<input id=\"real\" required>");
        let cands = extract(&dir.0);
        assert!(cands.iter().all(|c| !c.id.contains("ghost")));
        assert!(!has(&cands, "contract:input:built:required"));
        find(&cands, "contract:input:real:required");
    }

    #[test]
    fn output_is_sorted_by_id() {
        let dir = TempDir::new("sorted");
        dir.write(
            "app.html",
            "<input id=\"zeta\" required><input id=\"alpha\" required>",
        );
        let cands = extract(&dir.0);
        let ids: Vec<&str> = cands.iter().map(|c| c.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }
}
