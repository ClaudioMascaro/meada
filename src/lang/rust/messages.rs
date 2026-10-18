//! The message libraries of the Rust pack (spec #30): rdkafka, lapin, aws-sdk-sqs, async-nats
//! and the redis crate. A form is a method called on a client that traces back to the library's
//! constructor (`Code::value_origins`, the constructor named by the file's `use` declarations),
//! so a `send` or a `publish` on anything else is no producer. The methods that make a client of
//! another (`create_channel`, `get_connection`, the setters of a builder) are listed per library
//! (`LanguagePack::passes`), as for the HTTP clients.
//!
//! Every client of these crates is polled or streamed, so the function that holds the call
//! naming the topic or the queue handles what it receives.

use crate::lang::{
    Message, Role, Value, channel_name as name, consume, produce, publication, queue_name, topics,
};
use crate::resolve::{Code, Keywords};

/// How many calls a builder chain is walked down: `FutureRecord::to(t).payload(p).key(k)` and
/// `client.send_message().message_body(b).queue_url(u)` are a few; a crafted file's chain of
/// thousands is not read.
// ponytail: `Code::chain_tail` bounds the value flow the same way.
const CHAIN: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Client {
    Kafka,
    Lapin,
    Sqs,
    Nats,
    Redis,
}

/// A library by the crate it is imported from, what constructs its client (the tail of the name
/// the `use` declarations give the call, `ClientConfig.new` under `rdkafka.config`) and the
/// methods that hand the client back or make the one the forms are called on.
struct Library {
    client: Client,
    krate: &'static str,
    constructors: &'static [&'static str],
    passes: &'static [&'static str],
}

impl Library {
    /// Whether `origin`, what a constructor is called outside the repository, is one of the
    /// library's.
    fn made(&self, origin: &str) -> bool {
        let Some(rest) = origin.strip_prefix(self.krate) else {
            return false;
        };
        let Some(rest) = rest.strip_prefix('.') else {
            return false;
        };
        (self.constructors.iter())
            .any(|c| rest == *c || rest.strip_suffix(c).is_some_and(|r| r.ends_with('.')))
    }
}

const LIBRARIES: &[Library] = &[
    Library {
        client: Client::Kafka,
        krate: "rdkafka",
        constructors: &["ClientConfig.new", "ClientConfig.default"],
        passes: &["set", "set_log_level", "create", "create_with_context"],
    },
    Library {
        client: Client::Lapin,
        krate: "lapin",
        constructors: &["Connection.connect", "Connection.connect_uri"],
        passes: &["create_channel"],
    },
    Library {
        client: Client::Sqs,
        krate: "aws_sdk_sqs",
        constructors: &["Client.new", "Client.from_conf"],
        passes: &[],
    },
    Library {
        client: Client::Nats,
        krate: "async_nats",
        constructors: &["connect", "connect_with_options", "ConnectOptions.new"],
        passes: &[
            "connect",
            "name",
            "token",
            "user_and_password",
            "require_tls",
            "ping_interval",
            "connection_timeout",
            "retry_on_initial_connect",
        ],
    },
    Library {
        client: Client::Redis,
        krate: "redis",
        constructors: &["Client.open", "ConnectionManager.new", "ClusterClient.new"],
        passes: &[
            "get_connection",
            "get_connection_with_timeout",
            "get_async_connection",
            "get_multiplexed_async_connection",
            "get_multiplexed_tokio_connection",
        ],
    },
];

/// The methods that hand back what a `Result` holds, or the object itself.
const UNWRAPS: &[&str] = &["clone", "unwrap", "expect"];

/// The methods a form is made of.
const METHODS: &[&str] = &[
    "send",
    "subscribe",
    "queue_subscribe",
    "publish",
    "xadd",
    "xread_options",
    "basic_publish",
    "basic_consume",
    "queue_bind",
    "queue_url",
];

/// The builders of aws-sdk-sqs a queue is named on.
const SQS_PRODUCERS: &[&str] = &["send_message", "send_message_batch"];
const SQS_CONSUMERS: &[&str] = &["receive_message"];

