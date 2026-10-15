//! Channels (spec #20): the queues and topics of a broker, identified by
//! `channel::<broker>::<name>`. Language packs report the producer and consumer calls of their
//! message libraries (`LanguagePack::messages`); this kind turns them into contracts and links
//! every consumer of a channel to every producer of it, in any repository of the workspace,
//! its own included. Producers are the provider side: messages go from the producing call to
//! the consumer's handler. Broker clusters are not modelled: one name of one broker kind is
//! one channel across the workspace.

use std::collections::HashMap;

use crate::contract::{ContractKind, Found, Level, Link, Role, Site};
use crate::index::{Consumer, Provider};
use crate::lang::{self, Handler};
use crate::resolve::Code;
use crate::workspace::Repository;

pub struct Channel;

/// The start of every channel contract identifier.
pub const PREFIX: &str = "channel::";

/// The contract identifier of the channel `name` of `broker`, kept as evaluated. A side whose
/// name could not be evaluated is `channel::<broker>`: it names its broker kind, never links
/// and is not an open item.
pub fn contract(broker: &str, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{PREFIX}{broker}::{name}"),
        None => format!("{PREFIX}{broker}"),
    }
}

/// The broker kind and the name of a channel contract; no name for an unresolved side.
pub fn parse(contract: &str) -> Option<(&str, Option<&str>)> {
    let rest = contract.strip_prefix(PREFIX)?;
    Some(match rest.split_once("::") {
        Some((broker, name)) => (broker, Some(name)),
        None => (rest, None),
    })
}

/// Whether `contract` is a channel side whose name could not be evaluated.
pub fn is_unresolved(contract: &str) -> bool {
    matches!(parse(contract), Some((_, None)))
}

/// `contract` as a person reads it: an unresolved channel side says so.
pub fn shown(contract: &str) -> String {
    match is_unresolved(contract) {
        true => format!("{contract} (unresolved)"),
        false => contract.to_string(),
    }
}

impl ContractKind for Channel {
    fn prefix(&self) -> &'static str {
        "channel"
    }

    fn extract(&self, code: &Code) -> Vec<Found> {
        let messages = code.pack().messages(code);
        // An inline handler is named after its broker kind and the channels it handles, in
        // the order found: `kafka orders.created`.
        let mut inline: HashMap<(&str, &str), (&str, Vec<&str>)> = HashMap::new();
        for m in &messages {
            if let lang::Role::Consumer(Handler::Inline { path, def }) = &m.role {
                let (_, names) = inline.entry((path, def)).or_insert((m.broker, Vec::new()));
                if let Some(name) = m.name.as_deref().filter(|n| !names.contains(n)) {
                    names.push(name);
                }
            }
        }
        messages
            .iter()
            .map(|m| {
                let (role, site) = match &m.role {
                    lang::Role::Producer => (
                        Role::Provider,
                        Site::Call {
                            path: m.path.clone(),
                            index: m.call,
                        },
                    ),
                    lang::Role::Consumer(Handler::Def { path, name }) => (
                        Role::Consumer,
                        Site::Def {
                            path: path.clone(),
                            name: name.clone(),
                        },
                    ),
                    lang::Role::Consumer(Handler::Inline { path, def }) => {
                        let (broker, names) = &inline[&(path.as_str(), def.as_str())];
                        let name = std::iter::once(*broker).chain(names.iter().copied());
                        let name = name.collect::<Vec<_>>().join(" ");
                        let (path, def) = (path.clone(), def.clone());
                        (Role::Consumer, Site::Inline { path, def, name })
                    }
                };
                Found {
                    role,
                    site,
                    contract: Some(contract(m.broker, m.name.as_deref())),
                    via: None,
                    method: None,
                    fragments: Vec::new(),
                }
            })
            .collect()
    }

    /// The consumer links to every producer of its channel, in any repository, at `exact`.
    /// Several producers or consumers are fan-in or fan-out, never ambiguity.
    fn link(
        &self,
        consumer: &Consumer,
        _from: &Repository,
        providers: &[(&Repository, &Provider)],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        if !matches!(parse(contract), Some((_, Some(_)))) {
            return Vec::new();
        }
        (providers.iter())
            .filter(|(_, p)| p.contract == *contract)
            .map(|(_, p)| Link {
                consumer: consumer.site.clone(),
                provider: p.site.clone(),
                contract: contract.clone(),
                level: Level::Exact,
                reason: contract.clone(),
                by: None,
                p: None,
            })
            .collect()
    }
}
