//! Channels (spec #20): the queues and topics of a broker, identified by
//! `channel::<broker>::<name>`. Language packs report the producer and consumer calls of their
//! message libraries (`LanguagePack::messages`); this kind turns them into contracts and links
//! every consumer of a channel to every producer of it, in any repository of the workspace,
//! its own included. Producers are the provider side: messages go from the producing call to
//! the consumer's handler. Broker clusters are not modelled: one name of one broker kind is
//! one channel across the workspace.

use std::collections::{BTreeMap, HashMap};

use crate::contract::{ContractKind, Found, Level, Link, Role, Site};
use crate::index::{Binding, Consumer, Provider};
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

/// The contract of a publish to the named exchange `exchange` with routing key `key`: not a
/// queue (the bindings decide which queues it reaches), so it lives under its own broker kind
/// and never equals a queue's contract. A publish to the empty exchange is the queue `key`.
fn publication(exchange: &str, key: &str) -> String {
    contract("amqp-exchange", Some(&format!("{exchange}::{key}")))
}

/// The routing key of a `publication` to `exchange`.
fn routing_key<'c>(contract: &'c str, exchange: &str) -> Option<&'c str> {
    contract
        .strip_prefix(PREFIX)?
        .strip_prefix("amqp-exchange::")?
        .strip_prefix(exchange)?
        .strip_prefix("::")
}

/// Whether the topic-exchange `pattern` matches the routing `key`: words split at `.`, `*` is
/// exactly one word and `#` zero or more. Meada applies it whatever the exchange type.
fn pattern_matches(pattern: &str, key: &str) -> bool {
    fn go(pattern: &[&str], key: &[&str]) -> bool {
        match pattern.split_first() {
            None => key.is_empty(),
            Some((&"#", rest)) => (0..=key.len()).any(|n| go(rest, &key[n..])),
            Some((&"*", rest)) => !key.is_empty() && go(rest, &key[1..]),
            Some((word, rest)) => key.first() == Some(word) && go(rest, &key[1..]),
        }
    }
    let words = |text| Vec::from_iter(str::split(text, '.'));
    go(&words(pattern), &words(key))
}

/// The call a message was made at.
fn call(m: &lang::Message) -> Site {
    Site::Call {
        path: m.path.clone(),
        index: m.call,
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
                let mut contract = contract(m.broker, m.name.as_deref());
                let (mut via, mut method) = (None, None);
                let (role, site) = match &m.role {
                    lang::Role::Producer => (Role::Provider, call(m)),
                    lang::Role::Publish { exchange } => {
                        if let Some(key) = &m.name {
                            contract = publication(exchange, key);
                        }
                        (Role::Provider, call(m))
                    }
                    lang::Role::Binding { exchange, pattern } => {
                        (via, method) = (Some(exchange.clone()), Some(pattern.clone()));
                        (Role::Binding, call(m))
                    }
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
                    contract: Some(contract),
                    via,
                    method,
                    fragments: Vec::new(),
                }
            })
            .collect()
    }

    /// The consumer links to every producer of its channel, in any repository, at `exact`; a
    /// queue also to every publish whose exchange the workspace binds it to (see `matched`).
    /// Several producers or consumers are fan-in or fan-out, never ambiguity.
    fn link(
        &self,
        consumer: &Consumer,
        _from: &Repository,
        providers: &[(&Repository, &Provider)],
        bindings: &[&Binding],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        if !matches!(parse(contract), Some((_, Some(_)))) {
            return Vec::new();
        }
        matched(contract, providers, bindings)
            .into_iter()
            .map(|(provider, level, reason)| Link {
                consumer: consumer.site.clone(),
                provider,
                contract: contract.clone(),
                level,
                reason,
                by: None,
                p: None,
            })
            .collect()
    }
}

/// The producers (by site) that reach the channel `contract`, with the level and reason of
/// each: the producers of the same contract at `exact`, reason the contract; and, for each
/// binding of the queue, the publishes to its exchange whose routing key the pattern matches,
/// `exact` when the pattern is the key and `inferred` for a wildcard, the reason adding
/// `binding <exchange> <pattern>`. Of several matches the strongest level wins.
fn matched(
    contract: &str,
    providers: &[(&Repository, &Provider)],
    bindings: &[&Binding],
) -> Vec<(String, Level, String)> {
    let mut found: BTreeMap<&str, (Level, String)> = BTreeMap::new();
    for (_, p) in providers {
        let mut matches = Vec::new();
        if p.contract == contract {
            matches.push((Level::Exact, contract.to_string()));
        }
        for b in bindings.iter().filter(|b| b.contract == contract) {
            let Some(key) = routing_key(&p.contract, &b.exchange) else {
                continue;
            };
            if pattern_matches(&b.pattern, key) {
                let level = if b.pattern == key {
                    Level::Exact
                } else {
                    Level::Inferred
                };
                let reason = format!("{contract} binding {} {}", b.exchange, b.pattern);
                matches.push((level, reason));
            }
        }
        if let Some(best) = matches.into_iter().min() {
            found
                .entry(&p.site)
                .and_modify(|b| *b = best.clone().min(b.clone()))
                .or_insert(best);
        }
    }
    (found.into_iter())
        .map(|(site, (level, reason))| (site.to_string(), level, reason))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::pattern_matches;

    /// Topic-exchange rules: words split at `.`, `*` is one word, `#` zero or more.
    #[test]
    fn amqp_patterns_follow_topic_exchange_rules() {
        let table = [
            ("order.created", "order.created", true),
            ("order.created", "order.shipped", false),
            ("order.created", "order", false),
            ("order", "order.created", false),
            ("*.created", "order.created", true),
            ("*.created", "created", false),
            ("*.created", "a.b.created", false),
            ("order.*", "order", false),
            ("order.*.paid", "order.eu.paid", true),
            ("order.*.paid", "order.paid", false),
            ("#", "", true),
            ("#", "order.created.eu", true),
            ("order.#", "order", true),
            ("order.#", "order.created.eu", true),
            ("order.#", "orders.created", false),
            ("#.created", "created", true),
            ("#.created", "a.b.created", true),
            ("#.created", "a.b.shipped", false),
            ("a.#.z", "a.z", true),
            ("a.#.z", "a.b.c.z", true),
            ("a.#.z", "a.b.c", false),
            ("*.#", "a", true),
            ("#.#", "a.b", true),
        ];
        for (pattern, key, matches) in table {
            assert_eq!(pattern_matches(pattern, key), matches, "{pattern} on {key}");
        }
    }
}
