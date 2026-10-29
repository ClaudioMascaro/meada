//! Workspace changes in manifest order, computed from the same entry validation as import.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};
use toml::de::{DeTable, DeValue, ValueDeserializer};

use crate::ids::entry_blocks;
use crate::manifest::{EntryFile, validate_import};
use crate::qualify;

#[derive(Debug, Serialize, PartialEq, Eq, Default)]
pub struct Diff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<Change>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Change {
    pub repo: String,
    pub keys: Vec<String>,
}

struct Entry {
    repo: String,
    fields: Vec<(String, Vec<Json>, usize)>,
}

/// Added and changed entries follow `after`; removed entries follow `before`. Changed dotted
/// keys follow their first occurrence in `after`, then keys removed from `before`. Formatting
/// and comments do not change values; array order does. Invalid texts keep import's errors.
pub fn diff(file: &str, before: &str, after: &str) -> Result<Diff, Vec<String>> {
    let before = entries(file, before)?;
    let after = entries(file, after)?;
    let old: BTreeMap<_, _> = before.iter().map(|entry| (&entry.repo, entry)).collect();
    let new: BTreeMap<_, _> = after.iter().map(|entry| (&entry.repo, entry)).collect();
    let old_order: BTreeMap<_, _> = before
        .iter()
        .filter(|entry| new.contains_key(&entry.repo))
        .enumerate()
        .map(|(position, entry)| (&entry.repo, position))
        .collect();
    let mut position = 0;
    let mut diff = Diff::default();
    for entry in &after {
        let Some(previous) = old.get(&entry.repo) else {
            diff.added.push(entry.repo.clone());
            continue;
        };
        let mut keys = if old_order.get(&entry.repo) == Some(&position) {
            Vec::new()
        } else {
            vec!["repository".to_string()]
        };
        position += 1;
        for (key, _, _) in entry.fields.iter().chain(&previous.fields) {
            let current = entry
                .fields
                .iter()
                .find(|(name, _, _)| name == key)
                .map(|(_, values, _)| values);
            let old = previous
                .fields
                .iter()
                .find(|(name, _, _)| name == key)
                .map(|(_, values, _)| values);
            if current != old && !keys.contains(key) {
                keys.push(key.clone());
            }
        }
        let keys: Vec<_> = keys
            .iter()
            .filter(|key| {
                key.as_str() == "repository"
                    || !keys
                        .iter()
                        .any(|other| other.starts_with(&format!("{key}.")))
            })
            .cloned()
            .collect();
        if !keys.is_empty() {
            diff.changed.push(Change {
                repo: entry.repo.clone(),
                keys,
            });
        }
    }
    diff.removed = before
        .iter()
        .filter(|entry| !new.contains_key(&entry.repo))
        .map(|entry| entry.repo.clone())
        .collect();
    Ok(diff)
}

fn entries(file: &str, src: &str) -> Result<Vec<Entry>, Vec<String>> {
    let checked = validate_import(file, src)?;
    let table = DeTable::parse(src).map_err(|error| vec![error.to_string()])?;
    let Some(repositories) = table.get_ref().get("repository") else {
        return Ok(Vec::new());
    };
    let DeValue::Array(repositories) = repositories.get_ref() else {
        unreachable!("validated repository array");
    };
    Ok(checked
        .into_iter()
        .zip(repositories.iter())
        .map(|(entry, raw)| {
            let DeValue::Table(table) = raw.get_ref() else {
                unreachable!("validated repository table");
            };
            let mut entry = Entry {
                repo: entry.repo,
                fields: Vec::new(),
            };
            fields(table, "repository", &mut entry.fields);
            entry.fields.sort_by_key(|(_, _, at)| *at);
            entry
        })
        .collect())
}

