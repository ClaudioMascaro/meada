//! Environment targets are resolved within one namespace and evaluated overlay.

use super::{Obj, list, text};
use crate::iac::environment::{ENVIRONMENT_BYTES_MAX, Entry, literal};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Input {
    container: Arc<str>,
    name: String,
    source: Source,
}

#[derive(Debug, Clone, PartialEq)]
enum Source {
    Literal(Option<String>),
    Key {
        kind: String,
        name: String,
        key: String,
    },
    From {
        kind: String,
        name: String,
    },
    Unknown,
    UnknownFrom,
}

pub(super) fn read(container: &Value) -> Vec<Input> {
    let container_name: Arc<str> = text(container, &["name"]).unwrap_or_default().into();
    let mut entries = list(container, &["envFrom"])
        .map(|e| {
            let source = [("ConfigMap", "configMapRef"), ("Secret", "secretRef")]
                .into_iter()
                .find_map(|(kind, key)| {
                    text(e, &[key, "name"]).map(|name| Source::From {
                        kind: kind.into(),
                        name: name.into(),
                    })
                })
                .unwrap_or(Source::UnknownFrom);
            Input {
                container: container_name.clone(),
                name: text(e, &["prefix"]).unwrap_or_default().into(),
                source,
            }
        })
        .collect::<Vec<_>>();
    // Explicit entries override envFrom regardless of their order in the document.
    entries.extend(list(container, &["env"]).filter_map(|e| {
        let name = text(e, &["name"])?;
        let source = if let Some(value) = text(e, &["value"]) {
            Source::Literal(literal(value))
        } else if let Some((kind, reference)) =
            [("ConfigMap", "configMapKeyRef"), ("Secret", "secretKeyRef")]
                .into_iter()
                .find(|(_, key)| e["valueFrom"][key].is_object())
        {
            Source::Key {
                kind: kind.into(),
                name: text(&e["valueFrom"], &[reference, "name"])
                    .unwrap_or_default()
                    .into(),
                key: text(&e["valueFrom"], &[reference, "key"])
                    .unwrap_or_default()
                    .into(),
            }
        } else {
            Source::Unknown
        };
        Some(Input {
            container: container_name.clone(),
            name: name.into(),
            source,
        })
    }));
    entries
}

/// Follow references while the objects they name are still in the same overlay stage.
/// Child overlays have already transformed their own references before a parent combines them.
/// A reference to a ConfigMap or Secret two objects of the stage share is dropped; one to a
/// ServiceAccount or Service they share keeps its name, as it did before stages renamed them.
pub(super) fn transform(objects: &mut [Obj], prefix: &str, suffix: &str, namespace: Option<&str>) {
    let mut names = BTreeMap::new();
    for o in objects.iter().filter(|o| {
        matches!(
            o.kind.as_str(),
            "ConfigMap" | "Secret" | "ServiceAccount" | "Service"
        ) && o.group.is_empty()
    }) {
        let key = (
            namespace.or(o.ns.as_deref()).map(str::to_string),
            o.kind.clone(),
            o.name.clone(),
        );
        names
            .entry(key)
            .and_modify(|value| *value = None)
            .or_insert_with(|| Some(format!("{prefix}{}{suffix}", o.name)));
    }
    for o in objects {
        let ns = namespace.or(o.ns.as_deref()).map(str::to_string);
        let mut room = ENVIRONMENT_BYTES_MAX;
        let mut complete = true;
        for input in &mut o.facts.environment {
            let from = matches!(input.source, Source::From { .. });
            let (Source::Key { kind, name, .. } | Source::From { kind, name }) = &mut input.source
            else {
                continue;
            };
            match names.get(&(ns.clone(), kind.clone(), name.clone())) {
                Some(Some(next)) => {
                    let Some(left) = room.checked_sub(next.len()) else {
                        complete = false;
                        break;
                    };
                    room = left;
                    *name = next.clone();
                }
                Some(None) => {
                    input.source = if from {
                        Source::UnknownFrom
                    } else {
                        Source::Unknown
                    }
                }
                None => {}
            }
        }
        if !complete {
            // Prefix expansion must not multiply a large overlay string by every reference.
            o.facts.environment.clear();
        }
        let mut room = ENVIRONMENT_BYTES_MAX;
        o.facts.mounts.retain_mut(|(kind, name, _)| {
            match names.get(&(ns.clone(), (*kind).into(), name.clone())) {
                Some(Some(next)) => {
                    let Some(left) = room.checked_sub(next.len()) else {
                        return false;
                    };
                    room = left;
                    *name = next.clone();
                    true
                }
                Some(None) => false,
                None => true,
            }
        });
        let facts = &mut o.facts;
        let services = (facts.backends.iter_mut().map(|(name, _)| name))
            .chain(facts.ingress.iter_mut().map(|route| &mut route.service));
        for (kind, name) in (facts
            .account
            .iter_mut()
            .map(|name| ("ServiceAccount", name)))
        .chain(services.map(|name| ("Service", name)))
        {
            if let Some(Some(next)) = names.get(&(ns.clone(), kind.into(), name.clone())) {
                // Past the stage's budget the name is unknown, which names no object.
                *name = match room.checked_sub(next.len()) {
                    Some(left) => {
                        room = left;
                        next.clone()
                    }
                    None => String::new(),
                };
            }
        }
    }
}

pub(super) type Configs<'a> = BTreeMap<(Option<&'a str>, &'a str, &'a str), &'a Obj>;