pub(super) fn passes(origin: &str, method: &str) -> bool {
    (LIBRARIES.iter())
        .any(|l| l.made(origin) && (l.passes.contains(&method) || UNWRAPS.contains(&method)))
}

pub(super) fn messages(code: &Code) -> Vec<Message> {
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let method = (call.callee.rsplit_once('.')).map_or("", |(_, method)| method);
            if !METHODS.contains(&method) {
                continue;
            }
            if method == "queue_url" {
                out.extend(sqs(code, path, n));
                continue;
            }
            // The record names the topic of a Kafka `send`; any other form names it itself.
            let record = match method {
                "send" => match record(code, path, n) {
                    Some(record) => Some(record),
                    None => continue,
                },
                _ => None,
            };
            let Some(receiver) = code.receiver(path, n) else {
                continue;
            };
            let Some(client) = client(code, path, &call.scope, receiver) else {
                continue;
            };
            match (client, method) {
                (Client::Kafka, "send") => {
                    if let Some((file, at)) = record {
                        out.extend(produce(code, file, at, "kafka", &one));
                    }
                }
                (Client::Kafka, "subscribe") => {
                    out.extend(consume(
                        code,
                        path,
                        n,
                        "kafka",
                        &|a: &[Value], _: &Keywords| topics(arg(a, 0)),
                        None,
                    ));
                }
                (Client::Nats, "publish") => {
                    out.extend(produce(code, path, n, "nats", &one));
                }
                (Client::Nats, "subscribe" | "queue_subscribe") => {
                    out.extend(consume(code, path, n, "nats", &one, None));
                }
                (Client::Redis, "xadd") => {
                    out.extend(produce(code, path, n, "redis", &one));
                }
                (Client::Redis, "xread_options") if grouped(code, path, n) => {
                    out.extend(consume(code, path, n, "redis", &keys, None));
                }
                (Client::Lapin, "basic_consume") => {
                    out.extend(consume(code, path, n, "amqp", &one, None));
                }
                (Client::Lapin, "basic_publish") => {
                    let read =
                        |a: &[Value], _: &Keywords| Some((name(arg(a, 0))?, name(arg(a, 1))?));
                    for (site, at, (exchange, key)) in code.expand(path, n, &read) {
                        let (name, role) = publication(exchange, key);
                        out.push(Message {
                            broker: "amqp",
                            name,
                            path: site.into(),
                            call: at,
                            role,
                        });
                    }
                }
                (Client::Lapin, "queue_bind") => {
                    let read = |a: &[Value], _: &Keywords| {
                        Some((name(arg(a, 0))?, name(arg(a, 1))?, name(arg(a, 2))?))
                    };
                    for (site, at, bound) in code.expand(path, n, &read) {
                        // ponytail: a binding that cannot be fully evaluated is dropped, not
                        // shown as unresolved.
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
    out
}

/// The client that `receiver`, written in `scope` of `path`, traces back to: the library of the
/// first constructor it may be, named by the file's imports.
fn client<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    receiver: &'a Value,
) -> Option<Client> {
    // A client held by a `let mut` (redis commands take `&mut self`) is no constant, but it is
    // still what its call made.
    let held = match receiver {
        Value::Ref(name) => code.mutable(path, scope, name),
        _ => None,
    };
    let origins = match held {
        Some((within, value)) => code.value_origins(path, within, value),
        None => code.value_origins(path, scope, receiver),
    };
    origins.into_iter().find_map(|(at, i)| {
        let ctor = code.call(at, i)?;
        let origin = code.external(at, &ctor.scope, &ctor.callee)?;
        LIBRARIES.iter().find(|l| l.made(&origin)).map(|l| l.client)
    })
}

/// The `FutureRecord::to(topic)` or `BaseRecord::to(topic)` call that the first argument of the
/// `send` at `n` of `path` is, at the head of its builder chain (`.payload(..).key(..)`): the
/// argument written in place or a `let` holding it. Its file and position.
fn record<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Option<(&'a str, usize)> {
    let send = code.call(path, n)?;
    let (file, mut at) = call_of(code, path, &send.scope, send.args.first()?)?;
    for _ in 0..CHAIN {
        let call = code.call(file, at)?;
        let origin = code.external(file, &call.scope, &call.callee);
        let record = origin.is_some_and(|o| {
            o.starts_with("rdkafka.")
                && (o.ends_with(".FutureRecord.to") || o.ends_with(".BaseRecord.to"))
        });
        if record {
            return Some((file, at));
        }
        at = down(code, file, at)?;
    }
    None
}

/// The call that makes the aws-sdk-sqs request whose `queue_url` is the call at `n` of `path`
/// (`client.send_message().message_body(b).queue_url(u)`), as a producer or a consumer of the
/// queue the URL ends in.
fn sqs<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Vec<Message> {
    let mut at = n;
    for _ in 0..CHAIN {
        let Some(next) = down(code, path, at) else {
            return Vec::new();
        };
        at = next;
        let Some(call) = code.call(path, at) else {
            return Vec::new();
        };
        let method = (call.callee.rsplit_once('.')).map_or("", |(_, method)| method);
        let producer = SQS_PRODUCERS.contains(&method);
        if !producer && !SQS_CONSUMERS.contains(&method) {
            continue;
        }
        let client = code
            .receiver(path, at)
            .and_then(|r| client(code, path, &call.scope, r));
        if client != Some(Client::Sqs) {
            return Vec::new();
        }
        let read = |a: &[Value], _: &Keywords| Some(vec![queue_name(a.first())?]);
        return match producer {
            true => produce(code, path, n, "sqs", &read),
            false => consume(code, path, n, "sqs", &read, None),
        };
    }
    Vec::new()
}

/// The call the call at `at` of `path` is made on: its receiver, when that is a call.
fn down<'a>(code: &Code<'a>, path: &'a str, at: usize) -> Option<usize> {
    match code.receiver(path, at)? {
        Value::Call(inv) => code.call_index(path, &inv.range),
        _ => None,
    }
}

/// The call that `value`, written in `scope` of `path`, is: written in place, or the `let`
/// that holds it. Its file and position.
fn call_of<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
) -> Option<(&'a str, usize)> {
    match value {
        Value::Call(inv) => Some((path, code.call_index(path, &inv.range)?)),
        Value::Ref(held) => match code.constant(path, scope, held)? {
            ((file, _), Value::Call(inv)) => Some((file, code.call_index(file, &inv.range)?)),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the options the `xread_options` at `n` of `path` is given (its third argument) set a
/// consumer group: `StreamReadOptions::default().group(..)` in place or in a `let`. A read
/// without one is not a consumer.
fn grouped<'a>(code: &Code<'a>, path: &'a str, n: usize) -> bool {
    let Some(read) = code.call(path, n) else {
        return false;
    };
    let options = read.args.get(2);
    let Some((file, mut at)) = options.and_then(|o| call_of(code, path, &read.scope, o)) else {
        return false;
    };
    let mut group = false;
    for _ in 0..CHAIN {
        let Some(call) = code.call(file, at) else {
            return false;
        };
        group |= call.callee.ends_with(".group");
        match down(code, file, at) {
            Some(next) => at = next,
            None => {
                let origin = code.external(file, &call.scope, &call.callee);
                let made = origin
                    .is_some_and(|o| o.starts_with("redis.") && o.contains(".StreamReadOptions."));
                return group && made;
            }
        }
    }
    false
}

/// The argument at position `at`, `Undefined` when the call leaves it out.
fn arg(args: &[Value], at: usize) -> &Value {
    args.get(at).unwrap_or(&Value::Undefined)
}

/// The one channel name the first argument gives.
fn one(args: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    Some(vec![name(arg(args, 0))?])
}

/// The streams `xread_options` reads: the keys of its first argument, a list. A key that is not
/// a literal string is unresolved; a list built elsewhere depends on a parameter or is unknown.
fn keys(args: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    match arg(args, 0) {
        Value::List(items) => items.iter().map(name).collect(),
        Value::Ref(_) => None,
        _ => Some(vec![None]),
    }
}
