//! The query engine: opens a workspace from a directory and answers over its indexes.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::contract::{self, Link};
use crate::index::{Export, State, Symbol};
use crate::workspace::{Member, Workspace};
use crate::{Result, index, render};

/// SQLite attaches at most 10 databases to one connection (`SQLITE_MAX_ATTACHED`).
const MAX_PEERS: usize = 10;

pub struct Engine {
    /// The index of the repository where Meada runs, with every other repository of the
    /// workspace attached as `peer<n>`, `n` its position in `workspace.repos`.
    db: Connection,
    workspace: Workspace,
    root: PathBuf,
    identity: String,
}

impl Engine {
    /// Opens the workspace of the repository that contains `dir` and brings every index in it
    /// up to date, each inside its own repository's `.meada/`.
    pub fn open(dir: &Path) -> Result<Self> {
        let workspace = Workspace::discover(dir)?;
        if workspace.repos.len() > MAX_PEERS + 1 {
            // ponytail: one connection attaches every peer; batch the attaches when a
            // workspace outgrows SQLite's limit.
            return Err(format!(
                "the workspace has {} repositories; v0.1 reads at most {}",
                workspace.repos.len(),
                MAX_PEERS + 1
            )
            .into());
        }
        // ponytail: full rebuild on every open; incremental refresh by mtime and size comes
        // with the freshness ticket.
        for peer in &workspace.repos[1..] {
            index::rebuild(&mut index::open(&peer.root)?, &peer.root, &peer.identity)?;
        }
        let here = &workspace.repos[0];
        let mut db = index::open(&here.root)?;
        index::rebuild(&mut db, &here.root, &here.identity)?;
        for (n, peer) in workspace.repos.iter().enumerate().skip(1) {
            let path = index::path(&peer.root);
            let path = path.to_str().ok_or("index path is not UTF-8")?;
            db.execute(&format!("ATTACH DATABASE ?1 AS peer{n}"), [path])?;
        }
        Ok(Self {
            db,
            root: here.root.clone(),
            identity: here.identity.clone(),
            workspace,
        })
    }

    /// The canonical export of the repository where Meada runs: its whole index, plus the
    /// links from its consumers and to its providers, as sorted, pretty-printed JSON.
    pub fn export(&self) -> Result<String> {
        let mut export = index::export(&self.db, "main", &self.identity)?;
        export.links = self.links()?;
        Ok(serde_json::to_string_pretty(&export)? + "\n")
    }

    /// Every link that touches the repository where Meada runs, sorted.
    fn links(&self) -> Result<Vec<Link>> {
        let exports: Vec<Export> = self
            .workspace
            .repos
            .iter()
            .enumerate()
            .map(|(n, m)| index::export(&self.db, &schema(n), &m.identity))
            .collect::<Result<_>>()?;
        let members: Vec<&Member> = self.workspace.repos.iter().collect();
        let providers: Vec<_> = exports
            .iter()
            .zip(&members)
            .flat_map(|(e, m)| e.providers.iter().map(move |p| (*m, p)))
            .collect();
        let here = format!("{}::", self.identity);
        let mut links = Vec::new();
        for (n, (export, from)) in exports.iter().zip(&members).enumerate() {
            for consumer in &export.consumers {
                let Some(kind) = consumer.contract.as_deref().and_then(contract::kind_of) else {
                    continue;
                };
                let found = kind.link(consumer, from, &providers).into_iter();
                links.extend(found.filter(|l| n == 0 || l.provider.starts_with(&here)));
            }
        }
        links.sort_by(|a, b| (&a.consumer, &a.provider).cmp(&(&b.consumer, &b.provider)));
        Ok(links)
    }

