use std::collections::BTreeMap;

use super::message::{
    Handler, Message, Side, bind, channel_name as name, env_of, publish, queue_name,
};
use crate::config::ChannelClient;
use crate::contract::Level;
use crate::http::AppConfig;
use crate::index::Indexed;
use crate::lang::{Part, Value, literal, property};
use crate::names::ts::{Factory, factory, field_factory};
use crate::resolve::{Code, Expand, Expanded};

const KAFKAJS: &str = "kafkajs";
const KAFKA_PRODUCER: &str = "new Kafka().producer()";
const KAFKA_CONSUMER: &str = "new Kafka().consumer()";
const AWS_SQS: &str = "@aws-sdk/client-sqs";
const SQS_CONSUMER: &str = "sqs-consumer";
const NATS: &str = "nats";
const NATS_CONNECTION: &str = "connect()";
const BULLMQ: &str = "bullmq";
const BULLMQ_QUEUE: &str = "new Queue()";
const IOREDIS: &str = "ioredis";
const IOREDIS_CLIENT: [&str; 2] = ["new default()", "new Redis()"];
const NODE_REDIS: &str = "redis";
const NODE_REDIS_CLIENT: &str = "createClient()";
const AMQPLIB: &str = "amqplib";
const AMQP_CHANNELS: [&str; 4] = [
    "default.connect().createChannel()",
    "default.connect().createConfirmChannel()",
    "connect().createChannel()",
    "connect().createConfirmChannel()",
];

type Read<'a> = &'a dyn Fn(&[Value]) -> Option<Vec<Option<String>>>;