pub(super) fn configs(objects: &[Obj]) -> Configs<'_> {
    objects
        .iter()
        .filter(|o| matches!(o.kind.as_str(), "ConfigMap" | "Secret") && o.group.is_empty())
        .map(|o| ((o.ns.as_deref(), o.kind.as_str(), o.name.as_str()), o))
        .collect()
}

type Entries = BTreeMap<(String, String), Entry>;

fn add(entries: &mut Entries, room: &mut usize, entry: Entry) -> bool {
    let Some(left) = room.checked_sub(entry.size()) else {
        return false;
    };
    *room = left;
    entries.insert((entry.container.clone(), entry.name.clone()), entry);
    true
}

/// An unread later source can replace any earlier imported name with this prefix.
fn unknown(entries: &mut Entries, room: &mut usize, mut entry: Entry) -> bool {
    for ((container, name), previous) in
        entries.range_mut((entry.container.clone(), entry.name.clone())..)
    {
        if container != &entry.container || !name.starts_with(&entry.name) {
            break;
        }
        previous.value = None;
        previous.target.clone_from(&entry.target);
        let Some(left) = room.checked_sub(previous.size()) else {
            return false;
        };
        *room = left;
    }
    entry.name.push('*');
    add(entries, room, entry)
}

pub(super) fn resolve(object: &Obj, configs: &Configs<'_>) -> Vec<Entry> {
    let images = object
        .facts
        .images
        .iter()
        .map(|(name, image)| (name.as_str(), image.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut entries = BTreeMap::new();
    let mut room = ENVIRONMENT_BYTES_MAX;
    for input in &object.facts.environment {
        let entry = |name: String, target: String, value: Option<String>| Entry {
            contract: None,
            container: input.container.to_string(),
            image: images.get(input.container.as_ref()).map(|i| (*i).into()),
            name,
            target,
            value,
        };
        let added = match &input.source {
            Source::Literal(v) => add(
                &mut entries,
                &mut room,
                entry(input.name.clone(), "literal".into(), v.clone()),
            ),
            Source::Unknown => add(
                &mut entries,
                &mut room,
                entry(input.name.clone(), "unresolved".into(), None),
            ),
            Source::UnknownFrom => unknown(
                &mut entries,
                &mut room,
                entry(input.name.clone(), "envFrom (unresolved)".into(), None),
            ),
            Source::Key { kind, name, key } => {
                let config = configs.get(&(object.ns.as_deref(), kind.as_str(), name.as_str()));
                let value = config
                    .and_then(|c| c.facts.data.get(key))
                    .cloned()
                    .flatten();
                let address = config.map_or_else(|| format!("{kind}/{name}"), |c| c.address());
                let mark = if kind == "Secret" { " (redacted)" } else { "" };
                add(
                    &mut entries,
                    &mut room,
                    entry(
                        input.name.clone(),
                        format!("{address} key {key}{mark}"),
                        value,
                    ),
                )
            }
            Source::From { kind, name } => {
                if let Some(config) =
                    configs.get(&(object.ns.as_deref(), kind.as_str(), name.as_str()))
                {
                    let address = config.address();
                    let mark = if kind == "Secret" { " (redacted)" } else { "" };
                    config.facts.data.iter().all(|(key, value)| {
                        add(
                            &mut entries,
                            &mut room,
                            entry(
                                format!("{}{key}", input.name),
                                format!("envFrom {address} key {key}{mark}"),
                                value.clone(),
                            ),
                        )
                    })
                } else {
                    unknown(
                        &mut entries,
                        &mut room,
                        entry(
                            input.name.clone(),
                            format!("envFrom {kind}/{name} (unresolved)"),
                            None,
                        ),
                    )
                }
            }
        };
        if !added {
            // A truncated prefix could miss an overriding entry and invent a value.
            return Vec::new();
        }
    }
    entries.into_values().collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn environment_expansion_accounts_for_a_long_container_name() {
        let entries = (0..512)
            .map(|i| format!("        - {{name: Q{i}, value: orders}}\n"))
            .collect::<String>();
        let source = format!(
            "apiVersion: v1\nkind: Pod\nmetadata: {{name: app}}\nspec:\n  containers:\n    - name: {}\n      image: nginx\n      env:\n{entries}",
            "w".repeat(400 << 10)
        );
        assert!(source.len() < crate::lang::MAX_BYTES);
        let out = crate::kube::evaluate(&[("app.yaml".into(), source.into_bytes())]);
        assert_eq!(out.nodes.len(), 1);
        assert_eq!(out.nodes[0].address, "Pod/app");
        assert!(out.nodes[0].environment.is_empty());
        assert_eq!(out.images.len(), 1);
    }

    #[test]
    fn environment_expansion_past_its_budget_keeps_no_partial_override() {
        let data = (0..600)
            .map(|i| format!("  Q{i}: {}\n", "x".repeat(1024)))
            .collect::<String>();
        let source = format!(
            "apiVersion: v1\nkind: ConfigMap\nmetadata: {{name: queues}}\ndata:\n{data}---\napiVersion: v1\nkind: Pod\nmetadata: {{name: app}}\nspec:\n  containers:\n    - name: app\n      image: nginx\n      envFrom:\n        - configMapRef: {{name: queues}}\n        - configMapRef: {{name: queues}}\n"
        );
        let out = crate::kube::evaluate(&[("app.yaml".into(), source.into_bytes())]);
        let app = out.nodes.iter().find(|n| n.address == "Pod/app").unwrap();
        assert!(app.environment.is_empty());
        assert_eq!(out.images.len(), 1);
    }
}
