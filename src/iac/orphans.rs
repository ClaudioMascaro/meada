use std::collections::{BTreeMap, BTreeSet};

use crate::{
    contract,
    index::Export,
    msg::{self, Absent, Msg},
};

use super::{provision, types};

#[derive(Default)]
struct Group {
    provisioned: bool,
    used: bool,
    opens_scope: bool,
}

pub(crate) struct Scope<'e> {
    exports: &'e [Export],
    groups: BTreeMap<(&'static str, &'e str), Group>,
}

fn group(contract: &str) -> Option<(&'static str, &str)> {
    let kind = contract::kind_of(contract)?;
    let key = (kind.prefix(), kind.provisioned_group(contract)?);
    types::provisions(key.0, key.1).then_some(key)
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

    pub(crate) fn reports(&self) -> Vec<Vec<String>> {
        let links = provision::links(self.exports);
        let both = |contract: &str| {
            group(contract)
                .and_then(|key| self.groups.get(&key))
                .is_some_and(|g| g.provisioned && g.opens_scope)
        };
        let scope = msg::Scope::of(self.exports);
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
                            Msg::Orphan {
                                contract: &p.contract,
                                absent: Absent::Use,
                                scope,
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
                        let place = format!("{place} [{}]", level.name());
                        let line = Msg::Orphan {
                            contract,
                            absent: Absent::Provisioner,
                            scope,
                            place: &place,
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

    #[test]
    fn only_a_kind_the_type_table_provisions_has_an_orphan_group() {
        assert_eq!(group("resource::postgres::orders"), None);
        assert_eq!(group("resource::routing::team-b"), None);
        assert_eq!(group("channel::nats::orders.created"), None);
        assert_eq!(group("channel::sqs::orders"), Some(("channel", "sqs")));
        assert_eq!(group("resource::s3::archive"), Some(("resource", "s3")));
    }

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
