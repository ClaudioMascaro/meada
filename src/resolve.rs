use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::lang::{
    Annotated, Binds, Bound, Call, Class, Closure, Const, Def, Export, Facts, FirstResult, Import,
    Imported, Invocation, Kind, LanguagePack, Marked, Modules, Param, Part, Reaches, Star, Step,
    Taker, Through, Ty, Use, Value,
};

mod mutations;
mod ts;
pub(crate) use mutations::MemberEffect;
pub(crate) type CallWrites = for<'a> fn(&Code<'a>, &'a str, usize) -> Option<Vec<MemberEffect<'a>>>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Target<'a> {
    pub path: &'a str,
    pub name: &'a str,
}

fn own_name(name: &str) -> &str {
    name[crate::parent(name).len()..].trim_start_matches('.')
}

#[derive(Clone)]
pub enum Resolution<'a> {
    Exact(Target<'a>),
    Inferred(Target<'a>),
    Narrowed(Target<'a>),
    Ambiguous(Vec<Target<'a>>),
    Unresolved,
}

pub fn scope_of(name: &str) -> &str {
    crate::parent(name)
}

static NAMESPACE: Imported = Imported::Namespace;

type Row<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>);

#[derive(Default)]
struct Named<'a> {
    stars: Vec<usize>,
    names: HashMap<&'a str, Vec<usize>>,
}

impl<'a> Named<'a> {
    fn exports(exports: &'a [Export]) -> Self {
        let mut named = Self::default();
        for (at, export) in exports.iter().enumerate() {
            match export {
                Export::All { .. } => named.stars.push(at),
                Export::Local { exported, .. } | Export::From { exported, .. } => {
                    named.names.entry(exported.as_str()).or_default().push(at);
                }
            }
        }
        named
    }

    fn imports(imports: &'a [Import]) -> Self {
        let mut named = Self::default();
        for (at, import) in imports.iter().enumerate() {
            named
                .names
                .entry(import.local.as_str())
                .or_default()
                .push(at);
            if matches!(import.name, Imported::All) {
                named.stars.push(at);
            }
        }
        named
    }

    fn of(&self, name: &str) -> &[usize] {
        self.names.get(name).map_or(&[], Vec::as_slice)
    }
}

pub fn call_at(calls: &[Call], range: &std::ops::Range<usize>) -> Option<usize> {
    let key = |r: &std::ops::Range<usize>| (r.start, std::cmp::Reverse(r.end));
    let at = calls.partition_point(|c| key(&c.range) < key(range));
    match calls.get(at) {
        Some(c) if c.range == *range => Some(at),
        _ => calls.iter().position(|c| c.range == *range),
    }
}

#[derive(Clone)]
enum Binding<'a> {
    Def(Target<'a>),
    Instance(Target<'a>),
    Module(&'a str),
    Const(&'a str, &'a str),
    Param(Target<'a>, usize, Option<usize>),
    Local(&'a str, &'a str, &'a str),
    External(&'a str, &'a Imported),
    Hidden,
    Several(Vec<Target<'a>>),
    Unknown,
}

#[derive(Clone, Copy)]
struct Caller<'a> {
    path: &'a str,
    index: usize,
}

#[derive(Default)]
struct Seen<'a> {
    pairs: HashMap<(&'a str, &'a str, bool), Option<Given<'a>>>,
    aliases: HashSet<(&'a str, &'a str)>,
    globs: HashSet<(&'a str, usize, &'a str)>,
    depth: usize,
    cut: bool,
    looped: bool,
    outside: Option<&'a str>,
    unsure: bool,
    load: Option<&'a str>,
    from: usize,
    rows: bool,
}

type Given<'a> = (Option<Binding<'a>>, bool);

impl Seen<'_> {
    fn new() -> Self {
        Self::default()
    }
}

// ponytail: real chains are a few links long; the limit keeps a crafted chain of thousands from
// overflowing the stack.
const LINK_DEPTH: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Reading {
    Any,
    Value,
    Path,
    Type,
    Alias,
    Method,
    Read,
    Member,
    Decorator,
}

impl Reading {
    fn read(self) -> bool {
        matches!(self, Reading::Read | Reading::Member)
    }

    fn types(self) -> bool {
        matches!(
            self,
            Reading::Path | Reading::Type | Reading::Alias | Reading::Method
        )
    }

    fn value(self) -> bool {
        matches!(
            self,
            Reading::Value
                | Reading::Decorator
                | Reading::Path
                | Reading::Method
                | Reading::Read
                | Reading::Member
        )
    }
}

#[derive(PartialEq)]
enum Busy<'a> {
    Local(&'a str, &'a str, &'a str),
    Call(&'a str, usize),
}

// ponytail: real receivers name their class through a few others; the limit keeps a crafted
// chain of thousands of parameters from overflowing the stack.
const RECEIVER_DEPTH: usize = 64;

#[derive(Default)]
struct Work<'a> {
    busy: Vec<Busy<'a>>,
    families: HashMap<Target<'a>, Family<'a>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Ancestor<'a> {
    Class(Target<'a>),
    Opaque(String),
}

type Family<'a> = Option<Vec<Ancestor<'a>>>;

// ponytail: real hierarchies hold a few dozen classes; the limit keeps a crafted chain of
// thousands from overflowing the stack and from keeping families quadratic in its length.
const FAMILY_DEPTH: usize = 128;

// ponytail: real partial types have a handful of parts; the limit keeps a crafted type in
// thousands of files from making every member lookup of it read them all.
const TYPE_PARTS: usize = 64;

struct Cut;

type Params<'a> = HashMap<(&'a str, &'a str), (usize, Option<usize>)>;

#[derive(Default)]
struct Binders<'a> {
    values: HashMap<&'a str, Vec<&'a str>>,
    functions: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Binders<'a> {
    fn of(
        locals: &HashMap<&'a str, HashSet<&'a str>>,
        consts: &HashMap<&'a str, HashSet<&'a str>>,
        params: &HashMap<&'a str, Params<'a>>,
        receivers: &HashMap<&'a str, HashMap<(&'a str, &'a str), &'a Class>>,
        overloads: &HashMap<&'a str, HashMap<&'a str, Vec<Target<'a>>>>,
    ) -> HashMap<&'a str, Self> {
        let split = |q: &'a str| {
            let scope = crate::parent(q);
            (
                scope,
                if scope.is_empty() {
                    q
                } else {
                    &q[scope.len() + 1..]
                },
            )
        };
        let mut all: HashMap<&'a str, Self> = HashMap::new();
        for (&path, names) in locals.iter().chain(consts) {
            let values = &mut all.entry(path).or_default().values;
            for &q in names {
                let (scope, name) = split(q);
                values.entry(name).or_default().push(scope);
            }
        }
        for (&path, bound) in params {
            let values = &mut all.entry(path).or_default().values;
            for &(scope, name) in bound.keys() {
                values.entry(name).or_default().push(scope);
            }
        }
        for (&path, bound) in receivers {
            let values = &mut all.entry(path).or_default().values;
            for &(scope, name) in bound.keys() {
                values.entry(name).or_default().push(scope);
            }
        }
        for (&path, families) in overloads {
            let functions = &mut all.entry(path).or_default().functions;
            for &q in families.keys() {
                let (scope, name) = split(q);
                functions.entry(name).or_default().push(scope);
            }
        }
        all
    }
}

pub struct Resolver<'a> {
    ts: bool,
    ts_site: Cell<Option<(usize, usize, bool)>>,
    ts_key_read: Cell<bool>,
    ts_locals: HashMap<&'a str, ts::FileLocals<'a>>,
    ts_packages: BTreeSet<String>,
    ts_heirs: HashMap<String, Vec<Target<'a>>>,
    ts_writes: Option<ts::Writes<'a>>,
    ts_overrides: RefCell<HashMap<String, HashMap<String, bool>>>,
    files: &'a BTreeMap<&'a str, &'a Facts>,
    defs: HashMap<&'a str, HashMap<&'a str, &'a Def>>,
    overloads: HashMap<&'a str, HashMap<&'a str, Vec<Target<'a>>>>,
    constructors: HashMap<&'a str, HashMap<&'a str, Vec<Target<'a>>>>,
    arities: HashMap<Target<'a>, Arity<'a>>,
    statics: HashSet<Target<'a>>,
    members_in_scope: bool,
    drops_external_uses: bool,
    assigned_members: bool,
    guesses_constructs: bool,
    unordered_globs: bool,
    combines_globs: bool,
    qualified_types: bool,
    static_imports_inherit: bool,
    namespaced: bool,
    consts: HashMap<&'a str, HashSet<&'a str>>,
    declared: HashMap<&'a str, HashSet<&'a str>>,
    internal: HashMap<&'a str, HashSet<&'a str>>,
    sealed: HashMap<&'a str, HashSet<&'a str>>,
    confined: HashMap<&'a str, HashSet<&'a str>>,
    written: HashSet<&'a str>,
    written_outside_tests: HashSet<&'a str>,
    written_in: HashSet<(&'a str, &'a str)>,
    locals: HashMap<&'a str, HashSet<&'a str>>,
    receivers: HashMap<&'a str, HashMap<(&'a str, &'a str), &'a Class>>,
    params: HashMap<&'a str, Params<'a>>,
    named: HashMap<&'a str, Named<'a>>,
    by_local: HashMap<&'a str, Named<'a>>,
    externs: HashMap<&'a str, HashSet<&'a str>>,
    pathed: HashSet<&'a str>,
    takers: HashMap<&'a str, Taker<'a>>,
    families: HashMap<Target<'a>, Family<'a>>,
    linearizing: RefCell<Vec<(String, String)>>,
    binders: HashMap<&'a str, Binders<'a>>,
    cut: Cell<bool>,
    links: Cell<usize>,
    chain: Cell<usize>,
    companions: HashMap<Target<'a>, Target<'a>>,
    receiver: Option<&'static str>,
    base_receiver: Option<&'static str>,
    unordered_bases: bool,
    extension_methods: bool,
    outside_blocks: bool,
    implicit: &'static [&'static str],
    known: Vec<crate::lang::KnownEntry>,
    globals: &'static [&'static str],
    callers: HashMap<Target<'a>, Vec<Caller<'a>>>,
    test_callers: HashMap<Target<'a>, Vec<Caller<'a>>>,
    ts_constructions: HashMap<Target<'a>, Vec<Caller<'a>>>,
    built: HashMap<&'a str, Vec<Resolution<'a>>>,
    built_final: bool,
    extension_callers: HashMap<Target<'a>, Vec<Caller<'a>>>,
    by_name: HashMap<(bool, &'a str), Vec<Target<'a>>>,
    ts_literal_candidates: HashMap<&'a str, Vec<Target<'a>>>,
    modules: Modules,
    impls: HashMap<Target<'a>, Vec<Block<'a>>>,
    blocks: HashMap<(&'a str, &'a str), Target<'a>>,
    outside: HashSet<(&'a str, &'a str)>,
    unread: HashSet<(&'a str, &'a str)>,
    extensions: HashMap<&'a str, Vec<Extension<'a>>>,
    extending: HashSet<&'a str>,
    published: Option<BTreeSet<String>>,
    reaches: Option<Reaches>,
    stars_bind: bool,
    strict: bool,
    later: HashMap<&'a str, (Vec<&'a str>, HashMap<&'a str, usize>)>,
    shared: HashMap<&'a str, usize>,
    groups: Vec<Group<'a>>,
    parts: HashMap<(&'a str, &'a str), Vec<&'a str>>,
    actuals: HashMap<&'a str, Vec<&'a str>>,
}

#[cfg(test)]
thread_local! {
    static CANDIDATES_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn nested_in(inner: Target, outer: Target) -> bool {
    inner.path == outer.path && crate::parent(inner.name) == outer.name
}

#[derive(Clone, Copy)]
struct Arity<'a> {
    least: usize,
    most: Option<usize>,
    receiver: bool,
    params: &'a [Param],
    suspends: bool,
}

impl<'a> Arity<'a> {
    fn of(def: &'a Def) -> Self {
        let params = &def.params[..];
        let required = |p: &&Param| !matches!(p.binds, Binds::Optional | Binds::Rest);
        Arity {
            least: params.iter().filter(required).count(),
            most: (!params.iter().any(|p| p.binds == Binds::Rest)).then_some(params.len()),
            receiver: params.first().is_some_and(|p| p.binds == Binds::Instance),
            params,
            suspends: def.suspends,
        }
    }

    fn fits(self, arity: usize) -> bool {
        self.least <= arity && self.most.is_none_or(|most| arity <= most)
    }

    fn fits_call(self, args: Args, on_receiver: &dyn Fn() -> bool) -> Fit {
        let arity = args.count + usize::from(self.receiver && on_receiver());
        if !self.fits(arity) || !(args.awaited || !self.suspends) {
            return Fit::No;
        }
        args.labels
            .map_or(Fit::Yes, |labels| fit(self.params, labels))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Fit {
    No,
    Maybe,
    Yes,
}

#[derive(Clone, Copy)]
struct Args<'c> {
    count: usize,
    labels: Option<&'c [String]>,
    awaited: bool,
}

impl<'c> Args<'c> {
    fn of(call: &'c Call) -> Option<Self> {
        Some(Args {
            count: call.arity?,
            labels: call.labels.as_deref(),
            awaited: call.awaited,
        })
    }
}

// ponytail: real calls pass a handful of arguments; this keeps a crafted one cheap.
const LABELLED_CELLS: usize = 1 << 16;

pub(crate) fn labelled(params: &[Param], labels: &[String]) -> bool {
    fit(params, labels) != Fit::No
}

fn fit(params: &[Param], labels: &[String]) -> Fit {
    if !params.is_empty() && params.iter().all(|p| p.label.is_none()) {
        return Fit::Yes;
    }
    let sure = labelled_args(params, labels);
    if !params.iter().any(|p| p.closure == Closure::Unknown) {
        return if sure.is_some() { Fit::Yes } else { Fit::No };
    }
    match (sure, labelled_args_as(params, labels, true)) {
        (_, None) => Fit::No,
        (Some(sure), Some(maybe)) if sure == maybe => Fit::Yes,
        _ => Fit::Maybe,
    }
}

fn labelled_args(params: &[Param], labels: &[String]) -> Option<Vec<Option<usize>>> {
    labelled_args_as(params, labels, false)
}

fn labelled_args_as(
    params: &[Param],
    labels: &[String],
    unknown_takes: bool,
) -> Option<Vec<Option<usize>>> {
    let refuses = |p: &Param| match p.closure {
        Closure::Refuses => true,
        Closure::Unknown => !unknown_takes,
        Closure::Takes | Closure::Named(_) => false,
    };
    let skips = |p: &Param| matches!(p.binds, Binds::Optional | Binds::Rest);
    let ends = |p: &Param, j: usize| {
        let Some(first) = labels.get(j) else {
            return j..j;
        };
        if first != p.label.as_deref().unwrap_or("_") && !first.is_empty() {
            return j..j;
        }
        if first.is_empty() && refuses(p) {
            return j..j;
        }
        let mut k = j + 1;
        while p.binds == Binds::Rest && labels.get(k).is_some_and(|l| l == "_") {
            k += 1;
        }
        j + 1..k + 1
    };
    if (params.len() + 1).saturating_mul(labels.len() + 1) > LABELLED_CELLS {
        return None;
    }
    let mut fit = vec![vec![false; labels.len() + 1]; params.len() + 1];
    fit[params.len()][labels.len()] = true;
    for (n, p) in params.iter().enumerate().rev() {
        for j in 0..=labels.len() {
            fit[n][j] = (skips(p) && fit[n + 1][j]) || ends(p, j).any(|e| fit[n + 1][e]);
        }
    }
    if !fit[0][0] {
        return None;
    }
    let mut takes = vec![None; params.len()];
    let mut j = 0;
    for (n, p) in params.iter().enumerate() {
        let skip = skips(p) && fit[n + 1][j];
        let closure = labels.get(j).is_some_and(String::is_empty);
        match ends(p, j).rev().find(|&e| fit[n + 1][e]) {
            Some(end) if !(skip && closure) => {
                takes[n] = Some(j);
                j = end;
            }
            _ => {}
        }
    }
    Some(takes)
}

struct Group<'a> {
    files: Vec<&'a str>,
    tops: HashMap<&'a str, Vec<&'a str>>,
    methods: HashMap<&'a str, Vec<&'a str>>,
    exporting: Named<'a>,
    families: HashMap<&'a str, Vec<Target<'a>>>,
}

struct Extension<'a> {
    target: Target<'a>,
    within: &'a str,
    class: Option<Target<'a>>,
    private: bool,
}

struct Block<'a> {
    path: &'a str,
    scope: &'a str,
    of_trait: bool,
    of: Option<Target<'a>>,
    outside: Option<String>,
}

impl<'a> Resolver<'a> {
    #[cfg(test)]
    pub fn new(
        files: &'a BTreeMap<&'a str, &'a Facts>,
        modules: Modules,
        language: &dyn LanguagePack,
    ) -> Self {
        Self::with_shared(files, modules, Vec::new(), language)
    }

    #[cfg(test)]
    pub fn with_shared(
        files: &'a BTreeMap<&'a str, &'a Facts>,
        modules: Modules,
        shared: Vec<Vec<String>>,
        language: &dyn LanguagePack,
    ) -> Self {
        Self::built(files, modules, shared, language, None, BTreeSet::new())
    }

    pub fn with_layout(
        files: &'a BTreeMap<&'a str, &'a Facts>,
        layout: crate::lang::Layout,
        language: &dyn LanguagePack,
    ) -> Self {
        Self::with_layout_and_packages(files, layout, language, BTreeSet::new())
    }

    pub(crate) fn with_layout_and_packages(
        files: &'a BTreeMap<&'a str, &'a Facts>,
        layout: crate::lang::Layout,
        language: &dyn LanguagePack,
        packages: BTreeSet<String>,
    ) -> Self {
        let resolver = Self::built(
            files,
            layout.modules,
            layout.shared,
            language,
            layout.reaches,
            packages,
        );
        resolver.publishing(layout.published)
    }

    fn built(
        files: &'a BTreeMap<&'a str, &'a Facts>,
        modules: Modules,
        shared: Vec<Vec<String>>,
        language: &dyn LanguagePack,
        reaches: Option<Reaches>,
        packages: BTreeSet<String>,
    ) -> Self {
        let defs: HashMap<&str, HashMap<&str, &Def>> = files
            .iter()
            .map(|(path, facts)| {
                let names = facts.defs.iter().map(|d| (d.name.as_str(), d)).collect();
                (*path, names)
            })
            .collect();
        let named: HashMap<&str, Named> = (files.iter())
            .map(|(path, facts)| (*path, Named::exports(&facts.exports)))
            .collect();
        let mut arities = HashMap::new();
        let mut families = |select: fn(&Facts) -> &BTreeMap<String, Vec<String>>| {
            files
                .iter()
                .map(|(&path, facts)| {
                    let aliases = select(facts)
                        .iter()
                        .map(|(alias, names)| {
                            let targets = names
                                .iter()
                                .filter_map(|name| {
                                    let (&name, def) = defs[path].get_key_value(name.as_str())?;
                                    let target = Target { path, name };
                                    arities.insert(target, Arity::of(def));
                                    Some(target)
                                })
                                .collect();
                            (alias.as_str(), targets)
                        })
                        .collect();
                    (path, aliases)
                })
                .collect()
        };
        let overloads: HashMap<&str, HashMap<&str, Vec<Target>>> = families(|f| &f.overloads);
        let constructors = families(|f| &f.constructors);
        let statics = files
            .iter()
            .flat_map(|(&path, facts)| {
                facts.statics.iter().map(move |name| Target {
                    path,
                    name: name.as_str(),
                })
            })
            .collect();
        let mut groups = Vec::new();
        let mut sharing = HashMap::new();
        for group in &shared {
            let members: Vec<&str> = (group.iter())
                .filter_map(|p| files.get_key_value(p.as_str()).map(|(p, _)| *p))
                .collect();
            let mut tops: HashMap<&str, Vec<&str>> = HashMap::new();
            for &path in &members {
                let facts = files[path];
                let defs = (facts.defs.iter())
                    .filter(|d| !matches!(d.kind, Kind::Method | Kind::Inline))
                    .map(|d| d.name.as_str());
                let names = defs.chain(facts.consts.iter().map(|c| c.name.as_str()));
                for name in names.filter(|n| !n.contains('.')) {
                    let found = tops.entry(name).or_default();
                    if found.last() != Some(&path) {
                        found.push(path);
                    }
                }
            }
            let (mut methods, mut exporting) = (HashMap::new(), Named::default());
            for (at, &path) in members.iter().enumerate() {
                let defs = defs[path].iter().filter(|(_, d)| d.kind == Kind::Method);
                for (&name, _) in defs {
                    methods.entry(name).or_insert_with(Vec::new).push(path);
                }
                let named: &Named = &named[path];
                for &name in named.names.keys() {
                    exporting.names.entry(name).or_default().push(at);
                }
                if !named.stars.is_empty() {
                    exporting.stars.push(at);
                }
                sharing.insert(path, groups.len());
            }
            let mut families: HashMap<&str, Vec<Target>> = HashMap::new();
            for &path in &members {
                for (&alias, targets) in &overloads[path] {
                    if !alias.contains('.') {
                        families.entry(alias).or_default().extend(targets);
                    }
                }
            }
            families.values_mut().for_each(|f| f.sort());
            groups.push(Group {
                files: members,
                tops,
                methods,
                exporting,
                families,
            });
        }
        let mut parts: HashMap<(&str, &str), Vec<&str>> = HashMap::new();
        for (&path, facts) in files {
            for name in &facts.partial {
                let key = (facts.unit.as_str(), name.as_str());
                parts.entry(key).or_default().push(path);
            }
        }
        let mut actuals: HashMap<&str, Vec<&str>> = HashMap::new();
        for (&path, facts) in files {
            for name in &facts.actual {
                actuals.entry(name.as_str()).or_default().push(path);
            }
        }
        parts.retain(|_, paths| {
            paths.dedup();
            (2..=TYPE_PARTS).contains(&paths.len())
        });
        let marks: Vec<(&str, &str)> = (files.iter())
            .flat_map(|(&path, f)| f.consts.iter().map(move |c| (path, c)))
            .filter_map(|(path, c)| Some((path, c.name.strip_prefix('.')?)))
            .collect();
        let written = (marks.iter())
            .filter(|(_, m)| !m.contains('@'))
            .map(|&(_, m)| m)
            .collect();
        let written_outside_tests = (marks.iter())
            .filter(|(path, m)| !m.contains('@') && !path.ends_with("_test.go"))
            .map(|&(_, m)| m)
            .collect();
        let written_in = (marks.iter())
            .filter_map(|(_, m)| m.split_once('@'))
            .map(|(name, class)| (class, name))
            .collect();
        let params = (defs.iter())
            .map(|(path, names)| {
                let mut params = HashMap::new();
                for (&function, def) in names {
                    for (n, p) in def.params.iter().enumerate() {
                        params
                            .entry((function, p.name.as_str()))
                            .or_insert((n, None));
                        for (f, field) in p.fields.iter().enumerate() {
                            let name = field.name.as_str();
                            params.entry((function, name)).or_insert((n, Some(f)));
                        }
                    }
                }
                (*path, params)
            })
            .collect();
        let consts = files
            .iter()
            .map(|(path, facts)| {
                let names = facts.consts.iter().map(|c| c.name.as_str()).collect();
                (*path, names)
            })
            .collect();
        let internal = (files.iter())
            .filter(|(_, facts)| !facts.unit.is_empty() && !facts.private.is_empty())
            .map(|(path, facts)| (*path, facts.private.iter().map(String::as_str).collect()))
            .collect();
        let confined = (files.iter())
            .filter(|(_, facts)| !facts.confined.is_empty())
            .map(|(path, facts)| (*path, facts.confined.iter().map(String::as_str).collect()))
            .collect();
        let sealed = (files.iter())
            .filter(|(_, facts)| !facts.sealed.is_empty())
            .map(|(path, facts)| (*path, facts.sealed.iter().map(String::as_str).collect()))
            .collect();
        let locals = files
            .iter()
            .map(|(path, facts)| (*path, facts.locals.iter().map(String::as_str).collect()))
            .collect();
        let receivers = files
            .iter()
            .map(|(path, facts)| {
                let scoped: HashSet<_> = facts
                    .ts_locals
                    .values()
                    .flatten()
                    .filter_map(|b| b.receiver)
                    .collect();
                let names = facts
                    .receivers
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !scoped.contains(index))
                    .map(|(_, l)| ((l.scope.as_str(), l.name.as_str()), &l.class))
                    .collect();
                (*path, names)
            })
            .collect();
        let by_local = (files.iter())
            .map(|(path, facts)| (*path, Named::imports(&facts.imports)))
            .collect();
        let externs = files
            .iter()
            .map(|(path, facts)| (*path, facts.externs.iter().map(String::as_str).collect()))
            .collect();
        let pathed = (files.iter())
            .filter(|(_, facts)| {
                (facts.imports.iter()).any(|i| matches!(i.name, Imported::Path(_)))
            })
            .map(|(path, _)| *path)
            .collect();
        let takers = (files.iter())
            .map(|(path, facts)| (*path, facts.star.taker()))
            .collect();
        let mut by_name: HashMap<_, Vec<Target>> = HashMap::new();
        let mut ts_literal_candidates: HashMap<_, Vec<Target>> = HashMap::new();
        for (path, facts) in files {
            let names: &HashMap<&str, &Def> = &defs[path];
            let in_modules = |mut scope: &str| {
                while !scope.is_empty() {
                    if names.get(scope).map(|d| d.kind) != Some(Kind::Module) {
                        return false;
                    }
                    scope = crate::parent(scope);
                }
                true
            };
            for def in &facts.defs {
                let name = def.name.as_str();
                if arities.contains_key(&Target { path, name }) {
                    continue;
                }
                if def.kind == Kind::Function
                    && let Some((owner, member)) = name.rsplit_once('.')
                    && facts
                        .ts_literal_methods
                        .get(owner)
                        .is_some_and(|methods| methods.contains_key(member))
                {
                    ts_literal_candidates
                        .entry(member)
                        .or_default()
                        .push(Target { path, name });
                }
                let key = match (def.kind, name.rsplit_once('.')) {
                    (Kind::Method, Some((_, last))) => (true, last),
                    (Kind::Function | Kind::Class, None) => (false, name),
                    (Kind::Function | Kind::Class, Some((scope, last))) if in_modules(scope) => {
                        (false, last)
                    }
                    _ => continue,
                };
                by_name.entry(key).or_default().push(Target { path, name });
            }
            for (alias, names) in &facts.overloads {
                let last = alias.rsplit('.').next().unwrap_or(alias);
                for name in names {
                    by_name
                        .entry((true, last))
                        .or_default()
                        .push(Target { path, name });
                }
            }
        }
        by_name.values_mut().for_each(|targets| targets.sort());
        let stars_bind = language.stars_bind();
        let later = (files.iter().filter(|_| stars_bind))
            .filter_map(|(path, facts)| {
                let (mut stars, mut after) = (Vec::new(), HashMap::new());
                let mut sources = HashSet::new();
                for export in &facts.exports {
                    match export {
                        Export::All { source } if sources.insert(source.as_str()) => {
                            stars.push(source.as_str());
                        }
                        Export::Local { exported, .. } if !stars.is_empty() => {
                            after.insert(exported.as_str(), stars.len());
                        }
                        _ => {}
                    }
                }
                (!stars.is_empty()).then_some((*path, (stars, after)))
            })
            .collect();
        let binders = if language.members_in_scope() {
            Binders::of(&locals, &consts, &params, &receivers, &overloads)
        } else {
            HashMap::new()
        };
        let declared = (files.iter())
            .filter(|(_, facts)| !facts.declarations.is_empty())
            .map(|(&path, facts)| {
                let names = facts.declarations.iter().map(|d| d.name.as_str());
                (path, names.collect())
            })
            .collect();
        let ts = language.extensions().contains(&"ts");
        let mut resolver = Self {
            ts,
            ts_packages: packages,
            ts_heirs: HashMap::new(),
            ts_writes: None,
            ts_site: Cell::new(None),
            ts_key_read: Cell::new(false),
            ts_locals: if ts {
                ts::locals(files)
            } else {
                HashMap::new()
            },
            ts_overrides: RefCell::default(),
            files,
            declared,
            defs,
            overloads,
            constructors,
            arities,
            statics,
            members_in_scope: language.members_in_scope(),
            drops_external_uses: language.drops_external_uses(),
            assigned_members: language.assigned_members(),
            guesses_constructs: language.guesses_constructs(),
            unordered_globs: language.unordered_globs(),
            combines_globs: language.combines_globs(),
            qualified_types: language.qualified_types(),
            static_imports_inherit: language.static_imports_inherit(),
            namespaced: language.namespaced(),
            written,
            written_outside_tests,
            written_in,
            consts,
            internal,
            confined,
            sealed,
            locals,
            receivers,
            params,
            named,
            by_local,
            externs,
            pathed,
            takers,
            families: HashMap::new(),
            linearizing: RefCell::new(Vec::new()),
            binders,
            cut: Cell::new(false),
            links: Cell::new(0),
            chain: Cell::new(0),
            companions: HashMap::new(),
            receiver: language.receiver(),
            base_receiver: language.base_receiver(),
            unordered_bases: language.unordered_bases(),
            extension_methods: language.extension_methods(),
            outside_blocks: language.outside_blocks(),
            implicit: language.implicit_members(),
            known: crate::lang::known_outside(language),
            globals: language.globals(),
            callers: HashMap::new(),
            test_callers: HashMap::new(),
            ts_constructions: HashMap::new(),
            built: HashMap::new(),
            built_final: false,
            extension_callers: HashMap::new(),
            by_name,
            ts_literal_candidates,
            modules,
            impls: HashMap::new(),
            blocks: HashMap::new(),
            outside: HashSet::new(),
            unread: HashSet::new(),
            extensions: HashMap::new(),
            extending: (files.keys().copied())
                .filter(|p| {
                    crate::lang::pack_for(p)
                        .is_some_and(|n| crate::lang::pack(n).extension_functions())
                })
                .collect(),
            published: None,
            reaches: None,
            stars_bind,
            strict: language.strict(),
            later,
            shared: sharing,
            groups,
            parts,
            actuals,
        };
        if resolver.ts {
            resolver.ts_heirs = resolver.index_ts_heirs();
        }
        let mut work = Work::default();
        for (path, facts) in files {
            for def in facts.defs.iter().filter(|d| d.kind == Kind::Class) {
                resolver.family(
                    Target {
                        path,
                        name: &def.name,
                    },
                    &mut work,
                );
            }
        }
        resolver.families = work.families;
        let mut companions = HashMap::new();
        for (path, facts) in files {
            for def in facts.defs.iter().filter(|d| d.kind == Kind::Class) {
                let class = Target {
                    path,
                    name: &def.name,
                };
                if let Some(companion) = resolver.companion(class, &mut Work::default()) {
                    companions.insert(class, companion);
                }
            }
        }
        resolver.companions = companions;
        let mut impls: HashMap<Target, Vec<Block>> = HashMap::new();
        let mut blocks = HashMap::new();
        let mut outside = HashSet::new();
        let mut unread = HashSet::new();
        let mut extensions: HashMap<&str, Vec<Extension>> = HashMap::new();
        for (path, facts) in files {
            for block in &facts.impls {
                let class = |name: &'a str| match resolver.path_binding(path, &block.within, name) {
                    Binding::Def(t) if resolver.is_class(t) => Some(t),
                    _ => None,
                };
                let Some(ty) = class(&block.ty) else {
                    let ty = resolver.path_binding(path, &block.within, &block.ty);
                    if matches!(ty, Binding::External(..)) {
                        outside.insert((*path, block.scope.as_str()));
                    }
                    unread.insert((*path, block.scope.as_str()));
                    continue;
                };
                blocks.insert((*path, block.scope.as_str()), ty);
                if (resolver.defs[path].get(block.scope.as_str()))
                    .is_some_and(|d| d.kind == Kind::Inline)
                {
                    continue;
                }
                let of = block.of.as_deref().and_then(class);
                let outside = (block.of.as_deref())
                    .filter(|_| of.is_none())
                    .map(|p| resolver.imported_name(path, &block.within, p));
                impls.entry(ty).or_default().push(Block {
                    path,
                    scope: &block.scope,
                    of_trait: block.of.is_some(),
                    of,
                    outside,
                });
            }
            let mut extended = HashMap::new();
            for block in &facts.extensions {
                let class = match resolver.path_binding(path, &block.within, &block.ty) {
                    Binding::Def(t) if resolver.is_class(t) => Some(t),
                    Binding::External(..) => {
                        outside.insert((*path, block.scope.as_str()));
                        None
                    }
                    _ => None,
                };
                let at = (*path, block.scope.as_str());
                if let Some(t) = class {
                    blocks.insert(at, t);
                } else {
                    unread.insert(at);
                }
                extended.insert(block.scope.as_str(), (class, block.within.as_str()));
            }
            if extended.is_empty() {
                continue;
            }
            let exported: HashSet<&str> = (facts.exports.iter())
                .filter_map(|e| match e {
                    Export::Local { local, .. } => Some(local.as_str()),
                    _ => None,
                })
                .collect();
            let defs = &resolver.defs[path];
            for (alias, targets) in &resolver.overloads[path] {
                let Some((scope, name)) = alias.rsplit_once('.') else {
                    continue;
                };
                let Some(&(class, within)) = extended.get(scope) else {
                    continue;
                };
                let class_scope = defs.get(scope).is_some_and(|d| d.kind == Kind::Class);
                for &target in targets {
                    if class_scope && defs[target.name].kind == Kind::Method {
                        continue;
                    }
                    extensions.entry(name).or_default().push(Extension {
                        target,
                        within,
                        class,
                        private: !exported.contains(alias),
                    });
                }
            }
        }
        extensions
            .values_mut()
            .for_each(|e| e.sort_by_key(|e| e.target));
        impls
            .values_mut()
            .for_each(|b| b.sort_by_key(|b| b.of_trait));
        (resolver.impls, resolver.blocks, resolver.outside) = (impls, blocks, outside);
        (resolver.unread, resolver.extensions) = (unread, extensions);
        let mut reaches = reaches;
        if !resolver.extension_methods {
            resolver.reaches = reaches.take();
            resolver.built_final = true;
        }
        if resolver.ts {
            resolver.ts_writes = Some(ts::Writes::index(&resolver));
        }
        let mut callers: HashMap<Target, Vec<Caller>> = HashMap::new();
        let mut extension_callers: HashMap<Target, Vec<Caller>> = HashMap::new();
        let mut built = HashMap::new();
        for (path, facts) in files {
            let test = crate::http::is_test_file(path);
            let supplies = |target: Target| !test || crate::http::is_test_file(target.path);
            let built: &mut Vec<Resolution> = built.entry(*path).or_default();
            for (index, call) in facts.calls.iter().enumerate() {
                let resolution = resolver.call(path, call);
                built.push(resolution.clone());
                match resolution {
                    Resolution::Exact(target) => {
                        if !supplies(target) {
                            if path.ends_with(".go") {
                                resolver
                                    .test_callers
                                    .entry(target)
                                    .or_default()
                                    .push(Caller { path, index });
                            }
                            continue;
                        }
                        callers
                            .entry(target)
                            .or_default()
                            .push(Caller { path, index });
                        if resolver.ts && call.constructs {
                            resolver
                                .ts_constructions
                                .entry(target)
                                .or_default()
                                .push(Caller { path, index });
                        }
                    }
                    Resolution::Inferred(target)
                        if supplies(target) && resolver.extension_call(path, call, target) =>
                    {
                        (extension_callers.entry(target).or_default()).push(Caller { path, index });
                    }
                    _ => {}
                }
            }
        }
        (resolver.callers, resolver.extension_callers) = (callers, extension_callers);
        resolver.built = built;
        if reaches.is_some() {
            resolver.reaches = reaches;
        }
        resolver
    }

    fn extension_call(&self, path: &'a str, call: &'a Call, target: Target<'a>) -> bool {
        let class = crate::parent(target.name);
        let simple = class.rsplit('.').next().unwrap_or(class);
        self.extension_methods
            && call.callee.contains('.')
            && (self.defs[target.path][target.name].params.first())
                .is_some_and(|p| p.binds == Binds::Instance)
            && matches!(self.binding(path, &call.scope, simple), Binding::Def(c) if c.name == class)
    }

    fn flows_to(&self, path: &'a str, call: &'a Call) -> Option<Target<'a>> {
        match self.call(path, call) {
            Resolution::Exact(target) => Some(target),
            Resolution::Inferred(target) if self.extension_call(path, call, target) => Some(target),
            _ => None,
        }
    }

    pub fn publishing(mut self, published: Option<BTreeSet<String>>) -> Self {
        self.published = published;
        self
    }

    pub fn call(&self, path: &'a str, call: &'a Call) -> Resolution<'a> {
        self.resolved(path, call, true, &mut Work::default())
    }

    pub fn resolved_at(&self, path: &'a str, index: usize) -> Resolution<'a> {
        match self.built.get(path).and_then(|b| b.get(index)) {
            Some(known) if self.built_final => known.clone(),
            Some(exact @ Resolution::Exact(_)) => exact.clone(),
            _ => self.call(path, &self.files[path].calls[index]),
        }
    }

    pub fn reference(&self, path: &'a str, index: usize) -> Option<Option<(&'a str, &'a str)>> {
        let found = self.named(path, index)?;
        match found {
            None if self.drops_external_uses => None,
            found => Some(found),
        }
    }

    fn named(&self, path: &'a str, index: usize) -> Option<Option<(&'a str, &'a str)>> {
        let r = self.files[path].references.get(index)?;
        let (scope, name) = (r.scope.as_str(), r.name.as_str());
        let bare = |s: &str| !s.starts_with('.') && !s.starts_with('/');
        let rows = || Seen {
            rows: true,
            ..self.seen(path, scope)
        };
        let work = &mut Work::default();
        match (r.kind, r.from.as_deref()) {
            (Use::Dynamic | Use::Reexport, Some(from))
                if bare(from) && self.module(path, from).is_none() =>
            {
                return Some(None);
            }
            (Use::Reexport, _) if name == "*" => return None,
            (Use::Reexport, Some(from)) => {
                let imported = (self.files[path].exports.iter()).find_map(|e| match e {
                    Export::From {
                        source,
                        name: imported @ Imported::Name(n),
                        ..
                    } if source == from && n == name => Some((source.as_str(), imported)),
                    _ => None,
                })?;
                let found = self.import(path, "", imported.0, imported.1, &mut rows());
                return self.edge(found, true);
            }
            (Use::Dynamic, Some(from)) => {
                let module = self.module(path, from)?;
                return self.edge(self.export_as(module, name, &mut rows(), true), true);
            }
            _ => {}
        }
        let head_of_call = r.kind == Use::Member && !name.contains('.');
        let reading = match r.kind {
            Use::Type => Reading::Type,
            _ => Reading::Read,
        };
        let mut prefix = name;
        loop {
            let mut found =
                self.resolve_labelled(path, scope, prefix, &mut rows(), work, reading, None);
            if !matches!(found, Binding::Def(_) | Binding::External(..)) && reading != Reading::Read
            {
                found = self.resolve_labelled(
                    path,
                    scope,
                    prefix,
                    &mut rows(),
                    work,
                    Reading::Read,
                    None,
                );
            }
            if matches!(found, Binding::Unknown)
                && !prefix.contains('.')
                && self.declared.get(path).is_some_and(|d| d.contains(prefix))
            {
                found = Binding::Const(path, prefix);
            }
            if let Some(edge) = self.edge(found, !head_of_call) {
                return Some(edge);
            }
            prefix = prefix.rsplit_once('.')?.0;
        }
    }

    fn edge(&self, found: Binding<'a>, defs: bool) -> Option<Option<(&'a str, &'a str)>> {
        match found {
            Binding::Def(t) if defs => Some(Some((t.path, t.name))),
            Binding::Const(p, n) if self.declared.get(p).is_some_and(|d| d.contains(n)) => {
                Some(Some((p, n)))
            }
            Binding::External(source, _)
                if defs && !source.starts_with('.') && !source.starts_with('/') =>
            {
                Some(None)
            }
            _ => None,
        }
    }

    fn resolved(
        &self,
        path: &'a str,
        call: &'a Call,
        matching: bool,
        work: &mut Work<'a>,
    ) -> Resolution<'a> {
        if self.ts {
            return self.at_ts_site(path, call.range.start, || {
                self.resolved_as(path, call, false, matching, work)
            });
        }
        let found = self.resolved_as(path, call, call.constructs, matching, work);
        if !(self.guesses_constructs && call.constructs) || matches!(found, Resolution::Exact(_)) {
            return found;
        }
        let plain = self.resolved_as(path, call, false, matching, work);
        let function = |t: &Target| self.defs[t.path][t.name].kind != Kind::Class;
        let better = matches!(found, Resolution::Unresolved);
        match &plain {
            Resolution::Exact(t) if function(t) => plain,
            Resolution::Inferred(t) | Resolution::Narrowed(t) if better && function(t) => plain,
            Resolution::Ambiguous(all) if better && all.iter().all(function) => plain,
            _ => found,
        }
    }

    fn resolved_as(
        &self,
        path: &'a str,
        call: &'a Call,
        constructs: bool,
        matching: bool,
        work: &mut Work<'a>,
    ) -> Resolution<'a> {
        if self.strict && call.decorates.is_some() {
            return Resolution::Unresolved;
        }
        let (scope, callee) = (call.scope.as_str(), call.callee.as_str());
        if self.ts
            && let Some(answer) = self.ts_library_call(path, call, matching, work)
        {
            return answer;
        }
        let seen = &mut self.seen(path, scope);
        let mut on = None;
        let labels = call.labels.as_deref();
        let binding = match (call.on, constructs, call.by_path) {
            (None, false, false)
                if self.ts
                    && let Some(root) = call.root.as_deref() =>
            {
                let receiver = self.read_member(path, scope, root);
                let members = own_name(callee);
                if root == "this"
                    && let Binding::Def(class) = receiver
                    && !self.ts_override(class, members)
                {
                    self.member(class, members, false, labels, work)
                } else if self.holder(&receiver).is_some() {
                    self.walk(receiver, members, seen, work, Reading::Value, labels)
                } else {
                    Binding::Unknown
                }
            }
            (Some(inner), false, _) => {
                let members = callee.split_once('.').map_or("", |(_, m)| m);
                match self.on_result(path, call, inner, work) {
                    Some(result) => {
                        on = Some(result.clone());
                        self.walk(result, members, seen, work, Reading::Value, labels)
                    }
                    None => self.resolve_labelled(
                        path,
                        scope,
                        callee,
                        seen,
                        work,
                        Reading::Value,
                        labels,
                    ),
                }
            }
            (_, true, _) => self.resolve_as(path, scope, callee, seen, work, Reading::Type),
            (_, false, true) if self.members_in_scope && !callee.contains('.') => {
                self.resolve_labelled(path, scope, callee, seen, work, Reading::Method, labels)
            }
            (_, false, true) => {
                self.resolve_labelled(path, scope, callee, seen, work, Reading::Path, labels)
            }
            (_, false, false) => {
                let reading = match call.decorates {
                    Some(_) => Reading::Decorator,
                    None => Reading::Value,
                };
                self.resolve_labelled(path, scope, callee, seen, work, reading, labels)
            }
        };
        let binding = match binding {
            Binding::Several(all) if constructs => {
                let classes: Vec<_> = (all.into_iter())
                    .filter(|t| self.is_class(*t) && !self.sealed_from(call.scope.as_str(), *t))
                    .collect();
                match classes[..] {
                    [] => return Resolution::Unresolved,
                    [one] => Binding::Def(one),
                    _ => self
                        .one_type(&classes)
                        .map_or(Binding::Several(classes), Binding::Def),
                }
            }
            found => found,
        };
        if constructs && matches!(binding, Binding::Def(t) if !self.is_class(t)) {
            return Resolution::Unresolved;
        }
        let binding = match binding {
            Binding::Def(t) if call.arity.is_some() && self.is_class(t) => {
                let (families, absent) = if constructs {
                    (&self.constructors, Binding::Def(t))
                } else {
                    (&self.overloads, Binding::Unknown)
                };
                let blocks = self.impls.get(&t).into_iter().flatten();
                let scopes = (self.parts(t).into_iter())
                    .map(|part| (part.path, part.name))
                    .chain(blocks.map(|b| (b.path, b.scope)));
                let mut all: Vec<Target> = scopes
                    .flat_map(|(path, scope)| families[path].get(scope).into_iter().flatten())
                    .copied()
                    .collect();
                all.sort();
                all.dedup();
                all.retain(|t| !self.sealed_from(call.scope.as_str(), *t));
                let absent = match absent {
                    Binding::Def(t) if self.sealed_from(call.scope.as_str(), t) => {
                        return Resolution::Unresolved;
                    }
                    absent => absent,
                };
                match all.is_empty() {
                    true => absent,
                    false => self.accessible(path, Binding::Several(all)),
                }
            }
            found => found,
        };
        let known = matches!(binding, Binding::Def(_) | Binding::Several(_));
        let binding = self.narrow(binding, Args::of(call), &|| self.on_receiver(path, call));
        if matches!(binding, Binding::Unknown | Binding::External(..))
            && let Some(found) = self.extended(path, call, constructs, on.clone(), work)
        {
            return found;
        }
        if let Some(tops) = self.top_level_functions(path, call, constructs, &binding)
            && let Some(found) = self.extended(path, call, constructs, on.clone(), work)
        {
            return match found {
                Resolution::Exact(_) => found,
                _ if !matching => Resolution::Unresolved,
                Resolution::Inferred(t) | Resolution::Narrowed(t) => {
                    one_of_all([&[t][..], &tops].concat())
                }
                Resolution::Ambiguous(all) => one_of_all([&all[..], &tops].concat()),
                Resolution::Unresolved => Resolution::Ambiguous(tops),
            };
        }
        if known && matches!(binding, Binding::Unknown) {
            return Resolution::Unresolved;
        }
        match binding {
            Binding::Def(t) => Resolution::Exact(t),
            _ if !matching => Resolution::Unresolved,
            Binding::Several(all) => one_of(all),
            Binding::External(..) => Resolution::Unresolved,
            Binding::Unknown if self.owned_outside(path, call) || self.lacks(path, call) => {
                Resolution::Unresolved
            }
            Binding::Instance(_)
            | Binding::Module(_)
            | Binding::Const(..)
            | Binding::Param(..)
            | Binding::Local(..)
            | Binding::Hidden
            | Binding::Unknown => self.matched(path, call, on, work),
        }
    }

    fn on_receiver(&self, path: &'a str, call: &'a Call) -> bool {
        let Some((head, _)) = call.callee.rsplit_once('.') else {
            return false;
        };
        if call.on.is_some() || Some(head) == self.receiver || Some(head) == self.base_receiver {
            return true;
        }
        let (seen, work) = (&mut self.seen(path, &call.scope), &mut Work::default());
        let found = self.resolve_as(path, &call.scope, head, seen, work, Reading::Value);
        !matches!(found, Binding::Def(t) if self.is_class(t))
    }

    fn outside_type(
        &self,
        path: &'a str,
        scope: &'a str,
        head: &'a str,
        receiver: &Binding<'a>,
        work: &mut Work<'a>,
    ) -> Option<Option<&'a str>> {
        let outside = |name: &'a str, work: &mut Work<'a>| -> Option<&'a str> {
            let seen = &mut self.seen(path, scope);
            let found = self.resolve_as(path, scope, name, seen, work, Reading::Path);
            let spelled = seen.outside.filter(|_| !seen.unsure && !seen.cut);
            matches!(found, Binding::Unknown | Binding::External(..))
                .then_some(spelled.unwrap_or(name))
        };
        if let Some((p, s, n)) = self.holder(receiver) {
            return match self.receivers[p].get(&(s, n)) {
                Some(Class::Type(ty) | Class::Named(ty)) => outside(ty, work).map(Some),
                Some(Class::Member(read)) => {
                    let first = read.split('.').next().unwrap_or(read);
                    (first.starts_with(|c: char| c.is_ascii_uppercase())
                        && outside(first, work).is_some())
                    .then_some(None)
                }
                _ => None,
            };
        }
        let first = head.split('.').next().unwrap_or(head);
        if !first.starts_with(|c: char| c.is_ascii_uppercase()) {
            return None;
        }
        let spelled = outside(first, work)?;
        Some((first == head).then_some(spelled))
    }

    fn matched(
        &self,
        path: &'a str,
        call: &'a Call,
        on: Option<Binding<'a>>,
        work: &mut Work<'a>,
    ) -> Resolution<'a> {
        let (scope, callee) = (call.scope.as_str(), call.callee.as_str());
        if call.arity.is_none() {
            return self.infer(path, callee, call.by_path, None, false);
        }
        let head = callee.rsplit_once('.').map(|(head, _)| head);
        if head.is_some() && head == self.base_receiver {
            return Resolution::Unresolved;
        }
        let bare = head.is_none() && self.members_in_scope;
        let on_receiver = || self.on_receiver(path, call);
        let mut all = self.candidates(
            path,
            callee,
            call.by_path,
            Args::of(call),
            bare,
            &on_receiver,
        );
        all.retain(|t| !self.sealed_from(scope, *t));
        let mut written = false;
        let mut outside = None;
        let owner = match head {
            None if bare => self.class(path, scope),
            None => Binding::Unknown,
            Some(head) => {
                let seen = &mut self.seen(path, scope);
                let (on, inner) = match head.contains('.') {
                    true => (None, None),
                    false => (on, call.on),
                };
                let receiver = match (on, inner) {
                    (Some(known), _) => Some(known),
                    (None, Some(inner)) => self.on_result(path, call, inner, work),
                    (None, None) => None,
                };
                let receiver = receiver.unwrap_or_else(|| {
                    self.resolve_as(path, scope, head, seen, work, Reading::Value)
                });
                written = (self.holder(&receiver)).is_some_and(|(p, s, n)| {
                    matches!(self.receivers[p].get(&(s, n)), Some(Class::Type(_)))
                });
                if self.extension_methods
                    && !written
                    && let Some((p, s, n)) = self.holder(&receiver)
                    && let Some(Class::Result(made)) = self.receivers[p].get(&(s, n))
                {
                    written = self.outside_result(p, *made, work);
                }
                if self.outside_blocks {
                    outside = self.outside_type(path, scope, head, &receiver, work);
                }
                self.instance(receiver, work)
            }
        };
        if let Some(ty) = outside {
            let blocks = (all.iter().copied())
                .filter(|t| {
                    let block = crate::parent(t.name);
                    ty == Some(block) && self.unread.contains(&(t.path, block))
                })
                .collect();
            return one_of(blocks);
        }
        let on_instance = matches!(owner, Binding::Instance(_)) || head == self.receiver;
        let extensions = |all: &[Target<'a>]| -> Vec<Target<'a>> {
            (all.iter().copied())
                .filter(|t| {
                    (self.defs[t.path][t.name].params.first())
                        .is_some_and(|p| p.binds == Binds::Instance)
                })
                .collect()
        };
        if self.extension_methods
            && written
            && !matches!(owner, Binding::Instance(_) | Binding::Def(_))
        {
            return one_of(extensions(&all));
        }
        let family = match owner {
            Binding::Def(t) | Binding::Instance(t) if self.is_class(t) => {
                self.family(t, work).map(|f| (f, true))
            }
            _ => match self.class(path, scope) {
                Binding::Def(t) => self.family(t, work).map(|f| (f, false)),
                _ => None,
            },
        };
        let Some((family, owned)) = family else {
            return one_of(all);
        };
        let classes: HashSet<Target<'a>> = (family.iter())
            .filter_map(|a| match a {
                Ancestor::Class(c) => Some(*c),
                Ancestor::Opaque(_) => None,
            })
            .flat_map(|c| {
                let actuals = self.actuals.get(c.name).filter(|_| self.is_expect(c));
                let actuals = actuals.into_iter().flatten();
                std::iter::once(c).chain(actuals.map(move |&path| Target { path, name: c.name }))
            })
            .collect();
        let declared = |t: &Target<'a>| {
            let owner = Target {
                path: t.path,
                name: crate::parent(t.name),
            };
            classes.contains(&owner)
        };
        let caller = Target { path, name: scope };
        let narrowed = |one: Target<'a>, of: usize| match of {
            1 => Resolution::Inferred(one),
            _ => Resolution::Narrowed(one),
        };
        if !owned {
            let within: Vec<_> = (all.iter().copied())
                .filter(|t| declared(t) && *t != caller)
                .collect();
            return match (&all[..], &within[..]) {
                (_, [one]) if all.len() > 1 => narrowed(*one, all.len()),
                _ => one_of(all),
            };
        }
        let within: Vec<_> = all.iter().copied().filter(declared).collect();
        match within[..] {
            [one] => narrowed(one, all.len()),
            [_, _, ..] => Resolution::Ambiguous(within),
            [] if bare
                && (self.opaque(&family, callee) || self.outer_opaque(path, scope, callee)) =>
            {
                let found = self.resolve_as(
                    path,
                    scope,
                    callee,
                    &mut self.seen(path, scope),
                    work,
                    Reading::Path,
                );
                match self.narrow(found, Args::of(call), &|| self.on_receiver(path, call)) {
                    Binding::Def(t) => narrowed(t, all.len().max(1)),
                    Binding::Several(several) => one_of(several),
                    _ => Resolution::Unresolved,
                }
            }
            [] if self.extension_methods && on_instance && !self.opaque(&family, callee) => {
                one_of(extensions(&all))
            }
            [] => Resolution::Unresolved,
        }
    }

    fn result_of(
        &self,
        path: &'a str,
        index: usize,
        through: Through,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        let Some(call) = self.files[path].calls.get(index) else {
            return Binding::Unknown;
        };
        if !self.enter(Busy::Call(path, index), work) {
            return Binding::Unknown;
        }
        let found = match self.resolved(path, call, false, work) {
            Resolution::Exact(t) => match self.written_result(t, through) {
                Some(Class::Type(class)) => {
                    let seen = &mut self.seen(t.path, t.name);
                    match self.resolve_as(t.path, t.name, class, seen, work, Reading::Path) {
                        Binding::Def(c) if self.is_class(c) => Binding::Def(c),
                        external @ Binding::External(..) => external,
                        _ => Binding::Unknown,
                    }
                }
                _ => Binding::Unknown,
            },
            _ => Binding::Unknown,
        };
        self.leave(work);
        found
    }

    fn written_result(&self, target: Target<'a>, through: Through) -> Option<&'a Class> {
        let def = &self.defs[target.path][target.name];
        let class = match (&def.output, through.awaited, through.tried) {
            (Some(_), true, true) => &None,
            (Some(_), true, false) => &def.output,
            (None, true, false) if self.ts => &None,
            (_, _, true) => &def.unwrapped,
            _ => &def.result,
        };
        class.as_ref()
    }

    fn on_result(
        &self,
        path: &'a str,
        call: &'a Call,
        inner: usize,
        work: &mut Work<'a>,
    ) -> Option<Binding<'a>> {
        let through = Through {
            tried: call.unwraps,
            awaited: call.awaits,
        };
        let result = self.result_of(path, inner, through, work);
        (!call.unwraps || matches!(result, Binding::Def(_))).then_some(result)
    }

    // ponytail: the callee keeps no type argument, so `Deserialize<Order>(..)` counts as from
    // outside although it may give a type of the repository: a call on it takes extension
    // methods alone, a name match lost rather than one guessed.
    fn outside_result(&self, path: &'a str, index: usize, work: &mut Work<'a>) -> bool {
        let Some(call) = self.files[path].calls.get(index) else {
            return false;
        };
        let Some((head, _)) = call.callee.rsplit_once('.') else {
            return false;
        };
        if call.on.is_none() && head.starts_with('(') {
            return false;
        }
        if self.chain.get() >= RECEIVER_DEPTH || !self.enter(Busy::Call(path, index), work) {
            return false;
        }
        let unanswered = matches!(
            self.resolved(path, call, false, work),
            Resolution::Unresolved
        );
        let outside = unanswered
            && match call.on {
                Some(inner) => self.outside_result(path, inner, work),
                None => {
                    let seen = &mut self.seen(path, &call.scope);
                    let receiver =
                        self.resolve_as(path, &call.scope, head, seen, work, Reading::Value);
                    match self.holder(&receiver) {
                        Some((p, s, n)) => match self.receivers[p].get(&(s, n)) {
                            Some(Class::Type(_)) => !matches!(
                                self.instance(receiver, work),
                                Binding::Instance(_) | Binding::Def(_)
                            ),
                            Some(Class::Result(made)) => self.outside_result(p, *made, work),
                            _ => false,
                        },
                        None => matches!(receiver, Binding::Unknown | Binding::External(..)),
                    }
                }
            };
        self.leave(work);
        outside
    }

    fn narrow(
        &self,
        binding: Binding<'a>,
        args: Option<Args>,
        on_receiver: &dyn Fn() -> bool,
    ) -> Binding<'a> {
        let Some(args) = args else { return binding };
        let fit = |t: &Target<'a>| {
            (self.arities.get(t)).map_or(Fit::Yes, |n| n.fits_call(args, on_receiver))
        };
        match binding {
            Binding::Def(t) => match fit(&t) {
                Fit::No => Binding::Unknown,
                Fit::Maybe => Binding::Several(vec![t]),
                Fit::Yes => Binding::Def(t),
            },
            Binding::Several(all) => {
                let kept: Vec<_> = (all.into_iter())
                    .map(|t| (fit(&t), t))
                    .filter(|(fit, _)| *fit != Fit::No)
                    .collect();
                match kept[..] {
                    [] => Binding::Unknown,
                    [(Fit::Yes, one)] => Binding::Def(one),
                    _ => Binding::Several(kept.into_iter().map(|(_, t)| t).collect()),
                }
            }
            found => found,
        }
    }

    fn top_level_functions(
        &self,
        path: &str,
        call: &Call,
        constructs: bool,
        binding: &Binding<'a>,
    ) -> Option<Vec<Target<'a>>> {
        if constructs || call.on.is_some() || call.callee.contains('.') {
            return None;
        }
        if !self.extending.contains(path) {
            return None;
        }
        let all = match binding {
            Binding::Def(t) => vec![*t],
            Binding::Several(all) => all.clone(),
            _ => return None,
        };
        let top = |t: &Target| {
            self.defs[t.path][t.name].kind == Kind::Function
                && crate::parent(t.name) == self.files[t.path].package
        };
        all.iter().all(top).then_some(all)
    }

    fn is_object(&self, t: Target<'a>) -> bool {
        self.extending.contains(t.path)
            && self.is_class(t)
            && self.defs[t.path][t.name].result.is_none()
            && matches!(
                self.receivers[t.path].get(&(t.name, "INSTANCE")),
                Some(Class::Type(_))
            )
    }

    fn extended(
        &self,
        path: &'a str,
        call: &'a Call,
        constructs: bool,
        on: Option<Binding<'a>>,
        work: &mut Work<'a>,
    ) -> Option<Resolution<'a>> {
        if !self.extending.contains(path) || constructs {
            return None;
        }
        let scope = call.scope.as_str();
        let (head, name) = match call.callee.rsplit_once('.') {
            Some((head, name)) => (Some(head), name),
            None => (None, call.callee.as_str()),
        };
        let fits = |t: Target<'a>| {
            (call.arity).is_none_or(|n| self.arities.get(&t).is_none_or(|a| a.fits(n)))
        };
        let found: Vec<(u8, &Extension<'a>)> = (self.extensions.get(name)?.iter())
            .filter(|e| fits(e.target))
            .filter_map(|e| Some((self.visible(path, scope, name, e)?, e)))
            .collect();
        if found.is_empty() {
            return None;
        }
        let receiver = match head {
            None => self.class(path, scope),
            Some(head) if Some(head) == self.receiver => self.class(path, scope),
            Some(head) if Some(head) == self.base_receiver => return None,
            Some(head) => {
                let made = match (head.contains('.'), on) {
                    (true, _) => None,
                    (false, Some(known)) => Some(known),
                    (false, None) => call
                        .on
                        .and_then(|inner| self.on_result(path, call, inner, work)),
                };
                match made {
                    Some(known) => known,
                    None => {
                        let seen = &mut self.seen(path, scope);
                        let found = self.resolve_as(path, scope, head, seen, work, Reading::Value);
                        match found {
                            Binding::Def(t) if self.is_object(t) => found,
                            _ => {
                                self.holder(&found)?;
                                self.instance(found, work)
                            }
                        }
                    }
                }
            }
        };
        let family = match receiver {
            Binding::Def(t) | Binding::Instance(t) if self.is_class(t) => self.family(t, work),
            Binding::External(..) => None,
            _ => return None,
        };
        let applicable: Vec<(u8, Option<usize>, Target<'a>)> = (found.into_iter())
            .filter_map(|(level, e)| {
                let place = match e.class {
                    None => None,
                    Some(c) => Some(
                        family
                            .as_ref()?
                            .iter()
                            .position(|a| *a == Ancestor::Class(c))?,
                    ),
                };
                Some((level, place, e.target))
            })
            .collect();
        let known = family.as_ref().is_some_and(|f| !self.opaque(f, name))
            && !self.implicit.contains(&name);
        let first = applicable.iter().map(|a| a.0).min()?;
        let nearest = (applicable.iter())
            .filter(|a| a.0 == first)
            .filter_map(|a| a.1)
            .min()
            .filter(|_| known);
        let mut chosen: Vec<Target<'a>> = (applicable.iter())
            .filter(|a| nearest.is_none() || (a.0 == first && a.1 == nearest))
            .map(|a| a.2)
            .collect();
        chosen.sort();
        chosen.dedup();
        Some(match (nearest, &chosen[..]) {
            (Some(_), [one]) => Resolution::Exact(*one),
            (Some(_), _) => Resolution::Ambiguous(chosen),
            (None, _) => one_of(chosen),
        })
    }

    fn visible(&self, path: &str, scope: &str, name: &str, e: &Extension) -> Option<u8> {
        let package = self.files[e.target.path].package.as_str();
        if e.within != package {
            let inside = (scope.strip_prefix(e.within))
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'));
            return (e.target.path == path && inside).then_some(0);
        }
        if e.private && e.target.path != path {
            return None;
        }
        let imports = &self.files[path].imports;
        let qualified = crate::qualify(package, name);
        let explicit = (self.by_local[path].of(name).iter()).any(|&at| {
            matches!(imports[at].name, Imported::Qualified) && imports[at].source == qualified
        });
        if explicit {
            return Some(1);
        }
        if self.files[path].package == package {
            return Some(2);
        }
        let star =
            (imports.iter()).any(|i| matches!(i.name, Imported::Namespace) && i.source == package);
        star.then_some(3)
    }

    fn owned_outside(&self, path: &'a str, call: &'a Call) -> bool {
        if self.ts {
            return false;
        }
        let Some((head, last)) = call.callee.rsplit_once('.') else {
            return false;
        };
        let Some(root) = &call.root else {
            return self.strict && head.starts_with('(') && self.rooted_outside(path, call);
        };
        let callee = format!("{root}.{last}");
        let (seen, work) = (&mut self.seen(path, &call.scope), &mut Work::default());
        let found = self.resolve_as(path, &call.scope, &callee, seen, work, Reading::Value);
        matches!(found, Binding::External(..))
    }

    fn rooted_outside(&self, path: &'a str, call: &'a Call) -> bool {
        let calls = &self.files[path].calls;
        let Some(at) = call_at(calls, &call.range) else {
            return false;
        };
        let start = call.range.start;
        let chain = calls[at + 1..]
            .iter()
            .take_while(|c| c.range.start == start);
        let Some(root) = chain.last().filter(|c| !c.callee.starts_with('(')) else {
            return false;
        };
        let binding = match root.by_path {
            true => self.path_binding(path, &root.scope, &root.callee),
            false => self.binding(path, &root.scope, &root.callee),
        };
        matches!(binding, Binding::External(..))
    }

    fn lacks(&self, path: &'a str, call: &'a Call) -> bool {
        let Some((head, _)) = call.callee.rsplit_once('.').filter(|_| self.strict) else {
            return false;
        };
        let reading = match call.by_path {
            true => Reading::Path,
            false => Reading::Value,
        };
        let (seen, work) = (&mut self.seen(path, &call.scope), &mut Work::default());
        let found = self.resolve_as(path, &call.scope, head, seen, work, reading);
        let found = self.instance(found, work);
        let enclosing = head == "self" || Some(head) == self.receiver;
        matches!(found, Binding::Def(t) | Binding::Instance(t) if self.is_class(t) && !self.is_expect(t))
            || (enclosing && self.in_outside_block(path, &call.scope))
    }

    fn in_outside_block(&self, path: &'a str, scope: &str) -> bool {
        let mut scope = scope;
        while !scope.is_empty() {
            if self.outside.contains(&(path, scope)) {
                return true;
            }
            let class = self.defs[path]
                .get(scope)
                .is_some_and(|d| d.kind == Kind::Class);
            if class || self.blocks.contains_key(&(path, scope)) {
                return false;
            }
            scope = crate::parent(scope);
        }
        false
    }

    fn binding(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut self.seen(path, scope), &mut Work::default());
        self.resolve_as(path, scope, callee, seen, work, Reading::Value)
    }

    fn read_member(&self, path: &'a str, scope: &'a str, name: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut self.seen(path, scope), &mut Work::default());
        self.resolve_as(path, scope, name, seen, work, Reading::Member)
    }

    fn read(&self, path: &'a str, scope: &'a str, name: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut self.seen(path, scope), &mut Work::default());
        self.resolve_as(path, scope, name, seen, work, Reading::Read)
    }

    fn path_binding(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut self.seen(path, scope), &mut Work::default());
        self.resolve_as(path, scope, callee, seen, work, Reading::Path)
    }

    fn resolve(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        seen: &mut Seen<'a>,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        self.resolve_as(path, scope, callee, seen, work, Reading::Any)
    }

    fn resolve_as(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        seen: &mut Seen<'a>,
        work: &mut Work<'a>,
        reading: Reading,
    ) -> Binding<'a> {
        self.resolve_labelled(path, scope, callee, seen, work, reading, None)
    }

    #[expect(clippy::too_many_arguments)]
    fn resolve_labelled(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        seen: &mut Seen<'a>,
        work: &mut Work<'a>,
        reading: Reading,
        labels: Option<&'a [String]>,
    ) -> Binding<'a> {
        if let Some((head, members)) = callee.split_once('.')
            && Some(head) == self.base_receiver
        {
            let (first, rest) = members.split_once('.').unwrap_or((members, ""));
            let found =
                self.base_member(path, scope, first, work, labels.filter(|_| rest.is_empty()));
            return self.walk(found, rest, seen, work, reading, labels);
        }
        if self.assigned_members
            && let Some(found) = self.assigned(path, scope, callee)
        {
            return self.in_unit(path, self.accessible(path, found));
        }
        let lexical = |seen: &mut Seen<'a>| {
            let (head, members) = callee.split_once('.').unwrap_or((callee, ""));
            let head_reading = match reading {
                Reading::Value | Reading::Any if !members.is_empty() && self.members_in_scope => {
                    Reading::Read
                }
                reading => reading,
            };
            let binding = match head {
                _ if Some(head) == self.receiver => match self.class(path, scope) {
                    Binding::Def(t) if !members.is_empty() => Binding::Instance(t),
                    found => found,
                },
                _ if Some(head) == self.base_receiver => Binding::Unknown,
                _ if head.starts_with('(') => Binding::Unknown,
                _ => self.lexical_as(path, scope, head, seen, head_reading),
            };
            (binding, members)
        };
        let (mut binding, mut members) = if self.qualified_types {
            let found = lexical(seen);
            if matches!(found.0, Binding::Unknown) {
                self.dotted(path, callee, seen).unwrap_or(found)
            } else {
                found
            }
        } else {
            self.dotted(path, callee, seen)
                .unwrap_or_else(|| lexical(seen))
        };
        if self.qualified_types
            && matches!(binding, Binding::Unknown | Binding::Module(_))
            && !seen.cut
            && let Some(found) = self.qualified_type(path, callee, seen)
        {
            (binding, members) = found;
        }
        let found = self.walk(binding, members, seen, work, reading, labels);
        self.in_unit(path, self.accessible(path, found))
    }

    fn assigned(&self, path: &'a str, scope: &str, callee: &str) -> Option<Binding<'a>> {
        let head = callee.split_once('.')?.0;
        let mut s = scope;
        loop {
            let candidate = crate::qualify(s, callee);
            if let Some((name, def)) = self.defs[path].get_key_value(candidate.as_str())
                && def.kind != Kind::Method
            {
                if self.ts
                    && s.is_empty()
                    && let Some((object, member)) = name.split_once('.')
                    && let Some(found) = self.ts_literal_method(path, object, member, true)
                {
                    return Some(found);
                }
                return Some(Binding::Def(Target { path, name }));
            }
            let held = crate::qualify(s, head);
            if s.is_empty()
                || self.locals[path].contains(held.as_str())
                || self.consts[path].contains(held.as_str())
                || self.defs[path].contains_key(held.as_str())
                || self.params[path].contains_key(&(s, head))
            {
                return None;
            }
            s = crate::parent(s);
        }
    }

    fn walk(
        &self,
        mut binding: Binding<'a>,
        members: &'a str,
        seen: &mut Seen<'a>,
        work: &mut Work<'a>,
        reading: Reading,
        labels: Option<&'a [String]>,
    ) -> Binding<'a> {
        let members: Vec<&str> = members.split('.').filter(|m| !m.is_empty()).collect();
        for (n, member) in members.iter().enumerate() {
            let last = n + 1 == members.len();
            let value = reading.value() && last;
            let read = self.members_in_scope && (!last || reading.read());
            let labels = labels.filter(|_| last);
            let direct_instance = n == 0 && matches!(binding, Binding::Instance(_));
            let typed_ts = self.ts && self.holder(&binding).is_some();
            if self.ts
                && let Binding::Const(p, name) = binding
                && let Some(found) = self.ts_literal_method(p, name, member, false)
            {
                binding = found;
                continue;
            }
            let instance = self.instance(binding, work);
            if let Binding::Instance(t) = instance
                && let Some(found) = self.anonymous_field(t, &members[n..])
            {
                return found;
            }
            binding = match instance {
                Binding::External(..) if typed_ts => Binding::Unknown,
                Binding::Instance(class) if self.ts => {
                    let found = self.member(class, member, read, labels, work);
                    match found {
                        Binding::Def(t)
                            if self.defs[t.path][t.name].kind == Kind::Method
                                && !(direct_instance
                                    && t.path == class.path
                                    && crate::parent(t.name) == class.name)
                                && self.ts_override(class, member) =>
                        {
                            Binding::Unknown
                        }
                        _ => found,
                    }
                }
                Binding::Def(t)
                    if self.is_class(t)
                        && let Some(&c) = self.companions.get(&t) =>
                {
                    match self.member(c, member, read, labels, work) {
                        Binding::Unknown => match self.member(t, member, read, labels, work) {
                            found @ Binding::Def(d) if self.is_class(d) => found,
                            found @ Binding::Const(..) => found,
                            _ => Binding::Unknown,
                        },
                        found => found,
                    }
                }
                Binding::Def(t) | Binding::Instance(t) if self.is_class(t) => {
                    self.member(t, member, read, labels, work)
                }
                Binding::Def(t) => match self.def(t.path, &format!("{}.{member}", t.name)) {
                    Binding::Unknown if self.defs[t.path][t.name].kind == Kind::Module => {
                        self.module_member(t, member, value, seen)
                    }
                    found => found,
                },
                Binding::Module(m) => self.export_as(m, member, seen, value),
                Binding::Several(all)
                    if self.unordered_globs && all.iter().all(|t| self.is_class(*t)) =>
                {
                    alternatives(
                        all.into_iter()
                            .map(|t| self.member(t, member, read, labels, work))
                            .collect(),
                    )
                    .unwrap_or(Binding::Unknown)
                }
                Binding::Several(_) => Binding::Unknown,
                other => other,
            };
        }
        binding
    }

    fn anonymous_field(&self, class: Target<'a>, rest: &[&str]) -> Option<Binding<'a>> {
        let (last, inner) = rest.split_last()?;
        if inner.is_empty() {
            return None;
        }
        let path = std::iter::once(class.name)
            .chain(inner.iter().copied())
            .collect::<Vec<_>>()
            .join(".");
        let facts: &'a Facts = self.files.get(class.path)?;
        let found = (facts.env_fields.iter()).find(|e| e.class == path && e.field == *last)?;
        Some(Binding::Local(class.path, &found.class, &found.field))
    }

    fn qualified_type(
        &self,
        path: &'a str,
        callee: &'a str,
        seen: &mut Seen<'a>,
    ) -> Option<(Binding<'a>, &'a str)> {
        for at in std::iter::once(callee.len()).chain(callee.rmatch_indices('.').map(|(at, _)| at))
        {
            let source = &callee[..at];
            let Some(module) = self.module(path, source) else {
                continue;
            };
            let found = self.export(module, source, seen);
            if matches!(found, Binding::Def(t) if self.is_class(t))
                || matches!(&found, Binding::Several(all) if all.iter().all(|t| self.is_class(*t)))
            {
                return Some((found, callee.get(at + 1..).unwrap_or("")));
            }
            if seen.cut {
                return None;
            }
        }
        None
    }

    fn module_member(
        &self,
        module: Target<'a>,
        member: &'a str,
        value: bool,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let (path, dotted) = (module.path, format!("{}.{member}", module.name));
        if let Some((exported, _)) = self.named[path].names.get_key_value(dotted.as_str()) {
            return self.export_as(path, exported, seen, value);
        }
        let imports = &self.files[path].imports;
        let at = self.by_local[path].of(module.name).iter().rev();
        let namespace = at
            .map(|&at| &imports[at])
            .find(|i| matches!(i.name, Imported::Namespace));
        match namespace {
            Some(i) => match self.import(path, "", &i.source, &i.name, seen) {
                Binding::Module(m) => self.export_as(m, member, seen, value),
                _ => Binding::Unknown,
            },
            None => Binding::Unknown,
        }
    }

    fn is_class(&self, t: Target) -> bool {
        self.defs[t.path][t.name].kind == Kind::Class
    }

    fn holder(&self, binding: &Binding<'a>) -> Option<(&'a str, &'a str, &'a str)> {
        Some(match *binding {
            Binding::Param(f, n, field) => {
                let param = &self.defs[f.path][f.name].params[n];
                let name = match field {
                    Some(field) => param.fields[field].name.as_str(),
                    None => param.name.as_str(),
                };
                (f.path, f.name, name)
            }
            Binding::Local(path, scope, name) => (path, scope, name),
            Binding::Const(path, constant) => match constant.rsplit_once('.') {
                Some((scope, name)) => (path, scope, name),
                None => (path, "", constant),
            },
            _ => return None,
        })
    }

    fn instance(&self, binding: Binding<'a>, work: &mut Work<'a>) -> Binding<'a> {
        let Some((path, scope, name)) = self.holder(&binding) else {
            return binding;
        };
        let Some(class) = self.receiver_class(path, scope, name) else {
            return Binding::Unknown;
        };
        if let Class::Imported = class {
            return self.imported(path, name, &mut self.seen(path, scope));
        }
        if !self.enter(Busy::Local(path, scope, name), work) {
            return Binding::Unknown;
        }
        let found = match class {
            Class::Enclosing => self.class(path, scope),
            Class::Named(class) if self.ts => self.ts_construction(path, scope, name, class, work),
            Class::Named(class) => {
                self.resolve(path, scope, class, &mut self.seen(path, scope), work)
            }
            Class::Type(class) => {
                let (seen, reading) = (&mut self.seen(path, scope), Reading::Path);
                self.resolve_as(path, scope, class, seen, work, reading)
            }
            Class::Returned(callee) => self.returned(path, scope, callee, false, work),
            Class::Called(callee) => self.returned(path, scope, callee, true, work),
            Class::Result(index) => {
                let awaited = self.ts
                    && self.files[path]
                        .calls
                        .get(*index)
                        .is_some_and(|c| c.awaited);
                self.result_of(
                    path,
                    *index,
                    Through {
                        awaited,
                        ..Through::default()
                    },
                    work,
                )
            }
            Class::TsGlobalValue
            | Class::TsGlobalType(_)
            | Class::TsGeneric(_)
            | Class::Imported
            | Class::Member(_)
            | Class::Unknown => Binding::Unknown,
        };
        self.leave(work);
        match found {
            Binding::Def(t) if self.is_class(t) => Binding::Instance(t),
            external @ Binding::External(..) => external,
            _ => Binding::Unknown,
        }
    }

    fn returned(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        value: bool,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        let (seen, path_reading) = (&mut self.seen(path, scope), Reading::Path);
        let by_path = callee.rsplit_once('.').filter(|_| !value);
        let reading = if by_path.is_some() {
            path_reading
        } else if value && self.members_in_scope && !callee.contains('.') {
            Reading::Method
        } else {
            Reading::Value
        };
        let called = self.resolve_as(path, scope, callee, seen, work, reading);
        match called {
            Binding::Def(t) if self.is_class(t) => Binding::Def(t),
            Binding::Def(t) => match &self.defs[t.path][t.name].result {
                Some(Class::Type(class)) if class == "Self" => {
                    let Some((ty, _)) = by_path else {
                        return Binding::Unknown;
                    };
                    let seen = &mut self.seen(path, scope);
                    match self.resolve_as(path, scope, ty, seen, work, path_reading) {
                        Binding::Def(c) if self.is_class(c) => Binding::Def(c),
                        _ => Binding::Unknown,
                    }
                }
                Some(Class::Type(class)) => {
                    let seen = &mut self.seen(t.path, t.name);
                    self.resolve_as(t.path, t.name, class, seen, work, path_reading)
                }
                _ => Binding::Unknown,
            },
            external @ Binding::External(..) => external,
            _ => Binding::Unknown,
        }
    }

    fn member(
        &self,
        class: Target<'a>,
        member: &'a str,
        read: bool,
        labels: Option<&[String]>,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        let Some(family) = self.family(class, work) else {
            let qualified = format!("{}.{member}", class.name);
            return match self.defs[class.path].get_key_value(qualified.as_str()) {
                Some((name, def)) if def.kind == Kind::Class => Binding::Def(Target {
                    path: class.path,
                    name,
                }),
                _ => Binding::Unknown,
            };
        };
        if !read && let Some(found) = self.overloaded(&family, member, labels) {
            return found;
        }
        for ancestor in self.with_parts(&family).iter() {
            let c = match ancestor {
                Ancestor::Class(c) => *c,
                Ancestor::Opaque(o) if !self.may_declare(o, member, labels) => continue,
                Ancestor::Opaque(_) => return Binding::Unknown,
            };
            if let Some(found) = self.block_member(c, member) {
                return found;
            }
            let qualified = format!("{}.{member}", c.name);
            let found = match self.defs[c.path].get_key_value(qualified.as_str()) {
                Some((name, _)) if read => Binding::Def(Target { path: c.path, name }),
                _ if read => Binding::Unknown,
                _ => match self.def(c.path, &qualified) {
                    Binding::Def(t) if self.extends(t) => Binding::Unknown,
                    Binding::Several(all) if all.iter().all(|t| self.extends(*t)) => {
                        Binding::Unknown
                    }
                    found => found,
                },
            };
            if !matches!(found, Binding::Unknown) {
                return found;
            }
            if let Some(found) = self.sibling_method(c, &qualified).filter(|_| !read) {
                return found;
            }
            if let Some(constant) = self.consts[c.path].get(qualified.as_str()) {
                return Binding::Const(c.path, constant);
            }
            if self.receivers[c.path].contains_key(&(c.name, member)) {
                return Binding::Local(c.path, c.name, member);
            }
            if self.ts && self.ts_field(c, member) {
                return Binding::Unknown;
            }
        }
        match self.companion(class, work) {
            Some(companion) => self.member(companion, member, read, labels, work),
            None => Binding::Unknown,
        }
    }

    fn overloaded(
        &self,
        family: &[Ancestor<'a>],
        member: &str,
        labels: Option<&[String]>,
    ) -> Option<Binding<'a>> {
        let (mut found, mut signatures, mut opaque) = (Vec::new(), HashSet::new(), false);
        let blocks = |c: Target<'a>| self.impls.get(&c).into_iter().flatten();
        let (mut levels, mut conformed) = (Vec::new(), Vec::new());
        for ancestor in self.with_parts(family).iter() {
            let c = match ancestor {
                Ancestor::Class(c) => *c,
                Ancestor::Opaque(o) => {
                    opaque |= self.may_declare(o, member, labels);
                    continue;
                }
            };
            opaque |= self.actual_adds(c, member);
            let mut scopes = vec![(c.path, c.name)];
            for b in blocks(c) {
                scopes.push((b.path, b.scope));
                conformed.extend(b.of);
                opaque |=
                    (b.outside.as_deref()).is_some_and(|o| self.may_declare(o, member, labels));
            }
            levels.push((c, scopes));
        }
        for p in conformed {
            let inherent = blocks(p).filter(|pb| !pb.of_trait);
            let scopes =
                std::iter::once((p.path, p.name)).chain(inherent.map(|b| (b.path, b.scope)));
            levels.push((p, scopes.collect()));
        }
        for (class, scopes) in levels {
            let protocol = self.files[class.path]
                .interfaces
                .iter()
                .any(|i| i == class.name);
            let mut level: Vec<(&str, usize, Target<'a>)> = Vec::new();
            for (at, (path, scope)) in scopes.into_iter().enumerate() {
                if self.overloads[path].is_empty() {
                    continue;
                }
                let qualified = format!("{scope}.{member}");
                if self.defs[path].contains_key(qualified.as_str()) {
                    continue;
                }
                let Some(all) = self.overloads[path].get(qualified.as_str()) else {
                    continue;
                };
                for t in all {
                    let signature = t.name.strip_prefix(scope).unwrap_or(t.name);
                    let required = || level.iter().any(|(s, a, _)| *s == signature && *a == 0);
                    if self.extends(*t)
                        || signatures.contains(signature)
                        || (protocol && at > 0 && required())
                    {
                        continue;
                    }
                    level.push((signature, at, *t));
                }
            }
            for (signature, _, t) in level {
                signatures.insert(signature);
                found.push(t);
            }
        }
        found.sort();
        found.dedup();
        match found[..] {
            [] => None,
            _ if opaque => Some(Binding::Unknown),
            [one] => Some(Binding::Def(one)),
            _ => Some(Binding::Several(found)),
        }
    }

    fn base_member(
        &self,
        path: &'a str,
        scope: &str,
        member: &'a str,
        work: &mut Work<'a>,
        labels: Option<&[String]>,
    ) -> Binding<'a> {
        let Binding::Def(class) = self.class(path, scope) else {
            return Binding::Unknown;
        };
        let family = self.family(class, work).unwrap_or_default();
        if self.ts {
            return match family.get(1) {
                Some(Ancestor::Class(base)) => self.member(*base, member, false, labels, work),
                _ => Binding::Unknown,
            };
        }
        (family.get(1..))
            .and_then(|bases| self.overloaded(bases, member, labels))
            .unwrap_or(Binding::Unknown)
    }

    fn inherited_field(&self, path: &'a str, class: &'a str, name: &'a str) -> Option<Binding<'a>> {
        let Some(family) = self.families.get(&Target { path, name: class })? else {
            return Some(Binding::Unknown);
        };
        for ancestor in self.with_parts(family).iter() {
            let c = match ancestor {
                Ancestor::Class(c) => *c,
                Ancestor::Opaque(o) if !self.may_declare(o, name, None) => continue,
                Ancestor::Opaque(_) => return Some(Binding::Unknown),
            };
            let qualified = format!("{}.{name}", c.name);
            if let Some(constant) = self.consts[c.path].get(qualified.as_str()) {
                return Some(Binding::Const(c.path, constant));
            }
            match self.receivers[c.path].get(&(c.name, name)) {
                Some(Class::Imported) => return None,
                Some(_) => return Some(Binding::Local(c.path, c.name, name)),
                None => {}
            }
        }
        let companion = self.companion(Target { path, name: class }, &mut Work::default())?;
        (self.inherited_field(companion.path, companion.name, name))
            .filter(|found| !matches!(found, Binding::Unknown))
    }

    fn opaque(&self, family: &[Ancestor<'a>], name: &str) -> bool {
        let name = name.rsplit('.').next().unwrap_or(name);
        family.iter().any(|a| match a {
            Ancestor::Opaque(o) => self.may_declare(o, name, None),
            Ancestor::Class(c) => (self.impls.get(c).into_iter().flatten())
                .any(|b| (b.outside.as_deref()).is_some_and(|o| self.may_declare(o, name, None))),
        })
    }

    fn outer_opaque(&self, path: &'a str, scope: &str, name: &str) -> bool {
        let Binding::Def(class) = self.class(path, scope) else {
            return false;
        };
        let mut at = crate::parent(class.name);
        while !at.is_empty() {
            if let Some((name_at, def)) = self.defs[path].get_key_value(at)
                && def.kind == Kind::Class
                && let Some(Some(family)) = self.families.get(&Target {
                    path,
                    name: name_at,
                })
                && self.opaque(family, name)
            {
                return true;
            }
            at = crate::parent(at);
        }
        false
    }

    fn may_declare(&self, outside: &str, name: &str, labels: Option<&[String]>) -> bool {
        let base = |n: &'static str| n.split('(').next().unwrap_or(n);
        let name = name.split('(').next().unwrap_or(name);
        let full = (labels.filter(|l| !l.iter().any(String::is_empty))).map(|l| {
            format!(
                "{name}({})",
                l.iter().map(|l| format!("{l}:")).collect::<String>()
            )
        });
        let known = || {
            (self.known.iter()).filter(|(ty, _)| {
                *ty == outside || (!outside.contains('.') && ty.rsplit('.').next() == Some(outside))
            })
        };
        known().next().is_none()
            || known().any(|(_, members)| {
                members.iter().any(|m| {
                    base(m) == name && full.as_ref().is_none_or(|f| !m.contains('(') || m == f)
                })
            })
    }

    fn inherited(&self, path: &'a str, class: &'a str, name: &str) -> Option<Binding<'a>> {
        let family = self.families.get(&Target { path, name: class })?;
        let Some(family) = family else {
            return Some(Binding::Unknown);
        };
        match self.overloaded(family, name, None) {
            None if self.implicit.contains(&name) || self.opaque(family, name) => {
                Some(Binding::Unknown)
            }
            None => {
                let class = Target { path, name: class };
                let companion = self.companion(class, &mut Work::default())?;
                self.inherited(companion.path, companion.name, name)
            }
            found => found,
        }
    }

    fn companion(&self, class: Target<'a>, work: &mut Work<'a>) -> Option<Target<'a>> {
        let defs = &self.defs[class.path];
        let nested = |b: &String| {
            b.rsplit_once('.').is_some_and(|(_, last)| {
                defs.contains_key(format!("{}.{last}", class.name).as_str())
            })
        };
        if !defs[class.name].bases.iter().any(nested) {
            return None;
        }
        (self.bases(class, work).into_iter()).find_map(|b| match b {
            Ancestor::Class(b) if nested_in(b, class) => Some(b),
            _ => None,
        })
    }

    fn extends(&self, t: Target<'a>) -> bool {
        let name = t.name.split('(').next().unwrap_or(t.name);
        let member = name.rsplit('.').next().unwrap_or(name);
        (self.extensions.get(member)).is_some_and(|all| all.iter().any(|e| e.target == t))
    }

    fn sibling_method(&self, class: Target<'a>, qualified: &str) -> Option<Binding<'a>> {
        let group = &self.groups[*self.shared.get(class.path)?];
        let (&name, files) = group.methods.get_key_value(qualified)?;
        let path = *files.iter().find(|f| **f != class.path)?;
        Some(Binding::Def(Target { path, name }))
    }

    fn parts_of(&self, class: Target<'a>) -> Option<&Vec<&'a str>> {
        let unit = self.files.get(class.path)?.unit.as_str();
        self.parts.get(&(unit, class.name))
    }

    fn parts(&self, class: Target<'a>) -> Vec<Target<'a>> {
        let others = self.parts_of(class).into_iter().flatten();
        let others = others
            .filter(|&&path| path != class.path)
            .map(|&path| Target {
                path,
                name: class.name,
            });
        std::iter::once(class).chain(others).collect()
    }

    fn with_parts<'f>(&self, family: &'f [Ancestor<'a>]) -> std::borrow::Cow<'f, [Ancestor<'a>]> {
        if self.parts.is_empty() {
            return std::borrow::Cow::Borrowed(family);
        }
        let parts = family.iter().flat_map(|ancestor| match ancestor {
            Ancestor::Class(c) => self.parts(*c).into_iter().map(Ancestor::Class).collect(),
            opaque => vec![opaque.clone()],
        });
        std::borrow::Cow::Owned(parts.collect())
    }

    fn one_type(&self, all: &[Target<'a>]) -> Option<Target<'a>> {
        let first = *all.iter().min()?;
        if let Some(paths) = self.parts_of(first) {
            return (all.iter())
                .all(|t| t.name == first.name && paths.contains(&t.path))
                .then_some(first);
        }
        let mut expect = all.iter().filter(|t| self.is_expect(**t));
        let (Some(&expect), None) = (expect.next(), expect.next()) else {
            return None;
        };
        let actual = |t: &Target| {
            self.actuals
                .get(t.name)
                .is_some_and(|a| a.contains(&t.path))
        };
        (all.iter())
            .all(|t| *t == expect || (t.name == expect.name && actual(t)))
            .then_some(expect)
    }

    fn actual_adds(&self, c: Target<'a>, member: &str) -> bool {
        if !self.is_expect(c) {
            return false;
        }
        let qualified = format!("{}.{member}", c.name);
        let signatures = |path: &'a str| {
            (self.overloads.get(path))
                .and_then(|o| o.get(qualified.as_str()))
                .into_iter()
                .flatten()
                .map(|t| t.name)
        };
        let own: HashSet<&str> = signatures(c.path).collect();
        (self.actuals.get(c.name).into_iter().flatten())
            .any(|&path| signatures(path).any(|s| !own.contains(s)))
    }

    fn is_expect(&self, t: Target<'a>) -> bool {
        (self.files.get(t.path)).is_some_and(|f| f.expect.iter().any(|n| n == t.name))
    }

    fn block_member(&self, class: Target<'a>, member: &str) -> Option<Binding<'a>> {
        let blocks = self.impls.get(&class)?;
        let find = |path: &'a str, scope: &str| {
            let qualified = format!("{scope}.{member}");
            let (name, _) = self.defs[path].get_key_value(qualified.as_str())?;
            Some(Target { path, name })
        };
        let one = |found: BTreeSet<Target<'a>>| match found.into_iter().collect::<Vec<_>>()[..] {
            [] => None,
            [one] => Some(Binding::Def(one)),
            ref several => Some(Binding::Several(several.to_vec())),
        };
        let inherent = blocks.iter().filter(|b| !b.of_trait);
        let found = one(inherent.filter_map(|b| find(b.path, b.scope)).collect());
        if found.is_some() {
            return found;
        }
        let traits = blocks.iter().filter(|b| b.of_trait);
        one(traits
            .filter_map(|b| find(b.path, b.scope).or_else(|| find(b.of?.path, b.of?.name)))
            .collect())
    }

    fn family(&self, class: Target<'a>, work: &mut Work<'a>) -> Family<'a> {
        let first = self.linearizing.borrow().is_empty();
        if first {
            self.cut.set(false);
        }
        self.lineage(class, work).unwrap_or_else(|Cut| {
            if first && !self.cut.get() {
                work.families.insert(class, None);
            } else {
                self.cut.set(true);
            }
            None
        })
    }

    fn lineage(&self, class: Target<'a>, work: &mut Work<'a>) -> Result<Family<'a>, Cut> {
        if let Some(family) = self.families.get(&class).or(work.families.get(&class)) {
            return Ok(family.clone());
        }
        if (self.linearizing.borrow().iter()).any(|(p, n)| *p == class.path && *n == class.name) {
            return Ok(None);
        }
        if self.cut.get() || self.linearizing.borrow().len() >= FAMILY_DEPTH {
            return Err(Cut);
        }
        (self.linearizing.borrow_mut()).push((class.path.into(), class.name.into()));
        let mut bases = self.bases(class, work);
        if self.cut.get() {
            self.linearizing.borrow_mut().pop();
            return Err(Cut);
        }
        bases.retain(|b| !matches!(b, Ancestor::Class(b) if nested_in(*b, class)));
        let mut lists = Vec::new();
        for base in &bases {
            lists.push(match base {
                Ancestor::Class(base) => match self.lineage(*base, work) {
                    Ok(family) => family,
                    Err(cut) => {
                        self.linearizing.borrow_mut().pop();
                        return Err(cut);
                    }
                },
                Ancestor::Opaque(_) => Some(vec![base.clone()]),
            });
        }
        if !self.unordered_bases {
            lists.push(Some(bases));
        }
        let family = (lists.into_iter().collect::<Option<Vec<_>>>())
            .and_then(merge)
            .map(|rest| [Ancestor::Class(class)].into_iter().chain(rest).collect());
        self.linearizing.borrow_mut().pop();
        work.families.insert(class, family.clone());
        Ok(family)
    }

    fn bases(&self, class: Target<'a>, work: &mut Work<'a>) -> Vec<Ancestor<'a>> {
        let scope = crate::parent(class.name);
        let mut out = Vec::new();
        for class in self.parts(class) {
            for (n, base) in self.defs[class.path][class.name].bases.iter().enumerate() {
                let seen = &mut self.seen(class.path, scope);
                let base = match self.resolve(class.path, scope, base, seen, work) {
                    Binding::Def(t) if self.is_class(t) => Ancestor::Class(t),
                    _ if base.starts_with('(') => {
                        Ancestor::Opaque(format!("{}::{}#{n}", class.path, class.name))
                    }
                    _ => Ancestor::Opaque(self.imported_name(class.path, scope, base)),
                };
                if !out.contains(&base) {
                    out.push(base);
                }
            }
        }
        out
    }

    fn imported_name(&self, path: &str, scope: &str, name: &str) -> String {
        let imports = &self.files[path].imports;
        let mut name = name.to_string();
        for _ in 0..LINK_DEPTH {
            let (head, rest) = name.split_at(name.find('.').unwrap_or(name.len()));
            let mut at = Some(scope);
            let target = loop {
                let Some(s) = at else { break None };
                let bound = self.by_local[path].of(&crate::qualify(s, head));
                let found = bound.iter().rev().find_map(|&at| match &imports[at].name {
                    Imported::Path(t) => Some(t),
                    _ => None,
                });
                if found.is_some() {
                    break found;
                }
                at = (!s.is_empty()).then(|| crate::parent(s));
            };
            let Some(target) = target else { break };
            let next = format!("{target}{rest}");
            if next.split('.').next() == Some(head) {
                return next;
            }
            name = next;
        }
        let (head, rest) = name.split_at(name.find('.').unwrap_or(name.len()));
        let mut at = Some(scope);
        let latest = loop {
            let Some(s) = at else { break None };
            if let Some(found) = self.by_local[path].of(&crate::qualify(s, head)).last() {
                break Some(found);
            }
            at = (!s.is_empty()).then(|| crate::parent(s));
        };
        match latest.map(|&at| (&imports[at], &imports[at].name)) {
            Some((i, Imported::Namespace | Imported::Qualified)) => format!("{}{rest}", i.source),
            Some((i, Imported::Name(n) | Imported::Member { name: n, .. })) => {
                format!("{}.{n}{rest}", i.source)
            }
            _ => name,
        }
    }

    fn infer(
        &self,
        path: &str,
        callee: &str,
        by_path: bool,
        args: Option<Args>,
        bare: bool,
    ) -> Resolution<'a> {
        one_of(self.candidates(path, callee, by_path, args, bare, &|| false))
    }

    fn candidates(
        &self,
        path: &str,
        callee: &str,
        by_path: bool,
        args: Option<Args>,
        bare: bool,
        on_receiver: &dyn Fn() -> bool,
    ) -> Vec<Target<'a>> {
        let (member, last) = match callee.rsplit_once('.') {
            Some((_, last)) => (true, last),
            None => (bare, callee),
        };
        let found = |member| (self.by_name.get(&(member, last))).map_or(&[][..], Vec::as_slice);
        let mut found = found(member).to_vec();
        if member && by_path && !bare {
            found.extend(self.by_name.get(&(false, last)).into_iter().flatten());
            found.sort();
        }
        if callee.starts_with(crate::lang::IMPLICIT_HEAD) {
            found.retain(|t| last != "init" && self.defs[t.path][t.name].kind == Kind::Method);
        }
        if let Some(reaches) = &self.reaches {
            found.retain(|t| reaches(path, t.path));
        }
        if self.files.get(path).is_some_and(|f| !f.unit.is_empty()) {
            found.retain(|t| !self.hidden(path, *t));
        }
        if let Some(args) = args {
            found.retain(|t| {
                (self.arities.get(t)).is_none_or(|n| n.fits_call(args, on_receiver) != Fit::No)
            });
        }
        if !self.extending.contains(path) {
            found.retain(|t| !self.extends(*t));
        }
        found
    }

    fn def(&self, path: &'a str, name: &str) -> Binding<'a> {
        match self.defs[path].get_key_value(name) {
            Some((name, _)) => Binding::Def(Target { path, name }),
            None => match self.overloads[path].get(name).map(Vec::as_slice) {
                Some([one]) => Binding::Def(*one),
                Some(several) if !several.is_empty() => Binding::Several(several.to_vec()),
                _ => Binding::Unknown,
            },
        }
    }

    fn class(&self, path: &'a str, scope: &str) -> Binding<'a> {
        let defs = &self.defs[path];
        let mut scope = scope;
        while !scope.is_empty() {
            if let Some((name, def)) = defs.get_key_value(scope)
                && def.kind == Kind::Class
            {
                return Binding::Def(Target { path, name });
            }
            if let Some(class) = self.blocks.get(&(path, scope)) {
                return Binding::Def(*class);
            }
            scope = crate::parent(scope);
        }
        Binding::Unknown
    }

    fn seen(&self, path: &'a str, scope: &str) -> Seen<'a> {
        let links = self.links.get();
        let mut s = scope;
        while !s.is_empty() {
            let program = self.files[path].programs.iter().any(|p| p == s);
            if !program
                && !self.defs[path]
                    .get(s)
                    .is_some_and(|d| d.kind == Kind::Class)
            {
                return Seen {
                    depth: links,
                    from: links,
                    ..Seen::new()
                };
            }
            s = crate::parent(s);
        }
        Seen {
            load: Some(path),
            depth: links,
            from: links,
            ..Seen::new()
        }
    }

    fn cut_link(&self, seen: &mut Seen<'a>) {
        seen.cut = true;
        if seen.from > 0 {
            self.cut.set(true);
        }
    }

    fn enter(&self, key: Busy<'a>, work: &mut Work<'a>) -> bool {
        if self.chain.get() >= RECEIVER_DEPTH {
            self.cut.set(true);
            return false;
        }
        if work.busy.contains(&key) {
            return false;
        }
        work.busy.push(key);
        self.chain.set(self.chain.get() + 1);
        true
    }

    fn leave(&self, work: &mut Work<'a>) {
        work.busy.pop();
        self.chain.set(self.chain.get() - 1);
    }

    fn past<R>(&self, seen: &mut Seen<'a>, lookup: impl FnOnce(&mut Seen<'a>) -> R) -> R {
        let outer = self.links.replace(seen.depth);
        let found = lookup(seen);
        self.links.set(outer);
        found
    }

    fn lexical(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        self.lexical_as(path, scope, name, seen, Reading::Any)
    }

    fn lexical_as(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        reading: Reading,
    ) -> Binding<'a> {
        let defs = &self.defs[path];
        let start = scope;
        let (mut locals, mut functions) = (None, None);
        let mut scope = Some(scope);
        let (mut first, mut top) = (true, false);
        while let Some(s) = scope {
            top |= s.is_empty();
            let module = defs.get(s).is_some_and(|d| d.kind == Kind::Module);
            scope = (!s.is_empty() && !module).then(|| crate::parent(s));
            let class_body = !first && defs.get(s).is_some_and(|d| d.kind == Kind::Class);
            first = false;
            if class_body && !self.members_in_scope {
                continue;
            }
            let candidate = if s.is_empty() {
                name.to_string()
            } else {
                format!("{s}.{name}")
            };
            if reading.types() {
                match defs.get_key_value(candidate.as_str()) {
                    Some((name, def))
                        if def.kind != Kind::Method
                            && (reading == Reading::Alias || def.kind != Kind::Function) =>
                    {
                        return Binding::Def(Target { path, name });
                    }
                    _ => {
                        if reading == Reading::Method
                            && defs.get(s).is_some_and(|d| d.kind == Kind::Class)
                            && let Some(found) = self.inherited(path, s, name)
                        {
                            return found;
                        }
                        if reading == Reading::Method
                            && (self.blocks.contains_key(&(path, s))
                                || self.unread.contains(&(path, s)))
                            && let Some(found) = self.local_function(
                                path,
                                s,
                                name,
                                functions.get_or_insert_with(|| {
                                    let binders = self.binders.get(path);
                                    self.around(
                                        path,
                                        start,
                                        binders.and_then(|b| b.functions.get(name)),
                                    )
                                }),
                            )
                        {
                            return found;
                        }
                        if reading == Reading::Method {
                            if let Some(class) = self.blocks.get(&(path, s)) {
                                if let Some(found) = self.inherited(class.path, class.name, name) {
                                    return found;
                                }
                            } else if self.unread.contains(&(path, s)) {
                                return Binding::Unknown;
                            }
                        }
                        if reading != Reading::Type
                            && let Some(found) = self.family_at(path, s, &candidate)
                        {
                            return found;
                        }
                        if let Some(found) = self.imported_at(path, s, name, seen) {
                            let method = matches!(found, Binding::Def(t) if self.defs[t.path][t.name].kind == Kind::Method)
                                || matches!(&found, Binding::Several(all) if all.iter().any(|t| self.defs[t.path][t.name].kind == Kind::Method));
                            if reading != Reading::Type || !method {
                                return found;
                            }
                        }
                        continue;
                    }
                }
            }
            if s.is_empty()
                && let Some(found) = self.rebound(path, name, seen)
            {
                return found;
            }
            let local = self.ts_local(path, s, name);
            if let Some(Some(local)) = local {
                if self
                    .ts_site
                    .get()
                    .is_some_and(|(_, at, _)| at < local.declared)
                {
                    return Binding::Hidden;
                }
                if local.receiver.is_some() {
                    if let Some(Class::Result(_)) = self.receiver_class(path, s, name)
                        && let Some(constant) = self.consts[path].get(candidate.as_str())
                    {
                        return Binding::Const(path, constant);
                    }
                    return Binding::Local(path, s, name);
                }
                if let Some(constant) = self.consts[path].get(candidate.as_str()) {
                    return Binding::Const(path, constant);
                }
                return Binding::Hidden;
            }
            if local.is_none() && self.locals[path].contains(candidate.as_str()) {
                return Binding::Hidden;
            }
            if let Some((name, def)) = defs.get_key_value(candidate.as_str())
                && def.kind != Kind::Method
                && !(def.kind == Kind::Module
                    && matches!(
                        reading,
                        Reading::Value | Reading::Decorator | Reading::Read | Reading::Member
                    ))
            {
                return Binding::Def(Target { path, name });
            }
            let read = reading.read() && self.members_in_scope;
            if !read && let Some(found) = self.family_at(path, s, &candidate) {
                return found;
            }
            if self.members_in_scope
                && reading != Reading::Decorator
                && defs.get(s).is_some_and(|d| d.kind == Kind::Class)
                && let Some(found) = self.inherited_field(path, s, name)
            {
                return found;
            }
            let field = self.ts
                && self.files[path].ts_classes.get(s).is_some_and(|c| {
                    c.fields.iter().any(|f| f == name) || c.constructed.contains_key(name)
                });
            if field {
                continue;
            }
            if local.is_none()
                && let Some(constant) = self.consts[path].get(candidate.as_str())
            {
                return Binding::Const(path, constant);
            }
            if let Some(&(n, field)) = self.params[path].get(&(s, name)) {
                return Binding::Param(Target { path, name: s }, n, field);
            }
            match self.receivers[path].get(&(s, name)) {
                Some(Class::Imported) => {
                    return self
                        .imported_at(path, s, name, seen)
                        .unwrap_or_else(|| self.imported_in(path, name, seen, true));
                }
                Some(_) => return Binding::Local(path, s, name),
                None => {}
            }
            if self.members_in_scope
                && reading.read()
                && !(locals.get_or_insert_with(|| {
                    let binders = self.binders.get(path);
                    self.around(path, start, binders.and_then(|b| b.values.get(name)))
                }))
                .iter()
                .any(|&(block, _)| block == s.len())
            {
                if let Some(class) = self.blocks.get(&(path, s)) {
                    if let Some(found) = self.inherited_field(class.path, class.name, name) {
                        return found;
                    }
                } else if reading == Reading::Member && self.unread.contains(&(path, s)) {
                    return Binding::Hidden;
                }
            }
            if let Some(found) = self.imported_at(path, s, name, seen) {
                return found;
            }
        }
        match self.imported_in(path, name, seen, top) {
            Binding::Unknown if top && !seen.cut => {
                self.sibling(path, name).unwrap_or(Binding::Unknown)
            }
            found => found,
        }
    }

    fn family_at(&self, path: &'a str, scope: &str, candidate: &str) -> Option<Binding<'a>> {
        let group = (self.shared.get(path)).filter(|_| scope.is_empty());
        let found = match group.and_then(|&g| self.groups[g].families.get(candidate)) {
            Some(all) => all,
            None => self.overloads[path].get(candidate)?,
        };
        Some(match found[..] {
            [one] => Binding::Def(one),
            _ => Binding::Several(found.clone()),
        })
    }

    fn around(
        &self,
        path: &'a str,
        scope: &'a str,
        binders: Option<&Vec<&'a str>>,
    ) -> Vec<(usize, &'a str)> {
        let bound: HashSet<usize> = (binders.into_iter().flatten())
            .filter(|b| scope.starts_with(**b))
            .map(|b| b.len())
            .collect();
        if bound.is_empty() {
            return Vec::new();
        }
        let defs = &self.defs[path];
        let mut chain = vec![scope];
        while let Some(&s) = chain.last().filter(|s| !s.is_empty()) {
            chain.push(crate::parent(s));
        }
        let (mut found, mut out) = (None, Vec::new());
        for pair in chain.windows(2).rev() {
            let (block, outer) = (pair[0], pair[1]);
            let function = (defs.get(outer))
                .is_some_and(|d| matches!(d.kind, Kind::Function | Kind::Method | Kind::Inline));
            found = if !function {
                None
            } else if bound.contains(&outer.len()) {
                Some(outer)
            } else {
                found
            };
            if let Some(function) = found {
                out.push((block.len(), function));
            }
        }
        out
    }

    fn local_function(
        &self,
        path: &'a str,
        scope: &str,
        name: &str,
        around: &[(usize, &'a str)],
    ) -> Option<Binding<'a>> {
        let &(_, function) = around.iter().find(|&&(block, _)| block == scope.len())?;
        let found = self.overloads[path].get(crate::qualify(function, name).as_str())?;
        Some(match found[..] {
            [one] => Binding::Def(one),
            _ => Binding::Several(found.clone()),
        })
    }

    fn redeclared(&self, path: &'a str, name: &str) -> bool {
        let marked = format!("{name}.var");
        if self.written.contains(name) && self.consts[path].contains(marked.as_str()) {
            return true;
        }
        let Some(&group) = self.shared.get(path) else {
            return false;
        };
        let tops = &self.groups[group].tops;
        tops.get(name)
            .is_some_and(|files| files.iter().any(|f| *f != path))
    }

    fn sibling(&self, path: &'a str, name: &'a str) -> Option<Binding<'a>> {
        let group = &self.groups[*self.shared.get(path)?];
        let found = (group.tops.get(name)?.iter())
            .filter(|f| **f != path)
            .map(|&f| match self.def(f, name) {
                Binding::Unknown => {
                    let constant = self.consts[f].get(name).copied().unwrap_or(name);
                    Binding::Const(f, constant)
                }
                def => def,
            });
        one(found.collect())
    }

    fn accessible(&self, path: &'a str, found: Binding<'a>) -> Binding<'a> {
        let all = match found {
            Binding::Def(t) if self.hidden(path, t) => return Binding::Unknown,
            Binding::Several(all) => all,
            found => return found,
        };
        let left: Vec<Target<'a>> = (all.iter().copied())
            .filter(|t| !self.hidden(path, *t))
            .collect();
        match left[..] {
            _ if left.len() == all.len() => Binding::Several(all),
            [] => Binding::Unknown,
            [one] => Binding::Def(one),
            _ => Binding::Several(left),
        }
    }

    fn sees_internals(&self, path: &str, module: &str) -> bool {
        let (Some(from), Some(at)) = (self.files.get(path), self.files.get(module)) else {
            return false;
        };
        from.internals.contains(&at.unit)
    }

    fn sealed_from(&self, scope: &str, t: Target) -> bool {
        if !(self.sealed.get(t.path)).is_some_and(|s| s.contains(t.name)) {
            return false;
        }
        let class = match self.defs[t.path][t.name].kind {
            Kind::Class => t.name,
            _ => crate::parent(t.name),
        };
        !(scope == class || scope.starts_with(&format!("{class}.")))
    }

    // ponytail: `InternalsVisibleTo` is not read, so a friend project gets no link.
    // A file sees the `private` names of the units it lists in `Facts::internals`, but for the
    // `confined` ones.
    fn hidden(&self, path: &str, t: Target) -> bool {
        let (Some(from), Some(at)) = (self.files.get(path), self.files.get(t.path)) else {
            return false;
        };
        if from.unit == at.unit || from.unit == path || at.unit == t.path {
            return false;
        }
        let Some(internal) = self.internal.get(t.path) else {
            return false;
        };
        let sees = from.internals.contains(&at.unit);
        let mut name = t.name;
        while !name.is_empty() {
            if internal.contains(name) {
                let confined = self.confined.get(t.path).is_some_and(|c| c.contains(name));
                if !sees || confined {
                    return true;
                }
            }
            name = crate::parent(name);
        }
        false
    }

    fn in_unit(&self, path: &'a str, found: Binding<'a>) -> Binding<'a> {
        let Binding::Several(all) = found else {
            return found;
        };
        let owner = |t: &Target<'a>| {
            let scope = crate::parent(t.name);
            (self.blocks.get(&(t.path, scope)).copied()).unwrap_or(Target {
                path: t.path,
                name: scope,
            })
        };
        let owners: HashSet<Target<'a>> = all.iter().map(owner).collect();
        let methods = (all.iter()).all(|t| self.defs[t.path][t.name].kind == Kind::Method);
        let apart = (owners.iter()).any(|a| owners.iter().any(|b| a.name == b.name && a != b));
        if methods && !apart {
            return Binding::Several(all);
        }
        let unit = self.files.get(path).map_or("", |f| f.unit.as_str());
        let own = |t: &&Target| self.files.get(t.path).is_some_and(|f| f.unit == unit);
        let kept: Vec<Target<'a>> = all.iter().filter(own).copied().collect();
        match kept[..] {
            _ if unit.is_empty() => Binding::Several(all),
            [] => self.in_references(path, all),
            [one] => Binding::Def(one),
            _ => Binding::Several(kept),
        }
    }

    fn in_references(&self, path: &'a str, all: Vec<Target<'a>>) -> Binding<'a> {
        let reached = |t: &&Target| {
            let unit = self.files.get(t.path).map_or("", |f| f.unit.as_str());
            (self.files.get(path)).is_some_and(|f| f.unit_references.iter().any(|r| r == unit))
        };
        let mut referenced = all.iter().filter(reached);
        match (referenced.next(), referenced.next()) {
            (Some(&one), None) => Binding::Def(one),
            _ => Binding::Several(all),
        }
    }

    // ponytail: a star import from outside the repository (`from numpy import *`) is taken to
    // bind nothing, here, in `rebound` and in `own_export`, since what it takes is not known: a
    // name it rebinds stays linked to the import or definition before it (`from _heapq import
    // *` after `def heappush`). Taking it to rebind every name loses about as many right links
    // as wrong ones on the standard library.
    fn imported(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        self.imported_in(path, name, seen, true)
    }

    fn imported_in(
        &self,
        path: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        top: bool,
    ) -> Binding<'a> {
        let (imports, index) = (&self.files[path].imports, &self.by_local[path]);
        let mut at = [index.of(name), index.stars.as_slice()].concat();
        at.sort_unstable();
        at.dedup();
        let given = at.into_iter().rev().map(|at| {
            let import = &imports[at];
            let scoped = matches!(import.name, Imported::Path(_) | Imported::Glob { .. });
            let declared = matches!(import.name, Imported::Namespace)
                && (self.defs[path].get(import.local.as_str()))
                    .is_some_and(|d| d.kind == Kind::Module);
            match import.name {
                _ if import.local == name && !scoped && (top || !declared) => {
                    let scope = scope_of(&import.local);
                    let found = self.import(path, scope, &import.source, &import.name, seen);
                    Some((found, false))
                }
                Imported::All => self.taken(path, &import.source, name, seen),
                _ => None,
            }
        });
        if let Some(found) = latest(given, false) {
            return found;
        }
        if top && let Some(found) = self.globs(path, "", name, false, seen) {
            return found;
        }
        if !self.externs[path].contains(name) {
            return Binding::Unknown;
        }
        match self.module(path, name) {
            Some(module) => Binding::Module(module),
            None => Binding::External(name, &NAMESPACE),
        }
    }

    fn imported_at(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
    ) -> Option<Binding<'a>> {
        let imports = &self.files[path].imports;
        for &at in self.by_local[path]
            .of(&crate::qualify(scope, name))
            .iter()
            .rev()
        {
            let import = &imports[at];
            if let Imported::Path(target) = &import.name
                && !seen.aliases.contains(&(path, target.as_str()))
            {
                return Some(self.import(path, scope, &import.source, &import.name, seen));
            }
            if !scope.is_empty()
                && !matches!(
                    import.name,
                    Imported::Path(_) | Imported::Glob { .. } | Imported::All
                )
            {
                return Some(self.import(path, scope, &import.source, &import.name, seen));
            }
        }
        match scope.is_empty() {
            true => None,
            false => self.globs(path, scope, name, false, seen),
        }
    }

    fn on_demand(&self, path: &'a str, name: &'a str) -> Vec<&'a str> {
        let (imports, seen) = (&self.files[path].imports, &mut self.seen(path, ""));
        let mut sources = Vec::new();
        for &at in self.by_local[path].of(&crate::qualify("", "*")) {
            let Imported::Glob {
                path: target,
                inside,
                ..
            } = &imports[at].name
            else {
                continue;
            };
            if seen.depth >= LINK_DEPTH {
                return Vec::new();
            }
            seen.depth += 1;
            let source = self.past(seen, |seen| {
                let work = &mut Work::default();
                match self.resolve_as(path, "", target, seen, work, Reading::Alias) {
                    Binding::External(source, Imported::Namespace | Imported::Qualified) => {
                        Ok(Some(source))
                    }
                    _ if *inside => Ok(None),
                    other => match self
                        .through_glob(path, other, false, name, work, seen)
                        .of_namespace(self, self.glob_namespace(path, target))
                    {
                        Binding::Unknown => Ok(None),
                        _ => Err(()),
                    },
                }
            });
            let Ok(source) = source else {
                return Vec::new();
            };
            seen.depth -= 1;
            if seen.cut {
                return Vec::new();
            }
            sources.extend(source);
        }
        sources.sort_unstable();
        sources.dedup();
        sources
    }

    fn glob_namespace(&self, path: &'a str, target: &str) -> Option<&'a str> {
        self.namespaced.then(|| self.glob_source(path, target))?
    }

    fn glob_source(&self, path: &'a str, target: &str) -> Option<&'a str> {
        let imports = &self.files[path].imports;
        (self.by_local[path].of(target).iter().rev())
            .map(|&at| &imports[at])
            .find(|i| matches!(i.name, Imported::Namespace))
            .map(|i| i.source.as_str())
    }

    fn is_self_or_ancestor(&self, path: &str, m: &str) -> bool {
        let mut at = path;
        for _ in 0..LINK_DEPTH {
            if at == m {
                return true;
            }
            match self.module(at, "super") {
                Some(up) => at = up,
                None => return false,
            }
        }
        false
    }

    fn through_glob(
        &self,
        path: &str,
        bound: Binding<'a>,
        inside: bool,
        name: &'a str,
        work: &mut Work<'a>,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        match bound {
            Binding::Module(m)
                if inside || self.sees_internals(path, m) || self.is_self_or_ancestor(path, m) =>
            {
                self.export(m, name, seen)
            }
            Binding::Module(m) => self.star(m, name, seen).unwrap_or(Binding::Unknown),
            Binding::Def(t) if inside && self.defs[t.path][t.name].kind == Kind::Module => {
                let member = format!("{}.{name}", t.name);
                match self.def(t.path, &member) {
                    Binding::Unknown => self
                        .imported_at(t.path, t.name, name, seen)
                        .unwrap_or(Binding::Unknown),
                    found => found,
                }
            }
            Binding::Def(t) if self.is_class(t) => {
                let offered = |m: &Target<'a>| {
                    self.statics.contains(m)
                        && (self.static_imports_inherit || declared_in(t.name, m.name))
                };
                match self.member(t, name, false, None, work) {
                    Binding::Def(m) if offered(&m) => Binding::Def(m),
                    Binding::Several(all) => {
                        one(all.into_iter().filter(offered).map(Binding::Def).collect())
                            .unwrap_or(Binding::Unknown)
                    }
                    Binding::Const(path, name) if offered(&Target { path, name }) => {
                        Binding::Const(path, name)
                    }
                    _ => Binding::Unknown,
                }
            }
            Binding::External(source, name) if matches!(name, Imported::Qualified) => {
                Binding::External(source, name)
            }
            _ => Binding::Unknown,
        }
    }

    fn globs(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        public: bool,
        seen: &mut Seen<'a>,
    ) -> Option<Binding<'a>> {
        let imports = &self.files[path].imports;
        let (mut candidates, mut inside_found) = (Vec::new(), Vec::new());
        for &at in self.by_local[path]
            .of(&crate::qualify(scope, "*"))
            .iter()
            .rev()
        {
            let import = &imports[at];
            let Imported::Glob {
                path: target,
                inside,
                public: re_exports,
            } = &import.name
            else {
                continue;
            };
            if public && !re_exports {
                continue;
            }
            if !*inside && !inside_found.is_empty() {
                break;
            }
            if seen.depth >= LINK_DEPTH {
                self.cut_link(seen);
                return Some(Binding::Unknown);
            }
            if !seen.globs.insert((path, at, name)) {
                continue;
            }
            seen.depth += 1;
            let found = self.past(seen, |seen| {
                let work = &mut Work::default();
                let bound = self.resolve_as(path, scope, target, seen, work, Reading::Alias);
                self.through_glob(path, bound, *inside, name, work, seen)
            });
            let found = found.of_namespace(self, self.glob_namespace(path, target));
            let found = self.accessible(path, found);
            seen.depth -= 1;
            if seen.cut {
                return Some(Binding::Unknown);
            }
            if matches!(
                found,
                Binding::Def(_)
                    | Binding::Several(_)
                    | Binding::Const(..)
                    | Binding::External(..)
                    | Binding::Module(_)
            ) {
                match (self.combines_globs, *inside) {
                    (false, _) => return Some(found),
                    (true, true) if inside_found.is_empty() && self.own_glob(path, target) => {
                        return Some(found);
                    }
                    (true, true) => inside_found.push(found),
                    (true, false) => candidates.push(found),
                }
            }
        }
        alternatives(match inside_found.is_empty() {
            true => candidates,
            false => inside_found,
        })
    }

    fn own_glob(&self, path: &'a str, target: &str) -> bool {
        self.glob_source(path, target) == Some(self.files[path].package.as_str())
    }

    fn alias(
        &self,
        path: &'a str,
        scope: &'a str,
        target: &'a str,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        if seen.depth >= LINK_DEPTH {
            self.cut_link(seen);
            return Binding::Unknown;
        }
        if !seen.aliases.insert((path, target)) {
            seen.unsure = true;
            return Binding::Unknown;
        }
        seen.depth += 1;
        let found = self.past(seen, |seen| {
            let work = &mut Work::default();
            self.resolve_as(path, scope, target, seen, work, Reading::Alias)
        });
        seen.depth -= 1;
        if seen.outside.is_none() && matches!(found, Binding::Unknown | Binding::External(..)) {
            seen.outside = Some(target);
        }
        found
    }

    fn dotted(
        &self,
        path: &'a str,
        callee: &'a str,
        seen: &mut Seen<'a>,
    ) -> Option<(Binding<'a>, &'a str)> {
        callee
            .rmatch_indices('.')
            .map(|(at, _)| at)
            .filter(|&at| callee[..at].contains('.'))
            .find_map(|at| match self.imported(path, &callee[..at], seen) {
                Binding::Unknown if !seen.cut => None,
                found => Some((found, &callee[at + 1..])),
            })
    }

    fn import(
        &self,
        path: &'a str,
        scope: &'a str,
        source: &'a str,
        name: &'a Imported,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        match name {
            Imported::Path(target) => return self.alias(path, scope, target, seen),
            Imported::Glob { .. } => return Binding::Unknown,
            _ => {}
        }
        let found = match (name, self.module(path, source)) {
            (_, None) => Binding::External(source, name),
            (Imported::Namespace, Some(module)) => Binding::Module(module),
            (Imported::Qualified, Some(module)) => self.export(module, source, seen),
            (Imported::Name(name) | Imported::Member { name, .. }, Some(module)) => {
                self.export(module, name, seen)
            }
            (Imported::All | Imported::Path(_) | Imported::Glob { .. }, Some(_)) => {
                Binding::Unknown
            }
        };
        if let Imported::Member { module, .. } = name
            && matches!(found, Binding::Unknown | Binding::External(..))
            && !seen.cut
            && let Some(module) = self.module(path, module)
        {
            return Binding::Module(module);
        }
        found
    }

    fn star(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Option<Binding<'a>> {
        let shared = self.shared.get(path).is_some_and(|&group| {
            let group = &self.groups[group];
            group.exporting.names.get(name).is_some_and(|places| {
                places
                    .iter()
                    .any(|&at| self.takers[group.files[at]].takes(name))
            })
        });
        if !self.takers[path].takes(name) && !shared {
            return self.globs(path, "", name, true, seen);
        }
        let outer = std::mem::take(&mut seen.looped);
        let found = self.exports(path, name, seen, false);
        let looped = std::mem::replace(&mut seen.looped, outer);
        match found {
            Some(Binding::Unknown) if looped && !seen.cut => None,
            found => found,
        }
    }

    fn taken(
        &self,
        path: &'a str,
        source: &str,
        name: &'a str,
        seen: &mut Seen<'a>,
    ) -> Option<(Binding<'a>, bool)> {
        let module = self.module(path, source)?;
        let found = self.star(module, name, seen)?;
        Some((found, matches!(self.files[module].star, Star::Computed)))
    }

    fn export(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        self.export_as(path, name, seen, false)
    }

    fn export_as(
        &self,
        path: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        value: bool,
    ) -> Binding<'a> {
        self.exports(path, name, seen, value)
            .unwrap_or(Binding::Unknown)
    }

    fn exports(
        &self,
        path: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        value: bool,
    ) -> Option<Binding<'a>> {
        let Some(&group) = self.shared.get(path) else {
            return self.file_exports(path, name, seen, value);
        };
        let Group {
            files, exporting, ..
        } = &self.groups[group];
        let mut at = [exporting.of(name), exporting.stars.as_slice()].concat();
        at.sort_unstable();
        at.dedup();
        let found = at
            .into_iter()
            .filter_map(|at| self.file_exports(files[at], name, seen, value));
        // The parts of one type the files export are that type: the asking file's own part, else
        match one(found.collect()) {
            Some(Binding::Several(all)) => Some(match self.one_type(&all) {
                Some(first) => {
                    let own = Target {
                        path,
                        name: first.name,
                    };
                    Binding::Def(if all.contains(&own) { own } else { first })
                }
                None => Binding::Several(all),
            }),
            found => found,
        }
    }

    fn file_exports(
        &self,
        path: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        value: bool,
    ) -> Option<Binding<'a>> {
        if seen.depth >= LINK_DEPTH {
            self.cut_link(seen);
            return Some(Binding::Unknown);
        }
        let key = (path, name, value);
        match seen.pairs.get(&key).cloned() {
            Some(None) => {
                seen.looped = true;
                return None;
            }
            Some(Some((found, looped))) => {
                seen.looped |= looped;
                return found;
            }
            None => {}
        }
        seen.pairs.insert(key, None);
        let outer = std::mem::take(&mut seen.looped);
        seen.depth += 1;
        let found = self.own_export(path, name, seen, value);
        seen.depth -= 1;
        seen.pairs.insert(key, Some((found.clone(), seen.looped)));
        seen.looped |= outer;
        found
    }

    fn own_export(
        &self,
        path: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        value: bool,
    ) -> Option<Binding<'a>> {
        let (exports, index) = (&self.files[path].exports, &self.named[path]);
        if self.stars_bind {
            let mut at = [index.of(name), index.stars.as_slice()].concat();
            at.sort_unstable();
            let given = at.into_iter().map(|at| match &exports[at] {
                Export::All { source } => self.taken(path, source, name, seen),
                export => (self.gives(path, export, name, seen, value)).map(|found| (found, false)),
            });
            return latest(given, false);
        }
        let named = || index.of(name).iter().map(|&at| &exports[at]);
        let namespace = |e: &&Export| {
            matches!(
                e,
                Export::From {
                    name: Imported::Namespace,
                    ..
                }
            )
        };
        let own = match value {
            true => named().find(|e| !namespace(e)).or_else(|| named().next()),
            false => named().next(),
        };
        if let Some(export) = own {
            return self.gives(path, export, name, seen, value);
        }
        let mut given = (index.stars.iter())
            .filter_map(|&at| self.gives(path, &exports[at], name, seen, value));
        let Some(first) = given.next() else {
            if self.unordered_globs && self.namespaced {
                return None;
            }
            return self.globs(path, "", name, false, seen);
        };
        match given.all(|other| same(&first, &other)) {
            true => Some(first),
            false => Some(Binding::Unknown),
        }
    }

    fn gives(
        &self,
        path: &'a str,
        export: &'a Export,
        name: &'a str,
        seen: &mut Seen<'a>,
        value: bool,
    ) -> Option<Binding<'a>> {
        match export {
            Export::Local { exported, local } if exported == name => {
                let (scope, bare) = match local.rsplit_once('.') {
                    Some((m, _))
                        if self.defs[path]
                            .get(m)
                            .is_some_and(|d| d.kind == Kind::Module) =>
                    {
                        (m, &local[m.len() + 1..])
                    }
                    _ => ("", local.as_str()),
                };
                let reading = if value { Reading::Value } else { Reading::Any };
                let found = self.lexical_as(path, scope, bare, seen, reading);
                if seen.rows
                    && scope.is_empty()
                    && matches!(found, Binding::Unknown)
                    && self.declared.get(path).is_some_and(|d| d.contains(bare))
                {
                    return Some(Binding::Const(path, bare));
                }
                Some(found)
            }
            Export::From {
                exported,
                source,
                name: imported,
            } if exported == name => Some(self.import(path, "", source, imported, seen)),
            Export::All { source } => self.star(self.module(path, source)?, name, seen),
            _ => None,
        }
    }

    fn rebound(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Option<Binding<'a>> {
        let (stars, after) = self.later.get(path)?;
        let stars = &stars[..*after.get(name)?];
        let given = stars
            .iter()
            .map(|source| self.taken(path, source, name, seen));
        let found = latest(given, true)?;
        // ponytail: where the code sits is not compared with the star import's line, so what a
        // star import rebinds is unknown to all of it.
        Some(match seen.load == Some(path) {
            true => Binding::Unknown,
            false => found,
        })
    }

    pub fn ancestries(&self) -> Vec<Ancestry> {
        if self.overloads.values().all(HashMap::is_empty) {
            return Vec::new();
        }
        let mut rows = Vec::new();
        for (&path, facts) in self.files {
            for def in facts.defs.iter().filter(|d| d.kind == Kind::Class) {
                if facts.hidden.contains(&def.name) {
                    continue;
                }
                let class = Target {
                    path,
                    name: &def.name,
                };
                let ancestors = match self.families.get(&class) {
                    Some(Some(family)) if family.len() < 2 => continue,
                    Some(Some(family)) => Some(
                        (family[1..].iter())
                            .map(|a| match a {
                                Ancestor::Class(t) => {
                                    Some((t.path.to_string(), t.name.to_string()))
                                }
                                Ancestor::Opaque(_) => None,
                            })
                            .collect(),
                    ),
                    _ => None,
                };
                rows.push(Ancestry {
                    path: path.to_string(),
                    class: def.name.clone(),
                    ancestors,
                });
            }
        }
        rows.sort_by(|a, b| (&a.path, &a.class).cmp(&(&b.path, &b.class)));
        rows
    }

    pub fn flexible(&self) -> Vec<Flexible> {
        let mut rows: Vec<_> = (self.arities.iter())
            .filter(|(_, a)| a.most != Some(a.least))
            .map(|(t, a)| Flexible {
                path: t.path.to_string(),
                name: t.name.to_string(),
                least: a.least,
                most: a.most,
            })
            .collect();
        rows.sort_by(|a, b| (&a.path, &a.name).cmp(&(&b.path, &b.name)));
        rows
    }

    pub fn exported(&self) -> Vec<Exported> {
        let mut rows = BTreeSet::new();
        for (&path, facts) in self.files {
            if self.published.as_ref().is_some_and(|p| !p.contains(path)) {
                continue;
            }
            let private: HashSet<&str> = facts.private.iter().map(String::as_str).collect();
            for export in &facts.exports {
                if let Some(row) = self.sourced(path, export) {
                    rows.insert((path, row.0, row.1));
                    continue;
                }
                let found = match export {
                    Export::Local { exported, .. } | Export::From { exported, .. } => {
                        if private.contains(exported.as_str()) && !self.takers[path].takes(exported)
                        {
                            continue;
                        }
                        self.rows(path, exported, private.contains(exported.as_str()))
                    }
                    Export::All { source } => match self.module(path, source) {
                        Some(m) => vec![(None, Some(m), None)],
                        None => continue,
                    },
                };
                rows.extend(found.into_iter().map(|row| (path, row, None)));
            }
            let mut names = BTreeSet::new();
            self.glob_names(path, &mut HashSet::new(), &mut names);
            names.retain(|n| self.named[path].of(n).is_empty());
            let found = names
                .into_iter()
                .flat_map(|name| self.rows(path, name, false));
            rows.extend(found.map(|row| (path, row, None)));
        }
        (rows.into_iter())
            .map(|(path, (name, file, target), source)| Exported {
                path: path.to_string(),
                name: name.map(String::from),
                file: file.map(String::from),
                target: target.map(String::from),
                source: source.map(String::from),
            })
            .collect()
    }

    fn sourced(&self, path: &'a str, export: &'a Export) -> Option<(Row<'a>, Option<&'a str>)> {
        let (_, extension) = path.rsplit_once('.')?;
        if !crate::lang::ts::EXTENSIONS.contains(&extension) {
            return None;
        }
        let (row, source): (Row<'a>, &'a str) = match export {
            Export::From {
                exported,
                source,
                name: Imported::Name(name),
            } => ((Some(exported), None, Some(name)), source),
            Export::All { source } => ((Some("*"), None, None), source),
            _ => return None,
        };
        let bare = !source.starts_with('.') && !source.starts_with('/');
        (bare && self.module(path, source).is_none()).then_some((row, Some(source)))
    }

    fn rows(&self, path: &'a str, exported: &'a str, private: bool) -> Vec<Row<'a>> {
        let (item, any) = (
            self.row(path, exported, true),
            self.row(path, exported, false),
        );
        match (item == any || item.1.is_none(), private) {
            (true, _) => vec![any],
            (false, true) => vec![item],
            (false, false) => vec![item, any],
        }
    }

    fn row(&self, path: &'a str, exported: &'a str, value: bool) -> Row<'a> {
        let mut seen = Seen {
            rows: true,
            ..Seen::new()
        };
        match self.export_as(path, exported, &mut seen, value) {
            Binding::Def(t) => (Some(exported), Some(t.path), Some(t.name)),
            Binding::Module(m) => (Some(exported), Some(m), None),
            Binding::Const(at, c) => (Some(exported), Some(at), Some(c)),
            _ => (Some(exported), None, None),
        }
    }

    fn glob_names(&self, path: &'a str, done: &mut HashSet<&'a str>, out: &mut BTreeSet<&'a str>) {
        if done.len() >= LINK_DEPTH || !done.insert(path) {
            return;
        }
        for import in &self.files[path].imports {
            let Imported::Glob {
                path: target,
                public: true,
                ..
            } = &import.name
            else {
                continue;
            };
            if import.local == "*"
                && let Binding::Module(m) = self.alias(path, "", target, &mut Seen::new())
            {
                let facts = self.files[m];
                let named = facts.exports.iter().filter_map(|e| match e {
                    Export::Local { exported, .. } | Export::From { exported, .. } => {
                        Some(exported.as_str())
                    }
                    Export::All { .. } => None,
                });
                out.extend(named.filter(|n| self.takers[m].takes(n)));
                self.glob_names(m, done, out);
            }
        }
    }

    fn module(&self, path: &str, specifier: &str) -> Option<&'a str> {
        (self.modules)(path, specifier)
            .iter()
            .find_map(|candidate| {
                #[cfg(test)]
                CANDIDATES_VISITED.with(|n| n.set(n.get() + 1));
                self.files.get_key_value(candidate.as_str())
            })
            .map(|(path, _)| *path)
    }
}

fn latest<'a>(
    bindings: impl Iterator<Item = Option<(Binding<'a>, bool)>>,
    before: bool,
) -> Option<Binding<'a>> {
    let mut found = None;
    for (binding, maybe) in bindings.flatten() {
        if found.as_ref().is_some_and(|first| !same(first, &binding)) {
            return Some(Binding::Unknown);
        }
        found = Some(binding);
        if !maybe {
            return found;
        }
    }
    match found {
        Some(_) if before => Some(Binding::Unknown),
        found => found,
    }
}

fn one_of_all(mut found: Vec<Target>) -> Resolution {
    found.sort();
    found.dedup();
    Resolution::Ambiguous(found)
}

fn one_of(found: Vec<Target>) -> Resolution {
    match found[..] {
        [] => Resolution::Unresolved,
        [one] => Resolution::Inferred(one),
        _ => Resolution::Ambiguous(found),
    }
}

impl<'a> Binding<'a> {
    fn of_namespace(self, resolver: &Resolver<'a>, namespace: Option<&str>) -> Binding<'a> {
        let Some(namespace) = namespace else {
            return self;
        };
        let foreign = |t: &Target| resolver.is_class(*t) && crate::parent(t.name) != namespace;
        match self {
            Binding::Def(t) if foreign(&t) => Binding::Unknown,
            Binding::Several(all) => {
                let kept: Vec<_> = all.into_iter().filter(|t| !foreign(t)).collect();
                match kept[..] {
                    [] => Binding::Unknown,
                    [one] => Binding::Def(one),
                    _ => Binding::Several(kept),
                }
            }
            found => found,
        }
    }
}

fn alternatives(found: Vec<Binding>) -> Option<Binding> {
    one(found
        .into_iter()
        .flat_map(|binding| match binding {
            Binding::Several(all) => all.into_iter().map(Binding::Def).collect(),
            other => vec![other],
        })
        .collect())
}

fn declared_in(owner: &str, member: &str) -> bool {
    let rest = member.strip_prefix(owner).and_then(|r| r.strip_prefix('.'));
    rest.is_some_and(|r| !r.split('(').next().unwrap_or(r).contains('.'))
}

fn one(found: Vec<Binding>) -> Option<Binding> {
    let mut distinct: Vec<Binding> = Vec::new();
    for binding in found {
        if !distinct.iter().any(|d| same(d, &binding)) {
            distinct.push(binding);
        }
    }
    if distinct.len() < 2 {
        return distinct.pop();
    }
    let defs: Option<Vec<Target>> = (distinct.iter())
        .map(|b| match b {
            Binding::Def(t) => Some(*t),
            _ => None,
        })
        .collect();
    Some(match defs {
        Some(mut all) => {
            all.sort();
            Binding::Several(all)
        }
        None => Binding::Unknown,
    })
}

fn same(a: &Binding, b: &Binding) -> bool {
    match (a, b) {
        (Binding::Def(x), Binding::Def(y)) => x == y,
        (Binding::Module(x), Binding::Module(y)) => x == y,
        (Binding::Const(p, x), Binding::Const(q, y)) => (p, x) == (q, y),
        (Binding::External(p, x), Binding::External(q, y)) => {
            p == q
                && match (x, y) {
                    (Imported::Namespace, Imported::Namespace) => true,
                    (Imported::Qualified, Imported::Qualified) => true,
                    (Imported::Name(x), Imported::Name(y)) => x == y,
                    _ => false,
                }
        }
        _ => false,
    }
}

fn merge(mut lists: Vec<Vec<Ancestor>>) -> Option<Vec<Ancestor>> {
    let mut merged = Vec::new();
    loop {
        lists.retain(|l| !l.is_empty());
        if lists.is_empty() {
            return Some(merged);
        }
        let head = (lists.iter().map(|l| &l[0]))
            .find(|h| lists.iter().all(|l| !l[1..].contains(h)))?
            .clone();
        for list in &mut lists {
            if list[0] == head {
                list.remove(0);
            }
        }
        merged.push(head);
        if merged.len() >= FAMILY_DEPTH {
            return None;
        }
    }
}

pub type Keywords = [(String, Value)];

#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    pub path: String,
    pub name: Option<String>,
    pub file: Option<String>,
    pub target: Option<String>,
    pub source: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ancestry {
    pub path: String,
    pub class: String,
    pub ancestors: Option<Vec<Option<(String, String)>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flexible {
    pub path: String,
    pub name: String,
    pub least: usize,
    pub most: Option<usize>,
}

#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub def: Target<'a>,
    pub path: &'a str,
    pub index: usize,
}

#[derive(Clone, Copy)]
pub enum Expand<'a> {
    Call(&'a str, usize),
    Constructed {
        held: (&'a str, &'a str),
        at: (&'a str, usize),
    },
    Chain(&'a str, usize),
}

pub struct Expanded<'a, R> {
    pub site: (&'a str, usize),
    pub frames: Vec<Frame<'a>>,
    pub found: R,
}

type Folded = (Vec<Value>, Vec<(String, Value)>);

struct Written<'a, 'i> {
    path: &'a str,
    scope: &'a str,
    args: &'a [Value],
    kwargs: &'a Keywords,
    at: (&'a str, usize),
    chain: Option<&'a Call>,
    inject: Option<&'i Inject<'a, 'i>>,
    external_value: Option<&'i ExternalValue<'a, 'i>>,
    retain_site: Option<fn(&[Value]) -> bool>,
    writes: Option<CallWrites>,
}

// ponytail: real files spend a few units per call; a crafted one doubling a string through
// forty constants (`const b = a + a`), or a lattice of functions each calling or returning the
// next twice, would cost 2^40.
pub(crate) const EVAL_BUDGET: usize = 1 << 16;

// ponytail: real values nest a few levels; the limit keeps a crafted chain of thousands of
// constants from overflowing the stack.
pub(crate) const EVAL_DEPTH: usize = 256;

const PY_KEEPS_BODY: [&str; 6] = [
    "staticmethod",
    "classmethod",
    "dataclass",
    "dataclasses.dataclass",
    "lru_cache",
    "functools.lru_cache",
];

// ponytail: real clients are wrapped a few levels deep; the limit keeps a crafted chain of
// thousands of wrappers from overflowing the stack.
const WRAPPER_DEPTH: usize = 64;

// ponytail: real clients copy a base through a field or two; the limit keeps a crafted chain of
// fields, each set from the next, from overflowing the stack.
pub(crate) const FIELD_DEPTH: usize = 16;

#[derive(Default)]
struct Fold<'a, 'i> {
    held: Vec<(&'a str, &'a str)>,
    record_site: Option<(&'a str, usize)>,
    nested: usize,
    chain: bool,
    new_types: Option<fn(&str) -> bool>,
    selected: Option<mutations::Selection<'a>>,
    objects: Vec<(mutations::Object<'a>, usize)>,
    inject: Option<&'i Inject<'a, 'i>>,
    external_value: Option<&'i ExternalValue<'a, 'i>>,
    returning: Vec<(&'a str, usize)>,
    independent: Option<Target<'a>>,
    foreign: usize,
    field_frames: Option<&'i [Frame<'a>]>,
    field_wants: Option<Target<'a>>,
    field_depth: usize,
}

pub type Inject<'a, 'i> = dyn Fn(&Code<'a>, &'a str, Injected<'a>) -> Option<Value> + 'i;

pub(crate) type ExternalValue<'a, 'i> =
    dyn Fn(&'a Call, &mut dyn FnMut(usize) -> Value) -> Option<Value> + 'i;

#[derive(Clone, Copy)]
pub enum Injected<'a> {
    Annotated(&'a Annotated),
    Param(Target<'a>, usize),
}

impl<'a> Injected<'a> {
    pub fn annotated(self) -> Option<&'a Annotated> {
        match self {
            Injected::Annotated(a) => Some(a),
            Injected::Param(..) => None,
        }
    }
}

// ponytail: a chain of more builder calls than this is not read to its start; its calls past
// the limit have no origin.
pub const FLUENT_CHAIN: usize = 1024;

#[derive(Clone, Copy)]
struct Tail {
    stop: usize,
    depth: usize,
}

#[derive(PartialEq)]
enum Flow<'a> {
    Const(&'a str, &'a str),
    Param(Target<'a>, usize),
    Returns(Target<'a>),
    Local(&'a str, &'a str, &'a str),
}

struct Trail<'a> {
    frames: Vec<Frame<'a>>,
    busy: Vec<Flow<'a>>,
    out: BTreeMap<(&'a str, usize), BTreeSet<Route<'a>>>,
    unbound: BTreeSet<(Target<'a>, usize)>,
    steps: Vec<&'a str>,
    held: Option<(&'a str, &'a str)>,
    passes: Passes,
}

impl<'a> Trail<'a> {
    fn new(passes: Passes) -> Self {
        Self {
            frames: Vec::new(),
            busy: Vec::new(),
            out: BTreeMap::new(),
            unbound: BTreeSet::new(),
            steps: Vec::new(),
            held: None,
            passes,
        }
    }
}

pub type Passes = fn(origin: &str, method: &str) -> bool;

pub fn passes_nothing(_origin: &str, _method: &str) -> bool {
    false
}

pub type Field<'a> = (&'a str, &'a str, &'a str);

pub fn method(callee: &str) -> &str {
    callee.rsplit('.').next().unwrap_or(callee)
}

pub struct Code<'a> {
    resolver: &'a Resolver<'a>,
    full: &'a BTreeMap<&'a str, &'a Facts>,
    misses: RefCell<BTreeSet<&'a str>>,
    read: RefCell<HashSet<&'a str>>,
    chains: RefCell<HashMap<(&'a str, usize), Option<Tail>>>,
    folded: RefCell<HashMap<&'a str, usize>>,
    folded_apart: RefCell<HashMap<&'a str, usize>>,
    field_reads_apart: RefCell<HashMap<(Field<'a>, bool), Value>>,
    folded_cuts: RefCell<HashMap<&'a str, usize>>,
    field_reads_cuts: RefCell<HashMap<(Field<'a>, bool), Value>>,
    refused: Cell<bool>,
    fold_returned_strings: Cell<bool>,
    follow_request_parameters: Cell<bool>,
    by_name: HashMap<&'a str, OnceCell<ByName<'a>>>,
    literals: OnceCell<HashMap<&'a str, BTreeSet<(&'a str, &'a str)>>>,
    field_reads: RefCell<HashMap<(Field<'a>, bool), Value>>,
    callees: OnceCell<HashSet<&'a str>>,
    matched: OnceCell<(HashSet<Target<'a>>, HashMap<&'a str, usize>)>,
    py_rebinding: OnceCell<[HashSet<Target<'a>>; 2]>,
    mutations: RefCell<HashMap<usize, mutations::Index<'a>>>,
    pack: &'static dyn LanguagePack,
}

#[derive(Default)]
struct ByName<'a> {
    defs: HashMap<&'a str, &'a Def>,
    consts: HashMap<&'a str, Vec<&'a Const>>,
    bound: HashMap<(&'a str, &'a str), Vec<&'a Bound>>,
    first_results: HashMap<(&'a str, &'a str), Vec<&'a FirstResult>>,
}

impl<'a> ByName<'a> {
    fn new(facts: &'a Facts) -> Self {
        let mut by_name = Self::default();
        for def in &facts.defs {
            by_name.defs.entry(def.name.as_str()).or_insert(def);
        }
        for constant in &facts.consts {
            let named = by_name.consts.entry(constant.name.as_str()).or_default();
            named.push(constant);
        }
        for bound in &facts.bound {
            let key = (bound.scope.as_str(), bound.name.as_str());
            by_name.bound.entry(key).or_default().push(bound);
        }
        for first in &facts.first_results {
            let key = (first.scope.as_str(), first.name.as_str());
            by_name.first_results.entry(key).or_default().push(first);
        }
        by_name
    }
}

#[derive(Clone, Copy)]
pub enum Of<'a> {
    Name(&'a str),
    Value(&'a Value),
}

#[derive(Clone, Copy, Debug)]
pub enum Follow {
    Values { passes: Passes },
    Constants,
    Declarations,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Call,
    New,
    Type,
    Name,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Origin<'a> {
    pub call: Option<(&'a str, usize)>,
    pub form: Form,
    pub outside: Option<Outside<'a>>,
    pub routes: Vec<Route<'a>>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Route<'a> {
    pub through: Vec<(&'a str, usize)>,
    pub steps: Vec<&'a str>,
    pub held: Option<(&'a str, &'a str)>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outside<'a> {
    pub imported: Option<(&'a str, String)>,
    pub spelled: String,
    pub direct: bool,
    pub candidates: Vec<String>,
}

impl Outside<'_> {
    pub fn spellings(self) -> Vec<String> {
        match self {
            o if !o.candidates.is_empty() => o.candidates,
            o if o.direct => vec![o.spelled],
            _ => Vec::new(),
        }
    }
}

struct Made<'a> {
    form: Form,
    outside: Outside<'a>,
    steps: Vec<&'a str>,
    held: Option<(&'a str, &'a str)>,
    pending: Option<&'a str>,
}

impl<'a> Code<'a> {
    pub fn new(
        resolver: &'a Resolver<'a>,
        full: &'a BTreeMap<&'a str, &'a Facts>,
        pack: &'static dyn LanguagePack,
    ) -> Self {
        Self {
            resolver,
            full,
            misses: RefCell::new(BTreeSet::new()),
            read: RefCell::new(HashSet::new()),
            chains: RefCell::new(HashMap::new()),
            folded: RefCell::new(HashMap::new()),
            folded_apart: RefCell::new(HashMap::new()),
            field_reads_apart: RefCell::new(HashMap::new()),
            folded_cuts: RefCell::new(HashMap::new()),
            field_reads_cuts: RefCell::new(HashMap::new()),
            refused: Cell::new(false),
            fold_returned_strings: Cell::new(false),
            follow_request_parameters: Cell::new(false),
            by_name: full.keys().map(|&path| (path, OnceCell::new())).collect(),
            literals: OnceCell::new(),
            field_reads: RefCell::new(HashMap::new()),
            callees: OnceCell::new(),
            matched: OnceCell::new(),
            py_rebinding: OnceCell::new(),
            mutations: RefCell::new(HashMap::new()),
            pack,
        }
    }

    pub(crate) fn with_returned_strings<T>(&self, read: impl FnOnce() -> T) -> T {
        let previous = self.fold_returned_strings.replace(true);
        let result = read();
        self.fold_returned_strings.set(previous);
        result
    }

    pub(crate) fn with_request_parameters<T>(&self, checked: bool, read: impl FnOnce() -> T) -> T {
        let previous = self.follow_request_parameters.replace(checked);
        let result = read();
        self.follow_request_parameters.set(previous);
        result
    }

    fn charge(&self, file: &'a str, cost: usize) -> bool {
        let mut folded = self.folded.borrow_mut();
        let spent = folded.entry(file).or_default();
        *spent = spent.saturating_add(cost);
        let within = *spent <= EVAL_BUDGET;
        self.refused.set(self.refused.get() || !within);
        within
    }

    pub(crate) fn apart<T>(&self, work: impl FnOnce() -> T) -> T {
        self.aside(&self.folded_apart, &self.field_reads_apart, work)
    }

    fn cut_apart<T>(&self, work: impl FnOnce() -> T) -> T {
        self.aside(&self.folded_cuts, &self.field_reads_cuts, work)
    }

    fn aside<T>(
        &self,
        folded: &RefCell<HashMap<&'a str, usize>>,
        field_reads: &RefCell<HashMap<(Field<'a>, bool), Value>>,
        work: impl FnOnce() -> T,
    ) -> T {
        let swap = || {
            self.folded.swap(folded);
            self.field_reads.swap(field_reads);
        };
        swap();
        let done = work();
        swap();
        done
    }

    pub(crate) fn spent_apart(&self, file: &'a str) -> usize {
        self.folded_apart
            .borrow()
            .get(file)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn set_spent_apart(&self, file: &'a str, spent: usize) {
        self.folded_apart.borrow_mut().insert(file, spent);
    }

    pub fn pack(&self) -> &'static dyn LanguagePack {
        self.pack
    }

    pub fn origins(
        &self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        follow: Follow,
    ) -> Vec<Origin<'a>> {
        match follow {
            Follow::Values { passes } => self.origins_within(path, scope, of, passes, &[]),
            Follow::Constants => {
                let seen = &mut Vec::new();
                let made = match of {
                    Of::Name(chain) => self.trace(path, scope, chain, seen),
                    Of::Value(value) => self.made_by(path, scope, value, seen),
                };
                let made = made.filter(|m| m.pending.is_none());
                (made.into_iter())
                    .map(|m| Origin {
                        call: None,
                        form: m.form,
                        outside: Some(m.outside),
                        routes: vec![Route {
                            through: Vec::new(),
                            steps: m.steps,
                            held: m.held,
                        }],
                    })
                    .collect()
            }
            Follow::Declarations => {
                let name = match of {
                    Of::Name(name) => name,
                    Of::Value(Value::Ref(name)) => name.as_str(),
                    Of::Value(_) => return Vec::new(),
                };
                let Some((form, outside, steps)) = self.declared(path, scope, name, 0) else {
                    return Vec::new();
                };
                let held = match self.resolver.read_member(path, scope, name) {
                    Binding::Const(at, constant) => Some((at, constant)),
                    _ => None,
                };
                vec![Origin {
                    call: None,
                    form,
                    outside,
                    routes: vec![Route {
                        through: Vec::new(),
                        steps,
                        held,
                    }],
                }]
            }
        }
    }

    fn trace(
        &self,
        path: &'a str,
        scope: &'a str,
        chain: &'a str,
        seen: &mut Vec<(&'a str, &'a str)>,
    ) -> Option<Made<'a>> {
        let mut members = chain.split('.');
        let head = members.next()?;
        let mut made =
            match self
                .resolver
                .lexical(path, scope, head, &mut self.resolver.seen(path, scope))
            {
                Binding::External(package, name) => {
                    let name = match name {
                        Imported::Namespace | Imported::Qualified => String::new(),
                        Imported::Name(name) | Imported::Member { name, .. } => name.clone(),
                        Imported::All | Imported::Path(_) | Imported::Glob { .. } => {
                            return None;
                        }
                    };
                    let spelled = self.resolver.imported_name(path, scope, head);
                    Made {
                        form: Form::Name,
                        outside: Outside {
                            imported: Some((package, name)),
                            spelled,
                            direct: true,
                            candidates: Vec::new(),
                        },
                        steps: Vec::new(),
                        held: None,
                        pending: None,
                    }
                }
                Binding::Const(at, constant) => {
                    if seen.contains(&(at, constant)) || seen.len() >= EVAL_DEPTH {
                        return None;
                    }
                    seen.push((at, constant));
                    let (value, within) = self.held(at, constant)?;
                    let mut made = self.made_by(at, within, value, seen)?;
                    made.held = Some((at, constant));
                    made
                }
                _ => return None,
            };
        for member in members {
            match made.form {
                Form::Name => {
                    let outside = &mut made.outside;
                    if let Some((_, name)) = &mut outside.imported {
                        if !name.is_empty() {
                            name.push('.');
                        }
                        *name += member;
                    }
                    outside.spelled = format!("{}.{member}", outside.spelled);
                }
                _ if made.pending.is_none() => made.pending = Some(member),
                _ => return None,
            }
        }
        Some(made)
    }

    fn made_by(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        seen: &mut Vec<(&'a str, &'a str)>,
    ) -> Option<Made<'a>> {
        match value {
            Value::Await(value)
                if self
                    .pack
                    .extensions()
                    .iter()
                    .any(|e| matches!(*e, "ts" | "cs")) =>
            {
                self.made_by(path, scope, value, seen)
            }
            Value::Ref(name) => self.trace(path, scope, name, seen),
            Value::Call(call) => {
                let mut made = self.trace(path, scope, &call.callee, seen)?;
                match (made.form, made.pending.take()) {
                    (Form::Name, _) => made.form = Form::Call,
                    (_, Some(step)) => made.steps.push(step),
                    (_, None) => return None,
                }
                Some(made)
            }
            Value::New(callee, _) => {
                let mut made = self.trace(path, scope, callee, seen)?;
                if made.form != Form::Name {
                    return None;
                }
                made.form = Form::New;
                Some(made)
            }
            _ => None,
        }
    }

    pub fn imported(
        &self,
        path: &'a str,
        scope: &'a str,
        chain: &'a str,
    ) -> Option<(&'a str, String)> {
        self.imported_as(path, scope, chain, Reading::Any)
    }

    fn imported_as(
        &self,
        path: &'a str,
        scope: &'a str,
        chain: &'a str,
        reading: Reading,
    ) -> Option<(&'a str, String)> {
        let (head, members) = chain
            .split_once('.')
            .map_or((chain, None), |(h, m)| (h, Some(m)));
        let seen = &mut self.resolver.seen(path, scope);
        let Binding::External(specifier, name) =
            self.resolver.lexical_as(path, scope, head, seen, reading)
        else {
            return None;
        };
        let pathed = self.resolver.pathed.contains(path);
        if pathed && matches!(name, Imported::Namespace) {
            let full = self.resolver.imported_name(path, scope, chain);
            let rest = full.strip_prefix(specifier)?;
            let rest = rest
                .strip_prefix('.')
                .or_else(|| rest.is_empty().then_some(rest))?;
            return Some((specifier, rest.to_string()));
        }
        let mut spelled = match name {
            Imported::Namespace | Imported::Qualified => String::new(),
            Imported::Name(name) | Imported::Member { name, .. } => name.clone(),
            Imported::All | Imported::Path(_) | Imported::Glob { .. } => return None,
        };
        if let Some(members) = members {
            if !spelled.is_empty() {
                spelled.push('.');
            }
            spelled += members;
        }
        Some((specifier, spelled))
    }

    pub fn imported_member(
        &self,
        path: &'a str,
        scope: &'a str,
        head: &'a str,
        member: &str,
    ) -> Option<(&'a str, String)> {
        let (specifier, spelled) = self.imported(path, scope, head)?;
        match spelled.is_empty() {
            true => Some((specifier, member.to_string())),
            false => Some((specifier, format!("{spelled}.{member}"))),
        }
    }

    pub fn misses(&self) -> Vec<&'a str> {
        self.misses.borrow().iter().copied().collect()
    }

    pub fn read(&self) -> Vec<&'a str> {
        self.read.borrow().iter().copied().collect()
    }

    pub fn files(&self) -> &'a BTreeMap<&'a str, &'a Facts> {
        self.resolver.files
    }

    pub fn reference(&self, path: &'a str, index: usize) -> Option<Option<(&'a str, &'a str)>> {
        self.resolver.reference(path, index)
    }

    pub fn args(&self, path: &'a str, index: usize) -> Option<&'a [Value]> {
        Some(self.call(path, index)?.args.as_slice())
    }

    pub fn call(&self, path: &'a str, index: usize) -> Option<&'a Call> {
        self.full(path)?.calls.get(index)
    }

    pub fn receiver(&self, path: &'a str, index: usize) -> Option<&'a Value> {
        self.call(path, index)?.receiver.as_ref()
    }

    pub fn local(&self, path: &'a str, scope: &'a str, name: &str) -> Option<&'a Bound> {
        let by_name = self.by_name(path)?;
        let r = self.resolver;
        let mut at = Some(scope);
        while let Some(s) = at {
            let mut bound = (by_name.bound.get(&(s, name)).into_iter().flatten()).copied();
            let held = bound.next();
            let qualified = crate::qualify(s, name);
            let receiver = r.receivers[path].get(&(s, name));
            let others = bound.next().is_some()
                || (r.locals[path].contains(qualified.as_str())
                    && (held.is_none() || receiver.is_some()))
                || r.consts[path].contains(qualified.as_str())
                || r.defs[path]
                    .get(s)
                    .is_some_and(|d| d.params.iter().any(|p| p.name == name))
                || (held.is_none() && receiver.is_some_and(|c| **c == Class::Unknown));
            match (held, others) {
                (Some(found), false) => return Some(found),
                (None, false) if receiver.is_none() => {
                    at = (!s.is_empty()).then(|| crate::parent(s))
                }
                _ => return None,
            }
        }
        None
    }

    pub(crate) fn first_result(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(&'a str, usize)> {
        let Binding::Local(at, within, local) =
            self.resolver.lexical(path, scope, name, &mut Seen::new())
        else {
            return None;
        };
        let [first] = self
            .by_name(at)?
            .first_results
            .get(&(within, local))?
            .as_slice()
        else {
            return None;
        };
        Some((at, self.call_index(at, &first.call)?))
    }

    pub fn bindings(&self, path: &'a str) -> &'a [Bound] {
        self.full(path).map_or(&[], |f| f.bound.as_slice())
    }

    pub fn mentioned(&self, path: &'a str) -> &'a [crate::lang::Mentioned] {
        self.full(path).map_or(&[], |f| f.mentioned.as_slice())
    }

    pub fn private_fields(&self, path: &'a str) -> &'a [String] {
        self.full(path).map_or(&[], |f| f.private_fields.as_slice())
    }

    pub fn field_writes(&self, path: &'a str) -> &'a [crate::lang::FieldWrites] {
        self.full(path).map_or(&[], |f| f.field_writes.as_slice())
    }

    pub fn assigned(&self, path: &'a str) -> &'a [Bound] {
        self.full(path).map_or(&[], |f| f.assigned.as_slice())
    }

    pub fn bounds(&self, path: &'a str, scope: &'a str, name: &str) -> Vec<&'a Bound> {
        let Some(by_name) = self.by_name(path) else {
            return Vec::new();
        };
        (by_name.bound.get(&(scope, name)).into_iter().flatten())
            .copied()
            .collect()
    }

    pub fn returned(&self, path: &'a str, index: usize) -> Vec<(&'a str, &'a str, &'a Value)> {
        let call = &self.resolver.files[path].calls[index];
        let targets = match self.resolver.call(path, call) {
            Resolution::Exact(t) | Resolution::Inferred(t) | Resolution::Narrowed(t) => vec![t],
            Resolution::Ambiguous(ts) => ts,
            Resolution::Unresolved => return Vec::new(),
        };
        let mut out = Vec::new();
        for t in targets {
            let def = self.def(t.path, t.name);
            let values = def.map(|d| d.return_values.iter()).into_iter().flatten();
            out.extend(values.map(|v| (t.path, t.name, v)));
        }
        out
    }

    pub fn coroutine(&self, function: Target<'a>) -> bool {
        self.def(function.path, function.name)
            .is_some_and(|d| d.asynchronous)
    }

    pub fn mentions(&self, path: &'a str) -> Option<&'a BTreeSet<String>> {
        self.full(path)?.mentions.as_ref()
    }

    pub fn rebound(&self, path: &'a str) -> &'a [(String, Value)] {
        self.full(path).map_or(&[], |f| f.rebound.as_slice())
    }

    pub fn mutable(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(&'a str, &'a Value)> {
        let held = &self.full(path)?.mutable;
        let r = self.resolver;
        let mut scope = scope;
        loop {
            let local = crate::qualify(scope, name);
            if let Some(value) = held.get(&local) {
                return Some((scope, value));
            }
            if r.locals[path].contains(local.as_str())
                || r.consts[path].contains(local.as_str())
                || r.receivers[path].contains_key(&(scope, name))
                || r.params[path].contains_key(&(scope, name))
                || scope.is_empty()
            {
                return None;
            }
            scope = crate::parent(scope);
        }
    }

    pub fn call_index(&self, path: &'a str, range: &std::ops::Range<usize>) -> Option<usize> {
        call_at(&self.resolver.files[path].calls, range)
    }

    pub fn binds_everywhere(&self, name: &str) -> bool {
        self.resolver.globals.contains(&name)
    }

    pub fn outside(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        by_path: bool,
    ) -> Option<Outside<'a>> {
        self.outside_read(path, scope, name, by_path, false)
    }

    pub(crate) fn value_bound(&self, path: &'a str, scope: &'a str, name: &'a str) -> bool {
        matches!(
            self.resolver
                .lexical(path, scope, name, &mut self.resolver.seen(path, scope)),
            Binding::Param(..) | Binding::Local(..) | Binding::Const(..) | Binding::Hidden
        )
    }

    pub fn outside_decorator(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<Outside<'a>> {
        self.outside_read(path, scope, name, false, true)
    }

    fn outside_read(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        by_path: bool,
        decorator: bool,
    ) -> Option<Outside<'a>> {
        let r = self.resolver;
        let (reading, head_reading) = match (by_path, decorator) {
            (true, _) => (Reading::Path, Reading::Path),
            (false, true) => (Reading::Decorator, Reading::Decorator),
            (false, false) => (Reading::Value, Reading::Any),
        };
        let resolve = |name| {
            let (seen, work) = (&mut r.seen(path, scope), &mut Work::default());
            let found = r.resolve_as(path, scope, name, seen, work, reading);
            (found, seen.cut)
        };
        let external = |name| matches!(resolve(name).0, Binding::External(..));
        let head = name.split_once('.').map_or(name, |(head, _)| head);
        let on_demand = || -> Vec<String> {
            let spelled = |source: &str| format!("{source}.{name}");
            let sources = match r.unordered_globs && !by_path {
                true => r.on_demand(path, head),
                false => Vec::new(),
            };
            sources.into_iter().map(spelled).collect()
        };
        let candidates = match resolve(name) {
            (Binding::External(_, Imported::Qualified), _)
                if r.by_local[path].of(head).is_empty() =>
            {
                on_demand()
            }
            (Binding::External(source, Imported::Namespace), _)
                if source == head && r.by_local[path].of(head).is_empty() =>
            {
                on_demand()
            }
            (Binding::External(..), _) => Vec::new(),
            (Binding::Unknown, false) => match on_demand() {
                found if found.is_empty() => return None,
                found => {
                    return Some(Outside {
                        imported: None,
                        spelled: name.to_string(),
                        direct: false,
                        candidates: found,
                    });
                }
            },
            _ => return None,
        };
        Some(Outside {
            imported: self.imported_as(path, scope, name, head_reading),
            spelled: r.imported_name(path, scope, name),
            direct: external(head),
            candidates,
        })
    }

    fn outside_written(
        &self,
        path: &'a str,
        scope: &'a str,
        written: &'a str,
    ) -> Option<Outside<'a>> {
        let r = self.resolver;
        let (seen, work) = (&mut r.seen(path, scope), &mut Work::default());
        let found = r.resolve_as(path, scope, written, seen, work, Reading::Path);
        let head = written.split('.').next().unwrap_or(written);
        let Binding::External(package, imported) = found else {
            let candidates = match (found, r.unordered_globs && !seen.cut) {
                (Binding::Unknown, true) => r.on_demand(path, head),
                _ => Vec::new(),
            };
            return (!candidates.is_empty()).then(|| Outside {
                imported: None,
                spelled: written.to_string(),
                direct: false,
                candidates: (candidates.into_iter())
                    .map(|s| format!("{s}.{written}"))
                    .collect(),
            });
        };
        let name = written.rsplit('.').next()?;
        let (spelled, candidates) = match (imported, written.contains('.')) {
            (Imported::Qualified, false) => (package.to_string(), Vec::new()),
            (Imported::Namespace, _)
                if r.unordered_globs && package == head && r.by_local[path].of(head).is_empty() =>
            {
                let found = r.on_demand(path, head).into_iter();
                (
                    written.to_string(),
                    found.map(|s| format!("{s}.{written}")).collect(),
                )
            }
            _ => (format!("{package}.{name}"), Vec::new()),
        };
        Some(Outside {
            imported: Some((package, name.to_string())),
            spelled,
            direct: true,
            candidates,
        })
    }

    fn is_repository_type(&self, path: &'a str, scope: &'a str, written: &'a str) -> bool {
        let r = self.resolver;
        let (seen, work) = (&mut r.seen(path, scope), &mut Work::default());
        matches!(
            r.resolve_as(path, scope, written, seen, work, Reading::Path),
            Binding::Def(_)
        )
    }

    fn declared(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        depth: usize,
    ) -> Option<(Form, Option<Outside<'a>>, Vec<&'a str>)> {
        if depth >= RECEIVER_DEPTH {
            return None;
        }
        let r = self.resolver;
        let binding = r.read_member(path, scope, name);
        let (at, within, held) = r.holder(&binding)?;
        match r.receiver_class(at, within, held)? {
            Class::Type(written) => {
                let outside = self.outside_written(at, within, written);
                if outside.is_none() && !self.is_repository_type(at, within, written) {
                    return None;
                }
                Some((Form::Type, outside, Vec::new()))
            }
            Class::Called(callee) => {
                if callee.contains('.')
                    && let Some(function) = self.outside(at, within, callee, false)
                    && function.direct
                {
                    return Some((Form::Call, Some(function), Vec::new()));
                }
                if let Binding::Def(t) = r.binding(at, within, callee) {
                    let Some(Class::Type(written)) = &r.defs[t.path][t.name].result else {
                        return None;
                    };
                    let outside = self.outside_written(t.path, t.name, written)?;
                    return Some((Form::Call, Some(outside), Vec::new()));
                }
                let (receiver, member) = callee.rsplit_once('.')?;
                let (form, outside, mut steps) = self.declared(at, within, receiver, depth + 1)?;
                steps.push(member);
                Some((form, Some(outside?), steps))
            }
            _ => None,
        }
    }

    pub fn field(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<Field<'a>> {
        let Binding::Local(at, ty, field) = self.resolver.read(path, scope, name) else {
            return None;
        };
        let class = self.resolver.defs.get(at)?.get(ty)?;
        (class.kind == Kind::Class).then_some((at, ty, field))
    }

    // ponytail: a literal of an alias or a defined type (`type B = T`, `type B T`) written by
    // position, and a conversion from another struct type (`T(other)`), are not seen as setting
    // the fields of `T`.
    pub fn field_values(
        &self,
        field: Field<'a>,
        test_read: bool,
    ) -> Option<Vec<(&'a str, &'a str, &'a Value)>> {
        let (at, ty, name) = field;
        let index = self.literals.get_or_init(|| {
            let mut index: HashMap<&'a str, BTreeSet<(&'a str, &'a str)>> = HashMap::new();
            for (&path, facts) in self.resolver.files {
                for constant in &facts.consts {
                    let last = constant.name.rsplit('.').next().unwrap_or_default();
                    if let Some(set) = last.strip_prefix('{').and_then(|l| l.strip_suffix('}')) {
                        let marker = (path, constant.name.as_str());
                        index.entry(set).or_default().insert(marker);
                    }
                }
            }
            index
        });
        let mut out = Vec::new();
        let markers = [index.get(name), index.get("*")];
        for &(path, marker) in markers.into_iter().flatten().flatten() {
            if !test_read && crate::http::is_test_file(path) {
                continue;
            }
            let by_position = marker.ends_with("{*}");
            let scope = scope_of(marker);
            for &literal in self.consts(path, marker) {
                let Value::New(written, set) = &literal.value else {
                    return None;
                };
                if !self.charge(path, 1) {
                    return None;
                }
                let [value] = &set[..] else {
                    return None;
                };
                if written == "?" {
                    return None;
                }
                match self.resolver.binding(path, scope, written) {
                    Binding::Def(t) if (t.path, t.name) == (at, ty) => match by_position {
                        true => return None,
                        false => out.push((path, scope, value)),
                    },
                    Binding::Def(t)
                        if by_position
                            || (self.resolver.receivers.get(t.path))
                                .is_some_and(|r| r.contains_key(&(t.name, name))) => {}
                    Binding::External(..) => {}
                    // ponytail: a function-local alias of the struct (`type L = T; L{a, b}`) or
                    // a type parameter instantiated with it sets the struct's fields unseen;
                    // read local type declarations when one matters.
                    Binding::Hidden
                        if by_position
                            && written.chars().all(|c| c.is_alphanumeric() || c == '_')
                            && !self.go_package_may_name_type(path, written) => {}
                    _ => return None,
                }
            }
        }
        Some(out)
    }

    fn go_package_may_name_type(&self, path: &str, name: &str) -> bool {
        let r = self.resolver;
        r.defs[path].contains_key(name)
            || r.shared
                .get(path)
                .is_some_and(|&g| r.groups[g].tops.contains_key(name))
            || (name.starts_with(char::is_uppercase) && !r.by_local[path].stars.is_empty())
    }

    pub fn field_written(&self, field: &str) -> bool {
        self.resolver.written.contains(field)
    }

    pub fn field_written_in(&self, class: &str, field: &str, test_read: bool) -> bool {
        if self.resolver.ts {
            return self.resolver.defs.iter().any(|(&path, defs)| {
                defs.get_key_value(class).is_some_and(|(&name, def)| {
                    def.kind == Kind::Class
                        && self
                            .resolver
                            .ts_field_written(Target { path, name }, field, test_read)
                })
            });
        }
        let written = match test_read {
            true => &self.resolver.written,
            false => &self.resolver.written_outside_tests,
        };
        written.contains(field) || self.resolver.written_in.contains(&(class, field))
    }

    fn field_read(&self, field: Field<'a>, depth: usize, nested: usize, test_read: bool) -> Value {
        let (at, ty, _) = field;
        let struct_field = (self.resolver.defs.get(at).and_then(|d| d.get(ty)))
            .is_some_and(|d| d.kind == Kind::Class);
        if depth >= FIELD_DEPTH || !struct_field {
            return Value::Unknown;
        }
        let key = (field, test_read);
        if let Some(known) = self.field_reads.borrow().get(&key) {
            return known.clone();
        }
        (self.field_reads.borrow_mut()).insert(key, Value::Unknown);
        let mut reads = Vec::new();
        let all = self.field_values(field, test_read).is_some_and(|values| {
            !values.is_empty()
                && (values.into_iter()).all(|(path, scope, value)| {
                    let frames = &mut Vec::new();
                    let at = (depth, nested, test_read);
                    self.literal_reads(path, scope, value, frames, at, &mut reads)
                })
        });
        let read = match (all, &reads[..]) {
            (true, [first, rest @ ..]) if rest.iter().all(|r| r == first) => {
                Value::Str(vec![first.clone()])
            }
            _ => Value::Unknown,
        };
        (self.field_reads.borrow_mut()).insert(key, read.clone());
        read
    }

    // ponytail: a constructor read as a value (`mk := New; mk(x)`) is not seen, so its exact
    // callers alone give the field; reading every value of the repository for its name would
    // see it.
    fn literal_reads(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &mut Vec<Frame<'a>>,
        (depth, nested, test_read): (usize, usize, bool),
        reads: &mut Vec<Part>,
    ) -> bool {
        let mut wants = None;
        let fold = &mut Fold {
            field_depth: depth + 1,
            nested,
            ..Fold::default()
        };
        let folded = self.eval(path, scope, value, frames, 0, fold, &mut wants);
        if let Value::Str(mut parts) = folded
            && let [Part::Env(_) | Part::EnvOr(..)] = &parts[..]
        {
            reads.extend(parts.pop());
            return true;
        }
        let Some(function) = wants else {
            return false;
        };
        if frames.iter().any(|f| f.def == function)
            || frames.len() >= WRAPPER_DEPTH
            || self.name_matched(function)
        {
            return false;
        }
        let callers: Vec<Caller> = (self.resolver.callers.get(&function).into_iter().flatten())
            .chain(
                self.resolver
                    .test_callers
                    .get(&function)
                    .into_iter()
                    .flatten()
                    .filter(|_| test_read),
            )
            .copied()
            .collect();
        !callers.is_empty()
            && callers.into_iter().all(|caller| {
                if !self.charge(caller.path, 1) {
                    return false;
                }
                frames.push(Frame {
                    def: function,
                    path: caller.path,
                    index: caller.index,
                });
                let at = (depth, nested, test_read);
                let read = self.literal_reads(path, scope, value, frames, at, reads);
                frames.pop();
                read
            })
    }

    fn arg(
        &self,
        path: &'a str,
        index: usize,
        function: Target<'a>,
        n: usize,
    ) -> Option<&'a Value> {
        let call = self.call(path, index)?;
        let params = &self.resolver.defs[function.path][function.name].params;
        let param = params.get(n)?;
        if let (Some(_), Some(labels)) = (&param.label, &call.labels) {
            return match param.binds {
                Binds::Rest => None,
                _ => Self::labelled_arg(call, &labelled_args(params, labels)?, labels, n),
            };
        }
        let key = &param.name;
        let bound = match params.first().map(|p| p.binds) {
            Some(Binds::Class) => true,
            Some(Binds::Instance) => call.callee.rsplit_once('.').is_some_and(|(receiver, _)| {
                let class = self.definition(path, &call.scope, receiver);
                !class.is_some_and(|c| self.resolver.is_class(c))
            }),
            _ => false,
        };
        let position = match bound {
            true => n.checked_sub(1),
            false => Some(n),
        };
        if bound && n == 0 && param.binds == Binds::Instance {
            return call.receiver.as_ref();
        }
        match param.binds {
            Binds::Rest => None,
            Binds::Keyword => call.keyword(key),
            Binds::Any | Binds::Optional | Binds::Instance | Binds::Class => (position)
                .and_then(|at| call.args.get(at))
                .or_else(|| call.keyword(key)),
        }
    }

    fn labelled_arg(
        call: &'a Call,
        takes: &[Option<usize>],
        labels: &[String],
        n: usize,
    ) -> Option<&'a Value> {
        let i = (*takes.get(n)?)?;
        let before = labels[..i].iter().filter(|l| *l == "_").count();
        match labels[i].as_str() {
            "_" => call.args.get(before),
            "" => call.args.get(labels.iter().filter(|l| *l == "_").count()),
            label => call.keyword(label),
        }
    }

    fn follows(&self, trail: &Trail<'a>, flow: &Flow<'a>) -> bool {
        let file = match flow {
            Flow::Const(at, _) | Flow::Local(at, ..) => at,
            Flow::Param(function, _) | Flow::Returns(function) => function.path,
        };
        !trail.busy.contains(flow) && trail.busy.len() < EVAL_DEPTH && self.charge(file, 1)
    }

    fn flow(&self, path: &'a str, scope: &'a str, value: &'a Value, trail: &mut Trail<'a>) {
        match value {
            Value::Await(value) => self.flow(path, scope, value, trail),
            Value::Ref(name) => self.flow_name(path, scope, name, trail),
            Value::Call(call) => {
                let calls = &self.resolver.files[path].calls;
                let Some(index) = call_at(calls, &call.range) else {
                    return;
                };
                let fluent = self.pack.fluent();
                let tail = match fluent {
                    true => self.chain_tail(path, index),
                    false => Some(Tail {
                        stop: index,
                        depth: 0,
                    }),
                };
                let Some(Tail { stop, .. }) = tail else {
                    return;
                };
                let kept = fluent.then(|| std::mem::take(&mut trail.out));
                let mark = trail.steps.len();
                if fluent {
                    self.chain_steps(path, index, &mut trail.steps);
                }
                self.flow_from(path, stop, trail);
                if fluent {
                    let steps = trail.steps.split_off(mark);
                    trail.out.retain(|&(p, n), _| {
                        let call = &self.resolver.files[p].calls[n];
                        match self.outside(p, &call.scope, &call.callee, call.by_path) {
                            Some(o) => steps.iter().all(|m| (trail.passes)(&o.spelled, m)),
                            None => steps.is_empty(),
                        }
                    });
                }
                if let Some(mut kept) = kept {
                    if kept.len() < trail.out.len() {
                        std::mem::swap(&mut kept, &mut trail.out);
                    }
                    for (origin, mut routes) in std::mem::take(&mut trail.out) {
                        let into = kept.entry(origin).or_default();
                        if into.len() < routes.len() {
                            std::mem::swap(into, &mut routes);
                        }
                        into.extend(routes);
                    }
                    trail.out = kept;
                }
            }
            Value::List(items) => {
                for item in items {
                    self.flow(path, scope, item, trail);
                }
            }
            _ => {}
        }
    }

    fn reached(&self, path: &'a str, index: usize, trail: &mut Trail<'a>) {
        let route = Route {
            through: trail.frames.iter().map(|f| (f.path, f.index)).collect(),
            steps: trail.steps.iter().rev().copied().collect(),
            held: trail.held,
        };
        trail.out.entry((path, index)).or_default().insert(route);
    }

    fn flow_from(&self, path: &'a str, index: usize, trail: &mut Trail<'a>) {
        let calls = &self.resolver.files[path].calls;
        let function = match self.resolver.flows_to(path, &calls[index]) {
            Some(t) if !self.resolver.is_class(t) => t,
            _ if self.pack.fluent() => match self.receiver(path, index) {
                None => {
                    self.reached(path, index, trail);
                    return;
                }
                Some(Value::Call(_)) => return,
                Some(other) => {
                    trail.steps.push(method(&calls[index].callee));
                    return self.flow(path, &calls[index].scope, other, trail);
                }
            },
            _ => {
                self.reached(path, index, trail);
                return;
            }
        };
        let Some(def) = self.def(function.path, function.name) else {
            return;
        };
        if !self.follows(trail, &Flow::Returns(function)) {
            return;
        }
        trail.busy.push(Flow::Returns(function));
        trail.frames.push(Frame {
            def: function,
            path,
            index,
        });
        for returned in &def.return_values {
            self.flow(function.path, function.name, returned, trail);
        }
        trail.frames.pop();
        trail.busy.pop();
    }

    pub fn chain(&self, path: &'a str, index: usize) -> Option<Vec<usize>> {
        let stop = self.chain_tail(path, index)?.stop;
        let calls = &self.resolver.files[path].calls;
        let mut out = vec![index];
        while out[out.len() - 1] != stop {
            let Some(Value::Call(next)) = self.receiver(path, out[out.len() - 1]) else {
                return None;
            };
            out.push(call_at(calls, &next.range)?);
        }
        Some(out)
    }

    fn chain_steps(&self, path: &'a str, index: usize, steps: &mut Vec<&'a str>) {
        let calls = &self.resolver.files[path].calls;
        if let Some((_, above)) = self
            .chain(path, index)
            .as_deref()
            .and_then(<[usize]>::split_last)
        {
            steps.extend(above.iter().map(|&at| method(&calls[at].callee)));
        }
    }

    fn chain_tail(&self, path: &'a str, index: usize) -> Option<Tail> {
        let calls = &self.resolver.files[path].calls;
        let mut walked: Vec<usize> = Vec::new();
        let mut at = index;
        let mut tail = loop {
            if let Some(&known) = self.chains.borrow().get(&(path, at)) {
                break known;
            }
            let resolution = self.resolver.call(path, &calls[at]);
            let answered =
                matches!(&resolution, Resolution::Exact(t) if !self.resolver.is_class(*t));
            match (self.receiver(path, at), answered) {
                (Some(Value::Call(next)), false) => {
                    walked.push(at);
                    match call_at(calls, &next.range) {
                        Some(next) => at = next,
                        None => break None,
                    }
                }
                _ => {
                    break Some(Tail { stop: at, depth: 0 });
                }
            }
        };
        for &step in walked.iter().rev() {
            tail = tail.filter(|t| t.depth < FLUENT_CHAIN).map(|t| Tail {
                depth: t.depth + 1,
                ..t
            });
            self.chains.borrow_mut().insert((path, step), tail);
        }
        tail
    }

    fn flow_name(&self, path: &'a str, scope: &'a str, name: &'a str, trail: &mut Trail<'a>) {
        let binding = self.resolver.read_member(path, scope, name);
        let binding = match binding {
            Binding::Local(at, scope, name) => {
                let constant = self.resolver.consts[at].get(crate::qualify(scope, name).as_str());
                constant.map_or(binding, |&constant| Binding::Const(at, constant))
            }
            binding => binding,
        };
        match binding {
            Binding::Const(at, constant) => {
                let Some(found) = self.consts(at, constant).first() else {
                    return;
                };
                if !self.follows(trail, &Flow::Const(at, constant)) {
                    return;
                }
                let asked = trail.busy.is_empty();
                if asked {
                    trail.held = Some((at, constant));
                }
                trail.busy.push(Flow::Const(at, constant));
                self.flow(at, crate::parent(constant), &found.value, trail);
                trail.busy.pop();
                if asked {
                    trail.held = None;
                }
            }
            Binding::Param(function, n, None) if self.unchanged_parameter(function, name) => {
                let sites = match trail.frames.last() {
                    Some(frame) if frame.def == function => vec![(frame.path, frame.index)],
                    // ponytail: a parameter of an outer function, captured by a closure, is
                    // not followed.
                    Some(_) => return,
                    None => (self.resolver.callers.get(&function).into_iter().flatten())
                        .chain(
                            self.resolver
                                .extension_callers
                                .get(&function)
                                .into_iter()
                                .flatten(),
                        )
                        .map(|c| (c.path, c.index))
                        .collect(),
                };
                if sites.is_empty() {
                    trail.unbound.insert((function, n));
                }
                if !self.follows(trail, &Flow::Param(function, n)) {
                    return;
                }
                trail.busy.push(Flow::Param(function, n));
                let frame = trail.frames.pop();
                for (caller, index) in sites {
                    let scope = &self.resolver.files[caller].calls[index].scope;
                    if let Some(arg) = self.arg(caller, index, function, n) {
                        self.flow(caller, scope, arg, trail);
                    }
                }
                trail.frames.extend(frame);
                trail.busy.pop();
            }
            Binding::Hidden | Binding::Local(..)
                if let Some(bound) = self.local(path, scope, name) =>
            {
                let flow = Flow::Local(path, &bound.scope, &bound.name);
                if !self.follows(trail, &flow) {
                    return;
                }
                trail.busy.push(flow);
                self.flow(path, &bound.within, &bound.value, trail);
                trail.busy.pop();
            }
            // ponytail: a name its scope binds more than once, and an attribute not assigned a
            // call once in its class, hold no constant, so what they hold is not followed.
            _ => {}
        }
    }

    pub fn arg_type(&self, path: &'a str, index: usize, n: usize) -> Option<&'a Ty> {
        self.full(path)?
            .calls
            .get(index)?
            .arg_types
            .get(n)?
            .as_ref()
    }

    pub fn written(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(&'a str, &'a str, &'a str)> {
        let r = self.resolver;
        let (at, within, held) = r.holder(&r.read_member(path, scope, name))?;
        match r.receiver_class(at, within, held)? {
            Class::Type(written) => Some((at, within, written)),
            _ => None,
        }
    }

    pub fn constant_type(&self, path: &'a str, constant: &str) -> Option<&'a Ty> {
        let consts = self.consts(path, constant);
        consts.iter().find(|c| c.ty.is_some())?.ty.as_ref()
    }

    pub fn uses(&self, path: &'a str, index: usize) -> Option<&'a [Vec<Step>]> {
        Some(self.full(path)?.calls.get(index)?.uses.as_slice())
    }

    pub fn returns(&self, path: &'a str, name: &str) -> Option<&'a Ty> {
        self.def(path, name)?.returns.as_ref()
    }

    pub fn type_params(&self, path: &'a str, name: &str) -> &'a [String] {
        let def = self.def(path, name);
        def.map_or(&[], |d| d.type_params.as_slice())
    }

    fn full(&self, path: &'a str) -> Option<&'a Facts> {
        let found = self.full.get(path).copied();
        match found {
            Some(_) => self.read.borrow_mut().insert(path),
            None => self.misses.borrow_mut().insert(path),
        };
        found
    }

    fn by_name(&self, path: &'a str) -> Option<&ByName<'a>> {
        let facts = self.full(path)?;
        Some(self.by_name[path].get_or_init(|| ByName::new(facts)))
    }

    pub(crate) fn def(&self, path: &'a str, name: &str) -> Option<&'a Def> {
        self.by_name(path)?.defs.get(name).copied()
    }

    fn consts(&self, path: &'a str, name: &str) -> &[&'a Const] {
        let consts = self.by_name(path).and_then(|b| b.consts.get(name));
        consts.map_or(&[], Vec::as_slice)
    }

    pub fn param(&self, function: Target<'a>, n: usize) -> Option<&'a Param> {
        self.def(function.path, function.name)?.params.get(n)
    }

    fn unchanged_parameter(&self, function: Target<'a>, name: &str) -> bool {
        self.def(function.path, function.name)
            .is_some_and(|d| d.reassigned_params.iter().all(|p| p != name))
    }

    pub fn value(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Value {
        let folded = self.eval(path, scope, value, &[], 0, &mut Fold::default(), &mut None);
        unbind(folded, false, &[])
    }

    pub fn constant(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<((&'a str, &'a str), &'a Value)> {
        match self
            .resolver
            .lexical(path, scope, name, &mut self.resolver.seen(path, scope))
        {
            Binding::Const(at, constant) => Some(((at, constant), self.held(at, constant)?.0)),
            _ => None,
        }
    }

    pub fn constant_value(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(&'a str, &'a str, &'a Value)> {
        let binding = self.resolver.read_member(path, scope, name);
        let binding = match binding {
            Binding::Local(at, within, name) => self.resolver.consts[at]
                .get(crate::qualify(within, name).as_str())
                .map_or(binding, |&constant| Binding::Const(at, constant)),
            binding => binding,
        };
        let Binding::Const(at, constant) = binding else {
            return None;
        };
        let (value, within) = self.held(at, constant)?;
        Some((at, within, value))
    }

    fn held(&self, path: &'a str, constant: &'a str) -> Option<(&'a Value, &'a str)> {
        let consts = self.consts(path, constant);
        let held = consts.iter().find(|c| !matches!(c.value, Value::Type(_)))?;
        Some((
            &held.value,
            held.scope.as_deref().unwrap_or(scope_of(constant)),
        ))
    }

    pub fn declared_type(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(&'a str, &'a str, &'a Ty)> {
        let Binding::Const(at, constant) =
            self.resolver
                .lexical(path, scope, name, &mut self.resolver.seen(path, scope))
        else {
            return None;
        };
        let mut declared =
            (self.consts(at, constant).iter()).filter(|c| matches!(c.value, Value::Type(_)));
        let declared = declared.next().filter(|_| declared.next().is_none())?;
        match &declared.value {
            Value::Type(ty) => Some((at, constant, ty)),
            _ => None,
        }
    }

    pub fn param_defaults_to(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        default: &str,
    ) -> bool {
        let Binding::Param(function, n, field) =
            self.resolver
                .lexical(path, scope, name, &mut self.resolver.seen(path, scope))
        else {
            return false;
        };
        let param = self.param(function, n);
        let value = match field {
            Some(f) => param.and_then(|p| p.fields.get(f)).map(|f| &f.default),
            None => param.map(|p| &p.default),
        };
        matches!(value, Some(Some(Value::Ref(r))) if r == default)
    }

    pub fn is_param(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        function: Target<'a>,
        n: usize,
    ) -> bool {
        matches!(
            self.resolver.lexical(path, scope, name, &mut self.resolver.seen(path, scope)),
            Binding::Param(f, at, None) if f == function && at == n
        )
    }

    pub fn param_of(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(Target<'a>, usize)> {
        match self
            .resolver
            .lexical(path, scope, name, &mut self.resolver.seen(path, scope))
        {
            Binding::Param(function, n, None) => Some((function, n)),
            _ => None,
        }
    }

    pub(crate) fn unchanged_param_of(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(Target<'a>, usize)> {
        self.param_of(path, scope, name)
            .filter(|(function, _)| self.unchanged_parameter(*function, name))
    }

    pub fn param_args(&self, function: Target<'a>, n: usize) -> Vec<(&'a str, &'a str, &'a Value)> {
        let callers = self.resolver.callers.get(&function).into_iter().flatten();
        let args = callers.filter_map(|c| {
            let arg = self.arg(c.path, c.index, function, n)?;
            Some((
                c.path,
                self.resolver.files[c.path].calls[c.index].scope.as_str(),
                arg,
            ))
        });
        args.collect()
    }

    pub fn param_values(
        &self,
        function: Target<'a>,
        n: usize,
        frame: Option<(&'a str, usize)>,
    ) -> Option<Vec<(&'a str, &'a str, &'a Value)>> {
        let sites: Vec<_> = frame.map_or_else(
            || {
                self.resolver
                    .callers
                    .get(&function)
                    .into_iter()
                    .flatten()
                    .map(|c| (c.path, c.index))
                    .collect()
            },
            |site| vec![site],
        );
        if sites.is_empty() {
            return None;
        }
        sites
            .into_iter()
            .map(|(path, index)| {
                let call = self.call(path, index)?;
                if call.keyword("...").is_some() || call.keyword("*").is_some() {
                    return None;
                }
                if let Some(value) = self.arg(path, index, function, n) {
                    Some((path, call.scope.as_str(), value))
                } else {
                    Some((
                        function.path,
                        crate::parent(function.name),
                        self.param(function, n)?.default.as_ref()?,
                    ))
                }
            })
            .collect()
    }

    pub fn reaches(&self, path: &'a str, index: usize) -> Option<Target<'a>> {
        let call = self.resolver.files[path].calls.get(index)?;
        self.resolver.flows_to(path, call)
    }

    pub fn origins_within(
        &self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        passes: Passes,
        through: &[(&'a str, usize)],
    ) -> Vec<Origin<'a>> {
        self.origins_unbound(path, scope, of, passes, through).0
    }

    pub fn origins_unbound(
        &self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        passes: Passes,
        through: &[(&'a str, usize)],
    ) -> (Vec<Origin<'a>>, Vec<(Target<'a>, usize)>) {
        let mut trail = Trail::new(passes);
        for &(at, index) in through {
            let Some(def) = self.reaches(at, index) else {
                return Default::default();
            };
            trail.frames.push(Frame {
                def,
                path: at,
                index,
            });
        }
        match of {
            Of::Name(name) => self.flow_name(path, scope, name, &mut trail),
            Of::Value(value) => self.flow(path, scope, value, &mut trail),
        }
        let origin = |((p, n), routes): ((&'a str, usize), BTreeSet<Route<'a>>)| {
            let call = &self.resolver.files[p].calls[n];
            Origin {
                call: Some((p, n)),
                form: Form::Call,
                outside: self.outside(p, &call.scope, &call.callee, call.by_path),
                routes: routes.into_iter().collect(),
            }
        };
        let unbound = trail.unbound.into_iter().collect();
        (trail.out.into_iter().map(origin).collect(), unbound)
    }

    pub fn is_global(&self, path: &'a str, scope: &'a str, name: &'a str) -> bool {
        matches!(
            self.resolver
                .lexical(path, scope, name, &mut self.resolver.seen(path, scope)),
            Binding::Unknown
        )
    }

    pub fn definition(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<Target<'a>> {
        match self.resolver.binding(path, scope, name) {
            Binding::Def(t) => Some(t),
            _ => None,
        }
    }

    pub fn extensions(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &str,
        arity: Option<usize>,
    ) -> Vec<(u8, Target<'a>)> {
        let r = self.resolver;
        let defs = &r.defs[path];
        let mut hiding = None;
        let mut at = Some(scope);
        while let Some(s) = at.filter(|s| !s.is_empty()) {
            let class = defs.get(s).is_some_and(|d| d.kind == Kind::Class);
            let param = defs
                .get(s)
                .is_some_and(|d| d.params.iter().any(|p| p.name == name));
            if !class
                && (param
                    || r.locals[path].contains(crate::qualify(s, name).as_str())
                    || r.receivers[path].contains_key(&(s, name)))
            {
                hiding = Some(s);
                break;
            }
            at = Some(crate::parent(s));
        }
        let hidden = |within: &str| {
            hiding.is_some_and(|h: &str| {
                within.is_empty() || h.strip_prefix(within).is_some_and(|r| r.starts_with('.'))
            })
        };
        let fits =
            |t: &Target<'a>| arity.is_none_or(|n| r.arities.get(t).is_none_or(|a| a.fits(n)));
        let mut found: Vec<(u8, Target<'a>)> = (r.extensions.get(name).into_iter().flatten())
            .filter(|e| fits(&e.target) && !hidden(e.within))
            .filter_map(|e| Some((r.visible(path, scope, name, e)?, e.target)))
            .collect();
        found.sort();
        found.dedup();
        found
    }

    pub fn method(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        method: &'a str,
    ) -> Option<Target<'a>> {
        let r = self.resolver;
        let (seen, work) = (&mut r.seen(path, scope), &mut Work::default());
        let found = r.resolve_as(path, scope, name, seen, work, Reading::Value);
        match r.instance(found, work) {
            Binding::Def(t) | Binding::Instance(t) if r.is_class(t) => {
                match r.member(t, method, false, None, work) {
                    Binding::Def(t) => Some(t),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub fn module(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<&'a str> {
        match self.resolver.binding(path, scope, name) {
            Binding::Module(m) => Some(m),
            _ => None,
        }
    }

    pub fn published(&self, path: &str) -> bool {
        (self.resolver.published.as_ref()).is_none_or(|files| files.contains(path))
    }

    pub fn import_file(&self, path: &str, specifier: &str) -> Option<&'a str> {
        self.resolver.module(path, specifier)
    }

    // ponytail: every simple path up the wrappers' callers is walked; memoize per (wrapper,
    // call site) if a dense call graph makes this slow.
    pub fn expand<R>(
        &self,
        from: Expand<'a>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<Expanded<'a, R>> {
        self.expand_with(from, None, read)
    }

    pub fn expand_with<R>(
        &self,
        from: Expand<'a>,
        inject: Option<&Inject<'a, '_>>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<Expanded<'a, R>> {
        self.expand_using(from, inject, None, None, read)
    }

    pub(crate) fn expand_with_call<R>(
        &self,
        from: Expand<'a>,
        external_value: &ExternalValue<'a, '_>,
        retain_site: fn(&[Value]) -> bool,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<Expanded<'a, R>> {
        self.expand_using(from, None, Some(external_value), Some(retain_site), read)
    }

    fn expand_using<R>(
        &self,
        from: Expand<'a>,
        inject: Option<&Inject<'a, '_>>,
        external_value: Option<&ExternalValue<'a, '_>>,
        retain_site: Option<fn(&[Value]) -> bool>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<Expanded<'a, R>> {
        let mut out = Vec::new();
        match from {
            Expand::Call(path, index) => {
                let Some(call) = self.call(path, index) else {
                    return out;
                };
                let written = Written {
                    path,
                    scope: &call.scope,
                    args: &call.args,
                    kwargs: &call.kwargs,
                    at: (path, index),
                    chain: None,
                    inject,
                    external_value,
                    retain_site,
                    writes: None,
                };
                self.walk(
                    &written,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    read,
                    &mut out,
                    &mut Vec::new(),
                );
            }
            Expand::Chain(path, index) => {
                let Some(call) = self.call(path, index) else {
                    return out;
                };
                let written = Written {
                    path,
                    scope: &call.scope,
                    args: &call.args,
                    kwargs: &call.kwargs,
                    at: (path, index),
                    chain: Some(call),
                    inject,
                    external_value,
                    retain_site,
                    writes: None,
                };
                self.walk(
                    &written,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    read,
                    &mut out,
                    &mut Vec::new(),
                );
            }
            Expand::Constructed {
                held: (file, constant),
                at,
            } => {
                let Some((Value::New(_, args), scope)) = self.held(file, constant) else {
                    return out;
                };
                let written = Written {
                    path: file,
                    scope,
                    args,
                    kwargs: &[],
                    at,
                    chain: None,
                    inject,
                    external_value,
                    retain_site,
                    writes: None,
                };
                self.walk(
                    &written,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    read,
                    &mut out,
                    &mut Vec::new(),
                );
            }
        }
        out
    }

    pub(crate) fn expand_written<R>(
        &self,
        path: &'a str,
        index: usize,
        writes: CallWrites,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<Expanded<'a, R>> {
        let mut out = Vec::new();
        let Some(call) = self.call(path, index) else {
            return out;
        };
        let written = Written {
            path,
            scope: &call.scope,
            args: &call.args,
            kwargs: &call.kwargs,
            at: (path, index),
            chain: None,
            inject: None,
            external_value: None,
            retain_site: None,
            writes: Some(writes),
        };
        self.walk(
            &written,
            &mut Vec::new(),
            &mut Vec::new(),
            read,
            &mut out,
            &mut Vec::new(),
        );
        out
    }

    fn walk<R>(
        &self,
        written: &Written<'a, '_>,
        frames: &mut Vec<Frame<'a>>,
        fields: &mut Vec<Frame<'a>>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
        out: &mut Vec<Expanded<'a, R>>,
        cuts: &mut Vec<usize>,
    ) {
        let (site, at) = frames.last().map_or(written.at, |f| (f.path, f.index));
        if !self.charge(site, 1) {
            return;
        }
        let chain = written.chain.is_some();
        let ((values, kwargs), wants, field) = self.folded(written, frames, fields);
        if let Some(found) = read(&values, &kwargs) {
            out.push(Expanded {
                site: (site, at),
                frames: frames.clone(),
                found,
            });
            return;
        }
        if let Some(def) = field
            && fields.len() + frames.len() < WRAPPER_DEPTH
            && !fields.iter().any(|f| f.def == def)
        {
            for caller in self.field_callers(def) {
                fields.push(Frame {
                    def,
                    path: caller.path,
                    index: caller.index,
                });
                self.walk(written, frames, fields, read, out, cuts);
                fields.pop();
            }
            return;
        }
        let Some(wrapper) = wants else {
            if field.is_some() {
                let (values, kwargs) = unbound((values, kwargs), chain, &[]);
                out.extend(read(&values, &kwargs).map(|found| Expanded {
                    site: (site, at),
                    frames: frames.clone(),
                    found,
                }));
            }
            return;
        };
        if let Some(first) = frames.iter().position(|f| f.def == wrapper) {
            let more = || {
                self.refused.set(false);
                self.cycle_sends_more(written, frames, fields, first, wrapper, read)
                    && !self.refused.get()
            };
            if self.cut_apart(more) {
                cuts.push(first);
            }
            return;
        }
        // ponytail: so a wrapper reached only by a name match (a method on a receiver whose
        // class Meada cannot tell, a bare name no import binds) is read where it stands, its
        // parameters unknown.
        let callers = self
            .resolver
            .callers
            .get(&wrapper)
            .map_or(&[][..], Vec::as_slice);
        if callers.is_empty() || frames.len() + fields.len() >= WRAPPER_DEPTH {
            let (values, kwargs) = unbound((values, kwargs), chain, &[]);
            out.extend(read(&values, &kwargs).map(|found| Expanded {
                site: (site, at),
                frames: frames.clone(),
                found,
            }));
            return;
        }
        if written.retain_site.is_some_and(|keep| keep(&values)) {
            let (args, keywords) = unbound((values.clone(), kwargs.clone()), chain, &[]);
            out.extend(read(&args, &keywords).map(|found| Expanded {
                site: (site, at),
                frames: frames.clone(),
                found,
            }));
        }
        let (under, climbed) = (frames.len(), out.len());
        for caller in callers {
            frames.push(Frame {
                def: wrapper,
                path: caller.path,
                index: caller.index,
            });
            self.walk(written, frames, fields, read, out, cuts);
            frames.pop();
        }
        if !cuts.contains(&under) {
            return;
        }
        cuts.retain(|&cut| cut != under);
        let mut reached: Vec<((&'a str, usize), Vec<Frame<'a>>)> = Vec::new();
        for expanded in &out[climbed..] {
            if reached.iter().all(|(site, _)| *site != expanded.site) {
                reached.push((expanded.site, expanded.frames.clone()));
            }
        }
        if reached.is_empty() {
            reached.push(((site, at), frames.clone()));
        }
        let (values, kwargs) = unbound((values, kwargs), chain, &[]);
        self.cut_apart(|| {
            for (site, frames) in reached {
                out.extend(read(&values, &kwargs).map(|found| Expanded {
                    site,
                    frames,
                    found,
                }));
            }
        });
    }

    fn folded(
        &self,
        written: &Written<'a, '_>,
        frames: &[Frame<'a>],
        fields: &[Frame<'a>],
    ) -> (Folded, Option<Target<'a>>, Option<Target<'a>>) {
        let &Written {
            path,
            scope,
            args,
            kwargs,
            ..
        } = written;
        let mut wants = None;
        let mut field = None;
        let chain = written.chain.is_some();
        let mut eval = |v: &'a Value| {
            let fold = &mut Fold {
                field_frames: Some(fields),
                chain,
                inject: written.inject,
                external_value: written.external_value,
                selected: (written.writes).map(|writes| mutations::Selection {
                    site: Some(written.at),
                    writes,
                }),
                ..Fold::default()
            };
            let value = self.eval(path, scope, v, frames, 0, fold, &mut wants);
            field = field.or(fold.field_wants);
            value
        };
        let folded = match written.chain {
            Some(call) => {
                let receiver = call.receiver.as_ref().map(|r| Box::new(eval(r)));
                let invocation = Invocation {
                    callee: call.callee.clone(),
                    receiver,
                    args: call.args.iter().map(&mut eval).collect(),
                    kwargs: Vec::new(),
                    range: call.range.clone(),
                };
                let kwargs = (call.kwargs.iter()).map(|(k, v)| (k.clone(), eval(v)));
                (vec![Value::Call(invocation)], kwargs.collect())
            }
            None => (
                args.iter().map(&mut eval).collect(),
                (kwargs.iter()).map(|(k, v)| (k.clone(), eval(v))).collect(),
            ),
        };
        (folded, wants, field)
    }

    fn cycle_sends_more<R>(
        &self,
        written: &Written<'a, '_>,
        frames: &[Frame<'a>],
        fields: &[Frame<'a>],
        first: usize,
        wrapper: Target<'a>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> bool {
        let chain = written.chain.is_some();
        let def = self.resolver.defs[wrapper.path][wrapper.name];
        let names = (def.params.iter())
            .flat_map(|p| std::iter::once(&p.name).chain(p.fields.iter().map(|f| &f.name)));
        let changed: Vec<&str> = names
            .map(String::as_str)
            .filter(|&name| {
                let fold = &mut Fold {
                    chain,
                    ..Fold::default()
                };
                let (path, scope) = (wrapper.path, wrapper.name);
                let handed = self.name(path, scope, name, frames, first, fold, &mut None);
                !matches!(&handed, Value::Ref(same) if same == name)
            })
            .collect();
        if changed.is_empty() {
            return false;
        }
        let (first_turn, _, _) = self.folded(written, &frames[..first], fields);
        let (values, kwargs) = unbound(first_turn, chain, &changed);
        read(&values, &kwargs).is_none()
    }

    // ponytail: any other definition of a method's name counts, an interface's or not, since no
    // pack marks interfaces: two types with a constructor method of one name leave it unknown.
    pub(crate) fn name_matched(&self, function: Target<'a>) -> bool {
        self.matched().0.contains(&function) || self.shares_name(function)
    }

    fn matched(&self) -> &(HashSet<Target<'a>>, HashMap<&'a str, usize>) {
        self.matched.get_or_init(|| {
            let matched = (self.resolver.built.values().flatten())
                .flat_map(|r| match r {
                    Resolution::Inferred(t) | Resolution::Narrowed(t) => std::slice::from_ref(t),
                    Resolution::Ambiguous(targets) => targets.as_slice(),
                    _ => &[],
                })
                .copied()
                .collect();
            let mut names = HashMap::new();
            for name in self.resolver.defs.values().flat_map(|d| d.keys()) {
                *names.entry(own_name(name)).or_insert(0) += 1;
            }
            (matched, names)
        })
    }

    fn shares_name(&self, function: Target<'a>) -> bool {
        let method = self.resolver.defs[function.path][function.name].kind == Kind::Method;
        method && self.matched().1.get(own_name(function.name)) > Some(&1)
    }

    pub fn runs_alone(&self, function: Target<'a>) -> bool {
        let def = &self.resolver.defs[function.path][function.name];
        match def.kind {
            Kind::Function => true,
            Kind::Method => def.private || !self.shares_name(function),
            _ => false,
        }
    }

    fn may_be_called(&self, function: Target<'a>) -> bool {
        if (self.resolver.callers.get(&function)).is_some_and(|c| !c.is_empty()) {
            return true;
        }
        let callees = self.callees.get_or_init(|| {
            (self.resolver.files.values().flat_map(|f| &f.calls))
                .map(|c| c.callee.rsplit('.').next().unwrap_or(&c.callee))
                .collect()
        });
        let own = function.name[crate::parent(function.name).len()..].trim_start_matches('.');
        callees.contains(own.split('(').next().unwrap_or(own))
    }

    fn injected(
        &self,
        path: &'a str,
        marks: impl Fn(&Marked) -> bool,
        inject: &Inject<'a, '_>,
    ) -> Option<Value> {
        let annotated = &self.full(path)?.annotated;
        (annotated.iter())
            .filter(|a| marks(&a.on))
            .find_map(|a| inject(self, path, Injected::Annotated(a)))
    }

    #[expect(clippy::too_many_arguments)]
    fn eval(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        if fold.nested >= EVAL_DEPTH || !self.charge(path, 1) {
            return Value::Unknown;
        }
        fold.nested += 1;
        let folded = match value {
            Value::Await(inner)
                if self.fold_returned_strings.get()
                    && let Value::Call(call) = inner.as_ref() =>
            {
                self.built(path, &call.range, true, frames, depth, fold, wants)
                    .unwrap_or_else(|| inner.as_ref().clone())
            }
            Value::Await(value) => self.eval(path, scope, value, frames, depth, fold, wants),
            Value::Str(parts) => self.eval_string(path, scope, parts, frames, depth, fold, wants),
            Value::Object(_) => self.eval_object(path, scope, value, frames, depth, fold, wants),
            Value::List(items) => Value::List(
                (items.iter())
                    .map(|v| self.eval(path, scope, v, frames, depth, fold, wants))
                    .collect(),
            ),
            Value::New(callee, args) => {
                self.eval_new(path, scope, callee, args, frames, depth, fold, wants)
            }
            Value::Call(call) if fold.chain && !self.fold_returned_strings.get() => {
                self.eval_chain(path, scope, call, frames, depth, fold, wants)
            }
            Value::Call(call) => self
                .built(path, &call.range, false, frames, depth, fold, wants)
                .unwrap_or_else(|| {
                    if fold.chain {
                        self.eval_chain(path, scope, call, frames, depth, fold, wants)
                    } else {
                        value.clone()
                    }
                }),
            Value::Ref(name) => self.name(path, scope, name, frames, depth, fold, wants),
            Value::Member(name, member) if fold.selected.is_some() => {
                self.eval_member(path, scope, name, member, frames, depth, fold, wants)
            }
            other => other.clone(),
        };
        fold.nested -= 1;
        folded
    }

    #[expect(clippy::too_many_arguments)]
    fn eval_string(
        &self,
        path: &'a str,
        scope: &'a str,
        parts: &'a [Part],
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        let mut out = Vec::new();
        let mut records = Vec::new();
        for part in parts {
            let value = match part {
                Part::RecordStart {
                    at,
                    field,
                    property,
                } => {
                    records.push((out.len(), *at, field, property));
                    continue;
                }
                Part::RecordEnd if let Some((at, _, field, property)) = records.pop() => {
                    let key = out.split_off(at);
                    self.ts_record(
                        (path, scope),
                        (field, property),
                        key,
                        frames,
                        depth,
                        fold,
                        wants,
                    )
                    .unwrap_or(Value::Unknown)
                }
                Part::Ref(name) if let Some((_, at, _, _)) = records.last() => {
                    let previous = fold.record_site.replace((path, *at));
                    let value = self.name(path, scope, name, frames, depth, fold, wants);
                    fold.record_site = previous;
                    value
                }
                Part::Ref(name) => self.name(path, scope, name, frames, depth, fold, wants),
                Part::EnvKey { name, reader } => {
                    if reader.as_deref().is_some_and(|reader| {
                        !matches!(self.imported(path, scope, reader),
                            Some(("os", name)) if matches!(name.as_str(), "getenv" | "environ" | "environ.get"))
                    }) {
                        out.push(Part::Hole);
                        continue;
                    }
                    let key = self.name(path, scope, name, frames, depth, fold, wants);
                    out.push(Part::env_key(&key).unwrap_or(Part::Hole));
                    continue;
                }
                Part::Read {
                    scope: read_scope,
                    callee,
                    key,
                    parameter: true,
                    then,
                } => {
                    let key = self.name(path, scope, key, frames, depth, fold, wants);
                    out.push(match key {
                        Value::Str(parts) if let Some(key) = crate::lang::literal(&parts) => {
                            Part::Read {
                                scope: read_scope.clone(),
                                callee: callee.clone(),
                                key,
                                parameter: false,
                                then: then.clone(),
                            }
                        }
                        Value::Ref(key) => Part::Read {
                            scope: read_scope.clone(),
                            callee: callee.clone(),
                            key,
                            parameter: true,
                            then: then.clone(),
                        },
                        _ => Part::Hole,
                    });
                    continue;
                }
                Part::Invoke {
                    callee,
                    range,
                    awaited,
                } => {
                    if self.fold_returned_strings.get()
                        && let Some(value) =
                            self.built(path, range, *awaited, frames, depth, fold, wants)
                    {
                        value
                    } else {
                        out.push(Part::Limit(crate::lang::Limit::Returned(callee.clone())));
                        continue;
                    }
                }
                _ => {
                    out.push(part.clone());
                    continue;
                }
            };
            match value {
                Value::Str(parts) => match self.charge(path, parts.len()) {
                    true => out.extend(parts),
                    false => out.push(Part::Hole),
                },
                Value::Ref(name) => out.push(Part::Ref(name)),
                other => out.push(Part::unread(&other)),
            }
        }
        Value::Str(crate::lang::fold_strips(out))
    }

    #[expect(clippy::too_many_arguments)]
    fn eval_object(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        let Value::Object(properties) = value else {
            return Value::Unknown;
        };
        let tracked = (fold.selected.zip(fold.objects.last().copied()))
            .filter(|(_, (_, pointer))| *pointer == value as *const Value as usize);
        let changed = |field: &str| {
            tracked.is_some_and(|(selected, (object, _))| {
                self.member_changed(object, field, selected, frames)
            })
        };
        Value::Object(
            properties
                .iter()
                .map(|(k, v)| {
                    if changed(k) {
                        return (k.clone(), Value::Unknown);
                    }
                    let v = self.eval(path, scope, v, frames, depth, fold, wants);
                    (k.clone(), unwritten(k, v, &changed))
                })
                .collect(),
        )
    }

    #[expect(clippy::too_many_arguments)]
    fn eval_new(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        args: &'a [Value],
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        Value::New(
            if let Some(known) = fold.new_types {
                self.outside_written(path, scope, callee)
                    .and_then(|o| crate::names::one(&o.spellings(), known).map(str::to_string))
                    .unwrap_or_default()
            } else {
                callee.to_string()
            },
            (args.iter())
                .map(|v| self.eval(path, scope, v, frames, depth, fold, wants))
                .collect(),
        )
    }

    #[expect(clippy::too_many_arguments)]
    fn eval_chain(
        &self,
        path: &'a str,
        scope: &'a str,
        call: &'a Invocation,
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        let stored = fold
            .new_types
            .and_then(|_| self.call_index(path, &call.range))
            .and_then(|index| self.call(path, index));
        let scope = stored.map_or(scope, |c| c.scope.as_str());
        let args = stored.map_or(call.args.as_slice(), |c| c.args.as_slice());
        let kwargs = stored.map_or(call.kwargs.as_slice(), |c| c.kwargs.as_slice());
        let receiver = call
            .receiver
            .as_deref()
            .or_else(|| stored.and_then(|c| c.receiver.as_ref()));
        let receiver = match receiver {
            Some(receiver) => Some(Box::new(
                self.eval(path, scope, receiver, frames, depth, fold, wants),
            )),
            None if stored.is_some() => call.callee.rsplit_once('.').map(|(head, _)| {
                Box::new(self.name(path, scope, head, frames, depth, fold, wants))
            }),
            None => None,
        };
        let mut eval = |v| self.eval(path, scope, v, frames, depth, fold, wants);
        Value::Call(Invocation {
            callee: call.callee.clone(),
            receiver,
            args: args.iter().map(&mut eval).collect(),
            kwargs: (kwargs.iter()).map(|(k, v)| (k.clone(), eval(v))).collect(),
            range: call.range.clone(),
        })
    }

    #[expect(clippy::too_many_arguments)]
    fn eval_member(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        member: &'a str,
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        let object = self.name(path, scope, name, frames, depth, fold, wants);
        match object {
            Value::Object(fields) => crate::lang::property(&fields, member)
                .ok()
                .flatten()
                .cloned()
                .unwrap_or(Value::Unknown),
            _ => Value::Unknown,
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn argument_value(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        let object = fold
            .selected
            .and_then(|_| object_value(value))
            .map(|pointer| (mutations::Object::Value(path, pointer), pointer));
        if let Some(object) = object {
            fold.objects.push(object);
        }
        let result = self.eval(path, scope, value, frames, depth, fold, wants);
        if object.is_some() {
            fold.objects.pop();
        }
        result
    }

    fn py_decorated(&self, function: Target<'a>) -> bool {
        let [decorated, blocked] = self.py_rebinding.get_or_init(|| {
            let rebinds = |name: &str| !PY_KEEPS_BODY.contains(&name);
            let mut decorated = HashSet::new();
            for (&path, facts) in self.resolver.files {
                let calls = facts.calls.iter().filter(|c| rebinds(&c.callee));
                let names = calls.filter_map(|c| c.decorates.as_deref());
                decorated.extend(names.map(|name| Target { path, name }));
                let defs = facts
                    .defs
                    .iter()
                    .filter(|d| d.decorators.iter().any(|d| rebinds(d)));
                decorated.extend(defs.map(|d| Target {
                    path,
                    name: d.name.as_str(),
                }));
            }
            let mut blocked = HashSet::new();
            for &class in &decorated {
                blocked.insert(class);
                let family = self.resolver.families.get(&class).and_then(Option::as_ref);
                blocked.extend(family.into_iter().flatten().filter_map(|a| match a {
                    Ancestor::Class(c) => Some(*c),
                    Ancestor::Opaque(_) => None,
                }));
            }
            [decorated, blocked]
        });
        let class = crate::parent(function.name);
        decorated.contains(&function)
            || (!class.is_empty()
                && blocked.contains(&Target {
                    path: function.path,
                    name: class,
                }))
    }

    #[expect(clippy::too_many_arguments)]
    fn built(
        &self,
        path: &'a str,
        range: &std::ops::Range<usize>,
        awaited: bool,
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Option<Value> {
        let extended = self.fold_returned_strings.get();
        let javascript = self.pack.extensions().contains(&"ts");
        if !extended && !javascript {
            return None;
        }
        let index = self.call_index(path, range)?;
        let site = (path, index);
        if fold.returning.len() >= WRAPPER_DEPTH || fold.returning.contains(&site) {
            return None;
        }
        let stored = &self.resolver.files[path].calls[index];
        let resolution = if extended {
            self.resolver.resolved_at(path, index)
        } else {
            self.resolver.call(path, stored)
        };
        let Resolution::Exact(function) = resolution else {
            if !extended {
                return None;
            }
            let read = fold.external_value?;
            let call = self.call(path, index)?;
            let mut eval = |i| {
                call.args.get(i).map_or(Value::Unknown, |value| {
                    self.eval(path, &call.scope, value, frames, depth, fold, wants)
                })
            };
            return read(call, &mut eval);
        };
        let def = self.def(function.path, function.name)?;
        let (first, rest) = def.return_values.split_first()?;
        let same_method = extended && javascript && self.args(path, index).is_some_and(<[_]>::is_empty)
            && stored.callee.strip_prefix("this.").is_some_and(|member| {
                matches!(self.resolver.class(path, &stored.scope), Binding::Def(class)
                    if class.path == function.path && class.name == crate::parent(function.name)
                        && member == own_name(function.name) && !self.resolver.ts_override(class, member))
            });
        if extended {
            if stored.constructs
                || !(same_method || self.runs_alone(function))
                || (def.asynchronous && !(awaited || stored.awaited))
                || (javascript && !def.terminal_return)
                || (self.pack.extensions().contains(&"py") && self.py_decorated(function))
            {
                return None;
            }
        } else if !(def.single_return && matches!(first, Value::Str(_)) && rest.is_empty()) {
            return None;
        }
        let mut bound = vec![Frame {
            def: function,
            path,
            index,
        }];
        bound.extend_from_slice(frames.get(depth..).unwrap_or_default());
        if !extended {
            let value = self.eval(function.path, function.name, first, &bound, 0, fold, wants);
            return matches!(&value, Value::Str(parts) if parts.iter().all(|p| {
                matches!(p, Part::Text(_) | Part::Ref(_))
            }))
            .then_some(value);
        }
        let string = |value: &Value| match value {
            Value::Str(parts) => parts.iter().all(|p| {
                matches!(
                    p,
                    Part::Text(_)
                        | Part::Ref(_)
                        | Part::Env(_)
                        | Part::EnvOr(..)
                        | Part::EnvKey { .. }
                        | Part::StripStart
                        | Part::StripEnd(_)
                        | Part::QuerySuffix
                        | Part::Read { .. }
                )
            }),
            Value::Ref(_) => true,
            _ => false,
        };
        let mut pending = None;
        let independent = fold.independent;
        if same_method {
            fold.independent = Some(function);
        }
        fold.returning.push(site);
        let mut returned = |value| {
            self.eval(
                function.path,
                function.name,
                value,
                &bound,
                0,
                fold,
                &mut pending,
            )
        };
        let value = returned(first);
        let agrees = string(&value)
            && rest.iter().all(|other| match (&value, returned(other)) {
                (Value::Str(first), Value::Str(other)) => *first == other,
                (Value::Ref(first), Value::Ref(other)) => *first == other,
                _ => false,
            });
        fold.returning.pop();
        fold.independent = independent;
        if agrees {
            if let Some(pending) = pending {
                wants.get_or_insert(pending);
            }
            Some(value)
        } else {
            None
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn name(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        frames: &[Frame<'a>],
        depth: usize,
        fold: &mut Fold<'a, '_>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        if self.resolver.ts
            && let Some(field) = name.strip_prefix("this.")
            && let Some(value) = self.ts_field_argument(path, scope, field, fold)
        {
            return value;
        }
        let (binding, local) = match fold.record_site {
            Some((file, at)) if file == path => {
                let Some(facts) = self.full(path) else {
                    return Value::Unknown;
                };
                self.resolver.record_binding(path, scope, name, at, facts)
            }
            Some(_) => return Value::Unknown,
            None => (self.resolver.read(path, scope, name), None),
        };
        match binding {
            Binding::Const(at, constant) => {
                if fold.held.contains(&(at, constant)) || self.resolver.redeclared(at, constant) {
                    return Value::Unknown;
                }
                let local = if fold.record_site.is_some() && at != path {
                    let Some(facts) = self.full(at) else {
                        return Value::Unknown;
                    };
                    match facts
                        .ts_locals
                        .get(constant)
                        .or_else(|| facts.ts_record_locals.get(constant))
                        .map(Vec::as_slice)
                    {
                        Some([binding]) => Some(binding),
                        Some(_) => return Value::Unknown,
                        None => None,
                    }
                } else {
                    local
                };
                let held = match local {
                    Some(binding) => binding
                        .constant
                        .and_then(|i| self.full(at)?.consts.get(i))
                        .filter(|c| c.name == constant)
                        .map(|c| (&c.value, scope_of(constant))),
                    None => self.held(at, constant),
                };
                let Some((value, within)) = held else {
                    return Value::Unknown;
                };
                let foreign = usize::from(within != scope_of(constant));
                fold.held.push((at, constant));
                let object = fold
                    .selected
                    .and_then(|_| object_value(value))
                    .map(|pointer| (mutations::Object::Constant(at, constant), pointer));
                if let Some(object) = object {
                    fold.objects.push(object);
                }
                fold.foreign += foreign;
                let previous = fold.record_site;
                if let Some(binding) = local {
                    fold.record_site = Some((at, binding.declared.saturating_sub(1)));
                }
                let folded = self.eval(at, within, value, frames, depth, fold, wants);
                fold.record_site = previous;
                fold.foreign -= foreign;
                fold.held.pop();
                if object.is_some() {
                    fold.objects.pop();
                }
                folded
            }
            Binding::Param(function, _, _) if fold.independent == Some(function) => Value::Unknown,
            Binding::Param(function, _, _)
                if !self.follow_request_parameters.get()
                    && !self.unchanged_parameter(function, name) =>
            {
                Value::Unknown
            }
            Binding::Param(function, n, _)
                if self.resolver.defs[function.path][function.name].params[n].binds
                    == Binds::Rest =>
            {
                Value::Unknown
            }
            Binding::Param(function, n, field) => match frames.get(depth) {
                Some(frame) if frame.def == function => {
                    let Some(call) = self.resolver.files[frame.path].calls.get(frame.index) else {
                        return Value::Unknown;
                    };
                    if self.args(frame.path, frame.index).is_none() {
                        return Value::Unknown;
                    }
                    let arg = self.arg(frame.path, frame.index, function, n);
                    let param = self.param(function, n);
                    let bound = match (arg, param.and_then(|p| p.default.as_ref())) {
                        (Some(arg), _) => {
                            let previous = fold.record_site;
                            if previous.is_some() {
                                fold.record_site = Some((frame.path, call.range.start));
                            }
                            let value = self.argument_value(
                                frame.path,
                                &call.scope,
                                arg,
                                frames,
                                depth + 1,
                                fold,
                                wants,
                            );
                            fold.record_site = previous;
                            value
                        }
                        (None, Some(default)) => self.argument_value(
                            function.path,
                            function.name,
                            default,
                            frames,
                            depth,
                            fold,
                            wants,
                        ),
                        (None, None) if field.is_some() => return Value::Unknown,
                        (None, None) => return Value::Undefined,
                    };
                    let Some(field) = field.and_then(|f| param?.fields.get(f)) else {
                        return bound;
                    };
                    let Value::Object(properties) = bound else {
                        return Value::Unknown;
                    };
                    let mut read = None;
                    for (key, value) in properties {
                        if key == "..." {
                            read = Some(Value::Unknown);
                        } else if key == field.key {
                            read = Some(value);
                        }
                    }
                    match (read, field.default.as_ref()) {
                        (Some(Value::Undefined) | None, Some(default)) => self.eval(
                            function.path,
                            function.name,
                            default,
                            frames,
                            depth,
                            fold,
                            wants,
                        ),
                        (Some(read), _) => read,
                        (None, None) => Value::Undefined,
                    }
                }
                frame => {
                    let called = self.may_be_called(function);
                    let marks = |on: &Marked| matches!(on, Marked::Param(f, at) if f == function.name && *at == n);
                    if let (false, None, Some(inject)) = (called, field, fold.inject)
                        && let Some(value) = (self.injected(function.path, marks, inject))
                            .or_else(|| inject(self, function.path, Injected::Param(function, n)))
                    {
                        return value;
                    }
                    // ponytail: a parameter of an outer function, captured by a closure, is
                    // unknown; bind it when closures that escape their function matter. So is
                    // a constructor's parameter a field holds: it is no wrapper's.
                    if frame.is_some() || fold.foreign > 0 {
                        return Value::Unknown;
                    }
                    wants.get_or_insert(function);
                    Value::Ref(name.to_string())
                }
            },
            Binding::Hidden | Binding::Local(..)
                if let Some(bound) = self.local(path, scope, name) =>
            {
                let held = (bound.within.as_str(), bound.name.as_str());
                if fold.held.contains(&held) {
                    return Value::Unknown;
                }
                fold.held.push(held);
                let object =
                    fold.selected
                        .and_then(|_| object_value(&bound.value))
                        .map(|pointer| {
                            (
                                mutations::Object::Local(path, &bound.scope, &bound.name),
                                pointer,
                            )
                        });
                if let Some(object) = object {
                    fold.objects.push(object);
                }
                let folded = self.eval(
                    path,
                    &bound.within,
                    &bound.value,
                    frames,
                    depth,
                    fold,
                    wants,
                );
                fold.held.pop();
                if object.is_some() {
                    fold.objects.pop();
                }
                folded
            }
            Binding::Local(at, class, field)
                if !self.resolver.written_in.contains(&(class, field))
                    && let Some(var) = (self.resolver.files[at].env_fields.iter())
                        .find(|e| e.class == class && e.field == field) =>
            {
                Value::Str(vec![Part::Env(var.var.clone())])
            }
            Binding::Local(at, class, field)
                if !self.field_written_in(class, field, path.ends_with("_test.go")) =>
            {
                let marks = |on: &Marked| matches!(on, Marked::Field { class: c, name, .. } if c == class && name == field);
                let injected = fold.inject.and_then(|i| self.injected(at, marks, i));
                let field = (at, class, field);
                let test_read = crate::http::is_test_file(path);
                injected.unwrap_or_else(|| {
                    self.field_read(field, fold.field_depth, fold.nested, test_read)
                })
            }
            Binding::External(..) | Binding::Unknown if fold.chain => {
                let hidden_head = self.resolver.unordered_globs
                    && name.split_once('.').is_some_and(|(head, _)| {
                        matches!(
                            self.resolver.lexical(
                                path,
                                scope,
                                head,
                                &mut self.resolver.seen(path, scope)
                            ),
                            Binding::Param(..)
                                | Binding::Local(..)
                                | Binding::Const(..)
                                | Binding::Hidden
                        )
                    });
                match self.outside(path, scope, name, false) {
                    Some(o) if !o.candidates.is_empty() && !hidden_head => {
                        Value::External(o.candidates)
                    }
                    Some(o) if o.direct => Value::External(vec![o.spelled]),
                    _ => Value::Unknown,
                }
            }
            _ => Value::Unknown,
        }
    }
}

fn unwritten(key: &str, value: Value, changed: &dyn Fn(&str) -> bool) -> Value {
    match (key, value) {
        ("[]", Value::List(mut pair)) => {
            if let [Value::Str(key), value] = pair.as_mut_slice()
                && crate::lang::literal(key).is_some_and(|key| changed(&key))
            {
                *value = Value::Unknown;
            }
            Value::List(pair)
        }
        ("...", Value::Object(fields)) => Value::Object(
            (fields.into_iter())
                .map(|(field, value)| {
                    let value = match field.as_str() {
                        "[]" | "..." => unwritten(&field, value, changed),
                        named if changed(named) => Value::Unknown,
                        _ => value,
                    };
                    (field, value)
                })
                .collect(),
        ),
        (_, value) => value,
    }
}

fn object_value(value: &Value) -> Option<usize> {
    match value {
        Value::Object(_) => Some(value as *const Value as usize),
        Value::New(_, args) if args.len() == 1 => object_value(&args[0]),
        _ => None,
    }
}

fn unbound((values, kwargs): Folded, chain: bool, keep: &[&str]) -> Folded {
    let values = values.into_iter().map(|v| unbind(v, chain, keep));
    let kwargs = (kwargs.into_iter()).map(|(k, v)| (k, unbind(v, chain, keep)));
    (values.collect(), kwargs.collect())
}

fn unbind(value: Value, chain: bool, keep: &[&str]) -> Value {
    let unbind = |v| unbind(v, chain, keep);
    match value {
        Value::Ref(name) if !keep.contains(&name.as_str()) => Value::Unknown,
        Value::Str(parts) => Value::Str(
            parts
                .into_iter()
                .map(|p| {
                    if let Part::RecordValues {
                        key: Some(name),
                        values,
                    } = &p
                        && !keep.contains(&name.as_str())
                    {
                        return Part::RecordValues {
                            key: None,
                            values: values.clone(),
                        };
                    }
                    if let Some(name) = p.parameter().filter(|name| !keep.contains(name)) {
                        Part::Limit(crate::lang::Limit::Parameter(name.to_owned()))
                    } else {
                        p
                    }
                })
                .collect(),
        ),
        Value::Object(properties) => Value::Object(
            properties
                .into_iter()
                .map(|(k, v)| (k, unbind(v)))
                .collect(),
        ),
        Value::List(items) => Value::List(items.into_iter().map(unbind).collect()),
        Value::New(callee, args) => Value::New(callee, args.into_iter().map(unbind).collect()),
        Value::Call(call) if chain => Value::Call(Invocation {
            receiver: call.receiver.map(|r| Box::new(unbind(*r))),
            args: call.args.into_iter().map(unbind).collect(),
            kwargs: (call.kwargs.into_iter())
                .map(|(k, v)| (k, unbind(v)))
                .collect(),
            ..call
        }),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::python::Python;

    fn chained(caller: &str, whole: bool) -> Vec<(String, Option<Vec<String>>, Option<String>)> {
        use crate::lang::java::Java;
        let w = "package a;\nimport org.lib.Mode;\nclass W {\n\
                 void wrap(Object m, String p) { Lib.start().with(m, \"/\" + p).go(); }\n\
                 void calls() { wrap(Mode.FAST, \"x\"); }\n}\n";
        let sources = [("a/W.java", w), ("a/X.java", caller)];
        let parsed: Vec<(&str, Facts)> = (sources.iter())
            .map(|(p, src)| (*p, Java.parse(p, src.as_bytes()).unwrap()))
            .collect();
        let files: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        let n = (files["a/W.java"].calls.iter())
            .position(|c| c.callee.ends_with(".go"))
            .unwrap();
        let read = |values: &[Value], _: &Keywords| {
            let args = match values {
                [Value::Call(go)] => match go.receiver.as_deref() {
                    Some(Value::Call(with)) => with.args.clone(),
                    _ => return Some((None, None)),
                },
                _ => return Some((None, None)),
            };
            if args.iter().any(|a| matches!(a, Value::Ref(_))) {
                return None;
            }
            let mode = match &args[0] {
                Value::External(spellings) => Some(spellings.clone()),
                _ => None,
            };
            let path = match &args[1] {
                Value::Str(parts) => crate::lang::literal(parts),
                _ => None,
            };
            Some((mode, path))
        };
        let from = match whole {
            true => Expand::Chain("a/W.java", n),
            false => Expand::Call("a/W.java", n),
        };
        let mut found: Vec<_> = (code.expand(from, &read).into_iter())
            .map(|e| (e.site.0.to_string(), e.found.0, e.found.1))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_kotlin_chain_of_calls_past_the_receiver_depth_resolves_each_call_once() {
        use crate::lang::kotlin::Kotlin;
        let calls = ".m()".repeat(RECEIVER_DEPTH + 16);
        let src = format!("package a\nimport b.outside\nfun f() {{ outside(){calls} }}\n");
        let facts = Kotlin.parse("a/C.kt", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a/C.kt", &facts)]);
        let layout = Kotlin.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Kotlin);
        let chained = facts.calls.iter().filter(|c| c.on.is_some()).count();
        assert!(chained > RECEIVER_DEPTH, "{chained} calls on calls");
        for call in &facts.calls {
            assert!(!matches!(
                resolver.call("a/C.kt", call),
                Resolution::Exact(_)
            ));
        }
    }

    #[test]
    fn a_chain_of_calls_past_the_receiver_depth_resolves_each_call_once() {
        use crate::lang::java::Java;
        let calls = ".m()".repeat(RECEIVER_DEPTH + 16);
        let src = format!(
            "package a;\nimport b.Outside;\nclass C {{ void f() {{ Outside.start(){calls}; }} }}\n"
        );
        let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a/C.java", &facts)]);
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let chained = facts.calls.iter().filter(|c| c.on.is_some()).count();
        assert!(chained > RECEIVER_DEPTH, "{chained} calls on calls");
        for call in &facts.calls {
            assert!(!matches!(
                resolver.call("a/C.java", call),
                Resolution::Exact(_)
            ));
        }
    }

    #[test]
    fn a_chain_of_locals_on_call_results_resolves_each_call_once() {
        use crate::lang::java::Java;
        let n = RECEIVER_DEPTH + 16;
        let locals: String = (1..=n)
            .map(|i| format!("var a{i} = a{}.m(); ", i - 1))
            .collect();
        let src = format!(
            "package a;\nimport b.Outside;\nclass C {{ void f(Outside a0) {{ {locals}a{n}.m(); }} }}\n"
        );
        let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a/C.java", &facts)]);
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        assert!(
            facts.calls.len() > RECEIVER_DEPTH,
            "{} calls",
            facts.calls.len()
        );
        let start = std::time::Instant::now();
        for call in &facts.calls {
            assert!(!matches!(
                resolver.call("a/C.java", call),
                Resolution::Exact(_)
            ));
        }
        let took = start.elapsed();
        assert!(took.as_secs() < 10, "{took:?}");
    }

    #[test]
    fn a_chain_read_whole_folds_its_calls_and_spells_an_outside_name_where_it_is_written() {
        let hidden = "package a;\nclass X { enum Mode { FAST }\n\
                      void go(W w) { w.wrap(Mode.FAST, \"y\"); } }\n";
        let fast = Some(vec!["org.lib.Mode.FAST".to_string()]);
        assert_eq!(
            chained(hidden, true),
            [
                ("a/W.java".into(), fast, Some("/x".into())),
                ("a/X.java".into(), None, Some("/y".into())),
            ]
        );
        assert_eq!(chained(hidden, false), [("a/W.java".into(), None, None)]);
    }

    #[test]
    fn an_origin_followed_through_methods_past_the_limit_gives_nothing() {
        use crate::lang::go::Go;
        let origin = |n: usize| {
            let mut src =
                String::from("package x\nimport amqp \"github.com/rabbitmq/amqp091-go\"\n");
            src += &format!("func f() {{\n\tx{n}, _ := amqp.Dial(\"u\")\n");
            for i in (0..n).rev() {
                src += &format!("\tx{i}, _ := x{}.M()\n", i + 1);
            }
            src += "}\n";
            let facts = Go.parse("x.go", src.as_bytes()).unwrap();
            let files = BTreeMap::from([("x.go", &facts)]);
            let modules = Go.modules(&[], &BTreeMap::new()).unwrap();
            let resolver = Resolver::new(&files, modules, &Go);
            let code = Code::new(&resolver, &files, &Go);
            let found = code.origins("x.go", "f", Of::Name("x0"), Follow::Declarations);
            let origin = found.into_iter().next()?;
            let steps = origin.routes[0].steps.iter().map(|s| format!(".{s}"));
            Some(origin.outside?.spelled + &steps.collect::<String>())
        };
        assert!(origin(3).unwrap().ends_with("Dial.M.M.M"));
        assert!(origin(RECEIVER_DEPTH - 1).is_some());
        assert_eq!(origin(RECEIVER_DEPTH), None);
    }

    #[test]
    fn the_build_pass_answers_every_call_as_call_does() {
        use crate::lang::rust::Rust;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/conformance-rust");
        let (mut sources, mut configs) = (Vec::new(), Vec::new());
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let rel = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                match path.extension().and_then(|e| e.to_str()) {
                    _ if path.is_dir() => dirs.push(path),
                    Some("rs") => sources.push((rel, std::fs::read(&path).unwrap())),
                    _ if rel.ends_with("Cargo.toml") => {
                        configs.push((rel, std::fs::read(&path).unwrap()))
                    }
                    _ => {}
                }
            }
        }
        let parsed: Vec<(String, Facts)> = (sources.iter())
            .map(|(rel, src)| (rel.clone(), Rust.parse(rel, src).unwrap()))
            .collect();
        let configs: Vec<(&str, &[u8])> = (configs.iter())
            .map(|(p, c)| (p.as_str(), c.as_slice()))
            .collect();
        let mut files: BTreeMap<&str, &Facts> =
            (parsed.iter()).map(|(p, f)| (p.as_str(), f)).collect();
        let layout = Rust.layout(&configs, &files).unwrap();
        if let Some(members) = &layout.members {
            files.retain(|path, _| members.contains(*path));
        }
        let resolver = Resolver::with_layout(&files, layout, &Rust);
        let answer = |r: Resolution| match r {
            Resolution::Exact(t) => format!("exact {}::{}", t.path, t.name),
            Resolution::Inferred(t) => format!("inferred {}::{}", t.path, t.name),
            Resolution::Narrowed(t) => format!("narrowed {}::{}", t.path, t.name),
            Resolution::Ambiguous(ts) => (ts.iter())
                .map(|t| format!("{}::{}", t.path, t.name))
                .fold("ambiguous".to_string(), |all, t| all + " " + &t),
            Resolution::Unresolved => "unresolved".to_string(),
        };
        let mut calls = 0;
        for (&path, facts) in &files {
            for (index, call) in facts.calls.iter().enumerate() {
                let built = answer(resolver.resolved_at(path, index));
                assert_eq!(built, answer(resolver.call(path, call)), "{path} #{index}");
                calls += 1;
            }
        }
        assert!(calls > 100, "{calls} calls");
    }

    fn chain(n: usize, reverse: bool) -> String {
        let mut classes = vec!["class C0:\n    def m(self):\n        pass\n".to_string()];
        for i in 1..n {
            let p = i - 1;
            classes.push(format!(
                "class C{i}(C{p}):\n    def go(self):\n        self.m()\n"
            ));
        }
        if reverse {
            classes.reverse();
        }
        classes.join("\n\n")
    }

    fn reexports(
        ext: &str,
        n: usize,
        define: &str,
        link: impl Fn(usize) -> String,
    ) -> Vec<(String, String)> {
        let mut sources = vec![(format!("f0.{ext}"), define.to_string())];
        sources.extend((1..n).map(|i| (format!("f{i}.{ext}"), link(i - 1))));
        sources
    }

    #[test]
    fn an_import_through_export_star_binds_the_literal() {
        use crate::lang::ts::TsJs;
        let source = TsJs
            .parse("service.ts", b"export const svc = { go() {} };")
            .unwrap();
        let barrel = TsJs
            .parse("barrel.ts", b"export * from './service';")
            .unwrap();
        let main = TsJs.parse("main.ts", b"import { svc as direct } from './service'; import { svc as shared } from './barrel';").unwrap();
        let files = BTreeMap::from([
            ("main.ts", &main),
            ("barrel.ts", &barrel),
            ("service.ts", &source),
        ]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        for name in ["direct", "shared"] {
            let found = resolver.read_member("main.ts", "", name);
            assert!(
                matches!(found, Binding::Const("service.ts", "svc")),
                "{name}"
            );
        }
    }

    fn exact_in(
        pack: &dyn LanguagePack,
        sources: &[(String, String)],
        main: &str,
        callee: &str,
    ) -> Option<String> {
        let facts: Vec<(String, Facts)> = (sources.iter())
            .map(|(path, src)| (path.clone(), pack.parse(path, src.as_bytes()).unwrap()))
            .collect();
        let files: BTreeMap<&str, &Facts> = (facts.iter()).map(|(p, f)| (p.as_str(), f)).collect();
        let resolver = Resolver::new(&files, pack.modules(&[], &BTreeMap::new()).unwrap(), pack);
        let call = files[main]
            .calls
            .iter()
            .find(|c| c.callee == callee)
            .unwrap();
        match resolver.call(main, call) {
            Resolution::Exact(t) => Some(t.path.to_string()),
            _ => None,
        }
    }

    fn reexported(
        pack: &dyn LanguagePack,
        ext: &str,
        n: usize,
        define: &str,
        link: impl Fn(usize) -> String,
        import: impl Fn(usize) -> String,
    ) -> bool {
        let mut sources = reexports(ext, n, define, link);
        let main = format!("main.{ext}");
        sources.push((main.clone(), import(n - 1)));
        exact_in(pack, &sources, &main, "x") == Some(format!("f0.{ext}"))
    }

    #[test]
    fn a_name_reexported_deeper_than_the_limit_is_unknown() {
        use crate::lang::ts::TsJs;

        let far = 5_000;
        let ts = |n| {
            let link = |p: usize| match p % 2 {
                0 => format!("export {{ x }} from './f{p}';\n"),
                _ => format!("export * from './f{p}';\n"),
            };
            let import = |p| format!("import {{ x }} from './f{p}';\nx();\n");
            reexported(&TsJs, "ts", n, "export function x() {}\n", link, import)
        };
        let py = |n| {
            let link = |p: usize| match p % 2 {
                0 => format!("from f{p} import x\n"),
                _ => format!("from f{p} import *\n"),
            };
            let import = |p| format!("from f{p} import x\nx()\n");
            reexported(&Python, "py", n, "def x():\n    pass\n", link, import)
        };

        for exact in [ts, py] {
            assert!(exact(LINK_DEPTH));
            assert!(!exact(LINK_DEPTH + 1));
            assert!(!exact(far));
        }
    }

    #[test]
    fn a_name_cut_past_the_limit_is_taken_from_no_other_module() {
        use crate::lang::ts::TsJs;

        let cut = LINK_DEPTH + 1;
        let py_link = |p| format!("from f{p} import x\n");
        let py = |n: usize| {
            let mut sources = reexports("py", n, "def x():\n    pass\n", py_link);
            sources.extend([
                ("a.py".into(), py_link(n - 1)),
                ("b.py".into(), "def x():\n    pass\n".into()),
                (
                    "main.py".into(),
                    "from b import *\nfrom a import *\nx()\n".into(),
                ),
            ]);
            exact_in(&Python, &sources, "main.py", "x")
        };
        assert_eq!(py(2).as_deref(), Some("f0.py"));
        assert_eq!(py(cut), None);

        let ts = |n: usize| {
            let link = |p| format!("export {{ x }} from './f{p}';\n");
            let mut sources = reexports("ts", n, "export function x() {}\n", link);
            sources.extend([
                ("a.ts".into(), link(n - 1)),
                ("b.ts".into(), link(0)),
                (
                    "m.ts".into(),
                    "export * from './a';\nexport * from './b';\n".into(),
                ),
                ("main.ts".into(), "import { x } from './m';\nx();\n".into()),
            ]);
            exact_in(&TsJs, &sources, "main.ts", "x")
        };
        assert_eq!(ts(2).as_deref(), Some("f0.ts"));
        assert_eq!(ts(cut), None);

        let member = |init: String| {
            let mut sources = reexports("py", cut, "def x():\n    pass\n", py_link);
            sources.extend([
                ("pkg/__init__.py".into(), init),
                ("pkg/x.py".into(), "def y():\n    pass\n".into()),
                ("main.py".into(), "from pkg import x\nx.y()\n".into()),
            ]);
            exact_in(&Python, &sources, "main.py", "x.y")
        };
        assert_eq!(member(String::new()).as_deref(), Some("pkg/x.py"));
        assert_eq!(member(py_link(cut - 1)), None);
    }

    #[test]
    fn a_star_that_binds_a_name_to_no_function_hides_the_stars_before_it() {
        use crate::lang::ts::TsJs;

        let src = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            (pairs.iter())
                .map(|(p, s)| (p.to_string(), s.to_string()))
                .collect()
        };
        for a in ["x = 1\n", "from os import x\n", "x = f()\nx = g()\n"] {
            let b = "def x():\n    pass\n";
            let imported = src(&[
                ("a.py", a),
                ("b.py", b),
                ("main.py", "from b import *\nfrom a import *\nx()\n"),
            ]);
            assert_eq!(exact_in(&Python, &imported, "main.py", "x"), None, "{a}");
            let exported = src(&[
                ("a.py", a),
                ("b.py", b),
                ("m.py", "from b import *\nfrom a import *\n"),
                ("main.py", "from m import x\nx()\n"),
            ]);
            assert_eq!(exact_in(&Python, &exported, "main.py", "x"), None, "{a}");
        }
        for a in ["export const x = 1;\n", "export { x } from 'ext';\n"] {
            let sources = src(&[
                ("a.ts", a),
                ("b.ts", "export function x() {}\n"),
                ("m.ts", "export * from './a';\nexport * from './b';\n"),
                ("main.ts", "import { x } from './m';\nx();\n"),
            ]);
            assert_eq!(exact_in(&TsJs, &sources, "main.ts", "x"), None, "{a}");
        }
    }

    #[test]
    fn the_last_star_import_of_a_module_exports_the_name() {
        let def = "def x():\n    pass\n";
        let sources: Vec<(String, String)> = [
            ("a.py", def),
            ("b.py", def),
            ("m.py", "from b import *\nfrom a import *\n"),
            ("main.py", "from m import x\nx()\n"),
        ]
        .map(|(p, s)| (p.to_string(), s.to_string()))
        .into();
        assert_eq!(
            exact_in(&Python, &sources, "main.py", "x").as_deref(),
            Some("a.py")
        );
    }

    fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        (pairs.iter())
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect()
    }

    #[test]
    fn a_star_import_from_outside_the_repository_binds_nothing() {
        let b = ("b.py", "def x():\n    pass\n");
        for (main, before) in [
            ("from b import *\nfrom numpy import *\nx()\n", "b.py"),
            ("from b import x\nfrom numpy import *\nx()\n", "b.py"),
            ("def x():\n    pass\nfrom numpy import *\nx()\n", "main.py"),
        ] {
            let sources = owned(&[b, ("main.py", main)]);
            let found = exact_in(&Python, &sources, "main.py", "x");
            assert_eq!(found.as_deref(), Some(before), "{main}");
        }
        let sources = owned(&[
            b,
            ("m.py", "from b import *\nfrom numpy import *\n"),
            ("main.py", "from m import x\nx()\n"),
        ]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("b.py"));
    }

    #[test]
    fn a_star_import_written_many_times_is_followed_once() {
        let n = 5_000;
        let stars = "from m import *\n".repeat(n);
        let main = format!("def x():\n    pass\n{stars}{}", "x()\n".repeat(n));
        let sources = owned(&[("m.py", "def y():\n    pass\n"), ("main.py", &main)]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("main.py"));
    }

    #[test]
    fn a_file_of_many_names_exports_them_in_linear_time() {
        use crate::lang::ts::TsJs;

        let n = 150_000;
        let exports = (0..n).map(|i| Export::Local {
            exported: format!("a{i}"),
            local: format!("a{i}"),
        });
        let facts = Facts {
            exports: exports.collect(),
            ..Facts::default()
        };
        let files = BTreeMap::from([("a", &facts)]);
        for (pack, label) in [(&TsJs as &dyn LanguagePack, "ts"), (&Python, "py")] {
            let modules = pack.modules(&[], &BTreeMap::new()).unwrap();
            let start = std::time::Instant::now();
            assert_eq!(Resolver::new(&files, modules, pack).exported().len(), n);
            let took = start.elapsed();
            assert!(took.as_secs() < 50, "{label}: {took:?}");
        }
    }

    #[test]
    fn a_file_of_many_declarations_exports_them_in_linear_time() {
        use crate::lang::{Declaration, ts::TsJs};

        let n = 150_000;
        let facts = Facts {
            exports: (0..n)
                .map(|i| Export::Local {
                    exported: format!("a{i}"),
                    local: format!("a{i}"),
                })
                .collect(),
            declarations: (0..n)
                .map(|i| Declaration {
                    name: format!("a{i}"),
                    range: 0..0,
                    hash: String::new(),
                })
                .collect(),
            ..Facts::default()
        };
        let files = BTreeMap::from([("a.ts", &facts)]);
        let modules = TsJs.modules(&[], &BTreeMap::new()).unwrap();
        let start = std::time::Instant::now();
        let exported = Resolver::new(&files, modules, &TsJs).exported();
        let took = start.elapsed();
        assert_eq!(exported.len(), n);
        assert!(exported.iter().all(|e| e.target == e.name));
        assert!(took.as_secs() < 30, "{took:?}");
    }

    fn filler(name: Imported) -> Import {
        Import {
            local: "z".into(),
            source: String::new(),
            name,
        }
    }

    fn module_def(name: &str) -> Def {
        Def::new(name, Kind::Module, 0..0)
    }

    fn timed_rows(facts: Facts, label: &str, secs: u64) -> usize {
        let files = BTreeMap::from([("main.rs", &facts)]);
        let start = std::time::Instant::now();
        let rows = resolver(&files).exported().len();
        let took = start.elapsed();
        assert!(took.as_secs() < secs, "{label}: {took:?}");
        rows
    }

    #[test]
    fn a_file_of_many_imports_looks_each_name_up_in_linear_time() {
        let imports = (0..300_000).map(|i| {
            filler(match i % 3 {
                0 => Imported::Namespace,
                1 => Imported::Path(String::new()),
                _ => Imported::Glob {
                    path: String::new(),
                    inside: false,
                    public: false,
                },
            })
        });
        let n = 40_000;
        let facts = Facts {
            imports: imports.collect(),
            externs: (0..300_000).map(|i| format!("e{i}")).collect(),
            exports: (0..n)
                .map(|i| Export::Local {
                    exported: format!("u{i}"),
                    local: format!("u{i}"),
                })
                .collect(),
            ..Facts::default()
        };
        assert_eq!(timed_rows(facts, "imports", 20), n);
    }

    #[test]
    fn a_file_of_many_imports_finds_each_module_in_linear_time() {
        let n = 40_000;
        let mut facts = Facts::default();
        for i in 0..n {
            facts.defs.push(module_def(&format!("m{i}")));
            facts.imports.push(Import {
                local: format!("m{i}"),
                source: format!("mod m{i}"),
                name: Imported::Namespace,
            });
        }
        for i in 0..n {
            facts.imports.push(Import {
                local: format!("a{i}"),
                source: String::new(),
                name: Imported::Path(format!("m{i}.f")),
            });
            facts.exports.push(Export::Local {
                exported: format!("a{i}"),
                local: format!("a{i}"),
            });
        }
        let fillers = (0..300_000).map(|_| filler(Imported::Namespace));
        facts.imports.extend(fillers);
        assert_eq!(timed_rows(facts, "modules", 30), n);
    }

    #[test]
    fn a_file_of_many_imports_resolves_each_external_call_in_linear_time() {
        let (n, fillers) = (60_000, 500_000);
        let names: Vec<String> = (0..n).map(|i| format!("a{i}")).collect();
        let named = names.iter().map(|name| Import {
            local: name.clone(),
            source: "ext".into(),
            name: Imported::Name(name.clone()),
        });
        let facts = Facts {
            imports: (named.chain((0..fillers).map(|_| filler(Imported::Namespace)))).collect(),
            ..Facts::default()
        };
        let files = BTreeMap::from([("a.py", &facts)]);
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Python);
        let code = Code::new(&resolver, &files, &Python);
        let start = std::time::Instant::now();
        for name in &names {
            assert_eq!(code.imported("a.py", "", name), Some(("ext", name.clone())));
            let outside = code.outside("a.py", "", name, false);
            assert_eq!(outside.map(|o| o.spelled), Some(format!("ext.{name}")));
        }
        let took = start.elapsed();
        assert!(took.as_secs() < 45, "{took:?}");
    }

    #[test]
    fn a_file_of_many_all_names_takes_each_export_in_linear_time() {
        let (n, listed) = (30_000, 500_000);
        let m = Facts {
            exports: (0..n)
                .map(|i| Export::Local {
                    exported: format!("p{i}"),
                    local: format!("p{i}"),
                })
                .collect(),
            private: (0..n).map(|i| format!("p{i}")).collect(),
            star: Star::Only((0..listed).map(|i| format!("n{i}")).collect()),
            ..Facts::default()
        };
        let all = |name: Imported| Import {
            local: "*".into(),
            source: "m".into(),
            name,
        };
        let main = Facts {
            imports: vec![all(Imported::All)],
            exports: std::iter::once(Export::All { source: "m".into() })
                .chain((0..n).map(|i| Export::Local {
                    exported: format!("x{i}"),
                    local: format!("x{i}"),
                }))
                .collect(),
            ..Facts::default()
        };
        let lib = Facts {
            imports: vec![
                Import {
                    local: "m".into(),
                    source: "m".into(),
                    name: Imported::Namespace,
                },
                all(Imported::Glob {
                    path: "m".into(),
                    inside: false,
                    public: true,
                }),
            ],
            ..Facts::default()
        };
        let files = BTreeMap::from([("m.py", &m), ("main.py", &main), ("lib.py", &lib)]);
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let start = std::time::Instant::now();
        let rows = Resolver::new(&files, modules, &Python).exported();
        let took = start.elapsed();
        assert_eq!(rows.len(), n + 1);
        assert!(took.as_secs() < 40, "{took:?}");
    }

    #[test]
    fn labelled_parameters_take_the_labels_of_a_call_in_order() {
        let labelled_param = |label: &str, binds| Param {
            label: Some(label.into()),
            binds,
            ..param(label)
        };
        let (any, optional, rest) = (Binds::Any, Binds::Optional, Binds::Rest);
        type Case<'c> = (&'c [(&'c str, Binds)], &'c [&'c str], bool);
        let cases: &[Case] = &[
            (
                &[("order", any), ("quantity", any)],
                &["order", "quantity"],
                true,
            ),
            (&[("order", any), ("quantity", any)], &["order"], false),
            (
                &[("order", any), ("quantity", any)],
                &["quantity", "order"],
                false,
            ),
            (&[("order", any)], &["_"], false),
            (&[("_", any)], &["_"], true),
            (&[("_", any)], &["order"], false),
            (&[("times", optional), ("_", any)], &[""], true),
            (&[("times", optional), ("_", any)], &["times", ""], true),
            (&[("times", optional), ("_", any)], &["_"], true),
            (&[("times", optional), ("_", any)], &[], false),
            (
                &[("a", any), ("b", optional), ("c", any)],
                &["a", "c"],
                true,
            ),
            (&[("xs", rest)], &["xs", "_", "_"], true),
            (&[("xs", rest)], &[], true),
            (&[("xs", rest), ("y", any)], &["xs", "_", "y"], true),
            (&[("xs", rest)], &["xs", "y"], false),
            (&[], &[], true),
            (&[], &["_"], false),
            (&[("a", optional), ("a", any)], &["a"], true),
            (&[("xs", rest), ("_", any)], &["xs", "_", "_"], true),
        ];
        for (params, labels, fits) in cases {
            let params: Vec<Param> = (params.iter())
                .map(|(label, binds)| labelled_param(label, *binds))
                .collect();
            let labels: Vec<String> = labels.iter().map(|l| l.to_string()).collect();
            assert_eq!(labelled(&params, &labels), *fits, "{labels:?}");
            assert_eq!(
                labelled_args(&params, &labels).is_some(),
                *fits,
                "{labels:?}"
            );
        }
        assert!(labelled(&[param("x")], &["y".to_string()]));
        let params = [
            labelled_param("on", any),
            labelled_param("times", optional),
            labelled_param("_", any),
        ];
        let strings = |labels: &[&str]| labels.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        assert_eq!(
            labelled_args(&params, &strings(&["on", ""])),
            Some(vec![Some(0), None, Some(1)])
        );
        assert_eq!(
            labelled_args(&params, &strings(&["on", "times", "_"])),
            Some(vec![Some(0), Some(1), Some(2)])
        );
        let params = [labelled_param("a", optional), labelled_param("a", any)];
        assert_eq!(
            labelled_args(&params, &strings(&["a"])),
            Some(vec![None, Some(0)])
        );
        let refusing = |label: &str, binds| Param {
            closure: Closure::Refuses,
            ..labelled_param(label, binds)
        };
        let params = [
            labelled_param("_", any),
            refusing("animation", optional),
            labelled_param("assert", optional),
        ];
        assert_eq!(
            labelled_args(&params, &strings(&["_", ""])),
            Some(vec![Some(0), None, Some(1)])
        );
        let params = [labelled_param("_", any), refusing("animation", optional)];
        assert_eq!(labelled_args(&params, &strings(&["_", ""])), None);
        assert!(labelled_args(&params, &strings(&["_"])).is_some());
        let unknown = |label: &str, binds| Param {
            closure: Closure::Unknown,
            ..labelled_param(label, binds)
        };
        let params = [labelled_param("_", any), unknown("completion", any)];
        assert!(fit(&params, &strings(&["_", ""])) == Fit::Maybe);
        assert!(fit(&params, &strings(&["_", "completion"])) == Fit::Yes);
        assert!(fit(&params, &strings(&["_"])) == Fit::No);
        let params = [
            labelled_param("_", any),
            unknown("animation", optional),
            labelled_param("body", any),
        ];
        assert!(fit(&params, &strings(&["_", ""])) == Fit::Yes);
        let params = [labelled_param("_", any), unknown("animation", optional)];
        assert!(fit(&params, &strings(&["_", ""])) == Fit::Maybe);
        assert!(fit(&params, &strings(&["_"])) == Fit::Yes);
        let wide = |n: usize| -> (Vec<Param>, Vec<String>) {
            (
                (0..n).map(|_| labelled_param("a", any)).collect(),
                vec!["a".into(); n],
            )
        };
        let (params, labels) = wide(200);
        assert!(labelled(&params, &labels));
        let (params, labels) = wide(300);
        assert!(!labelled(&params, &labels));
    }

    fn param(name: &str) -> Param {
        Param {
            name: name.into(),
            binds: Binds::Any,
            label: None,
            default: None,
            fields: Vec::new(),
            closure: Default::default(),
        }
    }

    fn function(name: &str, params: Vec<Param>) -> Def {
        Def {
            kind: Kind::Function,
            params,
            ..module_def(name)
        }
    }

    #[test]
    fn a_file_of_many_definitions_reads_each_one_in_linear_time() {
        let (n, fillers) = (40_000, 220_000);
        let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
        let invoked: Vec<Value> = (names.iter().enumerate())
            .map(|(i, name)| {
                Value::Call(crate::lang::Invocation {
                    callee: name.clone(),
                    receiver: None,
                    args: Vec::new(),
                    kwargs: Vec::new(),
                    range: i..i + 1,
                })
            })
            .collect();
        let defined = |name: &str| {
            let x = Param {
                default: Some(Value::Ref("d".into())),
                ..param("x")
            };
            Def {
                return_values: vec![Value::Unknown],
                type_params: vec!["T".into()],
                ..function(name, vec![x])
            }
        };
        let defs = (0..fillers).map(|_| module_def("z"));
        let mut facts = Facts {
            defs: defs.chain(names.iter().map(|name| defined(name))).collect(),
            ..Facts::default()
        };
        facts.calls = (names.iter().enumerate())
            .map(|(i, name)| Call {
                awaited: false,
                arity: None,
                labels: None,
                constructs: false,
                scope: String::new(),
                callee: name.clone(),
                range: i..i + 1,
                args: Vec::new(),
                kwargs: Vec::new(),
                decorates: None,
                by_path: false,
                root: None,
                on: None,
                unwraps: false,
                awaits: false,
                receiver: None,
                arg_types: Vec::new(),
                uses: Vec::new(),
            })
            .collect();
        let files = BTreeMap::from([("a.py", &facts)]);
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Python);
        let code = Code::new(&resolver, &files, &Python);
        let start = std::time::Instant::now();
        for (i, name) in names.iter().enumerate() {
            assert!(code.returns("a.py", name).is_none());
            assert_eq!(code.type_params("a.py", name), ["T"]);
            assert!(code.param_defaults_to("a.py", name, "x", "d"));
            assert_eq!(code.returned("a.py", i).len(), 1);
            let found = code.origins(
                "a.py",
                "",
                Of::Value(&invoked[i]),
                Follow::Values {
                    passes: passes_nothing,
                },
            );
            assert!(found.is_empty());
        }
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn a_file_of_many_constants_reads_each_one_in_linear_time() {
        let (n, fillers) = (40_000, 300_000);
        let names: Vec<String> = (0..n).map(|i| format!("c{i}")).collect();
        let constant = |name: &str| Const {
            name: name.into(),
            value: Value::Unknown,
            ty: None,
            scope: None,
        };
        let consts = (0..fillers).map(|_| constant("z"));
        let facts = Facts {
            consts: consts
                .chain(names.iter().map(|name| constant(name)))
                .collect(),
            ..Facts::default()
        };
        let files = BTreeMap::from([("a.py", &facts)]);
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Python);
        let code = Code::new(&resolver, &files, &Python);
        let start = std::time::Instant::now();
        for name in &names {
            assert!(code.constant("a.py", "", name).is_some());
            assert!(code.constant_type("a.py", name).is_none());
            assert!(code.declared_type("a.py", "", name).is_none());
            assert!(
                code.origins(
                    "a.py",
                    "",
                    Of::Name(name),
                    Follow::Values {
                        passes: passes_nothing
                    },
                )
                .is_empty()
            );
        }
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn a_function_of_many_parameters_binds_each_name_in_linear_time() {
        let (n, fillers) = (40_000, 400_000);
        let names: Vec<String> = (0..n).map(|i| format!("x{i}")).collect();
        let fields = names.iter().map(|name| crate::lang::Field {
            name: name.clone(),
            key: String::new(),
            default: None,
        });
        let destructured = Param {
            fields: fields.collect(),
            ..param("")
        };
        let params = ((0..fillers).map(|_| param("z")))
            .chain([destructured])
            .chain(names.iter().map(|name| param(name)));
        let facts = Facts {
            defs: vec![function("f", params.collect())],
            ..Facts::default()
        };
        let files = BTreeMap::from([("a.py", &facts)]);
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Python);
        let code = Code::new(&resolver, &files, &Python);
        let start = std::time::Instant::now();
        for (i, name) in names.iter().enumerate() {
            let found = resolver.lexical("a.py", "f", name, &mut resolver.seen("a.py", "f"));
            assert!(matches!(
                found,
                Binding::Param(t, at, Some(field)) if t.name == "f" && at == fillers && field == i
            ));
            assert!(code.mutable("a.py", "f", name).is_none());
        }
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn a_package_of_many_files_finds_each_method_and_export_in_linear_time() {
        let n = 30_000;
        let paths: Vec<String> = (0..n).map(|i| format!("p/f{i}.go")).collect();
        let exported: Vec<String> = (0..n).map(|i| format!("e{i}")).collect();
        let facts: Vec<Facts> = (0..n)
            .map(|i| Facts {
                defs: vec![Def {
                    kind: Kind::Method,
                    ..module_def(&format!("T.m{i}"))
                }],
                exports: vec![Export::Local {
                    exported: exported[i].clone(),
                    local: exported[i].clone(),
                }],
                ..Facts::default()
            })
            .collect();
        let files: BTreeMap<&str, &Facts> =
            (paths.iter().map(String::as_str)).zip(&facts).collect();
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::with_shared(&files, modules, vec![paths.clone()], &Python);
        let (first, class) = (
            paths[0].as_str(),
            Target {
                path: &paths[0],
                name: "T",
            },
        );
        let start = std::time::Instant::now();
        for i in 1..n {
            let found = resolver.sibling_method(class, &format!("T.m{i}"));
            assert!(matches!(found, Some(Binding::Def(t)) if t.path == paths[i]));
            let seen = &mut resolver.seen(first, "");
            assert!(resolver.exports(first, &exported[i], seen, false).is_some());
        }
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn an_import_after_a_star_import_from_outside_the_repository_binds_the_name() {
        let b = ("b.py", "def x():\n    pass\n");
        for main in [
            "from numpy import *\nfrom b import *\nx()\n",
            "from numpy import *\nfrom b import x\nx()\n",
        ] {
            let sources = owned(&[b, ("main.py", main)]);
            let found = exact_in(&Python, &sources, "main.py", "x");
            assert_eq!(found.as_deref(), Some("b.py"), "{main}");
        }
    }

    #[test]
    fn a_star_import_after_a_definition_rebinds_its_name() {
        let def = "def x():\n    pass\n";
        let a = ("a.py", def);
        let main = format!("{def}from a import *\ndef f():\n    x()\n");
        let sources = owned(&[a, ("main.py", &main)]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("a.py"));
        for call in [
            "x()\n",
            "class C:\n    x()\n",
            "if __name__ == \"__main__\":\n    x()\n",
        ] {
            for main in [
                format!("{def}{call}from a import *\n"),
                format!("{def}from a import *\n{call}"),
            ] {
                let sources = owned(&[a, ("main.py", &main)]);
                assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{main}");
            }
        }
        let m = format!("{def}from a import *\n");
        let sources = owned(&[a, ("m.py", &m), ("main.py", "from m import x\nx()\n")]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("a.py"));
        let after = format!("from a import *\n{def}x()\n");
        let none = ("a.py", "def y():\n    pass\n");
        let before = format!("{def}from a import *\nx()\n");
        for (a, main) in [(a, &after), (none, &before)] {
            let sources = owned(&[a, ("main.py", main)]);
            let found = exact_in(&Python, &sources, "main.py", "x");
            assert_eq!(found.as_deref(), Some("main.py"), "{main}");
        }
    }

    #[test]
    fn two_export_stars_that_give_a_name_differently_export_neither() {
        use crate::lang::ts::TsJs;

        let m = ("m.ts", "export * from './a';\nexport * from './b';\n");
        let main = ("main.ts", "import { x } from './m';\nx();\n");
        let def = "export function x() {}\n";
        let sources = owned(&[("a.ts", def), ("b.ts", def), m, main]);
        assert_eq!(exact_in(&TsJs, &sources, "main.ts", "x"), None);
        let sources = owned(&[
            ("a.ts", "export * from './c';\n"),
            ("b.ts", "export { x } from './c';\n"),
            ("c.ts", def),
            m,
            main,
        ]);
        let found = exact_in(&TsJs, &sources, "main.ts", "x");
        assert_eq!(found.as_deref(), Some("c.ts"));
        let sources = owned(&[("a.ts", def), ("b.ts", "export function y() {}\n"), m, main]);
        let found = exact_in(&TsJs, &sources, "main.ts", "x");
        assert_eq!(found.as_deref(), Some("a.ts"));
    }

    #[test]
    fn a_top_level_binding_after_a_star_import_rebinds_its_name() {
        let def = "def x():\n    pass\n";
        let global = "def f():\n    global x\n    x = 1\n";
        for binding in [
            "with open(p) as x:\n    pass\n",
            "for x in xs:\n    pass\n",
            "try:\n    pass\nexcept E as x:\n    pass\n",
            "(x := g())\n",
            "match v:\n    case x:\n        pass\n",
            "y, x = g()\n",
            "x += 1\n",
            global,
        ] {
            let main = format!("from b import x\nfrom a import *\n{binding}x()\n");
            let sources = owned(&[("a.py", def), ("b.py", def), ("main.py", &main)]);
            assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{main}");
            let m = format!("from b import x\nfrom a import *\n{binding}");
            let main = ("main.py", "from m import x\nx()\n");
            let sources = owned(&[("a.py", def), ("b.py", def), ("m.py", &m), main]);
            assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{m}");
        }
        let main = format!("{global}from a import *\nx()\n");
        let sources = owned(&[("a.py", def), ("main.py", &main)]);
        assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None);
    }

    #[test]
    fn a_top_level_binding_after_a_definition_hides_it() {
        let def = "def x():\n    pass\n";
        let a = ("a.py", "def y():\n    pass\n");
        let global = "def f():\n    global x\n    x = 1\n";
        for binding in [
            "x = 1\n",
            "x: int = 1\n",
            "x += 1\n",
            "y, x = g()\n",
            "a, *x = xs\n",
            "[*x] = xs\n",
            "(x) = 1\n",
            "from a import y as x\n",
            "import a as x\n",
            "with open(p) as x:\n    pass\n",
            "for x in xs:\n    pass\n",
            "try:\n    pass\nexcept E as x:\n    pass\n",
            "(x := g())\n",
            "match v:\n    case x:\n        pass\n",
            global,
        ] {
            for call in ["x()\n", "def h():\n    x()\n"] {
                let main = format!("{def}{binding}{call}");
                let found = exact_in(&Python, &owned(&[a, ("main.py", &main)]), "main.py", "x");
                assert_ne!(found.as_deref(), Some("main.py"), "{main}");
            }
            let m = format!("{def}{binding}");
            let sources = owned(&[a, ("m.py", &m), ("main.py", "from m import x\nx()\n")]);
            let found = exact_in(&Python, &sources, "main.py", "x");
            assert_ne!(found.as_deref(), Some("m.py"), "{m}");
        }
        let main = format!("{def}from a import *\n{global}def h():\n    x()\n");
        let found = exact_in(&Python, &owned(&[a, ("main.py", &main)]), "main.py", "x");
        assert_eq!(found, None, "{main}");
        let again = format!("x = 1\n{def}x()\n");
        let found = exact_in(&Python, &owned(&[a, ("main.py", &again)]), "main.py", "x");
        assert_eq!(found.as_deref(), Some("main.py"));
        let star = format!("{def}x = 1\nfrom b import *\ndef h():\n    x()\n");
        let sources = owned(&[("b.py", def), ("main.py", &star)]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("b.py"));
    }

    #[test]
    fn a_binding_after_a_nested_definition_hides_it() {
        let a = ("a.py", "def y():\n    pass\n");
        let def = "    def g():\n        pass\n";
        let nonlocal = "    def h():\n        nonlocal g\n        g = 1\n";
        for def in [def, "    class g:\n        pass\n"] {
            for binding in [
                "    g = 1\n",
                "    g: int = 1\n",
                "    g += 1\n",
                "    y, g = k()\n",
                "    a, *g = k()\n",
                "    from a import y as g\n",
                "    import a as g\n",
                "    with open(p) as g:\n        pass\n",
                "    for g in gs:\n        pass\n",
                "    try:\n        pass\n    except E as g:\n        pass\n",
                "    (g := k())\n",
                "    match v:\n        case g:\n            pass\n",
                nonlocal,
            ] {
                for call in ["    g()\n", "    def i():\n        g()\n"] {
                    let main = format!("def f():\n{def}{binding}{call}");
                    let found = exact_in(&Python, &owned(&[a, ("main.py", &main)]), "main.py", "g");
                    assert_ne!(found.as_deref(), Some("main.py"), "{main}");
                }
            }
        }
        let before = format!("def f():\n{nonlocal}{def}    g()\n");
        let found = exact_in(&Python, &owned(&[("main.py", &before)]), "main.py", "g");
        assert_eq!(found, None, "{before}");
        let global = "    def h():\n        global g\n        g = 1\n";
        for main in [
            format!("def f():\n{def}{global}    g()\n"),
            format!("def f():\n    g = 1\n{def}    g()\n"),
        ] {
            let found = exact_in(&Python, &owned(&[("main.py", &main)]), "main.py", "g");
            assert_eq!(found.as_deref(), Some("main.py"), "{main}");
        }
        let class = "class C:\n    def m():\n        pass\n    m = staticmethod(m)\n\
                     \n    def n(self):\n        self.m()\n";
        let found = exact_in(&Python, &owned(&[("main.py", class)]), "main.py", "self.m");
        assert_eq!(found.as_deref(), Some("main.py"));
    }

    #[test]
    fn a_top_level_comprehension_or_lambda_name_hides_no_definition() {
        let def = "def h():\n    pass\n";
        for code in ["[h() for h in hs]\n", "run = lambda h: h()\n"] {
            let main = format!("{def}{code}");
            let found = exact_in(&Python, &owned(&[("main.py", &main)]), "main.py", "h");
            assert_eq!(found.as_deref(), Some("main.py"), "{main}");
            let main = format!("{def}def f():\n    {code}");
            let found = exact_in(&Python, &owned(&[("main.py", &main)]), "main.py", "h");
            assert_eq!(found, None, "{main}");
        }
    }

    #[test]
    fn a_definition_under_global_or_nonlocal_rebinds_its_name() {
        let global = "def x():\n    pass\ndef setup():\n    global x\n    def x():\n        pass\n";
        let nonlocal = "def f():\n    def g():\n        pass\n    def h():\n        nonlocal g\n        \
                        def g():\n            pass\n";
        for (main, name) in [
            (format!("{global}x()\n"), "x"),
            (format!("{global}def k():\n    x()\n"), "x"),
            (format!("{nonlocal}    g()\n"), "g"),
        ] {
            let found = exact_in(&Python, &owned(&[("main.py", &main)]), "main.py", name);
            assert_eq!(found, None, "{main}");
        }
    }

    #[test]
    fn a_conditional_definition_after_a_binding_hides_it() {
        let fallback = "try:\n    from collections import OrderedDict\nexcept ImportError:\n    \
                        class OrderedDict(dict):\n        pass\n";
        let def = "def x():\n    pass\n";
        let branches = "if c:\n    def x():\n        pass\nelse:\n    def x():\n        pass\n";
        let indent = |code: &str| {
            code.lines()
                .map(|l| format!("    {l}\n"))
                .collect::<String>()
        };
        for (code, name) in [
            (fallback.to_string(), "OrderedDict"),
            (branches.to_string(), "x"),
            (format!("x = 1\nfor _ in xs:\n{}", indent(def)), "x"),
        ] {
            let call = format!("{name}()\n");
            for main in [
                format!("{code}{call}"),
                format!("def f():\n{}", indent(&format!("{code}{call}"))),
            ] {
                let found = exact_in(&Python, &owned(&[("main.py", &main)]), "main.py", name);
                assert_eq!(found, None, "{main}");
            }
        }
        for code in [
            format!("if c:\n{}", indent(def)),
            format!("try:\n{}except E:\n    pass\n", indent(def)),
        ] {
            for main in [
                format!("{code}x()\n"),
                format!("def f():\n{}", indent(&format!("{code}x()\n"))),
            ] {
                let found = exact_in(&Python, &owned(&[("main.py", &main)]), "main.py", "x");
                assert_eq!(found.as_deref(), Some("main.py"), "{main}");
            }
        }
    }

    #[test]
    fn a_grown_all_decides_what_a_star_import_takes() {
        let def = "def x():\n    pass\n";
        let main = format!("{def}from pkg import *\ndef f():\n    x()\n");
        let pkg = |all: &str| format!("{all}{def}def y():\n    pass\n");
        for grown in [
            "__all__ = ['y']\n__all__ += ['x']\n",
            "__all__ = ['y']\n__all__ += ('x',)\n",
            "__all__ = ['y']\n__all__.extend(['x'])\n",
            "__all__ = ['y']\n__all__.append('x')\n",
        ] {
            let sources = owned(&[("pkg.py", &pkg(grown)), ("main.py", &main)]);
            let found = exact_in(&Python, &sources, "main.py", "x");
            assert_eq!(found.as_deref(), Some("pkg.py"), "{grown}");
        }
        for changed in [
            "__all__ = ['y']\n__all__ += names\n",
            "__all__ = ['y']\n__all__.extend(names)\n",
            "__all__ = ['y']\ndef g():\n    __all__.append('x')\n",
            "__all__ = ['x', 'y']\n__all__.remove('x')\n",
            "__all__ = ['x', 'y']\n__all__ -= ['x']\n",
            "__all__ = ['y']\nif WIN:\n    __all__ += ['x']\n",
            "__all__ = ['y']\ntry:\n    __all__.append('x')\nexcept E:\n    pass\n",
            "__all__ = ['y']\nfor _ in xs:\n    __all__.extend(['x'])\n",
            "__all__ = ['y']\nadd = lambda: __all__.append('x')\n",
            "if WIN:\n    __all__ = ['x', 'y']\nelse:\n    __all__ = ['y']\n",
            "__all__ = ['y']\n__all__[:] = ['x', 'y']\n",
            "__all__ = ['y']\n__all__[0] = 'x'\n",
            "__all__ = ['x', 'y']\ndel __all__[0]\n",
            "__all__ = ['y']\ndef g():\n    __all__[0] = 'x'\n",
            "__all__ = ['y']\ndef g():\n    global __all__\n    __all__ = ['x']\n",
            "__all__ = ['y']\ndef g():\n    global __all__\n    __all__ += ['x']\n",
        ] {
            let sources = owned(&[("pkg.py", &pkg(changed)), ("main.py", &main)]);
            let found = exact_in(&Python, &sources, "main.py", "x");
            assert_eq!(found, None, "{changed}");
        }
    }

    #[test]
    fn a_deleted_name_reaches_nothing() {
        let def = "def x():\n    pass\n";
        let b = ("b.py", def);
        for before in [def, "from b import x\n"] {
            for del in ["del x\n", "del y, x\n", "del [x]\n", "del (x)\n"] {
                for call in ["x()\n", "def f():\n    x()\n"] {
                    let main = format!("{before}{del}{call}");
                    let sources = owned(&[b, ("main.py", &main)]);
                    assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{main}");
                }
                let m = format!("{before}{del}");
                let sources = owned(&[b, ("m.py", &m), ("main.py", "from m import x\nx()\n")]);
                assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{m}");
            }
        }
        let instance = "class A:\n    def m(self):\n        pass\nx = A()\ndel x\nx.m()\n";
        let sources = owned(&[("main.py", instance)]);
        assert_eq!(exact_in(&Python, &sources, "main.py", "x.m"), None);
        let nested = "def f():\n    def x():\n        pass\n    del x\n    x()\n";
        let sources = owned(&[("main.py", nested)]);
        assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None);
        let star = format!("{def}from b import *\ndel x\ndef f():\n    x()\n");
        let sources = owned(&[b, ("main.py", &star)]);
        assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None);
        let again = format!("{def}del x\n{def}x()\n");
        let sources = owned(&[b, ("main.py", &again)]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("main.py"));
        let star = format!("{def}del x\nfrom b import *\ndef f():\n    x()\n");
        let sources = owned(&[b, ("main.py", &star)]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("b.py"));
    }

    #[test]
    fn a_star_import_of_a_computed_all_rebinds_a_name_to_nothing_known() {
        let def = "def x():\n    pass\n";
        let pkg = (
            "pkg.py",
            "import core\n__all__ = core.__all__ + ['y']\ndef x():\n    pass\n",
        );
        let (b, core) = (("b.py", def), ("core.py", "__all__ = []\n"));
        let call = "def f():\n    x()\n";
        for before in [def, "from b import x\n", "from b import *\n"] {
            let main = format!("{before}from pkg import *\n{call}");
            let sources = owned(&[pkg, b, core, ("main.py", &main)]);
            assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{main}");
            let m = format!("{before}from pkg import *\n");
            let main = ("main.py", "from m import x\nx()\n");
            let sources = owned(&[pkg, b, core, ("m.py", &m), main]);
            assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{m}");
        }
        let sources = owned(&[
            pkg,
            core,
            ("main.py", &format!("from pkg import *\n{call}")),
        ]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("pkg.py"));
    }

    #[test]
    fn a_star_import_that_reaches_the_name_back_through_a_cycle_gives_nothing() {
        use crate::lang::ts::TsJs;

        let sources = owned(&[
            (
                "pkg/__init__.py",
                "from .core import *\nfrom .utils import *\n",
            ),
            ("pkg/core.py", "def helper():\n    pass\n"),
            ("pkg/utils.py", "from pkg import helper\n"),
            ("main.py", "from pkg import helper\nhelper()\n"),
        ]);
        let found = exact_in(&Python, &sources, "main.py", "helper");
        assert_eq!(found.as_deref(), Some("pkg/core.py"));
        let sources = owned(&[
            ("index.ts", "export * from './a';\nexport * from './c';\n"),
            ("a.ts", "export { x } from './index';\n"),
            ("c.ts", "export function x() {}\n"),
            ("main.ts", "import { x } from './index';\nx();\n"),
        ]);
        let found = exact_in(&TsJs, &sources, "main.ts", "x");
        assert_eq!(found.as_deref(), Some("c.ts"));
    }

    #[test]
    fn a_star_import_through_a_cycle_and_two_differing_stars_gives_neither() {
        use crate::lang::ts::TsJs;

        let def = "export function x() {}\n";
        let sources = owned(&[
            ("index.ts", "export * from './a';\nexport * from './c';\n"),
            (
                "a.ts",
                "export * from './index';\nexport * from './p';\nexport * from './q';\n",
            ),
            ("p.ts", def),
            ("q.ts", def),
            ("c.ts", def),
            ("main.ts", "import { x } from './index';\nx();\n"),
        ]);
        assert_eq!(exact_in(&TsJs, &sources, "main.ts", "x"), None);
    }

    #[test]
    fn a_call_is_found_by_its_range_in_a_list_in_any_order() {
        let facts = Python.parse("a.py", b"f(g(1))\nh()\n").unwrap();
        let mut calls = facts.calls;
        assert_eq!(calls.len(), 3);
        for _ in 0..2 {
            for (i, call) in calls.iter().enumerate() {
                assert_eq!(call_at(&calls, &call.range), Some(i));
            }
            calls.reverse();
        }
        assert_eq!(call_at(&calls, &(0..1)), None);
    }

    fn fetched(src: &str) -> Vec<Option<String>> {
        use crate::lang::ts::TsJs;
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let code = Code::new(&resolver, &files, crate::lang::pack(0));
        let n = (facts.calls.iter())
            .position(|c| c.callee == "fetch")
            .unwrap();
        let url = |args: &[Value], _: &Keywords| match args.first()? {
            Value::Ref(_) => None,
            url => Some(url.clone()),
        };
        (code.expand(Expand::Call("a.ts", n), &url).into_iter())
            .map(|e| match e.found {
                Value::Str(parts) => crate::lang::literal(&parts),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_wrapper_cycle_that_hands_another_value_on_is_read_where_it_stands() {
        let urls = |wrappers: &str, call: &str| {
            let src = format!("{wrappers}\nfunction place() {{ {call}; }}\n");
            let mut found: Vec<String> = (fetched(&src).into_iter())
                .map(|url| url.unwrap_or_else(|| "?".into()))
                .collect();
            found.sort();
            found
        };
        let send = "function send(n: number, a: string, b: string) {\n\
                    fetch(a); if (n > 0) send(n - 1, b, ''); }";
        assert_eq!(urls(send, "send(1, '/first', '/second')"), ["/first", "?"]);
        assert_eq!(urls(send, "0"), ["?"]);
        let twice = "function send(n: number, a: string, b: string) {\n\
                     fetch(a); if (n > 0) send(n - 1, b, b); }";
        assert_eq!(urls(twice, "send(1, '/first', '/second')"), ["/first", "?"]);
        let ping = "function ping(n: number, a: string, b: string) {\n\
                    fetch(a); if (n > 0) pong(n, a, b); }\n\
                    function pong(n: number, a: string, b: string) { ping(n - 1, b, a); }";
        assert_eq!(urls(ping, "ping(1, '/first', '/second')"), ["/first", "?"]);
        let retry = "function retry(n: number, a: string, b: string) {\n\
                     fetch(a); if (n > 0) retry(n - 1, a, a); }";
        assert_eq!(urls(retry, "retry(1, '/first', '/second')"), ["/first"]);
        let tick = "function tick(n: number, a: string) { fetch(a); if (n > 0) tock(n, a); }\n\
                    function tock(n: number, a: string) { tick(n - 1, a); }";
        assert_eq!(urls(tick, "tick(1, '/first')"), ["/first"]);
    }

    #[test]
    fn what_a_cut_cycle_adds_spends_nothing_of_the_file_s_budget() {
        use crate::lang::ts::TsJs;
        let spent = |extra: usize| {
            let more: String = (0..extra).map(|i| format!(", p{i}")).collect();
            let zeros = ", 0".repeat(extra);
            let src = format!(
                "function send(n, a, b{more}) {{ fetch(a); if (n > 0) send(n - 1, b, a{more}); }}\n\
                 function place() {{ send(1, '/first', '/second'{zeros}); }}\n"
            );
            let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
            let files = BTreeMap::from([("a.ts", &facts)]);
            let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
            let code = Code::new(&resolver, &files, crate::lang::pack(0));
            let n = (facts.calls.iter())
                .position(|c| c.callee == "fetch")
                .unwrap();
            let url = |args: &[Value], _: &Keywords| match args.first()? {
                Value::Ref(_) => None,
                Value::Unknown => code.charge("a.ts", 1000).then_some(Value::Unknown),
                url => Some(url.clone()),
            };
            assert_eq!(code.expand(Expand::Call("a.ts", n), &url).len(), 2);
            let of = |budget: &RefCell<HashMap<&str, usize>>| {
                budget.borrow().get("a.ts").copied().unwrap_or_default()
            };
            (of(&code.folded), of(&code.folded_cuts))
        };
        let ((file, cuts), (file_wide, cuts_wide)) = (spent(0), spent(30));
        assert_eq!(file, file_wide);
        assert!(file < 1000 && cuts >= 1000, "{file} {cuts}");
        assert!(cuts_wide >= cuts + 30, "{cuts} {cuts_wide}");
    }

    #[test]
    fn past_its_budget_a_cut_cycle_adds_nothing() {
        use crate::lang::ts::TsJs;
        let src = "function send(n, a, b) { fetch(`/x/${a}`); if (n > 0) send(n - 1, b, a); }\n\
                   function place() { send(1, 'first', 'second'); }\n";
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let n = (facts.calls.iter())
            .position(|c| c.callee == "fetch")
            .unwrap();
        let url = |args: &[Value], _: &Keywords| match args.first()? {
            Value::Str(parts) if !parts.iter().any(|p| matches!(p, Part::Ref(_))) => Some(()),
            _ => None,
        };
        let sites = |spent: usize| {
            let code = Code::new(&resolver, &files, crate::lang::pack(0));
            code.cut_apart(|| code.charge("a.ts", spent));
            code.expand(Expand::Call("a.ts", n), &url).len()
        };
        assert_eq!((sites(0), sites(EVAL_BUDGET)), (2, 1));
    }

    #[test]
    fn a_value_folded_deeper_than_the_limit_is_unknown() {
        let chain = |n: usize| {
            let consts: String = (0..n)
                .map(|i| format!("const a{} = a{i};\n", i + 1))
                .collect();
            fetched(&format!("const a0 = '/a';\n{consts}fetch(a{n});\n"))
        };
        assert_eq!(chain(EVAL_DEPTH - 2), [Some("/a".to_string())]);
        assert_eq!(chain(EVAL_DEPTH - 1), [None]);
        assert_eq!(chain(20 * EVAL_DEPTH), [None]);
    }

    #[test]
    fn a_string_doubled_past_the_budget_has_parts_not_known() {
        let doubled = |n: usize| {
            let consts: String = (0..n)
                .map(|i| format!("const s{} = s{i} + s{i};\n", i + 1))
                .collect();
            fetched(&format!("const s0 = '/a';\n{consts}fetch(s{n});\n"))
        };
        assert_eq!(doubled(4), [Some("/a".repeat(16))]);
        assert_eq!(doubled(40), [None]);
    }

    #[test]
    fn a_wrapper_deeper_than_the_limit_is_read_where_the_climb_stops() {
        let source = |n: usize| {
            let wrappers: String = (0..n)
                .map(|i| format!("function w{}(u) {{ return w{i}(u); }}\n", i + 1))
                .collect();
            format!("function w0(u) {{ return fetch(u); }}\n{wrappers}w{n}('/a');\n")
        };
        let wrapped = |n: usize| fetched(&source(n));
        assert_eq!(wrapped(WRAPPER_DEPTH - 1), [Some("/a".to_string())]);
        assert_eq!(wrapped(WRAPPER_DEPTH), [None]);

        let up = |last: &str| {
            let calls: Vec<String> = (0..WRAPPER_DEPTH - 1).map(|i| format!("w{i}(u)")).collect();
            format!("{} > {last}", calls.join(" > "))
        };
        let top = format!("w{}('/a')", WRAPPER_DEPTH - 1);
        let stop = format!("w{}(u)", WRAPPER_DEPTH - 1);
        assert_eq!(climbed(&source(WRAPPER_DEPTH - 1)), [up(&top)]);
        assert_eq!(climbed(&source(WRAPPER_DEPTH)), [up(&stop)]);
    }

    #[test]
    fn a_lattice_of_wrappers_is_climbed_within_the_budget() {
        let source = |n: usize| {
            let wrappers: String = (0..n)
                .map(|i| format!("function w{}(u) {{ w{i}(u); return w{i}(u); }}\n", i + 1))
                .collect();
            format!("function w0(u) {{ return fetch(u); }}\n{wrappers}w{n}('/a');\n")
        };
        let lattice = |n: usize| fetched(&source(n));
        assert_eq!(lattice(3), vec![Some("/a".to_string()); 8]);
        let way = "w0(u) > w1(u) > w2(u) > w3('/a')";
        assert_eq!(climbed(&source(3)), vec![way; 8]);
        let at = climbed_at(&source(3));
        assert_eq!(at.iter().collect::<BTreeSet<_>>().len(), 8, "{at:?}");
        let ways = lattice(40);
        assert!(
            !ways.is_empty() && ways.len() < EVAL_BUDGET,
            "{}",
            ways.len()
        );
    }

    fn origins_of_p(src: &str) -> Vec<String> {
        let facts = Python.parse("a.py", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.py", &facts)]);
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Python);
        let code = Code::new(&resolver, &files, &Python);
        (code
            .origins(
                "a.py",
                "",
                Of::Name("p"),
                Follow::Values {
                    passes: passes_nothing,
                },
            )
            .into_iter())
        .map(|o| facts.calls[o.call.unwrap().1].callee.clone())
        .collect()
    }

    #[test]
    fn a_value_traced_deeper_than_the_limit_has_no_origin() {
        let returned = |n: usize| {
            let defs: String = (0..n)
                .map(|i| format!("def k{}():\n    return k{i}()\n", i + 1))
                .collect();
            origins_of_p(&format!("def k0():\n    return make()\n{defs}p = k{n}()\n"))
        };
        assert_eq!(returned(EVAL_DEPTH - 2), ["make"]);
        assert_eq!(returned(EVAL_DEPTH - 1), Vec::<String>::new());
    }

    #[test]
    fn a_lattice_of_returns_is_traced_within_the_budget() {
        let lattice = |n: usize| {
            let defs: String = (0..n)
                .map(|i| {
                    let call = format!("return k{i}()");
                    format!("def k{}():\n    if c:\n        {call}\n    {call}\n", i + 1)
                })
                .collect();
            origins_of_p(&format!("def k0():\n    return make()\n{defs}p = k{n}()\n"))
        };
        assert_eq!(lattice(3), ["make"]);
        assert_eq!(lattice(40), ["make"]);
    }

    #[test]
    fn a_family_longer_than_the_limit_is_absent() {
        let n = 5_000;
        for reverse in [false, true] {
            let facts = Python.parse("a.py", chain(n, reverse).as_bytes()).unwrap();
            let files = BTreeMap::from([("a.py", &facts)]);
            let resolver = Resolver::new(&files, Box::new(|_, _| std::rc::Rc::default()), &Python);
            let exact = |class: usize| {
                let scope = format!("C{class}.go");
                let call = facts.calls.iter().find(|c| c.scope == scope).unwrap();
                matches!(resolver.call("a.py", call), Resolution::Exact(t) if t.name == "C0.m")
            };

            assert!(exact(FAMILY_DEPTH - 1), "reverse: {reverse}");
            assert!(!exact(FAMILY_DEPTH), "reverse: {reverse}");
            assert!(!exact(n - 1), "reverse: {reverse}");
        }
    }

    fn parsed(files: &[(&'static str, &str)]) -> Vec<(&'static str, Facts)> {
        use crate::lang::rust::Rust;
        let parse =
            |(name, src): &(&'static str, &str)| (*name, Rust.parse(name, src.as_bytes()).unwrap());
        files.iter().map(parse).collect()
    }

    fn resolver<'a>(files: &'a BTreeMap<&'a str, &'a Facts>) -> Resolver<'a> {
        use crate::lang::rust::Rust;
        let modules: Modules = Box::new(|from, specifier| match (specifier, from) {
            ("crate", _) => ["main.rs".into()].into(),
            ("self", from) => [from.into()].into(),
            ("super", "c.rs") => ["b.rs".into()].into(),
            ("super", "b.rs") => ["a.rs".into()].into(),
            ("super", _) => ["main.rs".into()].into(),
            (s, _) => (s.strip_prefix("mod ").into_iter())
                .map(|m| format!("{m}.rs"))
                .collect(),
        });
        Resolver::new(files, modules, &Rust)
    }

    fn calls(files: &[(&'static str, &str)]) -> BTreeMap<String, String> {
        calls_with(files, false)
    }

    fn calls_with(files: &[(&'static str, &str)], candidates: bool) -> BTreeMap<String, String> {
        let parsed = parsed(files);
        let map: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let resolver = resolver(&map);
        let mut out = BTreeMap::new();
        for (path, facts) in &parsed {
            for call in &facts.calls {
                let found = match resolver.call(path, call) {
                    Resolution::Exact(t) => format!("exact {}::{}", t.path, t.name),
                    Resolution::Inferred(t) | Resolution::Narrowed(t) => {
                        format!("inferred {}::{}", t.path, t.name)
                    }
                    Resolution::Ambiguous(all) if candidates => {
                        let all: Vec<_> = all
                            .iter()
                            .map(|t| format!("{}::{}", t.path, t.name))
                            .collect();
                        format!("ambiguous {}", all.join(" "))
                    }
                    Resolution::Ambiguous(_) => "ambiguous".to_string(),
                    Resolution::Unresolved => "none".to_string(),
                };
                out.insert(format!("{path} {} {}", call.scope, call.callee), found);
            }
        }
        out
    }

    #[test]
    fn a_chain_of_uses_longer_than_the_limit_gives_nothing() {
        let n = 3_000;
        let mut src = String::from("fn a0() {}\n");
        for i in 1..=n {
            src.push_str(&format!("use a{} as a{i};\n", i - 1));
        }
        let last = LINK_DEPTH - 2;
        src.push_str(&format!("fn main() {{ a{last}(); a{n}(); }}\n"));
        let found = calls(&[("main.rs", &src)]);
        assert_eq!(found[&format!("main.rs main a{last}")], "exact main.rs::a0");
        assert_eq!(found[&format!("main.rs main a{n}")], "none");
    }

    #[test]
    fn a_name_a_glob_reaches_past_the_limit_is_taken_from_no_other_glob() {
        let x = |n: usize, by_name: bool| {
            let mut files = vec![("main.rs".to_string(), String::new())];
            let mut main = String::from("mod near;\n");
            for i in 0..n {
                main.push_str(&format!("mod m{i};\n"));
                let next = match by_name {
                    true => format!("pub use crate::m{}::x;\n", i + 1),
                    false => format!("pub use crate::m{}::*;\n", i + 1),
                };
                files.push((format!("m{i}.rs"), next));
            }
            main.push_str(&format!(
                "mod m{n};\nuse crate::near::*;\nuse crate::m0::*;\n"
            ));
            main.push_str("fn main() { x(); }\n");
            files[0].1 = main;
            files.push((format!("m{n}.rs"), "pub fn x() {}\n".to_string()));
            files.push(("near.rs".to_string(), "pub fn x() {}\n".to_string()));
            let files: Vec<(&'static str, &str)> = (files.iter())
                .map(|(path, src)| (&*path.clone().leak(), src.as_str()))
                .collect();
            calls(&files)["main.rs main x"].clone()
        };
        for by_name in [false, true] {
            assert_eq!(x(3, by_name), "ambiguous", "by name: {by_name}");
            let past = x(3 * LINK_DEPTH, by_name);
            assert!(!past.starts_with("exact"), "by name: {by_name}: {past}");
        }
    }

    #[test]
    fn a_glob_gives_the_public_items_alone() {
        let found = calls(&[
            (
                "main.rs",
                "mod extras;\nmod prelude;\nuse crate::extras::*;\nuse crate::prelude::*;\n\
                 fn main() { dup(); }\n",
            ),
            ("extras.rs", "pub fn dup() {}\n"),
            ("prelude.rs", "fn dup() {}\npub fn greet() {}\n"),
        ]);
        assert_eq!(found["main.rs main dup"], "exact extras.rs::dup");
    }

    #[test]
    fn a_crate_outside_the_repository_stays_external_in_every_form_of_its_use() {
        let found = calls(&[
            (
                "main.rs",
                "use tokio::{self, select};\nuse serde_json::{self, json};\nuse foo::foo;\n\
                 fn main() { tokio::spawn(f); serde_json::spawn(); foo::spawn(); \
                 foo::foo::spawn(); }\n",
            ),
            ("other.rs", "pub fn spawn() {}\n"),
        ]);
        for call in [
            "tokio.spawn",
            "serde_json.spawn",
            "foo.spawn",
            "foo.foo.spawn",
        ] {
            assert_eq!(found[&format!("main.rs main {call}")], "none", "{call}");
        }
    }

    #[test]
    fn a_module_is_no_value_and_shares_its_name_with_an_item() {
        let found = calls(&[
            (
                "main.rs",
                "mod run;\nmod lib2;\nuse run::run;\nfn main() { run(); run::run(); \
                 lib2::x(); lib2::z(); }\n",
            ),
            ("run.rs", "pub fn run() {}\n"),
            ("lib2.rs", "mod x;\npub use x::x;\npub fn z() {}\n"),
            ("x.rs", "pub fn x() {}\n"),
        ]);
        assert_eq!(found["main.rs main run"], "exact run.rs::run");
        assert_eq!(found["main.rs main run.run"], "exact run.rs::run");
        assert_eq!(found["main.rs main lib2.x"], "exact x.rs::x");
        assert_eq!(found["main.rs main lib2.z"], "exact lib2.rs::z");
    }

    #[test]
    fn a_use_binds_in_the_module_that_holds_it() {
        let found = calls(&[
            (
                "main.rs",
                "mod time;\nuse crate::time;\nmod m { fn f() { time::now(); } }\n\
                 fn main() { time::now(); }\n",
            ),
            ("time.rs", "pub fn now() {}\n"),
        ]);
        assert_eq!(found["main.rs main time.now"], "exact time.rs::now");
        assert_eq!(found["main.rs m.f time.now"], "none");
    }

    #[test]
    fn two_files_that_glob_their_parent_each_see_it() {
        let found = calls(&[
            ("main.rs", "mod a;\nfn main() {}\n"),
            ("a.rs", "mod b;\npub fn helper() {}\n"),
            ("b.rs", "use super::*;\nmod c;\n"),
            ("c.rs", "use super::*;\nfn f() { helper(); }\n"),
        ]);
        assert_eq!(found["c.rs f helper"], "exact a.rs::helper");
    }

    #[test]
    fn a_glob_gives_a_type_as_it_gives_a_function() {
        let parsed = parsed(&[(
            "main.rs",
            "pub struct Def;\npub fn free() {}\nmod inner { use super::*; }\n",
        )]);
        let map: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let r = resolver(&map);
        for name in ["Def", "free"] {
            let seen = &mut r.seen("main.rs", "inner");
            let found = r.globs("main.rs", "inner", name, false, seen);
            assert!(
                matches!(found, Some(Binding::Def(t)) if t.path == "main.rs" && t.name == name),
                "{name}"
            );
        }
    }

    #[test]
    fn a_path_head_a_glob_gives_is_found_through_it() {
        let found = calls(&[(
            "main.rs",
            "pub struct Def;\nimpl Def { pub fn new() -> Self { Def } }\npub fn free() {}\n\
             mod inner { use super::*; fn a() { Def::new(); } fn b() { free(); } }\n\
             mod own { use super::*; struct Def; impl Def { fn new() -> Self { Def } }\n\
             fn a() { Def::new(); } }\n\
             mod ext { use serde::*; fn a() { Ext::new(); } }\n",
        )]);
        assert_eq!(found["main.rs inner.a Def.new"], "exact main.rs::Def.new");
        assert_eq!(found["main.rs inner.b free"], "exact main.rs::free");
        assert_eq!(found["main.rs own.a Def.new"], "exact main.rs::own.Def.new");
        assert_eq!(found["main.rs ext.a Ext.new"], "none");
    }

    #[test]
    fn a_path_head_a_glob_of_a_file_module_gives_is_found_through_it() {
        let found = calls(&[
            ("main.rs", "mod a;\nmod x;\nfn main() {}\n"),
            (
                "x.rs",
                "pub struct Def;\nimpl Def { pub fn new() -> Self { Def } }\n\
                 struct Priv;\nimpl Priv { pub fn new() -> Self { Priv } }\n\
                 mod named { use crate::x::*; fn h() { Def::new(); } }\n",
            ),
            (
                "a.rs",
                "use crate::x::Def;\nmod tests { use super::*; fn f() { Def::new(); }\n\
                 mod deep { use super::super::*; fn g() { Def::new(); } } }\n\
                 mod sibling { use crate::x::*; fn p() { Priv::new(); } }\n",
            ),
        ]);
        assert_eq!(found["x.rs named.h Def.new"], "exact x.rs::Def.new");
        assert_eq!(found["a.rs tests.f Def.new"], "exact x.rs::Def.new");
        assert_eq!(found["a.rs tests.deep.g Def.new"], "exact x.rs::Def.new");
        assert_eq!(found["a.rs sibling.p Priv.new"], "none");
    }

    #[test]
    fn a_crate_glob_of_an_ancestor_file_sees_its_private_use() {
        let found = calls(&[
            ("main.rs", "mod a;\nmod x;\nfn main() {}\n"),
            (
                "x.rs",
                "pub struct Def;\nimpl Def { pub fn new() -> Self { Def } }\n",
            ),
            ("a.rs", "use crate::x::Def;\nmod b;\n"),
            ("b.rs", "mod c;\n"),
            ("c.rs", "use crate::a::*;\nfn h() { Def::new(); }\n"),
        ]);
        assert_eq!(found["c.rs h Def.new"], "exact x.rs::Def.new");
    }

    #[test]
    fn a_crate_glob_of_an_ancestor_sees_what_super_glob_sees() {
        let found = calls(&[
            ("main.rs", "mod a;\nmod b;\nmod x;\nfn main() {}\n"),
            (
                "x.rs",
                "pub struct Def;\nimpl Def { pub fn new() -> Self { Def } }\npub fn xf() {}\n",
            ),
            (
                "a.rs",
                "use crate::x::Def;\nuse crate::x::xf;\n\
                 mod named { use crate::a::*; fn h() { Def::new(); xf(); } }\n\
                 mod sup { use super::*; fn h() { Def::new(); xf(); } }\n\
                 mod sib { use crate::b::*; fn h() { Def::new(); xf(); bf(); } }\n",
            ),
            (
                "b.rs",
                "use crate::x::Def;\nuse crate::x::xf;\npub fn bf() {}\n",
            ),
        ]);
        assert_eq!(found["a.rs named.h Def.new"], "exact x.rs::Def.new");
        assert_eq!(found["a.rs named.h xf"], "exact x.rs::xf");
        assert_eq!(found["a.rs sup.h Def.new"], "exact x.rs::Def.new");
        assert_eq!(found["a.rs sup.h xf"], "exact x.rs::xf");
        assert_eq!(found["a.rs sib.h bf"], "exact b.rs::bf");
        assert_eq!(found["a.rs sib.h Def.new"], "none");
        assert_eq!(found["a.rs sib.h xf"], "inferred x.rs::xf");
    }

    #[test]
    fn a_path_head_two_globs_give_is_read_as_a_name_called_is() {
        let found = calls(&[
            (
                "main.rs",
                "mod x;\nmod y;\nmod m { use crate::x::*; use crate::y::*;\n\
             fn f() { Def::new(); free(); } }\n",
            ),
            (
                "x.rs",
                "pub struct Def;\nimpl Def { pub fn new() -> Self { Def } }\npub fn free() {}\n",
            ),
            (
                "y.rs",
                "pub struct Def;\nimpl Def { pub fn new() -> Self { Def } }\npub fn free() {}\n",
            ),
        ]);
        assert!(found["main.rs m.f Def.new"].starts_with("ambiguous"));
        assert!(found["main.rs m.f free"].starts_with("ambiguous"));
    }

    #[test]
    fn two_pub_use_globs_giving_one_name_are_ambiguous_with_both_candidates() {
        let found = calls_with(
            &[
                (
                    "main.rs",
                    "mod x;\nmod y;\nmod m { pub use crate::x::*; pub use crate::y::*; }\n\
                 mod user { use crate::m::*; fn f() { free(); } }\n",
                ),
                ("x.rs", "pub fn free() {}\n"),
                ("y.rs", "pub fn free() {}\n"),
            ],
            true,
        );
        assert_eq!(
            found["main.rs user.f free"],
            "ambiguous x.rs::free y.rs::free"
        );
    }

    #[test]
    fn an_item_or_use_beside_two_globs_still_wins() {
        let found = calls(&[
            (
                "main.rs",
                "mod x;\nmod y;\nmod m { use crate::x::*; use crate::y::*; use crate::x::free;\n\
             fn f() { free(); } }\n\
             mod n { use crate::x::*; use crate::y::*; fn free() {}\nfn f() { free(); } }\n",
            ),
            ("x.rs", "pub fn free() {}\n"),
            ("y.rs", "pub fn free() {}\n"),
        ]);
        assert_eq!(found["main.rs m.f free"], "exact x.rs::free");
        assert_eq!(found["main.rs n.f free"], "exact main.rs::n.free");
    }

    #[test]
    fn a_pub_use_in_a_module_written_in_place_is_a_member_of_it() {
        let found = calls(&[
            (
                "main.rs",
                "mod lib2;\nfn main() { lib2::entries::secret(); lib2::entries::seal(); }\n",
            ),
            (
                "lib2.rs",
                "pub mod entries { pub use super::internal::secret; use super::internal::seal; }\n\
                 mod internal { pub fn secret() {} pub fn seal() {} }\n",
            ),
        ]);
        assert_eq!(
            found["main.rs main lib2.entries.secret"],
            "exact lib2.rs::internal.secret"
        );
        assert_eq!(
            found["main.rs main lib2.entries.seal"],
            "inferred lib2.rs::internal.seal"
        );
    }

    #[test]
    fn a_used_name_is_spelled_by_the_path_it_imports() {
        let parsed = parsed(&[(
            "main.rs",
            "use reqwest::Client;\nuse tokio::{self};\nuse reqwest as r;\nuse r::blocking as b;\n\
             fn f() { use std::fs as files; }\n",
        )]);
        let map: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let resolver = resolver(&map);
        let spelled = |scope, name| resolver.imported_name("main.rs", scope, name);
        assert_eq!(spelled("main", "Client.new"), "reqwest.Client.new");
        assert_eq!(spelled("", "tokio.spawn"), "tokio.spawn");
        assert_eq!(spelled("", "b.Client.new"), "reqwest.blocking.Client.new");
        assert_eq!(spelled("f", "files.read"), "std.fs.read");
        assert_eq!(spelled("", "files.read"), "files.read");
        let binding = resolver.binding("main.rs", "main", "Client.new");
        assert!(matches!(binding, Binding::External("reqwest", _)));
    }

    #[test]
    fn a_result_followed_past_the_limit_gives_nothing() {
        let parsed = parsed(&[(
            "main.rs",
            "struct S;\nimpl S { fn new() -> Self { S } fn go(&self) {} }\n\
             fn f() { let s = S::new(); s.go(); }\n",
        )]);
        let map: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let resolver = resolver(&map);
        let s = |depth: usize| {
            resolver.chain.set(depth);
            resolver.cut.set(false);
            let found =
                resolver.instance(Binding::Local("main.rs", "f", "s"), &mut Work::default());
            (
                matches!(found, Binding::Instance(t) if t.name == "S"),
                resolver.cut.get(),
            )
        };
        assert_eq!(s(0), (true, false));
        assert_eq!(s(RECEIVER_DEPTH - 1), (true, false));
        assert_eq!(s(RECEIVER_DEPTH), (false, true));
    }

    #[test]
    fn a_class_annotated_deeper_than_the_limit_is_unknown() {
        let exact = |n: usize, quoted: bool| {
            let annotation = |i: usize| match (i + 1 == n, quoted) {
                (true, _) => "K".to_string(),
                (false, true) => format!("\"x{}.C\"", i + 1),
                (false, false) => format!("x{}.C", i + 1),
            };
            let params: Vec<String> = (0..n).map(|i| format!("x{i}: {}", annotation(i))).collect();
            let src = format!(
                "class K:\n    class C(K):\n        def m(self):\n            pass\n\n\
                 def f({}):\n    x0.m()\n",
                params.join(", ")
            );
            let facts = Python.parse("a.py", src.as_bytes()).unwrap();
            let files = BTreeMap::from([("a.py", &facts)]);
            let resolver = Resolver::new(&files, Box::new(|_, _| std::rc::Rc::default()), &Python);
            let call = facts.calls.iter().find(|c| c.callee == "x0.m").unwrap();
            matches!(resolver.call("a.py", call), Resolution::Exact(t) if t.name == "K.C.m")
        };

        let far = 5_000;
        for quoted in [false, true] {
            assert!(exact(RECEIVER_DEPTH, quoted), "quoted: {quoted}");
            assert!(!exact(RECEIVER_DEPTH + 1, quoted), "quoted: {quoted}");
            assert!(!exact(far, quoted), "quoted: {quoted}");
        }
    }

    #[test]
    fn a_family_does_not_depend_on_the_bases_of_the_walk_that_reaches_it() {
        let k = FAMILY_DEPTH - 1;
        let mut head =
            "class C0:\n    class D:\n        def m(self):\n            pass\n".to_string();
        for i in 1..=k {
            head.push_str(&format!("\n\nclass C{i}(C{}):\n    pass\n", i - 1));
        }
        let z = format!("class Z(C{k}.D):\n    pass\n");
        let go = "def go(z: Z):\n    z.m()\n";
        for z_first in [false, true] {
            let src = match z_first {
                true => format!("{z}\n{head}\n{go}"),
                false => format!("{head}\n{z}\n{go}"),
            };
            let facts = Python.parse("a.py", src.as_bytes()).unwrap();
            let files = BTreeMap::from([("a.py", &facts)]);
            let resolver = Resolver::new(&files, Box::new(|_, _| std::rc::Rc::default()), &Python);
            let call = facts.calls.iter().find(|c| c.callee == "z.m").unwrap();
            let found = resolver.call("a.py", call);
            assert!(
                matches!(found, Resolution::Exact(t) if t.name == "C0.D.m"),
                "z first: {z_first}"
            );
        }
    }

    #[test]
    fn a_family_does_not_depend_on_the_receivers_of_the_walk_that_reaches_it() {
        let n = RECEIVER_DEPTH;
        let mut head =
            "class L:\n    class D(L):\n        pass\n\n    def m(self):\n        pass\n\n\
                        w = L()\ny = w.D()\n\n\
                        class K(y.D):\n    class C(K):\n        pass\n"
                .to_string();
        for i in 0..n - 1 {
            head.push_str(&format!("x{i} = x{}.C()\n", i + 1));
        }
        head.push_str(&format!("x{} = K()\n", n - 1));
        let z = "class Z(x0.C):\n    pass\n";
        let go = "def go(k: K, z: Z):\n    k.m()\n    z.m()\n";
        for z_first in [false, true] {
            let src = match z_first {
                true => format!("{z}\n{head}\n{go}"),
                false => format!("{head}\n{z}\n{go}"),
            };
            let facts = Python.parse("a.py", src.as_bytes()).unwrap();
            let files = BTreeMap::from([("a.py", &facts)]);
            let resolver = Resolver::new(&files, Box::new(|_, _| std::rc::Rc::default()), &Python);
            for callee in ["k.m", "z.m"] {
                let call = facts.calls.iter().find(|c| c.callee == callee).unwrap();
                let found = resolver.call("a.py", call);
                assert!(
                    matches!(found, Resolution::Exact(t) if t.name == "L.m"),
                    "{callee}, z first: {z_first}"
                );
            }
        }
    }

    fn over<T>(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        ask: impl for<'a> FnOnce(&Code<'a>) -> T,
    ) -> T {
        let parse =
            |&(path, src): &(&'static str, &str)| (path, pack.parse(path, src.as_bytes()).unwrap());
        let facts: Vec<(&'static str, Facts)> = sources.iter().map(parse).collect();
        let files: BTreeMap<&str, &Facts> = facts.iter().map(|(p, f)| (*p, f)).collect();
        let resolver = Resolver::new(&files, pack.modules(&[], &files).unwrap(), pack);
        ask(&Code::new(&resolver, &files, pack))
    }

    fn written(code: &Code, sources: &[(&str, &str)], path: &str, index: usize) -> String {
        let src = sources.iter().find(|(p, _)| *p == path).unwrap().1;
        let call = &code.files()[path].calls[index];
        src[call.range.clone()].to_string()
    }

    #[derive(Clone, Copy)]
    enum Shown {
        Nothing,
        Through,
        All,
    }

    fn origin_lines<'a>(
        code: &Code<'a>,
        sources: &[(&str, &str)],
        found: Vec<Origin<'a>>,
        shown: Shown,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        for origin in found {
            let (path, n) = origin.call.unwrap();
            let mut head = format!("{path} {}", code.call(path, n).unwrap().callee);
            if let Some(outside) = &origin.outside {
                head += &format!(" = {}", outside.spelled);
            }
            let mut own: Vec<Route> = Vec::new();
            for route in origin.routes {
                let route = match shown {
                    Shown::Nothing => Route {
                        through: Vec::new(),
                        steps: Vec::new(),
                        held: None,
                    },
                    Shown::Through => Route {
                        steps: Vec::new(),
                        held: None,
                        ..route
                    },
                    Shown::All => route,
                };
                if !own.contains(&route) {
                    own.push(route);
                }
            }
            lines.extend(
                own.iter()
                    .map(|r| format!("{head}{}", route_line(code, sources, r))),
            );
        }
        lines
    }

    fn via(code: &Code, sources: &[(&str, &str)], route: &Route) -> String {
        let calls = route.through.iter();
        calls
            .map(|&(at, index)| format!(" via {}", written(code, sources, at, index)))
            .collect()
    }

    fn route_line(code: &Code, sources: &[(&str, &str)], route: &Route) -> String {
        let mut line = String::new();
        if !route.steps.is_empty() {
            line += &format!(" steps={}", route.steps.join(","));
        }
        if let Some((file, name)) = route.held {
            line += &format!(" held={file}:{name}");
        }
        line + &via(code, sources, route)
    }

    fn held_by(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
        scope: &'static str,
        name: &'static str,
    ) -> Vec<String> {
        over(pack, sources, |code| {
            let path = code.files().get_key_value(path).unwrap().0;
            let found = code.origins(
                path,
                scope,
                Of::Name(name),
                Follow::Values { passes: TABLE },
            );
            origin_lines(code, sources, found, Shown::Nothing)
        })
    }

    fn routes_of(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
        scope: &'static str,
        name: &'static str,
    ) -> Vec<String> {
        over(pack, sources, |code| {
            let path = code.files().get_key_value(path).unwrap().0;
            let found = code.origins(
                path,
                scope,
                Of::Name(name),
                Follow::Values { passes: TABLE },
            );
            origin_lines(code, sources, found, Shown::All)
        })
    }

    fn probed<'a>(code: &Code<'a>, path: &'a str) -> (&'a str, &'a Value) {
        let calls = code.files()[path].calls.iter();
        let call = calls.into_iter().find(|c| c.callee == "probe").unwrap();
        (call.scope.as_str(), &call.args[0])
    }

    fn framework_table(origin: &str, method: &str) -> bool {
        (origin.starts_with("axum.") && ["route", "nest", "layer"].contains(&method))
            || (origin.starts_with("actix_web.") && method == "service")
    }
    const TABLE: Passes = framework_table;

    fn probe_passing(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
        shown: Shown,
        passes: Passes,
    ) -> Vec<String> {
        over(pack, sources, |code| {
            let path = code.files().get_key_value(path).unwrap().0;
            let (scope, value) = probed(code, path);
            let found = code.origins(path, scope, Of::Value(value), Follow::Values { passes });
            origin_lines(code, sources, found, shown)
        })
    }

    fn probe_lines(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
        shown: Shown,
    ) -> Vec<String> {
        probe_passing(pack, sources, path, shown, TABLE)
    }

    fn held_by_value(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
    ) -> Vec<String> {
        probe_lines(pack, sources, path, Shown::Nothing)
    }

    fn held_through(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
    ) -> Vec<String> {
        probe_lines(pack, sources, path, Shown::Through)
    }

    fn probe_routes(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
    ) -> Vec<String> {
        probe_lines(pack, sources, path, Shown::All)
    }

    fn ts() -> &'static dyn LanguagePack {
        &crate::lang::ts::TsJs
    }

    fn py() -> &'static dyn LanguagePack {
        &Python
    }

    fn rust() -> &'static dyn LanguagePack {
        &crate::lang::rust::Rust
    }

    fn go() -> &'static dyn LanguagePack {
        &crate::lang::go::Go
    }

    fn read_names(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
    ) -> Vec<String> {
        over(pack, sources, |code| {
            let path = code.files().get_key_value(path).unwrap().0;
            let or_none = |s: Option<String>| s.unwrap_or_else(|| "-".into());
            let mut lines = Vec::new();
            for call in &code.files()[path].calls {
                let (scope, callee) = (call.scope.as_str(), call.callee.as_str());
                let read = code.outside(path, scope, callee, false);
                let by_path = code.outside(path, scope, callee, call.by_path);
                let imported = (code.imported(path, scope, callee))
                    .map(|(package, name)| format!("{package} {name}"));
                lines.push(format!(
                    "{callee}: external={} by_path={} direct={} import={}",
                    or_none(read.as_ref().map(|o| o.spelled.clone())),
                    or_none(by_path.map(|o| o.spelled)),
                    read.is_some_and(|o| o.direct),
                    or_none(imported),
                ));
            }
            lines
        })
    }

    fn outside_of(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
        callee: &str,
        by_path: bool,
    ) -> Option<String> {
        over(pack, sources, |code| {
            let path = code.files().get_key_value(path).unwrap().0;
            let calls = code.files()[path].calls.iter();
            let call = calls.into_iter().find(|c| c.callee == callee).unwrap();
            let outside = code.outside(path, &call.scope, &call.callee, by_path)?;
            let imported = (outside.imported).map_or("-".into(), |(p, n)| format!("{p} {n}"));
            Some(format!(
                "{imported}; {}; direct={}",
                outside.spelled, outside.direct
            ))
        })
    }

    fn constant_origin(
        sources: &[(&'static str, &str)],
        scope: &'static str,
        chain: &'static str,
    ) -> Option<String> {
        over(ts(), sources, |code| {
            let path = code.files().get_key_value("a.ts").unwrap().0;
            let mut found = code.origins(path, scope, Of::Name(chain), Follow::Constants);
            assert!(found.len() <= 1);
            let origin = found.pop()?;
            assert_eq!(origin.call, None);
            let [route] = &origin.routes[..] else {
                panic!("one route");
            };
            assert!(route.through.is_empty());
            let (package, name) = origin.outside?.imported?;
            let mut expr = match origin.form {
                Form::New => format!("new {name}()"),
                Form::Call => format!("{name}()"),
                Form::Name => name,
                Form::Type => panic!("no written type among constants"),
            };
            for step in &route.steps {
                expr += &format!(".{step}()");
            }
            let held = (route.held).map_or(String::new(), |(f, n)| format!(" held {f}:{n}"));
            Some(format!("{package} {expr}{held}"))
        })
    }

    fn declared(src: &str, scope: &'static str, name: &'static str) -> String {
        let (origin, written, _) = declared_form(src, scope, name);
        format!("origin={origin} type={written}")
    }

    fn declared_form(
        src: &str,
        scope: &'static str,
        name: &'static str,
    ) -> (String, String, String) {
        over(go(), &[("a.go", src)], |code| {
            let path = code.files().get_key_value("a.go").unwrap().0;
            let found = code.origins(path, scope, Of::Name(name), Follow::Declarations);
            let none = || "-".to_string();
            let Some(origin) = found.into_iter().next() else {
                return (none(), none(), none());
            };
            let [route] = &origin.routes[..] else {
                panic!("one route");
            };
            let spelled = origin.outside.unwrap().spelled;
            let typed = origin.form == Form::Type && route.steps.is_empty();
            let written = if typed { spelled.clone() } else { none() };
            let steps = route.steps.iter().map(|s| format!(".{s}"));
            let full = spelled + &steps.collect::<String>();
            (full, written, format!("{:?}", origin.form))
        })
    }

    fn climbed(src: &str) -> Vec<String> {
        climbed_with(src, &|text, _| text)
    }

    fn climbed_at(src: &str) -> Vec<String> {
        climbed_with(src, &|text, index| format!("{text}#{index}"))
    }

    fn climbed_with(src: &str, show: &dyn Fn(String, usize) -> String) -> Vec<String> {
        let sources = [("a.ts", src)];
        over(ts(), &sources, |code| {
            let path = code.files().get_key_value("a.ts").unwrap().0;
            let at = (code.files()[path].calls.iter())
                .position(|c| c.callee == "fetch")
                .unwrap();
            let url = |args: &[Value], _: &Keywords| match args.first()? {
                Value::Ref(_) => None,
                url => Some(url.clone()),
            };
            let mut lines = Vec::new();
            for expanded in code.expand(Expand::Call(path, at), &url) {
                let frames = (expanded.frames.iter())
                    .map(|f| show(written(code, &sources, f.path, f.index), f.index))
                    .collect::<Vec<_>>();
                lines.push(frames.join(" > "));
            }
            lines
        })
    }

    fn constructed_sites(src: &str, held: &'static str, at: &str) -> Vec<String> {
        over(ts(), &[("a.ts", src)], |code| {
            let path = code.files().get_key_value("a.ts").unwrap().0;
            let at = code.files()[path].calls.iter().position(|c| c.callee == at);
            let first = |args: &[Value], _: &Keywords| match args.first()? {
                Value::Ref(_) => None,
                first => Some(first.clone()),
            };
            let from = Expand::Constructed {
                held: (path, held),
                at: (path, at.unwrap()),
            };
            let mut lines = Vec::new();
            for Expanded {
                site: (path, index),
                found: first,
                ..
            } in code.expand(from, &first)
            {
                let Value::Str(parts) = first else {
                    panic!("the first argument is not a string");
                };
                let name = crate::lang::literal(&parts).unwrap_or_default();
                lines.push(format!(
                    "{} -> {name}",
                    code.call(path, index).unwrap().callee
                ));
            }
            lines
        })
    }

    #[test]
    fn a_constant_holding_a_call_is_made_by_that_call() {
        let sources = [("a.py", "p = make()\nq = 3\n")];
        assert_eq!(held_by(py(), &sources, "a.py", "", "p"), ["a.py make"]);
        assert_eq!(
            held_by(py(), &sources, "a.py", "", "q"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_constant_imported_from_another_file_is_spelled_through_the_import() {
        let sources = [
            (
                "b.py",
                "from fastapi import APIRouter\nrouter = APIRouter()\n",
            ),
            ("a.py", "from b import router\np = router\n"),
        ];
        assert_eq!(
            held_by(py(), &sources, "a.py", "", "p"),
            ["b.py APIRouter = fastapi.APIRouter"]
        );
    }

    #[test]
    fn a_parameter_is_reached_through_an_exact_caller_and_not_a_name_matched_one() {
        let def = "class S:\n    def use(self, r):\n        return r\n";
        let exact = format!("{def}\ndef go():\n    s = S()\n    s.use(Router())\n");
        let matched = "\ndef other(thing):\n    thing.use(Other())\n";
        let both = format!("{exact}{matched}");
        let param = |src: &str| held_by(py(), &[("a.py", src)], "a.py", "S.use", "r");
        assert_eq!(param(&exact), ["a.py Router"]);
        assert_eq!(param(&both), ["a.py Router"]);
        let only = format!("{def}{matched}");
        assert_eq!(param(&only), Vec::<String>::new());
    }

    #[test]
    fn a_function_and_a_method_of_a_name_of_its_own_run_alone() {
        let src = "package p\n\ntype A struct{}\n\ntype B struct{}\n\n\
                   type I interface {\n\tshown()\n}\n\nfunc plain() {}\n\nfunc (A) own() {}\n\n\
                   func (A) both() {}\n\nfunc (B) both() {}\n\nfunc (A) shown() {}\n\n\
                   func holder() {\n\tf := func() {}\n\tf()\n}\n";
        let alone = over(go(), &[("a.go", src)], |code| {
            let (&path, &facts) = code.files().get_key_value("a.go").unwrap();
            let mut alone: Vec<String> = (facts.defs.iter())
                .filter(|d| matches!(d.kind, Kind::Function | Kind::Method | Kind::Inline))
                .filter(|d| {
                    code.runs_alone(Target {
                        path,
                        name: &d.name,
                    })
                })
                .map(|d| d.name.clone())
                .collect();
            alone.sort();
            alone
        });
        assert_eq!(alone, ["A.own", "holder", "plain"]);
    }

    #[test]
    fn test_arguments_do_not_supply_production_parameters_or_origins() {
        let sources = [
            ("a.py", "def wrap(r):\n    return r\nwrap(Alpha())\n"),
            ("test_a.py", "from a import wrap\nwrap(Beta())\n"),
        ];
        assert_eq!(held_by(py(), &sources, "a.py", "wrap", "r"), ["a.py Alpha"]);
        over(py(), &sources, |code| {
            let function = Target {
                path: "a.py",
                name: "wrap",
            };
            let paths = |args: Vec<(&str, &str, &Value)>| {
                args.into_iter()
                    .map(|(path, _, _)| path.to_string())
                    .collect::<Vec<_>>()
            };
            assert_eq!(paths(code.param_args(function, 0)), ["a.py"]);
            assert_eq!(
                paths(code.param_values(function, 0, None).unwrap()),
                ["a.py"]
            );
        });
    }

    #[test]
    fn a_parameter_bound_by_a_frame_takes_the_argument_of_that_call_alone() {
        let src = "def wrap(r):\n    return r\n\na = wrap(Alpha())\nb = wrap(Beta())\n";
        let sources = [("a.py", src)];
        assert_eq!(held_by(py(), &sources, "a.py", "", "a"), ["a.py Alpha"]);
        assert_eq!(held_by(py(), &sources, "a.py", "", "b"), ["a.py Beta"]);
        let callers = held_by(py(), &sources, "a.py", "wrap", "r");
        assert_eq!(callers, ["a.py Alpha", "a.py Beta"]);
    }

    #[test]
    fn a_repository_functions_return_is_followed_and_the_climbed_call_recorded() {
        let src = "def make():\n    return Client()\n\nc = make()\nprobe(c)\n";
        let sources = [("a.py", src)];
        assert_eq!(held_by_value(py(), &sources, "a.py"), ["a.py Client"]);
        assert_eq!(
            held_through(py(), &sources, "a.py"),
            ["a.py Client via make()"]
        );
    }

    #[test]
    fn two_clients_one_factory_makes_differ_by_the_call_that_made_each() {
        let src = "def mk(u):\n    return Client(u)\n\na = mk('x')\nb = mk('y')\nprobe([a, b])\n";
        let sources = [("a.py", src)];
        assert_eq!(held_by_value(py(), &sources, "a.py"), ["a.py Client"]);
        assert_eq!(
            held_through(py(), &sources, "a.py"),
            ["a.py Client via mk('x')", "a.py Client via mk('y')"]
        );
        let alike = "def mk(u):\n    return Client(u)\n\na = mk('x')\nb = mk('x')\nprobe([a, b])\n";
        assert_eq!(
            held_through(py(), &[("a.py", alike)], "a.py"),
            ["a.py Client via mk('x')", "a.py Client via mk('x')"]
        );
    }

    #[test]
    fn a_lists_items_are_followed_and_the_origins_sorted() {
        let sources = [
            ("b.py", "x = Bx()\n"),
            ("c.py", "y = Cy()\n"),
            ("a.py", "from c import y\nfrom b import x\np = [y, x]\n"),
        ];
        assert_eq!(
            held_by(py(), &sources, "a.py", "", "p"),
            ["b.py Bx", "c.py Cy"]
        );
    }

    #[test]
    fn a_class_attribute_assigned_once_is_followed_and_not_when_assigned_twice() {
        let init = "class S:\n    def __init__(self):\n        self.client = Client()\n";
        let go = "    def go(self):\n        pass\n";
        let once = format!("{init}{go}");
        let attribute = |src: &str| held_by(py(), &[("a.py", src)], "a.py", "S.go", "self.client");
        assert_eq!(attribute(&once), ["a.py Client"]);
        let reset = "    def reset(self):\n        self.client = Other()\n";
        let twice = format!("{init}{reset}{go}");
        assert_eq!(attribute(&twice), Vec::<String>::new());
    }

    #[test]
    fn a_parameter_of_an_outer_function_a_closure_captures_gives_nothing() {
        let outer = "def outer(r):\n    def inner():\n        return r\n    return inner()\n";
        let src = format!("{outer}\nx = outer(Maker())\nprobe(x)\n");
        assert_eq!(
            held_through(py(), &[("a.py", &src)], "a.py"),
            Vec::<String>::new()
        );
        let direct = "def outer(r):\n    return r\n\nx = outer(Maker())\nprobe(x)\n";
        let found = held_through(py(), &[("a.py", direct)], "a.py");
        assert_eq!(found, ["a.py Maker"]);
    }

    #[test]
    fn functions_that_return_each_other_end_with_no_origin() {
        let src = "def f():\n    return g()\n\ndef g():\n    return f()\n\np = f()\n";
        assert_eq!(
            held_by(py(), &[("a.py", src)], "a.py", "", "p"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn two_tables_over_one_file_each_keep_only_their_own_chains() {
        fn axum_table(origin: &str, method: &str) -> bool {
            origin.starts_with("axum.") && method == "layer"
        }
        fn actix_table(origin: &str, method: &str) -> bool {
            origin.starts_with("actix_web.") && method == "service"
        }
        let src = "use axum::Router;\nuse actix_web::App;\n\
                   fn make(f: bool) -> Router {\n    \
                   if f { return Router::new().layer(l); }\n    App::new().service(s)\n}\n\
                   fn main() {\n    probe(make(true));\n}\n";
        let sources = [("main.rs", src)];
        let asked = |passes| probe_passing(rust(), &sources, "main.rs", Shown::Nothing, passes);
        assert_eq!(asked(axum_table), ["main.rs Router.new = axum.Router.new"]);
        assert_eq!(asked(actix_table), ["main.rs App.new = actix_web.App.new"]);
        assert_eq!(asked(passes_nothing), Vec::<String>::new());
    }

    fn axum_main(defs: &str, body: &str) -> String {
        format!(
            "use axum::Router;\nuse actix_web::App;\n{defs}\nfn main() {{\n{body}\nprobe(r);\n}}\n"
        )
    }

    fn built(defs: &str, body: &str) -> (Vec<String>, Vec<String>) {
        let src = axum_main(defs, body);
        let sources = [("main.rs", src.as_str())];
        let plain = held_by_value(rust(), &sources, "main.rs");
        (plain, held_through(rust(), &sources, "main.rs"))
    }

    const ROUTER: &str = "main.rs Router.new = axum.Router.new";

    #[test]
    fn a_builder_chain_is_the_value_its_maker_made() {
        let (plain, _) = built(
            "",
            "let r = Router::new().route(\"/a\", get(h)).nest(\"/b\", x);",
        );
        assert_eq!(plain, [ROUTER]);
        let (plain, _) = built("", "let r = Router::new();");
        assert_eq!(plain, [ROUTER]);
    }

    #[test]
    fn a_chain_through_a_method_the_library_does_not_pass_on_gives_nothing() {
        let (plain, _) = built("", "let r = Router::new().route(\"/a\", h).frobnicate();");
        assert_eq!(plain, Vec::<String>::new());
    }

    #[test]
    fn a_step_only_a_name_match_found_gives_nothing_and_an_outside_constructor_keeps_its_method() {
        let defs = "struct Mine;\nimpl Mine { fn route(&self) -> Mine { Mine } }\n";
        let (plain, _) = built(defs, "let r = build().route(\"/a\");");
        assert_eq!(plain, Vec::<String>::new());
        let (plain, _) = built(defs, "let r = build().layer(l).route(\"/a\");");
        assert_eq!(plain, Vec::<String>::new());
        let (plain, _) = built(defs, "let r = Router::new().route(\"/a\");");
        assert_eq!(plain, [ROUTER]);
        let (plain, _) = built(defs, "let r = Router::new().layer(l).route(\"/a\");");
        assert_eq!(plain, [ROUTER]);
    }

    #[test]
    fn a_chain_through_a_repository_function_records_the_climbed_call() {
        let defs = "fn make() -> Router { Router::new().layer(l) }\n";
        let (plain, through) = built(defs, "let r = make().route(\"/a\");");
        assert_eq!(plain, [ROUTER]);
        assert_eq!(through, [format!("{ROUTER} via make()")]);
        let (plain, through) = built(defs, "let r = make();");
        assert_eq!(plain, [ROUTER]);
        assert_eq!(through, [format!("{ROUTER} via make()")]);
    }

    #[test]
    fn a_function_returning_two_chains_keeps_the_one_the_library_passes_on() {
        let defs = "fn make(f: bool) -> Router {\n    if f { return Router::new().frobnicate(); }\n    \
                    Router::default().route(\"/a\", h)\n}\n";
        let (plain, _) = built(defs, "let r = make(true);");
        assert_eq!(plain, ["main.rs Router.default = axum.Router.default"]);
    }

    #[test]
    fn the_methods_called_on_a_functions_result_are_judged_per_library_of_its_origins() {
        let defs =
            "fn make(f: bool) -> Router {\n    if f { return Router::new(); }\n    App::new()\n}\n";
        let (plain, _) = built(defs, "let r = make(true).layer(l);");
        assert_eq!(plain, [ROUTER]);
        let (plain, _) = built(defs, "let r = make(true).frobnicate();");
        assert_eq!(plain, Vec::<String>::new());
        let (plain, _) = built(defs, "let r = make(true);");
        assert_eq!(plain, [ROUTER, "main.rs App.new = actix_web.App.new"]);
    }

    #[test]
    fn a_path_written_callee_is_read_by_path_and_not_by_a_local_function_that_spells_its_head() {
        let src = "fn rocket() {}\nfn main() { rocket::build(); }\n";
        assert_eq!(
            read_names(rust(), &[("main.rs", src)], "main.rs"),
            ["rocket.build: external=- by_path=rocket.build direct=false import=-"]
        );
    }

    #[test]
    fn a_use_imported_module_function_is_direct() {
        let src = "use reqwest::Client;\nfn main() { Client::new(); }\n";
        assert_eq!(
            read_names(rust(), &[("main.rs", src)], "main.rs"),
            [
                "Client.new: external=reqwest.Client.new by_path=reqwest.Client.new direct=true \
              import=reqwest Client.new"
            ]
        );
        let module = "use reqwest;\nfn main() { reqwest::get(u); }\n";
        assert_eq!(
            read_names(rust(), &[("main.rs", module)], "main.rs"),
            [
                "reqwest.get: external=reqwest.get by_path=reqwest.get direct=true import=reqwest get"
            ]
        );
        let item = "use reqwest::get;\nfn main() { get(u); }\n";
        assert_eq!(
            read_names(rust(), &[("main.rs", item)], "main.rs"),
            ["get: external=reqwest.get by_path=reqwest.get direct=true import=reqwest get"]
        );
    }

    #[test]
    fn a_module_function_is_direct_and_a_method_of_a_client_made_from_the_module_is_not() {
        let src = "package x\nimport \"github.com/gin-gonic/gin\"\nfunc f() {\n\
                   \tgin.SetMode(\"x\")\n\tr := gin.Default()\n\tr.GET(\"/a\", h)\n}\n";
        let gin = "github.com/gin-gonic/gin";
        assert_eq!(
            read_names(go(), &[("a.go", src)], "a.go"),
            [
                format!(
                    "gin.SetMode: external={gin}.SetMode by_path={gin}.SetMode direct=true import={gin} SetMode"
                ),
                format!(
                    "gin.Default: external={gin}.Default by_path={gin}.Default direct=true import={gin} Default"
                ),
                "r.GET: external=r.GET by_path=r.GET direct=false import=-".to_string(),
            ]
        );
    }

    #[test]
    fn a_method_of_a_typed_local_from_another_package_is_not_direct() {
        let src = "package x\nimport \"net/http\"\nfunc f(c *http.Client) {\n\tc.Get(\"u\")\n\
                   \tvar d http.Client\n\td.Do(1)\n\thttp.Get(\"u\")\n}\n";
        assert_eq!(
            read_names(go(), &[("a.go", src)], "a.go"),
            [
                "c.Get: external=c.Get by_path=c.Get direct=false import=-",
                "d.Do: external=d.Do by_path=d.Do direct=false import=-",
                "http.Get: external=net/http.Get by_path=net/http.Get direct=true import=net/http Get",
            ]
        );
        let other = "package x\nfunc f() {\n\te := other()\n\te.Default()\n}\n";
        let found = read_names(go(), &[("a.go", other)], "a.go");
        let none = "-";
        assert_eq!(
            found,
            [
                format!("other: external={none} by_path={none} direct=false import={none}"),
                format!("e.Default: external={none} by_path={none} direct=false import={none}"),
            ]
        );
    }

    #[test]
    fn a_name_reexported_through_a_repository_file_carries_the_package_the_name_and_the_files_spelling()
     {
        let py_files = [
            ("b.py", "from fastapi import APIRouter as Base\n"),
            ("a.py", "from b import Base as R\nR()\n"),
        ];
        assert_eq!(
            read_names(py(), &py_files, "a.py"),
            ["R: external=b.Base by_path=b.Base direct=true import=fastapi APIRouter"]
        );
        let ts_files = [
            ("b.ts", "export { Kafka as K } from 'kafkajs';\n"),
            ("a.ts", "import { K as Q } from './b';\nQ();\n"),
        ];
        assert_eq!(
            read_names(ts(), &ts_files, "a.ts"),
            ["Q: external=./b.K by_path=./b.K direct=true import=kafkajs Kafka"]
        );
    }

    #[test]
    fn the_kafkajs_producer_chain_is_spelled_and_the_held_constant_named() {
        let src = "import { Kafka } from 'kafkajs';\nconst kafka = new Kafka({ brokers: [] });\n\
                   const producer = kafka.producer();\nconst consumer = kafka.consumer({});\n";
        let sources = [("a.ts", src)];
        let origin = |chain| constant_origin(&sources, "", chain);
        assert_eq!(
            origin("producer").unwrap(),
            "kafkajs new Kafka().producer() held a.ts:producer"
        );
        assert_eq!(
            origin("consumer").unwrap(),
            "kafkajs new Kafka().consumer() held a.ts:consumer"
        );
        assert_eq!(
            origin("kafka").unwrap(),
            "kafkajs new Kafka() held a.ts:kafka"
        );
        assert_eq!(origin("producer.send"), None);
    }

    #[test]
    fn the_amqplib_channel_chain_is_spelled_under_a_default_and_a_namespace_import() {
        let body =
            "const conn = await amqp.connect('u');\nconst ch = await conn.createChannel();\n";
        let default = format!("import amqp from 'amqplib';\n{body}");
        let namespace = format!("import * as amqp from 'amqplib';\n{body}");
        assert_eq!(
            constant_origin(&[("a.ts", &default)], "", "ch").unwrap(),
            "amqplib default.connect().createChannel() held a.ts:ch"
        );
        assert_eq!(
            constant_origin(&[("a.ts", &namespace)], "", "ch").unwrap(),
            "amqplib connect().createChannel() held a.ts:ch"
        );
    }

    #[test]
    fn a_parameter_hides_a_module_constant_of_its_name_from_the_constants_walk() {
        let src = "import { Kafka } from 'kafkajs';\nconst kafka = new Kafka({});\n\
                   function f(kafka) { return kafka.producer(); }\n";
        let sources = [("a.ts", src)];
        assert_eq!(constant_origin(&sources, "f", "kafka"), None);
        assert_eq!(constant_origin(&sources, "f", "kafka.producer"), None);
        assert_eq!(
            constant_origin(&sources, "", "kafka").unwrap(),
            "kafkajs new Kafka() held a.ts:kafka"
        );
    }

    #[test]
    fn a_parameter_a_repository_factory_a_star_import_and_a_constant_cycle_give_no_origin() {
        let none = |sources: &[(&'static str, &str)], scope, chain| {
            assert_eq!(constant_origin(sources, scope, chain), None, "{chain}");
        };
        none(
            &[("a.ts", "function f(client) { client.send(); }\n")],
            "f",
            "client",
        );
        let factory = "import { Kafka } from 'kafkajs';\nfunction mk() { return new Kafka({}); }\n\
                       const k = mk();\nconst p = k.producer();\n";
        none(&[("a.ts", factory)], "", "p");
        let user =
            "import { Kafka } from './b';\nconst k = new Kafka({});\nconst p = k.producer();\n";
        none(
            &[("b.ts", "export * from 'kafkajs';\n"), ("a.ts", user)],
            "",
            "p",
        );
        let named = [
            ("b.ts", "export { Kafka } from 'kafkajs';\n"),
            ("a.ts", user),
        ];
        assert!(constant_origin(&named, "", "p").is_some());
        none(&[("a.ts", "const a = b;\nconst b = a;\n")], "", "a");
    }

    #[test]
    fn a_written_outside_type_is_the_origin_and_the_type() {
        let src =
            "package x\nimport \"github.com/segmentio/kafka-go\"\nfunc f(w *kafka.Writer) {}\n";
        let kafka = "github.com/segmentio/kafka-go.Writer";
        assert_eq!(
            declared(src, "f", "w"),
            format!("origin={kafka} type={kafka}")
        );
    }

    #[test]
    fn a_value_declared_from_an_outside_call_has_the_methods_on_the_way() {
        let src = "package x\nimport amqp \"github.com/rabbitmq/amqp091-go\"\nfunc f() {\n\
                   \tconn, _ := amqp.Dial(\"u\")\n\tch, _ := conn.Channel()\n}\n";
        let amqp = "github.com/rabbitmq/amqp091-go";
        assert_eq!(
            declared(src, "f", "conn"),
            format!("origin={amqp}.Dial type=-")
        );
        assert_eq!(
            declared(src, "f", "ch"),
            format!("origin={amqp}.Dial.Channel type=-")
        );
    }

    #[test]
    fn a_reference_declares_what_its_name_does_and_another_value_declares_nothing() {
        let src = "package x\nimport amqp \"github.com/rabbitmq/amqp091-go\"\nfunc f() {\n\
                   \tconn, _ := amqp.Dial(\"u\")\n\tch, _ := conn.Channel()\n}\n";
        let reference: &'static Value = Box::leak(Box::new(Value::Ref("ch".into())));
        over(go(), &[("a.go", src)], |code| {
            let path = code.files().get_key_value("a.go").unwrap().0;
            let ask = |of| code.origins(path, "f", of, Follow::Declarations);
            let by_name = ask(Of::Name("ch"));
            assert_eq!(by_name.len(), 1);
            assert_eq!(ask(Of::Value(reference)), by_name);
            assert_eq!(ask(Of::Value(&Value::Undefined)), []);
        });
    }

    #[test]
    fn a_name_declared_from_a_repository_function_has_the_type_its_result_writes() {
        let head = "package x\nimport \"github.com/segmentio/kafka-go\"\n";
        let outside = format!(
            "{head}func mk() *kafka.Writer {{ return nil }}\nfunc f() {{\n\tw := mk()\n}}\n"
        );
        let kafka = "github.com/segmentio/kafka-go.Writer";
        assert_eq!(
            declared(&outside, "f", "w"),
            format!("origin={kafka} type=-")
        );
        // A function is no value of the repository's own type: its result decides, and a type of
        let local = format!(
            "{head}type T struct{{}}\nfunc mk() *T {{ return nil }}\nfunc f() {{\n\tw := mk()\n}}\n"
        );
        assert_eq!(declared(&local, "f", "w"), "origin=- type=-");
    }

    #[test]
    fn a_builder_chain_carries_its_steps_maker_first() {
        let src = axum_main(
            "",
            "let r = Router::new().route(\"/a\", get(h)).nest(\"/b\", x);",
        );
        let found = probe_routes(rust(), &[("main.rs", &src)], "main.rs");
        assert_eq!(
            found,
            [format!("{ROUTER} steps=route,nest held=main.rs:main.r")]
        );
        let defs = "fn make() -> Router { Router::new().layer(l) }\n";
        let src = axum_main(defs, "let r = make().route(\"/a\");");
        let found = probe_routes(rust(), &[("main.rs", &src)], "main.rs");
        assert_eq!(
            found,
            [format!(
                "{ROUTER} steps=layer,route held=main.rs:main.r via make()"
            )]
        );
    }

    #[test]
    fn each_route_holds_the_constant_the_asked_name_refers_to() {
        let sources = [("a.py", "p = make()\n")];
        assert_eq!(
            routes_of(py(), &sources, "a.py", "", "p"),
            ["a.py make held=a.py:p"]
        );
        let list = [("a.py", "a = Alpha()\nb = Beta()\nprobe([a, b])\n")];
        let found = probe_routes(py(), &list, "a.py");
        assert_eq!(found, ["a.py Alpha held=a.py:a", "a.py Beta held=a.py:b"]);
        assert_eq!(probe_routes(py(), &list, "a.py"), found);
        let named = [("a.py", "a = Alpha()\nb = Beta()\np = [b, a]\n")];
        assert_eq!(
            routes_of(py(), &named, "a.py", "", "p"),
            ["a.py Alpha held=a.py:p", "a.py Beta held=a.py:p"]
        );
        let param = [(
            "a.py",
            "def wrap(r):\n    return r\n\nx = Alpha()\nwrap(x)\n",
        )];
        assert_eq!(routes_of(py(), &param, "a.py", "wrap", "r"), ["a.py Alpha"]);
    }

    #[test]
    fn the_name_reading_carries_the_package_the_spelling_and_whether_the_head_is_the_import() {
        let py_files = [
            ("b.py", "from fastapi import APIRouter as Base\n"),
            ("a.py", "from b import Base as R\nR()\n"),
        ];
        assert_eq!(
            outside_of(py(), &py_files, "a.py", "R", false).unwrap(),
            "fastapi APIRouter; b.Base; direct=true"
        );
        let src = "package x\nimport \"github.com/gin-gonic/gin\"\nfunc f() {\n\
                   \tr := gin.Default()\n\tr.GET(\"/a\", h)\n}\n";
        let gin = "github.com/gin-gonic/gin";
        let go_files = [("a.go", src)];
        assert_eq!(
            outside_of(go(), &go_files, "a.go", "gin.Default", false).unwrap(),
            format!("{gin} Default; {gin}.Default; direct=true")
        );
        assert_eq!(
            outside_of(go(), &go_files, "a.go", "r.GET", false).unwrap(),
            "-; r.GET; direct=false"
        );
        let rocket = [(
            "main.rs",
            "fn rocket() {}\nfn main() { rocket::build(); }\n",
        )];
        assert_eq!(
            outside_of(rust(), &rocket, "main.rs", "rocket.build", false),
            None
        );
        assert_eq!(
            outside_of(rust(), &rocket, "main.rs", "rocket.build", true).unwrap(),
            "rocket build; rocket.build; direct=true"
        );
    }

    #[test]
    fn a_written_type_is_the_form_type_and_a_repository_functions_result_type_the_form_call() {
        let head = "package x\nimport \"github.com/segmentio/kafka-go\"\n";
        let kafka = "github.com/segmentio/kafka-go.Writer";
        let typed = format!("{head}func f(w *kafka.Writer) {{}}\n");
        assert_eq!(
            declared_form(&typed, "f", "w"),
            (kafka.to_string(), kafka.to_string(), "Type".to_string())
        );
        let made = format!(
            "{head}func mk() *kafka.Writer {{ return nil }}\nfunc f() {{\n\tw := mk()\n}}\n"
        );
        assert_eq!(
            declared_form(&made, "f", "w"),
            (kafka.to_string(), "-".to_string(), "Call".to_string())
        );
        let src = "package x\nimport amqp \"github.com/rabbitmq/amqp091-go\"\nfunc f() {\n\
                   \tconn, _ := amqp.Dial(\"u\")\n\tch, _ := conn.Channel()\n}\n";
        let amqp = "github.com/rabbitmq/amqp091-go";
        assert_eq!(
            declared_form(src, "f", "ch"),
            (
                format!("{amqp}.Dial.Channel"),
                "-".to_string(),
                "Call".to_string()
            )
        );
    }

    #[test]
    fn what_the_constants_walk_does_not_express_has_no_origin() {
        let head = "import { Kafka } from 'kafkajs';\nconst kafka = new Kafka({});\n";
        let none = |body: &str, chain| {
            let src = format!("{head}{body}");
            assert_eq!(
                constant_origin(&[("a.ts", &src)], "", chain),
                None,
                "{body}"
            );
        };
        none("const p = kafka.producer;\n", "p");
        none("const p = kafka.a.producer();\n", "p");
        none("const p = new kafka.producer();\n", "p");
        none("const p = kafka.producer();\nconst q = new p();\n", "q");
        none("const p = new (kafka.producer())();\n", "p");
        none("const p = kafka.producer();\nconst q = p();\n", "q");
        let named = "import * as k from 'kafkajs';\nconst c = new k.Kafka({});\n";
        assert_eq!(
            constant_origin(&[("a.ts", named)], "", "c").unwrap(),
            "kafkajs new Kafka() held a.ts:c"
        );
    }

    #[test]
    fn a_chain_of_constants_past_the_limit_has_no_origin() {
        let origin = |n: usize| {
            let consts = (0..n).map(|i| format!("const c{} = c{i};\n", i + 1));
            let src = format!(
                "import {{ Kafka }} from 'kafkajs';\nconst c0 = new Kafka({{}});\n{}",
                consts.collect::<String>()
            );
            let chain: &'static str = format!("c{n}").leak();
            constant_origin(&[("a.ts", &src)], "", chain)
        };
        let held = format!("kafkajs new Kafka() held a.ts:c{}", EVAL_DEPTH - 1);
        assert_eq!(origin(EVAL_DEPTH - 1), Some(held));
        assert_eq!(origin(EVAL_DEPTH), None);
        assert_eq!(origin(20 * EVAL_DEPTH), None);
    }

    #[test]
    fn a_constructed_constant_is_read_at_the_wrappers_call_site_and_in_place() {
        let head = "import { Queue } from 'bullmq';\n";
        let in_place = format!("{head}const q = new Queue('jobs');\nq.add('x', {{}});\n");
        assert_eq!(
            constructed_sites(&in_place, "q", "q.add"),
            ["q.add -> jobs"]
        );
        let wrapper = "function send(name) { const q = new Queue(name); q.add('x', {}); }\n\
                       send('jobs');\nsend('mail');\n";
        assert_eq!(
            constructed_sites(&format!("{head}{wrapper}"), "send.q", "q.add"),
            ["send -> jobs", "send -> mail"]
        );
    }

    fn java_calls(src: &str) -> BTreeMap<String, String> {
        pack_calls(&crate::lang::java::Java, "A.java", src)
    }

    fn pack_calls(pack: &dyn LanguagePack, path: &str, src: &str) -> BTreeMap<String, String> {
        let facts = pack.parse(path, src.as_bytes()).unwrap();
        let files = BTreeMap::from([(path, &facts)]);
        let resolver = Resolver::new(&files, Box::new(|_, _| std::rc::Rc::default()), pack);
        let mut out = BTreeMap::new();
        for call in &facts.calls {
            let found = match resolver.call(path, call) {
                Resolution::Exact(t) => format!("exact {}", t.name),
                Resolution::Inferred(t) | Resolution::Narrowed(t) => format!("inferred {}", t.name),
                Resolution::Ambiguous(all) => {
                    let names: Vec<_> = all.iter().map(|t| t.name).collect();
                    format!("ambiguous {}", names.join(" "))
                }
                Resolution::Unresolved => "none".to_string(),
            };
            out.insert(format!("{} {}", call.scope, call.callee), found);
        }
        out
    }

    #[test]
    fn a_rust_call_after_try_or_await_is_typed_by_the_type_they_unwrap() {
        let calls = pack_calls(
            &crate::lang::rust::Rust,
            "src/lib.rs",
            "use anyhow::Result;
             use std::future::Future;
             use std::pin::Pin;
             pub struct Cfg;
             impl Cfg { pub fn port(&self) {} }
             pub struct Other;
             impl Other { pub fn port(&self) {} pub fn send(&self) {} }
             pub fn load() -> Result<Cfg> { todo!() }
             pub fn opt() -> Option<Cfg> { todo!() }
             pub fn cfg() -> Cfg { Cfg }
             pub fn fut() -> impl Future<Output = Cfg> { async { Cfg } }
             pub fn boxed() -> Pin<Box<dyn Future<Output = Cfg>>> { todo!() }
             pub fn tried() -> impl Future<Output = Result<Cfg>> { todo!() }
             impl Cfg { pub fn map(&self) {} }
             pub async fn asynced() -> Result<Cfg> { todo!() }
             pub fn outside() -> Result<reqwest::Client> { todo!() }
             pub fn later() -> impl Future<Output = reqwest::Client> { todo!() }
             pub async fn a() -> Result<()> { load()?.port(); Ok(()) }
             pub fn b() -> Option<()> { opt()?.port(); None }
             pub async fn c() { (cfg()).port(); }
             pub async fn d() { fut().await.port(); }
             pub async fn e() { boxed().await.port(); }
             pub async fn f() -> Result<()> { tried().await?.port(); Ok(()) }
             pub async fn k() { fut().map(); }
             pub async fn g() -> Result<()> { asynced().await?.port(); Ok(()) }
             pub fn h() -> Result<()> { outside()?.send(); Ok(()) }
             pub async fn i() { later().await.send(); }
             pub fn j() -> Result<()> { load()??.port(); Ok(()) }
             pub async fn l() -> Result<()> { load()?.await.port(); Ok(()) }",
        );
        let exact = "exact Cfg.port";
        assert_eq!(calls["a (try_expression).port"], exact);
        assert_eq!(calls["b (try_expression).port"], exact);
        assert_eq!(calls["c (parenthesized_expression).port"], exact);
        assert_eq!(calls["d (await_expression).port"], exact);
        assert_eq!(calls["e (await_expression).port"], exact);
        assert_eq!(
            calls["f (try_expression).port"],
            "ambiguous Cfg.port Other.port"
        );
        assert_eq!(calls["k (call_expression).map"], "none");
        assert_eq!(calls["g (try_expression).port"], exact);
        assert_eq!(calls["h (try_expression).send"], "inferred Other.send");
        assert_eq!(calls["i (await_expression).send"], "none");
        assert_eq!(
            calls["j (try_expression).port"],
            "ambiguous Cfg.port Other.port"
        );
        assert_eq!(
            calls["l (await_expression).port"],
            "ambiguous Cfg.port Other.port"
        );
    }

    #[test]
    fn a_rust_chain_of_calls_on_call_results_is_read_in_linear_time() {
        let n = 4 * RECEIVER_DEPTH;
        let src = format!(
            "pub struct T;\nimpl T {{ pub fn new() -> Self {{ T }} pub fn m(self) -> Self {{ self }} }}\n\
             pub fn f() {{ T::new(){}; }}\n",
            ".m()".repeat(n)
        );
        let start = std::time::Instant::now();
        let calls = pack_calls(&crate::lang::rust::Rust, "src/lib.rs", &src);
        let took = start.elapsed();
        assert_eq!(calls["f T.new"], "exact T.new");
        assert!(took.as_secs() < 10, "{took:?}");
    }

    #[test]
    fn a_rust_chain_of_calls_through_try_is_read_in_linear_time() {
        let n = 4 * RECEIVER_DEPTH;
        let src = format!(
            "pub struct T;\nimpl T {{ pub fn new() -> Option<T> {{ None }} pub fn m(self) -> Option<T> {{ None }} }}\n\
             pub fn f() -> Option<()> {{ T::new(){}; None }}\n",
            "?.m()".repeat(n)
        );
        let start = std::time::Instant::now();
        let calls = pack_calls(&crate::lang::rust::Rust, "src/lib.rs", &src);
        let took = start.elapsed();
        assert_eq!(calls["f T.new"], "exact T.new");
        assert_eq!(calls["f (try_expression).m"], "exact T.m");
        assert!(took.as_secs() < 10, "{took:?}");
    }

    #[test]
    fn a_python_chain_of_receivers_across_globs_is_bounded_by_the_receiver_depth() {
        let n = RECEIVER_DEPTH + 8;
        let mut sources = vec![(
            "base.py".to_string(),
            "class K:\n    class C(K):\n        def m(self):\n            pass\n".to_string(),
        )];
        for i in 0..n {
            let rest = match i + 1 == n {
                true => format!("v{i} = K()\n"),
                false => format!("from m{} import *\nv{i} = v{}.C()\n", i + 1, i + 1),
            };
            sources.push((format!("m{i}.py"), format!("from base import *\n{rest}")));
        }
        let parsed: Vec<(&str, Facts)> = (sources.iter())
            .map(|(p, s)| (p.as_str(), Python.parse(p, s.as_bytes()).unwrap()))
            .collect();
        let files: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let modules = Python.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &Python);
        let start = std::time::Instant::now();
        let exact: Vec<bool> = (parsed.iter())
            .flat_map(|(path, facts)| facts.calls.iter().map(move |call| (*path, call)))
            .map(|(path, call)| matches!(resolver.call(path, call), Resolution::Exact(_)))
            .collect();
        let took = start.elapsed();
        assert!(took.as_secs() < 10, "{took:?}");
        for (i, known) in exact.iter().enumerate() {
            assert_eq!(*known, i + RECEIVER_DEPTH + 1 >= n, "call {i} of {n}");
        }
    }

    #[test]
    fn a_swift_chain_of_receivers_through_typealiases_counts_them_across_each_alias() {
        let n = RECEIVER_DEPTH / 2 + 8;
        assert!(n < LINK_DEPTH);
        let mut src = String::from("class K { class B: K { func m() {} } }\n");
        for i in 0..n {
            let q = match i + 1 == n {
                true => "K()".to_string(),
                false => format!("A{}()", i + 1),
            };
            src.push_str(&format!(
                "typealias A{i} = H{i}.r.B\n\
                 enum H{i} {{ static let q = {q}; static let r = q.B() }}\n\
                 func f{i}() {{ H{i}.r.m() }}\n"
            ));
        }
        let start = std::time::Instant::now();
        let calls = pack_calls(&crate::lang::swift::Swift, "A.swift", &src);
        let took = start.elapsed();
        assert!(took.as_secs() < 10, "{took:?}");
        for i in 0..n {
            let known = calls[&format!("f{i}() H{i}.r.m")] == "exact K.B.m()";
            assert_eq!(known, 2 * (n - i) <= RECEIVER_DEPTH, "call {i} of {n}");
        }
    }

    #[test]
    fn an_outside_base_anywhere_in_the_family_keeps_a_member_from_exact() {
        let calls = java_calls(
            "package p;
             class Item {}
             interface Port { void f(String s); }
             abstract class Own extends Remote { void g(int n) {} void run() { this.g(1); g(1); } }
             abstract class Past extends Remote implements Port {
                 void f(int n) {}
                 void run() { this.f(1); f(1); }
             }
             class Names extends ArrayList<Item> {
                 boolean add(String s) { return add(new Item()); }
                 void run(Names n) { n.add(\"x\"); }
             }",
        );
        assert_eq!(calls["p.Own.run() this.g"], "inferred p.Own.g(int)");
        assert_eq!(calls["p.Own.run() g"], "inferred p.Own.g(int)");
        let both = "ambiguous p.Past.f(int) p.Port.f(String)";
        assert_eq!(calls["p.Past.run() this.f"], both);
        assert_eq!(calls["p.Past.run() f"], both);
        assert_eq!(
            calls["p.Names.add(String) add"],
            "inferred p.Names.add(String)"
        );
        assert_eq!(
            calls["p.Names.run(Names) n.add"],
            "inferred p.Names.add(String)"
        );
    }

    #[test]
    fn a_known_outside_base_hides_only_the_names_it_declares() {
        let calls = java_calls(
            "package p;
             import java.io.Serializable;
             class Ticket implements Serializable, Comparable<Ticket> {
                 void open() {} void open(int n) {}
                 public int compareTo(Ticket t) { return 0; }
                 void run(Ticket t) { t.open(1); open(); this.open(1); }
                 void cmp(Ticket t) { t.compareTo(t); compareTo(t); }
                 void none(Ticket t) { t.missing(); }
             }
             class Late extends Ticket { void go() { open(1); } }
             class Other implements com.x.Serializable { void f() {} void g() { f(); } }
             class Both implements Serializable, Remote { void f() {} void g() { f(); } }",
        );
        assert_eq!(
            calls["p.Ticket.run(Ticket) t.open"],
            "exact p.Ticket.open(int)"
        );
        assert_eq!(calls["p.Ticket.run(Ticket) open"], "exact p.Ticket.open()");
        assert_eq!(
            calls["p.Ticket.run(Ticket) this.open"],
            "exact p.Ticket.open(int)"
        );
        let compare = "inferred p.Ticket.compareTo(Ticket)";
        assert_eq!(calls["p.Ticket.cmp(Ticket) t.compareTo"], compare);
        assert_eq!(calls["p.Ticket.cmp(Ticket) compareTo"], compare);
        assert_eq!(calls["p.Ticket.none(Ticket) t.missing"], "none");
        assert_eq!(calls["p.Late.go() open"], "exact p.Ticket.open(int)");
        assert_eq!(calls["p.Other.g() f"], "inferred p.Other.f()");
        assert_eq!(calls["p.Both.g() f"], "inferred p.Both.f()");
    }

    #[test]
    fn listed_swift_protocols_hide_only_the_names_they_declare() {
        let calls = pack_calls(
            &crate::lang::swift::Swift,
            "A.swift",
            "protocol Builder { func add(_ x: Int) }
             protocol Service {}
             extension Builder {
                 func get(_ path: String..., note: String? = nil, use closure: (Int) -> Int) -> Int { 0 }
                 func get(_ path: [String], note: String? = nil, use closure: (Int) -> Int) -> Int { 0 }
             }
             final class App: Sendable, Service {}
             extension App: Builder { func add(_ x: Int) {} }
             final class Plain: Service {}
             extension Plain: Builder { func add(_ x: Int) {} }
             func f(_ x: Int) -> Int { x }
             func trail(app: App, plain: Plain) { app.get(\"a\", \"b\") { _ in 1 }; plain.get(\"a\", \"b\") { _ in 1 } }
             func use(app: App, plain: Plain) { app.get(\"a\", use: f); plain.get(\"a\", use: f); app.add(1); plain.add(1) }
             struct Point: Hashable {
                 func scaled(by k: Int) -> Point { self }
                 func hash(into hasher: inout Hasher) {}
             }
             func runPoint(p: Point, h: inout Hasher) { _ = p.scaled(by: 2); p.hash(into: &h) }
             struct Late { func scaled(by k: Int) -> Late { self } }
             extension Late: Hashable { func hash(into hasher: inout Hasher) {} }
             func runLate(l: Late, h: inout Hasher) { _ = l.scaled(by: 2); l.hash(into: &h) }
             final class Mixed: Sendable, CustomDebugStringConvertible { func go() {} }
             func runMixed(m: Mixed) { m.go() }",
        );
        let variadic = "Builder.get(_:String,note:String?,use:(Int)->Int)";
        let both =
            format!("ambiguous {variadic} Builder.get(_:[String],note:String?,use:(Int)->Int)");
        for receiver in ["app", "plain"] {
            let trail = format!("trail(app:plain:) {receiver}.get");
            assert_eq!(calls[&trail], format!("exact {variadic}"), "{receiver}");
            assert_eq!(
                calls[&format!("use(app:plain:) {receiver}.get")],
                both,
                "{receiver}"
            );
        }
        assert_eq!(calls["use(app:plain:) app.add"], "exact App.add(_:)");
        assert_eq!(calls["use(app:plain:) plain.add"], "exact Plain.add(_:)");
        assert_eq!(calls["runPoint(p:h:) p.scaled"], "exact Point.scaled(by:)");
        assert_eq!(calls["runPoint(p:h:) p.hash"], "inferred Point.hash(into:)");
        assert_eq!(calls["runMixed(m:) m.go"], "inferred Mixed.go()");
        assert_eq!(calls["runLate(l:h:) l.scaled"], "exact Late.scaled(by:)");
        assert_eq!(calls["runLate(l:h:) l.hash"], "inferred Late.hash(into:)");
        let own = pack_calls(
            &crate::lang::swift::Swift,
            "A.swift",
            "protocol Hashable {}
             struct Own: Hashable { func hash(into n: inout Int) {} }
             func run(o: Own, n: inout Int) { o.hash(into: &n) }",
        );
        assert_eq!(own["run(o:n:) o.hash"], "exact Own.hash(into:)");
    }

    #[test]
    fn a_swift_call_is_compared_with_a_known_outside_member_by_its_labels() {
        let calls = pack_calls(
            &crate::lang::swift::Swift,
            "A.swift",
            "struct P: Hashable { func hash(seed: Int) {} }
             func own(p: P) { p.hash(seed: 1) }
             func declared(p: P, h: inout Hasher) { p.hash(into: &h) }
             func closure(p: P) { p.hash { 1 } }
",
        );
        assert_eq!(calls["own(p:) p.hash"], "exact P.hash(seed:)");
        assert_eq!(calls["declared(p:h:) p.hash"], "none");
        assert_eq!(calls["closure(p:) p.hash"], "inferred P.hash(seed:)");
    }

    #[test]
    fn a_swift_member_read_on_an_outside_name_is_opaque() {
        let calls = pack_calls(
            &crate::lang::swift::Swift,
            "A.swift",
            "struct Cache { func data(from key: String) -> Int { 0 } }
             enum Remote { static let shared = Cache() }
             func outside() {
                 let session = URLSession.shared
                 _ = session.data(from: \"k\")
                 _ = URLSession.shared.data(from: \"k\")
             }
             func inside() {
                 let cache = Remote.shared
                 _ = cache.data(from: \"k\")
             }
             struct Holder { let cache = Cache() }
             func viaParam(h: Holder) {
                 let c = h.cache
                 _ = c.data(from: \"k\")
             }
             func again(other: URLSession) {
                 var client = URLSession.shared
                 client = other
                 _ = client.data(from: \"k\")
             }",
        );
        assert_eq!(calls["outside() session.data"], "none");
        assert_eq!(calls["outside() URLSession.shared.data"], "none");
        assert_eq!(calls["inside() cache.data"], "inferred Cache.data(from:)");
        assert_eq!(
            calls["again(other:) client.data"],
            "inferred Cache.data(from:)"
        );
    }

    #[test]
    fn a_bare_call_past_an_outer_opaque_base_is_a_name_match() {
        let calls = java_calls(
            "package p;
             abstract class Listing extends ArrayList<String> {
                 void own() {}
                 class Plain { void run() { own(); } }
                 class Task implements Runnable { public void run() { own(); } }
             }
             class Mine { void own() {} class Plain { void run() { own(); } } }",
        );
        assert_eq!(
            calls["p.Listing.Plain.run() own"],
            "inferred p.Listing.own()"
        );
        assert_eq!(
            calls["p.Listing.Task.run() own"],
            "inferred p.Listing.own()"
        );
        assert_eq!(calls["p.Mine.Plain.run() own"], "exact p.Mine.own()");
    }

    #[test]
    fn a_nested_class_reads_the_outer_field_unless_it_declares_one_of_its_name() {
        let calls = java_calls(
            "package p;
             class A { void go() {} }
             class B { void go() {} }
             class Outer {
                 A repo;
                 void mine() { repo.go(); }
                 class Inner { void run() { repo.go(); } }
                 static class Nested { B repo; void run() { repo.go(); } }
             }",
        );
        assert_eq!(calls["p.Outer.mine() repo.go"], "exact p.A.go()");
        assert_eq!(calls["p.Outer.Inner.run() repo.go"], "exact p.A.go()");
        assert_eq!(calls["p.Outer.Nested.run() repo.go"], "exact p.B.go()");
    }

    #[test]
    fn a_redundant_interface_keeps_the_family_and_its_subclasses() {
        let calls = java_calls(
            "package p;
             interface I { void i(); }
             interface J extends I { void j(); }
             class C implements I, J {
                 public void i() {} public void j() {}
                 void run() { i(); this.j(); }
             }
             class D extends C { void go() { i(); this.j(); } }",
        );
        assert_eq!(calls["p.C.run() i"], "exact p.C.i()");
        assert_eq!(calls["p.C.run() this.j"], "exact p.C.j()");
        assert_eq!(calls["p.D.go() i"], "exact p.C.i()");
        assert_eq!(calls["p.D.go() this.j"], "exact p.C.j()");
    }

    #[test]
    fn a_name_match_reads_the_receivers_family_and_never_the_calling_method() {
        let calls = java_calls(
            "package p;
             class Order {}
             class OrderRepo extends Remote {}
             class Archive { void save(Order o) {} }
             class OrderService {
                 OrderRepo repo;
                 void save(Order o) {
                     repo.save(o);
                     java.util.function.Consumer<Object> each = x -> x.save(o);
                 }
             }",
        );
        assert_eq!(calls["p.OrderService.save(Order) repo.save"], "none");
        let both = "ambiguous p.Archive.save(Order) p.OrderService.save(Order)";
        assert_eq!(calls["p.OrderService.save(Order) x.save"], both);
    }

    #[test]
    fn an_inherited_field_hides_an_outer_field() {
        let calls = java_calls(
            "package p;
             class Repo { void save() {} }
             class Other { void save() {} }
             class Base { protected Other repo; }
             class Outer {
                 Repo repo;
                 class Inner extends Base { void m() { repo.save(); } }
                 class Far extends Remote { void m() { repo.save(); } }
             }
             class Sub extends Base { void m() { repo.save(); } }",
        );
        assert_eq!(calls["p.Outer.Inner.m() repo.save"], "exact p.Other.save()");
        assert_eq!(calls["p.Sub.m() repo.save"], "exact p.Other.save()");
        assert_eq!(
            calls["p.Outer.Far.m() repo.save"],
            "ambiguous p.Other.save() p.Repo.save()"
        );
    }

    #[test]
    fn a_field_and_a_method_of_one_name_keep_their_own_namespaces() {
        let calls = java_calls(
            "package p;
             class Repo { void save() {} }
             class Other { void save() {} }
             class Svc {
                 Repo repo;
                 Other repo() { return null; }
                 void run() { repo.save(); this.repo.save(); repo(); this.repo(); }
             }",
        );
        assert_eq!(calls["p.Svc.run() repo.save"], "exact p.Repo.save()");
        assert_eq!(calls["p.Svc.run() this.repo.save"], "exact p.Repo.save()");
        assert_eq!(calls["p.Svc.run() repo"], "exact p.Svc.repo()");
        assert_eq!(calls["p.Svc.run() this.repo"], "exact p.Svc.repo()");
    }

    #[test]
    fn a_call_on_a_call_takes_the_declared_return_type_of_the_overload_called() {
        let calls = java_calls(
            "package p;
             class Repo { void save() {} Repo self() { return this; } }
             class Other { void save() {} }
             class Maker {
                 Repo make() { return null; }
                 Other make(int a, int b) { return null; }
                 void one() { make().save(); }
                 void two() { make().self().save(); }
                 void three() { var x = make(1, 2); x.save(); }
             }",
        );
        assert_eq!(
            calls["p.Maker.one() (method_invocation).save"],
            "exact p.Repo.save()"
        );
        assert_eq!(
            calls["p.Maker.two() (method_invocation).save"],
            "exact p.Repo.save()"
        );
        assert_eq!(calls["p.Maker.three() x.save"], "exact p.Other.save()");
    }

    #[test]
    fn a_super_call_reads_the_family_past_the_class() {
        let calls = java_calls(
            "package p;
             class Base { void f(int n) {} }
             class Kid extends Base { void f(int n) { super.f(n); } void g() { super.h(); } }
             class Ext extends Remote { void f(int n) { super.f(n); } }",
        );
        assert_eq!(calls["p.Kid.f(int) super.f"], "exact p.Base.f(int)");
        assert_eq!(calls["p.Kid.g() super.h"], "none");
        assert_eq!(calls["p.Ext.f(int) super.f"], "none");
    }

    #[test]
    fn initializer_locals_and_implicit_members_bind_no_outer_name() {
        let calls = java_calls(
            "package p;
             class Other { void save() {} }
             class Init {
                 { Other q = null; }
                 java.util.function.Supplier<Object> s = () -> { Other w = null; return w; };
                 void m() { q.save(); w.save(); }
             }
             class Outer { public String toString() { return null; } class In { void m() { toString(); } } }",
        );
        assert!(!calls["p.Init.m() q.save"].starts_with("exact"));
        assert!(!calls["p.Init.m() w.save"].starts_with("exact"));
        assert_eq!(calls["p.Outer.In.m() toString"], "none");
    }

    #[test]
    fn a_family_without_overloads_keeps_the_first_class_that_defines_the_name() {
        let src = "class A:\n    def f(self): pass\nclass B(A):\n    def f(self): pass\n\
                   class C(B, A):\n    def go(self): self.f()\n";
        let facts = Python.parse("a.py", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.py", &facts)]);
        let resolver = Resolver::new(&files, Box::new(|_, _| std::rc::Rc::default()), &Python);
        let call = facts.calls.iter().find(|c| c.scope == "C.go").unwrap();
        assert!(matches!(resolver.call("a.py", call), Resolution::Exact(t) if t.name == "B.f"));
    }

    #[test]
    fn a_receiver_whose_class_the_source_does_not_settle_is_no_exact_owner() {
        let calls = java_calls(
            "package p;
             class T { void f() {} }
             class Repo { void f() {} void h() {} }
             class Other { void f() {} void h() {} }
             class Box<T> { T item; void run() { item.f(); } }
             class Field {
                 Repo x;
                 void run() { x.f(); Other x = null; x.h(); }
             }
             class Blocks {
                 void run(boolean b) {
                     if (b) { Repo y = null; y.h(); } else { Other y = null; y.h(); }
                 }
             }
             class Anonymous {
                 void run() { var z = new Repo() { void f() {} }; z.f(); }
                 void field() { Object o = new Object() { Repo w; }; w.f(); }
             }
             class Cycle extends Loop { void run() { this.f(); } }
             class Loop extends Cycle { void f() {} }",
        );
        for call in [
            "p.Box.run() item.f",
            "p.Field.run() x.f",
            "p.Field.run() x.h",
            "p.Blocks.run(boolean) y.h",
            "p.Anonymous.run() z.f",
            "p.Anonymous.field() w.f",
            "p.Cycle.run() this.f",
        ] {
            assert!(!calls[call].starts_with("exact"), "{call}: {}", calls[call]);
        }
    }
}

#[cfg(test)]
mod csharp_tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::csharp::CSharp;

    const PROJECT: &[u8] = b"<Project Sdk=\"Microsoft.NET.Sdk\"></Project>";
    const ORDER: &str = "namespace P;\npublic class Order { public static Order Of() => null; \
                         public int Total() => 1; }\n";

    fn resolved(files: &[(&str, &str)]) -> Vec<(String, String, &'static str, Vec<String>)> {
        let configs: Vec<(&str, &[u8])> = ["Kit/Kit.csproj", "Kit2/Kit2.csproj", "App/App.csproj"]
            .into_iter()
            .map(|p| (p, PROJECT))
            .collect();
        resolved_in(files, &configs)
    }

    fn with_resolver<R>(
        files: &[(&str, &str)],
        configs: &[(&str, &[u8])],
        f: impl FnOnce(Resolver) -> R,
    ) -> R {
        let mut parsed: Vec<(&str, Facts)> = (files.iter())
            .map(|(p, s)| (*p, CSharp.parse(p, s.as_bytes()).unwrap()))
            .collect();
        let join = {
            let raw: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
            CSharp.join(configs, &raw)
        };
        for (path, facts) in &mut parsed {
            join(path, facts);
        }
        let files: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(configs, &files).unwrap();
        f(Resolver::with_shared(
            &files,
            layout.modules,
            layout.shared,
            &CSharp,
        ))
    }

    fn unbound_calls_against(files: usize) -> usize {
        let calls: String = (0..10_000).map(|i| format!("Missing{i}();\n")).collect();
        let app = format!("using P;\nnamespace Q;\nclass C {{ void F() {{\n{calls}}} }}\n");
        let mut sources: Vec<(String, String)> = (0..files)
            .map(|i| {
                let source = format!("namespace P;\npublic class T{i} {{}}\n");
                (format!("P/T{i}.cs"), source)
            })
            .collect();
        sources.push(("App/C.cs".into(), app));
        let sources: Vec<(&str, &str)> = sources.iter().map(|(p, s)| (&**p, &**s)).collect();
        with_resolver(&sources, &[("App/App.csproj", PROJECT)], |resolver| {
            let facts = resolver.files["App/C.cs"];
            CANDIDATES_VISITED.with(|n| n.set(0));
            for call in &facts.calls {
                let found = resolver.call("App/C.cs", call);
                assert!(matches!(found, Resolution::Unresolved));
            }
            CANDIDATES_VISITED.with(|n| n.get())
        })
    }

    #[test]
    fn unbound_calls_cost_the_same_whatever_the_namespaces_they_import_hold() {
        let (one, many) = (unbound_calls_against(1), unbound_calls_against(4000));
        assert!(one > 0, "the lookups no longer reach `Resolver::module`");
        assert!(many <= one, "{one} {many}");
    }

    fn resolved_in(
        files: &[(&str, &str)],
        configs: &[(&str, &[u8])],
    ) -> Vec<(String, String, &'static str, Vec<String>)> {
        with_resolver(files, configs, |resolver| {
            let files = resolver.files;
            let paths = |ts: &[Target]| ts.iter().map(|t| t.path.to_string()).collect();
            let mut out = Vec::new();
            for (&path, facts) in files {
                for call in &facts.calls {
                    let (level, targets) = match resolver.call(path, call) {
                        Resolution::Exact(t) => ("exact", paths(&[t])),
                        Resolution::Inferred(t) => ("inferred", paths(&[t])),
                        Resolution::Narrowed(t) => ("narrowed", paths(&[t])),
                        Resolution::Ambiguous(ts) => ("ambiguous", paths(&ts)),
                        Resolution::Unresolved => ("unresolved", Vec::new()),
                    };
                    out.push((path.to_string(), call.callee.clone(), level, targets));
                }
            }
            out
        })
    }

    fn families_in(
        files: &[(&str, &str)],
        configs: &[(&str, &[u8])],
    ) -> BTreeMap<String, Option<Vec<String>>> {
        with_resolver(files, configs, |resolver| {
            let name = |a: &Ancestor| match a {
                Ancestor::Class(t) => t.name.to_string(),
                Ancestor::Opaque(name) => format!("opaque {name}"),
            };
            (resolver.families.iter())
                .map(|(t, f)| {
                    (
                        t.name.to_string(),
                        f.as_ref().map(|f| f.iter().map(name).collect()),
                    )
                })
                .collect()
        })
    }

    #[test]
    fn a_static_import_of_a_class_naming_its_own_base_leaves_the_base_opaque() {
        let files = [(
            "App/A.cs",
            "using static N.A;\nnamespace N { public class A : Inner { } }\n",
        )];
        let families = families_in(&files, &[("App/App.csproj", PROJECT)]);
        let a = Some(vec!["N.A".to_string(), "opaque Inner".to_string()]);
        assert_eq!(families["N.A"], a);
    }

    #[test]
    fn a_static_import_cycle_gives_the_same_families_from_either_end() {
        let project = b"<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup>\
                        <Using Include=\"Acme.Query\" /></ItemGroup></Project>";
        let service = "namespace Acme.Query;\npublic interface ILanguageBasedService { }\n";
        let interface = "using static Acme.Query.Internal.Generator;\n\
                         namespace Acme.Query.Internal;\n\
                         public interface IGenerator : ILanguageBasedService { }\n";
        let class = "namespace Acme.Query.Internal;\npublic class Generator : IGenerator { }\n";
        let families = |class_file: &str| {
            let files = [
                ("Acme/Query/ILanguageBasedService.cs", service),
                ("Acme/Query/Internal/IGenerator.cs", interface),
                (class_file, class),
            ];
            families_in(&files, &[("Acme/Acme.csproj", project)])
        };
        let first = families("Acme/Query/Internal/Generator.cs");
        assert_eq!(first, families("Acme/Query/Internal/ZGenerator.cs"));
        let names = |names: &[&str]| Some(names.iter().map(|n| n.to_string()).collect());
        let (service, interface) = (
            "Acme.Query.ILanguageBasedService",
            "Acme.Query.Internal.IGenerator",
        );
        assert_eq!(first[interface], names(&[interface, service]));
        assert_eq!(
            first["Acme.Query.Internal.Generator"],
            names(&["Acme.Query.Internal.Generator", interface, service])
        );
    }

    #[test]
    fn a_glob_lookup_during_linearization_sees_the_families_being_linearized() {
        let src = "using static N.A;\nnamespace N { public class B { } \
                   public class A : B { public static void Go() {} } }\n";
        with_resolver(
            &[("App/A.cs", src)],
            &[("App/App.csproj", PROJECT)],
            |mut resolver| {
                resolver.families.clear();
                let go = |families: Vec<(String, String)>| {
                    *resolver.linearizing.borrow_mut() = families;
                    resolver.globs("App/A.cs", "", "Go", false, &mut Seen::new())
                };
                assert!(matches!(go(vec![]), Some(Binding::Def(t)) if t.name == "N.A.Go()"));
                let a = ("App/A.cs".to_string(), "N.A".to_string());
                assert!(!matches!(go(vec![a]), Some(Binding::Def(_))));
            },
        );
    }

    #[test]
    fn a_static_import_gives_the_declared_members_alone() {
        let bases = "namespace N { public class B { public static void Go() {} \
                     public class Inner { public static void Run() {} } } \
                     public class A : B { public class Own { public static void Run() {} } } }\n";
        let call = |using: &str, callee: &str| {
            let user = format!("{using}\nclass U {{ void M() {{ {callee}(); }} }}\n");
            let found = resolved(&[("App/Bases.cs", bases), ("App/U.cs", user.as_str())]);
            let found = found
                .into_iter()
                .find(|c| c.0 == "App/U.cs" && c.1 == callee);
            found.map(|c| c.2)
        };
        assert_ne!(call("using static N.A;", "Go"), Some("exact"));
        assert_eq!(call("using static N.B;", "Go"), Some("exact"));
        assert_eq!(call("using static N.A;", "Own.Run"), Some("exact"));
        assert_ne!(call("using static N.A;", "Inner.Run"), Some("exact"));
    }

    #[test]
    fn a_static_import_of_a_generic_type_gives_its_declared_overloads() {
        let types = "namespace N { public class B { public static void Go() {} } \
                     public class A<T> : B { public static void Go(int x) {} \
                     public static void Own() {} } }\n";
        let call = |callee: &str, args: &str| {
            let user =
                format!("using static N.A;\nclass U {{ void M() {{ {callee}({args}); }} }}\n");
            let found = resolved(&[("App/Types.cs", types), ("App/U.cs", user.as_str())]);
            let found = found
                .into_iter()
                .find(|c| c.0 == "App/U.cs" && c.1 == callee);
            found.map(|c| (c.2, c.3))
        };
        assert_eq!(call("Own", "").map(|c| c.0), Some("exact"));
        assert_ne!(call("Go", "").map(|c| c.0), Some("exact"));
    }

    #[test]
    fn a_lookup_past_an_import_goes_on_from_the_links_followed() {
        let src =
            "using static N.B;\nnamespace N { public class B { public static void Go() {} } }\n";
        with_resolver(
            &[("App/A.cs", src)],
            &[("App/App.csproj", PROJECT)],
            |resolver| {
                let go = |links: usize| {
                    resolver.links.set(links);
                    resolver.cut.set(false);
                    let seen = &mut resolver.seen("App/A.cs", "");
                    let found = resolver.globs("App/A.cs", "", "Go", false, seen);
                    let found = matches!(found, Some(Binding::Def(t)) if t.name == "N.B.Go()");
                    (found, seen.cut, resolver.cut.get())
                };
                assert_eq!(go(0), (true, false, false));
                assert_eq!(go(LINK_DEPTH), (false, true, true));
            },
        );
    }

    #[test]
    fn a_typed_lambda_parameter_types_its_receiver() {
        let calls = |body: &str| {
            let src = format!(
                "class U {{ Bin shelf; void M() {{ {body} }} void N() => shelf.Stock(); }}"
            );
            let found = resolved(&[
                ("App/C.cs", "class Crate { public int Stock() => 1; }"),
                ("App/B.cs", "class Bin { public int Stock() => 2; }"),
                ("App/U.cs", &src),
            ]);
            let of = |from: usize| {
                let at: Vec<_> = found.iter().filter(|c| c.1 == "shelf.Stock").collect();
                (at[from].2, at[from].3.clone())
            };
            (of(0), of(1))
        };
        for body in [
            "Func<Crate,int> f = (Crate shelf) => shelf.Stock();",
            "Func<Crate,int> f = (Crate shelf) => { return shelf.Stock(); };",
            "Func<int,Crate,int> f = (int n, Crate shelf) => shelf.Stock();",
            "Func<Crate,int> f = ([Attr] Crate shelf) => shelf.Stock();",
            "Func<Crate,int> f = (ref Crate shelf) => shelf.Stock();",
            "Func<Crate,int> f = delegate (Crate shelf) { return shelf.Stock(); };",
            "app.MapPost(\"/\", ([FromBody] Crate shelf) => shelf.Stock());",
        ] {
            let (lambda, field) = calls(body);
            assert_eq!(lambda, ("exact", vec!["App/C.cs".to_string()]), "{body}");
            assert_eq!(field, ("exact", vec!["App/B.cs".to_string()]), "{body}");
        }
        for body in [
            "Func<Crate,int> f = shelf => shelf.Stock();",
            "Func<Crate,int> f = (shelf) => shelf.Stock();",
        ] {
            assert_eq!(calls(body).0.0, "ambiguous", "{body}");
        }
    }

    #[test]
    fn a_known_base_hides_its_names_and_ienumerable_stays_opaque() {
        let src = "using System;\nusing System.Collections.Generic;\nnamespace P;\n\
                   class Lease : IDisposable { public void Open(int n) { } public void Dispose() { } \
                   void Run() { this.Open(1); this.Dispose(); } }\n\
                   class Bag : IEnumerable<int> { public bool Contains(string k) => true; \
                   void Run() { this.Contains(1); } }\n";
        let found = resolved(&[("App/Lease.cs", src)]);
        let level = |callee: &str| (found.iter()).find(|c| c.1 == callee).map(|c| c.2).unwrap();
        assert_eq!(level("this.Open"), "exact");
        assert_eq!(level("this.Dispose"), "inferred");
        assert_eq!(level("this.Contains"), "inferred");
    }

    #[test]
    fn a_family_s_overloads_in_another_project_stay_candidates() {
        let base = "namespace P;\npublic class Base { public void F(int n) { } }\n";
        let app = "namespace Q;\nusing P;\nclass Derived : Base { public void F(string s) { } \
                   void Run(Derived d) { d.F(1); } }\n";
        let found = resolved(&[("Kit/Base.cs", base), ("App/Derived.cs", app)]);
        let call = (found.iter())
            .find(|(_, callee, ..)| callee == "d.F")
            .unwrap();
        let both = vec!["App/Derived.cs".to_string(), "Kit/Base.cs".to_string()];
        assert_eq!((call.2, call.3.clone()), ("ambiguous", both));
    }

    #[test]
    fn a_type_two_projects_declare_binds_to_the_caller_s_project_first() {
        let use_it = "namespace P;\nclass Use { void F(Order o) { o.Total(); Order.Of(); \
                      P.Order.Of(); } }\n";
        let app =
            "namespace Q;\nusing P;\nclass Run { void F(Order o) { o.Total(); Order.Of(); } }\n";
        let found = resolved(&[
            ("Kit/Order.cs", ORDER),
            ("Kit2/Order.cs", ORDER),
            ("Kit2/Use.cs", use_it),
            ("App/Run.cs", app),
        ]);
        let of = |path: &str| {
            (found.iter())
                .filter(|(p, ..)| p == path)
                .map(|(_, callee, level, targets)| (callee.as_str(), *level, targets.clone()))
                .collect::<Vec<_>>()
        };
        let kit2 = vec!["Kit2/Order.cs".to_string()];
        assert_eq!(
            of("Kit2/Use.cs"),
            [
                ("o.Total", "exact", kit2.clone()),
                ("Order.Of", "exact", kit2.clone()),
                ("P.Order.Of", "exact", kit2),
            ]
        );
        let both = vec!["Kit/Order.cs".to_string(), "Kit2/Order.cs".to_string()];
        assert_eq!(
            of("App/Run.cs"),
            [
                ("o.Total", "unresolved", vec![]),
                ("Order.Of", "ambiguous", both)
            ]
        );
    }

    fn project(refs: &[(&str, &str)]) -> Vec<u8> {
        let items: String = (refs.iter())
            .map(|(to, attrs)| format!("<ProjectReference Include=\"{to}\" {attrs} />"))
            .collect();
        format!("<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup>{items}</ItemGroup></Project>")
            .into_bytes()
    }

    fn order_called(projects: &[(&str, Vec<u8>)]) -> (&'static str, Vec<String>) {
        let mut configs: Vec<(&str, &[u8])> = vec![
            ("Kit/Kit.csproj", PROJECT),
            ("Kit2/Kit2.csproj", PROJECT),
            ("Mid/Mid.csproj", PROJECT),
            ("App/App.csproj", PROJECT),
        ];
        for (path, src) in projects {
            configs.retain(|(p, _)| p != path);
            configs.push((path, src));
        }
        let app = "namespace Q;\nusing P;\nclass Run { void F() { Order.Of(); } }\n";
        let found = resolved_in(
            &[
                ("Kit/Order.cs", ORDER),
                ("Kit2/Order.cs", ORDER),
                ("App/Run.cs", app),
            ],
            &configs,
        );
        let (_, _, level, targets) = (found.into_iter())
            .find(|(p, callee, ..)| p == "App/Run.cs" && callee == "Order.Of")
            .unwrap();
        (level, targets)
    }

    #[test]
    fn a_type_two_projects_declare_binds_to_the_one_project_the_caller_references() {
        let kit = vec!["Kit/Order.cs".to_string()];
        let both = vec!["Kit/Order.cs".to_string(), "Kit2/Order.cs".to_string()];
        let to = |reference: &str, attrs: &str| project(&[(reference, attrs)]);
        let app_kit = || ("App/App.csproj", to("..\\Kit\\Kit.csproj", ""));
        assert_eq!(order_called(&[app_kit()]), ("exact", kit.clone()));
        let through = |attrs: &str| {
            [
                ("App/App.csproj", to("..\\Mid\\Mid.csproj", "")),
                ("Mid/Mid.csproj", to("..\\Kit\\Kit.csproj", attrs)),
            ]
        };
        assert_eq!(order_called(&through("")), ("exact", kit.clone()));
        assert_eq!(
            order_called(&through("PrivateAssets=\"all\"")),
            ("ambiguous", both.clone())
        );
        let twice = project(&[("..\\Kit\\Kit.csproj", ""), ("..\\Kit2\\Kit2.csproj", "")]);
        assert_eq!(
            order_called(&[("App/App.csproj", twice)]),
            ("ambiguous", both.clone())
        );
        assert_eq!(order_called(&[]), ("ambiguous", both.clone()));
        let outside = to("..\\Other\\Other.csproj", "");
        assert_eq!(
            order_called(&[("App/App.csproj", outside)]),
            ("ambiguous", both)
        );
        let configs: [(&str, &[u8]); 2] = [
            ("Kit/Kit.csproj", PROJECT),
            ("App/App.csproj", &to("..\\Kit2\\Kit2.csproj", "")),
        ];
        let app = "namespace Q;\nusing P;\nclass Run { void F() { Order.Of(); } }\n";
        let found = resolved_in(&[("Kit/Order.cs", ORDER), ("App/Run.cs", app)], &configs);
        assert_eq!(found[0].2, "exact");
        let cycle = [
            ("App/App.csproj", to("..\\Mid\\Mid.csproj", "")),
            (
                "Mid/Mid.csproj",
                project(&[("..\\App\\App.csproj", ""), ("..\\Kit\\Kit.csproj", "")]),
            ),
        ];
        assert_eq!(order_called(&cycle), ("exact", kit));
    }

    fn props(body: &str) -> Vec<u8> {
        format!("<Project><ItemGroup>{body}</ItemGroup></Project>").into_bytes()
    }

    fn order_through(props: &[(&str, Vec<u8>)]) -> &'static str {
        let mut configs: Vec<(&str, &[u8])> =
            vec![("Kit/Kit.csproj", PROJECT), ("App/App.csproj", PROJECT)];
        configs.extend(props.iter().map(|(path, src)| (*path, src.as_slice())));
        let app = "namespace Q;\nclass Run { void F() { Order.Of(); } }\n";
        let found = resolved_in(&[("Kit/Order.cs", ORDER), ("App/Run.cs", app)], &configs);
        (found.into_iter())
            .find(|(p, callee, ..)| p == "App/Run.cs" && callee == "Order.Of")
            .unwrap()
            .2
    }

    #[test]
    fn a_directory_build_props_adds_its_usings_to_the_projects_below_it() {
        let root = ("Directory.Build.props", props("<Using Include=\"P\" />"));
        assert_eq!(order_through(&[]), "unresolved");
        assert_eq!(order_through(std::slice::from_ref(&root)), "exact");
        let inner = |body: &str| ("App/Directory.Build.props", props(body));
        assert_eq!(order_through(&[root.clone(), inner("")]), "unresolved");
        let import = "</ItemGroup><Import Project=\"$([MSBuild]::GetPathOfFileAbove(\
            'Directory.Build.props', '$(MSBuildThisFileDirectory)../'))\" /><ItemGroup>";
        assert_eq!(order_through(&[root.clone(), inner(import)]), "exact");
        let own = inner(&import.replace("../", ""));
        assert_eq!(order_through(&[root.clone(), own]), "unresolved");
        let big = format!(
            "<Project><!--{}--></Project>",
            "x".repeat(crate::lang::MAX_BYTES)
        );
        let big = ("Directory.Build.props", big.into_bytes());
        assert_eq!(order_through(&[big]), "unresolved");
        let removing = "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup><Using Remove=\"P\" /></ItemGroup></Project>";
        let configs: [(&str, &[u8]); 3] = [
            ("Kit/Kit.csproj", PROJECT),
            ("App/App.csproj", removing.as_bytes()),
            ("Directory.Build.props", &root.1),
        ];
        let app = "namespace Q;\nclass Run { void F() { Order.Of(); } }\n";
        let found = resolved_in(&[("Kit/Order.cs", ORDER), ("App/Run.cs", app)], &configs);
        assert_eq!(found[0].2, "unresolved");
    }

    #[test]
    fn a_project_reference_of_a_directory_build_props_binds_like_one_of_the_project() {
        let both = vec!["Kit/Order.cs".to_string(), "Kit2/Order.cs".to_string()];
        let kit = vec!["Kit/Order.cs".to_string()];
        let props = |to: &str| {
            let item = format!("<ProjectReference Include=\"{to}\" />");
            ("Directory.Build.props", props(&item))
        };
        assert_eq!(
            order_called(&[props("..\\Kit\\Kit.csproj")]),
            ("exact", kit.clone())
        );
        assert_eq!(
            order_called(&[props("Kit\\Kit.csproj")]),
            ("ambiguous", both.clone())
        );
        let to_mid = project(&[("..\\Mid\\Mid.csproj", "")]);
        let mid = ("Mid/Mid.csproj", project(&[("..\\Kit\\Kit.csproj", "")]));
        let own = ("App/App.csproj", to_mid);
        assert_eq!(
            order_called(&[mid, own, props("..\\Kit2\\Kit2.csproj")]),
            ("ambiguous", both)
        );
    }

    fn save_called(members: &str, use_it: &str) -> (&'static str, Vec<String>) {
        let repo = format!(
            "public class Repo {{ {members} public void Save() {{}} \
             public static Repo Make() => new(); }}"
        );
        let store = "public class Store { public void Save() {} public Store Next; }";
        let app = format!("class U {{ void M() {{ {use_it} }} }}");
        let found = resolved(&[("Kit/R.cs", &repo), ("Kit/S.cs", store), ("App/U.cs", &app)]);
        let (_, _, level, targets) = (found.into_iter())
            .find(|(p, callee, ..)| p == "App/U.cs" && callee.ends_with(".Save"))
            .unwrap();
        (level, targets)
    }

    #[test]
    fn a_member_step_after_a_call_is_the_receiver_not_the_call_s_class() {
        let both = ["Kit/R.cs", "Kit/S.cs"].map(String::from).to_vec();
        let store = vec!["Kit/S.cs".to_string()];
        for (members, use_it, expected) in [
            (
                "public dynamic x;",
                "Repo.Make().x.Save();",
                ("ambiguous", &both),
            ),
            (
                "public dynamic X { get; }",
                "Repo.Make().X.Save();",
                ("ambiguous", &both),
            ),
            (
                "public dynamic a;",
                "Repo.Make().a.Next.Save();",
                ("ambiguous", &both),
            ),
            (
                "public dynamic x;",
                "Repo.Make()?.x.Save();",
                ("ambiguous", &both),
            ),
            (
                "public dynamic x;",
                "Repo.Make().x?.Save();",
                ("ambiguous", &both),
            ),
            (
                "public Store x;",
                "Repo.Make().x.Save();",
                ("exact", &store),
            ),
            (
                "public Store X { get; }",
                "Repo.Make().X.Save();",
                ("exact", &store),
            ),
            (
                "public Store a;",
                "Repo.Make().a.Next.Save();",
                ("exact", &store),
            ),
            (
                "public Store x;",
                "Repo.Make()?.x.Save();",
                ("exact", &store),
            ),
            (
                "public Store x;",
                "Repo.Make().x?.Save();",
                ("exact", &store),
            ),
        ] {
            let (level, targets) = save_called(members, use_it);
            assert_eq!((level, &targets), expected, "{members} {use_it}");
        }
    }

    #[test]
    fn a_call_made_on_a_call_result_directly_narrows_to_its_class() {
        let repo = vec!["Kit/R.cs".to_string()];
        for use_it in ["Repo.Make().Save();", "Repo.Make()?.Save();"] {
            let (level, targets) = save_called("public dynamic x;", use_it);
            assert_eq!((level, &targets), ("exact", &repo), "{use_it}");
        }
    }

    fn calls_in(
        found: &[(String, String, &'static str, Vec<String>)],
        path: &str,
    ) -> Vec<(String, &'static str, Vec<String>)> {
        (found.iter())
            .filter(|(p, ..)| p == path)
            .map(|(_, callee, level, targets)| (callee.clone(), *level, targets.clone()))
            .collect()
    }

    const USE_TAX: &str = "using Shop; class U { void M() { new Tax().Apply(); Tax.Of(); } }";

    fn tax(access: &str) -> String {
        format!(
            "namespace Shop; {access} class Tax {{ public void Apply() {{}} \
             public static void Of() {{}} }}"
        )
    }

    #[test]
    fn an_internal_type_links_from_its_own_project_alone() {
        for access in ["internal", ""] {
            let found = resolved(&[("Kit/Tax.cs", &tax(access)), ("App/Use.cs", USE_TAX)]);
            assert_eq!(
                calls_in(&found, "App/Use.cs"),
                [
                    ("(expression).Apply".to_string(), "unresolved", vec![]),
                    ("Tax".to_string(), "unresolved", vec![]),
                    ("Tax.Of".to_string(), "unresolved", vec![]),
                ],
                "{access:?}"
            );
        }
    }

    const FOO: &str = "namespace Shop; public class Foo { internal Foo(int n) {} public Foo() {} \
                       internal static void Make() {} private static void Own() {} \
                       protected void Sub() {} }";

    #[test]
    fn a_non_public_member_of_a_public_type_links_from_its_own_project_alone() {
        let use_foo = "using Shop; class U { void M() { new Foo(1); Foo.Make(); Foo.Own(); \
                       new Foo().Sub(); } }";
        let found = resolved(&[("Kit/Foo.cs", FOO), ("App/Use.cs", use_foo)]);
        let outside = calls_in(&found, "App/Use.cs");
        for callee in ["Foo.Make", "Foo.Own"] {
            let call = outside.iter().find(|(c, ..)| c == callee).unwrap();
            assert_eq!(call.1, "unresolved", "{outside:?}");
        }
        let ctor = outside.iter().find(|(c, ..)| c == "Foo").unwrap();
        assert!(
            ctor.2.iter().all(|t| !t.contains("Foo.cs")) || ctor.1 != "exact",
            "{outside:?}"
        );
        let sub = outside.iter().find(|(c, ..)| c.ends_with("Sub")).unwrap();
        assert_ne!(sub.1, "unresolved", "{outside:?}");
        let found = resolved(&[("Kit/Foo.cs", FOO), ("Kit/Use.cs", use_foo)]);
        let own = calls_in(&found, "Kit/Use.cs");
        for callee in ["Foo.Make", "Foo.Own", "Foo"] {
            let call = own.iter().find(|(c, ..)| c == callee).unwrap();
            assert_eq!(call.1, "exact", "{own:?}");
        }
    }

    #[test]
    fn a_name_an_internal_type_hides_binds_to_the_type_the_file_sees() {
        let kit2 = vec!["Kit2/Tax.cs".to_string()];
        let found = resolved(&[
            ("Kit/Tax.cs", &tax("internal")),
            ("Kit2/Tax.cs", &tax("public")),
            ("App/Use.cs", USE_TAX),
        ]);
        assert_eq!(
            calls_in(&found, "App/Use.cs"),
            [
                ("(expression).Apply".to_string(), "inferred", kit2.clone()),
                ("Tax".to_string(), "exact", kit2.clone()),
                ("Tax.Of".to_string(), "exact", kit2),
            ]
        );
        let found = resolved(&[
            ("Kit/Tax.cs", &tax("internal")),
            ("Kit2/Tax.cs", &tax("internal")),
            ("Kit/Use.cs", USE_TAX),
            ("Kit2/Use.cs", USE_TAX),
        ]);
        for project in ["Kit", "Kit2"] {
            let own = vec![format!("{project}/Tax.cs")];
            assert_eq!(
                calls_in(&found, &format!("{project}/Use.cs")),
                [
                    ("(expression).Apply".to_string(), "inferred", own.clone()),
                    ("Tax".to_string(), "exact", own.clone()),
                    ("Tax.Of".to_string(), "exact", own),
                ]
            );
        }
    }

    #[test]
    fn a_hidden_type_of_the_file_s_namespace_lets_a_using_bind_the_name() {
        let billing = vec!["Kit2/Tax.cs".to_string()];
        let public = tax("public").replace("namespace Shop;", "namespace Billing;");
        let found = resolved(&[
            ("Kit/Tax.cs", &tax("internal")),
            ("Kit2/Tax.cs", &public),
            (
                "App/Use.cs",
                &format!("namespace Shop; {}", USE_TAX.replace("Shop", "Billing")),
            ),
        ]);
        assert_eq!(
            calls_in(&found, "App/Use.cs"),
            [
                (
                    "(expression).Apply".to_string(),
                    "inferred",
                    billing.clone()
                ),
                ("Tax".to_string(), "exact", billing.clone()),
                ("Tax.Of".to_string(), "exact", billing),
            ]
        );
    }

    #[test]
    fn a_file_of_many_internal_types_hides_them_in_linear_time() {
        let n = 30_000;
        let internal: String = (0..n)
            .map(|i| format!("internal class I{i} {{}}\n"))
            .collect();
        let kit = format!(
            "namespace Shop;\n{internal}public class P {{ public static void M() {{}} }}\n"
        );
        let calls: String = (0..n).map(|_| "P.M();\n").collect();
        let app = format!("using Shop;\nclass U {{ void F() {{\n{calls}}} }}\n");
        let start = std::time::Instant::now();
        let found = resolved(&[("Kit/T.cs", &kit), ("App/U.cs", &app)]);
        let took = start.elapsed();
        let exact = (found.iter()).filter(|(p, callee, level, _)| {
            p == "App/U.cs" && callee == "P.M" && *level == "exact"
        });
        assert_eq!(exact.count(), n);
        assert!(took.as_secs() < 130, "{took:?}");
    }

    fn enclosing(files: &[(&str, &str)], usings: &str, block: bool) -> (Vec<String>, String) {
        let body = "public class Gen { void M() { Util.Go(); new Util(); } }\n\
                    public class Sub : Util { }\n";
        let source = match block {
            true => format!(
                "{usings}namespace Acme {{ namespace Query {{ namespace Internal {{\n{body}}} }} }}\n"
            ),
            false => format!("{usings}namespace Acme.Query.Internal;\n{body}"),
        };
        let mut all = files.to_vec();
        all.push(("App/Gen.cs", &source));
        let found = resolved(&all);
        let calls = (calls_in(&found, "App/Gen.cs").into_iter())
            .map(|(callee, level, targets)| format!("{callee} {level} {}", targets.join(" ")))
            .collect();
        let configs = [("App/App.csproj", PROJECT)];
        let family = families_in(&all, &configs)["Acme.Query.Internal.Sub"].clone();
        (calls, family.unwrap_or_default().join(" "))
    }

    fn util(ns: &str) -> String {
        format!("namespace {ns};\npublic class Util {{ public static void Go() {{ }} }}\n")
    }

    #[test]
    fn a_type_of_an_enclosing_namespace_binds_without_a_using() {
        let query = util("Acme.Query");
        for block in [false, true] {
            let found = enclosing(&[("App/Util.cs", &query)], "", block);
            let with_using = enclosing(&[("App/Util.cs", &query)], "using Acme.Query;\n", block);
            assert_eq!(
                found,
                (
                    vec![
                        "Util.Go exact App/Util.cs".to_string(),
                        "Util exact App/Util.cs".to_string(),
                    ],
                    "Acme.Query.Internal.Sub Acme.Query.Util".to_string(),
                ),
                "block: {block}"
            );
            assert_eq!(found, with_using, "block: {block}");
        }
    }

    #[test]
    fn the_innermost_enclosing_namespace_binds_before_outer_ones_and_usings() {
        let (inner, query, acme, other) = (
            util("Acme.Query.Internal"),
            util("Acme.Query"),
            util("Acme"),
            util("Other"),
        );
        let cases: [(&[(&str, &str)], &str); 3] = [
            (&[("App/A.cs", &acme), ("App/Q.cs", &query)], "App/Q.cs"),
            (
                &[
                    ("App/A.cs", &acme),
                    ("App/I.cs", &inner),
                    ("App/Q.cs", &query),
                ],
                "App/I.cs",
            ),
            (&[("App/A.cs", &acme), ("App/O.cs", &other)], "App/A.cs"),
        ];
        for block in [false, true] {
            for (files, wanted) in cases {
                let (calls, _) = enclosing(files, "using Other;\n", block);
                let names: Vec<&str> = files.iter().map(|f| f.0).collect();
                assert_eq!(
                    calls,
                    [
                        format!("Util.Go exact {wanted}"),
                        format!("Util exact {wanted}")
                    ],
                    "{names:?}, block: {block}"
                );
            }
        }
    }

    #[test]
    fn a_sibling_namespace_of_the_file_does_not_enclose_the_code() {
        let (acme, other) = (util("Acme"), util("Other"));
        let source = "namespace Acme.Query { public class Gen { void M() { Util.Go(); } } }\n\
                    namespace Other.Query { public class Gen { void M() { Util.Go(); } } }\n";
        let found = resolved(&[
            ("App/A.cs", &acme),
            ("App/O.cs", &other),
            ("App/Gen.cs", source),
        ]);
        let (a, o) = (vec!["App/A.cs".to_string()], vec!["App/O.cs".to_string()]);
        assert_eq!(
            calls_in(&found, "App/Gen.cs"),
            [
                ("Util.Go".to_string(), "exact", a),
                ("Util.Go".to_string(), "exact", o),
            ]
        );
    }
}

#[cfg(test)]
mod kotlin_tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::java::Java;
    use crate::lang::kotlin::Kotlin;

    fn resolved(files: &[(&str, &str)]) -> BTreeMap<String, String> {
        let parsed: Vec<(&str, Facts)> = (files.iter())
            .map(|(p, s)| (*p, Kotlin.parse(p, s.as_bytes()).unwrap()))
            .collect();
        let files: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let at = |t: &Target| format!("{}::{}", t.path, t.name);
        let mut out = BTreeMap::new();
        for (&path, facts) in &files {
            for call in &facts.calls {
                let found = match resolver.call(path, call) {
                    Resolution::Exact(t) => format!("exact {}", at(&t)),
                    Resolution::Inferred(t) | Resolution::Narrowed(t) => {
                        format!("inferred {}", at(&t))
                    }
                    Resolution::Ambiguous(ts) => {
                        format!(
                            "ambiguous {}",
                            ts.iter().map(at).collect::<Vec<_>>().join(" ")
                        )
                    }
                    Resolution::Unresolved => "none".to_string(),
                };
                out.insert(format!("{path} {} {}", call.scope, call.callee), found);
            }
        }
        out
    }

    #[test]
    fn a_private_constructor_is_called_from_inside_its_class_alone() {
        let found = resolved(&[(
            "p/A.kt",
            "package p
class P private constructor(val n: Int) {
    fun again() = P(1)
    companion object { fun make() = P(2) }
}
class S {
    private constructor(n: Int) {}
    constructor() : this(1)
    fun again() = S(1)
}
class I internal constructor(val n: Int)
class Q private constructor(val n: Int) {
    constructor() : this(1)
    class N { fun go() = Q(1) }
}
fun top() = P(1)
class Out {
    fun go() { P(1); S(1); I(1); Q() }
    fun one() = Q(1)
}
",
        )]);
        let get = |key: &str| {
            found
                .get(key)
                .unwrap_or_else(|| panic!("{key} in {found:?}"))
        };
        assert_eq!(get("p/A.kt p.Out.go() P"), "none");
        assert_eq!(get("p/A.kt p.Out.go() S"), "none");
        assert_eq!(get("p/A.kt p.P.again() P"), "exact p/A.kt::p.P");
        assert_eq!(get("p/A.kt p.P.Companion.make() P"), "exact p/A.kt::p.P");
        assert_eq!(get("p/A.kt p.S.again() S"), "exact p/A.kt::p.S.S(Int)");
        assert_eq!(get("p/A.kt p.Out.go() Q"), "exact p/A.kt::p.Q.Q()");
        assert_eq!(get("p/A.kt p.Q.N.go() Q"), "exact p/A.kt::p.Q");
        assert_eq!(get("p/A.kt p.Out.one() Q"), "none");
        assert_eq!(get("p/A.kt p.top() P"), "none");
        assert!(get("p/A.kt p.Out.go() I").starts_with("exact"), "{found:?}");
    }

    #[test]
    fn a_chain_of_receivers_across_star_imports_is_bounded_by_the_receiver_depth() {
        let n = RECEIVER_DEPTH + 8;
        let mut sources = vec![(
            "base/K.kt".to_string(),
            "package base\nopen class K { inner class C : K() { fun m() {} } }\n".to_string(),
        )];
        for i in 0..n {
            let rest = match i + 1 == n {
                true => format!("val v{i} = K()\n"),
                false => format!("import p{}.*\nval v{i} = v{}.C()\n", i + 1, i + 1),
            };
            sources.push((
                format!("p{i}/M.kt"),
                format!("package p{i}\nimport base.*\n{rest}"),
            ));
        }
        let parsed: Vec<(&str, Facts)> = (sources.iter())
            .map(|(p, s)| (p.as_str(), Kotlin.parse(p, s.as_bytes()).unwrap()))
            .collect();
        let files: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let start = std::time::Instant::now();
        let exact: Vec<bool> = (parsed.iter())
            .flat_map(|(path, facts)| facts.calls.iter().map(move |call| (*path, call)))
            .map(|(path, call)| matches!(resolver.call(path, call), Resolution::Exact(_)))
            .collect();
        let took = start.elapsed();
        assert!(took.as_secs() < 10, "{took:?}");
        for (i, known) in exact.iter().enumerate() {
            assert_eq!(*known, i + RECEIVER_DEPTH + 1 >= n, "call {i} of {n}");
        }
    }

    #[test]
    fn an_inner_class_reads_the_outer_field_and_a_companion_one_of_its_name_hides_it() {
        let found = resolved(&[(
            "p/A.kt",
            "package p
class A { fun go() {} }
class B { fun go() {} }
class Outer {
    val repo: A = A()
    fun mine() { repo.go() }
    inner class Inner { fun run() { repo.go() } }
    class Nested {
        val repo: B = B()
        fun run() { repo.go() }
    }
    companion object {
        val repo: B = B()
        fun run() { repo.go() }
    }
}
",
        )]);
        let go = |scope: &str| found[&format!("p/A.kt p.Outer{scope} repo.go")].as_str();
        assert_eq!(go(".mine()"), "exact p/A.kt::p.A.go()");
        assert_eq!(go(".Inner.run()"), "exact p/A.kt::p.A.go()");
        assert_eq!(go(".Nested.run()"), "exact p/A.kt::p.B.go()");
        assert_eq!(go(".Companion.run()"), "exact p/A.kt::p.B.go()");
    }

    #[test]
    fn the_own_package_comes_before_star_imports_and_two_stars_are_ambiguous() {
        let base = |package: &str| format!("package {package}\nopen class Base\n");
        let (t, v) = (base("t"), base("v"));
        let own = "package u\nimport t.*\nclass Base\nfun go() { Base() }\n";
        let two = "package w\nimport t.*\nimport v.*\nfun go() { Base() }\n";
        let found = resolved(&[
            ("t/Base.kt", &t),
            ("v/Base.kt", &v),
            ("u/U.kt", own),
            ("w/W.kt", two),
        ]);
        assert_eq!(found["u/U.kt u.go() Base"], "exact u/U.kt::u.Base");
        assert_eq!(
            found["w/W.kt w.go() Base"],
            "ambiguous t/Base.kt::t.Base v/Base.kt::v.Base"
        );
    }

    #[test]
    fn an_expect_class_and_its_actuals_are_the_expect_one() {
        let class = |modifier: &str| {
            format!(
                "package e\n{modifier} open class C() {{ fun m(n: Int) {{}} fun own() {{}} }}\n"
            )
        };
        let (expect, actual, plain) = (class("expect"), class("actual"), class(""));
        let user = "package u\nimport e.C\nfun go(c: C) { C(); c.m(1); c.own() }\n";
        let level = |files: &[(&str, &str)], callee: &str| {
            resolved(files)[&format!("u/U.kt u.go(C) {callee}")].clone()
        };
        let one = [("a/C.kt", &*expect), ("b/C.kt", &*actual), ("u/U.kt", user)];
        assert_eq!(level(&one, "C"), "exact a/C.kt::e.C");
        assert_eq!(level(&one, "c.m"), "exact a/C.kt::e.C.m(Int)");
        assert_eq!(level(&one, "c.own"), "exact a/C.kt::e.C.own()");
        let actual_only = class("actual").replace("fun own() {}", "fun more() {}");
        let user_more = user.replace("c.own()", "c.more()");
        let more = [
            ("a/C.kt", &*expect),
            ("b/C.kt", &*actual_only),
            ("u/U.kt", &*user_more),
        ];
        assert_eq!(level(&more, "c.more"), "inferred b/C.kt::e.C.more()");
        let overloaded = actual.replace("fun own() {}", "fun own() {} fun m(s: String) {}");
        let added = [
            ("a/C.kt", &*expect),
            ("b/C.kt", &*overloaded),
            ("u/U.kt", user),
        ];
        assert_ne!(level(&added, "c.m"), "exact a/C.kt::e.C.m(Int)");
        assert!(!level(&added, "c.m").starts_with("none"));
        assert_eq!(level(&added, "c.own"), "exact a/C.kt::e.C.own()");
        for other in [&expect, &plain] {
            let two = [("a/C.kt", &*expect), ("b/C.kt", &**other), ("u/U.kt", user)];
            assert_eq!(level(&two, "C"), "ambiguous a/C.kt::e.C b/C.kt::e.C");
        }
    }

    #[test]
    fn a_class_nested_in_an_expect_class_is_the_expect_one() {
        let class = |m: &str| {
            format!("package e\n{m} class Outer {{ {m} class Inner {{ fun go() {{}} }} }}\n")
        };
        let (expect, actual) = (
            class("expect").replace("expect class Inner", "class Inner"),
            class("actual"),
        );
        let user = "package u\nimport e.Outer.Inner\nfun run() { Inner().go() }\n";
        let files = [
            ("a/O.kt", &*expect),
            ("b/O.kt", &*actual),
            ("c/O.kt", &*actual),
            ("u/U.kt", user),
        ];
        let found = resolved(&files);
        let hit = found.iter().find(|(k, _)| k.ends_with("Inner")).unwrap();
        assert_eq!(hit.1, "exact a/O.kt::e.Outer.Inner", "{found:?}");
    }

    #[test]
    fn an_extension_on_an_objects_name_is_reached_as_on_a_typed_local() {
        let object = "package p\nobject P\n";
        let extension = "package q\nimport p.P\nfun P.setMain(x: Int) {}\n";
        let user = "package u\nimport p.P\nimport q.setMain\nfun go() { P.setMain(1) }\n";
        let found = resolved(&[("p/P.kt", object), ("q/E.kt", extension), ("u/U.kt", user)]);
        assert_eq!(
            found["u/U.kt u.go() P.setMain"],
            "exact q/E.kt::q.P.setMain(Int)"
        );
        for class in [
            "package p\nclass P\n",
            "package p\nclass P { val INSTANCE: P? = null }\n",
        ] {
            let found = resolved(&[("p/P.kt", class), ("q/E.kt", extension), ("u/U.kt", user)]);
            assert_eq!(found["u/U.kt u.go() P.setMain"], "none");
        }
    }

    #[test]
    fn a_bare_call_takes_the_extension_of_the_implicit_receiver_before_a_top_level_function() {
        let scope = "package k\ninterface CoroutineScope\n";
        let top = "package k\nfun launch(block: () -> Unit) {}\n";
        let ext = "package k\nfun CoroutineScope.launch(block: () -> Unit) {}\n";
        let user = "package u\nimport k.*\nfun CoroutineScope.x() { launch { } }\n";
        let level = |files: &[(&str, &str)]| {
            let found = resolved(files);
            (found.iter())
                .find(|(k, _)| k.starts_with("u/U.kt") && k.ends_with(" launch"))
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        let (s, t, e, u) = (
            ("k/S.kt", scope),
            ("k/T.kt", top),
            ("k/E.kt", ext),
            ("u/U.kt", user),
        );
        assert_eq!(
            level(&[s, t, e, u]),
            "exact k/E.kt::k.CoroutineScope.launch(()->Unit)"
        );
        assert_eq!(level(&[s, t, u]), "exact k/T.kt::k.launch(()->Unit)");
        let outside = "package k\nfun String.launch(block: () -> Unit) {}\n";
        assert_eq!(
            level(&[s, t, ("k/O.kt", outside), u]),
            "ambiguous k/O.kt::k.String.launch(()->Unit) k/T.kt::k.launch(()->Unit)"
        );
    }
}
