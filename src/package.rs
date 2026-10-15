//! Packages (spec #22): every `package.json` of a workspace repository with a valid `name`
//! provides `package::npm::<name>`, and every dependency entry that names a package of the
//! workspace consumes it. `refresh` reads each manifest into a `Summary`, derived from its bytes
//! alone and stored in the index; `Packages::declare` turns the named ones into providers and
//! `consume` (a workspace-wide step, since one repository cannot see the others' providers)
//! turns the entries that name one into consumers. Meada reads no lockfile and queries no
//! registry: a dependency on anything outside the workspace adds nothing.
//!
//! Imports cross too: `extract` stores every call through a binding imported from a bare
//! specifier, and `import`, a workspace step after `consume`, resolves those whose package the
//! workspace provides into the provider's symbol, through its entry fields and what its files
//! export (`resolve::Exported`). Nothing of it is stored as a link: `link` derives the hop.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::channel::Routes;
use crate::contract::{ContractKind, Found, Level, Link, Role, Site};
use crate::index::{self, CallSite, Consumer, Export, Provider};
use crate::lang::ts::{EXTENSIONS, strip_jsonc, with_extensions};
use crate::resolve::{Code, Exported};
use crate::workspace::Repository;

pub struct Packages;

/// The start of every package contract identifier.
pub const PREFIX: &str = "package::";

/// The ecosystem segment of the identifier: `package::npm::<name>`.
const ECOSYSTEM: &str = "package::npm::";

/// The longest package name npm accepts, in bytes; a longer one makes no provider.
// ponytail: npm's own limit; a name past it is not a package.
const MAX_NAME: usize = 214;

/// The largest `package.json` read; a larger one counts as empty. Real manifests are a few
/// kilobytes, and the stored summary and the export stay proportional to what is read.
// ponytail: a flat 1 MiB; a streaming reader that keeps only the dependency tables lifts it.
const MAX_BYTES: usize = 1 << 20;

/// The dependency fields of a manifest that `read` collects.
const FIELDS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// The prefix of a range that keeps a dependency inside its own repository.
const WORKSPACE: &str = "workspace:";

/// Whether a dependency `range` may reach a provider: a `workspace:` range keeps only the
/// dependent's own repository (`own`), any other reaches every repository of the workspace.
fn reaches(range: Option<&str>, own: bool) -> bool {
    own || !range.is_some_and(|r| r.starts_with(WORKSPACE))
}

/// The conditions of an `exports` target Meada reads, in order: at each level the first one
/// present is taken, and no other is followed.
const CONDITIONS: [&str; 4] = ["types", "import", "default", "require"];

/// The longest chain of `export *` a lookup in a provider follows; a name further down is not
/// found.
// ponytail: real packages re-export a few levels deep; this bounds the walk over a hostile one.
const MAX_REEXPORTS: usize = 64;

/// The deepest an `exports` target nests conditions; deeper, the subpath has no target.
// ponytail: real maps nest two or three levels; this bounds the walk over a hostile one.
const MAX_CONDITIONS: usize = 32;

/// The target the `exports` field of a manifest gives `subpath` (`.` for the bare name, else
/// `./<rest>`), as written, `*` replaced: exact keys first, then the pattern with one `*` whose
/// prefix is the longest. A string, or an object without subpath keys, is the target of `.`. A
/// `null` target, an array of fallbacks, a key or a target with two `*`, keys that mix subpaths
/// and conditions, or none of `CONDITIONS` present give no target.
fn exported(exports: &Value, subpath: &str) -> Option<String> {
    let map = match exports {
        Value::Object(map) if map.keys().any(|k| k.starts_with('.')) => map,
        _ => return (subpath == ".").then(|| target(exports, None, 0))?,
    };
    if !map.keys().all(|k| k.starts_with('.')) {
        return None;
    }
    if let Some(value) = map.get(subpath) {
        return target(value, None, 0);
    }
    let (_, star, value) = (map.iter())
        .filter_map(|(key, value)| {
            let (prefix, suffix) = key.split_once('*')?;
            let star = subpath.strip_prefix(prefix)?.strip_suffix(suffix)?;
            (!suffix.contains('*') && !star.is_empty()).then_some((
                (prefix.len(), key.len()),
                star,
                value,
            ))
        })
        .max_by_key(|(rank, _, _)| *rank)?;
    target(value, Some(star), 0)
}

/// The path a target of `exports` names, through its conditions (see `exported`).
fn target(value: &Value, star: Option<&str>, depth: usize) -> Option<String> {
    match value {
        // One `*` at most, as in a key: the expansion stays the size of the subpath.
        Value::String(t) if t.matches('*').nth(1).is_none() => {
            Some(star.map_or_else(|| t.clone(), |s| t.replace('*', s)))
        }
        Value::Object(conditions) if depth < MAX_CONDITIONS => {
            let taken = CONDITIONS.iter().find_map(|c| conditions.get(*c))?;
            target(taken, star, depth + 1)
        }
        _ => None,
    }
}

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
    /// The `exports` field as written; `None` when absent or `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exports: Option<Value>,
    /// The `types`, `module` and `main` entry fields; a field that is not a string is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
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

impl Package {
    /// The site of its manifest, which a dependency entry's link names as its provider.
    pub fn site(&self) -> String {
        match self.directory.as_str() {
            "." => site(&self.repository, "package.json"),
            dir => site(&self.repository, &format!("{dir}/package.json")),
        }
    }
}

/// A call through a binding imported from a bare specifier, as the index stores it.
pub struct ImportCall {
    /// The call site.
    pub site: String,
    /// The file making the call.
    pub path: String,
    pub specifier: String,
    /// What the call names of the import: `greet`, `default`, `Client.create`.
    pub chain: String,
}

/// What resolved an import in a provider: the declared field or rule that named the entry, or
/// `OutDir` when the output-to-source mapping found the file. For an `ambiguous` import, the
/// field or rule whose entry could not be followed to a source file.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum How {
    Exports,
    Types,
    Module,
    Main,
    Path,
    OutDir,
}

impl How {
    pub fn name(self) -> &'static str {
        match self {
            How::Exports => "exports",
            How::Types => "types",
            How::Module => "module",
            How::Main => "main",
            How::Path => "path",
            How::OutDir => "outDir",
        }
    }
}

/// Where a call through a package import resolves in one provider of the package.
pub struct Hop {
    /// The provider's site: its manifest.
    pub manifest: String,
    /// The symbol reached.
    pub target: String,
    pub how: How,
    /// The import's level.
    pub level: Level,
}

/// An import resolved into a package of the workspace (`meada export`).
#[derive(Serialize)]
pub struct Import {
    /// The call through the imported binding.
    pub site: String,
    pub specifier: String,
    /// What the call names of the import.
    pub name: String,
    pub level: Level,
    pub how: How,
    pub target: String,
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

/// The two `compilerOptions` of a `tsconfig.json` that map build output back to source, as
/// written. `extends` is not followed.
// ponytail: the trunk follows no `extends` either (`lang/ts.rs`); a base config's `outDir`
// counts as absent. Lifted together with the trunk's `paths`.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Tsconfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_dir: Option<String>,
}

/// A `tsconfig.json` of the index: its repository-relative path and what was read of it.
pub struct Config {
    pub path: String,
    pub tsconfig: Tsconfig,
}

