//! RabbitMQ topology identities and local endpoint references, prepared before queries.

use crate::iac::types::{self, TopologyRole as Role};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Cluster {
    Broker,
    Secret,
}

#[derive(Debug, Clone, PartialEq)]
struct Scope {
    cluster: Cluster,
    name: String,
    namespace: Option<String>,
    vhost: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Facts {
    role: Role,
    name: Option<String>,
    scope: Option<Scope>,
    exchange_type: Option<String>,
    source: Option<String>,
    destination: Option<String>,
    destination_type: Option<Role>,
    key: Option<String>,
    unread: bool,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|s| super::custom::literal(s))
        .map(String::from)
}

pub(super) fn read(value: &Value, kind: &str, group: &str) -> Option<Facts> {
    let role = types::topology(&format!("{kind}.{group}"))?;
    let spec = &value["spec"];
    let cluster = &spec["rabbitmqClusterReference"];
    let scope = (|| {
        let (kind, name, namespace) = match (cluster.get("name"), cluster.get("connectionSecret")) {
            (Some(Value::String(name)), None) if super::custom::literal(name) => {
                let namespace = match cluster.get("namespace") {
                    None => None,
                    Some(Value::String(ns)) if super::custom::literal(ns) => Some(ns.clone()),
                    _ => return None,
                };
                (Cluster::Broker, name.clone(), namespace)
            }
            (None, Some(secret)) if cluster.get("namespace").is_none() => {
                (Cluster::Secret, text(secret, "name")?, None)
            }
            _ => return None,
        };
        let vhost = match spec.get("vhost") {
            None => "/".into(),
            Some(Value::String(v)) if super::custom::literal(v) => v.clone(),
            _ => return None,
        };
        Some(Scope {
            cluster: kind,
            name,
            namespace,
            vhost,
        })
    })();
    let key = match spec.get("routingKey") {
        None => Some(String::new()),
        Some(Value::String(key)) if key.is_empty() || super::custom::literal(key) => {
            Some(key.clone())
        }
        _ => None,
    };
    let exchange_type = match spec.get("type") {
        None => Some("direct".into()),
        Some(Value::String(ty)) if super::custom::literal(ty) => Some(ty.clone()),
        _ => None,
    };
    Some(Facts {
        role,
        scope,
        name: text(spec, "name").filter(|s| crate::channel::amqp_name(s)),
        exchange_type,
        source: text(spec, "source"),
        destination: text(spec, "destination"),
        key,
        destination_type: match spec.get("destinationType").and_then(Value::as_str) {
            Some("queue") => Some(Role::Queue),
            Some("exchange") => Some(Role::Exchange),
            _ => None,
        },
        unread: false,
    })
}

impl Facts {
    pub(super) fn physical(&self) -> Option<&str> {
        if self.role == Role::Binding || self.unread {
            None
        } else {
            self.name.as_deref()
        }
    }
    pub(super) fn size(&self) -> usize {
        let strings = self
            .name
            .iter()
            .chain(&self.exchange_type)
            .chain(&self.source)
            .chain(&self.destination)
            .chain(&self.key)
            .map(String::len)
            .sum::<usize>();
        strings * 3
            + self.scope.as_ref().map_or(0, |s| {
                s.name.len() + s.namespace.as_ref().map_or(0, String::len) + s.vhost.len()
            })
            + 128
    }
    pub(super) fn defining(&self, block: &str) -> Vec<String> {
        let keys: &[&str] = match self.role {
            Role::Binding => &["source", "destination", "routingKey"],
            _ => &["name"],
        };
        let dot = if block.is_empty() { "" } else { "." };
        keys.iter()
            .map(|key| format!("{block}{dot}spec.{key}"))
            .collect()
    }
    pub(super) fn affected(&self, path: &str) -> bool {
        let Some(written) = crate::config_values::sites::parts_without_selectors(path) else {
            return true;
        };
        [
            "metadata.namespace",
            "spec.name",
            "spec.type",
            "spec.source",
            "spec.destination",
            "spec.destinationType",
            "spec.routingKey",
            "spec.vhost",
            "spec.rabbitmqClusterReference",
        ]
        .iter()
        .any(|path| {
            crate::config_values::sites::parts_without_selectors(path)
                .is_none_or(|read| read.starts_with(&written) || written.starts_with(&read))
        })
    }
    pub(super) fn invalidate(&mut self) {
        self.unread = true;
    }
}

pub(super) struct View<'a> {
    pub slot: usize,
    pub address: &'a str,
    pub namespace: Option<&'a str>,
    pub facts: &'a Facts,
}

type Key<'a> = (Role, &'a str, Cluster, &'a str, Option<&'a str>, &'a str);

fn key<'a>(view: &'a View<'_>, role: Role, name: &'a str) -> Option<Key<'a>> {
    let scope = view.facts.scope.as_ref()?;
    Some((
        role,
        name,
        scope.cluster,
        scope.name.as_str(),
        scope.namespace.as_deref().or(view.namespace),
        scope.vhost.as_str(),
    ))
}

pub(super) struct Targets<'a> {
    views: &'a [View<'a>],
    names: HashMap<Key<'a>, Vec<usize>>,
}

pub(super) struct Prepared<'a> {
    pub binding: Option<(&'a str, &'a str, &'a str)>,
    pub references: Vec<(usize, &'static str)>,
}

impl<'a> Targets<'a> {
    pub(super) fn new(views: &'a [View<'a>]) -> Self {
        let mut names: HashMap<Key<'_>, Vec<usize>> = HashMap::new();
        for (i, view) in views.iter().enumerate() {
            if let Some(name) = view.facts.physical()
                && let Some(key) = key(view, view.facts.role, name)
            {
                names.entry(key).or_default().push(i);
            }
        }
        Self { views, names }
    }
    fn endpoint(&self, view: &View<'_>, role: Role, name: &str) -> Option<usize> {
        let found = self.names.get(&key(view, role, name)?)?;
        match found.as_slice() {
            [index] => Some(*index),
            _ => None,
        }
    }
    pub(super) fn of(&self, index: usize) -> Prepared<'_> {
        let view = &self.views[index];
        let facts = view.facts;
        let mut prepared = Prepared {
            binding: None,
            references: Vec::new(),
        };
        if facts.role != Role::Binding || facts.unread {
            return prepared;
        }
        let source = facts
            .source
            .as_deref()
            .and_then(|name| self.endpoint(view, Role::Exchange, name));
        let destination = facts
            .destination_type
            .zip(facts.destination.as_deref())
            .and_then(|(ty, name)| self.endpoint(view, ty, name));
        if let Some(source) = source {
            prepared.references.push((source, "spec.source"));
        }
        if let Some(destination) = destination {
            prepared.references.push((destination, "spec.destination"));
        }
        if facts.destination_type == Some(Role::Queue)
            && let (Some(source), Some(_), Some(name), Some(exchange), Some(routing_key)) = (
                source,
                destination,
                facts.destination.as_deref(),
                facts.source.as_deref(),
                facts.key.as_deref(),
            )
        {
            let ty = self.views[source]
                .facts
                .exchange_type
                .as_deref()
                .unwrap_or("");
            if let Some(pattern) = crate::channel::topology_pattern(ty, routing_key) {
                prepared.binding = Some((name, exchange, pattern));
            }
        }
        prepared
    }
}
