//! The channel kind's Java forms (spec #26): Spring Kafka (`kafka`), Spring AMQP (`amqp`), Spring
//! JMS (`jms`), the SQS client of the AWS SDK v2 (`sqs`), jnats (`nats`) and the Jedis and Lettuce
//! Redis clients (`redis`).
//!
//! A client counts by the type the source writes for its receiver (a field, a parameter, a
//! local: `Follow::Declarations`, as the HTTP kind reads a RestTemplate), and a listener
//! annotation by `Code::outside`: each through the one spelling in this module's table
//! (`jvm::names::one`), so a class of the repository with the same name, or a name two on-demand
//! imports may bind, is none. A name is evaluated like a URL: constants, `final` fields and the
//! arguments of a wrapper's callers fold (`Code::expand`), a field or a constructor parameter
//! with Spring's `@Value("${key}")` holds the key from the module's configuration
//! (`jvm::spring::inject`), and an annotation's text has its placeholders resolved the same way
//! (`jvm::spring::resolved`). A name that cannot be evaluated is an unresolved side.
//!
//! An annotated listener's handler is the annotated method. Every other client polls (or calls
//! back through a lambda, which the Java pack keeps in the method that writes it), so its
//! handler is the method holding the call that names the topic or the queue: the call itself,
//! or the call of a wrapper that passes the name (`consume`'s default), wherever the receive
//! loop runs.
//!
//! The pack reads an annotation written as an element value (`@QueueBinding(value = @Queue(..),
//! exchange = @Exchange(..), key = ..)`) as `Value::New(name, [Object(elements)])`, its unnamed
//! element as `value`; like every argument, it lives only while the contracts are extracted. Java
//! types an element by the annotation that declares it, so a nested annotation is read by its
//! element's key, never by its own name.

use std::cell::OnceCell;

use super::message::{
    Handler, Message, Side, bind_with, channel_name, consume_with, produce_with, publish_with,
    queue_name,
};
use crate::index::Indexed;
use crate::jvm::names::{known, one, spellings, typed};
use crate::jvm::spring::{self, Configs, Properties};
use crate::lang::{Call, Invocation, Kind, Part, Value};
use crate::resolve::{Code, Expand, Inject, Keywords, method as member};

const KAFKA_LISTENER: &str = "org.springframework.kafka.annotation.KafkaListener";
const RABBIT_LISTENER: &str = "org.springframework.amqp.rabbit.annotation.RabbitListener";
const JMS_LISTENER: &str = "org.springframework.jms.annotation.JmsListener";
const LISTENERS: &[&str] = &[KAFKA_LISTENER, RABBIT_LISTENER, JMS_LISTENER];

const BINDING_BUILDER: &str = "org.springframework.amqp.core.BindingBuilder.bind";
/// The types `BindingBuilder.bind(..).to(..).with(..)` reads a queue and an exchange name off,
/// as `new` writes them (`new Queue("orders")`): `with` follows only a topic or a direct
/// exchange.
const QUEUES: &[&str] = &["Queue"];
const EXCHANGES: &[&str] = &["TopicExchange", "DirectExchange"];
/// The exchange Spring Boot gives the `RabbitTemplate` it configures; empty, the default one.
const TEMPLATE_EXCHANGE: &str = "spring.rabbitmq.template.exchange";
const AMQP_POST_PROCESSOR: &str = "org.springframework.amqp.core.MessagePostProcessor";
const JMS_POST_PROCESSOR: &str = "org.springframework.jms.core.MessagePostProcessor";
const STREAM_OFFSET: &str = "io.lettuce.core.XReadArgs.StreamOffset";

#[derive(Clone, Copy, PartialEq)]
enum Client {
    Kafka,
    Rabbit,
    Jms,
    Sqs,
    Nats,
    Jedis,
    Lettuce,
}

/// Each client by the types the source may write for it.
const CLIENTS: &[(Client, &[&str])] = &[
    (
        Client::Kafka,
        &["org.springframework.kafka.core.KafkaTemplate"],
    ),
    (
        Client::Rabbit,
        &["org.springframework.amqp.rabbit.core.RabbitTemplate"],
    ),
    (Client::Jms, &["org.springframework.jms.core.JmsTemplate"]),
    (
        Client::Sqs,
        &[
            "software.amazon.awssdk.services.sqs.SqsClient",
            "software.amazon.awssdk.services.sqs.SqsAsyncClient",
        ],
    ),
    (
        Client::Nats,
        &["io.nats.client.Connection", "io.nats.client.Dispatcher"],
    ),
    (
        Client::Jedis,
        &[
            "redis.clients.jedis.Jedis",
            "redis.clients.jedis.JedisPooled",
            "redis.clients.jedis.UnifiedJedis",
            "redis.clients.jedis.JedisCluster",
        ],
    ),
    (
        Client::Lettuce,
        &[
            "io.lettuce.core.api.sync.RedisCommands",
            "io.lettuce.core.api.async.RedisAsyncCommands",
            "io.lettuce.core.api.reactive.RedisReactiveCommands",
        ],
    ),
];

