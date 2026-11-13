//! Directionless config uses (ADR 0026, #661), derived from non-secret, uncut stored strings.
//! Only a structured cloud identifier or a final typed key gives evidence of a contract.

use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;

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
    /// In a file the CI reader owns, the CI node that holds the key (ADR 0028): the site the
    /// export writes. `None` elsewhere: the repository.
    pub holder: Option<String>,
}

impl Side {
    pub(crate) fn written(&self, repository: &str) -> Use {
        let mut side = Use::default();
        side.contract = self.contract.clone();
        side.site = (self.holder.clone()).unwrap_or_else(|| repository.into());
        side.at = format!("{}#{}", self.path, self.key);
        side
    }

    pub(crate) fn place(&self) -> String {
        format!("{}#{} (line {})", self.path, self.key, self.line)
    }
}

/// A value of a file the CI reader owns, as static evaluation reads it (ADR 0028,
/// "Expressions"): a `${{ env.X }}` is the literal `env` gives for `X`, and any other
/// expression an unresolved part, `*`. An expression left open stays as written.
fn evaluated<'v>(value: &str, env: impl Fn(&str) -> Option<&'v str>) -> String {
    let (mut out, mut rest) = (String::new(), value);
    while let Some((before, after)) = rest.split_once("${{") {
        let Some((inner, after)) = after.split_once("}}") else {
            break;
        };
        let plain = |name: &&str| name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        let name = inner.trim().strip_prefix("env.").filter(plain);
        // A literal that holds an expression or an asterisk of its own gives no name.
        let literal = (name.and_then(&env)).filter(|v| !v.contains("${{") && !v.contains('*'));
        out += before;
        out += literal.unwrap_or("*");
        rest = after;
    }
    out + rest
}

