//! SwiftPM packages (spec #29): a `Package.swift` with a literal `name:` provides
//! `package::swiftpm::<name>`, and each `.package(url:)` or `.package(path:)` dependency consumes
//! the package of the workspace it points at. The manifest is read by the Swift pack's own reader
//! (`lang::swift::manifest`), so a manifest is read one way only. The module answers the trait's
//! questions as follows:
//!
//! - `read`: the package's name, its library products with the targets each holds (the modules
//!   an `import` can name) and the directory of every target; `Package.swift` carries no version.
//! - `entries`: every dependency of the nearest `Package.swift` above the file.
//! - `reaches`: the entry's source. A `url:` reaches the root `Package.swift` of the repository
//!   whose identity is the URL normalized as a remote is (ADR 0004), a `path:` the package at
//!   that directory (`sourced`, Cargo's rule). A computed part of either (an interpolation, a
//!   variable) is written `*`: the entry is `ambiguous` and reaches every provider whose location
//!   fits the literal parts (`computed git <pattern>`, `computed path <pattern>`).
//! - `settle`: a dependency names no package, only a location, so the workspace step gives each
//!   literal one the name its provider gives the package.
//! - `imports`, `resolutions` and `declares`: an `import Module` of a module a depended-on
//!   package exposes through a library product reaches that module's public declarations, found
//!   by their written name and the argument labels of the call.
//!
//! `Package.resolved` is never read, and a dependency outside the workspace is ignored.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::{
    Ecosystem, Entry, Export, How, Manifest, Provided, Providers, Resolution, Resolved, Summary,
    bare_directory, folded, owner, pattern_fits, reachable, resolutions_by_ways, sourced,
    valid_name,
};
use crate::contract::Level;
use crate::lang::swift::manifest::{MANIFEST, targets};
use crate::lang::{Binds, Call, Param};
use crate::repo::{normalize_remote, relative};
use crate::resolve::{Code, Follow, Form, Of, Origin, Outside, labelled, passes_nothing};

mod manifest;

/// The field every dependency is kept under.
const FIELD: &str = "dependencies";

/// The modules of the platform and its toolchain, which no package of the workspace provides.
// A table of the language's runtime, not a library form; a package named like one is not
// followed.
const PLATFORM: &[&str] = &[
    "Swift",
    "Foundation",
    "FoundationNetworking",
    "Dispatch",
    "Darwin",
    "Glibc",
    "CoreFoundation",
    "CoreGraphics",
    "UIKit",
    "AppKit",
    "SwiftUI",
    "Combine",
    "XCTest",
    "Testing",
];

/// Whether `module` can be the module of a package: a Swift identifier, not the platform's.
fn names_module(module: &str) -> bool {
    let mut chars = module.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && !PLATFORM.contains(&module)
}

/// A pattern as an entry's name: always with a `*`, so `fits` knows it is computed even when
/// folding the location (a userinfo, a port, a `..`) took the computed part out of it.
fn marked(pattern: &str) -> String {
    match pattern.contains('*') {
        true => pattern.to_string(),
        false => format!("{pattern}*"),
    }
}

/// The entry of the dependency `d` written in the package at `dir`. `None` when its path is
/// absolute.
fn entry(dir: &str, d: &manifest::Dependency) -> Option<Entry> {
    let (name, via) = match (d.url, d.computed) {
        (true, false) => {
            let identity = normalize_remote(&d.text);
            (identity.clone(), format!("git {identity}"))
        }
        (true, true) => {
            let pattern = normalize_remote(&d.text);
            (marked(&pattern), format!("computed git {pattern}"))
        }
        (false, computed) => {
            // A path that starts with a computed part cannot be placed in the repository; one
            // that folds to a computed first part is the repository's, not the workspace's.
            let at = match (d.text.starts_with('*'), computed) {
                (true, _) => d.text.clone(),
                (false, false) => relative(dir, &d.text)?,
                (false, true) => match relative(dir, &d.text)? {
                    at if at.starts_with('*') => format!("./{at}"),
                    at => at,
                },
            };
            match computed {
                false => {
                    let manifest = match at.as_str() {
                        "" => MANIFEST.to_string(),
                        at => format!("{at}/{MANIFEST}"),
                    };
                    // The repository's own root has no name of its path.
                    let name = if at.is_empty() { ".".to_string() } else { at };
                    (name, format!("path {manifest}"))
                }
                true => (marked(&at), format!("computed path {at}")),
            }
        }
    };
    (!name.is_empty()).then(|| Entry {
        field: FIELD.to_string(),
        name,
        range: d.range.clone(),
        via: Some(via),
        ..Entry::default()
    })
}

