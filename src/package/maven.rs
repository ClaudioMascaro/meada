//! Maven and Gradle share the Maven artifact ecosystem. Build readers are grammar-independent.
//!
//! An import reaches a provider's sources by package declaration, not by a path: the call's
//! class (`imports`, a type a single-type or on-demand import brings, written or declared as a
//! receiver's type) is looked up under the name the provider's `exports` rows give its public
//! types, in the files of the depended-on artifact that are not test sources (`tested`), and the
//! method is picked by name and argument count among the overloads of the class's family (the
//! provider's `families` rows, as Java merges them), a constructor among the class's own. The
//! chain of such an import is `name(arity)`. A Kotlin import names a package as well: the
//! chain then picks a top-level function of that package by name and argument count, a class
//! (its constructor), or, written `.name(arity)` after a receiver, an extension function.

use std::collections::{HashMap, HashSet};

use super::{
    Ecosystem, Entry, Export, How, Manifest, Provided, Providers, Resolution, Resolved, Summary,
    bare_directory, holding,
};
use crate::contract::Level;
use crate::jvm;
use crate::lang::Call;
use crate::resolve::{Code, Follow, Form, Of, Outside};

/// A class's ancestors past it, as the `families` rows give them (`Ancestry::ancestors`).
type Ancestors<'e> = &'e [Option<(String, String)>];

/// The public classes and the methods of a Java provider, built the first time an import asks.
pub struct Classes<'e> {
    /// The files declaring a type, as `(file, qualified name)`, by qualified name.
    types: HashMap<&'e str, Vec<(&'e str, &'e str)>>,
    /// The ids of the methods and constructors, by their id up to the parameter list.
    methods: HashMap<&'e str, Vec<&'e str>>,
    /// The ancestors of the public classes with a base, by `(file, qualified name)`
    /// (`Ancestry::ancestors`: `None` for a family not known).
    families: HashMap<(&'e str, &'e str), Option<Ancestors<'e>>>,
    /// The names the Kotlin files export, by file: a top-level declaration is the one exported
    /// both as written (`Money.format`) and with its package (`dev.kt.core.Money.format`).
    published: HashMap<&'e str, HashSet<&'e str>>,
    /// The functions and classes of Kotlin files by simple name, as `(qualified name, file,
    /// id)`.
    named: HashMap<&'e str, Vec<(&'e str, &'e str, &'e str)>>,
}

impl<'e> Classes<'e> {
    fn new(at: &Provided<'e>) -> Self {
        let source = |path: &str| path.ends_with(".java") || path.ends_with(".kt");
        let mut types: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for rows in at
            .exported
            .iter()
            .filter(|(path, _)| source(path))
            .map(|(_, r)| r)
        {
            for e in rows {
                // The row under the qualified name itself; the simple and relative aliases name
                // other things in another package.
                if let (Some(name), Some(file), Some(target)) =
                    (e.name.as_deref(), e.file.as_deref(), e.target.as_deref())
                    && name == target
                    && name.contains('.')
                {
                    types.entry(name).or_default().push((file, target));
                }
            }
        }
        types.values_mut().for_each(|files| files.sort());
        let published = (at.exported.iter())
            .filter(|(path, _)| path.ends_with(".kt"))
            .map(|(&path, rows)| {
                (
                    path,
                    rows.iter().filter_map(|e| e.name.as_deref()).collect(),
                )
            })
            .collect();
        let mut named: HashMap<&str, Vec<(&str, &str, &str)>> = HashMap::new();
        for &id in at.symbols.iter().filter(|id| id.contains(".kt::")) {
            // `<repository>::<file>.kt::<qualified name>[(<parameters>)]`
            if let Some(end) = id.find(".kt::").map(|i| i + ".kt".len())
                && let Some(start) = id[..end].find("::").map(|i| i + 2)
            {
                let qualified = id[end + 2..].split('(').next().unwrap_or("");
                let simple = qualified.rsplit('.').next().unwrap_or(qualified);
                named
                    .entry(simple)
                    .or_default()
                    .push((qualified, &id[start..end], id));
            }
        }
        named.values_mut().for_each(|all| all.sort());
        let mut methods: HashMap<&str, Vec<&str>> = HashMap::new();
        for &id in at
            .symbols
            .iter()
            .filter(|id| id.contains(".java::") || id.contains(".kt::"))
        {
            if let Some(open) = id.find('(') {
                methods.entry(&id[..open]).or_default().push(id);
            }
        }
        methods.values_mut().for_each(|ids| ids.sort());
        let families = (at.ancestries.iter())
            .map(|a| ((a.path.as_str(), a.class.as_str()), a.ancestors.as_deref()))
            .collect();
        Self {
            types,
            methods,
            families,
            published,
            named,
        }
    }
}

