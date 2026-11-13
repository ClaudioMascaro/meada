//! The PyPI ecosystem (spec #24): a `pyproject.toml` or a `requirements*.txt` read into the same
//! `Summary` a `package.json` gives, its names normalized as PEP 503 does. Ranges are kept as
//! written and checked against the provider's version as PEP 440 says (`pep440`): `range
//! mismatch` when the specifiers reject it, `range unchecked` when either cannot be read.
//!
//! `Pypi` answers the trait's six required questions, `entries` among them (every distribution
//! the manifests holding the importing file depend on, nearest first, the import's module as
//! the subpath), and overrides some of the rest: `flag`, `owns` (only a `pyproject.toml` bounds a
//! package) and `resolutions`, whole: an import names a module, not a distribution, so a distribution is a
//! candidate when a provider of it exposes the import's top-level module under its source roots
//! (`exact` when that is the distribution's own name, `inferred` when it is another of its
//! modules, `ambiguous` when several distributions expose it). The source roots (`roots`), the
//! module lookup (`module`) and the console scripts (`programs`) are its own rules.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use super::{
    Ecosystem, Entry, Flag, How, Install, MAX_BYTES, MAX_NAME, Manifest, Program, Provided,
    Providers, Resolution, Resolved, Summary, bare_directory, directory, holding,
    npm::{MAX_RANGE, split},
    owner, pattern_fits, pep440, sourced,
};
use crate::contract::Level;
use crate::iac::actions::run::{self, Part, Word};
use crate::index::Export;
use crate::lang::python;
use crate::lang::python::pyproject::{Pyproject, Requirement, UvSource};
use crate::repo::{normalize_remote, relative};

/// The dependency fields, as entries and consumer sites name them.
pub const PROJECT: &str = "project.dependencies";
pub const POETRY: &str = "tool.poetry.dependencies";
pub const REQUIREMENTS: &str = "requirements";

/// Whether the file `name` is a pip requirements file: `requirements.txt`,
/// `requirements-dev.txt`, `requirements_test.txt` and the like.
pub fn is_requirements(name: &str) -> bool {
    name.starts_with("requirements") && name.ends_with(".txt")
}

/// `name` normalized as PEP 503 does (lower case, each run of `-`, `_` and `.` one `-`), when it
/// is a valid PEP 508 name (ASCII letters, digits, `-`, `_` and `.`, starting and ending with a
/// letter or digit) of at most `MAX_NAME` bytes; `None` otherwise.
pub fn normalize(name: &str) -> Option<String> {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let valid = name.len() <= MAX_NAME
        && edge(name.chars().next())
        && edge(name.chars().last())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    valid.then(|| folded(name))
}

/// The characters of `name` as PEP 503 writes them: lower case, each run of `-`, `_` and `.`
/// one `-`. Any other character stays (the `*` of a pattern).
fn folded(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '-' | '_' | '.' if out.ends_with('-') => {}
            '-' | '_' | '.' => out.push('-'),
            c => out.push(c.to_ascii_lowercase()),
        }
    }
    out
}

/// The commands that install a package by its name, by their leading words (ADR 0028, "The
/// `run` script and the command table").
const INSTALLS: [&[&str]; 6] = [
    &["pip", "install"],
    &["pip3", "install"],
    &["python", "-m", "pip", "install"],
    &["uv", "pip", "install"],
    &["pipx", "install"],
    &["uv", "tool", "install"],
];

/// The options of those commands that take no value. The word after any other option is its
/// value, never a name: a flag missing here costs the name after it, and gives no wrong one.
const FLAGS: [&str; 35] = [
    "-U",
    "--upgrade",
    "-q",
    "--quiet",
    "-v",
    "--verbose",
    "-I",
    "--ignore-installed",
    "--user",
    "--pre",
    "--no-deps",
    "--no-cache-dir",
    "--no-cache",
    "--force-reinstall",
    "--no-input",
    "--disable-pip-version-check",
    "--break-system-packages",
    "--prefer-binary",
    "--no-build-isolation",
    "--system",
    "--force",
    "--include-deps",
    "--no-index",
    "--dry-run",
    "--require-hashes",
    "--no-compile",
    "--reinstall",
    "--offline",
    "-qq",
    "-vv",
    "--isolated",
    "--ignore-requires-python",
    "--no-warn-script-location",
    "--no-warn-conflicts",
    "--no-color",
];

/// How a consumer says its name is a pattern (`Consumer::via`).
const PATTERN: &str = "pattern";

/// What the argument `word` of an install names: a requirement by name with its range as
/// written (open parts included), a pattern of the name when a part of it is open, or the name
/// left open whole. `None` for anything else: a path, a URL, a direct reference, no name.
fn installed(word: &[Part]) -> Option<Install> {
    let entry = |name, range, via: Option<&str>| {
        let via = via.map(str::to_string);
        Install::Named(Entry {
            name,
            range,
            via,
            ..Entry::default()
        })
    };
    if let Some(text) = run::literal(word) {
        let (name, range) = requirement(text)?;
        return Some(entry(name, range, None));
    }
    // The name runs to the first character a name cannot hold; the rest is read as written.
    let (mut name, mut rest, mut open) = (String::new(), String::new(), String::new());
    let mut parts = word.iter();
    for part in parts.by_ref() {
        match part {
            Part::Open(written) => {
                name.push('*');
                open += written;
            }
            Part::Text(text) => {
                let named = |c: char| c.is_ascii_alphanumeric() || "-_.".contains(c);
                let end = text.find(|c| !named(c)).unwrap_or(text.len());
                name += &text[..end];
                if end < text.len() {
                    rest += &text[end..];
                    break;
                }
            }
        }
    }
    for part in parts {
        let (Part::Text(text) | Part::Open(text)) = part;
        rest += text;
    }
    let name = folded(&name).replace("**", "*");
    // The rest, after a name that stands for this one: extras, then the range.
    let (_, range) = requirement(&format!("x{rest}"))?;
    if name.bytes().all(|b| b == b'*') {
        // No name at all (a path under `~`, a lone range) installs nothing by name.
        return (!name.is_empty()).then_some(Install::Open(open));
    }
    normalize(&name.replace('*', "x"))?;
    // A literal name whose extras or range hold the open part is the package itself.
    let pattern = name.contains('*').then_some(PATTERN);
    Some(entry(name, range, pattern))
}

/// The module name a distribution's import is spelled with when it follows the convention:
/// its normalized name with `-` turned into `_` (`acme-billing` is `acme_billing`).
pub fn import_name(normalized: &str) -> String {
    normalized.replace('-', "_")
}

