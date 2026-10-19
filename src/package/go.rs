//! Go packages (spec #25): a `go.mod` with a `module` path provides `package::go::<module path>`,
//! and each `require` entry, and each `replace` to a local directory, consumes the module it
//! names. The lines come from the Go pack's one `go.mod` reader (`lang::go::modfile`). A
//! `go.mod` carries no version of its own, so a provider's version is always none.
//!
//! An entry keeps where its module comes from in `Entry::via`, as Cargo's does: `path
//! <go.mod>` for a `replace` to a local directory (folded lexically from the manifest's
//! directory: a path of the repository, or one that starts with `../` when it leaves it, which
//! `Ecosystem::reaches` matches against the workspace repositories' roots), `outside` for a
//! `replace` by anything else (a module, an absolute path), which leaves the module out of the
//! workspace, and nothing otherwise. Ranges are never checked, and neither `go.sum` nor the
//! module proxy is read.

use super::cargo::relative;
use super::{Entry, MAX_BYTES, Summary, directory, valid_name};
use crate::lang::go::modfile;

/// The field a `require` entry is kept under.
const REQUIRE: &str = "require";
/// The field of an entry only a `replace` gives (a module some other module requires).
const REPLACE: &str = "replace";

/// Reads a `go.mod` from its bytes. Hostile input: a file too large reads as empty, a line it
/// cannot read is skipped, and a module path that cannot be a contract name makes no provider
/// and no entry.
pub fn read(path: &str, src: &[u8]) -> Summary {
    if src.len() > MAX_BYTES {
        return Summary::default();
    }
    let dir = directory(path);
    let dir = if dir == "." { "" } else { dir };
    let file = modfile::read(&String::from_utf8_lossy(src));
    let via = |module: &str| {
        let (_, target) = file.replaces.iter().find(|(old, _)| old == module)?;
        let local = target.starts_with("./") || target.starts_with("../");
        let local = local || matches!(target.as_str(), "." | "..");
        Some(match local.then(|| relative(dir, target)).flatten() {
            Some(at) if at.is_empty() => "path go.mod".to_string(),
            Some(at) => format!("path {at}/go.mod"),
            None => "outside".to_string(),
        })
    };
    let mut dependencies: Vec<Entry> = Vec::new();
    for (module, version) in &file.requires {
        if valid_name(module) && !dependencies.iter().any(|e| e.name == *module) {
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
        let required = dependencies.iter().any(|e| e.name == *module);
        let via = via(module).filter(|v| v.starts_with("path "));
        if via.is_some() && valid_name(module) && !required {
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

/// Whether `specifier`, an import path, may name a module of the workspace: its first element
/// has a dot, as every module path outside the standard library does.
pub fn names_module(specifier: &str) -> bool {
    let first = specifier.split('/').next().unwrap_or_default();
    first.contains('.') && !first.starts_with('.') && valid_name(specifier)
}

/// What follows the module path `module` in the import path `specifier`, if it starts with it:
/// the package directory inside the module, empty for its root.
pub fn inside<'s>(specifier: &'s str, module: &str) -> Option<&'s str> {
    match specifier.strip_prefix(module)? {
        "" => Some(""),
        rest => rest.strip_prefix('/'),
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
        let s = read(
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
        assert_eq!(read("go.mod", b"go 1.22\n").name, None);
        assert_eq!(read("go.mod", b"module a:b\n").name, None);
        assert_eq!(read("go.mod", &[0xff, 0xfe, b'\n']).dependencies, []);
        assert_eq!(
            read("go.mod", &vec![b'x'; MAX_BYTES + 1]),
            Summary::default()
        );
        // A replace at the root of the repository names its `go.mod`.
        let s = read("go.mod", b"replace example.com/a => .\n");
        assert_eq!(
            rows(&s),
            [("replace", "example.com/a", None, Some("path go.mod"))]
        );
    }

    #[test]
    fn an_import_path_names_a_module_only_outside_the_standard_library() {
        assert!(names_module("example.com/a/b"));
        assert!(names_module("github.com/x/y"));
        assert!(!names_module("net/http"));
        assert!(!names_module("fmt"));
        assert!(!names_module("./local"));
        assert!(!names_module(""));
        assert_eq!(inside("example.com/a/b/c", "example.com/a"), Some("b/c"));
        assert_eq!(inside("example.com/a", "example.com/a"), Some(""));
        assert_eq!(inside("example.com/ab", "example.com/a"), None);
    }
}
