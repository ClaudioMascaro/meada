//! Kind-owned projection of receiver names. The generic collector supplies scoped evidence.

use std::collections::{BTreeMap, BTreeSet};

use super::{Argument, EnvironmentScope, EnvironmentValue, ReceiverIdentity};

pub(super) fn identity(name: &Argument, sub: Option<&Argument>) -> ReceiverIdentity {
    let name = match name {
        Argument::Literal(name) => crate::contract::azure::servicebus(name)
            .map(Argument::Literal)
            .unwrap_or(Argument::Unresolved),
        name => name.clone(),
    };
    let mut parts = vec![name];
    if let Some(sub) = sub {
        parts.push(match sub {
            Argument::Literal(sub) => {
                // Persist only validated selectors. Invalid name arguments can themselves be
                // credentials or connection strings; they carry no identity evidence.
                if let Some(qualified) = crate::contract::azure::subscription(sub) {
                    Argument::Literal(qualified.trim_start_matches("subscription:").into())
                } else if crate::contract::azure::subscription_target("topic", sub).is_some() {
                    Argument::Literal(sub.clone())
                } else {
                    Argument::Unresolved
                }
            }
            sub => sub.clone(),
        });
    }
    ReceiverIdentity {
        kind: if sub.is_some() {
            "subscription"
        } else {
            "entity"
        }
        .into(),
        parts,
    }
}

pub(super) fn subscription_name(topic: &str, value: &str) -> Option<String> {
    if let Some(name) = crate::contract::azure::subscription(value) {
        return name
            .strip_prefix("subscription:")?
            .rsplit_once('/')
            .filter(|(found, _)| *found == topic)
            .map(|_| name.clone());
    }
    crate::contract::azure::subscription_target(topic, value)
}

pub(crate) fn environment_name(contract: &str, value: &str) -> Option<String> {
    let name = match contract {
        "channel::servicebus" => crate::contract::azure::servicebus(value),
        "channel::eventhubs" => crate::contract::azure::eventhub(value),
        _ => return None,
    }?;
    Some(format!("{contract}::{name}"))
}

struct Choice {
    value: String,
    typed: bool,
    reason: String,
}

enum Choices {
    Literal(Choice),
    Scoped(BTreeMap<EnvironmentScope, Vec<Choice>>),
    Unknown,
}

fn choices(part: &Argument, values: &dyn Fn(&str) -> Vec<EnvironmentValue>) -> Choices {
    match part {
        Argument::Literal(value) => Choices::Literal(Choice {
            value: value.clone(),
            typed: false,
            reason: String::new(),
        }),
        Argument::Environment(variable) => {
            let mut scopes: BTreeMap<_, Vec<_>> = BTreeMap::new();
            for entry in values(variable) {
                // An unread value remains in its scope and prevents accepting a conflicting
                // known value. Typed resource references take precedence over raw text.
                scopes.entry(entry.scope).or_default().push(Choice {
                    typed: entry.target.is_some(),
                    value: entry.target.or(entry.value).unwrap_or_default(),
                    reason: entry.reason,
                });
            }
            Choices::Scoped(scopes)
        }
        Argument::Unresolved => Choices::Unknown,
    }
}

fn unique(values: &[Choice]) -> Option<&Choice> {
    let first = values.first()?;
    (!first.value.is_empty()
        && values
            .iter()
            .all(|v| v.value == first.value && v.typed == first.typed))
    .then_some(first)
}

fn entity(choice: &Choice) -> Option<String> {
    let value = if choice.typed {
        choice.value.strip_prefix("channel::servicebus::")?
    } else {
        &choice.value
    };
    crate::contract::azure::servicebus(value)
}

fn subscription(topic: &Choice, sub: &Choice) -> Option<String> {
    let topic = entity(topic)?;
    if sub.typed {
        let qualified = sub
            .value
            .strip_prefix("channel::servicebus::subscription:")?;
        let (found, name) = qualified.rsplit_once('/')?;
        return (found == topic)
            .then(|| crate::contract::azure::subscription_target(&topic, name))
            .flatten();
    }
    subscription_name(&topic, &sub.value)
}

pub(crate) fn environment_identity(
    contract: &str,
    identity: &ReceiverIdentity,
    values: &dyn Fn(&str) -> Vec<EnvironmentValue>,
) -> Option<Vec<(String, String)>> {
    if contract != "channel::servicebus" {
        return None;
    }
    let mut found = BTreeMap::<String, BTreeSet<String>>::new();
    let mut record = |name: Option<String>, parts: &[&Choice]| {
        if let Some(name) = name {
            found
                .entry(format!("{contract}::{name}"))
                .or_default()
                .extend(
                    parts
                        .iter()
                        .filter(|v| !v.reason.is_empty())
                        .map(|v| v.reason.clone()),
                );
        }
    };
    match (identity.kind.as_str(), identity.parts.as_slice()) {
        ("entity", [name]) => match choices(name, values) {
            Choices::Literal(name) => record(entity(&name), &[&name]),
            Choices::Scoped(scopes) => {
                for name in scopes.values().filter_map(|v| unique(v)) {
                    record(entity(name), &[name]);
                }
            }
            Choices::Unknown => {}
        },
        ("subscription", [topic, sub]) => match (choices(topic, values), choices(sub, values)) {
            (Choices::Literal(topic), Choices::Literal(sub)) => {
                record(subscription(&topic, &sub), &[&topic, &sub]);
            }
            (Choices::Literal(topic), Choices::Scoped(subs)) => {
                for sub in subs.values().filter_map(|v| unique(v)) {
                    record(subscription(&topic, sub), &[&topic, sub]);
                }
            }
            (Choices::Scoped(topics), Choices::Literal(sub)) => {
                for topic in topics.values().filter_map(|v| unique(v)) {
                    record(subscription(topic, &sub), &[topic, &sub]);
                }
            }
            (Choices::Scoped(topics), Choices::Scoped(subs)) => {
                for (scope, topics) in topics {
                    if let Some(topic) = unique(&topics)
                        && let Some(sub) = subs.get(&scope).and_then(|v| unique(v))
                    {
                        record(subscription(topic, sub), &[topic, sub]);
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
    Some(
        found
            .into_iter()
            .map(|(name, reasons)| (name, reasons.into_iter().collect::<Vec<_>>().join("; ")))
            .collect(),
    )
}
