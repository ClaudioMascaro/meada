//! Cargo packages (spec #30): a `Cargo.toml` with a `[package]` name provides
//! `package::cargo::<name>`, and each dependency entry consumes the crate it names. The entries
//! come from the Rust pack's walker (`crates::entries`), so a manifest is read one way only.
//!
//! An entry keeps where its crate comes from in `Entry::via`: `path <manifest>` (the
//! `Cargo.toml` the path names, inside the repository), `git <identity>` (the remote, as
//! `repo::normalize_remote` spells it) or nothing for a version. A path that leaves the
//! repository names no workspace crate and is dropped, as is any entry that gives no valid name.
//! Cargo ranges are never checked, and neither `Cargo.lock` nor a registry is read.

use std::collections::HashMap;

use super::{Entry, MAX_BYTES, MAX_NAME, Manifest, Summary, directory};
use crate::lang::rust::crates::{crate_name, entries, join, workspace_deps};
use crate::repo::normalize_remote;

/// The field a `[workspace.dependencies]` entry is kept under, in `Summary::workspace`.
const WORKSPACE: &str = "workspace.dependencies";

/// Reads a `Cargo.toml` from its bytes. Hostile input: a file too large, not UTF-8 or not TOML
/// reads as empty; a field of another type counts as absent; a name Cargo would refuse makes no
/// provider; a `workspace = true` entry is kept as such until `inherit` fills it.
pub fn read(path: &str, src: &[u8]) -> Summary {
    let table: Option<toml::Table> = (src.len() <= MAX_BYTES)
        .then(|| std::str::from_utf8(src).ok()?.parse().ok())
        .flatten();
    let Some(table) = table else {
        return Summary::default();
    };
    let dir = directory(path);
    let dir = if dir == "." { "" } else { dir };
    let text = |v: Option<&toml::Value>| match v {
        Some(toml::Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    let package = table.get("package");
    let name = text(package.and_then(|p| p.get("name"))).filter(|n| valid_name(n));
    let mut dependencies: Vec<Entry> = Vec::new();
    for (field, key, spec) in entries(&table) {
        let Some(entry) = entry(dir, field, key, spec) else {
            continue;
        };
        if !dependencies
            .iter()
            .any(|e| (&e.field, e.key()) == (&entry.field, entry.key()))
        {
            dependencies.push(entry);
        }
    }
    dependencies.sort_by(|a, b| (&a.field, a.key()).cmp(&(&b.field, b.key())));
    let mut workspace: Vec<Entry> = Vec::new();
    for (key, spec) in workspace_deps(&table).into_iter().flatten() {
        workspace.extend(entry(dir, WORKSPACE, key, spec));
    }
    workspace.sort_by(|a, b| a.key().cmp(b.key()));
    // A library with no `path` keeps `src/lib.rs`; both are relative to the manifest.
    let lib = table.get("lib").and_then(|l| l.get("path")?.as_str());
    Summary {
        version: text(package.and_then(|p| p.get("version"))).filter(|_| name.is_some()),
        main: name
            .is_some()
            .then(|| lib.unwrap_or("src/lib.rs").to_string()),
        name,
        dependencies,
        workspace,
        ..Summary::default()
    }
}

/// The dependency `key = spec` of the table `field`, for a manifest in `dir` (empty at the
/// root); `None` when its crate has no valid name or its path leaves the repository.
fn entry(dir: &str, field: &str, key: &str, spec: &toml::Value) -> Option<Entry> {
    let get = |name: &str| spec.get(name)?.as_str().filter(|s| !s.is_empty());
    if spec.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
        return valid_name(key).then(|| Entry {
            field: field.to_string(),
            name: key.to_string(),
            inherits: true,
            ..Entry::default()
        });
    }
    let name = get("package").unwrap_or(key);
    if !valid_name(name) || !valid_name(key) {
        return None;
    }
    let via = match (get("path"), get("git")) {
        (Some(path), _) => {
            let at = join(dir, path)?;
            let manifest = if at.is_empty() {
                "Cargo.toml".to_string()
            } else {
                format!("{at}/Cargo.toml")
            };
            Some(format!("path {manifest}"))
        }
        (None, Some(url)) => Some(format!("git {}", normalize_remote(url))),
        (None, None) => None,
    };
    let range = match spec {
        toml::Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => get("version").map(String::from),
    };
    Some(Entry {
        field: field.to_string(),
        name: name.to_string(),
        alias: (name != key).then(|| key.to_string()),
        range,
        via,
        inherits: false,
    })
}

/// Fills each `workspace = true` entry of a Cargo manifest from the `[workspace.dependencies]`
/// of the nearest manifest above it (or itself) that has any; one that finds no entry of its key
/// is dropped, as Cargo refuses it. The member keeps its own table (`dev-dependencies`...) and
/// key, so a workspace entry that renames a crate renames it for the member too.
pub fn inherit(manifests: &mut [Manifest]) {
    let roots: Vec<(String, HashMap<String, Entry>)> = (manifests.iter())
        .filter(|m| m.path.ends_with("Cargo.toml") && !m.summary.workspace.is_empty())
        .map(|m| {
            let table = (m.summary.workspace.iter())
                .map(|e| (e.key().to_string(), e.clone()))
                .collect();
            (directory(&m.path).to_string(), table)
        })
        .collect();
    for m in manifests
        .iter_mut()
        .filter(|m| m.path.ends_with("Cargo.toml"))
    {
        if !m.summary.dependencies.iter().any(|e| e.inherits) {
            continue;
        }
        let dir = directory(&m.path);
        let near = (roots.iter())
            .filter(|(root, _)| root == "." || dir == root || dir.starts_with(&format!("{root}/")))
            .max_by_key(|(root, _)| if root == "." { 0 } else { root.len() + 1 });
        let entries = std::mem::take(&mut m.summary.dependencies);
        m.summary.dependencies = entries
            .into_iter()
            .filter_map(|e| {
                if !e.inherits {
                    return Some(e);
                }
                let from = near?.1.get(&e.name)?;
                Some(Entry {
                    field: e.field,
                    alias: (from.name != e.name).then(|| e.name.clone()),
                    inherits: false,
                    name: from.name.clone(),
                    ..from.clone()
                })
            })
            .collect();
        m.summary
            .dependencies
            .sort_by(|a, b| (&a.field, a.key()).cmp(&(&b.field, b.key())));
    }
}

/// Whether `name` can be a package name: Cargo's alphabet (ASCII letters and digits, `-` and
/// `_`), not empty, at most `MAX_NAME` bytes.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether `specifier`, the first segment of a `use` path, may name a dependency: a crate
/// identifier that is not the standard library's.
pub fn names_crate(specifier: &str) -> bool {
    !matches!(specifier, "std" | "core" | "alloc")
        && valid_name(specifier)
        && !specifier.contains('-')
}

/// What a `use` writes for the dependency `entry`: its key with hyphens turned into
/// underscores.
pub fn import_name(entry: &Entry) -> String {
    crate_name(entry.key())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_at(path: &str, toml: &str) -> Summary {
        read(path, toml.as_bytes())
    }

    type Row<'a> = (&'a str, &'a str, &'a str, Option<&'a str>, Option<&'a str>);

    fn keys(s: &Summary) -> Vec<Row<'_>> {
        (s.dependencies.iter())
            .map(|e| {
                (
                    e.field.as_str(),
                    e.key(),
                    e.name.as_str(),
                    e.range.as_deref(),
                    e.via.as_deref(),
                )
            })
            .collect()
    }

    #[test]
    fn a_manifest_reads_its_name_version_library_and_entries() {
        let s = read_at(
            "crates/api/Cargo.toml",
            "[package]\nname = \"api-lib\"\nversion = \"0.4.1\"\n[lib]\npath = \"src/x.rs\"\n\
             [dependencies]\nserde = \"1\"\nlocal = { path = \"../core\", version = \"2\" }\n\
             pinned = { package = \"real\", git = \"git@example.com:org/Repo.git\" }\n\
             [dev-dependencies]\nfar = { path = \"../../../x\" }\n\
             [target.'cfg(unix)'.build-dependencies]\nsys = { version = \"0.1\" }\n",
        );
        assert_eq!(s.name.as_deref(), Some("api-lib"));
        assert_eq!(s.version.as_deref(), Some("0.4.1"));
        assert_eq!(s.main.as_deref(), Some("src/x.rs"));
        assert_eq!(
            keys(&s),
            [
                ("build-dependencies", "sys", "sys", Some("0.1"), None),
                (
                    "dependencies",
                    "local",
                    "local",
                    Some("2"),
                    Some("path crates/core/Cargo.toml")
                ),
                (
                    "dependencies",
                    "pinned",
                    "real",
                    None,
                    Some("git example.com/org/repo")
                ),
                ("dependencies", "serde", "serde", Some("1"), None),
            ]
        );
    }

    #[test]
    fn a_virtual_manifest_provides_nothing_and_a_hostile_one_reads_as_empty() {
        let root = read_at(
            "Cargo.toml",
            "[workspace]\nmembers = [\"a\"]\n[workspace.dependencies]\nx = \"1\"\n",
        );
        assert_eq!(
            (root.name, root.main, root.workspace.len()),
            (None, None, 1)
        );
        let bad = read_at(
            "Cargo.toml",
            "[package]\nname = \"a:b\"\n[dependencies]\n\"a b\" = \"1\"\n",
        );
        assert_eq!(bad, Summary::default());
        assert_eq!(read("Cargo.toml", &[0xff, 0xfe]), Summary::default());
        assert_eq!(
            read("Cargo.toml", &vec![b'#'; MAX_BYTES + 1]),
            Summary::default()
        );
    }

    #[test]
    fn workspace_entries_fill_from_the_nearest_manifest_above() {
        let at = |path: &str, toml: &str| Manifest {
            path: path.to_string(),
            summary: read_at(path, toml),
        };
        let mut manifests = vec![
            at(
                "Cargo.toml",
                "[workspace]\n[workspace.dependencies]\nshared = \"1\"\nfork = { package = \"real\", path = \"vendor/real\" }\n",
            ),
            at(
                "app/Cargo.toml",
                "[package]\nname = \"app\"\n[dependencies]\nshared = { workspace = true }\nfork = { workspace = true }\n\
                 [dev-dependencies]\nlost = { workspace = true }\n",
            ),
            at(
                "other/Cargo.toml",
                "[package]\nname = \"o\"\n[dependencies]\nshared = { workspace = true, optional = true }\n",
            ),
        ];
        inherit(&mut manifests);
        assert_eq!(
            keys(&manifests[1].summary),
            [
                (
                    "dependencies",
                    "fork",
                    "real",
                    None,
                    Some("path vendor/real/Cargo.toml")
                ),
                ("dependencies", "shared", "shared", Some("1"), None),
            ]
        );
        // The nearest root is the workspace's, wherever it sits above.
        assert_eq!(keys(&manifests[2].summary).len(), 1);
    }

    #[test]
    fn a_use_names_a_crate_with_underscores_and_never_the_standard_library() {
        assert!(names_crate("shared_core"));
        assert!(!names_crate("std"));
        assert!(!names_crate("shared-core"));
        assert!(!names_crate(""));
    }
}
