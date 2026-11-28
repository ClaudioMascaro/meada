use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

use crate::contract::{self, Link, kind_of};
use crate::index::{Consumer, Export, Provider};
use crate::lang::Limit;
use crate::render::{Files, callee};
use crate::workspace::{Missing, Repository};
use meada_core::export::DecisionKind;

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
#[derive(Clone, Debug)]
pub struct Candidate {
    /// The identity of the repository that holds the provider.
    pub repository: String,
    /// The contract it serves, such as `http::GET::/api/health`.
    pub contract: String,
    /// The provider's site, such as `example.com/web::app/api/health/route.ts::GET`.
    pub site: String,
}

#[derive(Default)]
pub(crate) struct NameCandidates {
    pub candidates: Vec<Candidate>,
    pub distinct: usize,
    pub selected: Option<(String, usize)>,
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
    /// 1-based line from the source or indexed declaration; `None` when unknown.
    pub line: Option<usize>,
    /// The call or definition on one line; repository-level and configuration-key items use
    /// contract/location metadata instead of scalar values.
    pub expression: String,
    /// `None` for a call at the top level of its file.
    pub enclosing: Option<Enclosing>,
    /// Sorted by repository identity, then label (method and path of a route), then site.
    pub candidates: Vec<Candidate>,
    /// Why no provider can be a candidate, for an unresolved consumer whose kind says so
    /// (`ContractKind::unresolved_causes`): `meada decide` has nothing to offer for it.
    pub cause: Option<String>,
    /// Total length of the URL's known path fragments, when it is an unresolved URL.
    pub literal_coverage: Option<usize>,
    /// For a consumer whose URL starts with a part Meada could not read, why (#801).
    pub limit: Option<Limit>,
}

impl OpenItem {
    pub(crate) fn decision_kind(&self) -> DecisionKind {
        match self.kind {
            ItemKind::Ambiguous => DecisionKind::AmbiguousLink,
            ItemKind::Unresolved => DecisionKind::DynamicUrl,
        }
    }

    pub(crate) fn written(&self) -> meada_core::export::OpenItem {
        let mut targets: Vec<String> = self
            .candidates
            .iter()
            .map(crate::decisions::Decision::target)
            .collect();
        targets.sort();
        targets.dedup();
        let mut item =
            meada_core::export::OpenItem::new(self.id.clone(), self.decision_kind(), targets);
        item.limit = self.limit.as_ref().map(Limit::describe);
        item
    }

    pub(crate) fn rank_candidates(&mut self) {
        if let Some(coverage) = self.literal_coverage {
            self.candidates.sort_by_cached_key(|candidate| {
                contract::parts(&candidate.contract)
                    .1
                    .map_or(usize::MAX, |path| {
                        path.chars().count().saturating_sub(coverage)
                    })
            });
        }
    }

    /// `path:line`, or `path` when the line is unknown.
    pub fn location(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{line}", self.path),
            None => self.path.clone(),
        }
    }
}

pub(crate) fn sites(exports: &[Export], links: &[Link]) -> Vec<(usize, String)> {
    let ambiguous: HashSet<String> = (links.iter())
        .filter(|l| contract::consumer_item(l, exports))
        .map(|l| l.item_site().into_owned())
        .collect();
    let items = exports.iter().enumerate().flat_map(|(n, e)| {
        let open =
            |c: &&Consumer| c.contract.is_none() || ambiguous.contains(c.item_site().as_ref());
        (e.consumers.iter().filter(open)).map(move |c| (n, c.site.clone()))
    });
    items.collect()
}

pub(crate) fn causes(exports: &[Export]) -> HashMap<String, &'static str> {
    (contract::KINDS.iter())
        .flat_map(|kind| kind.unresolved_causes(exports))
        .collect()
}

pub(crate) fn fits(consumer: &Consumer, provider: &Provider) -> bool {
    kind_of(&provider.contract).is_some_and(|k| k.candidate(consumer, provider))
}

