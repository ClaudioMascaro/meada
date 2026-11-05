//! Provisioner sides and their links (ADR 0026): derived from stored IaC names and the
//! workspace's contract sides, never mixed with consumer-to-provider links.

use meada_core::export::{IacNode, Level, Provision, Provisioner};
use rusqlite::Connection;

use super::{Node, types};
use crate::Result;
use crate::index::Export;

/// A provisioner with the evaluated name's evidence, kept only during a query.
pub(crate) struct Side {
    pub identity: String,
    pub contract: String,
    level: Level,
    pub node: Node,
}

impl Side {
    pub(crate) fn matches(&self, contract: &str) -> bool {
        if !self.node.pattern {
            return self.contract == contract;
        }
        let (Some((kind, pattern)), Some((other, name))) =
            (self.contract.rsplit_once("::"), contract.rsplit_once("::"))
        else {
            return false;
        };
        kind == other && super::pattern::matches(pattern, name)
    }

    pub(crate) fn place(&self, repository: &str) -> String {
        format!(
            "{repository} {}:{} {} (root {})",
            self.node.path, self.node.line, self.node.address, self.node.root
        )
    }

    pub(crate) fn written(&self) -> (IacNode, Provisioner) {
        let mut node = IacNode::default();
        node.identity = self.identity.clone();
        node.address = self.node.address.clone();
        node.ty = self.node.ty.clone();
        node.root = self.node.root.clone();
        node.path = self.node.path.clone();
        node.line = self.node.line;
        node.end_line = self.node.end_line;
        let mut side = Provisioner::default();
        side.contract = self.contract.clone();
        side.node = self.identity.clone();
        side.root = self.node.root.clone();
        (node, side)
    }
}

/// Only resource blocks of known types with a physical name provision contracts.
/// A data block names an existing resource; it does not create one.
pub(crate) fn read(db: &Connection, repository: &str) -> Result<Vec<Side>> {
    let mut statement = db.prepare(&format!("SELECT {} FROM iac_nodes", super::COLUMNS))?;
    let mut sides = Vec::new();
    for node in statement.query_map([], super::row)? {
        let node = node?;
        let Some(format) = types::format_of(&node.path) else {
            continue;
        };
        let Some(entry) = types::entry(format, &node.ty) else {
            continue;
        };
        if !node.block.starts_with("resource.") {
            continue;
        }
        let Some(name) = node.name.as_deref().filter(|n| !n.is_empty()) else {
            continue;
        };
        if node.pattern && !super::pattern::supported(name) {
            continue;
        }
        sides.push(Side {
            identity: format!("{repository}::{}::{}", node.path, node.address),
            contract: entry.contract(name),
            level: match entry.contract {
                types::Contract::Channel => Level::Exact,
                types::Contract::Resource => Level::Declared,
            },
            node,
        });
    }
    sides.sort_by(|a, b| {
        (&a.contract, &a.identity, &a.node.root).cmp(&(&b.contract, &b.identity, &b.node.root))
    });
    Ok(sides)
}

/// The existing directed use sides: provisioning never invents a producer or consumer.
pub(crate) fn uses(export: &Export) -> impl Iterator<Item = (&str, &str)> {
    export
        .providers
        .iter()
        .map(|p| (p.site.as_str(), p.contract.as_str()))
        .chain(
            export
                .consumers
                .iter()
                .filter_map(|c| Some((c.site.as_str(), c.contract.as_deref()?))),
        )
}

/// Every use of the same contract reaches every provisioner, including one in its repository.
/// Several roots are independent sides, never ambiguous candidates of one another.
pub(crate) fn links(exports: &[Export]) -> Vec<Provision> {
    let sides = exports.iter().flat_map(uses).collect::<Vec<_>>();
    let mut links = Vec::new();
    for export in exports {
        for p in &export.provisioners {
            let mut level = p.level;
            let contracts = sides
                .iter()
                .map(|(_, c)| *c)
                .filter(|c| p.matches(c))
                .collect::<std::collections::BTreeSet<_>>();
            let mut reason = p.contract.clone();
            if p.node.pattern {
                level = if contracts.len() > 1 {
                    Level::Ambiguous
                } else {
                    Level::Inferred
                };
                reason += "; pattern";
            }
            if !p.node.overridable.is_empty() {
                level = level.max(Level::Inferred);
                let values = p
                    .node
                    .overridable
                    .iter()
                    .map(|o| {
                        let from = if o.tfvars { "tfvars" } else { "default" };
                        format!(
                            "{} {}:{} {} ({from})",
                            export.repository, o.path, o.line, o.variable
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                reason += &format!("; overridable: {values}");
            }
            for &(site, contract) in &sides {
                if contracts.contains(contract) {
                    links.push(Provision::new(
                        site.into(),
                        p.identity.clone(),
                        p.node.root.clone(),
                        contract.into(),
                        level,
                        reason.clone(),
                    ));
                }
            }
        }
    }
    links.sort();
    links.dedup();
    links
}

pub(crate) fn target<'a>(exports: &'a [Export], link: &Provision) -> Option<(usize, &'a Side)> {
    exports.iter().enumerate().find_map(|(n, e)| {
        e.provisioners
            .iter()
            .find(|p| p.identity == link.provisioner && p.node.root == link.root)
            .map(|p| (n, p))
    })
}

/// Provisioners of the symbol's own contracts and of the calls it makes.
pub(crate) fn context(exports: &[Export], symbol: &str) -> Vec<String> {
    let calls = exports
        .iter()
        .flat_map(|e| &e.calls)
        .filter(|c| c.caller == symbol)
        .map(|c| c.id.as_str())
        .collect::<Vec<_>>();
    let mut lines = links(exports)
        .iter()
        .filter(|l| l.side == symbol || calls.contains(&l.side.as_str()))
        .filter_map(|l| {
            let (n, p) = target(exports, l)?;
            Some(format!(
                "{} [{}: {}]",
                p.place(&exports[n].repository),
                l.level.name(),
                l.reason
            ))
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines.dedup();
    if lines.is_empty() {
        Vec::new()
    } else {
        crate::render::section("provisioners", lines)
    }
}