fn fields(table: &DeTable<'_>, prefix: &str, out: &mut Vec<(String, Vec<Json>, usize)>) {
    for (key, value) in table.iter() {
        let name = qualify(prefix, key.get_ref());
        match value.get_ref() {
            DeValue::Table(inner) => {
                out.push((name.clone(), vec![Json::Bool(true)], key.span().start));
                fields(inner, &name, out);
            }
            DeValue::Array(items)
                if !items.is_empty()
                    && items
                        .iter()
                        .all(|item| matches!(item.get_ref(), DeValue::Table(_))) =>
            {
                out.push((name.clone(), vec![Json::Bool(true)], key.span().start));
                let mut rows = Vec::new();
                let mut columns = Vec::new();
                for item in items.iter() {
                    let mut row = Vec::new();
                    if let DeValue::Table(inner) = item.get_ref() {
                        fields(inner, &name, &mut row);
                    }
                    for (key, _, at) in &row {
                        if !columns.iter().any(|(name, _)| name == key) {
                            columns.push((key.clone(), *at));
                        }
                    }
                    rows.push(row);
                }
                for (key, at) in columns {
                    // Missing fields keep their row position; TOML has no null value to confuse it with.
                    let values = rows
                        .iter()
                        .map(|row| {
                            row.iter()
                                .find(|(name, _, _)| name == &key)
                                .map_or(Json::Null, |(_, values, _)| Json::Array(values.clone()))
                        })
                        .collect();
                    out.push((key, values, at));
                }
            }
            _ => {
                let json = Json::deserialize(ValueDeserializer::from(value.clone()))
                    .expect("validated entry fields are JSON values");
                if let Some((_, values, _)) = out.iter_mut().find(|(key, _, _)| key == &name) {
                    values.push(json);
                } else {
                    out.push((name, vec![json], key.span().start));
                }
            }
        }
    }
}

/// One entry of a version document: `repo` and `path`, the `owner` when there is one, then the
/// entry's blocks (`entry_blocks`).
pub fn entry(e: &EntryFile, blocks: Json) -> Json {
    let mut entry = Map::new();
    entry.insert("repo".into(), e.repo.as_str().into());
    entry.insert("path".into(), e.path.as_str().into());
    if let Some(owner) = &e.owner {
        entry.insert("owner".into(), owner.as_str().into());
    }
    if let Json::Object(blocks) = blocks {
        entry.extend(blocks);
    }
    Json::Object(entry)
}

/// The entries of the workspace text `src` as a version document holds them, with import's
/// validation errors when it is not valid.
pub fn version_entries(file: &str, src: &str) -> Result<Vec<Json>, Vec<String>> {
    let entries = validate_import(file, src)?;
    Ok((entries.iter().zip(entry_blocks(src)))
        .map(|(e, blocks)| entry(e, blocks))
        .collect())
}

/// A document's entries as `[[repository]]` tables, `None` when they are not TOML tables. An
/// entry's `owner` follows its `path`, before its block tables: a key after a block's header
/// would belong to the block.
pub fn entries_text(repositories: &[Json]) -> Option<String> {
    if repositories.is_empty() {
        let entries = toml::Value::try_from(repositories).ok()?;
        return toml::to_string(&toml::Table::from_iter([(
            "repository".to_string(),
            entries,
        )]))
        .ok();
    }
    let entries: Vec<String> = (repositories.iter())
        .map(|entry| {
            let toml::Value::Table(mut table) = toml::Value::try_from(entry).ok()? else {
                return None;
            };
            let key = |name: &str, value: Option<toml::Value>| {
                let line = toml::Table::from_iter(value.map(|v| (name.to_string(), v)));
                toml::to_string(&line).ok()
            };
            let (path, owner) = (table.remove("path"), table.remove("owner"));
            let rest = toml::Table::from_iter([(
                "repository".to_string(),
                toml::Value::Array(vec![toml::Value::Table(table)]),
            )]);
            let rest = toml::to_string(&rest).ok()?;
            let (header, rest) = rest.split_once('\n')?;
            Some(format!(
                "{header}\n{}{}{rest}",
                key("path", path)?,
                key("owner", owner)?
            ))
        })
        .collect::<Option<_>>()?;
    Some(entries.join("\n"))
}