pub(crate) fn read(db: &Connection, repository: &str) -> Result<Vec<Side>> {
    // A creation's defining name is its provisioner, not a second side using itself.
    // Other config attributes in the same IaC file can still name resources they use.
    let mut definitions = std::collections::HashSet::new();
    // The files a kind's reader recognizes whole, by their content (ADR 0026, #800): the
    // key-name rule reads none of them. Here, the pipelines and actions of the CI reader
    // (ADR 0028), each with the addresses of its CI jobs and CI steps and whether it is a
    // pipeline.
    let mut owned = HashMap::<String, (Vec<String>, bool)>::new();
    // The `env` keys of a CI job or a CI step that hold no config value and define their name.
    let mut others = Vec::new();
    for node in super::nodes(db)? {
        if super::actions::owns(&node) {
            let hollow = node.actions.iter().flat_map(|facts| &facts.hollow);
            let keys = hollow.map(|name| format!("{}.env.{name}", node.address));
            others.extend(keys.map(|key| (node.path.clone(), key)));
            let (nodes, pipeline) = owned.entry(node.path).or_default();
            if node.address == "." {
                *pipeline = node.ty != super::actions::ACTION;
            } else {
                nodes.push(node.address);
            }
        } else if let Some(mapped) = node.provisioned.as_ref().filter(|p| !p.defining.is_empty()) {
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
    let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    // What a `${{ env.X }}` of an owned file reads: each key of that file, with its value when
    // it is a literal string. A key that holds anything else still defines its name.
    if !owned.is_empty() {
        let mut query = db.prepare(
            "SELECT path, key FROM config_values \
             WHERE type != 'string' OR value IS NULL OR cut != 0",
        )?;
        let keys = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        others.extend(keys.collect::<rusqlite::Result<Vec<(String, String)>>>()?);
    }
    let mut entries = HashMap::<&str, BTreeMap<&str, Option<&str>>>::new();
    for (path, key, value, _) in rows.iter().filter(|row| owned.contains_key(&row.0)) {
        entries.entry(path).or_default().insert(key, Some(value));
    }
    for (path, key) in others.iter().filter(|row| owned.contains_key(&row.0)) {
        entries.entry(path).or_default().insert(key, None);
    }
    let mut uses = Vec::new();
    for (path, key, value, line) in &rows {
        if definitions.contains(&(path.clone(), key.clone())) {
            continue;
        }
        // The CI nodes that hold the key, the nearest first: a CI step, then its CI job.
        let holders = owned.get(path).map(|(nodes, _)| {
            let holds = |node: &&String| {
                (key.strip_prefix(node.as_str())).is_some_and(|rest| rest.starts_with(['.', '[']))
            };
            let mut holders: Vec<&str> = nodes.iter().filter(holds).map(|n| &**n).collect();
            holders.sort_by_key(|node| std::cmp::Reverse(node.len()));
            holders
        });
        let text = match &holders {
            // A written asterisk is no unresolved part: such a value is read as before.
            Some(holders) if value.contains("${{") && !value.contains('*') => {
                // The `env` of the CI step, of its CI job, then of the pipeline (an action's
                // file holds none at its root); never the one the key itself is written in.
                let pipeline = owned[path].1.then(|| "env.".to_string());
                let scopes = holders.iter().map(|node| format!("{node}.env."));
                let scopes: Vec<String> = scopes.chain(pipeline).collect();
                evaluated(value, |name| {
                    let outer = scopes
                        .iter()
                        .filter(|scope| !key.starts_with(scope.as_str()));
                    let file = entries.get(path.as_str())?;
                    // The nearest `env` that defines the name decides, literal or not.
                    let mut found = outer.filter_map(|scope| given(file, scope, name));
                    found.next().flatten()
                })
            }
            _ => value.clone(),
        };
        let pattern = text != *value && text.contains('*');
        // The object key after a bucket is opaque; punctuation there cannot change the
        // resource identity. Validate the structured resource name, not that suffix.
        let named = types::identified(&text)
            .filter(|contract| {
                contract.rsplit_once("::").is_some_and(|(_, name)| {
                    !name.is_empty()
                        && !name
                            .bytes()
                            .any(|b| b.is_ascii_whitespace() || b"${}?#".contains(&b))
                        // An unresolved part makes the name a pattern (ADR 0026, "Patterns").
                        && (!name.contains('*') || pattern && super::pattern::supported(name))
                })
            })
            .map(|c| (c, Level::Exact))
            // A key of `with` belongs to the action's vocabulary, not to the cloud's.
            .or_else(|| holders.is_none().then(|| typed(key, value)).flatten());
        if let Some((contract, level)) = named {
            let holder = holders.map(|holders| {
                let address = holders.first().copied().unwrap_or(".");
                format!("{repository}::{path}::{address}")
            });
            let side = Side {
                contract,
                site: format!("{repository}::{path}#{key}"),
                path: path.clone(),
                key: key.clone(),
                line: *line,
                level,
                holder,
            };
            uses.push(side);
        }
    }
    uses.sort();
    uses.dedup();
    Ok(uses)
}

/// What the `env` map at `scope` (a key path that ends in `.`) gives `name` among the keys of
/// its file: `None` when the map does not define the name, and inside it the literal string, or
/// `None` when the map defines the name with anything else.
fn given<'v>(
    file: &BTreeMap<&str, Option<&'v str>>,
    scope: &str,
    name: &str,
) -> Option<Option<&'v str>> {
    // An `env` written as one expression may define any name.
    if file.contains_key(scope.trim_end_matches('.')) {
        return Some(None);
    }
    let key = format!("{scope}{name}");
    if let Some(value) = file.get(key.as_str()) {
        return Some(*value);
    }
    // A list or a map under the name.
    let under = |open: char| {
        let prefix = format!("{key}{open}");
        let from = (Bound::Included(prefix.as_str()), Bound::Unbounded);
        let mut next = file.range::<str, _>(from);
        next.next().is_some_and(|(k, _)| k.starts_with(&prefix))
    };
    (under('.') || under('[')).then_some(None)
}

/// The key-name rule (ADR 0026): a plain name under a typed final key is an `inferred` use.
fn typed(key: &str, value: &str) -> Option<(String, Level)> {
    let last = |step: &str| {
        key == step || key.ends_with(&format!(".{step}")) || key.ends_with(&format!("[\"{step}\"]"))
    };
    let azure = (crate::contract::azure::ALIASES.iter())
        .find_map(|alias| last(alias).then(|| crate::contract::azure::typed(alias, value))?);
    if let Some(contract) = azure {
        return Some((contract, Level::Inferred));
    }
    let alias = [
        "pubsub_topic",
        "pubsub_subscription",
        "gcs_bucket",
        "cloudsql_instance",
        "cloudsql_database",
        "gcpsecretmanager_secret",
    ]
    .into_iter()
    .find(|alias| last(alias));
    if let Some(contract) = alias.and_then(|alias| crate::contract::gcp::typed(alias, value)) {
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
    .find(|(step, _)| last(step))?
    .1;
    Some((format!("{kind}::{value}"), Level::Inferred))
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
