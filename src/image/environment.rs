//! Resolve directed contract reads only through the container that runs their code.

use crate::contract::Level;
use crate::contract::{EnvironmentScope, EnvironmentValue, ReceiverIdentity};
use crate::iac::environment::Entry;
use crate::index::Export;
use crate::workspace::Repository;

use std::collections::BTreeMap;
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
    certain: bool,
    /// A chart environment's scope (`image::chart_env`): only a kind that reads chart
    /// environments sees it, for sites the scope's package owns.
    helm: Option<Helm<'a>>,
}

/// What a chart environment's scope knows of its chart: its directory, its values file, its
/// key and the chart's environment files, so a variable's agreement across the chart can be told.
#[derive(Clone)]
struct Helm<'a> {
    chart: &'a str,
    file: &'a str,
    key: &'a str,
    environments: Arc<[String]>,
}

type Paths<'a> = BTreeMap<&'a str, Vec<Scope<'a>>>;
type Variables<'a> = BTreeMap<&'a str, Paths<'a>>;

/// Workload-scoped values, shared with kinds that resolve an HTTP base or a resource name.
pub(crate) struct Environments<'a> {
    scopes: BTreeMap<String, Variables<'a>>,
    /// Each repository's manifests, for the package a chart environment's site belongs to.
    manifests: BTreeMap<&'a str, &'a [crate::package::Manifest]>,
}

/// A base URL carries its workload scope without putting identities in human reason text.
pub(crate) struct ScopedValue<'a> {
    pub value: Option<&'a str>,
    pub reason: String,
    pub workload: &'a str,
    pub root: &'a str,
}

