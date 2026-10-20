//! The message libraries of the TypeScript and JavaScript pack (spec #20). A form is a method
//! called on a client whose origin is the library's own factory, imported from the library's
//! package: a `send` on anything else is no producer. The origin is the one question,
//! `Code::origins` with `Follow::Constants`: only `const` bindings are followed, so a client
//! passed in as a parameter or returned by a function has none (certain or absent). `factory`
//! rebuilds the spelling each form table matches (`new Kafka().producer()`) from the origin's
//! form, outside name and steps.

use std::collections::BTreeMap;

use crate::lang::{
    Handler, Message, Role, Value, bind, channel_name as name, literal, property, publish,
    queue_name,
};
use crate::resolve::{Code, Expand, Expanded, Follow, Form, Of};

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
// A default import takes the name it is given; the client is also exported as `Redis`.
const IOREDIS_CLIENT: [&str; 2] = ["new default()", "new Redis()"];
const NODE_REDIS: &str = "redis";
const NODE_REDIS_CLIENT: &str = "createClient()";
const AMQPLIB: &str = "amqplib";
/// What a channel of amqplib is, from the default import, a namespace or a named `connect`.
const AMQP_CHANNELS: [&str; 4] = [
    "default.connect().createChannel()",
    "default.connect().createConfirmChannel()",
    "connect().createChannel()",
    "connect().createConfirmChannel()",
];

/// The package a client's factory is imported from, the factory as JavaScript would write it
/// (`new Kafka().producer()`), and the constant the client is bound to (its file and qualified
/// name), from the `Constants` origin of `chain`: `None` unless it comes from an import of
/// something outside the repository.
fn factory<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    chain: &'a str,
) -> Option<(&'a str, String, Option<(&'a str, &'a str)>)> {
    let found = code.origins(path, scope, Of::Name(chain), Follow::Constants);
    let origin = found.into_iter().next()?;
    let (package, name) = origin.outside?.imported?;
    let route = origin.routes.into_iter().next()?;
    let mut expr = match origin.form {
        Form::New => format!("new {name}()"),
        Form::Call => format!("{name}()"),
        Form::Type | Form::Name => name,
    };
    for step in &route.steps {
        expr += &format!(".{step}()");
    }
    Some((package, expr, route.held))
}

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
            let Some((package, expr, binding)) = factory(code, path, &call.scope, chain) else {
                continue;
            };
            let produce = |broker, read: Read| {
                producers(broker, code.expand(Expand::Call(path, n), &|a, _| read(a)))
            };
            // The handler of the call itself (its `option` of argument `arg`), else the
            // function the call is in.
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
                            broker,
                            name,
                            path: site.into(),
                            call: at,
                            role: Role::Consumer(handler.clone()),
                        })
                    },
                )
                .collect::<Vec<_>>()
            };
            match (package, expr.as_str(), method) {
                (KAFKAJS, KAFKA_PRODUCER, "send") => {
                    out.extend(produce("kafka", &|a| names(a, &["topic"])));
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
                    out.extend(produce("nats", &first));
                }
                (NATS, NATS_CONNECTION, "subscribe") => {
                    let handler = handler(code, path, n, 1, &["callback"]);
                    out.extend(consume("nats", &first, handler));
                }
                // The queue is named where the binding is made, not at the `add`: the `add` is
                // the producer, or, when the name is a parameter of the function that makes
                // the binding, each call site of that wrapper.
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
                        out.extend(producers("bullmq", found));
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
        let read = |a: &[Value]| names(a, &["topics", "topic"]);
        for Expanded {
            site: (site, at),
            found: names,
            ..
        } in code.expand(Expand::Call(path, n), &|a, _| read(a))
        {
            // With no `run` handler Meada can read, the function that subscribes handles.
            // ponytail: a handler passed through a wrapper's parameter is not followed.
            let handler = binding.and_then(|b| handlers.get(&b)).cloned();
            let handler = handler.unwrap_or_else(|| enclosing(code, site, at));
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

/// A producer per name read at each site an expansion found.
fn producers(broker: &'static str, found: Vec<Expanded<Vec<Option<String>>>>) -> Vec<Message> {
    (found.into_iter())
        .flat_map(
            |Expanded {
                 site: (site, at),
                 found: names,
                 ..
             }| {
                names.into_iter().map(move |name| Message {
                    broker,
                    name,
                    path: site.into(),
                    call: at,
                    role: Role::Producer,
                })
            },
        )
        .collect()
}

/// The function the call `at` of `path` is in, as the handler of what it consumes.
fn enclosing(code: &Code, path: &str, at: usize) -> Handler {
    Handler::Def {
        path: path.into(),
        name: code.files()[path].calls[at].scope.clone(),
    }
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

/// The options object a call takes first: `None` when it is a parameter (a client wrapper),
/// `Some(None)` when it is not an object.
fn options(args: &[Value]) -> Option<Option<&[(String, Value)]>> {
    match args.first() {
        Some(Value::Object(properties)) => Some(Some(properties)),
        Some(Value::Ref(_)) => None,
        _ => Some(None),
    }
}

/// The channel name a call gives as its first argument: a NATS subject, the queue of a BullMQ
/// `Queue` or `Worker`, the key of a Redis `xadd`, an amqplib queue.
fn first(args: &[Value]) -> Option<Vec<Option<String>>> {
    Some(vec![name(args.first().unwrap_or(&Value::Undefined))?])
}

/// The SQS queue the URL under `key` of the options object a call takes first names: the last
/// segment of the URL, whatever precedes it (an account and a region are often dynamic).
fn queue(args: &[Value], key: &str) -> Option<Vec<Option<String>>> {
    let Some(options) = options(args)? else {
        return Some(vec![None]);
    };
    Some(vec![queue_name(property(options, key).ok()?)?])
}

/// The streams an ioredis `xreadgroup` reads: the arguments after `STREAMS` that name them,
/// the first half; the ids that close the list are the other half.
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

/// The streams a node-redis `xReadGroup` reads: the `key` of its third argument, an object or
/// a list of them.
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

/// The queue, exchange, routing key or pattern argument `at` of an amqplib call: `None` when it
/// depends on a parameter, `Some(None)` when it is not a literal string.
fn arg_name(args: &[Value], at: usize) -> Option<Option<String>> {
    args.get(at).map_or(Some(None), name)
}
