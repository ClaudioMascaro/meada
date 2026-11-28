use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use super::message::{
    Handler, Message, Side, bind_with, channel_name, consume_with, produce_with, publish_with,
    queue_name,
};
use crate::index::Indexed;
use crate::jvm::names::{array_of, jvm_files, kotlin};
use crate::jvm::spring::{self, Configs, Properties};
use crate::lang::{Call, Invocation, Kind, Part, Value};
use crate::names::{annotation, declared, known, one, spellings, typed};
use crate::resolve::{Code, Expand, Inject, Injected, Keywords, Target, method as member};

const KAFKA_LISTENER: &str = "org.springframework.kafka.annotation.KafkaListener";
const RABBIT_LISTENER: &str = "org.springframework.amqp.rabbit.annotation.RabbitListener";
const JMS_LISTENER: &str = "org.springframework.jms.annotation.JmsListener";
const LISTENERS: &[&str] = &[KAFKA_LISTENER, RABBIT_LISTENER, JMS_LISTENER];

const BINDING_BUILDER: &str = "org.springframework.amqp.core.BindingBuilder.bind";
const QUEUES: &[&str] = &["Queue"];
const EXCHANGES: &[&str] = &["TopicExchange", "DirectExchange"];
const TEMPLATE_EXCHANGE: &str = "spring.rabbitmq.template.exchange";
const AMQP_POST_PROCESSOR: &str = "org.springframework.amqp.core.MessagePostProcessor";
const CORRELATION_DATA: &str = "org.springframework.amqp.rabbit.connection.CorrelationData";
const JMS_POST_PROCESSOR: &str = "org.springframework.jms.core.MessagePostProcessor";
const EXCHANGE_TYPES: &str = "org.springframework.amqp.core.ExchangeTypes.";
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
    let configs = OnceCell::new();
    let configs = || configs.get_or_init(|| Configs::read(indexed.configs, indexed.manifests));
    let inject = |code: &Code<'a>, path: &'a str, on: Injected<'a>| {
        spring::inject(configs(), code, path, on.annotated()?)
    };
    let mut out = Vec::new();
    for (path, facts) in jvm_files(code) {
        let methods: OnceCell<HashSet<&str>> = OnceCell::new();
        let starts: OnceCell<HashMap<(usize, usize), &Call>> = OnceCell::new();
        for (n, stored) in facts.calls.iter().enumerate() {
            if let Some(target) = &stored.decorates {
                let listener = annotation(code, path, stored, |s| LISTENERS.contains(&s));
                let methods = || {
                    methods.get_or_init(|| {
                        (facts.defs.iter())
                            .filter(|d| d.kind == Kind::Method)
                            .map(|d| d.name.as_str())
                            .collect()
                    })
                };
                if let Some(listener) = listener
                    && methods().contains(target.as_str())
                {
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
                out.extend(binding(code, path, n, &starts, &inject));
                continue;
            }
            let Some(client) = client(code, path, stored) else {
                continue;
            };
            if client == Client::Sqs && !kotlin(path) {
                continue;
            }
            let call = || Expand::Call(path, n);
            let chain = || Expand::Chain(path, n);
            let inject = Some(&inject as &Inject<'a, '_>);
            let broker = broker(client);
            match (client, method) {
                (Client::Kafka, "send")
                | (Client::Nats, "publish")
                | (Client::Jedis | Client::Lettuce, "xadd") => {
                    out.extend(produce_with(code, call(), inject, broker, &first));
                }
                (Client::Nats, "subscribe") => {
                    out.extend(consume_with(code, call(), inject, broker, &first, None));
                }
                (Client::Jedis, "xreadGroup") | (Client::Lettuce, "xreadgroup") => {
                    let map_of = kotlin(path)
                        && known(code, path, &stored.scope, "mapOf", |s| {
                            s == "kotlin.collections.mapOf"
                        })
                        .is_some();
                    let read = |a: &[Value], _: &Keywords| streams(a, map_of);
                    out.extend(consume_with(code, chain(), inject, broker, &read, None));
                }
                (Client::Sqs, "sendMessage" | "receiveMessage") => {
                    use crate::contract::sdk::java::{rebuilt, root};
                    let changed = code.call(path, n).is_some_and(|c| {
                        (c.args.first().and_then(root)).is_some_and(|b| {
                            rebuilt(code, path, &c.scope, b, &c.range, &|m| m == "queueUrl")
                        })
                    });
                    let read = |a: &[Value], k: &Keywords| match changed {
                        true => Some(vec![None]),
                        false => queue_url(a, k),
                    };
                    out.extend(match method {
                        "sendMessage" => produce_with(code, chain(), inject, broker, &read),
                        _ => consume_with(code, chain(), inject, broker, &read, None),
                    });
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
                    out.extend(produce_with(code, call(), inject, broker, &read));
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

fn client<'a>(code: &Code<'a>, path: &'a str, call: &'a Call) -> Option<Client> {
    let (receiver, _) = call.callee.rsplit_once('.')?;
    let typed = typed(code, path, &call.scope, receiver);
    let all = |t: &str| CLIENTS.iter().any(|(_, types)| types.contains(&t));
    let spelled = one(&typed, all)?;
    let found = CLIENTS.iter().find(|(_, types)| types.contains(&spelled));
    found.map(|(client, _)| *client)
}

fn first(a: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    Some(vec![channel_name(a.first()?)?])
}

fn queue_url(a: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    let url = match a {
        [Value::Call(call)] => match call.args.first() {
            Some(Value::Call(request)) => steps(request)
                .find_map(|step| (member(&step.callee) == "queueUrl").then(|| step.args.first())),
            Some(Value::Ref(_)) => return None,
            _ => None,
        },
        _ => None,
    };
    Some(vec![queue_name(url.flatten())?])
}

fn steps(top: &Invocation) -> impl Iterator<Item = &Invocation> {
    std::iter::successors(Some(top), |step| match step.receiver.as_deref() {
        Some(Value::Call(next)) => Some(next),
        _ => None,
    })
}

fn streams(a: &[Value], map_of: bool) -> Option<Vec<Option<String>>> {
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
        if map_of && made.receiver.is_none() && made.callee == "mapOf" {
            for entry in &made.args {
                names.push(channel_name(entry)?);
            }
            continue;
        }
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

fn is_class(code: &Code, target: Target) -> bool {
    (code.files()[target.path].defs.iter()).any(|d| d.name == target.name && d.kind == Kind::Class)
}

fn of<'a>(code: &Code<'a>, path: &'a str, call: &'a Call, at: usize, class: &str) -> Option<bool> {
    let spelled = match call.args.get(at)? {
        Value::Call(made) if kotlin(path) && made.receiver.is_none() => {
            let ty = made.callee.rsplit('.').next().unwrap_or(&made.callee);
            if !ty.starts_with(char::is_uppercase) {
                return None;
            }
            match code.definition(path, &call.scope, &made.callee) {
                Some(t) if is_class(code, t) => return Some(false),
                Some(_) => return None,
                None => match spellings(code, path, &call.scope, &made.callee) {
                    spelled if spelled.is_empty() => return None,
                    spelled => spelled,
                },
            }
        }
        Value::Unknown | Value::Call(_) | Value::Function(_) => return None,
        Value::Ref(name) => declared(code, path, &call.scope, name)?,
        Value::New(ty, _) => spellings(code, path, &call.scope, ty),
        _ => return Some(false),
    };
    Some(one(&spelled, |s| s == class).is_some())
}

#[expect(clippy::type_complexity)]
fn rabbit<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    config: Option<&Properties>,
) -> Box<dyn Fn(&[Value], &Keywords) -> Option<(Option<String>, Option<String>)>> {
    let post = |at| of(code, path, call, at, AMQP_POST_PROCESSOR);
    let exchanged = match call.args.len() {
        2 => (post(1) == Some(false)).then_some(false),
        3.. => match (post(2), of(code, path, call, 2, CORRELATION_DATA)) {
            (Some(true), _) => Some(false),
            (_, Some(true)) => (post(1) == Some(false)).then_some(false),
            (Some(false), _) => Some(true),
            (None, _) => None,
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

fn jms<'a>(code: &Code<'a>, path: &'a str, call: &'a Call, method: &str) -> Option<usize> {
    let named = match (method, call.args.len()) {
        ("send", 2) | ("convertAndSend", 3) => true,
        ("convertAndSend", 2) => of(code, path, call, 1, JMS_POST_PROCESSOR) == Some(false),
        _ => false,
    };
    named.then_some(0)
}

fn binding<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    starts: &OnceCell<HashMap<(usize, usize), &'a Call>>,
    inject: &Inject<'a, '_>,
) -> Vec<Message> {
    let calls = &code.files()[path].calls;
    let on = |call: &Call| call.on.and_then(|at| calls.get(at));
    let kotlin = kotlin(path);
    if kotlin {
        if calls[n].callee != "(call_expression).with" {
            return Vec::new();
        }
    } else {
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
    }
    let read = |a: &[Value], _: &Keywords| {
        let [Value::Call(with)] = a else {
            return Some((None, None, None));
        };
        let mut steps = steps(with).skip(1);
        let (Some(to), Some(bind)) = (steps.next(), steps.next()) else {
            return Some((None, None, None));
        };
        if kotlin && !binds(code, path, starts, to, bind) {
            return Some((None, None, None));
        }
        let found = (
            made(bind.args.first(), QUEUES, kotlin)?,
            made(to.args.first(), EXCHANGES, kotlin)?,
            channel_name(with.args.first()?)?,
        );
        let direct = matches!(constructed(to.args.first(), kotlin),
            Some((ty, _)) if ty.rsplit('.').next() == Some("DirectExchange"));
        match found.2.as_deref() {
            Some(key) if direct && wildcard(key) => Some((None, None, None)),
            _ => Some(found),
        }
    };
    bind_with(code, Expand::Chain(path, n), Some(inject), &read)
}

fn binds<'a>(
    code: &Code<'a>,
    path: &'a str,
    starts: &OnceCell<HashMap<(usize, usize), &'a Call>>,
    to: &Invocation,
    bind: &Invocation,
) -> bool {
    let calls = &code.files()[path].calls;
    let starts = starts.get_or_init(|| {
        (calls.iter())
            .map(|c| ((c.range.start, c.range.end), c))
            .collect()
    });
    member(&to.callee) == "to"
        && !matches!(bind.receiver.as_deref(), Some(Value::Call(_)))
        && (starts.get(&(bind.range.start, bind.range.end))).is_some_and(|c| {
            let spelled = spellings(code, path, &c.scope, &c.callee);
            one(&spelled, |s| s == BINDING_BUILDER).is_some()
        })
}

fn wildcard(key: &str) -> bool {
    key.split('.').any(|w| w == "*" || w == "#")
}

fn constructed(value: Option<&Value>, kotlin: bool) -> Option<(&str, &[Value])> {
    match value {
        Some(Value::New(ty, args)) => Some((ty, args)),
        Some(Value::Call(c)) if kotlin && c.receiver.is_none() => Some((&c.callee, &c.args)),
        _ => None,
    }
}

fn made(value: Option<&Value>, types: &[&str], kotlin: bool) -> Option<Option<String>> {
    match constructed(value, kotlin) {
        Some((ty, args)) if types.contains(&ty.rsplit('.').next().unwrap_or(ty)) => {
            channel_name(args.first()?)
        }
        _ if matches!(value, Some(Value::Ref(_))) => None,
        _ => Some(None),
    }
}

fn text(value: &Value, config: Option<&Properties>) -> Option<String> {
    channel_name(&spring::resolved(value, config)).flatten()
}

fn items(value: &Value) -> Vec<&Value> {
    match value {
        Value::List(items) => items.iter().collect(),
        one => vec![one],
    }
}

fn array<'a>(code: &Code<'a>, path: &'a str, call: &'a Call, key: &str) -> Vec<&'a Value> {
    match call.keyword(key) {
        Some(value) => match array_of(code, path, &call.scope, value) {
            Some(args) => args.iter().collect(),
            None => items(value),
        },
        None => Vec::new(),
    }
}

fn element<'v>(annotation: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::New(_, args) = annotation else {
        return None;
    };
    let [Value::Object(elements)] = args.as_slice() else {
        return None;
    };
    elements.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn named(annotation: &Value, config: Option<&Properties>) -> Option<String> {
    let given = element(annotation, "value").or(element(annotation, "name"))?;
    text(given, config).filter(|n| !n.is_empty())
}