/// The methods a form is made of.
const METHODS: &[&str] = &[
    "send",
    "convertAndSend",
    "sendMessage",
    "receiveMessage",
    "publish",
    "subscribe",
    "xadd",
    "xreadGroup",
    "xreadgroup",
    "with",
];

pub(super) fn messages<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Message> {
    if !code.pack().extensions().contains(&"java") {
        return Vec::new();
    }
    // Read the first time a form needs them: a repository without one parses none.
    let configs = OnceCell::new();
    let configs = || configs.get_or_init(|| Configs::read(indexed.configs));
    let inject = |code: &Code<'a>, path: &'a str, annotation: &'a Call| {
        spring::inject(configs(), code, path, annotation)
    };
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        if !path.ends_with(".java") {
            continue;
        }
        for (n, stored) in facts.calls.iter().enumerate() {
            if let Some(target) = &stored.decorates {
                // A method's annotation; one on a class is not read.
                let on_method =
                    (facts.defs.iter()).any(|d| d.kind == Kind::Method && d.name == *target);
                let listener = on_method.then(|| {
                    known(code, path, &stored.scope, &stored.callee, |s| {
                        LISTENERS.contains(&s)
                    })
                });
                if let Some(listener) = listener.flatten() {
                    let config = configs().of(path);
                    out.extend(listen(code, path, n, target, &listener, config));
                }
                continue;
            }
            let method = member(&stored.callee);
            if !METHODS.contains(&method) {
                continue;
            }
            if method == "with" {
                out.extend(binding(code, path, n, &inject));
                continue;
            }
            let Some(client) = client(code, path, stored) else {
                continue;
            };
            let call = || Expand::Call(path, n);
            let chain = || Expand::Chain(path, n);
            let inject = Some(&inject as &Inject<'a, '_>);
            match (client, method) {
                (Client::Kafka, "send")
                | (Client::Nats, "publish")
                | (Client::Jedis | Client::Lettuce, "xadd") => {
                    let broker = broker(client);
                    out.extend(produce_with(code, call(), inject, broker, &first));
                }
                (Client::Nats, "subscribe") => {
                    out.extend(consume_with(code, call(), inject, "nats", &first, None));
                }
                (Client::Jedis, "xreadGroup") | (Client::Lettuce, "xreadgroup") => {
                    out.extend(consume_with(code, chain(), inject, "redis", &streams, None));
                }
                (Client::Sqs, "sendMessage") => {
                    out.extend(produce_with(code, chain(), inject, "sqs", &queue_url));
                }
                (Client::Sqs, "receiveMessage") => {
                    out.extend(consume_with(code, chain(), inject, "sqs", &queue_url, None));
                }
                (Client::Rabbit, "convertAndSend") => {
                    let Some(written) = code.call(path, n) else {
                        continue;
                    };
                    let config = configs().of(path);
                    let read = rabbit(code, path, written, config);
                    out.extend(publish_with(code, call(), inject, &*read));
                }
                (Client::Jms, "send" | "convertAndSend") => {
                    let Some(written) = code.call(path, n) else {
                        continue;
                    };
                    let at = jms(code, path, written, method);
                    let read = move |a: &[Value], _: &Keywords| match at {
                        Some(at) => Some(vec![channel_name(a.get(at)?)?]),
                        None => Some(vec![None]),
                    };
                    out.extend(produce_with(code, call(), inject, "jms", &read));
                }
                _ => {}
            }
        }
    }
    out
}

fn broker(client: Client) -> &'static str {
    match client {
        Client::Kafka => "kafka",
        Client::Rabbit => "amqp",
        Client::Jms => "jms",
        Client::Sqs => "sqs",
        Client::Nats => "nats",
        Client::Jedis | Client::Lettuce => "redis",
    }
}

/// The client the call `call` of `path` is made on, by the type the source writes for its
/// receiver.
fn client<'a>(code: &Code<'a>, path: &'a str, call: &'a Call) -> Option<Client> {
    let (receiver, _) = call.callee.rsplit_once('.')?;
    let typed = typed(code, path, &call.scope, receiver);
    let all = |t: &str| CLIENTS.iter().any(|(_, types)| types.contains(&t));
    let spelled = one(&typed, all)?;
    let found = CLIENTS.iter().find(|(_, types)| types.contains(&spelled));
    found.map(|(client, _)| *client)
}

/// The name the first argument gives (`channel_name`).
fn first(a: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    Some(vec![channel_name(a.first()?)?])
}

/// The queue of an SQS request read whole (`Expand::Chain`): the last segment of the outermost
/// `queueUrl(..)` of the request's builder chain. A request built elsewhere, or by a lambda
/// (`sendMessage(b -> b.queueUrl(..))`), names none.
fn queue_url(a: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    let url = match a {
        [Value::Call(call)] => match call.args.first() {
            Some(Value::Call(request)) => steps(request)
                .find_map(|step| (member(&step.callee) == "queueUrl").then(|| step.args.first())),
            // A wrapper's parameter: read at each caller.
            Some(Value::Ref(_)) => return None,
            _ => None,
        },
        _ => None,
    };
    Some(vec![queue_name(url.flatten())?])
}

