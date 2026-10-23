//! Shape drift as `context`, `search` and `meada status` show it (spec #23, ADR 0009). It shows
//! a link's stored drift (`Link::drift`, from `shape::drift`) and computes none of its own; the
//! engine places each consumer at its call.

use crate::export::{Level, Link};
use crate::render::rank;
use crate::search::Hit;
use crate::shape::{Drift, shown};

/// A link of a consumer with a shape, placed: the consumer's repository (its position in the
/// workspace and its identity), file, line and enclosing symbol, and the drift if any.
pub struct Entry<'a> {
    pub repo: usize,
    pub repository: String,
    pub path: String,
    pub line: usize,
    pub caller: String,
    pub link: &'a Link,
    pub drift: Option<&'a Drift>,
}

impl Entry<'_> {
    /// `path:line caller`.
    fn place(&self) -> String {
        format!("{}:{} {}", self.path, self.line, self.caller)
    }
}

/// The consumers of the route `contract` that read fields of its response, sorted by
/// repository, file and line; each followed by its missing paths with their level and the paths
/// below an open part that were not checked, when it has any.
pub fn consumers(entries: &[Entry], contract: &str) -> Vec<String> {
    let mut mine: Vec<&Entry> = (entries.iter())
        .filter(|e| e.link.contract == contract)
        .collect();
    mine.sort_by_key(|e| (&e.repository, &e.path, e.line));
    let mut lines = Vec::new();
    for e in mine {
        lines.push(format!("  {} consumer {}", e.repository, e.place()));
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
/// `route` gives the part of a contract `search` ranks a route against: its path alone.
pub fn matches(entries: &[Entry], needle: &str, route: impl Fn(&str) -> &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    for e in entries {
        let route = route(&e.link.contract);
        for (path, level) in e.drift.iter().flat_map(|d| &d.missing) {
            let names = [route.to_lowercase(), path.to_lowercase()];
            let Some(r) = names.iter().filter_map(|n| rank(n, needle)).min() else {
                continue;
            };
            let (who, contract) = (&e.repository, &e.link.contract);
            let missing = format!("missing {} ({})", shown(path), level.name());
            let line = format!("{who} {} {contract} {missing}", e.place());
            hits.push((r, line.clone(), line));
        }
    }
    hits
}

/// The missing reads (link and path pairs) of the consumers of each of `repos` repositories, by
/// position.
pub fn counts(repos: usize, entries: &[Entry]) -> Vec<usize> {
    let mut counts = vec![0; repos];
    for e in entries {
        counts[e.repo] += e.drift.map_or(0, |d| d.missing.len());
    }
    counts
}

/// Whether a link counts as a consumer of the route with a shape: it has drift, or it is no
/// guess and its consumer read fields (`reads`). Ambiguous links are no consumer of the route
/// yet, so they count only with a drift.
pub fn shown_link(link: &Link, reads: impl Fn(&Link) -> bool) -> bool {
    link.drift.is_some() || (link.level != Level::Ambiguous && reads(link))
}
