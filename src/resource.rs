//! Resources: shared things a repository reads or writes, such as a database table or bucket.
//! Declarations in `meada.toml` (#19, #641) name symbols, IaC addresses or config key paths,
//! else the repository as a whole, and
//! link writers to readers in other repositories at `declared`. AWS SDK forms (#659) supply
//! code sides through their client origins and name arguments; literal SDK writers and readers
//! link at `exact`, in the same repository too. Both use `resource::<kind>::<id>` contracts.

use std::collections::HashSet;

use crate::config;
use crate::contract::{ContractKind, Found, Level, Link};
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::msg::Msg;
use crate::resolve::Code;

mod aws;
mod python;
mod ts;
mod via;
use crate::workspace::Repository;

pub struct Resource;

/// Both reader families establish SDK origins, through stored provenance or an operation.
fn sdk_use(via: Option<&str>, operation: Option<&str>) -> bool {
    via == Some("sdk:aws") || operation.is_some()
}

/// The start of every resource contract identifier.
const PREFIX: &str = "resource::";

/// The contract identifier of `resource`: `resource::<kind>::<id>`.
fn contract(resource: &config::Resource) -> String {
    format!("{PREFIX}{}::{}", resource.kind, resource.id)
}

/// The site a `via` entry of `repo` names.
fn site(repo: &Repository, via: &str) -> String {
    format!("{}::{via}", repo.identity)
}

/// The exact symbols and IaC nodes a declaration names in its own repository.
fn resolved(repo: &Repository, export: &Export, via: &str) -> Vec<String> {
    let symbol = site(repo, via);
    if export.symbols.iter().any(|s| s.id == symbol) {
        return vec![symbol];
    }
    let mut nodes: Vec<_> = crate::iac::named(&repo.identity, &export.iac, via)
        .iter()
        .filter(|n| {
            via == n.address
                || via == format!("{}::{}", n.path, n.address)
                || via == n.identity(&repo.identity)
        })
        .map(|n| n.identity(&repo.identity))
        .collect();
    nodes.sort();
    nodes.dedup();
    if nodes.len() == 1 {
        return nodes;
    }
    // An address shared by different source blocks needs its file qualifier.
    if !nodes.is_empty() {
        return Vec::new();
    }
    if via.contains('#') {
        return export
            .config_sites
            .iter()
            .filter(|s| s.via == via)
            .map(|s| s.id.clone())
            .collect();
    }
    Vec::new()
}

/// Every existing location named by the resource, once in identity order.
fn matched(repo: &Repository, export: &Export, resource: &config::Resource) -> Vec<String> {
    let mut sites: Vec<_> = resource
        .via
        .iter()
        .flat_map(|v| resolved(repo, export, v))
        .collect();
    sites.sort();
    sites.dedup();
    sites
}

/// The sides of `resource`: its matched locations, else the repository as a whole.
fn sites(repo: &Repository, export: &Export, resource: &config::Resource) -> Vec<String> {
    match matched(repo, export, resource) {
        sites if sites.is_empty() => vec![repo.identity.clone()],
        sites => sites,
    }
}

/// The status lines of `repo` (whose index content is `export`) about its resources: each
/// `via` that names no location, the fallback to the repository as a whole, and each resource
/// no link of the workspace (`links`) reaches.
fn report(repo: &Repository, export: &Export, links: &[Link]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut seen = HashSet::new();
    for resource in &repo.config.resources {
        let contract = contract(resource);
        let found = matched(repo, export, resource);
        for via in &resource.via {
            if resolved(repo, export, via).is_empty() {
                let msg = Msg::ViaUnmatched {
                    contract: &contract,
                    via,
                };
                lines.push(msg.text());
            }
        }
        if found.is_empty() && !resource.via.is_empty() {
            lines.push(Msg::ViaFallback(&contract).text());
        }
        if seen.insert(contract.clone()) && !links.iter().any(|l| l.contract == contract) {
            lines.push(Msg::UnlinkedResource(&contract).text());
        }
    }
    lines
}

