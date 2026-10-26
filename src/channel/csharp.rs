//! The channel kind's C# forms (spec #28, #115): Confluent.Kafka (`kafka`), RabbitMQ.Client
//! (`amqp`), AWSSDK.SQS (`sqs`), NATS.Client (`nats`), StackExchange.Redis streams (`redis`) and
//! Azure.Messaging.ServiceBus (`servicebus`). MassTransit is not read.
//!
//! A form is a method called on a client, and a call counts only when its receiver is one: the
//! type the source writes for it (a field, a property, a parameter, a typed local) or the
//! construction it was made by (`new ProducerBuilder<..>(..).Build()`, a factory's
//! `CreateConnection()`, a local bound to either), the type read through the usings
//! (`names::spellings`) and accepted only as the one spelling of this module's table
//! (`names::one`), so a type of the repository with the same name, or a name two namespaces may
//! bind (`ConnectionFactory` is RabbitMQ's and NATS's), is none. The methods that make a client of
//! another (`Build`, `CreateConnection`, `CreateModel`, `GetDatabase`) are listed in `STEPS`, as
//! the HTTP kind lists its clients.
//!
//! A channel name is evaluated like a URL: constants, `static readonly` fields, locals bound once
//! and the arguments of a wrapper's callers fold (`Code::expand`). A name that cannot be
//! evaluated, an environment or configuration read included, is an unresolved side.
//!
//! A callback is the handler it names: the lambda or the method group given to the call, or each
//! one attached to the event of the client the call's result is held by with `+=` (a consumer
//! per handler). Every other client polls, so its handler is the method holding the call that
//! names the topic or the queue (`consume`'s default), wherever the receive loop runs.
//!
//! A Service Bus send names the queue or topic of the `CreateSender` that made the sender, which
//! it counts only when nothing else writes the name (`Facts::bound`'s `name=` is one per write).
//! When that argument is a parameter, each caller of the method around it is a producer.

use super::message::{
    Handler, Message, Side, bind, channel_name as name, consume, produce, publish, queue_name,
    topics,
};
use crate::lang::{Bound, Invocation, Value, property};
use crate::names::{one, spellings};
use crate::resolve::{Code, Keywords, method as member};
use std::cell::OnceCell;
use std::collections::HashMap;

/// How many links a receiver is followed through: a local bound to a call on a local bound to
/// another, down to the construction or the declared type.
// ponytail: real clients are made a few steps deep; the limit keeps a crafted chain of thousands
// of locals (or of calls on one) from overflowing the stack.
const CHANNEL_CLIENT_DEPTH: usize = 32;

/// What a receiver may be: the client of a library, or something a client is made by.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Obj {
    KafkaProducer,
    KafkaConsumer,
    KafkaProducerBuilder,
    KafkaConsumerBuilder,
    RabbitFactory,
    RabbitConnection,
    RabbitChannel,
    RabbitConsumer,
    SqsClient,
    NatsFactory,
    NatsConnection,
    RedisMultiplexer,
    RedisDatabase,
    BusClient,
    BusSender,
}

/// The types of the libraries, by the full name the usings give them (generics dropped).
const TYPES: &[(&str, Obj)] = &[
    ("Confluent.Kafka.IProducer", Obj::KafkaProducer),
    ("Confluent.Kafka.IConsumer", Obj::KafkaConsumer),
    ("Confluent.Kafka.ProducerBuilder", Obj::KafkaProducerBuilder),
    ("Confluent.Kafka.ConsumerBuilder", Obj::KafkaConsumerBuilder),
    ("RabbitMQ.Client.ConnectionFactory", Obj::RabbitFactory),
    ("RabbitMQ.Client.IConnectionFactory", Obj::RabbitFactory),
    ("RabbitMQ.Client.IConnection", Obj::RabbitConnection),
    ("RabbitMQ.Client.IModel", Obj::RabbitChannel),
    ("RabbitMQ.Client.IChannel", Obj::RabbitChannel),
    (
        "RabbitMQ.Client.Events.EventingBasicConsumer",
        Obj::RabbitConsumer,
    ),
    (
        "RabbitMQ.Client.Events.AsyncEventingBasicConsumer",
        Obj::RabbitConsumer,
    ),
    ("Amazon.SQS.IAmazonSQS", Obj::SqsClient),
    ("Amazon.SQS.AmazonSQSClient", Obj::SqsClient),
    ("NATS.Client.ConnectionFactory", Obj::NatsFactory),
    ("NATS.Client.IConnection", Obj::NatsConnection),
    ("NATS.Client.Connection", Obj::NatsConnection),
    (
        "StackExchange.Redis.ConnectionMultiplexer",
        Obj::RedisMultiplexer,
    ),
    (
        "StackExchange.Redis.IConnectionMultiplexer",
        Obj::RedisMultiplexer,
    ),
    ("StackExchange.Redis.IDatabase", Obj::RedisDatabase),
    ("StackExchange.Redis.IDatabaseAsync", Obj::RedisDatabase),
    (
        "Azure.Messaging.ServiceBus.ServiceBusClient",
        Obj::BusClient,
    ),
    (
        "Azure.Messaging.ServiceBus.ServiceBusSender",
        Obj::BusSender,
    ),
];

/// The methods that make a client of another: what a method called on the first returns. A
/// builder's setters hand the builder back.
const STEPS: &[(Obj, &[&str], Obj)] = &[
    (Obj::KafkaProducerBuilder, &["Build"], Obj::KafkaProducer),
    (Obj::KafkaConsumerBuilder, &["Build"], Obj::KafkaConsumer),
    (
        Obj::RabbitFactory,
        &["CreateConnection", "CreateConnectionAsync"],
        Obj::RabbitConnection,
    ),
    (
        Obj::RabbitConnection,
        &["CreateModel", "CreateChannel", "CreateChannelAsync"],
        Obj::RabbitChannel,
    ),
    (Obj::NatsFactory, &["CreateConnection"], Obj::NatsConnection),
    (Obj::RedisMultiplexer, &["GetDatabase"], Obj::RedisDatabase),
    (Obj::BusClient, &["CreateSender"], Obj::BusSender),
    // The static factory, called on the type's own name.
    (
        Obj::RedisMultiplexer,
        &["Connect", "ConnectAsync"],
        Obj::RedisMultiplexer,
    ),
];