/// The Swift symbols of a provider by their written name, built the first time an import asks.
pub struct Symbols<'e> {
    /// `(file, id)` of each function, method, initializer and type, by its qualified name up to
    /// the parameter list (`Greeter.hello`, `greet`, `Greeter.init`).
    by_name: HashMap<&'e str, Vec<(&'e str, &'e str)>>,
    /// The argument counts of the functions that take more than one, as `(least, most)`, by
    /// `<file>::<name with labels>`.
    flexible: HashMap<String, (usize, Option<usize>)>,
}

impl<'e> Symbols<'e> {
    pub(super) fn new(at: &Provided<'e>) -> Self {
        let mut by_name: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for &id in &at.symbols {
            // `<repository>::<file>::<qualified name>[(<labels>)]`
            let Some((_, rest)) = id.split_once("::") else {
                continue;
            };
            let Some((file, name)) = rest.split_once("::") else {
                continue;
            };
            if !file.ends_with(".swift") {
                continue;
            }
            let base = match name.split_once('(') {
                // A nested function (`f(x:).g(y:)`) or a closure is no member of its type.
                Some((_, inner)) if inner.contains(").") || inner.contains('@') => continue,
                Some((base, _)) => base,
                None => name,
            };
            by_name.entry(base).or_default().push((file, id));
        }
        by_name.values_mut().for_each(|ids| ids.sort());
        let flexible = (at.flexible.iter())
            .map(|f| (format!("{}::{}", f.path, f.name), (f.least, f.most)))
            .collect();
        Self { by_name, flexible }
    }
}

/// The labels a symbol id writes after its name: `place(order:quantity:)` or, for overloads told
/// apart by their types, `audit(_:Order,limit:Int)`. `None` for a type.
fn labels_of(id: &str) -> Option<Vec<&str>> {
    let inner = id.split_once("::")?.1.split_once("::")?.1;
    let inner = inner.split_once('(')?.1.strip_suffix(')')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    if !inner.contains(',') && inner.ends_with(':') {
        return Some(inner.strip_suffix(':')?.split(':').collect());
    }
    let (mut labels, mut depth, mut start) = (Vec::new(), 0usize, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                labels.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    labels.push(&inner[start..]);
    Some(
        labels
            .into_iter()
            .map(|p| p.split(':').next().unwrap_or(p))
            .collect(),
    )
}

/// Whether a call with argument `labels` (`_` for none, empty for a trailing closure) fits a
/// function whose parameters are labelled `params` and which takes `least` to `most` arguments
/// (any number more when `None`, a variadic): the labels the resolver matches
/// (`resolve::labelled`), each parameter one the call may leave out and, when the function has a
/// variadic one, each variadic.
// ponytail: which parameters have defaults, and which is variadic, is not stored, only how many
// arguments the function takes; a call that skips a required parameter and keeps an optional
// one still fits.
fn fits(params: &[&str], (least, most): (usize, Option<usize>), labels: &[String]) -> bool {
    let n = labels.len();
    let binds = match most {
        None => Binds::Rest,
        Some(_) => Binds::Optional,
    };
    let params: Vec<Param> = (params.iter())
        .map(|label| Param {
            name: label.to_string(),
            binds,
            label: Some(label.to_string()),
            default: None,
            fields: Vec::new(),
        })
        .collect();
    least <= n && most.is_none_or(|m| n <= m) && labelled(&params, labels)
}

/// The qualified name and labels of an import's chain, `Greeter.hello(to:loudly:)`; the labels
/// are `None` when the call's are not known.
fn parse(chain: &str) -> (&str, Option<Vec<String>>) {
    let Some((name, inner)) = chain.split_once('(') else {
        return (chain, None);
    };
    let inner = inner.strip_suffix(')').unwrap_or(inner);
    let labels = (inner.split(':'))
        .filter(|l| !l.is_empty())
        .map(|l| match l {
            "{}" => String::new(),
            l => l.to_string(),
        })
        .collect();
    (name, Some(labels))
}

/// The directory of each module the package at `manifest` exposes through a library product.
fn exposed<'m>(manifest: &'m Manifest, module: &str) -> Option<&'m str> {
    let s = &manifest.summary;
    let held = s
        .products
        .iter()
        .any(|(_, targets)| targets.iter().any(|t| t == module));
    let target = s.targets.iter().find(|(name, _)| name == module);
    target.filter(|_| held).map(|(_, dir)| dir.as_str())
}

