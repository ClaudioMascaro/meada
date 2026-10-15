//! The message libraries of the TypeScript and JavaScript pack (spec #20). A form is a method
//! called on a client whose origin (`Code::origin`) is the library's own factory, imported
//! from the library's package: a `send` on anything else is no producer.

use std::collections::BTreeMap;

use crate::lang::{Handler, Message, Part, Role, Value, literal, property};
use crate::resolve::Code;

const KAFKAJS: &str = "kafkajs";
const KAFKA_PRODUCER: &str = "new Kafka().producer()";
const KAFKA_CONSUMER: &str = "new Kafka().consumer()";

const AMQPLIB: &str = "amqplib";
/// What a channel of amqplib is, from the default import, a namespace or a named `connect`.
const AMQP_CHANNELS: [&str; 4] = [
    "default.connect().createChannel()",
    "default.connect().createConfirmChannel()",
    "connect().createChannel()",
    "connect().createConfirmChannel()",
];

pub(super) fn messages(code: &Code) -> Vec<Message> {
    let mut out = Vec::new();
    // kafkajs subscriptions, each with the consumer binding it is made on.
    let mut subscriptions = Vec::new();
    // The handler `run` gives each kafkajs consumer binding; the first `run` counts.
    let mut handlers = BTreeMap::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let Some((receiver, method)) = call.callee.rsplit_once('.') else {
                continue;
            };
            if !matches!(
                method,
                "send" | "subscribe" | "run" | "sendToQueue" | "consume" | "publish" | "bindQueue"
            ) {
                continue;
            }
            let Some(origin) = code.origin(path, &call.scope, receiver) else {
                continue;
            };
            match (origin.package, origin.expr.as_str(), method) {
                (KAFKAJS, KAFKA_PRODUCER, "send") => {
                    for (site, at, names) in code.expand(path, n, &|a| names(a, &["topic"])) {
                        out.extend(names.into_iter().map(|name| Message {
                            broker: "kafka",
                            name,
                            path: site.into(),
                            call: at,
                            role: Role::Producer,
                        }));
                    }
                }
                (KAFKAJS, KAFKA_CONSUMER, "subscribe") => {
                    subscriptions.push((path, n, origin.binding));
                }
                (KAFKAJS, KAFKA_CONSUMER, "run") => {
                    if let (Some(binding), Some(handler)) = (origin.binding, run(code, path, n)) {
                        handlers.entry(binding).or_insert(handler);
                    }
                }
                (AMQPLIB, expr, "sendToQueue") if AMQP_CHANNELS.contains(&expr) => {
                    for (site, at, queue) in code.expand(path, n, &|a| queue(a, 0)) {
                        out.push(Message {
                            broker: "amqp",
                            name: queue,
                            path: site.into(),
                            call: at,
                            role: Role::Producer,
                        });
                    }
                }
                (AMQPLIB, expr, "consume") if AMQP_CHANNELS.contains(&expr) => {
                    let handler = handler(code, path, n, 1);
                    for (site, at, queue) in code.expand(path, n, &|a| queue(a, 0)) {
                        out.push(Message {
                            broker: "amqp",
                            name: queue,
                            path: site.into(),
                            call: at,
                            role: Role::Consumer(handler.clone()),
                        });
                    }
                }
                (AMQPLIB, expr, "publish") if AMQP_CHANNELS.contains(&expr) => {
                    let read = |a: &[Value]| Some((queue(a, 0)?, queue(a, 1)?));
                    for (site, at, (exchange, key)) in code.expand(path, n, &read) {
                        // The empty exchange is the queue named by the key; one that cannot
                        // be evaluated leaves the producer unresolved.
                        let (name, role) = match exchange.as_deref() {
                            Some("") => (key, Role::Producer),
                            Some(exchange) => (
                                key,
                                Role::Publish {
                                    exchange: exchange.into(),
                                },
                            ),
                            None => (None, Role::Producer),
                        };
                        out.push(Message {
                            broker: "amqp",
                            name,
                            path: site.into(),
                            call: at,
                            role,
                        });
                    }
                }
                (AMQPLIB, expr, "bindQueue") if AMQP_CHANNELS.contains(&expr) => {
                    let read = |a: &[Value]| Some((queue(a, 0)?, queue(a, 1)?, queue(a, 2)?));
                    for (site, at, bound) in code.expand(path, n, &read) {
                        // A binding that cannot be fully evaluated is dropped.
                        if let (Some(queue), Some(exchange), Some(pattern)) = bound {
                            out.push(Message {
                                broker: "amqp",
                                name: Some(queue),
                                path: site.into(),
                                call: at,
                                role: Role::Binding { exchange, pattern },
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for (path, n, binding) in subscriptions {
        let read = |a: &[Value]| names(a, &["topics", "topic"]);
        for (site, at, names) in code.expand(path, n, &read) {
            // With no `run` handler Meada can read, the function that subscribes handles.
            let handler = binding.and_then(|b| handlers.get(&b)).cloned();
            let handler = handler.unwrap_or_else(|| Handler::Def {
                path: site.into(),
                name: code.files()[site].calls[at].scope.clone(),
            });
            out.extend(names.into_iter().map(|name| Message {
                broker: "kafka",
                name,
                path: site.into(),
                call: at,
                role: Role::Consumer(handler.clone()),
            }));
        }
    }
    out
}

/// The handler a kafkajs `run({ eachMessage })` or `run({ eachBatch })` gives, when it is a
/// function written in place or a definition of the repository.
fn run<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Option<Handler> {
    let scope = &code.files()[path].calls[n].scope;
    let Some(Value::Object(options)) = code.args(path, n)?.first() else {
        return None;
    };
    let handler = ["eachMessage", "eachBatch"]
        .iter()
        .find_map(|key| property(options, key).ok().flatten())?;
    function(code, path, scope, handler)
}

/// The handler of a `consume` call: its argument `at`, else the function that calls it.
fn handler<'a>(code: &Code<'a>, path: &'a str, n: usize, at: usize) -> Handler {
    let call = &code.files()[path].calls[n];
    let given = code.args(path, n).and_then(|a| a.get(at));
    given
        .and_then(|v| function(code, path, &call.scope, v))
        .unwrap_or_else(|| Handler::Def {
            path: path.into(),
            name: call.scope.clone(),
        })
}

/// A function written in place or a definition of the repository, as a handler.
fn function<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
) -> Option<Handler> {
    match value {
        Value::Function(def) => Some(Handler::Inline {
            path: path.into(),
            def: def.clone(),
        }),
        Value::Ref(name) => code.definition(path, scope, name).map(|t| Handler::Def {
            path: t.path.into(),
            name: t.name.into(),
        }),
        _ => None,
    }
}

/// The queue or exchange name argument `at` of a call: `None` when it depends on a parameter,
/// `Some(None)` when it is not a literal string.
fn queue(args: &[Value], at: usize) -> Option<Option<String>> {
    args.get(at).map_or(Some(None), name)
}

/// The channel names the options object a call takes first gives under the first of `keys`
/// it sets, a list giving several. `None` when one depends on a parameter (a client wrapper);
/// a name that is not a literal string is unresolved (`None` in the list).
fn names(args: &[Value], keys: &[&str]) -> Option<Vec<Option<String>>> {
    let options = match args.first() {
        Some(Value::Object(properties)) => properties,
        Some(Value::Ref(_)) => return None,
        _ => return Some(vec![None]),
    };
    let mut given = None;
    for key in keys {
        given = property(options, key).ok()?;
        if given.is_some() {
            break;
        }
    }
    match given {
        Some(Value::List(items)) => items.iter().map(name).collect(),
        Some(value) => Some(vec![name(value)?]),
        None => Some(vec![None]),
    }
}

/// A channel name as evaluated: `None` when it depends on a parameter, `Some(None)` when it is
/// not a literal string (an environment read, a runtime value, a regular expression).
fn name(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Ref(_) => None,
        Value::Str(parts) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        Value::Str(parts) => Some(literal(parts)),
        _ => Some(None),
    }
}
