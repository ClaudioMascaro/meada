//! The channel kind's Go forms: the message libraries of the Go pack (spec #25): segmentio/kafka-go,
//! confluent-kafka-go, sarama, amqp091-go, aws-sdk-go-v2's SQS, nats.go and go-redis. A form is a method called on a value
//! that the source says the library made (`Code::origins` with `Follow::Declarations`: written as
//! its type, or made by its constructor, through the methods that hand the object on, such as
//! `Connection.Channel`; the constructor is spelled from the origin's outside name and steps), so a
//! `Publish` or a `Send` on anything else is no producer. A function of the library is a callee
//! that `Code::outside` names with the package itself as its head. The names are folded like URLs
//! (`Code::expand`), struct fields of composite literals included.
//!
//! A client that calls back (`Subscribe(subject, handler)`) has the callback as its handler; a
//! polling one, the function that holds the call naming the topic or the queue.

use crate::channel::message::{
    Handler, Message, bind, channel_name as name, consume, produce, publish, queue_name, topic,
    topics,
};
use crate::lang::go::modfile::unversioned;
use crate::lang::go::package_call;
use crate::lang::{Call, Value, property};
use crate::resolve::{Code, Follow, Keywords, Of};

#[derive(Clone, Copy, PartialEq)]
enum Client {
    KafkaWriter,
    ConfluentProducer,
    ConfluentConsumer,
    SaramaProducer,
    SaramaConsumer,
    SaramaGroup,
    Amqp,
    Sqs,
    Nats,
    Redis,
}

/// A library by the packages it is imported from (without a major version), what makes its
/// client (the type it is written as, or the function that constructs it) and the methods that
/// hand the client on (`Connection.Channel`).
struct Library {
    client: Client,
    packages: &'static [&'static str],
    makers: &'static [&'static str],
    passes: &'static [&'static str],
}

impl Library {
    /// Whether `origin`, what a value is made by outside the repository, is the library's client.
    fn made(&self, origin: &str) -> bool {
        self.packages.iter().any(|package| {
            let rest = origin.strip_prefix(package);
            let rest = rest.and_then(|r| r.strip_prefix('.'));
            let mut steps = rest.into_iter().flat_map(|r| r.split('.'));
            steps.next().is_some_and(|m| self.makers.contains(&m))
                && steps.all(|m| self.passes.contains(&m))
        })
    }
}

const SARAMA: &[&str] = &["github.com/IBM/sarama", "github.com/Shopify/sarama"];
const REDIS: &[&str] = &["github.com/redis/go-redis", "github.com/go-redis/redis"];
const CONFLUENT: &str = "github.com/confluentinc/confluent-kafka-go/kafka";
const KAFKA_GO: &str = "github.com/segmentio/kafka-go";

const LIBRARIES: &[Library] = &[
    Library {
        client: Client::KafkaWriter,
        packages: &[KAFKA_GO],
        makers: &["Writer", "NewWriter"],
        passes: &[],
    },
    Library {
        client: Client::ConfluentProducer,
        packages: &[CONFLUENT],
        makers: &["Producer", "NewProducer"],
        passes: &[],
    },
    Library {
        client: Client::ConfluentConsumer,
        packages: &[CONFLUENT],
        makers: &["Consumer", "NewConsumer"],
        passes: &[],
    },
    Library {
        client: Client::SaramaProducer,
        packages: SARAMA,
        makers: &[
            "SyncProducer",
            "NewSyncProducer",
            "NewSyncProducerFromClient",
        ],
        passes: &[],
    },
    Library {
        client: Client::SaramaConsumer,
        packages: SARAMA,
        makers: &["Consumer", "NewConsumer", "NewConsumerFromClient"],
        passes: &[],
    },
    Library {
        client: Client::SaramaGroup,
        packages: SARAMA,
        makers: &[
            "ConsumerGroup",
            "NewConsumerGroup",
            "NewConsumerGroupFromClient",
        ],
        passes: &[],
    },
    Library {
        client: Client::Amqp,
        packages: &["github.com/rabbitmq/amqp091-go"],
        makers: &[
            "Channel",
            "Connection",
            "Dial",
            "DialConfig",
            "DialTLS",
            "DialTLS_ExternalAuth",
        ],
        passes: &["Channel"],
    },
    Library {
        client: Client::Sqs,
        packages: &["github.com/aws/aws-sdk-go-v2/service/sqs"],
        makers: &["Client", "NewFromConfig", "New"],
        passes: &[],
    },
    Library {
        client: Client::Nats,
        packages: &["github.com/nats-io/nats.go"],
        makers: &["Conn", "Connect"],
        passes: &[],
    },
    Library {
        client: Client::Redis,
        packages: REDIS,
        makers: &[
            "Client",
            "ClusterClient",
            "UniversalClient",
            "Cmdable",
            "NewClient",
            "NewClusterClient",
            "NewUniversalClient",
            "NewFailoverClient",
        ],
        passes: &[],
    },
];