/// The ids of the public declarations the import `(module, chain)` names in the package at
/// `manifest`: the module must be exposed, the head of the name a public top-level declaration of
/// it, and the labels fit (`fits`).
fn found<'e>(
    at: &Provided<'e>,
    manifest: &Manifest,
    (module, chain): (&str, &str),
    known: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let Some(dir) = exposed(manifest, module) else {
        return Vec::new();
    };
    let (name, labels) = parse(chain);
    let symbols = at.swift.get_or_init(|| Symbols::new(at));
    let head = name.split('.').next().unwrap_or(name);
    let inside = |file: &str| {
        let under = dir.is_empty() || file.strip_prefix(dir).is_some_and(|r| r.starts_with('/'));
        under && at.owned_source(manifest, file)
    };
    // A public declaration at the top level of a file of the module.
    let public = (at.exported.iter())
        .filter(|(file, _)| inside(file))
        .any(|(_, rows)| rows.iter().any(|e| e.name.as_deref() == Some(head)));
    if !public {
        return Vec::new();
    }
    let mut ids: Vec<String> = (symbols.by_name.get(name).into_iter().flatten())
        .filter(|(file, _)| inside(file))
        .filter(|(file, id)| {
            let Some(call) = &labels else {
                return true;
            };
            let Some(params) = labels_of(id) else {
                // A type: named by no parameter list.
                return false;
            };
            let key = format!("{file}::{}", id.split("::").nth(2).unwrap_or(""));
            let counts =
                (symbols.flexible.get(&key).copied()).unwrap_or((params.len(), Some(params.len())));
            fits(&params, counts, call)
        })
        .filter(|(_, id)| known(id))
        .map(|(_, id)| id.to_string())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

// ponytail: the dependencies of the nearest `Package.swift` all count for every file below it,
// not only those a file's target lists with `.product(name:package:)`; an `internal` member of a
// public type, a member inherited from a base class and a declaration an extension of another
// module adds are not told apart or followed; `.executable` products expose no module.
pub struct Swiftpm;

impl Swiftpm {
    /// The packages (a repository and a manifest in it) the file's dependencies reach that expose
    /// `module` through a library product.
    fn exposing(
        &self,
        exports: &[Export],
        providers: &Providers,
        from: (usize, &str),
        module: &str,
    ) -> HashSet<(usize, usize)> {
        (reachable(self, exports, providers, from, module).into_iter())
            .flat_map(|(_, _, reached)| reached)
            .filter(|&(m, i)| exposed(&exports[m].manifests[i], module).is_some())
            .collect()
    }
}

