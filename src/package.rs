//! Packages (spec #22): every manifest of a workspace repository with a valid name provides
//! `package::<ecosystem>::<name>` (`package.json`: `npm`; `pyproject.toml`: `pypi`, spec #24;
//! `Cargo.toml`: `cargo`, spec #30; `go.mod`: `go`, spec #25; `pom.xml` and the Gradle build and
//! settings files: `maven`, spec #26),
//! and every dependency entry that names a package of the workspace, in the same ecosystem,
//! consumes it. `refresh` reads each manifest into a `Summary`, derived from its bytes
//! alone and stored in the index; `Packages::declare` turns the named ones into providers and
//! `consume` (a workspace-wide step, since one repository cannot see the others' providers)
//! turns the entries that name one into consumers. Meada reads no lockfile and queries no
//! registry: a dependency on anything outside the workspace adds nothing.
//!
//! This file holds what the ecosystems share: the `Ecosystem` trait with its defaults (Node's
//! rule for the ways in, the candidates and the provider view), the list `ECOSYSTEMS`, the
//! summary types and the workspace steps. Each ecosystem's own rules live in its module under
//! `package/` (`npm`, `pypi`, `cargo`, `go`, `maven`).
//!
//! Imports cross too: `extract` stores every call through a binding imported from a bare
//! specifier, and `import`, a workspace step after `consume`, resolves those whose package the
//! workspace provides into the provider's symbol, through its entry fields and what its files
//! export (`resolve::Exported`). Nothing of it is stored as a link: `link` derives the hop.
//!
//! Shapes cross the same way (issue #175): a repository with a named manifest keeps the shape of
//! each constant, schema or type its files export (`exported_shapes`), never its source text, and
//! `shapes`, a workspace step after `import`, fills the parts of the shapes that import one.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::{ContractKind, Found, Hit, Level, Link, Role, Search, Site};
use crate::entry;
use crate::index::{self, CallSite, Consumer, Export, Indexed, Provider};
use crate::lang::ts::{EXTENSIONS, strip_jsonc, with_extensions};
use crate::lang::{Call, MAX_BYTES, python};
use crate::literals::Literals;
use crate::render::{Files, label, rank, section};
use crate::resolve::{Code, Exported};
use crate::shape::{self, Shape, Want};
use crate::workspace::Repository;

mod cargo;
mod go;
mod maven;
mod npm;
mod pypi;

use cargo::Cargo;
use go::Go;
use maven::Maven;
use npm::{Npm, exported};
use pypi::Pypi;

pub struct Packages;

/// The start of every package contract identifier.
const PREFIX: &str = "package::";

/// A package ecosystem: the manifests it reads, the contract its packages provide, and the rules
/// its imports and dependency entries follow. Every question but the first six has a default
/// body; an adapter answers differently only where its ecosystem does. An npm and a PyPI package
/// of one name are two contracts. A new ecosystem implements this and appends to `ECOSYSTEMS`:
/// the order of the list fixes the order `of` and `of_contract` search in and the order `inherit`
/// runs in.
trait Ecosystem {
    /// Its name, as the contract identifier and the export spell it.
    fn name(&self) -> &'static str;

    /// Whether the file `file_name` is one of its manifests.
    fn is_manifest(&self, file_name: &str) -> bool;

    /// What the manifest at `path` says, read from its bytes; never fails, hostile bytes read
    /// as empty.
    fn read(&self, path: &str, src: &[u8]) -> Summary;

    /// The extensions of the source files whose imports name its packages.
    fn sources(&self) -> &'static [&'static str];

    /// Whether a bare `specifier` may name one of its packages.
    fn names_package(&self, specifier: &str) -> bool;

    /// The imports a call of the file `path` goes through, as `(specifier, chain)` (`Code::imported`):
    /// the one import that binds its head.
    fn imports<'a>(&self, code: &Code<'a>, path: &'a str, call: &'a Call) -> Vec<(String, String)> {
        let imported = code.imported(path, &call.scope, &call.callee);
        (imported.into_iter())
            .map(|(specifier, chain)| (specifier.to_string(), chain))
            .collect()
    }

    /// The dependency entries an import of `specifier` by the file `from.1` of the repository
    /// `from.0` may name, each with the subpath inside the package.
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        from: (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)>;

    /// The start of its contract identifiers.
    fn prefix(&self) -> String {
        format!("{PREFIX}{}::", self.name())
    }

    /// Whether an uncertain dependency name may name a provider: literal equality.
    /// Certain names use the provider index directly.
    fn fits(&self, wanted: &str, provided: &str) -> bool {
        wanted == provided
    }

    /// Why the dependency's source cannot establish an exact coordinate, if any.
    fn uncertainty(&self, _via: Option<&str>) -> Option<&'static str> {
        None
    }

    /// What the dependency `range` says of the provider's `version`: never evaluated.
    fn flag(&self, _range: Option<&str>, _version: Option<&str>) -> Option<Flag> {
        Some(Flag::Unchecked)
    }

    /// Whether a dependency entry of source `(via, range)` may reach the provider whose
    /// manifest is `to.1` in the repository `to.0`, from a manifest of `from` (each repository
    /// by identity and root): every provider.
    fn reaches(
        &self,
        _source: (Option<&str>, Option<&str>),
        _from: (&str, &Path),
        _to: ((&str, &Path), &str),
    ) -> bool {
        true
    }

    /// Whether `manifest`, one of its manifests, bounds the package of the files under it.
    fn owns(&self, _manifest: &Manifest) -> bool {
        true
    }

    /// The ways an import of `subpath` of the package at `manifest` may be followed: Node's
    /// rule (`ways`).
    fn ways(&self, manifest: &Manifest, subpath: &str) -> Vec<(How, Vec<String>)> {
        ways(manifest, subpath)
    }

    /// The symbol an import of `subpath` of the package at `manifest` of `repository`, through
    /// `entry` and named `specifier`, names as `chain` in the provider `at`, by Node's rule: of
    /// the ways in (`Ecosystem::ways`), the first that leads to a source file the package owns
    /// wins, `exact` through the declared field or rule, `inferred` through the package's
    /// `outDir` and `rootDir` (`sources`); a present field that leads nowhere does not stop the
    /// search. When none does, the import is `ambiguous`, named by the first way, and every
    /// source file of the package that exports the chain's head gives a symbol
    /// (`Provided::candidates`).
    fn resolve(
        &self,
        at: &Provided,
        repository: &str,
        manifest: &Manifest,
        (_entry, subpath): (&Entry, &str),
        (_specifier, chain): (&str, &str),
        known: &dyn Fn(&str) -> bool,
    ) -> Option<Resolved> {
        let tried = self.ways(manifest, subpath);
        let first = tried.first()?.0;
        let Some((file, how, level)) = at.entry(manifest, &tried) else {
            let targets = at.candidates(repository, manifest, chain, known);
            return (!targets.is_empty()).then_some(Resolved {
                how: first,
                level: Level::Ambiguous,
                targets,
            });
        };
        let id = at.reach(repository, &file, chain, known)?;
        Some(Resolved {
            how,
            level,
            targets: vec![id],
        })
    }

    /// Where the name `chain`, which the file `from.1` of the repository `from.0` imports from
    /// `specifier`, resolves: each entry the import may name (`Ecosystem::entries`), and each
    /// provider of the entry's package that the entry reaches and where the name resolves there
    /// (`Ecosystem::resolve`). `at` gives a repository's provider view, `known` which of its
    /// ids the name may resolve to.
    fn resolutions<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        (n, from): (usize, &str),
        (specifier, chain): (&str, &str),
        known: &dyn Fn(&Provided<'e>, &str) -> bool,
    ) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
        let mut out = Vec::new();
        for (entry, subpath) in self.entries(exports, providers, (n, from), specifier) {
            let mut resolved = Vec::new();
            for (m, i) in providers.matching(self, entry) {
                let manifest = &exports[m].manifests[i];
                let source = (entry.via.as_deref(), entry.range.as_deref());
                let to = (exports[m].place(), manifest.path.as_str());
                if !self.reaches(source, exports[n].place(), to) {
                    continue;
                }
                let at = at(m);
                let known = |id: &str| known(at, id);
                let repository = &exports[m].repository;
                let import = ((entry, subpath.as_str()), (specifier, chain));
                if let Some(r) = self.resolve(at, repository, manifest, import.0, import.1, &known)
                {
                    resolved.push((m, manifest, at, r));
                }
            }
            out.push((entry, resolved));
        }
        out
    }

    /// Whether shapes cross its packages (`surface`).
    fn shapes(&self) -> bool {
        false
    }

    /// The program entry points its manifests declare, as `(file, function)`, given the files
    /// of the repository: none.
    fn programs(&self, _manifests: &[Manifest], _files: &[&str]) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Fills what a member manifest of `manifests` inherits from a parent one: nothing.
    fn inherit(&self, _manifests: &mut [Manifest]) {}
}

/// Every ecosystem, in the order `of` and `of_contract` search and `inherit` runs.
const ECOSYSTEMS: &[&dyn Ecosystem] = &[&Npm, &Pypi, &Cargo, &Go, &Maven];

/// The ecosystem of the manifest at `path`, by its file name (`Ecosystem::is_manifest`); `None`
/// for any other file.
fn of(path: &str) -> Option<&'static dyn Ecosystem> {
    let name = path.rsplit('/').next()?;
    ECOSYSTEMS.iter().copied().find(|e| e.is_manifest(name))
}

