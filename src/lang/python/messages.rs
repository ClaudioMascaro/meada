//! The message libraries of the Python pack (spec #20, #73). A form is a method called on a
//! client that traces back to the library's constructor (`Code::origins`), imported from the
//! library's package: a `produce` on anything else is no producer.

use crate::lang::{Call, Handler, Message, Role, Value, channel_name as name, literal, queue_name};
use crate::resolve::{Code, Keywords};

const CONFLUENT_PRODUCER: &str = "confluent_kafka.Producer";
const CONFLUENT_CONSUMER: &str = "confluent_kafka.Consumer";
const KAFKA_PRODUCER: &str = "kafka.KafkaProducer";
const KAFKA_CONSUMER: &str = "kafka.KafkaConsumer";
/// A pika channel is made by a connection.
const PIKA_CHANNEL: &str = "pika.BlockingConnection.channel";
const NATS_CLIENTS: [&str; 2] = ["nats.connect", "nats.aio.client.Client"];
/// A boto3 client is made for a service, by the package or by a session.
const BOTO3_CLIENTS: [&str; 3] = [
    "boto3.client",
    "boto3.Session.client",
    "boto3.session.Session.client",
];
const REDIS_CLIENTS: [&str; 4] = [
    "redis.Redis",
    "redis.Redis.from_url",
    "redis.asyncio.Redis",
    "redis.asyncio.Redis.from_url",
];
/// The methods of a client that a form is made of.
const METHODS: [&str; 12] = [
    "produce",
    "send",
    "subscribe",
    "publish",
    "xadd",
    "xreadgroup",
    "basic_publish",
    "basic_consume",
    "queue_bind",
    "send_message",
    "send_message_batch",
    "receive_message",
];
/// The calls on a client that make another one: a connection's channel, a session's client.
const FACTORIES: [&str; 2] = ["channel", "client"];

/// What a form reads of a call's arguments: the channel names, see `names`.
type Read<'a> = &'a dyn Fn(&[Value], &Keywords) -> Option<Vec<Option<String>>>;

#[derive(Clone, Copy)]
enum Client {
    KafkaProducer,
    KafkaConsumer,
    Pika,
    Sqs,
    Nats,
    Redis,
}

/// A call that made a client, with the name its imports give what it called, such as
/// `pika.BlockingConnection.channel`.
struct Maker<'a> {
    name: String,
    call: &'a Call,
}

pub(super) fn messages(code: &Code) -> Vec<Message> {
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let (receiver, method) = (call.callee.rsplit_once('.')).unwrap_or(("", &call.callee));
            // kafka-python consumers take their topics from the constructor.
            if method == "KafkaConsumer" {
                if imported(code, path, &call.scope, &call.callee).as_deref()
                    == Some(KAFKA_CONSUMER)
                {
                    let read = |a: &[Value], _: &Keywords| a.iter().map(name).collect();
                    out.extend(consume(code, path, n, "kafka", &read, None));
                }
                continue;
            }
            if receiver.is_empty() || receiver.starts_with('(') || !METHODS.contains(&method) {
                continue;
            }
            let makers = makers(code, path, &call.scope, receiver, true);
            let Some(client) = makers.iter().find_map(client) else {
                continue;
            };
            match (client, method) {
                (Client::KafkaProducer, "produce" | "send") => {
                    out.extend(produce(code, path, n, "kafka", &|a, k| {
                        one(a, k, 0, "topic")
                    }));
                }
                (Client::KafkaConsumer, "subscribe") => {
                    let read = |a: &[Value], k: &Keywords| names(arg(a, k, 0, "topics"));
                    out.extend(consume(code, path, n, "kafka", &read, None));
                }
                (Client::Sqs, "send_message" | "send_message_batch") => {
                    out.extend(produce(code, path, n, "sqs", &|_, k| queue(k)));
                }
                (Client::Sqs, "receive_message") => {
                    out.extend(consume(code, path, n, "sqs", &|_, k| queue(k), None));
                }
                (Client::Nats, "publish") => {
                    out.extend(produce(code, path, n, "nats", &|a, k| {
                        one(a, k, 0, "subject")
                    }));
                }
                (Client::Nats, "subscribe") => {
                    let handler = callback(code, path, n, 2, "cb");
                    let read = |a: &[Value], k: &Keywords| one(a, k, 0, "subject");
                    out.extend(consume(code, path, n, "nats", &read, handler));
                }
                (Client::Redis, "xadd") => {
                    out.extend(produce(code, path, n, "redis", &|a, k| {
                        one(a, k, 0, "name")
                    }));
                }
                (Client::Redis, "xreadgroup") => {
                    out.extend(consume(code, path, n, "redis", &streams, None));
                }
                (Client::Pika, "basic_consume") => {
                    let handler = callback(code, path, n, 1, "on_message_callback");
                    let read = |a: &[Value], k: &Keywords| one(a, k, 0, "queue");
                    out.extend(consume(code, path, n, "amqp", &read, handler));
                }
                (Client::Pika, "basic_publish") => {
                    let read = |a: &[Value], k: &Keywords| {
                        Some((
                            arg_name(a, k, 0, "exchange")?,
                            arg_name(a, k, 1, "routing_key")?,
                        ))
                    };
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
                (Client::Pika, "queue_bind") => {
                    let read = |a: &[Value], k: &Keywords| {
                        Some((
                            arg_name(a, k, 0, "queue")?,
                            arg_name(a, k, 1, "exchange")?,
                            arg_name(a, k, 2, "routing_key")?,
                        ))
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

/// The client `maker` made, if it is one a form is written for.
fn client(maker: &Maker) -> Option<Client> {
    Some(match maker.name.as_str() {
        CONFLUENT_PRODUCER | KAFKA_PRODUCER => Client::KafkaProducer,
        CONFLUENT_CONSUMER | KAFKA_CONSUMER => Client::KafkaConsumer,
        PIKA_CHANNEL => Client::Pika,
        name if NATS_CLIENTS.contains(&name) => Client::Nats,
        name if REDIS_CLIENTS.contains(&name) => Client::Redis,
        name if BOTO3_CLIENTS.contains(&name) && is_sqs(maker.call) => Client::Sqs,
        _ => return None,
    })
}

/// The calls that made the client `receiver` may be, written in `path` within `scope`: the
/// constructors and factories of a package outside the repository. A call that is a method of
/// another client (`connection.channel()`) is named after the constructor of that one, a level
/// deep.
// ponytail: only one level of client factories is followed (a channel of a connection, a client
// of a session).
fn makers<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    receiver: &'a str,
    through: bool,
) -> Vec<Maker<'a>> {
    let mut found = Vec::new();
    for (at, i) in code.origins(path, scope, receiver) {
        let Some(call) = code.call(at, i) else {
            continue;
        };
        if let Some(name) = imported(code, at, &call.scope, &call.callee) {
            found.push(Maker { name, call });
            continue;
        }
        let Some((inner, method)) = call.callee.rsplit_once('.') else {
            continue;
        };
        if through && FACTORIES.contains(&method) {
            for made in makers(code, at, &call.scope, inner, false) {
                let name = format!("{}.{method}", made.name);
                found.push(Maker { name, call });
            }
        }
    }
    found
}

/// What `callee`, written in `scope` of `path`, names in a package outside the repository,
/// spelled from the package's import: `pika.BlockingConnection` however it is imported. A
/// method of a local (`connection.channel`) is none.
fn imported<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, callee: &'a str) -> Option<String> {
    let (package, spelled) = code.imported(path, scope, callee)?;
    Some(format!("{package}.{spelled}"))
}

/// Whether a boto3 call makes a client for SQS: its service, the first argument, is `"sqs"`.
fn is_sqs(call: &Call) -> bool {
    let service = call.args.first().or_else(|| call.keyword("service_name"));
    matches!(service, Some(Value::Str(parts)) if literal(parts).as_deref() == Some("sqs"))
}

/// A producer per name read at each site an expansion found.
fn produce<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    broker: &'static str,
    read: Read,
) -> Vec<Message> {
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
        .collect()
}

/// A consumer per name read at each site an expansion found: the call's own `handler`, else
/// the function the site is in (a polling client).
fn consume<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    broker: &'static str,
    read: Read,
    handler: Option<Handler>,
) -> Vec<Message> {
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
        .collect()
}

