//! The message libraries of the TypeScript and JavaScript pack (spec #20). A form is a method
//! called on a client whose origin (`Code::origin`) is the library's own factory, imported
//! from the library's package: a `send` on anything else is no producer.

use std::collections::BTreeMap;

use crate::lang::{Handler, Message, Part, Role, Value, literal, property};
use crate::resolve::Code;

const KAFKAJS: &str = "kafkajs";
const KAFKA_PRODUCER: &str = "new Kafka().producer()";
const KAFKA_CONSUMER: &str = "new Kafka().consumer()";

pub(super) fn messages(code: &Code) -> Vec<Message> {
    let mut out = Vec::new();
    // kafkajs subscriptions, each with the consumer binding it is made on.
    let mut subscriptions = Vec::new();
    // The handler `run` gives each kafkajs consumer binding; the first `run` counts.
    let mut handlers = BTreeMap::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let Some((receiver, method)) = call.callee.rsplit_once('.') else {
                continue;
            };
            if !matches!(method, "send" | "subscribe" | "run") {
                continue;
            }
            let Some(origin) = code.origin(path, &call.scope, receiver) else {
                continue;
            };
            match (origin.package, origin.expr.as_str(), method) {
                (KAFKAJS, KAFKA_PRODUCER, "send") => {
                    for (site, at, names) in code.expand(path, n, &|a| names(a, &["topic"])) {
                        out.extend(names.into_iter().map(|name| Message {
                            broker: "kafka",
                            name,
                            path: site.into(),
                            call: at,
                            role: Role::Producer,
                        }));
                    }
                }
                (KAFKAJS, KAFKA_CONSUMER, "subscribe") => {
                    subscriptions.push((path, n, origin.binding));
                }
                (KAFKAJS, KAFKA_CONSUMER, "run") => {
                    if let (Some(binding), Some(handler)) = (origin.binding, run(code, path, n)) {
                        handlers.entry(binding).or_insert(handler);
                    }
                }
                _ => {}
            }
        }
    }
    for (path, n, binding) in subscriptions {
        let read = |a: &[Value]| names(a, &["topics", "topic"]);
        for (site, at, names) in code.expand(path, n, &read) {
            // With no `run` handler Meada can read, the function that subscribes handles.
            let handler = binding.and_then(|b| handlers.get(&b)).cloned();
            let handler = handler.unwrap_or_else(|| Handler::Def {
                path: site.into(),
                name: code.files()[site].calls[at].scope.clone(),
            });
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

/// The handler a kafkajs `run({ eachMessage })` or `run({ eachBatch })` gives, when it is a
/// function written in place or a definition of the repository.
fn run<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Option<Handler> {
    let scope = &code.files()[path].calls[n].scope;
    let Some(Value::Object(options)) = code.args(path, n)?.first() else {
        return None;
    };
    let handler = ["eachMessage", "eachBatch"]
        .iter()
        .find_map(|key| property(options, key).ok().flatten())?;
    match handler {
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
    let options = match args.first() {
        Some(Value::Object(properties)) => properties,
        Some(Value::Ref(_)) => return None,
        _ => return Some(vec![None]),
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

/// A channel name as evaluated: `None` when it depends on a parameter, `Some(None)` when it is
/// not a literal string (an environment read, a runtime value, a regular expression).
fn name(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Ref(_) => None,
        Value::Str(parts) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        Value::Str(parts) => Some(literal(parts)),
        _ => Some(None),
    }
}