/// Reads a `tsconfig.json` (JSON with comments and trailing commas) from its bytes. Hostile
/// input, as for `read`: a file too large, not UTF-8, not JSON, nested past `serde_json`'s depth
/// limit or not an object is invalid, `None`, which counts as no config at all (the repository
/// root's is tried instead); an option that is not a string counts as absent in a valid one.
pub fn read_tsconfig(src: &[u8]) -> Option<Tsconfig> {
    let text = std::str::from_utf8(src)
        .ok()
        .filter(|_| src.len() <= MAX_BYTES);
    let Some(Ok(Value::Object(root))) = text.map(|t| serde_json::from_str(&strip_jsonc(t))) else {
        return None;
    };
    let option = |name: &str| match root.get("compilerOptions")?.get(name)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    Some(Tsconfig {
        out_dir: option("outDir"),
        root_dir: option("rootDir"),
    })
}

/// `outDir` and `rootDir` as repository-relative directories, for the config at `config`
/// (its options are relative to its directory) of a package at `package_dir`: `rootDir` defaults
/// to the package directory. `None` when there is no `outDir`, when either path is not relative
/// and inside the repository (absolute, or with an empty or `..` segment), or when `outDir` is
/// the config's own directory. A path read from a config stays inside the repository, and a
/// declared option that cannot be read voids the mapping instead of falling back.
fn roots(config: &str, t: &Tsconfig, package_dir: &str) -> Option<(String, String)> {
    let dir = directory(config);
    let resolve = |path: &str| {
        let path = path.strip_prefix("./").unwrap_or(path);
        let path = path.strip_suffix('/').unwrap_or(path);
        match path {
            "" | "." => Some(dir.to_string()),
            path => inside(dir, path),
        }
    };
    let out = resolve(t.out_dir.as_deref()?)?;
    let root = match &t.root_dir {
        Some(root) => resolve(root)?,
        None => package_dir.to_string(),
    };
    (out != dir).then_some((out, root))
}

/// Whether the manifest-table row at `path` is a `tsconfig.json` (else a `package.json`).
pub fn is_tsconfig(path: &str) -> bool {
    path.rsplit('/').next() == Some("tsconfig.json")
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
        exports: root.get("exports").filter(|v| !v.is_null()).cloned(),
        types: text(root.get("types")),
        module: text(root.get("module")),
        main: text(root.get("main")),
    }
}

/// Whether `name` can be a package name: not empty, at most `MAX_NAME` bytes, and no
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

/// The package a bare specifier names and the subpath inside it: `@a/b/c` is `@a/b` and
/// `./c`, `x` is `x` and `.`. `None` for a relative or absolute specifier, or a name npm does not
/// accept (`node:fs` among them).
fn split(specifier: &str) -> Option<(&str, String)> {
    if specifier.starts_with(['.', '/']) {
        return None;
    }
    let segments = if specifier.starts_with('@') { 2 } else { 1 };
    let end = (specifier.match_indices('/').nth(segments - 1)).map_or(specifier.len(), |(i, _)| i);
    let (name, rest) = specifier.split_at(end);
    (valid_name(name) && (segments == 1 || name.contains('/'))).then(|| (name, format!(".{rest}")))
}

/// The manifest of the deepest directory holding `path`.
fn nearest<'m>(manifests: &'m [Manifest], path: &str) -> Option<&'m Manifest> {
    (manifests.iter())
        .filter(|m| match directory(&m.path) {
            "." => true,
            dir => path
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/')),
        })
        .max_by_key(|m| m.path.len())
}

/// The ways an import of `subpath` of the package at `manifest` may be followed, in the order to
/// try them, each with the files it may name (most likely first) and what named them: `exports`
/// when the manifest has it, the only way in; else, for the bare name, one for each of `types`,
/// `module` and `main` present; else the subpath as a path inside the package, with the trunk's
/// extension and `index` rules (`path`), the bare name being `./index`. Every file stays inside
/// the package directory.
fn ways(manifest: &Manifest, subpath: &str) -> Vec<(How, Vec<String>)> {
    let dir = directory(&manifest.path);
    let s = &manifest.summary;
    let path = |subpath: &str| {
        let files = with_extensions(&inside(dir, subpath.strip_prefix("./")?)?);
        Some((How::Path, files))
    };
    if let Some(exports) = &s.exports {
        let followed = || {
            let target = exported(exports, subpath)?;
            Some((How::Exports, vec![inside(dir, target.strip_prefix("./")?)?]))
        };
        return followed().into_iter().collect();
    }
    if subpath != "." {
        return path(subpath).into_iter().collect();
    }
    let fields = [
        (How::Types, &s.types),
        (How::Module, &s.module),
        (How::Main, &s.main),
    ];
    if fields.iter().all(|(_, t)| t.is_none()) {
        // Node's default tries `index.<ext>` only, not `index` or `index/index.<ext>`.
        let index = EXTENSIONS
            .iter()
            .map(|ext| inside(dir, &format!("index.{ext}")));
        return index
            .collect::<Option<_>>()
            .map(|files| (How::Path, files))
            .into_iter()
            .collect();
    }
    (fields.into_iter())
        .filter_map(|(how, t)| {
            let t = t.as_deref()?;
            Some((how, vec![inside(dir, t.strip_prefix("./").unwrap_or(t))?]))
        })
        .collect()
}

/// `path`, relative to the package directory `dir`, as a path of the repository; `None` when
/// it is empty, absolute, or has an empty, `.` or `..` segment.
fn inside(dir: &str, path: &str) -> Option<String> {
    if (path.split('/')).any(|s| s.is_empty() || s == "." || s == "..") {
        return None;
    }
    Some(match dir {
        "." => path.to_string(),
        dir => format!("{dir}/{path}"),
    })
}

/// The output extensions `sources` strips, longest first.
const OUTPUT: [&str; 5] = [".d.ts", ".js", ".mjs", ".cjs", ".jsx"];

/// The source files a build-output `entry` may come from, in the order to try them: the same
/// path relative to `out_dir` under `root_dir`, its output extension (`.d.ts`, `.js`, `.mjs`,
/// `.cjs`, `.jsx`) replaced by each extension the TS/JS pack handles. All are repository-relative
/// (`root_dir` may be `.`). Nothing when `entry` is not inside `out_dir` or has no output
/// extension.
fn sources(entry: &str, out_dir: &str, root_dir: &str) -> Vec<String> {
    let Some(relative) = entry
        .strip_prefix(out_dir)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return Vec::new();
    };
    let Some(stem) = OUTPUT.iter().find_map(|ext| relative.strip_suffix(ext)) else {
        return Vec::new();
    };
    let base = match root_dir {
        "." => stem.to_string(),
        root => format!("{root}/{stem}"),
    };
    EXTENSIONS
        .iter()
        .map(|ext| format!("{base}.{ext}"))
        .collect()
}

/// What a provider answers imports with: its files and symbols, and what each file exports.
struct Provided<'e> {
    files: HashSet<&'e str>,
    symbols: HashSet<&'e str>,
    exported: HashMap<&'e str, Vec<&'e Exported>>,
    manifests: &'e [Manifest],
    configs: &'e [Config],
    /// The source files that export something, by the path of the manifest that is their
    /// nearest, none under a `node_modules` directory. Built the first time an ambiguous import
    /// asks (`candidates`), once for all.
    owned: OnceCell<HashMap<&'e str, Vec<&'e str>>>,
}