pub(super) fn aws(code: &Code) -> Vec<crate::contract::Found> {
    if !code.pack().extensions().contains(&"js") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            if !call.callee.ends_with(".send") {
                continue;
            }
            let Some((client, package, command)) =
                crate::names::ts::constructor_argument(code, path, n)
            else {
                continue;
            };
            if package != client.package {
                continue;
            }
            let Some(operation) = super::aws::OPERATIONS.iter().find(|op| {
                op.js == command && op.client == client.expr && client.package == op.package()
            }) else {
                continue;
            };
            for found in crate::names::ts::constructor_options(code, path, n, &|fields| {
                operation.read(fields)
            }) {
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

pub(super) fn messages(code: &Code, indexed: &Indexed) -> Vec<Message> {
    if !code.pack().extensions().contains(&"js") {
        return Vec::new();
    }
    let mut declarations = BTreeMap::<_, BTreeMap<_, _>>::new();
    for client in indexed.channel_clients {
        if let Some((file, name)) = client.definition() {
            declarations
                .entry(name.rsplit('.').next().unwrap_or(name))
                .or_default()
                .entry((file, name))
                .or_insert(client);
        }
    }
    let config = AppConfig::new(indexed);
    let mut out = Vec::new();
    let mut subscriptions = Vec::new();
    let mut handlers = BTreeMap::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let (receiver, method) = (call.callee.rsplit_once('.')).unwrap_or(("", &call.callee));
            if let Some(named) = declarations.get(method)
                && let Some(target) = code.reaches(path, n)
                && let Some(client) = named.get(&(target.path, target.name))
            {
                out.extend(declared_sides(code, path, n, client, &config));
                continue;
            }
            let chain = match method {
                "send" | "subscribe" | "run" | "publish" | "add" | "xadd" | "xAdd"
                | "xreadgroup" | "xReadGroup" | "sendToQueue" | "consume" | "bindQueue"
                    if !receiver.is_empty() =>
                {
                    receiver
                }
                "SendMessageCommand" | "ReceiveMessageCommand" | "create" | "Worker" => {
                    &call.callee
                }
                _ => continue,
            };
            let found = factory(code, path, &call.scope, chain)
                .or_else(|| field_factory(code, path, &call.scope, chain));
            let Some(Factory {
                package,
                expr,
                binding,
            }) = found
            else {
                continue;
            };
            let produce = |broker, read: Read| {
                let read = |a: &[Value], _: &_| read(a).map(|names| (names, None, None));
                producers(broker, code.expand(Expand::Call(path, n), &read))
            };
            let consume = |broker, read: Read, handler: Option<Handler>| {
                (code
                    .expand(Expand::Call(path, n), &|a, _| read(a))
                    .into_iter())
                .flat_map(
                    |Expanded {
                         site: (site, at),
                         found: names,
                         ..
                     }| {
                        let handler = handler.clone().unwrap_or_else(|| enclosing(code, site, at));
                        names.into_iter().map(move |name| Message {
                            environment: None,
                            broker,
                            name,
                            path: site.into(),
                            call: at,
                            role: Side::Consumer(handler.clone()),
                            client: None,
                            cap: None,
                        })
                    },
                )
                .collect::<Vec<_>>()
            };
            match (package, expr.as_str(), method) {
                (KAFKAJS, KAFKA_PRODUCER, "send") => {
                    let read = |a: &[Value], _: &_| topics(&config, a, &["topic"]);
                    out.extend(producers(
                        "kafka",
                        code.expand(Expand::Call(path, n), &read),
                    ));
                }
                (KAFKAJS, KAFKA_CONSUMER, "subscribe") => {
                    subscriptions.push((path, n, binding));
                }
                (KAFKAJS, KAFKA_CONSUMER, "run") => {
                    let handler = handler(code, path, n, 0, &["eachMessage", "eachBatch"]);
                    if let (Some(binding), Some(handler)) = (binding, handler) {
                        // ponytail: only the first `run` on a consumer binding counts.
                        handlers.entry(binding).or_insert(handler);
                    }
                }
                (AWS_SQS, "SendMessageCommand", _) => {
                    out.extend(sqs(code, path, n, "QueueUrl", false, None));
                }
                (AWS_SQS, "ReceiveMessageCommand", _) => {
                    out.extend(sqs(code, path, n, "QueueUrl", true, None));
                }
                (SQS_CONSUMER, "Consumer.create", _) => {
                    let handler =
                        handler(code, path, n, 0, &["handleMessage", "handleMessageBatch"]);
                    out.extend(sqs(code, path, n, "queueUrl", true, handler));
                }
                (NATS, NATS_CONNECTION, "publish") => {
                    out.extend(produce("nats", &first));
                }
                (NATS, NATS_CONNECTION, "subscribe") => {
                    let handler = handler(code, path, n, 1, &["callback"]);
                    out.extend(consume("nats", &first, handler));
                }
                // ponytail: a `Queue` with no `const` binding (`new Queue(name).add(...)`, a
                // class field, a parameter) and `addBulk` are not recognized.
                (BULLMQ, BULLMQ_QUEUE, "add") => {
                    if let Some(binding) = binding {
                        let found = code.expand(
                            Expand::Constructed {
                                held: binding,
                                at: (path, n),
                            },
                            &|a, _| first(a),
                        );
                        let found = found.into_iter().map(|e| Expanded {
                            site: e.site,
                            frames: e.frames,
                            found: (e.found, None, None),
                        });
                        out.extend(producers("bullmq", found.collect()));
                    }
                }
                // ponytail: a processor given as a file path is not followed (the function
                // that makes the `Worker` handles).
                (BULLMQ, "Worker", _) => {
                    let processor = (code.args(path, n).and_then(|a| a.get(1)))
                        .and_then(|value| function(code, path, &call.scope, value));
                    out.extend(consume("bullmq", &first, processor));
                }
                (IOREDIS, expr, "xadd") if IOREDIS_CLIENT.contains(&expr) => {
                    out.extend(produce("redis", &first));
                }
                // ponytail: a client made as `await createClient().connect()` is not traced
                // (the `connect` chain has no binding).
                (NODE_REDIS, NODE_REDIS_CLIENT, "xAdd") => {
                    out.extend(produce("redis", &first));
                }
                (IOREDIS, expr, "xreadgroup") if IOREDIS_CLIENT.contains(&expr) => {
                    out.extend(consume("redis", &after_streams, None));
                }
                (NODE_REDIS, NODE_REDIS_CLIENT, "xReadGroup") => {
                    out.extend(consume("redis", &keys, None));
                }
                (AMQPLIB, expr, "sendToQueue") if AMQP_CHANNELS.contains(&expr) => {
                    out.extend(produce("amqp", &first));
                }
                (AMQPLIB, expr, "consume") if AMQP_CHANNELS.contains(&expr) => {
                    let handler = function_arg(code, path, n, 1);
                    out.extend(consume("amqp", &first, handler));
                }
                (AMQPLIB, expr, "publish") if AMQP_CHANNELS.contains(&expr) => {
                    let read = |a: &[Value]| Some((arg_name(a, 0)?, arg_name(a, 1)?));
                    out.extend(publish(code, path, n, &|a, _| read(a)));
                }
                (AMQPLIB, expr, "bindQueue") if AMQP_CHANNELS.contains(&expr) => {
                    let read =
                        |a: &[Value]| Some((arg_name(a, 0)?, arg_name(a, 1)?, arg_name(a, 2)?));
                    out.extend(bind(code, path, n, &|a, _| read(a)));
                }
                _ => {}
            }
        }
    }
    for (path, n, binding) in subscriptions {
        let read = |a: &[Value], _: &_| topics(&config, a, &["topics", "topic"]);
        for Expanded {
            site: (site, at),
            found: (names, environment, cap),
            ..
        } in code.expand(Expand::Call(path, n), &read)
        {
            // ponytail: a handler passed through a wrapper's parameter is not followed.
            let handler = binding.and_then(|b| handlers.get(&b)).cloned();
            let handler = handler.unwrap_or_else(|| enclosing(code, site, at));
            out.extend(names.into_iter().map(|name| Message {
                environment: environment.clone(),
                broker: "kafka",
                name,
                path: site.into(),
                call: at,
                role: Side::Consumer(handler.clone()),
                client: None,
                cap,
            }));
        }
    }
    out
}

