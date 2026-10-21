//! Shape drift as `context`, `search` and `meada status` show it (spec #23). It shows the stored
//! `Link::drift` (set by a kind's pass after links) and computes no drift of its own; to place each
//! consumer it reads the exports' consumer shapes and finds the consumer's call.

use crate::contract::{self, Hit, Level, Link};
use crate::index::Export;
use crate::render::{Files, label, rank};
use crate::shape::{Drift, shown};
use crate::workspace::Repository;

/// A link of a consumer with a shape: the consumer's repository and place, and the drift if any.
pub(crate) struct Entry<'a> {
    repo: usize,
    /// The consumer's file, line and enclosing symbol, in the order `consumers` sorts by.
    path: String,
    line: usize,
    caller: String,
    link: &'a Link,
    drift: Option<&'a Drift>,
}

impl Entry<'_> {
    /// `path:line caller`.
    fn place(&self) -> String {
        format!("{}:{} {}", self.path, self.line, self.caller)
    }
}

/// Every link of a consumer that read fields (a shape), or has drift, with where it calls.
/// Ambiguous links are no consumer of the route yet, so they count only with a drift.
pub(crate) fn entries<'a>(
    repos: &[Repository],
    exports: &[Export],
    links: &'a [Link],
) -> Vec<Entry<'a>> {
    let mut files: Vec<Files> = repos.iter().map(|r| Files::new(&r.root)).collect();
    let reads = |link: &Link| {
        let mut consumers = exports.iter().flat_map(|e| &e.consumers);
        consumers.any(|c| c.site == link.consumer && c.shape.is_some())
    };
    links
        .iter()
        .filter(|l| l.drift.is_some() || (l.level != Level::Ambiguous && reads(l)))
        .filter_map(|link| {
            // Scans the calls of every repository per link (Known ceilings).
            let (repo, export, call) = exports
                .iter()
                .enumerate()
                .find_map(|(n, e)| Some((n, e, e.calls.iter().find(|c| c.id == link.consumer)?)))?;
            let at = files[repo].at(&call.span.path, call.span.start);
            let (path, line) = at
                .rsplit_once(':')
                .and_then(|(p, l)| Some((p.to_string(), l.parse().ok()?)))
                .unwrap_or((at, 0));
            let caller = export.symbols.iter().find(|s| s.id == call.caller);
            let caller = caller.map_or("(top level)", |s| label(&s.id)).to_string();
            let drift = link.drift.as_ref();
            Some(Entry {
                repo,
                path,
                line,
                caller,
                link,
                drift,
            })
        })
        .collect()
}

/// The consumers of the route `contract` that read fields of its response, sorted by
/// repository, file and line; each followed by its missing paths with their level and the paths
/// below an open part that were not checked, when it has any.
pub(crate) fn consumers(repos: &[Repository], entries: &[Entry], contract: &str) -> Vec<String> {
    let mut mine: Vec<&Entry> = (entries.iter())
        .filter(|e| e.link.contract == contract)
        .collect();
    mine.sort_by_key(|e| (&repos[e.repo].identity, &e.path, e.line));
    let mut lines = Vec::new();
    for e in mine {
        lines.push(format!(
            "  {} consumer {}",
            repos[e.repo].identity,
            e.place()
        ));
        let Some(drift) = e.drift else { continue };
        if !drift.missing.is_empty() {
            let paths: Vec<String> = (drift.missing.iter())
                .map(|(path, level)| format!("{} ({})", shown(path), level.name()))
                .collect();
            lines.push(format!("    drift: missing {}", paths.join(", ")));
        }
        if !drift.unchecked.is_empty() {
            let paths: Vec<String> = (drift.unchecked.iter())
                .map(|(path, open)| format!("{} under {}", shown(path), shown(open)))
                .collect();
            lines.push(format!("    not checked: {}", paths.join(", ")));
        }
    }
    lines
}

/// One hit per missing path whose route path or field path matches `needle` (lowercase), as
/// `(rank, tie-break, <repository> <path>:<line> <caller> <route> missing <path> (<level>))`.
pub(crate) fn matches(repos: &[Repository], entries: &[Entry], needle: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    for e in entries {
        // The part `search` ranks a route against: its path alone, pinned by the HTTP test
        // `a_route_ranks_against_its_path_alone`.
        let route = contract::parts(&e.link.contract).1.unwrap_or("");
        for (path, level) in e.drift.iter().flat_map(|d| &d.missing) {
            let names = [route.to_lowercase(), path.to_lowercase()];
            let Some(r) = names.iter().filter_map(|n| rank(n, needle)).min() else {
                continue;
            };
            let (who, contract) = (&repos[e.repo].identity, &e.link.contract);
            let missing = format!("missing {} ({})", shown(path), level.name());
            let line = format!("{who} {} {contract} {missing}", e.place());
            hits.push((r, line.clone(), line));
        }
    }
    hits
}

/// The missing reads (link and path pairs) of the consumers of each repository, indexed like `repos`.
pub(crate) fn counts(repos: &[Repository], entries: &[Entry]) -> Vec<usize> {
    let mut counts = vec![0; repos.len()];
    for e in entries {
        counts[e.repo] += e.drift.map_or(0, |d| d.missing.len());
    }
    counts
}
