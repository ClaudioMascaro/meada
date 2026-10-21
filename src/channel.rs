//! Channels (spec #20): the queues and topics of a broker, identified by
//! `channel::<broker>::<name>`. This kind owns the forms of the message libraries of each
//! language (one module per language under `channel/`, each answering for its own pack only, as
//! the HTTP kind's readers do) and turns the calls they find into contracts and links every consumer of
//! a channel to every producer of it, in any repository of the workspace, its own included.
//! Producers are the provider side: messages go from the producing call to the consumer's
//! handler. A RabbitMQ publish to a named exchange is no channel's side by itself: the
//! bindings of the workspace route it to queues (`Routes`).

use std::collections::{BTreeMap, HashMap};

use crate::contract::{ContractKind, Found, Level, Link, Related, Role, Site};
use crate::document;
use crate::entry::EntryKind;
use crate::index::{self, Binding, Consumer, Export, Indexed, Provider, Publish};
use crate::resolve::Code;
use crate::shape::Shape;
use crate::workspace::Repository;

mod go;
pub(crate) mod message;
mod python;
mod rust;
mod ts;
use message::{Handler, Message, Side};

pub struct Channel;

/// The words for a producer and a consumer of a channel in `context`.
const SIDES: (&str, &str) = ("producer", "consumer");

/// The start of every channel contract identifier.
const PREFIX: &str = "channel::";

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
struct Routes<'a> {
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
fn call(m: &Message) -> Site {
    Site::Call {
        path: m.path.clone(),
        index: m.call,
    }
}

