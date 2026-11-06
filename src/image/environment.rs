//! Resolve directed contract reads only through the container that runs their code.

use super::Image;
use crate::contract::{ContractKind, Level};
use crate::iac::environment::Entry;
use crate::index::Export;
use crate::workspace::Repository;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

// Legacy channel sides exposed no environment provenance before workload resolution.
// Keep their new variable data distinct from established SDK `env:` metadata.
const PENDING: &str = "pending-workload-env:";

pub(crate) fn pending(variable: &str) -> String {
    format!("{PENDING}{variable}")
}

struct Scope<'a> {
    workload: Arc<str>,
    root: &'a str,
    entry: &'a Entry,
}

type Paths<'a> = BTreeMap<&'a str, Vec<Scope<'a>>>;
type Variables<'a> = BTreeMap<&'a str, Paths<'a>>;

/// Workload-scoped values, shared with kinds that resolve an HTTP base or a resource name.
pub(crate) struct Environments<'a> {
    scopes: BTreeMap<String, Variables<'a>>,
}

impl<'a> Environments<'a> {
    pub(crate) fn new(exports: &'a [Export], repositories: &[Repository]) -> Self {
        if !exports
            .iter()
            .any(|e| e.iac.iter().any(|n| !n.environment.is_empty()))
        {
            return Self {
                scopes: BTreeMap::new(),
            };
        }
        let providers = exports
            .iter()
            .zip(repositories)
            .flat_map(|(e, r)| {
                e.providers
                    .iter()
                    .filter(|p| p.contract.starts_with("image::"))
                    .map(move |p| (r, p))
            })
            .collect::<Vec<_>>();
        let links = exports
            .iter()
            .zip(repositories)
            .flat_map(|(e, repo)| {
                e.consumers
                    .iter()
                    .filter(|c| {
                        c.contract
                            .as_deref()
                            .is_some_and(|c| c.starts_with("image::"))
                    })
                    .flat_map(|c| Image.link(c, repo, &providers, exports))
            })
            .collect::<Vec<_>>();
        let mut builders = BTreeMap::<_, BTreeSet<_>>::new();
        for link in &links {
            if link.level != Level::Ambiguous {
                builders
                    .entry((
                        link.consumer.clone(),
                        link.root.as_deref().unwrap_or(".").to_string(),
                        link.contract.clone(),
                    ))
                    .or_default()
                    .insert(link.provider.as_str());
            }
        }
        let mut executions = BTreeMap::<_, Vec<_>>::new();
        for x in exports.iter().flat_map(|e| &e.executions) {
            executions
                .entry((x.workload.as_str(), x.root.as_str()))
                .or_default()
                .push((x.repository.as_str(), x.path.as_str()));
        }
        let mut out = Self {
            scopes: BTreeMap::new(),
        };
        for export in exports {
            for node in &export.iac {
                if node.environment.is_empty() {
                    continue;
                }
                let workload: Arc<str> = node.identity(&export.repository).into();
                let direct = executions.get(&(workload.as_ref(), node.root.as_str()));
                // Resolve one container image once, independently of how many variables it sets.
                let mut images = BTreeMap::new();
                for entry in &node.environment {
                    if entry.value.is_none() {
                        continue;
                    }
                    let repositories = images.entry(entry.image.as_deref()).or_insert_with(|| {
                        entry
                            .image
                            .as_deref()
                            .and_then(super::evaluated)
                            .and_then(|i| {
                                builders.get(&(
                                    workload.to_string(),
                                    node.root.clone(),
                                    format!("image::{i}"),
                                ))
                            })
                            .map(|repos| repos.iter().copied().collect::<Vec<_>>())
                            .unwrap_or_default()
                    });
                    let paths = direct
                        .into_iter()
                        .flatten()
                        .copied()
                        .chain(repositories.iter().map(|r| (*r, ".")));
                    for (repository, path) in paths {
                        out.scopes
                            .entry(repository.into())
                            .or_default()
                            .entry(&entry.name)
                            .or_default()
                            .entry(path)
                            .or_default()
                            .push(Scope {
                                workload: workload.clone(),
                                root: &node.root,
                                entry,
                            });
                    }
                }
            }
        }
        out
    }