impl Ecosystem for Swiftpm {
    fn name(&self) -> &'static str {
        "swiftpm"
    }

    fn is_manifest(&self, file_name: &str) -> bool {
        file_name == MANIFEST
    }

    /// Reads a `Package.swift` from its bytes. Hostile input: a file too large or not Swift reads
    /// as empty, and only literal values count: a computed name (written `*` for each computed
    /// part) or URL or path makes an uncertain entry.
    fn read(&self, path: &str, src: &[u8]) -> Summary {
        if src.len() > super::MAX_BYTES {
            return Summary::default();
        }
        let package = manifest::package(src);
        let dir = bare_directory(path);
        let mut dependencies: Vec<Entry> = (package.dependencies.iter())
            .filter_map(|d| entry(dir, d))
            .collect();
        dependencies.sort_by(|a, b| (&a.name, &a.via, &a.range).cmp(&(&b.name, &b.via, &b.range)));
        dependencies.dedup();
        let targets = (targets(path, src).into_iter())
            .map(|t| (t.name, t.dir))
            .collect();
        Summary {
            name: package.name.filter(|n| match package.name_computed {
                // A literal name with a `*` of its own is no name.
                false => valid_name(n) && !n.contains('*'),
                true => valid_name(&n.replace('*', "x")),
            }),
            dependencies,
            products: package.products,
            targets,
            ..Summary::default()
        }
    }

    fn sources(&self) -> &'static [&'static str] {
        &["swift"]
    }

    fn names_package(&self, specifier: &str) -> bool {
        names_module(specifier)
    }

    /// A call's module and what it names there: `Kit.greet(name:)` or an unqualified `greet(name:)`
    /// under `import Kit`, `Greeter.init(name:)` for a construction, `Greeter.hello(to:)` for a
    /// method of a type the source writes for its receiver. The chain ends with the call's
    /// argument labels (`_` for none, `{}` for a trailing closure).
    fn imports<'a>(&self, code: &Code<'a>, path: &'a str, call: &'a Call) -> Vec<(String, String)> {
        let callee = call.callee.as_str();
        // `(type or module path, member)`
        let mut found: Vec<(String, String)> = Vec::new();
        if let (false, Some((receiver, member))) = (call.constructs, callee.rsplit_once('.')) {
            let origins = |follow| code.origins(path, &call.scope, Of::Name(receiver), follow);
            // What the source writes for the receiver, else the construction it holds.
            let mut found_origins = origins(Follow::Declarations);
            if found_origins.is_empty() {
                found_origins = origins(Follow::Values {
                    passes: passes_nothing,
                });
            }
            // A written type, or a construction: the receiver is an instance of the type built.
            let made = |o: &Origin<'a>| match (o.form, o.call) {
                (Form::Type, _) => true,
                (Form::Call, Some((file, index))) => {
                    code.call(file, index).is_some_and(|c| c.constructs)
                }
                _ => false,
            };
            let origin = match found_origins.as_slice() {
                [one] if made(one) && one.routes.iter().all(|r| r.steps.is_empty()) => {
                    Some(one.clone())
                }
                _ => None,
            };
            // A construction found through values names its type outside only by its own callee.
            let outside = origin.and_then(|o| match (o.outside, o.call) {
                (Some(outside), _) => Some(outside),
                (None, Some((file, index))) => {
                    let made = code.call(file, index)?;
                    code.outside(file, &made.scope, &made.callee, false)
                }
                _ => None,
            });
            let typed = outside.map_or_else(Vec::new, Outside::spellings);
            found.extend(typed.into_iter().map(|t| (t, member.to_string())));
        }
        if found.is_empty() {
            let outside = code.outside(path, &call.scope, callee, false);
            for name in outside.map_or_else(Vec::new, Outside::spellings) {
                match call.constructs {
                    true => found.push((name, "init".to_string())),
                    false => found.extend(name.rsplit_once('.').map(|(t, m)| (t.into(), m.into()))),
                }
            }
        }
        found.sort();
        found.dedup();
        let labels = call.labels.as_ref().map(|labels| {
            (labels.iter())
                .map(|l| match l.as_str() {
                    "" => "{}:".to_string(),
                    l => format!("{l}:"),
                })
                .collect::<String>()
        });
        (found.into_iter())
            .filter_map(|(owner, member)| {
                let (module, within) = match owner.split_once('.') {
                    Some((module, within)) => (module, Some(within)),
                    None => (owner.as_str(), None),
                };
                let name = match within {
                    Some(within) => format!("{within}.{member}"),
                    None => member,
                };
                let chain = match &labels {
                    Some(labels) => format!("{name}({labels})"),
                    None => name,
                };
                names_module(module).then(|| (module.to_string(), chain))
            })
            .collect()
    }

    /// A constructor or a method of the type itself, by its id: the type's id, a dot, then a
    /// name and a parameter list; a nested type's member is not.
    fn member_of(&self, target: &str, class: &str) -> bool {
        (target.strip_prefix(class))
            .and_then(|rest| rest.strip_prefix('.'))
            .and_then(|rest| rest.split_once('('))
            .is_some_and(|(name, _)| !name.is_empty() && !name.contains('.'))
    }

    fn fits(&self, wanted: &str, provided: &str) -> bool {
        // A computed dependency is named by its pattern; `reaches` tells where it may point.
        wanted.contains('*') || wanted == provided
    }

    fn uncertainty(&self, via: Option<&str>) -> Option<&'static str> {
        via.is_some_and(|v| v.starts_with("computed "))
            .then_some("computed dependency")
    }

    fn reaches(
        &self,
        (via, _): (Option<&str>, Option<&str>),
        from: (&str, &Path),
        to: ((&str, &Path), &str),
    ) -> bool {
        let ((identity, root), provider) = to;
        let Some((kind, rest)) = via.and_then(|v| v.split_once(' ')) else {
            return via.is_none();
        };
        match kind {
            "git" => identity == rest && provider == MANIFEST,
            "path" => sourced(via, from, to),
            "computed" => match rest.split_once(' ') {
                Some(("git", pattern)) => pattern_fits(pattern, identity) && provider == MANIFEST,
                Some(("path", pattern)) => {
                    let place = folded(&root.join(bare_directory(provider)));
                    let wanted = match pattern.starts_with('*') {
                        true => pattern.to_string(),
                        false => folded(&from.1.join(pattern)).to_string_lossy().into_owned(),
                    };
                    pattern_fits(&wanted, &place.to_string_lossy())
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn entries<'e>(
        &self,
        exports: &'e [Export],
        _providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let own = owner(&exports[n].manifests, from, self);
        (own.into_iter().flat_map(|m| &m.summary.dependencies))
            .map(|e| (e, specifier.to_string()))
            .collect()
    }

    /// The module must be one the package exposes (`found`); the targets are the declarations
    /// the written name and labels fit, one `exact` and several `ambiguous`. The module is exposed
    /// by one package when only one is reached (`resolutions`).
    fn resolve(
        &self,
        at: &Provided,
        _repository: &str,
        manifest: &Manifest,
        _entry: (&Entry, &str),
        (module, chain): (&str, &str),
        known: &dyn Fn(&str) -> bool,
    ) -> Option<Resolved> {
        let targets = found(at, manifest, (module, chain), known);
        let level = match targets.len() {
            0 => return None,
            1 => Level::Exact,
            _ => Level::Ambiguous,
        };
        Some(Resolved {
            how: How::Module,
            level,
            targets,
        })
    }

    /// Node's rule over the dependencies, then `ambiguous` when several packages the file
    /// depends on expose the module the import names, whether or not they declare the name.
    fn resolutions<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        from: (usize, &str),
        import: (&str, &str),
        known: &dyn Fn(&Provided<'e>, &str) -> bool,
    ) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
        let mut out = resolutions_by_ways(self, exports, providers, at, from, import, known);
        if self.exposing(exports, providers, from, import.0).len() > 1 {
            for (_, resolved) in &mut out {
                resolved
                    .iter_mut()
                    .for_each(|r| r.3.level = Level::Ambiguous);
            }
        }
        out
    }

    /// Whether a package the file depends on exposes the module and declares the import's head
    /// at the top level of it, whatever the labels.
    fn declares<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        from: (usize, &str),
        (module, chain): (&str, &str),
    ) -> bool {
        let (name, _) = parse(chain);
        let head = name.split('.').next().unwrap_or(name);
        (self.exposing(exports, providers, from, module).into_iter()).any(|(m, i)| {
            let (provided, manifest) = (at(m), &exports[m].manifests[i]);
            let dir = exposed(manifest, module).unwrap_or("");
            (provided.exported.iter())
                .filter(|(file, _)| {
                    let under = dir.is_empty()
                        || file.strip_prefix(dir).is_some_and(|r| r.starts_with('/'));
                    under && provided.owned_source(manifest, file)
                })
                .any(|(_, rows)| rows.iter().any(|e| e.name.as_deref() == Some(head)))
        })
    }

    /// Each literal dependency takes the name the package it points at gives itself: a `url:`
    /// the root `Package.swift` of the repository with that identity, a `path:` the package at
    /// that directory. One that points at no package of the workspace keeps the location as its
    /// name and reaches nothing.
    fn settle(&self, exports: &mut [Export]) {
        let swift = |m: &Manifest| self.is_manifest(m.path.rsplit('/').next().unwrap_or(""));
        let mut located: HashMap<PathBuf, String> = HashMap::new();
        let mut roots: HashMap<String, String> = HashMap::new();
        for export in exports.iter() {
            let (identity, root) = export.place();
            for m in export.manifests.iter().filter(|m| swift(m)) {
                let Some(name) = &m.summary.name else {
                    continue;
                };
                located
                    .entry(folded(&root.join(&m.path)))
                    .or_insert_with(|| name.clone());
                if m.path == MANIFEST {
                    roots
                        .entry(identity.to_string())
                        .or_insert_with(|| name.clone());
                }
            }
        }
        for export in exports.iter_mut() {
            let root = export.root.clone();
            for entry in (export.manifests.iter_mut().filter(|m| swift(m)))
                .flat_map(|m| &mut m.summary.dependencies)
            {
                let name = match entry.via.as_deref().and_then(|v| v.split_once(' ')) {
                    Some(("git", identity)) => roots.get(identity),
                    Some(("path", at)) => located.get(&folded(&root.join(at))),
                    _ => None,
                };
                // A provider whose name is computed cannot be told from its name: the entry is
                // uncertain, held to the location it was written with.
                if let Some(name) = name {
                    entry.name = name.clone();
                    if name.contains('*') {
                        entry.via = entry.via.as_deref().map(computed);
                    }
                }
            }
        }
    }
}