/// The ecosystem whose packages the source file at `path` imports, by its extension
/// (`Ecosystem::sources`); `None` for a file no ecosystem claims.
fn of_source(path: &str) -> Option<&'static dyn Ecosystem> {
    let (_, extension) = path.rsplit_once('.')?;
    (ECOSYSTEMS.iter().copied()).find(|e| e.sources().contains(&extension))
}

/// The program entry points the manifests of every ecosystem declare (`Ecosystem::programs`),
/// sorted, without repeats, for the index writer.
pub fn programs(manifests: &[Manifest], files: &[&str]) -> Vec<(String, String)> {
    programs_of(ECOSYSTEMS, manifests, files)
}

fn programs_of(
    list: &[&dyn Ecosystem],
    manifests: &[Manifest],
    files: &[&str],
) -> Vec<(String, String)> {
    let mut found: Vec<_> = (list.iter())
        .flat_map(|e| e.programs(manifests, files))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Lets every ecosystem fill what its member manifests inherit (`Ecosystem::inherit`), in the
/// order of the list, for the index writer.
pub fn inherit(manifests: &mut [Manifest]) {
    inherit_of(ECOSYSTEMS, manifests);
}

fn inherit_of(list: &[&dyn Ecosystem], manifests: &mut [Manifest]) {
    for e in list {
        e.inherit(manifests);
    }
}

/// The ecosystem of a package contract identifier, by its prefix.
fn of_contract(contract: &str) -> Option<&'static dyn Ecosystem> {
    (ECOSYSTEMS.iter().copied()).find(|e| contract.starts_with(&e.prefix()))
}

/// The contract a package name of `ecosystem` provides.
fn contract(ecosystem: &dyn Ecosystem, name: &str) -> String {
    format!("{}{name}", ecosystem.prefix())
}

/// Whether `ecosystem` is npm's, the one a package of the export names only when it is not: the
/// export's compatibility rule, since the `ecosystem` field did not exist when npm was the only
/// one and the goldens leave it out.
/// Whether the file at `path` is one the manifests table holds: a manifest of an ecosystem or
/// a `tsconfig.json`.
pub fn is_manifest(path: &str) -> bool {
    is_tsconfig(path) || of(path).is_some()
}

/// What the manifest at `path` says (see `of`); empty for a file of no ecosystem.
pub fn read_manifest(path: &str, src: &[u8]) -> Summary {
    of(path).map_or_else(Summary::default, |e| e.read(path, src))
}

/// The longest package name read, in bytes; a longer one makes no provider.
// ponytail: npm's own limit, for PyPI names too; a name past it is not a package.
pub(crate) const MAX_NAME: usize = 214;

/// The longest chain of `export *` a lookup in a provider follows; a name further down is not
/// found.
// ponytail: real packages re-export a few levels deep; this bounds the walk over a hostile one.
const MAX_REEXPORTS: usize = 64;

/// What Meada read from one manifest of any ecosystem (a `package.json`, `pyproject.toml`,
/// `requirements*.txt`, `Cargo.toml` or `go.mod`). Nothing else of the file is kept; a field
/// the ecosystem has no use for stays empty.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Summary {
    /// JVM build metadata needed to join Gradle settings and project manifests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<crate::jvm::Build>,
    /// The package's name as its ecosystem spells it; `None` when absent, not a string or not
    /// valid there, which makes no provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The declared version; `None` when absent or when the format carries none (`go.mod`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Sorted by field, then name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Entry>,
    /// The `exports` field as written; `None` when absent or `null` (npm only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exports: Option<Value>,
    /// The `types`, `module` and `main` entry fields; a field that is not a string is absent
    /// (npm only, except `main`, which Cargo sets too).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    /// Source roots relative to the manifest's directory (PyPI and JVM).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    /// JVM only: the source roots of `roots` that hold tests or another source set than the
    /// artifact's `main`, whose classes a dependent never imports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_roots: Vec<String>,
    /// The console scripts a `pyproject.toml` declares, as `(module, function)` (PyPI only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<(String, String)>,
    /// The `[workspace.dependencies]` entries of a `Cargo.toml` (Cargo only). For Cargo,
    /// `main` is the library's root file, as a path of the repository.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace: Vec<Entry>,
    /// Cargo only: `[lib] name` when it differs from what a path writes for the package's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lib_name: Option<String>,
    /// Cargo only: `version.workspace = true`, until the seam's `inherit` fills `version`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherits_version: bool,
    /// Cargo only: the `[workspace.package] version` a member may inherit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_version: Option<String>,
}

/// One dependency entry of a manifest. `name` is the package the entry names.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct Entry {
    pub field: String,
    pub name: String,
    /// The declared range; `None` when absent, empty or not a string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// Cargo only: the key the entry is written under when it renames the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Where the package comes from: Cargo's `path <manifest>` or `git <identity>`,
    /// or `computed` for a coordinate whose literal fragments only constrain candidates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Cargo only: `workspace = true`, until the seam's `inherit` fills the entry.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherits: bool,
}

impl Entry {
    /// The name the dependent writes for the package: the key, else the package's own.
    pub fn key(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

/// A manifest of the index: its repository-relative path and what was read of it.
pub struct Manifest {
    pub path: String,
    pub summary: Summary,
}

/// A package a repository provides (`meada export`).
pub struct Package {
    pub name: String,
    pub version: Option<String>,
    pub repository: String,
    /// The directory of the manifest, relative to the repository root: `.` for the root.
    pub directory: String,
    ecosystem: &'static dyn Ecosystem,
    /// The path of the manifest.
    pub manifest: String,
}

impl Package {
    /// The package as `meada export` writes it: its ecosystem named only when it is not npm's.
    pub(crate) fn written(self) -> meada_core::export::Package {
        let ecosystem = self.ecosystem.name();
        meada_core::export::Package {
            name: self.name,
            version: self.version,
            repository: self.repository,
            directory: self.directory,
            ecosystem: (ecosystem != Npm.name()).then(|| ecosystem.to_string()),
        }
    }

    /// The site of its manifest, which a dependency entry's link names as its provider.
    pub fn site(&self) -> String {
        site(&self.repository, &self.manifest)
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

/// The shape a package's index keeps for a name one of its files exports (issue #175): the
/// module-level constant or type `name` of the file `path`, read for the view `want`.
pub struct ExportedShape {
    pub path: String,
    pub name: String,
    pub want: Want,
    pub shape: Shape,
}

pub use meada_core::export::How;

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

pub use meada_core::export::{Dependency, Import};

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
    let root = tsconfig_root(src)?;
    let option = |name: &str| match root.get("compilerOptions")?.get(name)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    Some(Tsconfig {
        out_dir: option("outDir"),
        root_dir: option("rootDir"),
    })
}

/// The root object of a `tsconfig.json` or `jsconfig.json`, read from its bytes; `None` when the
/// file is invalid, as `read_tsconfig` says.
pub(crate) fn tsconfig_root(src: &[u8]) -> Option<Value> {
    if src.len() > MAX_BYTES {
        return None;
    }
    serde_json::from_str(&strip_jsonc(std::str::from_utf8(src).ok()?))
        .ok()
        .filter(Value::is_object)
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

/// Whether `name` can be a package name: not empty, at most `MAX_NAME` bytes, and no
/// control character, whitespace or `:` (which would break the `::` of a contract identifier).
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == ':')
}

/// The site of the manifest at `path` of `repository`.
fn site(repository: &str, path: &str) -> String {
    format!("{repository}::{path}")
}

/// The directory of the manifest at `path`: `.` for one at the root.
fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

/// The directory of the manifest at `path` as a repository path to join onto: the root is empty.
fn bare_directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Whether a Cargo or Go entry of source `via` may reach the provider whose manifest is
/// `provider` in the repository `to`, from a manifest of `from` (each repository by identity and
/// root): a `path` entry only the package at that path, a `git` one only the repository of that
/// remote; an entry with no source every provider. A path that leaves its repository (`../`) is
/// matched against the provider's location by the two roots, folded lexically: nothing is read.
fn sourced(
    via: Option<&str>,
    from: (&str, &Path),
    ((to, to_root), provider): ((&str, &Path), &str),
) -> bool {
    let Some(via) = via else {
        return true;
    };
    match via.split_once(' ') {
        Some(("path", manifest)) if manifest.starts_with("../") => {
            folded(&from.1.join(manifest)) == folded(&to_root.join(provider))
        }
        Some(("path", manifest)) => from.0 == to && provider == manifest,
        Some(("git", identity)) => to == identity,
        _ => false,
    }
}

/// `path` with its `.` and `..` folded without touching the file system, so a symlink never
/// changes where it lands.
fn folded(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    out
}

/// Every manifest of `ecosystem` whose directory holds `path`, the deepest directory first and
/// those of one directory by path (a `pyproject.toml` before the `requirements*.txt` beside it).
fn holding<'m>(
    manifests: &'m [Manifest],
    path: &str,
    ecosystem: &dyn Ecosystem,
) -> Vec<&'m Manifest> {
    let mut found: Vec<&Manifest> = (manifests.iter())
        .filter(|m| (m.path.rsplit('/').next()).is_some_and(|name| ecosystem.is_manifest(name)))
        .filter(|m| match directory(&m.path) {
            "." => true,
            dir => path
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/')),
        })
        .collect();
    found.sort_by_key(|m| {
        let depth = m.path.rsplit_once('/').map_or(0, |(dir, _)| dir.len() + 1);
        (std::cmp::Reverse(depth), &m.path)
    });
    found
}

