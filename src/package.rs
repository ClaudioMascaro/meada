//! Packages (spec #22): every `package.json` of a workspace repository with a valid `name`
//! provides `package::npm::<name>`, and every dependency entry that names a package of the
//! workspace consumes it. `refresh` reads each manifest into a `Summary`, derived from its bytes
//! alone and stored in the index; `Packages::declare` turns the named ones into providers and
//! `consume` (a workspace-wide step, since one repository cannot see the others' providers)
//! turns the entries that name one into consumers. Meada reads no lockfile and queries no
//! registry: a dependency on anything outside the workspace adds nothing.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::channel::Routes;
use crate::contract::{ContractKind, Found, Level, Link};
use crate::index::{self, Consumer, Export, Provider};
use crate::resolve::Code;
use crate::workspace::Repository;

pub struct Packages;

/// The start of every package contract identifier.
pub const PREFIX: &str = "package::";

/// The ecosystem segment of the identifier: `package::npm::<name>`.
const ECOSYSTEM: &str = "package::npm::";

/// The longest package name npm accepts; a longer one makes no provider.
// ponytail: npm's own limit; a name past it is not a package.
const MAX_NAME: usize = 214;

/// The largest `package.json` read; a larger one counts as empty. Real manifests are a few
/// kilobytes, and the stored summary and the export stay proportional to what is read.
// ponytail: a flat 1 MiB; a streaming reader that keeps only the dependency tables lifts it.
const MAX_BYTES: usize = 1 << 20;

/// The dependency fields of a manifest, in the order the export lists them within a manifest.
const FIELDS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// The prefix of a range that keeps a dependency inside its own repository.
const WORKSPACE: &str = "workspace:";

/// What Meada read from one `package.json`. Nothing else of the file is kept.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Summary {
    /// A valid npm name; `None` when absent, not a string or not valid, which makes no provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Sorted by field, then name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Entry>,
}

/// One dependency entry of a manifest.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Entry {
    pub field: String,
    pub name: String,
    /// The declared range; `None` when absent, empty or not a string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
}

/// A `package.json` of the index: its repository-relative path and what was read of it.
pub struct Manifest {
    pub path: String,
    pub summary: Summary,
}

/// A package a repository provides (`meada export`).
#[derive(Serialize)]
pub struct Package {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub repository: String,
    /// The directory of the manifest, relative to the repository root: `.` for the root.
    pub directory: String,
}

/// A dependency entry that names a package of the workspace (`meada export`).
#[derive(Serialize)]
pub struct Dependency {
    /// The path of the manifest holding the entry.
    pub manifest: String,
    pub field: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
}

/// Reads a `package.json` from its bytes. Hostile input: a file too large, not JSON, nested
/// past `serde_json`'s depth limit or not an object reads as empty; a field of another type
/// counts as absent; a name that breaks npm's rules makes no provider.
pub fn read(src: &[u8]) -> Summary {
    if src.len() > MAX_BYTES {
        return Summary::default();
    }
    let Ok(Value::Object(root)) = serde_json::from_slice(src) else {
        return Summary::default();
    };
    let text = |v: Option<&Value>| match v {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    let mut dependencies = Vec::new();
    for field in FIELDS {
        let Some(Value::Object(table)) = root.get(field) else {
            continue;
        };
        for (name, range) in table {
            if valid_name(name) {
                dependencies.push(Entry {
                    field: field.to_string(),
                    name: name.clone(),
                    range: text(Some(range)),
                });
            }
        }
    }
    dependencies.sort_by(|a, b| (&a.field, &a.name).cmp(&(&b.field, &b.name)));
    Summary {
        name: text(root.get("name")).filter(|n| valid_name(n)),
        version: text(root.get("version")),
        dependencies,
    }
}

/// Whether `name` can be a package name: not empty, at most `MAX_NAME` characters, and no
/// control character, whitespace or `:` (which would break the `::` of a contract identifier).
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == ':')
}

/// The contract a package name provides.
fn contract(name: &str) -> String {
    format!("{ECOSYSTEM}{name}")
}

