//! Directionless config uses (ADR 0026, #661), derived from non-secret, uncut stored strings.
//! Only a structured cloud identifier or a final typed key gives evidence of a contract.

use meada_core::export::{Level, Use};
use rusqlite::Connection;

use super::types;
use crate::Result;

/// Internal key identity and evidence; the public use keeps a repository-level site.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Side {
    pub contract: String,
    pub site: String,
    pub path: String,
    pub key: String,
    pub line: usize,
    pub level: Level,
}

impl Side {
    pub(crate) fn written(&self, repository: &str) -> Use {
        let mut side = Use::default();
        side.contract = self.contract.clone();
        side.site = repository.into();
        side.at = format!("{}#{}", self.path, self.key);
        side
    }

    pub(crate) fn place(&self) -> String {
        format!("{}#{} (line {})", self.path, self.key, self.line)
    }
}

pub(crate) fn read(db: &Connection, repository: &str) -> Result<Vec<Side>> {
    // A creation's defining name is its provisioner, not a second side using itself.
    // Other config attributes in the same IaC file can still name resources they use.
    let mut definitions = std::collections::HashSet::new();
    for node in super::nodes(db)? {
        if let Some(mapped) = node.provisioned.as_ref().filter(|p| p.entry.is_some()) {
            definitions.extend(
                mapped
                    .defining
                    .iter()
                    .map(|key| (node.path.clone(), key.clone())),
            );
        } else if let Some(entry) = types::node_entry(&node) {
            definitions.insert((node.path, entry.defining_key(&node.block)));
        }
    }
    let mut query = db.prepare(
        "SELECT path, key, value, line FROM config_values \
         WHERE type = 'string' AND value IS NOT NULL AND cut = 0",
    )?;
    let rows = query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)? as usize,
        ))
    })?;
    let mut uses = Vec::new();
    for row in rows {
        let (path, key, value, line) = row?;
        if definitions.contains(&(path.clone(), key.clone())) {
            continue;
        }
        // The object key after a bucket is opaque; punctuation there cannot change the
        // resource identity. Validate the structured resource name, not that suffix.
        let named = types::identified(&value)
            .filter(|contract| {
                contract.rsplit_once("::").is_some_and(|(_, name)| {
                    !name.is_empty()
                        && !name
                            .bytes()
                            .any(|b| b.is_ascii_whitespace() || b"${}*?#".contains(&b))
                })
            })
            .map(|c| (c, Level::Exact))
            .or_else(|| {
                crate::contract::azure::ALIASES
                    .iter()
                    .find_map(|alias| {
                        (key == *alias
                            || key.ends_with(&format!(".{alias}"))
                            || key.ends_with(&format!("[\"{alias}\"]")))
                        .then(|| crate::contract::azure::typed(alias, &value))
                        .flatten()
                    })
                    .map(|contract| (contract, Level::Inferred))
            })
            .or_else(|| {
                let alias = [
                    "pubsub_topic",
                    "pubsub_subscription",
                    "gcs_bucket",
                    "cloudsql_instance",
                    "cloudsql_database",
                    "gcpsecretmanager_secret",
                ]
                .into_iter()
                .find(|alias| {
                    key == *alias
                        || key.ends_with(&format!(".{alias}"))
                        || key.ends_with(&format!("[\"{alias}\"]"))
                });
                if let Some(contract) =
                    alias.and_then(|alias| crate::contract::gcp::typed(alias, &value))
                {
                    return Some((contract, Level::Inferred));
                }
                if value.is_empty()
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                {
                    return None;
                }
                let kind = [
                    ("queue", "channel::sqs"),
                    ("topic", "channel::sns"),
                    ("bucket", "resource::s3"),
                    ("table", "resource::dynamodb"),
                ]
                .into_iter()
                .find(|(step, _)| {
                    key == *step
                        || key.ends_with(&format!(".{step}"))
                        || key.ends_with(&format!("[\"{step}\"]"))
                })?
                .1;
                Some((format!("{kind}::{value}"), Level::Inferred))
            });
        if let Some((contract, level)) = named {
            let side = Side {
                contract,
                site: format!("{repository}::{path}#{key}"),
                path,
                key,
                line,
                level,
            };
            uses.push(side);
        }
    }
    uses.sort();
    uses.dedup();
    Ok(uses)
}

/// Key-specific starts accept the full identity or its repository-relative locator.
pub(crate) fn found<'e>(exports: &'e [crate::index::Export], name: &str) -> Vec<(usize, &'e Side)> {
    exports
        .iter()
        .enumerate()
        .flat_map(|(n, e)| {
            e.config_uses.iter().filter_map(move |u| {
                (u.site == name
                    || u.site.strip_prefix(&format!("{}::", e.repository)) == Some(name))
                .then_some((n, u))
            })
        })
        .collect()
}
