use super::message::{
    Handler, Message, bind, channel_name as name, consume, produce, publish, queue_name, topic,
    topics,
};
use crate::lang::Value;
use crate::names::python::{Construction, Factories, imported};
use crate::resolve::{Code, Keywords};

const CONFLUENT_PRODUCER: &str = "confluent_kafka.Producer";
const CONFLUENT_CONSUMER: &str = "confluent_kafka.Consumer";
const KAFKA_PRODUCER: &str = "kafka.KafkaProducer";
const KAFKA_CONSUMER: &str = "kafka.KafkaConsumer";
const PIKA_CHANNEL: &str = "pika.BlockingConnection.channel";
const NATS_CLIENTS: [&str; 2] = ["nats.connect", "nats.aio.client.Client"];
const BOTO3_CLIENTS: [&str; 3] = [
    "boto3.client",
    "boto3.Session.client",
    "boto3.session.Session.client",
];
const REDIS_CLIENTS: [&str; 7] = [
    "redis.Redis",
    "redis.StrictRedis",
    "redis.Redis.from_url",
    "redis.from_url",
    "redis.asyncio.Redis",
    "redis.asyncio.Redis.from_url",
    "redis.asyncio.from_url",
];
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
const FACTORIES: [(&str, &str); 3] = [
    ("pika.BlockingConnection", "channel"),
    ("boto3.Session", "client"),
    ("boto3.session.Session", "client"),
];
const CALL: &str = "(call)";

#[derive(Clone, Copy)]
enum Client {
    KafkaProducer,
    KafkaConsumer,
    Pika,
    Sqs,
    Nats,
    Redis,
}

pub(super) fn messages(code: &Code) -> Vec<Message> {
    if !code.pack().extensions().contains(&"py") {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut factories = Factories::new(code, &BOTO3_CLIENTS, &FACTORIES, "service_name");
    for (&path, facts) in code.files() {
        let kafka = facts.imports.iter().any(|i| i.source == "kafka");
        for (n, call) in facts.calls.iter().enumerate() {
            let (receiver, method) = (call.callee.rsplit_once('.')).unwrap_or(("", &call.callee));
            if kafka
                && imported(code, path, &call.scope, &call.callee).as_deref()
                    == Some(KAFKA_CONSUMER)
            {
                let read = |a: &[Value], _: &Keywords| a.iter().map(topic).collect();
                out.extend(consume(code, path, n, "kafka", &read, None));
                continue;
            }
            let written = receiver == CALL || !receiver.starts_with('(');
            if receiver.is_empty() || !written || !METHODS.contains(&method) {
                continue;
            }
            let Some(client) = factories.read(path, call).as_ref().and_then(client) else {
                continue;
            };
            match (client, method) {
                (Client::KafkaProducer, "produce" | "send") => {
                    out.extend(produce(code, path, n, "kafka", &|a, k| {
                        one(a, k, 0, "topic")
                    }));
                }
                (Client::KafkaConsumer, "subscribe") => {
                    let read = |a: &[Value], k: &Keywords| topics(arg(a, k, 0, "topics"));
                    out.extend(consume(code, path, n, "kafka", &read, None));
                }
                (Client::Sqs, "send_message" | "send_message_batch") => {
                    out.extend(sqs(code, path, n, false));
                }
                (Client::Sqs, "receive_message") => {
                    out.extend(sqs(code, path, n, true));
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
                    out.extend(publish(code, path, n, &read));
                }
                (Client::Pika, "queue_bind") => {
                    let read = |a: &[Value], k: &Keywords| {
                        Some((
                            arg_name(a, k, 0, "queue")?,
                            arg_name(a, k, 1, "exchange")?,
                            arg_name(a, k, 2, "routing_key")?,
                        ))
                    };
                    out.extend(bind(code, path, n, &read));
                }
                _ => {}
            }
        }
    }
    out
}

fn client(maker: &Construction) -> Option<Client> {
    Some(match maker.name.as_str() {
        CONFLUENT_PRODUCER | KAFKA_PRODUCER => Client::KafkaProducer,
        CONFLUENT_CONSUMER | KAFKA_CONSUMER => Client::KafkaConsumer,
        PIKA_CHANNEL => Client::Pika,
        name if NATS_CLIENTS.contains(&name) => Client::Nats,
        name if REDIS_CLIENTS.contains(&name) => Client::Redis,
        name if BOTO3_CLIENTS.contains(&name) && maker.argument.as_deref() == Some("sqs") => {
            Client::Sqs
        }
        _ => return None,
    })
}

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

fn arg<'v>(args: &'v [Value], kwargs: &'v Keywords, at: usize, key: &str) -> &'v Value {
    (args.get(at))
        .or_else(|| kwargs.iter().find(|(k, _)| k == key).map(|(_, v)| v))
        .unwrap_or(&Value::Undefined)
}

fn arg_name(args: &[Value], kwargs: &Keywords, at: usize, key: &str) -> Option<Option<String>> {
    name(arg(args, kwargs, at, key))
}

fn one(args: &[Value], kwargs: &Keywords, at: usize, key: &str) -> Option<Vec<Option<String>>> {
    Some(vec![arg_name(args, kwargs, at, key)?])
}

fn queue(kwargs: &Keywords) -> Option<Vec<Option<String>>> {
    let url = kwargs.iter().find(|(k, _)| k == "QueueUrl").map(|(_, v)| v);
    Some(vec![queue_name(url)?])
}

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

pub(super) fn aws(code: &Code) -> Vec<crate::contract::Found> {
    if !code.pack().extensions().contains(&"py") {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut factories = Factories::new(code, &BOTO3_CLIENTS, &FACTORIES, "service_name");
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let Some((_, method)) = call.callee.rsplit_once('.') else {
                continue;
            };
            if !super::aws::OPERATIONS.iter().any(|op| op.python == method) {
                continue;
            }
            let Some(first) = factories.read(path, call).and_then(|made| made.argument) else {
                continue;
            };
            let Some(operation) = super::aws::OPERATIONS
                .iter()
                .find(|op| op.service == first && op.python == method)
            else {
                continue;
            };
            let read = |_: &[Value], kwargs: &Keywords| operation.read(kwargs);
            for found in code.expand(crate::resolve::Expand::Call(path, n), &read) {
                out.extend(
                    found
                        .found
                        .into_iter()
                        .map(|name| operation.found(name, found.site)),
                );
            }
        }
    }
    out
}

