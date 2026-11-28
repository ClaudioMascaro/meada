use std::collections::{HashMap, HashSet};

use super::{
    Ecosystem, Entry, Export, How, Manifest, Provided, Providers, Resolution, Resolved, Summary,
    bare_directory, holding, of, of_source, pattern_fits,
};
use crate::contract::Level;
use crate::jvm;
use crate::lang::Call;
use crate::resolve::{Code, Follow, Form, Of, Outside};

type Ancestors<'e> = &'e [Option<(String, String)>];

pub struct Classes<'e> {
    types: HashMap<&'e str, Vec<(&'e str, &'e str)>>,
    methods: HashMap<&'e str, Vec<&'e str>>,
    families: HashMap<(&'e str, &'e str), Option<Ancestors<'e>>>,
    published: HashMap<&'e str, HashSet<&'e str>>,
    named: HashMap<&'e str, Vec<(&'e str, &'e str, &'e str)>>,
    flexible: HashMap<String, (usize, Option<usize>)>,
}

impl<'e> Classes<'e> {
    fn new(at: &Provided<'e>) -> Self {
        let mut types: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for rows in at
            .exported
            .iter()
            .filter(|(path, _)| of_source(path).is_some())
            .map(|(_, r)| r)
        {
            for e in rows {
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
            .filter(|id| id.split("::").nth(1).and_then(of_source).is_some())
        {
            if let Some(open) = id.find('(') {
                methods.entry(&id[..open]).or_default().push(id);
            }
        }
        methods.values_mut().for_each(|ids| ids.sort());
        let families = (at.ancestries.iter())
            .map(|a| ((a.path.as_str(), a.class.as_str()), a.ancestors.as_deref()))
            .collect();
        let flexible = (at.flexible.iter())
            .map(|f| (format!("{}::{}", f.path, f.name), (f.least, f.most)))
            .collect();
        Self {
            types,
            methods,
            families,
            published,
            named,
            flexible,
        }
    }
}

struct Chain<'a> {
    call: (&'a str, Option<usize>),
    extension: bool,
    on_class: bool,
    receiver: Option<&'a str>,
}

fn chain(written: &str) -> Chain<'_> {
    let (written, receiver) = match written.split_once('@') {
        Some((rest, receiver)) => (rest, Some(receiver)),
        None => (written, None),
    };
    let (extension, written) = (written.strip_prefix('.')).map_or((false, written), |w| (true, w));
    let (on_class, written) =
        (written.strip_prefix("Companion.")).map_or((false, written), |w| (true, w));
    Chain {
        call: call(written),
        extension,
        on_class,
        receiver,
    }
}

fn call(chain: &str) -> (&str, Option<usize>) {
    match chain.strip_suffix(')').and_then(|c| c.split_once('(')) {
        Some((name, arity)) => (name, arity.parse().ok()),
        None => (chain, None),
    }
}

impl Classes<'_> {
    fn fits(&self, id: &str, arity: Option<usize>) -> bool {
        let (Some(arity), Some(open)) = (arity, id.find('(')) else {
            return true;
        };
        let own = id.split_once("::").map(|(_, rest)| rest);
        if let Some(&(least, most)) = own.and_then(|rest| self.flexible.get(rest)) {
            return least <= arity && most.is_none_or(|most| arity <= most);
        }
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
}

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

fn declaring<'e>(at: &Provided<'e>, manifest: &Manifest, class: &str) -> Vec<(&'e str, &'e str)> {
    let classes = at.classes.get_or_init(|| Classes::new(at));
    (classes.types.get(class).into_iter().flatten())
        .filter(|(file, _)| at.owned_source(manifest, file) && !tested(manifest, file))
        .filter(|(file, _)| {
            let ecosystem = |e: Option<&dyn Ecosystem>| e.map(|e| e.name());
            ecosystem(of_source(file)) == ecosystem(of(&manifest.path))
        })
        .copied()
        .collect()
}

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