/// The manifest of `ecosystem` in the deepest directory holding `path`.
fn nearest<'m>(
    manifests: &'m [Manifest],
    path: &str,
    ecosystem: &dyn Ecosystem,
) -> Option<&'m Manifest> {
    holding(manifests, path, ecosystem).into_iter().next()
}

/// The manifest of the package `path` belongs to: the nearest manifest of `ecosystem` that
/// bounds a package (`Ecosystem::owns`).
fn owner<'m>(
    manifests: &'m [Manifest],
    path: &str,
    ecosystem: &dyn Ecosystem,
) -> Option<&'m Manifest> {
    (holding(manifests, path, ecosystem).into_iter()).find(|m| ecosystem.owns(m))
}

/// The ways an import of `subpath` of the package at `manifest` may be followed, in the order to
/// try them, each with the files it may name (most likely first) and what named them: `exports`
/// when the manifest has it, the only way in; else, for the bare name, one for each of `types`,
/// `module` and `main` present; else the subpath as a path inside the package, with the trunk's
/// extension and `index` rules (`path`), the bare name being `./index`. Every file stays inside
/// the package directory. Node's rule, every ecosystem's unless it says otherwise
/// (`Ecosystem::ways`).
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
    /// The shapes the provider keeps for what its files export (issue #175), by the symbol-like
    /// identity of the constant or type (`<repository>::<path>::<name>`) and the view.
    shapes: HashMap<(String, Want), &'e Shape>,
    /// The source files that export something, by the path of the manifest that is their nearest
    /// in the ecosystem, none under a `node_modules` directory: one table per ecosystem of
    /// `ECOSYSTEMS`, in its order, built the first time an ambiguous import of that ecosystem
    /// asks (`candidates`).
    owned: [OnceCell<HashMap<&'e str, Vec<&'e str>>>; ECOSYSTEMS.len()],
    /// The Go source files that are no tests, by directory. Built the first time a Go import
    /// asks (`Go::resolve`), once for all.
    go_dirs: OnceCell<HashMap<&'e str, Vec<&'e str>>>,
    /// The public classes and methods of the Java files. Built the first time a Java import asks
    /// (`Maven::resolutions`), once for all.
    classes: OnceCell<maven::Classes<'e>>,
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
            shapes: (export.shapes.iter())
                .map(|k| {
                    let id = format!("{}::{}::{}", export.repository, k.path, k.name);
                    ((id, k.want), &k.shape)
                })
                .collect(),
            owned: Default::default(),
            go_dirs: OnceCell::new(),
            classes: OnceCell::new(),
        }
    }

    /// Whether `id` is a symbol of the provider: what a call through an import reaches.
    fn symbol(&self, id: &str) -> bool {
        self.symbols.contains(id)
    }

    /// Whether `path` is a source file the provider indexes (a declaration file is none).
    fn source(&self, path: &str) -> bool {
        self.files.contains(path) && !path.ends_with(".d.ts")
    }

    /// The source file the first of the ways `tried` that leads to one names, with how and at
    /// which level: directly `exact`, through the `outDir` mapping `inferred`.
    fn entry(
        &self,
        manifest: &Manifest,
        tried: &[(How, Vec<String>)],
    ) -> Option<(String, How, Level)> {
        tried.iter().find_map(|(how, files)| {
            if let Some(file) = files.iter().find(|f| self.owned_source(manifest, f)) {
                return Some((file.clone(), *how, Level::Exact));
            }
            Some((self.mapped(manifest, files)?, How::OutDir, Level::Inferred))
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
        of(&manifest.path).is_some_and(|ecosystem| {
            self.source(path)
                && owner(self.manifests, path, ecosystem)
                    .is_some_and(|m| directory(&m.path) == directory(&manifest.path))
        })
    }

    /// The symbols `chain` names in the source files of the package at `manifest` (those whose
    /// nearest manifest it is, `node_modules` ones left out) that export its head, sorted,
    /// without repeats. Every one is listed: a package has as many as it has files exporting
    /// the name.
    fn candidates(
        &self,
        repository: &str,
        manifest: &Manifest,
        chain: &str,
        known: &dyn Fn(&str) -> bool,
    ) -> Vec<String> {
        let head = chain.split('.').next();
        let Some(ecosystem) = of(&manifest.path) else {
            return Vec::new();
        };
        let at = (ECOSYSTEMS.iter())
            .position(|e| e.name() == ecosystem.name())
            .expect("`of` answers from ECOSYSTEMS");
        let owned = self.owned[at].get_or_init(|| {
            let mut owned: HashMap<&str, Vec<&str>> = HashMap::new();
            for &path in self.exported.keys() {
                if !self.source(path) || path.split('/').any(|s| s == "node_modules") {
                    continue;
                }
                if let Some(m) = nearest(self.manifests, path, ecosystem) {
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
            .filter_map(|path| self.reach(repository, path, chain, known))
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// The symbol `chain` names as exported by `file`: the head is what the file exports, and
    /// each further name a member of that definition or a name the whole file exports.
    fn reach(
        &self,
        repository: &str,
        file: &str,
        chain: &str,
        known: &dyn Fn(&str) -> bool,
    ) -> Option<String> {
        let mut names = chain.split('.');
        let mut at = self.member(file, names.next()?)?;
        for member in names {
            at = match at {
                (file, None) => self.member(&file, member)?,
                (file, Some(def)) => (file, Some(format!("{def}.{member}"))),
            };
        }
        let (file, Some(def)) = at else {
            return None;
        };
        let id = format!("{repository}::{file}::{def}");
        known(&id).then_some(id)
    }

    /// What `name` names as a member of the whole file `file`: what it exports (`lookup`), or,
    /// for a Python package's `__init__.py` that exports no such name, its submodule `name`
    /// (`pkg/name.py` or `pkg/name/__init__.py`), as `import pkg` then `pkg.name.f()` reaches it.
    fn member(&self, file: &str, name: &str) -> Option<(String, Option<String>)> {
        self.lookup(file, name, 0, &mut HashSet::new()).or_else(|| {
            let dir = match file {
                "__init__.py" => "",
                file => file.strip_suffix("/__init__.py")?,
            };
            let module = python::files(dir, name);
            let found = module
                .into_iter()
                .find(|f| self.files.contains(f.as_str()))?;
            Some((found, None))
        })
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

/// The providers of each package of the workspace, by ecosystem name and package name, so two
/// ecosystems never link, as positions: the
/// repository in `exports`, then its manifest in `Export::manifests`. Built once for the
/// workspace steps (`consume`, `import`), both of which only add to the exports and never move
/// a manifest.
struct Providers(HashMap<(&'static str, String), Vec<(usize, usize)>>);

impl Providers {
    fn new(exports: &[Export]) -> Self {
        let mut providers: HashMap<_, Vec<(usize, usize)>> = HashMap::new();
        for (n, export) in exports.iter().enumerate() {
            for (i, m) in export.manifests.iter().enumerate() {
                if let (Some(name), Some(ecosystem)) = (&m.summary.name, of(&m.path)) {
                    let key = (ecosystem.name(), name.clone());
                    providers.entry(key).or_default().push((n, i));
                }
            }
        }
        Self(providers)
    }

    /// Providers of a literal name, or candidates of an uncertain coordinate, sorted.
    fn matching<E: Ecosystem + ?Sized>(&self, ecosystem: &E, entry: &Entry) -> Vec<(usize, usize)> {
        if ecosystem.uncertainty(entry.via.as_deref()).is_none() {
            return self.of(ecosystem.name(), &entry.name).to_vec();
        }
        let name = &entry.name;
        let mut found: Vec<_> = self
            .0
            .iter()
            .filter(|((kind, provided), _)| {
                *kind == ecosystem.name() && ecosystem.fits(name, provided)
            })
            .flat_map(|(_, locations)| locations.iter().copied())
            .collect();
        found.sort();
        found
    }

    /// The providers of the package `name` of the ecosystem named `ecosystem`.
    fn of(&self, ecosystem: &'static str, name: &str) -> &[(usize, usize)] {
        (self.0.get(&(ecosystem, name.to_string()))).map_or(&[], Vec::as_slice)
    }
}

/// The workspace step after `consume`: each call through a package import, whose package the
/// importing file's nearest manifest (PyPI: any above it) depends on, in any field, and some
/// repository of the workspace provides (a `workspace:` range: its own), resolves in each such
/// provider (`resolutions`). A call that resolves in one at least becomes a consumer of the
/// package at its site, the range of the entry in `Consumer::method` and where it resolves in
/// `Consumer::hops` (`Packages::link` derives its links), and adds an `imports` row per
/// provider; a Python import several depended-on packages expose is a consumer of each. Any
/// other, an import of a package outside the workspace included, adds nothing.
fn import(exports: &mut [Export], providers: &Providers) {
    // Built for a provider the first time an import reaches it.
    let provided: Vec<OnceCell<Provided>> = exports.iter().map(|_| OnceCell::new()).collect();
    let mut found = Vec::new();
    for (n, export) in exports.iter().enumerate() {
        let mut consumers = Vec::new();
        let mut imports = Vec::new();
        for call in &export.import_calls {
            let (from, chain) = ((n, call.path.as_str()), call.chain.as_str());
            let symbol = |at: &Provided, id: &str| at.symbol(id);
            let Some(ecosystem) = of_source(&call.path) else {
                continue;
            };
            for (entry, resolved) in resolutions(
                exports,
                providers,
                &provided,
                from,
                &call.specifier,
                chain,
                symbol,
            ) {
                let mut hops = Vec::new();
                for (
                    m,
                    manifest,
                    _,
                    Resolved {
                        how,
                        level,
                        targets,
                    },
                ) in resolved
                {
                    let repository = &exports[m].repository;
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
                        contract: Some(contract(ecosystem, &entry.name)),
                        site: call.site.clone(),
                        via: entry.via.clone(),
                        shape: None,
                        method: entry.range.clone(),
                        fragments: Vec::new(),
                        hops,
                    });
                }
            }
        }
        found.push((consumers, imports));
    }
    for (export, (consumers, mut imports)) in exports.iter_mut().zip(found) {
        export.consumers.extend(consumers);
        index::sort_consumers(&mut export.consumers);
        imports.sort_by(|a, b| (&a.site, &a.target).cmp(&(&b.site, &b.target)));
        // Several entries of one package (a computed coordinate) reach the same symbol.
        imports.dedup();
        export.imports = imports;
    }
}

/// The workspace step after `import` (issue #175): every provider and consumer shape with a part
/// a package import gives has it filled (`shape::fill`) from the shape the package's index keeps
/// for the imported name (`kept`), so an edit to the package changes its importers' drift at the
/// next query. A part that finds none is open on a provider and adds nothing on a consumer.
fn shapes(exports: &mut [Export], providers: &Providers) {
    let provided: Vec<OnceCell<Provided>> = exports.iter().map(|_| OnceCell::new()).collect();
    let all: &[Export] = exports;
    let mut find = |n, import: &shape::Import| kept(all, providers, &provided, n, import);
    // Each side with imports, filled: (repository, position, shape).
    let (mut provided_by, mut consumed_by) = (Vec::new(), Vec::new());
    for (n, export) in all.iter().enumerate() {
        let mut fill = |side: &Option<Shape>| {
            let s = side.as_ref().filter(|s| !s.imports.is_empty())?;
            Some(shape::fill(s, n, &mut find))
        };
        for (i, p) in export.providers.iter().enumerate() {
            provided_by.extend(fill(&p.shape).map(|s| (n, i, s)));
        }
        for (i, c) in export.consumers.iter().enumerate() {
            consumed_by.extend(fill(&c.shape).map(|s| (n, i, s)));
        }
    }
    for (n, i, s) in provided_by {
        exports[n].providers[i].shape = Some(s);
    }
    // A consumer whose imports added nothing reads nothing, as one that never had any.
    for (n, i, s) in consumed_by {
        exports[n].consumers[i].shape = (!s.fields.is_empty()).then_some(s);
    }
}

/// The shape the name `import` names keeps in the package it imports from, as the repository
/// `n` reaches it: the package the nearest manifest of the importing file depends on, resolved in
/// each repository providing it (`Ecosystem::resolve`, the rules of a call through an import) to
/// a constant or type that keeps a shape for the import's view, with that repository and the
/// level it resolved at (`inferred` through an `outDir` mapping). `None`
/// when no provider resolves it, when one resolves it `ambiguous` or to several names, or when
/// two give it different shapes.
fn kept<'e>(
    exports: &'e [Export],
    providers: &Providers,
    provided: &'e [OnceCell<Provided<'e>>],
    n: usize,
    import: &shape::Import,
) -> Option<(usize, &'e Shape, Level)> {
    let want = import.want.kept();
    let from = (n, import.from.as_str());
    let known = |at: &Provided, id: &str| at.shapes.contains_key(&(id.to_string(), want));
    let resolved = resolutions(
        exports,
        providers,
        provided,
        from,
        &import.specifier,
        &import.name,
        known,
    );
    let mut found: Option<(usize, &Shape, Level)> = None;
    for (m, _, at, resolved) in resolved.into_iter().flat_map(|(_, r)| r) {
        let [target] = resolved.targets.as_slice() else {
            return None;
        };
        if resolved.level == Level::Ambiguous {
            return None;
        }
        let shape = at.shapes[&(target.clone(), want)];
        found = match found {
            None => Some((m, shape, resolved.level)),
            Some((_, s, _)) if s != shape => return None,
            // The same shape from two providers is as certain as the weaker resolution.
            Some((first, s, level)) => Some((first, s, level.max(resolved.level))),
        };
    }
    found
}

/// A repository providing a package (its position), its manifest and its `Provided`, and where
/// a name imported from the package resolves there.
type Resolution<'e> = (usize, &'e Manifest, &'e Provided<'e>, Resolved);

/// Where the name `chain`, which the file `from.1` of the repository `from.0` imports from
/// `specifier`, resolves, by the rules of the file's ecosystem (`of_source`,
/// `Ecosystem::resolutions`): each dependency entry the import may name, and each repository
/// providing its package that the entry reaches and where the name resolves there, with that
/// repository's position, its manifest and its `Provided`. `known` says which ids of a
/// provider the name may resolve to. Empty when the import names no dependency of the file, or
/// the file is of no ecosystem.
fn resolutions<'e>(
    exports: &'e [Export],
    providers: &Providers,
    provided: &'e [OnceCell<Provided<'e>>],
    from: (usize, &str),
    specifier: &str,
    chain: &str,
    known: impl Fn(&Provided<'e>, &str) -> bool,
) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
    let Some(ecosystem) = of_source(from.1) else {
        return Vec::new();
    };
    let at = |m: usize| provided[m].get_or_init(|| Provided::new(&exports[m]));
    ecosystem.resolutions(exports, providers, &at, from, (specifier, chain), &known)
}

/// The files of a repository an import of one of its packages can reach (issue #175), so the
/// ones whose exported constants and types keep a shape: for each named manifest, the entry
/// file (`Provided::entry`, as an import resolves it) of the bare name and of each subpath its
/// `exports` lists without a `*`, then every file the `exports` rows of those name, a definition's,
/// a constant's or a re-exported one's, in turn.
fn surface(
    manifests: &[Manifest],
    configs: &[Config],
    exported: &[Exported],
    files: &[&str],
) -> HashSet<String> {
    let mut by_file: HashMap<&str, Vec<&Exported>> = HashMap::new();
    for e in exported {
        by_file.entry(&e.path).or_default().push(e);
    }
    let provided = Provided {
        files: files.iter().copied().collect(),
        symbols: HashSet::new(),
        exported: by_file,
        manifests,
        configs,
        shapes: HashMap::new(),
        owned: Default::default(),
        go_dirs: OnceCell::new(),
        classes: OnceCell::new(),
    };
    let mut queue = Vec::new();
    // Shapes cross through the packages of an ecosystem that says so (`Ecosystem::shapes`).
    let crossing = (manifests.iter())
        .filter_map(|m| Some((m, of(&m.path).filter(|e| e.shapes())?)))
        .filter(|(m, _)| m.summary.name.is_some());
    for (m, ecosystem) in crossing {
        let subpaths: Vec<String> = match &m.summary.exports {
            Some(Value::Object(keys)) if keys.keys().any(|k| k.starts_with('.')) => (keys.keys())
                .filter(|k| k.starts_with('.') && !k.contains('*'))
                .cloned()
                .collect(),
            _ => vec![".".to_string()],
        };
        for subpath in subpaths {
            let tried = ecosystem.ways(m, &subpath);
            queue.extend(provided.entry(m, &tried).map(|(file, ..)| file));
        }
    }
    let mut seen = HashSet::new();
    while let Some(file) = queue.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let rows = provided.exported.get(file.as_str()).into_iter().flatten();
        queue.extend(rows.filter_map(|e| e.file.clone()));
    }
    seen
}

/// The shapes a repository keeps for what the files of its packages' `surface` export (issue
/// #175), as `Role::Shape` rows: for each module-level constant or type such a file exports, the declared
/// paths of an interface or type alias (`Want::Type`, `declared.rs`), the output and the
/// `z.infer` paths of a zod schema (`Want::Zod` and `Want::Infer`, `zod.rs`), or else the fields
/// of an object or array literal (`Want::Literals`, `literals.rs`). Each source's rules stay in
/// its own module; a name none of them reads keeps nothing.
fn exported_shapes(code: &Code, surface: &HashSet<String>) -> Vec<Found> {
    // One reader of each source, the types and zod schemas through `literals`: each budget per
    // file once.
    let mut literals = Literals::default();
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        if !surface.contains(path) {
            continue;
        }
        for export in &facts.exports {
            let crate::lang::Export::Local { local, .. } = export else {
                continue;
            };
            if local.contains('.') || !facts.consts.iter().any(|c| c.name == *local) {
                continue;
            }
            let mut kept = Vec::new();
            kept.extend((literals.types().exported(code, path, local)).map(|s| (Want::Type, s)));
            match literals.types().zod().exported(code, path, local) {
                Some((sent, typed)) => kept.extend([(Want::Zod, sent), (Want::Infer, typed)]),
                None => kept.extend(
                    literals
                        .exported(code, path, local)
                        .map(|s| (Want::Literals, s)),
                ),
            }
            for (want, shape) in kept {
                found.push(Found {
                    role: Role::Shape,
                    site: Site::File(path.to_string()),
                    contract: None,
                    via: Some(want.name().to_string()),
                    method: Some(local.clone()),
                    fragments: Vec::new(),
                    shape: Some(shape),
                });
            }
        }
    }
    found
}

/// The workspace step that follows every repository's `declare`: for each dependency entry
/// that names a package some repository of the workspace provides, a consumer and a
/// `dependencies` row. An entry whose package is outside the workspace, or a `workspace:` one
/// whose package is outside its own repository, adds nothing.
fn consume(exports: &mut [Export], providers: &Providers) {
    let found: Vec<_> = (exports.iter().enumerate())
        .map(|(n, export)| {
            let mut consumers = Vec::new();
            let mut dependencies = Vec::new();
            for (i, manifest) in export.manifests.iter().enumerate() {
                let Some(ecosystem) = of(&manifest.path) else {
                    continue;
                };
                for e in &manifest.summary.dependencies {
                    let source = (e.via.as_deref(), e.range.as_deref());
                    // A manifest is never a candidate of its own dependency.
                    let mut of = providers.matching(ecosystem, e);
                    of.retain(|&p| p != (n, i));
                    let reaches = |&&(m, i): &&(usize, usize)| {
                        let at = &exports[m].manifests[i];
                        ecosystem.reaches(source, export.place(), (exports[m].place(), &at.path))
                    };
                    if !of.iter().any(|p| reaches(&p)) {
                        continue;
                    }
                    consumers.push(Consumer {
                        contract: Some(contract(ecosystem, &e.name)),
                        site: format!(
                            "{}::{}::{}::{}",
                            export.repository,
                            manifest.path,
                            e.field,
                            e.key()
                        ),
                        via: e.via.clone(),
                        shape: None,
                        method: e.range.clone(),
                        fragments: Vec::new(),
                        hops: Vec::new(),
                    });
                    dependencies.push(Dependency {
                        manifest: manifest.path.clone(),
                        field: e.field.clone(),
                        name: e.name.clone(),
                        alias: e.alias.clone(),
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
            (&a.manifest, &a.field, &a.name, &a.alias).cmp(&(
                &b.manifest,
                &b.field,
                &b.name,
                &b.alias,
            ))
        });
        export.dependencies = dependencies;
    }
}

/// What checking a dependency range against the provider's version found, when it is not a
/// plain accept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
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

/// What a dependency `range` on the package `contract` says of the provider's `version`, by
/// the contract's ecosystem (`Ecosystem::flag`).
fn flag_of(contract: &str, range: Option<&str>, version: Option<&str>) -> Option<Flag> {
    of_contract(contract)?.flag(range, version)
}

/// The reason of a package link: `<identifier> <range|none> (local <version|none>)`, then
/// `, range mismatch` or `, range unchecked` when `flag_of` says so. The one place the package
/// reason is built; a path that extends it (an import hop) builds on this.
fn reason(contract: &str, range: Option<&str>, version: Option<&str>) -> String {
    let said = format!(
        "{contract} {} (local {})",
        range.unwrap_or("none"),
        version.unwrap_or("none")
    );
    match flag_of(contract, range, version) {
        Some(f) => format!("{said}, {}", f.words()),
        None => said,
    }
}

/// A call that reaches a symbol through a package import, for `context`.
struct Reach<'e> {
    /// The position in `exports` of the repository holding the call.
    pub repository: usize,
    pub call: &'e CallSite,
    pub level: Level,
    /// What the dependency's range says of the provider's version, when not a plain accept.
    pub flag: Option<Flag>,
}

/// The calls that reach `symbol` through a package import, from `links` (the workspace's): each
/// with the link's level and the range flag of the dependency entry it went through.
fn reached<'e>(exports: &'e [Export], links: &[Link], symbol: &str) -> Vec<Reach<'e>> {
    let mut found = Vec::new();
    for link in links
        .iter()
        .filter(|l| l.provider == symbol && l.contract.starts_with(PREFIX))
    {
        let Some((repository, consumer)) = (exports.iter().enumerate())
            .flat_map(|(n, e)| e.consumers.iter().map(move |c| (n, c)))
            .find(|(_, c)| c.site == link.consumer && c.contract.as_ref() == Some(&link.contract))
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
            flag: flag_of(&link.contract, consumer.method.as_deref(), version),
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

    /// A manifest provider (`<repository>::<path>`) sits at `<path>`, a dependency entry's
    /// consumer (`<repository>::<path>::<field>::<name>`) at `<path> <field>`.
    fn place(&self, repository: &str, site: &str) -> Option<String> {
        let rest = site.strip_prefix(repository)?.strip_prefix("::")?;
        let parts: Vec<&str> = rest.split("::").collect();
        match parts[..] {
            [path] if of(path).is_some() => Some(path.to_string()),
            [path, field, _] if of(path).is_some() => Some(format!("{path} {field}")),
            _ => None,
        }
    }

    fn search_word(&self) -> Search {
        Search::Own("package")
    }

    /// The packages whose name matches `needle`, one line each, as `<repository> <directory>
    /// <name>@<version> (consumers: <n>)`, the version left out when the manifest has none. The
    /// consumers are the dependency entries linked to the package, at any level. An npm package
    /// is named alone, as before PyPI's: any other says its ecosystem.
    fn own_search(&self, exports: &[Export], links: &[Link], needle: &str) -> Vec<Hit> {
        let mut entries: HashMap<&str, usize> = HashMap::new();
        for link in links {
            *entries.entry(&link.provider).or_default() += 1;
        }
        let mut hits = Vec::new();
        for p in exports.iter().flat_map(|e| &e.packages) {
            let Some(r) = rank(&p.name.to_lowercase(), needle) else {
                continue;
            };
            let name = match &p.version {
                Some(version) => format!("{}@{version}", p.name),
                None => p.name.clone(),
            };
            let n = entries.get(p.site().as_str()).copied().unwrap_or(0);
            let name = match p.ecosystem.name() == Npm.name() {
                true => name,
                false => format!("{name} ({})", p.ecosystem.name()),
            };
            let line = format!("{} {} {name} (consumers: {n})", p.repository, p.directory);
            hits.push((r, format!("{} {}", p.repository, p.directory), line));
        }
        hits
    }

    /// Several providers of a package fit a dependency entry, but no call is there to pick
    /// one for.
    fn decidable(&self) -> bool {
        false
    }

    /// Every call through a binding imported from a bare specifier, as an `Import`: which of
    /// them name a package of the workspace is known at query time only (`import`). After the
    /// imports it builds the public `surface` from `indexed` (the manifests, tsconfigs, exported
    /// rows and files) and, when the surface is not empty, appends the `exported_shapes` of the
    /// pack's `Code`.
    fn extract(&self, code: &Code, indexed: &Indexed) -> Vec<Found> {
        let mut found = Vec::new();
        // A file of no ecosystem imports no package.
        let files = (code.files().iter())
            .filter_map(|(&path, facts)| Some((path, facts, of_source(path)?)));
        for (path, facts, ecosystem) in files {
            for (index, call) in facts.calls.iter().enumerate() {
                for (specifier, chain) in ecosystem.imports(code, path, call) {
                    if chain.is_empty() || !ecosystem.names_package(&specifier) {
                        continue;
                    }
                    found.push(Found {
                        role: Role::Import,
                        site: Site::Call {
                            path: path.to_string(),
                            index,
                        },
                        contract: None,
                        via: Some(specifier),
                        method: Some(chain),
                        fragments: Vec::new(),
                        shape: None,
                    });
                }
            }
        }
        // The files an import of one of the repository's packages reaches: only they keep shapes.
        let surface = surface(
            indexed.manifests,
            indexed.tsconfigs,
            indexed.exported,
            indexed.files,
        );
        if !surface.is_empty() {
            found.extend(exported_shapes(code, &surface));
        }
        found
    }

    /// One provider and one `packages` row per manifest with a valid name.
    fn declare(&self, repo: &Repository, export: &mut Export) {
        let mut packages = Vec::new();
        for m in &export.manifests {
            let (Some(name), Some(ecosystem)) = (&m.summary.name, of(&m.path)) else {
                continue;
            };
            export.providers.push(Provider {
                contract: contract(ecosystem, name),
                site: site(&repo.identity, &m.path),
                version: m.summary.version.clone(),
                ..Provider::default()
            });
            packages.push(Package {
                name: name.clone(),
                version: m.summary.version.clone(),
                repository: repo.identity.clone(),
                directory: directory(&m.path).to_string(),
                ecosystem,
                manifest: m.path.clone(),
            });
        }
        packages.sort_by(|a, b| (&a.name, &a.directory).cmp(&(&b.name, &b.directory)));
        export.packages = packages;
        index::sort_providers(&mut export.providers);
    }

    /// The steps that need every repository's packages: the dependency entries, the imports
    /// (which resolve into a provider wherever it lives), the export entry points a call
    /// reaches through a package import of their own repository (a monorepo: no door, at any
    /// level; an entry point of another kind stays one), and the shapes imported from a
    /// package (issue #175).
    fn workspace_step(&self, exports: &mut [Export]) {
        let providers = Providers::new(exports);
        consume(exports, &providers);
        import(exports, &providers);
        for export in exports.iter_mut() {
            let own = format!("{}::", export.repository);
            let imported: HashSet<&str> = (export.imports.iter())
                .map(|i| i.target.as_str())
                .filter(|t| t.starts_with(&own))
                .collect();
            entry::drop_imported_exports(&mut export.entry_points, &imported);
        }
        shapes(exports, &providers);
    }

    /// `consumers:` with the calls that reach `symbol` through a package import (spec #22),
    /// from any repository of the workspace, as `<repository> <path>:<line> <caller>`, then
    /// ` [<level>, <range flag>]` when the link is not exact or the dependency's range does not
    /// plainly accept the package's version. Nothing when there are none.
    fn context_sections(
        &self,
        exports: &[Export],
        repos: &[Repository],
        links: &[Link],
        symbol: &str,
    ) -> Vec<String> {
        let mut found = reached(exports, links, symbol);
        found.sort_by_key(|r| {
            (
                &repos[r.repository].identity,
                &r.call.span.path,
                r.call.span.start,
            )
        });
        let lines: Vec<String> = found
            .iter()
            .map(|r| {
                let repo = &repos[r.repository];
                let location = Files::new(&repo.root).at(&r.call.span.path, r.call.span.start);
                let caller = &r.call.caller;
                let caller = match exports[r.repository]
                    .symbols
                    .iter()
                    .any(|s| s.id == *caller)
                {
                    true => label(caller),
                    false => "(top level)",
                };
                let level = (r.level != Level::Exact).then(|| r.level.name());
                let flag = r.flag.map(Flag::words);
                let tags: Vec<&str> = level.into_iter().chain(flag).collect();
                let tags = match tags.is_empty() {
                    true => String::new(),
                    false => format!(" [{}]", tags.join(", ")),
                };
                format!("{} {location} {caller}{tags}", repo.identity)
            })
            .collect();
        match lines.is_empty() {
            true => lines,
            false => section("consumers", lines),
        }
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
        _exports: &[Export],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        let range = consumer.method.as_deref();
        let Some(ecosystem) = of_contract(contract) else {
            return Vec::new();
        };
        let source = (consumer.via.as_deref(), range);
        let fits: Vec<_> = (providers.iter())
            // A manifest is never a candidate of its own dependency.
            .filter(|(_, p)| !consumer.site.starts_with(&format!("{}::", p.site)))
            .filter(|(repo, p)| {
                let at = (p.site.strip_prefix(&repo.identity)).and_then(|s| s.strip_prefix("::"));
                let (from, to) = (
                    (&*from.identity, &*from.root),
                    (&*repo.identity, &*repo.root),
                );
                p.contract
                    .strip_prefix(&ecosystem.prefix())
                    .is_some_and(|provided| {
                        let wanted = contract.strip_prefix(&ecosystem.prefix()).unwrap();
                        ecosystem.fits(wanted, provided)
                    })
                    && at.is_some_and(|manifest| ecosystem.reaches(source, from, (to, manifest)))
            })
            .collect();
        let uncertain = ecosystem.uncertainty(consumer.via.as_deref());
        let (level, n) = match fits.len() {
            0 => return Vec::new(),
            1 if uncertain.is_none() => (Level::Exact, None),
            1 => (Level::Ambiguous, None),
            n => (Level::Ambiguous, Some(n)),
        };
        fits.into_iter()
            .flat_map(|(_, p)| {
                let said = reason(contract, range, p.version.as_deref());
                let said = match uncertain {
                    Some(why) => format!("{why}; {said}"),
                    None => said,
                };
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
    use crate::index::Span;
    use std::path::PathBuf;

    #[test]
    fn an_ambiguous_package_link_is_no_call_for_a_person() {
        assert!(!Packages.decidable());
    }

    /// A symbol a package import reaches lists the calls that reach it as `consumers:`, with
    /// the range flag of the dependency entry; a symbol nothing reaches adds no section.
    #[test]
    fn a_symbol_reached_through_a_package_import_gets_a_consumers_section() {
        let contract = "package::npm::kit";
        let call = "r::web/app.ts::go#0";
        let export = Export {
            repository: "r".into(),
            consumers: vec![Consumer {
                contract: Some(contract.into()),
                site: call.into(),
                via: None,
                shape: None,
                method: Some("^1.0.0".into()),
                fragments: Vec::new(),
                hops: Vec::new(),
            }],
            calls: vec![CallSite {
                id: call.into(),
                caller: "r::web/app.ts::go".into(),
                target: None,
                level: None,
                candidates: Vec::new(),
                narrowed: false,
                span: Span {
                    path: "web/app.ts".into(),
                    start: 0,
                    end: 0,
                },
            }],
            ..Export::default()
        };
        let link = Link {
            consumer: call.into(),
            provider: "r::kit/index.ts::greet".into(),
            contract: contract.into(),
            level: Level::Exact,
            reason: format!("{contract} ^1.0.0 (local none)"),
            by: None,
            p: None,
            drift: None,
        };
        let repo = Repository {
            root: "/nonexistent".into(),
            identity: "r".into(),
            config: Default::default(),
            decisions: Default::default(),
            index: "".into(),
        };
        let (exports, repos, links) = ([export], [repo], [link]);
        let sections = |symbol| Packages.context_sections(&exports, &repos, &links, symbol);
        assert_eq!(
            sections("r::kit/index.ts::greet"),
            ["consumers:", "r web/app.ts (top level) [range unchecked]"]
        );
        assert!(sections("r::kit/index.ts::other").is_empty());
    }

    #[test]
    fn packages_render_their_own_search_under_the_package_word() {
        assert_eq!(Packages.search_word(), Search::Own("package"));
    }

    #[test]
    fn the_own_search_lists_a_package_by_name_with_the_entries_linked_to_it() {
        let package = |name: &str, ecosystem: &'static dyn Ecosystem| Package {
            name: name.into(),
            version: Some("1.0.0".into()),
            repository: "r".into(),
            directory: format!("{name}-dir"),
            ecosystem,
            manifest: format!("{name}-dir/manifest"),
        };
        let export = Export {
            packages: vec![
                package("@a/Kit", &Npm),
                package("kit-py", &Pypi),
                package("other", &Npm),
            ],
            ..Export::default()
        };
        let link = Link {
            consumer: "r::web/package.json::dependencies::@a/Kit".into(),
            provider: export.packages[0].site(),
            contract: "package::npm::@a/Kit".into(),
            level: Level::Exact,
            reason: "x".into(),
            by: None,
            p: None,
            drift: None,
        };
        let hits = Packages.own_search(&[export], &[link], "kit");
        assert_eq!(
            hits,
            [
                (
                    2,
                    "r @a/Kit-dir".into(),
                    "r @a/Kit-dir @a/Kit@1.0.0 (consumers: 1)".into()
                ),
                (
                    1,
                    "r kit-py-dir".into(),
                    "r kit-py-dir kit-py@1.0.0 (pypi) (consumers: 0)".into()
                ),
            ]
        );
    }

    #[test]
    fn a_package_site_shows_its_manifest_and_field() {
        let repo = "example.com/a";
        let at = |site: &str| Packages.place(repo, site);
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

    fn manifest(path: &str, json: &str) -> Manifest {
        Manifest {
            path: path.to_string(),
            summary: read_manifest(path, json.as_bytes()),
        }
    }

    #[test]
    fn the_surface_is_what_the_entries_of_named_packages_lead_to() {
        let manifests = [
            manifest("package.json", r#"{"private": true}"#),
            manifest(
                "pkg/package.json",
                r#"{"name": "p", "main": "src/index.ts"}"#,
            ),
            manifest(
                "sub/package.json",
                r#"{"name": "s", "exports": {".": "./a.ts", "./extra": "./b.ts", "./*": "./c/*.ts"}}"#,
            ),
        ];
        let rows = [
            row(
                "pkg/src/index.ts",
                Some("A"),
                Some("pkg/src/a.ts"),
                Some("A"),
            ),
            row("pkg/src/index.ts", None, Some("pkg/src/all.ts"), None),
            row("pkg/src/all.ts", Some("B"), Some("pkg/src/b.ts"), Some("B")),
            row(
                "pkg/src/hidden.ts",
                Some("H"),
                Some("pkg/src/hidden.ts"),
                Some("H"),
            ),
            row("app.ts", Some("X"), Some("app.ts"), Some("X")),
        ];
        let files = [
            "app.ts",
            "pkg/src/index.ts",
            "pkg/src/a.ts",
            "pkg/src/all.ts",
            "pkg/src/b.ts",
            "pkg/src/hidden.ts",
            "sub/a.ts",
            "sub/b.ts",
            "sub/c/d.ts",
        ];
        let surface = surface(&manifests, &[], &rows, &files);
        let mut got: Vec<&str> = surface.iter().map(String::as_str).collect();
        got.sort();
        // A file no entry leads to, one of an unnamed package and a `*` subpath's are left out.
        let expected = [
            "pkg/src/a.ts",
            "pkg/src/all.ts",
            "pkg/src/b.ts",
            "pkg/src/index.ts",
            "sub/a.ts",
            "sub/b.ts",
        ];
        assert_eq!(got, expected);
    }

    /// The package kind's `extract` over an in-memory TypeScript repository: the import rows,
    /// then the shape of a constant a named package's entry file exports, none for a file
    /// outside the surface, and the import rows alone when no manifest names a package.
    #[test]
    fn extract_adds_the_shapes_of_the_surface_after_the_imports() {
        use crate::lang::{LanguagePack, ts::TsJs};
        use crate::resolve::Resolver;
        use std::collections::BTreeMap;

        let sources = [
            ("app.ts", "import { go } from 'lib';\ngo();\n"),
            ("pkg/src/index.ts", "export const Limits = { max: 3 };\n"),
            ("pkg/src/hidden.ts", "export const Secret = { key: 1 };\n"),
        ];
        let facts: Vec<_> = (sources.iter())
            .map(|(path, src)| TsJs.parse(path, src.as_bytes()).unwrap())
            .collect();
        let files: BTreeMap<&str, &_> = (sources.iter().map(|(p, _)| *p)).zip(&facts).collect();
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let code = Code::new(&resolver, &files, crate::lang::pack(0));
        let exported = resolver.exported();
        let names: Vec<&str> = files.keys().copied().collect();
        let answer = |manifests: &[Manifest]| {
            let indexed = Indexed {
                manifests,
                tsconfigs: &[],
                exported: &exported,
                files: &names,
                configs: &[],
            };
            let found = Packages.extract(&code, &indexed);
            let mut rows: Vec<_> = (found.iter())
                .map(|f| (f.role == Role::Shape, f.method.clone().unwrap_or_default()))
                .collect();
            rows.sort();
            rows
        };
        let named = [manifest(
            "pkg/package.json",
            r#"{"name": "p", "main": "src/index.ts"}"#,
        )];
        let import = (false, "go".to_string());
        assert_eq!(
            answer(&named),
            [import.clone(), (true, "Limits".to_string())]
        );
        assert_eq!(answer(&[]), [import]);
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
    fn the_nearest_manifest_is_in_the_deepest_directory_holding_the_file() {
        let all = [
            manifest("package.json", "{}"),
            manifest("packages/a/package.json", "{}"),
            manifest("packages/ab/package.json", "{}"),
        ];
        let of = |path| nearest(&all, path, &Npm).map(|m| m.path.as_str());
        assert_eq!(of("packages/a/src/x.ts"), Some("packages/a/package.json"));
        assert_eq!(of("packages/abc/x.ts"), Some("package.json"));
        assert_eq!(of("x.ts"), Some("package.json"));
        assert!(nearest(&all[1..], "x.ts", &Npm).is_none());
    }

    #[test]
    fn a_python_file_depends_on_every_manifest_above_it_and_belongs_to_its_pyproject() {
        let pypi = |path: &str| Manifest {
            path: path.to_string(),
            summary: Summary::default(),
        };
        let all = [
            pypi("pyproject.toml"),
            pypi("requirements.txt"),
            pypi("app/requirements-dev.txt"),
            pypi("app/pyproject.toml"),
            manifest("app/web/package.json", "{}"),
            pypi("tools/requirements.txt"),
        ];
        let holding = |path, e: &dyn Ecosystem| -> Vec<&str> {
            (holding(&all, path, e).into_iter())
                .map(|m| m.path.as_str())
                .collect()
        };
        // Every manifest of the ecosystem above the file, the deepest directory first.
        assert_eq!(
            holding("app/web/x.py", &Pypi),
            [
                "app/pyproject.toml",
                "app/requirements-dev.txt",
                "pyproject.toml",
                "requirements.txt"
            ]
        );
        assert_eq!(
            holding("tools/x.py", &Pypi),
            [
                "tools/requirements.txt",
                "pyproject.toml",
                "requirements.txt"
            ]
        );
        // A manifest of another ecosystem, deeper or not, is not the file's.
        assert_eq!(holding("app/web/x.ts", &Npm), ["app/web/package.json"]);
        assert!(holding("x.ts", &Npm).is_empty());
        // A requirements file names no package: the file belongs to the nearest
        // `pyproject.toml`, and to the nearest `package.json` for npm.
        let owner = |path, e: &dyn Ecosystem| owner(&all, path, e).map(|m| m.path.as_str());
        assert_eq!(owner("tools/x.py", &Pypi), Some("pyproject.toml"));
        assert_eq!(owner("app/x.py", &Pypi), Some("app/pyproject.toml"));
        assert_eq!(owner("app/web/x.ts", &Npm), Some("app/web/package.json"));
    }

    /// The name of the ecosystem the seam answers, for comparing answers.
    fn named(e: Option<&dyn Ecosystem>) -> Option<&'static str> {
        e.map(|e| e.name())
    }

    /// A stand-in adapter: `programs` answers `scripts`, `inherit` appends its name to the
    /// path of the first manifest, so the order the seam runs the adapters in is observable.
    struct Stand(&'static str, &'static [(&'static str, &'static str)]);

    impl Ecosystem for Stand {
        fn name(&self) -> &'static str {
            self.0
        }
        fn is_manifest(&self, _file_name: &str) -> bool {
            false
        }
        fn read(&self, _path: &str, _src: &[u8]) -> Summary {
            Summary::default()
        }
        fn sources(&self) -> &'static [&'static str] {
            &[]
        }
        fn names_package(&self, _specifier: &str) -> bool {
            false
        }
        fn entries<'e>(
            &self,
            _exports: &'e [Export],
            _providers: &Providers,
            _from: (usize, &str),
            _specifier: &str,
        ) -> Vec<(&'e Entry, String)> {
            Vec::new()
        }
        fn programs(&self, _manifests: &[Manifest], _files: &[&str]) -> Vec<(String, String)> {
            (self.1.iter())
                .map(|(file, def)| (file.to_string(), def.to_string()))
                .collect()
        }
        fn inherit(&self, manifests: &mut [Manifest]) {
            manifests[0].path.push_str(self.0);
        }
    }

    fn read_at(path: &str, src: &str) -> Manifest {
        Manifest {
            path: path.to_string(),
            summary: read_manifest(path, src.as_bytes()),
        }
    }

    #[test]
    fn the_programs_of_every_adapter_come_sorted_without_repeats() {
        let first = Stand("a", &[("z.py", "main"), ("b.py", "run")]);
        let second = Stand("b", &[("b.py", "run"), ("a.py", "go")]);
        let found = programs_of(&[&first, &second], &[], &[]);
        let pair = |f: &str, d: &str| (f.to_string(), d.to_string());
        assert_eq!(
            found,
            [
                pair("a.py", "go"),
                pair("b.py", "run"),
                pair("z.py", "main")
            ]
        );
    }

    #[test]
    fn the_seam_finds_the_console_scripts_of_the_pypi_manifests() {
        let manifests = [
            read_at("web/package.json", r#"{"name": "web"}"#),
            read_at(
                "pyproject.toml",
                "[project]\nname = \"acme\"\n[project.scripts]\nacme = \"acme.cli:main\"\n",
            ),
        ];
        assert_eq!(
            programs(&manifests, &["acme/cli.py"]),
            [("acme/cli.py".to_string(), "main".to_string())]
        );
    }

    #[test]
    fn the_adapters_inherit_in_list_order() {
        let mut manifests = vec![read_at("package.json", "{}")];
        inherit_of(
            &[&Stand("1", &[]), &Stand("2", &[]), &Stand("3", &[])],
            &mut manifests,
        );
        assert_eq!(manifests[0].path, "package.json123");
    }

    #[test]
    fn cargo_inherits_through_the_seam_among_manifests_of_other_ecosystems() {
        let mut manifests = vec![
            read_at("pyproject.toml", "[project]\nname = \"p\"\n"),
            read_at(
                "Cargo.toml",
                "[workspace]\nmembers = [\"m\"]\n[workspace.package]\nversion = \"3.1.0\"\n",
            ),
            read_at(
                "m/Cargo.toml",
                "[package]\nname = \"m\"\nversion.workspace = true\n",
            ),
            read_at("web/package.json", r#"{"name": "web"}"#),
        ];
        inherit(&mut manifests);
        let versions: Vec<_> = (manifests.iter())
            .map(|m| m.summary.version.as_deref())
            .collect();
        assert_eq!(versions, [None, None, Some("3.1.0"), None]);
    }

    #[test]
    fn the_list_names_each_ecosystem_once_in_its_order() {
        let names: Vec<_> = ECOSYSTEMS.iter().map(|e| e.name()).collect();
        assert_eq!(names, ["npm", "pypi", "cargo", "go", "maven"]);
        let prefixes: Vec<_> = ECOSYSTEMS.iter().map(|e| e.prefix()).collect();
        assert_eq!(
            prefixes,
            [
                "package::npm::",
                "package::pypi::",
                "package::cargo::",
                "package::go::",
                "package::maven::"
            ]
        );
    }

    #[test]
    fn each_file_is_claimed_by_one_ecosystem_at_most() {
        let mut extensions = HashSet::new();
        for e in ECOSYSTEMS {
            for extension in e.sources() {
                assert!(extensions.insert(*extension), "{extension}");
            }
        }
        for manifest in [
            "package.json",
            "pyproject.toml",
            "requirements.txt",
            "requirements-dev.txt",
            "Cargo.toml",
            "go.mod",
        ] {
            let claims = ECOSYSTEMS.iter().filter(|e| e.is_manifest(manifest));
            assert_eq!(claims.count(), 1, "{manifest}");
        }
    }

    #[test]
    fn sources_contracts_and_manifests_round_trip_each_ecosystem() {
        let sources = [
            ("a.py", "pypi"),
            ("a.rs", "cargo"),
            ("a.go", "go"),
            ("a.ts", "npm"),
            ("a.tsx", "npm"),
            ("a.js", "npm"),
            ("a.jsx", "npm"),
            ("a.mjs", "npm"),
            ("a.cjs", "npm"),
        ];
        for (path, want) in sources {
            assert_eq!(named(of_source(path)), Some(want), "{path}");
        }
        for e in ECOSYSTEMS {
            for extension in e.sources() {
                let path = format!("src/a.{extension}");
                assert_eq!(named(of_source(&path)), Some(e.name()), "{path}");
            }
            assert_eq!(named(of_contract(&contract(*e, "x"))), Some(e.name()));
        }
        for (manifest, want) in [
            ("package.json", "npm"),
            ("pyproject.toml", "pypi"),
            ("a/requirements.txt", "pypi"),
            ("Cargo.toml", "cargo"),
            ("a/go.mod", "go"),
        ] {
            assert_eq!(named(of(manifest)), Some(want), "{manifest}");
        }
    }

    #[test]
    fn a_file_no_ecosystem_claims_is_none() {
        for path in [
            "A.unknown",
            "notes.txt",
            "Makefile",
            "tsconfig.json",
            "unknown.xml",
        ] {
            assert!(of_source(path).is_none(), "{path}");
            assert!(of(path).is_none(), "{path}");
        }
        assert!(of_contract("package::unknown::x").is_none());
    }

    #[test]
    fn a_contract_of_no_ecosystem_has_no_flag_no_suffix_and_no_link() {
        let contract = "package::unknown::x";
        assert!(of_contract(contract).is_none());
        assert_eq!(flag_of(contract, Some("1"), Some("2")), None);
        assert_eq!(
            reason(contract, Some("1"), Some("2")),
            "package::unknown::x 1 (local 2)"
        );
        let consumer = Consumer {
            contract: Some(contract.into()),
            site: "r::pom.xml".into(),
            via: None,
            shape: None,
            method: Some("1".into()),
            fragments: Vec::new(),
            hops: Vec::new(),
        };
        let repo = Repository {
            root: "/nonexistent".into(),
            identity: "r".into(),
            config: Default::default(),
            decisions: Default::default(),
            index: "".into(),
        };
        let provider = Provider {
            contract: contract.into(),
            site: "r::pom.xml".into(),
            version: Some("2".into()),
            ..Provider::default()
        };
        assert!(
            Packages
                .link(&consumer, &repo, &[(&repo, &provider)], &[])
                .is_empty()
        );
    }

    #[test]
    fn manifests_and_contracts_name_their_ecosystem() {
        for (path, want) in [
            ("package.json", Some("npm")),
            ("a/pyproject.toml", Some("pypi")),
            ("requirements.txt", Some("pypi")),
            ("a/requirements-dev.txt", Some("pypi")),
            ("requirements/base.txt", None),
            ("a/requirements.in", None),
            ("tsconfig.json", None),
            ("a/package.json.bak", None),
        ] {
            assert_eq!(named(of(path)), want, "{path}");
        }
        assert_eq!(contract(&Pypi, "shared-lib"), "package::pypi::shared-lib");
        assert_eq!(named(of_contract("package::pypi::x")), Some("pypi"));
        assert_eq!(named(of_contract("package::npm::x")), Some("npm"));
        // The reason says the flag of the contract's ecosystem.
        assert_eq!(
            reason("package::pypi::a", Some(">=1"), Some("1.2")),
            "package::pypi::a >=1 (local 1.2), range unchecked"
        );
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

    /// A repository `name` with one manifest at its root, holding `files`, each exporting the
    /// constant `S` that keeps `fields` as its literal shape, and a `tsconfig.json` with
    /// `outDir` `dist` when `config`.
    fn repository(
        name: &str,
        manifest_json: &str,
        files: &[(&str, &[&str])],
        config: bool,
    ) -> Export {
        let shape = |fields: &[&str]| Shape {
            fields: fields.iter().map(|f| f.to_string()).collect(),
            ..Shape::new(shape::Source::Literals("x.ts".into()))
        };
        Export {
            repository: name.into(),
            root: PathBuf::new(),
            files: (files.iter())
                .map(|(p, _)| index::File {
                    path: p.to_string(),
                    hash: String::new(),
                })
                .collect(),
            symbols: Vec::new(),
            calls: Vec::new(),
            providers: Vec::new(),
            consumers: Vec::new(),
            bindings: Vec::new(),
            documents: Vec::new(),
            publishes: Vec::new(),
            entry_points: Vec::new(),
            processes: Vec::new(),
            manifests: vec![manifest("package.json", manifest_json)],
            tsconfigs: (config.then(|| Config {
                path: "tsconfig.json".into(),
                tsconfig: Tsconfig {
                    out_dir: Some("dist".into()),
                    root_dir: None,
                },
            }))
            .into_iter()
            .collect(),
            packages: Vec::new(),
            dependencies: Vec::new(),
            import_calls: Vec::new(),
            communities: Vec::new(),
            exported: (files.iter())
                .map(|(p, _)| row(p, Some("S"), Some(p), Some("S")))
                .collect(),
            shapes: (files.iter())
                .map(|(p, fields)| ExportedShape {
                    path: p.to_string(),
                    name: "S".into(),
                    want: Want::Literals,
                    shape: shape(fields),
                })
                .collect(),
            imports: Vec::new(),
            links: Vec::new(),
            stale_decisions: Vec::new(),
        }
    }

    /// What `kept` gives the import of `S` from `p` in `src/a.ts` of the first repository.
    fn kept_by(exports: &[Export]) -> Option<(usize, Vec<String>, Level)> {
        let providers = Providers::new(exports);
        let provided: Vec<OnceCell<Provided>> = exports.iter().map(|_| OnceCell::new()).collect();
        let import = shape::Import::new("", "src/a.ts", ("p", "S".into()), Want::Literals);
        let (m, s, level) = kept(exports, &providers, &provided, 0, &import)?;
        Some((m, s.fields.iter().cloned().collect(), level))
    }

    #[test]
    fn a_kept_shape_is_found_like_a_call_and_only_when_certain_of_one() {
        let consumer = || repository("c", r#"{"dependencies": {"p": "^1.0.0"}}"#, &[], false);
        let main = r#"{"name": "p", "version": "1.0.0", "main": "index.ts"}"#;
        let built = r#"{"name": "p", "version": "1.0.0", "main": "dist/index.js"}"#;
        let one = |fields: &'static [&'static str]| {
            repository("p1", main, &[("index.ts", fields)], false)
        };
        let id = vec!["id".to_string()];
        assert_eq!(
            kept_by(&[consumer(), one(&["id"])]),
            Some((1, id.clone(), Level::Exact))
        );
        // Through the `outDir` mapping: a guess, so `inferred`.
        let mapped = repository("p1", built, &[("index.ts", &["id"])], true);
        assert_eq!(
            kept_by(&[consumer(), mapped]),
            Some((1, id.clone(), Level::Inferred))
        );
        // No way in leads to a file: `ambiguous`, with one candidate or several.
        let lost = repository("p1", built, &[("a.ts", &["id"])], false);
        assert_eq!(kept_by(&[consumer(), lost]), None);
        let lost = repository("p1", built, &[("a.ts", &["id"]), ("b.ts", &["id"])], false);
        assert_eq!(kept_by(&[consumer(), lost]), None);
        // Two providers of the package: one shape is kept, two different ones are none.
        let twice = [consumer(), one(&["id"]), one(&["id"])];
        assert_eq!(kept_by(&twice), Some((1, id, Level::Exact)));
        assert_eq!(kept_by(&[consumer(), one(&["id"]), one(&["key"])]), None);
        // A package the importing file's manifest does not depend on is no provider.
        let other = repository("c", r#"{"dependencies": {"q": "1.0.0"}}"#, &[], false);
        assert_eq!(kept_by(&[other, one(&["id"])]), None);
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
            shapes: HashMap::new(),
            owned: Default::default(),
            go_dirs: OnceCell::new(),
            classes: OnceCell::new(),
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
    fn a_function_and_a_constant_or_type_two_export_stars_give_collide() {
        use crate::lang::{LanguagePack, ts::TsJs};
        use crate::resolve::Resolver;
        for other in [
            "export const f = { a: 1 };",
            "export type f = { a: string };",
        ] {
            let sources = [
                ("i.ts", "export * from './a';\nexport * from './b';\n"),
                ("a.ts", "export function f() {}\nexport function g() {}\n"),
                ("b.ts", other),
            ];
            let facts: Vec<_> = (sources.iter())
                .map(|(p, s)| (*p, TsJs.parse(p, s.as_bytes()).unwrap()))
                .collect();
            let files = facts.iter().map(|(p, f)| (*p, f)).collect();
            let rows = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs).exported();
            let p = provided(&rows);
            // As in ES modules, two `export *` giving `f` differently export no `f`.
            assert_eq!(lookup(&p, "i.ts", "f"), None, "{other}");
            let g = Some(("a.ts".to_string(), Some("g".to_string())));
            assert_eq!(lookup(&p, "i.ts", "g"), g, "{other}");
        }
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
        let pyproject = Manifest {
            path: "pyproject.toml".into(),
            summary: Summary::default(),
        };
        let manifests = [root, nested, pyproject];
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

        // Another ecosystem asking first keeps its own view of the files: npm's stays npm's.
        assert_eq!(
            p.candidates("r", &manifests[2], "f", &|id| p.symbol(id)),
            ["r::sealed/src/b.ts::f", "r::src/a.ts::f"]
        );
        // Only the asking ecosystem's table is built (PyPI is the second of `ECOSYSTEMS`).
        assert!(p.owned[1].get().is_some());
        assert!(
            p.owned
                .iter()
                .enumerate()
                .all(|(i, t)| i == 1 || t.get().is_none())
        );
        // The root package does not take the nested package's file (a directory-prefix test
        // would), a declaration file, or one under `node_modules`; the nested one neither.
        assert_eq!(
            p.candidates("r", &manifests[0], "f", &|id| p.symbol(id)),
            ["r::src/a.ts::f"]
        );
        assert_eq!(
            p.candidates("r", &manifests[1], "f", &|id| p.symbol(id)),
            ["r::sealed/src/b.ts::f"]
        );
        assert!(
            p.candidates("r", &manifests[0], "g", &|id| p.symbol(id))
                .is_empty()
        );
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