fn sqs<'a>(code: &Code<'a>, path: &'a str, n: usize, consumer: bool) -> Vec<Message> {
    code.expand(crate::resolve::Expand::Call(path, n), &|_, keywords| {
        let names = queue(keywords)?;
        let environment = keywords
            .iter()
            .find(|(key, _)| key == "QueueUrl")
            .and_then(|(_, value)| crate::names::argument(value))
            .and_then(|a| match a {
                crate::names::Argument::Environment(name) => Some(name),
                _ => None,
            });
        Some((names, environment))
    })
    .into_iter()
    .flat_map(|expanded| {
        let (names, environment) = expanded.found;
        let role = if consumer {
            super::message::Side::Consumer(Handler::Def {
                path: expanded.site.0.into(),
                name: code.files()[expanded.site.0].calls[expanded.site.1]
                    .scope
                    .clone(),
            })
        } else {
            super::message::Side::Producer
        };
        names.into_iter().map(move |name| Message {
            broker: "sqs",
            name,
            environment: environment.clone(),
            path: expanded.site.0.into(),
            call: expanded.site.1,
            role: role.clone(),
            client: None,
            cap: None,
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::python::Python;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    fn lines(src: &str) -> Vec<String> {
        let facts = Python.parse("x.py", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("x.py", &facts)]);
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Python);
        let code = Code::new(&resolver, &files, &Python);
        crate::channel::message::lines(messages(&code))
    }

    #[test]
    fn a_nats_client_produces_and_consumes_with_its_handler() {
        let src = r#"
import os
import nats

async def on_message(msg):
    pass

async def run():
    nc = await nats.connect("nats://nats:4222")
    await nc.publish("orders.created", b"o")
    await nc.publish(os.getenv("SUBJECT"), b"o")
    await nc.subscribe("orders.created", cb=on_message)
"#;
        assert_eq!(
            lines(src),
            [
                "nats orders.created producer",
                "nats unresolved producer",
                "nats orders.created consumer x.py::on_message"
            ]
        );
    }
}
