//! Open items: the ambiguous links and the unresolved consumers of a workspace, each with the
//! candidates a decision may pick (spec #18). `search` lists them and `meada decide` resolves
//! them.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::contract::{self, Level, Link, kind_of};
use crate::index::{Consumer, Export, Provider};
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
    /// The contract as its kind labels it: method and path for a route.
    pub fn label(&self) -> String {
        contract::label(&self.contract)
    }

    /// The candidate as a person or a decider sees it: repository identity and label.
    pub fn describe(&self) -> String {
        format!("{} {}", self.repository, self.label())
    }
}

/// The symbol a call sits in; for a consumer at a definition, the definition itself.
pub struct Enclosing {
    pub name: String,
    /// Byte range in the call's file.
    pub span: Range<usize>,
}

/// A call of a repository that Meada could not link to one provider, or a consumer at a
/// definition (a method of a declarative client), which stands for its call.
pub struct OpenItem {
    /// The call-site identity, as `meada export` writes it (the definition's for a consumer at
    /// one).
    pub id: String,
    pub kind: ItemKind,
    /// The identity of the repository that holds the call.
    pub repository: String,
    /// Repository-relative path of the call's file.
    pub path: String,
    /// 1-based line of the call, from the file as it is now; `None` when it cannot be read.
    pub line: Option<usize>,
    /// The call (or the definition) as written, on one line; the callee when the file cannot be
    /// read.
    pub expression: String,
    /// `None` for a call at the top level of its file.
    pub enclosing: Option<Enclosing>,
    /// Sorted by repository identity, then label (method and path of a route), then site.
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

/// Whether `link` is an ambiguous link a person can decide, as its kind says.
fn decidable(link: &Link) -> bool {
    link.level == Level::Ambiguous && kind_of(&link.contract).is_none_or(|k| k.decidable())
}

/// The sites `list` places, each with the position of its repository: the unresolved
/// consumers and those an ambiguous link of `links` leaves from (a call, or a definition).
pub(crate) fn sites(exports: &[Export], links: &[Link]) -> Vec<(usize, String)> {
    let ambiguous: HashSet<&str> = (links.iter())
        .filter(|l| decidable(l))
        .map(|l| l.consumer.as_str())
        .collect();
    let items = exports.iter().enumerate().flat_map(|(n, e)| {
        let open = |c: &&Consumer| c.contract.is_none() || ambiguous.contains(c.site.as_str());
        (e.consumers.iter().filter(open)).map(move |c| (n, c.site.clone()))
    });
    items.collect()
}

/// The items of every repository as the core sees them, in id order: `links` are the links
/// before any decision, so a decided call is listed too (the caller drops it to keep the open
/// ones). The candidates of an ambiguous link are the providers `links` lists for it; those of
/// an unresolved consumer are the providers of the workspace that its kind's `candidate` leaves
/// (HTTP: by method and literal URL fragments).
pub(crate) fn list(repos: &[Repository], exports: &[Export], links: &[Link]) -> Vec<OpenItem> {
    let mut ambiguous: HashMap<&str, Vec<&Link>> = HashMap::new();
    for link in links.iter().filter(|l| decidable(l)) {
        ambiguous.entry(&link.consumer).or_default().push(link);
    }
    let providers: Vec<(&str, &Provider)> = exports
        .iter()
        .flat_map(|e| e.providers.iter().map(|p| (e.repository.as_str(), p)))
        .collect();
    // A candidate carries the provider's own contract, also when it fits by its gateway path,
    // so a decision for it names a contract its repository serves.
    fn candidate<'a>(&(repository, p): &(&'a str, &'a Provider)) -> (&'a str, &'a str, &'a str) {
        (repository, p.contract.as_str(), p.site.as_str())
    }
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
                        let fitting = providers.iter().filter(|(_, p)| {
                            kind_of(&p.contract).is_some_and(|k| k.candidate(consumer, p))
                        });
                        (
                            ItemKind::Unresolved,
                            fitting.map(candidate).collect::<Vec<_>>(),
                        )
                    }
                    (Some(_), Some(links)) => {
                        let listed = links.iter().filter_map(|l| {
                            (providers.iter())
                                .find(|(_, p)| {
                                    p.site == l.provider && contract::serves(p, &l.contract)
                                })
                                .map(candidate)
                        });
                        (ItemKind::Ambiguous, listed.collect())
                    }
                    _ => continue,
                };
            // A consumer at a definition (a method of a declarative client, which has no call
            // of its own) is the definition: its lines are the excerpt (ADR 0006).
            let (span, caller) = match calls.get(consumer.site.as_str()) {
                Some(call) => (&call.span, symbols.get(call.caller.as_str())),
                None => match symbols.get(consumer.site.as_str()) {
                    Some(def) => (&def.span, Some(def)),
                    None => continue,
                },
            };
            let range = |start: i64, end: i64| start as usize..end as usize;
            let expression = (files.get(&span.path))
                .and_then(|src| src.get(range(span.start, span.end)))
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
            // The label of a route orders as its method then path: the space sorts before
            // every method letter and before the slash.
            candidates.sort_by_cached_key(|c| (c.repository.clone(), c.label(), c.site.clone()));
            items.push(OpenItem {
                id: consumer.site.clone(),
                kind,
                repository: export.repository.clone(),
                line: files
                    .get(&span.path)
                    .map(|src| crate::render::line_of(src, span.start)),
                path: span.path.clone(),
                expression,
                enclosing: caller.map(|s| Enclosing {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ambiguous(contract: &str) -> Link {
        Link {
            consumer: "r::src/a.ts::f::g#0".into(),
            provider: "r::src/b.ts::h".into(),
            contract: contract.into(),
            level: Level::Ambiguous,
            reason: String::new(),
            by: None,
            p: None,
            drift: None,
        }
    }

    #[test]
    fn only_an_ambiguous_link_of_a_decidable_kind_is_an_item() {
        assert!(decidable(&ambiguous("http::GET::/users")));
        let exact = Link {
            level: Level::Exact,
            ..ambiguous("http::GET::/users")
        };
        assert!(!decidable(&exact));
    }
}