    /// Symbols of this repository whose name matches `query` case-insensitively, and contracts
    /// of the workspace whose identifier does, one line each: exact first, then prefix, then
    /// substring, ties by identity. `kind` keeps only `symbol` or `contract` matches. The
    /// answer is cut at `budget` tokens.
    pub fn search(&self, query: &str, kind: Option<&str>, budget: Option<usize>) -> Result<String> {
        let export = index::export(&self.db, "main", &self.identity)?;
        let needle = query.to_lowercase();
        // (rank, identity to break ties, line)
        let mut hits: Vec<(u8, String, String)> = Vec::new();
        let mut files = Files::new(&self.root);
        if kind.is_none_or(|k| k == "symbol") {
            // ponytail: scans every symbol; an indexed lookup waits for a repository that
            // makes this slow.
            for symbol in &export.symbols {
                let name = label(&symbol.id).to_lowercase();
                let last = name.rsplit('.').next().unwrap_or(&name);
                if let Some(r) = [name.as_str(), last]
                    .iter()
                    .filter_map(|n| rank(n, &needle))
                    .min()
                {
                    let line = self.line(&mut files, symbol, label(&symbol.id));
                    hits.push((r, symbol.id.clone(), line));
                }
            }
        }
        if kind.is_none_or(|k| k == "contract") {
            for (n, member) in self.workspace.repos.iter().enumerate() {
                let peer;
                let export = match n {
                    0 => &export,
                    _ => {
                        peer = index::export(&self.db, &schema(n), &member.identity)?;
                        &peer
                    }
                };
                let mut files = Files::new(&member.root);
                let provided = export.providers.iter().map(|p| (&p.contract, &p.site));
                let consumed = export
                    .consumers
                    .iter()
                    .filter_map(|c| Some((c.contract.as_ref()?, &c.site)));
                for (contract, site) in provided.chain(consumed) {
                    let Some(r) = rank(&contract.to_lowercase(), &needle) else {
                        continue;
                    };
                    let span = export
                        .symbols
                        .iter()
                        .find(|s| s.id == *site)
                        .map(|s| &s.span);
                    let span = span
                        .or_else(|| export.calls.iter().find(|c| c.id == *site).map(|c| &c.span));
                    let location = match span {
                        Some(span) => self.at(&mut files, &span.path, span.start),
                        // A route file that serves the contract with no function of its own.
                        None => site
                            .rsplit_once("::")
                            .map_or(site.as_str(), |(_, p)| p)
                            .to_string(),
                    };
                    let line = format!("{} {location} {contract}", member.identity);
                    hits.push((r, format!("{contract} {site}"), line));
                }
            }
        }
        hits.sort();
        if hits.is_empty() {
            return Ok(format!("no matches for `{query}`\n"));
        }
        let lines: Vec<String> = hits.into_iter().map(|(_, _, line)| line).collect();
        Ok(render::budgeted(&lines, budget))
    }

    /// One symbol: its span text read from disk (only while the file still has the indexed
    /// hash), its callers and its callees. `symbol` is an identity, `<path>::<name>` or a
    /// name; when it fits several symbols, they are listed instead of guessed. The answer is
    /// cut at `budget` tokens.
    pub fn context(&self, symbol: &str, budget: Option<usize>) -> Result<String> {
        let export = index::export(&self.db, "main", &self.identity)?;
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

/// The workspace of the repository containing `dir`: each repository with where it is,
/// relative to the one Meada runs in, and its index state, then every peer not found. It
/// reads only; nothing is indexed.
pub fn status(dir: &Path) -> Result<String> {
    let workspace = Workspace::discover(dir)?;
    let here = &workspace.repos[0].root;
    let mut lines: Vec<String> = workspace
        .repos
        .iter()
        .map(|m| {
            let state = match index::state(&m.root) {
                State::Missing => "not indexed".to_string(),
                State::OtherSchema => "indexed by another version; rebuilt on next use".into(),
                State::Indexed { files } => format!("indexed, {files} files"),
            };
            format!("{} ({}) {state}", m.identity, relative(here, &m.root))
        })
        .collect();
    lines.extend(
        workspace
            .missing
            .iter()
            .map(|m| format!("missing peer: {}, named by {}", m.repo, m.named_by)),
    );
    Ok(lines.iter().map(|l| format!("{l}\n")).collect())
}

/// 0 when `name` is `needle`, 1 when it starts with it, 2 when it contains it.
fn rank(name: &str, needle: &str) -> Option<u8> {
    match () {
        _ if name == needle => Some(0),
        _ if name.starts_with(needle) => Some(1),
        _ if name.contains(needle) => Some(2),
        _ => None,
    }
}

/// The schema a workspace repository's index is attached as.
fn schema(n: usize) -> String {
    if n == 0 {
        "main".into()
    } else {
        format!("peer{n}")
    }
}

/// `to` as a path from `from`, with `/` separators: `.`, `../api`.
fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
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
