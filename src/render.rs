//! Compact answers: line-oriented text under a token budget (ADR 0008). Every query shares
//! this, so the CLI and each MCP tool cut answers the same way.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Tokens an answer may use unless the caller asks for another budget.
pub const DEFAULT_BUDGET: usize = 2_000;

/// `lines` joined by newlines. When they exceed `budget` tokens (`DEFAULT_BUDGET` when
/// `None`; one token is four bytes of UTF-8, so the estimate is deterministic), the answer
/// keeps the lines that fit and ends with a line saying where it stopped.
pub fn budgeted(lines: &[String], budget: Option<usize>) -> String {
    let budget = budget.unwrap_or(DEFAULT_BUDGET);
    cut(lines, budget, budget).1
}

/// The estimated tokens of `text`: one per four bytes, rounded up.
pub fn tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// `lines` within `room` tokens, cut as `budgeted` cuts them, with the cut note naming
/// `budget`, and how many lines it kept. `room` is below `budget` when the caller appends
/// lines of its own.
pub fn cut(lines: &[String], room: usize, budget: usize) -> (usize, String) {
    let cap = room * 4;
    let all: usize = lines.iter().map(|l| l.len() + 1).sum();
    let note = |shown: usize| {
        format!(
            "… cut after {shown} of {} lines (budget {budget} tokens); raise budget for more\n",
            lines.len()
        )
    };
    let mut shown = lines.len();
    if all > cap {
        // The note's length depends on `shown`'s digits: reserve for the widest.
        let room = cap.saturating_sub(note(lines.len()).len());
        let mut used = 0;
        shown = lines
            .iter()
            .take_while(|l| {
                used += l.len() + 1;
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
pub(crate) const REPOSITORY_LEVEL: &str = "(repository-level)";

/// The name part of a symbol identity: `<repository>::<path>::<name>`.
pub(crate) fn label(id: &str) -> &str {
    id.rsplit_once("::").map_or(id, |(_, name)| name)
}

/// The callee of the call site `<caller>::<callee>#<ordinal>`.
pub(crate) fn callee(site: &str) -> &str {
    let callee = label(site);
    callee.rsplit_once('#').map_or(callee, |(name, _)| name)
}

/// How an unresolved consumer shows: `(unresolved) fetch` for the call site
/// `<caller>::fetch#<ordinal>`.
pub(crate) fn unresolved(site: &str) -> String {
    format!("(unresolved) {}", callee(site))
}

/// Source files read once per answer.
pub(crate) struct Files<'a> {
    root: &'a Path,
    read: HashMap<String, Option<Vec<u8>>>,
}

impl<'a> Files<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self {
            root,
            read: HashMap::new(),
        }
    }

    pub(crate) fn get(&mut self, path: &str) -> Option<&[u8]> {
        self.read
            .entry(path.to_string())
            .or_insert_with(|| fs::read(self.root.join(path)).ok())
            .as_deref()
    }

    /// `path:line`; just `path` when the file cannot be read. The line is display only: it
    /// comes from the byte offset and the file as it is now.
    pub(crate) fn at(&mut self, path: &str, offset: i64) -> String {
        match self.get(path) {
            Some(src) => format!("{path}:{}", line_of(src, offset)),
            None => path.to_string(),
        }
    }
}

/// The 1-based line holding byte `offset` of `src`.
pub(crate) fn line_of(src: &[u8], offset: i64) -> usize {
    let before = &src[..(offset.max(0) as usize).min(src.len())];
    before.iter().filter(|&&b| b == b'\n').count() + 1
}