/// The methods a form is made of.
const METHODS: &[&str] = &[
    "WriteMessages",
    "Produce",
    "Subscribe",
    "SubscribeTopics",
    "SendMessage",
    "SendMessageBatch",
    "ConsumePartition",
    "Consume",
    "Publish",
    "PublishWithContext",
    "QueueBind",
    "ReceiveMessage",
    "QueueSubscribe",
    "XAdd",
    "XReadGroup",
    "NewReader",
];

pub(super) fn messages(code: &Code) -> Vec<Message> {
    if !code.pack().extensions().contains(&"go") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        for (n, stored) in facts.calls.iter().enumerate() {
            let method = (stored.callee.rsplit_once('.')).map_or(stored.callee.as_str(), |m| m.1);
            if !METHODS.contains(&method) {
                continue;
            }
            // The values of the call: `files` holds none, only the facts the index keeps.
            let Some(call) = code.call(path, n) else {
                continue;
            };
            if method == "NewReader" {
                if is_kafka_reader(code, path, call) {
                    out.extend(consume(code, path, n, "kafka", &reader, None));
                }
                continue;
            }
            let Some((receiver, _)) = call.callee.rsplit_once('.') else {
                continue;
            };
            if receiver.starts_with('(') {
                continue;
            }
            let Some(origin) = made_by(code, path, &call.scope, receiver) else {
                continue;
            };
            let origin = unversioned(&origin);
            let Some(library) = LIBRARIES.iter().find(|l| l.made(&origin)) else {
                continue;
            };
            out.extend(form(code, path, n, call, receiver, library.client, method));
        }
    }
    out
}

/// What the source says the value `name`, written in `scope` of `path`, is made by outside the
/// repository, spelled as the imports name it: a type it is written as
/// (`github.com/segmentio/kafka-go.Writer`), or the function that made it with the methods
/// called on the way (`github.com/rabbitmq/amqp091-go.Dial.Channel`). The `Declarations`
/// origin, its outside name and its steps.
fn made_by<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Option<String> {
    let found = code.origins(path, scope, Of::Name(name), Follow::Declarations);
    let origin = found.into_iter().next()?;
    let mut made = origin.outside?.spelled;
    for step in &origin.routes.first()?.steps {
        made = format!("{made}.{step}");
    }
    Some(made)
}

/// What the call at `n` of `path`, `method` of a `client`, produces or consumes.
fn form<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    call: &'a Call,
    receiver: &'a str,
    client: Client,
    method: &str,
) -> Vec<Message> {
    match (client, method) {
        (Client::KafkaWriter, "WriteMessages") => {
            let given = configured(code, path, call, receiver);
            let read = move |a: &[Value], _: &Keywords| match &given {
                Some(topic) => Some(vec![topic.clone()]),
                None => (a.get(1..).unwrap_or(&[]).iter())
                    .map(messages_topics)
                    .try_fold(Vec::new(), |mut all, next| {
                        all.extend(next?);
                        Some(all)
                    }),
            };
            produce(code, path, n, "kafka", &read)
        }
        (Client::ConfluentProducer, "Produce") => {
            produce(code, path, n, "kafka", &|a, _| confluent_topic(arg(a, 0)))
        }
        (Client::ConfluentConsumer, "Subscribe") => consume(
            code,
            path,
            n,
            "kafka",
            &|a, _| Some(vec![topic(arg(a, 0))?]),
            None,
        ),
        (Client::ConfluentConsumer, "SubscribeTopics") => {
            consume(code, path, n, "kafka", &|a, _| topics(arg(a, 0)), None)
        }
        (Client::SaramaProducer, "SendMessage") => produce(code, path, n, "kafka", &|a, _| {
            one_field(arg(a, 0), "Topic", topic)
        }),
        (Client::SaramaConsumer, "ConsumePartition") => consume(
            code,
            path,
            n,
            "kafka",
            &|a, _| Some(vec![topic(arg(a, 0))?]),
            None,
        ),
        (Client::SaramaGroup, "Consume") => {
            let handler = claim_handler(code, path, call);
            consume(code, path, n, "kafka", &|a, _| topics(arg(a, 1)), handler)
        }
        (Client::Amqp, "Publish" | "PublishWithContext") => {
            let first = usize::from(method == "PublishWithContext");
            let read =
                |a: &[Value], _: &Keywords| Some((name(arg(a, first))?, name(arg(a, first + 1))?));
            publish(code, path, n, &read)
        }
        (Client::Amqp, "Consume") => consume(code, path, n, "amqp", &one, None),
        (Client::Amqp, "QueueBind") => {
            // `QueueBind(queue, key, exchange, ..)`.
            let read = |a: &[Value], _: &Keywords| {
                Some((name(arg(a, 0))?, name(arg(a, 2))?, name(arg(a, 1))?))
            };
            bind(code, path, n, &read)
        }
        (Client::Sqs, "SendMessage" | "SendMessageBatch") => {
            produce(code, path, n, "sqs", &|a, _| queue_of(arg(a, 1)))
        }
        (Client::Sqs, "ReceiveMessage") => {
            consume(code, path, n, "sqs", &|a, _| queue_of(arg(a, 1)), None)
        }
        (Client::Nats, "Publish") => produce(code, path, n, "nats", &one),
        (Client::Nats, "Subscribe" | "QueueSubscribe") => {
            let at = if method == "Subscribe" { 1 } else { 2 };
            let handler = callback(code, path, call, at);
            consume(code, path, n, "nats", &one, handler)
        }
        (Client::Redis, "XAdd") => produce(code, path, n, "redis", &|a, _| {
            one_field(arg(a, 1), "Stream", name)
        }),
        (Client::Redis, "XReadGroup") => {
            consume(code, path, n, "redis", &|a, _| streams(arg(a, 1)), None)
        }
        _ => Vec::new(),
    }
}

