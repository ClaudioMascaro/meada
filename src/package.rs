use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::{ContractKind, Found, Hit, Level, Link, Role, Search, Site};
use crate::entry;
use crate::iac::actions::run::{self, Script, Word};
use crate::index::{self, CallSite, Consumer, Export, Indexed, Provider};
use crate::lang::ts::{EXTENSIONS, strip_jsonc, with_extensions};
use crate::lang::{Call, MAX_BYTES, Use};
use crate::literals::Literals;
use crate::render::{Files, label, rank, section};
use crate::resolve::{Ancestry, Code, Exported, Flexible};
use crate::shape::{self, Shape, Want};
use crate::workspace::Repository;

mod cargo;
mod go;
#[cfg(test)]
mod hostile;
pub(crate) mod lock;
mod maven;
pub(crate) mod npm;
mod nuget;
mod pep440;
mod pypi;
mod swiftpm;

use cargo::Cargo;
use go::Go;
use maven::Maven;
use npm::{Npm, exported};
use nuget::Nuget;
use pypi::Pypi;
use swiftpm::Swiftpm;

pub struct Packages;

fn import_found(site: Site, specifier: String, chain: String) -> Found {
    Found {
        identity: None,
        role: Role::Import,
        site,
        contract: None,
        via: Some(specifier),
        method: Some(chain),
        fragments: Vec::new(),
        shape: None,
        lead: None,
        cap: None,
        client: None,
    }
}

const PREFIX: &str = "package::";

trait Ecosystem {
    fn name(&self) -> &'static str;

    fn is_manifest(&self, file_name: &str) -> bool;

    fn read(&self, path: &str, src: &[u8]) -> Summary;

    fn sources(&self) -> &'static [&'static str];

    fn names_package(&self, specifier: &str) -> bool;

    fn imports<'a>(&self, code: &Code<'a>, path: &'a str, call: &'a Call) -> Vec<(String, String)> {
        let imported = code.imported(path, &call.scope, &call.callee);
        (imported.into_iter())
            .map(|(specifier, chain)| (specifier.to_string(), chain))
            .collect()
    }

    fn carried<'a>(
        &self,
        _code: &Code<'a>,
        _path: &'a str,
        _index: usize,
        _call: &'a Call,
    ) -> Vec<(String, String)> {
        Vec::new()
    }

    fn entries<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        from: (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)>;

    fn prefix(&self) -> String {
        format!("{PREFIX}{}::", self.name())
    }

    fn fits(&self, wanted: &str, provided: &str) -> bool {
        wanted == provided
    }

    fn uncertainty(&self, _via: Option<&str>) -> Option<(&'static str, Level)> {
        None
    }

    fn installs(&self, _command: &[Word]) -> Vec<Install> {
        Vec::new()
    }

    fn runs(&self, _command: &[Word]) -> Option<(String, Option<Entry>)> {
        None
    }

    fn unsourced(&self, _via: Option<&str>) -> Option<&'static str> {
        None
    }

    fn member_of(&self, _target: &str, _class: &str) -> bool {
        false
    }

    fn flag(&self, _range: Option<&str>, _version: Option<&str>) -> Option<Flag> {
        Some(Flag::Unchecked)
    }

    fn reaches(
        &self,
        _source: (Option<&str>, Option<&str>),
        _from: (&str, &Path),
        _to: ((&str, &Path), &str),
    ) -> bool {
        true
    }

    fn owns(&self, _manifest: &Manifest) -> bool {
        true
    }

    fn submodule(&self, _file: &str, _name: &str) -> Vec<String> {
        Vec::new()
    }

    fn ways(&self, manifest: &Manifest, subpath: &str) -> Vec<(How, Vec<String>)> {
        ways(manifest, subpath)
    }

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
        let id = at.reach(self, repository, &file, chain, known)?;
        Some(Resolved {
            how,
            level,
            targets: vec![id],
        })
    }

    fn resolutions<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        (n, from): (usize, &str),
        (specifier, chain): (&str, &str),
        known: &dyn Fn(&Provided<'e>, &str) -> bool,
    ) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
        resolutions_by_ways(
            self,
            exports,
            providers,
            at,
            (n, from),
            (specifier, chain),
            known,
        )
    }

    fn declares<'e>(
        &self,
        _exports: &'e [Export],
        _providers: &Providers,
        _at: &dyn Fn(usize) -> &'e Provided<'e>,
        _from: (usize, &str),
        _import: (&str, &str),
    ) -> bool {
        true
    }

    fn shapes(&self) -> bool {
        false
    }

    fn programs(&self, _manifests: &[Manifest], _files: &[&str]) -> Vec<Program> {
        Vec::new()
    }

    fn inherit(&self, _manifests: &mut [Manifest]) {}

    fn settle(&self, _exports: &mut [Export]) {}
}

fn resolutions_by_ways<'e, E: Ecosystem + ?Sized>(
    ecosystem: &E,
    exports: &'e [Export],
    providers: &Providers,
    at: &dyn Fn(usize) -> &'e Provided<'e>,
    from: (usize, &str),
    import: (&str, &str),
    known: &dyn Fn(&Provided<'e>, &str) -> bool,
) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
    resolutions_hopping(ecosystem, exports, providers, at, from, import, known, true)
}

#[expect(clippy::too_many_arguments)]
fn resolutions_hopping<'e, E: Ecosystem + ?Sized>(
    ecosystem: &E,
    exports: &'e [Export],
    providers: &Providers,
    provider: &dyn Fn(usize) -> &'e Provided<'e>,
    (n, from): (usize, &str),
    (specifier, chain): (&str, &str),
    known: &dyn Fn(&Provided<'e>, &str) -> bool,
    hop: bool,
) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
    let mut out = Vec::new();
    for (entry, subpath, reached) in reachable(ecosystem, exports, providers, (n, from), specifier)
    {
        let mut resolved = Vec::new();
        for (m, i) in reached {
            let manifest = &exports[m].manifests[i];
            let at = provider(m);
            let known_here = |id: &str| known(at, id);
            let repository = &exports[m].repository;
            let import = ((entry, subpath.as_str()), (specifier, chain));
            let found =
                ecosystem.resolve(at, repository, manifest, import.0, import.1, &known_here);
            if let Some(r) = found {
                resolved.push((m, manifest, at, r));
                continue;
            }
            let file = hop
                .then(|| at.entry(manifest, &ecosystem.ways(manifest, &subpath)))
                .flatten();
            let Some((source, chain)) = file.as_ref().and_then(|(f, ..)| at.reexport(f, chain))
            else {
                continue;
            };
            let from = (m, file.as_ref().map_or("", |(f, ..)| f.as_str()));
            let further = resolutions_hopping(
                ecosystem,
                exports,
                providers,
                provider,
                from,
                (&source, &chain),
                known,
                false,
            );
            resolved.extend(further.into_iter().flat_map(|(_, r)| r));
        }
        out.push((entry, resolved));
    }
    out
}