type Found<'a> = Vec<Expanded<'a, Named>>;

type Named = (Vec<Option<String>>, Option<String>, Option<Level>);

fn declared_sides<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    client: &ChannelClient,
    config: &AppConfig,
) -> Vec<Message> {
    let read = |args: &[Value], _: &_| {
        let args = args.get(client.topic.arg..).unwrap_or_default();
        let value = match &client.topic.field {
            None => args.first(),
            Some(key) => match options(args)? {
                Some(fields) => property(fields, key).ok()?,
                None => None,
            },
        }
        .unwrap_or(&Value::Undefined);
        Some(configured_topic(
            config,
            Some(value),
            vec![super::message::topic(value)?],
            env_of(value),
        ))
    };
    let mut sides = producers(
        client.broker.name(),
        code.expand(Expand::Call(path, n), &read),
    );
    for side in &mut sides {
        side.client = Some(client.call.clone());
        if client.role == meada_core::manifest::ClientRole::Consumer {
            side.role = Side::Consumer(enclosing(code, &side.path, side.call));
        }
    }
    sides
}

fn producers(broker: &'static str, found: Found) -> Vec<Message> {
    (found.into_iter())
        .flat_map(
            |Expanded {
                 site: (site, at),
                 found: (names, environment, cap),
                 ..
             }| {
                names.into_iter().map(move |name| Message {
                    environment: environment.clone(),
                    broker,
                    name,
                    path: site.into(),
                    call: at,
                    role: Side::Producer,
                    client: None,
                    cap,
                })
            },
        )
        .collect()
}

fn enclosing(code: &Code, path: &str, at: usize) -> Handler {
    Handler::Def {
        path: path.into(),
        name: code.files()[path].calls[at].scope.clone(),
    }
}

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

