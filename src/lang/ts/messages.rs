//! The message libraries of the TypeScript and JavaScript pack (spec #20). A form is a method
//! called on a client whose origin (`Code::origin`) is the library's own factory, imported
//! from the library's package: a `send` on anything else is no producer.

use std::collections::BTreeMap;

use crate::lang::{Handler, Message, Part, Role, Value, literal, property};
use crate::resolve::Code;

const KAFKAJS: &str = "kafkajs";
const KAFKA_PRODUCER: &str = "new Kafka().producer()";
const KAFKA_CONSUMER: &str = "new Kafka().consumer()";
const AWS_SQS: &str = "@aws-sdk/client-sqs";
const SQS_CONSUMER: &str = "sqs-consumer";
const NATS: &str = "nats";
const NATS_CONNECTION: &str = "connect()";
const AMQPLIB: &str = "amqplib";
/// What a channel of amqplib is, from the default import, a namespace or a named `connect`.
const AMQP_CHANNELS: [&str; 4] = [
    "default.connect().createChannel()",
    "default.connect().createConfirmChannel()",
    "connect().createChannel()",
    "connect().createConfirmChannel()",
];

/// What a form reads of a call's arguments: the channel names, see `names`.
type Read<'a> = &'a dyn Fn(&[Value]) -> Option<Vec<Option<String>>>;

pub(super) fn messages(code: &Code) -> Vec<Message> {
    let mut out = Vec::new();
    // kafkajs subscriptions, each with the consumer binding it is made on.
    let mut subscriptions = Vec::new();
    // The handler `run` gives each kafkajs consumer binding; the first `run` counts.
    let mut handlers = BTreeMap::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let (receiver, method) = (call.callee.rsplit_once('.')).unwrap_or(("", &call.callee));
            // A library form is a method of a client, or a class or function called itself.
            let chain = match method {
                "send" | "subscribe" | "run" | "publish" | "sendToQueue" | "consume"
                | "bindQueue"
                    if !receiver.is_empty() =>
                {
                    receiver
                }
                "SendMessageCommand" | "ReceiveMessageCommand" | "create" => &call.callee,
                _ => continue,
            };
            let Some(origin) = code.origin(path, &call.scope, chain) else {
                continue;
            };
            let produce = |broker, read: Read| {
                (code.expand(path, n, read).into_iter())
                    .flat_map(|(site, at, names)| {
                        names.into_iter().map(move |name| Message {
                            broker,
                            name,
                            path: site.into(),
                            call: at,
                            role: Role::Producer,
                        })
                    })
                    .collect::<Vec<_>>()
            };
            // The handler of the call itself (its `option` of argument `arg`), else the
            // function the call is in.
            let consume = |broker, read: Read, handler: Option<Handler>| {
                (code.expand(path, n, read).into_iter())
                    .flat_map(|(site, at, names)| {
                        let handler = handler.clone().unwrap_or_else(|| Handler::Def {
                            path: site.into(),
                            name: code.files()[site].calls[at].scope.clone(),
                        });
                        names.into_iter().map(move |name| Message {
                            broker,
                            name,
                            path: site.into(),
                            call: at,
                            role: Role::Consumer(handler.clone()),
                        })
                    })
                    .collect::<Vec<_>>()
            };
            match (origin.package, origin.expr.as_str(), method) {
                (KAFKAJS, KAFKA_PRODUCER, "send") => {
                    out.extend(produce("kafka", &|a| names(a, &["topic"])));
                }
                (KAFKAJS, KAFKA_CONSUMER, "subscribe") => {
                    subscriptions.push((path, n, origin.binding));
                }
                (KAFKAJS, KAFKA_CONSUMER, "run") => {
                    if let (Some(binding), Some(handler)) = (origin.binding, run(code, path, n)) {
                        handlers.entry(binding).or_insert(handler);
                    }
                }
                (AWS_SQS, "SendMessageCommand", _) => {
                    out.extend(produce("sqs", &|a| queue(a, "QueueUrl")));
                }
                (AWS_SQS, "ReceiveMessageCommand", _) => {
                    out.extend(consume("sqs", &|a| queue(a, "QueueUrl"), None));
                }
                (SQS_CONSUMER, "Consumer.create", _) => {
                    let handler =
                        handler(code, path, n, 0, &["handleMessage", "handleMessageBatch"]);
                    out.extend(consume("sqs", &|a| queue(a, "queueUrl"), handler));
                }
                (NATS, NATS_CONNECTION, "publish") => {
                    out.extend(produce("nats", &|a| subject(a)));
                }
                (NATS, NATS_CONNECTION, "subscribe") => {
                    let handler = handler(code, path, n, 1, &["callback"]);
                    out.extend(consume("nats", &|a| subject(a), handler));
                }
                (AMQPLIB, expr, "sendToQueue") if AMQP_CHANNELS.contains(&expr) => {
                    out.extend(produce("amqp", &|a| Some(vec![arg_name(a, 0)?])));
                }
                (AMQPLIB, expr, "consume") if AMQP_CHANNELS.contains(&expr) => {
                    let handler = function_arg(code, path, n, 1);
                    out.extend(consume("amqp", &|a| Some(vec![arg_name(a, 0)?]), handler));
                }
                (AMQPLIB, expr, "publish") if AMQP_CHANNELS.contains(&expr) => {
                    let read = |a: &[Value]| Some((arg_name(a, 0)?, arg_name(a, 1)?));
                    for (site, at, (exchange, key)) in code.expand(path, n, &read) {
                        // The empty exchange is the queue named by the key; one that cannot
                        // be evaluated leaves the producer unresolved.
                        let (name, role) = match exchange {
                            Some(e) if e.is_empty() => (key, Role::Producer),
                            Some(exchange) => (key, Role::Publish { exchange }),
                            None => (None, Role::Producer),
                        };
                        let (path, call) = (site.into(), at);
                        out.push(Message {
                            broker: "amqp",
                            name,
                            path,
                            call,
                            role,
                        });
                    }
                }
                (AMQPLIB, expr, "bindQueue") if AMQP_CHANNELS.contains(&expr) => {
                    let read =
                        |a: &[Value]| Some((arg_name(a, 0)?, arg_name(a, 1)?, arg_name(a, 2)?));
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
    handler(code, path, n, 0, &["eachMessage", "eachBatch"])
}

/// The handler the options object that argument `arg` of the call gives under the first of
/// `keys` it sets: a function written in place or a definition of the repository.
fn handler<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    arg: usize,
    keys: &[&str],
) -> Option<Handler> {
    let scope = &code.files()[path].calls[n].scope;
    let Some(Value::Object(options)) = code.args(path, n)?.get(arg) else {
        return None;
    };
    let handler = keys
        .iter()
        .find_map(|key| property(options, key).ok().flatten())?;
    function(code, path, scope, handler)
}

/// The handler argument `arg` of the call is: a function written in place or a definition of
/// the repository.
fn function_arg<'a>(code: &Code<'a>, path: &'a str, n: usize, arg: usize) -> Option<Handler> {
    let scope = &code.files()[path].calls[n].scope;
    function(code, path, scope, code.args(path, n)?.get(arg)?)
}

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