/// Whether the call `NewReader` of `call` is kafka-go's: a function of the package.
fn is_kafka_reader<'a>(code: &Code<'a>, path: &'a str, call: &'a Call) -> bool {
    is_library(code, path, &call.scope, &call.callee, "NewReader")
}

/// Whether `callee`, written in `scope` of `path`, is the function `function` of kafka-go.
fn is_library<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    callee: &'a str,
    function: &str,
) -> bool {
    let named = callee
        .contains('.')
        .then(|| package_call(code, path, scope, callee));
    named
        .flatten()
        .is_some_and(|n| unversioned(&n) == format!("{KAFKA_GO}.{function}"))
}

/// The topics of `kafka.NewReader(kafka.ReaderConfig{Topic:, GroupTopics:})`.
fn reader(args: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    let config = arg(args, 0);
    let (single, group) = (field(config, "Topic")?, field(config, "GroupTopics")?);
    let mut names = Vec::new();
    if let Some(v) = single {
        names.push(topic(v)?);
    }
    if let Some(v) = group {
        names.extend(topics(v)?);
    }
    Some(match names.is_empty() {
        true => vec![None],
        false => names,
    })
}

/// The topic the Writer `receiver` is configured with when it is a constant the source builds:
/// `Writer{Topic:}` or `NewWriter(WriterConfig{Topic:})`. `None` when it sets none (the messages
/// name it), the name `Some(None)` when it cannot be evaluated.
fn configured<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    receiver: &'a str,
) -> Option<Option<String>> {
    let ((file, constant), value) = code.constant(path, &call.scope, receiver)?;
    let scope = crate::parent(constant);
    let config = match value {
        // Only the library's constructor: a function of the repository of the same name says
        // nothing about the topic its writer has.
        Value::Call(made) if is_library(code, file, scope, &made.callee, "NewWriter") => {
            made.args.first()?
        }
        Value::Call(_) => return None,
        value => value,
    };
    // A `Topic` written after the literal (`w.Topic = ..`) may be any other.
    if code.field_written("Topic") {
        return Some(None);
    }
    let topic = field(config, "Topic")??;
    let folded = code.value(file, scope, topic);
    let named = name(&folded).unwrap_or(None);
    // An empty topic leaves the messages to name it.
    named
        .as_deref()
        .is_none_or(|t| !t.is_empty())
        .then_some(named)
}

/// The topics of the messages a `WriteMessages` is given: a message, or a slice of them. A
/// parameter makes the call a wrapper; anything else is unresolved.
fn messages_topics(value: &Value) -> Option<Vec<Option<String>>> {
    match value {
        Value::List(items) => {
            let each: Option<Vec<_>> = items.iter().map(|m| one_field(m, "Topic", topic)).collect();
            Some(each?.into_iter().flatten().collect())
        }
        value => one_field(value, "Topic", topic),
    }
}

/// The topic `TopicPartition.Topic` of a confluent `Message`, the field a pointer to a string.
fn confluent_topic(message: &Value) -> Option<Vec<Option<String>>> {
    match field(message, "TopicPartition")? {
        Some(partition) => one_field(partition, "Topic", topic),
        None => Some(vec![None]),
    }
}

/// The one name the field `key` of the struct literal `value` gives, read by `read`: unresolved
/// when the literal leaves it out. `None` when it depends on a parameter.
fn one_field(
    value: &Value,
    key: &str,
    read: fn(&Value) -> Option<Option<String>>,
) -> Option<Vec<Option<String>>> {
    Some(vec![match field(value, key)? {
        Some(v) => read(v)?,
        None => None,
    }])
}