fn names(args: &[Value], keys: &[&str]) -> Option<Vec<Option<String>>> {
    let Some(options) = options(args)? else {
        return Some(vec![None]);
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

fn names_env(args: &[Value], keys: &[&str]) -> Option<(Vec<Option<String>>, Option<String>)> {
    let found = names(args, keys)?;
    let given = (options(args).flatten())
        .and_then(|o| keys.iter().find_map(|key| property(o, key).ok().flatten()));
    Some((found, given.and_then(env_of)))
}

// ponytail: a `${VAR}` value is not mapped to its variable as HTTP's `decided` does, and a read
// inside a list of topics is not looked up.
fn topics(config: &AppConfig, args: &[Value], keys: &[&str]) -> Option<Named> {
    let (names, environment) = names_env(args, keys)?;
    let given = (options(args).flatten())
        .and_then(|o| keys.iter().find_map(|key| property(o, key).ok().flatten()));
    Some(configured_topic(config, given, names, environment))
}

fn configured_topic(
    config: &AppConfig,
    given: Option<&Value>,
    names: Vec<Option<String>>,
    environment: Option<String>,
) -> Named {
    let Some(key) = given.and_then(config_key) else {
        return (names, environment, None);
    };
    let values = config.literals(key);
    let mut names: Vec<Option<String>> = (values.iter())
        .filter(|v| crate::contract::environment_name("channel::kafka", v).is_some())
        .map(|v| Some(v.clone()))
        .collect();
    if names.is_empty() {
        names.push(None);
    }
    (
        names,
        environment,
        (values.len() > 1 || !config.whole()).then_some(Level::Inferred),
    )
}

fn config_key(value: &Value) -> Option<&str> {
    match value {
        Value::Str(parts) => match &parts[..] {
            [Part::Read { key, .. }] => Some(key),
            _ => None,
        },
        _ => None,
    }
}

fn options(args: &[Value]) -> Option<Option<&[(String, Value)]>> {
    match args.first() {
        Some(Value::Object(properties)) => Some(Some(properties)),
        Some(Value::Ref(_)) => None,
        _ => Some(None),
    }
}

fn first(args: &[Value]) -> Option<Vec<Option<String>>> {
    Some(vec![name(args.first().unwrap_or(&Value::Undefined))?])
}

fn queue(args: &[Value], key: &str) -> Option<Vec<Option<String>>> {
    let Some(options) = options(args)? else {
        return Some(vec![None]);
    };
    Some(vec![queue_name(property(options, key).ok()?)?])
}

fn after_streams(args: &[Value]) -> Option<Vec<Option<String>>> {
    let keyword = |v: &Value| match v {
        Value::Str(parts) => literal(parts).is_some_and(|s| s.eq_ignore_ascii_case("STREAMS")),
        _ => false,
    };
    let Some(at) = args.iter().position(keyword) else {
        return Some(vec![None]);
    };
    let rest = &args[at + 1..];
    rest[..rest.len() / 2].iter().map(name).collect()
}

fn keys(args: &[Value]) -> Option<Vec<Option<String>>> {
    let key = |value: &Value| match value {
        Value::Object(properties) => {
            name(property(properties, "key").ok()?.unwrap_or(&Value::Unknown))
        }
        Value::Ref(_) => None,
        _ => Some(None),
    };
    match args.get(2) {
        Some(Value::List(items)) => items.iter().map(key).collect(),
        Some(value) => Some(vec![key(value)?]),
        None => Some(vec![None]),
    }
}

fn arg_name(args: &[Value], at: usize) -> Option<Option<String>> {
    args.get(at).map_or(Some(None), name)
}

fn sqs<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    key: &str,
    consumer: bool,
    handler: Option<Handler>,
) -> Vec<Message> {
    let writes = crate::contract::sdk::mutations::writes;
    code.expand_written(path, n, writes, &|args, _| {
        let names = queue(args, key)?;
        let value = match args.first() {
            Some(Value::Object(fields)) => property(fields, key).ok().flatten(),
            _ => None,
        };
        let environment = value
            .and_then(crate::names::argument)
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
            Side::Consumer(
                handler
                    .clone()
                    .unwrap_or_else(|| enclosing(code, expanded.site.0, expanded.site.1)),
            )
        } else {
            Side::Producer
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
    use crate::lang::ts::TsJs;
    use crate::resolve::Resolver;

    fn lines(src: &str) -> Vec<String> {
        configured(src, &[])
    }

    fn configured(src: &str, configs: &[(&str, &[u8])]) -> Vec<String> {
        crate::channel::message::lines(configured_messages(src, configs))
    }

    fn configured_messages(src: &str, configs: &[(&str, &[u8])]) -> Vec<Message> {
        let facts = TsJs.parse("x.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("x.ts", &facts)]);
        let modules = TsJs.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &TsJs);
        let code = Code::new(&resolver, &files, &TsJs);
        let indexed = Indexed {
            manifests: &[],
            tsconfigs: &[],
            exported: &[],
            files: &[],
            configs,
            symlinks: &[],
            channel_clients: &[],
        };
        messages(&code, &indexed)
    }

    #[test]
    fn a_kafkajs_topic_read_from_app_config_names_each_value_of_its_key() {
        let send = |f: &str, key: &str| {
            format!(
                "export async function {f}(config) {{ await producer.send({{ topic: config.getString('{key}') }}); }}\n"
            )
        };
        let src = format!(
            "import {{ Kafka }} from 'kafkajs';\nconst kafka = new Kafka({{ brokers: [] }});\n\
             const producer = kafka.producer();\n{}",
            ["one", "two", "none", "bad", "mixed"]
                .map(|k| send(k, &format!("t.{k}")))
                .concat()
        );
        let base = b"t:\n  one: orders.created\n  two: orders.v2\n  bad: '${TOPIC}'\n  mixed: orders.shipped\n";
        let production = b"t:\n  two: orders.audit\n  mixed: 'orders shipped'\n";
        let configs: [(&str, &[u8]); 2] = [
            ("app-config.yaml", base),
            ("app-config.production.yaml", production),
        ];
        let found: Vec<(Option<String>, Option<Level>)> = (configured_messages(&src, &configs))
            .into_iter()
            .map(|m| (m.name, m.cap))
            .collect();
        let named = |n: &str| Some(n.to_string());
        let inferred = Some(Level::Inferred);
        assert_eq!(
            found,
            [
                (named("orders.created"), None),
                (named("orders.audit"), inferred),
                (named("orders.v2"), inferred),
                (None, None),
                (None, None),
                (named("orders.shipped"), inferred),
            ]
        );
        let unconfigured = configured(&src, &[]);
        assert!(
            unconfigured
                .iter()
                .all(|l| l == "kafka unresolved producer"),
            "{unconfigured:?}"
        );
    }

    #[test]
    fn an_app_config_include_not_followed_caps_the_topic() {
        let src = "import { Kafka } from 'kafkajs';\nconst kafka = new Kafka({ brokers: [] });\n\
                   const producer = kafka.producer();\nexport async function f(config) { \
                   await producer.send({ topic: config.getString('t.one') }); }\n";
        let base: &[u8] = b"t:\n  one: orders.created\n";
        let production: &[u8] = b"t:\n  $include: ./missing.yaml\n";
        let found = |configs: &[(&str, &[u8])]| -> Vec<(Option<String>, Option<Level>)> {
            (configured_messages(src, configs).into_iter())
                .map(|m| (m.name, m.cap))
                .collect()
        };
        let created = Some("orders.created".to_string());
        assert_eq!(
            found(&[("app-config.yaml", base)]),
            [(created.clone(), None)]
        );
        let configs = [
            ("app-config.yaml", base),
            ("app-config.production.yaml", production),
        ];
        assert_eq!(found(&configs), [(created.clone(), Some(Level::Inferred))]);
        let broken: &[u8] = b"t: [\n";
        let configs = [
            ("app-config.yaml", base),
            ("app-config.production.yaml", broken),
        ];
        assert_eq!(found(&configs), [(created, Some(Level::Inferred))]);
    }

    #[test]
    fn a_private_field_is_the_one_client_its_class_writes_it() {
        let src = r#"import { Kafka } from 'kafkajs';
class Held {
  private kafka = new Kafka({ brokers: [] });
  private p = this.kafka.producer();
  async go() { await this.p.send({ topic: 'orders.audit' }); }
}
class Loop {
  private a: any;
  private b: any;
  constructor() { this.a = this.b.producer(); this.b = this.a.producer(); }
  async go() { await this.a.send({ topic: 'orders.created' }); }
}
class Split {
  private kafka = new Kafka({ brokers: [] });
  private p: any;
  constructor() { this.p = this.kafka.producer(); }
  swap() { this.p = this.kafka.consumer({ groupId: 'g' }); }
  async go() { await this.p.send({ topic: 'orders.v2' }); }
}
"#;
        assert_eq!(lines(src), ["kafka orders.audit producer"]);
    }

    #[test]
    fn a_kafkajs_client_is_a_producer_or_a_consumer_by_its_method() {
        let src = r#"import { Kafka } from 'kafkajs';
const kafka = new Kafka({ brokers: [] });
const producer = kafka.producer();
const consumer = kafka.consumer({ groupId: 'g' });
async function produce() {
  await producer.send({ topic: 'orders.created', messages: [] });
}
async function consume(topic: string) {
  await consumer.subscribe({ topic: 'orders.created' });
  await consumer.subscribe({ topic });
  await consumer.run({ eachMessage: async () => {} });
}
function other(p: Other) {
  p.send({ topic: 'nope' });
}
"#;
        let at = src.find("async () =>").unwrap();
        assert_eq!(
            lines(src),
            [
                "kafka orders.created producer".to_string(),
                format!("kafka orders.created consumer inline x.ts::consume.@{at}"),
                format!("kafka unresolved consumer inline x.ts::consume.@{at}"),
            ]
        );
    }
}