impl<'a> Environments<'a> {
    pub(crate) fn new(exports: &'a [Export], repositories: &[Repository]) -> Self {
        if !exports
            .iter()
            .any(|e| e.iac.iter().any(|n| !n.environment.is_empty()))
        {
            return Self {
                scopes: BTreeMap::new(),
                manifests: BTreeMap::new(),
            };
        }
        let links = super::execution_links(exports, repositories);
        let mut builders = BTreeMap::<_, BTreeMap<_, bool>>::new();
        for link in &links {
            builders
                .entry((
                    link.consumer.clone(),
                    link.root.as_deref().unwrap_or(".").to_string(),
                    link.contract.clone(),
                ))
                .or_default()
                .insert(
                    super::repository(&link.provider),
                    link.level != Level::Ambiguous,
                );
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
            manifests: (exports.iter())
                .map(|e| (e.repository.as_str(), e.manifests.as_slice()))
                .collect(),
        };
        for export in exports {
            let charts: BTreeMap<String, Arc<[String]>> = if export.iac.iter().any(super::chart_env)
            {
                (super::summaries(export).into_iter())
                    .filter(|(_, s)| !s.envs.is_empty())
                    .map(|(path, s)| (path, s.chart.into()))
                    .collect()
            } else {
                BTreeMap::new()
            };
            for node in &export.iac {
                if node.environment.is_empty() {
                    continue;
                }
                let helm = super::chart_env(node).then(|| Helm {
                    chart: &node.root,
                    file: &node.path,
                    key: &node.address,
                    environments: charts.get(&node.path).cloned().unwrap_or_else(|| [].into()),
                });
                let workload: Arc<str> = node.identity(&export.repository).into();
                let direct = executions.get(&(workload.as_ref(), node.root.as_str()));
                // Resolve one container image once, independently of how many variables it sets.
                let mut images = BTreeMap::new();
                for entry in &node.environment {
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
                            .map(|repos| {
                                repos
                                    .iter()
                                    .map(|(repo, certain)| (*repo, *certain))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    });
                    let paths = direct
                        .into_iter()
                        .flatten()
                        .map(|(repository, path)| (*repository, *path, true))
                        .chain(repositories.iter().map(|(r, certain)| (*r, ".", *certain)));
                    for (repository, path, certain) in paths {
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
                                certain,
                                helm: helm.clone(),
                            });
                    }
                }
            }
        }
        out
    }

    /// HTTP keeps unread entries too, so a Secret cannot fall back to a guessed base URL.
    pub(crate) fn scoped_values(
        &self,
        repository: &str,
        site: &str,
        variable: &str,
    ) -> Vec<ScopedValue<'_>> {
        let mut entries = self.entries(repository, site, variable, false);
        // An unresolved envFrom can hold any matching name, except an explicit override in
        // that same container. Reuse the indexed paths rather than scan source or processes.
        if let Some(variables) = self.scopes.get(repository) {
            let unread = variables
                .keys()
                .filter(|name| {
                    name.strip_suffix('*')
                        .is_some_and(|prefix| variable.starts_with(prefix))
                })
                .flat_map(|name| self.entries(repository, site, name, false))
                .filter(|scope| {
                    !entries.iter().any(|exact| {
                        exact.workload == scope.workload
                            && exact.root == scope.root
                            && exact.entry.container == scope.entry.container
                    })
                })
                .collect::<Vec<_>>();
            entries.extend(unread);
        }
        entries
            .into_iter()
            .map(|scope| ScopedValue {
                value: scope
                    .entry
                    .value
                    .as_deref()
                    .filter(|_| scope.certain && scope.entry.contract.is_none()),
                reason: scope.reason(variable),
                workload: &scope.workload,
                root: scope.root,
            })
            .collect()
    }

    /// Values and their explanations for one variable at one code site, each with the cap a
    /// chart environment's value carries (Goal 2): `declared` when every environment of every
    /// chart in scope that sets the variable gives the one literal, else `inferred`; `None` for
    /// any other workload's. `helm` is whether the reader's kind reads chart environments. No
    /// process is read.
    fn values(
        &self,
        repository: &str,
        site: &str,
        variable: &str,
        helm: bool,
    ) -> Vec<(&str, String, Option<Level>)> {
        let scopes = (self.entries(repository, site, variable, helm).into_iter())
            .filter(|s| s.certain && s.entry.contract.is_none())
            .collect::<Vec<_>>();
        let charts = scopes.iter().filter_map(|s| s.helm.as_ref());
        let set: std::collections::HashSet<(&str, &str)> =
            charts.clone().map(|h| (h.chart, h.file)).collect();
        let every: BTreeMap<&str, &[String]> =
            charts.map(|h| (h.chart, &*h.environments)).collect();
        let mut values = scopes.iter().filter(|s| s.helm.is_some());
        let first = values.next().and_then(|s| s.entry.value.as_deref());
        // A chart that never sets the variable has no scope here, so it says nothing.
        let agreed = first.is_some()
            && values.all(|s| s.entry.value.as_deref() == first)
            && (every.iter()).all(|(chart, environments)| {
                (environments.iter()).all(|env| set.contains(&(*chart, env.as_str())))
            });
        let cap = if agreed {
            Level::Declared
        } else {
            Level::Inferred
        };
        (scopes.into_iter())
            .filter_map(|s| {
                let value = s.entry.value.as_deref()?;
                Some((value, s.reason(variable), s.helm.as_ref().map(|_| cap)))
            })
            .collect()
    }

    /// Typed known resource references share exactly the same code/container/root scope.
    fn targets(
        &self,
        repository: &str,
        site: &str,
        variable: &str,
        helm: bool,
    ) -> Vec<(&str, String)> {
        self.entries(repository, site, variable, helm)
            .into_iter()
            .filter(|s| s.certain)
            .filter_map(|s| Some((s.entry.contract.as_deref()?, s.reason(variable))))
            .collect()
    }

    fn receiver_values(
        &self,
        repository: &str,
        site: &str,
        variable: &str,
        helm: bool,
    ) -> Vec<EnvironmentValue> {
        self.entries(repository, site, variable, helm)
            .into_iter()
            .filter(|scope| scope.certain)
            .map(|scope| EnvironmentValue {
                scope: EnvironmentScope {
                    workload: scope.workload.to_string(),
                    root: scope.root.into(),
                    container: scope.entry.container.clone(),
                },
                value: scope.entry.value.clone(),
                target: scope.entry.contract.clone(),
                reason: scope.reason(variable),
            })
            .collect()
    }

    /// The scopes that hold `variable` for `site`: a workload's whose code path holds the
    /// site's file, and, when `helm` (the reader's kind reads chart environments), a chart
    /// environment's whose package is the one that owns the file, so a nested package is out.
    fn entries(&self, repository: &str, site: &str, variable: &str, helm: bool) -> Vec<&Scope<'a>> {
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
        let owner = helm
            .then(|| self.manifests.get(repository))
            .flatten()
            .and_then(|manifests| crate::package::owner_of(manifests, path))
            .map(|m| m.path.rsplit_once('/').map_or(".", |(dir, _)| dir));
        // Only the file and its ancestors can contain this site; unrelated build paths are skipped.
        let ancestors =
            std::iter::successors(Some(path), |p| p.rsplit_once('/').map(|(parent, _)| parent));
        std::iter::once(".")
            .chain(ancestors)
            .filter_map(|path| Some((path, paths.get(path)?)))
            .flat_map(|(path, scopes)| scopes.iter().map(move |s| (path, s)))
            .filter(|(path, s)| s.helm.is_none() || owner == Some(*path))
            .map(|(_, s)| s)
            .collect()
    }
}

