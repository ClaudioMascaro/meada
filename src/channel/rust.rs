use super::message::{
    Message, bind, channel_name as name, consume, produce, publish, queue_name, topics,
};
use crate::lang::{Call, Value};
use crate::resolve::{Code, Follow, Keywords, Of};

// ponytail: a record or a request with more setters is no form; `FLUENT_CHAIN` bounds the walk
// itself, for the value flow.
const CHAIN: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Client {
    Kafka,
    Lapin,
    Sqs,
    Nats,
    Redis,
}

struct Library {
    client: Client,
    krate: &'static str,
    constructors: &'static [&'static str],
    passes: &'static [&'static str],
}

impl Library {
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

const UNWRAPS: &[&str] = &["clone", "unwrap", "expect"];

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

const SQS_PRODUCERS: &[&str] = &["send_message", "send_message_batch"];
const SQS_CONSUMERS: &[&str] = &["receive_message"];

fn passes(origin: &str, method: &str) -> bool {
    (LIBRARIES.iter())
        .any(|l| l.made(origin) && (l.passes.contains(&method) || UNWRAPS.contains(&method)))
}

pub(super) fn messages(code: &Code) -> Vec<Message> {
    if !code.pack().extensions().contains(&"rs") {
        return Vec::new();
    }
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
                    out.extend(publish(code, path, n, &read));
                }
                (Client::Lapin, "queue_bind") => {
                    let read = |a: &[Value], _: &Keywords| {
                        Some((name(arg(a, 0))?, name(arg(a, 1))?, name(arg(a, 2))?))
                    };
                    out.extend(bind(code, path, n, &read));
                }
                _ => {}
            }
        }
    }
    out
}

fn outside_spelling<'a>(code: &Code<'a>, path: &'a str, call: &'a Call) -> Option<String> {
    let outside = code.outside(path, &call.scope, &call.callee, call.by_path)?;
    outside.direct.then_some(outside.spelled)
}

fn client<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    receiver: &'a Value,
) -> Option<Client> {
    let held = match receiver {
        Value::Ref(name) => code.mutable(path, scope, name),
        _ => None,
    };
    let origins = match held {
        Some((within, value)) => {
            code.origins(path, within, Of::Value(value), Follow::Values { passes })
        }
        None => code.origins(path, scope, Of::Value(receiver), Follow::Values { passes }),
    };
    origins.into_iter().find_map(|o| {
        let (at, i) = o.call?;
        let origin = outside_spelling(code, at, code.call(at, i)?)?;
        LIBRARIES.iter().find(|l| l.made(&origin)).map(|l| l.client)
    })
}

fn record<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Option<(&'a str, usize)> {
    let send = code.call(path, n)?;
    let (file, at) = call_of(code, path, &send.scope, send.args.first()?)?;
    let chain = chain(code, file, at)?;
    let at = chain.into_iter().find(|&at| {
        let call = code.call(file, at);
        let origin = call.and_then(|c| outside_spelling(code, file, c));
        origin.is_some_and(|o| {
            o.starts_with("rdkafka.")
                && (o.ends_with(".FutureRecord.to") || o.ends_with(".BaseRecord.to"))
        })
    });
    Some((file, at?))
}

fn sqs<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Vec<Message> {
    let Some(chain) = chain(code, path, n) else {
        return Vec::new();
    };
    for at in chain.into_iter().skip(1) {
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

fn chain<'a>(code: &Code<'a>, path: &'a str, at: usize) -> Option<Vec<usize>> {
    let mut chain = code.chain(path, at)?;
    chain.truncate(CHAIN + 1);
    Some(chain)
}

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

fn grouped<'a>(code: &Code<'a>, path: &'a str, n: usize) -> bool {
    let Some(read) = code.call(path, n) else {
        return false;
    };
    let options = read.args.get(2);
    let Some((file, at)) = options.and_then(|o| call_of(code, path, &read.scope, o)) else {
        return false;
    };
    let calls: Vec<&Call> = (chain(code, file, at).into_iter().flatten())
        .filter_map(|at| code.call(file, at))
        .collect();
    let group = calls.iter().any(|c| c.callee.ends_with(".group"));
    let start = calls
        .last()
        .filter(|c| !matches!(c.receiver, Some(Value::Call(_))));
    let origin = start.and_then(|c| outside_spelling(code, file, c));
    group && origin.is_some_and(|o| o.starts_with("redis.") && o.contains(".StreamReadOptions."))
}

fn arg(args: &[Value], at: usize) -> &Value {
    args.get(at).unwrap_or(&Value::Undefined)
}

fn one(args: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    Some(vec![name(arg(args, 0))?])
}

fn keys(args: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    match arg(args, 0) {
        Value::List(items) => items.iter().map(name).collect(),
        Value::Ref(_) => None,
        _ => Some(vec![None]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::rust::Rust;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    fn lines(src: &str) -> Vec<String> {
        let facts = Rust.parse("x.rs", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("x.rs", &facts)]);
        let modules = Rust.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Rust);
        let code = Code::new(&resolver, &files, &Rust);
        crate::channel::message::lines(messages(&code))
    }

    #[test]
    fn a_client_of_a_library_is_a_producer_or_a_consumer_by_its_method() {
        let src = "use async_nats::connect;\n\
            async fn produce() {\n    let c = connect(\"u\").await.unwrap();\n    c.publish(\"orders.created\", b.into()).await.unwrap();\n}\n\
            async fn consume() {\n    let c = connect(\"u\").await.unwrap();\n    c.subscribe(\"orders.created\").await.unwrap();\n    c.subscribe(std::env::var(\"T\").unwrap()).await.unwrap();\n    tokio::spawn(async move {\n        c.subscribe(\"spawned\").await.unwrap();\n    });\n}\n\
            fn other(c: Other) {\n    c.publish(\"nope\", b.into());\n}\n";
        assert_eq!(
            lines(src),
            [
                "nats orders.created producer",
                "nats orders.created consumer x.rs::consume",
                "nats unresolved consumer x.rs::consume",
                "nats spawned consumer x.rs::consume",
            ]
        );
    }

    #[test]
    fn a_path_written_constructor_is_read_by_path_past_a_value_binding_of_its_head() {
        let src = "use lapin;\nfn lapin() {}\n\
            async fn go() {\n    let c = lapin::Connection::connect(\"u\").await.unwrap();\n    c.basic_publish(\"x\", \"k\").await.unwrap();\n}\n";
        assert_eq!(lines(src), ["amqp k publish x"]);
    }

    #[test]
    fn a_constructor_whose_head_is_not_the_import_itself_gives_no_client() {
        let src = "use async_nats;\nfn go(async_nats: X) { async_nats.connect(\"u\"); }\n";
        let facts = Rust.parse("x.rs", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("x.rs", &facts)]);
        let modules = Rust.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Rust);
        let code = Code::new(&resolver, &files, &Rust);
        let call = &facts.calls[0];
        let outside = code.outside("x.rs", &call.scope, &call.callee, call.by_path);
        assert!(outside.is_some_and(|o| !o.direct && o.spelled == "async_nats.connect"));
        assert_eq!(outside_spelling(&code, "x.rs", call), None);
    }
}
