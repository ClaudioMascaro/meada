//! What a language reader reports of a channel call, and the helpers the readers share: the
//! queue or topic name as evaluated, and the broker forms (produce, consume, AMQP publish and
//! bind) read through `Code::expand`.

use crate::lang::{Part, Value, literal};
use crate::resolve::{Code, Expand, Expanded, Keywords};

/// A call that produces to or consumes from a channel: a queue or topic of one broker kind.
pub struct Message {
    /// `kafka`, `amqp`, `sqs`, `nats`, `bullmq` or `redis`.
    pub broker: &'static str,
    /// The channel name as evaluated, kept as it is; `None` when it cannot be evaluated (an
    /// unresolved side, never linked).
    pub name: Option<String>,
    /// The call site: the producing or subscribing call, or the call of a client wrapper that
    /// leads to it, as its file and position in `Facts::calls`.
    pub path: String,
    pub call: usize,
    pub role: Side,
}

pub enum Side {
    /// The provider side of the channel.
    Producer,
    /// A publish to a named exchange (`name` is its routing key): the message reaches the
    /// queues bound to the exchange whose pattern matches, wherever the binding is declared.
    Publish { exchange: String },
    /// A binding of the queue `name` to an exchange by a pattern. It is neither side of a
    /// channel; the channel kind keeps it for the links.
    Binding { exchange: String, pattern: String },
    /// The consumer side, with the function that handles what it receives.
    Consumer(Handler),
}

/// Where a consumer's messages are handled.
#[derive(Clone)]
pub enum Handler {
    /// A definition by qualified name; an empty name is the file's top level. A name whose
    /// last segments are inline definitions no kind names stands for the definition that
    /// encloses them.
    Def { path: String, name: String },
    /// An anonymous function written in place: the `Kind::Inline` definition `def` of `path`,
    /// which the channel kind names after the channels it handles.
    Inline { path: String, def: String },
}

/// A channel name as evaluated: `None` when it depends on a parameter, `Some(None)` when it is
/// not a literal string (an environment read, a runtime value, a regular expression).
pub fn channel_name(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Ref(_) => None,
        Value::Str(parts) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        Value::Str(parts) => Some(literal(parts)),
        _ => Some(None),
    }
}

/// The SQS queue the URL `url` names: the last segment of the URL, whatever precedes it (an
/// account and a region are often dynamic). `None` when it depends on a parameter, `Some(None)`
/// when it cannot be read.
pub fn queue_name(url: Option<&Value>) -> Option<Option<String>> {
    let parts = match url {
        Some(Value::Str(parts)) => parts,
        Some(Value::Ref(_)) => return None,
        _ => return Some(None),
    };
    // The literal text at the end: a name folded into the URL leaves two text parts side by side.
    let mut tail: Vec<&str> = (parts.iter().rev())
        .map_while(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    tail.reverse();
    let tail = tail.concat();
    if tail.contains('/') {
        let queue = tail.rsplit('/').next().filter(|q| !q.is_empty());
        return Some(queue.map(String::from));
    }
    match parts.iter().any(|p| matches!(p, Part::Ref(_))) {
        true => None,
        false => Some(None),
    }
}

/// What a form reads of a call's arguments: the channel names.
pub type Read<'a> = &'a dyn Fn(&[Value], &Keywords) -> Option<Vec<Option<String>>>;

/// A producer per name read at each site an expansion found.
pub fn produce<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    broker: &'static str,
    read: Read,
) -> Vec<Message> {
    (code.expand(Expand::Call(path, n), read).into_iter())
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
                    role: Side::Producer,
                })
            },
        )
        .collect()
}

/// A consumer per name read at each site an expansion found: the call's own `handler`, else
/// the function the site is in (a polling client).
pub fn consume<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    broker: &'static str,
    read: Read,
    handler: Option<Handler>,
) -> Vec<Message> {
    (code.expand(Expand::Call(path, n), read).into_iter())
        .flat_map(
            |Expanded {
                 site: (site, at),
                 found: names,
                 ..
             }| {
                let handler = handler.clone().unwrap_or_else(|| Handler::Def {
                    path: site.into(),
                    name: code.files()[site].calls[at].scope.clone(),
                });
                names.into_iter().map(move |name| Message {
                    broker,
                    name,
                    path: site.into(),
                    call: at,
                    role: Side::Consumer(handler.clone()),
                })
            },
        )
        .collect()
}

/// A Kafka topic name (see `channel_name`). A name that starts with `^` is a regular expression
/// that picks topics at run time, so it names none.
pub fn topic(value: &Value) -> Option<Option<String>> {
    Some(channel_name(value)?.filter(|t| !t.starts_with('^')))
}

/// The topics of a list of them, or of the one a value gives. `None` when one depends on a
/// parameter (a client wrapper); a name that is not a literal string is unresolved (`None` in
/// the list).
pub fn topics(value: &Value) -> Option<Vec<Option<String>>> {
    match value {
        Value::List(items) => items.iter().map(topic).collect(),
        value => Some(vec![topic(value)?]),
    }
}

/// What an AMQP publish reads of a call's arguments: the exchange and the routing key.
pub type ReadPublish<'a> =
    &'a dyn Fn(&[Value], &Keywords) -> Option<(Option<String>, Option<String>)>;

/// What an AMQP binding reads of a call's arguments: the queue, the exchange and the pattern.
pub type ReadBinding<'a> =
    &'a dyn Fn(&[Value], &Keywords) -> Option<(Option<String>, Option<String>, Option<String>)>;

/// An AMQP publish at each site an expansion found (`publication`).
pub fn publish<'a>(code: &Code<'a>, path: &'a str, n: usize, read: ReadPublish) -> Vec<Message> {
    (code.expand(Expand::Call(path, n), read).into_iter())
        .map(
            |Expanded {
                 site: (site, at),
                 found: (exchange, key),
                 ..
             }| {
                let (name, role) = publication(exchange, key);
                Message {
                    broker: "amqp",
                    name,
                    path: site.into(),
                    call: at,
                    role,
                }
            },
        )
        .collect()
}

/// An AMQP binding of a queue to an exchange at each site an expansion found.
pub fn bind<'a>(code: &Code<'a>, path: &'a str, n: usize, read: ReadBinding) -> Vec<Message> {
    (code.expand(Expand::Call(path, n), read).into_iter())
        .filter_map(
            |Expanded {
                 site: (site, at),
                 found: bound,
                 ..
             }| {
                // ponytail: a binding that cannot be fully evaluated is dropped, not shown as
                // unresolved.
                let (Some(queue), Some(exchange), Some(pattern)) = bound else {
                    return None;
                };
                Some(Message {
                    broker: "amqp",
                    name: Some(queue),
                    path: site.into(),
                    call: at,
                    role: Side::Binding { exchange, pattern },
                })
            },
        )
        .collect()
}

/// A publish to the exchange `exchange` with the routing key `key`, as its name and role: the
/// empty exchange is the queue named by the key; one that cannot be evaluated leaves the
/// producer unresolved.
pub fn publication(exchange: Option<String>, key: Option<String>) -> (Option<String>, Side) {
    match exchange {
        Some(e) if e.is_empty() => (key, Side::Producer),
        Some(exchange) => (key, Side::Publish { exchange }),
        None => (None, Side::Producer),
    }
}
