//! Cargo packages (spec #30): a `Cargo.toml` with a `[package]` name provides
//! `package::cargo::<name>`, and each dependency entry consumes the crate it names. The entries
//! come from the Rust pack's walker (`crates::entries`), so a manifest is read one way only.
//! The module answers the trait's questions as follows:
//!
//! - `owns`: only a manifest with a package; a virtual workspace root owns nothing.
//! - `ways`: the crate is its library's root file, named by the crate (`How::CrateName`).
//! - `entries`: the `use` head names the entry of the owning manifest whose key it is when the
//!   entry renames the package, else whose provider's library head it is.
//! - `reaches`: the entry's source (`sourced`), see below.
//! - `resolve`: the entry file only, no candidates; an entry that does not rename its crate
//!   must be written as the provider's library head.
//!
//! An entry keeps where its crate comes from in `Entry::via`: `path <manifest>` (the
//! `Cargo.toml` the path names, folded lexically from the manifest's directory: a path of the
//! repository, or one that starts with `../` when it leaves it, which `sourced` matches
//! against the workspace repositories' roots), `git <identity>` (the remote, as
//! `repo::normalize_remote` spells it) or nothing for a version. A declared source that cannot
//! be read (an empty or mistyped `path`, `git` or `package`, an absolute path, or both `path`
//! and `git`) drops the entry, as does any entry that gives no valid name. Cargo ranges are
//! never checked, and neither `Cargo.lock` nor a registry is read.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::{
    Ecosystem, Entry, How, MAX_BYTES, MAX_NAME, Manifest, Provided, Providers, Resolved, Summary,
    bare_directory, directory, owner, sourced,
};
use crate::index::Export;
use crate::lang::rust::crates::{crate_name, entries, lib_name, lib_root, workspace_deps};
use crate::repo::{normalize_remote, relative};

/// The field a `[workspace.dependencies]` entry is kept under, in `Summary::workspace`.
const WORKSPACE: &str = "workspace.dependencies";

/// The dependency `key = spec` of the table `field`, for a manifest in `dir` (empty at the
/// root); `None` when it is neither a version nor a table, its crate has no valid name or its
/// source cannot be read (see `source`).
fn entry(dir: &str, field: &str, key: &str, spec: &toml::Value) -> Option<Entry> {
    if spec.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
        return is_crate_name(key).then(|| Entry {
            field: field.to_string(),
            name: key.to_string(),
            inherits: true,
            ..Entry::default()
        });
    }
    if !spec.is_str() && !spec.is_table() {
        return None;
    }
    let name = declared(spec, "package")?.unwrap_or(key);
    if !is_crate_name(name) || !is_crate_name(key) {
        return None;
    }
    let via = source(dir, spec)?;
    let range = match spec {
        toml::Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => declared(spec, "version").flatten().map(String::from),
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

/// The string field `name` of a dependency table: `Some(None)` when absent, `None` when it is
/// there but not a non-empty string.
fn declared<'a>(spec: &'a toml::Value, name: &str) -> Option<Option<&'a str>> {
    match spec.get(name) {
        None => Some(None),
        Some(toml::Value::String(s)) if !s.is_empty() => Some(Some(s)),
        Some(_) => None,
    }
}

/// Where a dependency's crate comes from, as `Entry::via`: `Some(None)` for a version,
/// `None` when the source it declares cannot be read (see `declared`, `relative`), or when it
/// declares two.
fn source(dir: &str, spec: &toml::Value) -> Option<Option<String>> {
    Some(match (declared(spec, "path")?, declared(spec, "git")?) {
        (Some(path), None) => match relative(dir, path)?.as_str() {
            "" => Some("path Cargo.toml".to_string()),
            at => Some(format!("path {at}/Cargo.toml")),
        },
        (None, Some(url)) => Some(format!("git {}", normalize_remote(url))),
        (None, None) => None,
        (Some(_), Some(_)) => return None,
    })
}

/// Whether `name` can be a package name: Cargo's alphabet (ASCII letters and digits, `-` and
/// `_`), not empty, at most `MAX_NAME` bytes.
fn is_crate_name(name: &str) -> bool {
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
        && is_crate_name(specifier)
        && !specifier.contains('-')
}

/// What a `use` writes for the dependency `entry` when it renames the package: its key with
/// hyphens turned into underscores. An entry that does not rename it writes the library's own
/// name (`lib_head`) instead.
pub fn key_head(entry: &Entry) -> String {
    crate_name(entry.key())
}