pub(crate) fn list(
    repos: &[Repository],
    exports: &[Export],
    links: &[Link],
    missing: &[Missing],
) -> Vec<OpenItem> {
    let mut ambiguous: HashMap<String, Vec<&Link>> = HashMap::new();
    for link in links.iter().filter(|l| contract::consumer_item(l, exports)) {
        ambiguous
            .entry(link.item_site().into_owned())
            .or_default()
            .push(link);
    }
    let providers: Vec<(&str, &Provider)> = exports
        .iter()
        .flat_map(|e| e.providers.iter().map(|p| (e.repository.as_str(), p)))
        .collect();
    fn candidate<'a>(&(repository, p): &(&'a str, &'a Provider)) -> (&'a str, &'a str, &'a str) {
        (repository, p.contract.as_str(), p.site.as_str())
    }
    let causes = causes(exports);
    let missing: BTreeSet<&str> = missing.iter().map(|m| m.repo.as_str()).collect();
    let missing = missing.into_iter().collect::<Vec<_>>().join(", ");
    let mut items = Vec::new();
    for (export, repo) in exports.iter().zip(repos) {
        let calls: HashMap<&str, _> = export.calls.iter().map(|c| (c.id.as_str(), c)).collect();
        let symbols: HashMap<&str, _> = (export.symbols.iter())
            .map(|s| (s.id.as_str(), s))
            .collect();
        let mut files = Files::new(&repo.root);
        let mut nodes: Option<HashMap<String, &crate::iac::Node>> = None;
        for consumer in &export.consumers {
            let (kind, candidates) = match (
                &consumer.contract,
                ambiguous.get(consumer.item_site().as_ref()),
            ) {
                (None, _) => {
                    let open = !causes.contains_key(consumer.site.as_str());
                    let fitting = (providers.iter()).filter(|(_, p)| open && fits(consumer, p));
                    (
                        ItemKind::Unresolved,
                        fitting.map(candidate).collect::<Vec<_>>(),
                    )
                }
                (Some(_), Some(links)) => {
                    let listed = providers
                        .iter()
                        .filter(|(_, p)| {
                            links
                                .iter()
                                .any(|l| contract::link_candidate(p, l, exports))
                        })
                        .map(candidate);
                    (ItemKind::Ambiguous, listed.collect())
                }
                _ => continue,
            };
            let infrastructure;
            let (span, caller) = match calls.get(consumer.site.as_str()) {
                Some(call) => (&call.span, symbols.get(call.caller.as_str())),
                None => match symbols.get(consumer.site.as_str()) {
                    Some(def) => (&def.span, Some(def)),
                    None => {
                        let nodes = nodes.get_or_insert_with(|| {
                            let nodes = export.iac.iter().rev();
                            nodes.map(|n| (n.identity(&export.repository), n)).collect()
                        });
                        let Some(node) = nodes.get(&consumer.site) else {
                            continue;
                        };
                        infrastructure = node.span(files.get(&node.path));
                        (&infrastructure, None)
                    }
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
            candidates.sort_by_cached_key(|c| (c.repository.clone(), c.label(), c.site.clone()));
            let mut listed = HashSet::new();
            candidates.retain(|c| listed.insert((c.repository.clone(), c.contract.clone())));
            let (mut cause, mut literal_coverage) = (None, None);
            if kind == ItemKind::Unresolved {
                cause = causes.get(&consumer.site).map(|cause| (*cause).to_string());
                if cause.is_none() {
                    literal_coverage =
                        Some(consumer.fragments.iter().map(|s| s.chars().count()).sum());
                    if candidates.is_empty() {
                        let providers = exports.iter().flat_map(|e| &e.providers);
                        cause = contract::KINDS.iter().find_map(|kind| {
                            kind.no_candidates(consumer, &mut providers.clone(), &missing)
                        });
                    }
                }
            }
            items.push(OpenItem {
                id: consumer.item_site().into_owned(),
                kind,
                repository: export.repository.clone(),
                line: files.line(&span.path, span.start),
                path: span.path.clone(),
                expression,
                enclosing: caller.map(|s| Enclosing {
                    name: crate::render::label(&s.id).to_string(),
                    span: range(s.span.start, s.span.end),
                }),
                candidates,
                cause,
                literal_coverage,
                limit: (consumer.contract.is_none())
                    .then(|| consumer.lead.as_ref().map(|l| l.limit.clone()))
                    .flatten(),
            });
        }
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Level;

    fn ambiguous(contract: &str) -> Link {
        Link::new(
            "r::src/a.ts::f::g#0".into(),
            "r::src/b.ts::h".into(),
            contract.into(),
            Level::Ambiguous,
            String::new(),
            None,
            None,
            None,
        )
    }

    #[test]
    fn only_an_ambiguous_link_of_a_decidable_kind_is_an_item() {
        assert!(contract::consumer_item(
            &ambiguous("http::GET::/users"),
            &[]
        ));
        let mut exact = ambiguous("http::GET::/users");
        exact.level = Level::Exact;
        assert!(!contract::consumer_item(&exact, &[]));
    }
}