/// The uncertain form of the literal source `via` (`git <identity>`, `path <dir>/Package.swift`):
/// `computed git <identity>`, `computed path <dir>`.
fn computed(via: &str) -> String {
    match via.split_once(' ') {
        Some(("path", manifest)) => {
            let dir = manifest.strip_suffix(MANIFEST).unwrap_or(manifest);
            format!("computed path {}", dir.trim_end_matches('/'))
        }
        _ => format!("computed {via}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(src: &str) -> Summary {
        Swiftpm.read("pkg/Package.swift", src.as_bytes())
    }

    #[test]
    fn a_package_is_named_by_a_literal_and_keeps_its_products_and_targets() {
        let src = r#"let p = Package(name: "Kit",
            products: [.library(name: "Kit", targets: ["Core", "Money"])],
            targets: [.target(name: "Core"), .target(name: "Money"), .testTarget(name: "CoreTests")])"#;
        let s = read(src);
        assert_eq!(s.name.as_deref(), Some("Kit"));
        assert_eq!(s.version, None);
        assert_eq!(
            s.products,
            [(
                "Kit".to_string(),
                vec!["Core".to_string(), "Money".to_string()]
            )]
        );
        assert!(
            s.targets
                .contains(&("Core".to_string(), "pkg/Sources/Core".to_string()))
        );
        // A computed name is an uncertain provider: `*` for each computed part.
        let computed = read("let p = Package(name: base + \"x\")");
        assert_eq!(computed.name.as_deref(), Some("*"));
        let partly = read("let p = Package(name: \"Kit\\(v)\")");
        assert_eq!(partly.name.as_deref(), Some("Kit*"));
        assert_eq!(read("let p = Package(name: \"a b\")").name, None);
        assert_eq!(read("let p = Package(name: \"a*b\")").name, None);
    }

    #[test]
    fn a_dependency_is_kept_with_its_source_and_range() {
        let src = r#"let p = Package(name: "A", dependencies: [
            .package(url: "git@example.com:org/kit.git", from: "1.2.0"),
            .package(path: "../lib"),
            .package(path: "local"),
            .package(path: "/abs/lib"),
            .package(url: "https://example.com/org/\(v).git", exact: "2.0.0"),
            .package(path: "../\(v)"),
            .package(path: ".."),
            .package(url: "https://\(t)@example.com/org/own.git", from: "1.0.0"),
        ])"#;
        let rows: Vec<_> = (read(src).dependencies.into_iter())
            .map(|e| (e.name, e.via.unwrap(), e.range))
            .collect();
        let row =
            |n: &str, v: &str, r: Option<&str>| (n.to_string(), v.to_string(), r.map(String::from));
        assert_eq!(
            rows,
            [
                row(".", "path Package.swift", None),
                row("./*", "computed path ./*", None),
                row(
                    "example.com/org/*",
                    "computed git example.com/org/*",
                    Some("exact 2.0.0")
                ),
                row(
                    "example.com/org/kit",
                    "git example.com/org/kit",
                    Some("from 1.2.0")
                ),
                row(
                    "example.com/org/own*",
                    "computed git example.com/org/own",
                    Some("from 1.0.0")
                ),
                row("lib", "path lib/Package.swift", None),
                row("pkg/local", "path pkg/local/Package.swift", None),
            ]
        );
    }

    #[test]
    fn a_call_fits_the_labels_some_of_the_parameters_have_in_order() {
        let labels = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let params = ["order", "quantity", "note"];
        let call = |l: &[&str], least, most| fits(&params, (least, most), &labels(l));
        assert!(call(&["order", "quantity", "note"], 3, Some(3)));
        assert!(!call(&["order", "quantity"], 3, Some(3)));
        // Defaults let a call leave labelled parameters out, but not reorder them.
        assert!(call(&["order", "note"], 1, Some(3)));
        assert!(!call(&["note", "order"], 1, Some(3)));
        assert!(!call(&["quantity"], 2, Some(3)));
        assert!(!call(&["other"], 0, Some(3)));
        // A trailing closure takes a parameter of any label.
        assert!(fits(&["x", "body"], (2, Some(2)), &labels(&["x", ""])));
        // A variadic parameter takes the unlabelled arguments after its first.
        assert!(fits(&["_"], (0, None), &labels(&["_", "_", "_"])));
        // Nor does it need to be unlabelled: `log(level: 1, items: "a", "b")`.
        let log = labels(&["level", "items", "_"]);
        assert!(fits(&["level", "items"], (1, None), &log));
        assert!(!fits(&["level", "items"], (1, Some(2)), &log));
        assert!(!fits(&["_"], (0, Some(1)), &labels(&["_", "_"])));
    }

    #[test]
    fn a_symbol_id_gives_its_labels_plain_or_with_types() {
        let id = |name: &str| format!("r::a/B.swift::{name}");
        assert_eq!(
            labels_of(&id("place(order:quantity:)")),
            Some(vec!["order", "quantity"])
        );
        assert_eq!(labels_of(&id("run()")), Some(vec![]));
        assert_eq!(labels_of(&id("S.audit(_:Order)")), Some(vec!["_"]));
        assert_eq!(
            labels_of(&id("S.audit(_:[String:Int],limit:(Int,Int))")),
            Some(vec!["_", "limit"])
        );
        assert_eq!(labels_of(&id("S")), None);
    }

    #[test]
    fn a_chain_carries_its_name_and_labels() {
        assert_eq!(
            parse("Greeter.hello(to:loudly:)"),
            ("Greeter.hello", Some(vec!["to".into(), "loudly".into()]))
        );
        assert_eq!(parse("run()"), ("run", Some(vec![])));
        assert_eq!(
            parse("each(_:{}:)"),
            ("each", Some(vec!["_".into(), String::new()]))
        );
        assert_eq!(parse("run"), ("run", None));
    }

    #[test]
    fn a_member_of_a_type_is_its_own_method_or_initializer() {
        let class = "r::a/G.swift::Greeter";
        let member = |rest: &str| Swiftpm.member_of(&format!("{class}{rest}"), class);
        assert!(member(".hello(to:)") && member(".init(name:)"));
        assert!(!member(".Inner.make()") && !member("") && !member("s.hello()"));
    }

    #[test]
    fn a_computed_dependency_reaches_the_locations_its_literal_parts_fit() {
        let at = |id, root| (id, Path::new(root));
        let reaches = |via: &str, to: ((&str, &Path), &str)| {
            Swiftpm.reaches((Some(via), None), at("app", "/w/app"), to)
        };
        let kit = (at("example.com/swift/kit", "/w/kit"), "Package.swift");
        assert!(reaches("git example.com/swift/kit", kit));
        assert!(!reaches("git example.com/swift/other", kit));
        assert!(reaches("computed git example.com/swift/*", kit));
        assert!(!reaches("computed git example.com/other/*", kit));
        // Only the root manifest of a repository is what a URL names.
        let nested = (at("example.com/swift/kit", "/w/kit"), "sub/Package.swift");
        assert!(!reaches("git example.com/swift/kit", nested));
        assert!(reaches(
            "computed path ../*",
            (at("k", "/w/kit"), "Package.swift")
        ));
        assert!(!reaches(
            "computed path ../x*",
            (at("k", "/w/kit"), "Package.swift")
        ));
        assert!(reaches(
            "computed path *",
            (at("k", "/w/kit"), "Package.swift")
        ));
        // A computed path of a nested manifest is held to the repository it was written in.
        let own = |root, manifest| (at("x", root), manifest);
        assert!(reaches(
            "computed path ./*",
            own("/w/app", "lib/Package.swift")
        ));
        assert!(!reaches(
            "computed path ./*",
            own("/w/kit", "Package.swift")
        ));
        assert!(!reaches("computed path ./*", kit));
        // A package computed in name is the location it was written with, uncertain.
        assert_eq!(computed("git example.com/k"), "computed git example.com/k");
        assert_eq!(computed("path lib/Package.swift"), "computed path lib");
        assert_eq!(computed("path Package.swift"), "computed path ");
        assert!(reaches("computed path ", own("/w/app", "Package.swift")));
        assert!(!reaches(
            "computed path ",
            own("/w/app", "lib/Package.swift")
        ));
        assert!(reaches(
            "path ../kit/Package.swift",
            (at("k", "/w/kit"), "Package.swift")
        ));
    }

    #[test]
    fn hostile_manifests_read_as_empty() {
        for src in [&b"\xff\xfe"[..], b"not swift {{{", b""] {
            assert_eq!(Swiftpm.read("Package.swift", src), Summary::default());
        }
    }
}
