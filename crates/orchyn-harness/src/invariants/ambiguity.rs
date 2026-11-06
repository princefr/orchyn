//! Spec ambiguity lint: model-free heuristics that point at phrases in a task
//! spec a coding agent could read more than one way. Examples: "rating value
//! from 0 to 1" (fraction, index or count?), "append to the form" (before or
//! after the submit button?), "remove the question" (removed from the DOM or
//! hidden?). Each finding carries one concrete question and the plausible
//! readings, so a human or the agent can settle it before coding starts.
//!
//! Rules are regex-based and conservative. A finding is suppressed when the
//! same line already settles the question: a stated unit or format, a stated
//! storage place, an HTTP status, a visibility qualifier, a stated default.
//! Nothing here calls a model, and the linter never changes an exit code.

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ambiguity {
    pub kind: String,
    /// Up to 160 characters of the line around the matched phrase.
    pub excerpt: String,
    /// 1-based line number in the linted text.
    pub line: usize,
    /// One concrete question to settle the ambiguity.
    pub question: String,
    /// Two to four plausible interpretations.
    pub readings: Vec<String>,
}

/// Lint `text` and return every ambiguity found, sorted by line. Duplicates
/// (same kind, line and question) are removed.
pub fn lint(text: &str) -> Vec<Ambiguity> {
    let lines: Vec<&str> = text.lines().collect();
    let scopes = Scopes::new(&lines);
    let rx = rules();
    let mut hits = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        // Headings are titles, not requirements.
        if line.trim_start().starts_with('#') {
            continue;
        }
        value_range(rx, line, n, &mut hits);
        removal(rx, line, n, &mut hits);
        visibility(rx, line, n, scopes.section(i), &mut hits);
        vague_quality(rx, line, n, &mut hits);
        unspecified_value(rx, line, n, &mut hits);
        persistence(rx, line, n, &mut hits);
        error_feedback(rx, line, n, &mut hits);
        status_code(rx, line, n, &mut hits);
        position(rx, line, n, &scopes.scope(i), &mut hits);
    }
    naming(rx, &lines, &mut hits);

    // Stable sort: rule order is kept within a line.
    hits.sort_by_key(|h| h.line);
    let mut seen = HashSet::new();
    hits.into_iter()
        .filter(|h| seen.insert((h.kind, h.line, h.question.clone())))
        .map(|h| Ambiguity {
            kind: h.kind.to_string(),
            excerpt: h.excerpt,
            line: h.line,
            question: h.question,
            readings: h.readings,
        })
        .collect()
}

struct Hit {
    kind: &'static str,
    line: usize,
    excerpt: String,
    question: String,
    readings: Vec<String>,
}

fn hit(
    kind: &'static str,
    n: usize,
    line: &str,
    start: usize,
    end: usize,
    question: String,
    readings: Vec<String>,
) -> Hit {
    Hit {
        kind,
        line: n,
        excerpt: excerpt(line, start, end),
        question,
        readings,
    }
}

