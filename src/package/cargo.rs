use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::{
    Ecosystem, Entry, How, MAX_BYTES, MAX_NAME, Manifest, Provided, Providers, Resolved, Summary,
    bare_directory, directory, owner, sourced,
};
use crate::index::Export;
use crate::lang::rust::crates::{crate_name, entries, lib_name, lib_root, workspace_deps};
use crate::repo::{normalize_remote, relative};

const WORKSPACE: &str = "workspace.dependencies";

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
        ..Entry::default()
    })
}

fn declared<'a>(spec: &'a toml::Value, name: &str) -> Option<Option<&'a str>> {
    match spec.get(name) {
        None => Some(None),
        Some(toml::Value::String(s)) if !s.is_empty() => Some(Some(s)),
        Some(_) => None,
    }
}

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

fn is_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn names_crate(specifier: &str) -> bool {
    !matches!(specifier, "std" | "core" | "alloc")
        && is_crate_name(specifier)
        && !specifier.contains('-')
}

pub fn key_head(entry: &Entry) -> String {
    crate_name(entry.key())
}

pub fn lib_head(s: &Summary) -> Option<String> {
    s.lib_name
        .clone()
        .or_else(|| s.name.as_deref().map(crate_name))
}

pub struct Cargo;

impl Ecosystem for Cargo {
    fn name(&self) -> &'static str {
        "cargo"
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name == "Cargo.toml"
    }

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
        let version = package.and_then(|p| p.get("version"));
        let inherits_version = version.and_then(|v| v.get("workspace")?.as_bool()) == Some(true);
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

    fn reaches(
        &self,
        (via, _): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        to: ((&str, &Path), &str),
    ) -> bool {
        sourced(via, from, to)
    }

    fn owns(&self, manifest: &Manifest) -> bool {
        manifest.summary.name.is_some()
    }

    fn ways(&self, manifest: &Manifest, subpath: &str) -> Vec<(How, Vec<String>)> {
        match (subpath, &manifest.summary.main) {
            (".", Some(root)) => vec![(How::CrateName, vec![root.clone()])],
            _ => Vec::new(),
        }
    }

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
        let id = at.reach(self, repository, &file, chain, known)?;
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

    fn workspace() -> Export {
        let file = |path: &str| {
            let mut file = crate::index::File::default();
            file.path = path.to_string();
            file
        };
        let row =
            |path: &str, name: &str, file: &str, target: Option<&str>| crate::resolve::Exported {
                path: path.to_string(),
                name: Some(name.to_string()),
                file: Some(file.to_string()),
                target: target.map(String::from),
                source: None,
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
        assert_eq!(named("corelib"), entry("core-lib"));
        assert!(named("core_lib").is_empty());
        assert_eq!(named("fancy"), entry("other-lib"));
        assert!(named("other_lib").is_empty());
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
        assert_eq!(resolve("corelib", "m.g"), exact("r::core/src/m.rs::g"));
        assert_eq!(resolve("corelib", "missing"), None);
        assert_eq!(resolve("core_lib", "f"), None);
    }
}
