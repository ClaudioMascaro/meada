//! A builder's unresolved name is a source item when several literal runner names fit.

use std::collections::{BTreeMap, BTreeSet};

use crate::contract::Link;
use crate::decisions::{Cause, Decision, Evaluation, Kind, Stale, Waiting};
use crate::index::{Export, Provider};
use crate::items::{Candidate, ItemKind, NameCandidates, OpenItem};
use crate::workspace::Workspace;

pub(super) struct Builder<'a> {
    export: &'a Export,
    provider: &'a Provider,
}

impl Builder<'_> {
    fn id(&self) -> String {
        format!(
            "image-builder::{}::{}",
            self.export.repository, self.provider.contract
        )
    }

    fn names(&self) -> Option<&NameCandidates> {
        self.export.name_candidates.get(&self.id())
    }

    fn candidates(&self) -> &[Candidate] {
        self.names().map_or(&[], |names| &names.candidates)
    }

    fn ambiguous(&self) -> bool {
        self.names().is_some_and(|names| names.distinct > 1)
    }

    fn evaluate<'a>(&self, decision: &'a Decision, exports: &[Export]) -> Result<&'a str, Cause> {
        if decision.kind != Kind::AmbiguousLink || !self.ambiguous() {
            return Err(Cause::ItemNotOpen);
        }
        let (repository, contract) = decision.target.split_once("::").ok_or(Cause::TargetGone)?;
        if self
            .candidates()
            .binary_search_by(|c| {
                (c.repository.as_str(), c.contract.as_str()).cmp(&(repository, contract))
            })
            .is_ok()
        {
            return Ok(contract);
        }
        if !exports
            .iter()
            .filter(|e| e.repository == repository)
            .flat_map(|e| &e.consumers)
            .any(|c| c.contract.as_deref() == Some(contract))
        {
            return Err(Cause::TargetGone);
        }
        Err(Cause::NotCandidate)
    }

    pub(super) fn selected(&self) -> Option<(&str, &Decision)> {
        let (contract, at) = self.names()?.selected.as_ref()?;
        Some((contract, &self.export.decisions.items[*at]))
    }
}

/// The cross-side scan happens once per distinct builder pattern, never once per runner.
pub(super) fn prepare(exports: &mut [Export]) {
    let mut prepared = Vec::new();
    for (at, export) in exports.iter().enumerate() {
        let decisions: BTreeMap<_, _> = export
            .decisions
            .items
            .iter()
            .enumerate()
            .map(|(n, d)| (d.call.as_str(), (n, d)))
            .collect();
        for builder in builders(export)
            .into_iter()
            .filter(|b| b.provider.contract.contains('*'))
        {
            let mut candidates = Vec::new();
            for e in exports.iter() {
                for c in &e.consumers {
                    let Some(contract) = c
                        .contract
                        .as_deref()
                        .filter(|c| super::matches(&builder.provider.contract, c))
                    else {
                        continue;
                    };
                    if e.executions.iter().any(|x| super::runs_locally(x, c)) {
                        continue;
                    }
                    candidates.push(Candidate {
                        repository: e.repository.clone(),
                        contract: contract.into(),
                        site: c.site.clone(),
                    });
                }
            }
            candidates.sort_by(|a, b| {
                (&a.repository, &a.contract, &a.site).cmp(&(&b.repository, &b.contract, &b.site))
            });
            candidates.dedup_by(|a, b| a.repository == b.repository && a.contract == b.contract);
            let distinct = candidates
                .iter()
                .map(|c| &c.contract)
                .collect::<BTreeSet<_>>()
                .len();
            let id = builder.id();
            let selected = decisions
                .get(id.as_str())
                .filter(|(_, d)| d.kind == Kind::AmbiguousLink && distinct > 1)
                .and_then(|(n, d)| {
                    let (repo, contract) = d.target.split_once("::")?;
                    candidates
                        .binary_search_by(|c| {
                            (c.repository.as_str(), c.contract.as_str()).cmp(&(repo, contract))
                        })
                        .ok()
                        .map(|_| (contract.to_owned(), *n))
                });
            prepared.push((
                at,
                id,
                NameCandidates {
                    candidates,
                    distinct,
                    selected,
                },
            ));
        }
    }
    for export in exports.iter_mut() {
        export
            .name_candidates
            .retain(|id, _| !id.starts_with("image-builder::"));
    }
    for (at, id, candidates) in prepared {
        exports[at].name_candidates.insert(id, candidates);
    }
}