/// Up to 60 characters either side of `start..end`, capped at 160 characters.
fn excerpt(line: &str, start: usize, end: usize) -> String {
    const CONTEXT: usize = 60;
    const MAX: usize = 160;
    let chars: Vec<char> = line.chars().collect();
    let s = line[..start].chars().count();
    let e = s + line[start..end].chars().count();
    let from = s.saturating_sub(CONTEXT);
    let to = (e + CONTEXT).min(chars.len()).min(from + MAX);
    chars[from..to]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// The text each line can be read against: its own `#` section, plus the
/// shared sections (a heading containing "context", and any text before the
/// first heading). A spec's fixed elements are often stated in "Context".
struct Scopes {
    section_of: Vec<usize>,
    sections: Vec<String>,
    shared: String,
}

impl Scopes {
    fn new(lines: &[&str]) -> Self {
        let mut sections = vec![String::new()];
        let mut is_shared = vec![true];
        let mut section_of = Vec::with_capacity(lines.len());
        for line in lines {
            if line.trim_start().starts_with('#') {
                sections.push(String::new());
                is_shared.push(line.to_lowercase().contains("context"));
            }
            let idx = sections.len() - 1;
            sections[idx].push_str(line);
            sections[idx].push('\n');
            section_of.push(idx);
        }
        let shared = sections
            .iter()
            .zip(&is_shared)
            .filter(|(_, shared)| **shared)
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Scopes {
            section_of,
            sections,
            shared,
        }
    }

    fn section(&self, i: usize) -> &str {
        &self.sections[self.section_of[i]]
    }

    fn scope(&self, i: usize) -> String {
        format!("{}\n{}", self.section(i), self.shared)
    }
}

struct Rules {
    zero_one: Regex,
    between: Regex,
    percent: Regex,
    discrete: Regex,
    append: Regex,
    before_after: Regex,
    fixed: Regex,
    removal_verb: Regex,
    removal_element: Regex,
    removal_settled: Regex,
    role: Regex,
    vis_element: Regex,
    vis_qualifier: Regex,
    clause_break: Regex,
    vague: Regex,
    criteria: Regex,
    unspecified: Regex,
    default_value: Regex,
    format: Regex,
    persist_verb: Regex,
    persist_where: Regex,
    error_trigger: Regex,
    required: Regex,
    shown: Regex,
    endpoint: Regex,
    status: Regex,
    selector: Regex,
    attribute: Regex,
}

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static ambiguity regex must compile")
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| Rules {
        zero_one: compile(r"(?i)\bfrom\s+0\s+to\s+1\b|\b0\s*(?:-|\.\.)\s*1\b"),
        between: compile(r"(?i)\bbetween\s+-?\d+(?:\.\d+)?\s+and\s+-?\d+(?:\.\d+)?\b"),
        percent: compile(r"(?i)\bpercent(?:age|ages)?\b"),
        discrete: compile(
            r"(?i)\b(?:stars?|scores?|ratings?|levels?|counts?|votes?|points?|likes?|ranks?|ranking|items?|options?|choices?|statements?|questions?|nps)\b",
        ),
        append: compile(
            r"(?i)\b(?:append|appends|appended|insert|inserts|inserted|add|adds|added)\b[^\n]{0,80}?\bto the (?:form|list|page)\b",
        ),
        before_after: compile(
            r"(?i)\b(?:before|after|above|below|top|bottom|start|end|first|last)\b",
        ),
        fixed: compile(r"(?i)\bsubmit\s+(?:button|control)\b|\bfooter\b|\bheader\b|\bnav(?:bar)?\b"),
        removal_verb: compile(
            r"(?i)\b(?:remove|removes|removed|removing|delete|deletes|deleted|deleting|hide|hides|hidden|hiding)\b",
        ),
        removal_element: compile(
            r"(?i)\b(?:button|link|panel|item|row|option|question|card|section|element|field|column|statement|cell|menu|tab|image|box|popover|list|contents?|div)\b",
        ),
        removal_settled: compile(
            r#"(?i)display\s*:\s*none|from the dom|in the dom|disabled|still in|stays? in|visibility|type\s*=\s*['"]?hidden"#,
        ),
        role: compile(
            r"(?i)\b(?:admins?|administrators?|owners?|logged[- ]in|signed[- ]in|roles?|privileges?|superusers?|moderators?|portal)\b",
        ),
        vis_element: compile(
            r"(?i)\b(?:links?|buttons?|sections?|panels?|menus?|nav|navigation|tabs?|entry|header|footer)\b",
        ),
        clause_break: compile(r"[.;]\s+|\s\d+\)\s"),
        vis_qualifier: compile(
            r"(?i)\b(?:only|visible|hidden|shown|everyone|anyone|all users|public|guests?|for all|for (?:admins?|owners?|users?)|to (?:admins?|owners?)|if (?:not )?(?:logged|signed)|when (?:not )?(?:logged|signed)|not shown)\b",
        ),
        vague: compile(
            r"(?i)\b(?:beautify|beautified|beautiful|beautifully|nice|nicely|appealing|appealingly|modern|pretty|elegant|stylish|attractive|responsive)\b",
        ),
        criteria: compile(
            r"(?i)\d+\s*(?:(?:px|rem|em|ms|columns?)\b|%)|\b(?:colou?rs?|fonts?|spacing|margins?|paddings?|grid|breakpoints?|contrast|shadows?|flexbox|palette|radius|animations?|hover|dark mode|light mode)\b",
        ),
        unspecified: compile(r"(?i)\b(?:random|randomly|unique|uniquely|default)\b"),
        default_value: compile(r#"^\s*(?:[:=]|to\b|-?\d|[A-Z]|['"]|true\b|false\b|null\b)"#),
        format: compile(
            r"(?i)\bformat\b|\bpatterns?\b|\blength\b|\bcharacters?\b|\bdigits?\b|\bletters?\b|\buuids?\b|\bseed\b|\bexample\b|\be\.g\.|\bsuch as\b|\d+\s*(?:chars?|characters?|digits?|letters?|words?)\b|\balphanumeric\b|\bnumeric\b",
        ),
        persist_verb: compile(
            r"(?i)\b(?:save|saves|saved|saving|store|stores|stored|storing|remember|remembers|remembered|persist|persists|persisted)\b",
        ),
        persist_where: compile(
            r"(?i)storage|indexeddb|cookies?|database|\bdb\b|\bserver\b|backend|\bapi\b|\bfiles?\b|\.(?:js|json|sql|db|txt|csv|html|css)\b|sqlite|postgres|mysql|\btables?\b|\bcontrols?\b|\binputs?\b|\bfields?\b|\bcache\b|\bdisk\b",
        ),
        error_trigger: compile(
            r"(?i)\b(?:validation|validate|validates|validated|invalid|fails|failed|failure|errors?|not be submitted|can ?not be submitted|must (?:not )?be (?:valid|filled|empty|entered|provided|selected|checked))\b",
        ),
        required: compile(r"(?i)\brequired\b"),
        shown: compile(
            r"(?i)\balerts?\b|\bdialog\b|\binline\b|\bmessages?\b|\bnative\b|\battribute\b|\btooltip\b|\btoast\b|\bdisabled?\b|\bhighlight\w*|\bnotif\w*|\bshow\w*\s+(?:an?\s+|the\s+)?(?:error|alert|message|warning)\b|\bred\b|\boutline\b",
        ),
        endpoint: compile(r#"\b(GET|POST|PUT|PATCH|DELETE)\s+(/[^\s'"`,;)]*)"#),
        status: compile(r"(?i)\bstatus\b|\b(?:200|201|204|301|302|400|401|403|404|409|422|500)\b"),
        selector: compile(r"[.#]([a-z][a-z0-9]*(?:[-_][a-z0-9]+)+)"),
        attribute: compile(
            r#"(?i)\b(?:class|classname|id)\b\s*(?:[:=]\s*)?['"`]?([a-z][a-z0-9]*(?:[-_][a-z0-9]+)+)"#,
        ),
    })
}

/// Numeric ranges on a discrete thing: "from 0 to 1", "0-1", "0..1",
/// "between X and Y", "percentage" (only when the line counts stars, scores,
/// ratings, options and the like; a continuous slider is not flagged).
fn value_range(rx: &Rules, line: &str, n: usize, hits: &mut Vec<Hit>) {
    if !rx.discrete.is_match(line) {
        return;
    }
    for re in [&rx.zero_one, &rx.between, &rx.percent] {
        for m in re.find_iter(line) {
            hits.push(hit(
                "value_range",
                n,
                line,
                m.start(),
                m.end(),
                "Is this value a normalised fraction (k/n), a 0-based index, a 1-based count, or a continuous slider?".into(),
                strings(&[
                    "normalised fraction k/n",
                    "0-based index",
                    "1-based count",
                    "continuous slider",
                ]),
            ));
        }
    }
}

/// "The append/insert/add ... to the form/list/page" when the same section or
/// a context section names a fixed element (submit button, footer, header).
/// Prepend is not flagged: it always lands at the top of the container.
fn position(rx: &Rules, line: &str, n: usize, scope: &str, hits: &mut Vec<Hit>) {
    let Some(fixed) = rx.fixed.find(scope) else {
        return;
    };
    let fixed = fixed.as_str().to_lowercase();
    for m in rx.append.find_iter(line) {
        // The line already states the side of the element.
        if rx.before_after.is_match(line) {
            continue;
        }
        // "Do not add more elements to the form" is a constraint, not an insertion.
        let lead = line[..m.start()].to_lowercase();
        if lead.ends_with("not ") || lead.ends_with("never ") {
            continue;
        }
        hits.push(hit(
            "position",
            n,
            line,
            m.start(),
            m.end(),
            format!("Should the new element go before or after the {fixed} it sits next to?"),
            vec![format!("before the {fixed}"), format!("after the {fixed}")],
        ));
    }
}

/// "remove/delete/hide" an element. Flagged unless the line already says how
/// (display:none, removed from the DOM, disabled).
fn removal(rx: &Rules, line: &str, n: usize, hits: &mut Vec<Hit>) {
    if rx.removal_settled.is_match(line) {
        return;
    }
    let Some(el) = rx.removal_element.find(line) else {
        return;
    };
    let el = el.as_str().to_lowercase();
    for m in rx.removal_verb.find_iter(line) {
        hits.push(hit(
            "removal",
            n,
            line,
            m.start(),
            m.end(),
            format!(
                "'{}' is ambiguous for this {el}: should it be removed from the DOM, hidden with display:none, or disabled?",
                m.as_str()
            ),
            strings(&[
                "removed from the DOM",
                "hidden with display:none (still in the DOM)",
                "disabled (visible but inert)",
            ]),
        ));
    }
}

/// A link, button, section or panel tied to a role (admin, owner, logged-in)
/// without saying who may see it. The role may come from the section heading,
/// so "Admin portal" headings count for the links beneath them.
fn visibility(rx: &Rules, line: &str, n: usize, section: &str, hits: &mut Vec<Hit>) {
    let Some(el) = rx.vis_element.find(line) else {
        return;
    };
    // Qualifiers count only within the element's own clause, so "Admin portal
    // for admin role" elsewhere on the line does not settle the link's audience.
    let start = rx
        .clause_break
        .find_iter(&line[..el.start()])
        .last()
        .map_or(0, |b| b.end());
    let end = rx
        .clause_break
        .find(&line[el.end()..])
        .map_or(line.len(), |b| el.end() + b.start());
    if rx.vis_qualifier.is_match(&line[start..end]) {
        return;
    }
    let Some(role) = rx.role.find(section) else {
        return;
    };
    let role = role.as_str().to_lowercase();
    let el_text = el.as_str().to_lowercase();
    hits.push(hit(
        "visibility_scope",
        n,
        line,
        el.start(),
        el.end(),
        format!("Who should see this {el_text}: everyone, or only {role} users?"),
        vec![
            "visible to everyone".into(),
            format!("visible only to {role} users"),
        ],
    ));
}

/// Subjective styling words with no criteria on the line. Informational:
/// one finding per line, low priority.
fn vague_quality(rx: &Rules, line: &str, n: usize, hits: &mut Vec<Hit>) {
    if rx.criteria.is_match(line) {
        return;
    }
    let Some(m) = rx.vague.find(line) else {
        return;
    };
    let word = m.as_str().to_lowercase();
    hits.push(hit(
        "vague_quality",
        n,
        line,
        m.start(),
        m.end(),
        format!(
            "What does '{word}' mean here? Give concrete criteria (colours, spacing, breakpoints, contrast), or accept any reasonable styling (low priority)."
        ),
        strings(&[
            "any reasonable styling",
            "a specific visual spec (colours, spacing, fonts)",
            "matches a named reference design",
        ]),
    ));
}

/// "random", "unique" or "default" with no format or value. "default" is
/// settled when a value follows it ("default 5", "default false").
fn unspecified_value(rx: &Rules, line: &str, n: usize, hits: &mut Vec<Hit>) {
    for m in rx.unspecified.find_iter(line) {
        let word = m.as_str().to_lowercase();
        if word == "default" {
            if rx.default_value.is_match(&line[m.end()..]) {
                continue;
            }
            hits.push(hit(
                "unspecified_value",
                n,
                line,
                m.start(),
                m.end(),
                format!(
                    "What value should '{}' take when none is given?",
                    m.as_str()
                ),
                strings(&[
                    "the literal value written in the spec",
                    "empty or none",
                    "read from configuration",
                ]),
            ));
            continue;
        }
        if rx.format.is_match(line) {
            continue;
        }
        hits.push(hit(
            "unspecified_value",
            n,
            line,
            m.start(),
            m.end(),
            format!("What format should the {word} value take: length, character set, seed, or uniqueness scope?"),
            strings(&[
                "fixed-length random word",
                "uuid or counter-based id",
                "random number suffix",
                "pick from a fixed sample list",
            ]),
        ));
    }
}

/// save/store/remember with no place named (localStorage, database, server,
/// cookie, file, form control). Read-back timing is not checked: a named place
/// is enough to stop the question.
fn persistence(rx: &Rules, line: &str, n: usize, hits: &mut Vec<Hit>) {
    if rx.persist_where.is_match(line) {
        return;
    }
    for m in rx.persist_verb.find_iter(line) {
        hits.push(hit(
            "persistence",
            n,
            line,
            m.start(),
            m.end(),
            format!(
                "Where does '{}' keep the data (localStorage key, server database, cookie), and when is it read back?",
                m.as_str()
            ),
            strings(&["localStorage under a named key", "server database, per user", "cookie"]),
        ));
    }
}

/// Validation, required fields or failure without saying how the failure is
/// shown. A `required` that is part of a name ("q-required", "Question.required
/// (boolean)") or that names the native attribute is not a trigger.
fn error_feedback(rx: &Rules, line: &str, n: usize, hits: &mut Vec<Hit>) {
    if rx.shown.is_match(line) {
        return;
    }
    let Some(m) = rx
        .error_trigger
        .find(line)
        .or_else(|| first_required(rx, line))
    else {
        return;
    };
    hits.push(hit(
        "error_feedback",
        n,
        line,
        m.start(),
        m.end(),
        "When validation fails, how is the failure shown to the user: alert, inline message, native validation, or a disabled submit button?".into(),
        strings(&[
            "alert dialog",
            "inline message next to the field",
            "native browser validation (required attribute)",
            "submit button disabled until valid",
        ]),
    ));
}

fn first_required<'a>(rx: &Rules, line: &'a str) -> Option<regex::Match<'a>> {
    rx.required.find_iter(line).find(|m| {
        let before = line[..m.start()].chars().last();
        let after = line[m.end()..].trim_start();
        let after_char = line[m.end()..].chars().next();
        let part_of_name =
            matches!(before, Some('.' | '-' | '_')) || matches!(after_char, Some('-' | '_'));
        !part_of_name && !after.starts_with('(') && !after.starts_with("attribute")
    })
}

