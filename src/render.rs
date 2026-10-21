//! Compact answers: line-oriented text under a token budget (ADR 0008). Every query shares
//! this, so the CLI and each MCP tool cut answers the same way.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path};

use crate::contract::Level;
use crate::index::CallSite;

/// ` [<level>: <reason>]` after a call that is not exact. Only a name match makes a call
/// inferred or ambiguous, so the reason is how many definitions have its name: methods for a
/// member call, top-level functions and classes for a plain one.
pub(crate) fn mark(call: &CallSite) -> String {
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
pub(crate) fn note(shown: usize, total: usize, budget: usize) -> String {
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
pub(crate) const REPOSITORY_LEVEL: &str = "(repository-level)";

/// How a side only a provider document lists shows (spec #21): no code of it is indexed.
pub(crate) const DOCUMENTED_ONLY: &str = "(documented only)";

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
            .or_insert_with(|| source(self.root, path))
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

// ponytail: a source file an answer reads past this renders as unavailable; indexing reads a
// source file whole, and an answer keeps each file it reads, so several large ones add up.
pub(crate) const SOURCE_MAX: u64 = 16 << 20;

/// The source file at `path` under `root`, at most `SOURCE_MAX` bytes, or `None` when it cannot
/// be read so. `path` comes from an index row, hostile in a pulled generation (ADR 0012): only
/// plain parts are joined (no root, no `..`), no part is a symbolic link and the file is a
/// regular one, so an absolute path, a FIFO or `/dev/zero` is refused before it is opened. The
/// walk follows no symbolic link, so no indexed path goes through one.
pub(crate) fn source(root: &Path, path: &str) -> Option<Vec<u8>> {
    let mut abs = root.to_path_buf();
    for part in Path::new(path).components() {
        let Component::Normal(part) = part else {
            return None;
        };
        abs.push(part);
        if fs::symlink_metadata(&abs).ok()?.file_type().is_symlink() {
            return None;
        }
    }
    if !fs::symlink_metadata(&abs).ok()?.is_file() {
        return None;
    }
    let mut src = Vec::new();
    (fs::File::open(&abs).ok()?)
        .take(SOURCE_MAX + 1)
        .read_to_end(&mut src)
        .ok()?;
    (src.len() as u64 <= SOURCE_MAX).then_some(src)
}

/// The 1-based line holding byte `offset` of `src`.
pub(crate) fn line_of(src: &[u8], offset: i64) -> usize {
    let before = &src[..(offset.max(0) as usize).min(src.len())];
    before.iter().filter(|&&b| b == b'\n').count() + 1
}
/// 0 when `name` is `needle`, 1 when it starts with it, 2 when it contains it.
pub(crate) fn rank(name: &str, needle: &str) -> Option<u8> {
    match () {
        _ if name == needle => Some(0),
        _ if name.starts_with(needle) => Some(1),
        _ if name.contains(needle) => Some(2),
        _ => None,
    }
}

/// `name:` followed by the items, or `name: none`.
pub(crate) fn section(name: &str, items: Vec<String>) -> Vec<String> {
    if items.is_empty() {
        return vec![format!("{name}: none")];
    }
    let mut lines = vec![format!("{name}:")];
    lines.extend(items);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(case: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("meada-render-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("root/src")).unwrap();
        fs::write(dir.join("root/src/a.ts"), "one\ntwo\n").unwrap();
        fs::write(dir.join("outside.ts"), "secret\nlines\n").unwrap();
        dir
    }

    #[test]
    fn a_source_inside_the_root_is_read() {
        let dir = scratch("inside");
        let root = dir.join("root");
        assert_eq!(
            source(&root, "src/a.ts").as_deref(),
            Some(&b"one\ntwo\n"[..])
        );
        assert_eq!(Files::new(&root).at("src/a.ts", 4), "src/a.ts:2");
    }

    #[test]
    fn an_absolute_or_escaping_source_path_is_unavailable() {
        let dir = scratch("escape");
        let root = dir.join("root");
        let outside = dir.join("outside.ts").display().to_string();
        for path in [
            outside.as_str(),
            "../outside.ts",
            "src/../../outside.ts",
            "/dev/zero",
            "",
        ] {
            assert_eq!(source(&root, path), None, "{path}");
            assert_eq!(Files::new(&root).at(path, 8), path, "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_source_through_a_symbolic_link_is_unavailable() {
        let dir = scratch("link");
        let root = dir.join("root");
        std::os::unix::fs::symlink(dir.join("outside.ts"), root.join("src/l.ts")).unwrap();
        std::os::unix::fs::symlink(&dir, root.join("up")).unwrap();
        assert_eq!(source(&root, "src/l.ts"), None);
        assert_eq!(source(&root, "up/outside.ts"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_source_is_unavailable_without_blocking() {
        let dir = scratch("fifo");
        let root = dir.join("root");
        let made = std::process::Command::new("mkfifo")
            .arg(root.join("src/f.ts"))
            .status()
            .unwrap();
        assert!(made.success());
        assert_eq!(source(&root, "src/f.ts"), None);
    }

    #[test]
    fn a_source_past_the_ceiling_is_unavailable() {
        let dir = scratch("large");
        let root = dir.join("root");
        let file = fs::File::create(root.join("src/big.ts")).unwrap();
        file.set_len(SOURCE_MAX + 1).unwrap();
        assert_eq!(source(&root, "src/big.ts"), None);
        fs::File::options()
            .write(true)
            .open(root.join("src/big.ts"))
            .unwrap()
            .set_len(SOURCE_MAX)
            .unwrap();
        assert_eq!(
            source(&root, "src/big.ts").map(|s| s.len() as u64),
            Some(SOURCE_MAX)
        );
    }
}