type Reached<'e> = (&'e Entry, String, Vec<(usize, usize)>);

fn reachable<'e, E: Ecosystem + ?Sized>(
    ecosystem: &E,
    exports: &'e [Export],
    providers: &Providers,
    (n, from): (usize, &str),
    specifier: &str,
) -> Vec<Reached<'e>> {
    (ecosystem
        .entries(exports, providers, (n, from), specifier)
        .into_iter())
    .map(|(entry, subpath)| {
        let source = (entry.via.as_deref(), entry.range.as_deref());
        let reached = (providers.matching(ecosystem, entry).into_iter())
            .filter(|&(m, i)| {
                let to = (exports[m].place(), exports[m].manifests[i].path.as_str());
                ecosystem.reaches(source, exports[n].place(), to)
            })
            .collect();
        (entry, subpath, reached)
    })
    .collect()
}

const ECOSYSTEMS: &[&dyn Ecosystem] = &[&Npm, &Pypi, &Cargo, &Go, &Maven, &Nuget, &Swiftpm];

fn of(path: &str) -> Option<&'static dyn Ecosystem> {
    let name = path.rsplit('/').next()?;
    ECOSYSTEMS.iter().copied().find(|e| e.is_manifest(name))
}

fn of_source(path: &str) -> Option<&'static dyn Ecosystem> {
    let (_, extension) = path.rsplit_once('.')?;
    (ECOSYSTEMS.iter().copied()).find(|e| e.sources().contains(&extension))
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Program {
    file: String,
    function: String,
    name: String,
    manifest: usize,
}

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
        .map(|p| (p.file, p.function))
        .collect();
    found.sort();
    found.dedup();
    found
}

pub fn inherit(manifests: &mut [Manifest]) {
    inherit_of(ECOSYSTEMS, manifests);
}

fn inherit_of(list: &[&dyn Ecosystem], manifests: &mut [Manifest]) {
    for e in list {
        e.inherit(manifests);
    }
}

fn of_contract(contract: &str) -> Option<&'static dyn Ecosystem> {
    (ECOSYSTEMS.iter().copied()).find(|e| contract.starts_with(&e.prefix()))
}

fn contract(ecosystem: &dyn Ecosystem, name: &str) -> String {
    format!("{}{name}", ecosystem.prefix())
}

pub fn is_manifest(path: &str) -> bool {
    is_tsconfig(path) || of(path).is_some()
}

pub fn read_manifest(path: &str, src: &[u8]) -> Summary {
    of(path).map_or_else(Summary::default, |e| e.read(path, src))
}

// ponytail: npm's own limit, for PyPI names too; a name past it is not a package.
pub(crate) const MAX_NAME: usize = 214;

// ponytail: real packages re-export a few levels deep; this bounds the walk over a hostile one.
const MAX_REEXPORTS: usize = 64;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Summary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<crate::jvm::Build>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exports: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<(String, String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_workspace: Option<(Vec<String>, Vec<String>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lib_name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherits_version: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_version: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub project: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<crate::dotnet::msbuild::Import>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub products: Vec<(String, Vec<String>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub properties: crate::dotnet::msbuild::Properties,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct Entry {
    pub field: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherits: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<(String, String)>,
}

impl Entry {
    pub fn key(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

pub struct Manifest {
    pub path: String,
    pub summary: Summary,
}

pub struct Package {
    pub name: String,
    pub version: Option<String>,
    pub repository: String,
    /// The directory of the manifest, relative to the repository root: `.` for the root.
    pub directory: String,
    ecosystem: &'static dyn Ecosystem,
    pub manifest: String,
}

impl Package {
    pub(crate) fn written(self) -> meada_core::export::Package {
        let ecosystem = self.ecosystem.name();
        let mut package = meada_core::export::Package::default();
        package.name = self.name;
        package.version = self.version;
        package.repository = self.repository;
        package.directory = self.directory;
        package.ecosystem = (ecosystem != Npm.name()).then(|| ecosystem.to_string());
        package
    }

    pub fn site(&self) -> String {
        site(&self.repository, &self.manifest)
    }
}

pub struct ImportCall {
    pub site: String,
    pub path: String,
    pub specifier: String,
    pub chain: String,
}

pub struct ExportedShape {
    pub path: String,
    pub name: String,
    pub want: Want,
    pub shape: Shape,
}

pub use meada_core::export::How;

#[derive(Clone)]
pub struct Hop {
    pub manifest: String,
    pub target: String,
    pub how: String,
    pub level: Level,
}

pub use meada_core::export::{Dependency, Import};

// ponytail: the trunk follows `extends` for `paths` only (`lang/ts.rs`); a base config's `outDir`
// counts as absent. Lifted on its own: the trunk's `paths` lift did not carry it.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Tsconfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_dir: Option<String>,
}

pub struct Config {
    pub path: String,
    pub tsconfig: Tsconfig,
}

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

pub(crate) fn tsconfig_root(src: &[u8]) -> Option<Value> {
    if src.len() > MAX_BYTES {
        return None;
    }
    serde_json::from_str(&strip_jsonc(std::str::from_utf8(src).ok()?))
        .ok()
        .filter(Value::is_object)
}

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

pub fn is_tsconfig(path: &str) -> bool {
    path.rsplit('/').next() == Some("tsconfig.json")
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == ':')
}

fn site(repository: &str, path: &str) -> String {
    format!("{repository}::{path}")
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

fn pattern_fits(pattern: &str, value: &str) -> bool {
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or("");
    let Some(mut rest) = value.strip_prefix(first) else {
        return false;
    };
    let pieces: Vec<_> = pieces.collect();
    if pieces.is_empty() {
        return rest.is_empty();
    }
    for (i, piece) in pieces.iter().enumerate() {
        if i + 1 == pieces.len() {
            return rest.ends_with(piece);
        }
        let Some(at) = rest.find(piece) else {
            return false;
        };
        rest = &rest[at + piece.len()..];
    }
    false
}

fn bare_directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

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
        Some(("run", _)) => true,
        _ => false,
    }
}

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

fn nearest<'m>(
    manifests: &'m [Manifest],
    path: &str,
    ecosystem: &dyn Ecosystem,
) -> Option<&'m Manifest> {
    holding(manifests, path, ecosystem).into_iter().next()
}

fn owner<'m>(
    manifests: &'m [Manifest],
    path: &str,
    ecosystem: &dyn Ecosystem,
) -> Option<&'m Manifest> {
    (holding(manifests, path, ecosystem).into_iter()).find(|m| ecosystem.owns(m))
}

pub(crate) fn owner_of<'m>(manifests: &'m [Manifest], path: &str) -> Option<&'m Manifest> {
    (ECOSYSTEMS.iter())
        .filter_map(|ecosystem| owner(manifests, path, *ecosystem))
        .min_by_key(|m| std::cmp::Reverse(m.path.rsplit_once('/').map_or(0, |(d, _)| d.len() + 1)))
}

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