/// The site of the manifest at `path` of `repository`.
fn site(repository: &str, path: &str) -> String {
    format!("{repository}::{path}")
}

/// The directory of the manifest at `path`: `.` for one at the root.
fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

/// The workspace step that follows every repository's `declare`: for each dependency entry
/// that names a package some repository of the workspace provides, a consumer and a
/// `dependencies` row. An entry whose package is outside the workspace, or a `workspace:` one
/// whose package is outside its own repository, adds nothing.
pub fn consume(exports: &mut [Export]) {
    let mut providers: HashMap<String, Vec<String>> = HashMap::new();
    for export in exports.iter() {
        for p in export
            .providers
            .iter()
            .filter(|p| p.contract.starts_with(PREFIX))
        {
            providers
                .entry(p.contract.clone())
                .or_default()
                .push(export.repository.clone());
        }
    }
    for export in exports.iter_mut() {
        let mut consumers = Vec::new();
        let mut dependencies = Vec::new();
        for manifest in &export.manifests {
            for e in &manifest.summary.dependencies {
                let contract = contract(&e.name);
                let local = e.range.as_deref().is_some_and(|r| r.starts_with(WORKSPACE));
                let fits = |repo: &&String| !local || **repo == export.repository;
                if !providers
                    .get(&contract)
                    .is_some_and(|r| r.iter().any(|r| fits(&r)))
                {
                    continue;
                }
                consumers.push(Consumer {
                    contract: Some(contract),
                    site: format!(
                        "{}::{}::{}::{}",
                        export.repository, manifest.path, e.field, e.name
                    ),
                    via: None,
                    shape: None,
                    method: e.range.clone(),
                    fragments: Vec::new(),
                });
                dependencies.push(Dependency {
                    manifest: manifest.path.clone(),
                    field: e.field.clone(),
                    name: e.name.clone(),
                    range: e.range.clone(),
                });
            }
        }
        export.consumers.extend(consumers);
        index::sort_consumers(&mut export.consumers);
        dependencies.sort_by(|a, b| {
            (&a.manifest, &a.field, &a.name).cmp(&(&b.manifest, &b.field, &b.name))
        });
        export.dependencies = dependencies;
    }
}