/// The setters of a builder, which hand it back (`SetValueSerializer`, `SetErrorHandler`).
const SETTER: &str = "Set";
/// The method of a task that hands back what it is awaited for.
const CONFIGURE: &str = "ConfigureAwait";

/// The methods a form is made of.
const METHODS: &[&str] = &[
    "Produce",
    "ProduceAsync",
    "Subscribe",
    "BasicPublish",
    "BasicPublishAsync",
    "QueueBind",
    "QueueBindAsync",
    "BasicConsume",
    "BasicConsumeAsync",
    "SendMessageAsync",
    "ReceiveMessageAsync",
    "Publish",
    "SubscribeAsync",
    "StreamAdd",
    "StreamAddAsync",
    "StreamReadGroup",
    "StreamReadGroupAsync",
    "SendMessagesAsync",
    "ScheduleMessageAsync",
    "ScheduleMessagesAsync",
    "CreateProcessor",
];

/// The events of a RabbitMQ consumer that hand it a delivery.
const DELIVERED: &[&str] = &["Received", "ReceivedAsync"];
/// The event of a NATS subscription that hands it a message.
const MESSAGE: &[&str] = &["MessageHandler"];
/// The event of a Service Bus processor that hands it a message.
const PROCESS: &[&str] = &["ProcessMessageAsync"];

pub(super) fn messages(code: &Code) -> Vec<Message> {
    if !code.pack().extensions().contains(&"cs") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        // The names that hold a call's result, built the first time a form needs one.
        let held: OnceCell<Held> = OnceCell::new();
        let holder = |n: usize| -> Option<&Bound> {
            let range = &code.call(path, n)?.range;
            held.get_or_init(|| held_by(code, path))
                .get(&(range.start, range.end))
                .copied()
        };
        // The position of a call in the file, by its range, and the producers each
        // `CreateSender` call names.
        let positions: OnceCell<HashMap<(usize, usize), usize>> = OnceCell::new();
        let position = |made: &Invocation| -> Option<usize> {
            positions
                .get_or_init(|| {
                    (0..facts.calls.len())
                        .filter_map(|n| Some((code.call(path, n)?.range.clone(), n)))
                        .map(|(range, n)| ((range.start, range.end), n))
                        .collect()
                })
                .get(&(made.range.start, made.range.end))
                .copied()
        };
        let mut made: HashMap<usize, Vec<Message>> = HashMap::new();
        for (n, stored) in facts.calls.iter().enumerate() {
            let method = member(&stored.callee);
            if stored.decorates.is_some()
                || stored.constructs
                || !stored.callee.contains('.')
                || !METHODS.contains(&method)
            {
                continue;
            }
            let Some(receiver) = code.receiver(path, n) else {
                continue;
            };
            let Some(of) = client(code, path, &stored.scope, receiver, 0) else {
                continue;
            };
            match (of, method) {
                (Obj::KafkaProducer, "Produce" | "ProduceAsync") => {
                    let read =
                        |a: &[Value], k: &Keywords| Some(vec![topic(arg(a, k, 0, &["topic"]))?]);
                    out.extend(produce(code, path, n, "kafka", &read));
                }
                (Obj::KafkaConsumer, "Subscribe") => {
                    let read =
                        |a: &[Value], k: &Keywords| topics(arg(a, k, 0, &["topic", "topics"]));
                    out.extend(consume(code, path, n, "kafka", &read, None));
                }
                (Obj::RabbitChannel, "BasicPublish" | "BasicPublishAsync") => {
                    // `String.Empty` is read as the call writes it: folded, it is unknown.
                    let blank = Value::Str(Vec::new());
                    let default = code.call(path, n).is_some_and(|call| {
                        let exchange = arg(&call.args, &call.kwargs, 0, &["exchange"]);
                        empty(code, path, &stored.scope, exchange)
                    });
                    let read = |a: &[Value], k: &Keywords| {
                        let exchange = match default {
                            true => &blank,
                            false => arg(a, k, 0, &["exchange"]),
                        };
                        Some((name(exchange)?, name(arg(a, k, 1, &["routingKey"]))?))
                    };
                    out.extend(publish(code, path, n, &read));
                }
                (Obj::RabbitChannel, "QueueBind" | "QueueBindAsync") => {
                    let read = |a: &[Value], k: &Keywords| {
                        let queue = name(arg(a, k, 0, &["queue"]))?;
                        let exchange = name(arg(a, k, 1, &["exchange"]))?;
                        Some((queue, exchange, name(arg(a, k, 2, &["routingKey"]))?))
                    };
                    out.extend(bind(code, path, n, &read));
                }
                (Obj::RabbitChannel, "BasicConsume" | "BasicConsumeAsync") => {
                    // The queue is the first argument, or the second after the consumer that
                    // some overloads take first: the one the first argument's type tells.
                    let consumer_first = (code.call(path, n).and_then(|c| c.args.first()))
                        .is_some_and(|first| {
                            client(code, path, &stored.scope, first, 0) == Some(Obj::RabbitConsumer)
                        });
                    let read = |a: &[Value], k: &Keywords| {
                        let at = usize::from(consumer_first);
                        Some(vec![name(arg(a, k, at, &["queue"]))?])
                    };
                    for handler in each(delivery(code, path, n)) {
                        out.extend(consume(code, path, n, "amqp", &read, handler));
                    }
                }
                (Obj::SqsClient, "SendMessageAsync") => {
                    out.extend(produce(code, path, n, "sqs", &queue));
                }
                (Obj::SqsClient, "ReceiveMessageAsync") => {
                    out.extend(consume(code, path, n, "sqs", &queue, None));
                }
                (Obj::NatsConnection, "Publish") => {
                    out.extend(produce(code, path, n, "nats", &first));
                }
                (Obj::NatsConnection, "SubscribeAsync") => {
                    let handlers = match callback(code, path, n) {
                        Some(handler) => vec![handler],
                        None => (holder(n).map(|held| {
                            let name = held.name.trim_end_matches('=');
                            attached(code, path, &held.scope, name, MESSAGE)
                        }))
                        .unwrap_or_default(),
                    };
                    for handler in each(handlers) {
                        out.extend(consume(code, path, n, "nats", &first, handler));
                    }
                }
                (Obj::BusSender, "SendMessageAsync" | "SendMessagesAsync")
                | (Obj::BusSender, "ScheduleMessageAsync" | "ScheduleMessagesAsync") => {
                    // The name is where the sender was made: the send reads no argument.
                    let Some(at) = sender(code, path, &stored.scope, receiver).and_then(position)
                    else {
                        let unknown: Vec<Option<String>> = vec![None];
                        out.extend(produce(code, path, n, "servicebus", &|_, _| {
                            Some(unknown.clone())
                        }));
                        continue;
                    };
                    let first_send = !made.contains_key(&at);
                    let made = made
                        .entry(at)
                        .or_insert_with(|| produce(code, path, at, "servicebus", &first));
                    for m in made.iter() {
                        // A name read where the sender stands is that send's; one a caller of
                        // its method passes is that caller's, once for all the sends.
                        let here = m.call == at && m.path == path;
                        if here || first_send {
                            out.push(Message {
                                broker: m.broker,
                                name: m.name.clone(),
                                path: m.path.clone(),
                                call: if here { n } else { m.call },
                                role: Side::Producer,
                            });
                        }
                    }
                }
                (Obj::BusClient, "CreateProcessor") => {
                    let handlers = (holder(n).map(|held| {
                        let name = held.name.trim_end_matches('=');
                        attached(code, path, &held.scope, name, PROCESS)
                    }))
                    .unwrap_or_default();
                    for handler in each(handlers) {
                        out.extend(consume(code, path, n, "servicebus", &first, handler));
                    }
                }
                (Obj::RedisDatabase, "StreamAdd" | "StreamAddAsync") => {
                    out.extend(produce(code, path, n, "redis", &first));
                }
                (Obj::RedisDatabase, "StreamReadGroup" | "StreamReadGroupAsync") => {
                    out.extend(consume(code, path, n, "redis", &first, None));
                }
                _ => {}
            }
        }
    }
    out
}

