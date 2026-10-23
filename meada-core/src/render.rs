//! Compact answers: line-oriented text under a token budget (ADR 0008). Every query shares
//! this, so the CLI, each MCP tool and the console cut answers the same way.

use crate::export::{CallSite, Level};

/// ` [<level>: <reason>]` after a call that is not exact. Only a name match makes a call
/// inferred or ambiguous, so the reason is how many definitions have its name: methods for a
/// member call, top-level functions and classes for a plain one.
pub fn mark(call: &CallSite) -> String {
    let count = match call.level {
        Some(Level::Exact) | None => return String::new(),
        Some(Level::Inferred) => 1,
        Some(Level::Ambiguous) => call.candidates.len(),
        Some(level) => return format!(" [{}]", level.name()),
    };
    let callee = callee(&call.id);
    let (one, many, name) = match callee.rsplit_once('.') {
        Some((_, name)) => ("method", "methods", name),
        None => ("function or class", "functions or classes", callee),
    };
    let what = match count {
        1 => format!("one {one}"),
        n => format!("{n} {many}"),
    };
    let level = call.level.map_or("", Level::name);
    format!(" [{level}: {what} named {name}]")
}

/// Tokens an answer may use unless the caller asks for another budget.
pub const DEFAULT_BUDGET: usize = 2_000;

/// `lines` joined by newlines. When they exceed `budget` tokens (`DEFAULT_BUDGET` when
/// `None`; one token is four bytes of UTF-8, so the estimate is deterministic), the answer
/// keeps the lines that fit and ends with a line saying where it stopped.
pub fn budgeted(lines: &[String], budget: Option<usize>) -> String {
    let budget = budget.unwrap_or(DEFAULT_BUDGET);
    cut(lines, bytes(budget), budget).1
}

/// The estimated tokens of `text`: one per four bytes, rounded up.
pub fn tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// The bytes `tokens` estimated tokens allow, the reverse of `tokens`.
pub fn bytes(tokens: usize) -> usize {
    tokens * 4
}

/// The bytes a line takes in an answer: its text and its newline.
pub fn line_bytes(line: &str) -> usize {
    line.len() + 1
}

/// The line that ends a cut answer: it kept `shown` of `total` lines, under `budget` tokens.
pub fn note(shown: usize, total: usize, budget: usize) -> String {
    format!(
        "… cut after {shown} of {total} lines (budget {budget} tokens); raise budget for more\n"
    )
}

/// `lines` within `cap` bytes, cut as `budgeted` cuts them, with the cut note naming `budget`
/// tokens, and how many lines it kept. `cap` is below the budget's bytes when the caller
/// appends lines of its own.
pub fn cut(lines: &[String], cap: usize, budget: usize) -> (usize, String) {
    let all: usize = lines.iter().map(|l| line_bytes(l)).sum();
    let note = |shown: usize| note(shown, lines.len(), budget);
    let mut shown = lines.len();
    if all > cap {
        // The note's length depends on `shown`'s digits: reserve for the widest.
        let room = cap.saturating_sub(note(lines.len()).len());
        let mut used = 0;
        shown = lines
            .iter()
            .take_while(|l| {
                used += line_bytes(l);
                used <= room
            })
            .count();
    }
    let mut out: String = lines[..shown]
        .iter()
        .flat_map(|l| [l.as_str(), "\n"])
        .collect();
    if shown < lines.len() {
        out.push_str(&note(shown));
    }
    (shown, out)
}

/// How a side that is a whole repository shows: a resource declared with no `via`.
pub const REPOSITORY_LEVEL: &str = "(repository-level)";

/// How a side only a provider document lists shows (spec #21): no code of it is indexed.
pub const DOCUMENTED_ONLY: &str = "(documented only)";

/// The name part of a symbol identity: `<repository>::<path>::<name>`.
pub fn label(id: &str) -> &str {
    id.rsplit_once("::").map_or(id, |(_, name)| name)
}

/// The callee of the call site `<caller>::<callee>#<ordinal>`.
pub fn callee(site: &str) -> &str {
    let callee = label(site);
    callee.rsplit_once('#').map_or(callee, |(name, _)| name)
}

/// How an unresolved consumer shows: `(unresolved) fetch` for the call site
/// `<caller>::fetch#<ordinal>`.
pub fn unresolved(site: &str) -> String {
    format!("(unresolved) {}", callee(site))
}

/// The 1-based line holding byte `offset` of `src`.
pub fn line_of(src: &[u8], offset: i64) -> usize {
    let before = &src[..(offset.max(0) as usize).min(src.len())];
    before.iter().filter(|&&b| b == b'\n').count() + 1
}

/// 0 when `name` is `needle`, 1 when it starts with it, 2 when it contains it.
pub fn rank(name: &str, needle: &str) -> Option<u8> {
    match () {
        _ if name == needle => Some(0),
        _ if name.starts_with(needle) => Some(1),
        _ if name.contains(needle) => Some(2),
        _ => None,
    }
}

/// `name:` followed by the items, or `name: none`.
pub fn section(name: &str, items: Vec<String>) -> Vec<String> {
    if items.is_empty() {
        return vec![format!("{name}: none")];
    }
    let mut lines = vec![format!("{name}:")];
    lines.extend(items);
    lines
}
