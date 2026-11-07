//! Curated custom resources: the table owns types, this reader owns Kubernetes key paths.

use serde_json::Value;
use std::collections::HashMap;

use super::Obj;
use crate::document::events::Step;
use crate::iac::{Projection, subscription::Subscription, types};

#[derive(Debug, Clone, PartialEq)]
enum Input {
    Literal(String),
    Reference {
        ty: String,
        name: String,
        namespace: Option<String>,
    },
    Unknown,
}

impl Input {
    fn read(raw: Option<&Value>, ty: Option<&str>) -> Self {
        let Some(raw) = raw else { return Self::Unknown };
        if let Some(text) = raw.as_str() {
            return if literal(text) {
                Self::Literal(text.into())
            } else {
                Self::Unknown
            };
        }
        let external = raw.get("external");
        let name = raw.get("name");
        if let (Some(external), None) = (external, name) {
            return Self::read(Some(external), None);
        }
        if external.is_some() {
            return Self::Unknown;
        }
        let Some(name) = name.and_then(Value::as_str).filter(|s| literal(s)) else {
            return Self::Unknown;
        };
        let Some(ty) = ty else { return Self::Unknown };
        let namespace = match raw.get("namespace") {
            None => None,
            Some(Value::String(namespace)) if literal(namespace) => Some(namespace.clone()),
            _ => return Self::Unknown,
        };
        Self::Reference {
            ty: ty.into(),
            name: name.into(),
            namespace,
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::Literal(s) => s.len(),
            Self::Reference {
                ty,
                name,
                namespace,
            } => ty.len() + name.len() + namespace.as_ref().map_or(0, String::len),
            Self::Unknown => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Name {
    entry: String,
    value: Option<String>,
    key: String,
    metadata: bool,
    qualifier: Option<(String, Input)>,
    source: Option<(String, Input)>,
    filter: Option<String>,
    filtered: bool,
    unread: bool,
}

fn at<'a>(mut value: &'a Value, path: &str) -> Option<&'a Value> {
    for step in crate::config_values::sites::parts_without_selectors(path)? {
        value = match step {
            Step::Key(k) => value.get(&k)?,
            Step::Index(i) => value.get(i)?,
        };
    }
    Some(value)
}

pub(super) fn literal(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= crate::config_values::VALUE_MAX
        && !text.contains(['$', '{', '}', '\n', '\r'])
}

fn input(
    value: &Value,
    path: Option<&str>,
    fallback: Option<&str>,
    ty: Option<&str>,
) -> Option<(String, Input)> {
    let path = path?;
    let (key, raw) = match at(value, path) {
        Some(raw) => (path, Some(raw)),
        None => match fallback {
            Some(key) => (key, at(value, key)),
            None => (path, None),
        },
    };
    Some((key.into(), Input::read(raw, ty)))
}

pub(super) fn read(
    value: &Value,
    kind: &str,
    group: &str,
    mappings: &crate::iac::mapping::Config,
) -> Option<Name> {
    let builtin = types::entry("kubernetes", &format!("{kind}.{group}"));
    let configured = mappings.custom(group, kind);
    let (entry, name, fallback) = match (configured, builtin) {
        (Some(custom), _) => (custom.entry.clone(), custom.key_path.as_str(), None),
        (None, Some(entry)) => (
            format!("{}:{}", entry.format, entry.ty),
            entry.name.as_str(),
            entry.fallback.as_deref(),
        ),
        _ => return None,
    };
    let shape = types::named(&entry)?;
    // A malformed spec is not an absent optional name and cannot invoke a default.
    let malformed = value
        .get("spec")
        .is_some_and(|spec| !spec.is_object() && !spec.is_null());
    let (key, raw) = match at(value, name) {
        Some(raw) => (name, Some(raw)),
        None if !malformed => match fallback {
            Some(key) => (key, at(value, key)),
            None => (name, None),
        },
        None => (name, None),
    };
    Some(Name {
        entry: entry.clone(),
        value: raw
            .and_then(Value::as_str)
            .filter(|s| literal(s))
            .map(String::from),
        metadata: key == "metadata.name",
        key: key.into(),
        qualifier: configured
            .is_none()
            .then(|| {
                input(
                    value,
                    shape.qualifier.as_deref(),
                    shape.qualifier_ref.as_deref(),
                    shape.qualifier_type.as_deref(),
                )
            })
            .flatten(),
        source: configured
            .is_none()
            .then(|| {
                input(
                    value,
                    shape.source.as_deref(),
                    shape.source_ref.as_deref(),
                    shape.source_type.as_deref(),
                )
            })
            .flatten(),
        filter: configured.is_none().then(|| shape.filter.clone()).flatten(),
        filtered: configured.is_none()
            && shape
                .filter
                .as_deref()
                .and_then(|key| at(value, key))
                .is_some_and(|v| v.as_str() != Some("")),
        unread: false,
    })
}

/// Root-local references use the same original Kubernetes names as the ordinary reader.
/// A duplicate or absent referent stays unresolved; no metadata-name fallback guesses its API name.
pub(super) struct Targets<'a> {
    objects: &'a [Obj],
    named: HashMap<(&'a str, &'a str, Option<&'a str>, &'a str), Vec<usize>>,
}

