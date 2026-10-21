//! The npm ecosystem (spec #22): a `package.json` with a valid name provides
//! `package::npm::<name>`. Its answers delegate to the package kind's npm rules: the reader
//! (`read`), the specifier split (`split`), the range check (`flag`) and the `workspace:` rule.

use std::path::Path;

use super::{Ecosystem, Entry, Flag, Providers, Summary, WORKSPACE, nearest, split};
use crate::index::Export;
use crate::lang::ts::EXTENSIONS;

/// The npm ecosystem.
pub struct Npm;

impl Ecosystem for Npm {
    fn name(&self) -> &'static str {
        "npm"
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name == "package.json"
    }

    fn read(&self, _path: &str, src: &[u8]) -> Summary {
        super::read(src)
    }

    /// The TS/JS pack's.
    fn sources(&self) -> &'static [&'static str] {
        &EXTENSIONS
    }

    fn names_package(&self, specifier: &str) -> bool {
        split(specifier).is_some()
    }

    /// The entry of the split name in the importing file's nearest `package.json`.
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        _providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let Some((name, subpath)) = split(specifier) else {
            return Vec::new();
        };
        let entry = (nearest(&exports[n].manifests, from, self))
            .and_then(|m| m.summary.dependencies.iter().find(|e| e.name == name));
        entry.map(|e| (e, subpath)).into_iter().collect()
    }

    /// Semver, through the range check.
    fn flag(&self, range: Option<&str>, version: Option<&str>) -> Option<Flag> {
        super::flag(range, version)
    }

    /// A `workspace:` range keeps only the dependent's own repository.
    fn reaches(
        &self,
        (_, range): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        ((to, _), _): ((&str, &Path), &str),
    ) -> bool {
        from.0 == to || !range.is_some_and(|r| r.starts_with(WORKSPACE))
    }

    /// Shapes cross npm packages alone.
    fn shapes(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_range_reaches_its_own_repository_only() {
        let (a, b) = (("a", Path::new("/w/a")), ("b", Path::new("/w/b")));
        let workspace = (None, Some("workspace:*"));
        assert!(!Npm.reaches(workspace, a, (b, "package.json")));
        assert!(Npm.reaches(workspace, b, (b, "package.json")));
        assert!(Npm.reaches((None, Some("^1")), a, (b, "package.json")));
    }

    #[test]
    fn npm_checks_its_ranges_and_crosses_shapes() {
        assert_eq!(Npm.flag(Some("^2"), Some("1.0.0")), Some(Flag::Mismatch));
        assert_eq!(Npm.flag(Some("^1"), Some("1.0.0")), None);
        assert_eq!(
            Npm.flag(Some("latest"), Some("1.0.0")),
            Some(Flag::Unchecked)
        );
        assert!(Npm.shapes());
    }
}
