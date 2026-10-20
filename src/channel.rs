//! Channels (spec #20): the queues and topics of a broker, identified by
//! `channel::<broker>::<name>`. Language packs report the producer and consumer calls of their
//! message libraries (`LanguagePack::messages`); this kind turns them into contracts and links
//! every consumer of a channel to every producer of it, in any repository of the workspace,
//! its own included. Producers are the provider side: messages go from the producing call to
//! the consumer's handler. A RabbitMQ publish to a named exchange is no channel's side by
//! itself: the bindings of the workspace route it to queues (`Routes`).

use std::collections::{BTreeMap, HashMap};

use crate::contract::{ContractKind, Found, Level, Link, Role, Site};
use crate::document;
use crate::index::{self, Binding, Consumer, Export, Provider, Publish};
use crate::lang::{self, Handler};
use crate::resolve::Code;
use crate::shape::Shape;
use crate::workspace::Repository;

pub struct Channel;

/// The start of every channel contract identifier.
pub const PREFIX: &str = "channel::";

/// The contract identifier of the channel `name` of `broker`, kept as evaluated. A side whose
/// name could not be evaluated is `channel::<broker>`: it names its broker kind, never links
/// and is not an open item.
// ponytail: broker clusters are not modelled, so one name of one broker kind is one channel
// across the workspace; a connection identity in the contract would split them.
pub fn contract(broker: &str, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{PREFIX}{broker}::{name}"),
        None => format!("{PREFIX}{broker}"),
    }
}

/// The RabbitMQ routing of the workspace: every binding and every publish to a named
/// exchange, from any repository, since the broker is shared.
pub struct Routes<'a> {
    pub bindings: Vec<&'a Binding>,
    pub publishes: Vec<&'a Publish>,
}

impl<'a> Routes<'a> {
    pub fn new(exports: &'a [Export]) -> Self {
        Self {
            bindings: exports.iter().flat_map(|e| &e.bindings).collect(),
            publishes: exports.iter().flat_map(|e| &e.publishes).collect(),
        }
    }

    /// The queues `publish` reaches, each with the level and reason of the strongest binding
    /// that routes it there.
    pub fn reached(&self, publish: &Publish) -> BTreeMap<&'a str, (Level, String)> {
        let mut found = BTreeMap::new();
        for b in self.bindings.iter().copied() {
            if let Some(matched) = routed(publish, b) {
                strongest(&mut found, &b.contract, matched);
            }
        }
        found
    }
}

/// Whether the topic-exchange `pattern` matches the routing `key`: words split at `.`, `*` is
/// exactly one word and `#` zero or more.
// ponytail: an exchange's type (`assertExchange`) is not read, so every pattern follows
// topic-exchange rules, a direct or fanout exchange included.
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

