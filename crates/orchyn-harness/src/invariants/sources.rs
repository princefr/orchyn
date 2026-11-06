//! Invariants from intent and production signals.
//!
//! * `from_transcript`: requirements the user stated in a vibecoding chat, read
//!   from Markdown or JSON. Only user turns count; assistant text is not intent.
//! * `from_production_log`: NDJSON request and error events. Routes that are
//!   reliably successful, and errors that keep recurring, become invariants.
//!
//! No model is involved: sentences are kept by keyword markers, and production
//! events are aggregated by counting.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::{Evidence, InvariantCandidate, InvariantSource};

/// Characters of source text kept inside an evidence quote.
const QUOTE_CHARS: usize = 150;
const MAX_STATEMENT_CHARS: usize = 200;
/// Events a route needs before its success rate counts as an invariant.
const MIN_PRODUCTION_EVENTS: usize = 20;
/// Required share of 2xx/3xx responses, in percent.
const MIN_SUCCESS_PERCENT: usize = 99;
const REPEATED_ERROR_MIN: usize = 3;
const ERROR_TEXT_CHARS: usize = 80;

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static sources regex must compile")
}

/// Requirement markers, matched case-insensitively.
static REQUIREMENT: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?i)\b(?:must|should|always|never|make sure|ensure\w*|needs to|has to|only|every|do not)\b|\bdon['’]t\b|\bwhen\b.*(?:\bthen\b|,)|\bso that\b",
    )
});
/// Markers that earn confidence 0.7 rather than 0.5.
static STRONG: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?i)\b(?:must|never|always|ensure\w*)\b"));
static HEADING: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^\s{0,3}#{1,6}(?:\s+|$)(.*?)\s*#*\s*$"));
/// `User:`, `**User:**`, `- Human: ...` and the assistant equivalents.
static ROLE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?i)^\s*(?:[-*+]\s+)?(?:\*\*|__)?\s*(user|human|assistant|claude)\s*(?:\*\*|__)?\s*:\s*(?:\*\*|__)?\s*(.*)$",
    )
});
static LIST_MARKER: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*(?:[-*+]|\d+[.)])\s+"));

/// One user turn of a transcript.
struct UserTurn {
    /// 1-based ordinal among user turns.
    number: usize,
    /// 1-based line where the turn starts, when known.
    line: Option<usize>,
    /// Lines of the turn joined by `\n` at paragraph or list breaks.
    text: String,
    /// A blank line followed the last appended text.
    paragraph_break: bool,
}

/// `Some(true)` for user words, `Some(false)` for assistant words, `None` otherwise.
fn role_word(word: &str) -> Option<bool> {
    match word.trim().to_ascii_lowercase().as_str() {
        "user" | "human" => Some(true),
        "assistant" | "claude" | "ai" => Some(false),
        _ => None,
    }
}

fn start_turn(turns: &mut Vec<UserTurn>, line: Option<usize>, text: &str) {
    turns.push(UserTurn {
        number: turns.len() + 1,
        line,
        text: text.trim().to_string(),
        paragraph_break: false,
    });
}

fn append_line(turn: &mut UserTurn, text: &str, force_break: bool) {
    if text.is_empty() {
        return;
    }
    if !turn.text.is_empty() {
        let separator = if turn.paragraph_break || force_break {
            '\n'
        } else {
            ' '
        };
        turn.text.push(separator);
    }
    turn.text.push_str(text);
    turn.paragraph_break = false;
}