/// The queue the URL an SQS call is given ends in: the URL as the first argument, or the
/// `QueueUrl` of the request it is given (`new SendMessageRequest { QueueUrl = url }`, in place
/// or in a local). A request without one names no queue.
fn queue(args: &[Value], named: &Keywords) -> Option<Vec<Option<String>>> {
    let first = arg(args, named, 0, &["queueUrl", "request"]);
    let url = match first {
        Value::New(_, parts) => (parts.iter()).find_map(|p| match p {
            Value::Object(set) => property(set, "QueueUrl").ok().flatten(),
            _ => None,
        }),
        url => Some(url),
    };
    Some(vec![queue_name(url)?])
}

/// The handlers of the consumer the `BasicConsume` at `n` of `path` is given: each one attached
/// to its `Received` event (`+=`).
fn delivery<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Vec<Handler> {
    let Some(call) = code.call(path, n) else {
        return Vec::new();
    };
    let given = (call.args.iter()).chain(call.kwargs.iter().map(|(_, v)| v));
    let consumer = given.into_iter().find_map(|a| match a {
        Value::Ref(name) if client(code, path, &call.scope, a, 0) == Some(Obj::RabbitConsumer) => {
            Some(name.as_str())
        }
        _ => None,
    });
    let Some(consumer) = consumer else {
        return Vec::new();
    };
    let holder = consumer.strip_prefix("this.").unwrap_or(consumer);
    match code.written(path, &call.scope, holder) {
        Some((at, within, _)) if at == path => attached(code, path, within, holder, DELIVERED),
        _ => Vec::new(),
    }
}

/// A consumer per handler, or one with the default handler (the method holding the call) when
/// the call has none.
fn each(handlers: Vec<Handler>) -> Vec<Option<Handler>> {
    match handlers.is_empty() {
        true => vec![None],
        false => handlers.into_iter().map(Some).collect(),
    }
}

/// The `CreateSender` call that made the Service Bus sender `receiver`, written in `scope` of
/// `path`: in place, or held by a local, a field or a property written once. `None` when no such
/// call is found.
fn sender<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    receiver: &'a Value,
) -> Option<&'a Invocation> {
    let (within, made) = match receiver {
        Value::Call(made) => (scope, made),
        Value::Ref(held) => {
            let held = held.strip_prefix("this.").unwrap_or(held);
            let bound = match code.local(path, scope, held) {
                Some(bound) => bound,
                None => {
                    // Every write of the name is a `name=`: the one made by `CreateSender` is
                    // the sender only when no other writes it.
                    let (at, within, _) = code.written(path, scope, held)?;
                    let mut assigned = code.bounds(at, within, &format!("{held}=")).into_iter();
                    let only = assigned.next().filter(|_| at == path)?;
                    assigned.next().is_none().then_some(only)?
                }
            };
            match &bound.value {
                Value::Call(made) => (bound.within.as_str(), made),
                _ => return None,
            }
        }
        _ => return None,
    };
    let from = client(code, path, within, made.receiver.as_deref()?, 0);
    (from == Some(Obj::BusClient) && member(&made.callee) == "CreateSender").then_some(made)
}

