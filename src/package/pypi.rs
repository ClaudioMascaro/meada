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

pub const PROJECT: &str = "project.dependencies";
pub const POETRY: &str = "tool.poetry.dependencies";
pub const REQUIREMENTS: &str = "requirements";

pub fn is_requirements(name: &str) -> bool {
    name.starts_with("requirements") && name.ends_with(".txt")
}

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

const INSTALLS: [&[&str]; 6] = [
    &["pip", "install"],
    &["pip3", "install"],
    &["python", "-m", "pip", "install"],
    &["uv", "pip", "install"],
    &["pipx", "install"],
    &["uv", "tool", "install"],
];

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

const PATTERN: &str = "pattern";

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
    let mut tail = None;
    for part in parts {
        if matches!(part, Part::Open(_)) && rest.ends_with(']') {
            tail.get_or_insert(rest.len());
        }
        let (Part::Text(text) | Part::Open(text)) = part;
        rest += text;
    }
    let name = folded(&name).replace("**", "*");
    let (rest, tail) = rest.split_at(tail.unwrap_or(rest.len()));
    let (_, range) = requirement(&format!("x{rest}"))?;
    let range = range.or((!tail.is_empty()).then(|| tail.to_string()));
    if name.bytes().all(|b| b == b'*') {
        return (!name.is_empty()).then_some(Install::Open(open));
    }
    normalize(&name.replace('*', "x"))?;
    let pattern = name.contains('*').then_some(PATTERN);
    Some(entry(name, range, pattern))
}

pub fn import_name(normalized: &str) -> String {
    normalized.replace('-', "_")
}

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

type Globs = (Vec<String>, Vec<String>);

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

// ponytail: real `members` globs have a few parts; this bounds the match over a hostile one.
const MAX_GLOB_PARTS: usize = 64;

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

pub fn read_requirements(src: &[u8]) -> Summary {
    let Some(text) = (src.len() <= MAX_BYTES)
        .then(|| std::str::from_utf8(src).ok())
        .flatten()
    else {
        return Summary::default();
    };
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

fn roots(manifest: &Manifest) -> Vec<String> {
    let base = bare_directory(&manifest.path);
    python::pyproject::source_roots(base, &manifest.summary.roots)
}

fn module(at: &Provided, manifest: &Manifest, specifier: &str) -> Option<String> {
    let name = specifier.replace('.', "/");
    (roots(manifest).iter())
        .flat_map(|root| python::files(root, &name))
        .find(|f| at.owned_source(manifest, f))
}

pub struct Pypi;

impl Ecosystem for Pypi {
    fn name(&self) -> &'static str {
        "pypi"
    }

    fn programs(&self, manifests: &[Manifest], files: &[&str]) -> Vec<Program> {
        let files: HashSet<&str> = files.iter().copied().collect();
        let mut found = Vec::new();
        for (n, manifest) in manifests.iter().enumerate() {
            let file_name = manifest.path.rsplit('/').next().unwrap_or_default();
            if !(Pypi.is_manifest(file_name) && Pypi.owns(manifest)) {
                continue;
            }
            let roots = roots(manifest);
            for (name, module, function) in &manifest.summary.scripts {
                let path = module.replace('.', "/");
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

    fn unsourced(&self, via: Option<&str>) -> Option<&'static str> {
        via.is_none_or(|via| via.starts_with("run ")).then_some(
            "no uv source names the local package, the registry package is the likely provider",
        )
    }

    fn reaches(
        &self,
        (via, _): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        to: ((&str, &Path), &str),
    ) -> bool {
        via == Some(PATTERN) || sourced(via, from, to)
    }

    fn fits(&self, wanted: &str, provided: &str) -> bool {
        match wanted.contains('*') {
            true => crate::iac::pattern::matches(wanted, provided),
            false => wanted == provided,
        }
    }

    fn uncertainty(&self, via: Option<&str>) -> Option<(&'static str, Level)> {
        (via == Some(PATTERN)).then_some(("a pattern of the name", Level::Inferred))
    }

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

    fn inherit(&self, manifests: &mut [Manifest]) {
        let pyproject = |m: &Manifest| self.owns(m);
        let roots: Vec<(&str, &Globs)> = (manifests.iter())
            .filter(|m| pyproject(m))
            .filter_map(|m| Some((bare_directory(&m.path), m.summary.uv_workspace.as_ref()?)))
            .collect();
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

    fn owns(&self, manifest: &Manifest) -> bool {
        manifest.path.rsplit('/').next() == Some("pyproject.toml")
    }

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
        assert_eq!(flag(Some("==24.4.2"), Some("0.1.0")), mismatch);
        assert_eq!(flag(Some("==24.4.2"), Some("24.4.2")), None);
        assert_eq!(flag(Some(">=0.1,<1"), Some("0.5.0")), None);
        assert_eq!(flag(Some(">=0.1,<1"), Some("1.0")), mismatch);
        assert_eq!(flag(Some("~=1.4.5"), Some("1.5")), mismatch);
        assert_eq!(flag(Some("==1.4.*"), Some("1.4.2")), None);
        assert_eq!(flag(Some(">=1.0"), Some("2.0rc1")), mismatch);
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
                    source: None,
                })
                .collect(),
            ..Export::default()
        }
    }

    fn provider(name: &str, repository_name: &str, files: &[&str]) -> Export {
        let pyproject = format!("[project]\nname = \"{name}\"\nversion = \"1.0.0\"\n");
        repository(repository_name, &[("pyproject.toml", &pyproject)], files)
    }

    fn consumer(requirements: &str) -> Export {
        repository("app", &[("requirements.txt", requirements)], &["app/x.py"])
    }

    type Of = Vec<(String, Vec<(usize, How, Level, Vec<String>)>)>;

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
        assert_eq!(resolved(&exports, "lib_c")[0].1[0].2, Level::Exact);
    }

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
        assert_eq!(ran("Core_Publish"), "Core_Publish");
        assert_eq!(ran("'core-publish' x"), "core-publish");
        assert_eq!(ran("pipx install core-sdk"), "pipx");
        assert_eq!(ran("uvx core-sdk status"), "core-sdk of core-sdk -");
        assert_eq!(ran("pipx run Core_SDK"), "Core_SDK of core-sdk -");
        assert_eq!(
            ran("uvx 'core-sdk>=1.0'"),
            "core-sdk>=1.0 of core-sdk >=1.0"
        );
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
            source: None,
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
