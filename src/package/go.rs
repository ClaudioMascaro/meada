//! Go packages (spec #25): a `go.mod` with a `module` path provides `package::go::<module path>`,
//! and each `require` entry, and each `replace` to a local directory, consumes the module it
//! names. The lines come from the Go pack's one `go.mod` reader (`lang::go::modfile`). A
//! `go.mod` carries no version of its own, so a provider's version is always none.
//!
//! An entry keeps where its module comes from in `Entry::via`, as Cargo's does: `path
//! <go.mod>` for a `replace` to a local directory (folded lexically from the manifest's
//! directory: a path of the repository, or one that starts with `../` when it leaves it),
//! `outside` for a `replace` by anything else (a module, an absolute path), which leaves the
//! module out of the workspace, and nothing otherwise. Ranges are never checked, and neither
//! `go.sum` nor the module proxy is read.
//!
//! Each rule answers one question of `Ecosystem`:
//! - `names_package`: any valid contract name may name a module (a module path needs no dot).
//! - `entries`: the entry of the owning `go.mod` with the longest module path the import starts
//!   with, the rest of the import as the subpath.
//! - `reaches`: a local `replace` is a path source, the rule Cargo's `path` follows (`sourced`).
//! - `resolve`: the one package directory the subpath names in the module, test files and the
//!   files of a nested module left out, through the provider view's directory table.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::{
    Ecosystem, Entry, How, MAX_BYTES, Manifest, Provided, Providers, Resolved, Summary,
    bare_directory, directory, inside, owner, sourced, valid_name,
};
use crate::contract::Level;
use crate::index::Export;
use crate::lang::go::modfile;
use crate::repo::relative;

/// The field a `require` entry is kept under.
const REQUIRE: &str = "require";
/// The field of an entry only a `replace` gives (a module some other module requires).
const REPLACE: &str = "replace";

/// The Go ecosystem.
pub struct Go;

