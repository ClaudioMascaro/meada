//! AsyncAPI 2.x and 3.0 (spec #21): every operation of a document is a side of a channel, on
//! the broker kind its server protocol names, with the payloads of its messages as the shape.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::openapi::{local, text};
use super::{Format, Problem, Role, Side, Summary, schema};
use crate::shape::Source;

/// What `doc`, the document at `path`, gives: its channel sides when its `asyncapi` field is a
/// 2.x or 3.0 version, else the format it has instead.
pub fn read(path: &str, doc: &Value) -> Summary {
    let version = doc.get("asyncapi").and_then(text);
    let Some(version) = version.filter(|v| supported(v)) else {
        let fields = ["asyncapi", "openapi", "swagger"];
        let found = fields
            .into_iter()
            .find_map(|field| Some(format!("{field} {}", text(doc.get(field)?)?)));
        return Summary::problem(Problem::AsyncFormat { found });
    };
    let operations = match version.starts_with("2.") {
        true => operations_v2(doc),
        false => operations_v3(doc),
    };
    // Each server with its protocol, lower case, when it has one.
    let servers: BTreeMap<&str, Option<String>> = (doc.get("servers").and_then(Value::as_object))
        .into_iter()
        .flatten()
        .map(|(name, server)| {
            let protocol = server.get("protocol").and_then(Value::as_str);
            (name.as_str(), protocol.map(str::to_lowercase))
        })
        .collect();
    let mut payloads: BTreeMap<(String, String, Role), Vec<&Value>> = BTreeMap::new();
    let (mut protocols, mut parameters) = (BTreeSet::new(), BTreeSet::new());
    let (mut no_servers, mut no_protocol, mut unknown) = (false, BTreeSet::new(), BTreeSet::new());
    for op in operations {
        if op.name.contains(['{', '}']) {
            parameters.insert(op.name);
            continue;
        }
        if servers.is_empty() {
            no_servers = true;
            continue;
        }
        // The servers the channel lists, else every server of the document.
        let used: Vec<(&str, &Option<String>)> = match op.servers.is_empty() {
            true => servers.iter().map(|(n, p)| (*n, p)).collect(),
            false => (op.servers.iter())
                .filter_map(|n| match servers.get_key_value(n.as_str()) {
                    Some((n, p)) => Some((*n, p)),
                    None => {
                        unknown.insert((op.name.clone(), n.clone()));
                        None
                    }
                })
                .collect(),
        };
        for (server, protocol) in used {
            let Some(protocol) = protocol else {
                no_protocol.insert(server.to_string());
                continue;
            };
            match broker(protocol) {
                Some(broker) => {
                    let key = (broker.to_string(), op.name.clone(), op.role);
                    let own = payloads.entry(key).or_default();
                    for payload in &op.payloads {
                        if !own.contains(payload) {
                            own.push(payload);
                        }
                    }
                }
                None => {
                    protocols.insert(protocol.clone());
                }
            }
        }
    }
    let mut external = BTreeSet::new();
    let mut work = schema::Work::default();
    let channels = (payloads.into_iter())
        .map(|((broker, name, role), payloads)| {
            let schema = match payloads[..] {
                [] => None,
                [one] => Some(one.clone()),
                _ => Some(json!({ "oneOf": payloads })),
            };
            let shape = schema.map(|schema| {
                let source = Source::Document(path.to_string());
                let (shape, refs) = schema::read(doc, &schema, source, &mut work);
                external.extend(refs);
                shape
            });
            Side {
                broker,
                name,
                role,
                shape,
            }
        })
        .collect();
    let problems = (external.into_iter())
        .map(|reference| Problem::ExternalRef { reference })
        .chain((protocols.into_iter()).map(|protocol| Problem::Protocol { protocol }))
        .chain((parameters.into_iter()).map(|channel| Problem::Parameters { channel }))
        .chain(no_servers.then_some(Problem::NoServers))
        .chain((no_protocol.into_iter()).map(|server| Problem::NoProtocol { server }))
        .chain(
            (unknown.into_iter())
                .map(|(channel, server)| Problem::UnknownServer { channel, server }),
        );
    Summary {
        format: Some(Format::Asyncapi),
        version: Some(version),
        title: doc.pointer("/info/title").and_then(text),
        channels,
        problems: problems.collect(),
        ..Summary::default()
    }
}

/// One operation of a document: the channel it acts on, which way, the names of the servers the
/// channel lists and the payload schemas of its messages.
struct Operation<'a> {
    name: String,
    role: Role,
    servers: Vec<String>,
    payloads: Vec<&'a Value>,
}

