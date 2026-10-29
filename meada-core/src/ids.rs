//! The ids of ADR 0012 (bundle, generation) and ADR 0014 (workspace version), their shared shape
//! and the decimal fence: what anything read from a store, a pointer or Valkey is checked against
//! before it becomes part of a path or a key.

use serde_json::Value as Json;

/// The first 16 hex characters of the BLAKE3 hash of `text`: an id (`^[0-9a-f]{16}$`).
fn id(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
}

/// Whether `text` has the shape of an id: what anything read from the store, a pointer or
/// Valkey is checked against before it becomes part of a path or a key.
pub fn is_id(text: &str) -> bool {
    text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A fence written as decimal digits, as a pointer file or Valkey holds it: nothing but ASCII
/// digits, at least one, within a `u64`.
pub fn fence(text: &str) -> Option<u64> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| text.parse().ok()).flatten()
}

/// The bundle id of one repository: its identity, its `HEAD` commit, the schema version, the
/// binary version and its entry blocks (`blocks`, canonical JSON). The same inputs give the
/// same id on any machine. No input holds a newline: the identity and the commit are single
/// tokens, and JSON escapes its own.
pub fn bundle_id(identity: &str, head: &str, schema: i32, binary: &str, blocks: &str) -> String {
    id(&format!(
        "meada bundle\nidentity {identity}\nhead {head}\nschema {schema}\nbinary {binary}\n\
         blocks {blocks}\n"
    ))
}

/// The generation id of a workspace: the sorted lines `<identity> <bundle id>`.
pub fn generation_id(bundles: &[(&str, &str)]) -> String {
    let mut lines: Vec<String> = (bundles.iter())
        .map(|(identity, bundle)| format!("{identity} {bundle}\n"))
        .collect();
    lines.sort();
    id(&lines.concat())
}

/// The workspace version id of `entries` (identity, path, blocks as `entry_blocks` gives them
/// and the owner, if any), in manifest order: the hash of their canonical text, a JSON array of
/// one object per entry, `{"blocks":...,"path":...,"repo":...}` with `"owner":...` between
/// `blocks` and `path` only when the entry has one, so an entry without an owner keeps the text
/// it had before owners. Nothing else is hashed: the same entries give the same id on any
/// machine, whoever saves them, whenever and with whatever message.
pub fn workspace_version_id(entries: &[(&str, &str, &Json, Option<&str>)]) -> String {
    let entries: Vec<Json> = (entries.iter())
        .map(|(repo, path, blocks, owner)| {
            // Inserted in key order, so sorted whether or not the map keeps insertion order.
            let fields = [
                ("blocks", Some((*blocks).clone())),
                ("owner", owner.map(Json::from)),
                ("path", Some((*path).into())),
                ("repo", Some((*repo).into())),
            ];
            let present = fields.into_iter().filter_map(|(k, v)| Some((k.into(), v?)));
            Json::Object(present.collect())
        })
        .collect();
    id(&Json::Array(entries).to_string())
}

/// Each `[[repository]]` of the manifest text `src`, in order, as its blocks were written
/// (every key but `repo`, `path` and `owner`), as JSON with every table's keys sorted: TOML's spellings
/// of one value give one text.
pub fn entry_blocks(src: &str) -> Vec<Json> {
    let Ok(table) = toml::from_str::<toml::Table>(src) else {
        return Vec::new();
    };
    let Some(toml::Value::Array(entries)) = table.get("repository") else {
        return Vec::new();
    };
    (entries.iter())
        .map(|entry| {
            let mut entry = entry.clone();
            if let toml::Value::Table(table) = &mut entry {
                table.remove("repo");
                table.remove("path");
                table.remove("owner");
            }
            canonical(&entry)
        })
        .collect()
}

fn canonical(value: &toml::Value) -> Json {
    match value {
        toml::Value::String(s) => Json::String(s.clone()),
        toml::Value::Integer(i) => Json::from(*i),
        toml::Value::Float(f) => Json::from(*f),
        toml::Value::Boolean(b) => Json::Bool(*b),
        toml::Value::Datetime(d) => Json::String(d.to_string()),
        toml::Value::Array(items) => Json::Array(items.iter().map(canonical).collect()),
        toml::Value::Table(table) => {
            let mut keys: Vec<_> = table.iter().collect();
            keys.sort_by_key(|(key, _)| *key);
            // Inserted in key order, so sorted whether or not the map keeps insertion order.
            Json::Object(
                keys.into_iter()
                    .map(|(k, v)| (k.clone(), canonical(v)))
                    .collect(),
            )
        }
    }
}