impl Ecosystem for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name == "go.mod"
    }

    /// Reads a `go.mod` from its bytes. Hostile input: a file too large reads as empty, a line it
    /// cannot read is skipped, and a module path that cannot be a contract name makes no provider
    /// and no entry.
    fn read(&self, path: &str, src: &[u8]) -> Summary {
        if src.len() > MAX_BYTES {
            return Summary::default();
        }
        let dir = bare_directory(path);
        let file = modfile::read(&String::from_utf8_lossy(src));
        // The first `replace` of a module decides, found once for all.
        let mut replaced: HashMap<&str, &str> = HashMap::new();
        for (old, new) in &file.replaces {
            replaced.entry(old).or_insert(new);
        }
        let via = |module: &str| {
            let target = replaced.get(module)?;
            let local = modfile::local_target(target).then(|| relative(dir, target));
            Some(match local.flatten() {
                Some(at) if at.is_empty() => "path go.mod".to_string(),
                Some(at) => format!("path {at}/go.mod"),
                None => "outside".to_string(),
            })
        };
        let mut dependencies: Vec<Entry> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for (module, version) in &file.requires {
            if valid_name(module) && seen.insert(module) {
                dependencies.push(Entry {
                    field: REQUIRE.to_string(),
                    name: module.clone(),
                    range: Some(version.clone()),
                    via: via(module),
                    ..Entry::default()
                });
            }
        }
        // A replaced module nobody here requires may be one a dependency requires.
        for (module, _) in &file.replaces {
            let via = via(module).filter(|v| v.starts_with("path "));
            if via.is_some() && valid_name(module) && seen.insert(module) {
                dependencies.push(Entry {
                    field: REPLACE.to_string(),
                    name: module.clone(),
                    via,
                    ..Entry::default()
                });
            }
        }
        dependencies.sort_by(|a, b| (&a.field, &a.name).cmp(&(&b.field, &b.name)));
        Summary {
            name: file.module.filter(|m| valid_name(m)),
            dependencies,
            ..Summary::default()
        }
    }

    fn sources(&self) -> &'static [&'static str] {
        &["go"]
    }

    /// Any path that can be a contract name may name a module of the workspace: a module path
    /// needs no dot (`module kit` with a local `replace`), and the standard library stays
    /// external since no `require` names it.
    fn names_package(&self, specifier: &str) -> bool {
        valid_name(specifier)
    }

    /// The entry of the owning `go.mod` with the longest module path the import starts with,
    /// the rest of the import as the subpath.
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        _providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let found = (owner(&exports[n].manifests, from, self)).and_then(|m| {
            (m.summary.dependencies.iter())
                .filter_map(|e| Some((e, modfile::inside(specifier, &e.name)?)))
                .max_by_key(|(e, _)| e.name.len())
        });
        let subpath = |rest: &str| match rest {
            "" => ".".to_string(),
            rest => format!("./{rest}"),
        };
        found
            .map(|(e, rest)| (e, subpath(rest)))
            .into_iter()
            .collect()
    }

    /// A local `replace` by the folded location, as Cargo's `path` (`sourced`).
    fn reaches(
        &self,
        (via, _): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        to: ((&str, &Path), &str),
    ) -> bool {
        sourced(via, from, to)
    }

    /// The symbol `chain` names in the Go package at `subpath` (`.` for the module's root, else
    /// `./<directory>`) of the module at `manifest`: the exported name of the one source file of
    /// that directory (a test file or one of a nested module is none) that defines it. `exact`
    /// when found; none when the directory lacks or the name is not exported there.
    fn resolve(
        &self,
        at: &Provided,
        repository: &str,
        manifest: &Manifest,
        (_, subpath): (&Entry, &str),
        (_, chain): (&str, &str),
        known: &dyn Fn(&str) -> bool,
    ) -> Option<Resolved> {
        let base = directory(&manifest.path);
        let dir = match subpath.strip_prefix("./") {
            Some(rest) => inside(base, rest)?,
            None if base == "." => String::new(),
            None => base.to_string(),
        };
        let by_dir = at.go_dirs.get_or_init(|| {
            let mut by_dir: HashMap<&str, Vec<&str>> = HashMap::new();
            for &f in at.files.iter().filter(|f| f.ends_with(".go")) {
                if !f.ends_with("_test.go") {
                    by_dir
                        .entry(f.rsplit_once('/').map_or("", |(d, _)| d))
                        .or_default()
                        .push(f);
                }
            }
            by_dir
        });
        let mut ids: Vec<String> = (by_dir.get(dir.as_str()).into_iter().flatten())
            .filter(|f| at.owned_source(manifest, f))
            .filter_map(|f| at.reach(repository, f, chain, known))
            .collect();
        ids.sort();
        ids.dedup();
        (ids.len() == 1).then_some(Resolved {
            how: How::ModulePath,
            level: Level::Exact,
            targets: ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &Summary) -> Vec<(&str, &str, Option<&str>, Option<&str>)> {
        (s.dependencies.iter())
            .map(|e| {
                (
                    e.field.as_str(),
                    e.name.as_str(),
                    e.range.as_deref(),
                    e.via.as_deref(),
                )
            })
            .collect()
    }

    #[test]
    fn a_go_mod_gives_its_module_its_requires_and_where_a_replace_sends_them() {
        let s = Go.read(
            "app/go.mod",
            b"module example.com/app\ngo 1.22\n\
              require (\n\texample.com/a v1.0.0\n\texample.com/b v1.1.0 // indirect\n\texample.com/c v1.2.0\n\texample.com/d v1.3.0\n)\n\
              require example.com/a v9.9.9\n\
              replace example.com/a => ../a\n\
              replace example.com/b v1.1.0 => example.com/fork v1.0.0\n\
              replace example.com/c => ./vendor/c\n\
              replace example.com/e => ../../e\n\
              replace example.com/f => example.com/fork v1.0.0\n",
        );
        assert_eq!(s.name.as_deref(), Some("example.com/app"));
        assert_eq!(s.version, None);
        assert_eq!(
            rows(&s),
            [
                ("replace", "example.com/e", None, Some("path ../e/go.mod")),
                (
                    "require",
                    "example.com/a",
                    Some("v1.0.0"),
                    Some("path a/go.mod")
                ),
                ("require", "example.com/b", Some("v1.1.0"), Some("outside")),
                (
                    "require",
                    "example.com/c",
                    Some("v1.2.0"),
                    Some("path app/vendor/c/go.mod")
                ),
                ("require", "example.com/d", Some("v1.3.0"), None),
            ]
        );
    }

    #[test]
    fn a_module_without_a_usable_path_provides_nothing_and_a_hostile_file_reads_as_empty() {
        assert_eq!(Go.read("go.mod", b"go 1.22\n").name, None);
        assert_eq!(Go.read("go.mod", b"module a:b\n").name, None);
        assert_eq!(Go.read("go.mod", &[0xff, 0xfe, b'\n']).dependencies, []);
        assert_eq!(
            Go.read("go.mod", &vec![b'x'; MAX_BYTES + 1]),
            Summary::default()
        );
        // A replace at the root of the repository names its `go.mod`.
        let s = Go.read("go.mod", b"replace example.com/a => .\n");
        assert_eq!(
            rows(&s),
            [("replace", "example.com/a", None, Some("path go.mod"))]
        );
    }

    #[test]
    fn an_import_path_may_name_a_module_and_the_standard_library_is_left_to_the_requires() {
        assert!(valid_name("example.com/a/b"));
        // A module path needs no dot: a local one is `module kit` with a `replace`.
        assert!(valid_name("kit/text"));
        assert!(!valid_name(""));
        assert!(!valid_name("a:b"));
        assert_eq!(
            modfile::inside("example.com/a/b/c", "example.com/a"),
            Some("b/c")
        );
        assert_eq!(modfile::inside("example.com/a", "example.com/a"), Some(""));
        assert_eq!(modfile::inside("example.com/ab", "example.com/a"), None);
    }

    fn manifest(path: &str, src: &str) -> Manifest {
        Manifest {
            path: path.into(),
            summary: Go.read(path, src.as_bytes()),
        }
    }

    /// A repository with these manifests and Go files, each exporting the name of its stem,
    /// capitalised (`upper.go` exports `Upper`).
    fn repository(name: &str, manifests: Vec<Manifest>, files: &[&str]) -> Export {
        let stem = |f: &str| {
            let stem = f.rsplit('/').next().unwrap().trim_end_matches(".go");
            stem[..1].to_uppercase() + &stem[1..]
        };
        Export {
            repository: name.into(),
            files: (files.iter())
                .map(|f| crate::index::File {
                    path: f.to_string(),
                    hash: String::new(),
                })
                .collect(),
            exported: (files.iter())
                .map(|f| crate::resolve::Exported {
                    path: f.to_string(),
                    name: Some(stem(f)),
                    file: Some(f.to_string()),
                    target: Some(stem(f)),
                })
                .collect(),
            manifests,
            ..Export::default()
        }
    }

    #[test]
    fn the_longest_module_path_the_import_starts_with_wins_and_the_rest_is_the_subpath() {
        let consumer = repository(
            "c",
            vec![manifest(
                "go.mod",
                "module example.com/app\nrequire (\n\texample.com/a v1.0.0\n\texample.com/a/b v1.0.0\n)\n",
            )],
            &["main.go"],
        );
        let exports = [consumer];
        let providers = Providers::new(&exports);
        let entry = |specifier: &str| {
            let found = Go.entries(&exports, &providers, (0, "main.go"), specifier);
            (found.into_iter())
                .map(|(e, subpath)| (e.name.clone(), subpath))
                .collect::<Vec<_>>()
        };
        let one = |name: &str, subpath: &str| vec![(name.to_string(), subpath.to_string())];
        assert_eq!(
            entry("example.com/a/b/c/d"),
            one("example.com/a/b", "./c/d")
        );
        assert_eq!(entry("example.com/a/b"), one("example.com/a/b", "."));
        assert_eq!(entry("example.com/a/x"), one("example.com/a", "./x"));
        assert_eq!(entry("example.com/ab"), []);
        assert_eq!(entry("fmt"), []);
    }

    #[test]
    fn a_package_directory_leaves_out_test_files_and_the_files_of_a_nested_module() {
        let provider = repository(
            "p",
            vec![
                manifest("go.mod", "module example.com/lib\n"),
                manifest("sub/go.mod", "module example.com/lib/sub\n"),
            ],
            &[
                "text/upper.go",
                "text/upper_test.go",
                "text/deep/lower.go",
                "sub/nested.go",
            ],
        );
        let at = Provided::new(&provider);
        let known = |_: &str| true;
        let resolve = |manifest: usize, subpath: &str, chain: &str| {
            let entry = Entry::default();
            let r = Go.resolve(
                &at,
                "p",
                &provider.manifests[manifest],
                (&entry, subpath),
                ("", chain),
                &known,
            )?;
            Some((r.targets, r.level))
        };
        let exact = |id: &str| Some((vec![id.to_string()], Level::Exact));
        // The one directory the subpath names: not the directories under it, not the tests.
        assert_eq!(
            resolve(0, "./text", "Upper"),
            exact("p::text/upper.go::Upper")
        );
        assert_eq!(resolve(0, "./text", "Lower"), None);
        assert_eq!(
            resolve(0, "./text/deep", "Lower"),
            exact("p::text/deep/lower.go::Lower")
        );
        assert_eq!(resolve(0, "./text", "Upper_test"), None);
        // The nested module's file belongs to the nested module alone.
        assert_eq!(resolve(0, "./sub", "Nested"), None);
        assert_eq!(resolve(1, ".", "Nested"), exact("p::sub/nested.go::Nested"));
        assert_eq!(resolve(0, ".", "Upper"), None);
    }

    #[test]
    fn a_local_replace_is_a_path_source_that_reaches_the_directory_it_names_and_nothing_else() {
        let s = Go.read(
            "app/go.mod",
            b"require example.com/lib v1.0.0\nreplace example.com/lib => ../lib\n",
        );
        let via = s.dependencies[0].via.as_deref();
        assert_eq!(via, Some("path lib/go.mod"));
        let at = |id| (id, Path::new(if id == "a" { "/w/a" } else { "/w/b" }));
        let source = (via, None);
        assert!(Go.reaches(source, at("a"), (at("a"), "lib/go.mod")));
        assert!(!Go.reaches(source, at("a"), (at("a"), "go.mod")));
        assert!(!Go.reaches(source, at("a"), (at("a"), "other/go.mod")));
        assert!(!Go.reaches(source, at("a"), (at("b"), "lib/go.mod")));
        // A module no `replace` moves is every repository's.
        assert!(Go.reaches((None, None), at("a"), (at("b"), "go.mod")));
    }

    #[test]
    fn a_go_mod_of_many_requires_and_replaces_reads_in_linear_time() {
        let mut text = String::from(
            "module example.com/app
",
        );
        let mut n = 0;
        while text.len() < MAX_BYTES / 2 {
            text += &format!("require example.com/m{n} v1.0.0\n");
            n += 1;
        }
        for i in 0..n {
            text += &format!("replace example.com/m{i} => ./m{i}\n");
            text += &format!("replace example.com/r{i} => ./r{i}\n");
        }
        let text = &text.as_bytes()[..text.len().min(MAX_BYTES)];
        let start = std::time::Instant::now();
        let s = Go.read("go.mod", text);
        assert!(s.dependencies.len() > n, "{}", s.dependencies.len());
        // Scanning the replaces for each require compares every pair: hundreds of millions here.
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{took:?}");
    }
}