/// The normalized name and the version specifier (`None` when there is none) of a PEP 508
/// requirement: extras and environment markers are dropped, and the specifier is kept as
/// written, without the parentheses around it. `None` for a direct reference (`name @ url`) or
/// anything that is no requirement.
pub fn requirement(s: &str) -> Option<(String, Option<String>)> {
    let s = s.split(';').next()?.trim();
    let end =
        (s.find(|c: char| !(c.is_ascii_alphanumeric() || "-_.".contains(c)))).unwrap_or(s.len());
    let name = normalize(&s[..end])?;
    let mut rest = s[end..].trim_start();
    if let Some(extras) = rest.strip_prefix('[') {
        rest = extras.split_once(']')?.1.trim_start();
    }
    let rest = (rest.strip_prefix('(').and_then(|r| r.strip_suffix(')'))).unwrap_or(rest);
    let rest = rest.trim();
    if !(rest.is_empty() || rest.starts_with(['<', '>', '=', '!', '~'])) {
        return None;
    }
    Some((name, (!rest.is_empty()).then(|| rest.to_string())))
}

/// The entries `found` gives under `field`, sorted by name, the first of a name kept.
fn entries(field: &str, found: impl Iterator<Item = (String, Option<String>)>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = found
        .map(|(name, range)| Entry {
            field: field.to_string(),
            name,
            range,
            ..Entry::default()
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries.dedup_by(|b, a| a.name == b.name);
    entries
}

/// Reads the `pyproject.toml` in the directory `dir` (empty at the root). Hostile input, as for
/// a `package.json`: a file over `MAX_BYTES`, not UTF-8 or not TOML reads as empty; a name that
/// is not a valid distribution name makes no provider, and a dependency that is not a valid
/// requirement is left out. A `[project] dependencies` entry takes its `[tool.uv.sources]` source
/// (`uv_source`).
pub fn read_pyproject(dir: &str, src: &[u8]) -> Summary {
    let Some(p) = (src.len() <= MAX_BYTES)
        .then(|| Pyproject::parse(src).ok())
        .flatten()
    else {
        return Summary::default();
    };
    let (mut pep621, mut poetry) = (Vec::new(), Vec::new());
    for r in p.requirements() {
        match r {
            Requirement::Pep508(s) => pep621.extend(requirement(s)),
            Requirement::Poetry(name, version) => poetry.extend(
                normalize(name).map(|n| (n, version.filter(|v| !v.is_empty()).map(String::from))),
            ),
        }
    }
    let mut dependencies = entries(PROJECT, pep621.into_iter());
    // Normalized once, so a crafted file reads in time linear in its entries; the first key of
    // a name wins, as a scan would.
    let mut sources = BTreeMap::new();
    for (key, source) in p.uv_sources() {
        if let Some(name) = normalize(key) {
            sources.entry(name).or_insert(source);
        }
    }
    for e in &mut dependencies {
        if let Some(source) = sources.get(&e.name) {
            uv_source(dir, source, e);
        }
    }
    dependencies.extend(entries(POETRY, poetry.into_iter()));
    Summary {
        name: p.name().and_then(normalize),
        version: p.version().filter(|v| !v.is_empty()).map(String::from),
        dependencies,
        roots: p.roots(),
        scripts: p.scripts(),
        uv_workspace: p.uv_workspace(),
        ..Summary::default()
    }
}

/// Gives `entry`, of a `pyproject.toml` in `dir`, what its uv `source` says, as Cargo's sources
/// do (`sourced`): `path <manifest>` for a path (the `pyproject.toml` there, folded from `dir`),
/// `git <identity>` for a git URL, `outside` for a URL, an index or a path that cannot be read
/// (absolute), and `inherits` for `workspace = true` until `Pypi::inherit` names the member.
fn uv_source(dir: &str, source: &UvSource, entry: &mut Entry) {
    entry.via = match source {
        UvSource::Workspace => {
            entry.inherits = true;
            None
        }
        UvSource::Path(path) => Some(match relative(dir, path).as_deref() {
            Some("") => "path pyproject.toml".to_string(),
            Some(at) => format!("path {at}/pyproject.toml"),
            None => "outside".to_string(),
        }),
        UvSource::Git(url) => Some(format!("git {}", normalize_remote(url))),
        UvSource::Outside => Some("outside".to_string()),
    };
}

/// The `members` and `exclude` globs of a uv workspace (`Summary::uv_workspace`).
type Globs = (Vec<String>, Vec<String>);

/// Whether the directory `dir` is a member of the uv workspace whose root is `root` (each a
/// repository path, empty at the root) and whose `members` and `exclude` globs are `globs`: the
/// root itself, or a directory one of `members` fits and none of `exclude` does (`glob`).
fn member(root: &str, (members, exclude): &Globs, dir: &str) -> bool {
    let rel = match root {
        "" => dir,
        root if dir == root => "",
        root => match dir.strip_prefix(root).and_then(|r| r.strip_prefix('/')) {
            Some(rel) => rel,
            None => return false,
        },
    };
    let fits = |globs: &[String]| (globs.iter()).any(|g| glob(g, rel));
    rel.is_empty() || (fits(members) && !fits(exclude))
}

/// The most parts (`/`-separated) a uv workspace glob is read with; a longer one fits nothing.
// ponytail: real `members` globs have a few parts; this bounds the match over a hostile one.
const MAX_GLOB_PARTS: usize = 64;

/// Whether the glob `pattern`, relative and `/`-separated, fits the relative path `path`: `**`
/// is any number of directories, and `*` in a part any run of characters within it. Matched
/// part by part without recursion, each pair of parts once, so a hostile glob stays bounded.
// ponytail: `?` and `[...]` are matched literally; read them when a real workspace uses one.
fn glob(pattern: &str, path: &str) -> bool {
    let (Some(pattern), Some(path)) = (relative("", pattern), relative("", path)) else {
        return false;
    };
    let parts = |s: &'_ str| -> Vec<String> {
        (s.split('/').filter(|p| !p.is_empty()).map(String::from)).collect()
    };
    let (pattern, path) = (parts(&pattern), parts(&path));
    if pattern.len() > MAX_GLOB_PARTS {
        return false;
    }
    // `fits[j]`: the pattern's parts so far fit the first `j` parts of the path.
    let mut fits = vec![false; path.len() + 1];
    fits[0] = true;
    for part in &pattern {
        let mut next = vec![false; path.len() + 1];
        for j in 0..=path.len() {
            next[j] = match part.as_str() {
                "**" => fits[j] || (j > 0 && next[j - 1]),
                part => j > 0 && fits[j - 1] && pattern_fits(part, &path[j - 1]),
            };
        }
        fits = next;
    }
    fits[path.len()]
}

/// Reads a `requirements*.txt` line by line, a line ending in `\` joined to the next as pip does:
/// comments (`#` at the start or after whitespace), blank lines, option lines (`-r`, `-e`,
/// `--index-url`, `--hash`, ...) and URLs or paths are skipped, and a per-requirement option
/// (` --hash=...`) is cut. Nothing the file
/// names elsewhere (`-r other.txt`) is followed. A file over `MAX_BYTES` or not UTF-8 reads as
/// empty.
pub fn read_requirements(src: &[u8]) -> Summary {
    let Some(text) = (src.len() <= MAX_BYTES)
        .then(|| std::str::from_utf8(src).ok())
        .flatten()
    else {
        return Summary::default();
    };
    // A line ending in `\` continues on the next, as pip joins them; a comment line ends it.
    let mut lines = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        match line.strip_suffix('\\') {
            Some(head) if !line.trim_start().starts_with('#') => pending.push_str(head),
            _ => {
                pending.push_str(line);
                lines.push(std::mem::take(&mut pending));
            }
        }
    }
    lines.push(pending);
    let found = lines.iter().filter_map(|line| {
        let comment = line
            .char_indices()
            .find(|&(i, c)| c == '#' && (i == 0 || line[..i].ends_with(char::is_whitespace)));
        let line = comment.map_or(line.as_str(), |(i, _)| &line[..i]).trim();
        if line.is_empty() || line.starts_with('-') || line.contains("://") {
            return None;
        }
        requirement(line.split_once(" -").map_or(line, |(r, _)| r))
    });
    Summary {
        dependencies: entries(REQUIREMENTS, found),
        ..Summary::default()
    }
}