/// What an import of a package resolved to in one provider: what named the entry, the import's
/// level, and the symbols reached, more than one only when ambiguous.
struct Resolved {
    how: How,
    level: Level,
    targets: Vec<String>,
}

impl<'e> Provided<'e> {
    fn new(export: &'e Export) -> Self {
        let mut exported: HashMap<&str, Vec<&Exported>> = HashMap::new();
        for e in &export.exported {
            exported.entry(&e.path).or_default().push(e);
        }
        Self {
            files: export.files.iter().map(|f| f.path.as_str()).collect(),
            symbols: export.symbols.iter().map(|s| s.id.as_str()).collect(),
            exported,
            manifests: &export.manifests,
            configs: &export.tsconfigs,
            owned: OnceCell::new(),
        }
    }

    /// Whether `path` is a source file the provider indexes (a declaration file is none).
    fn source(&self, path: &str) -> bool {
        self.files.contains(path) && !path.ends_with(".d.ts")
    }

    /// The symbol an import of `subpath` of the package at `manifest` of `repository` names as
    /// `chain`. The entry must be a source file the provider indexes: found by the declared
    /// field or rule that names it, the import is `exact`; found through the package's `outDir`
    /// and `rootDir` (`sources`), `inferred`. Of the ways in (`ways`), the first that leads
    /// to a file wins; a present field that leads nowhere does not stop the search. When none
    /// does, the import is `ambiguous`, named by the first way, and every source file of the
    /// package that exports the chain's head gives a symbol (`candidates`). The chain's head is
    /// what the file exports, and each further name a member of that definition or a name the
    /// whole file exports.
    fn resolve(
        &self,
        repository: &str,
        manifest: &Manifest,
        subpath: &str,
        chain: &str,
    ) -> Option<Resolved> {
        let tried = ways(manifest, subpath);
        let first = tried.first()?.0;
        let followed = tried.iter().find_map(|(how, files)| {
            if let Some(file) = files.iter().find(|f| self.owned_source(manifest, f)) {
                return Some((file.clone(), *how, Level::Exact));
            }
            Some((self.mapped(manifest, files)?, How::OutDir, Level::Inferred))
        });
        let Some((file, how, level)) = followed else {
            let targets = self.candidates(repository, manifest, chain);
            return (!targets.is_empty()).then_some(Resolved {
                how: first,
                level: Level::Ambiguous,
                targets,
            });
        };
        let id = self.reach(repository, &file, chain)?;
        Some(Resolved {
            how,
            level,
            targets: vec![id],
        })
    }

    /// The first source file the provider indexes that one of `entries`, build output inside
    /// the `outDir` of the package's `tsconfig.json` (its directory's, else the repository
    /// root's), maps back to.
    fn mapped(&self, manifest: &Manifest, entries: &[String]) -> Option<String> {
        let dir = directory(&manifest.path);
        let config = [
            inside(dir, "tsconfig.json"),
            Some("tsconfig.json".to_string()),
        ]
        .into_iter()
        .flatten()
        .find_map(|p| self.configs.iter().find(|c| c.path == p))?;
        let (out_dir, root_dir) = roots(&config.path, &config.tsconfig, dir)?;
        (entries.iter())
            .flat_map(|e| sources(e, &out_dir, &root_dir))
            .find(|s| self.owned_source(manifest, s))
    }

    /// Whether `path` is a source file the provider indexes (`source`) whose nearest manifest
    /// is `manifest`: a file of a nested package is not the outer package's.
    fn owned_source(&self, manifest: &Manifest, path: &str) -> bool {
        self.source(path) && nearest(self.manifests, path).is_some_and(|m| m.path == manifest.path)
    }

    /// The symbols `chain` names in the source files of the package at `manifest` (those whose
    /// nearest manifest it is, `node_modules` ones left out) that export its head, sorted,
    /// without repeats. Every one is listed: a package has as many as it has files exporting
    /// the name.
    fn candidates(&self, repository: &str, manifest: &Manifest, chain: &str) -> Vec<String> {
        let head = chain.split('.').next();
        let owned = self.owned.get_or_init(|| {
            let mut owned: HashMap<&str, Vec<&str>> = HashMap::new();
            for &path in self.exported.keys() {
                if self.source(path)
                    && !path.split('/').any(|s| s == "node_modules")
                    && let Some(m) = nearest(self.manifests, path)
                {
                    owned.entry(&m.path).or_default().push(path);
                }
            }
            owned
        });
        let files = owned.get(manifest.path.as_str()).into_iter().flatten();
        let mut ids: Vec<String> = files
            .filter(|path| {
                self.exported[**path]
                    .iter()
                    .any(|e| e.name.as_deref() == head)
            })
            .filter_map(|path| self.reach(repository, path, chain))
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// The symbol `chain` names as exported by `file`: the head is what the file exports, and
    /// each further name a member of that definition or a name the whole file exports.
    fn reach(&self, repository: &str, file: &str, chain: &str) -> Option<String> {
        let mut names = chain.split('.');
        let mut at = self.lookup(file, names.next()?, 0, &mut HashSet::new())?;
        for member in names {
            at = match at {
                (file, None) => self.lookup(&file, member, 0, &mut HashSet::new())?,
                (file, Some(def)) => (file, Some(format!("{def}.{member}"))),
            };
        }
        let (file, Some(def)) = at else {
            return None;
        };
        let id = format!("{repository}::{file}::{def}");
        self.symbols.contains(id.as_str()).then_some(id)
    }

    /// What `file` exports as `name`: a file, and the qualified name of a definition in it or
    /// `None` for the whole file. The file's own export of `name` decides; without one, the
    /// files it re-exports with `export *` are looked into, for any name but `default`, at most
    /// `MAX_REEXPORTS` deep: two that give it differently make it ambiguous, so not exported, as
    /// in ES modules. `seen` cuts cycles.
    fn lookup(
        &self,
        file: &str,
        name: &str,
        depth: usize,
        seen: &mut HashSet<(String, String)>,
    ) -> Option<(String, Option<String>)> {
        if depth > MAX_REEXPORTS || !seen.insert((file.to_string(), name.to_string())) {
            return None;
        }
        let rows = self.exported.get(file).map_or(&[][..], Vec::as_slice);
        if let Some(own) = rows.iter().find(|e| e.name.as_deref() == Some(name)) {
            return Some((own.file.clone()?, own.target.clone()));
        }
        if name == "default" {
            return None;
        }
        let mut found = None;
        for all in rows.iter().filter(|e| e.name.is_none()) {
            let Some(hit) =
                (all.file.as_deref()).and_then(|f| self.lookup(f, name, depth + 1, seen))
            else {
                continue;
            };
            if found.as_ref().is_some_and(|f| *f != hit) {
                return None;
            }
            found = Some(hit);
        }
        found
    }
}

/// The providers of each package name of the workspace, as positions: the repository in
/// `exports`, then its manifest in `Export::manifests`. Built once for the workspace steps
/// (`consume`, `import`), both of which only add to the exports and never move a manifest.
pub struct Providers(HashMap<String, Vec<(usize, usize)>>);

impl Providers {
    pub fn new(exports: &[Export]) -> Self {
        let mut providers: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        for (n, export) in exports.iter().enumerate() {
            for (i, m) in export.manifests.iter().enumerate() {
                if let Some(name) = &m.summary.name {
                    providers.entry(name.clone()).or_default().push((n, i));
                }
            }
        }
        Self(providers)
    }