fn exchange_type<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    exchange: &'a Value,
    config: Option<&Properties>,
) -> Option<String> {
    let Some(written) = element(exchange, "type") else {
        return Some("direct".into());
    };
    if let Value::Ref(name) = written
        && let Some(spelled) = known(code, path, &call.scope, name, |s| {
            s.starts_with(EXCHANGE_TYPES)
        })
    {
        return Some(spelled[EXCHANGE_TYPES.len()..].to_lowercase());
    }
    text(&code.value(path, &call.scope, written), config)
}

fn listen<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    target: &str,
    listener: &str,
    config: Option<&Properties>,
) -> Vec<Message> {
    let Some(call) = code.call(path, n) else {
        return Vec::new();
    };
    let folded = |key: &str| call.keyword(key).map(|v| code.value(path, &call.scope, v));
    let all = |key: &str| -> Vec<Value> {
        let fold = |v: &'a Value| code.value(path, &call.scope, v);
        let folded: Vec<Value> = array(code, path, call, key).into_iter().map(fold).collect();
        (folded.iter().flat_map(items).cloned()).collect()
    };
    let texts =
        |key: &str| -> Vec<Option<String>> { all(key).iter().map(|v| text(v, config)).collect() };
    let message = |name: Option<String>, role: Side| Message {
        environment: None,
        broker: "",
        name,
        path: path.into(),
        call: n,
        role,
        client: None,
        cap: None,
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
            for partition in &all("topicPartitions") {
                names.push(element(partition, "topic").and_then(|t| text(t, config)));
            }
            (
                broker(Client::Kafka),
                names.into_iter().map(|t| message(t, handler())).collect(),
            )
        }
        RABBIT_LISTENER => {
            let mut out: Vec<Message> = (texts("queues").into_iter())
                .map(|q| message(q, handler()))
                .collect();
            for queue in &all("queuesToDeclare") {
                out.push(message(named(queue, config), handler()));
            }
            for written in array(code, path, call, "bindings") {
                let b = &code.value(path, &call.scope, written);
                let queue = element(b, "value").and_then(|q| named(q, config));
                let exchange = element(b, "exchange").and_then(|e| named(e, config));
                out.push(message(queue.clone(), handler()));
                let (Some(queue), Some(exchange)) = (queue, exchange) else {
                    continue;
                };
                let kind = element(written, "exchange")
                    .and_then(|e| exchange_type(code, path, call, e, config));
                let routes = |key: &String| match kind.as_deref() {
                    None | Some("topic") => true,
                    Some("direct" | "fanout") => !wildcard(key),
                    Some(_) => false,
                };
                // ponytail: a binding without a key, which Spring declares with an empty
                // one, is dropped.
                let keys = element(b, "key").into_iter().flat_map(items);
                let keys = keys.filter_map(|k| text(k, config)).filter(routes);
                for pattern in keys {
                    let exchange = exchange.clone();
                    out.push(message(
                        Some(queue.clone()),
                        Side::Binding { exchange, pattern },
                    ));
                }
            }
            (broker(Client::Rabbit), out)
        }
        _ => {
            let destination = folded("destination");
            let name = destination.and_then(|d| text(&d, config));
            (broker(Client::Jms), vec![message(name, handler())])
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
    const KOTLIN: &str = "api/src/main/kotlin/a/C.kt";
    const CONFIG: &str = "api/src/main/resources/application.properties";

    fn sides(imports: &str, body: &str, others: &[(&str, &str)], config: &str) -> Vec<String> {
        let src = format!("package a;\n{imports}\nclass C {{\n{body}\n}}\n");
        let mut parsed = vec![(MODULE, Java.parse(MODULE, src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, Java.parse(path, source.as_bytes()).unwrap()));
        }
        lines(&parsed, MODULE, config)
    }

    fn kt(imports: &str, body: &str, others: &[(&str, &str)], config: &str) -> Vec<String> {
        use crate::lang::kotlin::Kotlin;
        let src = format!("package a\n{imports}\n{body}\n");
        let mut parsed = vec![(KOTLIN, Kotlin.parse(KOTLIN, src.as_bytes()).unwrap())];
        for (path, source) in others {
            let facts = match path.ends_with(".kt") {
                true => Kotlin.parse(path, source.as_bytes()),
                false => Java.parse(path, source.as_bytes()),
            };
            parsed.push((path, facts.unwrap()));
        }
        lines(&parsed, KOTLIN, config)
    }

    fn lines(parsed: &[(&str, crate::lang::Facts)], module: &str, config: &str) -> Vec<String> {
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
            symlinks: &[],
            channel_clients: &[],
        };
        let mut messages = messages(&code, &indexed);
        messages.extend(super::super::aws::test_messages(&code, Some(&indexed)));
        let lines = crate::channel::message::lines(messages);
        let module = format!("{module}::");
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

    #[test]
    fn a_listener_in_a_class_with_an_outside_base_has_a_handler() {
        let imports = format!("{KAFKA}import com.corp.shared.Base;\n");
        let body = "static class Based extends Base { @KafkaListener(topics = \"t\") void on(String m) {} }\n\
            static class Own { @KafkaListener(topics = \"t\") void on(String m) {} }";
        assert_eq!(
            read(&imports, body),
            [
                "kafka t consumer a.C.Based.on(String)",
                "kafka t consumer a.C.Own.on(String)"
            ]
        );
    }

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
        let annotation = [(
            "api/src/main/java/a/KafkaListener.java",
            "package a;\npublic @interface KafkaListener { String[] topics(); }\n",
        )];
        let listener = "@KafkaListener(topics = \"orders.paid\") void paid(String m) {}";
        assert!(sides(spring, listener, &annotation, "").is_empty());
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

    #[test]
    fn a_correlation_data_takes_the_routing_key_form() {
        let imports =
            format!("{RABBIT}import org.springframework.amqp.rabbit.connection.CorrelationData;\n");
        let body = "RabbitTemplate rabbit;\n\
            void f(Order o, MessagePostProcessor mpp, CorrelationData cd) {\n\
            rabbit.convertAndSend(\"orders\", o, new CorrelationData(\"1\"));\n\
            rabbit.convertAndSend(\"orders\", o, cd);\n\
            rabbit.convertAndSend(o, mpp, cd);\n\
            rabbit.convertAndSend(\"events\", \"order.paid\", o, cd);\n\
            rabbit.convertAndSend(\"retries\", o, mpp, cd);\n\
            rabbit.convertAndSend(\"events\", \"order.paid\", o, mpp, cd);\n}";
        assert_eq!(
            read(&imports, body),
            [
                "amqp orders producer",
                "amqp orders producer",
                "amqp unresolved producer",
                "amqp order.paid publish events",
                "amqp retries producer",
                "amqp order.paid publish events",
            ]
        );
    }

    #[test]
    fn an_argument_of_unknown_type_does_not_tell_the_overload() {
        let body = "static final String PAYLOAD = \"p\";\n\
            RabbitTemplate rabbit;\n\
            void f(Order o) {\n\
            rabbit.convertAndSend(PAYLOAD, postProcessor());\n\
            rabbit.convertAndSend(PAYLOAD, tracing);\n\
            rabbit.convertAndSend(\"orders\", o);\n}";
        assert_eq!(
            read(RABBIT, body),
            [
                "amqp unresolved producer",
                "amqp unresolved producer",
                "amqp orders producer",
            ]
        );
    }

    #[test]
    fn a_message_of_a_repository_type_tells_the_overload() {
        let body = "class JOrder {}\n\
            RabbitTemplate rabbit;\n\
            void f(JOrder o) {\n\
            rabbit.convertAndSend(\"j.repo\", o);\n\
            rabbit.convertAndSend(\"jx.repo\", \"jkey.repo\", o);\n\
}";
        assert_eq!(
            read(RABBIT, body),
            ["amqp j.repo producer", "amqp jkey.repo publish jx.repo",]
        );
    }

    #[test]
    fn a_wildcard_key_binds_only_on_a_topic_exchange() {
        let imports = format!(
            "{RABBIT}import org.springframework.amqp.core.ExchangeTypes;\n\
             import org.springframework.amqp.core.DirectExchange;\n"
        );
        let body = "@RabbitListener(bindings = @QueueBinding(value = @Queue(\"a\"), exchange = @Exchange(\"ex\"), key = {\"x.*\", \"x.y\"}))\n\
            void a(String m) {}\n\
            @RabbitListener(bindings = @QueueBinding(value = @Queue(\"b\"), exchange = @Exchange(value = \"ex\", type = ExchangeTypes.TOPIC), key = \"x.*\"))\n\
            void b(String m) {}\n\
            @RabbitListener(bindings = @QueueBinding(value = @Queue(\"c\"), exchange = @Exchange(value = \"ex\", type = \"headers\"), key = \"x.y\"))\n\
            void c(String m) {}\n\
            @RabbitListener(bindings = @QueueBinding(value = @Queue(\"d\"), exchange = @Exchange(value = \"ex\", type = TYPE), key = \"x.*\"))\n\
            void d(String m) {}\n\
            Object e() { return BindingBuilder.bind(new org.springframework.amqp.core.Queue(\"e\")).to(new DirectExchange(\"ex\")).with(\"x.#\"); }\n\
            Object f() { return BindingBuilder.bind(new org.springframework.amqp.core.Queue(\"f\")).to(new DirectExchange(\"ex\")).with(\"x.y\"); }";
        assert_eq!(
            read(&imports, body),
            [
                "amqp a consumer a.C.a(String)",
                "amqp a binding ex x.y",
                "amqp b consumer a.C.b(String)",
                "amqp b binding ex x.*",
                "amqp c consumer a.C.c(String)",
                "amqp d consumer a.C.d(String)",
                "amqp d binding ex x.*",
                "amqp f binding ex x.y",
            ]
        );
    }

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
        import org.springframework.jms.core.MessagePostProcessor;\n\
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
            @JmsListener(destination = \"${app.queue}\") void on(String m) {}\n\
            void g(Order o, MessagePostProcessor mpp) { jms.convertAndSend(o, mpp); }";
        assert_eq!(
            read(JMS, body),
            [
                "jms invoices producer",
                "jms invoices producer",
                "jms invoices producer",
                "jms unresolved producer",
                "jms unresolved producer",
                "jms billing consumer a.C.on(String)",
                "jms unresolved producer",
            ]
        );
    }

    const SQS: &str = "import software.amazon.awssdk.services.sqs.SqsClient;\n\
        import software.amazon.awssdk.services.sqs.model.SendMessageRequest;\n\
        import software.amazon.awssdk.services.sqs.model.ReceiveMessageRequest;\n\
        import org.springframework.beans.factory.annotation.Value;\n";

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
        let body = "SqsClient sqs;\n\
            void send() {\n\
            var b = SendMessageRequest.builder().queueUrl(\"https://sqs/1/old\");\n\
            b.queueUrl(\"https://sqs/1/new\");\n\
            sqs.sendMessage(b.messageBody(\"m\").build());\n\
            var k = SendMessageRequest.builder().queueUrl(\"https://sqs/1/kept\");\n\
            sqs.sendMessage(k.build());\n}";
        assert_eq!(
            read(SQS, body),
            ["sqs unresolved producer", "sqs kept producer"]
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

    #[test]
    fn redis_streams_are_read_from_each_written_form() {
        let imports = format!("{REDIS}import java.util.Collections;\n");
        let body = "JedisPooled jedis;\n\
            RedisCommands<String, String> lettuce;\n\
            void f() {\n\
            jedis.xreadGroup(\"g\", \"c\", null, Map.entry(\"a\", StreamEntryID.UNRECEIVED_ENTRY));\n\
            jedis.xreadGroup(\"g\", \"c\", null, Collections.singletonMap(\"b\", StreamEntryID.UNRECEIVED_ENTRY));\n\
            lettuce.xreadgroup(Consumer.from(\"g\", \"c\"), XReadArgs.StreamOffset.from(\"c\", \"0\"), XReadArgs.StreamOffset.latest(\"d\"));\n}\n\
            void poll(String stream) { lettuce.xreadgroup(Consumer.from(\"g\", \"c\"), XReadArgs.StreamOffset.lastConsumed(stream)); }\n\
            void run() { poll(\"e\"); }\n\
            void each(Map<String, StreamEntryID> streams) { jedis.xreadGroup(\"g\", \"c\", null, streams); }\n\
            void start() { each(Map.of(\"h\", StreamEntryID.UNRECEIVED_ENTRY)); }";
        assert_eq!(
            read(&imports, body),
            [
                "redis a consumer a.C.f()",
                "redis b consumer a.C.f()",
                "redis c consumer a.C.f()",
                "redis d consumer a.C.f()",
                "redis e consumer a.C.run()",
                "redis h consumer a.C.start()",
            ]
        );
    }

    #[test]
    fn an_sqs_request_a_wrapper_takes_is_read_at_each_caller() {
        let body = "SqsClient sqs;\n\
            void send(SendMessageRequest request) { sqs.sendMessage(request); }\n\
            void ship() { send(SendMessageRequest.builder().queueUrl(\"https://sqs/1/shipments\").build()); }";
        assert_eq!(read(SQS, body), ["sqs shipments producer"]);
    }

    #[test]
    fn a_repository_type_named_like_the_sqs_client_hides_it() {
        let body = "static class SqsClient { void sendMessage(Object request) {} }\n\
            SqsClient sqs;\n\
            void send() { sqs.sendMessage(SendMessageRequest.builder().queueUrl(\"https://sqs/1/hidden\").build()); }";
        assert!(read(SQS, body).is_empty());
    }

    #[test]
    fn a_declared_sqs_client_is_read_through_an_alias() {
        let body = "void send(SqsClient conduit, foreign.Factory opaque) {\n\
            var alias = conduit;\n\
            alias.sendMessage(SendMessageRequest.builder().queueUrl(\"https://sqs/1/aliased\").build());\n\
            SqsClient made = opaque.makeClient();\n\
            made.sendMessage(SendMessageRequest.builder().queueUrl(\"https://sqs/1/typed\").build());\n\
            }";
        assert_eq!(
            read(SQS, body),
            ["sqs aliased producer", "sqs typed producer"]
        );
    }

    #[test]
    fn a_wrapper_reached_by_a_name_match_feeds_no_channel() {
        let body = "Connection nc;\n\
            void emit(String subject) { nc.publish(subject, new byte[0]); }\n\
            void run(Object other) { ((C) other).emit(\"orders.created\"); }";
        assert_eq!(read(NATS, body), ["nats unresolved producer"]);
    }

    const KT_KAFKA: &str = "import org.springframework.kafka.core.KafkaTemplate\n\
        import org.springframework.kafka.annotation.KafkaListener\n\
        import org.springframework.kafka.annotation.TopicPartition\n\
        import org.springframework.beans.factory.annotation.Value\n";

    #[test]
    fn a_kotlin_kafka_template_and_listener_read_like_the_java_ones() {
        let body = "const val TOPIC = \"orders.shipped\"\n\
            class C(private val kafka: KafkaTemplate<String, String>,\n\
            @Value(\"\\${app.topic}\") private val configured: String) {\n\
            companion object { const val INNER = \"orders.inner\" }\n\
            fun f(id: String, env: String) {\n\
            kafka.send(\"orders.paid\", id)\n\
            this.kafka.send(TOPIC, id, id)\n\
            kafka.send(INNER, id)\n\
            kafka.send(configured, id)\n\
            kafka.send(System.getenv(\"TOPIC\"), id)\n\
            kafka.send(\"orders-$env\", id)\n\
            kafka.send(id + \".x\", id)\n}\n\
            @KafkaListener(topics = [\"orders.paid\"]) fun paid(m: String) {}\n\
            @KafkaListener(topics = [TOPIC, \"\\${app.topic}\"], groupId = \"g\") fun two(m: String) {}\n\
            @KafkaListener(topics = arrayOf(\"orders.a\", INNER)) fun array(m: String) {}\n\
            @KafkaListener(\"orders.b\") fun positional(m: String) {}\n\
            @KafkaListener(topicPartitions = [TopicPartition(topic = \"orders.eu\", partitions = [\"0\"])])\n\
            fun partitioned(m: String) {}\n\
            @KafkaListener(topics = [\"\\${missing.topic}\"]) fun missing(m: String) {}\n\
            @KafkaListener(topicPattern = \"orders.*\") fun pattern(m: String) {}\n\
            @KafkaListener(topics = [\"orders-$TOPIC\"]) fun templated(m: String) {}\n}";
        assert_eq!(
            kt(KT_KAFKA, body, &[], CONFIGURED),
            [
                "kafka orders.paid producer",
                "kafka orders.shipped producer",
                "kafka orders.inner producer",
                "kafka orders.created producer",
                "kafka unresolved producer",
                "kafka unresolved producer",
                "kafka unresolved producer",
                "kafka orders.paid consumer a.C.paid(String)",
                "kafka orders.shipped consumer a.C.two(String)",
                "kafka orders.created consumer a.C.two(String)",
                "kafka orders.a consumer a.C.array(String)",
                "kafka orders.inner consumer a.C.array(String)",
                "kafka unresolved consumer a.C.positional(String)",
                "kafka orders.eu consumer a.C.partitioned(String)",
                "kafka unresolved consumer a.C.missing(String)",
                "kafka unresolved consumer a.C.pattern(String)",
                "kafka orders-orders.shipped consumer a.C.templated(String)",
            ]
        );
    }

    #[test]
    fn a_kotlin_name_that_is_not_springs_makes_no_side() {
        let own = [
            (
                "api/src/main/kotlin/a/KafkaListener.kt",
                "package a\nannotation class KafkaListener(val topics: Array<String>)\n",
            ),
            (
                "api/src/main/kotlin/a/KafkaTemplate.kt",
                "package a\nclass KafkaTemplate { fun send(t: String, m: String) {} }\n",
            ),
        ];
        let body = "class C(private val kafka: KafkaTemplate) {\n\
            @KafkaListener(topics = [\"orders.paid\"]) fun paid(m: String) {}\n\
            fun f() { kafka.send(\"orders.paid\", \"m\") }\n}";
        let spring = "import org.springframework.kafka.annotation.*\n\
            import org.springframework.kafka.core.*\n";
        assert!(kt(spring, body, &own, "").is_empty());
        assert_eq!(kt(spring, body, &[], "").len(), 2);
        let body = "class C(private val kafka: Client, private val other: Other) {\n\
            fun f() { kafka.send(\"orders.paid\", \"m\"); other.publish(\"orders.paid\", \"m\") }\n}";
        let imports = "import com.corp.shared.Client\nimport com.corp.shared.Other\n";
        assert!(kt(imports, body, &[], "").is_empty());
        let body = "class C(private val kafka: KafkaTemplate<String, String>) {\n\
            fun f(kafka: Local) { kafka.send(\"orders.paid\", \"m\") }\n\
            fun g() { val kafka = Local(); kafka.send(\"orders.paid\", \"m\") }\n\
            fun h() { kafka.send(\"orders.shipped\", \"m\") }\n}";
        assert_eq!(
            kt(KT_KAFKA, body, &[], ""),
            ["kafka orders.shipped producer"]
        );
        let imports = format!("{KT_KAFKA}import com.corp.shared.Base\n");
        let body = "class Based : Base() { @KafkaListener(topics = [\"t\"]) fun on(m: String) {} }\n\
            class Own { @KafkaListener(topics = [\"t\"]) fun on(m: String) {} }";
        assert_eq!(
            kt(&imports, body, &[], ""),
            [
                "kafka t consumer a.Based.on(String)",
                "kafka t consumer a.Own.on(String)"
            ]
        );
        let own = [(
            "api/src/main/kotlin/a/Own.kt",
            "package a\nfun arrayOf(vararg s: String): Array<String> = emptyArray()\n",
        )];
        let body =
            "class C { @KafkaListener(topics = arrayOf(\"orders.a\")) fun on(m: String) {} }";
        assert_eq!(
            kt(KT_KAFKA, body, &own, ""),
            ["kafka unresolved consumer a.C.on(String)"]
        );
    }

    const KT_RABBIT: &str = "import org.springframework.amqp.rabbit.core.RabbitTemplate\n\
        import org.springframework.amqp.rabbit.annotation.RabbitListener\n\
        import org.springframework.amqp.rabbit.annotation.QueueBinding\n\
        import org.springframework.amqp.rabbit.annotation.Exchange\n\
        import org.springframework.amqp.core.BindingBuilder\n\
        import org.springframework.amqp.core.Binding\n\
        import org.springframework.amqp.core.ExchangeTypes\n\
        import org.springframework.amqp.core.Queue\n\
        import org.springframework.amqp.core.DirectExchange\n\
        import org.springframework.amqp.core.TopicExchange\n";

    #[test]
    fn a_kotlin_rabbit_template_listener_and_binding_read_like_the_java_ones() {
        let body = "const val KEY = \"order.#\"\n\
            class C(private val rabbit: RabbitTemplate, private val audit: Queue) {\n\
            fun f(o: Order) {\n\
            rabbit.convertAndSend(\"events\", \"order.created\", o)\n\
            rabbit.convertAndSend(\"billing\", o)\n\
            rabbit.convertAndSend(\"\", \"audit\", o)\n\
            rabbit.convertAndSend(o)\n\
            rabbit.convertAndSend(\"retries\", o) { m -> m }\n}\n\
            fun a() = BindingBuilder.bind(Queue(\"orders\")).to(TopicExchange(\"events\")).with(\"order.*\")\n\
            fun b(): Binding = BindingBuilder.bind(Queue(\"audit\")).to(TopicExchange(\"events\")).with(KEY)\n\
            fun c() = BindingBuilder.bind(Queue(\"direct\")).to(DirectExchange(\"events\")).with(\"x.y\")\n\
            fun d() = BindingBuilder.bind(Queue(\"wild\")).to(DirectExchange(\"events\")).with(\"x.*\")\n\
            fun e(q: Queue, x: TopicExchange) = BindingBuilder.bind(q).to(x).with(\"x\")\n\
            fun g() = other.bind(Queue(\"q\")).to(TopicExchange(\"e\")).with(\"x\")\n\
            @RabbitListener(queues = [\"refunds\", \"audit\"]) fun refund(m: String) {}\n\
            @RabbitListener(queues = arrayOf(\"arr\")) fun array(m: String) {}\n\
            @RabbitListener(bindings = [QueueBinding(\n\
            value = org.springframework.amqp.rabbit.annotation.Queue(\"invoices\"),\n\
            exchange = Exchange(value = \"billing\", type = ExchangeTypes.TOPIC),\n\
            key = [\"invoice.*\", \"invoice.#\"])])\n\
            fun invoice(m: String) {}\n\
            @RabbitListener(bindings = arrayOf(QueueBinding(\n\
            value = org.springframework.amqp.rabbit.annotation.Queue(\"pays\"),\n\
            exchange = Exchange(\"direct\"), key = arrayOf(\"pay.ok\", \"pay.*\"))))\n\
            fun pay(m: String) {}\n}";
        assert_eq!(
            kt(KT_RABBIT, body, &[], ""),
            [
                "amqp order.created publish events",
                "amqp billing producer",
                "amqp audit producer",
                "amqp unresolved producer",
                "amqp unresolved producer",
                "amqp orders binding events order.*",
                "amqp audit binding events order.#",
                "amqp direct binding events x.y",
                "amqp refunds consumer a.C.refund(String)",
                "amqp audit consumer a.C.refund(String)",
                "amqp arr consumer a.C.array(String)",
                "amqp invoices consumer a.C.invoice(String)",
                "amqp invoices binding billing invoice.*",
                "amqp invoices binding billing invoice.#",
                "amqp pays consumer a.C.pay(String)",
                "amqp pays binding direct pay.ok",
            ]
        );
    }

    #[test]
    fn a_kotlin_jms_template_sends_to_its_destination_and_a_listener_consumes_it() {
        let imports = "import org.springframework.jms.core.JmsTemplate\n\
            import org.springframework.jms.core.MessagePostProcessor\n\
            import org.springframework.jms.annotation.JmsListener\n";
        let body = "class C(private val jms: JmsTemplate) {\n\
            fun f(o: Order, mpp: MessagePostProcessor) {\n\
            jms.send(\"invoices\") { s -> s.createTextMessage(\"x\") }\n\
            jms.convertAndSend(\"invoices\", o)\n\
            jms.convertAndSend(\"invoices\", o) { m -> m }\n\
            jms.convertAndSend(\"hello\") { m -> m }\n\
            jms.convertAndSend(o, mpp)\n\
            jms.convertAndSend(o)\n}\n\
            @JmsListener(destination = \"\\${app.queue}\") fun on(m: String) {}\n}";
        assert_eq!(
            kt(imports, body, &[], CONFIGURED),
            [
                "jms invoices producer",
                "jms invoices producer",
                "jms invoices producer",
                "jms unresolved producer",
                "jms unresolved producer",
                "jms unresolved producer",
                "jms billing consumer a.C.on(String)",
            ]
        );
    }

    #[test]
    fn a_kotlin_message_constructed_in_the_call_picks_the_overload() {
        let imports = format!(
            "{KT_RABBIT}import org.springframework.jms.core.JmsTemplate\n\
            import org.springframework.jms.core.MessagePostProcessor\n\
            import com.corp.shared.Dto\n"
        );
        let body = "data class Order(val id: String)\n\
            fun buildMpp(): MessagePostProcessor = MessagePostProcessor { it }\n\
            class C(private val rabbit: RabbitTemplate, private val jms: JmsTemplate) {\n\
            fun f() {\n\
            rabbit.convertAndSend(\"kt.ctor\", Order(\"1\"))\n\
            rabbit.convertAndSend(\"kt.dto\", Dto())\n\
            rabbit.convertAndSend(\"kt.mpp\", buildMpp())\n\
            jms.convertAndSend(\"kt.jms\", Order(\"1\"))\n\
            jms.convertAndSend(\"kt.jmsdto\", Dto())\n\
            jms.convertAndSend(\"kt.jmsmpp\", buildMpp())\n\
            jms.convertAndSend(\"kt.jmssam\", MessagePostProcessor { it })\n}\n}";
        assert_eq!(
            kt(&imports, body, &[], ""),
            [
                "amqp kt.ctor producer",
                "amqp kt.dto producer",
                "amqp unresolved producer",
                "jms kt.jms producer",
                "jms kt.jmsdto producer",
                "jms unresolved producer",
                "jms unresolved producer",
            ]
        );
    }

    #[test]
    fn a_kotlin_sqs_client_sends_and_receives_on_the_queue_its_url_names() {
        let imports = "import software.amazon.awssdk.services.sqs.SqsClient\n\
            import software.amazon.awssdk.services.sqs.model.SendMessageRequest\n\
            import software.amazon.awssdk.services.sqs.model.ReceiveMessageRequest\n\
            import org.springframework.beans.factory.annotation.Value\n";
        let body = "const val URL = \"https://sqs.us-east-1.amazonaws.com/123/orders\"\n\
            class C(private val sqs: SqsClient, @Value(\"\\${app.queue-url}\") private val configured: String) {\n\
            fun send() {\n\
            sqs.sendMessage(SendMessageRequest.builder().queueUrl(URL).messageBody(\"b\").build())\n\
            sqs.sendMessage(SendMessageRequest.builder().queueUrl(configured).build())\n\
            sqs.sendMessage { b -> b.queueUrl(URL) }\n}\n\
            fun poll(url: String): Any = sqs.receiveMessage(ReceiveMessageRequest.builder().queueUrl(url).build())\n\
            fun run() { while (true) { handle(poll(URL)) } }\n\
            fun handle(o: Any) {}\n}";
        assert_eq!(
            kt(imports, body, &[], CONFIGURED),
            [
                "sqs orders producer",
                "sqs invoices producer",
                "sqs unresolved producer",
                "sqs orders consumer a.C.run()",
            ]
        );
    }

    #[test]
    fn a_kotlin_nats_connection_and_dispatcher_publish_and_subscribe() {
        let imports = "import io.nats.client.Connection\nimport io.nats.client.Dispatcher\n";
        let body = "class C(private val nc: Connection) {\n\
            fun f(d: Dispatcher) {\n\
            nc.publish(\"orders.created\", ByteArray(0))\n\
            nc.subscribe(\"orders.*\")\n\
            d.subscribe(\"orders.>\") { m -> }\n}\n\
            fun emit(subject: String) { nc.publish(subject, ByteArray(0)) }\n\
            fun run() { emit(\"orders.paid\") }\n}";
        assert_eq!(
            kt(imports, body, &[], ""),
            [
                "nats orders.created producer",
                "nats orders.* consumer a.C.f(Dispatcher)",
                "nats orders.> consumer a.C.f(Dispatcher)",
                "nats orders.paid producer",
            ]
        );
    }

    #[test]
    fn kotlin_streams_are_read_from_each_written_form() {
        let imports = "import redis.clients.jedis.JedisPooled\n\
            import redis.clients.jedis.StreamEntryID\n\
            import io.lettuce.core.api.sync.RedisCommands\n\
            import io.lettuce.core.Consumer\n\
            import io.lettuce.core.XReadArgs\n\
            import java.util.Collections\n";
        let body = "class C(private val jedis: JedisPooled, private val lettuce: RedisCommands<String, String>) {\n\
            fun f() {\n\
            jedis.xadd(\"orders\", StreamEntryID.NEW_ENTRY, mapOf(\"k\" to \"v\"))\n\
            jedis.xreadGroup(\"g\", \"c\", null, mapOf(\"orders\" to StreamEntryID.UNRECEIVED_ENTRY, \"refunds\" to StreamEntryID.UNRECEIVED_ENTRY))\n\
            jedis.xreadGroup(\"g\", \"c\", null, Collections.singletonMap(\"b\", StreamEntryID.UNRECEIVED_ENTRY))\n\
            jedis.xreadGroup(\"g\", \"c\", null, mapOf(pair))\n\
            lettuce.xadd(\"payments\", mapOf(\"k\" to \"v\"))\n\
            lettuce.xreadgroup(Consumer.from(\"g\", \"c\"), XReadArgs.StreamOffset.lastConsumed(\"payments\"))\n}\n\
            fun poll(stream: String) { jedis.xreadGroup(\"g\", \"c\", null, mapOf(stream to StreamEntryID.UNRECEIVED_ENTRY)) }\n\
            fun run() { poll(\"e\") }\n}";
        assert_eq!(
            kt(imports, body, &[], ""),
            [
                "redis orders producer",
                "redis orders consumer a.C.f()",
                "redis refunds consumer a.C.f()",
                "redis b consumer a.C.f()",
                "redis unresolved consumer a.C.f()",
                "redis payments producer",
                "redis payments consumer a.C.f()",
                "redis e consumer a.C.run()",
            ]
        );
        let own = [(
            "api/src/main/kotlin/a/Own.kt",
            "package a\nfun mapOf(vararg s: Pair<String, StreamEntryID>): Map<String, StreamEntryID> = emptyMap()\n",
        )];
        let body = "class C(private val jedis: JedisPooled) {\n\
            fun f() { jedis.xreadGroup(\"g\", \"c\", null, mapOf(\"orders\" to StreamEntryID.UNRECEIVED_ENTRY)) }\n}";
        assert_eq!(
            kt(imports, body, &own, ""),
            ["redis unresolved consumer a.C.f()"]
        );
        assert_eq!(
            kt(imports, body, &[], ""),
            ["redis orders consumer a.C.f()"]
        );
    }

    #[test]
    fn a_kotlin_call_on_an_implicit_receiver_makes_no_side() {
        let body = "fun KafkaTemplate<String, String>.publish(topic: String, v: String) { send(topic, v); this.send(topic, v) }\n\
            class C(private val kafka: KafkaTemplate<String, String>) {\n\
            fun f() { kafka.publish(\"orders.x\", \"v\"); kafka.apply { send(\"orders.y\", \"v\") }; with(kafka) { send(\"orders.z\", \"v\") } }\n}";
        assert!(kt(KT_KAFKA, body, &[], "").is_empty());
    }

    #[test]
    fn a_member_of_an_implicit_receiver_hides_the_template() {
        let body = "class Holder(val kafka: Other)\n\
            class Plain(val other: Other)\n\
            class Other { fun send(t: String, m: String) {} }\n\
            class C(private val kafka: KafkaTemplate<String, String>) {\n\
            fun a(h: Holder) { with(h) { kafka.send(\"orders.with\", \"m\") } }\n\
            fun b(h: Holder) { h.run { kafka.send(\"orders.run\", \"m\") } }\n\
            fun c(h: Holder) { h.apply { kafka.send(\"orders.apply\", \"m\") } }\n\
            fun d(f: Foreign) { with(f) { kafka.send(\"orders.foreign\", \"m\") } }\n\
            fun e(p: Plain) { with(p) { kafka.send(\"orders.plain\", \"m\") } }\n\
            fun g(h: Holder) { with(h) { val kafka = Other(); kafka.send(\"orders.local\", \"m\") } }\n\
            private fun Holder.ext() { kafka.send(\"orders.ext\", \"m\") }\n\
            private fun Plain.ext() { kafka.send(\"orders.plainext\", \"m\") }\n\
            private fun Foreign.ext() { kafka.send(\"orders.foreignext\", \"m\") }\n}";
        let imports = format!("{KT_KAFKA}import com.corp.shared.Foreign\n");
        assert_eq!(
            kt(&imports, body, &[], ""),
            [
                "kafka orders.plain producer",
                "kafka orders.plainext producer"
            ]
        );
    }

    #[test]
    fn many_annotated_methods_are_read_in_linear_time() {
        let time = |n: usize| {
            let body: String = (0..n)
                .map(|i| format!("@Override void m{i}() {{}}\n"))
                .collect();
            let src = format!("package a;\n{KAFKA}\nclass C {{\n{body}}}\n");
            let facts = Java.parse(MODULE, src.as_bytes()).unwrap();
            let files = BTreeMap::from([(MODULE, &facts)]);
            let layout = Java.layout(&[], &files).unwrap();
            let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
            let code = Code::new(&resolver, &files, &Java);
            let indexed = Indexed {
                manifests: &[],
                tsconfigs: &[],
                exported: &[],
                files: &[],
                configs: &[],
                symlinks: &[],
                channel_clients: &[],
            };
            (0..3)
                .map(|_| {
                    let at = std::time::Instant::now();
                    assert!(messages(&code, &indexed).is_empty());
                    at.elapsed()
                })
                .min()
                .unwrap()
        };
        let (small, large) = (time(8000), time(32000));
        assert!(large < small * 9, "{small:?} then {large:?}");
    }

    #[test]
    fn many_kotlin_binding_chains_are_read_in_linear_time() {
        use crate::lang::kotlin::Kotlin;
        let time = |n: usize| {
            let body: String = (0..n)
                .map(|i| format!(
                    "fun m{i}() = BindingBuilder.bind(Queue(\"q{i}\")).to(TopicExchange(\"e\")).with(\"k\")\n"
                ))
                .collect();
            let src = format!("package a\n{KT_RABBIT}\nclass C {{\n{body}}}\n");
            let facts = Kotlin.parse(KOTLIN, src.as_bytes()).unwrap();
            let files = BTreeMap::from([(KOTLIN, &facts)]);
            let layout = Java.layout(&[], &files).unwrap();
            let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
            let code = Code::new(&resolver, &files, &Java);
            let indexed = Indexed {
                manifests: &[],
                tsconfigs: &[],
                exported: &[],
                files: &[],
                configs: &[],
                symlinks: &[],
                channel_clients: &[],
            };
            (0..3)
                .map(|_| {
                    let at = std::time::Instant::now();
                    assert_eq!(messages(&code, &indexed).len(), n);
                    at.elapsed()
                })
                .min()
                .unwrap()
        };
        let (small, large) = (time(500), time(2000));
        assert!(large < small * 9, "{small:?} then {large:?}");
    }

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
