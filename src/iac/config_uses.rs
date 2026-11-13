//! Directionless config uses (ADR 0026, #661), derived from non-secret, uncut stored strings.
//! Only a structured cloud identifier or a final typed key gives evidence of a contract, and a
//! final typed key gives none in a document the reader of a contract kind recognizes as its own.
//! An identifier in the resource list of an IAM policy statement is a policy grant instead, and a
//! value under a keyword of a JSON Schema document gives no side (ADR 0026, "Orphan sides").

use std::collections::HashMap;

use meada_core::export::{Level, PolicyGrant, Use};
use rusqlite::Connection;

use super::types;
use crate::Result;
use crate::config_values::sites::parts_without_selectors;
use crate::document::events::{Step, Syntax};

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
        side.level = Some(self.level);
        side
    }

    pub(crate) fn granted(&self, repository: &str) -> PolicyGrant {
        let mut grant = PolicyGrant::default();
        grant.contract = self.contract.clone();
        grant.site = repository.into();
        grant.at = format!("{}#{}", self.path, self.key);
        grant
    }

    pub(crate) fn place(&self) -> String {
        format!("{}#{} (line {})", self.path, self.key, self.line)
    }
}

/// The resource list of an IAM policy statement: `Resource` or `NotResource` of an entry of
/// `Statement`, wherever the key path is, and `resources` or `not_resources` of a `statement`
/// block of an `aws_iam_policy_document`.
fn policy_resource(steps: &[Step]) -> bool {
    let keys: Vec<&str> = (steps.iter())
        .filter_map(|step| match step {
            Step::Key(key) => Some(key.as_str()),
            Step::Index(_) => None,
        })
        .collect();
    matches!(
        keys.as_slice(),
        [.., "Statement", "Resource" | "NotResource"]
            | [
                "data",
                "aws_iam_policy_document",
                _,
                "statement",
                "resources" | "not_resources"
            ]
    )
}

/// Under a JSON Schema keyword whose value documents a shape, read from the document's root:
/// the key after `properties` or its kin names a property, whatever its name.
fn schema_keyword(steps: &[Step]) -> bool {
    let mut name = false;
    for step in steps {
        let Step::Key(key) = step else { continue };
        if std::mem::take(&mut name) {
            continue;
        }
        match key.as_str() {
            "examples" | "default" | "enum" | "const" => return true,
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas"
            | "dependencies" => name = true,
            _ => {}
        }
    }
    false
}

/// The JSON Schema documents, by file: those whose root holds `$schema`, as a document index
/// (`None` for a file of one document). A JSON file is one document, so a leading index there is
/// an item of its root array; in YAML it is a document of a stream, or an item of a root sequence,
/// which writes its keys the same way (ADR 0025).
fn schemas(db: &Connection) -> Result<HashMap<String, Vec<Option<usize>>>> {
    let mut query =
        db.prepare("SELECT path, key FROM config_values WHERE key LIKE '%[\"$schema\"]'")?;
    let rows = query.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut schemas = HashMap::<_, Vec<_>>::new();
    for row in rows {
        let (path, key) = row?;
        let document = match parts_without_selectors(&key).as_deref() {
            Some([Step::Key(key)]) if key == "$schema" => None,
            Some([Step::Index(n), Step::Key(key)])
                if key == "$schema" && Syntax::of(&path) == Syntax::Yaml =>
            {
                Some(*n)
            }
            _ => continue,
        };
        schemas.entry(path).or_default().push(document);
    }
    Ok(schemas)
}

/// The documents the reader of a contract kind recognizes as its own, by file, which the key-name
/// rule does not read: a provider document whole (`None`), and each document of a configuration
/// file that is no IaC source and that the alert reader takes a rule, a route or a receiver from
/// (`alert::documents`).
fn owned(db: &Connection) -> Result<HashMap<String, Vec<Option<usize>>>> {
    let mut owned = HashMap::<_, Vec<_>>::new();
    let mut query = db.prepare("SELECT path FROM documents")?;
    for path in query.query_map([], |row| row.get::<_, String>(0))? {
        owned.entry(path?).or_default().push(None);
    }
    let mut query = db.prepare("SELECT path, summary FROM contract_configs")?;
    let rows = query.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (path, summary) = row?;
        if super::is_source(&path) {
            continue;
        }
        let documents = crate::alert::documents(&serde_json::from_str(&summary)?);
        if !documents.is_empty() {
            owned.entry(path).or_default().extend(documents);
        }
    }
    Ok(owned)
}