fn inside(dir: &str, path: &str) -> Option<String> {
    if (path.split('/')).any(|s| s.is_empty() || s == "." || s == "..") {
        return None;
    }
    Some(match dir {
        "." => path.to_string(),
        dir => format!("{dir}/{path}"),
    })
}

const OUTPUT: [&str; 5] = [".d.ts", ".js", ".mjs", ".cjs", ".jsx"];

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

struct Provided<'e> {
    files: HashSet<&'e str>,
    symbols: HashSet<&'e str>,
    exported: HashMap<&'e str, Vec<&'e Exported>>,
    manifests: &'e [Manifest],
    configs: &'e [Config],
    shapes: HashMap<(String, Want), &'e Shape>,
    owned: [OnceCell<HashMap<&'e str, Vec<&'e str>>>; ECOSYSTEMS.len()],
    go_dirs: OnceCell<HashMap<&'e str, Vec<&'e str>>>,
    classes: OnceCell<maven::Classes<'e>>,
    swift: OnceCell<swiftpm::Symbols<'e>>,
    ancestries: &'e [Ancestry],
    flexible: &'e [Flexible],
}

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
            swift: OnceCell::new(),
            ancestries: &export.ancestries,
            flexible: &export.flexible,
        }
    }

    fn symbol(&self, id: &str) -> bool {
        self.symbols.contains(id)
    }

    fn source(&self, path: &str) -> bool {
        self.files.contains(path) && !path.ends_with(".d.ts")
    }

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

    fn owned_source(&self, manifest: &Manifest, path: &str) -> bool {
        of(&manifest.path).is_some_and(|ecosystem| {
            self.source(path)
                && owner(self.manifests, path, ecosystem)
                    .is_some_and(|m| directory(&m.path) == directory(&manifest.path))
        })
    }

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
            .filter_map(|path| self.reach(ecosystem, repository, path, chain, known))
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    fn reach<E: Ecosystem + ?Sized>(
        &self,
        ecosystem: &E,
        repository: &str,
        file: &str,
        chain: &str,
        known: &dyn Fn(&str) -> bool,
    ) -> Option<String> {
        let mut names = chain.split('.');
        let mut at = self.member(ecosystem, file, names.next()?)?;
        for member in names {
            at = match at {
                (file, None) => self.member(ecosystem, &file, member)?,
                (file, Some(def)) => (file, Some(format!("{def}.{member}"))),
            };
        }
        let (file, Some(def)) = at else {
            return None;
        };
        let id = format!("{repository}::{file}::{def}");
        known(&id).then_some(id)
    }

    fn member<E: Ecosystem + ?Sized>(
        &self,
        ecosystem: &E,
        file: &str,
        name: &str,
    ) -> Option<(String, Option<String>)> {
        self.lookup(file, name, 0, &mut HashSet::new()).or_else(|| {
            let found = (ecosystem.submodule(file, name).into_iter())
                .find(|f| self.files.contains(f.as_str()))?;
            Some((found, None))
        })
    }

    /// ponytail: the file's own rows only, not those of a local `export *` it goes through;
    /// follow them as `lookup` does when an entry re-exports a package two files deep.
    fn reexport(&self, file: &str, chain: &str) -> Option<(String, String)> {
        let (head, rest) = chain
            .split_once('.')
            .map_or((chain, None), |(h, r)| (h, Some(r)));
        let rows = self.exported.get(file).map_or(&[][..], Vec::as_slice);
        let renamed = |name: &str| rest.map_or(name.to_string(), |r| format!("{name}.{r}"));
        if let Some(own) = rows.iter().find(|e| e.name.as_deref() == Some(head)) {
            let source = own.source.clone()?;
            return Some((source, renamed(own.target.as_deref().unwrap_or(head))));
        }
        let stars: Vec<&str> = (rows.iter())
            .filter(|e| e.name.as_deref() == Some("*"))
            .filter_map(|e| e.source.as_deref())
            .collect();
        match (head, stars.as_slice()) {
            ("default", _) => None,
            (_, [source]) => Some((source.to_string(), chain.to_string())),
            _ => None,
        }
    }

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

struct Providers {
    by_name: HashMap<(&'static str, String), Vec<(usize, usize)>>,
    walked: RefCell<HashMap<(&'static str, usize, String), Positions>>,
}

type Positions = Vec<(usize, usize)>;

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
        Self {
            by_name: providers,
            walked: RefCell::default(),
        }
    }

    fn walked<'e>(
        &self,
        ecosystem: &'static str,
        exports: &'e [Export],
        (n, dir): (usize, &str),
        walk: impl FnOnce(&'e [Manifest]) -> Positions,
    ) -> Vec<&'e Entry> {
        let manifests = &exports[n].manifests;
        let mut walked = self.walked.borrow_mut();
        let key = (ecosystem, n, dir.to_string());
        let found = walked.entry(key).or_insert_with(|| walk(manifests));
        (found.iter())
            .map(|&(i, j)| &manifests[i].summary.dependencies[j])
            .collect()
    }

    fn matching<E: Ecosystem + ?Sized>(&self, ecosystem: &E, entry: &Entry) -> Vec<(usize, usize)> {
        if ecosystem.uncertainty(entry.via.as_deref()).is_none() {
            return self.of(ecosystem.name(), &entry.name).to_vec();
        }
        let name = &entry.name;
        let mut found: Vec<_> = self
            .by_name
            .iter()
            .filter(|((kind, provided), _)| {
                *kind == ecosystem.name() && ecosystem.fits(name, provided)
            })
            .flat_map(|(_, locations)| locations.iter().copied())
            .collect();
        found.sort();
        found
    }

    fn of(&self, ecosystem: &'static str, name: &str) -> &[(usize, usize)] {
        (self.by_name.get(&(ecosystem, name.to_string()))).map_or(&[], Vec::as_slice)
    }
}

fn import(exports: &mut [Export], providers: &Providers) {
    let provided: Vec<OnceCell<Provided>> = exports.iter().map(|_| OnceCell::new()).collect();
    let mut found = Vec::new();
    for (n, export) in exports.iter().enumerate() {
        let mut consumers = Vec::new();
        let mut imports = Vec::new();
        let sites = export.import_calls.chunk_by(|a, b| a.site == b.site);
        let declared = |call: &ImportCall| {
            let from = (n, call.path.as_str());
            declares(
                exports,
                providers,
                &provided,
                from,
                (&call.specifier, &call.chain),
            )
        };
        let calls = sites
            .filter(|calls| calls.len() < 2 || calls.iter().filter(|c| declared(c)).count() < 2)
            .flatten();
        for call in calls {
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
                        imports.push(Import::new(
                            call.site.clone(),
                            call.specifier.clone(),
                            call.chain.clone(),
                            level,
                            how,
                            target.clone(),
                        ));
                        hops.push(Hop {
                            manifest: site(repository, &manifest.path),
                            target,
                            how: format!("import {}", how.name()),
                            level,
                        });
                    }
                }
                if !hops.is_empty() {
                    consumers.push(Consumer {
                        identity: None,
                        contract: Some(contract(ecosystem, &entry.name)),
                        site: call.site.clone(),
                        via: entry.via.clone(),
                        shape: None,
                        method: entry.range.clone(),
                        fragments: Vec::new(),
                        hops,
                        root: None,
                        lead: None,
                        cap: None,
                        client: None,
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
        imports.dedup();
        export.imports = imports;
    }
}

fn shapes(exports: &mut [Export], providers: &Providers) {
    let provided: Vec<OnceCell<Provided>> = exports.iter().map(|_| OnceCell::new()).collect();
    let all: &[Export] = exports;
    let mut find = |n, import: &shape::Import| kept(all, providers, &provided, n, import);
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
    for (n, i, s) in consumed_by {
        exports[n].consumers[i].shape = (!s.fields.is_empty()).then_some(s);
    }
}

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
            Some((first, s, level)) => Some((first, s, level.max(resolved.level))),
        };
    }
    found
}

