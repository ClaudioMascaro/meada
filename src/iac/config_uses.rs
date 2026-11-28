use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Bound;

use meada_core::export::{Level, PolicyGrant, Use};
use rusqlite::Connection;

use super::actions::run::{self, Script};
use super::types;
use crate::Result;
use crate::config_values::sites::parts_without_selectors;
use crate::document::events::{Step, Syntax};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Side {
    pub contract: String,
    pub site: String,
    pub path: String,
    pub key: String,
    pub line: usize,
    pub level: Level,
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

fn evaluated<'v>(value: &str, env: impl Fn(&str) -> Option<&'v str>) -> String {
    let (mut out, mut rest) = (String::new(), value);
    while let Some((before, after)) = rest.split_once("${{") {
        let Some((inner, after)) = after.split_once("}}") else {
            break;
        };
        let plain = |name: &&str| name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        let name = inner.trim().strip_prefix("env.").filter(plain);
        let literal = (name.and_then(&env)).filter(|v| !v.contains("${{") && !v.contains('*'));
        out += before;
        out += literal.unwrap_or("*");
        rest = after;
    }
    out + rest
}

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

fn key_name(key: &str, value: &str) -> Option<(String, Level)> {
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

pub(crate) fn read(
    db: &Connection,
    repository: &str,
) -> Result<(Vec<Side>, Vec<Side>, Vec<Script>)> {
    let mut definitions = std::collections::HashSet::new();
    let mut ci_files = HashMap::<String, (HashSet<String>, bool)>::new();
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
        let document = crate::config_values::leading_index(key).unwrap_or(0);
        let holds = |documents: &Vec<Option<usize>>| {
            documents.contains(&None) || documents.contains(&Some(document))
        };
        let holders = ci_files.get(path).map(|(nodes, _)| {
            let cuts = key.match_indices(['.', '[']).map(|(at, _)| &key[..at]);
            let mut holders: Vec<&str> = cuts.filter(|cut| nodes.contains(*cut)).collect();
            holders.reverse();
            holders
        });
        let rebound = |name: &str| match written.get(path.as_str()) {
            Some(names) => names.as_ref().is_none_or(|names| names.contains(name)),
            None => false,
        };
        let scopes = |holders: &[&str], key: &str| -> Vec<String> {
            let pipeline = ci_files[path].1.then(|| key.to_string());
            let scopes = holders.iter().map(|node| format!("{node}.{key}"));
            scopes.chain(pipeline).collect()
        };
        let step = holders.as_ref().and_then(|holders| holders.first());
        if let (Some(holders), Some(step)) = (&holders, step.filter(|step| step.ends_with(']')))
            && key.strip_prefix(*step) == Some(".run")
        {
            let file = &entries[path.as_str()];
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
            Some(holders) if value.contains("${{") && !value.contains('*') => {
                let scopes = scopes(holders, "env.");
                evaluated(value, |name| {
                    let outer = scopes
                        .iter()
                        .filter(|scope| !key.starts_with(scope.as_str()));
                    let file = entries.get(path.as_str())?;
                    let mut found = outer.filter_map(|scope| given(file, scope, name));
                    found.next().flatten().filter(|_| !rebound(name))
                })
            }
            _ => value.clone(),
        };
        let pattern = text != *value && text.contains('*');
        let named = types::identified(&text)
            .filter(|contract| {
                contract.rsplit_once("::").is_some_and(|(_, name)| {
                    !name.is_empty()
                        && !name
                            .bytes()
                            .any(|b| b.is_ascii_whitespace() || b"${}?#".contains(&b))
                        && (!name.contains('*') || pattern && super::pattern::supported(name))
                })
            })
            .map(|c| (c, Level::Exact))
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

fn given<'v>(
    file: &BTreeMap<&str, Option<&'v str>>,
    scope: &str,
    name: &str,
) -> Option<Option<&'v str>> {
    let holds = |prefix: String| {
        let from = (Bound::Included(prefix.as_str()), Bound::Unbounded);
        let mut next = file.range::<str, _>(from);
        next.next().is_some_and(|(k, _)| k.starts_with(&prefix))
    };
    let map = scope.trim_end_matches('.');
    if file.contains_key(map) || holds(format!("{map}[\"<<\"]")) {
        return Some(None);
    }
    let key = format!("{scope}{name}");
    if let Some(value) = file.get(key.as_str()) {
        return Some(*value);
    }
    (holds(format!("{key}.")) || holds(format!("{key}["))).then_some(None)
}

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