/// The key-name rule (#661): a value under a final typed key names a contract at `inferred`,
/// through an Azure or Google Cloud alias, or as a plain name under `queue`, `topic`, `bucket` or
/// `table`.
fn key_name(key: &str, value: &str) -> Option<(String, Level)> {
    // Whether the key path ends with the step `name`: alone, after a `.`, or quoted.
    let last = |name: &str| {
        key == name || key.ends_with(&format!(".{name}")) || key.ends_with(&format!("[\"{name}\"]"))
    };
    crate::contract::azure::ALIASES
        .iter()
        .find_map(|alias| {
            last(alias)
                .then(|| crate::contract::azure::typed(alias, value))
                .flatten()
        })
        .map(|contract| (contract, Level::Inferred))
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
            .find(|alias| last(alias));
            if let Some(contract) =
                alias.and_then(|alias| crate::contract::gcp::typed(alias, value))
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
            .find(|(step, _)| last(step))?
            .1;
            Some((format!("{kind}::{value}"), Level::Inferred))
        })
}

/// The config use sides of a repository, then its policy grants.
pub(crate) fn read(db: &Connection, repository: &str) -> Result<(Vec<Side>, Vec<Side>)> {
    // A creation's defining name is its provisioner, not a second side using itself.
    // Other config attributes in the same IaC file can still name resources they use.
    let mut definitions = std::collections::HashSet::new();
    for node in super::nodes(db)? {
        if let Some(mapped) = node.provisioned.as_ref().filter(|p| !p.defining.is_empty()) {
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
    let schemas = schemas(db)?;
    let owned = owned(db)?;
    let mut uses = Vec::new();
    let mut grants = Vec::new();
    for row in rows {
        let (path, key, value, line) = row?;
        if definitions.contains(&(path.clone(), key.clone())) {
            continue;
        }
        // Whether `documents` (by index in a stream, `None` for a whole file) hold this row.
        let document = crate::config_values::leading_index(&key);
        let holds = |documents: &Vec<Option<usize>>| {
            documents.contains(&None) || document.is_some_and(|n| documents.contains(&Some(n)))
        };
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
            // A document the reader of a contract kind recognizes means what that reader says.
            .or_else(|| match owned.get(&path).is_some_and(holds) {
                true => None,
                false => key_name(&key, &value),
            });
        let Some((contract, level)) = named else {
            continue;
        };
        let steps = parts_without_selectors(&key).unwrap_or_default();
        if schemas.get(&path).is_some_and(holds) && schema_keyword(&steps) {
            continue;
        }
        let side = Side {
            contract,
            site: format!("{repository}::{path}#{key}"),
            path,
            key,
            line,
            level,
        };
        if level == Level::Exact && policy_resource(&steps) {
            grants.push(side);
        } else {
            uses.push(side);
        }
    }
    for sides in [&mut uses, &mut grants] {
        sides.sort();
        sides.dedup();
    }
    Ok((uses, grants))
}

/// The sides `of` each export holds (its uses, or its policy grants) at a key: its full
/// identity or its repository-relative locator.
pub(crate) fn found<'e>(
    exports: &'e [crate::index::Export],
    name: &str,
    of: impl Fn(&'e crate::index::Export) -> &'e [Side],
) -> Vec<(usize, &'e Side)> {
    exports
        .iter()
        .enumerate()
        .flat_map(|(n, e)| {
            of(e).iter().filter_map(move |u| {
                (u.site == name
                    || u.site.strip_prefix(&format!("{}::", e.repository)) == Some(name))
                .then_some((n, u))
            })
        })
        .collect()
}