/// An endpoint ("POST /api/products") whose own clause (up to the next
/// endpoint on the line, at most 300 bytes) names no status code.
fn status_code(rx: &Rules, line: &str, n: usize, hits: &mut Vec<Hit>) {
    let caps: Vec<Captures> = rx.endpoint.captures_iter(line).collect();
    for (i, c) in caps.iter().enumerate() {
        let whole = c.get(0).expect("group 0 always present");
        let next = caps
            .get(i + 1)
            .map(|next| next.get(0).expect("group 0 always present").start())
            .unwrap_or(line.len());
        let mut stop = next.min(whole.end() + 300).min(line.len());
        while !line.is_char_boundary(stop) {
            stop -= 1;
        }
        if rx.status.is_match(&line[whole.end()..stop]) {
            continue;
        }
        hits.push(hit(
            "status_code",
            n,
            line,
            whole.start(),
            whole.end(),
            format!(
                "Which status codes should `{} {}` return on success and on failure?",
                &c[1], &c[2]
            ),
            strings(&[
                "201 created on success, 400 on invalid input",
                "200 ok on success, 400 on invalid input",
                "200 ok on success, 401 or 404 on failure",
            ]),
        ));
    }
}

struct Occurrence {
    name: String,
    key: String,
    hyphen: bool,
    line: usize,
    excerpt: String,
}