impl Scope<'_> {
    fn reason(&self, variable: &str) -> String {
        let key = self.helm.as_ref();
        let key = key.map_or(String::new(), |h| format!(", helm_env {}", h.key));
        format!(
            "workload:{} (root {}), env:{variable}{key}",
            self.workload, self.root
        )
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
            let resolve = |site: &str,
                           contract: &str,
                           via: Option<&str>,
                           operation: Option<&str>,
                           identity: Option<&ReceiverIdentity>| {
                let via = via?;
                let variable = via
                    .strip_prefix("env:")
                    .or_else(|| via.strip_prefix(PENDING))?;
                if !crate::contract::unresolved(contract) {
                    return None;
                }
                let kind = crate::contract::kind_of(contract)?;
                // Only channel resolution reads chart environments (front 6, Goal 2).
                let helm = kind.prefix() == "channel";
                if let Some(identity) = identity
                    && let Some(values) =
                        kind.environment_identity(contract, identity, &|variable| {
                            environments.receiver_values(&export.repository, site, variable, helm)
                        })
                {
                    let values = values.into_iter().map(|(name, via)| (name, via, None));
                    return Some(values.collect::<Vec<_>>()).filter(|v| !v.is_empty());
                }
                type Sources = (std::collections::BTreeSet<String>, Option<Level>);
                let mut values = std::collections::BTreeMap::<String, Sources>::new();
                let repository = &export.repository;
                for (value, reason, cap) in environments.values(repository, site, variable, helm) {
                    if let Some(name) = kind.environment_name(contract, value, operation) {
                        let (reasons, side) = values.entry(name).or_default();
                        reasons.insert(reason);
                        // A side is `declared` when one of its sources is.
                        *side = [*side, cap].into_iter().flatten().min();
                    }
                }
                for (target, reason) in environments.targets(repository, site, variable, helm) {
                    if kind.environment_target(contract, target, operation) {
                        values.entry(target.into()).or_default().0.insert(reason);
                    }
                }
                let values = values
                    .into_iter()
                    .map(|(name, (reasons, cap))| {
                        (
                            name,
                            reasons.into_iter().collect::<Vec<_>>().join("; "),
                            cap,
                        )
                    })
                    .collect::<Vec<_>>();
                (!values.is_empty()).then_some(values)
            };
            let providers = export
                .providers
                .iter()
                .map(|p| {
                    resolve(
                        &p.site,
                        &p.contract,
                        p.via.as_deref(),
                        p.method.as_deref(),
                        p.identity.as_ref(),
                    )
                })
                .collect::<Vec<_>>();
            let consumers = export
                .consumers
                .iter()
                .map(|c| {
                    c.contract.as_deref().and_then(|contract| {
                        resolve(
                            &c.site,
                            contract,
                            c.via.as_deref(),
                            c.method.as_deref(),
                            c.identity.as_ref(),
                        )
                    })
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
                    .map(|(contract, via, cap)| {
                        let mut p = p.clone();
                        p.contract = contract;
                        p.via = Some(via);
                        p.cap = cap;
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
                    .map(|(contract, via, cap)| {
                        let mut c = c.clone();
                        c.contract = Some(contract);
                        c.via = Some(via);
                        c.cap = cap;
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
