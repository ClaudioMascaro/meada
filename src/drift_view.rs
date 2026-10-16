//! Shape drift as `context`, `search` and `meada status` show it (spec #23). It reads the one
//! value `http::drift` sets on `Link::drift` and computes nothing of its own.

use crate::contract::Link;
use crate::engine::rank;
use crate::http;
use crate::index::Export;
use crate::render::{Files, label};
use crate::shape::{Drift, shown};
use crate::workspace::Repository;

/// A link with drift: the consumer's repository and place, as `path:line caller`.
pub(crate) struct Entry<'a> {
    repo: usize,
    place: String,
    link: &'a Link,
    drift: &'a Drift,
}

/// Every link with drift, with where its consumer calls.
pub(crate) fn entries<'a>(
    repos: &[Repository],
    exports: &[Export],
    links: &'a [Link],
) -> Vec<Entry<'a>> {
    let mut files: Vec<Files> = repos.iter().map(|r| Files::new(&r.root)).collect();
    let drifting = links.iter().filter_map(|l| Some((l, l.drift.as_ref()?)));
    drifting
        .filter_map(|(link, drift)| {
            // ponytail: scans the calls of every repository per drifting link; an index by call
            // id waits for a workspace that makes this slow.
            let (repo, export, call) = exports
                .iter()
                .enumerate()
                .find_map(|(n, e)| Some((n, e, e.calls.iter().find(|c| c.id == link.consumer)?)))?;
            let at = files[repo].at(&call.span.path, call.span.start);
            let caller = export.symbols.iter().find(|s| s.id == call.caller);
            let caller = caller.map_or("(top level)", |s| label(&s.id));
            let place = format!("{at} {caller}");
            Some(Entry {
                repo,
                place,
                link,
                drift,
            })
        })
        .collect()
}

/// The drifting consumers of the route `contract`, each followed by its missing paths with their
/// level and the paths below an open part that were not checked, sorted by place.
pub(crate) fn consumers(repos: &[Repository], entries: &[Entry], contract: &str) -> Vec<String> {
    let mut mine: Vec<&Entry> = (entries.iter())
        .filter(|e| e.link.contract == contract)
        .collect();
    mine.sort_by_key(|e| (&repos[e.repo].identity, &e.place));
    let mut lines = Vec::new();
    for e in mine {
        lines.push(format!("  {} consumer {}", repos[e.repo].identity, e.place));
        if !e.drift.missing.is_empty() {
            let paths: Vec<String> = (e.drift.missing.iter())
                .map(|(path, level)| format!("{} ({})", shown(path), level.name()))
                .collect();
            lines.push(format!("    drift: missing {}", paths.join(", ")));
        }
        if !e.drift.unchecked.is_empty() {
            let paths: Vec<String> = (e.drift.unchecked.iter())
                .map(|(path, open)| format!("{} under {}", shown(path), shown(open)))
                .collect();
            lines.push(format!("    not checked: {}", paths.join(", ")));
        }
    }
    lines
}

/// One hit per missing path whose route path or field path matches `needle` (lowercase), as
/// `(rank, tie-break, <repository> <path>:<line> <caller> <route> missing <path> (<level>))`.
pub(crate) fn matches(
    repos: &[Repository],
    entries: &[Entry],
    needle: &str,
) -> Vec<(u8, String, String)> {
    let mut hits = Vec::new();
    for e in entries {
        let route = http::route(&e.link.contract).map_or("", |(_, path)| path);
        for (path, level) in &e.drift.missing {
            let names = [route.to_lowercase(), path.to_lowercase()];
            let Some(r) = names.iter().filter_map(|n| rank(n, needle)).min() else {
                continue;
            };
            let (who, contract) = (&repos[e.repo].identity, &e.link.contract);
            let missing = format!("missing {} ({})", shown(path), level.name());
            let line = format!("{who} {} {contract} {missing}", e.place);
            hits.push((r, line.clone(), line));
        }
    }
    hits
}

/// The missing paths the consumers of each repository read, indexed like `repos`.
pub(crate) fn counts(repos: &[Repository], entries: &[Entry]) -> Vec<usize> {
    let mut counts = vec![0; repos.len()];
    for e in entries {
        counts[e.repo] += e.drift.missing.len();
    }
    counts
}
