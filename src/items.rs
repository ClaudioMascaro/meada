//! Open items: the ambiguous links and the unresolved consumers of a workspace, each with the
//! candidates a decision may pick (spec #18). `search` lists them and `meada decide` resolves
//! them.

use std::collections::HashMap;
use std::ops::Range;

use crate::contract::{Level, Link};
use crate::http;
use crate::index::Export;
use crate::render::{Files, callee};
use crate::workspace::Repository;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    /// The consumer links to several providers and picks none.
    Ambiguous,
    /// A dynamic URL: Meada could not evaluate the contract the consumer uses.
    Unresolved,
}

impl ItemKind {
    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Ambiguous => "ambiguous",
            ItemKind::Unresolved => "unresolved",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [ItemKind::Ambiguous, ItemKind::Unresolved]
            .into_iter()
            .find(|k| k.name() == name)
    }
}

/// A provider a decision may pick for an open item.
#[derive(Debug)]
pub struct Candidate {
    /// The identity of the repository that holds the provider.
    pub repository: String,
    /// The contract it serves, such as `http::GET::/api/health`.
    pub contract: String,
    /// The provider's site, such as `example.com/web::app/api/health/route.ts::GET`.
    pub site: String,
}

impl Candidate {
    /// The method of the route, empty for a contract that is not an HTTP route.
    pub fn method(&self) -> &str {
        http::route(&self.contract).map_or("", |(method, _)| method)
    }

    /// The path of the route, empty for a contract that is not an HTTP route.
    pub fn path(&self) -> &str {
        http::route(&self.contract).map_or("", |(_, path)| path)
    }
}

/// The symbol a call sits in.
pub struct Enclosing {
    pub name: String,
    /// Byte range in the call's file.
    pub span: Range<usize>,
}

/// A call of a repository that Meada could not link to one provider.
pub struct OpenItem {
    /// The call-site identity, as `meada export` writes it.
    pub id: String,
    pub kind: ItemKind,
    /// The identity of the repository that holds the call.
    pub repository: String,
    /// Repository-relative path of the call's file.
    pub path: String,
    /// 1-based line of the call, from the file as it is now; `None` when it cannot be read.
    pub line: Option<usize>,
    /// Byte range of the call in its file.
    pub call: Range<usize>,
    /// The call as written, on one line; the callee when the file cannot be read.
    pub expression: String,
    /// `None` for a call at the top level of its file.
    pub enclosing: Option<Enclosing>,
    /// Sorted by repository identity, then method, then path, then site.
    pub candidates: Vec<Candidate>,
}

impl OpenItem {
    /// `path:line`, or `path` when the line is unknown.
    pub fn location(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{line}", self.path),
            None => self.path.clone(),
        }
    }
}

/// The open items of every repository, in id order. The candidates of an ambiguous link are
/// the providers `links` lists for it; those of an unresolved consumer are the routes of the
/// workspace its method and literal URL fragments leave.
pub(crate) fn list(repos: &[Repository], exports: &[Export], links: &[Link]) -> Vec<OpenItem> {
    let mut ambiguous: HashMap<&str, Vec<&Link>> = HashMap::new();
    for link in links.iter().filter(|l| l.level == Level::Ambiguous) {
        ambiguous.entry(&link.consumer).or_default().push(link);
    }
    let routes: Vec<(&str, &str, &str)> = exports
        .iter()
        .flat_map(|e| {
            let repository = e.repository.as_str();
            e.providers
                .iter()
                .map(move |p| (repository, p.contract.as_str(), p.site.as_str()))
        })
        .collect();
    let mut items = Vec::new();
    for (export, repo) in exports.iter().zip(repos) {
        let calls: HashMap<&str, _> = export.calls.iter().map(|c| (c.id.as_str(), c)).collect();
        let symbols: HashMap<&str, _> = (export.symbols.iter())
            .map(|s| (s.id.as_str(), s))
            .collect();
        let mut files = Files::new(&repo.root);
        for consumer in &export.consumers {
            let (kind, candidates) =
                match (&consumer.contract, ambiguous.get(consumer.site.as_str())) {
                    (None, _) => {
                        let method = consumer.method.as_deref();
                        let fits = routes
                            .iter()
                            .filter(|(_, contract, _)| {
                                http::route_fits(contract, method, &consumer.fragments)
                            })
                            .map(|&(repository, contract, site)| (repository, contract, site));
                        (ItemKind::Unresolved, fits.collect::<Vec<_>>())
                    }
                    (Some(_), Some(links)) => {
                        let listed = links.iter().map(|l| {
                            let repository = exports
                                .iter()
                                .find(|e| l.provider.starts_with(&format!("{}::", e.repository)))
                                .map_or("", |e| e.repository.as_str());
                            (repository, l.contract.as_str(), l.provider.as_str())
                        });
                        (ItemKind::Ambiguous, listed.collect())
                    }
                    _ => continue,
                };
            let Some(call) = calls.get(consumer.site.as_str()) else {
                continue;
            };
            let range = |start: i64, end: i64| start as usize..end as usize;
            let expression = (files.get(&call.span.path))
                .and_then(|src| src.get(range(call.span.start, call.span.end)))
                .map(|text| {
                    String::from_utf8_lossy(text)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_else(|| callee(&consumer.site).to_string());
            let mut candidates: Vec<Candidate> = candidates
                .into_iter()
                .map(|(repository, contract, site)| Candidate {
                    repository: repository.into(),
                    contract: contract.into(),
                    site: site.into(),
                })
                .collect();
            candidates.sort_by(|a, b| {
                (&a.repository, a.method(), a.path(), &a.site).cmp(&(
                    &b.repository,
                    b.method(),
                    b.path(),
                    &b.site,
                ))
            });
            items.push(OpenItem {
                id: consumer.site.clone(),
                kind,
                repository: export.repository.clone(),
                line: files
                    .get(&call.span.path)
                    .map(|src| crate::render::line_of(src, call.span.start)),
                path: call.span.path.clone(),
                call: range(call.span.start, call.span.end),
                expression,
                enclosing: symbols.get(call.caller.as_str()).map(|s| Enclosing {
                    name: crate::render::label(&s.id).to_string(),
                    span: range(s.span.start, s.span.end),
                }),
                candidates,
            });
        }
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}