/// The operations of an AsyncAPI 2.x document: the key of a channel is its name, and
/// `subscribe` is the application sending messages, `publish` it receiving them.
fn operations_v2(doc: &Value) -> Vec<Operation<'_>> {
    let channels = doc.get("channels").and_then(Value::as_object);
    let mut found = Vec::new();
    for (name, item) in channels.into_iter().flatten() {
        let servers = (item.get("servers").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(|s| Some(s.as_str()?.to_string()))
            .collect::<Vec<_>>();
        for (verb, role) in [("subscribe", Role::Producer), ("publish", Role::Consumer)] {
            let Some(operation) = item.get(verb).filter(|o| o.is_object()) else {
                continue;
            };
            // A message, or a `oneOf` of them.
            let message = operation.get("message").and_then(|m| local(doc, m));
            let messages = match message {
                Some(m) => match m.get("oneOf").and_then(Value::as_array) {
                    Some(any) => any.iter().collect(),
                    None => vec![m],
                },
                None => Vec::new(),
            };
            found.push(Operation {
                name: name.clone(),
                role,
                servers: servers.clone(),
                payloads: payloads(doc, messages),
            });
        }
    }
    found
}

/// The operations of an AsyncAPI 3.0 document: the name of a channel is its `address`, `send`
/// is the application sending and `receive` it receiving. An operation that lists messages
/// keeps those; otherwise every message of its channel counts.
fn operations_v3(doc: &Value) -> Vec<Operation<'_>> {
    let operations = doc.get("operations").and_then(Value::as_object);
    let mut found = Vec::new();
    for operation in operations.into_iter().flat_map(|o| o.values()) {
        let role = match operation.get("action").and_then(Value::as_str) {
            Some("send") => Role::Producer,
            Some("receive") => Role::Consumer,
            _ => continue,
        };
        let Some(channel) = (operation.get("channel")).and_then(|c| local(doc, c)) else {
            continue;
        };
        let Some(name) = channel.get("address").and_then(Value::as_str) else {
            continue;
        };
        let servers = (channel.get("servers").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let reference = s.get("$ref")?.as_str()?;
                Some(reference.rsplit('/').next()?.to_string())
            })
            .collect();
        let listed = operation.get("messages").and_then(Value::as_array);
        let messages: Vec<&Value> = match listed {
            Some(listed) => listed.iter().collect(),
            None => (channel.get("messages").and_then(Value::as_object))
                .into_iter()
                .flat_map(|m| m.values())
                .collect(),
        };
        found.push(Operation {
            name: name.to_string(),
            role,
            servers,
            payloads: payloads(doc, messages),
        });
    }
    found
}

/// The payload schema of each message that has one; a message may be a local reference.
fn payloads<'a>(doc: &'a Value, messages: Vec<&'a Value>) -> Vec<&'a Value> {
    let messages = messages.into_iter().filter_map(|m| local(doc, m));
    messages.filter_map(|m| m.get("payload")).collect()
}

/// The broker kind of a server `protocol` (lower case).
fn broker(protocol: &str) -> Option<&'static str> {
    Some(match protocol {
        "kafka" | "kafka-secure" => "kafka",
        "amqp" | "amqps" => "amqp",
        "nats" => "nats",
        "redis" => "redis",
        "sqs" => "sqs",
        _ => return None,
    })
}