/// Class and id names in two spellings ("product_card" and "product-card"),
/// or underscore names in a text whose other names use hyphens.
fn naming(rx: &Rules, lines: &[&str], hits: &mut Vec<Hit>) {
    let mut occs: Vec<Occurrence> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut found: Vec<(usize, usize, String)> = Vec::new();
        for re in [&rx.selector, &rx.attribute] {
            for c in re.captures_iter(line) {
                let g = c.get(1).expect("group 1 is the name");
                found.push((g.start(), g.end(), g.as_str().to_string()));
            }
        }
        found.sort();
        for (s, e, name) in found {
            occs.push(Occurrence {
                key: name.to_lowercase().replace(['-', '_'], ""),
                hyphen: name.contains('-'),
                excerpt: excerpt(line, s, e),
                line: i + 1,
                name,
            });
        }
    }

    let has_hyphen = occs.iter().any(|o| o.hyphen);
    let mut keys: Vec<&str> = Vec::new();
    for o in &occs {
        if !keys.contains(&o.key.as_str()) {
            keys.push(&o.key);
        }
    }
    for key in keys {
        let group: Vec<&Occurrence> = occs.iter().filter(|o| o.key == key).collect();
        let mut spellings: Vec<&str> = Vec::new();
        for o in &group {
            if !spellings.contains(&o.name.as_str()) {
                spellings.push(&o.name);
            }
        }
        let first = group[0];
        if spellings.len() > 1 {
            let list = spellings
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", ");
            hits.push(Hit {
                kind: "naming_variant",
                line: first.line,
                excerpt: first.excerpt.clone(),
                question: format!(
                    "The same name appears as {list}: which exact spelling should the code use?"
                ),
                readings: spellings.iter().take(4).map(|s| s.to_string()).collect(),
            });
        } else if has_hyphen && first.name.contains('_') {
            let name = &first.name;
            hits.push(Hit {
                kind: "naming_variant",
                line: first.line,
                excerpt: first.excerpt.clone(),
                question: format!(
                    "Which exact spelling should `{name}` use, given that the other class and id names use hyphens?"
                ),
                readings: vec![name.clone(), name.replace('_', "-"), camel_case(name)],
            });
        }
    }
}

