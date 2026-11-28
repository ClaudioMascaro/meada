use crate::lang::{Part, Value, literal};
use crate::resolve::{Code, Expand, Expanded, Inject, Keywords};

pub struct Message {
    pub broker: &'static str,
    pub name: Option<String>,
    pub environment: Option<String>,
    pub path: String,
    pub call: usize,
    pub role: Side,
    pub client: Option<String>,
    pub cap: Option<crate::contract::Level>,
}

#[derive(Clone)]
pub enum Side {
    Producer,
    Publish { exchange: String },
    Binding { exchange: String, pattern: String },
    Consumer(Handler),
}

#[derive(Clone)]
pub enum Handler {
    Def { path: String, name: String },
    Inline { path: String, def: String },
}

pub fn channel_name(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Ref(_) => None,
        Value::Str(parts) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        Value::Str(parts) => Some(literal(parts)),
        _ => Some(None),
    }
}

pub fn queue_name(url: Option<&Value>) -> Option<Option<String>> {
    let parts = match url {
        Some(Value::Str(parts)) => parts,
        Some(Value::Ref(_)) => return None,
        _ => return Some(None),
    };
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

pub type Read<'a> = &'a dyn Fn(&[Value], &Keywords) -> Option<Vec<Option<String>>>;

pub type ReadEnv<'a> =
    &'a dyn Fn(&[Value], &Keywords) -> Option<(Vec<Option<String>>, Option<String>)>;

pub fn env_of(value: &Value) -> Option<String> {
    match crate::names::argument(value)? {
        crate::names::Argument::Environment(name) => Some(name),
        _ => None,
    }
}

pub fn produce<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    broker: &'static str,
    read: Read,
) -> Vec<Message> {
    produce_with(code, Expand::Call(path, n), None, broker, read)
}

pub fn produce_env<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    broker: &'static str,
    read: ReadEnv,
) -> Vec<Message> {
    produce_env_with(code, Expand::Call(path, n), None, broker, read)
}

pub fn produce_with<'a>(
    code: &Code<'a>,
    from: Expand<'a>,
    inject: Option<&Inject<'a, '_>>,
    broker: &'static str,
    read: Read,
) -> Vec<Message> {
    let read = |a: &[Value], k: &Keywords| read(a, k).map(|names| (names, None));
    produce_env_with(code, from, inject, broker, &read)
}

pub fn produce_env_with<'a>(
    code: &Code<'a>,
    from: Expand<'a>,
    inject: Option<&Inject<'a, '_>>,
    broker: &'static str,
    read: ReadEnv,
) -> Vec<Message> {
    (code.expand_with(from, inject, read).into_iter())
        .flat_map(
            |Expanded {
                 site: (site, at),
                 found: (names, environment),
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
                    cap: None,
                })
            },
        )
        .collect()
}

pub fn consume<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    broker: &'static str,
    read: Read,
    handler: Option<Handler>,
) -> Vec<Message> {
    consume_with(code, Expand::Call(path, n), None, broker, read, handler)
}

pub fn consume_env<'a>(
    code: &Code<'a>,
    path: &'a str,
    n: usize,
    broker: &'static str,
    read: ReadEnv,
    handler: Option<Handler>,
) -> Vec<Message> {
    consume_env_with(code, Expand::Call(path, n), None, broker, read, handler)
}

pub fn consume_with<'a>(
    code: &Code<'a>,
    from: Expand<'a>,
    inject: Option<&Inject<'a, '_>>,
    broker: &'static str,
    read: Read,
    handler: Option<Handler>,
) -> Vec<Message> {
    let read = |a: &[Value], k: &Keywords| read(a, k).map(|names| (names, None));
    consume_env_with(code, from, inject, broker, &read, handler)
}

pub fn consume_env_with<'a>(
    code: &Code<'a>,
    from: Expand<'a>,
    inject: Option<&Inject<'a, '_>>,
    broker: &'static str,
    read: ReadEnv,
    handler: Option<Handler>,
) -> Vec<Message> {
    (code.expand_with(from, inject, read).into_iter())
        .flat_map(
            |Expanded {
                 site: (site, at),
                 found: (names, environment),
                 ..
             }| {
                let handler = handler.clone().unwrap_or_else(|| Handler::Def {
                    path: site.into(),
                    name: code.files()[site].calls[at].scope.clone(),
                });
                names.into_iter().map(move |name| Message {
                    environment: environment.clone(),
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
        .collect()
}

pub fn topic(value: &Value) -> Option<Option<String>> {
    Some(channel_name(value)?.filter(|t| !t.starts_with('^')))
}

pub fn topics(value: &Value) -> Option<Vec<Option<String>>> {
    match value {
        Value::List(items) => items.iter().map(topic).collect(),
        value => Some(vec![topic(value)?]),
    }
}

pub type ReadPublish<'a> =
    &'a dyn Fn(&[Value], &Keywords) -> Option<(Option<String>, Option<String>)>;

pub type ReadBinding<'a> =
    &'a dyn Fn(&[Value], &Keywords) -> Option<(Option<String>, Option<String>, Option<String>)>;

pub fn publish<'a>(code: &Code<'a>, path: &'a str, n: usize, read: ReadPublish) -> Vec<Message> {
    publish_with(code, Expand::Call(path, n), None, read)
}

pub fn publish_with<'a>(
    code: &Code<'a>,
    from: Expand<'a>,
    inject: Option<&Inject<'a, '_>>,
    read: ReadPublish,
) -> Vec<Message> {
    (code.expand_with(from, inject, read).into_iter())
        .map(
            |Expanded {
                 site: (site, at),
                 found: (exchange, key),
                 ..
             }| {
                let (name, role) = publication(exchange, key);
                Message {
                    environment: None,
                    broker: "amqp",
                    name,
                    path: site.into(),
                    call: at,
                    role,
                    client: None,
                    cap: None,
                }
            },
        )
        .collect()
}

pub fn bind<'a>(code: &Code<'a>, path: &'a str, n: usize, read: ReadBinding) -> Vec<Message> {
    bind_with(code, Expand::Call(path, n), None, read)
}

pub fn bind_with<'a>(
    code: &Code<'a>,
    from: Expand<'a>,
    inject: Option<&Inject<'a, '_>>,
    read: ReadBinding,
) -> Vec<Message> {
    (code.expand_with(from, inject, read).into_iter())
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
                    environment: None,
                    broker: "amqp",
                    name: Some(queue),
                    path: site.into(),
                    call: at,
                    role: Side::Binding { exchange, pattern },
                    client: None,
                    cap: None,
                })
            },
        )
        .collect()
}

pub fn publication(exchange: Option<String>, key: Option<String>) -> (Option<String>, Side) {
    match exchange {
        Some(e) if e.is_empty() => (key, Side::Producer),
        Some(exchange) => (key, Side::Publish { exchange }),
        None => (None, Side::Producer),
    }
}

#[cfg(test)]
pub fn lines(messages: Vec<Message>) -> Vec<String> {
    (messages.into_iter())
        .map(|m| {
            let name = m.name.as_deref().unwrap_or("unresolved");
            let role = match m.role {
                Side::Producer => "producer".to_string(),
                Side::Consumer(Handler::Def { path, name }) => format!("consumer {path}::{name}"),
                Side::Consumer(Handler::Inline { path, def }) => {
                    format!("consumer inline {path}::{def}")
                }
                Side::Publish { exchange } => format!("publish {exchange}"),
                Side::Binding { exchange, pattern } => format!("binding {exchange} {pattern}"),
            };
            format!("{} {name} {role}", m.broker)
        })
        .collect()
}
