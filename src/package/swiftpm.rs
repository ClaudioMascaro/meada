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

const FIELD: &str = "dependencies";

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

fn names_module(module: &str) -> bool {
    let mut chars = module.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && !PLATFORM.contains(&module)
}

fn marked(pattern: &str) -> String {
    match pattern.contains('*') {
        true => pattern.to_string(),
        false => format!("{pattern}*"),
    }
}

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

pub struct Symbols<'e> {
    by_name: HashMap<&'e str, Vec<(&'e str, &'e str)>>,
    flexible: HashMap<String, (usize, Option<usize>)>,
}

impl<'e> Symbols<'e> {
    pub(super) fn new(at: &Provided<'e>) -> Self {
        let mut by_name: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for &id in &at.symbols {
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
            closure: Default::default(),
        })
        .collect();
    least <= n && most.is_none_or(|m| n <= m) && labelled(&params, labels)
}

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

fn exposed<'m>(manifest: &'m Manifest, module: &str) -> Option<&'m str> {
    let s = &manifest.summary;
    let held = s
        .products
        .iter()
        .any(|(_, targets)| targets.iter().any(|t| t == module));
    let target = s.targets.iter().find(|(name, _)| name == module);
    target.filter(|_| held).map(|(_, dir)| dir.as_str())
}

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

    fn imports<'a>(&self, code: &Code<'a>, path: &'a str, call: &'a Call) -> Vec<(String, String)> {
        let callee = call.callee.as_str();
        let mut found: Vec<(String, String)> = Vec::new();
        if let (false, Some((receiver, member))) = (call.constructs, callee.rsplit_once('.')) {
            let origins = |follow| code.origins(path, &call.scope, Of::Name(receiver), follow);
            let mut found_origins = origins(Follow::Declarations);
            found_origins.retain(|o| o.outside.is_some());
            if found_origins.is_empty() {
                found_origins = origins(Follow::Values {
                    passes: passes_nothing,
                });
            }
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

    fn member_of(&self, target: &str, class: &str) -> bool {
        (target.strip_prefix(class))
            .and_then(|rest| rest.strip_prefix('.'))
            .and_then(|rest| rest.split_once('('))
            .is_some_and(|(name, _)| !name.is_empty() && !name.contains('.'))
    }

    fn fits(&self, wanted: &str, provided: &str) -> bool {
        wanted.contains('*') || wanted == provided
    }

    fn uncertainty(&self, via: Option<&str>) -> Option<(&'static str, Level)> {
        via.is_some_and(|v| v.starts_with("computed "))
            .then_some(("computed dependency", Level::Ambiguous))
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
        assert!(call(&["order", "note"], 1, Some(3)));
        assert!(!call(&["note", "order"], 1, Some(3)));
        assert!(!call(&["quantity"], 2, Some(3)));
        assert!(!call(&["other"], 0, Some(3)));
        assert!(fits(&["x", "body"], (2, Some(2)), &labels(&["x", ""])));
        assert!(fits(&["_"], (0, None), &labels(&["_", "_", "_"])));
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
