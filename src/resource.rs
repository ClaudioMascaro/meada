//! Resources (spec #19): shared things a repository reads or writes, such as a database table
//! or a bucket, declared in `meada.toml` as `[[resource]]`. No code names them, so this kind
//! reads no code: its sides are the `via` symbols of each entry, or the repository as a whole
//! when it names none, writers as providers and
//! readers as consumers of the contract `resource::<kind>::<id>`. Every writer links to every
//! reader of the same resource in another repository, at the level `declared`.

use std::collections::HashSet;

use crate::config;
use crate::contract::{ContractKind, Found, Level, Link};
use crate::index::{Consumer, Export, Provider};
use crate::msg::Msg;
use crate::resolve::Code;
use crate::workspace::Repository;

pub struct Resource;

/// The `via` sites of `resource` that are symbols of the repository.
fn matched(repo: &Repository, export: &Export, resource: &config::Resource) -> Vec<String> {
    let site = |via: &String| format!("{}::{via}", repo.identity);
    let found = |site: &String| export.symbols.iter().any(|s| s.id == *site);
    resource.via.iter().map(site).filter(found).collect()
}

/// The sides of `resource`: its matched `via` symbols, else the repository as a whole.
fn sites(repo: &Repository, export: &Export, resource: &config::Resource) -> Vec<String> {
    match matched(repo, export, resource) {
        sites if sites.is_empty() => vec![repo.identity.clone()],
        sites => sites,
    }
}

/// The status lines of `repo` (whose index content is `export`) about its resources: each
/// `via` that names no symbol, the fallback to the repository as a whole, and each resource
/// no link of the workspace (`links`) reaches.
pub fn report(repo: &Repository, export: &Export, links: &[Link]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut seen = HashSet::new();
    for resource in &repo.config.resources {
        let contract = format!("resource::{}::{}", resource.kind, resource.id);
        let found = matched(repo, export, resource);
        for via in &resource.via {
            if !found.contains(&format!("{}::{via}", repo.identity)) {
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

    fn extract(&self, _code: &Code) -> Vec<Found> {
        Vec::new()
    }

    /// One side per `via` symbol the repository has; a `via` entry that names no symbol is
    /// ignored. An entry with no `via`, or none that names a symbol, makes the repository as a
    /// whole its side: the site is the repository identity.
    fn declare(&self, repo: &Repository, export: &mut Export) {
        for resource in &repo.config.resources {
            let contract = format!("resource::{}::{}", resource.kind, resource.id);
            for site in sites(repo, export, resource) {
                if resource.role.writes() {
                    export.providers.push(Provider {
                        contract: contract.clone(),
                        site: site.clone(),
                    });
                }
                if resource.role.reads() {
                    export.consumers.push(Consumer {
                        contract: Some(contract.clone()),
                        site,
                        via: None,
                        method: None,
                        fragments: Vec::new(),
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

    /// The reader `consumer` links to every writer of its resource held by another repository.
    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        providers
            .iter()
            .filter(|(repo, p)| p.contract == *contract && repo.identity != from.identity)
            .map(|(_, p)| Link {
                consumer: consumer.site.clone(),
                provider: p.site.clone(),
                contract: contract.clone(),
                level: Level::Declared,
                reason: contract.clone(),
                by: None,
                p: None,
            })
            .collect()
    }
}