/// The broker kind and the name of a channel contract; no name for an unresolved side.
fn parse(contract: &str) -> Option<(&str, Option<&str>)> {
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
        Some(SIDES)
    }

    /// The producers a consumer's wildcard pattern matches and the wildcard consumers that
    /// match the channel, the publishes its bindings route to it, and the bindings themselves.
    fn related_sides<'e>(
        &self,
        exports: &'e [Export],
        export: &'e Export,
        contract: &str,
    ) -> Vec<Related<'e>> {
        let (producer, consumer) = SIDES;
        let routes = Routes::new(exports);
        let provided = (export.providers.iter())
            .filter(|p| p.contract != contract)
            .filter_map(|p| {
                Some(Related {
                    site: &p.site,
                    role: Role::Provider,
                    words: producer.into(),
                    matched: Some(fits(&p.contract, contract)?),
                    shape: p.shape.as_ref(),
                })
            });
        let published = export.publishes.iter().filter_map(|q| {
            Some(Related {
                site: &q.site,
                role: Role::Provider,
                words: producer.into(),
                matched: Some(routes.reached(q).remove(contract)?),
                shape: None,
            })
        });
        let consumed = export.consumers.iter().filter_map(|c| {
            let own = c.contract.as_ref().filter(|own| *own != contract)?;
            Some(Related {
                site: &c.site,
                role: Role::Consumer,
                words: consumer.into(),
                matched: Some(fits(contract, own)?),
                shape: c.shape.as_ref(),
            })
        });
        let bound = (export.bindings.iter())
            .filter(|b| b.contract == contract)
            .map(|b| Related {
                site: &b.site,
                role: Role::Binding,
                words: format!("binding {} {}", b.exchange, b.pattern),
                matched: None,
                shape: None,
            });
        provided
            .chain(published)
            .chain(consumed)
            .chain(bound)
            .collect()
    }

    fn unresolved(&self, contract: &str) -> bool {
        matches!(parse(contract), Some((_, None)))
    }

    fn parts<'c>(&self, contract: &'c str) -> (Option<&'c str>, Option<&'c str>) {
        parse(contract).map_or((None, None), |(broker, name)| (Some(broker), name))
    }

    fn extract(&self, code: &Code, _: &Indexed) -> Vec<Found> {
        // Each reader answers for its own pack only; the order is the order found.
        let messages: Vec<Message> = [
            ts::messages(code),
            python::messages(code),
            rust::messages(code),
            go::messages(code),
        ]
        .into_iter()
        .flatten()
        .collect();
        // An inline handler is named after its broker kind and the channels it handles, in
        // the order found: `kafka orders.created`.
        let mut inline: HashMap<(&str, &str), (&str, Vec<&str>)> = HashMap::new();
        for m in &messages {
            if let Side::Consumer(Handler::Inline { path, def }) = &m.role {
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
                    Side::Producer => (Role::Provider, call(m)),
                    // A publish whose routing key could not be evaluated is an unresolved
                    // producer.
                    Side::Publish { exchange } => match &m.name {
                        Some(key) => {
                            (contract, via, method) =
                                (None, Some(exchange.clone()), Some(key.clone()));
                            (Role::Publish, call(m))
                        }
                        None => (Role::Provider, call(m)),
                    },
                    Side::Binding { exchange, pattern } => {
                        (via, method) = (Some(exchange.clone()), Some(pattern.clone()));
                        (Role::Binding, call(m))
                    }
                    Side::Consumer(Handler::Def { path, name }) => (
                        Role::Consumer,
                        Site::Def {
                            path: path.clone(),
                            name: name.clone(),
                        },
                    ),
                    Side::Consumer(Handler::Inline { path, def }) => {
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

    /// The publish at `call` and each queue the bindings of the workspace route it to.
    fn at_call(&self, exports: &[Export], export: &Export, call: &str) -> Vec<String> {
        let publishes: Vec<_> = export.publishes.iter().filter(|q| q.site == call).collect();
        if publishes.is_empty() {
            return Vec::new();
        }
        let routes = Routes::new(exports);
        let mut lines = Vec::new();
        for publish in publishes {
            lines.push(format!("publishes {} {}", publish.exchange, publish.key));
            for (queue, (level, reason)) in routes.reached(publish) {
                lines.push(format!("produces {queue} [{}: {reason}]", level.name()));
            }
        }
        lines
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

    /// A consumer is a door, an unresolved side included: the handler still consumes from
    /// somewhere.
    fn entry(&self, role: Role, contract: &str) -> Option<EntryKind> {
        (role == Role::Consumer && parse(contract).is_some()).then_some(EntryKind::Channel)
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
fn fits(produced: &str, consumed: &str) -> Option<(Level, String)> {
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
fn reaching<'p>(
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
    use crate::contract::{ContractKind, Role};
    use crate::entry::EntryKind;
    use crate::index::{Binding, Consumer, Export, Provider, Publish};

    fn provider(contract: &str, site: &str) -> Provider {
        Provider {
            contract: contract.into(),
            site: site.into(),
            ..Provider::default()
        }
    }

    fn consumer(contract: &str, site: &str) -> Consumer {
        Consumer {
            contract: Some(contract.into()),
            site: site.into(),
            via: None,
            shape: None,
            method: None,
            fragments: Vec::new(),
            hops: Vec::new(),
        }
    }

    /// What `related_sides` says of `contract` over `exports[0]`, a line per side: its site,
    /// the side it shows as, its words, then `[<level>: <reason>]` when a name matched it.
    fn related(exports: &[Export], contract: &str) -> Vec<String> {
        let sides = Channel.related_sides(exports, &exports[0], contract);
        (sides.into_iter())
            .map(|r| {
                let (site, role, words) = (r.site, r.role.name(), r.words);
                match r.matched {
                    Some((level, reason)) => {
                        format!("{site} {role} {words} [{}: {reason}]", level.name())
                    }
                    None => format!("{site} {role} {words}"),
                }
            })
            .collect()
    }

    /// A NATS producer, a wildcard and a plain consumer of it, and a RabbitMQ publish whose
    /// exchange a binding routes to the queue `q`.
    fn workspace() -> [Export; 1] {
        [Export {
            providers: vec![provider(
                "channel::nats::orders.created",
                "r::a.ts::produce",
            )],
            consumers: vec![
                consumer("channel::nats::orders.*", "r::b.ts::watch"),
                consumer("channel::nats::orders.created", "r::b.ts::handle"),
            ],
            bindings: vec![Binding {
                contract: "channel::rabbitmq::q".into(),
                exchange: "ex".into(),
                pattern: "order.*".into(),
                site: "r::b.ts::bind".into(),
            }],
            publishes: vec![Publish {
                exchange: "ex".into(),
                key: "order.created".into(),
                site: "r::a.ts::pub".into(),
            }],
            ..Export::default()
        }]
    }

    /// The sides that reach a channel through another name are the wildcard consumers and
    /// producers, the publishes its bindings route and the bindings; its own sides are left to
    /// the engine.
    #[test]
    fn a_channel_is_reached_through_wildcards_publishes_and_bindings() {
        let exports = workspace();
        let wildcard = "[inferred: channel::nats::orders.created wildcard orders.*]";
        assert_eq!(
            related(&exports, "channel::nats::orders.created"),
            [format!("r::b.ts::watch consumer consumer {wildcard}")]
        );
        assert_eq!(
            related(&exports, "channel::nats::orders.*"),
            [format!("r::a.ts::produce provider producer {wildcard}")]
        );
        let routed = "[inferred: channel::rabbitmq::q binding ex order.*]";
        assert_eq!(
            related(&exports, "channel::rabbitmq::q"),
            [
                format!("r::a.ts::pub provider producer {routed}"),
                "r::b.ts::bind binding binding ex order.*".to_string(),
            ]
        );
        assert!(related(&exports, "channel::kafka::none").is_empty());
    }

    #[test]
    fn a_consumer_of_a_channel_is_an_entry_point_named_or_unresolved_and_a_provider_is_not() {
        for contract in ["channel::kafka::orders.created", "channel::kafka"] {
            assert_eq!(
                Channel.entry(Role::Consumer, contract),
                Some(EntryKind::Channel)
            );
            assert_eq!(Channel.entry(Role::Provider, contract), None);
        }
    }

    #[test]
    fn a_side_with_only_its_broker_prefix_is_unresolved() {
        assert!(Channel.unresolved("channel::kafka"));
        assert!(!Channel.unresolved("channel::kafka::orders.created"));
    }

    #[test]
    fn a_channel_ranks_against_its_broker_and_its_name() {
        assert_eq!(
            Channel.parts("channel::kafka::orders.created"),
            (Some("kafka"), Some("orders.created"))
        );
        assert_eq!(Channel.parts("channel::kafka"), (Some("kafka"), None));
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

    /// A publish at a call adds its own line and one per queue its bindings route it to; a
    /// call with no publish adds nothing.
    #[test]
    fn a_publish_at_a_call_adds_the_queues_it_reaches() {
        let exports = workspace();
        assert_eq!(
            Channel.at_call(&exports, &exports[0], "r::a.ts::pub"),
            [
                "publishes ex order.created",
                "produces channel::rabbitmq::q [inferred: channel::rabbitmq::q binding ex order.*]",
            ]
        );
        assert!(
            Channel
                .at_call(&exports, &exports[0], "r::a.ts::other")
                .is_empty()
        );
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

    /// An inline handler is named after its broker kind and the channels it consumes, in the
    /// order the pack found them.
    #[test]
    fn an_inline_handler_is_named_after_its_channels_in_the_order_found() {
        use crate::contract::Site;
        use crate::index::Indexed;
        use crate::lang::{LanguagePack, ts::TsJs};
        use crate::resolve::{Code, Resolver};
        use std::collections::BTreeMap;

        let src = r#"import { Kafka } from "kafkajs";
const kafka = new Kafka({ brokers: ["k:9092"] });
const consumer = kafka.consumer({ groupId: "g" });
export async function listen() {
  await consumer.subscribe({ topics: ["orders.created", "billing.invoices"] });
  await consumer.run({ eachMessage: async () => {} });
}
"#;
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let modules = TsJs.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &TsJs);
        let code = Code::new(&resolver, &files, &TsJs);
        let indexed = Indexed {
            manifests: &[],
            tsconfigs: &[],
            exported: &[],
            files: &["a.ts"],
        };
        let sites: Vec<_> = Channel
            .extract(&code, &indexed)
            .into_iter()
            .map(|f| match f.site {
                Site::Inline { name, .. } => name,
                _ => panic!("a consumer of an inline handler"),
            })
            .collect();
        assert_eq!(
            sites, ["kafka orders.created billing.invoices"; 2],
            "one site per topic, both named alike"
        );
    }
}
