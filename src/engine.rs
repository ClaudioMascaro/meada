//! The query engine: opens a repository from a directory and answers over its index.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::index::{Export, Symbol};
use crate::{Result, index, render, repo};

pub struct Engine {
    db: Connection,
    root: PathBuf,
    identity: String,
}

impl Engine {
    /// Opens the repository that contains `dir` and brings its index up to date.
    pub fn open(dir: &Path) -> Result<Self> {
        let root = repo::root(dir)?;
        let identity = repo::identity(&root);
        let mut db = index::open(&root)?;
        // ponytail: full rebuild on every open; incremental refresh by mtime and size comes
        // with the freshness ticket.
        index::rebuild(&mut db, &root, &identity)?;
        Ok(Self { db, root, identity })
    }

    /// The canonical export: the whole index as sorted, pretty-printed JSON.
    pub fn export(&self) -> Result<String> {
        let export = index::export(&self.db, &self.identity)?;
        Ok(serde_json::to_string_pretty(&export)? + "\n")
    }

    /// Symbols whose name matches `query` case-insensitively, one line each: exact name
    /// first, then prefix, then substring, ties by identity. `kind` keeps only that kind of
    /// match (`symbol` is the only kind so far). The answer is cut at `budget` tokens.
    pub fn search(&self, query: &str, kind: Option<&str>, budget: Option<usize>) -> Result<String> {
        let export = index::export(&self.db, &self.identity)?;
        let needle = query.to_lowercase();
        let mut hits: Vec<(u8, &Symbol)> = Vec::new();
        if kind.is_none_or(|k| k == "symbol") {
            // ponytail: scans every symbol; an indexed lookup waits for a repository that
            // makes this slow.
            for symbol in &export.symbols {
                let name = label(&symbol.id).to_lowercase();
                let last = name.rsplit('.').next().unwrap_or(&name);
                let rank = [name.as_str(), last]
                    .iter()
                    .filter_map(|n| match () {
                        _ if *n == needle => Some(0),
                        _ if n.starts_with(&needle) => Some(1),
                        _ if n.contains(&needle) => Some(2),
                        _ => None,
                    })
                    .min();
                hits.extend(rank.map(|r| (r, symbol)));
            }
        }
        hits.sort_by(|a, b| (a.0, &a.1.id).cmp(&(b.0, &b.1.id)));
        if hits.is_empty() {
            return Ok(format!("no matches for `{query}`\n"));
        }
        let mut files = Files::new(&self.root);
        let lines: Vec<String> = hits
            .iter()
            .map(|(_, s)| self.line(&mut files, s, label(&s.id)))
            .collect();
        Ok(render::budgeted(&lines, budget))
    }

    /// One symbol: its span text read from disk (only while the file still has the indexed
    /// hash), its callers and its callees. `symbol` is an identity, `<path>::<name>` or a
    /// name; when it fits several symbols, they are listed instead of guessed. The answer is
    /// cut at `budget` tokens.
    pub fn context(&self, symbol: &str, budget: Option<usize>) -> Result<String> {
        let export = index::export(&self.db, &self.identity)?;
        let suffix = format!("::{symbol}");
        let found: Vec<&Symbol> = export
            .symbols
            .iter()
            .filter(|s| s.id == symbol || s.id.ends_with(&suffix))
            .collect();
        let mut files = Files::new(&self.root);
        let [symbol_found] = found.as_slice() else {
            if found.is_empty() {
                return Ok(format!("no symbol matches `{symbol}`\n"));
            }
            let mut lines = vec![format!(
                "`{symbol}` fits {} symbols; pass one of:",
                found.len()
            )];
            lines.extend(found.iter().map(|s| {
                let name = format!("{}::{}", s.span.path, label(&s.id));
                self.line(&mut files, s, &name)
            }));
            return Ok(render::budgeted(&lines, budget));
        };
        let mut lines = vec![self.line(&mut files, symbol_found, label(&symbol_found.id))];
        lines.extend(self.span_text(&export, symbol_found));
        lines.extend(self.relations(&export, &mut files, &symbol_found.id));
        Ok(render::budgeted(&lines, budget))
    }

