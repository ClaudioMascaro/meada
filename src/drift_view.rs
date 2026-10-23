//! Shape drift placed for `context`, `search` and `meada status` (spec #23): each consumer at its
//! call, read from the files as they are. `meada_core::drift` turns the entries into lines.

pub(crate) use meada_core::drift::{Entry, consumers, counts};

use crate::contract::{self, Hit, Link};
use crate::index::Export;
use crate::render::{Files, label};
use crate::workspace::Repository;

/// Every link of a consumer that read fields (a shape), or has drift, with where it calls.
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
        .filter(|l| meada_core::drift::shown_link(l, reads))
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
            Some(Entry {
                repo,
                repository: repos[repo].identity.clone(),
                path,
                line,
                caller,
                link,
                drift: link.drift.as_ref(),
            })
        })
        .collect()
}

/// One hit per missing path whose route path or field path matches `needle` (lowercase). The
/// part `search` ranks a route against is its path alone, pinned by the HTTP test
/// `a_route_ranks_against_its_path_alone`.
pub(crate) fn matches(entries: &[Entry], needle: &str) -> Vec<Hit> {
    meada_core::drift::matches(entries, needle, |c| contract::parts(c).1.unwrap_or(""))
}