/// The queue the `QueueUrl` of an SQS input names.
fn queue_of(input: &Value) -> Option<Vec<Option<String>>> {
    Some(vec![queue_name(field(input, "QueueUrl")?)?])
}

/// The streams of the `Streams` field of an `XReadGroupArgs`: the stream names are the first
/// half of the list, the ids the second.
fn streams(args: &Value) -> Option<Vec<Option<String>>> {
    match field(args, "Streams")? {
        Some(Value::List(items)) if items.len() % 2 == 0 => {
            items[..items.len() / 2].iter().map(name).collect()
        }
        Some(Value::Ref(_)) => None,
        _ => Some(vec![None]),
    }
}

/// The value of the field `key` in the struct literal `value`: `Some(Some(v))`, `Some(None)` when
/// the literal leaves it out or is not one (the field cannot be evaluated), and `None` when the
/// value is a parameter.
fn field<'v>(value: &'v Value, key: &str) -> Option<Option<&'v Value>> {
    let fields = match value {
        Value::Ref(_) => return None,
        Value::Object(fields) => fields,
        Value::New(_, inner) => match &inner[..] {
            [Value::Object(fields)] => fields,
            _ => return Some(None),
        },
        _ => return Some(None),
    };
    property(fields, key).ok()
}

/// The handler a callback argument is: a function written in place or a definition of the
/// repository.
fn callback<'a>(code: &Code<'a>, path: &'a str, call: &'a Call, at: usize) -> Option<Handler> {
    match call.args.get(at)? {
        Value::Function(def) => Some(Handler::Inline {
            path: path.into(),
            def: def.clone(),
        }),
        Value::Ref(name) => code
            .definition(path, &call.scope, name)
            .map(|t| Handler::Def {
                path: t.path.into(),
                name: t.name.into(),
            }),
        _ => None,
    }
}

/// The `ConsumeClaim` method of the handler type a sarama `ConsumerGroup.Consume` is given: the
/// literal written in place, or the name that holds one.
fn claim_handler<'a>(code: &Code<'a>, path: &'a str, call: &'a Call) -> Option<Handler> {
    let held = match call.args.get(2)? {
        Value::New(ty, _) | Value::Ref(ty) => ty,
        _ => return None,
    };
    let target = code.method(path, &call.scope, held, "ConsumeClaim")?;
    Some(Handler::Def {
        path: target.path.into(),
        name: target.name.into(),
    })
}

/// The argument at position `at`, `Undefined` when the call leaves it out.
fn arg(args: &[Value], at: usize) -> &Value {
    args.get(at).unwrap_or(&Value::Undefined)
}

/// The one channel name the first argument gives.
fn one(args: &[Value], _: &Keywords) -> Option<Vec<Option<String>>> {
    Some(vec![name(arg(args, 0))?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::message::Side;
    use crate::lang::LanguagePack;
    use crate::lang::go::Go;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    /// A line per message of the Go file `src`: broker, name (or `unresolved`), role, handler.
    fn lines(src: &str) -> Vec<String> {
        let facts = Go.parse("x.go", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("x.go", &facts)]);
        let modules = Go.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Go);
        let code = Code::new(&resolver, &files, &Go);
        (messages(&code).into_iter())
            .map(|m| {
                let name = m.name.as_deref().unwrap_or("unresolved");
                let (role, handler) = match &m.role {
                    Side::Producer => ("producer", String::new()),
                    Side::Consumer(Handler::Inline { def, .. }) => {
                        // The position of the literal is a byte offset: keep what encloses it.
                        let (parent, _) = def.rsplit_once('@').unwrap();
                        ("consumer", format!(" {parent}@"))
                    }
                    Side::Consumer(Handler::Def { name, .. }) => ("consumer", format!(" {name}")),
                    _ => ("other", String::new()),
                };
                format!("{} {name} {role}{handler}", m.broker)
            })
            .collect()
    }

    #[test]
    fn a_nats_client_produces_and_consumes_with_its_handler() {
        let src = r#"package x

import (
	"os"

	"github.com/nats-io/nats.go"
)

func run() {
	nc, _ := nats.Connect("nats://nats:4222")
	nc.Publish("orders.created", []byte("o"))
	nc.Publish(os.Getenv("SUBJECT"), []byte("o"))
	nc.Subscribe("orders.created", func(m *nats.Msg) {})
}
"#;
        let found = lines(src);
        assert_eq!(
            found,
            [
                "nats orders.created producer",
                "nats unresolved producer",
                "nats orders.created consumer run.@"
            ]
        );
    }
}