/// Orphan reporting follows a valid name choice while retaining direct-code counterparts.
pub(super) fn active_match(exports: &[Export], provider: &Provider, contract: &str) -> bool {
    super::matches(&provider.contract, contract)
        && builder(exports, provider)
            .is_none_or(|b| b.selected().is_none_or(|(chosen, _)| chosen == contract))
}

fn builders(export: &Export) -> Vec<Builder<'_>> {
    let mut seen = BTreeSet::new();
    export
        .providers
        .iter()
        .filter(|p| p.contract.starts_with("image::") && seen.insert(&p.contract))
        .map(|provider| Builder { export, provider })
        .collect()
}

pub(super) fn builder<'a>(exports: &'a [Export], provider: &'a Provider) -> Option<Builder<'a>> {
    let export = exports.iter().find(|e| e.repository == provider.site)?;
    let builder = Builder { export, provider };
    builder.ambiguous().then_some(builder)
}

pub(super) fn handles(decision: &Decision) -> bool {
    decision.call.starts_with("image-builder::")
}

/// Several builders remain a consumer choice. One ambiguous builder belongs to its own item.
pub(super) fn consumer_item(link: &Link, exports: &[Export]) -> bool {
    let candidates = exports
        .iter()
        .flat_map(|e| &e.providers)
        .filter(|p| active_match(exports, p, &link.contract))
        .map(|p| (&p.site, &p.contract))
        .collect::<BTreeSet<_>>();
    candidates.len() > 1
        || !exports.iter().flat_map(|e| &e.providers).any(|p| {
            p.site == link.provider
                && active_match(exports, p, &link.contract)
                && builder(exports, p).is_some()
        })
}

pub(super) fn items(exports: &[Export]) -> Vec<(OpenItem, bool)> {
    let mut items = Vec::new();
    for export in exports {
        for builder in builders(export).into_iter().filter(|b| b.ambiguous()) {
            let at = builder.provider.at.as_deref().unwrap_or("meada.toml");
            let path = at.split('#').next().unwrap_or(at).into();
            let selected = builder.selected().is_some();
            items.push((
                OpenItem {
                    id: builder.id(),
                    kind: ItemKind::Ambiguous,
                    repository: export.repository.clone(),
                    path,
                    line: None,
                    expression: format!("{} at {at}", builder.provider.contract),
                    enclosing: None,
                    candidates: builder.candidates().to_vec(),
                    limit: None,
                },
                selected,
            ));
        }
    }
    items
}

pub(super) fn reports(workspace: &Workspace, exports: &[Export]) -> Vec<Evaluation> {
    exports
        .iter()
        .map(|export| {
            let mut report = Evaluation {
                repository: export.repository.clone(),
                stale: Vec::new(),
                waiting: Vec::new(),
            };
            if !export.decisions.items.iter().any(handles) {
                return report;
            }
            let builders: BTreeMap<_, _> =
                builders(export).into_iter().map(|b| (b.id(), b)).collect();
            for decision in export.decisions.items.iter().filter(|d| handles(d)) {
                let source = builders.get(&decision.call);
                let missing = decision
                    .target
                    .split_once("::")
                    .and_then(|(r, _)| workspace.missing.iter().find(|m| m.repo == r));
                if let Some(peer) = missing.filter(|_| source.is_some()) {
                    report.waiting.push(Waiting {
                        call: decision.call.clone(),
                        target: decision.target.clone(),
                        peer: peer.repo.clone(),
                    });
                } else {
                    let result = source
                        .ok_or(Cause::CallGone)
                        .and_then(|b| b.evaluate(decision, exports));
                    if let Err(cause) = result {
                        report.stale.push(Stale::new(
                            decision.call.clone(),
                            decision.target.clone(),
                            cause,
                        ));
                    }
                }
            }
            report
        })
        .collect()
}