impl ContractKind for Resource {
    fn prefix(&self) -> &'static str {
        "resource"
    }

    fn flows_to_consumer(&self) -> bool {
        true
    }

    fn sides(&self) -> Option<(&'static str, &'static str)> {
        Some(("writer", "reader"))
    }

    fn extract(&self, code: &Code, indexed: &Indexed) -> Vec<Found> {
        ts::sides(code)
            .into_iter()
            .chain(python::sides(code))
            .chain(aws::extract(code, indexed))
            .collect()
    }

    fn unresolved(&self, contract: &str) -> bool {
        contract
            .strip_prefix(PREFIX)
            .is_some_and(|rest| !rest.contains("::"))
    }

    fn use_level(&self, via: Option<&str>, operation: Option<&str>) -> Level {
        if sdk_use(via, operation) {
            Level::Exact
        } else {
            Level::Declared
        }
    }

    fn at_call(&self, _: &[Export], export: &Export, call: &str) -> Vec<String> {
        let provided = export
            .providers
            .iter()
            .filter(|p| p.site == call && p.contract.starts_with(PREFIX))
            .filter_map(|p| p.via.as_deref());
        let consumed = export
            .consumers
            .iter()
            .filter(|c| {
                c.site == call
                    && c.contract
                        .as_deref()
                        .is_some_and(|contract| contract.starts_with(PREFIX))
            })
            .filter_map(|c| c.via.as_deref());
        provided
            .chain(consumed)
            .filter_map(|via| {
                via.strip_prefix("env:")
                    .map(|name| format!("unresolved environment {name}"))
            })
            .collect()
    }

    fn parts<'c>(&self, contract: &'c str) -> (Option<&'c str>, Option<&'c str>) {
        let rest = contract.strip_prefix(PREFIX);
        rest.and_then(|rest| rest.split_once("::")).unzip()
    }

    fn provisioned_group<'c>(&self, contract: &'c str) -> Option<&'c str> {
        let (kind, name) = self.parts(contract);
        name.filter(|n| !n.is_empty()).and(kind)
    }

    fn read_declarations(
        &self,
        repo: &Repository,
        db: &rusqlite::Connection,
        export: &mut Export,
    ) -> crate::Result<()> {
        via::read(repo, db, export)
    }

    fn declare(&self, repo: &Repository, export: &mut Export) {
        for resource in &repo.config.resources {
            let contract = contract(resource);
            for site in sites(repo, export, resource) {
                if resource.role.writes() {
                    export.providers.push(Provider {
                        contract: contract.clone(),
                        site: site.clone(),
                        ..Provider::default()
                    });
                }
                if resource.role.reads() {
                    export.consumers.push(Consumer {
                        contract: Some(contract.clone()),
                        site,
                        via: None,
                        shape: None,
                        method: None,
                        fragments: Vec::new(),
                        hops: Vec::new(),
                        root: None,
                    });
                }
            }
        }
        // The index's orders; several entries may name the same side.
        let providers = &mut export.providers;
        providers.sort_by(|a, b| (&a.contract, &a.site).cmp(&(&b.contract, &b.site)));
        providers.dedup_by(|a, b| (&a.contract, &a.site) == (&b.contract, &b.site));
        let consumers = &mut export.consumers;
        let key = |c: &Consumer| (c.site.clone(), c.contract.clone(), c.via.clone());
        consumers.sort_by_key(key);
        consumers.dedup_by(|a, b| key(a) == key(b));
    }

    fn context_at(
        &self,
        exports: &[Export],
        _: &[Repository],
        links: &[Link],
        site: &str,
        _: Option<&str>,
    ) -> Vec<String> {
        via::context(exports, links, site)
    }

    /// The `via` symbols of every resource the repository declares (`matched`).
    fn symbols_read(&self, repo: &Repository, _export: &Export) -> Option<Vec<String>> {
        let resources = repo.config.resources.iter();
        Some(
            resources
                .flat_map(|r| &r.via)
                .map(|via| site(repo, via))
                .collect(),
        )
    }

    fn status(&self, repository: &Repository, export: &Export, links: &[Link]) -> Vec<String> {
        report(repository, export, links)
    }

    /// The reader `consumer` links to every writer of its resource held by another repository.
    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
        _exports: &[Export],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        if self.unresolved(contract) {
            return Vec::new();
        }
        providers
            .iter()
            .filter(|(repo, p)| {
                p.contract == *contract
                    && (repo.identity != from.identity
                        || (sdk_use(consumer.via.as_deref(), consumer.method.as_deref())
                            && sdk_use(p.via.as_deref(), p.method.as_deref())))
            })
            .map(|(_, p)| {
                Link::new(
                    consumer.site.clone(),
                    p.site.clone(),
                    contract.clone(),
                    self.use_level(consumer.via.as_deref(), consumer.method.as_deref())
                        .max(self.use_level(p.via.as_deref(), p.method.as_deref())),
                    contract.clone(),
                    None,
                    None,
                    None,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Resource;
    use crate::contract::ContractKind;

    #[test]
    fn a_resource_ranks_against_its_kind_and_its_id() {
        assert_eq!(
            Resource.parts("resource::postgres::orders"),
            (Some("postgres"), Some("orders"))
        );
        assert_eq!(Resource.parts("resource::postgres"), (None, None));
    }
}