impl ContractKind for Channel {
    fn prefix(&self) -> &'static str {
        "channel"
    }

    fn sides(&self) -> Option<(&'static str, &'static str)> {
        Some(("producer", "consumer"))
    }

    fn unresolved(&self, contract: &str) -> bool {
        matches!(parse(contract), Some((_, None)))
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
                let mut contract = Some(contract(m.broker, m.name.as_deref()));
                let (mut via, mut method) = (None, None);
                let (role, site) = match &m.role {
                    lang::Role::Producer => (Role::Provider, call(m)),
                    // A publish whose routing key could not be evaluated is an unresolved
                    // producer.
                    lang::Role::Publish { exchange } => match &m.name {
                        Some(key) => {
                            (contract, via, method) =
                                (None, Some(exchange.clone()), Some(key.clone()));
                            (Role::Publish, call(m))
                        }
                        None => (Role::Provider, call(m)),
                    },
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
                    contract,
                    via,
                    method,
                    fragments: Vec::new(),
                    shape: None,
                }
            })
            .collect()
    }

    /// Adds the channel sides of the repository's AsyncAPI documents (spec #21). A document side
    /// with the contract of a code side of the same direction merges into it: the code keeps the
    /// handler or the producing call, and takes the document's payload shape. Any other is a side
    /// of its own, without a handler, at `<repository>::<document>::<broker> send|receive
    /// <name>`; a side an earlier document (by path) already lists adds nothing.
    fn declare(&self, _repo: &Repository, export: &mut Export) {
        for doc in &export.documents {
            for side in &doc.summary.channels {
                let contract = side.contract();
                let merged = match side.role {
                    document::Role::Producer => merge(
                        (export.providers.iter_mut())
                            .filter(|p| p.contract == contract)
                            .map(|p| &mut p.shape),
                        &side.shape,
                    ),
                    document::Role::Consumer => merge(
                        (export.consumers.iter_mut())
                            .filter(|c| c.contract.as_ref() == Some(&contract))
                            .map(|c| &mut c.shape),
                        &side.shape,
                    ),
                };
                if merged {
                    continue;
                }
                let site = format!("{}::{}::{}", export.repository, doc.path, side.label());
                let shape = side.shape.clone();
                match side.role {
                    document::Role::Producer => export.providers.push(Provider {
                        contract,
                        site,
                        shape,
                        ..Provider::default()
                    }),
                    document::Role::Consumer => export.consumers.push(Consumer {
                        contract: Some(contract),
                        site,
                        via: None,
                        shape,
                        method: None,
                        fragments: Vec::new(),
                        hops: Vec::new(),
                    }),
                }
            }
        }
        index::sort_providers(&mut export.providers);
        index::sort_consumers(&mut export.consumers);
    }

    /// The consumer links to every producer that reaches its channel (`reaching`), in any
    /// repository. Several producers or consumers are fan-in or fan-out, never ambiguity.
    fn link(
        &self,
        consumer: &Consumer,
        _from: &Repository,
        providers: &[(&Repository, &Provider)],
        exports: &[Export],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        let providers = providers.iter().map(|&(_, p)| p);
        (reaching(contract, providers, &Routes::new(exports)).into_iter())
            .map(|(provider, (level, reason))| Link {
                consumer: consumer.site.clone(),
                provider: provider.to_string(),
                contract: contract.clone(),
                level,
                reason,
                by: None,
                p: None,
                drift: None,
            })
            .collect()
    }
}

/// Gives each code side a document side merges into (`own`, their shapes) the document's
/// `shape`, unless it has one; whether there was any.
fn merge<'a>(own: impl Iterator<Item = &'a mut Option<Shape>>, shape: &Option<Shape>) -> bool {
    let mut any = false;
    for own in own {
        any = true;
        if own.is_none() {
            own.clone_from(shape);
        }
    }
    any
}

/// How a producer of the channel `produced` reaches a consumer of the channel `consumed`, the
/// one place a name match is decided: at `exact` when they are the same channel, reason the
/// contract; at `inferred` when the consumer's name is a wildcard pattern of the same broker
/// kind that matches the producer's (`wildcard`), reason `<produced> wildcard <pattern>`. An
/// unresolved side reaches nothing.
pub fn fits(produced: &str, consumed: &str) -> Option<(Level, String)> {
    let (Some((broker, Some(name))), Some((kind, Some(pattern)))) =
        (parse(produced), parse(consumed))
    else {
        return None;
    };
    if produced == consumed {
        return Some((Level::Exact, consumed.to_string()));
    }
    let reason = || format!("{produced} wildcard {pattern}");
    (kind == broker && wildcard(broker, pattern, name)).then(|| (Level::Inferred, reason()))
}

/// How `binding` routes `publish` to its queue: when the exchanges are the same and the
/// pattern matches the routing key (`pattern_matches`), at `exact` when the pattern is the
/// key and `inferred` for a wildcard, reason `<queue contract> binding <exchange> <pattern>`.
fn routed(publish: &Publish, binding: &Binding) -> Option<(Level, String)> {
    if publish.exchange != binding.exchange || !pattern_matches(&binding.pattern, &publish.key) {
        return None;
    }
    let level = match binding.pattern == publish.key {
        true => Level::Exact,
        false => Level::Inferred,
    };
    let (exchange, pattern) = (&binding.exchange, &binding.pattern);
    Some((
        level,
        format!("{} binding {exchange} {pattern}", binding.contract),
    ))
}

