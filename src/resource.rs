//! Resources (spec #19): shared things a repository reads or writes, such as a database table
//! or a bucket, declared in `meada.toml` as `[[resource]]`. No code names them, so this kind
//! reads no code: its sides are the `via` symbols of each entry, writers as providers and
//! readers as consumers of the contract `resource::<kind>::<id>`. Every writer links to every
//! reader of the same resource in another repository, at the level `declared`.

use crate::contract::{ContractKind, Found, Level, Link};
use crate::index::{Consumer, Export, Provider};
use crate::resolve::Code;
use crate::workspace::Repository;

pub struct Resource;

impl ContractKind for Resource {
    fn prefix(&self) -> &'static str {
        "resource"
    }

    fn extract(&self, _code: &Code) -> Vec<Found> {
        Vec::new()
    }

    /// One side per `via` symbol the repository has; a `via` entry that names no symbol is
    /// ignored.
    // ponytail: an entry without `via`, or with no `via` symbol found, has no side yet; the
    // repository-level side comes with the rest of spec #19.
    fn declare(&self, repo: &Repository, export: &mut Export) {
        for resource in &repo.config.resources {
            let contract = format!("resource::{}::{}", resource.kind, resource.id);
            for via in &resource.via {
                let site = format!("{}::{via}", repo.identity);
                if !export.symbols.iter().any(|s| s.id == site) {
                    continue;
                }
                if resource.role.writes() {
                    export.providers.push(Provider {
                        contract: contract.clone(),
                        site: site.clone(),
                        gateway: None,
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