    fn span_text(&self, export: &Export, symbol: &Symbol) -> Vec<String> {
        let path = &symbol.span.path;
        let indexed = export
            .files
            .iter()
            .find(|f| f.path == *path)
            .map(|f| &f.hash);
        let text = fs::read(self.root.join(path)).ok().filter(|src| {
            indexed.is_some_and(|h| blake3::hash(src).to_hex().as_str() == h.as_str())
        });
        match text.and_then(|src| {
            let span = src.get(symbol.span.start as usize..symbol.span.end as usize)?;
            Some(String::from_utf8_lossy(span).into_owned())
        }) {
            Some(text) => text.lines().map(str::to_string).collect(),
            None => vec![format!(
                "(span text unavailable: {path} changed since indexing)"
            )],
        }
    }

    fn relations(&self, export: &Export, files: &mut Files, id: &str) -> Vec<String> {
        let symbols: HashMap<&str, &Symbol> =
            export.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
        let mut callers: Vec<_> = export
            .calls
            .iter()
            .filter(|c| c.target.as_deref() == Some(id))
            .collect();
        callers.sort_by_key(|c| (&c.span.path, c.span.start));
        let callers: Vec<String> = callers
            .iter()
            .map(|c| {
                let name = symbols
                    .contains_key(c.caller.as_str())
                    .then(|| label(&c.caller));
                let location = self.at(files, &c.span.path, c.span.start);
                format!(
                    "{} {location} {}",
                    self.identity,
                    name.unwrap_or("(top level)")
                )
            })
            .collect();
        let mut callees: Vec<_> = export.calls.iter().filter(|c| c.caller == id).collect();
        callees.sort_by_key(|c| (c.target.is_none(), &c.id));
        let mut callees: Vec<String> = callees
            .iter()
            .map(|c| match c.target.as_deref().and_then(|t| symbols.get(t)) {
                Some(target) => self.line(files, target, label(&target.id)),
                None => {
                    let callee =
                        c.id.strip_prefix(&format!("{}::", c.caller))
                            .unwrap_or(&c.id);
                    let callee = callee.rsplit_once('#').map_or(callee, |(name, _)| name);
                    let location = self.at(files, &c.span.path, c.span.start);
                    format!("{} {location} (unresolved) {callee}", self.identity)
                }
            })
            .collect();
        callees.dedup();
        let mut lines = section("callers", callers);
        lines.extend(section("callees", callees));
        lines
    }

    /// `<repository> <path>:<line> <name>` for a symbol's definition.
    fn line(&self, files: &mut Files, symbol: &Symbol, name: &str) -> String {
        let location = self.at(files, &symbol.span.path, symbol.span.start);
        format!("{} {location} {name}", self.identity)
    }

    /// `path:line`; just `path` when the file cannot be read. The line is display only: it
    /// comes from the byte offset and the file as it is now.
    fn at(&self, files: &mut Files, path: &str, offset: i64) -> String {
        match files.get(path) {
            Some(src) => {
                let before = &src[..(offset as usize).min(src.len())];
                let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
                format!("{path}:{line}")
            }
            None => path.to_string(),
        }
    }
}

/// `name:` followed by the items, or `name: none`.
fn section(name: &str, items: Vec<String>) -> Vec<String> {
    if items.is_empty() {
        return vec![format!("{name}: none")];
    }
    let mut lines = vec![format!("{name}:")];
    lines.extend(items);
    lines
}

/// The name part of a symbol identity: `<repository>::<path>::<name>`.
fn label(id: &str) -> &str {
    id.rsplit_once("::").map_or(id, |(_, name)| name)
}

/// Source files read once per answer.
struct Files<'a> {
    root: &'a Path,
    read: HashMap<String, Option<Vec<u8>>>,
}

impl<'a> Files<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            read: HashMap::new(),
        }
    }

    fn get(&mut self, path: &str) -> Option<&[u8]> {
        self.read
            .entry(path.to_string())
            .or_insert_with(|| fs::read(self.root.join(path)).ok())
            .as_deref()
    }
}
