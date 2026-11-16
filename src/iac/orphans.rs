//! Orphans of infrastructure contracts (ADR 0026), within the query's visible workspace.

use std::collections::{BTreeMap, BTreeSet};

use crate::{contract, index::Export, msg::Msg};

use super::provision;

#[derive(Default)]
struct Group {
    provisioned: bool,
    /// Any use, a name match included: the marks of `context` and `search` count every side.
    used: bool,
    /// One of `provision::orphan_uses`, the only uses that open the orphan scope (ADR 0026).
    opens_scope: bool,
}

/// Groups are separate per contract kind and broker or resource kind, never per name.
pub(crate) struct Scope<'e> {
    exports: &'e [Export],
    groups: BTreeMap<(&'static str, &'e str), Group>,
}

fn group(contract: &str) -> Option<(&'static str, &str)> {
    let kind = contract::kind_of(contract)?;
    Some((kind.prefix(), kind.provisioned_group(contract)?))
}

impl<'e> Scope<'e> {
    pub(crate) fn new(exports: &'e [Export]) -> Self {
        let mut groups = BTreeMap::<_, Group>::new();
        for export in exports {
            for p in &export.provisioners {
                if let Some(key) = group(&p.contract) {
                    groups.entry(key).or_default().provisioned = true;
                }
            }
            for (_, contract) in provision::uses(export) {
                if let Some(key) = group(contract) {
                    groups.entry(key).or_default().used = true;
                }
            }
            for (_, contract, _) in provision::orphan_uses(export) {
                if let Some(key) = group(contract) {
                    groups.entry(key).or_default().opens_scope = true;
                }
            }
        }
        Self { exports, groups }
    }

    /// Query answers are English, like the rest of search and context.
    pub(crate) fn use_note(&self, contract: &str) -> &'static str {
        match group(contract).and_then(|key| self.groups.get(&key)) {
            Some(g) if !g.provisioned => " (no provisioner in the workspace)",
            _ => "",
        }
    }

    pub(crate) fn provisioner_note(&self, contract: &str) -> &'static str {
        match group(contract).and_then(|key| self.groups.get(&key)) {
            Some(g) if !g.used => " (no use in the workspace)",
            _ => "",
        }
    }

    /// Unknown types and data blocks are nodes, but hold no provisioner side to mark.
    pub(crate) fn node_note(&self, repository: &str, node: &super::Node) -> &'static str {
        let identity = format!("{repository}::{}::{}", node.path, node.address);
        self.exports
            .iter()
            .find(|e| e.repository == repository)
            .and_then(|e| {
                e.provisioners
                    .iter()
                    .find(|p| p.identity == identity && p.node.root == node.root)
            })
            .map_or("", |p| self.provisioner_note(&p.contract))
    }

    /// One report per repository, ordered by contract and side. Matching is exactly what
    /// provisioning links say, including patterns and independent root modules.
    pub(crate) fn reports(&self) -> Vec<Vec<String>> {
        let links = provision::links(self.exports);
        let both = |contract: &str| {
            group(contract)
                .and_then(|key| self.groups.get(&key))
                .is_some_and(|g| g.provisioned && g.opens_scope)
        };
        self.exports
            .iter()
            .map(|export| {
                let mut rows = BTreeSet::new();
                for p in &export.provisioners {
                    if both(&p.contract)
                        && !links
                            .iter()
                            .any(|l| l.provisioner == p.identity && l.root == p.node.root)
                    {
                        let place = p.place(&export.repository);
                        let place = place
                            .strip_prefix(&format!("{} ", export.repository))
                            .unwrap_or(&place);
                        rows.insert((
                            p.contract.as_str(),
                            Msg::OrphanProvisioner {
                                contract: &p.contract,
                                place,
                            }
                            .text(),
                        ));
                    }
                }
                for (site, contract, level) in provision::orphan_uses(export) {
                    if both(contract)
                        && !links
                            .iter()
                            .any(|l| l.side == site && l.contract == contract)
                    {
                        let place = if site == export.repository {
                            "(repository-level)"
                        } else {
                            site.strip_prefix(&format!("{}::", export.repository))
                                .unwrap_or(site)
                        };
                        let line = Msg::OrphanUse {
                            contract,
                            place,
                            level,
                        };
                        rows.insert((contract, line.text()));
                    }
                }
                rows.into_iter().map(|(_, line)| line).collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::group;
    use crate::iac::types::ENTRIES;

    /// ADR 0026, amended 2026-11-16 (#814): a kind no entry of the type table provisions has no
    /// orphan group, so its sides carry no mark.
    #[test]
    fn only_a_kind_the_type_table_provisions_has_an_orphan_group() {
        assert_eq!(group("resource::postgres::orders"), None);
        assert_eq!(group("resource::routing::team-b"), None);
        assert_eq!(group("channel::nats::orders.created"), None);
        assert_eq!(group("channel::sqs::orders"), Some(("channel", "sqs")));
        assert_eq!(group("resource::s3::archive"), Some(("resource", "s3")));
    }

    /// The groups come from the type table: every kind it provisions has one.
    #[test]
    fn every_kind_of_the_type_table_has_an_orphan_group() {
        for entry in ENTRIES.iter() {
            let contract = entry.contract("name");
            assert_eq!(
                group(&contract).map(|(_, kind)| kind),
                Some(entry.kind.as_str()),
                "{contract}"
            );
        }
    }
}