/// `2.x`, or `3.0` and any patch of it.
fn supported(version: &str) -> bool {
    let mut parts = version.split('.');
    match parts.next() {
        Some("2") => true,
        Some("3") => parts.next() == Some("0"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{Format, Problem, Role, read};

    /// The sides of the document `src` as `broker role name`, with its problems.
    fn sides(src: &str) -> (Vec<String>, Vec<Problem>) {
        let summary = read("asyncapi.yaml", src.as_bytes());
        let sides = summary.channels.iter().map(|s| {
            let role = if s.role == Role::Producer {
                "producer"
            } else {
                "consumer"
            };
            format!("{} {role} {}", s.broker, s.name)
        });
        (sides.collect(), summary.problems)
    }

    const V2: &str = "
asyncapi: 2.6.0
info: {title: Orders events}
servers:
  prod: {url: 'kafka.example.com:9092', protocol: kafka-secure}
channels:
  orders.created:
    subscribe:
      message:
        payload: {type: object, properties: {id: {type: string}}}
  payments.requested:
    publish:
      message:
        payload: {type: object, properties: {amount: {type: number}}}
";

    #[test]
    fn version_two_reads_subscribe_as_producer_and_publish_as_consumer() {
        let (sides, problems) = sides(V2);
        assert_eq!(
            sides,
            [
                "kafka producer orders.created",
                "kafka consumer payments.requested"
            ]
        );
        assert_eq!(problems, []);
        let summary = read("asyncapi.yaml", V2.as_bytes());
        assert_eq!(summary.format, Some(Format::Asyncapi));
        assert_eq!(summary.version.as_deref(), Some("2.6.0"));
        assert_eq!(summary.title.as_deref(), Some("Orders events"));
        let fields = |n: usize| summary.channels[n].shape.as_ref().unwrap().fields.clone();
        assert_eq!(fields(0), ["id".to_string()].into());
        assert_eq!(fields(1), ["amount".to_string()].into());
    }

    const V3: &str = "
asyncapi: 3.0.0
servers:
  broker: {host: 'rabbit.example.com', protocol: amqp}
channels:
  invoices:
    address: invoices.queue
    messages:
      issued: {payload: {type: object, properties: {number: {type: string}}}}
      voided: {payload: {type: object, properties: {reason: {type: string}}}}
  shipments:
    address: shipments.queue
    messages:
      requested: {$ref: '#/components/messages/Shipment'}
operations:
  sendInvoice: {action: send, channel: {$ref: '#/channels/invoices'}}
  onShipment: {action: receive, channel: {$ref: '#/channels/shipments'}}
components:
  messages:
    Shipment: {payload: {type: object, properties: {parcel: {type: string}}}}
";

    #[test]
    fn version_three_reads_the_action_and_the_channel_address() {
        let (sides, problems) = sides(V3);
        assert_eq!(
            sides,
            [
                "amqp producer invoices.queue",
                "amqp consumer shipments.queue"
            ]
        );
        assert_eq!(problems, []);
    }

    #[test]
    fn a_channel_sent_to_and_received_from_counts_once() {
        let src = "asyncapi: 2.6.0\nservers: {s: {url: k, protocol: kafka}}\nchannels:\n  a:\n    \
                   subscribe: {}\n    publish: {}\n  b:\n    publish: {}\n";
        let summary = read("asyncapi.yaml", src.as_bytes());
        assert_eq!(summary.channels.len(), 3);
        assert_eq!(summary.shown().unwrap(), "asyncapi 2.6.0, channels: 2");
    }

    #[test]
    fn several_messages_give_the_union_of_their_fields() {
        let summary = read("asyncapi.yaml", V3.as_bytes());
        let fields = |n: usize| {
            let shape = summary.channels[n].shape.as_ref().unwrap();
            shape.fields.iter().cloned().collect::<Vec<_>>()
        };
        assert_eq!(fields(0), ["number", "reason"]);
        // A message behind a local reference.
        assert_eq!(fields(1), ["parcel"]);
    }

    #[test]
    fn an_operation_naming_its_messages_keeps_only_those() {
        let src = V3.replace(
            "sendInvoice: {action: send, channel: {$ref: '#/channels/invoices'}}",
            "sendInvoice: {action: send, channel: {$ref: '#/channels/invoices'}, \
             messages: [{$ref: '#/channels/invoices/messages/voided'}]}",
        );
        let summary = read("asyncapi.yaml", src.as_bytes());
        let shape = summary.channels[0].shape.as_ref().unwrap();
        assert_eq!(shape.fields.iter().cloned().collect::<Vec<_>>(), ["reason"]);
    }

    #[test]
    fn the_server_protocol_gives_the_broker_kind() {
        for (protocol, broker) in [
            ("kafka", Some("kafka")),
            ("kafka-secure", Some("kafka")),
            ("amqp", Some("amqp")),
            ("AMQPS", Some("amqp")),
            ("nats", Some("nats")),
            ("redis", Some("redis")),
            ("sqs", Some("sqs")),
            ("mqtt", None),
            ("ws", None),
        ] {
            let src = format!(
                "asyncapi: 2.0.0\nservers: {{s: {{url: x, protocol: {protocol}}}}}\n\
                 channels:\n  a:\n    subscribe: {{}}\n"
            );
            let (sides, problems) = sides(&src);
            match broker {
                Some(broker) => {
                    assert_eq!(sides, [format!("{broker} producer a")], "{protocol}");
                    assert_eq!(problems, [], "{protocol}");
                }
                None => {
                    assert_eq!(sides, Vec::<String>::new(), "{protocol}");
                    let protocol = protocol.to_string();
                    assert_eq!(problems, [Problem::Protocol { protocol }]);
                }
            }
        }
    }

    #[test]
    fn a_channel_lists_its_servers_or_uses_them_all() {
        let src = "
asyncapi: 2.6.0
servers:
  logs: {url: k, protocol: kafka}
  jobs: {url: a, protocol: amqp}
  edge: {url: m, protocol: mqtt}
channels:
  both:
    subscribe: {}
  only-jobs:
    servers: [jobs]
    subscribe: {}
";
        let (sides, problems) = sides(src);
        assert_eq!(
            sides,
            [
                "amqp producer both",
                "amqp producer only-jobs",
                "kafka producer both"
            ]
        );
        // `both` uses every server, so the unsupported one is reported.
        assert_eq!(
            problems,
            [Problem::Protocol {
                protocol: "mqtt".into()
            }]
        );
    }

    #[test]
    fn channels_without_a_server_protocol_are_ignored_and_reported() {
        // No root `servers`: no channel has a broker kind.
        let src = "asyncapi: 2.6.0\nchannels:\n  a:\n    subscribe: {}\n";
        assert_eq!(sides(src), (vec![], vec![Problem::NoServers]));
        // A server without `protocol`, and a channel naming a server the document lacks.
        let src = "
asyncapi: 2.6.0
servers:
  bare: {url: k}
  logs: {url: k, protocol: kafka}
channels:
  a:
    servers: [bare]
    subscribe: {}
  b:
    servers: [gone, logs]
    subscribe: {}
";
        let server = |s: &str| s.to_string();
        assert_eq!(
            sides(src),
            (
                vec!["kafka producer b".to_string()],
                vec![
                    Problem::NoProtocol {
                        server: server("bare")
                    },
                    Problem::UnknownServer {
                        channel: "b".into(),
                        server: server("gone")
                    },
                ]
            )
        );
        // 3.0: a channel's servers are references.
        let src = "
asyncapi: 3.0.0
channels:
  c: {address: c.queue, servers: [{$ref: '#/servers/gone'}]}
operations:
  o: {action: send, channel: {$ref: '#/channels/c'}}
";
        assert_eq!(sides(src), (vec![], vec![Problem::NoServers]));
        let src = format!("{src}servers:\n  real: {{host: h, protocol: amqp}}\n");
        assert_eq!(
            sides(&src),
            (
                vec![],
                vec![Problem::UnknownServer {
                    channel: "c.queue".into(),
                    server: "gone".into()
                }]
            )
        );
        // A document without channels ignores nothing.
        assert_eq!(sides("asyncapi: 3.0.0\n"), (vec![], vec![]));
    }

    #[test]
    fn a_channel_with_parameters_is_ignored_and_reported() {
        let src = "
asyncapi: 2.6.0
servers: {s: {url: k, protocol: kafka}}
channels:
  'orders.{region}.created':
    subscribe: {}
  plain:
    publish: {}
";
        let (sides, problems) = sides(src);
        assert_eq!(sides, ["kafka consumer plain"]);
        assert_eq!(
            problems,
            [Problem::Parameters {
                channel: "orders.{region}.created".into()
            }]
        );
    }

    #[test]
    fn other_versions_are_reported_and_ignored() {
        for (src, found) in [
            ("asyncapi: 1.2.0\n", Some("asyncapi 1.2.0")),
            ("asyncapi: 3.1.0\n", Some("asyncapi 3.1.0")),
            ("asyncapi: 4.0.0\n", Some("asyncapi 4.0.0")),
        ] {
            let summary = read("asyncapi.yaml", src.as_bytes());
            let found = found.map(String::from);
            assert_eq!(summary.problems, [Problem::AsyncFormat { found }], "{src}");
            assert_eq!(summary.format, None);
        }
        // A file named like an AsyncAPI document with neither format field.
        let summary = read("asyncapi.yaml", b"swagger: '2.0'\n");
        let found = Some("swagger 2.0".to_string());
        assert_eq!(summary.problems, [Problem::AsyncFormat { found }]);
    }

    #[test]
    fn external_references_in_a_payload_are_problems_of_the_document() {
        let src = "
asyncapi: 2.6.0
servers: {s: {url: k, protocol: kafka}}
channels:
  a:
    subscribe:
      message:
        payload: {properties: {b: {$ref: 'b.yaml#/B'}}}
";
        let (_, problems) = sides(src);
        assert_eq!(
            problems,
            [Problem::ExternalRef {
                reference: "b.yaml#/B".into()
            }]
        );
    }
}
