//! The PyPI ecosystem (spec #24): a `pyproject.toml` or a `requirements*.txt` read into the same
//! `Summary` a `package.json` gives, its names normalized as PEP 503 does. Ranges are kept as
//! written and never evaluated: every PyPI package link says `range unchecked`.
//!
//! `Pypi` answers the trait's six required questions, `entries` among them (every distribution
//! the manifests holding the importing file depend on, nearest first, the import's module as
//! the subpath), and overrides two of the rest: `owns` (only a `pyproject.toml` bounds a
//! package) and `resolutions`, whole: an import names a module, not a distribution, so a distribution is a
//! candidate when a provider of it exposes the import's top-level module under its source roots
//! (`exact` when that is the distribution's own name, `inferred` when it is another of its
//! modules, `ambiguous` when several distributions expose it). The source roots (`roots`), the
//! module lookup (`module`) and the console scripts (`programs`) are its own rules.

use std::collections::HashSet;

use super::{
    Ecosystem, Entry, How, MAX_BYTES, MAX_NAME, Manifest, Provided, Providers, Resolution,
    Resolved, Summary, directory, holding, owner, split,
};
use crate::contract::Level;
use crate::index::Export;
use crate::lang::python;
use crate::lang::python::pyproject::{Pyproject, Requirement};

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
    if !valid {
        return None;
    }
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '-' | '_' | '.' if out.ends_with('-') => {}
            '-' | '_' | '.' => out.push('-'),
            c => out.push(c.to_ascii_lowercase()),
        }
    }
    Some(out)
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

