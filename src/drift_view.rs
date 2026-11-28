pub(crate) use meada_core::drift::{Entry, consumers, counts};

use std::collections::HashSet;

use crate::contract::{self, Hit, Link};
use crate::index::Export;
use crate::render::{Files, label};
use crate::workspace::Repository;

fn shown<'a>(exports: &[Export], links: &'a [Link]) -> impl Iterator<Item = &'a Link> {
    let shaped: HashSet<&str> = (exports.iter().flat_map(|e| &e.consumers))
        .filter(|c| c.shape.is_some())
        .map(|c| c.site.as_str())
        .collect();
    let reads = move |link: &Link| shaped.contains(link.consumer.as_str());
    (links.iter()).filter(move |l| meada_core::drift::shown_link(l, &reads))
}

pub(crate) fn sites<'a>(exports: &[Export], links: &'a [Link]) -> Vec<&'a str> {
    shown(exports, links).map(|l| l.consumer.as_str()).collect()
}

pub(crate) fn entries<'a>(
    repos: &[Repository],
    exports: &[Export],
    links: &'a [Link],
) -> Vec<Entry<'a>> {
    let mut files: Vec<Files> = repos.iter().map(|r| Files::new(&r.root)).collect();
    shown(exports, links)
        .filter_map(|link| {
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

pub(crate) fn matches(entries: &[Entry], needle: &str) -> Vec<Hit> {
    meada_core::drift::matches(entries, needle, |c| contract::parts(c).1.unwrap_or(""))
}