impl<'a> Targets<'a> {
    pub(super) fn new(objects: &'a [Obj]) -> Self {
        let mut named: HashMap<_, Vec<usize>> = HashMap::new();
        for (i, object) in objects.iter().enumerate() {
            named
                .entry((
                    object.kind.as_str(),
                    object.group.as_str(),
                    object.ns.as_deref(),
                    object.orig.as_str(),
                ))
                .or_default()
                .push(i);
        }
        Self { objects, named }
    }

    fn reference(&self, input: &Input, namespace: Option<&str>) -> Option<usize> {
        let Input::Reference {
            ty,
            name,
            namespace: explicit,
        } = input
        else {
            return None;
        };
        let (kind, group) = ty.split_once('.')?;
        let found = self.named.get(&(
            kind,
            group,
            explicit.as_deref().or(namespace),
            name.as_str(),
        ))?;
        match found.as_slice() {
            [index] => Some(*index),
            _ => None,
        }
    }

    fn value(&self, input: &Input, namespace: Option<&str>) -> Option<String> {
        match input {
            Input::Literal(text) => Some(text.clone()),
            Input::Reference { .. } => {
                let object = &self.objects[self.reference(input, namespace)?];
                let mapped = object.facts.custom.as_ref()?;
                Some(mapped.physical(&object.name)?.into())
            }
            Input::Unknown => None,
        }
    }

    pub(super) fn references(&self, name: &Name, namespace: Option<&str>) -> Vec<(usize, String)> {
        name.qualifier
            .iter()
            .chain(&name.source)
            .filter_map(|(path, input)| Some((self.reference(input, namespace)?, path.clone())))
            .collect()
    }
}

impl Name {
    pub(super) fn size(&self, block: &str, object_name: &str) -> usize {
        self.entry.len()
            + self.key.len()
            + block.len()
            + 1
            + self.physical(object_name).map_or(0, str::len) * 3
            + self
                .qualifier
                .iter()
                .chain(&self.source)
                .map(|(path, value)| path.len() + value.size() * 4)
                .sum::<usize>()
            + self.filter.as_ref().map_or(0, String::len)
            + 128
    }

    pub(super) fn physical<'a>(&'a self, object_name: &'a str) -> Option<&'a str> {
        if self.metadata && self.value.is_some() {
            Some(object_name)
        } else {
            self.value.as_deref()
        }
    }