/// The method `name` and the argument count of a chain `name(arity)`.
fn call(chain: &str) -> (&str, Option<usize>) {
    match chain.strip_suffix(')').and_then(|c| c.split_once('(')) {
        Some((name, arity)) => (name, arity.parse().ok()),
        None => (chain, None),
    }
}

/// Whether a call with `arity` arguments fits the symbol `id`: as many as its parameters, or at
/// least all but the last when that one is variadic. Any call fits when its count is unknown.
fn fits_arity(id: &str, arity: Option<usize>) -> bool {
    let (Some(arity), Some(open)) = (arity, id.find('(')) else {
        return true;
    };
    // Up to the `)`: a method named after its class carries `#method` past it.
    let params = id[open + 1..]
        .split_once(')')
        .map_or("", |(params, _)| params);
    if params.trim().is_empty() {
        return arity == 0;
    }
    let (mut count, mut depth) = (1, 0usize);
    for c in params.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    let variadic = params
        .rsplit(',')
        .next()
        .is_some_and(|last| last.contains("..."));
    arity == count || variadic && arity + 1 >= count
}

/// Whether `file` is a test source of the artifact at `manifest`, or of another source set than
/// its `main`: under a root that `Summary::test_roots` lists and no deeper `main` one. A file
/// outside every root is not, and resolves by its package declaration.
fn tested(manifest: &Manifest, file: &str) -> bool {
    let relative = match bare_directory(&manifest.path) {
        "" => Some(file),
        dir => (file.strip_prefix(dir)).and_then(|rest| rest.strip_prefix('/')),
    };
    let summary = &manifest.summary;
    let root = (summary.roots.iter())
        .filter(|root| {
            (relative.and_then(|f| f.strip_prefix(root.as_str())))
                .is_some_and(|r| r.starts_with('/'))
        })
        .max_by_key(|root| root.len());
    root.is_some_and(|root| summary.test_roots.contains(root))
}

/// The files declaring the class `class` that the artifact at `manifest` owns and that are no
/// test sources, as `(file, qualified name)`.
fn declaring<'e>(at: &Provided<'e>, manifest: &Manifest, class: &str) -> Vec<(&'e str, &'e str)> {
    let classes = at.classes.get_or_init(|| Classes::new(at));
    (classes.types.get(class).into_iter().flatten())
        .filter(|(file, _)| at.owned_source(manifest, file) && !tested(manifest, file))
        .copied()
        .collect()
}

/// The symbols named `name` that the package `package` holds at its top level, in the files the
/// artifact at `manifest` owns that are no test sources: its functions and, as the class
/// itself, its classes (by the capital of their name), or with `extension` its extension
/// functions, whatever their receiver. A declaration is top-level and public when its file
/// exports it as written and with the package (`Classes::published`).
fn top_level<'e>(
    at: &Provided<'e>,
    manifest: &Manifest,
    (package, name): (&str, &str),
    extension: bool,
) -> Vec<&'e str> {
    let classes = at.classes.get_or_init(|| Classes::new(at));
    (classes.named.get(name).into_iter().flatten())
        .filter(|(qualified, file, id)| {
            let written = qualified
                .strip_prefix(package)
                .and_then(|rest| rest.strip_prefix('.'));
            let class = !id.contains('(') && name.starts_with(char::is_uppercase);
            written.is_some_and(|w| {
                w.contains('.') == extension
                    && (id.contains('(') || class && !extension)
                    && (classes.published.get(file))
                        .is_some_and(|p| p.contains(w) && p.contains(qualified))
            }) && at.owned_source(manifest, file)
                && !tested(manifest, file)
        })
        .map(|(_, _, id)| *id)
        .collect()
}