/// Reads a `pyproject.toml`. Hostile input, as for a `package.json`: a file over `MAX_BYTES`,
/// not UTF-8 or not TOML reads as empty; a name that is not a valid distribution name makes no
/// provider, and a dependency that is not a valid requirement is left out.
pub fn read_pyproject(src: &[u8]) -> Summary {
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
    dependencies.extend(entries(POETRY, poetry.into_iter()));
    Summary {
        name: p.name().and_then(normalize),
        version: p.version().filter(|v| !v.is_empty()).map(String::from),
        dependencies,
        roots: p.roots(),
        scripts: p.scripts(),
        ..Summary::default()
    }
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
    let base = match directory(&manifest.path) {
        "." => "",
        dir => dir,
    };
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

/// The program entry points the console scripts of the PyPI manifests name, as the file of the
/// script's module (the one an import of it from the package finds, `roots`) and the function. A
/// script whose module is no file of the package names nothing. Sorted, without repeats.
fn programs(manifests: &[Manifest], files: &[&str]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for manifest in manifests {
        // A script belongs to the package of a manifest that bounds one.
        let file_name = manifest.path.rsplit('/').next().unwrap_or_default();
        if !(Pypi.is_manifest(file_name) && Pypi.owns(manifest)) {
            continue;
        }
        let roots = roots(manifest);
        for (module, function) in &manifest.summary.scripts {
            let name = module.replace('.', "/");
            // The first candidate that exists and is this package's, as `module` finds it: a
            // file of a nested package is not.
            let owned = |f: &String| {
                files.contains(&f.as_str())
                    && owner(manifests, f, &Pypi)
                        .is_some_and(|m| directory(&m.path) == directory(&manifest.path))
            };
            let file = (roots.iter())
                .flat_map(|root| python::files(root, &name))
                .find(owned);
            found.extend(file.map(|f| (f, function.clone())));
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The PyPI ecosystem.
pub struct Pypi;

impl Ecosystem for Pypi {
    fn name(&self) -> &'static str {
        "pypi"
    }

    fn programs(&self, manifests: &[Manifest], files: &[&str]) -> Vec<(String, String)> {
        programs(manifests, files)
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name == "pyproject.toml" || is_requirements(file_name)
    }

    fn read(&self, path: &str, src: &[u8]) -> Summary {
        match path.ends_with("pyproject.toml") {
            true => read_pyproject(src),
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

    /// Only a `pyproject.toml`: a `requirements*.txt` names no package, so it bounds none.
    fn owns(&self, manifest: &Manifest) -> bool {
        manifest.path.rsplit('/').next() == Some("pyproject.toml")
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
            for &(m, i) in providers.of(self.name(), &entry.name) {
                let (at, manifest) = (at(m), &exports[m].manifests[i]);
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
                    .and_then(|file| at.reach(&exports[m].repository, &file, chain, &known));
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
    use crate::index::{File, Span, Symbol};
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
    fn a_pypi_range_is_never_evaluated_and_keeps_to_no_repository() {
        assert_eq!(Pypi.flag(Some("==1.0"), Some("1.0")), Some(Flag::Unchecked));
        let (a, b) = (("a", Path::new("/w/a")), ("b", Path::new("/w/b")));
        assert!(Pypi.reaches((None, Some("workspace:*")), a, (b, "package.json")));
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
        let s = read_pyproject(src.as_bytes());
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
        assert_eq!(s.scripts, [("acme.cli".to_string(), "main".to_string())]);
        let poetry =
            read_pyproject(b"[tool.poetry]\nname = \"shared.tools\"\nversion = \"0.3.0\"\n");
        assert_eq!(poetry.name.as_deref(), Some("shared-tools"));
        let invalid = read_pyproject(b"[project]\nname = \"-x\"\n");
        assert_eq!(invalid.name, None);
    }

    #[test]
    fn a_manifest_too_large_or_unreadable_reads_as_empty() {
        let pad = format!("[project]\nname = \"a\"\n#{}", " ".repeat(MAX_BYTES));
        assert_eq!(read_pyproject(pad.as_bytes()), Summary::default());
        let reqs = format!("a\n#{}", " ".repeat(MAX_BYTES));
        assert_eq!(read_requirements(reqs.as_bytes()), Summary::default());
        for bad in [&b"[project\n"[..], b"\xff", b"name = "] {
            assert_eq!(read_pyproject(bad), Summary::default());
        }
        assert_eq!(read_requirements(b"\xffa"), Summary::default());
        let nested = format!("a = {}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert_eq!(read_pyproject(nested.as_bytes()), Summary::default());
    }

    /// A repository `name` whose manifests are `manifests` (path, text) read as PyPI reads them,
    /// holding `files`, each exporting the function `f`.
    fn repository(name: &str, manifests: &[(&str, &str)], files: &[&str]) -> Export {
        Export {
            repository: name.into(),
            files: (files.iter())
                .map(|p| File {
                    path: p.to_string(),
                    hash: String::new(),
                })
                .collect(),
            symbols: (files.iter())
                .map(|p| Symbol {
                    id: format!("{name}::{p}::f"),
                    span: Span {
                        path: p.to_string(),
                        start: 0,
                        end: 0,
                    },
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

    fn script_manifest(path: &str, scripts: &[(&str, &str)], roots: &[&str]) -> Manifest {
        Manifest {
            path: path.to_string(),
            summary: Summary {
                scripts: (scripts.iter())
                    .map(|(m, f)| (m.to_string(), f.to_string()))
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

        let found = programs(&all, &files);

        // `tool/__init__.py` over `tool.py`, as an import finds it; the `src` root of `lib`;
        // `none` has no file; `outer`'s `inner.py` is its own, and `pkg/cli.py` is not in
        // `outer`'s reach.
        assert_eq!(
            found,
            [
                ("app/cli.py".to_string(), "main".to_string()),
                ("lib/src/pkg/__init__.py".to_string(), "start".to_string()),
                ("outer/inner.py".to_string(), "go".to_string()),
                ("tool/__init__.py".to_string(), "run".to_string()),
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

        assert_eq!(
            programs(&all, &files),
            [("src/inner/mod.py".to_string(), "go".to_string())]
        );
    }

    #[test]
    fn a_requirements_file_declares_no_console_script() {
        let all = [script_manifest("requirements.txt", &[("app", "main")], &[])];
        assert!(programs(&all, &["app.py"]).is_empty());
    }
}