/// The source roots of the PyPI package at `manifest`, as the Python pack reads a repository's:
/// its directory, then the roots its `pyproject.toml` declares, or its `src` when none is.
fn roots(manifest: &Manifest) -> Vec<String> {
    let base = bare_directory(&manifest.path);
    python::pyproject::source_roots(base, &manifest.summary.roots)
}

/// The file of the Python module `specifier` (dotted, `a.b`) of the PyPI package at `manifest`
/// in the provider `at`: `a/b/__init__.py` or `a/b.py` under the first of its source roots
/// (`roots`) that holds one the package owns (`Provided::owned_source`).
fn module(at: &Provided, manifest: &Manifest, specifier: &str) -> Option<String> {
    let name = specifier.replace('.', "/");
    (roots(manifest).iter())
        .flat_map(|root| python::files(root, &name))
        .find(|f| at.owned_source(manifest, f))
}

/// The PyPI ecosystem.
pub struct Pypi;

impl Ecosystem for Pypi {
    fn name(&self) -> &'static str {
        "pypi"
    }

    /// The programs the console scripts of the PyPI manifests name, each with the file of the
    /// script's module (the one an import of it from the package finds, `roots`) and the function. A
    /// script whose module is no file of the package names nothing. Sorted.
    fn programs(&self, manifests: &[Manifest], files: &[&str]) -> Vec<Program> {
        // A set: a manifest of many scripts looks each candidate up once, never scans the files.
        let files: HashSet<&str> = files.iter().copied().collect();
        let mut found = Vec::new();
        for (n, manifest) in manifests.iter().enumerate() {
            // A script belongs to the package of a manifest that bounds one.
            let file_name = manifest.path.rsplit('/').next().unwrap_or_default();
            if !(Pypi.is_manifest(file_name) && Pypi.owns(manifest)) {
                continue;
            }
            let roots = roots(manifest);
            for (name, module, function) in &manifest.summary.scripts {
                let path = module.replace('.', "/");
                // The first candidate that exists and is this package's, as `module` finds it: a
                // file of a nested package is not.
                let owned = |f: &String| {
                    files.contains(f.as_str())
                        && owner(manifests, f, &Pypi)
                            .is_some_and(|m| directory(&m.path) == directory(&manifest.path))
                };
                let file = (roots.iter())
                    .flat_map(|root| python::files(root, &path))
                    .find(owned);
                found.extend(file.map(|file| Program {
                    file,
                    function: function.clone(),
                    name: name.clone(),
                    manifest: n,
                }));
            }
        }
        found.sort();
        found
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name == "pyproject.toml" || is_requirements(file_name)
    }

    fn read(&self, path: &str, src: &[u8]) -> Summary {
        match path.ends_with("pyproject.toml") {
            true => read_pyproject(bare_directory(path), src),
            false => read_requirements(src),
        }
    }

    fn sources(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn names_package(&self, specifier: &str) -> bool {
        split(specifier).is_some()
    }

    /// Every distribution the manifests holding the file depend on, the nearest first, once
    /// each, with the import's module as the subpath.
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        _providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let mut seen = HashSet::new();
        (holding(&exports[n].manifests, from, self).into_iter())
            .flat_map(|m| &m.summary.dependencies)
            .filter(|e| seen.insert(e.name.as_str()))
            .map(|e| (e, specifier.to_string()))
            .collect()
    }

    /// Whether the dependency `range` (PEP 440 specifiers) accepts the provider's `version`:
    /// `None` when it does, else the `Flag`. A missing range or version, a range or version
    /// that is not valid PEP 440 (a Poetry `^1.2`, an unknown operator, `==1.0a1.*`) and a range
    /// past `MAX_RANGE` are `Unchecked`; an unparseable specifier is never a mismatch.
    fn flag(&self, range: Option<&str>, version: Option<&str>) -> Option<Flag> {
        let (Some(range), Some(version)) = (range, version) else {
            return Some(Flag::Unchecked);
        };
        match (range.len() <= MAX_RANGE).then(|| pep440::accepts(range, version)) {
            Some(Some(true)) => None,
            Some(Some(false)) => Some(Flag::Mismatch),
            _ => Some(Flag::Unchecked),
        }
    }

    /// A package of the consumer's own repository that no uv source names is not what pip and uv
    /// install: they take the registry's package of the name.
    fn unsourced(&self, via: Option<&str>) -> Option<&'static str> {
        // A CI step that runs a console script (`run <command>`) names no source either.
        via.is_none_or(|via| via.starts_with("run ")).then_some(
            "no uv source names the local package, the registry package is the likely provider",
        )
    }

    /// A uv source (`uv_source`) keeps the provider it names, as Cargo's does (`sourced`). A
    /// pattern of a name has no source.
    fn reaches(
        &self,
        (via, _): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        to: ((&str, &Path), &str),
    ) -> bool {
        via == Some(PATTERN) || sourced(via, from, to)
    }

    /// A pattern fits the names its literal parts leave (`iac::pattern`, the glossary's
    /// Pattern): at least three letters or digits, each `*` a whole segment.
    fn fits(&self, wanted: &str, provided: &str) -> bool {
        match wanted.contains('*') {
            true => crate::iac::pattern::matches(wanted, provided),
            false => wanted == provided,
        }
    }

    fn uncertainty(&self, via: Option<&str>) -> Option<(&'static str, Level)> {
        (via == Some(PATTERN)).then_some(("a pattern of the name", Level::Inferred))
    }

    /// A command's first word is the console script it starts. `uvx <package>` and `pipx run
    /// <package>` start the script named like the package, from that package alone: the next
    /// word, when it is a literal name and no option.
    fn runs(&self, command: &[Word]) -> Option<(String, Option<Entry>)> {
        let literal = |n: usize| command.get(n).and_then(|word| run::literal(word));
        let package = match (literal(0)?, literal(1)) {
            ("uvx", _) => 1,
            ("pipx", Some("run")) => 2,
            (name, _) => return Some((name.to_string(), None)),
        };
        let name = literal(package).filter(|name| !name.starts_with('-'))?;
        match installed(&command[package])? {
            Install::Named(entry) if entry.via.is_none() => Some((name.to_string(), Some(entry))),
            _ => None,
        }
    }

    /// The names the command installs, when its leading words are a row of `INSTALLS`: each
    /// argument that is no option and no option's value (`installed`).
    fn installs(&self, command: &[Word]) -> Vec<Install> {
        let leads = |row: &&&[&str]| {
            let mut words = row.iter().zip(command);
            row.len() <= command.len() && words.all(|(a, b)| run::literal(b) == Some(a))
        };
        let Some(row) = INSTALLS.iter().find(leads) else {
            return Vec::new();
        };
        let (mut found, mut options) = (Vec::new(), true);
        let mut words = command[row.len()..].iter();
        while let Some(word) = words.next() {
            let option = match word.first() {
                Some(Part::Text(text)) if options && text.starts_with('-') => text,
                _ => {
                    found.extend(installed(word));
                    continue;
                }
            };
            options = run::literal(word) != Some("--");
            let flag = run::literal(word).is_some_and(|text| FLAGS.contains(&text));
            if options && !flag && !option.contains('=') {
                words.next();
            }
        }
        found
    }

    /// Names the member of the consumer's uv workspace that a `workspace = true` entry of a
    /// `pyproject.toml` takes, as `path <manifest>`: the workspace's root is the nearest
    /// `pyproject.toml` with `[tool.uv.workspace]`, at or above the consumer, whose members hold
    /// it (`member`), and the member the one `pyproject.toml` of the entry's name among them.
    /// When there is no such root, or no single such member, the entry keeps no source.
    fn inherit(&self, manifests: &mut [Manifest]) {
        let pyproject = |m: &Manifest| self.owns(m);
        let roots: Vec<(&str, &Globs)> = (manifests.iter())
            .filter(|m| pyproject(m))
            .filter_map(|m| Some((bare_directory(&m.path), m.summary.uv_workspace.as_ref()?)))
            .collect();
        // By name, so each entry looks only at the packages of its name.
        let mut named: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for m in manifests.iter().filter(|m| pyproject(m)) {
            if let Some(name) = m.summary.name.as_deref() {
                named.entry(name).or_default().push(m.path.as_str());
            }
        }
        let mut found = Vec::new();
        for (i, m) in manifests.iter().enumerate().filter(|(_, m)| pyproject(m)) {
            let dir = bare_directory(&m.path);
            let root = (roots.iter())
                .filter(|(root, globs)| member(root, globs, dir))
                .max_by_key(|(root, _)| if root.is_empty() { 0 } else { root.len() + 1 });
            for (j, e) in m.summary.dependencies.iter().enumerate() {
                if !e.inherits {
                    continue;
                }
                let via = root.and_then(|(root, globs)| {
                    let mut of = (named.get(e.name.as_str())?.iter())
                        .filter(|path| member(root, globs, bare_directory(path)));
                    let path = of.next()?;
                    of.next().is_none().then(|| format!("path {path}"))
                });
                found.push((i, j, via));
            }
        }
        for (i, j, via) in found {
            let e = &mut manifests[i].summary.dependencies[j];
            e.inherits = false;
            e.via = via;
        }
    }

    /// Only a `pyproject.toml`: a `requirements*.txt` names no package, so it bounds none.
    fn owns(&self, manifest: &Manifest) -> bool {
        manifest.path.rsplit('/').next() == Some("pyproject.toml")
    }

    /// The submodules of a package's `__init__.py` (the root one or `<dir>/__init__.py`), as
    /// `import pkg` then `pkg.name.f()` reaches them: `name/__init__.py`, then `name.py`, under
    /// the package's directory. Any other file has none.
    fn submodule(&self, file: &str, name: &str) -> Vec<String> {
        let dir = match file {
            "__init__.py" => "",
            file => match file.strip_suffix("/__init__.py") {
                Some(dir) => dir,
                None => return Vec::new(),
            },
        };
        python::files(dir, name)
    }

    /// The import names a module, not a distribution: each distribution a manifest of a
    /// directory holding the file depends on (`entries`, the nearest first) is a candidate when
    /// a provider of it exposes the import's top-level module under its source roots. The import
    /// is `exact` (`import module name`) when that module is the distribution's name with `-`
    /// turned into `_`, `inferred` (`import top-level module`) when it is only one of the
    /// provider's top-level modules, and `ambiguous` when several distributions expose it. The
    /// module the import names is then found under the same roots (`module`) and `chain`
    /// followed from it.
    fn resolutions<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        from: (usize, &str),
        (specifier, chain): (&str, &str),
        known: &dyn Fn(&Provided<'e>, &str) -> bool,
    ) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
        let top = specifier.split('.').next().unwrap_or_default();
        if top.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (entry, _) in self.entries(exports, providers, from, specifier) {
            let (mut exposed, mut resolved) = (false, Vec::new());
            let source = (entry.via.as_deref(), entry.range.as_deref());
            for &(m, i) in providers.of(self.name(), &entry.name) {
                let (at, manifest) = (at(m), &exports[m].manifests[i]);
                let to = (exports[m].place(), manifest.path.as_str());
                if !self.reaches(source, exports[from.0].place(), to) {
                    continue;
                }
                if module(at, manifest, top).is_none() {
                    continue;
                }
                exposed = true;
                let (how, level) = match import_name(&entry.name) == top {
                    true => (How::ModuleName, Level::Exact),
                    false => (How::TopLevel, Level::Inferred),
                };
                let known = |id: &str| known(at, id);
                let target = (module(at, manifest, specifier))
                    .and_then(|file| at.reach(self, &exports[m].repository, &file, chain, &known));
                if let Some(target) = target {
                    let targets = vec![target];
                    let r = Resolved {
                        how,
                        level,
                        targets,
                    };
                    resolved.push((m, manifest, at, r));
                }
            }
            if exposed {
                out.push((entry, resolved));
            }
        }
        if out.len() > 1 {
            for (_, resolved) in &mut out {
                for r in resolved {
                    r.3.level = Level::Ambiguous;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{File, Symbol};
    use crate::package::Flag;
    use crate::resolve::Exported;
    use std::cell::OnceCell;
    use std::path::Path;

    #[test]
    fn a_pyproject_and_a_requirements_file_are_pypi_manifests_and_only_one_owns() {
        for name in ["pyproject.toml", "requirements.txt", "requirements-dev.txt"] {
            assert!(Pypi.is_manifest(name), "{name}");
        }
        for name in ["requirements.in", "base.txt", "package.json"] {
            assert!(!Pypi.is_manifest(name), "{name}");
        }
        let at = |path: &str| Manifest {
            path: path.to_string(),
            summary: Summary::default(),
        };
        assert!(Pypi.owns(&at("app/pyproject.toml")));
        assert!(!Pypi.owns(&at("app/requirements.txt")));
    }

    #[test]
    fn a_pypi_range_keeps_to_no_repository() {
        let (a, b) = (("a", Path::new("/w/a")), ("b", Path::new("/w/b")));
        assert!(Pypi.reaches((None, Some("workspace:*")), a, (b, "package.json")));
        // A uv source keeps what it names, as Cargo's does: `git` the repository of its remote,
        // `outside` none.
        let reaches = |via| Pypi.reaches((Some(via), None), a, (b, "x/pyproject.toml"));
        assert!(reaches("git b"));
        assert!(!reaches("git c"));
        assert!(!reaches("outside"));
        assert!(!reaches("path y/pyproject.toml"));
        assert!(Pypi.reaches(
            (Some("path x/pyproject.toml"), None),
            a,
            (a, "x/pyproject.toml")
        ));
    }

    #[test]
    fn a_pypi_range_is_checked_against_the_local_version() {
        let flag = |range, version| Pypi.flag(range, version);
        let (mismatch, unchecked) = (Some(Flag::Mismatch), Some(Flag::Unchecked));
        // The real case of #534: a pin no workspace version satisfies.
        assert_eq!(flag(Some("==24.4.2"), Some("0.1.0")), mismatch);
        assert_eq!(flag(Some("==24.4.2"), Some("24.4.2")), None);
        // A range the version satisfies adds no words, a set needs every clause.
        assert_eq!(flag(Some(">=0.1,<1"), Some("0.5.0")), None);
        assert_eq!(flag(Some(">=0.1,<1"), Some("1.0")), mismatch);
        assert_eq!(flag(Some("~=1.4.5"), Some("1.5")), mismatch);
        assert_eq!(flag(Some("==1.4.*"), Some("1.4.2")), None);
        assert_eq!(flag(Some(">=1.0"), Some("2.0rc1")), mismatch);
        // Nothing that cannot be read is a mismatch.
        assert_eq!(flag(Some("==1.0"), None), unchecked);
        assert_eq!(flag(None, Some("1.0")), unchecked);
        assert_eq!(flag(Some("^1.0"), Some("2.0")), unchecked);
        assert_eq!(flag(Some(">=1,<"), Some("2.0")), unchecked);
        assert_eq!(flag(Some("==1.0"), Some("local")), unchecked);
        let long = format!("{}1.0", "!=2.0,".repeat(MAX_RANGE));
        assert_eq!(flag(Some(&long), Some("9.0")), unchecked);
    }

    #[test]
    fn only_an_entry_with_no_source_leaves_a_local_package_unproven() {
        assert!(Pypi.unsourced(None).is_some());
        for via in ["path a/pyproject.toml", "git example.com/a/b", "outside"] {
            assert_eq!(Pypi.unsourced(Some(via)), None, "{via}");
        }
    }

    #[test]
    fn a_uv_source_gives_the_entry_where_its_package_comes_from() {
        let s = read_pyproject(
            "apps/web",
            br#"
[project]
dependencies = ["Bird_Feeder>=1", "near", "far", "remote", "wheel", "pinned", "plain", "abs"]
[tool.poetry.dependencies]
near = "1"
[tool.uv.sources]
bird-feeder = { workspace = true }
near = { path = "lib" }
far = { path = "../../../vendor/far" }
remote = { git = "https://github.com/Acme/Remote.git" }
wheel = { url = "https://example.com/wheel.whl" }
pinned = { index = "internal" }
abs = { path = "/opt/abs" }
"#,
        );
        let got: Vec<_> = (s.dependencies.iter())
            .map(|e| {
                (
                    e.field.as_str(),
                    e.name.as_str(),
                    e.via.as_deref(),
                    e.inherits,
                )
            })
            .collect();
        // A Poetry entry takes no uv source.
        assert_eq!(
            got,
            [
                (PROJECT, "abs", Some("outside"), false),
                (PROJECT, "bird-feeder", None, true),
                (
                    PROJECT,
                    "far",
                    Some("path ../vendor/far/pyproject.toml"),
                    false
                ),
                (
                    PROJECT,
                    "near",
                    Some("path apps/web/lib/pyproject.toml"),
                    false
                ),
                (PROJECT, "pinned", Some("outside"), false),
                (PROJECT, "plain", None, false),
                (PROJECT, "remote", Some("git github.com/acme/remote"), false),
                (PROJECT, "wheel", Some("outside"), false),
                (POETRY, "near", None, false),
            ]
        );
    }

    #[test]
    fn a_members_glob_less_exclude_names_the_workspace_members() {
        let globs = (
            vec!["packages/*".to_string(), "./tools/**".to_string()],
            vec!["packages/old".to_string()],
        );
        let at = |root: &str, dir: &str| member(root, &globs, dir);
        assert!(at("ws", "ws"));
        assert!(at("ws", "ws/packages/bird-feeder"));
        assert!(!at("ws", "ws/packages/old"));
        assert!(!at("ws", "ws/packages/a/b"));
        assert!(at("ws", "ws/tools/a/b"));
        assert!(!at("ws", "wsx/packages/a"));
        assert!(!at("ws", "other"));
        assert!(at("", "packages/a"));
        assert!(at("", ""));
        assert!(glob("lib-*/py", "lib-a/py"));
        assert!(!glob("lib-*", "lib-a/py"));
        assert!(glob("**", ""));
        assert!(glob("a/**/b", "a/b"));
        assert!(glob("a/**/b", "a/x/y/b"));
        assert!(!glob("a/**/b", "a/x/y"));
    }

    #[test]
    fn a_hostile_glob_stays_bounded() {
        // Past `MAX_GLOB_PARTS` a glob fits nothing; under it, many `**` against a deep path
        // finish at once.
        let deep = "**/".repeat(100_000);
        assert!(!glob(&deep, "a"));
        let many = vec!["**"; MAX_GLOB_PARTS - 1].join("/") + "/z";
        assert!(!glob(&many, &vec!["a"; 2_000].join("/")));
        assert!(glob(&many, "a/b/z"));
    }

    #[test]
    fn a_workspace_source_takes_the_member_of_the_nearest_workspace_that_holds_it() {
        let manifest = |path: &str, name: Option<&str>, ws: Option<(&str, &str)>, dep: bool| {
            let workspace = |e: &str| Entry {
                field: PROJECT.to_string(),
                name: e.to_string(),
                inherits: true,
                ..Entry::default()
            };
            Manifest {
                path: path.to_string(),
                summary: Summary {
                    name: name.map(String::from),
                    uv_workspace: ws.map(|(m, x)| (vec![m.to_string()], vec![x.to_string()])),
                    dependencies: if dep {
                        vec![workspace("bird-feeder")]
                    } else {
                        vec![]
                    },
                    ..Summary::default()
                },
            }
        };
        let mut manifests = vec![
            manifest("pyproject.toml", None, Some(("*", "ws")), true),
            manifest(
                "ws/pyproject.toml",
                Some("ws"),
                Some(("packages/*", "packages/old")),
                true,
            ),
            manifest(
                "ws/packages/bird-feeder/pyproject.toml",
                Some("bird-feeder"),
                None,
                false,
            ),
            manifest(
                "ws/packages/old/pyproject.toml",
                Some("bird-feeder"),
                None,
                false,
            ),
            manifest("ws/packages/app/pyproject.toml", Some("app"), None, true),
            manifest("ws/tools/x/pyproject.toml", Some("x"), None, true),
            manifest(
                "bird-feeder/pyproject.toml",
                Some("bird-feeder"),
                None,
                false,
            ),
            manifest("bird-feeder/requirements.txt", None, None, true),
        ];
        Pypi.inherit(&mut manifests);
        let via = |i: usize| {
            let e = &manifests[i].summary.dependencies[0];
            assert!(!e.inherits || i == 7, "{i}");
            e.via.as_deref()
        };
        // The excluded `packages/old` is no member; `ws/tools/x` is no member of `ws`, whose
        // root the top-level workspace excludes, so it finds none.
        let member = Some("path ws/packages/bird-feeder/pyproject.toml");
        assert_eq!(via(0), Some("path bird-feeder/pyproject.toml"));
        assert_eq!(via(1), member);
        assert_eq!(via(4), member);
        assert_eq!(via(5), None);
        assert!(manifests[7].summary.dependencies[0].inherits);
    }

    fn deps(s: &Summary) -> Vec<(&str, &str, Option<&str>)> {
        (s.dependencies.iter())
            .map(|e| (e.field.as_str(), e.name.as_str(), e.range.as_deref()))
            .collect()
    }

    #[test]
    fn names_normalize_as_pep_503_does() {
        let table = [
            ("Shared_Lib", Some("shared-lib")),
            ("acme.billing", Some("acme-billing")),
            ("a--_.b", Some("a-b")),
            ("A1", Some("a1")),
            ("x", Some("x")),
            ("-a", None),
            ("a-", None),
            ("a b", None),
            ("a::b", None),
            ("ä", None),
            ("", None),
        ];
        for (name, want) in table {
            assert_eq!(normalize(name).as_deref(), want, "{name:?}");
        }
        assert_eq!(
            normalize(&"a".repeat(MAX_NAME)).map(|n| n.len()),
            Some(MAX_NAME)
        );
        assert_eq!(normalize(&"a".repeat(MAX_NAME + 1)), None);
        assert_eq!(import_name("acme-billing"), "acme_billing");
    }

    #[test]
    fn a_requirement_gives_its_name_and_specifier() {
        let some = |n: &str, r: Option<&str>| Some((n.to_string(), r.map(String::from)));
        let table = [
            ("shared-lib", some("shared-lib", None)),
            ("Shared_Lib>=1.0,<2", some("shared-lib", Some(">=1.0,<2"))),
            (
                "shared-lib [fast, x] ~= 1.4",
                some("shared-lib", Some("~= 1.4")),
            ),
            ("shared-lib (>=1.0)", some("shared-lib", Some(">=1.0"))),
            (
                "shared-lib==1.*; python_version >= '3.9'",
                some("shared-lib", Some("==1.*")),
            ),
            (
                "shared-lib ; sys_platform == 'linux'",
                some("shared-lib", None),
            ),
            ("shared-lib!=1.1", some("shared-lib", Some("!=1.1"))),
            // A direct reference, an unclosed extra or text after the name is no requirement.
            ("shared-lib @ https://example.com/s.whl", None),
            ("shared-lib[fast", None),
            ("shared-lib latest", None),
            ("./local", None),
            ("", None),
        ];
        for (s, want) in table {
            assert_eq!(requirement(s), want, "{s:?}");
        }
    }

    #[test]
    fn a_requirements_file_skips_options_comments_and_urls() {
        let src = "\
# the app's dependencies
-r requirements-dev.txt
-e ./local
--index-url https://pypi.example.com/simple
shared-lib[fast]>=1.0 # pinned below 2
Shared.Tools==0.3.0 \\
    --hash=sha256:abc
git+https://example.com/x.git#egg=x
https://example.com/y.whl
y @ https://example.com/y.whl
z#not-a-comment
shared-lib==9
";
        let s = read_requirements(src.as_bytes());
        assert_eq!(
            deps(&s),
            [
                (REQUIREMENTS, "shared-lib", Some(">=1.0")),
                (REQUIREMENTS, "shared-tools", Some("==0.3.0")),
            ]
        );
        assert_eq!((s.name, s.version), (None, None));
    }

    #[test]
    fn a_line_ending_in_a_backslash_continues_on_the_next() {
        let src = "shared-lib \\\n    >=1.0,<2 \\\n    ; python_version >= '3.9'\n\
                   other\\\n-tools==1\n# a comment ends no continuation \\\nlast==2\n";
        let s = read_requirements(src.as_bytes());
        assert_eq!(
            deps(&s),
            [
                (REQUIREMENTS, "last", Some("==2")),
                (REQUIREMENTS, "other-tools", Some("==1")),
                (REQUIREMENTS, "shared-lib", Some(">=1.0,<2")),
            ]
        );
    }

    #[test]
    fn a_pyproject_reads_its_distribution_and_both_dependency_tables() {
        let src = r#"
[project]
name = "Acme_Billing"
version = "1.2.0"
dependencies = ["shared-lib[fast]>=1.0; python_version >= '3.9'", "y @ https://x", "bad name"]
[tool.poetry.dependencies]
python = "^3.11"
Shared_Tools = { path = "../tools" }
other = "^2"
[tool.setuptools.packages.find]
where = ["src"]
[project.scripts]
serve = "acme.cli:main"
"#;
        let s = read_pyproject("", src.as_bytes());
        assert_eq!(s.name.as_deref(), Some("acme-billing"));
        assert_eq!(s.version.as_deref(), Some("1.2.0"));
        assert_eq!(
            deps(&s),
            [
                (PROJECT, "shared-lib", Some(">=1.0")),
                (POETRY, "other", Some("^2")),
                (POETRY, "shared-tools", None),
            ]
        );
        assert_eq!(s.roots, ["src"]);
        let script = (
            "serve".to_string(),
            "acme.cli".to_string(),
            "main".to_string(),
        );
        assert_eq!(s.scripts, [script]);
        let poetry = read_pyproject(
            "",
            b"[tool.poetry]\nname = \"shared.tools\"\nversion = \"0.3.0\"\n",
        );
        assert_eq!(poetry.name.as_deref(), Some("shared-tools"));
        let invalid = read_pyproject("", b"[project]\nname = \"-x\"\n");
        assert_eq!(invalid.name, None);
    }

    #[test]
    fn a_manifest_too_large_or_unreadable_reads_as_empty() {
        let pad = format!("[project]\nname = \"a\"\n#{}", " ".repeat(MAX_BYTES));
        assert_eq!(read_pyproject("", pad.as_bytes()), Summary::default());
        let reqs = format!("a\n#{}", " ".repeat(MAX_BYTES));
        assert_eq!(read_requirements(reqs.as_bytes()), Summary::default());
        for bad in [&b"[project\n"[..], b"\xff", b"name = "] {
            assert_eq!(read_pyproject("", bad), Summary::default());
        }
        assert_eq!(read_requirements(b"\xffa"), Summary::default());
        let nested = format!("a = {}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert_eq!(read_pyproject("", nested.as_bytes()), Summary::default());
    }

    /// A repository `name` whose manifests are `manifests` (path, text) read as PyPI reads them,
    /// holding `files`, each exporting the function `f`.
    fn repository(name: &str, manifests: &[(&str, &str)], files: &[&str]) -> Export {
        Export {
            repository: name.into(),
            files: (files.iter())
                .map(|p| {
                    let mut file = File::default();
                    file.path = p.to_string();
                    file
                })
                .collect(),
            symbols: (files.iter())
                .map(|p| {
                    let mut symbol = Symbol::default();
                    symbol.id = format!("{name}::{p}::f");
                    symbol.span.path = p.to_string();
                    symbol
                })
                .collect(),
            manifests: (manifests.iter())
                .map(|(path, src)| Manifest {
                    path: path.to_string(),
                    summary: Pypi.read(path, src.as_bytes()),
                })
                .collect(),
            exported: (files.iter())
                .map(|p| Exported {
                    path: p.to_string(),
                    name: Some("f".into()),
                    file: Some(p.to_string()),
                    target: Some("f".into()),
                })
                .collect(),
            ..Export::default()
        }
    }

    /// The distribution `name`, in the repository `repository`, holding `files`.
    fn provider(name: &str, repository_name: &str, files: &[&str]) -> Export {
        let pyproject = format!("[project]\nname = \"{name}\"\nversion = \"1.0.0\"\n");
        repository(repository_name, &[("pyproject.toml", &pyproject)], files)
    }

    /// The repository `app`, whose `requirements.txt` says `requirements`.
    fn consumer(requirements: &str) -> Export {
        repository("app", &[("requirements.txt", requirements)], &["app/x.py"])
    }

    /// What of an import: for each entry, each provider it reaches, as the repository's position,
    /// how and at which level the import resolved, and the symbols reached.
    type Of = Vec<(String, Vec<(usize, How, Level, Vec<String>)>)>;

    /// `f`, imported from `specifier` by `app/x.py` of the first repository, as `Pypi` resolves it.
    fn resolved(exports: &[Export], specifier: &str) -> Of {
        let providers = Providers::new(exports);
        let provided: Vec<OnceCell<Provided>> = exports.iter().map(|_| OnceCell::new()).collect();
        let at = |m: usize| provided[m].get_or_init(|| Provided::new(&exports[m]));
        let known = |at: &Provided, id: &str| at.symbol(id);
        let found = Pypi.resolutions(
            exports,
            &providers,
            &at,
            (0, "app/x.py"),
            (specifier, "f"),
            &known,
        );
        (found.into_iter())
            .map(|(entry, resolved)| {
                let resolved = (resolved.into_iter())
                    .map(|(m, _, _, r)| (m, r.how, r.level, r.targets))
                    .collect();
                (entry.name.clone(), resolved)
            })
            .collect()
    }

    #[test]
    fn the_entries_are_every_distribution_the_holding_manifests_depend_on_nearest_first() {
        let root = "[project]\nname = \"root\"\ndependencies = [\"far\", \"both\"]\n";
        let near = "[project]\nname = \"near\"\ndependencies = [\"both\", \"near-dep\"]\n";
        let exports = [repository(
            "app",
            &[
                ("pyproject.toml", root),
                ("requirements.txt", "root-reqs\nboth\n"),
                ("app/requirements-dev.txt", "dev-tool\n"),
                ("app/pyproject.toml", near),
                ("other/requirements.txt", "elsewhere\n"),
            ],
            &["app/x.py"],
        )];
        let providers = Providers::new(&exports);
        let found = Pypi.entries(&exports, &providers, (0, "app/x.py"), "a.b");
        let names: Vec<_> = found.iter().map(|(e, _)| e.name.as_str()).collect();
        // The deepest directory first, a `pyproject.toml` before the requirements beside it,
        // each distribution once, from its nearest manifest.
        assert_eq!(names, ["both", "near-dep", "dev-tool", "far", "root-reqs"]);
        assert_eq!(found[0].0.field, PROJECT);
        assert!(found.iter().all(|(_, subpath)| subpath == "a.b"));
    }

    #[test]
    fn an_import_of_the_distributions_own_module_is_exact() {
        let files = ["shared_lib/__init__.py", "shared_lib/util.py"];
        let exports = [
            consumer("shared-lib>=1.0\n"),
            provider("shared-lib", "lib", &files),
        ];
        let id = |p: &str| vec![format!("lib::{p}::f")];
        let exact = |p: &str| vec![(1, How::ModuleName, Level::Exact, id(p))];
        assert_eq!(
            resolved(&exports, "shared_lib"),
            [("shared-lib".to_string(), exact("shared_lib/__init__.py"))]
        );
        // The module the import names is found under the same roots.
        assert_eq!(
            resolved(&exports, "shared_lib.util"),
            [("shared-lib".to_string(), exact("shared_lib/util.py"))]
        );
    }

    #[test]
    fn an_import_of_another_top_level_module_of_the_distribution_is_inferred() {
        let exports = [
            consumer("acme-tools\n"),
            provider("acme-tools", "lib", &["tools_core/__init__.py"]),
        ];
        let id = vec!["lib::tools_core/__init__.py::f".to_string()];
        assert_eq!(
            resolved(&exports, "tools_core"),
            [(
                "acme-tools".to_string(),
                vec![(1, How::TopLevel, Level::Inferred, id)]
            )]
        );
        // A distribution that does not expose the module is no candidate.
        assert!(resolved(&exports, "missing").is_empty());
    }

    #[test]
    fn an_import_two_distributions_expose_is_ambiguous_in_each() {
        let exports = [
            consumer("lib-a\nlib-b\nlib-c\n"),
            provider("lib-a", "a", &["shared/__init__.py"]),
            provider("lib-b", "b", &["shared/__init__.py"]),
            provider("lib-c", "c", &["lib_c/__init__.py"]),
        ];
        let ambiguous = |m: usize, r: &str| {
            let id = vec![format!("{r}::shared/__init__.py::f")];
            vec![(m, How::TopLevel, Level::Ambiguous, id)]
        };
        assert_eq!(
            resolved(&exports, "shared"),
            [
                ("lib-a".to_string(), ambiguous(1, "a")),
                ("lib-b".to_string(), ambiguous(2, "b")),
            ]
        );
        // One alone is not.
        assert_eq!(resolved(&exports, "lib_c")[0].1[0].2, Level::Exact);
    }

    /// What `Pypi::runs` reads of the one command of `script`: the name it runs, then the
    /// package and its range when the command names the package.
    fn ran(script: &str) -> Option<String> {
        let commands = run::commands(script, |_| None).unwrap();
        let (name, package) = Pypi.runs(&commands[0])?;
        Some(match package {
            Some(e) => format!("{name} of {} {}", e.name, e.range.as_deref().unwrap_or("-")),
            None => name,
        })
    }

    #[test]
    fn a_command_runs_the_script_of_its_first_word_or_of_the_package_it_names() {
        let ran = |script: &str| ran(script).unwrap_or_else(|| "nothing".into());
        assert_eq!(ran("core-publish --to prod x"), "core-publish");
        // The name stays as written: pip installs the command under it byte for byte.
        assert_eq!(ran("Core_Publish"), "Core_Publish");
        assert_eq!(ran("'core-publish' x"), "core-publish");
        // An install is the command `pipx`, which no package declares.
        assert_eq!(ran("pipx install core-sdk"), "pipx");
        assert_eq!(ran("uvx core-sdk status"), "core-sdk of core-sdk -");
        assert_eq!(ran("pipx run Core_SDK"), "Core_SDK of core-sdk -");
        assert_eq!(
            ran("uvx 'core-sdk>=1.0'"),
            "core-sdk>=1.0 of core-sdk >=1.0"
        );
        // No package named as the next word: an option, nothing, a path, an open part.
        for script in [
            "uvx --from core-sdk core-publish",
            "uvx -q core-sdk",
            "uvx",
            "pipx run",
            "pipx run --spec core-sdk core-publish",
            "uvx ./tools/core",
            "uvx core-sdk@1.0",
            "uvx $PKG",
            "uvx core-${{ matrix.tool }}",
            "$TOOL run",
            "${{ vars.TOOL }} x",
        ] {
            assert_eq!(ran(script), "nothing", "{script}");
        }
    }

    /// A manifest whose console scripts are `scripts`, each named `run-<function>`.
    fn script_manifest(path: &str, scripts: &[(&str, &str)], roots: &[&str]) -> Manifest {
        Manifest {
            path: path.to_string(),
            summary: Summary {
                scripts: (scripts.iter())
                    .map(|(m, f)| (format!("run-{f}"), m.to_string(), f.to_string()))
                    .collect(),
                roots: roots.iter().map(|r| r.to_string()).collect(),
                ..Summary::default()
            },
        }
    }

    #[test]
    fn a_console_script_names_the_function_of_the_file_its_module_is() {
        let all = [
            script_manifest(
                "pyproject.toml",
                &[("app.cli", "main"), ("tool", "run")],
                &[],
            ),
            script_manifest(
                "lib/pyproject.toml",
                &[("pkg", "start"), ("none", "x")],
                &["src"],
            ),
            script_manifest(
                "outer/pyproject.toml",
                &[("inner", "go"), ("pkg.cli", "main")],
                &[],
            ),
            script_manifest("outer/inner/pyproject.toml", &[], &[]),
        ];
        let files = [
            "app/cli.py",
            "tool/__init__.py",
            "tool.py",
            "lib/src/pkg/__init__.py",
            "outer/inner.py",
            "outer/pkg.py",
            "outer/inner/pkg/cli.py",
        ];

        let found = Pypi.programs(&all, &files);

        // `tool/__init__.py` over `tool.py`, as an import finds it; the `src` root of `lib`;
        // `none` has no file; `outer`'s `inner.py` is its own, and `pkg/cli.py` is not in
        // `outer`'s reach. Each keeps the script's name and its manifest (#832).
        let program = |file: &str, function: &str, manifest| Program {
            file: file.to_string(),
            function: function.to_string(),
            name: format!("run-{function}"),
            manifest,
        };
        assert_eq!(
            found,
            [
                program("app/cli.py", "main", 0),
                program("lib/src/pkg/__init__.py", "start", 1),
                program("outer/inner.py", "go", 2),
                program("tool/__init__.py", "run", 0),
            ]
        );
    }

    #[test]
    fn a_console_script_skips_a_candidate_another_package_owns() {
        let all = [
            script_manifest("pyproject.toml", &[("inner.mod", "go")], &[]),
            script_manifest("inner/pyproject.toml", &[], &[]),
        ];
        // `inner/mod.py` comes first among the roots but belongs to the nested package.
        let files = ["inner/mod.py", "src/inner/mod.py"];

        let found = Pypi.programs(&all, &files);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (found[0].file.as_str(), found[0].function.as_str()),
            ("src/inner/mod.py", "go")
        );
    }

    #[test]
    fn a_package_init_names_its_submodules_and_no_other_file_does() {
        let files = |file: &str| Pypi.submodule(file, "name");
        assert_eq!(
            files("pkg/__init__.py"),
            ["pkg/name/__init__.py", "pkg/name.py"]
        );
        assert_eq!(files("__init__.py"), ["name/__init__.py", "name.py"]);
        assert!(files("pkg/core.py").is_empty());
        assert!(files("pkg/x__init__.py").is_empty());
    }

    #[test]
    fn a_submodule_is_reached_through_the_init_that_exports_no_such_name() {
        let row = |path: &str, name: &str| Exported {
            path: path.into(),
            name: Some(name.into()),
            file: Some(path.into()),
            target: Some(name.into()),
        };
        let export = Export {
            repository: "lib".into(),
            files: (["pkg/__init__.py", "pkg/api.py"].iter())
                .map(|p| {
                    let mut file = File::default();
                    file.path = p.to_string();
                    file
                })
                .collect(),
            exported: vec![row("pkg/api.py", "fetch")],
            ..Export::default()
        };
        let at = Provided::new(&export);
        let reach = |chain: &str| at.reach(&Pypi, "lib", "pkg/__init__.py", chain, &|_| true);
        assert_eq!(
            reach("api.fetch").as_deref(),
            Some("lib::pkg/api.py::fetch")
        );
        assert_eq!(reach("other.fetch"), None);
    }

    #[test]
    fn a_requirements_file_declares_no_console_script() {
        let all = [script_manifest("requirements.txt", &[("app", "main")], &[])];
        assert!(Pypi.programs(&all, &["app.py"]).is_empty());
    }
}