type Resolution<'e> = (usize, &'e Manifest, &'e Provided<'e>, Resolved);

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

fn declares<'e>(
    exports: &'e [Export],
    providers: &Providers,
    provided: &'e [OnceCell<Provided<'e>>],
    from: (usize, &str),
    import: (&str, &str),
) -> bool {
    let Some(ecosystem) = of_source(from.1) else {
        return false;
    };
    let at = |m: usize| provided[m].get_or_init(|| Provided::new(&exports[m]));
    ecosystem.declares(exports, providers, &at, from, import)
}

fn surface(
    manifests: &[Manifest],
    configs: &[Config],
    exported: &[Exported],
    files: &[&str],
    opaque: &HashSet<(&str, &str)>,
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
        swift: OnceCell::new(),
        ancestries: &[],
        flexible: &[],
    };
    let mut queue = Vec::new();
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
        let followed = rows.filter(|e| match (&e.file, &e.target) {
            (Some(f), Some(t)) => !opaque.contains(&(f.as_str(), t.as_str())),
            _ => true,
        });
        queue.extend(followed.filter_map(|e| e.file.clone()));
    }
    seen
}

fn exported_shapes(code: &Code, surface: &HashSet<String>) -> Vec<Found> {
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
                    identity: None,
                    role: Role::Shape,
                    site: Site::File(path.to_string()),
                    contract: None,
                    via: Some(want.name().to_string()),
                    method: Some(local.clone()),
                    fragments: Vec::new(),
                    shape: Some(shape),
                    lead: None,
                    cap: None,
                    client: None,
                });
            }
        }
    }
    found
}