fn methods(
    at: &Provided,
    repository: &str,
    (file, class): (&str, &str),
    (method, arity): (&str, Option<usize>),
    on_class: bool,
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
        let companion = (n == 0 && file.ends_with(".kt") && !constructs)
            .then(|| format!("{repository}::{file}::{class}.Companion.{method}"));
        let keys = match on_class {
            true => [companion.as_ref(), Some(&key)],
            false => [Some(&key), companion.as_ref()],
        };
        let published = classes.published.get(file);
        let extension = published.is_some_and(|p| p.contains(format!("{class}.{method}").as_str()));
        for key in keys.into_iter().flatten().filter(|_| !extension) {
            for &id in classes.methods.get(key.as_str()).into_iter().flatten() {
                let fits = !(constructs && id.ends_with("#method")) && classes.fits(id, arity);
                if fits && known(id) && signatures.insert(&id[key.len()..]) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids.sort();
    (ids, opaque)
}

// ponytail: real builds nest a few modules deep; this bounds the walk over a hostile one.
const MODULE_DEPTH: usize = 32;

fn parent_properties(manifests: &mut [Manifest]) {
    let pom = |m: &Manifest| m.path.rsplit('/').next() == Some("pom.xml");
    let mut named: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, m) in manifests.iter().enumerate().filter(|(_, m)| pom(m)) {
        if let Some(name) = &m.summary.name {
            named.entry(name).or_default().push(i);
        }
    }
    let mut filled = Vec::new();
    for (i, m) in manifests.iter().enumerate().filter(|(_, m)| pom(m)) {
        if !m.summary.dependencies.iter().any(|e| e.template.is_some()) {
            continue;
        }
        let mut chain = vec![i];
        while chain.len() <= MODULE_DEPTH
            && let Some(parent) = manifests[chain[chain.len() - 1]].summary.parent.as_deref()
            && let Some(&[only]) = named.get(parent).map(Vec::as_slice)
            && !chain.contains(&only)
        {
            chain.push(only);
        }
        let (group, artifact) = (m.summary.name.as_deref())
            .and_then(|n| n.split_once(':'))
            .unzip();
        let mut budget = jvm::maven::FILL_BUDGET;
        let lookup = |name: &str| match name {
            "project.groupId" => group,
            "project.artifactId" => artifact,
            _ => (chain.iter())
                .find_map(|&j| manifests[j].summary.properties.get(name))
                .map(String::as_str),
        };
        let entries: Vec<Entry> = (m.summary.dependencies.iter())
            .filter_map(|e| {
                let Some((g, a)) = &e.template else {
                    return Some(e.clone());
                };
                let (g, a) = (
                    jvm::maven::fill(g, &lookup, &mut budget),
                    jvm::maven::fill(a, &lookup, &mut budget),
                );
                let (name, computed) = jvm::maven::named(&g, &a)?;
                Some(Entry {
                    name,
                    via: computed.then(|| "computed".into()),
                    template: computed.then_some((g, a)),
                    ..e.clone()
                })
            })
            .collect();
        filled.push((i, entries));
    }
    for (i, mut entries) in filled {
        entries.sort_by(|a, b| (&a.field, &a.name).cmp(&(&b.field, &b.name)));
        entries.dedup();
        manifests[i].summary.dependencies = entries;
    }
}

fn declarations(name: &str) -> bool {
    name == "gradle.properties" || name.ends_with(".versions.toml")
}

fn gradle_holes(manifests: &mut [Manifest]) {
    let name = |m: &Manifest| m.path.rsplit('/').next().unwrap_or("").to_string();
    let at = |dir: &str, rel: &str| match dir {
        "" => rel.to_string(),
        dir => format!("{dir}/{rel}"),
    };
    let declared: HashMap<&str, &Manifest> = (manifests.iter())
        .filter(|m| declarations(&name(m)))
        .map(|m| (m.path.as_str(), m))
        .collect();
    let settings: Vec<(&str, &jvm::Build)> = (manifests.iter())
        .filter(|m| name(m).starts_with("settings.gradle"))
        .filter_map(|m| Some((bare_directory(&m.path), m.summary.build.as_ref()?)))
        .collect();
    let mut filled = Vec::new();
    for (i, m) in manifests.iter().enumerate() {
        if !matches!(name(m).as_str(), "build.gradle" | "build.gradle.kts") {
            continue;
        }
        let holes = |e: &Entry| {
            e.template.is_some()
                || e.via
                    .as_deref()
                    .is_some_and(|v| v.starts_with(jvm::gradle::CATALOG))
        };
        if !m.summary.dependencies.iter().any(holes) {
            continue;
        }
        let dir = bare_directory(&m.path);
        let root = (settings.iter())
            .filter(|(root, _)| {
                root.is_empty() || dir == *root || dir.starts_with(&format!("{root}/"))
            })
            .max_by_key(|(root, _)| root.len());
        let root_dir = root.map_or(dir, |(root, _)| root);
        let mut chain = vec!["", root_dir, dir];
        chain.dedup();
        let properties: Vec<_> = (chain.iter())
            .filter_map(|d| declared.get(at(d, "gradle.properties").as_str()))
            .collect();
        let mut budget = jvm::maven::FILL_BUDGET;
        let lookup = |name: &str| {
            (properties.iter().rev())
                .find_map(|m| m.summary.properties.get(name))
                .map(String::as_str)
        };
        let catalogs = root.map_or(&[][..], |(_, b)| &b.catalogs[..]);
        let catalog = |accessor: &str| {
            let (library, alias) = accessor.split_once('.')?;
            let file = match catalogs.iter().find(|(n, _)| n == library) {
                Some((_, file)) => file.as_str(),
                None if library == "libs" => "gradle/libs.versions.toml",
                None => return None,
            };
            declared
                .get(at(root_dir, file).as_str())?
                .summary
                .properties
                .get(alias)
        };
        let mut entries: Vec<Entry> = Vec::new();
        for e in &m.summary.dependencies {
            if let Some(accessor) = e
                .via
                .as_deref()
                .and_then(|v| v.strip_prefix(jvm::gradle::CATALOG))
            {
                let Some(found) = catalog(accessor) else {
                    continue;
                };
                let (name, range) = match found.match_indices(':').nth(1) {
                    Some((end, _)) => (&found[..end], Some(&found[end + 1..])),
                    None => (found.as_str(), None),
                };
                entries.push(Entry {
                    name: name.to_string(),
                    range: range.map(str::to_string),
                    via: None,
                    ..e.clone()
                });
                continue;
            }
            let mut entry = e.clone();
            if let Some((g, a)) = &e.template {
                let (g, a) = (
                    jvm::maven::fill(g, &lookup, &mut budget),
                    jvm::maven::fill(a, &lookup, &mut budget),
                );
                if let Some((name, computed)) = jvm::maven::named(&g, &a) {
                    if name == "*:*" {
                        continue;
                    }
                    entry.name = name;
                    entry.via = computed.then(|| "computed".into());
                    entry.template = computed.then_some((g, a));
                }
            }
            let range = e.range.as_deref().map(jvm::gradle::braced);
            let range = range.map(|r| jvm::maven::fill(&r, &lookup, &mut budget));
            entry.range = range.filter(|r| !r.contains('$')).or(entry.range);
            entries.push(entry);
        }
        entries.sort_by(|a, b| (&a.field, &a.name, &a.range).cmp(&(&b.field, &b.name, &b.range)));
        entries.dedup();
        filled.push((i, entries));
    }
    for (i, entries) in filled {
        manifests[i].summary.dependencies = entries;
    }
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
        ) || declarations(name)
    }
    fn read(&self, path: &str, src: &[u8]) -> Summary {
        match path.rsplit('/').next() {
            Some("pom.xml") => jvm::maven::read(src),
            Some("gradle.properties") => jvm::gradle::properties(src),
            Some(name) if declarations(name) => jvm::gradle::catalog(src),
            _ => jvm::gradle::read(path, src),
        }
    }
    fn owns(&self, manifest: &Manifest) -> bool {
        !manifest.path.rsplit('/').next().is_some_and(declarations)
    }
    fn fits(&self, wanted: &str, provided: &str) -> bool {
        pattern_fits(wanted, provided)
    }
    fn uncertainty(&self, via: Option<&str>) -> Option<(&'static str, Level)> {
        (via == Some("computed")).then_some(("computed coordinate", Level::Ambiguous))
    }
    fn member_of(&self, target: &str, class: &str) -> bool {
        (target.strip_prefix(class))
            .and_then(|rest| rest.strip_prefix('.'))
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
    fn imports<'a>(&self, code: &Code<'a>, path: &'a str, call: &'a Call) -> Vec<(String, String)> {
        imports(code, path, call, &["java.", "kotlin."])
    }
    fn entries<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        (n, from): (usize, &str),
        specifier: &str,
    ) -> Vec<(&'e Entry, String)> {
        let found = providers.walked(
            self.name(),
            exports,
            (n, bare_directory(from)),
            |manifests| {
                let at = |m: &Manifest| manifests.iter().position(|x| std::ptr::eq(x, m));
                let own: Vec<usize> = holding(manifests, from, self)
                    .into_iter()
                    .filter_map(at)
                    .collect();
                let dependencies = |i: usize| manifests[i].summary.dependencies.iter().enumerate();
                let mut found: Vec<(usize, usize)> = (own.iter())
                    .flat_map(|&i| dependencies(i).map(move |(j, _)| (i, j)))
                    .collect();
                // ponytail: a module is its own `pom.xml`, without its parent's dependencies, and a
                // Gradle dependency is not followed (`project(..)` is not read, and an `implementation`
                // one does not pass on).
                let mut modules: HashMap<&str, Vec<usize>> = HashMap::new();
                for (i, m) in (manifests.iter().enumerate())
                    .filter(|(_, m)| m.path.rsplit('/').next() == Some("pom.xml"))
                {
                    if let Some(name) = &m.summary.name {
                        modules.entry(name).or_default().push(i);
                    }
                }
                let entry = |(i, j): (usize, usize)| &manifests[i].summary.dependencies[j];
                let mut walked: HashSet<usize> = own.into_iter().collect();
                let mut level = found.clone();
                for _ in 0..MODULE_DEPTH {
                    let next: Vec<(usize, usize)> = (level.iter().copied())
                        .filter(|&e| !entry(e).private && entry(e).via.is_none())
                        .flat_map(|e| modules.get(entry(e).name.as_str()).into_iter().flatten())
                        .filter(|&&i| walked.insert(i))
                        .flat_map(|&i| {
                            (dependencies(i))
                                .filter(|(_, e)| !e.private)
                                .map(move |(j, _)| (i, j))
                        })
                        .collect();
                    if next.is_empty() {
                        break;
                    }
                    found.extend(&next);
                    level = next;
                }
                let mut seen = HashSet::new();
                (found.into_iter())
                    .filter(|&e| seen.insert(entry(e).name.as_str()))
                    .collect()
            },
        );
        (found.into_iter())
            .map(|e| (e, specifier.to_string()))
            .collect()
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
        resolutions(
            (self, How::Package),
            exports,
            providers,
            at,
            from,
            import,
            known,
        )
    }
    fn declares<'e>(
        &self,
        exports: &'e [Export],
        providers: &Providers,
        at: &dyn Fn(usize) -> &'e Provided<'e>,
        from: (usize, &str),
        import: (&str, &str),
    ) -> bool {
        declares(self, exports, providers, at, from, import)
    }
    fn inherit(&self, manifests: &mut [Manifest]) {
        parent_properties(manifests);
        gradle_holes(manifests);
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

pub(super) fn imports<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    platform: &[&str],
) -> Vec<(String, String)> {
    let callee = call.callee.as_str();
    let mut found: Vec<(String, String, String)> = Vec::new();
    let on_class = jvm::names::kotlin(path)
        && !call.constructs
        && (callee.rsplit_once('.'))
            .and_then(|(head, _)| head.rsplit('.').next())
            .is_some_and(|head| head.starts_with(char::is_uppercase));
    let marked = |method: &str| match on_class {
        true => format!("Companion.{method}"),
        false => method.to_string(),
    };
    let mut typed = Vec::new();
    if let (false, Some((receiver, method))) = (call.constructs, callee.rsplit_once('.')) {
        let origins = code.origins(path, &call.scope, Of::Name(receiver), Follow::Declarations);
        let origin = (origins.into_iter().next())
            .filter(|o| o.form == Form::Type && o.routes.iter().all(|r| r.steps.is_empty()))
            .filter(|o| o.outside.is_some());
        typed = (origin.and_then(|o| o.outside)).map_or_else(Vec::new, Outside::spellings);
        found.extend(
            typed
                .iter()
                .map(|c| (c.clone(), marked(method), String::new())),
        );
    }
    if found.is_empty() {
        let outside = code.outside(path, &call.scope, callee, false);
        for name in outside.map_or_else(Vec::new, Outside::spellings) {
            let split = match call.constructs {
                true => name.rsplit('.').next().map(|m| (name.as_str(), m)),
                false => name.rsplit_once('.'),
            };
            found.extend((split).map(|(c, m)| (c.to_string(), marked(m), String::new())));
        }
    }
    found.retain(|(class, ..)| {
        class.contains('.')
            && !class.contains('(')
            && !platform.iter().any(|p| class.starts_with(p))
    });
    if let (true, Some((_, name))) = (
        jvm::names::kotlin(path) && (found.is_empty() || !typed.is_empty()),
        callee.rsplit_once('.'),
    ) {
        let suffix = match &typed[..] {
            [one] if found.iter().any(|(class, ..)| class == one) => format!("@{one}"),
            _ => String::new(),
        };
        let outside = code.outside(path, &call.scope, name, false);
        for spelled in outside.map_or_else(Vec::new, Outside::spellings) {
            found.extend(
                (spelled.rsplit_once('.'))
                    .map(|(p, n)| (p.into(), format!(".{n}"), suffix.clone())),
            );
        }
    }
    found.sort();
    found.dedup();
    (found.into_iter())
        .map(|(class, method, receiver)| match call.arity {
            Some(arity) => (class, format!("{method}({arity}){receiver}")),
            None => (class, format!("{method}{receiver}")),
        })
        .collect()
}