/// User turns of a Markdown transcript. A turn starts at a `## User` heading, a
/// `User:` prefix, a `>` quote block, or a plain line under a user heading.
fn markdown_user_turns(text: &str) -> Vec<UserTurn> {
    let mut turns: Vec<UserTurn> = Vec::new();
    // Whether we are inside a user section.
    let mut in_user = false;
    // Whether the last turn can still take continuation lines.
    let mut open = false;
    let mut last_quote = false;

    for (index, raw) in text.lines().enumerate() {
        let line = Some(index + 1);
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            if open && let Some(turn) = turns.last_mut() {
                turn.paragraph_break = true;
            }
            continue;
        }
        if let Some(caps) = HEADING.captures(raw) {
            in_user = role_word(&caps[1]) == Some(true);
            open = false;
            last_quote = false;
            continue;
        }
        if let Some(caps) = ROLE_PREFIX.captures(raw) {
            in_user = role_word(&caps[1]) == Some(true);
            open = false;
            last_quote = false;
            if in_user {
                start_turn(&mut turns, line, &caps[2]);
                open = true;
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('>') {
            let content = rest.trim();
            match turns.last_mut() {
                Some(turn) if open && last_quote => append_line(turn, content, false),
                _ => start_turn(&mut turns, line, content),
            }
            open = true;
            last_quote = true;
            continue;
        }
        if in_user {
            let force_break = LIST_MARKER.is_match(trimmed);
            match turns.last_mut() {
                Some(turn) if open && !last_quote => append_line(turn, trimmed, force_break),
                _ => start_turn(&mut turns, line, trimmed),
            }
            open = true;
        } else {
            open = false;
        }
        last_quote = false;
    }
    turns
}

fn invalid_data(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Text of a message `content`, which is a string or an array of `{type, text}` blocks.
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// 1-based line where `text` first appears in `raw` at or after `cursor`, in its
/// JSON-escaped form. Advances `cursor` past the match so repeated turns map in order.
fn locate_json_text(raw: &str, text: &str, cursor: &mut usize) -> Option<usize> {
    let escaped = serde_json::to_string(text).ok()?;
    let inner = escaped.get(1..escaped.len().saturating_sub(1))?;
    let needle: String = inner.chars().take(40).collect();
    if needle.is_empty() {
        return None;
    }
    let found = raw.get(*cursor..)?.find(&needle)? + *cursor;
    *cursor = found + needle.len();
    Some(raw[..found].matches('\n').count() + 1)
}

/// User turns of a JSON transcript: `[{"role": "user", "content": "..."}]`.
fn json_user_turns(raw: &str) -> io::Result<Vec<UserTurn>> {
    let value: Value = serde_json::from_str(raw).map_err(invalid_data)?;
    let Value::Array(items) = value else {
        return Err(invalid_data("transcript JSON must be an array of turns"));
    };
    let mut turns = Vec::new();
    let mut cursor = 0usize;
    for item in &items {
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        if role_word(role) != Some(true) {
            continue;
        }
        let text = content_text(item.get("content"));
        let line = locate_json_text(raw, &text, &mut cursor);
        turns.push(UserTurn {
            number: turns.len() + 1,
            line,
            text: text.trim().to_string(),
            paragraph_break: false,
        });
    }
    Ok(turns)
}

/// Splits a line into sentences at `.`, `!` or `?` followed by whitespace or the end.
fn split_sentences(line: &str) -> Vec<String> {
    fn flush(out: &mut Vec<String>, current: &mut String) {
        let sentence = current.trim();
        if !sentence.is_empty() {
            out.push(sentence.to_string());
        }
        current.clear();
    }

    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        current.push(c);
        let at_end = matches!(c, '.' | '!' | '?')
            && match chars.get(i + 1) {
                None => true,
                Some(next) => next.is_whitespace(),
            };
        if at_end {
            flush(&mut out, &mut current);
        }
    }
    flush(&mut out, &mut current);
    out
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{head}...")
}

/// Sentence without list markers and with collapsed whitespace, capped at 200 chars.
fn clean_statement(sentence: &str) -> String {
    let without_marker = LIST_MARKER.replace(sentence.trim(), "");
    let collapsed = without_marker
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate(&collapsed, MAX_STATEMENT_CHARS)
}

fn quote(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("\"{}\"", truncate(&collapsed, QUOTE_CHARS))
}

fn sentence_candidates(turns: &[UserTurn], path: &Path) -> Vec<InvariantCandidate> {
    let mut out = Vec::new();
    for turn in turns {
        let mut sentence_no = 0usize;
        for line in turn.text.split('\n') {
            for sentence in split_sentences(line) {
                sentence_no += 1;
                if !REQUIREMENT.is_match(&sentence) {
                    continue;
                }
                let strong = STRONG.is_match(&sentence);
                out.push(InvariantCandidate {
                    id: format!("transcript:{}:{}", turn.number, sentence_no),
                    source: InvariantSource::Transcript,
                    statement: clean_statement(&sentence),
                    evidence: vec![Evidence {
                        file: Some(path.to_path_buf()),
                        line: turn.line,
                        note: quote(&sentence),
                    }],
                    confidence: if strong { 0.7 } else { 0.5 },
                    check: None,
                });
            }
        }
    }
    out
}

/// Requirements the user stated in a transcript, one candidate per requirement
/// sentence. Accepts Markdown or a JSON array of `{"role", "content"}` turns
/// (a `.json` file must be JSON). Assistant turns are ignored.
pub fn from_transcript(path: &Path) -> io::Result<Vec<InvariantCandidate>> {
    let raw = fs::read_to_string(path)?;
    let is_json_file = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    let turns = if is_json_file {
        json_user_turns(&raw)?
    } else if raw.trim_start().starts_with('[') {
        match json_user_turns(&raw) {
            Ok(turns) => turns,
            Err(_) => markdown_user_turns(&raw),
        }
    } else {
        markdown_user_turns(&raw)
    };
    Ok(sentence_candidates(&turns, path))
}

/// Normalises a URL or route to `/path`: origin, query and fragment dropped,
/// all-digit segments become `:n`, trailing slash removed.
fn normalize_route(raw: &str) -> String {
    let mut rest = raw.trim();
    if let Some(idx) = rest.find("://") {
        let after = &rest[idx + 3..];
        rest = after.find('/').map_or("/", |i| &after[i..]);
    }
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let mut out = path
        .split('/')
        .map(|seg| {
            if !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()) {
                ":n".to_string()
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

/// Invariants from a production NDJSON log, one JSON event per line.
///
/// Events may carry `method`, `route`/`url`/`path`, `status`, and `error` or
/// `message`. Unparsable lines are ignored. A route needs at least 20 events
/// with a status, and at least 99% of them below 400, to become an invariant.
/// An error message seen at least 3 times becomes one too.
pub fn from_production_log(path: &Path) -> io::Result<Vec<InvariantCandidate>> {
    let mut reader = BufReader::new(File::open(path)?);
    // (method, route) -> (responses below 400, responses with a status)
    let mut routes: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    let mut buf = Vec::new();

    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let text = String::from_utf8_lossy(&buf);
        let Ok(Value::Object(event)) = serde_json::from_str::<Value>(text.trim()) else {
            continue;
        };

        let method = event
            .get("method")
            .and_then(Value::as_str)
            .map(|m| m.trim().to_ascii_uppercase())
            .filter(|m| !m.is_empty());
        let route = ["route", "url", "path"]
            .iter()
            .find_map(|key| event.get(*key).and_then(Value::as_str))
            .map(normalize_route);
        let status = event.get("status").and_then(|s| {
            s.as_u64()
                .or_else(|| s.as_str().and_then(|t| t.trim().parse::<u64>().ok()))
        });
        if let (Some(method), Some(route), Some(status)) = (method, route, status) {
            let entry = routes.entry((method, route)).or_insert((0, 0));
            entry.1 += 1;
            if status < 400 {
                entry.0 += 1;
            }
        }

        let message = event
            .get("error")
            .and_then(Value::as_str)
            .or_else(|| event.get("message").and_then(Value::as_str));
        if let Some(message) = message {
            let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
            if !normalized.is_empty() {
                *errors
                    .entry(truncate(&normalized, ERROR_TEXT_CHARS))
                    .or_insert(0) += 1;
            }
        }
    }

    let mut out = Vec::new();
    for ((method, route), (ok, total)) in routes {
        if total < MIN_PRODUCTION_EVENTS || ok * 100 < total * MIN_SUCCESS_PERCENT {
            continue;
        }
        let key = format!("{method} {route}");
        let counts = format!("{ok}/{total} requests were 2xx/3xx");
        out.push(InvariantCandidate {
            id: format!("production:status:{key}"),
            source: InvariantSource::Production,
            statement: format!("{key} stays successful ({counts} in production)"),
            evidence: vec![Evidence {
                file: Some(path.to_path_buf()),
                line: None,
                note: counts,
            }],
            confidence: 0.8,
            check: None,
        });
    }
    for (message, count) in errors {
        if count < REPEATED_ERROR_MIN {
            continue;
        }
        out.push(InvariantCandidate {
            id: format!("production:error:{message}"),
            source: InvariantSource::Production,
            statement: format!("error '{message}' must not recur (seen {count} times)"),
            evidence: vec![Evidence {
                file: Some(path.to_path_buf()),
                line: None,
                note: format!("seen {count} times"),
            }],
            confidence: 0.6,
            check: None,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!(
                "orchyn-sources-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, rel: &str, text: &str) -> std::path::PathBuf {
            let path = self.0.join(rel);
            fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn ids(cands: &[InvariantCandidate]) -> Vec<&str> {
        cands.iter().map(|c| c.id.as_str()).collect()
    }

    const PROMPTS_MD: &str = r#"# Build notes

## User

Build a counter app. The count must persist after refresh.
Make it look nice when possible, then ship it.

## Assistant

Sure. The count must never reset because I always store it. You should ignore this.

**User:** Users should only see their own todos, so that data never leaks. Don't show the admin panel.

> Every save must show a toast.
"#;

    #[test]
    fn markdown_transcript_counts_only_user_turns() {
        let dir = TempDir::new("md");
        let path = dir.write("PROMPTS.md", PROMPTS_MD);
        let cands = from_transcript(&path).unwrap();

        assert_eq!(
            ids(&cands),
            vec![
                "transcript:1:2",
                "transcript:1:3",
                "transcript:2:1",
                "transcript:2:2",
                "transcript:3:1",
            ]
        );
        assert!(cands.iter().all(|c| !c.statement.contains("never reset")));

        let must = &cands[0];
        assert_eq!(must.statement, "The count must persist after refresh.");
        assert!((must.confidence - 0.7).abs() < 1e-6);
        assert_eq!(must.evidence[0].line, Some(5));
        assert_eq!(must.source, InvariantSource::Transcript);
        assert_eq!(must.evidence[0].file.as_deref(), Some(path.as_path()));

        let when = &cands[1];
        assert!((when.confidence - 0.5).abs() < 1e-6);

        let so_that = &cands[2];
        assert_eq!(
            so_that.statement,
            "Users should only see their own todos, so that data never leaks."
        );
        assert!((so_that.confidence - 0.7).abs() < 1e-6);

        assert_eq!(cands[3].statement, "Don't show the admin panel.");
        assert!((cands[3].confidence - 0.5).abs() < 1e-6);

        let toast = &cands[4];
        assert_eq!(toast.evidence[0].line, Some(14));
        assert!((toast.confidence - 0.7).abs() < 1e-6);
    }

    #[test]
    fn json_transcript_counts_only_user_turns() {
        let dir = TempDir::new("json");
        let path = dir.write(
            "chat.json",
            r#"[
  {"role":"user","content":"The save button must always be enabled. Keep it simple."},
  {"role":"assistant","content":"I will never disable it."},
  {"role":"user","content":[{"type":"text","text":"Make sure the list is sorted."}]},
  {"role":"human","content":"When the list is empty, then show a hint."}
]"#,
        );
        let cands = from_transcript(&path).unwrap();
        assert_eq!(
            ids(&cands),
            vec!["transcript:1:1", "transcript:2:1", "transcript:3:1"]
        );
        assert_eq!(
            cands[0].statement,
            "The save button must always be enabled."
        );
        assert!((cands[0].confidence - 0.7).abs() < 1e-6);
        assert_eq!(cands[0].evidence[0].line, Some(2));
        assert_eq!(cands[1].statement, "Make sure the list is sorted.");
        assert!((cands[1].confidence - 0.5).abs() < 1e-6);
        assert_eq!(cands[1].evidence[0].line, Some(4));
        assert!((cands[2].confidence - 0.5).abs() < 1e-6);
    }

    #[test]
    fn invalid_json_transcript_is_an_error() {
        let dir = TempDir::new("badjson");
        let path = dir.write("chat.json", "{not json");
        assert!(from_transcript(&path).is_err());
    }

    fn production_fixture(dir: &TempDir) -> std::path::PathBuf {
        let mut lines: Vec<String> = Vec::new();
        for i in 1..=25 {
            lines.push(format!(
                r#"{{"method":"GET","url":"http://localhost:3000/api/items/{i}?v=1","status":200}}"#
            ));
        }
        for i in 0..25 {
            let line = if i < 3 {
                r#"{"method":"POST","route":"/api/orders","status":500,"error":"upstream timeout"}"#
            } else {
                r#"{"method":"POST","route":"/api/orders","status":201}"#
            };
            lines.push(line.to_string());
        }
        for _ in 0..5 {
            lines.push(r#"{"method":"GET","route":"/api/small","status":200}"#.to_string());
        }
        lines.extend(["not json", "{broken", "", "[1,2,3]", "42"].map(String::from));
        dir.write("prod.ndjson", &lines.join("\n"))
    }

    #[test]
    fn production_log_status_and_repeated_errors() {
        let dir = TempDir::new("prod");
        let path = production_fixture(&dir);
        let cands = from_production_log(&path).unwrap();

        assert_eq!(
            ids(&cands),
            vec![
                "production:error:upstream timeout",
                "production:status:GET /api/items/:n",
            ]
        );

        let hot = cands
            .iter()
            .find(|c| c.id == "production:status:GET /api/items/:n")
            .unwrap();
        assert_eq!(
            hot.statement,
            "GET /api/items/:n stays successful (25/25 requests were 2xx/3xx in production)"
        );
        assert!((hot.confidence - 0.8).abs() < 1e-6);
        assert_eq!(hot.source, InvariantSource::Production);
        assert_eq!(hot.evidence[0].note, "25/25 requests were 2xx/3xx");

        let error = cands
            .iter()
            .find(|c| c.id == "production:error:upstream timeout")
            .unwrap();
        assert_eq!(
            error.statement,
            "error 'upstream timeout' must not recur (seen 3 times)"
        );
        assert!((error.confidence - 0.6).abs() < 1e-6);
        assert_eq!(error.evidence[0].note, "seen 3 times");
    }

    #[test]
    fn production_route_below_threshold_is_not_an_invariant() {
        let dir = TempDir::new("prod-flaky");
        let path = production_fixture(&dir);
        let cands = from_production_log(&path).unwrap();
        assert!(cands.iter().all(|c| !c.id.contains("/api/orders")));
        assert!(cands.iter().all(|c| !c.id.contains("/api/small")));
    }

    #[test]
    fn normalizes_routes() {
        assert_eq!(
            normalize_route("http://h:3000/api/items/12?x=1"),
            "/api/items/:n"
        );
        assert_eq!(normalize_route("/api/items/"), "/api/items");
        assert_eq!(normalize_route("/"), "/");
        assert_eq!(normalize_route("/a/b#frag"), "/a/b");
    }
}