fn camel_case(name: &str) -> String {
    let mut out = String::new();
    for (i, part) in name.split(['-', '_']).filter(|p| !p.is_empty()).enumerate() {
        if i == 0 {
            out.push_str(part);
        } else {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<String> {
        lint(text).into_iter().map(|a| a.kind).collect()
    }

    fn flags(text: &str, kind: &str) -> bool {
        kinds(text).iter().any(|k| k == kind)
    }

    #[test]
    fn value_range_flags_zero_one_on_a_rating() {
        assert!(flags(
            "Get the rating value (stored in a param-named control) from 0 to 1.",
            "value_range"
        ));
        assert!(flags(
            "Each score is a fraction between 0-1 of the stars.",
            "value_range"
        ));
    }

    #[test]
    fn value_range_ignores_continuous_slider() {
        assert!(!flags(
            "The opacity slider value goes from 0 to 1 in steps.",
            "value_range"
        ));
        assert!(!flags("Rating stars are numbered 1 to 5.", "value_range"));
    }

    #[test]
    fn position_flags_append_next_to_submit_button_from_context() {
        let text = "## Context (initial state)\n\nThe form has a submit button.\n\n## task-3\n\nAppend each question to the form.\n";
        let found = lint(text);
        let pos = found
            .iter()
            .find(|a| a.kind == "position")
            .expect("position flagged");
        assert_eq!(pos.line, 7);
        assert!(pos.question.contains("submit button"));
        assert_eq!(pos.readings.len(), 2);
    }

    #[test]
    fn position_ignores_stated_side_and_prepend() {
        let stated = "## Context\nThe form has a submit button.\n## task\nAppend the question to the form after the submit button.\n";
        assert!(!flags(stated, "position"));
        let prepend = "## Context\nThe form has a submit button.\n## task\nPrepend the question to the form.\n";
        assert!(!flags(prepend, "position"));
        let no_fixed = "## task\nAppend each question to the form.\n";
        assert!(!flags(no_fixed, "position"));
        let constraint = "## Context\nThere is a submit button in the form. Do not add more elements to the form.\n";
        assert!(!flags(constraint, "position"));
    }

    #[test]
    fn removal_flags_remove_without_mechanism() {
        assert!(flags(
            "A q-remove button is clicked to remove the question from the form.",
            "removal"
        ));
    }

    #[test]
    fn removal_ignores_stated_mechanism() {
        assert!(!flags(
            "In preview the config panel is hidden with display: none.",
            "removal"
        ));
        assert!(!flags(
            "Delete the record from the database on logout.",
            "removal"
        ));
    }

    #[test]
    fn visibility_flags_role_link_without_scope() {
        let text =
            "## Admin portal for the admin role\n\nAdd a .home-go-portal-link to the home page.\n";
        let found = lint(text);
        let vis = found
            .iter()
            .find(|a| a.kind == "visibility_scope")
            .expect("flagged");
        assert_eq!(vis.line, 3);
        assert!(vis.question.contains("admin"));
    }

    #[test]
    fn visibility_ignores_stated_condition() {
        let text = "## Header for logged-in users\n\nIf not logged in, show a login button; if logged in, show the username.\n";
        assert!(!flags(text, "visibility_scope"));
        let no_role = "## Home\n\nAdd a login link to the header.\n";
        assert!(!flags(no_role, "visibility_scope"));
    }

    #[test]
    fn naming_flags_underscore_among_hyphen_names() {
        let text = "Use .product-card for each card and .product_quantity for the input.";
        let found = lint(text);
        let naming: Vec<_> = found
            .iter()
            .filter(|a| a.kind == "naming_variant")
            .collect();
        assert_eq!(naming.len(), 1);
        assert!(naming[0].question.contains("product_quantity"));
        assert_eq!(naming[0].readings[1], "product-quantity");
    }

    #[test]
    fn naming_flags_two_spellings_of_one_name() {
        assert!(flags(
            "Each card has id product_card_1 and class product-card.",
            "naming_variant"
        ));
    }

    #[test]
    fn naming_ignores_consistent_hyphens() {
        assert!(!flags(
            "Use .product-quantity for the input and .product-card for each card.",
            "naming_variant"
        ));
    }

    #[test]
    fn vague_quality_flags_bare_styling_words() {
        assert!(flags("Create CSS to beautify the shop.", "vague_quality"));
    }

    #[test]
    fn vague_quality_ignores_stated_criteria() {
        assert!(!flags(
            "Use 16px padding and a 2-column grid to beautify the shop.",
            "vague_quality"
        ));
    }

    #[test]
    fn unspecified_value_flags_random_and_unique() {
        assert!(flags(
            "Add a card with a random title and a unique name.",
            "unspecified_value"
        ));
    }

    #[test]
    fn unspecified_value_ignores_format_and_default_values() {
        assert!(!flags(
            "The name is a 6-character random alphanumeric string.",
            "unspecified_value"
        ));
        assert!(!flags(
            "Add a starCount (number, default 5).",
            "unspecified_value"
        ));
        assert!(!flags(
            "Sign the token with the default HS256 algorithm.",
            "unspecified_value"
        ));
    }

    #[test]
    fn persistence_flags_save_without_place() {
        assert!(flags(
            "Save the survey when the user clicks Publish.",
            "persistence"
        ));
    }

    #[test]
    fn persistence_ignores_named_place() {
        assert!(!flags(
            "Save the survey to localStorage.data when the user clicks Save.",
            "persistence"
        ));
        assert!(!flags("Save codes to 'common/Question.js'.", "persistence"));
        assert!(!flags(
            "Store comma separated indices in the param-named control.",
            "persistence"
        ));
    }

    #[test]
    fn error_feedback_flags_validation_without_display() {
        assert!(flags(
            "Validate the form and block submission when a required answer is empty.",
            "error_feedback"
        ));
    }

    #[test]
    fn error_feedback_ignores_stated_display() {
        assert!(!flags(
            "Validate the form; on failure show an alert dialog.",
            "error_feedback"
        ));
        assert!(!flags(
            "Config panel checkbox (class 'q-required') associated with Question.required (boolean, default false).",
            "error_feedback"
        ));
        assert!(!flags(
            "Give priority to implement the feature; the existing codes must keep working.",
            "error_feedback"
        ));
    }

    #[test]
    fn status_code_flags_endpoint_without_status() {
        assert!(flags(
            "POST /api/products inserts a product and returns the new id.",
            "status_code"
        ));
    }

    #[test]
    fn status_code_ignores_endpoint_with_status() {
        assert!(!flags(
            "POST /api/products returns 201 with the new id, or 400 when the body is invalid.",
            "status_code"
        ));
    }

    #[test]
    fn status_code_checks_each_endpoint_clause() {
        let text = "GET /api/auth returns 401 when not logged in. POST /api/auth logs in and returns { success: true }.";
        let found = lint(text);
        let codes: Vec<_> = found.iter().filter(|a| a.kind == "status_code").collect();
        assert_eq!(codes.len(), 1);
        assert!(codes[0].question.contains("POST /api/auth"));
    }

    #[test]
    fn excerpt_is_capped_and_sorted_by_line() {
        let long = format!(
            "{} rating from 0 to 1 {}",
            "x ".repeat(120),
            "y ".repeat(120)
        );
        let text = format!("plain line\n{long}\n");
        let found = lint(&text);
        let value = found
            .iter()
            .find(|a| a.kind == "value_range")
            .expect("flagged");
        assert_eq!(value.line, 2);
        assert!(value.excerpt.chars().count() <= 160);
        assert!(value.excerpt.contains("from 0 to 1"));
        assert!(found.windows(2).all(|w| w[0].line <= w[1].line));
    }

    #[test]
    fn visibility_scopes_qualifiers_to_the_element_clause() {
        // Qualifier inside the element's clause: settled.
        let settled = "## Admin portal for the admin role\n\nAdd a link to the admin portal, visible only to admins.\n";
        assert!(!flags(settled, "visibility_scope"));
        // Qualifier in a different clause: the link itself is still unsettled.
        let other_clause = "## Admin portal for the admin role\n\nAdd a link on the home page; only admins see the portal.\n";
        assert!(flags(other_clause, "visibility_scope"));
    }

    #[test]
    fn ambiguity_round_trips_through_json() {
        let a = Ambiguity {
            kind: "value_range".into(),
            excerpt: "from 0 to 1".into(),
            line: 3,
            question: "q?".into(),
            readings: vec!["a".into(), "b".into()],
        };
        let json = serde_json::to_string(&a).expect("serialise");
        let back: Ambiguity = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(a, back);
    }
}