/// What a `use` writes for the library of the package a manifest summary describes: `[lib] name`,
/// else the package name, hyphens as underscores.
pub fn lib_head(s: &Summary) -> Option<String> {
    s.lib_name
        .clone()
        .or_else(|| s.name.as_deref().map(crate_name))
}

/// The Cargo ecosystem.
pub struct Cargo;

impl Ecosystem for Cargo {
    fn name(&self) -> &'static str {
        "cargo"
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name == "Cargo.toml"
    }

    /// Reads a `Cargo.toml` from its bytes. Hostile input: a file too large, not UTF-8 or not TOML
    /// reads as empty; a field of another type counts as absent; a name Cargo would refuse makes no
    /// provider; a `workspace = true` entry is kept as such until `inherit` fills it.
    fn read(&self, path: &str, src: &[u8]) -> Summary {
        let table: Option<toml::Table> = (src.len() <= MAX_BYTES)
            .then(|| std::str::from_utf8(src).ok()?.parse().ok())
            .flatten();
        let Some(table) = table else {
            return Summary::default();
        };
        let dir = bare_directory(path);
        let text = |v: Option<&toml::Value>| match v {
            Some(toml::Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let package = table.get("package");
        let name = text(package.and_then(|p| p.get("name"))).filter(|n| is_crate_name(n));
        let mut seen = HashSet::new();
        let mut dependencies: Vec<Entry> = Vec::new();
        for (field, key, spec) in entries(&table) {
            let Some(entry) = entry(dir, field, key, spec) else {
                continue;
            };
            if seen.insert((entry.field.clone(), entry.key().to_string())) {
                dependencies.push(entry);
            }
        }
        dependencies.sort_by(|a, b| (&a.field, a.key()).cmp(&(&b.field, b.key())));
        let mut workspace: Vec<Entry> = Vec::new();
        for (key, spec) in workspace_deps(&table).into_iter().flatten() {
            workspace.extend(entry(dir, WORKSPACE, key, spec));
        }
        workspace.sort_by(|a, b| a.key().cmp(b.key()));
        // `version.workspace = true` takes the version from `[workspace.package]` (see `inherit`).
        let version = package.and_then(|p| p.get("version"));
        let inherits_version = version.and_then(|v| v.get("workspace")?.as_bool()) == Some(true);
        // The library name only when it is not the one a path writes for the package anyway.
        let lib = (name.as_deref())
            .map(|n| (crate_name(n), lib_name(&table, n)))
            .filter(|(own, lib)| lib != own && is_crate_name(lib))
            .map(|(_, lib)| lib);
        Summary {
            version: text(version).filter(|_| name.is_some()),
            main: name.as_ref().and_then(|_| lib_root(&table, dir)),
            inherits_version: inherits_version && name.is_some(),
            workspace_version: text(
                (table.get("workspace")).and_then(|w| w.get("package")?.get("version")),
            ),
            lib_name: lib,
            name,
            dependencies,
            workspace,
            ..Summary::default()
        }
    }

    fn sources(&self) -> &'static [&'static str] {
        &["rs"]
    }

    /// Fills each `workspace = true` entry of a Cargo manifest from the `[workspace.dependencies]`
    /// of the nearest manifest above it (or itself) that has any, and `version.workspace = true`
    /// from that manifest's `[workspace.package] version`; an entry that finds no entry of its key
    /// is dropped, as Cargo refuses it. The member keeps its own table (`dev-dependencies`...) and
    /// key, so a workspace entry that renames a crate renames it for the member too.
    fn inherit(&self, manifests: &mut [Manifest]) {
        type Root = (String, HashMap<String, Entry>, Option<String>);
        let roots: Vec<Root> = (manifests.iter())
            .filter(|m| m.path.ends_with("Cargo.toml"))
            .filter(|m| !m.summary.workspace.is_empty() || m.summary.workspace_version.is_some())
            .map(|m| {
                let table = (m.summary.workspace.iter())
                    .map(|e| (e.key().to_string(), e.clone()))
                    .collect();
                let version = m.summary.workspace_version.clone();
                (directory(&m.path).to_string(), table, version)
            })
            .collect();
        for m in manifests
            .iter_mut()
            .filter(|m| m.path.ends_with("Cargo.toml"))
        {
            if !m.summary.inherits_version && !m.summary.dependencies.iter().any(|e| e.inherits) {
                continue;
            }
            let dir = directory(&m.path);
            let near = (roots.iter())
                .filter(|(root, ..)| {
                    root == "." || dir == root || dir.starts_with(&format!("{root}/"))
                })
                .max_by_key(|(root, ..)| if root == "." { 0 } else { root.len() + 1 });
            if std::mem::take(&mut m.summary.inherits_version) {
                m.summary.version = near.and_then(|(_, _, version)| version.clone());
            }
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

    fn names_package(&self, specifier: &str) -> bool {
        names_crate(specifier)
    }

    /// The specifier is the crate a `use` names: the entry of the owning manifest whose key it
    /// is when the entry renames the package, else whose provider's library head it is (the
    /// package's own name, with no provider to ask), hyphens as underscores.
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let heads = |e: &Entry| match (e.alias.is_some(), providers.of(self.name(), &e.name)) {
            (false, of) if !of.is_empty() => (of.iter()).any(|&(m, i)| {
                lib_head(&exports[m].manifests[i].summary).as_deref() == Some(specifier)
            }),
            _ => key_head(e) == specifier,
        };
        let entry = (owner(&exports[n].manifests, from, self))
            .filter(|_| names_crate(specifier))
            .and_then(|m| m.summary.dependencies.iter().find(|e| heads(e)));
        entry.map(|e| (e, ".".to_string())).into_iter().collect()
    }

    /// `path` by the folded location, `git` by the remote's identity (`sourced`).
    fn reaches(
        &self,
        (via, _): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        to: ((&str, &Path), &str),
    ) -> bool {
        sourced(via, from, to)
    }

    /// Only a manifest with a package: a virtual workspace root has none.
    fn owns(&self, manifest: &Manifest) -> bool {
        manifest.summary.name.is_some()
    }

    /// The crate is its library's root file, a path of the repository as `read` folded it; a
    /// subpath is a module path, followed by name.
    fn ways(&self, manifest: &Manifest, subpath: &str) -> Vec<(How, Vec<String>)> {
        match (subpath, &manifest.summary.main) {
            (".", Some(root)) => vec![(How::CrateName, vec![root.clone()])],
            _ => Vec::new(),
        }
    }

    /// The entry file only, no candidates; an entry that does not rename its crate reaches the
    /// libraries named as it writes.
    fn resolve(
        &self,
        at: &Provided,
        repository: &str,
        manifest: &Manifest,
        (entry, subpath): (&Entry, &str),
        (specifier, chain): (&str, &str),
        known: &dyn Fn(&str) -> bool,
    ) -> Option<Resolved> {
        if entry.alias.is_none() && lib_head(&manifest.summary).as_deref() != Some(specifier) {
            return None;
        }
        let (file, how, level) = at.entry(manifest, &self.ways(manifest, subpath))?;
        let id = at.reach(repository, &file, chain, known)?;
        Some(Resolved {
            how,
            level,
            targets: vec![id],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Level;

    /// A Cargo path keeps the crate at that path of the dependent's repository, a source from a
    /// remote the repository of that remote, a version every repository.
    #[test]
    fn a_path_or_git_source_reaches_the_provider_it_names() {
        let at = |id| {
            let root = match id {
                "a" => "/w/a",
                "b" => "/w/b",
                _ => "/w/x",
            };
            (id, Path::new(root))
        };
        let path = (Some("path core/Cargo.toml"), None);
        assert!(Cargo.reaches(path, at("a"), (at("a"), "core/Cargo.toml")));
        assert!(!Cargo.reaches(path, at("a"), (at("a"), "other/Cargo.toml")));
        assert!(!Cargo.reaches(path, at("a"), (at("b"), "core/Cargo.toml")));
        let remote = (Some("git example.com/org/lib"), Some("1"));
        let lib = ("example.com/org/lib", Path::new("/w/lib"));
        assert!(Cargo.reaches(remote, at("a"), (lib, "Cargo.toml")));
        assert!(!Cargo.reaches(remote, at("a"), (at("b"), "Cargo.toml")));
        assert!(Cargo.reaches((None, Some("1")), at("a"), (at("b"), "Cargo.toml")));
        // A path out of the repository lands where the two roots say, folded without reading.
        let out = (Some("path ../b/core/Cargo.toml"), None);
        assert!(Cargo.reaches(out, at("a"), (at("b"), "core/Cargo.toml")));
        assert!(!Cargo.reaches(out, at("a"), (at("b"), "Cargo.toml")));
        assert!(!Cargo.reaches(out, at("a"), (at("x"), "core/Cargo.toml")));
        let back = (Some("path ../a/core/Cargo.toml"), None);
        assert!(Cargo.reaches(back, at("a"), (at("a"), "core/Cargo.toml")));
    }

    fn read_at(path: &str, toml: &str) -> Summary {
        Cargo.read(path, toml.as_bytes())
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
        assert_eq!(s.main.as_deref(), Some("crates/api/src/x.rs"));
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
                // A path out of the repository keeps its leading `..`.
                (
                    "dev-dependencies",
                    "far",
                    "far",
                    None,
                    Some("path ../x/Cargo.toml")
                ),
            ]
        );
    }

    #[test]
    fn a_source_that_cannot_be_read_drops_the_entry() {
        let s = read_at(
            "Cargo.toml",
            "[package]\nname = \"a\"\n[dependencies]\nempty = { path = \"\" }\n\
             typed = { path = 3 }\nblank = { git = \"\" }\nboth = { path = \"x\", git = \"u\" }\n\
             abs = { path = \"/etc\" }\nrenamed = { package = 1 }\nnum = 1\nok = \"1\"\n",
        );
        assert_eq!(keys(&s), [("dependencies", "ok", "ok", Some("1"), None)]);
    }

    #[test]
    fn many_entries_of_one_key_keep_the_first() {
        let mut toml = String::from("[package]\nname = \"a\"\n[dependencies]\nx = \"1\"\n");
        for n in 0..2000 {
            toml += &format!("[target.'cfg(t{n})'.dependencies]\nx = \"2\"\n");
        }
        let s = read_at("Cargo.toml", &toml);
        assert_eq!(keys(&s), [("dependencies", "x", "x", Some("1"), None)]);
    }

    #[test]
    fn a_library_name_and_an_inherited_version_are_read_and_filled() {
        let at = |path: &str, toml: &str| Manifest {
            path: path.to_string(),
            summary: read_at(path, toml),
        };
        let lib = read_at(
            "Cargo.toml",
            "[package]\nname = \"a-b\"\n[lib]\nname = \"a_b\"\npath = \"./src/./l.rs\"\n",
        );
        // The package's own library name is not stored; the path is folded.
        assert_eq!(
            (lib.lib_name, lib.main.as_deref()),
            (None, Some("src/l.rs"))
        );
        let s = read_at(
            "Cargo.toml",
            "[package]\nname = \"a\"\n[lib]\nname = \"core_a\"\n",
        );
        assert_eq!(lib_head(&s).as_deref(), Some("core_a"));
        let mut manifests = vec![
            at(
                "Cargo.toml",
                "[workspace]\nmembers = [\"m\"]\n[workspace.package]\nversion = \"3.1.0\"\n",
            ),
            at(
                "m/Cargo.toml",
                "[package]\nname = \"m\"\nversion.workspace = true\n",
            ),
            at(
                "n/Cargo.toml",
                "[package]\nname = \"n\"\nversion = \"1.0.0\"\n",
            ),
        ];
        Cargo.inherit(&mut manifests);
        let versions: Vec<_> = (manifests.iter())
            .map(|m| m.summary.version.as_deref())
            .collect();
        assert_eq!(versions, [None, Some("3.1.0"), Some("1.0.0")]);
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
        assert_eq!(Cargo.read("Cargo.toml", &[0xff, 0xfe]), Summary::default());
        assert_eq!(
            Cargo.read("Cargo.toml", &vec![b'#'; MAX_BYTES + 1]),
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
        Cargo.inherit(&mut manifests);
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

    fn manifest(path: &str, toml: &str) -> Manifest {
        Manifest {
            path: path.to_string(),
            summary: read_at(path, toml),
        }
    }

    /// One repository: `app` depends on `core-lib` (its library is `corelib`, in the workspace)
    /// and on `fancy`, which renames the crate `other-lib` that no repository provides.
    fn workspace() -> Export {
        let file = |path: &str| crate::index::File {
            path: path.to_string(),
            hash: String::new(),
        };
        let row =
            |path: &str, name: &str, file: &str, target: Option<&str>| crate::resolve::Exported {
                path: path.to_string(),
                name: Some(name.to_string()),
                file: Some(file.to_string()),
                target: target.map(String::from),
            };
        Export {
            repository: "r".to_string(),
            files: vec![file("core/src/lib.rs"), file("core/src/m.rs")],
            manifests: vec![
                manifest(
                    "app/Cargo.toml",
                    "[package]\nname = \"app\"\n[dependencies]\n\
                     core-lib = { path = \"../core\" }\nfancy = { package = \"other-lib\", version = \"1\" }\n",
                ),
                manifest(
                    "core/Cargo.toml",
                    "[package]\nname = \"core-lib\"\n[lib]\nname = \"corelib\"\n",
                ),
                manifest("Cargo.toml", "[workspace]\nmembers = [\"app\", \"core\"]\n"),
            ],
            exported: vec![
                row("core/src/lib.rs", "f", "core/src/lib.rs", Some("f")),
                row("core/src/lib.rs", "m", "core/src/m.rs", None),
                row("core/src/m.rs", "g", "core/src/m.rs", Some("g")),
            ],
            ..Export::default()
        }
    }

    #[test]
    fn only_a_manifest_with_a_package_owns() {
        let package = manifest("Cargo.toml", "[package]\nname = \"a\"\n");
        let virtual_root = manifest("Cargo.toml", "[workspace]\nmembers = [\"a\"]\n");
        assert!(Cargo.owns(&package));
        assert!(!Cargo.owns(&virtual_root));
    }

    #[test]
    fn the_way_into_a_crate_is_its_library_root_and_no_other() {
        let m = manifest("crates/a/Cargo.toml", "[package]\nname = \"a\"\n");
        assert_eq!(
            Cargo.ways(&m, "."),
            [(How::CrateName, vec!["crates/a/src/lib.rs".to_string()])]
        );
        assert!(Cargo.ways(&m, "./m").is_empty());
        let custom = manifest(
            "Cargo.toml",
            "[package]\nname = \"a\"\n[lib]\npath = \"src/x.rs\"\n",
        );
        assert_eq!(
            Cargo.ways(&custom, "."),
            [(How::CrateName, vec!["src/x.rs".to_string()])]
        );
        // A virtual manifest has no library root.
        assert!(
            Cargo
                .ways(&manifest("Cargo.toml", "[workspace]\n"), ".")
                .is_empty()
        );
    }

    #[test]
    fn a_use_head_is_a_renamed_key_or_a_library_head() {
        let exports = [workspace()];
        let providers = Providers::new(&exports);
        let named = |specifier: &str| -> Vec<(String, String)> {
            (Cargo.entries(&exports, &providers, (0, "app/src/main.rs"), specifier))
                .into_iter()
                .map(|(e, subpath)| (e.name.clone(), subpath))
                .collect()
        };
        let entry = |name: &str| vec![(name.to_string(), ".".to_string())];
        // An entry that does not rename its crate is written as the provider's library head.
        assert_eq!(named("corelib"), entry("core-lib"));
        assert!(named("core_lib").is_empty());
        // One that renames it is written as its key, hyphens as underscores.
        assert_eq!(named("fancy"), entry("other-lib"));
        assert!(named("other_lib").is_empty());
        // Never the standard library, and only for a file a manifest with a package owns.
        assert!(named("std").is_empty());
        let providers_of = Cargo.entries(&exports, &providers, (0, "elsewhere/x.rs"), "corelib");
        assert!(providers_of.is_empty());
    }

    #[test]
    fn a_crate_resolves_at_its_library_root_by_the_name_a_use_writes() {
        let exports = [workspace()];
        let at = Provided::new(&exports[0]);
        let providers = Providers::new(&exports);
        let entries = Cargo.entries(&exports, &providers, (0, "app/src/main.rs"), "corelib");
        let (entry, subpath) = entries[0].clone();
        let manifest = &exports[0].manifests[1];
        let resolve = |specifier: &str, chain: &str| {
            let known = |_: &str| true;
            let import = ((entry, subpath.as_str()), (specifier, chain));
            Cargo
                .resolve(&at, "r", manifest, import.0, import.1, &known)
                .map(|r| (r.how, r.level, r.targets))
        };
        let exact = |id: &str| Some((How::CrateName, Level::Exact, vec![id.to_string()]));
        assert_eq!(resolve("corelib", "f"), exact("r::core/src/lib.rs::f"));
        // A module the library root exports.
        assert_eq!(resolve("corelib", "m.g"), exact("r::core/src/m.rs::g"));
        assert_eq!(resolve("corelib", "missing"), None);
        // The package's own name is not what a `use` writes for a library named otherwise.
        assert_eq!(resolve("core_lib", "f"), None);
    }
}