/// The producers that reach the channel `contract`, by site, with the level and reason of
/// each: the providers that `fit` it, and the publishes its bindings route to it (`routed`).
/// Of several matches to one producer the strongest wins.
pub fn reaching<'p>(
    contract: &str,
    providers: impl IntoIterator<Item = &'p Provider>,
    routes: &Routes<'p>,
) -> BTreeMap<&'p str, (Level, String)> {
    let mut found = BTreeMap::new();
    for p in providers {
        if let Some(matched) = fits(&p.contract, contract) {
            strongest(&mut found, &p.site, matched);
        }
    }
    for b in routes.bindings.iter().filter(|b| b.contract == contract) {
        for p in routes.publishes.iter().copied() {
            if let Some(matched) = routed(p, b) {
                strongest(&mut found, &p.site, matched);
            }
        }
    }
    found
}

/// Keeps the strongest of the matches to `key`.
fn strongest<'k>(
    found: &mut BTreeMap<&'k str, (Level, String)>,
    key: &'k str,
    matched: (Level, String),
) {
    if found.get(key).is_none_or(|known| matched < *known) {
        found.insert(key, matched);
    }
}

/// Whether the consumer's `pattern` matches the producer's `name` through a wildcard of the
/// broker kind. The one place a broker's own matching rules live; RabbitMQ's are the bindings
/// (`pattern_matches`), which route through an exchange.
fn wildcard(broker: &str, pattern: &str, name: &str) -> bool {
    match broker {
        "nats" => nats(pattern, name),
        _ => false,
    }
}

/// NATS subjects are dot-separated tokens: `*` stands for exactly one token, and `>` as the
/// last token for one or more. A subject holding a wildcard token is a pattern, which no
/// message is published to, so it matches nothing.
fn nats(pattern: &str, subject: &str) -> bool {
    if subject.split('.').any(|t| t == "*" || t == ">") {
        return false;
    }
    let mut subject = subject.split('.');
    let mut pattern = pattern.split('.').peekable();
    while let Some(token) = pattern.next() {
        match (token, subject.next()) {
            (">", Some(_)) => return pattern.peek().is_none(),
            ("*", Some(_)) => {}
            (token, Some(t)) if token == t => {}
            _ => return false,
        }
    }
    subject.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::{Channel, nats, pattern_matches};
    use crate::contract::ContractKind;

    #[test]
    fn a_side_with_only_its_broker_prefix_is_unresolved() {
        assert!(Channel.unresolved("channel::kafka"));
        assert!(!Channel.unresolved("channel::kafka::orders.created"));
    }

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

    #[test]
    fn nats_subjects_match_through_star_and_greater_than() {
        for (pattern, subject, matches) in [
            ("orders.*", "orders.created", true),
            ("orders.*", "orders.eu.created", false),
            ("orders.*", "orders", false),
            ("orders.>", "orders.created", true),
            ("orders.>", "orders.eu.created", true),
            ("orders.>", "orders", false),
            (">", "orders", true),
            (">", "orders.eu.created", true),
            ("*.created", "orders.created", true),
            ("*.created", "orders.eu.created", false),
            ("orders.*.created", "orders.eu.created", true),
            ("orders.*.created", "orders.created", false),
            ("orders.>.created", "orders.eu.created", false),
            ("orders.created", "orders.created", true),
            ("orders.created", "orders.created.late", false),
            ("orders.created.late", "orders.created", false),
            ("orders.*", "orders.>", false),
            (">", "orders.*", false),
        ] {
            assert_eq!(
                nats(pattern, subject),
                matches,
                "{pattern} against {subject}"
            );
        }
    }
}