/// The definition of the repository that the callback argument `at` (or the keyword `key`) of
/// the call is; a lambda and a callable that is no definition give none.
fn callback<'a>(code: &Code<'a>, path: &'a str, n: usize, at: usize, key: &str) -> Option<Handler> {
    let call = code.call(path, n)?;
    let Some(Value::Ref(callback)) = call.args.get(at).or_else(|| call.keyword(key)) else {
        return None;
    };
    code.definition(path, &call.scope, callback)
        .map(|t| Handler::Def {
            path: t.path.into(),
            name: t.name.into(),
        })
}

/// The argument at position `at` of a call or, left out, the keyword `key`.
fn arg<'v>(args: &'v [Value], kwargs: &'v Keywords, at: usize, key: &str) -> &'v Value {
    (args.get(at))
        .or_else(|| kwargs.iter().find(|(k, _)| k == key).map(|(_, v)| v))
        .unwrap_or(&Value::Undefined)
}

/// The channel name an argument gives (see `channel_name`).
fn arg_name(args: &[Value], kwargs: &Keywords, at: usize, key: &str) -> Option<Option<String>> {
    name(arg(args, kwargs, at, key))
}

/// The one channel name an argument gives, as `read` answers it.
fn one(args: &[Value], kwargs: &Keywords, at: usize, key: &str) -> Option<Vec<Option<String>>> {
    Some(vec![arg_name(args, kwargs, at, key)?])
}

/// The channel names of a list of them, or of the one a value gives. `None` when one depends on
/// a parameter (a client wrapper); a name that is not a literal string is unresolved (`None`
/// in the list).
fn names(value: &Value) -> Option<Vec<Option<String>>> {
    match value {
        Value::List(items) => items.iter().map(name).collect(),
        value => Some(vec![name(value)?]),
    }
}

/// The SQS queue the `QueueUrl` keyword names.
fn queue(kwargs: &Keywords) -> Option<Vec<Option<String>>> {
    let url = kwargs.iter().find(|(k, _)| k == "QueueUrl").map(|(_, v)| v);
    Some(vec![queue_name(url)?])
}

/// The streams a redis-py `xreadgroup` reads: the keys of its third argument, a dict literal
/// of stream and id.
fn streams(args: &[Value], kwargs: &Keywords) -> Option<Vec<Option<String>>> {
    match arg(args, kwargs, 2, "streams") {
        Value::Object(entries) => entries
            .iter()
            .map(|(key, value)| match (key.as_str(), value) {
                ("[]", Value::List(pair)) => pair.first().map_or(Some(None), name),
                ("...", _) => Some(None),
                (key, _) => Some(Some(key.to_string())),
            })
            .collect(),
        Value::Ref(_) => None,
        _ => Some(vec![None]),
    }
}