/// The calls of a builder chain, outermost first.
fn steps(top: &Invocation) -> impl Iterator<Item = &Invocation> {
    std::iter::successors(Some(top), |step| match step.receiver.as_deref() {
        Some(Value::Call(next)) => Some(next),
        _ => None,
    })
}

/// The streams of a Redis group read, read whole (`Expand::Chain`): the keys of a
/// `Map.of(key, id, ..)`, `Map.entry(key, id)` or `Collections.singletonMap(key, id)` (Jedis)
/// and of each `XReadArgs.StreamOffset.from(key, ..)`, `lastConsumed(key)` or `latest(key)`
/// (Lettuce). One unresolved stream when none is written so, unless an argument is a wrapper's
/// parameter.
fn streams(a: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    let [Value::Call(call)] = a else {
        return Some(vec![None]);
    };
    let mut names = Vec::new();
    for arg in &call.args {
        let Value::Call(made) = arg else {
            continue;
        };
        let of = |class: &str| {
            matches!(made.receiver.as_deref(),
                Some(Value::External(s)) if one(s, |s| s == class).is_some())
        };
        let keys: Vec<&Value> = match member(&made.callee) {
            "of" if of("java.util.Map") => made.args.iter().step_by(2).collect(),
            "entry" if of("java.util.Map") => made.args.first().into_iter().collect(),
            "singletonMap" if of("java.util.Collections") => {
                made.args.first().into_iter().collect()
            }
            "from" | "lastConsumed" | "latest" if of(STREAM_OFFSET) => {
                made.args.first().into_iter().collect()
            }
            _ => continue,
        };
        for key in keys {
            names.push(channel_name(key)?);
        }
    }
    if names.is_empty() {
        if call.args.iter().any(|a| matches!(a, Value::Ref(_))) {
            return None;
        }
        names.push(None);
    }
    Some(names)
}

/// Whether an argument, as written, is a `MessagePostProcessor` of `class`: what picks the
/// overload of `convertAndSend`.
#[derive(PartialEq)]
enum Post {
    Yes,
    No,
    /// A lambda, or any expression the pack cannot read: either overload.
    Maybe,
}

fn post<'a>(code: &Code<'a>, path: &'a str, call: &'a Call, at: usize, class: &str) -> Post {
    let is = |spelled: Vec<String>| match one(&spelled, |s| s == class) {
        Some(_) => Post::Yes,
        None => Post::No,
    };
    match call.args.get(at) {
        Some(Value::Unknown) => Post::Maybe,
        Some(Value::Ref(name)) => is(typed(code, path, &call.scope, name)),
        Some(Value::New(ty, _)) => is(spellings(code, path, &call.scope, ty)),
        _ => Post::No,
    }
}

/// What a `RabbitTemplate.convertAndSend` reads, by its overload: `(exchange, routingKey,
/// message, ..)`, or `(routingKey, message)` and `(routingKey, message, postProcessor, ..)` to
/// the template's exchange (the default one, unless the module's configuration names one).
/// `(message)` and `(message, postProcessor)` use the template's routing key, and an overload
/// the arguments do not tell, an unresolved producer.
#[allow(clippy::type_complexity)]
fn rabbit<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    config: Option<&Properties>,
) -> Box<dyn Fn(&[Value], &Keywords) -> Option<(Option<String>, Option<String>)>> {
    // Whether the exchange is the first argument; `None` for no routing key the call gives.
    let exchanged = match call.args.len() {
        2 => (post(code, path, call, 1, AMQP_POST_PROCESSOR) == Post::No).then_some(false),
        3.. => match post(code, path, call, 2, AMQP_POST_PROCESSOR) {
            Post::No => Some(true),
            Post::Yes => Some(false),
            Post::Maybe => None,
        },
        _ => None,
    };
    match exchanged {
        Some(true) => {
            return Box::new(|a, _| Some((channel_name(a.first()?)?, channel_name(a.get(1)?)?)));
        }
        None => return Box::new(|_, _| Some((None, None))),
        Some(false) => {}
    }
    let exchange = match config.and_then(|c| c.get(TEMPLATE_EXCHANGE)) {
        Some(set) => text(&Value::Str(vec![Part::Text(set.clone())]), config),
        None => Some(String::new()),
    };
    Box::new(move |a, _| Some((exchange.clone(), channel_name(a.first()?)?)))
}