/// The ids of the methods named `method` that fit `arity` in the class `class` that `file` of
/// the provider `at` of `repository` declares, sorted, and whether an ancestor of the class is
/// opaque. Java merges overloads across the family: each signature comes from the first class of
/// the family that declares it (an override hides the base's), and an opaque ancestor, or a
/// family not known, may declare a better one (CODING_STANDARDS, "Opaque outside"). A
/// constructor (`method` is the class's own name) is the class's alone, never a method of that
/// name (`#method`).
fn methods(
    at: &Provided,
    repository: &str,
    (file, class): (&str, &str),
    (method, arity): (&str, Option<usize>),
    known: &dyn Fn(&str) -> bool,
) -> (Vec<String>, bool) {
    let classes = at.classes.get_or_init(|| Classes::new(at));
    let constructs = class.rsplit('.').next() == Some(method);
    let (ancestors, mut opaque) = match classes.families.get(&(file, class)) {
        _ if constructs => (&[][..], false),
        Some(Some(ancestors)) => (*ancestors, false),
        Some(None) => (&[][..], true),
        None => (&[][..], false),
    };
    let own = std::iter::once(Some((file, class)));
    let rest = (ancestors.iter()).map(|a| a.as_ref().map(|(f, c)| (f.as_str(), c.as_str())));
    let (mut ids, mut signatures) = (Vec::new(), HashSet::new());
    for (n, ancestor) in own.chain(rest).enumerate() {
        let Some((file, class)) = ancestor else {
            opaque = true;
            continue;
        };
        let key = format!("{repository}::{file}::{class}.{method}");
        // A Kotlin class's unnamed companion holds what its name calls (`Money.of(1)`).
        let companion = (n == 0 && file.ends_with(".kt") && !constructs)
            .then(|| format!("{repository}::{file}::{class}.Companion.{method}"));
        for key in std::iter::once(&key).chain(&companion) {
            for &id in classes.methods.get(key.as_str()).into_iter().flatten() {
                let fits = !(constructs && id.ends_with("#method")) && fits_arity(id, arity);
                if fits && known(id) && signatures.insert(&id[key.len()..]) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids.sort();
    (ids, opaque)
}

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
    /// A constructor or a method of the class itself, by its id: the class's id, a dot, then a
    /// signature (`example.models.Order.total()`, a companion's `Money.Companion.of(Int)`); a
    /// nested class's member is not.
    fn member_of(&self, target: &str, class: &str) -> bool {
        (target.strip_prefix(class))
            .and_then(|rest| rest.strip_prefix('.'))
            // A Kotlin companion's member is called on the class's name.
            .map(|rest| rest.strip_prefix("Companion.").unwrap_or(rest))
            .and_then(|rest| rest.split_once('('))
            .is_some_and(|(name, _)| !name.is_empty() && !name.contains('.'))
    }
    fn sources(&self) -> &'static [&'static str] {
        &["java", "kt"]
    }
    fn names_package(&self, name: &str) -> bool {
        name.split('.').all(jvm::literal)
    }
    /// The class and the method a call names through an import, as `(class, name(arity))`: a
    /// type the call's head names (`Order.of(1)`, `new Order(1)`, an on-demand import's), else
    /// the type the source writes for its receiver (`order.total()`). A JDK class is none. Kotlin
    /// adds the top-level function or class a bare name imports (`greet(..)`: the package, then
    /// the name), and for a call on a receiver of no class of the repository the extension
    /// function its name imports, as `(package, .name(arity))`.
    fn imports<'a>(&self, code: &Code<'a>, path: &'a str, call: &'a Call) -> Vec<(String, String)> {
        let callee = call.callee.as_str();
        let mut found = Vec::new();
        // The type the source writes for the receiver comes first: a local named like a package
        // (`order.total()`) is no type the on-demand imports bring.
        let mut local = false;
        if let (false, Some((receiver, method))) = (call.constructs, callee.rsplit_once('.')) {
            let origins = code.origins(path, &call.scope, Of::Name(receiver), Follow::Declarations);
            let typed = (origins.into_iter().next())
                .filter(|o| o.form == Form::Type && o.routes.iter().all(|r| r.steps.is_empty()));
            // A type of the repository: its members and extensions are the repository's.
            local = typed.as_ref().is_some_and(|o| o.outside.is_none());
            let classes = (typed.and_then(|o| o.outside)).map_or_else(Vec::new, Outside::spellings);
            found.extend(classes.into_iter().map(|c| (c, method.to_string())));
        }
        if found.is_empty() {
            let outside = code.outside(path, &call.scope, callee, false);
            for name in outside.map_or_else(Vec::new, Outside::spellings) {
                // A constructor is named after its class.
                let split = match call.constructs {
                    true => name.rsplit('.').next().map(|m| (name.as_str(), m)),
                    false => name.rsplit_once('.'),
                };
                found.extend(split.map(|(c, m)| (c.to_string(), m.to_string())));
            }
        }
        found.retain(|(class, _)| {
            class.contains('.')
                && !class.contains('(')
                && !class.starts_with("java.")
                && !class.starts_with("kotlin.")
        });
        // An extension function is called on a receiver, and imported by its name alone.
        if let (true, false, Some((_, name))) = (
            found.is_empty() && path.ends_with(".kt"),
            local,
            callee.rsplit_once('.'),
        ) {
            let outside = code.outside(path, &call.scope, name, false);
            for spelled in outside.map_or_else(Vec::new, Outside::spellings) {
                found.extend(
                    spelled
                        .rsplit_once('.')
                        .map(|(p, n)| (p.into(), format!(".{n}"))),
                );
            }
        }
        found.sort();
        found.dedup();
        (found.into_iter())
            .map(|(class, method)| match call.arity {
                Some(arity) => (class, format!("{method}({arity})")),
                None => (class, method),
            })
            .collect()
    }
    /// Every artifact the manifests holding the file depend on, nearest first, once each; the
    /// import's class stands for the subpath.
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

    /// The class `specifier` is, by its package declaration, in each provider of each artifact
    /// the file depends on (`entries`, `declaring`), and the method or constructor its chain
    /// names there (`methods`): `exact` when one provider declares the class and one target
    /// fits, `ambiguous` when several providers declare the class (whichever declare the
    /// method) or several targets fit, `inferred` for one target beside an opaque ancestor;
    /// `import package` is the reason.
    fn resolutions<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        from: (usize, &str),
        (specifier, chain): (&str, &str),
        known: &dyn Fn(&Provided<'e>, &str) -> bool,
    ) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
        // A chain written `.name(arity)` is a call on a receiver: an extension function.
        let (extension, chain) = chain
            .strip_prefix('.')
            .map_or((false, chain), |c| (true, c));
        let wanted = call(chain);
        let (mut out, mut reached) = (Vec::new(), HashSet::new());
        for (entry, _) in self.entries(exports, providers, from, specifier) {
            let mut resolved = Vec::new();
            for (m, i) in providers.matching(self, entry) {
                let (at, manifest) = (at(m), &exports[m].manifests[i]);
                let repository = &exports[m].repository;
                let known = |id: &str| known(at, id);
                let (mut targets, mut opaque) = (Vec::new(), false);
                let declared = match extension {
                    true => Vec::new(),
                    false => declaring(at, manifest, specifier),
                };
                for class in &declared {
                    let (ids, beside) = methods(at, repository, *class, wanted, &known);
                    targets.extend(ids);
                    opaque |= beside;
                }
                let mut found = !declared.is_empty();
                // A package holds functions, classes (their constructors) and extensions.
                if !found {
                    for id in top_level(at, manifest, (specifier, wanted.0), extension) {
                        if (!id.contains('(') || fits_arity(id, wanted.1)) && known(id) {
                            targets.push(id.to_string());
                        }
                    }
                    found = !targets.is_empty();
                }
                if !found {
                    continue;
                }
                reached.insert((m, i));
                targets.sort();
                targets.dedup();
                // An extension may be shadowed by a member of a receiver's type that is not
                // read (Kotlin picks the member).
                let level = match (targets.len(), opaque || extension) {
                    (0, _) => continue,
                    (1, false) => Level::Exact,
                    (1, true) => Level::Inferred,
                    _ => Level::Ambiguous,
                };
                let r = Resolved {
                    how: How::Package,
                    level,
                    targets,
                };
                resolved.push((m, manifest, at, r));
            }
            if !resolved.is_empty() {
                out.push((entry, resolved));
            }
        }
        if reached.len() > 1 {
            for (_, resolved) in &mut out {
                resolved
                    .iter_mut()
                    .for_each(|r| r.3.level = Level::Ambiguous);
            }
        }
        out
    }
    /// Whether a provider of an artifact the file depends on declares what the import names: the
    /// class `specifier` (`declaring`), whatever method it declares, or what the package
    /// `specifier` holds under the chain's name (`top_level`).
    fn declares<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        from: (usize, &str),
        (specifier, chain): (&str, &str),
    ) -> bool {
        let (extension, chain) = chain
            .strip_prefix('.')
            .map_or((false, chain), |c| (true, c));
        (self
            .entries(exports, providers, from, specifier)
            .into_iter())
        .flat_map(|(entry, _)| providers.matching(self, entry))
        .any(|(m, i)| {
            let (at, manifest) = (at(m), &exports[m].manifests[i]);
            !extension && !declaring(at, manifest, specifier).is_empty()
                || !top_level(at, manifest, (specifier, call(chain).0), extension).is_empty()
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_carries_the_method_and_its_argument_count() {
        assert_eq!(call("place(2)"), ("place", Some(2)));
        assert_eq!(call("Order(0)"), ("Order", Some(0)));
        assert_eq!(call("place"), ("place", None));
        assert_eq!(call("place(x)"), ("place", None));
    }

    #[test]
    fn an_argument_count_fits_the_parameters_of_a_signature() {
        let fits = |id: &str, n| fits_arity(&format!("r::a/B.java::p.B.{id}"), Some(n));
        assert!(fits("m()", 0) && !fits("m()", 1));
        assert!(fits("m(int)", 1) && !fits("m(int)", 2));
        // A comma inside type arguments or an array separates nothing.
        assert!(
            fits("m(Map<String,List<int[]>>,int)", 2) && !fits("m(Map<String,List<int[]>>,int)", 3)
        );
        // A variadic parameter takes any number from none.
        assert!(fits("m(String,Object...)", 1) && fits("m(String,Object...)", 5));
        assert!(!fits("m(String,Object...)", 0));
        assert!(fits_arity("r::a/B.java::p.B.m(int)", None));
        // A method named after its class carries a mark past its parameters.
        assert!(fits("B(int)#method", 1) && !fits("B(int)#method", 0));
    }

    #[test]
    fn a_member_of_a_class_is_its_own_constructor_or_method() {
        let class = "r::a/Order.java::a.Order";
        let member = |rest: &str| Maven.member_of(&format!("{class}{rest}"), class);
        assert!(member(".total()") && member(".Order(int)") && member(".Order(int)#method"));
        // A Kotlin companion's member is called on the class's name.
        assert!(member(".Companion.of(Int)") && !member(".Companion") && !member(".Other.of(Int)"));
        // A nested class, its member, another class with the same start, the class itself.
        assert!(!member(".Line") && !member(".Line.total()") && !member("s.total()"));
        assert!(!member("") && !member(".(int)"));
    }

    fn manifest(path: &str, roots: &[&str], tests: &[&str]) -> Manifest {
        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        Manifest {
            path: path.into(),
            summary: Summary {
                roots: strings(roots),
                test_roots: strings(tests),
                ..Summary::default()
            },
        }
    }

    #[test]
    fn a_file_is_a_test_source_only_under_a_test_root_no_main_root_is_deeper_than() {
        let m = manifest(
            "models/pom.xml",
            &["src/main/java", "src/test/java", "src/test/java/shared"],
            &["src/test/java"],
        );
        assert!(tested(&m, "models/src/test/java/a/B.java"));
        assert!(!tested(&m, "models/src/main/java/a/B.java"));
        // Outside every root, or beside a root of the same prefix, or in another module.
        assert!(!tested(&m, "models/other/a/B.java"));
        assert!(!tested(&m, "models/src/test/javax/B.java"));
        assert!(!tested(&m, "other/src/test/java/a/B.java"));
        let root = manifest("pom.xml", &["src/test/java"], &["src/test/java"]);
        assert!(tested(&root, "src/test/java/a/B.java"));
        assert!(!tested(&root, "models/src/main/java/a/B.java"));
    }
}