    fn of(&self, name: &str) -> &[(usize, usize)] {
        self.0.get(name).map_or(&[], Vec::as_slice)
    }
}

/// The workspace step after `consume`: each call through a package import, whose package the
/// importing file's nearest manifest depends on (in any of the four fields) and some repository
/// of the workspace provides (a `workspace:` range: its own), resolves in each such provider
/// (`Provided::resolve`). A call that resolves in one at least becomes a consumer of the package
/// at its site, the range of the entry in `Consumer::method` and where it resolves in
/// `Consumer::hops` (`Packages::link` derives its links), and adds an `imports` row per
/// provider. Any other, an import of a package outside the workspace included, adds nothing.
pub fn import(exports: &mut [Export], providers: &Providers) {
    // Built for a provider the first time an import reaches it.
    let provided: Vec<OnceCell<Provided>> = exports.iter().map(|_| OnceCell::new()).collect();
    let mut found = Vec::new();
    for (n, export) in exports.iter().enumerate() {
        let mut consumers = Vec::new();
        let mut imports = Vec::new();
        for call in &export.import_calls {
            let Some((name, subpath)) = split(&call.specifier) else {
                continue;
            };
            let Some(entry) = nearest(&export.manifests, &call.path)
                .and_then(|m| m.summary.dependencies.iter().find(|e| e.name == name))
            else {
                continue;
            };
            let mut hops = Vec::new();
            for &(m, i) in providers.of(name) {
                if !reaches(entry.range.as_deref(), m == n) {
                    continue;
                }
                let at = provided[m].get_or_init(|| Provided::new(&exports[m]));
                let repository = &exports[m].repository;
                let manifest = &exports[m].manifests[i];
                let Some(Resolved {
                    how,
                    level,
                    targets,
                }) = at.resolve(repository, manifest, &subpath, &call.chain)
                else {
                    continue;
                };
                for target in targets {
                    imports.push(Import {
                        site: call.site.clone(),
                        specifier: call.specifier.clone(),
                        name: call.chain.clone(),
                        level,
                        how,
                        target: target.clone(),
                    });
                    hops.push(Hop {
                        manifest: site(repository, &manifest.path),
                        target,
                        how,
                        level,
                    });
                }
            }
            if !hops.is_empty() {
                consumers.push(Consumer {
                    contract: Some(contract(name)),
                    site: call.site.clone(),
                    via: None,
                    shape: None,
                    method: entry.range.clone(),
                    fragments: Vec::new(),
                    hops,
                });
            }
        }
        found.push((consumers, imports));
    }
    for (export, (consumers, mut imports)) in exports.iter_mut().zip(found) {
        export.consumers.extend(consumers);
        index::sort_consumers(&mut export.consumers);
        imports.sort_by(|a, b| (&a.site, &a.target).cmp(&(&b.site, &b.target)));
        export.imports = imports;
    }
}

/// The workspace step that follows every repository's `declare`: for each dependency entry
/// that names a package some repository of the workspace provides, a consumer and a
/// `dependencies` row. An entry whose package is outside the workspace, or a `workspace:` one
/// whose package is outside its own repository, adds nothing.
pub fn consume(exports: &mut [Export], providers: &Providers) {
    let found: Vec<_> = (exports.iter().enumerate())
        .map(|(n, export)| {
            let mut consumers = Vec::new();
            let mut dependencies = Vec::new();
            for manifest in &export.manifests {
                for e in &manifest.summary.dependencies {
                    let range = e.range.as_deref();
                    if !(providers.of(&e.name).iter()).any(|&(m, _)| reaches(range, m == n)) {
                        continue;
                    }
                    consumers.push(Consumer {
                        contract: Some(contract(&e.name)),
                        site: format!(
                            "{}::{}::{}::{}",
                            export.repository, manifest.path, e.field, e.name
                        ),
                        via: None,
                        shape: None,
                        method: e.range.clone(),
                        fragments: Vec::new(),
                        hops: Vec::new(),
                    });
                    dependencies.push(Dependency {
                        manifest: manifest.path.clone(),
                        field: e.field.clone(),
                        name: e.name.clone(),
                        range: e.range.clone(),
                    });
                }
            }
            (consumers, dependencies)
        })
        .collect();
    for (export, (consumers, mut dependencies)) in exports.iter_mut().zip(found) {
        export.consumers.extend(consumers);
        index::sort_consumers(&mut export.consumers);
        dependencies.sort_by(|a, b| {
            (&a.manifest, &a.field, &a.name).cmp(&(&b.manifest, &b.field, &b.name))
        });
        export.dependencies = dependencies;
    }
}

/// The longest range checked, in bytes; a longer one is `range unchecked`. The crate sets no
/// limit of its own on a range, and a real one is a few dozen bytes.
// ponytail: a flat 256 (the crate's own limit for versions); a measured need lifts it.
const MAX_RANGE: usize = 256;

/// What checking a dependency range against the provider's version found, when it is not a
/// plain accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    /// The range is npm semver and does not accept the local version.
    Mismatch,
    /// The range cannot be checked: not a semver range, or no version to check it against.
    Unchecked,
}

impl Flag {
    /// The words a link reason ends with.
    pub fn words(self) -> &'static str {
        match self {
            Flag::Mismatch => "range mismatch",
            Flag::Unchecked => "range unchecked",
        }
    }
}

/// Whether the dependency `range` accepts the provider's `version`, by npm's semantics: `None`
/// when it does, else the `Flag`. A missing range or version, a `workspace:`, `file:`, `link:`
/// or `npm:` range, a git or URL source (anything holding `:` or `/`), a tag such as `latest`
/// and a range past `MAX_RANGE` are `Unchecked`. The crate lets garbage through in a range of
/// several tokens (`foo 1.2.3` parses as `1.2.3`), so each token must also look like a
/// comparator.
pub fn flag(range: Option<&str>, version: Option<&str>) -> Option<Flag> {
    let (Some(range), Some(version)) = (range, version) else {
        return Some(Flag::Unchecked);
    };
    let parsed = (range.len() <= MAX_RANGE && comparators(range))
        .then(|| nodejs_semver::Range::parse(range).ok())
        .flatten()
        .zip(nodejs_semver::Version::parse(version).ok());
    match parsed {
        None => Some(Flag::Unchecked),
        Some((range, version)) if range.satisfies(&version) => None,
        Some(_) => Some(Flag::Mismatch),
    }
}

/// Whether every token of `range` looks like part of an npm range: `||` separates sets of at
/// least one token, `-` joins a hyphen range, and a comparator is an optional operator then a
/// version body that begins with a digit, or is a wildcard (`x`, `X`, `*`) alone or followed by
/// `.` (so no `:` or `/` gets through, nor `xyz`).
fn comparators(range: &str) -> bool {
    range.split("||").all(|set| {
        let mut tokens = set.split_whitespace().peekable();
        tokens.peek().is_some()
            && tokens.all(|t| {
                let body = t.trim_start_matches(['^', '~', '<', '>', '=', 'v']);
                t == "-"
                    || body.is_empty()
                    || ((body.starts_with(|c: char| c.is_ascii_digit())
                        || matches!(body, "x" | "X" | "*")
                        || body.starts_with(['x', 'X', '*']) && body[1..].starts_with('.'))
                        && body
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || ".+-*".contains(c)))
            })
    })
}

