//! GCP physical identities, shared by contract kinds, provisioning and config uses.
//! A subscription names its own target, never a topic guessed from its id.

pub(crate) mod sdk;

fn bounded(text: &str) -> bool {
    text.len() <= crate::config_values::VALUE_MAX
}

fn project(text: &str) -> bool {
    !text.is_empty()
        && text
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && text
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn pubsub_id(text: &str) -> bool {
    (3..=255).contains(&text.len())
        && text.as_bytes()[0].is_ascii_alphabetic()
        && !text.starts_with("goog")
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.~+%".contains(&b))
}

/// A topic id, or the disjoint `subscriptions/<id>` target of a subscription.
pub(crate) fn pubsub(text: &str, subscription: bool) -> Option<String> {
    if !bounded(text) {
        return None;
    }
    let collection = if subscription {
        "subscriptions"
    } else {
        "topics"
    };
    let id = if text.starts_with("projects/") {
        let parts = text.split('/').collect::<Vec<_>>();
        match parts.as_slice() {
            ["projects", p, c, id] if project(p) && *c == collection => *id,
            _ => return None,
        }
    } else {
        text
    };
    pubsub_id(id).then(|| {
        if subscription {
            format!("subscriptions/{id}")
        } else {
            id.into()
        }
    })
}

/// A Cloud Storage bucket; the object path after its slash is opaque.
pub(crate) fn bucket(text: &str) -> Option<String> {
    if !bounded(text) {
        return None;
    }
    let name = text
        .strip_prefix("gs://")
        .map_or(text, |s| s.split('/').next().unwrap_or(""));
    ((3..=222).contains(&name.len())
        && name
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name
            .as_bytes()
            .last()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        && name
            .split('.')
            .all(|part| !part.is_empty() && part.len() <= 63)
        && name.parse::<std::net::Ipv4Addr>().is_err())
    .then(|| name.into())
}

pub(crate) fn instance(text: &str) -> Option<String> {
    (bounded(text)
        && !text.is_empty()
        && text.as_bytes()[0].is_ascii_lowercase()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    .then(|| text.into())
}

pub(crate) fn database(instance_name: &str, name: &str) -> Option<String> {
    let instance = instance(instance_name)?;
    database_name(name).then(|| format!("{instance}/{name}"))
}

pub(crate) fn database_name(name: &str) -> bool {
    bounded(name)
        && !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Partial physical names keep the existing pattern ceilings; resource paths never become
/// patterns that can cross into a different GCP collection.
pub(crate) fn pattern(ty: &str, text: &str) -> Option<String> {
    (bounded(text)
        && crate::iac::pattern::supported(text)
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-*".contains(&b)))
    .then(|| {
        if ty == "google_pubsub_subscription" {
            format!("subscriptions/{text}")
        } else {
            text.into()
        }
    })
}

pub(crate) fn secret(text: &str) -> Option<String> {
    if !bounded(text) {
        return None;
    }
    let name = if text.starts_with("projects/") {
        let parts = text.split('/').collect::<Vec<_>>();
        match parts.as_slice() {
            ["projects", p, "secrets", name] if project(p) => *name,
            ["projects", p, "secrets", name, "versions", version]
                if project(p)
                    && (*version == "latest"
                        || (!version.is_empty()
                            && version.bytes().all(|b| b.is_ascii_digit()))) =>
            {
                *name
            }
            _ => return None,
        }
    } else {
        text
    };
    (!name.is_empty()
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)))
    .then(|| name.into())
}

/// Structured identifiers alone, with complete kind validation before dropping a project.
pub(crate) fn identified(text: &str) -> Option<String> {
    if text.starts_with("gs://") {
        return Some(format!("resource::gcs::{}", bucket(text)?));
    }
    if !text.starts_with("projects/") {
        return None;
    }
    match text.split('/').nth(2)? {
        "topics" => Some(format!("channel::pubsub::{}", pubsub(text, false)?)),
        "subscriptions" => Some(format!("channel::pubsub::{}", pubsub(text, true)?)),
        "secrets" => Some(format!("resource::gcpsecretmanager::{}", secret(text)?)),
        _ => None,
    }
}

/// Provider-specific aliases never change the established generic AWS key meanings.
pub(crate) fn typed(key: &str, text: &str) -> Option<String> {
    let (prefix, name) = match key {
        "pubsub_topic" => ("channel::pubsub", pubsub(text, false)?),
        "pubsub_subscription" => ("channel::pubsub", pubsub(text, true)?),
        "gcs_bucket" => ("resource::gcs", bucket(text)?),
        "cloudsql_instance" => ("resource::cloudsql", instance(text)?),
        "cloudsql_database" => {
            let (instance, name) = text.split_once('/')?;
            ("resource::cloudsql", database(instance, name)?)
        }
        "gcpsecretmanager_secret" => ("resource::gcpsecretmanager", secret(text)?),
        _ => return None,
    };
    Some(format!("{prefix}::{name}"))
}