fn reachable(
    eco: &dyn Ecosystem,
    exports: &[Export],
    providers: &Providers,
    from: usize,
    entry: &Entry,
) -> Vec<(usize, usize)> {
    let source = (entry.via.as_deref(), entry.range.as_deref());
    (providers.matching(eco, entry).into_iter())
        .filter(|&(m, i)| {
            let to = (exports[m].place(), exports[m].manifests[i].path.as_str());
            eco.reaches(source, exports[from].place(), to)
        })
        .collect()
}

pub(super) fn resolutions<'e>(
    (eco, how): (&dyn Ecosystem, How),
    exports: &'e [Export],
    providers: &Providers,
    at: &dyn Fn(usize) -> &'e Provided<'e>,
    from: (usize, &str),
    (specifier, chain): (&str, &str),
    known: &dyn Fn(&Provided<'e>, &str) -> bool,
) -> Vec<(&'e Entry, Vec<Resolution<'e>>)> {
    let Chain {
        call: wanted,
        extension,
        on_class,
        receiver,
    } = self::chain(chain);
    let mut members_known = false;
    if let Some(class) = receiver {
        let (mut hidden, mut declared, mut opaque) = (false, false, false);
        let matching = (eco.entries(exports, providers, from, specifier).into_iter())
            .flat_map(|(entry, _)| reachable(eco, exports, providers, from.0, entry));
        for (m, i) in matching {
            let (at, manifest) = (at(m), &exports[m].manifests[i]);
            let known = |id: &str| known(at, id);
            for c in declaring(at, manifest, class) {
                let (ids, beside) = methods(at, &exports[m].repository, c, wanted, false, &known);
                (hidden, declared, opaque) = (hidden || !ids.is_empty(), true, opaque || beside);
            }
        }
        if hidden {
            return Vec::new();
        }
        members_known = declared && !opaque;
    }
    let (mut out, mut reached) = (Vec::new(), HashSet::new());
    for (entry, _) in eco.entries(exports, providers, from, specifier) {
        let mut resolved = Vec::new();
        for (m, i) in reachable(eco, exports, providers, from.0, entry) {
            let (at, manifest) = (at(m), &exports[m].manifests[i]);
            let repository = &exports[m].repository;
            let known = |id: &str| known(at, id);
            let (mut targets, mut opaque) = (Vec::new(), false);
            let declared = match extension {
                true => Vec::new(),
                false => declaring(at, manifest, specifier),
            };
            for class in &declared {
                let (ids, beside) = methods(at, repository, *class, wanted, on_class, &known);
                targets.extend(ids);
                opaque |= beside;
            }
            let mut found = !declared.is_empty();
            if !found {
                let classes = at.classes.get_or_init(|| Classes::new(at));
                for id in top_level(at, manifest, (specifier, wanted.0), extension) {
                    if (!id.contains('(') || classes.fits(id, wanted.1)) && known(id) {
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
            let own = (receiver.filter(|_| members_known))
                .map(|class| format!("::{class}.{}(", wanted.0));
            let exact: Vec<String> = (targets.iter())
                .filter(|id| own.as_ref().is_some_and(|own| id.contains(own.as_str())))
                .cloned()
                .collect();
            if let [one] = &exact[..] {
                targets = vec![one.clone()];
            }
            let shadowed = extension && exact.len() != 1;
            let level = match (targets.len(), opaque || shadowed) {
                (0, _) => continue,
                (1, false) => Level::Exact,
                (1, true) => Level::Inferred,
                _ => Level::Ambiguous,
            };
            let r = Resolved {
                how,
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

pub(super) fn declares<'e>(
    eco: &dyn Ecosystem,
    exports: &'e [Export],
    providers: &Providers,
    at: &dyn Fn(usize) -> &'e Provided<'e>,
    from: (usize, &str),
    (specifier, chain): (&str, &str),
) -> bool {
    let Chain {
        call: (name, _),
        extension,
        receiver,
        ..
    } = self::chain(chain);
    if receiver.is_some() {
        return false;
    }
    (eco.entries(exports, providers, from, specifier).into_iter())
        .flat_map(|(entry, _)| reachable(eco, exports, providers, from.0, entry))
        .any(|(m, i)| {
            let (at, manifest) = (at(m), &exports[m].manifests[i]);
            !extension && !declaring(at, manifest, specifier).is_empty()
                || !top_level(at, manifest, (specifier, name), extension).is_empty()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inherited(poms: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        let mut manifests: Vec<_> = (poms.iter())
            .map(|(path, src)| Manifest {
                path: path.to_string(),
                summary: jvm::maven::read(src.as_bytes()),
            })
            .collect();
        Maven.inherit(&mut manifests);
        let app = manifests.iter().find(|m| m.path == "app/pom.xml").unwrap();
        (app.summary.dependencies.iter())
            .map(|e| (e.name.clone(), e.via.clone()))
            .collect()
    }

    #[test]
    fn the_gradle_properties_of_the_module_beat_the_repository_roots() {
        let build = "dependencies {\n  implementation \"${g}:${a}\"\n}\n";
        let mut manifests: Vec<_> = [
            ("gradle.properties", "g=root\na=models\n"),
            ("app/gradle.properties", "g=example\n"),
            ("app/build.gradle", build),
        ]
        .map(|(path, src)| Manifest {
            path: path.to_string(),
            summary: Maven.read(path, src.as_bytes()),
        })
        .into();
        Maven.inherit(&mut manifests);
        let names: Vec<_> = (manifests[2].summary.dependencies.iter())
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["example:models"]);
    }

    #[test]
    fn a_parent_pom_of_the_repository_gives_its_properties_to_the_dependencies_below() {
        let pom = |parent: &str, id: &str, properties: &str, dependency: &str| {
            format!(
                "<project>{parent}<groupId>example</groupId><artifactId>{id}</artifactId>\
                 <properties>{properties}</properties><dependencies><dependency>{dependency}\
                 </dependency></dependencies></project>"
            )
        };
        let parent = |id: &str| {
            format!("<parent><groupId>example</groupId><artifactId>{id}</artifactId></parent>")
        };
        let hole = "<groupId>${shared.group}</groupId><artifactId>m${suffix}</artifactId>";
        let app = pom(&parent("mid"), "app", "", hole);
        let mid = pom(&parent("root"), "mid", "<suffix>odels</suffix>", "");
        let root = pom("", "root", "<shared.group>example</shared.group>", "");
        let [a, m, r] = ["app", "mid", "root"].map(|id| format!("{id}/pom.xml"));
        let all = [(&a, &app), (&m, &mid), (&r, &root)].map(|(p, s)| (p.as_str(), s.as_str()));
        assert_eq!(inherited(&all), [("example:models".to_string(), None)]);
        let own = pom(&parent("mid"), "app", "<suffix>ore</suffix>", hole);
        let all = [(a.as_str(), own.as_str()), (&m, &mid), (&r, &root)];
        assert_eq!(inherited(&all), [("example:more".to_string(), None)]);
        let computed = |name: &str| [(name.to_string(), Some("computed".to_string()))];
        let missing = [(a.as_str(), app.as_str()), (&r, &root)];
        assert_eq!(inherited(&missing), computed("*:m*"));
        let twin = ("twin/pom.xml", mid.as_str());
        let twice = [(a.as_str(), app.as_str()), (&m, &mid), twin, (&r, &root)];
        assert_eq!(inherited(&twice), computed("*:m*"));
        let loop_a = pom(&parent("app"), "mid", "", "");
        let looped = [(a.as_str(), app.as_str()), (&m, loop_a.as_str())];
        assert_eq!(inherited(&looped), computed("*:m*"));
    }

    #[test]
    fn a_chain_carries_the_method_and_its_argument_count() {
        assert_eq!(call("place(2)"), ("place", Some(2)));
        assert_eq!(call("Order(0)"), ("Order", Some(0)));
        assert_eq!(call("place"), ("place", None));
        assert_eq!(call("place(x)"), ("place", None));
    }

    #[test]
    fn an_argument_count_fits_the_parameters_of_a_signature() {
        let bare = Classes {
            types: HashMap::new(),
            methods: HashMap::new(),
            families: HashMap::new(),
            published: HashMap::new(),
            named: HashMap::new(),
            flexible: HashMap::from([
                ("a/B.kt::p.B.pick(Int,Int)".to_string(), (1, Some(2))),
                ("a/B.kt::p.B.many(vararg String)".to_string(), (0, None)),
            ]),
        };
        let fits = |id: &str, n| bare.fits(&format!("r::a/B.java::p.B.{id}"), Some(n));
        assert!(fits("m()", 0) && !fits("m()", 1));
        assert!(fits("m(int)", 1) && !fits("m(int)", 2));
        assert!(
            fits("m(Map<String,List<int[]>>,int)", 2) && !fits("m(Map<String,List<int[]>>,int)", 3)
        );
        assert!(fits("m(String,Object...)", 1) && fits("m(String,Object...)", 5));
        assert!(!fits("m(String,Object...)", 0));
        assert!(bare.fits("r::a/B.java::p.B.m(int)", None));
        let kotlin = |id: &str, n| bare.fits(&format!("r::a/B.kt::p.B.{id}"), Some(n));
        assert!(
            kotlin("pick(Int,Int)", 1) && kotlin("pick(Int,Int)", 2) && !kotlin("pick(Int,Int)", 3)
        );
        assert!(!kotlin("pick(Int,Int)", 0));
        assert!(kotlin("many(vararg String)", 0) && kotlin("many(vararg String)", 9));
        assert!(kotlin("pick(String)", 1) && !kotlin("pick(String)", 2));
        assert!(fits("B(int)#method", 1) && !fits("B(int)#method", 0));
    }

    #[test]
    fn a_member_of_a_class_is_its_own_constructor_or_method() {
        let class = "r::a/Order.java::a.Order";
        let member = |rest: &str| Maven.member_of(&format!("{class}{rest}"), class);
        assert!(member(".total()") && member(".Order(int)") && member(".Order(int)#method"));
        assert!(member(".Companion.of(Int)") && !member(".Companion") && !member(".Other.of(Int)"));
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
        assert!(!tested(&m, "models/other/a/B.java"));
        assert!(!tested(&m, "models/src/test/javax/B.java"));
        assert!(!tested(&m, "other/src/test/java/a/B.java"));
        let root = manifest("pom.xml", &["src/test/java"], &["src/test/java"]);
        assert!(tested(&root, "src/test/java/a/B.java"));
        assert!(!tested(&root, "models/src/main/java/a/B.java"));
    }

    fn entries(poms: &[(&str, &str)]) -> Vec<String> {
        let exports = exports(poms);
        let providers = Providers::new(&exports);
        (Maven
            .entries(&exports, &providers, (0, "app/src/main/java/A.java"), "c")
            .iter())
        .map(|(e, _)| format!("{}{}", e.name, e.range.as_deref().unwrap_or("")))
        .collect()
    }

    fn exports(poms: &[(&str, &str)]) -> [Export; 1] {
        let manifests = (poms.iter())
            .map(|(module, deps)| {
                let src = format!("<project><groupId>g</groupId><artifactId>{module}</artifactId><dependencies>{deps}</dependencies></project>");
                Manifest {
                    path: format!("{module}/pom.xml"),
                    summary: Maven.read("pom.xml", src.as_bytes()),
                }
            })
            .collect();
        [Export {
            manifests,
            ..Export::default()
        }]
    }

    fn dep(artifact: &str, more: &str) -> String {
        format!(
            "<dependency><groupId>g</groupId><artifactId>{artifact}</artifactId>{more}</dependency>"
        )
    }

    #[test]
    fn a_module_passes_on_its_compile_dependencies_and_the_nearer_entry_wins() {
        let app = [dep("lib", ""), dep("x", "<version>1</version>")].concat();
        let lib = [
            dep("x", "<version>2</version>"),
            dep("y", ""),
            dep("t", "<scope>test</scope>"),
            dep("o", "<optional>true</optional>"),
        ]
        .concat();
        let found = entries(&[("app", &app), ("lib", &lib), ("y", &dep("z", ""))]);
        assert_eq!(found, ["g:lib", "g:x1", "g:y", "g:z"]);
        let found = entries(&[("app", &dep("lib", "<scope>test</scope>")), ("lib", &lib)]);
        assert_eq!(found, ["g:lib"]);
    }

    #[test]
    fn a_module_cycle_and_a_chain_past_the_depth_bound_are_walked_in_linear_time() {
        let names: Vec<String> = (0..100).map(|i| format!("m{i}")).collect();
        let deps: Vec<String> = (0..names.len())
            .map(|i| dep(names.get(i + 1).map_or("app", |n| n.as_str()), ""))
            .collect();
        let first = dep("m0", "");
        let mut poms = vec![("app", first.as_str())];
        poms.extend(
            names
                .iter()
                .map(String::as_str)
                .zip(deps.iter().map(String::as_str)),
        );
        assert_eq!(entries(&poms).len(), MODULE_DEPTH + 1);
        let found = entries(&[("app", &dep("a", "")), ("a", &dep("app", ""))]);
        assert_eq!(found, ["g:a", "g:app"]);
        const HUBS: usize = 4;
        const RING: usize = HUBS * 10_000;
        let names: Vec<String> = (0..RING).map(|i| format!("w{i}")).collect();
        let deps: Vec<String> = (0..RING).map(|i| dep(&names[(i + 1) % RING], "")).collect();
        let hubs: Vec<(String, String)> = (names.chunks(RING / HUBS).enumerate())
            .map(|(h, chunk)| (format!("h{h}"), chunk.iter().map(|n| dep(n, "")).collect()))
            .collect();
        let app: String = hubs.iter().map(|(h, _)| dep(h, "")).collect();
        let mut poms = vec![("app", app.as_str())];
        poms.extend(hubs.iter().map(|(h, d)| (h.as_str(), d.as_str())));
        poms.extend(
            names
                .iter()
                .map(String::as_str)
                .zip(deps.iter().map(String::as_str)),
        );
        let exports = exports(&poms);
        let providers = Providers::new(&exports);
        let start = std::time::Instant::now();
        let found = Maven.entries(&exports, &providers, (0, "app/src/main/java/A.java"), "c");
        let took = start.elapsed();
        assert_eq!(found.len(), HUBS + RING);
        assert!(took.as_secs() < 8, "{took:?}");
    }
}