fn consume(exports: &mut [Export], providers: &Providers) {
    let ecosystems = provided(exports);
    let scripts = OnceCell::new();
    let found: Vec<_> = (exports.iter().enumerate())
        .map(|(n, export)| {
            let mut consumers = Vec::new();
            let mut dependencies = Vec::new();
            let provided = |ecosystem: &dyn Ecosystem, e: &Entry, own: Option<(usize, usize)>| {
                let source = (e.via.as_deref(), e.range.as_deref());
                let mut of = providers.matching(ecosystem, e);
                of.retain(|&p| Some(p) != own);
                of.iter().any(|&(m, i)| {
                    let at = &exports[m].manifests[i];
                    ecosystem.reaches(source, export.place(), (exports[m].place(), &at.path))
                })
            };
            let mut installed: HashMap<&str, HashMap<String, Option<String>>> = HashMap::new();
            let mut steps: Vec<&Script> = export.scripts.iter().collect();
            steps.sort_by_key(|script| step_of(&script.site));
            for script in steps {
                let consumer = |contract, via, range, hops| Consumer {
                    identity: None,
                    contract,
                    site: script.site.clone(),
                    via,
                    shape: None,
                    method: range,
                    fragments: Vec::new(),
                    hops,
                    root: None,
                    lead: None,
                    cap: None,
                    client: None,
                };
                let installed = installed.entry(step_of(&script.site).0).or_default();
                let (mut named, mut open) = (HashSet::new(), Vec::new());
                let commands =
                    (script.commands.iter()).flat_map(|c| ECOSYSTEMS.iter().map(move |e| (c, *e)));
                for (command, ecosystem) in commands {
                    for install in ecosystem.installs(command) {
                        match install {
                            Install::Open(_) if !ecosystems.contains(ecosystem.name()) => {}
                            Install::Open(written) => open.push(written),
                            Install::Named(e) => {
                                let contract = contract(ecosystem, &e.name);
                                if !provided(ecosystem, &e, None) {
                                    continue;
                                }
                                if e.via.is_none() {
                                    let range = || e.range.clone();
                                    installed.entry(contract.clone()).or_insert_with(range);
                                }
                                if named.insert(contract.clone()) {
                                    let row = consumer(Some(contract), e.via, e.range, Vec::new());
                                    consumers.push(row);
                                }
                            }
                        }
                    }
                    let Some((name, package)) = ecosystem.runs(command) else {
                        continue;
                    };
                    let declared = scripts.get_or_init(|| console_scripts(exports));
                    let key = (ecosystem.name(), name);
                    let declared = declared.get(&key).map_or(&[][..], Vec::as_slice);
                    let text = format!("run {}", run::written(command));
                    let hop = |d: &Declared, level, why: &str| Hop {
                        manifest: d.manifest.clone(),
                        target: d.program.clone(),
                        how: format!("console script {}, {why}", key.1),
                        level,
                    };
                    let rows: Vec<(String, Option<String>, Vec<Hop>)> = if let Some(e) = package {
                        if !provided(ecosystem, &e, None) {
                            continue;
                        }
                        let contract = contract(ecosystem, &e.name);
                        let hops: Vec<Hop> = (declared.iter())
                            .filter(|d| d.contract == contract)
                            .map(|d| hop(d, Level::Exact, BY_PACKAGE))
                            .collect();
                        if hops.is_empty() {
                            if named.insert(contract.clone()) {
                                consumers.push(consumer(Some(contract), None, e.range, hops));
                            }
                            continue;
                        }
                        vec![(contract, e.range, hops)]
                    } else {
                        let mut packages: BTreeMap<&str, Vec<&Declared>> = BTreeMap::new();
                        for d in declared {
                            packages.entry(&d.contract).or_default().push(d);
                        }
                        if packages.len() > MAX_DECLARING {
                            continue;
                        }
                        let evidence = packages.keys().filter(|c| installed.contains_key(**c));
                        let evidence = evidence.count();
                        let (level, why) = match (evidence, packages.len()) {
                            (1, _) => (Level::Exact, INSTALLED.to_string()),
                            (0, 1) => (Level::Inferred, NOT_INSTALLED.to_string()),
                            (0, n) => (Level::Ambiguous, format!("{n} packages declare it")),
                            (n, _) => (
                                Level::Ambiguous,
                                format!("{n} packages installed earlier in the CI job declare it"),
                            ),
                        };
                        (packages.iter())
                            .filter(|(c, _)| evidence == 0 || installed.contains_key(**c))
                            .map(|(c, declared)| {
                                let range = installed.get(*c).cloned().flatten();
                                let hops = declared.iter().map(|d| hop(d, level, &why)).collect();
                                (c.to_string(), range, hops)
                            })
                            .collect()
                    };
                    for (contract, range, hops) in rows {
                        let how = hops.first().map_or("", |hop| hop.how.as_str());
                        if named.insert(format!("{contract} {how}")) {
                            consumers.push(consumer(
                                Some(contract),
                                Some(text.clone()),
                                range,
                                hops,
                            ));
                        }
                    }
                }
                if !open.is_empty() {
                    open.dedup();
                    consumers.push(consumer(None, Some(open.join(", ")), None, Vec::new()));
                }
            }
            for (i, manifest) in export.manifests.iter().enumerate() {
                let Some(ecosystem) = of(&manifest.path) else {
                    continue;
                };
                for e in &manifest.summary.dependencies {
                    if !provided(ecosystem, e, Some((n, i))) {
                        continue;
                    }
                    consumers.push(Consumer {
                        identity: None,
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
                        root: None,
                        lead: None,
                        cap: None,
                        client: None,
                    });
                    let mut dependency = Dependency::default();
                    dependency.manifest = manifest.path.clone();
                    dependency.field = e.field.clone();
                    dependency.name = e.name.clone();
                    dependency.alias = e.alias.clone();
                    dependency.range = e.range.clone();
                    dependencies.push(dependency);
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

fn step_of(site: &str) -> (&str, usize) {
    let (job, step) = site.rsplit_once(".steps[").unwrap_or((site, ""));
    (job, step.trim_end_matches(']').parse().unwrap_or(0))
}

struct Declared {
    contract: String,
    manifest: String,
    program: String,
}

fn console_scripts(exports: &[Export]) -> HashMap<(&'static str, String), Vec<Declared>> {
    let mut found: HashMap<_, Vec<Declared>> = HashMap::new();
    for export in exports {
        let files: Vec<&str> = export.files.iter().map(|f| f.path.as_str()).collect();
        let programs: HashSet<&str> = (export.entry_points.iter())
            .filter(|e| e.kind == entry::EntryKind::Program)
            .map(|e| e.identity.as_str())
            .collect();
        for ecosystem in ECOSYSTEMS {
            for p in ecosystem.programs(&export.manifests, &files) {
                let manifest = &export.manifests[p.manifest];
                let program = format!("{}::{}::{}", export.repository, p.file, p.function);
                let Some(name) = (manifest.summary.name.as_ref())
                    .filter(|_| programs.contains(program.as_str()))
                else {
                    continue;
                };
                let declared = Declared {
                    contract: contract(*ecosystem, name),
                    manifest: site(&export.repository, &manifest.path),
                    program,
                };
                let key = (ecosystem.name(), p.name);
                found.entry(key).or_default().push(declared);
            }
        }
    }
    found
}

// ponytail: a name that many packages declare tells nothing, and each would cost a row per command.
const MAX_DECLARING: usize = 8;

const INSTALLED: &str = "its package installed earlier in the CI job";
const NOT_INSTALLED: &str = "no install of its package earlier in the CI job";
const BY_PACKAGE: &str = "run by its package's name";
const BY_NAME: &str = "the name alone matched";

enum Install {
    Named(Entry),
    Open(String),
}

fn installs(script: &Script) -> Vec<(&'static dyn Ecosystem, Install)> {
    let mut found = Vec::new();
    for command in &script.commands {
        for ecosystem in ECOSYSTEMS {
            found.extend((ecosystem.installs(command).into_iter()).map(|i| (*ecosystem, i)));
        }
    }
    found
}

fn provided(exports: &[Export]) -> HashSet<&'static str> {
    let named = (exports.iter().flat_map(|e| &e.manifests)).filter(|m| m.summary.name.is_some());
    named.filter_map(|m| Some(of(&m.path)?.name())).collect()
}

const OPEN_INSTALL: &str =
    "the package name is not a literal, and static evaluation does not give it";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
    Mismatch,
    Unchecked,
}

impl Flag {
    pub fn words(self) -> &'static str {
        match self {
            Flag::Mismatch => "range mismatch",
            Flag::Unchecked => "range unchecked",
        }
    }
}

fn flag_of(contract: &str, range: Option<&str>, version: Option<&str>) -> Option<Flag> {
    of_contract(contract)?.flag(range, version)
}

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

struct Reach<'e> {
    pub repository: usize,
    pub call: &'e CallSite,
    pub level: Level,
    pub flag: Option<Flag>,
}

fn into<'l>(links: &'l [Link], symbol: &str) -> impl Iterator<Item = &'l Link> {
    let into = move |l: &Link| {
        l.provider == symbol
            || of_contract(&l.contract).is_some_and(|e| e.member_of(&l.provider, symbol))
    };
    (links.iter()).filter(move |l| l.contract.starts_with(PREFIX) && into(l))
}