/// The handlers attached with `+=` to the events `events` of the name `holder`, declared in
/// `within` of `path`, in the order they are attached: each lambda or method group.
fn attached<'a>(
    code: &Code<'a>,
    path: &'a str,
    within: &'a str,
    holder: &str,
    events: &[&str],
) -> Vec<Handler> {
    (events.iter())
        .flat_map(|event| code.bounds(path, within, &format!("{holder}.{event}+=")))
        .filter_map(|handler| function(code, path, &handler.within, &handler.value))
        .collect()
}

/// The callback the call at `n` of `path` is given after its subject: a lambda written in place
/// or a method of the repository.
fn callback<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Option<Handler> {
    let call = code.call(path, n)?;
    function(
        code,
        path,
        &call.scope,
        call.args.iter().skip(1).next_back()?,
    )
}

/// The handler a function value is: the lambda as the inline definition it makes, a method group
/// as the definition it names.
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
        Value::Ref(method) => code.definition(path, scope, method).map(|t| Handler::Def {
            path: t.path.into(),
            name: t.name.into(),
        }),
        _ => None,
    }
}

/// The names that hold a call's result in `path`, by the call's range.
type Held<'a> = HashMap<(usize, usize), &'a Bound>;

fn held_by<'a>(code: &Code<'a>, path: &'a str) -> Held<'a> {
    (code.bindings(path).iter())
        .filter_map(|b| match &b.value {
            Value::Call(call) => Some(((call.range.start, call.range.end), b)),
            _ => None,
        })
        .collect()
}

/// What `value`, written in `scope` of `path`, is: a client of a library or what makes one. A
/// name is its written type, else the construction a local is bound to, else a type's own name
/// (a static factory: `ConnectionMultiplexer.Connect`).
fn client<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
    depth: usize,
) -> Option<Obj> {
    if depth >= CHANNEL_CLIENT_DEPTH {
        return None;
    }
    match value {
        Value::New(ty, _) => type_of(code, path, scope, ty),
        Value::Ref(written) => {
            let written = written.strip_prefix("this.").unwrap_or(written);
            if let Some((at, within, ty)) = code.written(path, scope, written) {
                return type_of(code, at, within, ty);
            }
            match code.local(path, scope, written) {
                Some(bound) => client(code, path, &bound.within, &bound.value, depth + 1),
                None => type_of(code, path, scope, written),
            }
        }
        Value::Call(call) => {
            let made = client(code, path, scope, call.receiver.as_deref()?, depth + 1)?;
            let method = member(&call.callee);
            (STEPS.iter())
                .find(|(from, methods, _)| *from == made && methods.contains(&method))
                .map(|(.., to)| *to)
                .or_else(|| setter(made, method))
                // Awaiting without capturing the context is awaiting.
                .or((method == CONFIGURE).then_some(made))
        }
        _ => None,
    }
}

/// A builder's setter hands the builder back.
fn setter(made: Obj, method: &str) -> Option<Obj> {
    let builder = matches!(made, Obj::KafkaProducerBuilder | Obj::KafkaConsumerBuilder);
    (builder && method.starts_with(SETTER)).then_some(made)
}

/// The library type `ty`, written in `scope` of `path`, is: in full, or the one spelling in the
/// table the usings give it.
fn type_of<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, ty: &'a str) -> Option<Obj> {
    let found = |full: &str| TYPES.iter().find(|(t, _)| *t == full).map(|(_, o)| *o);
    if let Some(obj) = found(ty) {
        return Some(obj);
    }
    found(one(&spellings(code, path, scope, ty), |t| {
        found(t).is_some()
    })?)
}

/// Whether `value`, written in `scope` of `path`, is `String.Empty` of `System.String`, as the
/// usings bind the name (`string.Empty` is folded by the pack).
// ponytail: only an exchange the form's call writes is read; one a wrapper's caller passes folds
// to unknown, so it is an unresolved producer.
fn empty<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, value: &Value) -> bool {
    let Value::Ref(spelled) = value else {
        return false;
    };
    match spelled.strip_suffix(".Empty") {
        Some("System.String") => true,
        Some("String") => one(&spellings(code, path, scope, "String"), |t| {
            t == "System.String"
        })
        .is_some(),
        _ => false,
    }
}

/// The argument at position `at`, or the one named by any of `names`; `Undefined` when the call
/// leaves it out.
fn arg<'v>(args: &'v [Value], named: &'v Keywords, at: usize, names: &[&str]) -> &'v Value {
    let named = named.iter().find(|(k, _)| names.contains(&k.as_str()));
    named
        .map(|(_, v)| v)
        .or_else(|| args.get(at))
        .unwrap_or(&Value::Undefined)
}

/// The one channel the first argument names: a subject, a stream key, a queue or a topic.
fn first(args: &[Value], named: &Keywords) -> Option<Vec<Option<String>>> {
    let names = [
        "subject",
        "key",
        "queueName",
        "topicName",
        "queueOrTopicName",
    ];
    Some(vec![name(arg(args, named, 0, &names))?])
}