/// The argument of a `JmsTemplate` call that names its destination, by its overload: the first
/// of `send(destination, creator)`, `convertAndSend(destination, message)` and
/// `convertAndSend(destination, message, postProcessor)`. `None` for one that sends to the
/// template's default destination (`send(creator)`, `convertAndSend(message)`,
/// `convertAndSend(message, postProcessor)`) or that the arguments do not tell.
fn jms<'a>(code: &Code<'a>, path: &'a str, call: &'a Call, method: &str) -> Option<usize> {
    let named = match (method, call.args.len()) {
        ("send", 2) | ("convertAndSend", 3) => true,
        ("convertAndSend", 2) => post(code, path, call, 1, JMS_POST_PROCESSOR) == Post::No,
        _ => false,
    };
    named.then_some(0)
}

/// The binding `BindingBuilder.bind(queue).to(exchange).with(key)` at `n` of `path`, read whole
/// (`Expand::Chain`): the queue and the exchange as their `new` writes them (`QUEUES`,
/// `EXCHANGES`), through constants, `final` fields and a wrapper's arguments. A queue or an
/// exchange made any other way (a `@Bean` method's parameter, a builder) leaves it unread.
fn binding<'a>(code: &Code<'a>, path: &'a str, n: usize, inject: &Inject<'a, '_>) -> Vec<Message> {
    // What the kept facts rule out is ruled out before the call's values are read: `Code::call`
    // parses the file again on a refresh.
    let calls = &code.files()[path].calls;
    let on = |call: &Call| call.on.and_then(|at| calls.get(at));
    let Some((to, start)) = on(&calls[n]).and_then(|to| Some((to, on(to)?))) else {
        return Vec::new();
    };
    let started = spellings(code, path, &start.scope, &start.callee);
    if member(&to.callee) != "to"
        || start.on.is_some()
        || one(&started, |s| s == BINDING_BUILDER).is_none()
    {
        return Vec::new();
    }
    let read = |a: &[Value], _: &Keywords| {
        let [Value::Call(with)] = a else {
            return Some((None, None, None));
        };
        let mut steps = steps(with).skip(1);
        let (Some(to), Some(bind)) = (steps.next(), steps.next()) else {
            return Some((None, None, None));
        };
        Some((
            made(bind.args.first(), QUEUES)?,
            made(to.args.first(), EXCHANGES)?,
            channel_name(with.args.first()?)?,
        ))
    };
    bind_with(code, Expand::Chain(path, n), Some(inject), &read)
}

/// The name a `new` of one of `types` (by its last segment) gives as its first argument: `None`
/// while it is a wrapper's parameter.
fn made(value: Option<&Value>, types: &[&str]) -> Option<Option<String>> {
    match value {
        Some(Value::New(ty, args)) if types.contains(&ty.rsplit('.').next().unwrap_or(ty)) => {
            channel_name(args.first()?)
        }
        Some(Value::Ref(_)) => None,
        _ => Some(None),
    }
}

/// The text of a folded annotation value, its placeholders resolved against `config`; `None`
/// when it is no literal (`jvm::spring::resolved`).
fn text(value: &Value, config: Option<&Properties>) -> Option<String> {
    channel_name(&spring::resolved(value, config)).flatten()
}

/// The items of an array element, or the one value written in its place.
fn items(value: &Value) -> Vec<&Value> {
    match value {
        Value::List(items) => items.iter().collect(),
        one => vec![one],
    }
}

/// The element `key` of an annotation written as an element value (see the module).
fn element<'v>(annotation: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::New(_, args) = annotation else {
        return None;
    };
    let [Value::Object(elements)] = args.as_slice() else {
        return None;
    };
    elements.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// The name of a nested `@Queue` or `@Exchange`: its `value` or its `name`; `None` for an
/// anonymous queue, which the broker names.
fn named(annotation: &Value, config: Option<&Properties>) -> Option<String> {
    let given = element(annotation, "value").or(element(annotation, "name"))?;
    text(given, config).filter(|n| !n.is_empty())
}