fn reached<'e>(exports: &'e [Export], links: &[Link], symbol: &str) -> Vec<Reach<'e>> {
    let mut found: Vec<Reach> = Vec::new();
    for link in into(links, symbol) {
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
        let version = (consumer.hops.iter())
            .find(|h| h.target == link.provider)
            .and_then(|h| {
                (exports.iter().flat_map(|e| &e.providers))
                    .find(|p| p.site == h.manifest && p.contract == link.contract)
            })
            .and_then(|p| p.version.as_deref());
        let reach = Reach {
            repository,
            call,
            level: link.level,
            flag: flag_of(&link.contract, consumer.method.as_deref(), version),
        };
        match found.iter_mut().find(|r| r.call.id == call.id) {
            Some(r) if reach.level < r.level => *r = reach,
            Some(_) => {}
            None => found.push(reach),
        }
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

    fn calls_from_definition(&self) -> bool {
        true
    }

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

    fn decidable(&self) -> bool {
        false
    }

    fn extract(&self, code: &Code, indexed: &Indexed) -> Vec<Found> {
        let mut found = Vec::new();
        let files = (code.files().iter())
            .filter_map(|(&path, facts)| Some((path, facts, of_source(path)?)));
        for (path, facts, ecosystem) in files {
            for (index, call) in facts.calls.iter().enumerate() {
                let carried = ecosystem.carried(code, path, index, call);
                for (specifier, chain) in ecosystem
                    .imports(code, path, call)
                    .into_iter()
                    .chain(carried)
                {
                    if chain.is_empty() || !ecosystem.names_package(&specifier) {
                        continue;
                    }
                    let site = Site::Call {
                        path: path.to_string(),
                        index,
                    };
                    found.push(import_found(site, specifier, chain));
                }
            }
            for (index, r) in facts.references.iter().enumerate() {
                if code.reference(path, index) != Some(None) {
                    continue;
                }
                let (specifier, mut chain) = match &r.from {
                    Some(from) => (from.clone(), r.name.clone()),
                    None => match code.imported(path, &r.scope, &r.name) {
                        Some((specifier, chain)) => (specifier.to_string(), chain),
                        None => continue,
                    },
                };
                if r.kind == Use::Member {
                    chain.truncate(chain.find('.').unwrap_or(chain.len()));
                }
                if chain.is_empty() || !ecosystem.names_package(&specifier) {
                    continue;
                }
                let site = Site::Reference {
                    path: path.to_string(),
                    index,
                };
                found.push(import_found(site, specifier, chain));
            }
        }
        let opaque: HashSet<(&str, &str)> = (code.files().iter())
            .filter(|(_, facts)| !facts.declarations.is_empty())
            .flat_map(|(&path, facts)| {
                let consts: HashSet<&str> = facts.consts.iter().map(|c| c.name.as_str()).collect();
                (facts.declarations.iter())
                    .filter(move |d| !consts.contains(d.name.as_str()))
                    .map(move |d| (path, d.name.as_str()))
            })
            .collect();
        let surface = surface(
            indexed.manifests,
            indexed.tsconfigs,
            indexed.exported,
            indexed.files,
            &opaque,
        );
        if !surface.is_empty() {
            found.extend(exported_shapes(code, &surface));
        }
        found
    }

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

    fn workspace_step(&self, exports: &mut [Export], _: &[Repository]) {
        ECOSYSTEMS.iter().for_each(|e| e.settle(exports));
        let providers = Providers::new(exports);
        consume(exports, &providers);
        import(exports, &providers);
        for export in exports.iter_mut() {
            let own = format!("{}::", export.repository);
            let imported: HashSet<&str> = (export.imports.iter())
                .filter(|i| !Use::names(&i.site))
                .map(|i| i.target.as_str())
                .filter(|t| t.starts_with(&own))
                .collect();
            entry::drop_imported_exports(&mut export.entry_points, &imported);
        }
        shapes(exports, &providers);
    }

    fn symbols_read(&self, _repo: &Repository, export: &Export) -> Option<Vec<String>> {
        let provides =
            (export.manifests.iter()).any(|m| m.summary.name.is_some() && of(&m.path).is_some());
        match provides {
            true => None,
            false => Some(Vec::new()),
        }
    }

    fn context_reads(&self, links: &[Link], symbol: &str) -> Vec<String> {
        into(links, symbol).map(|l| l.consumer.clone()).collect()
    }

    fn unresolved_causes(&self, exports: &[Export]) -> HashMap<String, &'static str> {
        let ecosystems = provided(exports);
        let open = |s: &&Script| {
            let open = |(e, i): &(&dyn Ecosystem, Install)| {
                matches!(i, Install::Open(_)) && ecosystems.contains(e.name())
            };
            installs(s).iter().any(open)
        };
        let scripts = exports.iter().flat_map(|e| &e.scripts).filter(open);
        scripts.map(|s| (s.site.clone(), OPEN_INSTALL)).collect()
    }

    fn context_at(
        &self,
        exports: &[Export],
        repos: &[Repository],
        links: &[Link],
        site: &str,
        _root: Option<&str>,
    ) -> Vec<String> {
        let mut lines = self.context_sections(exports, repos, links, site);
        let scripts = exports
            .iter()
            .filter(|e| e.scripts.iter().any(|s| s.site == site));
        let mut shown = HashSet::new();
        for consumer in (scripts.flat_map(|e| &e.consumers)).filter(|c| c.site == site) {
            match (&consumer.contract, &consumer.via) {
                (Some(contract), _) if contract.starts_with(PREFIX) => {
                    if !shown.insert(contract) {
                        continue;
                    }
                    lines.push(format!("consumes {contract}"));
                    let found =
                        (links.iter()).filter(|l| l.consumer == site && l.contract == *contract);
                    lines.extend(found.map(|l| {
                        let level = l.level.name();
                        let at = l.provider.replacen("::", " ", 1);
                        format!("  provider: {at} [{level}: {}]", l.reason)
                    }));
                }
                (None, Some(written)) if self.unresolved_causes(exports).contains_key(site) => {
                    lines.push(format!("consumes (unresolved) {written}: {OPEN_INSTALL}"));
                }
                _ => {}
            }
        }
        lines
    }

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

    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
        exports: &[Export],
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
        let (uncertain, uncertain_level) = ecosystem.uncertainty(consumer.via.as_deref()).unzip();
        let unsourced = match fits.as_slice() {
            [(repo, _)] if uncertain.is_none() && repo.identity == from.identity => {
                ecosystem.unsourced(consumer.via.as_deref())
            }
            _ => None,
        };
        let by_name = match fits.len() {
            1 if uncertain.is_none() && unsourced.is_none() => (exports.iter())
                .filter(|e| e.repository == from.identity)
                .any(|e| e.scripts.iter().any(|s| s.site == consumer.site))
                .then_some(BY_NAME),
            _ => None,
        };
        let (level, n) = match fits.len() {
            0 => return Vec::new(),
            1 if unsourced.is_some() || by_name.is_some() => (Level::Inferred, None),
            1 => (uncertain_level.unwrap_or(Level::Exact), None),
            n => (Level::Ambiguous, Some(n)),
        };
        fits.into_iter()
            .flat_map(|(_, p)| {
                let plain = reason(contract, range, p.version.as_deref());
                let said = match uncertain.or(unsourced).or(by_name) {
                    Some(why) => format!("{why}; {plain}"),
                    None => plain.clone(),
                };
                let said = match n {
                    Some(n) => format!("{n} providers fit; {said}"),
                    None => said,
                };
                let link = Link::new(
                    consumer.site.clone(),
                    p.site.clone(),
                    contract.clone(),
                    level,
                    said,
                    None,
                    None,
                    None,
                );
                if consumer.hops.is_empty() {
                    return vec![link];
                }
                (consumer.hops.iter())
                    .filter(|h| h.manifest == p.site)
                    .map(|h| {
                        let why = match by_name {
                            Some(_) if h.level != Level::Exact => &plain,
                            _ => &link.reason,
                        };
                        Link::new(
                            link.consumer.clone(),
                            h.target.clone(),
                            link.contract.clone(),
                            link.level.max(h.level),
                            format!("{why}; {}", h.how),
                            None,
                            None,
                            None,
                        )
                    })
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_fits_only_by_its_literal_fragments() {
        assert!(pattern_fits("example:model*", "example:models"));
        assert!(pattern_fits("example:model*", "example:model"));
        assert!(pattern_fits("*:tools", "example:tools"));
        assert!(pattern_fits("a*b*c", "aXbYc"));
        assert!(pattern_fits("example:models", "example:models"));
        assert!(!pattern_fits("example:models", "example:models2"));
        assert!(!pattern_fits("*:tools", "example:toolsx"));
        assert!(!pattern_fits("ab*b", "ab"));
        assert!(!pattern_fits("a*b*c", "acb"));
        assert!(!pattern_fits("x*ab*b", "xab"));
    }
    use crate::index::Span;
    use std::path::PathBuf;

    #[test]
    fn an_ambiguous_package_link_is_no_call_for_a_person() {
        assert!(!Packages.decidable());
    }

    #[test]
    fn a_symbol_reached_through_a_package_import_gets_a_consumers_section() {
        let contract = "package::npm::kit";
        let call = "r::web/app.ts::go#0";
        let mut span = Span::default();
        span.path = "web/app.ts".into();
        let mut call_site = CallSite::default();
        call_site.id = call.into();
        call_site.caller = "r::web/app.ts::go".into();
        call_site.span = span;
        let export = Export {
            repository: "r".into(),
            consumers: vec![Consumer {
                identity: None,
                contract: Some(contract.into()),
                site: call.into(),
                via: None,
                shape: None,
                method: Some("^1.0.0".into()),
                fragments: Vec::new(),
                hops: Vec::new(),
                root: None,
                lead: None,
                cap: None,
                client: None,
            }],
            calls: vec![call_site],
            ..Export::default()
        };
        let link = Link::new(
            call.into(),
            "r::kit/index.ts::greet".into(),
            contract.into(),
            Level::Exact,
            format!("{contract} ^1.0.0 (local none)"),
            None,
            None,
            None,
        );
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
        let link = Link::new(
            "r::web/package.json::dependencies::@a/Kit".into(),
            export.packages[0].site(),
            "package::npm::@a/Kit".into(),
            Level::Exact,
            "x".into(),
            None,
            None,
            None,
        );
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
        let surface = surface(&manifests, &[], &rows, &files, &HashSet::new());
        let mut got: Vec<&str> = surface.iter().map(String::as_str).collect();
        got.sort();
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
                symlinks: &[],
                channel_clients: &[],
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
        let two = r#"{"types": "../t.ts", "main": "src/index.js"}"#;
        assert_eq!(at(two, "."), [(How::Main, vec!["lib/src/index.js".into()])]);
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
        let [(how, files)] = &at(all, "./util/x")[..] else {
            panic!("one way")
        };
        assert_eq!(*how, How::Path);
        assert_eq!(
            &files[..3],
            ["lib/util/x", "lib/util/x.ts", "lib/util/x.tsx"]
        );
        assert!(files.contains(&"lib/util/x/index.ts".to_string()));
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
        assert_eq!(holding("app/web/x.ts", &Npm), ["app/web/package.json"]);
        assert!(holding("x.ts", &Npm).is_empty());
        let owner = |path, e: &dyn Ecosystem| owner(&all, path, e).map(|m| m.path.as_str());
        assert_eq!(owner("tools/x.py", &Pypi), Some("pyproject.toml"));
        assert_eq!(owner("app/x.py", &Pypi), Some("app/pyproject.toml"));
        assert_eq!(owner("app/web/x.ts", &Npm), Some("app/web/package.json"));
    }

    fn named(e: Option<&dyn Ecosystem>) -> Option<&'static str> {
        e.map(|e| e.name())
    }

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
        fn programs(&self, _manifests: &[Manifest], _files: &[&str]) -> Vec<Program> {
            (self.1.iter())
                .map(|(file, def)| Program {
                    file: file.to_string(),
                    function: def.to_string(),
                    name: String::new(),
                    manifest: 0,
                })
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
        assert_eq!(
            names,
            ["npm", "pypi", "cargo", "go", "maven", "nuget", "swiftpm"]
        );
        let prefixes: Vec<_> = ECOSYSTEMS.iter().map(|e| e.prefix()).collect();
        assert_eq!(
            prefixes,
            [
                "package::npm::",
                "package::pypi::",
                "package::cargo::",
                "package::go::",
                "package::maven::",
                "package::nuget::",
                "package::swiftpm::"
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
            "Kit.csproj",
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
            identity: None,
            contract: Some(contract.into()),
            site: "r::pom.xml".into(),
            via: None,
            shape: None,
            method: Some("1".into()),
            fragments: Vec::new(),
            hops: Vec::new(),
            root: None,
            lead: None,
            cap: None,
            client: None,
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
        assert_eq!(
            reason("package::pypi::a", Some(">=2"), Some("1.2")),
            "package::pypi::a >=2 (local 1.2), range mismatch"
        );
        assert_eq!(
            reason("package::pypi::a", Some(">=1"), Some("1.2")),
            "package::pypi::a >=1 (local 1.2)"
        );
        assert_eq!(
            reason("package::pypi::a", Some("^1"), Some("1.2")),
            "package::pypi::a ^1 (local 1.2), range unchecked"
        );
    }

    #[test]
    fn build_output_maps_back_to_the_same_path_under_the_root_dir() {
        let all = |stem: &str| {
            ["ts", "tsx", "js", "jsx", "mjs", "cjs"]
                .map(|ext| format!("{stem}.{ext}"))
                .to_vec()
        };
        let table: [(&str, &str, &str, Vec<String>); 12] = [
            ("dist/index.d.ts", "dist", "src", all("src/index")),
            ("dist/index.js", "dist", "src", all("src/index")),
            ("dist/index.cjs", "dist", "src", all("src/index")),
            ("dist/index.mjs", "dist", "src", all("src/index")),
            ("dist/ui/button.jsx", "dist", "src", all("src/ui/button")),
            (
                "lib/a/dist/x.js",
                "lib/a/dist",
                "lib/a/src",
                all("lib/a/src/x"),
            ),
            ("lib/a/dist/x.js", "lib/a/dist", "lib/a", all("lib/a/x")),
            ("dist/x.js", "dist", ".", all("x")),
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
        let empty = Some(Tsconfig::default());
        for valid in [
            "{}",
            r#"{"compilerOptions": 3}"#,
            r#"{"compilerOptions": {"outDir": 1, "rootDir": ["src"]}}"#,
            r#"{"compilerOptions": {"outDir": "", "rootDir": ""}}"#,
        ] {
            assert_eq!(tsconfig(valid), empty, "{valid}");
        }
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

    fn repository(
        name: &str,
        manifest_json: &str,
        files: &[(&str, &[&str])],
        config: bool,
    ) -> Export {
        let shape = |fields: &[&str]| {
            let mut shape = Shape::new(shape::Source::Literals("x.ts".into()));
            shape.fields = fields.iter().map(|f| f.to_string()).collect();
            shape
        };
        Export {
            commit: None,
            configurations: Vec::new(),
            decisions: crate::decisions::Decisions::default(),
            name_candidates: std::collections::BTreeMap::new(),
            iac: Vec::new(),
            iac_references: Vec::new(),
            dockerfiles: Vec::new(),
            http: None,
            workload_images: Vec::new(),
            executions: Vec::new(),
            repository: name.into(),
            root: PathBuf::new(),
            files: (files.iter())
                .map(|(p, _)| {
                    let mut file = index::File::default();
                    file.path = p.to_string();
                    file
                })
                .collect(),
            dropped: Vec::new(),
            symbols: Vec::new(),
            calls: Vec::new(),
            providers: Vec::new(),
            consumers: Vec::new(),
            bindings: Vec::new(),
            provision_uses: Vec::new(),
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
            resolved: Vec::new(),
            import_calls: Vec::new(),
            communities: Vec::new(),
            ancestries: Vec::new(),
            flexible: Vec::new(),
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
            provisioners: Vec::new(),
            config_uses: Vec::new(),
            scripts: Vec::new(),
            policy_grants: Vec::new(),
            config_sites: Vec::new(),
            subscriptions: Vec::new(),
            provisions: Vec::new(),
            stale_decisions: Vec::new(),
            unmounted: Vec::new(),
            unfollowed: Vec::new(),
        }
    }

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
        let mapped = repository("p1", built, &[("index.ts", &["id"])], true);
        assert_eq!(
            kept_by(&[consumer(), mapped]),
            Some((1, id.clone(), Level::Inferred))
        );
        let lost = repository("p1", built, &[("a.ts", &["id"])], false);
        assert_eq!(kept_by(&[consumer(), lost]), None);
        let lost = repository("p1", built, &[("a.ts", &["id"]), ("b.ts", &["id"])], false);
        assert_eq!(kept_by(&[consumer(), lost]), None);
        let twice = [consumer(), one(&["id"]), one(&["id"])];
        assert_eq!(kept_by(&twice), Some((1, id, Level::Exact)));
        assert_eq!(kept_by(&[consumer(), one(&["id"]), one(&["key"])]), None);
        let other = repository("c", r#"{"dependencies": {"q": "1.0.0"}}"#, &[], false);
        assert_eq!(kept_by(&[other, one(&["id"])]), None);
    }

    fn second_hop(d_rows: Vec<Exported>) -> Vec<String> {
        let sourced = |path: &str, name: &str, source: &str| Exported {
            source: Some(source.into()),
            ..row(path, Some(name), None, Some(name))
        };
        let consumer = repository("c", r#"{"dependencies": {"b": "^1.0.0"}}"#, &[], false);
        let package = |name: &str, on: &str| {
            let json = format!(
                r#"{{"name": "{name}", "version": "1.0.0", "main": "index.ts", "dependencies": {{"{on}": "^1.0.0"}}}}"#
            );
            repository(name, &json, &[("index.ts", &[])], false)
        };
        let mut b = package("b", "d");
        b.exported = vec![sourced("index.ts", "X", "d")];
        let mut d = package("d", "b");
        d.exported = d_rows;
        let exports = [consumer, b, d];
        let providers = Providers::new(&exports);
        let provided: Vec<OnceCell<Provided>> = exports.iter().map(|_| OnceCell::new()).collect();
        let found = resolutions(
            &exports,
            &providers,
            &provided,
            (0, "src/a.ts"),
            "b",
            "X",
            |_, _| true,
        );
        let mut ids: Vec<String> = (found.into_iter())
            .flat_map(|(_, r)| r)
            .flat_map(|(_, _, _, r)| r.targets)
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn a_re_export_of_another_package_resolves_one_package_further() {
        let own = row("index.ts", Some("X"), Some("index.ts"), Some("X"));
        assert_eq!(second_hop(vec![own]), ["d::index.ts::X"]);
        let star = Exported {
            source: Some("e".into()),
            ..row("index.ts", Some("*"), None, None)
        };
        assert!(second_hop(vec![star]).is_empty());
    }

    #[test]
    fn a_re_export_cycle_stops() {
        let back = Exported {
            source: Some("b".into()),
            ..row("index.ts", Some("X"), None, Some("X"))
        };
        assert!(second_hop(vec![back]).is_empty());
    }

    #[test]
    fn a_re_exported_name_is_looked_up_by_its_own_name_then_a_lone_star() {
        let rows = [
            Exported {
                source: Some("a".into()),
                ..row("i.ts", Some("Y"), None, Some("X"))
            },
            Exported {
                source: Some("s".into()),
                ..row("i.ts", Some("*"), None, None)
            },
            row("i.ts", Some("own"), Some("i.ts"), Some("own")),
            Exported {
                source: Some("t".into()),
                ..row("two.ts", Some("*"), None, None)
            },
            Exported {
                source: Some("u".into()),
                ..row("two.ts", Some("*"), None, None)
            },
        ];
        let p = provided(&rows);
        let to = |s: &str, c: &str| Some((s.to_string(), c.to_string()));
        assert_eq!(p.reexport("i.ts", "Y.deep"), to("a", "X.deep"));
        assert_eq!(p.reexport("i.ts", "Z"), to("s", "Z"));
        assert_eq!(p.reexport("i.ts", "own"), None);
        assert_eq!(p.reexport("i.ts", "default"), None);
        assert_eq!(p.reexport("two.ts", "Z"), None);
    }

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
            ancestries: &[],
            flexible: &[],
            swift: OnceCell::new(),
        }
    }

    fn row(path: &str, name: Option<&str>, file: Option<&str>, target: Option<&str>) -> Exported {
        Exported {
            path: path.into(),
            name: name.map(Into::into),
            file: file.map(Into::into),
            target: target.map(Into::into),
            source: None,
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
            row("a.ts", None, Some("i.ts"), None),
        ];
        let p = provided(&rows);
        let def = |file: &str, name: &str| Some((file.to_string(), Some(name.to_string())));
        assert_eq!(lookup(&p, "i.ts", "own"), def("d.ts", "own"));
        assert_eq!(lookup(&p, "i.ts", "deep"), def("a.ts", "deep"));
        assert_eq!(lookup(&p, "i.ts", "ns"), Some(("n.ts".into(), None)));
        assert_eq!(lookup(&p, "i.ts", "gone"), None);
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
    fn an_ecosystem_without_a_submodule_rule_reaches_no_submodule() {
        assert!(Npm.submodule("pkg/__init__.py", "api").is_empty());
        let rows = [row(
            "pkg/api.py",
            Some("fetch"),
            Some("pkg/api.py"),
            Some("fetch"),
        )];
        let mut p = provided(&rows);
        p.files = ["pkg/__init__.py", "pkg/api.py"].into_iter().collect();
        assert_eq!(
            p.reach(&Npm, "r", "pkg/__init__.py", "api.fetch", &|_| true),
            None
        );
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

        assert_eq!(
            p.candidates("r", &manifests[2], "f", &|id| p.symbol(id)),
            ["r::sealed/src/b.ts::f", "r::src/a.ts::f"]
        );
        assert!(p.owned[1].get().is_some());
        assert!(
            p.owned
                .iter()
                .enumerate()
                .all(|(i, t)| i == 1 || t.get().is_none())
        );
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

        let configs = [root];
        p.configs = &configs;
        assert_eq!(p.mapped(&manifests[0], &entries), found);
        let configs = [
            config("pkg/tsconfig.json", None, "src"),
            config("tsconfig.json", Some("pkg/dist"), "pkg/src"),
        ];
        p.configs = &configs;
        assert_eq!(p.mapped(&manifests[0], &entries), None);
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