    /// Values and their explanations for one variable at one code site. No process is read.
    pub(crate) fn values(
        &self,
        repository: &str,
        site: &str,
        variable: &str,
    ) -> Vec<(&str, String)> {
        let Some(path) = site
            .strip_prefix(repository)
            .and_then(|s| s.strip_prefix("::"))
            .and_then(|s| s.split("::").next())
        else {
            return Vec::new();
        };
        let Some(paths) = self
            .scopes
            .get(repository)
            .and_then(|variables| variables.get(variable))
        else {
            return Vec::new();
        };
        // Only the file and its ancestors can contain this site; unrelated build paths are skipped.
        let ancestors =
            std::iter::successors(Some(path), |p| p.rsplit_once('/').map(|(parent, _)| parent));
        std::iter::once(".")
            .chain(ancestors)
            .filter_map(|path| paths.get(path))
            .flatten()
            .filter_map(|s| {
                Some((
                    s.entry.value.as_deref()?,
                    format!("workload:{} (root {}), env:{variable}", s.workload, s.root),
                ))
            })
            .collect()
    }
}

pub(super) fn resolve(exports: &mut [Export], repositories: &[Repository]) {
    let environments = Environments::new(exports, repositories);
    if environments.scopes.is_empty() {
        clear_pending(exports);
        return;
    }
    // Compute replacements before mutating the exports that own each scope's stored entries.
    let replacements = exports
        .iter()
        .map(|export| {
            let resolve = |site: &str, contract: &str, via: Option<&str>| {
                let via = via?;
                let variable = via
                    .strip_prefix("env:")
                    .or_else(|| via.strip_prefix(PENDING))?;
                if !crate::contract::unresolved(contract) {
                    return None;
                }
                let mut values =
                    std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
                for (value, reason) in environments.values(&export.repository, site, variable) {
                    if let Some(name) = named(contract, value) {
                        values.entry(name).or_default().insert(reason);
                    }
                }
                let values = values
                    .into_iter()
                    .map(|(name, reasons)| {
                        (name, reasons.into_iter().collect::<Vec<_>>().join("; "))
                    })
                    .collect::<Vec<_>>();
                (!values.is_empty()).then_some(values)
            };
            let providers = export
                .providers
                .iter()
                .map(|p| resolve(&p.site, &p.contract, p.via.as_deref()))
                .collect::<Vec<_>>();
            let consumers = export
                .consumers
                .iter()
                .map(|c| {
                    c.contract
                        .as_deref()
                        .and_then(|contract| resolve(&c.site, contract, c.via.as_deref()))
                })
                .collect::<Vec<_>>();
            (providers, consumers)
        })
        .collect::<Vec<_>>();
    for (export, (providers, consumers)) in exports.iter_mut().zip(replacements) {
        export.providers = std::mem::take(&mut export.providers)
            .into_iter()
            .zip(providers)
            .flat_map(|(p, replacements)| match replacements {
                None => vec![p],
                Some(values) => values
                    .into_iter()
                    .map(|(contract, via)| {
                        let mut p = p.clone();
                        p.contract = contract;
                        p.via = Some(via);
                        p
                    })
                    .collect(),
            })
            .collect();
        export.consumers = std::mem::take(&mut export.consumers)
            .into_iter()
            .zip(consumers)
            .flat_map(|(c, replacements)| match replacements {
                None => vec![c],
                Some(values) => values
                    .into_iter()
                    .map(|(contract, via)| {
                        let mut c = c.clone();
                        c.contract = Some(contract);
                        c.via = Some(via);
                        c
                    })
                    .collect(),
            })
            .collect();
        crate::index::sort_providers(&mut export.providers);
    }
    clear_pending(exports);
}

fn clear_pending(exports: &mut [Export]) {
    for export in exports {
        for via in export
            .providers
            .iter_mut()
            .map(|p| &mut p.via)
            .chain(export.consumers.iter_mut().map(|c| &mut c.via))
        {
            if via.as_deref().is_some_and(|via| via.starts_with(PENDING)) {
                *via = None;
            }
        }
        crate::index::sort_consumers(&mut export.consumers);
    }
}

fn named(prefix: &str, value: &str) -> Option<String> {
    if !(prefix.starts_with("channel::") || prefix.starts_with("resource::")) {
        return None;
    }
    if let Some(contract) = crate::iac::types::identified(value) {
        return (contract.rsplit_once("::")?.0 == prefix).then_some(contract);
    }
    (!value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || b"._-".contains(&b)
                || (prefix.starts_with("resource::") && b == b'/')
        }))
    .then(|| format!("{prefix}::{value}"))
}
