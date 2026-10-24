//! Maven and Gradle share the Maven artifact ecosystem. Build readers are grammar-independent.

use super::{Ecosystem, Entry, Export, Manifest, Providers, Summary, bare_directory};
use crate::jvm;

pub struct Maven;

impl Ecosystem for Maven {
    fn name(&self) -> &'static str {
        "maven"
    }
    fn is_manifest(&self, name: &str) -> bool {
        matches!(
            name,
            "pom.xml"
                | "settings.gradle"
                | "settings.gradle.kts"
                | "build.gradle"
                | "build.gradle.kts"
        )
    }
    fn read(&self, path: &str, src: &[u8]) -> Summary {
        match path.rsplit('/').next() {
            Some("pom.xml") => jvm::maven::read(src),
            _ => jvm::gradle::read(path, src),
        }
    }
    fn fits(&self, wanted: &str, provided: &str) -> bool {
        jvm::fits(wanted, provided)
    }
    fn uncertainty(&self, via: Option<&str>) -> Option<&'static str> {
        (via == Some("computed")).then_some("computed coordinate")
    }
    fn sources(&self) -> &'static [&'static str] {
        &["java", "kt"]
    }
    fn names_package(&self, name: &str) -> bool {
        name.split('.').all(jvm::literal)
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
    fn inherit(&self, manifests: &mut [Manifest]) {
        let settings: Vec<_> = manifests
            .iter()
            .filter(|m| {
                m.path
                    .rsplit('/')
                    .next()
                    .is_some_and(|n| n.starts_with("settings.gradle"))
            })
            .filter_map(|m| {
                m.summary
                    .build
                    .clone()
                    .map(|b| (bare_directory(&m.path).to_string(), b))
            })
            .collect();
        for manifest in manifests {
            if !manifest
                .path
                .rsplit('/')
                .next()
                .is_some_and(|n| matches!(n, "build.gradle" | "build.gradle.kts"))
            {
                continue;
            }
            let Some(build) = &manifest.summary.build else {
                continue;
            };
            let dir = bare_directory(&manifest.path);
            let project = settings
                .iter()
                .filter_map(|(root, s)| {
                    if dir == root {
                        return s.project.clone();
                    }
                    let rel = dir
                        .strip_prefix(root)
                        .and_then(|s| s.strip_prefix('/'))
                        .or_else(|| root.is_empty().then_some(dir))?;
                    s.includes
                        .iter()
                        .find(|(_, path)| path == rel)
                        .map(|(name, _)| name.clone())
                })
                .next();
            manifest.summary.name = build
                .group
                .as_deref()
                .zip(project.as_deref())
                .and_then(|(g, a)| jvm::coordinate(g, a));
        }
    }
}