/// A Kafka topic name (see `message::topic`).
fn topic(value: &Value) -> Option<Option<String>> {
    super::message::topic(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::csharp::CSharp;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    const USINGS: &str = "using System;\nusing System.Threading.Tasks;\n";

    /// The messages of the class body `body` in `A/C.cs` under `usings`, beside other files of
    /// the repository, as lines.
    fn lines_with(usings: &str, body: &str, others: &[(&str, &str)]) -> Vec<String> {
        let src = format!("{USINGS}{usings}\nnamespace A;\npublic class C {{\n{body}\n}}\n");
        let mut parsed = vec![("A/C.cs", CSharp.parse("A/C.cs", src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, CSharp.parse(path, source.as_bytes()).unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &CSharp);
        let code = Code::new(&resolver, &files, &CSharp);
        crate::channel::message::lines(messages(&code))
    }

    fn lines(usings: &str, body: &str) -> Vec<String> {
        lines_with(usings, body, &[])
    }

    const KAFKA: &str = "using Confluent.Kafka;\n";

    #[test]
    fn a_kafka_client_produces_and_subscribes_by_its_method() {
        let body = "IProducer<string, string> _producer;\n\
            async Task Send() {\n\
              _producer.Produce(\"orders.created\", new Message<string, string>());\n\
              await _producer.ProduceAsync(\"orders.shipped\", new Message<string, string>());\n\
              await _producer.ProduceAsync(topic: Environment.GetEnvironmentVariable(\"T\"), message: null);\n\
            }\n\
            void Listen() {\n\
              using var consumer = new ConsumerBuilder<string, string>(new ConsumerConfig()).SetErrorHandler((_, e) => {}).Build();\n\
              consumer.Subscribe(\"orders.created\");\n\
              consumer.Subscribe(new[] { \"a\", \"b\" });\n\
            }\n";
        assert_eq!(
            lines(KAFKA, body),
            [
                "kafka orders.created producer",
                "kafka orders.shipped producer",
                "kafka unresolved producer",
                "kafka orders.created consumer A/C.cs::A.C.Listen()",
                "kafka a consumer A/C.cs::A.C.Listen()",
                "kafka b consumer A/C.cs::A.C.Listen()",
            ]
        );
    }

    /// `lines`, with the position an inline handler is named after masked.
    fn masked(lines: Vec<String>) -> Vec<String> {
        let mask = |line: String| {
            let Some((head, tail)) = line.split_once("@") else {
                return line;
            };
            let rest = tail.trim_start_matches(|c: char| c.is_ascii_digit());
            format!("{head}@N{rest}")
        };
        lines.into_iter().map(mask).collect()
    }

    const RABBIT: &str = "using RabbitMQ.Client;\nusing RabbitMQ.Client.Events;\n";

    #[test]
    fn a_rabbitmq_channel_publishes_binds_and_consumes() {
        let body = "IChannel _channel;\n\
            async Task Send() {\n\
              await _channel.BasicPublishAsync(exchange: string.Empty, routingKey: \"orders\", body: null);\n\
              await _channel.BasicPublishAsync(\"events\", \"order.created\", false, null, null);\n\
              await _channel.QueueBindAsync(\"audit\", \"events\", \"order.#\");\n\
              await _channel.BasicPublishAsync(\"events\", Environment.GetEnvironmentVariable(\"K\"), false, null, null);\n\
            }\n\
            async Task Listen() {\n\
              var factory = new ConnectionFactory { HostName = \"h\" };\n\
              await using var connection = await factory.CreateConnectionAsync();\n\
              await using var channel = await connection.CreateChannelAsync();\n\
              var consumer = new AsyncEventingBasicConsumer(channel);\n\
              consumer.ReceivedAsync += async (model, ea) => { await Task.Yield(); };\n\
              await channel.BasicConsumeAsync(queue: \"orders\", autoAck: true, consumer: consumer);\n\
              var other = new AsyncEventingBasicConsumer(channel);\n\
              other.ReceivedAsync += OnReceived;\n\
              await channel.BasicConsumeAsync(\"audit\", true, other);\n\
              await channel.BasicConsumeAsync(\"polled\", true, new AsyncEventingBasicConsumer(channel));\n\
            }\n\
            Task OnReceived(object s, BasicDeliverEventArgs e) => Task.CompletedTask;\n";
        assert_eq!(
            masked(lines(RABBIT, body)),
            [
                "amqp orders producer",
                "amqp order.created publish events",
                "amqp audit binding events order.#",
                "amqp unresolved publish events",
                "amqp orders consumer inline A/C.cs::A.C.Listen().@N",
                "amqp audit consumer A/C.cs::A.C.OnReceived(object,BasicDeliverEventArgs)",
                "amqp polled consumer A/C.cs::A.C.Listen()",
            ]
        );
    }

    const SQS: &str = "using Amazon.SQS;\nusing Amazon.SQS.Model;\n";

    #[test]
    fn an_sqs_client_sends_to_and_receives_from_the_queue_its_url_ends_in() {
        let body = "IAmazonSQS _sqs;\n\
            const string Base = \"https://sqs.us-east-1.amazonaws.com/123/\";\n\
            async Task Send(string dynamicUrl) {\n\
              await _sqs.SendMessageAsync(\"https://sqs.us-east-1.amazonaws.com/123/orders\", \"body\");\n\
              await _sqs.SendMessageAsync(new SendMessageRequest { QueueUrl = Base + \"payments\", MessageBody = \"b\" });\n\
              var request = new SendMessageRequest { QueueUrl = Environment.GetEnvironmentVariable(\"Q\"), MessageBody = \"b\" };\n\
              await _sqs.SendMessageAsync(request);\n\
              await _sqs.SendMessageAsync(queueUrl: Base + \"emails\", messageBody: \"b\");\n\
            }\n\
            async Task Poll() {\n\
              var client = new AmazonSQSClient();\n\
              await client.ReceiveMessageAsync(\"https://sqs.us-east-1.amazonaws.com/123/orders\");\n\
              await client.ReceiveMessageAsync(new ReceiveMessageRequest { QueueUrl = Base + \"payments\", MaxNumberOfMessages = 10 });\n\
            }\n";
        assert_eq!(
            lines(SQS, body),
            [
                "sqs orders producer",
                "sqs payments producer",
                "sqs unresolved producer",
                "sqs emails producer",
                "sqs orders consumer A/C.cs::A.C.Poll()",
                "sqs payments consumer A/C.cs::A.C.Poll()",
            ]
        );
    }

    const NATS: &str = "using NATS.Client;\n";

    #[test]
    fn a_nats_connection_publishes_and_subscribes_with_the_callback_it_names() {
        let body = "IConnection _nats;\n\
            void Send() { _nats.Publish(\"orders.created\", new byte[0]); }\n\
            void Listen() {\n\
              IConnection c = new ConnectionFactory().CreateConnection(\"nats://h\");\n\
              c.SubscribeAsync(\"orders.created\", (sender, args) => { });\n\
              c.SubscribeAsync(\"orders.shipped\", \"workers\", OnShipped);\n\
              var sub = c.SubscribeAsync(\"orders.*\");\n\
              sub.MessageHandler += (sender, args) => { };\n\
              sub.Start();\n\
              c.SubscribeAsync(\"orders.polled\");\n\
            }\n\
            void OnShipped(object sender, MsgHandlerEventArgs args) { }\n";
        assert_eq!(
            masked(lines(NATS, body)),
            [
                "nats orders.created producer",
                "nats orders.created consumer inline A/C.cs::A.C.Listen().@N",
                "nats orders.shipped consumer A/C.cs::A.C.OnShipped(object,MsgHandlerEventArgs)",
                "nats orders.* consumer inline A/C.cs::A.C.Listen().@N",
                "nats orders.polled consumer A/C.cs::A.C.Listen()",
            ]
        );
    }

    const REDIS: &str = "using StackExchange.Redis;\n";

    #[test]
    fn a_redis_database_adds_to_and_reads_a_stream_group() {
        let body = "IDatabase _db;\n\
            async Task Send() { await _db.StreamAddAsync(\"orders\", \"k\", \"v\"); _db.StreamAdd(\"audit\", \"k\", \"v\"); }\n\
            async Task Read() {\n\
              var mux = await ConnectionMultiplexer.ConnectAsync(\"h\");\n\
              var db = mux.GetDatabase();\n\
              await db.StreamReadGroupAsync(\"orders\", \"g\", \"c1\", \">\");\n\
              db.StreamReadGroup(key: \"audit\", groupName: \"g\", consumerName: \"c1\");\n\
              db.StreamReadGroup(new[] { new StreamPosition(\"x\", \">\") }, \"g\", \"c1\");\n\
              db.StreamRead(\"orders\", \"0-0\");\n\
            }\n";
        assert_eq!(
            lines(REDIS, body),
            [
                "redis orders producer",
                "redis audit producer",
                "redis orders consumer A/C.cs::A.C.Read()",
                "redis audit consumer A/C.cs::A.C.Read()",
                "redis unresolved consumer A/C.cs::A.C.Read()",
            ]
        );
    }

    const BUS: &str = "using Azure.Messaging.ServiceBus;\n";

    #[test]
    fn a_service_bus_sender_and_processor_are_named_by_the_queue_or_topic_they_are_made_for() {
        let body = "const string Audit = \"audit\";\n\
            ServiceBusClient _client;\n\
            ServiceBusSender _sender;\n\
            ServiceBusSender _other;\n\
            C(ServiceBusClient client, ServiceBusSender injected) {\n\
              _client = client;\n\
              _sender = client.CreateSender(\"orders\");\n\
              _other = injected;\n\
            }\n\
            async Task Send(ServiceBusMessage m) {\n\
              await _sender.SendMessageAsync(m);\n\
              var audit = _client.CreateSender(Audit);\n\
              await audit.SendMessagesAsync(new[] { m });\n\
              await _client.CreateSender(\"events\").SendMessageAsync(m);\n\
              await _other.SendMessageAsync(m);\n\
            }\n\
            async Task Listen() {\n\
              await using var client = new ServiceBusClient(\"cs\");\n\
              var processor = client.CreateProcessor(\"orders\", new ServiceBusProcessorOptions());\n\
              processor.ProcessMessageAsync += async args => { };\n\
              processor.ProcessErrorAsync += OnError;\n\
              var onTopic = client.CreateProcessor(\"events\", \"audit-subscription\");\n\
              onTopic.ProcessMessageAsync += OnEvent;\n\
              var polled = client.CreateProcessor(\"polled\");\n\
              await processor.StartProcessingAsync();\n\
            }\n\
            Task OnError(ProcessErrorEventArgs e) => Task.CompletedTask;\n\
            Task OnEvent(ProcessMessageEventArgs e) => Task.CompletedTask;\n";
        assert_eq!(
            masked(lines(BUS, body)),
            [
                "servicebus orders producer",
                "servicebus audit producer",
                "servicebus events producer",
                "servicebus unresolved producer",
                "servicebus orders consumer inline A/C.cs::A.C.Listen().@N",
                "servicebus events consumer A/C.cs::A.C.OnEvent(ProcessMessageEventArgs)",
                "servicebus polled consumer A/C.cs::A.C.Listen()",
            ]
        );
    }

    #[test]
    fn a_service_bus_sender_written_more_than_once_is_unknown() {
        let unresolved = ["servicebus unresolved producer"];
        // Another constructor hands one in: the field is made by `CreateSender` or given.
        let given = "ServiceBusSender _sender;\n\
            C(ServiceBusClient c) { _sender = c.CreateSender(\"orders\"); }\n\
            internal C(ServiceBusSender s) { _sender = s; }\n\
            async Task Send(ServiceBusMessage m) { await _sender.SendMessageAsync(m); }\n";
        assert_eq!(lines(BUS, given), unresolved);
        // A setter replaces what the constructor made.
        let setter = "ServiceBusSender _sender;\n\
            C(ServiceBusClient c) { _sender = c.CreateSender(\"orders\"); }\n\
            public void Use(ServiceBusSender s) => _sender = s;\n\
            async Task Send(ServiceBusMessage m) { await _sender.SendMessageAsync(m); }\n";
        assert_eq!(lines(BUS, setter), unresolved);
        // A field initializer and a later assignment are two writes.
        let initialized = "static ServiceBusClient S;\n\
            ServiceBusSender _sender = S.CreateSender(\"init\");\n\
            void Reset() { _sender = S.CreateSender(\"other\"); }\n\
            async Task Send(ServiceBusMessage m) { await _sender.SendMessageAsync(m); }\n";
        assert_eq!(lines(BUS, initialized), unresolved);
        // A local made twice: both sends are unknown, neither is the last one.
        let local = "ServiceBusClient _c;\n\
            async Task Send(ServiceBusMessage m) {\n\
              ServiceBusSender s = _c.CreateSender(\"first\");\n\
              await s.SendMessageAsync(m);\n\
              s = _c.CreateSender(\"second\");\n\
              await s.SendMessageAsync(m);\n\
            }\n";
        assert_eq!(lines(BUS, local), [unresolved[0], unresolved[0]]);
        // Written once, by whatever form of assignment, it is that one.
        let once = "ServiceBusSender _sender;\n\
            C(ServiceBusClient c) { this._sender = c.CreateSender(\"orders\"); }\n\
            async Task Send(ServiceBusMessage m) { await _sender.SendMessageAsync(m); }\n";
        assert_eq!(lines(BUS, once), ["servicebus orders producer"]);
        // Handed out by a `ref` parameter, it is written there too.
        let by_ref = "ServiceBusSender _sender;\n\
            C(ServiceBusClient c) { _sender = c.CreateSender(\"orders\"); Fill(ref _sender); }\n\
            void Fill(ref ServiceBusSender s) { }\n\
            async Task Send(ServiceBusMessage m) { await _sender.SendMessageAsync(m); }\n";
        assert_eq!(lines(BUS, by_ref), unresolved);
    }

    #[test]
    fn a_service_bus_sender_made_for_a_parameter_is_named_by_its_callers() {
        // The sender is made for the queue the method is given: each caller is a producer, and
        // two sends on the one sender name them once.
        let body = "ServiceBusClient _c;\n\
            async Task Send(string queue, ServiceBusMessage m) {\n\
              var s = _c.CreateSender(queue);\n\
              await s.SendMessageAsync(m);\n\
              await s.SendMessageAsync(m);\n\
            }\n\
            async Task F(ServiceBusMessage m) { await Send(\"orders\", m); await Send(\"audit\", m); }\n";
        assert_eq!(
            lines(BUS, body),
            ["servicebus orders producer", "servicebus audit producer"]
        );
        // Through a second wrapper, and in a constructor that makes the field.
        let deep = "ServiceBusSender _sender;\n\
            C(ServiceBusClient c, string queue) { _sender = c.CreateSender(queue); }\n\
            async Task Send(ServiceBusMessage m) { await _sender.SendMessageAsync(m); }\n\
            static C Orders(ServiceBusClient c) => new C(c, \"orders\");\n";
        assert_eq!(lines(BUS, deep), ["servicebus orders producer"]);
        // A sender nobody makes for a name is unresolved where it stands.
        let alone = "ServiceBusClient _c;\n\
            async Task Send(string queue, ServiceBusMessage m) {\n\
              await _c.CreateSender(queue).SendMessageAsync(m);\n\
            }\n";
        assert_eq!(lines(BUS, alone), ["servicebus unresolved producer"]);
    }

    #[test]
    fn every_handler_attached_to_an_event_is_a_consumer_of_the_channel() {
        // Spec #20: several consumers of a channel are all linked, never an ambiguity.
        let body = "async Task Listen(IChannel channel) {\n\
              var consumer = new AsyncEventingBasicConsumer(channel);\n\
              consumer.ReceivedAsync += First;\n\
              consumer.ReceivedAsync += Second;\n\
              await channel.BasicConsumeAsync(\"q3\", true, consumer);\n\
              var bus = new ServiceBusClient(\"cs\");\n\
              var processor = bus.CreateProcessor(\"jobs\");\n\
              processor.ProcessMessageAsync += Run;\n\
              processor.ProcessMessageAsync += Audit;\n\
            }\n\
            Task First(object s, BasicDeliverEventArgs e) => Task.CompletedTask;\n\
            Task Second(object s, BasicDeliverEventArgs e) => Task.CompletedTask;\n\
            Task Run(ProcessMessageEventArgs e) => Task.CompletedTask;\n\
            Task Audit(ProcessMessageEventArgs e) => Task.CompletedTask;\n";
        assert_eq!(
            lines(&format!("{RABBIT}{BUS}"), body),
            [
                "amqp q3 consumer A/C.cs::A.C.First(object,BasicDeliverEventArgs)",
                "amqp q3 consumer A/C.cs::A.C.Second(object,BasicDeliverEventArgs)",
                "servicebus jobs consumer A/C.cs::A.C.Run(ProcessMessageEventArgs)",
                "servicebus jobs consumer A/C.cs::A.C.Audit(ProcessMessageEventArgs)",
            ]
        );
    }

    #[test]
    fn configure_await_hands_the_client_back() {
        let body = "async Task Send(ConnectionFactory f) {\n\
              var conn = await f.CreateConnectionAsync().ConfigureAwait(false);\n\
              var ch = await conn.CreateChannelAsync().ConfigureAwait(false);\n\
              await ch.BasicPublishAsync(\"\", \"q1\", null).ConfigureAwait(false);\n\
            }\n";
        assert_eq!(lines(RABBIT, body), ["amqp q1 producer"]);
    }

    #[test]
    fn the_queue_of_a_consumer_first_basic_consume_is_found_by_the_consumer_type() {
        // The extension overload takes the consumer first; the consumer may be a field or a
        // parameter, not only built in place or held by a local.
        let body = "AsyncEventingBasicConsumer _consumer;\n\
            async Task A(IChannel ch) { await ch.BasicConsumeAsync(_consumer, \"q1\", true); }\n\
            async Task B(IChannel ch, EventingBasicConsumer given) { await ch.BasicConsumeAsync(given, \"q2\", true); }\n\
            async Task C(IChannel ch) { await ch.BasicConsumeAsync(\"q3\", true, _consumer); }\n\
            async Task D(IChannel ch, Func<IChannel> make) { await ch.BasicConsumeAsync(make(), \"q4\", true); }\n";
        assert_eq!(
            lines(RABBIT, body),
            [
                "amqp q1 consumer A/C.cs::A.C.A(IChannel)",
                "amqp q2 consumer A/C.cs::A.C.B(IChannel,EventingBasicConsumer)",
                "amqp q3 consumer A/C.cs::A.C.C(IChannel)",
                "amqp unresolved consumer A/C.cs::A.C.D(IChannel,Func<IChannel>)",
            ]
        );
    }

    #[test]
    fn the_empty_string_type_is_the_default_exchange_through_the_usings() {
        let body = "IChannel _ch;\n\
            async Task A() { await _ch.BasicPublishAsync(String.Empty, \"q1\", null); }\n\
            async Task B() { await _ch.BasicPublishAsync(System.String.Empty, \"q2\", null); }\n\
            async Task C() { await _ch.BasicPublishAsync(exchange: string.Empty, routingKey: \"q3\", body: null); }\n";
        assert_eq!(
            lines(RABBIT, body),
            ["amqp q1 producer", "amqp q2 producer", "amqp q3 producer"]
        );
        // Without `using System;`, `String` is whatever else binds the name.
        let own =
            "namespace A;\npublic static class String { public const string Empty = \"x\"; }\n";
        let other = "IChannel _ch;\n\
            async Task A() { await _ch.BasicPublishAsync(String.Empty, \"q1\", null); }\n";
        assert_eq!(
            lines_with(RABBIT, other, &[("A/Own.cs", own)]),
            ["amqp q1 publish x"]
        );
    }

    #[test]
    fn a_call_on_anything_but_a_library_client_is_no_form() {
        // The same names on a type of the repository, on a client of the wrong kind, on a value
        // made by the repository's type, and with the library's own name hidden by a type of the
        // repository.
        let body = "IProducer<string, string> _hidden;\n\
            void F(Bus other, IConsumer<string, string> c) {\n\
              _hidden.Produce(\"a\", null);\n\
              other.Subscribe(\"b\");\n\
              c.Produce(\"c\", null);\n\
              new Bus().Produce(\"d\", null);\n\
              var made = new Bus();\n\
              made.Produce(\"e\", null);\n\
            }\n";
        let bus = "public class Bus { public void Produce(string t, object m) {} public void Subscribe(string t) {} }\n";
        let hiding = format!(
            "namespace A;\npublic interface IProducer<K, V> {{ void Produce(string t, object m); }}\n{bus}"
        );
        assert_eq!(
            lines_with(KAFKA, body, &[("A/Own.cs", &hiding)]),
            Vec::<String>::new()
        );
        // Without the repository's own `IProducer`, the library's is the one `_hidden` is.
        let plain = format!("namespace A;\n{bus}");
        assert_eq!(
            lines_with(KAFKA, body, &[("A/Own.cs", &plain)]),
            ["kafka a producer"]
        );
    }

    #[test]
    fn a_type_written_in_full_needs_no_using() {
        let body = "Confluent.Kafka.IProducer<string, string> _p;\n\
            void F() { _p.Produce(\"a\", null); }\n";
        assert_eq!(lines("", body), ["kafka a producer"]);
    }

    #[test]
    fn two_namespaces_that_bind_a_type_name_make_it_none() {
        // `IConnection` is RabbitMQ's and NATS's: with both imported it is neither.
        let body = "IConnection _c;\n\
            void F() { _c.Publish(\"a\", null); }\n";
        let both = format!("{RABBIT}{NATS}");
        assert_eq!(lines(&both, body), Vec::<String>::new());
        assert_eq!(lines(NATS, body), ["nats a producer"]);
    }

    #[test]
    fn a_name_assigned_again_is_unknown() {
        let body = "void F(IProducer<string, string> typed) {\n\
              var topic = \"a\";\n\
              topic = \"b\";\n\
              typed.Produce(topic, null);\n\
              var made = new ProducerBuilder<string, string>(null).Build();\n\
              made = Other();\n\
              made.Produce(\"c\", null);\n\
            }\n\
            IProducer<string, string> Other() => null;\n";
        // The producer made and then replaced is no client; the topic written twice is unknown.
        assert_eq!(lines(KAFKA, body), ["kafka unresolved producer"]);
    }

    #[test]
    fn a_wrapper_names_the_topics_its_callers_pass() {
        let body = "IProducer<string, string> _p;\n\
            void Publish(string topic) { _p.Produce(topic, null); }\n\
            void F() { Publish(\"a\"); Publish(\"b\"); }\n";
        assert_eq!(lines(KAFKA, body), ["kafka a producer", "kafka b producer"]);
    }

    #[test]
    fn a_receiver_is_followed_to_the_receiver_depth_only() {
        // A client held by `n` locals, each bound to the one before.
        let held = |n: usize| {
            let mut body = String::from(
                "void F() {\nvar x0 = new ProducerBuilder<string, string>(null).Build();\n",
            );
            for i in 1..=n {
                body += &format!("var x{i} = x{};\n", i - 1);
            }
            body + &format!("x{n}.Produce(\"t\", null);\n}}\n")
        };
        assert_eq!(lines(KAFKA, &held(5)), ["kafka t producer"]);
        let start = std::time::Instant::now();
        assert_eq!(
            lines(KAFKA, &held(CHANNEL_CLIENT_DEPTH + 2)),
            Vec::<String>::new()
        );
        assert_eq!(lines(KAFKA, &held(4_000)), Vec::<String>::new());
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
    }
}
