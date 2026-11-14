//! Directionless config uses (ADR 0026, #661), derived from non-secret, uncut stored strings.
//! Only a structured cloud identifier or a final typed key gives evidence of a contract, and a
//! final typed key gives none in a document the reader of a contract kind recognizes as its own.
//! An identifier in the resource list of an IAM policy statement is a policy grant instead, and a
//! value under a keyword of a JSON Schema document gives no side (ADR 0026, "Orphan sides").

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Bound;

use meada_core::export::{Level, PolicyGrant, Use};
use rusqlite::Connection;

use super::actions::run::{self, Script};
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
/// rule does not read: the first document of a file the document reader read (its summary names a
/// format; it reads no later document of a stream, and a file it ignored is none of its own), and
/// each document of a configuration file that is no IaC source and that the alert reader takes a
/// rule, a route or a receiver from (`alert::documents`).
fn owned(db: &Connection) -> Result<HashMap<String, Vec<Option<usize>>>> {
    let mut owned = HashMap::<_, Vec<_>>::new();
    let mut query = db.prepare("SELECT path, summary FROM documents")?;
    let rows = query.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (path, summary) = row?;
        let summary: crate::document::Summary = serde_json::from_str(&summary)?;
        if summary.format.is_some() {
            owned.entry(path).or_default().push(Some(0));
        }
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

/// The config use sides of a repository, its policy grants, then the `run` scripts of its CI
/// steps as the gate of the command table reads them (ADR 0028): all come from the stored
/// strings, so a script past the value length ceiling is not read.
pub(crate) fn read(
    db: &Connection,
    repository: &str,
) -> Result<(Vec<Side>, Vec<Side>, Vec<Script>)> {
    // A creation's defining name is its provisioner, not a second side using itself.
    // Other config attributes in the same IaC file can still name resources they use.
    let mut definitions = std::collections::HashSet::new();
    // The files a kind's reader recognizes whole, by their content (ADR 0026, #800): the
    // key-name rule reads none of them. Here, the pipelines and actions of the CI reader
    // (ADR 0028), each with the addresses of its CI jobs and CI steps and whether it is a
    // pipeline.
    let mut ci_files = HashMap::<String, (HashSet<String>, bool)>::new();
    // The `env` keys of a CI job or a CI step that hold no config value and define their name.
    let mut others = Vec::new();
    for node in super::nodes(db)? {
        if super::actions::owns(&node) {
            let hollow = node.actions.iter().flat_map(|facts| &facts.hollow);
            let keys = hollow.map(|name| format!("{}.env.{name}", node.address));
            others.extend(keys.map(|key| (node.path.clone(), key)));
            let (nodes, pipeline) = ci_files.entry(node.path).or_default();
            if node.address == "." {
                *pipeline = node.ty != super::actions::ACTION;
            } else {
                nodes.insert(node.address);
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
    if !ci_files.is_empty() {
        let mut query = db.prepare(
            "SELECT path, key FROM config_values \
             WHERE type != 'string' OR value IS NULL OR cut != 0",
        )?;
        let keys = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        others.extend(keys.collect::<rusqlite::Result<Vec<(String, String)>>>()?);
    }
    let mut entries = HashMap::<&str, BTreeMap<&str, Option<&str>>>::new();
    for (path, key, value, _) in rows.iter().filter(|row| ci_files.contains_key(&row.0)) {
        entries.entry(path).or_default().insert(key, Some(value));
    }
    for (path, key) in others.iter().filter(|row| ci_files.contains_key(&row.0)) {
        entries.entry(path).or_default().insert(key, None);
    }
    // The names the `run` scripts of each owned file may write to the environment file
    // (`GITHUB_ENV`, `github.env`): one of them reaches the CI steps after it, so no literal
    // `env` gives it. `None` for a file where a script writes names it does not show: there
    // no literal `env` gives any name.
    let mut written = HashMap::<&str, Option<HashSet<&str>>>::new();
    for (path, key, value, _) in rows.iter().filter(|row| ci_files.contains_key(&row.0)) {
        let Some(shown) = (key.ends_with(".run").then(|| run::environment_file(value))).flatten()
        else {
            continue;
        };
        let names = written.entry(path).or_insert_with(|| Some(HashSet::new()));
        match (shown, names.as_mut()) {
            (true, Some(names)) => names.extend(run::rebound(value)),
            _ => *names = None,
        }
    }
    let schemas = schemas(db)?;
    let recognized = owned(db)?;
    let mut uses = Vec::new();
    let mut grants = Vec::new();
    let mut scripts = Vec::new();
    for (path, key, value, line) in &rows {
        if definitions.contains(&(path.clone(), key.clone())) {
            continue;
        }
        // Whether `documents` (by index in a stream, `None` for a whole file) hold this row; a key
        // with no leading index is in the only document of its file, the first.
        let document = crate::config_values::leading_index(key).unwrap_or(0);
        let holds = |documents: &Vec<Option<usize>>| {
            documents.contains(&None) || documents.contains(&Some(document))
        };
        // The CI nodes that hold the key, the nearest first: a CI step, then its CI job.
        // An address is the key up to a `.` or a `[`: each such cut is looked up, never each node.
        let holders = ci_files.get(path).map(|(nodes, _)| {
            let cuts = key.match_indices(['.', '[']).map(|(at, _)| &key[..at]);
            let mut holders: Vec<&str> = cuts.filter(|cut| nodes.contains(*cut)).collect();
            holders.reverse();
            holders
        });
        // A name a script of the file may write to the environment file holds no literal.
        let rebound = |name: &str| match written.get(path.as_str()) {
            Some(names) => names.as_ref().is_none_or(|names| names.contains(name)),
            None => false,
        };
        // The `env` of the CI step, of its CI job, then of the pipeline (an action's file holds
        // none at its root).
        let scopes = |holders: &[&str], key: &str| -> Vec<String> {
            let pipeline = ci_files[path].1.then(|| key.to_string());
            let scopes = holders.iter().map(|node| format!("{node}.{key}"));
            scopes.chain(pipeline).collect()
        };
        // A CI step is the one node whose address ends in an index.
        let step = holders.as_ref().and_then(|holders| holders.first());
        if let (Some(holders), Some(step)) = (&holders, step.filter(|step| step.ends_with(']')))
            && key.strip_prefix(*step) == Some(".run")
        {
            let file = &entries[path.as_str()];
            // The shell: the CI step's, else `defaults.run.shell` of its CI job, then of the
            // pipeline. `bash`, `sh` or none declared is read; any other shell is not.
            let mut shells = vec![format!("{step}.shell")];
            shells.extend(scopes(&holders[1..], "defaults.run.shell"));
            let shell = shells.iter().find_map(|key| file.get(key.as_str()));
            let envs = scopes(holders, "env.");
            let env = |name: &str| {
                let mut found = envs.iter().filter_map(|scope| given(file, scope, name));
                found.next().flatten().filter(|_| !rebound(name))
            };
            let commands = (shell.is_none_or(|shell| matches!(shell, Some("bash" | "sh"))))
                .then(|| run::commands(value, env))
                .flatten();
            if let Some(commands) = commands.filter(|commands| !commands.is_empty()) {
                let site = format!("{repository}::{path}::{step}");
                scripts.push(Script { site, commands });
            }
        }
        let text = match &holders {
            // A written asterisk is no unresolved part: such a value is read as before.
            Some(holders) if value.contains("${{") && !value.contains('*') => {
                // Never the `env` the key itself is written in.
                let scopes = scopes(holders, "env.");
                evaluated(value, |name| {
                    let outer = scopes
                        .iter()
                        .filter(|scope| !key.starts_with(scope.as_str()));
                    let file = entries.get(path.as_str())?;
                    // The nearest `env` that defines the name decides, literal or not.
                    let mut found = outer.filter_map(|scope| given(file, scope, name));
                    found.next().flatten().filter(|_| !rebound(name))
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
            // A key of `with` belongs to the action's vocabulary, not to the cloud's, and a
            // document the reader of a contract kind recognizes means what that reader says.
            .or_else(|| {
                let owned = holders.is_some() || recognized.get(path).is_some_and(holds);
                (!owned).then(|| key_name(key, value)).flatten()
            });
        let Some((contract, level)) = named else {
            continue;
        };
        let steps = parts_without_selectors(key).unwrap_or_default();
        if schemas.get(path).is_some_and(holds) && schema_keyword(&steps) {
            continue;
        }
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
    scripts.sort_by(|a, b| a.site.cmp(&b.site));
    Ok((uses, grants, scripts))
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
