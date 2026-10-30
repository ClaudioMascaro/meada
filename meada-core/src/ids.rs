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
    generation_id_of(bundles, &[], &[])
}

/// The generation id with named workspaces (ADR 0021): besides `bundles`' lines, one
/// `# member <identity> <bundle id>` for each repository only named workspaces select and one
/// `# context <id> <digest>` for each context, all sorted together. Without members and contexts
/// it is `generation_id`'s, byte for byte.
pub fn generation_id_of(
    bundles: &[(&str, &str)],
    members: &[(&str, &str)],
    contexts: &[(&str, &str)],
) -> String {
    let plain = (bundles.iter()).map(|(identity, bundle)| format!("{identity} {bundle}\n"));
    let members =
        (members.iter()).map(|(identity, bundle)| format!("# member {identity} {bundle}\n"));
    let contexts = (contexts.iter()).map(|(id, digest)| format!("# context {id} {digest}\n"));
    let mut lines: Vec<String> = plain.chain(members).chain(contexts).collect();
    lines.sort();
    id(&lines.concat())
}

/// A context's digest as the generation id hashes it (ADR 0021): the first 16 hex characters of
/// the BLAKE3 hash of its canonical JSON `{"id","repositories","version"}` as recorded.
pub fn context_digest(context: &Json) -> String {
    id(&context.to_string())
}

/// The id of a named workspace (ADR 0021): the first 16 hex characters of the BLAKE3 hash of the
/// canonical JSON array `["meada workspace", <creator subject>, <name>, <first version id>]`.
/// Deterministic and immutable after creation: a rename, a delegation or a deletion does not
/// change it, and it reads no clock, host or address.
pub fn workspace_id(creator: &str, name: &str, first_version: &str) -> String {
    let parts = Json::Array(vec![
        "meada workspace".into(),
        creator.into(),
        name.into(),
        first_version.into(),
    ]);
    id(&parts.to_string())
}

/// The version id of a named workspace's entries (ADR 0021), in manifest order: `{"blocks",
/// "repo"}` for each, with no `path`, `owner` or `added_by`, so a named version never equals a
/// deployment version and the same entries in two workspaces are one version.
pub fn named_version_id(entries: &[(&str, &Json)]) -> String {
    let entries: Vec<Json> = (entries.iter())
        .map(|(repo, blocks)| {
            let fields = [("blocks", (*blocks).clone()), ("repo", (*repo).into())];
            Json::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
        })
        .collect();
    id(&Json::Array(entries).to_string())
}

/// One entry as `workspace_version_id` hashes it: identity, path, blocks, owner, `added_by`.
pub type VersionEntry<'a> = (&'a str, &'a str, &'a Json, Option<&'a str>, Option<&'a str>);

/// The workspace version id of `entries` (identity, path, blocks as `entry_blocks` gives them,
/// the owner and the login that added it, if any), in manifest order: the hash of their canonical
/// text, a JSON array of one object per entry with its keys sorted,
/// `{"blocks":...,"path":...,"repo":...}`, plus `"owner":...` and `"added_by":...` (#330) each
/// only when the entry has one, so an entry without them keeps the text it had before they
/// existed. Nothing else is hashed: the same entries give the same id on any machine, whoever
/// saves them, whenever and with whatever message.
pub fn workspace_version_id(entries: &[VersionEntry]) -> String {
    let entries: Vec<Json> = (entries.iter())
        .map(|(repo, path, blocks, owner, added_by)| {
            // Inserted in key order, so sorted whether or not the map keeps insertion order.
            let fields = [
                ("added_by", added_by.map(Json::from)),
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
/// (every key but `repo`, `path`, `owner`, `added_by`), as JSON with every table's keys sorted: TOML's spellings
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
                table.remove("added_by");
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