/// The subject a NATS `publish` or `subscribe` gives as its first argument.
fn subject(args: &[Value]) -> Option<Vec<Option<String>>> {
    Some(vec![name(args.first().unwrap_or(&Value::Undefined))?])
}

/// The SQS queue the URL under `key` of the options object a call takes first names: the last
/// segment of the URL, whatever precedes it (an account and a region are often dynamic).
fn queue(args: &[Value], key: &str) -> Option<Vec<Option<String>>> {
    let options = match args.first() {
        Some(Value::Object(properties)) => properties,
        Some(Value::Ref(_)) => return None,
        _ => return Some(vec![None]),
    };
    let parts = match property(options, key).ok()? {
        Some(Value::Str(parts)) => parts,
        Some(Value::Ref(_)) => return None,
        _ => return Some(vec![None]),
    };
    match parts.last() {
        Some(Part::Text(tail)) if tail.contains('/') => {
            let queue = tail.rsplit('/').next().filter(|q| !q.is_empty());
            Some(vec![queue.map(String::from)])
        }
        _ if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        _ => Some(vec![None]),
    }
}

/// The queue, exchange, routing key or pattern argument `at` of an amqplib call: `None` when it
/// depends on a parameter, `Some(None)` when it is not a literal string.
fn arg_name(args: &[Value], at: usize) -> Option<Option<String>> {
    args.get(at).map_or(Some(None), name)
}