    pub(super) fn projection(
        &self,
        block: &str,
        object_name: &str,
        namespace: Option<&str>,
        targets: &Targets<'_>,
    ) -> Projection {
        let name = self
            .physical(object_name)
            .filter(|_| !self.unread)
            .and_then(|name| {
                let entry = types::named(&self.entry)?;
                if let Some((_, qualifier)) = &self.qualifier {
                    let qualifier = targets.value(qualifier, namespace)?;
                    if entry.canonical()?.ty == "google_sql_database" {
                        return crate::contract::gcp::database(&qualifier, name);
                    }
                }
                entry.mapped(name, false)
            });
        let defining = (|| {
            let mut path = if block.is_empty() {
                Vec::new()
            } else {
                crate::config_values::sites::parts_without_selectors(block)?
            };
            path.extend(crate::config_values::sites::parts_without_selectors(
                &self.key,
            )?);
            Some(crate::config_values::key_path(&path))
        })()
        .into_iter()
        .collect();
        Projection {
            name,
            entry: Some(self.entry.clone()),
            defining,
            ..Default::default()
        }
    }

    pub(super) fn subscription(
        &self,
        projection: &Projection,
        namespace: Option<&str>,
        targets: &Targets<'_>,
        address: &str,
        root: &str,
    ) -> Option<Subscription> {
        let entry = types::named(&self.entry)?.canonical()?;
        let target = entry.contract(projection.name.as_deref()?);
        let source = match entry.ty.as_str() {
            "google_pubsub_subscription" => {
                let (_, input) = self.source.as_ref()?;
                let source = targets.value(input, namespace)?;
                format!(
                    "channel::pubsub::{}",
                    crate::contract::gcp::pubsub(&source, false)?
                )
            }
            "azurerm_servicebus_subscription" => {
                let qualified = projection.name.as_deref()?.strip_prefix("subscription:")?;
                let (topic, _) = qualified.rsplit_once('/')?;
                format!("channel::servicebus::{topic}")
            }
            _ => return None,
        };
        Some(Subscription {
            source,
            target,
            node: address.into(),
            root: root.into(),
            inferred: self.filtered,
            reason: format!(
                "{} subscription {address}{}",
                entry.kind,
                if self.filtered { "; filter" } else { "" }
            ),
        })
    }

    /// Crossplane field-path patches that can change any retained identity or route are unread.
    pub(super) fn affected(&self, path: &str) -> bool {
        self.fields_affected(path) || self.namespace_affected(path)
    }

    /// A default namespace matters only when a retained endpoint reference uses it.
    fn namespace_affected(&self, path: &str) -> bool {
        let implicit = self.qualifier.iter().chain(&self.source).any(|(_, input)| {
            matches!(
                input,
                Input::Reference {
                    namespace: None,
                    ..
                }
            )
        });
        if !implicit {
            return false;
        }
        let Some(written) = crate::config_values::sites::parts_without_selectors(path) else {
            return true;
        };
        let namespace = [Step::Key("metadata".into()), Step::Key("namespace".into())];
        namespace.starts_with(&written) || written.starts_with(&namespace)
    }

    pub(super) fn fields_affected(&self, path: &str) -> bool {
        let Some(written) = crate::config_values::sites::parts_without_selectors(path) else {
            return true;
        };
        let entry = types::named(&self.entry);
        let declared = entry.into_iter().flat_map(|entry| {
            [
                Some(entry.name.as_str()),
                entry.fallback.as_deref(),
                entry.qualifier.as_deref(),
                entry.qualifier_ref.as_deref(),
                entry.source.as_deref(),
                entry.source_ref.as_deref(),
                entry.filter.as_deref(),
            ]
            .into_iter()
            .flatten()
        });
        std::iter::once(self.key.as_str())
            .chain(declared)
            .chain(
                self.qualifier
                    .iter()
                    .chain(&self.source)
                    .map(|(path, _)| path.as_str()),
            )
            .chain(self.filter.as_deref())
            .any(|path| {
                crate::config_values::sites::parts_without_selectors(path)
                    .is_none_or(|input| input.starts_with(&written) || written.starts_with(&input))
            })
    }

    pub(super) fn invalidate(&mut self, retain_name: bool) {
        self.unread = true;
        if !retain_name {
            self.value = None;
        }
        self.qualifier = None;
        self.source = None;
    }
}