/// The reason of a package link: `<identifier> <range|none> (local <version|none>)`, then
/// `, range mismatch` or `, range unchecked` when `flag` says so. The one place the package
/// reason is built; a path that extends it (an import hop) builds on this.
pub fn reason(contract: &str, range: Option<&str>, version: Option<&str>) -> String {
    let said = format!(
        "{contract} {} (local {})",
        range.unwrap_or("none"),
        version.unwrap_or("none")
    );
    match flag(range, version) {
        Some(f) => format!("{said}, {}", f.words()),
        None => said,
    }
}

/// Where a package site of `repository` sits: a manifest provider (`<repository>::<path>`) as
/// `<path>`, a dependency entry's consumer (`<repository>::<path>::<field>::<name>`) as `<path>
/// <field>`; `None` for any other site.
pub fn place(repository: &str, site: &str) -> Option<String> {
    let rest = site.strip_prefix(repository)?.strip_prefix("::")?;
    let parts: Vec<&str> = rest.split("::").collect();
    match parts[..] {
        [path] if path.ends_with("package.json") => Some(path.to_string()),
        [path, field, _] if path.ends_with("package.json") => Some(format!("{path} {field}")),
        _ => None,
    }
}

/// A call that reaches a symbol through a package import, for `context`.
pub struct Reach<'e> {
    /// The position in `exports` of the repository holding the call.
    pub repository: usize,
    pub call: &'e CallSite,
    pub level: Level,
    /// What the dependency's range says of the provider's version, when not a plain accept.
    pub flag: Option<Flag>,
}

/// The calls that reach `symbol` through a package import, from `links` (the workspace's): each
/// with the link's level and the range flag of the dependency entry it went through.
pub fn reached<'e>(exports: &'e [Export], links: &[Link], symbol: &str) -> Vec<Reach<'e>> {
    let mut found = Vec::new();
    for link in links
        .iter()
        .filter(|l| l.provider == symbol && l.contract.starts_with(PREFIX))
    {
        let Some((repository, consumer)) = (exports.iter().enumerate())
            .flat_map(|(n, e)| e.consumers.iter().map(move |c| (n, c)))
            .find(|(_, c)| c.site == link.consumer)
        else {
            continue;
        };
        let Some(call) = exports[repository]
            .calls
            .iter()
            .find(|c| c.id == consumer.site)
        else {
            continue;
        };
        // The manifest the hop went through holds the provider's version.
        let version = (consumer.hops.iter())
            .find(|h| h.target == symbol)
            .and_then(|h| {
                (exports.iter().flat_map(|e| &e.providers))
                    .find(|p| p.site == h.manifest && p.contract == link.contract)
            })
            .and_then(|p| p.version.as_deref());
        found.push(Reach {
            repository,
            call,
            level: link.level,
            flag: flag(consumer.method.as_deref(), version),
        });
    }
    found
}