/// The sides the listener annotation `listener` at `n` of `path` gives the method `target`: a
/// consumer per topic, queue or destination it names, and for `@RabbitListener` a binding per
/// key of each `@QueueBinding`. One unresolved consumer when it names none it can read.
fn listen<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    target: &str,
    listener: &str,
    config: Option<&Properties>,
) -> Vec<Message> {
    // The index keeps no argument: they come from the full facts of the file.
    let Some(call) = code.call(path, n) else {
        return Vec::new();
    };
    let folded = |key: &str| call.keyword(key).map(|v| code.value(path, &call.scope, v));
    let texts = |key: &str| -> Vec<Option<String>> {
        let folded = folded(key);
        let items = folded.iter().flat_map(items);
        items.map(|v| text(v, config)).collect()
    };
    let message = |name: Option<String>, role: Side| Message {
        broker: "",
        name,
        path: path.into(),
        call: n,
        role,
    };
    let handler = || {
        Side::Consumer(Handler::Def {
            path: path.into(),
            name: target.into(),
        })
    };
    let (broker, mut out) = match listener {
        KAFKA_LISTENER => {
            let mut names = texts("topics");
            let partitions = folded("topicPartitions");
            for partition in partitions.iter().flat_map(items) {
                names.push(element(partition, "topic").and_then(|t| text(t, config)));
            }
            (
                "kafka",
                names.into_iter().map(|t| message(t, handler())).collect(),
            )
        }
        RABBIT_LISTENER => {
            let mut out: Vec<Message> = (texts("queues").into_iter())
                .map(|q| message(q, handler()))
                .collect();
            let declared = folded("queuesToDeclare");
            for queue in declared.iter().flat_map(items) {
                out.push(message(named(queue, config), handler()));
            }
            let bindings = folded("bindings");
            for b in bindings.iter().flat_map(items) {
                let queue = element(b, "value").and_then(|q| named(q, config));
                let exchange = element(b, "exchange").and_then(|e| named(e, config));
                out.push(message(queue.clone(), handler()));
                let (Some(queue), Some(exchange)) = (queue, exchange) else {
                    continue;
                };
                // ponytail: a binding without a key, which Spring declares with an empty
                // one, is dropped.
                let keys = element(b, "key").into_iter().flat_map(items);
                for pattern in keys.filter_map(|k| text(k, config)) {
                    let exchange = exchange.clone();
                    out.push(message(
                        Some(queue.clone()),
                        Side::Binding { exchange, pattern },
                    ));
                }
            }
            ("amqp", out)
        }
        _ => {
            let destination = folded("destination");
            let name = destination.and_then(|d| text(&d, config));
            ("jms", vec![message(name, handler())])
        }
    };
    if !out.iter().any(|m| matches!(m.role, Side::Consumer(_))) {
        out.push(message(None, handler()));
    }
    for m in &mut out {
        m.broker = broker;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::java::Java;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    const MODULE: &str = "api/src/main/java/a/C.java";
    const CONFIG: &str = "api/src/main/resources/application.properties";

    /// The messages of the class body `body` in `MODULE` (with the imports `imports`), the
    /// other files of the repository and the module's `application.properties`, as lines
    /// (`message::lines`), handlers without the file.
    fn sides(imports: &str, body: &str, others: &[(&str, &str)], config: &str) -> Vec<String> {
        let src = format!("package a;\n{imports}\nclass C {{\n{body}\n}}\n");
        let mut parsed = vec![(MODULE, Java.parse(MODULE, src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, Java.parse(path, source.as_bytes()).unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        let configs: Vec<(&str, &[u8])> = vec![(CONFIG, config.as_bytes())];
        let indexed = Indexed {
            manifests: &[],
            tsconfigs: &[],
            exported: &[],
            files: &[],
            configs: &configs,
        };
        let lines = crate::channel::message::lines(messages(&code, &indexed));
        let module = format!("{MODULE}::");
        lines.into_iter().map(|l| l.replace(&module, "")).collect()
    }

    fn read(imports: &str, body: &str) -> Vec<String> {
        sides(imports, body, &[], CONFIGURED)
    }

    const CONFIGURED: &str = "app.topic=orders.created\n\
        app.queue=billing\n\
        app.queue-url=https://sqs.us-east-1.amazonaws.com/123/invoices\n";

    const KAFKA: &str = "import org.springframework.kafka.core.KafkaTemplate;\n\
        import org.springframework.kafka.annotation.KafkaListener;\n\
        import org.springframework.kafka.annotation.TopicPartition;\n\
        import org.springframework.beans.factory.annotation.Value;\n";

    #[test]
    fn a_kafka_template_produces_to_the_topic_it_is_given() {
        let body = "static final String TOPIC = \"orders.shipped\";\n\
            private final KafkaTemplate<String, String> kafka;\n\
            @Value(\"${app.topic}\") private String configured;\n\
            void f(String id) {\n\
            kafka.send(\"orders.paid\", id);\n\
            this.kafka.send(TOPIC, id, id);\n\
            kafka.send(configured, id);\n\
            kafka.send(System.getenv(\"TOPIC\"), id);\n\
            kafka.send(id + \".x\", id);\n}";
        assert_eq!(
            read(KAFKA, body),
            [
                "kafka orders.paid producer",
                "kafka orders.shipped producer",
                "kafka orders.created producer",
                "kafka unresolved producer",
                "kafka unresolved producer",
            ]
        );
    }

    #[test]
    fn a_kafka_listener_consumes_its_topics_in_the_annotated_method() {
        let body = "static final String TOPIC = \"orders.shipped\";\n\
            @KafkaListener(topics = \"orders.paid\") void paid(String m) {}\n\
            @KafkaListener(topics = {TOPIC, \"${app.topic}\"}, groupId = \"g\") void two(String m) {}\n\
            @KafkaListener(topicPartitions = @TopicPartition(topic = \"orders.eu\", partitions = \"0\"))\n\
            void partitioned(String m) {}\n\
            @KafkaListener(topics = \"${missing.topic}\") void missing(String m) {}\n\
            @KafkaListener(topicPattern = \"orders.*\") void pattern(String m) {}\n\
            @KafkaListener(topics = \"#{'${app.topic}'.toUpperCase()}\") void spel(String m) {}";
        assert_eq!(
            read(KAFKA, body),
            [
                "kafka orders.paid consumer a.C.paid(String)",
                "kafka orders.shipped consumer a.C.two(String)",
                "kafka orders.created consumer a.C.two(String)",
                "kafka orders.eu consumer a.C.partitioned(String)",
                "kafka unresolved consumer a.C.missing(String)",
                "kafka unresolved consumer a.C.pattern(String)",
                "kafka unresolved consumer a.C.spel(String)",
            ]
        );
    }

    /// A repository's `KafkaListener` or `KafkaTemplate` (in the same package, or through an
    /// on-demand import of a repository package beside Spring's) is no Spring Kafka's; a
    /// listener on a class is not read.
    #[test]
    fn a_kafka_name_that_is_not_springs_makes_no_side() {
        let others = [
            (
                "api/src/main/java/a/KafkaListener.java",
                "package a;\npublic class KafkaListener {}\n",
            ),
            (
                "api/src/main/java/a/KafkaTemplate.java",
                "package a;\npublic class KafkaTemplate { void send(String t, String m) {} }\n",
            ),
        ];
        let body = "KafkaTemplate kafka;\n\
            @KafkaListener(topics = \"orders.paid\") void paid(String m) {}\n\
            void f() { kafka.send(\"orders.paid\", \"m\"); }";
        let spring = "import org.springframework.kafka.annotation.*;\n\
            import org.springframework.kafka.core.*;\n";
        assert!(sides(spring, body, &others, "").is_empty());
        assert_eq!(sides(spring, body, &[], "").len(), 2);
        let wild = "import org.springframework.kafka.annotation.*;\nimport b.*;\n";
        let body = "@KafkaListener(topics = \"orders.paid\") void paid(String m) {}";
        let own = [(
            "api/src/main/java/b/KafkaListener.java",
            "package b;\npublic class KafkaListener {}\n",
        )];
        assert!(sides(wild, body, &own, "").is_empty());
        let spring = "import org.springframework.kafka.annotation.*;\nimport java.util.*;\n";
        assert_eq!(
            sides(spring, body, &[], ""),
            ["kafka orders.paid consumer a.C.paid(String)"]
        );
        let body = "@KafkaListener(topics = \"orders.paid\") class Inner {}";
        assert!(read(KAFKA, body).is_empty());
        // A parameter hides the field of the same name.
        let body = "KafkaTemplate<String, String> kafka;\n\
            void f(Local kafka) { kafka.send(\"orders.paid\", \"m\"); }";
        assert!(read(KAFKA, body).is_empty());
    }

    const RABBIT: &str = "import org.springframework.amqp.rabbit.core.RabbitTemplate;\n\
        import org.springframework.amqp.core.MessagePostProcessor;\n\
        import org.springframework.amqp.rabbit.annotation.RabbitListener;\n\
        import org.springframework.amqp.rabbit.annotation.QueueBinding;\n\
        import org.springframework.amqp.rabbit.annotation.Queue;\n\
        import org.springframework.amqp.rabbit.annotation.Exchange;\n\
        import org.springframework.amqp.core.BindingBuilder;\n\
        import org.springframework.amqp.core.TopicExchange;\n";

    /// `(exchange, key, message)` publishes to the exchange; `(key, message)` and `(key,
    /// message, postProcessor)` to the default exchange, which is the queue the key names; an
    /// overload the arguments do not tell, or the template's own key, is unresolved.
    #[test]
    fn a_rabbit_template_publishes_by_its_overload() {
        let body = "RabbitTemplate rabbit;\n\
            void f(Order o, MessagePostProcessor mpp) {\n\
            rabbit.convertAndSend(\"events\", \"order.created\", o);\n\
            rabbit.convertAndSend(\"billing\", o);\n\
            rabbit.convertAndSend(\"\", \"audit\", o);\n\
            rabbit.convertAndSend(\"retries\", o, mpp);\n\
            rabbit.convertAndSend(\"retries\", o, m -> m);\n\
            rabbit.convertAndSend(o, m -> m);\n\
            rabbit.convertAndSend(o);\n}";
        assert_eq!(
            read(RABBIT, body),
            [
                "amqp order.created publish events",
                "amqp billing producer",
                "amqp audit producer",
                "amqp retries producer",
                "amqp unresolved producer",
                "amqp unresolved producer",
                "amqp unresolved producer",
            ]
        );
    }

    /// The exchange Spring Boot gives its template is where a routing key alone publishes.
    #[test]
    fn a_configured_template_exchange_takes_a_routing_key_alone() {
        let body = "RabbitTemplate rabbit;\n\
            void f(Order o) { rabbit.convertAndSend(\"order.created\", o); }";
        let config = "spring.rabbitmq.template.exchange=events\n";
        assert_eq!(
            sides(RABBIT, body, &[], config),
            ["amqp order.created publish events"]
        );
    }

    #[test]
    fn a_rabbit_listener_consumes_its_queues_and_declares_its_bindings() {
        let body = "@RabbitListener(queues = {\"billing\", \"${app.queue}\"}) void bill(String m) {}\n\
            @RabbitListener(queuesToDeclare = @Queue(\"audit\")) void audit(String m) {}\n\
            @RabbitListener(bindings = @QueueBinding(\n\
              value = @Queue(value = \"orders\", durable = \"true\"),\n\
              exchange = @Exchange(name = \"events\", type = \"topic\"),\n\
              key = {\"order.*\", \"${app.queue}\"}))\n\
            void orders(String m) {}\n\
            @RabbitListener(bindings = @QueueBinding(value = @Queue, exchange = @Exchange(\"events\"), key = \"x\"))\n\
            void anonymous(String m) {}\n\
            @RabbitListener(id = \"none\") void none(String m) {}";
        assert_eq!(
            read(RABBIT, body),
            [
                "amqp billing consumer a.C.bill(String)",
                "amqp billing consumer a.C.bill(String)",
                "amqp audit consumer a.C.audit(String)",
                "amqp orders consumer a.C.orders(String)",
                "amqp orders binding events order.*",
                "amqp orders binding events billing",
                "amqp unresolved consumer a.C.anonymous(String)",
                "amqp unresolved consumer a.C.none(String)",
            ]
        );
    }

    #[test]
    fn a_binding_builder_binds_the_queue_and_exchange_it_is_written_with() {
        let body = "static final String KEY = \"order.#\";\n\
            private final Queue audit = new Queue(\"audit\");\n\
            Binding a() { return BindingBuilder.bind(new Queue(\"orders\")).to(new TopicExchange(\"events\")).with(\"order.*\"); }\n\
            Binding b() { return BindingBuilder.bind(audit).to(new TopicExchange(\"events\")).with(KEY); }\n\
            Binding c(Queue q, TopicExchange e) { return BindingBuilder.bind(q).to(e).with(\"x\"); }\n\
            Binding d() { return other.bind(new Queue(\"q\")).to(new TopicExchange(\"e\")).with(\"x\"); }";
        let imports = RABBIT.replace(
            "import org.springframework.amqp.rabbit.annotation.Queue;\n",
            "import org.springframework.amqp.core.Queue;\nimport org.springframework.amqp.core.Binding;\n",
        );
        assert_eq!(
            read(&imports, body),
            [
                "amqp orders binding events order.*",
                "amqp audit binding events order.#",
            ]
        );
    }

    const JMS: &str = "import org.springframework.jms.core.JmsTemplate;\n\
        import org.springframework.jms.annotation.JmsListener;\n";

    #[test]
    fn a_jms_template_sends_to_its_destination_and_a_listener_consumes_it() {
        let body = "JmsTemplate jms;\n\
            void f(Order o) {\n\
            jms.send(\"invoices\", s -> s.createTextMessage(\"x\"));\n\
            jms.convertAndSend(\"invoices\", o);\n\
            jms.convertAndSend(\"invoices\", o, m -> m);\n\
            jms.convertAndSend(o, m -> m);\n\
            jms.convertAndSend(o);\n}\n\
            @JmsListener(destination = \"${app.queue}\") void on(String m) {}";
        assert_eq!(
            read(JMS, body),
            [
                "jms invoices producer",
                "jms invoices producer",
                "jms invoices producer",
                "jms unresolved producer",
                "jms unresolved producer",
                "jms billing consumer a.C.on(String)",
            ]
        );
    }

    const SQS: &str = "import software.amazon.awssdk.services.sqs.SqsClient;\n\
        import software.amazon.awssdk.services.sqs.model.SendMessageRequest;\n\
        import software.amazon.awssdk.services.sqs.model.ReceiveMessageRequest;\n\
        import org.springframework.beans.factory.annotation.Value;\n";

    /// The queue is the last segment of the request's `queueUrl`; a polling consumer's handler
    /// is the method that names the queue, wherever the receive loop runs.
    #[test]
    fn an_sqs_client_sends_and_receives_on_the_queue_its_url_names() {
        let body = "static final String URL = \"https://sqs.us-east-1.amazonaws.com/123/orders\";\n\
            SqsClient sqs;\n\
            @Value(\"${app.queue-url}\") String configured;\n\
            void send() {\n\
            sqs.sendMessage(SendMessageRequest.builder().queueUrl(URL).messageBody(\"b\").build());\n\
            sqs.sendMessage(SendMessageRequest.builder().queueUrl(configured).build());\n\
            sqs.sendMessage(b -> b.queueUrl(URL));\n}\n\
            Object poll(String url) { return sqs.receiveMessage(ReceiveMessageRequest.builder().queueUrl(url).build()); }\n\
            void run() { while (true) { handle(poll(URL)); } }\n\
            void handle(Object o) {}";
        assert_eq!(
            read(SQS, body),
            [
                "sqs orders producer",
                "sqs invoices producer",
                "sqs unresolved producer",
                "sqs orders consumer a.C.run()",
            ]
        );
    }

    const NATS: &str = "import io.nats.client.Connection;\nimport io.nats.client.Dispatcher;\n";

    #[test]
    fn a_nats_connection_and_dispatcher_publish_and_subscribe() {
        let body = "Connection nc;\n\
            void f(Dispatcher d) {\n\
            nc.publish(\"orders.created\", new byte[0]);\n\
            nc.subscribe(\"orders.*\");\n\
            d.subscribe(\"orders.>\", m -> {});\n}";
        assert_eq!(
            read(NATS, body),
            [
                "nats orders.created producer",
                "nats orders.* consumer a.C.f(Dispatcher)",
                "nats orders.> consumer a.C.f(Dispatcher)",
            ]
        );
    }

    const REDIS: &str = "import redis.clients.jedis.JedisPooled;\n\
        import redis.clients.jedis.StreamEntryID;\n\
        import io.lettuce.core.api.sync.RedisCommands;\n\
        import io.lettuce.core.Consumer;\n\
        import io.lettuce.core.XReadArgs;\n\
        import java.util.Map;\n";

    #[test]
    fn jedis_and_lettuce_add_to_and_read_streams_in_a_group() {
        let body = "JedisPooled jedis;\n\
            RedisCommands<String, String> lettuce;\n\
            void f() {\n\
            jedis.xadd(\"orders\", StreamEntryID.NEW_ENTRY, Map.of(\"k\", \"v\"));\n\
            jedis.xreadGroup(\"g\", \"c\", null, Map.of(\"orders\", StreamEntryID.UNRECEIVED_ENTRY, \"refunds\", StreamEntryID.UNRECEIVED_ENTRY));\n\
            lettuce.xadd(\"payments\", Map.of(\"k\", \"v\"));\n\
            lettuce.xreadgroup(Consumer.from(\"g\", \"c\"), XReadArgs.StreamOffset.lastConsumed(\"payments\"));\n\
            jedis.xreadGroup(\"g\", \"c\", null, streams());\n\
            jedis.publish(\"chat\", \"m\");\n}";
        assert_eq!(
            read(REDIS, body),
            [
                "redis orders producer",
                "redis orders consumer a.C.f()",
                "redis refunds consumer a.C.f()",
                "redis payments producer",
                "redis payments consumer a.C.f()",
                "redis unresolved consumer a.C.f()",
            ]
        );
    }

    /// A name a wrapper takes as its parameter is read at each caller, whose method handles
    /// what a polling client receives.
    #[test]
    fn a_wrapper_parameter_is_read_at_each_caller() {
        let body = "Connection nc;\n\
            void listen(String subject) { nc.subscribe(subject); }\n\
            void start() { listen(\"orders.created\"); }\n\
            void emit(String subject) { nc.publish(subject, new byte[0]); }\n\
            void run() { emit(\"orders.created\"); }";
        assert_eq!(
            read(NATS, body),
            [
                "nats orders.created consumer a.C.start()",
                "nats orders.created producer",
            ]
        );
    }

    /// An annotation written as an element value is its name and its elements, the unnamed one
    /// as `value`.
    #[test]
    fn the_pack_reads_a_nested_annotation_as_its_name_and_elements() {
        let src = "package a;\nclass C {\n\
            @RabbitListener(bindings = @QueueBinding(value = @Queue(\"q\"), key = {\"a\", \"b\"}))\n\
            void f() {}\n}\n";
        let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
        let call = facts.calls.iter().find(|c| c.decorates.is_some()).unwrap();
        let Some(binding @ Value::New(name, _)) = call.keyword("bindings") else {
            panic!("a nested annotation");
        };
        assert_eq!(name, "QueueBinding");
        let queue = element(binding, "value").unwrap();
        assert!(matches!(queue, Value::New(n, _) if n == "Queue"));
        assert_eq!(
            text(element(queue, "value").unwrap(), None).as_deref(),
            Some("q")
        );
        assert_eq!(items(element(binding, "key").unwrap()).len(), 2);
        let marker = "package a;\nclass C {\n@L(q = @Queue) void f() {}\n}\n";
        let facts = Java.parse("a/C.java", marker.as_bytes()).unwrap();
        let call = facts.calls.iter().find(|c| c.decorates.is_some()).unwrap();
        assert!(matches!(call.keyword("q"), Some(Value::New(n, args))
            if n == "Queue" && matches!(args.as_slice(), [Value::Object(e)] if e.is_empty())));
        // Nesting past the pack's value depth ends in an unknown value.
        let deep = format!("{}1{}", "@A(x = ".repeat(200), ")".repeat(200));
        let src = format!("package a;\nclass C {{\n{deep} void f() {{}}\n}}\n");
        let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
        let call = facts.calls.iter().find(|c| c.decorates.is_some()).unwrap();
        let mut at = call.keyword("x").unwrap();
        let mut depth = 0;
        while let Some(inner) = element(at, "x") {
            (at, depth) = (inner, depth + 1);
        }
        assert!(matches!(at, Value::Unknown), "{depth}");
        assert!(depth < 200);
    }
}