impl ContractKind for Packages {
    fn prefix(&self) -> &'static str {
        "package"
    }

    fn sides(&self) -> Option<(&'static str, &'static str)> {
        Some(("package", "dependent"))
    }

    /// No code names a package: the manifests are read apart from the packs.
    fn extract(&self, _code: &Code) -> Vec<Found> {
        Vec::new()
    }

    /// One provider and one `packages` row per manifest with a valid name.
    fn declare(&self, repo: &Repository, export: &mut Export) {
        let mut packages = Vec::new();
        for m in &export.manifests {
            let Some(name) = &m.summary.name else {
                continue;
            };
            export.providers.push(Provider {
                contract: contract(name),
                site: site(&repo.identity, &m.path),
                version: m.summary.version.clone(),
                ..Provider::default()
            });
            packages.push(Package {
                name: name.clone(),
                version: m.summary.version.clone(),
                repository: repo.identity.clone(),
                directory: directory(&m.path).to_string(),
            });
        }
        packages.sort_by(|a, b| (&a.name, &a.directory).cmp(&(&b.name, &b.directory)));
        export.packages = packages;
        index::sort_providers(&mut export.providers);
    }

    /// The dependency entry links to the providers of its package: one candidate is `exact`,
    /// several are each `ambiguous`. A `workspace:` range keeps the providers of its own
    /// repository. The reason is `<identifier> <range> (local <version>)`, `none` for a
    /// missing one.
    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
        _routes: &Routes,
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        let range = consumer.method.as_deref();
        let local = range.is_some_and(|r| r.starts_with(WORKSPACE));
        let fits: Vec<_> = (providers.iter())
            .filter(|(repo, p)| {
                p.contract == *contract && (!local || repo.identity == from.identity)
            })
            .collect();
        let (level, n) = match fits.len() {
            0 => return Vec::new(),
            1 => (Level::Exact, None),
            n => (Level::Ambiguous, Some(n)),
        };
        fits.into_iter()
            .map(|(_, p)| {
                let said = format!(
                    "{contract} {} (local {})",
                    range.unwrap_or("none"),
                    p.version.as_deref().unwrap_or("none")
                );
                Link {
                    consumer: consumer.site.clone(),
                    provider: p.site.clone(),
                    contract: contract.clone(),
                    level,
                    reason: match n {
                        Some(n) => format!("{n} providers fit; {said}"),
                        None => said,
                    },
                    by: None,
                    p: None,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(s: &Summary) -> Vec<(&str, &str, Option<&str>)> {
        (s.dependencies.iter())
            .map(|e| (e.field.as_str(), e.name.as_str(), e.range.as_deref()))
            .collect()
    }

    #[test]
    fn a_manifest_reads_its_name_version_and_dependency_entries() {
        let src = br#"{"name": "@a/b", "version": "1.0.0",
            "devDependencies": {"z": "^1"}, "dependencies": {"y": "~2", "x": "*"}}"#;
        let s = read(src);
        assert_eq!(s.name.as_deref(), Some("@a/b"));
        assert_eq!(s.version.as_deref(), Some("1.0.0"));
        assert_eq!(
            entries(&s),
            [
                ("dependencies", "x", Some("*")),
                ("dependencies", "y", Some("~2")),
                ("devDependencies", "z", Some("^1")),
            ]
        );
    }

    #[test]
    fn a_field_of_another_type_counts_as_absent() {
        let s = read(
            br#"{"name": 3, "version": ["1"], "dependencies": {"a": true, "b": null, "c": ""},
                "devDependencies": ["a"], "peerDependencies": "a"}"#,
        );
        assert_eq!((&s.name, &s.version), (&None, &None));
        assert_eq!(
            entries(&s),
            [
                ("dependencies", "a", None),
                ("dependencies", "b", None),
                ("dependencies", "c", None),
            ]
        );
    }

    #[test]
    fn a_name_that_breaks_the_rules_makes_no_provider() {
        let long = "a".repeat(MAX_NAME + 1);
        for name in [long.as_str(), "", "a b", "a\nb", "a\u{7}b", "a::b", "a\tb"] {
            let src = format!(r#"{{"name": {}}}"#, serde_json::to_string(name).unwrap());
            assert_eq!(read(src.as_bytes()).name, None, "{name:?}");
        }
        let longest = "a".repeat(MAX_NAME);
        let src = format!(r#"{{"name": "{longest}"}}"#);
        assert_eq!(read(src.as_bytes()).name.as_deref(), Some(longest.as_str()));
    }

    #[test]
    fn a_dependency_with_an_invalid_name_is_left_out() {
        let s = read(br#"{"dependencies": {"a b": "1", "ok": "1"}}"#);
        assert_eq!(entries(&s), [("dependencies", "ok", Some("1"))]);
    }

    #[test]
    fn a_file_too_large_or_not_an_object_reads_as_empty() {
        let pad = " ".repeat(MAX_BYTES);
        let big = format!(r#"{{"name": "a"{pad}}}"#);
        assert_eq!(read(big.as_bytes()), Summary::default());
        for src in ["", "not json", "[1]", "\"a\"", "null"] {
            assert_eq!(read(src.as_bytes()), Summary::default(), "{src}");
        }
        let at_limit = format!(r#"{{"name": "a"{}}}"#, " ".repeat(MAX_BYTES - 13));
        assert_eq!(at_limit.len(), MAX_BYTES);
        assert_eq!(read(at_limit.as_bytes()).name.as_deref(), Some("a"));
    }

    #[test]
    fn deeply_nested_input_reads_as_empty() {
        let n = 10_000;
        let src = format!(
            r#"{{"name": "a", "x": {}{}}}"#,
            "[".repeat(n),
            "]".repeat(n)
        );
        assert_eq!(read(src.as_bytes()), Summary::default());
    }

    #[test]
    fn a_directory_is_the_manifest_s_parent_and_dot_at_the_root() {
        assert_eq!(directory("package.json"), ".");
        assert_eq!(directory("packages/a/package.json"), "packages/a");
    }
}