impl ContractKind for Packages {
    fn prefix(&self) -> &'static str {
        "package"
    }

    fn sides(&self) -> Option<(&'static str, &'static str)> {
        Some(("package", "dependent"))
    }

    /// Every call through a binding imported from a bare specifier, as an `Import`: which of
    /// them name a package of the workspace is known at query time only (`import`). The
    /// manifests are read apart from the packs.
    fn extract(&self, code: &Code) -> Vec<Found> {
        let mut found = Vec::new();
        for (&path, facts) in code.files() {
            for (index, call) in facts.calls.iter().enumerate() {
                let Some((specifier, chain)) = code.imported(path, &call.scope, &call.callee)
                else {
                    continue;
                };
                if chain.is_empty() || split(specifier).is_none() {
                    continue;
                }
                found.push(Found {
                    role: Role::Import,
                    site: Site::Call {
                        path: path.to_string(),
                        index,
                    },
                    contract: None,
                    via: Some(specifier.to_string()),
                    method: Some(chain),
                    fragments: Vec::new(),
                    shape: None,
                });
            }
        }
        found
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
    /// repository. The reason is `reason()`'s, flag included. A call through a package import
    /// (`Consumer::hops`) links instead to the symbol it reaches in each provider, at the
    /// weaker of that level and the import's, the reason followed by `; import <how>`.
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
        let fits: Vec<_> = (providers.iter())
            .filter(|(repo, p)| {
                p.contract == *contract && reaches(range, repo.identity == from.identity)
            })
            .collect();
        let (level, n) = match fits.len() {
            0 => return Vec::new(),
            1 => (Level::Exact, None),
            n => (Level::Ambiguous, Some(n)),
        };
        fits.into_iter()
            .flat_map(|(_, p)| {
                let said = reason(contract, range, p.version.as_deref());
                let link = Link {
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
                    drift: None,
                };
                if consumer.hops.is_empty() {
                    return vec![link];
                }
                (consumer.hops.iter())
                    .filter(|h| h.manifest == p.site)
                    .map(|h| Link {
                        consumer: link.consumer.clone(),
                        provider: h.target.clone(),
                        contract: link.contract.clone(),
                        level: link.level.max(h.level),
                        reason: format!("{}; import {}", link.reason, h.how.name()),
                        by: None,
                        p: None,
                        drift: None,
                    })
                    .collect()
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
    fn a_package_site_shows_its_manifest_and_field() {
        let repo = "example.com/a";
        let at = |site: &str| place(repo, site);
        assert_eq!(
            at("example.com/a::package.json"),
            Some("package.json".into())
        );
        assert_eq!(
            at("example.com/a::kit/package.json::devDependencies::x"),
            Some("kit/package.json devDependencies".into())
        );
        for other in [
            "example.com/a",
            "example.com/a::src/app.ts::main",
            "example.com/a::src/app.ts::main::greet#0",
            "example.com/b::package.json",
            "example.com/a::package.json::dependencies",
        ] {
            assert_eq!(at(other), None, "{other}");
        }
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

    const MISMATCH: Option<Flag> = Some(Flag::Mismatch);
    const UNCHECKED: Option<Flag> = Some(Flag::Unchecked);

    #[test]
    fn npm_ranges_are_checked_against_the_local_version() {
        let table = [
            ("^1.0.0", "1.2.0", None),
            ("^1.0.0", "2.0.0", MISMATCH),
            ("^0.2.3", "0.2.9", None),
            ("^0.2.3", "0.3.0", MISMATCH),
            ("~1.1.0", "1.1.9", None),
            ("~1.1.0", "1.2.0", MISMATCH),
            ("1.x", "1.9.0", None),
            ("1.x", "2.0.0", MISMATCH),
            ("*", "7.0.0", None),
            ("1.2.0 - 1.4.0", "1.3.0", None),
            ("1.2.0 - 1.4.0", "1.5.0", MISMATCH),
            ("^1.0.0 || ^3.0.0", "3.1.0", None),
            ("^1.0.0 || ^3.0.0", "2.0.0", MISMATCH),
            (">=1.0.0 <2.0.0", "1.5.0", None),
            (">=1.0.0 <2.0.0", "2.0.0", MISMATCH),
            ("1.2.0", "1.2.0", None),
            ("1.2.0", "1.2.1", MISMATCH),
            // A prerelease matches only a comparator on the same major.minor.patch.
            ("^1.0.0", "1.2.0-beta.1", MISMATCH),
            ("^1.2.0-beta.0", "1.2.0-beta.1", None),
            ("^1.2.0-beta.0", "1.3.0-beta.1", MISMATCH),
        ];
        for (range, version, want) in table {
            assert_eq!(
                flag(Some(range), Some(version)),
                want,
                "{range} / {version}"
            );
        }
    }

    #[test]
    fn a_range_that_cannot_be_checked_is_unchecked() {
        let ranges = [
            "workspace:*",
            "workspace:^1.0.0",
            "file:../shared",
            "link:../shared",
            "npm:other@^1.0.0",
            "git+https://example.com/a.git",
            "github:user/repo",
            "user/repo",
            "https://example.com/a.tgz",
            "latest",
            "next",
            "foo 1.2.3",
            "xyz 1.2.3",
            "x-foo",
            "1.2.3 foo",
            "^1.0.0 || bar",
            "",
        ];
        for range in ranges {
            assert_eq!(flag(Some(range), Some("1.2.0")), UNCHECKED, "{range:?}");
        }
        assert_eq!(flag(None, Some("1.2.0")), UNCHECKED);
        assert_eq!(flag(Some("^1.0.0"), None), UNCHECKED);
        // A local version that is not a version cannot be compared either.
        assert_eq!(flag(Some("^1.0.0"), Some("not-a-version")), UNCHECKED);
        let long = "^1.0.0 || ".repeat(MAX_RANGE);
        assert!(long.len() > MAX_RANGE);
        assert_eq!(flag(Some(&long), Some("1.2.0")), UNCHECKED);
    }

    #[test]
    fn the_reason_ends_with_the_flag() {
        let said = |range, version| reason("package::npm::a", range, version);
        assert_eq!(
            said(Some("^2.0.0"), Some("1.0.0")),
            "package::npm::a ^2.0.0 (local 1.0.0), range mismatch"
        );
        assert_eq!(
            said(Some("latest"), Some("1.0.0")),
            "package::npm::a latest (local 1.0.0), range unchecked"
        );
        assert_eq!(
            said(Some("^1.0.0"), Some("1.0.0")),
            "package::npm::a ^1.0.0 (local 1.0.0)"
        );
        assert_eq!(
            said(None, None),
            "package::npm::a none (local none), range unchecked"
        );
    }

    #[test]
    fn the_exports_map_gives_each_subpath_its_target() {
        let map = serde_json::json!({
            ".": {"types": "./src/index.ts", "require": "./dist/index.cjs"},
            "./format": "./src/format.ts",
            "./conditions": {"node": "./node.ts", "import": "./import.ts", "default": "./d.ts"},
            "./nested": {"default": {"require": "./r.ts", "import": "./i.ts"}},
            "./required": {"require": "./required.ts"},
            "./none": {"node": "./node.ts"},
            "./utils/*": {"import": "./src/utils/*.ts"},
            "./utils/special/*": "./src/special/*.js",
            "./utils/blocked/*": null,
            "./blocked": null,
            "./typed": {"types": null, "default": "./typed.ts"},
            "./fallbacks": ["./a.ts", "./b.ts"],
            "./many/*/x/*": "./many.ts",
            "./two/*": "./src/*/*.ts",
        });
        let cases: [(&str, Option<&str>); 18] = [
            // The first of `types`, `import`, `default`, `require` present is taken.
            (".", Some("./src/index.ts")),
            ("./format", Some("./src/format.ts")),
            ("./conditions", Some("./import.ts")),
            ("./nested", Some("./i.ts")),
            ("./required", Some("./required.ts")),
            // Other conditions are not followed.
            ("./none", None),
            // Exact keys first, then the pattern with the longest prefix; `*` is replaced.
            ("./utils/slug", Some("./src/utils/slug.ts")),
            ("./utils/a/b", Some("./src/utils/a/b.ts")),
            ("./utils/special/x", Some("./src/special/x.js")),
            ("./utils/", None),
            // A `null` target blocks the subpath, even under a condition taken.
            ("./utils/blocked/x", None),
            ("./blocked", None),
            ("./typed", None),
            // Fallback arrays and keys with two `*` are not read.
            ("./fallbacks", None),
            ("./many/a/x/b", None),
            // A target with two `*` is not read either.
            ("./two/x", None),
            ("./missing", None),
            ("./format/more", None),
        ];
        for (subpath, expected) in cases {
            assert_eq!(exported(&map, subpath).as_deref(), expected, "{subpath}");
        }
        // A string or an object of conditions is the target of `.` alone; keys that mix
        // subpaths and conditions make no target.
        let sugar = serde_json::json!("./main.ts");
        assert_eq!(exported(&sugar, ".").as_deref(), Some("./main.ts"));
        assert_eq!(exported(&sugar, "./x"), None);
        let conditions = serde_json::json!({"import": "./i.ts", "require": "./r.ts"});
        assert_eq!(exported(&conditions, ".").as_deref(), Some("./i.ts"));
        let mixed = serde_json::json!({".": "./a.ts", "import": "./b.ts"});
        assert_eq!(exported(&mixed, "."), None);
    }

    #[test]
    fn a_target_of_many_stars_is_not_expanded() {
        let stars = format!("./{}.ts", "*".repeat(1 << 20));
        let map = serde_json::json!({ "./h/*": stars, ".": stars });
        assert_eq!(exported(&map, &format!("./h/{}", "x".repeat(64))), None);
        assert_eq!(exported(&map, "."), None);
    }

    #[test]
    fn conditions_nested_past_the_limit_give_no_target() {
        let nest = |n: usize| {
            let mut v = serde_json::json!("./deep.ts");
            for _ in 0..n {
                v = serde_json::json!({ "default": v });
            }
            v
        };
        assert_eq!(
            exported(&nest(MAX_CONDITIONS), ".").as_deref(),
            Some("./deep.ts")
        );
        assert_eq!(exported(&nest(MAX_CONDITIONS + 1), "."), None);
    }

    fn manifest(path: &str, json: &str) -> Manifest {
        Manifest {
            path: path.to_string(),
            summary: read(json.as_bytes()),
        }
    }

    #[test]
    fn without_exports_the_bare_name_tries_types_then_module_then_main() {
        let at = |json: &str, subpath: &str| ways(&manifest("lib/package.json", json), subpath);
        let all = r#"{"types": "./t.ts", "module": "m.ts", "main": "./main.ts"}"#;
        assert_eq!(
            at(all, "."),
            [
                (How::Types, vec!["lib/t.ts".into()]),
                (How::Module, vec!["lib/m.ts".into()]),
                (How::Main, vec!["lib/main.ts".into()]),
            ]
        );
        // A field that is absent, or cannot be followed, is skipped, not a stop.
        let two = r#"{"types": "../t.ts", "main": "src/index.js"}"#;
        assert_eq!(at(two, "."), [(How::Main, vec!["lib/src/index.js".into()])]);
        // None of the fields (an empty or non-string one is absent): Node's default, `index.<ext>`
        // alone, in the pack's order.
        for json in ["{}", r#"{"main": "", "module": 3}"#] {
            let [(how, files)] = &at(json, ".")[..] else {
                panic!("one way")
            };
            assert_eq!(*how, How::Path);
            assert_eq!(
                &files[..3],
                ["lib/index.ts", "lib/index.tsx", "lib/index.js"]
            );
            assert_eq!(files.len(), EXTENSIONS.len());
        }
        // A subpath is a path inside the package, with the trunk's extension and index rules.
        let [(how, files)] = &at(all, "./util/x")[..] else {
            panic!("one way")
        };
        assert_eq!(*how, How::Path);
        assert_eq!(
            &files[..3],
            ["lib/util/x", "lib/util/x.ts", "lib/util/x.tsx"]
        );
        assert!(files.contains(&"lib/util/x/index.ts".to_string()));
        // `exports`, when present, is the only way in, and its targets start with `./`.
        let both = r#"{"main": "./main.ts", "exports": {".": "./e.ts", "./bare": "e.ts"}}"#;
        assert_eq!(at(both, "."), [(How::Exports, vec!["lib/e.ts".into()])]);
        assert!(at(both, "./util/x").is_empty());
        assert!(at(both, "./bare").is_empty());
    }

    #[test]
    fn an_entry_never_leaves_the_package_directory() {
        let at = |json: &str, subpath: &str| ways(&manifest("lib/package.json", json), subpath);
        for main in ["../x.ts", "/x.ts", "./a/../../x.ts", "a//x.ts", "./"] {
            let json = format!(r#"{{"main": {}}}"#, serde_json::to_string(main).unwrap());
            assert!(at(&json, ".").is_empty(), "{main}");
        }
        let star = r#"{"exports": {"./*": "./src/*.ts"}}"#;
        assert!(at(star, "./../../x").is_empty());
        assert!(at("{}", "./../x").is_empty());
        // At the repository root the package directory is the root.
        let root = manifest("package.json", r#"{"main": "./index.ts"}"#);
        assert_eq!(ways(&root, "."), [(How::Main, vec!["index.ts".into()])]);
    }

    #[test]
    fn a_specifier_splits_into_its_package_and_subpath() {
        assert_eq!(split("x"), Some(("x", ".".into())));
        assert_eq!(split("x/a/b"), Some(("x", "./a/b".into())));
        assert_eq!(split("@a/b"), Some(("@a/b", ".".into())));
        assert_eq!(split("@a/b/c"), Some(("@a/b", "./c".into())));
        for none in ["./x", "../x", "/x", "node:fs", "@a", "a b"] {
            assert_eq!(split(none), None, "{none}");
        }
    }

    #[test]
    fn the_nearest_manifest_is_in_the_deepest_directory_holding_the_file() {
        let all = [
            manifest("package.json", "{}"),
            manifest("packages/a/package.json", "{}"),
            manifest("packages/ab/package.json", "{}"),
        ];
        let of = |path| nearest(&all, path).map(|m| m.path.as_str());
        assert_eq!(of("packages/a/src/x.ts"), Some("packages/a/package.json"));
        assert_eq!(of("packages/abc/x.ts"), Some("package.json"));
        assert_eq!(of("x.ts"), Some("package.json"));
        assert!(nearest(&all[1..], "x.ts").is_none());
    }

    #[test]
    fn build_output_maps_back_to_the_same_path_under_the_root_dir() {
        let all = |stem: &str| {
            ["ts", "tsx", "js", "jsx", "mjs", "cjs"]
                .map(|ext| format!("{stem}.{ext}"))
                .to_vec()
        };
        // (entry, outDir, rootDir, the source files tried, in order)
        let table: [(&str, &str, &str, Vec<String>); 12] = [
            ("dist/index.d.ts", "dist", "src", all("src/index")),
            ("dist/index.js", "dist", "src", all("src/index")),
            ("dist/index.cjs", "dist", "src", all("src/index")),
            ("dist/index.mjs", "dist", "src", all("src/index")),
            ("dist/ui/button.jsx", "dist", "src", all("src/ui/button")),
            // A package below the root keeps its directory in both.
            (
                "lib/a/dist/x.js",
                "lib/a/dist",
                "lib/a/src",
                all("lib/a/src/x"),
            ),
            // `rootDir` at the package directory (its default) or at the repository root.
            ("lib/a/dist/x.js", "lib/a/dist", "lib/a", all("lib/a/x")),
            ("dist/x.js", "dist", ".", all("x")),
            // Not in `outDir` (a name sharing its prefix included), no output extension, or the
            // directory itself: nothing to try.
            ("lib/index.js", "dist", "src", vec![]),
            ("distribution/index.js", "dist", "src", vec![]),
            ("dist/data.json", "dist", "src", vec![]),
            ("dist", "dist", "src", vec![]),
        ];
        for (entry, out_dir, root_dir, expected) in table {
            assert_eq!(sources(entry, out_dir, root_dir), expected, "{entry}");
        }
    }

    fn tsconfig(json: &str) -> Option<Tsconfig> {
        read_tsconfig(json.as_bytes())
    }

    #[test]
    fn a_tsconfig_gives_its_out_dir_and_root_dir() {
        let t = tsconfig(
            r#"{
                // comments and trailing commas are allowed
                "compilerOptions": { "outDir": "dist", /* x */ "rootDir": "src", },
                "extends": "./base.json",
            }"#,
        )
        .unwrap();
        assert_eq!(
            (t.out_dir.as_deref(), t.root_dir.as_deref()),
            (Some("dist"), Some("src"))
        );
        // `extends` is not followed: what the base declares is not read.
        // A valid config without usable options is empty, not invalid.
        let empty = Some(Tsconfig::default());
        for valid in [
            "{}",
            r#"{"compilerOptions": 3}"#,
            r#"{"compilerOptions": {"outDir": 1, "rootDir": ["src"]}}"#,
            r#"{"compilerOptions": {"outDir": "", "rootDir": ""}}"#,
        ] {
            assert_eq!(tsconfig(valid), empty, "{valid}");
        }
        // Invalid input is no config.
        for bad in ["", "[]", "{ not json", "3"] {
            assert_eq!(tsconfig(bad), None, "{bad}");
        }
        let huge = format!(
            r#"{{"compilerOptions": {{"outDir": "{}"}}}}"#,
            "d".repeat(MAX_BYTES)
        );
        assert_eq!(tsconfig(&huge), None);
        let nested = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert_eq!(tsconfig(&nested), None);
    }

    #[test]
    fn the_roots_stay_inside_the_repository_relative_to_the_config() {
        let at = |config: &str, out: Option<&str>, root: Option<&str>, package: &str| {
            let t = Tsconfig {
                out_dir: out.map(String::from),
                root_dir: root.map(String::from),
            };
            roots(config, &t, package)
        };
        let some = |out: &str, root: &str| Some((out.to_string(), root.to_string()));
        assert_eq!(
            at("lib/tsconfig.json", Some("dist"), Some("src"), "lib"),
            some("lib/dist", "lib/src")
        );
        // `rootDir` defaults to the package directory, not the config's.
        assert_eq!(
            at("tsconfig.json", Some("./dist/"), None, "lib"),
            some("dist", "lib")
        );
        assert_eq!(
            at("tsconfig.json", Some("dist"), Some("."), "."),
            some("dist", ".")
        );
        assert_eq!(
            at("lib/tsconfig.json", Some("dist"), Some("./"), "lib"),
            some("lib/dist", "lib")
        );
        // No `outDir`, one that is the config's own directory, or a path that leaves the
        // repository or is absolute: no mapping, and a bad `rootDir` voids the pair too.
        for (out, root) in [
            (None, Some("src")),
            (Some("."), Some("src")),
            (Some(""), None),
            (Some("../dist"), None),
            (Some("a/../../dist"), None),
            (Some("/abs/dist"), None),
            (Some("a//dist"), None),
            (Some("dist"), Some("../src")),
            (Some("dist"), Some("/src")),
        ] {
            assert_eq!(
                at("tsconfig.json", out, root, "."),
                None,
                "{out:?} {root:?}"
            );
        }
    }

    /// A provider whose files export what `rows` say.
    fn provided<'e>(rows: &'e [Exported]) -> Provided<'e> {
        let mut exported: HashMap<&str, Vec<&Exported>> = HashMap::new();
        for e in rows {
            exported.entry(&e.path).or_default().push(e);
        }
        Provided {
            files: HashSet::new(),
            symbols: HashSet::new(),
            exported,
            manifests: &[],
            configs: &[],
            owned: OnceCell::new(),
        }
    }

    fn row(path: &str, name: Option<&str>, file: Option<&str>, target: Option<&str>) -> Exported {
        Exported {
            path: path.into(),
            name: name.map(Into::into),
            file: file.map(Into::into),
            target: target.map(Into::into),
        }
    }

    fn lookup(p: &Provided, file: &str, name: &str) -> Option<(String, Option<String>)> {
        p.lookup(file, name, 0, &mut HashSet::new())
    }

    #[test]
    fn a_name_is_looked_up_in_the_file_then_through_its_export_stars() {
        let rows = [
            row("i.ts", Some("own"), Some("d.ts"), Some("own")),
            row("i.ts", Some("gone"), None, None),
            row("i.ts", None, Some("a.ts"), None),
            row("a.ts", Some("deep"), Some("a.ts"), Some("deep")),
            row("a.ts", Some("gone"), Some("a.ts"), Some("gone")),
            row("a.ts", Some("default"), Some("a.ts"), Some("main")),
            row("a.ts", Some("ns"), Some("n.ts"), None),
            // A cycle back to the entry.
            row("a.ts", None, Some("i.ts"), None),
        ];
        let p = provided(&rows);
        let def = |file: &str, name: &str| Some((file.to_string(), Some(name.to_string())));
        assert_eq!(lookup(&p, "i.ts", "own"), def("d.ts", "own"));
        assert_eq!(lookup(&p, "i.ts", "deep"), def("a.ts", "deep"));
        assert_eq!(lookup(&p, "i.ts", "ns"), Some(("n.ts".into(), None)));
        // The file's own export decides, even when it names nothing the repository holds.
        assert_eq!(lookup(&p, "i.ts", "gone"), None);
        // `export *` carries no `default`, and a cycle ends the lookup.
        assert_eq!(lookup(&p, "i.ts", "default"), None);
        assert_eq!(lookup(&p, "i.ts", "missing"), None);
    }

    #[test]
    fn a_name_two_export_stars_give_differently_is_not_exported() {
        let rows = [
            row("i.ts", None, Some("a.ts"), None),
            row("i.ts", None, Some("b.ts"), None),
            row("i.ts", None, Some("c.ts"), None),
            row("a.ts", Some("dup"), Some("a.ts"), Some("dup")),
            row("b.ts", Some("dup"), Some("b.ts"), Some("dup")),
            row("a.ts", Some("same"), Some("a.ts"), Some("same")),
            row("c.ts", None, Some("a.ts"), None),
        ];
        let p = provided(&rows);
        assert_eq!(lookup(&p, "i.ts", "dup"), None);
        // Two ways to one definition are one export.
        let same = Some(("a.ts".to_string(), Some("same".to_string())));
        assert_eq!(lookup(&p, "i.ts", "same"), same);
    }

    #[test]
    fn a_chain_of_export_stars_past_the_limit_is_not_followed() {
        let chain = |n: usize| {
            let mut rows: Vec<Exported> = (0..n)
                .map(|i| {
                    row(
                        &format!("{i}.ts"),
                        None,
                        Some(&format!("{}.ts", i + 1)),
                        None,
                    )
                })
                .collect();
            rows.push(row(&format!("{n}.ts"), Some("f"), Some("f.ts"), Some("f")));
            rows
        };
        let found = chain(MAX_REEXPORTS);
        assert!(lookup(&provided(&found), "0.ts", "f").is_some());
        let past = chain(MAX_REEXPORTS + 1);
        assert_eq!(lookup(&provided(&past), "0.ts", "f"), None);
    }

    #[test]
    fn candidates_are_the_exporting_source_files_of_that_package_alone() {
        let root = manifest("package.json", "{}");
        let nested = manifest("sealed/package.json", "{}");
        let manifests = [root, nested];
        let paths = [
            "src/a.ts",
            "src/a.d.ts",
            "sealed/src/b.ts",
            "node_modules/x/c.ts",
            "sealed/node_modules/y/d.ts",
        ];
        let rows: Vec<Exported> = (paths.iter())
            .map(|p| row(p, Some("f"), Some(p), Some("f")))
            .collect();
        let mut p = provided(&rows);
        p.files = paths.iter().copied().collect();
        let ids: Vec<String> = paths.iter().map(|p| format!("r::{p}::f")).collect();
        p.symbols = ids.iter().map(String::as_str).collect();
        p.manifests = &manifests;

        // The root package does not take the nested package's file (a directory-prefix test
        // would), a declaration file, or one under `node_modules`; the nested one neither.
        assert_eq!(p.candidates("r", &manifests[0], "f"), ["r::src/a.ts::f"]);
        assert_eq!(
            p.candidates("r", &manifests[1], "f"),
            ["r::sealed/src/b.ts::f"]
        );
        assert!(p.candidates("r", &manifests[0], "g").is_empty());
    }

    #[test]
    fn an_invalid_package_tsconfig_falls_back_to_the_root_one_and_a_valid_one_does_not() {
        let manifests = [manifest("pkg/package.json", "{}")];
        let rows = [];
        let mut p = provided(&rows);
        p.files = ["pkg/src/index.ts"].into_iter().collect();
        p.manifests = &manifests;
        let entries = ["pkg/dist/index.js".to_string()];
        let config = |path: &str, out: Option<&str>, root: &str| Config {
            path: path.into(),
            tsconfig: Tsconfig {
                out_dir: out.map(Into::into),
                root_dir: Some(root.into()),
            },
        };
        let root = config("tsconfig.json", Some("pkg/dist"), "pkg/src");
        let found = Some("pkg/src/index.ts".to_string());

        // An invalid `pkg/tsconfig.json` has no `Config` (see `read_tsconfig`): the root's maps.
        let configs = [root];
        p.configs = &configs;
        assert_eq!(p.mapped(&manifests[0], &entries), found);
        // A valid one without `outDir` maps nothing, and the root's is not tried.
        let configs = [
            config("pkg/tsconfig.json", None, "src"),
            config("tsconfig.json", Some("pkg/dist"), "pkg/src"),
        ];
        p.configs = &configs;
        assert_eq!(p.mapped(&manifests[0], &entries), None);
        // With its own `outDir`, the package's config is the one used.
        let configs = [
            config("pkg/tsconfig.json", Some("dist"), "src"),
            config("tsconfig.json", Some("elsewhere"), "pkg/src"),
        ];
        p.configs = &configs;
        assert_eq!(p.mapped(&manifests[0], &entries), found);
    }

    #[test]
    fn a_directory_is_the_manifest_s_parent_and_dot_at_the_root() {
        assert_eq!(directory("package.json"), ".");
        assert_eq!(directory("packages/a/package.json"), "packages/a");
    }
}
