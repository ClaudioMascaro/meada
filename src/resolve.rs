//! The shared resolution (ADR 0007): connects each call a language pack found to the
//! definitions it reaches, with Meada's own scope and import resolution and no types.
//!
//! Two questions read a name or a value. `Code::origins` asks what made a value (the call of a
//! library's constructor or factory, or what the source declares a name is), with the evidence
//! policy the reader needs: `Values` follows the flow of values (constants, imports, arguments,
//! returns), `Constants` only `const` bindings, `Declarations` what the source declares.
//! `Code::outside` asks what an import from outside binds a name to. The walks behind both are
//! private; `Code::expand` reads the wrappers a value passes through.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::lang::{
    Annotated, Binds, Bound, Call, Class, Const, Def, Export, Facts, Import, Imported, Invocation,
    Kind, LanguagePack, Marked, Modules, Param, Part, Reaches, Star, Step, Taker, Ty, Value,
};

/// A definition: the repository-relative path of its file and its qualified name.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Target<'a> {
    pub path: &'a str,
    pub name: &'a str,
}

pub enum Resolution<'a> {
    /// Scope and import resolution found one definition.
    Exact(Target<'a>),
    /// Only a unique name match in the repository found it.
    Inferred(Target<'a>),
    /// A name match narrowed to this one definition from several of its name: by the family of
    /// the class that owns the call, or by the scope a bare call is written in.
    Narrowed(Target<'a>),
    /// Several definitions fit, sorted.
    Ambiguous(Vec<Target<'a>>),
    Unresolved,
}

/// The scope a qualified name is written in: `outer` for `outer.inner`, the empty top-level
/// scope for a top-level name.
pub fn scope_of(name: &str) -> &str {
    crate::parent(name)
}

/// What a crate outside the repository is imported as.
static NAMESPACE: Imported = Imported::Namespace;

/// An `exports` row: the name, the file and the definition in it.
type Row<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>);

/// Where a file's star entries (`Export::All`, `Imported::All`) and those of each name sit in
/// its `Facts::exports` or its `Facts::imports`, each list in order: a name is looked up
/// without a scan of them all.
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

    /// The imports by the `local` they bind (a pack's scoped ones under the qualified name:
    /// `inner.c`, `inner.*`), and those of every name (`Imported::All`) among the stars.
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

    /// Where the entries of `name` sit.
    fn of(&self, name: &str) -> &[usize] {
        self.names.get(name).map_or(&[], Vec::as_slice)
    }
}

/// The index among `calls` of the call spanning `range`. Packs list calls in the order a
/// depth-first walk meets them, an enclosing call before the ones inside it, so the search is
/// a bisection; a list in another order is scanned.
pub fn call_at(calls: &[Call], range: &std::ops::Range<usize>) -> Option<usize> {
    let key = |r: &std::ops::Range<usize>| (r.start, std::cmp::Reverse(r.end));
    let at = calls.partition_point(|c| key(&c.range) < key(range));
    match calls.get(at) {
        Some(c) if c.range == *range => Some(at),
        _ => calls.iter().position(|c| c.range == *range),
    }
}

/// What a name or a member chain refers to.
#[derive(Clone)]
enum Binding<'a> {
    Def(Target<'a>),
    /// What a receiver whose class the source writes holds: an instance of this class, or
    /// the class itself (`cls`). Its members are looked up through the class's family.
    Instance(Target<'a>),
    /// A whole file, through a namespace import or re-export.
    Module(&'a str),
    /// A `const` that holds no function: its file and qualified name.
    Const(&'a str, &'a str),
    /// The parameter at this position of a function, and the field of its object pattern
    /// when the name is one of those it destructures.
    Param(Target<'a>, usize, Option<usize>),
    /// A name bound in a scope whose class the syntax may tell (`Facts::receivers`): its file,
    /// scope and name.
    Local(&'a str, &'a str, &'a str),
    /// Something an import brings from outside the repository: the specifier imported from
    /// and what it names there.
    External(&'a str, &'a Imported),
    /// A `let`, a `catch` binding, a destructured name: it hides every name it spells, and what
    /// it holds is not known.
    Hidden,
    /// Several definitions fit a member equally (two trait blocks of one class give it),
    /// sorted.
    Several(Vec<Target<'a>>),
    /// Neither scope nor import resolution knows it.
    Unknown,
}

/// A call site whose single target is a definition, `exact`.
#[derive(Clone, Copy)]
struct Caller<'a> {
    path: &'a str,
    index: usize,
}

/// What resolving one name follows through exports, aliases and globs: what each `(file,
/// exported name)` pair seen exports, read as a value or not (`Resolver::export_as`), as
/// re-exports may form cycles (a pair met again while it is resolved exports nothing) and
/// diamonds (one met again once resolved exports what it did); the aliases (`use a as b`: file
/// and path) and the glob imports (file, place in its imports and name) met, as a `use` may
/// reach itself; how many links deep it is now, and whether a link was cut at `LINK_DEPTH`. A
/// cut link is unknown, not absent (`Resolver::exports`, `Resolver::globs`): a module or a glob
/// that may give the name leaves it unknown rather than let another import, module or glob
/// bind it. What a cycle gives is absent instead, as in ES modules (and a Python module met
/// again while it is initialized has only what its bindings before gave): `looped` says what is
/// being resolved met a pair being resolved, since the innermost star import began
/// (`Resolver::star`), and each pair keeps whether it did.
#[derive(Default)]
struct Seen<'a> {
    /// `None` while the pair is resolved.
    pairs: HashMap<(&'a str, &'a str, bool), Option<Given<'a>>>,
    aliases: HashSet<(&'a str, &'a str)>,
    globs: HashSet<(&'a str, usize, &'a str)>,
    depth: usize,
    cut: bool,
    looped: bool,
    /// The file whose code being run while it loads (at its top level, in a class body there or
    /// in its main guard) the name is resolved from (`Resolver::seen`).
    load: Option<&'a str>,
}

/// What a pair exports, and whether it met a cycle (`Seen::looped`).
type Given<'a> = (Option<Binding<'a>>, bool);

impl Seen<'_> {
    fn new() -> Self {
        Self::default()
    }
}

/// The most links a name is followed through, each into the next: a file re-exporting it from
/// another (`export { x } from`, `export * from`, a Python `from m import x` or `import *`,
/// Rust's `pub use`), an alias (`use a as b`) or a glob (`use m::*`). Deeper, it is unknown.
// ponytail: real chains are a few links long; the limit keeps a crafted chain of thousands from
// overflowing the stack.
const LINK_DEPTH: usize = 64;

/// What a name is read as.
#[derive(Clone, Copy, PartialEq)]
enum Reading {
    /// Whatever the name is bound to: how exports and flows read it.
    Any,
    /// A called name: a module (Rust's `mod x;`) is no value, and a `use` of its name beyond
    /// it is read.
    Value,
    /// A path called (`a::b::f()`): its head among types and modules, no local or parameter
    /// hiding it, and its last member a value.
    Path,
    /// A constructor's type name: no method, parameter or local of the same name hides it.
    Type,
    /// A `use` path: its head among types and modules, its last member of any kind.
    Alias,
    /// A bare name called where class members are in scope (Java's `f()`): a path's head,
    /// that at each class body around it names the methods of the class's family
    /// (`Resolver::inherited`).
    Method,
    /// A name read, not called: `Value` but that, where class members are in scope (Java), a
    /// method does not bind it, since fields and methods are separate namespaces (`repo` in
    /// `repo.save()` is the field `repo`, never a method `repo()`). In Kotlin the members of a
    /// class of the repository that receives a block around it (`apply`, `run`, `with`, an
    /// extension function) come before the names outside the block, after its locals.
    Read,
    /// `Read`, asked for the object a name holds (`Code::declared`, the flow of `Follow::Values`):
    /// a receiver of a block whose type is not the repository's may hold one of any name, so
    /// the name is unknown there.
    Member,
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
            Reading::Value | Reading::Path | Reading::Method | Reading::Read | Reading::Member
        )
    }
}

/// A local whose class, or a class whose family, is being resolved: one met again while
/// resolving itself (`a: b.C` and `b: a.D`, `class A(B)` and `class B(A)`) resolves to
/// nothing.
#[derive(PartialEq)]
enum Busy<'a> {
    Local(&'a str, &'a str, &'a str),
    /// A call whose result is being resolved (`Call::on`, `Class::Result`): its file and place.
    Call(&'a str, usize),
    Family(Target<'a>),
}

/// The most receivers a class is followed through, each naming its class through the next
/// (`x: y.C`, `x = y.C()`, or the result of a call on it, Rust's `let x = y.f()`); deeper, the
/// class is unknown.
// ponytail: real receivers name their class through a few others; the limit keeps a crafted
// chain of thousands of parameters from overflowing the stack.
const RECEIVER_DEPTH: usize = 64;

/// What resolving one name carries along: what is being resolved, the families linearized so
/// far (while `Resolver::new` linearizes them all), and whether a receiver was cut at
/// `RECEIVER_DEPTH`, or a family a member lookup reached was cut at `FAMILY_DEPTH`, since the
/// outermost family began. The cut counts the receivers or bases of the walk that reached a
/// family, so every family then being linearized gives up (`Cut`).
#[derive(Default)]
struct Work<'a> {
    busy: Vec<Busy<'a>>,
    families: HashMap<Target<'a>, Family<'a>>,
    cut: bool,
}

/// One entry of a class family: a class of the repository, or a base that is not one
/// (outside the repository, or not resolved), whose members are not known. An opaque base is
/// named by what it imports (`abc.ABC`) or as written (`object`), so classes that share it
/// share one entry; a base that is no name (`mixin()`) is named by where it is written.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Ancestor<'a> {
    Class(Target<'a>),
    Opaque(String),
}

/// A class and its ancestors in method resolution order (C3), or `None` when they have none:
/// C3 rejects the hierarchy, the bases form a cycle, or the family is longer than
/// `FAMILY_DEPTH`.
type Family<'a> = Option<Vec<Ancestor<'a>>>;

/// The most classes a family holds, and so the deepest the linearization recurses down the
/// bases. A longer family is absent: every member of the class is unknown.
// ponytail: real hierarchies hold a few dozen classes; the limit keeps a crafted chain of
// thousands from overflowing the stack and from keeping families quadratic in its length.
const FAMILY_DEPTH: usize = 128;

/// A linearization given up because the bases nest deeper than `FAMILY_DEPTH` below the
/// families being resolved, or a receiver or a family reached through a member was cut
/// (`Work::cut`): no family is kept on the way up, since the cut depends on where the walk
/// started.
struct Cut;

/// A file's parameters and the names their object patterns destructure, by function and name:
/// the position of the first parameter, then field of one, that spells it.
type Params<'a> = HashMap<(&'a str, &'a str), (usize, Option<usize>)>;

/// The files of one language pack, resolved together.
pub struct Resolver<'a> {
    files: &'a BTreeMap<&'a str, &'a Facts>,
    defs: HashMap<&'a str, HashMap<&'a str, &'a Def>>,
    overloads: HashMap<&'a str, HashMap<&'a str, Vec<Target<'a>>>>,
    constructors: HashMap<&'a str, HashMap<&'a str, Vec<Target<'a>>>>,
    /// The argument counts each overload takes, fewest to most (`Arity`).
    arities: HashMap<Target<'a>, Arity>,
    statics: HashSet<Target<'a>>,
    members_in_scope: bool,
    unordered_globs: bool,
    qualified_types: bool,
    consts: HashMap<&'a str, HashSet<&'a str>>,
    /// The names some file of the repository writes through a selector (`cfg.Base = ..`): a
    /// constant marked `.Name` (Go). A variable of such a name holds no one value.
    written: HashSet<&'a str>,
    /// The other names bindings make (`Facts::locals`): each hides an outer name.
    locals: HashMap<&'a str, HashSet<&'a str>>,
    /// Each file's `Facts::receivers`, by scope and name.
    receivers: HashMap<&'a str, HashMap<(&'a str, &'a str), &'a Class>>,
    /// Each file's parameters and the names their object patterns destructure.
    params: HashMap<&'a str, Params<'a>>,
    /// Each file's `Facts::exports`, by the name they give.
    named: HashMap<&'a str, Named<'a>>,
    /// Each file's `Facts::imports`, by the `local` they bind.
    by_local: HashMap<&'a str, Named<'a>>,
    /// Each file's `Facts::externs`.
    externs: HashMap<&'a str, HashSet<&'a str>>,
    /// The files with a path import (`Imported::Path`).
    pathed: HashSet<&'a str>,
    /// What each file's star import takes (`Facts::star`).
    takers: HashMap<&'a str, Taker<'a>>,
    /// Every class's family, linearized once.
    families: HashMap<Target<'a>, Family<'a>>,
    /// Each class's Kotlin companion (`Resolver::companion`), read once.
    companions: HashMap<Target<'a>, Target<'a>>,
    /// The pack's keyword for the instance of the enclosing class (`LanguagePack::receiver`).
    receiver: Option<&'static str>,
    /// The pack's keyword for the base of the enclosing class (`LanguagePack::base_receiver`).
    base_receiver: Option<&'static str>,
    /// The pack's `LanguagePack::unordered_bases`.
    unordered_bases: bool,
    /// The pack's `LanguagePack::implicit_members`.
    implicit: &'static [&'static str],
    /// The pack's names bound everywhere (`LanguagePack::globals`).
    globals: &'static [&'static str],
    /// The call sites whose single target is each definition, `exact`, sorted. One only a name
    /// match found (`data.get(...)`, or a bare name no import binds) may be on any object or
    /// reach another function: it is not kept, so a wrapper or a parameter is never climbed
    /// through it.
    callers: HashMap<Target<'a>, Vec<Caller<'a>>>,
    /// Name inference candidates, sorted: methods by their last name (`true`), top-level
    /// functions and classes, and those of modules written in place, by their name (`false`).
    by_name: HashMap<(bool, &'a str), Vec<Target<'a>>>,
    modules: Modules,
    /// The blocks that add members to each class (`Facts::impls`), the inherent ones first.
    impls: HashMap<Target<'a>, Vec<Block<'a>>>,
    /// The class each block adds members to, by its file and scope: what `Self` names in it.
    blocks: HashMap<(&'a str, &'a str), Target<'a>>,
    /// The blocks that add members to a type from outside the repository, by file and scope
    /// (`impl Trait for reqwest::Client`): what `self` and `Self` name in them is opaque.
    outside: HashSet<(&'a str, &'a str)>,
    /// The blocks whose type is no class of the repository, by file and scope: what a bare call
    /// in one reaches is not known (Kotlin's `apply` on a value of an unread type).
    unread: HashSet<(&'a str, &'a str)>,
    /// The extension functions (`Facts::extensions`), by name.
    extensions: HashMap<&'a str, Vec<Extension<'a>>>,
    /// The files whose member calls may reach an extension function
    /// (`LanguagePack::extension_functions`).
    extending: HashSet<&'a str>,
    /// The files whose exports other packages can import, when the pack says which
    /// (`Layout::published`).
    published: Option<BTreeSet<String>>,
    /// The definitions a name match may reach from each file, when the pack says which
    /// (`Layout::reaches`).
    reaches: Option<Reaches>,
    /// The pack's `LanguagePack::stars_bind`.
    stars_bind: bool,
    /// The pack's `LanguagePack::strict`.
    strict: bool,
    /// Where stars bind, each file's star imports, the last first as `Facts::exports` lists
    /// them and the last of one specifier alone, and how many of them each top-level name comes
    /// before: those written after its last binding (`rebound`).
    later: HashMap<&'a str, (Vec<&'a str>, HashMap<&'a str, usize>)>,
    /// The group of files each file shares its top-level scope with (`Layout::shared`), by its
    /// place in `groups`.
    shared: HashMap<&'a str, usize>,
    groups: Vec<Group<'a>>,
}

/// Whether `inner` is a class nested directly in the class `outer`.
fn nested_in(inner: Target, outer: Target) -> bool {
    inner.path == outer.path && crate::parent(inner.name) == outer.name
}

/// The argument counts a function takes: one per parameter, fewer for the ones a call may leave
/// out (`Binds::Optional`), and any number more for a `Binds::Rest` one (Kotlin's `vararg`).
#[derive(Clone, Copy)]
struct Arity {
    least: usize,
    most: Option<usize>,
}

impl Arity {
    fn of(params: &[Param]) -> Self {
        let required = |p: &&Param| !matches!(p.binds, Binds::Optional | Binds::Rest);
        Arity {
            least: params.iter().filter(required).count(),
            most: (!params.iter().any(|p| p.binds == Binds::Rest)).then_some(params.len()),
        }
    }

    fn fits(self, arity: usize) -> bool {
        self.least <= arity && self.most.is_none_or(|most| arity <= most)
    }
}

/// Files that share one top-level scope (`Layout::shared`), sorted, and the files whose top
/// level binds each name: a definition that is no method, or a constant.
struct Group<'a> {
    files: Vec<&'a str>,
    tops: HashMap<&'a str, Vec<&'a str>>,
    /// The files whose definition of each name (`defs`) is a method, in order.
    methods: HashMap<&'a str, Vec<&'a str>>,
    /// Where in `files` sit the files that name each name in their exports, and those with a
    /// star export.
    exporting: Named<'a>,
}

/// An extension function (`Facts::extensions`): the scope it is written in (its file's package
/// at the top level), the class of the repository it extends (none for a type from outside the
/// repository or a type parameter), and whether its file alone sees it (`private`).
struct Extension<'a> {
    target: Target<'a>,
    within: &'a str,
    class: Option<Target<'a>>,
    private: bool,
}

/// A block of members of a class (`lang::Impl`): its file and scope, and for a trait's block,
/// the trait when it is a class of the repository.
struct Block<'a> {
    path: &'a str,
    scope: &'a str,
    of_trait: bool,
    of: Option<Target<'a>>,
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

    /// `new`, with the groups of files that share one top-level scope (`Layout::shared`).
    pub fn with_shared(
        files: &'a BTreeMap<&'a str, &'a Facts>,
        modules: Modules,
        shared: Vec<Vec<String>>,
        language: &dyn LanguagePack,
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
                                    arities.insert(target, Arity::of(&def.params));
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
        let overloads = families(|f| &f.overloads);
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
            groups.push(Group {
                files: members,
                tops,
                methods,
                exporting,
            });
        }
        let written = (files.values())
            .flat_map(|f| f.consts.iter())
            .filter_map(|c| c.name.strip_prefix('.'))
            .collect();
        // Those of the definition `defs` keeps for each name.
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
        let locals = files
            .iter()
            .map(|(path, facts)| (*path, facts.locals.iter().map(String::as_str).collect()))
            .collect();
        let receivers = files
            .iter()
            .map(|(path, facts)| {
                let names = (facts.receivers.iter())
                    .map(|l| ((l.scope.as_str(), l.name.as_str()), &l.class))
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
        for (path, facts) in files {
            let names: &HashMap<&str, &Def> = &defs[path];
            // Whether `scope` is the top level or modules written in place, all the way up.
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
                let key = match (def.kind, name.rsplit_once('.')) {
                    (Kind::Method, Some((_, last))) => (true, last),
                    (Kind::Function | Kind::Class, None) => (false, name),
                    // A path reaches one in a module written in place as it does a top-level
                    // one.
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
                        // The latest of one module's star imports is the one that decides.
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
        let mut resolver = Self {
            files,
            defs,
            overloads,
            constructors,
            arities,
            statics,
            members_in_scope: language.members_in_scope(),
            unordered_globs: language.unordered_globs(),
            qualified_types: language.qualified_types(),
            written,
            consts,
            locals,
            receivers,
            params,
            named,
            by_local,
            externs,
            pathed,
            takers,
            families: HashMap::new(),
            companions: HashMap::new(),
            receiver: language.receiver(),
            base_receiver: language.base_receiver(),
            unordered_bases: language.unordered_bases(),
            implicit: language.implicit_members(),
            globals: language.globals(),
            callers: HashMap::new(),
            by_name,
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
        };
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
        // Each class's Kotlin companion, read once: a call on the class's name reaches it first.
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
                // A block that is a definition itself (Kotlin's lambda with a receiver) says
                // what `this` is in it, and declares no member.
                if (resolver.defs[path].get(block.scope.as_str()))
                    .is_some_and(|d| d.kind == Kind::Inline)
                {
                    continue;
                }
                impls.entry(ty).or_default().push(Block {
                    path,
                    scope: &block.scope,
                    of_trait: block.of.is_some(),
                    of: block.of.as_deref().and_then(class),
                });
            }
            // An extension's block says what `this` is in it too; the functions it declares are
            // found by name, never as the type's members.
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
                // A method of a class of the file the block is named like (`class Foo` beside
                // `fun Foo.x()`) is a member, no extension.
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
        let mut callers: HashMap<Target, Vec<Caller>> = HashMap::new();
        for (path, facts) in files {
            for (index, call) in facts.calls.iter().enumerate() {
                if let Resolution::Exact(target) = resolver.call(path, call) {
                    callers
                        .entry(target)
                        .or_default()
                        .push(Caller { path, index });
                }
            }
        }
        resolver.callers = callers;
        resolver
    }

    /// Limits `exported` to these files (`Layout::published`); `None` keeps every file.
    pub fn publishing(mut self, published: Option<BTreeSet<String>>) -> Self {
        self.published = published;
        self
    }

    /// Limits name matches to the definitions each file reaches (`Layout::reaches`); `None`
    /// reaches every file. `callers` holds exact calls alone, which this leaves as they are.
    pub fn reaching(mut self, reaches: Option<Reaches>) -> Self {
        self.reaches = reaches;
        self
    }

    pub fn call(&self, path: &'a str, call: &'a Call) -> Resolution<'a> {
        self.resolved(path, call, &mut Work::default())
    }

    /// `call`, with what resolving it carries along (a chain of calls on call results is
    /// bounded by `RECEIVER_DEPTH`).
    fn resolved(&self, path: &'a str, call: &'a Call, work: &mut Work<'a>) -> Resolution<'a> {
        if self.strict && call.decorates.is_some() {
            return Resolution::Unresolved;
        }
        let (scope, callee) = (call.scope.as_str(), call.callee.as_str());
        let seen = &mut self.seen(path, scope);
        // The result of the call this one is made on, read once: the name match below reads it
        // too, and reading it twice per call would double the work at every call of a chain.
        let mut on = None;
        let binding = match (call.on, call.constructs, call.by_path) {
            (Some(inner), false, _) => {
                let members = callee.split_once('.').map_or("", |(_, m)| m);
                let result = self.result_of(path, inner, work);
                on = Some(result.clone());
                self.walk(result, members, seen, work, Reading::Value)
            }
            (_, true, _) => self.resolve_as(path, scope, callee, seen, work, Reading::Type),
            (_, false, true) if self.members_in_scope && !callee.contains('.') => {
                self.resolve_as(path, scope, callee, seen, work, Reading::Method)
            }
            (_, false, true) => self.resolve_as(path, scope, callee, seen, work, Reading::Path),
            (_, false, false) => self.resolve_as(path, scope, callee, seen, work, Reading::Value),
        };
        let binding = match binding {
            Binding::Several(all) if call.constructs => {
                let classes: Vec<_> = all.into_iter().filter(|t| self.is_class(*t)).collect();
                match classes[..] {
                    [] => return Resolution::Unresolved,
                    [one] => Binding::Def(one),
                    _ => Binding::Several(classes),
                }
            }
            found => found,
        };
        if call.constructs && matches!(binding, Binding::Def(t) if !self.is_class(t)) {
            return Resolution::Unresolved;
        }
        let binding = match binding {
            Binding::Def(t) if call.arity.is_some() && self.is_class(t) => {
                let (families, absent) = if call.constructs {
                    (&self.constructors, Binding::Def(t))
                } else {
                    (&self.overloads, Binding::Unknown)
                };
                families[t.path]
                    .get(t.name)
                    .map_or(absent, |all| Binding::Several(all.clone()))
            }
            found => found,
        };
        let known = matches!(binding, Binding::Def(_) | Binding::Several(_));
        let binding = self.narrow(binding, call.arity);
        // No member fits: an extension function in scope may.
        if matches!(binding, Binding::Unknown | Binding::External(..))
            && let Some(found) = self.extended(path, call, on.clone(), work)
        {
            return found;
        }
        if known && matches!(binding, Binding::Unknown) {
            return Resolution::Unresolved;
        }
        match binding {
            Binding::Def(t) => Resolution::Exact(t),
            Binding::Several(all) => Resolution::Ambiguous(all),
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

    /// The name match of a call that no binding answers (`infer`). A call narrowed by its
    /// argument count (`Call::arity`, a pack whose overloads merge across a class family) reads
    /// the family that owns it: a receiver whose class the source writes, `this` and a bare
    /// call match within that class's family alone, and a bare call whose family has an opaque
    /// base and declares none falls back to what its scope binds, as a name match. A base call
    /// (`super.f()`) the family does not answer resolves to nothing. Any other receiver keeps the
    /// repository's candidates, and several of them take the one declared in the family of the
    /// class around the call, other than the calling method itself.
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
        let all = self.candidates(path, callee, call.by_path, call.arity, bare);
        let owner = match head {
            None if bare => self.class(path, scope),
            None => Binding::Unknown,
            Some(head) => {
                let seen = &mut self.seen(path, scope);
                let receiver = match (on, call.on) {
                    (Some(known), _) => known,
                    (None, Some(inner)) => self.result_of(path, inner, work),
                    (None, None) => self.resolve_as(path, scope, head, seen, work, Reading::Value),
                };
                self.instance(receiver, work)
            }
        };
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
        let classes: HashSet<&Target<'a>> = (family.iter())
            .filter_map(|a| match a {
                Ancestor::Class(c) => Some(c),
                Ancestor::Opaque(_) => None,
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
            [] if bare && family.iter().any(|a| matches!(a, Ancestor::Opaque(_))) => {
                // What the scope would bind with the family's base read as declaring nothing:
                // that base may declare the name and hide it.
                let found = self.resolve_as(
                    path,
                    scope,
                    callee,
                    &mut self.seen(path, scope),
                    work,
                    Reading::Path,
                );
                match self.narrow(found, call.arity) {
                    Binding::Def(t) => narrowed(t, all.len().max(1)),
                    Binding::Several(several) => Resolution::Ambiguous(several),
                    _ => Resolution::Unresolved,
                }
            }
            [] => Resolution::Unresolved,
        }
    }

    /// What the call at `index` of `path`'s calls gives (`Call::on`, `Class::Result`): the class
    /// its exact target's declared result names (`Def::result`), or something outside the
    /// repository; unknown otherwise, and past `RECEIVER_DEPTH` calls and receivers.
    fn result_of(&self, path: &'a str, index: usize, work: &mut Work<'a>) -> Binding<'a> {
        let Some(call) = self.files[path].calls.get(index) else {
            return Binding::Unknown;
        };
        let key = Busy::Call(path, index);
        let depth = (work.busy.iter())
            .filter(|b| matches!(b, Busy::Local(..) | Busy::Call(..)))
            .count();
        if depth >= RECEIVER_DEPTH {
            work.cut = true;
            return Binding::Unknown;
        }
        if work.busy.contains(&key) {
            return Binding::Unknown;
        }
        work.busy.push(key);
        let found = match self.resolved(path, call, work) {
            Resolution::Exact(t) => match &self.defs[t.path][t.name].result {
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
        work.busy.pop();
        found
    }

    fn narrow(&self, binding: Binding<'a>, arity: Option<usize>) -> Binding<'a> {
        let Some(arity) = arity else { return binding };
        let fits = |t: &Target<'a>| self.arities.get(t).is_none_or(|n| n.fits(arity));
        match binding {
            Binding::Def(t) if !fits(&t) => Binding::Unknown,
            Binding::Several(all) => match all.into_iter().filter(fits).collect::<Vec<_>>()[..] {
                [] => Binding::Unknown,
                [one] => Binding::Def(one),
                ref several => Binding::Several(several.to_vec()),
            },
            found => found,
        }
    }

    /// A call of a pack with extension functions (`LanguagePack::extension_functions`) that no
    /// member answers: the extensions in scope (`visible`) that may apply to its receiver, as
    /// Kotlin picks one. On a class of the repository whose family is all known, the first scope
    /// that holds one the family may take gives the one its nearest class extends, `exact`. One of
    /// a type from outside the repository or of a type parameter may apply to any receiver
    /// (`Any`, `T`), and an outside receiver's members are unknown (Opaque outside): those are
    /// name matches. `None` when no extension in scope may apply, or the receiver is unknown or
    /// no instance (a type's name, `Outside.create()`): the name match reads it as any other.
    fn extended(
        &self,
        path: &'a str,
        call: &'a Call,
        on: Option<Binding<'a>>,
        work: &mut Work<'a>,
    ) -> Option<Resolution<'a>> {
        if !self.extending.contains(path) || call.constructs {
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
            Some(head) => match (on, call.on) {
                (Some(known), _) => known,
                (None, Some(inner)) => self.result_of(path, inner, work),
                (None, None) => {
                    let seen = &mut self.seen(path, scope);
                    let found = self.resolve_as(path, scope, head, seen, work, Reading::Value);
                    // A type's name calls its companion or its statics.
                    self.holder(&found)?;
                    self.instance(found, work)
                }
            },
        };
        let family = match receiver {
            Binding::Def(t) | Binding::Instance(t) if self.is_class(t) => self.family(t, work),
            Binding::External(..) => None,
            _ => return None,
        };
        // Where each may apply: the place of its class in the family (`None` for an outside type
        // or a type parameter's), with the scope that holds it.
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
        let known = (family.as_ref())
            .is_some_and(|f| f.iter().all(|a| matches!(a, Ancestor::Class(_))))
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

    /// Where Kotlin finds the extension `e`, for a call of `name` written in `scope` of `path`,
    /// first to last: a scope around the call that declares it (0), an explicit import (1), the
    /// file's package (2), a star import (3); `None` out of scope.
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

    /// Whether a method called on a call chain (`client.get(u).send()`) belongs to a type from
    /// outside the repository: the name the chain starts at (`Call::root`) resolves, with the
    /// last method, to something outside it, as a direct `client.send()` does. Such a call
    /// resolves to nothing, never to a repository method of the same name.
    fn owned_outside(&self, path: &'a str, call: &'a Call) -> bool {
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

    /// Whether, in a strict pack, the call chain `call` is made on (`rocket::build().mount(..)`,
    /// `Call::root` none)
    /// starts at a call that reaches outside the repository: the innermost call that begins
    /// where `call` does, a path or a name read as its own call is (a path's head among types
    /// and modules, so a `fn rocket()` of the file does not hide the crate).
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

    /// Whether, in a strict pack (`LanguagePack::strict`), the receiver or the path head of
    /// `call` is a type of the repository the source writes (`s: Store`, `Store::`): the member
    /// its blocks lack is outside the repository, not any member of the name.
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
        matches!(found, Binding::Def(t) | Binding::Instance(t) if self.is_class(t))
            || (enclosing && self.in_outside_block(path, &call.scope))
    }

    /// Whether the nearest block or class around `scope` is a block of a type from outside the
    /// repository (`outside`), whose `self` is opaque.
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

    /// `resolve` of one name on its own, with nothing seen or being resolved yet.
    fn binding(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut self.seen(path, scope), &mut Work::default());
        self.resolve_as(path, scope, callee, seen, work, Reading::Value)
    }

    /// `read`, for the object the name holds (`Reading::Member`).
    fn read_member(&self, path: &'a str, scope: &'a str, name: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut self.seen(path, scope), &mut Work::default());
        self.resolve_as(path, scope, name, seen, work, Reading::Member)
    }

    /// `binding` of a name read, not called (`Reading::Read`).
    fn read(&self, path: &'a str, scope: &'a str, name: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut self.seen(path, scope), &mut Work::default());
        self.resolve_as(path, scope, name, seen, work, Reading::Read)
    }

    /// `binding` of a path (Rust's `store::Store::new`, `Call::by_path`): its head names a
    /// module or a type, which a local or a parameter of the same name does not hide.
    fn path_binding(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut self.seen(path, scope), &mut Work::default());
        self.resolve_as(path, scope, callee, seen, work, Reading::Path)
    }

    /// What a callee-like name (`a.b.c`) written in `scope` refers to. A member of a class, or
    /// of a receiver whose class the source writes, is looked up through the class's family.
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

    /// `resolve`, read as `reading` says.
    fn resolve_as(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        seen: &mut Seen<'a>,
        work: &mut Work<'a>,
        reading: Reading,
    ) -> Binding<'a> {
        if let Some((head, members)) = callee.split_once('.')
            && Some(head) == self.base_receiver
        {
            let (first, rest) = members.split_once('.').unwrap_or((members, ""));
            let found = self.base_member(path, scope, first, work);
            return self.walk(found, rest, seen, work, reading);
        }
        let lexical = |seen: &mut Seen<'a>| {
            let (head, members) = callee.split_once('.').unwrap_or((callee, ""));
            // The head of a member chain is read, not called.
            let head_reading = match reading {
                Reading::Value | Reading::Any if !members.is_empty() && self.members_in_scope => {
                    Reading::Read
                }
                reading => reading,
            };
            let binding = match head {
                // `this.m()` is a call on an instance, never on the class's name.
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
        // A head bound only as a package (a type's first segment, in `Facts::externs`) may
        // start a fully qualified type.
        if self.qualified_types
            && matches!(binding, Binding::Unknown | Binding::Module(_))
            && !seen.cut
            && let Some(found) = self.qualified_type(path, callee, seen)
        {
            (binding, members) = found;
        }
        self.walk(binding, members, seen, work, reading)
    }

    /// `binding`'s dotted `members`, each looked up in what the one before gives (`member` for
    /// a class or a receiver whose class the source writes).
    fn walk(
        &self,
        mut binding: Binding<'a>,
        members: &'a str,
        seen: &mut Seen<'a>,
        work: &mut Work<'a>,
        reading: Reading,
    ) -> Binding<'a> {
        let members: Vec<&str> = members.split('.').filter(|m| !m.is_empty()).collect();
        for (n, member) in members.iter().enumerate() {
            let last = n + 1 == members.len();
            let value = reading.value() && last;
            // A member read on the way, or read last, is no method where fields and methods
            // are separate namespaces (`Reading::Read`).
            let read = self.members_in_scope && (!last || reading.read());
            binding = match self.instance(binding, work) {
                // A class's name (`Type.m()`, `Type.X`) reaches a Kotlin companion's members
                // before the class's, which only an instance reaches; past the companion, a
                // nested class or a constant of the class (an enum entry).
                Binding::Def(t)
                    if self.is_class(t)
                        && let Some(&c) = self.companions.get(&t) =>
                {
                    match self.member(c, member, read, work) {
                        Binding::Unknown => match self.member(t, member, read, work) {
                            found @ Binding::Def(d) if self.is_class(d) => found,
                            found @ Binding::Const(..) => found,
                            _ => Binding::Unknown,
                        },
                        found => found,
                    }
                }
                Binding::Def(t) | Binding::Instance(t) if self.is_class(t) => {
                    self.member(t, member, read, work)
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
                            .map(|t| self.member(t, member, read, work))
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

    /// A written type path without an import, after lexical lookup left its head unbound.
    /// Only a repository class proves a binding; an unknown expression stays unknown.
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

    /// A member of a module written in place that no item of it is: a name its `pub use` brings
    /// (a row of the exports under the dotted name), else, for a module declared in its file
    /// (Rust's `mod store;`, the file an import of its name binds), what that file exports.
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

    /// The file, scope and name under which `Facts::receivers` may hold the class of what
    /// `binding` is: a parameter, a local, a constant.
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
            // A constant is a name its scope binds once.
            Binding::Const(path, constant) => match constant.rsplit_once('.') {
                Some((scope, name)) => (path, scope, name),
                None => (path, "", constant),
            },
            _ => return None,
        })
    }

    /// What a parameter or a local holds, as far as the source writes its class: an instance
    /// of a class of the repository, something from outside it, or unknown (also more than
    /// `RECEIVER_DEPTH` receivers deep). Other bindings stay as they are.
    fn instance(&self, binding: Binding<'a>, work: &mut Work<'a>) -> Binding<'a> {
        let Some((path, scope, name)) = self.holder(&binding) else {
            return binding;
        };
        let Some(class) = self.receivers[path].get(&(scope, name)) else {
            return Binding::Unknown;
        };
        if let Class::Imported = class {
            return self.imported(path, name, &mut self.seen(path, scope));
        }
        let key = Busy::Local(path, scope, name);
        let depth = (work.busy.iter())
            .filter(|b| matches!(b, Busy::Local(..) | Busy::Call(..)))
            .count();
        if depth >= RECEIVER_DEPTH {
            work.cut = true;
            return Binding::Unknown;
        }
        if work.busy.contains(&key) {
            return Binding::Unknown;
        }
        work.busy.push(key);
        let found = match class {
            Class::Enclosing => self.class(path, scope),
            Class::Named(class) => {
                self.resolve(path, scope, class, &mut self.seen(path, scope), work)
            }
            Class::Type(class) => {
                let (seen, reading) = (&mut self.seen(path, scope), Reading::Path);
                self.resolve_as(path, scope, class, seen, work, reading)
            }
            Class::Returned(callee) => self.returned(path, scope, callee, false, work),
            Class::Called(callee) => self.returned(path, scope, callee, true, work),
            Class::Result(index) => self.result_of(path, *index, work),
            Class::Imported | Class::Unknown => Binding::Unknown,
        };
        work.busy.pop();
        match found {
            Binding::Def(t) if self.is_class(t) => Binding::Instance(t),
            external @ Binding::External(..) => external,
            _ => Binding::Unknown,
        }
    }

    /// What a call to `callee`, written in `scope`, gives: the class its definition writes as
    /// its result (`Def::result`), the class itself for a constructor, or something outside the
    /// repository for a call that reaches outside. A call that is not exact gives nothing known.
    /// Only `instance` calls it, so `RECEIVER_DEPTH` bounds a chain of results. A callee written
    /// as a `value` is read as an expression (`Class::Called`), a dotted one otherwise as a path.
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
                // `Self` is the type the call named (`Counter` in `Counter::make()`), not the
                // trait whose default body the call found.
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

    /// `member` of `class`: the first class of its family that defines it, or binds it as an
    /// attribute; for an overload family, each signature from the first class that declares
    /// it (`overloaded`), unless the member is `read` (a field, never a method). A base whose
    /// members are not known, met first, leaves it unknown, and so does a family C3 rejects.
    fn member(
        &self,
        class: Target<'a>,
        member: &'a str,
        read: bool,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        let Some(family) = self.family(class, work) else {
            // A class nested in the class is its own, whatever its bases: found while the family
            // is being linearized too (a Kotlin companion, written as a base of its class).
            let qualified = format!("{}.{member}", class.name);
            return match self.defs[class.path].get_key_value(qualified.as_str()) {
                Some((name, def)) if def.kind == Kind::Class => Binding::Def(Target {
                    path: class.path,
                    name,
                }),
                _ => Binding::Unknown,
            };
        };
        if !read && let Some(found) = self.overloaded(&family, member) {
            return found;
        }
        for ancestor in family {
            let Ancestor::Class(c) = ancestor else {
                return Binding::Unknown;
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
        }
        match self.companion(class, work) {
            Some(companion) => self.member(companion, member, read, work),
            None => Binding::Unknown,
        }
    }

    /// The overloads of `member` a family declares (`Facts::overloads`), as `member` reads a
    /// name: in family order, each written signature from the first class that declares it,
    /// so a class's `f(int)` overrides its base's and keeps the base's `f()`. `None` when no
    /// class declares one; unknown when the family has a base whose members are not known
    /// anywhere, since overloads merge across the family and that base may declare or override
    /// any of them (Opaque outside, strict: #353 tracks a relaxation). Other bindings of the
    /// name (a field) are not methods and do not join the family, nor does a Kotlin extension
    /// written in its class's file (`fun Foo.x()` beside `class Foo`, a function named under
    /// the class): it is reached through `Resolver::extensions` alone, after the members.
    fn overloaded(&self, family: &[Ancestor<'a>], member: &str) -> Option<Binding<'a>> {
        let (mut found, mut signatures, mut opaque) = (Vec::new(), HashSet::new(), false);
        for ancestor in family {
            let Ancestor::Class(c) = *ancestor else {
                opaque = true;
                continue;
            };
            if self.overloads[c.path].is_empty() {
                continue;
            }
            let qualified = format!("{}.{member}", c.name);
            if self.defs[c.path].contains_key(qualified.as_str()) {
                continue;
            }
            let Some(all) = self.overloads[c.path].get(qualified.as_str()) else {
                continue;
            };
            for t in all {
                if self.extends(*t) {
                    continue;
                }
                if signatures.insert(t.name.strip_prefix(c.name).unwrap_or(t.name)) {
                    found.push(*t);
                }
            }
        }
        found.sort();
        match found[..] {
            [] => None,
            _ if opaque => Some(Binding::Unknown),
            [one] => Some(Binding::Def(one)),
            _ => Some(Binding::Several(found)),
        }
    }

    /// `member` of the base of the class around `scope` (`super.f()`): the overloads its family
    /// declares past the class itself (`overloaded`), else unknown.
    fn base_member(
        &self,
        path: &'a str,
        scope: &str,
        member: &str,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        let Binding::Def(class) = self.class(path, scope) else {
            return Binding::Unknown;
        };
        let family = self.family(class, work).unwrap_or_default();
        (family.get(1..))
            .and_then(|bases| self.overloaded(bases, member))
            .unwrap_or(Binding::Unknown)
    }

    /// The field or constant `name` the family of class `class` of `path` binds, read from a
    /// bare name in its body (`members_in_scope`): the first class that binds it, as `member`
    /// does; unknown when a base whose members are not known comes first. `None` when no class
    /// binds it, so outer scopes are read, or while families are being linearized, when the
    /// class's own names stand.
    fn inherited_field(&self, path: &'a str, class: &'a str, name: &'a str) -> Option<Binding<'a>> {
        let Some(family) = self.families.get(&Target { path, name: class })? else {
            return Some(Binding::Unknown);
        };
        for ancestor in family {
            let Ancestor::Class(c) = *ancestor else {
                return Some(Binding::Unknown);
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
        // A Kotlin companion's own fields come after the family's, as its methods do; a base
        // of the companion's that is not known hides nothing of the names outside.
        let companion = self.companion(Target { path, name: class }, &mut Work::default())?;
        (self.inherited_field(companion.path, companion.name, name))
            .filter(|found| !matches!(found, Binding::Unknown))
    }

    /// What a bare method name written in the body of class `class` of `path` names
    /// (`Reading::Method`): the overloads its family declares (`overloaded`). Unknown when the
    /// family is absent, or when none declares it and a base whose members are not known may,
    /// or every class has it without declaring it (`LanguagePack::implicit_members`);
    /// `None` when the name is no member, so outer scopes are read, or while families are being
    /// linearized, when the class's own overloads stand.
    fn inherited(&self, path: &'a str, class: &'a str, name: &str) -> Option<Binding<'a>> {
        let family = self.families.get(&Target { path, name: class })?;
        let Some(family) = family else {
            return Some(Binding::Unknown);
        };
        match self.overloaded(family, name) {
            None if self.implicit.contains(&name)
                || family.iter().any(|a| matches!(a, Ancestor::Opaque(_))) =>
            {
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

    /// The class nested in `class` that `class` writes as a base: a Kotlin companion object,
    /// whose members a call on the class's name and a bare call in it reach after the class's
    /// own family. No other language can write one (a class extending its own nested class is a
    /// cycle in Java), so the written bases are checked by name before any is resolved.
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

    /// Whether `t` is a Kotlin extension function (`Resolver::extensions`): named under the
    /// class it extends when written in that class's file, it is never one of its members.
    fn extends(&self, t: Target<'a>) -> bool {
        let name = t.name.split('(').next().unwrap_or(t.name);
        let member = name.rsplit('.').next().unwrap_or(name);
        (self.extensions.get(member)).is_some_and(|all| all.iter().any(|e| e.target == t))
    }

    /// The method `qualified` (`Type.method`) of `class` that another file of its package
    /// declares (`Layout::shared`): Go declares methods apart from the type.
    fn sibling_method(&self, class: Target<'a>, qualified: &str) -> Option<Binding<'a>> {
        let group = &self.groups[*self.shared.get(class.path)?];
        let (&name, files) = group.methods.get_key_value(qualified)?;
        let path = *files.iter().find(|f| **f != class.path)?;
        Some(Binding::Def(Target { path, name }))
    }

    /// `member` of `class` in its blocks (`Facts::impls`): the inherent blocks' first, else the
    /// trait blocks' that define it or whose trait gives it a default body. Several definitions
    /// at that step are `Several` (two `impl From<_>` blocks, two traits); none, `None`.
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

    /// `class` and its ancestors in C3 order, from the bases each class writes: Python's
    /// method resolution order. Without written bases (TypeScript) it is `class` alone.
    fn family(&self, class: Target<'a>, work: &mut Work<'a>) -> Family<'a> {
        let first = !(work.busy.iter()).any(|b| matches!(b, Busy::Family(_)));
        if first {
            work.cut = false;
        }
        self.lineage(class, work).unwrap_or_else(|Cut| {
            // From no other family, a cut of the bases is the class's own: longer than the
            // limit. Reached from another family (through a member one of its bases names, as
            // in `class Z(K.D)`), the cut counts that family's bases too, and a receiver cut
            // depends on the families linearized before: those being linearized give up, and
            // each is linearized again when asked for.
            if first && !work.cut {
                work.families.insert(class, None);
            } else {
                work.cut = true;
            }
            None
        })
    }

    /// `family`, or `Cut` when the bases nest deeper than `FAMILY_DEPTH` below the families
    /// `work` is resolving, or a receiver was cut on the way (`Work::cut`).
    fn lineage(&self, class: Target<'a>, work: &mut Work<'a>) -> Result<Family<'a>, Cut> {
        if let Some(family) = self.families.get(&class).or(work.families.get(&class)) {
            return Ok(family.clone());
        }
        let key = Busy::Family(class);
        if work.busy.contains(&key) {
            return Ok(None);
        }
        if work.cut
            || (work.busy.iter())
                .filter(|b| matches!(b, Busy::Family(_)))
                .count()
                >= FAMILY_DEPTH
        {
            return Err(Cut);
        }
        work.busy.push(key);
        let mut bases = self.bases(class, work);
        if work.cut {
            work.busy.pop();
            return Err(Cut);
        }
        // A class nested in the class and written as its base (a Kotlin companion object) is
        // no ancestor of its instances: `Resolver::companion` reads it after the family.
        bases.retain(|b| !matches!(b, Ancestor::Class(b) if nested_in(*b, class)));
        let mut lists = Vec::new();
        for base in &bases {
            lists.push(match base {
                Ancestor::Class(base) => match self.lineage(*base, work) {
                    Ok(family) => family,
                    Err(cut) => {
                        work.busy.pop();
                        return Err(cut);
                    }
                },
                Ancestor::Opaque(_) => Some(vec![base.clone()]),
            });
        }
        // The written order is a precedence the merge keeps, unless the pack says it is none.
        if !self.unordered_bases {
            lists.push(Some(bases));
        }
        let family = (lists.into_iter().collect::<Option<Vec<_>>>())
            .and_then(merge)
            .map(|rest| [Ancestor::Class(class)].into_iter().chain(rest).collect());
        work.busy.pop();
        work.families.insert(class, family.clone());
        Ok(family)
    }

    /// What each base `class` writes is, in order, resolved from the scope enclosing it.
    fn bases(&self, class: Target<'a>, work: &mut Work<'a>) -> Vec<Ancestor<'a>> {
        let scope = crate::parent(class.name);
        let bases = self.defs[class.path][class.name].bases.iter().enumerate();
        bases
            .map(|(n, base)| {
                match self.resolve(
                    class.path,
                    scope,
                    base,
                    &mut self.seen(class.path, scope),
                    work,
                ) {
                    Binding::Def(t) if self.is_class(t) => Ancestor::Class(t),
                    _ if base.starts_with('(') => {
                        Ancestor::Opaque(format!("{}::{}#{n}", class.path, class.name))
                    }
                    _ => Ancestor::Opaque(self.imported_name(class.path, scope, base)),
                }
            })
            .collect()
    }

    /// `name` as the file's imports spell it: the module and name its head imports, then the
    /// rest (`ABC` imported from `abc` is `abc.ABC`); as written when no import binds it. A
    /// pack's path import (Rust's `use`), bound in the innermost of the scopes around `scope`
    /// that has one, gives the path it names, spelled the same way in turn.
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
        let latest = self.by_local[path].of(head).last();
        match latest.map(|&at| (&imports[at], &imports[at].name)) {
            Some((i, Imported::Namespace | Imported::Qualified)) => format!("{}{rest}", i.source),
            Some((i, Imported::Name(n) | Imported::Member { name: n, .. })) => {
                format!("{}.{n}{rest}", i.source)
            }
            _ => name,
        }
    }

    /// `one_of` the name match's candidates (`candidates`).
    fn infer(
        &self,
        path: &str,
        callee: &str,
        by_path: bool,
        arity: Option<usize>,
        bare: bool,
    ) -> Resolution<'a> {
        one_of(self.candidates(path, callee, by_path, arity, bare))
    }

    /// The definitions whose name matches the callee's last name anywhere `path` reaches
    /// (`reaches`), narrowed by the argument count: methods for a member call (`x.area()`) and
    /// for a `bare` one where class members are in scope, top-level functions and classes for
    /// a plain call (`area()`), both for a path (`a::area()`, `by_path`).
    fn candidates(
        &self,
        path: &str,
        callee: &str,
        by_path: bool,
        arity: Option<usize>,
        bare: bool,
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
        if let Some(reaches) = &self.reaches {
            found.retain(|t| reaches(path, t.path));
        }
        if let Some(arity) = arity {
            found.retain(|t| self.arities.get(t).is_none_or(|n| n.fits(arity)));
        }
        // A file whose calls reach no extension function (Java's) matches none by name either.
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

    /// The class whose body, or one of whose blocks (`Facts::impls`), encloses `scope`, for
    /// `this` and Rust's `Self`.
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

    /// A `Seen` to resolve a name written in `scope` of `path` with: one that runs while the
    /// module loads, unless it is in a function (`Seen::load`).
    fn seen(&self, path: &'a str, scope: &str) -> Seen<'a> {
        let mut s = scope;
        while !s.is_empty() {
            let program = self.files[path].programs.iter().any(|p| p == s);
            if !program
                && !self.defs[path]
                    .get(s)
                    .is_some_and(|d| d.kind == Kind::Class)
            {
                return Seen::new();
            }
            s = crate::parent(s);
        }
        Seen {
            load: Some(path),
            ..Seen::new()
        }
    }

    /// What `name` refers to from `scope`: the innermost enclosing definition first (a local
    /// binding, which hides the name; else what it defines, its constants, its parameters, its
    /// receivers), then the top level (where a star import written after every other binding
    /// of the name, a hiding one included, may rebind it first, `rebound`), then the file's
    /// imports. Methods are not lexical bindings, and a class body's names are in scope only for
    /// the statements written directly in it, not for its methods or nested classes. A module
    /// written in place (`Kind::Module`) ends the climb: the names around it are not its own,
    /// only the file's imports are.
    fn lexical(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        self.lexical_as(path, scope, name, seen, Reading::Any)
    }

    /// `lexical`, read as `reading` says: among types and modules alone for a path's head (no
    /// local, parameter or receiver, which are values, hides it), and for a called name with a
    /// module written in place no value.
    fn lexical_as(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        reading: Reading,
    ) -> Binding<'a> {
        let defs = &self.defs[path];
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
                    // A function is no head of a path called (`rocket::build()` beside a
                    // `fn rocket`), though a `use` may name one.
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
                        // A block of a class reaches its members; one of an unread type, whatever.
                        // A local function of the code around a block comes first.
                        if reading == Reading::Method
                            && (self.blocks.contains_key(&(path, s))
                                || self.unread.contains(&(path, s)))
                            && let Some(found) = self.local_function(path, s, name)
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
                            && let Some(overloads) = self.overloads[path].get(candidate.as_str())
                        {
                            return match overloads[..] {
                                [one] => Binding::Def(one),
                                _ => Binding::Several(overloads.clone()),
                            };
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
            // A `let`, a `catch` binding, a destructured name: what it holds is not known, and
            // it hides every name it spells, outer ones and those of its own definition (a
            // definition, constant or parameter it may shadow in a block alike).
            if self.locals[path].contains(candidate.as_str()) {
                return Binding::Hidden;
            }
            if let Some((name, def)) = defs.get_key_value(candidate.as_str())
                && def.kind != Kind::Method
                && !(def.kind == Kind::Module
                    && matches!(reading, Reading::Value | Reading::Read | Reading::Member))
            {
                return Binding::Def(Target { path, name });
            }
            let read = reading.read() && self.members_in_scope;
            if !read && let Some(overloads) = self.overloads[path].get(candidate.as_str()) {
                return match overloads[..] {
                    [one] => Binding::Def(one),
                    _ => Binding::Several(overloads.clone()),
                };
            }
            // A field a class body sees may be its base's (`protected Repo repo`), before an
            // outer class's.
            if self.members_in_scope
                && defs.get(s).is_some_and(|d| d.kind == Kind::Class)
                && let Some(found) = self.inherited_field(path, s, name)
            {
                return found;
            }
            if let Some(constant) = self.consts[path].get(candidate.as_str()) {
                return Binding::Const(path, constant);
            }
            if let Some(&(n, field)) = self.params[path].get(&(s, name)) {
                return Binding::Param(Target { path, name: s }, n, field);
            }
            match self.receivers[path].get(&(s, name)) {
                Some(Class::Imported) => return self.imported_in(path, name, seen, true),
                Some(_) => return Binding::Local(path, s, name),
                None => {}
            }
            // The receiver of a block (a Kotlin `with`, `apply`, `run`, or an extension
            // function) has its members before the names outside the block, after the locals
            // of the code around it: a field of a class of the repository is that name. A
            // receiver of another type may have one of any name, which only the object a name
            // holds asks (`Reading::Member`): a constant's value and a call's receiver are read
            // outside the block (a Ktor `Application` or `Route` declares no `V1`). A called
            // name reads the block's methods (`Reading::Method`).
            if self.members_in_scope && reading.read() && !self.local_around(path, s, name) {
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

    /// Whether a function or lambda around the block `block`, up to a class or the top level,
    /// binds `name` (a parameter, a local, a binding with a class): Kotlin reads a local before
    /// a member of an implicit receiver.
    fn local_around(&self, path: &'a str, block: &'a str, name: &'a str) -> bool {
        let defs = &self.defs[path];
        let mut s = crate::parent(block);
        while (defs.get(s))
            .is_some_and(|d| matches!(d.kind, Kind::Function | Kind::Method | Kind::Inline))
        {
            let qualified = crate::qualify(s, name);
            if self.locals[path].contains(qualified.as_str())
                || self.consts[path].contains(qualified.as_str())
                || self.params[path].contains_key(&(s, name))
                || self.receivers[path].contains_key(&(s, name))
            {
                return true;
            }
            s = crate::parent(s);
        }
        false
    }

    /// The local function `name` of the functions around the block `scope` (Kotlin's `fun size()`
    /// declared where an `apply` lambda calls `size()`), up to a class or the top level.
    fn local_function(&self, path: &'a str, scope: &str, name: &str) -> Option<Binding<'a>> {
        let defs = &self.defs[path];
        let mut s = crate::parent(scope);
        while (defs.get(s))
            .is_some_and(|d| matches!(d.kind, Kind::Function | Kind::Method | Kind::Inline))
        {
            if let Some(found) = self.overloads[path].get(format!("{s}.{name}").as_str()) {
                return Some(match found[..] {
                    [one] => Binding::Def(one),
                    _ => Binding::Several(found.clone()),
                });
            }
            s = crate::parent(s);
        }
        None
    }

    /// Whether the top-level name `name` of `path` is not the one thing its constant says (Go: a
    /// variable another file of the package assigns, or any file of the repository writes
    /// through a selector). In the first case another file `path` shares its scope with
    /// (`Layout::shared`) binds the name too.
    fn redeclared(&self, path: &'a str, name: &str) -> bool {
        // A variable (`Name.var`) some file writes through a selector, in any package.
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

    /// What the top level of the other files `path` shares its scope with (`Layout::shared`)
    /// binds `name` to: a definition or a constant, `Several` when files define it apart.
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

    /// What the file's imports bind `name` to. Several imports may bind one name: the latest
    /// in source order decides, whatever it resolves to. An import of every name binds
    /// `name` only if the module exports it (`star`), to whatever it is there. The imports a
    /// pack scopes (`Imported::Path` and `Glob`, Rust's `use`) are not these: `imported_at` reads
    /// them where they are written, and a name no import binds is a crate outside the file when
    /// the pack says so (`Facts::externs`).
    // ponytail: a star import from outside the repository (`from numpy import *`) is taken to
    // bind nothing, here, in `rebound` and in `own_export`, since what it takes is not known: a
    // name it rebinds stays linked to the import or definition before it (`from _heapq import
    // *` after `def heappush`). Taking it to rebind every name loses about as many right links
    // as wrong ones on the standard library.
    fn imported(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        self.imported_in(path, name, seen, true)
    }

    /// `imported`, the file's own top level (`top`) reached or not: a module written in place
    /// does not see the globs of the file's top level.
    fn imported_in(
        &self,
        path: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        top: bool,
    ) -> Binding<'a> {
        // Only the imports that bind `name` or every name may give it, the latest first.
        let (imports, index) = (&self.files[path].imports, &self.by_local[path]);
        let mut at = [index.of(name), index.stars.as_slice()].concat();
        at.sort_unstable();
        at.dedup();
        let given = at.into_iter().rev().map(|at| {
            let import = &imports[at];
            let scoped = matches!(import.name, Imported::Path(_) | Imported::Glob { .. });
            // A module declared in the file (Rust's `mod x;`) is in scope where its
            // definition is.
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

    /// What the imports a pack writes in `scope` (the top level, a module written in place, a
    /// function: Rust's `use`) bind `name` to, if any: a path import of the name, else a glob
    /// that gives it. An import met again while it is being resolved is skipped: its own path
    /// begins with a name it binds (`use tokio::{self}`), which is then the outer one.
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
        }
        // The globs of the top level come after the file's own imports (`imported`).
        match scope.is_empty() {
            true => None,
            false => self.globs(path, scope, name, false, seen),
        }
    }

    /// The outside sources of the top-level on-demand imports of `path` that may bind `name`,
    /// sorted: a package (`import a.b.*;`) or a class (`import static a.B.*;`). Called for a
    /// name nothing else binds (`Binding::Unknown` without a cut). Empty when any glob may give
    /// the name from the repository, or several globs give it ambiguously, or a link was cut:
    /// then the name is not an on-demand import's with certainty.
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
            let work = &mut Work::default();
            let bound = self.resolve_as(path, "", target, seen, work, Reading::Alias);
            let source = match bound {
                Binding::External(source, Imported::Namespace | Imported::Qualified) => {
                    Some(source)
                }
                // An `inside` glob (the own package's; Kotlin's written ones, which see
                // `internal` members) gives an outside source only, as the repository's names
                // are bound before this is asked.
                _ if *inside => None,
                other => match self.through_glob(other, false, name, work, seen) {
                    Binding::Unknown => None,
                    _ => return Vec::new(),
                },
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

    /// What a glob import whose target is `bound` gives `name` (`globs`).
    fn through_glob(
        &self,
        bound: Binding<'a>,
        inside: bool,
        name: &'a str,
        work: &mut Work<'a>,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        match bound {
            Binding::Module(m) if inside => self.export(m, name, seen),
            Binding::Module(m) => self.star(m, name, seen).unwrap_or(Binding::Unknown),
            // A module written in place states no visibility of its items: only a glob
            // from inside it, which sees them all, is read.
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
                let found = self.member(t, name, false, work);
                match found {
                    Binding::Def(t) if self.statics.contains(&t) => Binding::Def(t),
                    Binding::Several(all) => one(all
                        .into_iter()
                        .filter(|t| self.statics.contains(t))
                        .map(Binding::Def)
                        .collect())
                    .unwrap_or(Binding::Unknown),
                    Binding::Const(path, name) if self.statics.contains(&Target { path, name }) => {
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

    /// What the glob imports written in `scope` give `name`, the latest first; `public` limits
    /// them to the re-exporting ones (`pub use`). A glob is asked for a name once: it may reach
    /// itself. A glob cut at `LINK_DEPTH` may give the name: it is unknown, and no other glob
    /// binds it in its place.
    fn globs(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        public: bool,
        seen: &mut Seen<'a>,
    ) -> Option<Binding<'a>> {
        let imports = &self.files[path].imports;
        let mut candidates = Vec::new();
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
            if seen.depth >= LINK_DEPTH {
                seen.cut = true;
                return Some(Binding::Unknown);
            }
            if !seen.globs.insert((path, at, name)) {
                continue;
            }
            seen.depth += 1;
            let work = &mut Work::default();
            let bound = self.resolve_as(path, scope, target, seen, work, Reading::Alias);
            let found = self.through_glob(bound, *inside, name, work, seen);
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
                if !self.unordered_globs || *inside {
                    return Some(found);
                }
                candidates.push(found);
            }
        }
        alternatives(candidates)
    }

    /// What the callee-like name `target` written in `scope` gives (a `use` path): its head
    /// read among types and modules, so no local hides it. An alias met again while resolving
    /// itself gives nothing; one cut at `LINK_DEPTH` is unknown (`Seen::cut`).
    fn alias(
        &self,
        path: &'a str,
        scope: &'a str,
        target: &'a str,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        if seen.depth >= LINK_DEPTH {
            seen.cut = true;
            return Binding::Unknown;
        }
        if !seen.aliases.insert((path, target)) {
            return Binding::Unknown;
        }
        seen.depth += 1;
        let work = &mut Work::default();
        let found = self.resolve_as(path, scope, target, seen, work, Reading::Alias);
        seen.depth -= 1;
        found
    }

    /// The head of a callee that starts with a dotted import (`import a.b.c` binds `a.b.c`),
    /// the longest such name first, and the members left after it.
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

    /// What an import of `name` from `source`, written in `scope`, gives.
    fn import(
        &self,
        path: &'a str,
        scope: &'a str,
        source: &'a str,
        name: &'a Imported,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let found = match (name, self.module(path, source)) {
            (Imported::Path(target), _) => return self.alias(path, scope, target, seen),
            (Imported::Glob { .. }, _) => return Binding::Unknown,
            (_, None) => Binding::External(source, name),
            (Imported::Namespace, Some(module)) => Binding::Module(module),
            (Imported::Qualified, Some(module)) => self.export(module, source, seen),
            (Imported::Name(name) | Imported::Member { name, .. }, Some(module)) => {
                self.export(module, name, seen)
            }
            (Imported::All, Some(_)) => Binding::Unknown,
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

    /// What a star import of `path` takes as `name` (`exports`), else what its re-exporting
    /// globs give it (Rust's `pub use m::*`). An unknown it reaches through a cycle
    /// (`Seen::looped`) is absent, so that an earlier binding gives the name; one cut at
    /// `LINK_DEPTH` stays unknown.
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

    /// What a star import of `source` from `path` takes as `name` (`star`), and whether it may
    /// not take it (`Star::Computed`).
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

    /// What `path` exports as `name`: all it lets an explicit import name.
    fn export(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        self.export_as(path, name, seen, false)
    }

    /// `export`, for a `value` (the callee of a call) when two names are exported as one: of a
    /// module (Rust's `mod x;`) and of an item (`pub use x::x;`), the item.
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

    /// `export_as`, or `None` when `path` exports no `name`, so that another star may bind it.
    /// An export of what is not known (a constant, something from outside the repository, an
    /// export cut more than `LINK_DEPTH` links deep, where `seen` records the cut) is still the
    /// name's: no other star binds it. A file that shares its scope (`Layout::shared`) exports
    /// what the files of its group that name it in their exports give, `Several` when they
    /// define it apart.
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
        // Only the files that name `name` in their exports, or export every name, may give it.
        let Group {
            files, exporting, ..
        } = &self.groups[group];
        let mut at = [exporting.of(name), exporting.stars.as_slice()].concat();
        at.sort_unstable();
        at.dedup();
        let found = at
            .into_iter()
            .filter_map(|at| self.file_exports(files[at], name, seen, value));
        one(found.collect())
    }

    /// `exports` of the one file `path`.
    fn file_exports(
        &self,
        path: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        value: bool,
    ) -> Option<Binding<'a>> {
        if seen.depth >= LINK_DEPTH {
            seen.cut = true;
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

    /// `exports`, once `path` and `name` are seen. Where stars bind (`stars_bind`), the exports
    /// are listed last binding first, and the first that gives `name` exports it. Otherwise
    /// the module's own export of the name decides, else its `export *`: the binding they give
    /// it when they all give the same one, as in ES modules; two that differ leave the name
    /// unknown, still the module's, so that no other star binds it. With neither, the globs of
    /// its top level give it (Rust's `use m::*`).
    fn own_export(
        &self,
        path: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        value: bool,
    ) -> Option<Binding<'a>> {
        let (exports, index) = (&self.files[path].exports, &self.named[path]);
        if self.stars_bind {
            // Only the name's own exports and the star ones may give it: those, in order.
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
            return self.globs(path, "", name, false, seen);
        };
        match given.all(|other| same(&first, &other)) {
            true => Some(first),
            false => Some(Binding::Unknown),
        }
    }

    /// What one export of `path` gives `name`, if it gives it, read as a value when `value`.
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
                // The item of a module written in place is exported under its dotted name, and
                // read from inside that module.
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
                Some(self.lexical_as(path, scope, bare, seen, reading))
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

    /// Where stars bind (`stars_bind`), what a star import written after every other top-level
    /// binding of `name` in `path` binds it to (`later`), the last first, when its module exports
    /// the name (one from outside the repository binds nothing, `imported`).
    fn rebound(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Option<Binding<'a>> {
        let (stars, after) = self.later.get(path)?;
        let stars = &stars[..*after.get(name)?];
        let given = stars
            .iter()
            .map(|source| self.taken(path, source, name, seen));
        let found = latest(given, true)?;
        // Code run while the module loads may come before the star import, and see the binding
        // before it.
        // ponytail: where the code sits is not compared with the star import's line, so what a
        // star import rebinds is unknown to all of it.
        Some(match seen.load == Some(path) {
            true => Binding::Unknown,
            false => found,
        })
    }

    /// The family of each public class past the class itself, for a pack whose overloads merge
    /// across it (one that declares `Facts::overloads`, Java): what an import of the repository
    /// as a package merges the class's overloads over (spec #26). A class with no base has none.
    /// Sorted.
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

    /// The functions that take more than one count of arguments: a default parameter, or a
    /// vararg (`Arity`). A function with a fixed count has no row. For an import of the
    /// repository as a package to tell which calls its overloads take. Sorted.
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

    /// What each file exports, for an import of the repository as a package (spec #22), as
    /// `export` reads it: for each name a file exports, the definition, the module-level
    /// constant or type (issue #175) or the whole file it names, or nothing when that is outside
    /// the repository or not known (an import from another package stops there); for each `export * from` a file of the repository, that
    /// file. The names a pack keeps to the package (`Facts::private`, but for one that is also
    /// public: `mod x; pub use x::x;`) are left out, so are the files the pack does not publish
    /// (`publishing`), and a `pub use` glob of a file gives each name that file's star takes.
    /// Sorted.
    pub fn exported(&self) -> Vec<Exported> {
        let mut rows = BTreeSet::new();
        for (&path, facts) in self.files {
            if self.published.as_ref().is_some_and(|p| !p.contains(path)) {
                continue;
            }
            let private: HashSet<&str> = facts.private.iter().map(String::as_str).collect();
            for export in &facts.exports {
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
                rows.extend(found.into_iter().map(|row| (path, row)));
            }
            let mut names = BTreeSet::new();
            self.glob_names(path, &mut HashSet::new(), &mut names);
            names.retain(|n| self.named[path].of(n).is_empty());
            let found = names
                .into_iter()
                .flat_map(|name| self.rows(path, name, false));
            rows.extend(found.map(|row| (path, row)));
        }
        (rows.into_iter())
            .map(|(path, (name, file, target))| Exported {
                path: path.to_string(),
                name: name.map(String::from),
                file: file.map(String::from),
                target: target.map(String::from),
            })
            .collect()
    }

    /// The rows of `file` exporting `exported`: its name, the file and the definition it gives;
    /// two when a module and an item share the name (`mod x; pub use x::x;`), one when the
    /// module is `private` (only the item is exported).
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

    /// The row of `file` exporting `exported`, read as an item when `value`.
    fn row(&self, path: &'a str, exported: &'a str, value: bool) -> Row<'a> {
        match self.export_as(path, exported, &mut Seen::new(), value) {
            Binding::Def(t) => (Some(exported), Some(t.path), Some(t.name)),
            Binding::Module(m) => (Some(exported), Some(m), None),
            // A constant or a type: the shape a package keeps of it (issue #175) is found
            // through it; it is no symbol a call reaches.
            Binding::Const(at, c) => (Some(exported), Some(at), Some(c)),
            _ => (Some(exported), None, None),
        }
    }

    /// The names the `pub use` globs of `path` give, through the files they glob (each once).
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

    /// The file an import of `specifier` from `path` names, if the pack parses it.
    fn module(&self, path: &str, specifier: &str) -> Option<&'a str> {
        (self.modules)(path, specifier)
            .iter()
            .find_map(|candidate| self.files.get_key_value(candidate.as_str()))
            .map(|(path, _)| *path)
    }
}

/// What the latest of `bindings` (the latest first) binds a name to, if one binds it: the first
/// that gives it, unless it may not (`Resolver::taken`), when the one before it decides as well,
/// and so on; two that differ leave the name unknown, and so does one that may not when
/// something before them all (`before`) binds the name.
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

/// The one binding several files that share a scope give a name, when they give the same one:
/// `None` when none gives it, `Several` when they define it apart, unknown otherwise.
/// A name match over these candidates: none, `inferred` to one, or `ambiguous` among several.
fn one_of(found: Vec<Target>) -> Resolution {
    match found[..] {
        [] => Resolution::Unresolved,
        [one] => Resolution::Inferred(one),
        _ => Resolution::Ambiguous(found),
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

/// Whether two bindings are the same one, as two `export *` must give a name for a module to
/// export it. What is not known is the same as nothing else.
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

/// The C3 merge of `lists`: repeatedly the first head that is in no list's tail. `None` when
/// none is, or when it would make a family (the class, then the merge) longer than
/// `FAMILY_DEPTH`.
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

/// The keyword arguments of a call, as `(name, value)`.
pub type Keywords = [(String, Value)];

/// One name a file of the repository exports (`Resolver::exported`), or one of its `export *`.
#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    /// The exporting file.
    pub path: String,
    /// The exported name; `None` for an `export * from` the file `file`.
    pub name: Option<String>,
    /// The file holding what `name` names; `None` when that is not in the repository or not
    /// known, which ends the lookup of `name` in `path`.
    pub file: Option<String>,
    /// The qualified name of the definition, constant or type in `file`; `None` when `name` is
    /// the whole file.
    pub target: Option<String>,
}

/// A public class's family past the class itself (`Resolver::ancestries`).
#[derive(Clone, Debug, PartialEq)]
pub struct Ancestry {
    /// The file declaring the class.
    pub path: String,
    /// The class's qualified name.
    pub class: String,
    /// Its ancestors in method resolution order: a class of the repository as `(file, qualified
    /// name)`, one outside it or not resolved (opaque) as `None`. `None` for a family C3 rejects
    /// or that is cut, whose members are not known.
    pub ancestors: Option<Vec<Option<(String, String)>>>,
}

/// A function that takes more than one count of arguments (`Resolver::flexible`): from `least`
/// to `most`, any number more when that is `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct Flexible {
    /// The file declaring it.
    pub path: String,
    /// Its qualified name, with its parameter types.
    pub name: String,
    pub least: usize,
    pub most: Option<usize>,
}

/// A call whose arguments bind the parameters of `def`, the function it reaches.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub def: Target<'a>,
    pub path: &'a str,
    pub index: usize,
}

/// What a wrapper expansion starts from.
#[derive(Clone, Copy)]
pub enum Expand<'a> {
    /// The call at an index of a file.
    Call(&'a str, usize),
    /// The arguments of the `new` expression the `const` `held` (file, name) holds, on behalf of
    /// the call `at` made on that constant.
    Constructed {
        held: (&'a str, &'a str),
        at: (&'a str, usize),
    },
    /// The call at an index of a file, read as one `Value::Call`: the calls of the builder chain
    /// it is made on (its receivers) and every argument along it are folded, and a name an import
    /// from outside binds is a `Value::External` spelled where it is written, so an enum member
    /// passed through a wrapper keeps the evidence of the caller's imports. The call's keyword
    /// arguments are folded the same way and given to the reader.
    Chain(&'a str, usize),
}

/// What `Code::expand` finds at one site.
pub struct Expanded<'a, R> {
    /// The site's file and call position.
    pub site: (&'a str, usize),
    /// The wrapper calls climbed to reach the site, innermost first; the last is the site.
    pub frames: Vec<Frame<'a>>,
    /// What the reader made of the arguments there.
    pub found: R,
}

/// Arguments as written in `path` within `scope`, read on behalf of the call `at`.
struct Written<'a, 'i> {
    path: &'a str,
    scope: &'a str,
    args: &'a [Value],
    kwargs: &'a Keywords,
    at: (&'a str, usize),
    /// The call read whole, its chain folded (`Expand::Chain`).
    chain: Option<&'a Call>,
    /// What the kind reads off annotated fields and parameters (`Code::expand_with`).
    inject: Option<&'i Inject<'a, 'i>>,
}

/// What folding values and tracing where they come from may cost in one file, charged to the
/// file whose code each step reads: each value of it folded, each part copied, each call of it
/// a wrapper climb reaches and each constant, parameter or return value of it `Code::origins`
/// follows. Past it, those values are unknown, the climb stops there and no origin is found
/// through it.
// ponytail: real files spend a few units per call; a crafted one doubling a string through
// forty constants (`const b = a + a`), or a lattice of functions each calling or returning the
// next twice, would cost 2^40.
pub(crate) const EVAL_BUDGET: usize = 1 << 16;

/// How deep folding one value recurses: into an item of a list or an object, the constant a
/// name holds, the argument a caller passes for a parameter. Deeper, the value is unknown. It
/// also bounds the constants, parameters and return values `Code::origins` follows, each into
/// the next (`Follow::Values`), and the constants it follows for `Follow::Constants`.
// ponytail: real values nest a few levels; the limit keeps a crafted chain of thousands of
// constants from overflowing the stack.
const EVAL_DEPTH: usize = 256;

/// How many wrapper calls `Code::expand` climbs from a call; at the limit, the site is read
/// where the climb stops, its parameters unknown.
// ponytail: real clients are wrapped a few levels deep; the limit keeps a crafted chain of
// thousands of wrappers from overflowing the stack.
const WRAPPER_DEPTH: usize = 64;

/// Folding one value: the constants being folded (one met again folds to nothing), how deep it
/// is, whether calls are folded too, an outside name kept (`Expand::Chain`), what the kind
/// reads off the annotations of a field or a parameter (`Code::expand_with`), and how many of
/// the constants being folded hold a value written in a function (`Const::scope`), whose
/// parameters no wrapper frame binds.
#[derive(Default)]
struct Fold<'a, 'i> {
    held: Vec<(&'a str, &'a str)>,
    nested: usize,
    chain: bool,
    inject: Option<&'i Inject<'a, 'i>>,
    foreign: usize,
}

/// What a contract kind reads off one annotation written on a field or a parameter
/// (`Facts::annotated`), given the file and the annotation with what it marks: the value it
/// injects there, or `None` when it is no annotation the kind reads or it injects nothing there.
pub type Inject<'a, 'i> = dyn Fn(&Code<'a>, &'a str, &'a Annotated) -> Option<Value> + 'i;

/// How many calls a builder chain is read through, down to what starts it.
// ponytail: a chain of more builder calls than this is not read to its start; its calls past
// the limit have no origin.
pub const FLUENT_CHAIN: usize = 1024;

/// Where a builder chain stops (`Code::chain_tail`): the call and how many steps lead to it.
#[derive(Clone, Copy)]
struct Tail {
    stop: usize,
    depth: usize,
}

/// What `Code::origins` follows: a constant, a parameter, a function's return values.
#[derive(PartialEq)]
enum Flow<'a> {
    Const(&'a str, &'a str),
    Param(Target<'a>, usize),
    Returns(Target<'a>),
}

/// What `Code::origins` carries along: `frames` bind the parameters of the functions whose
/// return values are being followed, innermost last; `busy` is what is being followed, met
/// again only through a cycle; `out` holds the origins found, each with its routes.
struct Trail<'a> {
    frames: Vec<Frame<'a>>,
    busy: Vec<Flow<'a>>,
    out: BTreeMap<(&'a str, usize), BTreeSet<Route<'a>>>,
    /// The methods of the builder chains being followed, outermost first, which the library
    /// they start at must pass through (`Follow::Values::passes`).
    steps: Vec<&'a str>,
    /// The constant the asked name or value refers to, while the walk is inside it.
    held: Option<(&'a str, &'a str)>,
    /// What the question says the library passes on: the outside name of a constructor and a
    /// method name.
    passes: Passes,
}

impl<'a> Trail<'a> {
    fn new(passes: Passes) -> Self {
        Self {
            frames: Vec::new(),
            busy: Vec::new(),
            out: BTreeMap::new(),
            steps: Vec::new(),
            held: None,
            passes,
        }
    }
}

/// Whether a method, called on what the outside constructor named by the first argument
/// (`axum.Router.new`) made, hands back that same object (`Follow::Values`).
pub type Passes = fn(origin: &str, method: &str) -> bool;

/// The table of a reader whose pack is not fluent: it never answers, and the walk never asks.
pub fn passes_nothing(_origin: &str, _method: &str) -> bool {
    false
}

/// A struct field (`Code::field`): the file and the type that declare it, and its name.
pub type Field<'a> = (&'a str, &'a str, &'a str);

/// The method a call's callee names (`x.y.get` is `get`).
pub fn method(callee: &str) -> &str {
    callee.rsplit('.').next().unwrap_or(callee)
}

/// Values evaluated across files for contract kinds: constants folded where they are
/// defined, imported ones included, and the arguments of client wrapper calls substituted
/// for their parameters.
///
/// The index keeps no values (they hold string literals), so `full` holds the parsed facts
/// of only some files. A lookup into any other file answers `Unknown` and records the file
/// in `misses`; the index parses those files and asks again until nothing is missed.
pub struct Code<'a> {
    resolver: &'a Resolver<'a>,
    full: &'a BTreeMap<&'a str, &'a Facts>,
    misses: RefCell<BTreeSet<&'a str>>,
    chains: RefCell<HashMap<(&'a str, usize), Option<Tail>>>,
    /// What folding and tracing values has spent of each file's `EVAL_BUDGET`.
    folded: RefCell<HashMap<&'a str, usize>>,
    /// Each file of `full` by name, built the first time a name is looked up in it.
    by_name: HashMap<&'a str, OnceCell<ByName<'a>>>,
    /// The marker constants of the literals that set each field (`field_values`), by field name
    /// (`*` for a literal by position), built the first time a field is read.
    literals: OnceCell<HashMap<&'a str, BTreeSet<(&'a str, &'a str)>>>,
    /// The last segment of every callee of the repository (`save` of `repo.save`, `C` of
    /// `(opaque).C`), built the first time a parameter is injected (`Code::may_be_called`).
    callees: OnceCell<HashSet<&'a str>>,
    pack: &'static dyn LanguagePack,
}

/// A file's full definitions and constants by name: the first definition of each name, and its
/// constants of each name in source order.
#[derive(Default)]
struct ByName<'a> {
    defs: HashMap<&'a str, &'a Def>,
    consts: HashMap<&'a str, Vec<&'a Const>>,
    /// The values the locals were given (`Facts::bound`), by scope and name.
    bound: HashMap<(&'a str, &'a str), Vec<&'a Bound>>,
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
        by_name
    }
}

/// What `Code::origins` is asked about.
#[derive(Clone, Copy)]
pub enum Of<'a> {
    /// A name or a member chain written in the scope (`router`, `self.client`).
    Name(&'a str),
    /// A value as written, such as a call's argument or the receiver of a method.
    Value(&'a Value),
}

/// The evidence `Code::origins` accepts, which picks the walk. Each walk is its own, reads its
/// own facts and charges what it reads.
#[derive(Clone, Copy, Debug)]
pub enum Follow {
    /// The value flow: constants and imports, the arguments exact callers pass to parameters,
    /// what functions of the repository return, list items, a class attribute assigned once,
    /// and in a fluent pack a builder chain down its receivers, kept only through methods the
    /// reader's table `passes` on (`Passes`; a pack that is not fluent never consults it).
    /// Charged to `EVAL_BUDGET`, at most `EVAL_DEPTH` deep.
    Values { passes: Passes },
    /// `const` bindings alone, through `new`, calls and the heads of callees, at most
    /// `EVAL_DEPTH` constants deep: a parameter or a return value has no origin.
    Constants,
    /// What the source declares the name is (`Facts::receivers`): a written type, or the call it
    /// was declared from, through the methods called on the way, at most `RECEIVER_DEPTH`
    /// receivers deep.
    Declarations,
}

/// How the call that made an origin's value is written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// A call: a constructor or factory, or a function of the repository whose written result
    /// type is outside (`Declarations`).
    Call,
    /// A `new` (`Constants`).
    New,
    /// A written type (`Declarations`).
    Type,
    /// The bare name, uncalled (`Constants`).
    Name,
}

/// What made a value (`Code::origins`), and every route the walk took to it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Origin<'a> {
    /// The call that made the value, its file and position; `None` when it is read off a
    /// declaration or a constant's expression.
    pub call: Option<(&'a str, usize)>,
    pub form: Form,
    /// What the call that made the value names outside the repository; `None` when it names
    /// nothing outside.
    pub outside: Option<Outside<'a>>,
    /// Sorted, at least one.
    pub routes: Vec<Route<'a>>,
}

/// One route a walk took to an origin.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Route<'a> {
    /// The calls of the repository whose return values the walk climbed through, outermost
    /// first: two objects one function makes differ by the call that made each.
    pub through: Vec<(&'a str, usize)>,
    /// The methods called on what was made, the constructor's first (`kafka.producer()` is
    /// `producer`).
    pub steps: Vec<&'a str>,
    /// The constant the asked name refers to, its file and qualified name.
    pub held: Option<(&'a str, &'a str)>,
}

/// What a name names outside the repository (`Code::outside`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outside<'a> {
    /// The package and the name there, as the binding of the head resolves them through
    /// re-exports (`Code::imported`): `None` for a method of a local made outside (`r.GET`).
    pub imported: Option<(&'a str, String)>,
    /// The name as the file's imports spell it (`fastapi.APIRouter`).
    pub spelled: String,
    /// Whether the head of the name is the import itself, not a value made from it.
    pub direct: bool,
    /// For a pack with `unordered_globs` (Java), when nothing of higher precedence binds the head
    /// and on-demand imports (`import a.b.*;`, `import static a.B.*;`, the pack's implicit ones)
    /// may: the name as each of them would spell it (`a.b.Name`), sorted. An on-demand import is
    /// a candidate, not a fact: compiling code has at most one real candidate for a name, so a
    /// kind accepts the name when exactly one candidate is in its own table, and only a name it
    /// knows is no type of `java.lang` or of any package it cannot see. Empty when an import
    /// binds the name by itself, and when a repository type, an ambiguity or a cut link may
    /// take it.
    pub candidates: Vec<String>,
}

impl Outside<'_> {
    /// The spellings the name may have: the on-demand candidates when only they may bind it,
    /// else the import's own when the name's head is the import itself; none otherwise.
    pub fn spellings(self) -> Vec<String> {
        match self {
            o if !o.candidates.is_empty() => o.candidates,
            o if o.direct => vec![o.spelled],
            _ => Vec::new(),
        }
    }
}

/// What the constants walk found: the form of the call that made the value, what it names
/// outside, the methods called on what it made, the constant traced, and a member read off what
/// was made, not yet called.
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
            chains: RefCell::new(HashMap::new()),
            folded: RefCell::new(HashMap::new()),
            by_name: full.keys().map(|&path| (path, OnceCell::new())).collect(),
            literals: OnceCell::new(),
            callees: OnceCell::new(),
            pack,
        }
    }

    /// Charges `cost` to the `EVAL_BUDGET` of `file`; `false` once it is spent.
    fn charge(&self, file: &'a str, cost: usize) -> bool {
        let mut folded = self.folded.borrow_mut();
        let spent = folded.entry(file).or_default();
        *spent = spent.saturating_add(cost);
        *spent <= EVAL_BUDGET
    }

    /// The language pack whose files these are.
    pub fn pack(&self) -> &'static dyn LanguagePack {
        self.pack
    }

    /// What made the value of `of`, written in `path` within `scope`, as far as `follow` lets
    /// the walk see, sorted by the call that made each: one origin per call, each with every
    /// route the walk took to it. `Constants` and `Declarations` give at most one origin, read
    /// off a constant's expression or a declaration.
    pub fn origins(
        &self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        follow: Follow,
    ) -> Vec<Origin<'a>> {
        match follow {
            Follow::Values { passes } => {
                let mut trail = Trail::new(passes);
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
                trail.out.into_iter().map(origin).collect()
            }
            Follow::Constants => {
                let seen = &mut Vec::new();
                let made = match of {
                    Of::Name(chain) => self.trace(path, scope, chain, seen),
                    Of::Value(value) => self.made_by(path, scope, value, seen),
                };
                // A member read off what was made and not called is no method of the constructor.
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
                    // A value as written declares nothing.
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
                    outside: Some(outside),
                    routes: vec![Route {
                        through: Vec::new(),
                        steps,
                        held,
                    }],
                }]
            }
        }
    }

    /// The constants walk of `chain`: its head an import, or a constant followed into the value
    /// it holds, then its members. A member of what a call or a `new` made is pending until the
    /// call it is the callee of; a second one, or one after a pending one, has no origin.
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
                        // A star import spells no name of its own.
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
                    // `const a = b; const b = a;` comes from nowhere, and a chain of constants
                    // `EVAL_DEPTH` long from nothing known.
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
                // An uncalled member of what was made.
                _ => return None,
            }
        }
        Some(made)
    }

    /// The constants walk of `value`, written in `path` within `scope`: a name, a call of one,
    /// or a `new` of one. A call of a member of what was made is a step; a `new` is one of a
    /// name alone, and what was made is not called again.
    fn made_by(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        seen: &mut Vec<(&'a str, &'a str)>,
    ) -> Option<Made<'a>> {
        match value {
            Value::Ref(name) => self.trace(path, scope, name, seen),
            Value::Call(call) => {
                let mut made = self.trace(path, scope, &call.callee, seen)?;
                match (made.form, made.pending.take()) {
                    (Form::Name, _) => made.form = Form::Call,
                    (_, Some(step)) => made.steps.push(step),
                    // What a call made, called again.
                    (_, None) => return None,
                }
                Some(made)
            }
            Value::New(callee, _) => {
                let mut made = self.trace(path, scope, callee, seen)?;
                // A `new` after steps.
                if made.form != Form::Name {
                    return None;
                }
                made.form = Form::New;
                Some(made)
            }
            _ => None,
        }
    }

    /// The import the head of `chain`, a callee written in `path` within `scope`, names when
    /// that import is of something outside the repository: the specifier, and the chain spelled
    /// from the imported name (`greet`, `default`, `Client.create`; a namespace import adds no
    /// name). A binding of the same name in scope hides the import, as for any call.
    pub fn imported(
        &self,
        path: &'a str,
        scope: &'a str,
        chain: &'a str,
    ) -> Option<(&'a str, String)> {
        self.imported_as(path, scope, chain, Reading::Any)
    }

    /// `imported`, its head read as `reading` says.
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
        // A crate a pack's path import (Rust's `use`) names: the whole path, as `Code::outside` spells it.
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

    /// The same for `head.member`, as a value writes a member of a name (`Value::Member`): a
    /// namespace import's `C.Quote` names `Quote` of its specifier.
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

    /// The files looked into that `full` lacks, sorted.
    pub fn misses(&self) -> Vec<&'a str> {
        self.misses.borrow().iter().copied().collect()
    }

    /// Every file of the pack, with the facts the index keeps (no values).
    pub fn files(&self) -> &'a BTreeMap<&'a str, &'a Facts> {
        self.resolver.files
    }

    /// The arguments of the call at `index` of `path`, as written.
    pub fn args(&self, path: &'a str, index: usize) -> Option<&'a [Value]> {
        Some(self.call(path, index)?.args.as_slice())
    }

    /// The call at `index` of `path`, with its arguments.
    pub fn call(&self, path: &'a str, index: usize) -> Option<&'a Call> {
        self.full(path)?.calls.get(index)
    }

    /// The value the call at `index` of `path` is made on (`Call::receiver`).
    pub fn receiver(&self, path: &'a str, index: usize) -> Option<&'a Value> {
        self.call(path, index)?.receiver.as_ref()
    }

    /// The value the nearest binding of `name` around `scope` of `path` holds (`Facts::bound`,
    /// which only the Kotlin pack fills), when no other binding of the name is in the same scope
    /// (a name declared twice, a loop variable, a parameter, a constant, a property). Each scope
    /// is looked up by name (`ByName::bound` and the resolver's maps), so a file of many locals
    /// and calls costs each lookup its depth, never the file's width.
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
                // A `val` of unknown class has its own entry in `locals`: the pack drops its
                // bound when another binding shares the name. A typed one has a receiver, and no
                // entry.
                || (r.locals[path].contains(qualified.as_str())
                    && (held.is_none() || receiver.is_some()))
                || r.consts[path].contains(qualified.as_str())
                || r.defs[path].get(s).is_some_and(|d| d.params.iter().any(|p| p.name == name))
                || receiver.is_some_and(|c| **c == Class::Unknown);
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

    /// The values the call at `index` of `path` may return, when a function of the repository
    /// answers it (`Exact`) or a name match does (`Inferred`, `Ambiguous`): the file and name
    /// of each function with each value it returns.
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

    /// The names `path` mentions (`Facts::mentions`); `None` where the pack does not tell.
    pub fn mentions(&self, path: &'a str) -> Option<&'a BTreeSet<String>> {
        self.full(path)?.mentions.as_ref()
    }

    /// The scope and value of each `let` of `path` whose name is bound again or assigned again
    /// (`Facts::rebound`).
    pub fn rebound(&self, path: &'a str) -> &'a [(String, Value)] {
        self.full(path).map_or(&[], |f| f.rebound.as_slice())
    }

    /// The `let mut` that `name`, written in `scope` of `path`, refers to (`Facts::mutable`),
    /// looked up from the scope outwards: the scope it is bound in and the value it holds.
    /// A name an inner scope binds again (a closure's parameter, a `let` in a closure given to a
    /// call, a nested function's parameter) hides the outer `let mut`: nothing is returned.
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

    /// The index among the calls of `path` of the one spanning `range` (an `Invocation`'s).
    pub fn call_index(&self, path: &'a str, range: &std::ops::Range<usize>) -> Option<usize> {
        call_at(&self.resolver.files[path].calls, range)
    }

    /// Whether the pack binds `name` everywhere, as JavaScript does `fetch`.
    pub fn binds_everywhere(&self, name: &str) -> bool {
        self.resolver.globals.contains(&name)
    }

    /// What `name` (a callee, a decorator, a written type), written in `scope` of `path`, names
    /// outside the repository, when an import from outside binds it: the package and name the
    /// head's import resolves to, the name as the file's imports spell it, and whether the head
    /// is the import itself. With `by_path`, the name is a path (Rust's `a::b::f()`) whose head
    /// is read among types and modules, so a function of the file that spells like a crate
    /// (`fn rocket()` calling `rocket::build()`) does not hide it.
    pub fn outside(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        by_path: bool,
    ) -> Option<Outside<'a>> {
        let r = self.resolver;
        let (reading, head_reading) = match by_path {
            true => (Reading::Path, Reading::Path),
            false => (Reading::Value, Reading::Any),
        };
        let resolve = |name| {
            let (seen, work) = (&mut r.seen(path, scope), &mut Work::default());
            let found = r.resolve_as(path, scope, name, seen, work, reading);
            (found, seen.cut)
        };
        let external = |name| matches!(resolve(name).0, Binding::External(..));
        let head = name.split_once('.').map_or(name, |(head, _)| head);
        // The on-demand imports that may bind the head, spelled with the whole name.
        let on_demand = || -> Vec<String> {
            let spelled = |source: &str| format!("{source}.{name}");
            let sources = match r.unordered_globs && !by_path {
                true => r.on_demand(path, head),
                false => Vec::new(),
            };
            sources.into_iter().map(spelled).collect()
        };
        let candidates = match resolve(name) {
            // A static on-demand import brings the member, with no import naming the head.
            (Binding::External(_, Imported::Qualified), _)
                if r.by_local[path].of(head).is_empty() =>
            {
                on_demand()
            }
            // A head no import names, which only the file's outside names bind (Java's bare
            // `WebClient` under `import a.*;`): the on-demand imports may bring it.
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

    /// The outside type a type the source writes (`http.Client`), read in `scope` of `path`, is,
    /// spelled as the imports name its package (`net/http.Client`).
    fn outside_written(
        &self,
        path: &'a str,
        scope: &'a str,
        written: &'a str,
    ) -> Option<Outside<'a>> {
        let r = self.resolver;
        let (seen, work) = (&mut r.seen(path, scope), &mut Work::default());
        let found = r.resolve_as(path, scope, written, seen, work, Reading::Path);
        let Binding::External(package, imported) = found else {
            return None;
        };
        let name = written.rsplit('.').next()?;
        let head = written.split('.').next().unwrap_or(written);
        let (spelled, candidates) = match (imported, written.contains('.')) {
            // A single-type import (Java's `import a.B;`) is of the type itself.
            (Imported::Qualified, false) => (package.to_string(), Vec::new()),
            // A head only the file's outside names bind: what the on-demand imports may bring,
            // as `Code::outside` reads it.
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

    /// The declarations walk of `name`: the type the source writes for it, or the call it is
    /// declared from (a function of a package, a function of the repository whose result type
    /// is outside, or a method of a value declared so in turn, a step), at most `RECEIVER_DEPTH`
    /// receivers deep.
    fn declared(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        depth: usize,
    ) -> Option<(Form, Outside<'a>, Vec<&'a str>)> {
        if depth >= RECEIVER_DEPTH {
            return None;
        }
        let r = self.resolver;
        let binding = r.read_member(path, scope, name);
        let (at, within, held) = r.holder(&binding)?;
        match r.receivers[at].get(&(within, held))? {
            Class::Type(written) => {
                let outside = self.outside_written(at, within, written)?;
                Some((Form::Type, outside, Vec::new()))
            }
            Class::Called(callee) => {
                if callee.contains('.')
                    && let Some(function) = self.outside(at, within, callee, false)
                    && function.direct
                {
                    // A function of a package.
                    return Some((Form::Call, function, Vec::new()));
                }
                if let Binding::Def(t) = r.binding(at, within, callee) {
                    // A function of the repository: the type its result writes.
                    let Some(Class::Type(written)) = &r.defs[t.path][t.name].result else {
                        return None;
                    };
                    let outside = self.outside_written(t.path, t.name, written)?;
                    return Some((Form::Call, outside, Vec::new()));
                }
                // A method of a value that was made outside.
                let (receiver, member) = callee.rsplit_once('.')?;
                let (form, outside, mut steps) = self.declared(at, within, receiver, depth + 1)?;
                steps.push(member);
                Some((form, outside, steps))
            }
            _ => None,
        }
    }

    /// The struct field the name `name` (`c.rc`), written in `scope` of `path`, is: the file and
    /// the type that declare it, and the field.
    pub fn field(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<Field<'a>> {
        let Binding::Local(at, ty, field) = self.resolver.read(path, scope, name) else {
            return None;
        };
        let class = self.resolver.defs.get(at)?.get(ty)?;
        (class.kind == Kind::Class).then_some((at, ty, field))
    }

    /// The values the composite literals of the repository give the field `field` (`T{rc: x}`,
    /// `[]T{{rc: x}}`), each with the file and scope it is written in. `None` when a literal may
    /// set it to a value the source does not tell: one by position, one whose type is not read
    /// or not known, or one of a type that declares no such field (an alias). The pack writes
    /// each as a marker constant `{name}` (`{*}` by position) holding `New(type, [value])`, the
    /// type `?` when it is not read.
    // ponytail: a literal of an alias or a defined type (`type B = T`, `type B T`) written by
    // position, and a conversion from another struct type (`T(other)`), are not seen as setting
    // the fields of `T`.
    pub fn field_values(&self, field: Field<'a>) -> Option<Vec<(&'a str, &'a str, &'a Value)>> {
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
            let by_position = marker.ends_with("{*}");
            let scope = scope_of(marker);
            for &literal in self.consts(path, marker) {
                // Each literal read is charged to its file: past the budget, the field is open.
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
                    // Another struct that declares the field, or (by position) any other type.
                    Binding::Def(t)
                        if by_position
                            || (self.resolver.receivers.get(t.path))
                                .is_some_and(|r| r.contains_key(&(t.name, name))) => {}
                    Binding::External(..) => {}
                    _ => return None,
                }
            }
        }
        Some(out)
    }

    /// Whether some file of the repository writes the field `field` through a selector
    /// (`w.Topic = ..`, `*p.Topic = ..`, `&w.Topic`): a value read off a literal may differ.
    pub fn field_written(&self, field: &str) -> bool {
        self.resolver.written.contains(field)
    }

    /// The argument the call at `index` of `path` passes for parameter `n` of `function`: by
    /// position, or by the parameter's name among its keyword arguments. A method called on
    /// an instance (`x.m(a)`) is passed no argument for its receiver parameter.
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
        // The call's receiver binds a class method's first parameter, and a method's when it
        // is called on an instance rather than on the class.
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
        match param.binds {
            Binds::Rest => None,
            Binds::Keyword => call.keyword(&param.name),
            Binds::Any | Binds::Optional | Binds::Instance | Binds::Class => (position)
                .and_then(|at| call.args.get(at))
                .or_else(|| call.keyword(&param.name)),
        }
    }

    /// Whether `trail` may follow `flow`: not one it is following (a cycle), at most
    /// `EVAL_DEPTH` deep, and charged to the `EVAL_BUDGET` of the file it reads.
    fn follows(&self, trail: &Trail<'a>, flow: &Flow<'a>) -> bool {
        let file = match flow {
            Flow::Const(at, _) => at,
            Flow::Param(function, _) | Flow::Returns(function) => function.path,
        };
        !trail.busy.contains(flow) && trail.busy.len() < EVAL_DEPTH && self.charge(file, 1)
    }

    fn flow(&self, path: &'a str, scope: &'a str, value: &'a Value, trail: &mut Trail<'a>) {
        match value {
            Value::Ref(name) => self.flow_name(path, scope, name, trail),
            Value::Call(call) => {
                let calls = &self.resolver.files[path].calls;
                let Some(index) = call_at(calls, &call.range) else {
                    return;
                };
                // A builder chain (`Router::new().route(..).nest(..)`) is the value it started
                // from, down to the call that starts it or that a function of the repository
                // answers. The filter below judges the origins this chain found by the methods
                // it went through, and those alone.
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
                    // The methods the chain went through hand back the object they were called
                    // on only for the library the chain starts at (the question's `passes`):
                    // a value derived from a client (`resp.headers()`) is not a client. An
                    // origin with no outside name, one a repository function or a name match
                    // answered, does not survive a step above it. An origin filtered out goes
                    // with every route this chain found to it.
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
                    for (origin, routes) in std::mem::take(&mut trail.out) {
                        kept.entry(origin).or_default().extend(routes);
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

    /// Records the call at `index` of `path` as an origin, with the route the walk came by: the
    /// calls it climbed, the methods of the chains it went down (the constructor's first) and the
    /// constant asked for.
    fn reached(&self, path: &'a str, index: usize, trail: &mut Trail<'a>) {
        let route = Route {
            through: trail.frames.iter().map(|f| (f.path, f.index)).collect(),
            steps: trail.steps.iter().rev().copied().collect(),
            held: trail.held,
        };
        trail.out.entry((path, index)).or_default().insert(route);
    }

    /// The origins of the call at `index` of `path`, where a chain stops: what a function of
    /// the repository returns, the value the call is made on, or the call itself.
    fn flow_from(&self, path: &'a str, index: usize, trail: &mut Trail<'a>) {
        let calls = &self.resolver.files[path].calls;
        let function = match self.resolver.call(path, &calls[index]) {
            Resolution::Exact(t) if !self.resolver.is_class(t) => t,
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

    /// The calls of the builder chain that ends in the call at `index` of `path`, that call
    /// first, down its receivers to where `chain_tail` stops it: a call a function of the
    /// repository answers, or one made on no call. `None` past `FLUENT_CHAIN` calls. The one
    /// walk down a chain: the value flow and the kinds that read a chain step by step go through
    /// it.
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

    /// The methods of the calls of the chain ending at `index`, above where it stops, outermost
    /// first.
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

    /// Where the builder chain ending in the call at `index` of `path` stops: the first call
    /// down its receivers that a function of the repository answers or that is made on no
    /// call. Each call is walked once, whatever the chain.
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
        // An attribute assigned a call once in its class (`self.client = Client()`) is a
        // constant of the class.
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
                // Followed from nothing else, it is the constant the asked name refers to.
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
            Binding::Param(function, n, None) => {
                // Inside a function whose return value is followed, its call binds the
                // parameter; else every call of it does.
                let sites = match trail.frames.last() {
                    Some(frame) if frame.def == function => vec![(frame.path, frame.index)],
                    // ponytail: a parameter of an outer function, captured by a closure, is
                    // not followed.
                    Some(_) => return,
                    // Its exact callers alone (`callers`): one found only by a name match may
                    // pass any object.
                    None => (self.resolver.callers.get(&function).into_iter().flatten())
                        .map(|c| (c.path, c.index))
                        .collect(),
                };
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
            // ponytail: a name its scope binds more than once, and an attribute not assigned a
            // call once in its class, hold no constant, so what they hold is not followed.
            _ => {}
        }
    }

    /// The type the argument `n` of the call at `index` of `path` is cast to (`Call::arg_types`).
    pub fn arg_type(&self, path: &'a str, index: usize, n: usize) -> Option<&'a Ty> {
        self.full(path)?
            .calls
            .get(index)?
            .arg_types
            .get(n)?
            .as_ref()
    }

    /// The type the constant `constant` of `path` is declared to have (`Const::ty`).
    pub fn constant_type(&self, path: &'a str, constant: &str) -> Option<&'a Ty> {
        let consts = self.consts(path, constant);
        consts.iter().find(|c| c.ty.is_some())?.ty.as_ref()
    }

    /// What the function making the call at `index` of `path` does with its result
    /// (`Call::uses`).
    pub fn uses(&self, path: &'a str, index: usize) -> Option<&'a [Vec<Step>]> {
        Some(self.full(path)?.calls.get(index)?.uses.as_slice())
    }

    /// The declared return type of the definition `name` of `path` (`Def::returns`).
    pub fn returns(&self, path: &'a str, name: &str) -> Option<&'a Ty> {
        self.def(path, name)?.returns.as_ref()
    }

    /// The names of the type parameters of the definition `name` of `path`
    /// (`Def::type_params`).
    pub fn type_params(&self, path: &'a str, name: &str) -> &'a [String] {
        let def = self.def(path, name);
        def.map_or(&[], |d| d.type_params.as_slice())
    }

    fn full(&self, path: &'a str) -> Option<&'a Facts> {
        let found = self.full.get(path).copied();
        if found.is_none() {
            self.misses.borrow_mut().insert(path);
        }
        found
    }

    /// The full facts of `path` by name; a file `full` lacks is missed, as `full` misses it.
    fn by_name(&self, path: &'a str) -> Option<&ByName<'a>> {
        let facts = self.full(path)?;
        Some(self.by_name[path].get_or_init(|| ByName::new(facts)))
    }

    /// The first full definition `name` of `path`.
    fn def(&self, path: &'a str, name: &str) -> Option<&'a Def> {
        self.by_name(path)?.defs.get(name).copied()
    }

    /// The full constants `name` of `path`, in source order.
    fn consts(&self, path: &'a str, name: &str) -> &[&'a Const] {
        let consts = self.by_name(path).and_then(|b| b.consts.get(name));
        consts.map_or(&[], Vec::as_slice)
    }

    /// The parameter at position `n` of `function`.
    fn param(&self, function: Target<'a>, n: usize) -> Option<&'a Param> {
        self.def(function.path, function.name)?.params.get(n)
    }

    /// `value`, written in `path` within `scope`, with its constants folded. Parameters are
    /// unknown.
    pub fn value(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Value {
        let folded = self.eval(path, scope, value, &[], 0, &mut Fold::default(), &mut None);
        unbind(folded, false)
    }

    /// The constant `name` refers to from `scope` in `path`: its file and qualified name, and
    /// its value as written.
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

    /// The value the `const` `constant` of `path` holds, as written, and the scope it is
    /// written in.
    fn held(&self, path: &'a str, constant: &'a str) -> Option<(&'a Value, &'a str)> {
        let consts = self.consts(path, constant);
        let held = consts.iter().find(|c| !matches!(c.value, Value::Type(_)))?;
        Some((
            &held.value,
            held.scope.as_deref().unwrap_or(scope_of(constant)),
        ))
    }

    /// The `interface` or `type` declaration `name` refers to from `scope` in `path` (spec #23),
    /// through the file's imports and the repository's modules: the file it is written in, its
    /// qualified name and the type. A type of a package outside the repository, a class and a
    /// name no declaration spells give `None`.
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
        // Namespaces open no scope: two declarations of one name leave it unknown.
        let mut declared =
            (self.consts(at, constant).iter()).filter(|c| matches!(c.value, Value::Type(_)));
        let declared = declared.next().filter(|_| declared.next().is_none())?;
        match &declared.value {
            Value::Type(ty) => Some((at, constant, ty)),
            _ => None,
        }
    }

    /// Whether `name`, called from `scope` in `path`, is a parameter (or a name an object
    /// parameter destructures) whose default value is the identifier `default`.
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

    /// Whether `name`, written in `path` within `scope`, is the plain parameter at position `n`
    /// of `function`: no binding of the same spelling in between hides it.
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

    /// The function and the position of the plain parameter `name`, written in `path` within
    /// `scope`, is (a closure's capture included); `None` when a binding in between hides it.
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

    /// The arguments the exact callers of `function` pass for its parameter `n` (a call found
    /// only by a name passes any object), each with the path and the scope it is written in.
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

    /// Whether `name`, written in `path` within `scope`, is no binding of the file at all: a
    /// global such as `Response`. A local binding of the name hides the global.
    pub fn is_global(&self, path: &'a str, scope: &'a str, name: &'a str) -> bool {
        matches!(
            self.resolver
                .lexical(path, scope, name, &mut self.resolver.seen(path, scope)),
            Binding::Unknown
        )
    }

    /// The definition `name` (also a member chain, such as `views.index`) refers to from
    /// `scope` in `path`.
    pub fn definition(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<Target<'a>> {
        match self.resolver.binding(path, scope, name) {
            Binding::Def(t) => Some(t),
            _ => None,
        }
    }

    /// The extension functions named `name` that a call of it with `arity` arguments, written in
    /// `scope` of `path`, has in scope, whatever type they extend, sorted: each with where Kotlin
    /// finds it, the nearest first (`Resolver::visible`). A local or a parameter of the name in a
    /// scope around the call hides those declared outside that scope, as Kotlin calls it first.
    /// A kind that knows the implicit receiver keeps the nearest of those that extend it.
    pub fn extensions(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &str,
        arity: Option<usize>,
    ) -> Vec<(u8, Target<'a>)> {
        let r = self.resolver;
        let defs = &r.defs[path];
        // The nearest scope around the call that binds the name to a value of its own.
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
        // Whether the binding is inside the scope an extension is declared in.
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

    /// The method `method` of the class that `name` (a type, or a name whose class the source
    /// writes), written in `scope` of `path`, is an instance of: looked up through its family and
    /// the other files of its package.
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
                match r.member(t, method, false, work) {
                    Binding::Def(t) => Some(t),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The file `name`, written in `scope` of `path`, refers to as a whole module.
    pub fn module(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<&'a str> {
        match self.resolver.binding(path, scope, name) {
            Binding::Module(m) => Some(m),
            _ => None,
        }
    }

    /// Whether other packages may import the exports of `path` (`Layout::published`).
    pub fn published(&self, path: &str) -> bool {
        (self.resolver.published.as_ref()).is_none_or(|files| files.contains(path))
    }

    /// The file an import of `specifier` from `path` names, if the pack parses it.
    pub fn import_file(&self, path: &str, specifier: &str) -> Option<&'a str> {
        self.resolver.module(path, specifier)
    }

    /// What `read` makes of the positional and keyword arguments of `from`, at each call site
    /// that determines them, with the wrapper calls climbed from the call to each site. `from`
    /// is a call, or the `new` expression a `const` holds on behalf of a call made on that
    /// constant (`Route::held`): the call is the site when the arguments evaluate where
    /// the constant stands, and the function enclosing the constant is the client wrapper when they
    /// depend on its parameters. `read` answers `None` when what it needs holds a `Ref`: a
    /// parameter of the enclosing function, which makes that function a client wrapper. Each
    /// call site of the wrapper is then read with its own arguments substituted, through any
    /// number of wrapper levels. A wrapper reached again through its own callers (a cycle) adds
    /// nothing; a wrapper nobody calls is read where it stands, its parameters unknown.
    // ponytail: every simple path up the wrappers' callers is walked; memoize per (wrapper,
    // call site) if a dense call graph makes this slow.
    pub fn expand<R>(
        &self,
        from: Expand<'a>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<Expanded<'a, R>> {
        self.expand_with(from, None, read)
    }

    /// `expand`, with what `inject` reads off the annotations of a field or a parameter
    /// (`Facts::annotated`) as the value it holds where the source gives it none: a field that
    /// no file of the repository assigns (`Code::field_written`), and a parameter no wrapper
    /// frame binds, of a function no exact call reaches. A parameter of a function some exact
    /// call reaches takes the argument each caller passes, as in `expand`: an injected value
    /// never stands in for the callers' (a constructor called in the source, with a value or
    /// an unknown one). A constructor's parameter that a field holds (`Const::scope`) is no
    /// wrapper's: it is what is injected, or unknown. The injected value is transient, like the
    /// arguments it stands for.
    pub fn expand_with<R>(
        &self,
        from: Expand<'a>,
        inject: Option<&Inject<'a, '_>>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<Expanded<'a, R>> {
        let mut out = Vec::new();
        match from {
            Expand::Call(path, index) => {
                // The full call: the resolver's files may be kept from the index, without values.
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
                };
                self.walk(&written, &mut Vec::new(), read, &mut out);
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
                };
                self.walk(&written, &mut Vec::new(), read, &mut out);
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
                };
                self.walk(&written, &mut Vec::new(), read, &mut out);
            }
        }
        out
    }

    fn walk<R>(
        &self,
        written: &Written<'a, '_>,
        frames: &mut Vec<Frame<'a>>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
        out: &mut Vec<Expanded<'a, R>>,
    ) {
        let &Written {
            path,
            scope,
            args,
            kwargs,
            ..
        } = written;
        let (site, at) = frames.last().map_or(written.at, |f| (f.path, f.index));
        if !self.charge(site, 1) {
            return;
        }
        let mut wants = None;
        let chain = written.chain.is_some();
        let mut eval = |v: &'a Value| {
            let fold = &mut Fold {
                chain,
                inject: written.inject,
                ..Fold::default()
            };
            self.eval(path, scope, v, frames, 0, fold, &mut wants)
        };
        let (values, kwargs): (Vec<Value>, Vec<(String, Value)>) = match written.chain {
            Some(call) => {
                let receiver = call.receiver.as_ref().map(|r| Box::new(eval(r)));
                let invocation = Invocation {
                    callee: call.callee.clone(),
                    receiver,
                    args: call.args.iter().map(&mut eval).collect(),
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
        if let Some(found) = read(&values, &kwargs) {
            out.push(Expanded {
                site: (site, at),
                frames: frames.clone(),
                found,
            });
            return;
        }
        let Some(wrapper) = wants else {
            return;
        };
        if frames.iter().any(|f| f.def == wrapper) {
            return;
        }
        // A call that only matches the wrapper's name may be on any object, or reach another
        // function: it is not climbed.
        // ponytail: so a wrapper reached only by a name match (a method on a receiver whose
        // class Meada cannot tell, a bare name no import binds) is read where it stands, its
        // parameters unknown.
        let callers: Vec<Caller> = (self.resolver.callers.get(&wrapper).into_iter().flatten())
            .copied()
            .collect();
        // A wrapper `WRAPPER_DEPTH` calls up is read where it stands too.
        if callers.is_empty() || frames.len() >= WRAPPER_DEPTH {
            let values: Vec<Value> = values.into_iter().map(|v| unbind(v, chain)).collect();
            let kwargs: Vec<(String, Value)> = (kwargs.into_iter())
                .map(|(k, v)| (k, unbind(v, chain)))
                .collect();
            out.extend(read(&values, &kwargs).map(|found| Expanded {
                site: (site, at),
                frames: frames.clone(),
                found,
            }));
            return;
        }
        for caller in callers {
            frames.push(Frame {
                def: wrapper,
                path: caller.path,
                index: caller.index,
            });
            self.walk(written, frames, read, out);
            frames.pop();
        }
    }

    /// Whether a call of the repository may reach `function`, at any level: an exact caller, or
    /// any call whose callee ends in its name (the class's for a constructor), a name match, a
    /// call on an opaque receiver and `this(..)` or `super(..)` included.
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

    /// What `inject` reads off the first annotation of `path` (its full facts) that `marks`
    /// the binding and that it reads.
    fn injected(
        &self,
        path: &'a str,
        marks: impl Fn(&Marked) -> bool,
        inject: &Inject<'a, '_>,
    ) -> Option<Value> {
        let annotated = &self.full(path)?.annotated;
        (annotated.iter())
            .filter(|a| marks(&a.on))
            .find_map(|a| inject(self, path, a))
    }

    /// `value` with its names folded. `frames[depth..]` bind the parameters of the function
    /// enclosing `scope` and of its callers, innermost first; a parameter no frame binds stays
    /// a `Ref`, and its function is recorded in `wants`. Past `EVAL_DEPTH` or the file's
    /// `EVAL_BUDGET` the value is unknown.
    #[allow(clippy::too_many_arguments)]
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
            Value::Str(parts) => {
                let mut out = Vec::new();
                for part in parts {
                    let Part::Ref(name) = part else {
                        out.push(part.clone());
                        continue;
                    };
                    match self.name(path, scope, name, frames, depth, fold, wants) {
                        // The parts copied are charged: a string doubled through constants grows
                        // faster than the values visited.
                        Value::Str(parts) => match self.charge(path, parts.len()) {
                            true => out.extend(parts),
                            false => out.push(Part::Hole),
                        },
                        Value::Ref(name) => out.push(Part::Ref(name)),
                        _ => out.push(Part::Hole),
                    }
                }
                Value::Str(out)
            }
            Value::Object(properties) => Value::Object(
                properties
                    .iter()
                    .map(|(k, v)| {
                        let v = self.eval(path, scope, v, frames, depth, fold, wants);
                        (k.clone(), v)
                    })
                    .collect(),
            ),
            Value::List(items) => Value::List(
                (items.iter())
                    .map(|v| self.eval(path, scope, v, frames, depth, fold, wants))
                    .collect(),
            ),
            Value::New(callee, args) => Value::New(
                callee.clone(),
                (args.iter())
                    .map(|v| self.eval(path, scope, v, frames, depth, fold, wants))
                    .collect(),
            ),
            Value::Call(call) if fold.chain => {
                let mut eval = |v| self.eval(path, scope, v, frames, depth, fold, wants);
                let receiver = call.receiver.as_deref().map(|r| Box::new(eval(r)));
                Value::Call(Invocation {
                    callee: call.callee.clone(),
                    receiver,
                    args: call.args.iter().map(eval).collect(),
                    range: call.range.clone(),
                })
            }
            Value::Ref(name) => self.name(path, scope, name, frames, depth, fold, wants),
            other => other.clone(),
        };
        fold.nested -= 1;
        folded
    }

    #[allow(clippy::too_many_arguments)]
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
        match self.resolver.read(path, scope, name) {
            Binding::Const(at, constant) => {
                // `const a = b; const b = a;` folds to nothing.
                if fold.held.contains(&(at, constant)) || self.resolver.redeclared(at, constant) {
                    return Value::Unknown;
                }
                let Some((value, within)) = self.held(at, constant) else {
                    return Value::Unknown;
                };
                let foreign = usize::from(within != scope_of(constant));
                fold.held.push((at, constant));
                fold.foreign += foreign;
                let folded = self.eval(at, within, value, frames, depth, fold, wants);
                fold.foreign -= foreign;
                fold.held.pop();
                folded
            }
            // `*args` and `**kwargs` hold what no single argument gives.
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
                            self.eval(frame.path, &call.scope, arg, frames, depth + 1, fold, wants)
                        }
                        // A left-out argument takes the parameter's default.
                        (None, Some(default)) => self.eval(
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
                    // A destructured name reads one property of the object passed; anything
                    // but a literal object is unknown. ponytail: a spread makes the name
                    // unknown until a later key sets it, even a spread of an object whose
                    // properties are known; read into the spread object if wrappers need it.
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
                    // No frame binds it: what an annotation injects, when no exact call can
                    // pass it.
                    let called = self.may_be_called(function);
                    let marks = |on: &Marked| matches!(on, Marked::Param(f, at) if f == function.name && *at == n);
                    if let (false, None, Some(inject)) = (called, field, fold.inject)
                        && let Some(value) = self.injected(function.path, marks, inject)
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
            // A local the pack bound to one value (Kotlin's `val`, `Facts::bound`) holds it, read
            // where it is written.
            Binding::Hidden | Binding::Local(..)
                if let Some(bound) = self.local(path, scope, name) =>
            {
                let held = (bound.within.as_str(), bound.name.as_str());
                if fold.held.contains(&held) {
                    return Value::Unknown;
                }
                fold.held.push(held);
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
                folded
            }
            // A field no file assigns holds what an annotation on it injects.
            Binding::Local(at, class, field) => match fold.inject {
                Some(inject) if !self.field_written(field) => {
                    let marks = |on: &Marked| matches!(on, Marked::Field { class: c, name, .. } if c == class && name == field);
                    self.injected(at, marks, inject).unwrap_or(Value::Unknown)
                }
                _ => Value::Unknown,
            },
            Binding::External(..) | Binding::Unknown if fold.chain => {
                match self.outside(path, scope, name, false) {
                    Some(o) if !o.candidates.is_empty() => Value::External(o.candidates),
                    Some(o) if o.direct => Value::External(vec![o.spelled]),
                    _ => Value::Unknown,
                }
            }
            _ => Value::Unknown,
        }
    }
}

/// `value` with every parameter left in it unknown; with `chain`, inside its calls too, which
/// only `Expand::Chain` folds (elsewhere a call's arguments are as written).
fn unbind(value: Value, chain: bool) -> Value {
    let unbind = |v| unbind(v, chain);
    match value {
        Value::Ref(_) => Value::Unknown,
        Value::Str(parts) => Value::Str(
            parts
                .into_iter()
                .map(|p| match p {
                    Part::Ref(_) => Part::Hole,
                    p => p,
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
            ..call
        }),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::python::Python;

    /// What each site of the `go()` call of `a/W.java` reads, whole (`Expand::Chain`) or by its
    /// arguments (`Expand::Call`): the spellings of the outside name and the text of the path
    /// the chain is built with, or `None` for a value that is no such name or text.
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
    fn a_chain_of_calls_past_the_receiver_depth_resolves_each_call_once() {
        use crate::lang::java::Java;
        // `Outside.start().m().m()..`: each call is made on the one before, whose result no
        // definition answers. Every call reads the result of the one it is made on once, so the
        // chain costs its length, not two to the power of it.
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
    fn a_chain_read_whole_folds_its_calls_and_spells_an_outside_name_where_it_is_written() {
        // The caller of the other file hides `Mode` with a type of its own.
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
        // Read by its arguments, the call has none of its chain's, and climbs no wrapper.
        assert_eq!(chained(hidden, false), [("a/W.java".into(), None, None)]);
    }

    #[test]
    fn an_origin_followed_through_methods_past_the_limit_gives_nothing() {
        use crate::lang::go::Go;
        // `x0, _ := x1.M()`, ... `xn, _ := amqp.Dial(..)`: n methods on a value the library made.
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

    /// A Python file of `n` classes, each but the first deriving from the one before and
    /// calling `self.m()`, which only the first defines; in reverse source order when `reverse`.
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

    /// The file `f0.{ext}`, defining `x` as `define`, and the files `f{i}` up to `f{n - 1}`, each
    /// re-exporting it from the one before as `link(i - 1)` writes.
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

    /// The file defining the target of the call of `callee` in `main`, when it is exact.
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

    /// Whether a call of `x` in `main` is exact at `f0` when `main` imports it from `f{n - 1}`
    /// of `reexports` as `import(n - 1)` writes.
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

        // Far past the limit: a recursion through re-exports that does not stop overflows the
        // stack.
        let far = 5_000;
        // Every other file re-exports by name, the others re-export everything.
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

        // `a` re-exports the `x` of `f0` through `n` files, and `b` defines its own: the module
        // that binds `x` is `a`, whose `x` is unknown past the limit, and never `b`'s.
        let cut = LINK_DEPTH + 1;
        let py_link = |p| format!("from f{p} import x\n");
        let py = |n: usize| {
            let mut sources = reexports("py", n, "def x():\n    pass\n", py_link);
            sources.extend([
                ("a.py".into(), py_link(n - 1)),
                ("b.py".into(), "def x():\n    pass\n".into()),
                // The last star import binds `x`.
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
            // `b` re-exports `f0`'s `x` itself: both `export *` give the same `x` below the
            // limit; past it, `a`'s `x` is unknown, and so is `m`'s, never `b`'s.
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

        // `from pkg import x` names the submodule `pkg.x` only when `pkg` exports no `x`.
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
        // `a` binds `x` to a constant, to something from outside the repository, or to a name
        // it binds twice; `b` defines `def x()`. The last star import binds `x` to `a`'s, which
        // no call reaches exactly, and never to `b`'s.
        for a in ["x = 1\n", "from os import x\n", "x = f()\nx = g()\n"] {
            let b = "def x():\n    pass\n";
            let imported = src(&[
                ("a.py", a),
                ("b.py", b),
                ("main.py", "from b import *\nfrom a import *\nx()\n"),
            ]);
            assert_eq!(exact_in(&Python, &imported, "main.py", "x"), None, "{a}");
            // The same through a module that star-imports both and is imported by name.
            let exported = src(&[
                ("a.py", a),
                ("b.py", b),
                ("m.py", "from b import *\nfrom a import *\n"),
                ("main.py", "from m import x\nx()\n"),
            ]);
            assert_eq!(exact_in(&Python, &exported, "main.py", "x"), None, "{a}");
        }
        // Two `export *` that give `x` differently export neither.
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
        // `m` star-imports `b`, then `a`: the later rebinds `x`, so `from m import x` is `a`'s.
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
        // The ceiling (`ponytail:` in `Resolver::imported`): what `from numpy import *` binds is
        // not known, as `numpy` is no file of the repository, and it is taken to bind nothing,
        // so the import or definition before it keeps `x`. Python may bind `numpy`'s instead.
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
        // The same through a module that star-imports both and is imported by name.
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
        // Each call of `x` walks the star imports after `def x()`: once per module, or the calls
        // take time quadratic in the lines.
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
        // Where stars do not bind, and where they do.
        for (pack, label) in [(&TsJs as &dyn LanguagePack, "ts"), (&Python, "py")] {
            let modules = pack.modules(&[], &BTreeMap::new()).unwrap();
            let start = std::time::Instant::now();
            assert_eq!(Resolver::new(&files, modules, pack).exported().len(), n);
            // Each name was looked up among the file's exports one by one: minutes here.
            let took = start.elapsed();
            assert!(took.as_secs() < 20, "{label}: {took:?}");
        }
    }

    /// An import nothing looks up: its `local` is no name asked for.
    fn filler(name: Imported) -> Import {
        Import {
            local: "z".into(),
            source: String::new(),
            name,
        }
    }

    /// A module written in place, `name`.
    fn module_def(name: &str) -> Def {
        Def {
            name: name.into(),
            kind: Kind::Module,
            range: 0..0,
            params: Vec::new(),
            bases: Vec::new(),
            decorators: Vec::new(),
            return_values: Vec::new(),
            returns: None,
            type_params: Vec::new(),
            result: None,
        }
    }

    /// The rows `main.rs` of `facts` exports, timed.
    fn timed_rows(facts: Facts, label: &str) -> usize {
        let files = BTreeMap::from([("main.rs", &facts)]);
        let start = std::time::Instant::now();
        let rows = resolver(&files).exported().len();
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{label}: {took:?}");
        rows
    }

    #[test]
    fn a_file_of_many_imports_looks_each_name_up_in_linear_time() {
        // Imports of each kind and crates outside the file, none of which a name asked for is.
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
        // Each name nothing binds was looked up among the file's imports and among its
        // externs one by one: minutes here.
        assert_eq!(timed_rows(facts, "imports"), n);
    }

    #[test]
    fn a_file_of_many_imports_finds_each_module_in_linear_time() {
        // `mod m{i}; pub use m{i}::f as a{i};`, then imports nothing looks up: the namespace
        // import of `m{i}` sits at the far end of the walk, and so does `a{i}`'s own.
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
        // Each module was looked up among the file's imports one by one: minutes here.
        assert_eq!(timed_rows(facts, "modules"), n);
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
        // The imports the calls name come first, so that each lookup walks the fillers.
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
        // Each call looked its name up among the file's imports one by one, for the import,
        // for whether a path import exists and for the name it spells: minutes here.
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn a_file_of_many_all_names_takes_each_export_in_linear_time() {
        let (n, listed) = (30_000, 500_000);
        // `m.py` exports `p{i}` and lists none of them in `__all__`; `main.py` star-imports it
        // and exports `x{i}`, and `lib.py` globs it (`pub use`).
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
        // Each name was looked up in `__all__` one by one, to read it, to keep it out of the
        // exports and to glob it: minutes here. Only `main.py` exports: `m.py` keeps its names
        // out, and its glob gives none.
        let took = start.elapsed();
        assert_eq!(rows.len(), n + 1);
        assert!(took.as_secs() < 20, "{took:?}");
    }

    /// A plain parameter `name`.
    fn param(name: &str) -> Param {
        Param {
            name: name.into(),
            binds: Binds::Any,
            default: None,
            fields: Vec::new(),
        }
    }

    /// A top-level function `name` of `params`.
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
        // `f{i}()` at the top level, as a value flows from it.
        let invoked: Vec<Value> = (names.iter().enumerate())
            .map(|(i, name)| {
                Value::Call(crate::lang::Invocation {
                    callee: name.clone(),
                    receiver: None,
                    args: Vec::new(),
                    range: i..i + 1,
                })
            })
            .collect();
        // `def f{i}[T](x=d)` returning one value.
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
        // The functions looked up come after definitions nothing looks up, so that each lookup
        // walks them.
        let defs = (0..fillers).map(|_| module_def("z"));
        let mut facts = Facts {
            defs: defs.chain(names.iter().map(|name| defined(name))).collect(),
            ..Facts::default()
        };
        facts.calls = (names.iter().enumerate())
            .map(|(i, name)| Call {
                arity: None,
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
        // Each function was looked up among the file's definitions one by one, for its return
        // type, its type parameters, its parameter, its return values and the flow through
        // them: minutes here.
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
        // The constants looked up come after those nothing looks up, so that each lookup walks
        // them.
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
        // Each constant was looked up among the file's constants one by one, for its value, its
        // type, its declaration and the flow through it: minutes here.
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn a_function_of_many_parameters_binds_each_name_in_linear_time() {
        let (n, fillers) = (40_000, 400_000);
        let names: Vec<String> = (0..n).map(|i| format!("x{i}")).collect();
        // `def f(z, z, ..., {x0, x1, ...}, x0, x1, ...)`: each name looked up is a field of the
        // destructured parameter after the fillers, and a parameter after that, which the field
        // hides.
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
        // Each name was looked up among the function's parameters and their fields one by one,
        // to bind it and to tell it from a `let mut` around: minutes here.
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn a_package_of_many_files_finds_each_method_and_export_in_linear_time() {
        let n = 30_000;
        let paths: Vec<String> = (0..n).map(|i| format!("p/f{i}.go")).collect();
        let exported: Vec<String> = (0..n).map(|i| format!("e{i}")).collect();
        // File `i` of one package declares the method `T.m{i}` and exports `e{i}`.
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
        // Each name was looked up in every file of the package, for a method declared apart
        // from its type and for an export of the package: minutes here.
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
        // `a` exports `x`, so `from a import *` after `def x()` rebinds `x` to `a`'s for a call
        // in a function, which runs once the module is loaded.
        let def = "def x():\n    pass\n";
        let a = ("a.py", def);
        let main = format!("{def}from a import *\ndef f():\n    x()\n");
        let sources = owned(&[a, ("main.py", &main)]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("a.py"));
        // Code that runs while the module loads (at the top level, in a class body, in the main
        // guard) may run before the star import and reach `main`'s `x`, or after it and reach
        // `a`'s: neither exactly.
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
        // The same through a module that defines `x`, then star-imports `a`.
        let m = format!("{def}from a import *\n");
        let sources = owned(&[a, ("m.py", &m), ("main.py", "from m import x\nx()\n")]);
        let found = exact_in(&Python, &sources, "main.py", "x");
        assert_eq!(found.as_deref(), Some("a.py"));
        // A definition after the star import rebinds the name again, and a star import of a
        // module that exports no `x` leaves the definition.
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

        // As in ES modules: `a` and `b` each export their own `x`, so `m` exports no `x`.
        let m = ("m.ts", "export * from './a';\nexport * from './b';\n");
        let main = ("main.ts", "import { x } from './m';\nx();\n");
        let def = "export function x() {}\n";
        let sources = owned(&[("a.ts", def), ("b.ts", def), m, main]);
        assert_eq!(exact_in(&TsJs, &sources, "main.ts", "x"), None);
        // Both lead to the same definition, which is `m`'s `x`.
        let sources = owned(&[
            ("a.ts", "export * from './c';\n"),
            ("b.ts", "export { x } from './c';\n"),
            ("c.ts", def),
            m,
            main,
        ]);
        let found = exact_in(&TsJs, &sources, "main.ts", "x");
        assert_eq!(found.as_deref(), Some("c.ts"));
        // Only one of them exports `x`.
        let sources = owned(&[("a.ts", def), ("b.ts", "export function y() {}\n"), m, main]);
        let found = exact_in(&TsJs, &sources, "main.ts", "x");
        assert_eq!(found.as_deref(), Some("a.ts"));
    }

    #[test]
    fn a_top_level_binding_after_a_star_import_rebinds_its_name() {
        // `x` is `b`'s, then `a`'s through the star import, then the binding's, which no call
        // reaches exactly: never `a`'s or `b`'s. A function's `global x` may run at any time.
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
            // The same through a module that binds them all and is imported by name.
            let m = format!("from b import x\nfrom a import *\n{binding}");
            let main = ("main.py", "from m import x\nx()\n");
            let sources = owned(&[("a.py", def), ("b.py", def), ("m.py", &m), main]);
            assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{m}");
        }
        // A `global x` written before the star import may run after it.
        let main = format!("{global}from a import *\nx()\n");
        let sources = owned(&[("a.py", def), ("main.py", &main)]);
        assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None);
    }

    #[test]
    fn a_top_level_binding_after_a_definition_hides_it() {
        // The last top-level binding of `x` decides: one after `def x()` (an assignment, an
        // import, any other binding, a function's `global x`) leaves `x` no longer the
        // definition, for a call at the top level, in a function or through an import.
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
        // A star import between them changes nothing, as `global x` is bound last.
        let main = format!("{def}from a import *\n{global}def h():\n    x()\n");
        let found = exact_in(&Python, &owned(&[a, ("main.py", &main)]), "main.py", "x");
        assert_eq!(found, None, "{main}");
        // A definition after the binding is the name's again, and a star import after both
        // rebinds it.
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
        // In a function the last binding of `g` decides too: one after `def g()` or `class g`
        // (an assignment, an import, any other binding, a nested function's `nonlocal g`,
        // which may run at any time) leaves `g` no longer the definition, for a call in the
        // function or in one nested in it.
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
        // `global g` rebinds the module's `g`, not `f`'s, and a definition after the binding is
        // the name's again.
        let global = "    def h():\n        global g\n        g = 1\n";
        for main in [
            format!("def f():\n{def}{global}    g()\n"),
            format!("def f():\n    g = 1\n{def}    g()\n"),
        ] {
            let found = exact_in(&Python, &owned(&[("main.py", &main)]), "main.py", "g");
            assert_eq!(found.as_deref(), Some("main.py"), "{main}");
        }
        // A class body's binding hides no method: `self.m()` reads the definition through the
        // class, as it does under `@staticmethod`, and `m = staticmethod(m)` calls it.
        let class = "class C:\n    def m():\n        pass\n    m = staticmethod(m)\n\
                     \n    def n(self):\n        self.m()\n";
        let found = exact_in(&Python, &owned(&[("main.py", class)]), "main.py", "self.m");
        assert_eq!(found.as_deref(), Some("main.py"));
    }

    #[test]
    fn a_top_level_comprehension_or_lambda_name_hides_no_definition() {
        // A known ceiling: at the top level, a comprehension's variable or a lambda's parameter
        // `h` leaves a call of `h` in it linked to `def h()`, which it may not be. In a
        // function they hide `h` there.
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
        // `def x()` in a function that declares `global x` binds the module's `x` whenever the
        // function runs, and one under `nonlocal g` the enclosing function's `g`.
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
        // A `def` or `class` under `if`, `try` or a loop may not run: when the name was bound
        // before it, the name may still be that binding, at the top level or in a function.
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
        // With no binding before it, the definition is the name's.
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
        // `pkg`'s literal `__all__` grows by literal names at its top level: a star import of it
        // after `def x()` rebinds `x` to `pkg`'s once `x` is added.
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
        // Changed some other way, it may or may not list `x`: a call reaches neither exactly.
        for changed in [
            "__all__ = ['y']\n__all__ += names\n",
            "__all__ = ['y']\n__all__.extend(names)\n",
            "__all__ = ['y']\ndef g():\n    __all__.append('x')\n",
            "__all__ = ['x', 'y']\n__all__.remove('x')\n",
            "__all__ = ['x', 'y']\n__all__ -= ['x']\n",
            // A change that may not run, under `if`, `try`, a loop or in a lambda.
            "__all__ = ['y']\nif WIN:\n    __all__ += ['x']\n",
            "__all__ = ['y']\ntry:\n    __all__.append('x')\nexcept E:\n    pass\n",
            "__all__ = ['y']\nfor _ in xs:\n    __all__.extend(['x'])\n",
            "__all__ = ['y']\nadd = lambda: __all__.append('x')\n",
            "if WIN:\n    __all__ = ['x', 'y']\nelse:\n    __all__ = ['y']\n",
            // An item set or deleted, or a function's `global __all__`.
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
        // After `del x`, `x` is bound to nothing: no call reaches the definition or import it
        // was, at the top level, in a function or through an import.
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
        // Nor the class of the instance it held, nor a definition deleted in a function, nor a
        // star import's that it deletes.
        let instance = "class A:\n    def m(self):\n        pass\nx = A()\ndel x\nx.m()\n";
        let sources = owned(&[("main.py", instance)]);
        assert_eq!(exact_in(&Python, &sources, "main.py", "x.m"), None);
        let nested = "def f():\n    def x():\n        pass\n    del x\n    x()\n";
        let sources = owned(&[("main.py", nested)]);
        assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None);
        let star = format!("{def}from b import *\ndel x\ndef f():\n    x()\n");
        let sources = owned(&[b, ("main.py", &star)]);
        assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None);
        // Bound again after `del`, by a definition or a star import, the name is that binding's.
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
        // `pkg`'s `__all__` may or may not list `x`: a star import of it rebinds `x` to `pkg`'s,
        // or leaves the binding before it, so a call reaches neither exactly.
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
            // The same through a module that binds both and is imported by name.
            let m = format!("{before}from pkg import *\n");
            let main = ("main.py", "from m import x\nx()\n");
            let sources = owned(&[pkg, b, core, ("m.py", &m), main]);
            assert_eq!(exact_in(&Python, &sources, "main.py", "x"), None, "{m}");
        }
        // With nothing bound before it, `x` is `pkg`'s or no name at all.
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

        // `utils` imports `helper` from the package being initialized, which has `core`'s by
        // then: its star import gives nothing new, and `core`'s is the package's.
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
        // As in ES modules, where the cycle resolves to null: `c`'s `x` is `index`'s.
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

        // `a` reaches `index` back through a cycle, and `p` and `q` give `x` differently: `a`
        // exports no `x` it is sure of, so neither does `index`, `c`'s notwithstanding.
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

    /// The URL each site the expansion of the `fetch` in `src` reaches gives it, as text; `None`
    /// for one with a part not known.
    fn fetched(src: &str) -> Vec<Option<String>> {
        use crate::lang::ts::TsJs;
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let code = Code::new(&resolver, &files, crate::lang::pack(0));
        let n = (facts.calls.iter())
            .position(|c| c.callee == "fetch")
            .unwrap();
        // A parameter makes the function around the call a wrapper, read at its callers.
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
    fn a_value_folded_deeper_than_the_limit_is_unknown() {
        let chain = |n: usize| {
            let consts: String = (0..n)
                .map(|i| format!("const a{} = a{i};\n", i + 1))
                .collect();
            fetched(&format!("const a0 = '/a';\n{consts}fetch(a{n});\n"))
        };
        // The argument, the `n` constants and the literal they end at: `n + 2` values folded
        // within one another.
        assert_eq!(chain(EVAL_DEPTH - 2), [Some("/a".to_string())]);
        assert_eq!(chain(EVAL_DEPTH - 1), [None]);
        // Far past it, the fold still ends before the stack does.
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
        // Forty doublings would fold 2^40 parts.
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

        // The frames climbed: `WRAPPER_DEPTH` calls up the wrappers, from the call of `w0` to
        // the site, which is where the climb stops when it is not the call written at the top.
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
        // Each wrapper calls the one below twice: 2^n ways up from the `fetch`.
        let source = |n: usize| {
            let wrappers: String = (0..n)
                .map(|i| format!("function w{}(u) {{ w{i}(u); return w{i}(u); }}\n", i + 1))
                .collect();
            format!("function w0(u) {{ return fetch(u); }}\n{wrappers}w{n}('/a');\n")
        };
        let lattice = |n: usize| fetched(&source(n));
        assert_eq!(lattice(3), vec![Some("/a".to_string()); 8]);
        // Each way up is its own site, whichever of the two calls of a level it climbed.
        let way = "w0(u) > w1(u) > w2(u) > w3('/a')";
        assert_eq!(climbed(&source(3)), vec![way; 8]);
        // The eight ways are eight different climbs: the calls written alike are not the same.
        let at = climbed_at(&source(3));
        assert_eq!(at.iter().collect::<BTreeSet<_>>().len(), 8, "{at:?}");
        let ways = lattice(40);
        assert!(
            !ways.is_empty() && ways.len() < EVAL_BUDGET,
            "{}",
            ways.len()
        );
    }

    /// The callees of the calls `p` may hold at the top of the Python file `src`.
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
        // The constant `p` and the `n + 1` functions that hand on what `make()` made.
        assert_eq!(returned(EVAL_DEPTH - 2), ["make"]);
        assert_eq!(returned(EVAL_DEPTH - 1), Vec::<String>::new());
    }

    #[test]
    fn a_lattice_of_returns_is_traced_within_the_budget() {
        // Each function returns the one below in two places: 2^n ways down to `make()`.
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
        // Far past the limit: a recursion down the bases that does not stop overflows the
        // stack, and families kept whole take memory quadratic in the chain.
        let n = 5_000;
        for reverse in [false, true] {
            let facts = Python.parse("a.py", chain(n, reverse).as_bytes()).unwrap();
            let files = BTreeMap::from([("a.py", &facts)]);
            let resolver = Resolver::new(&files, Box::new(|_, _| Vec::new()), &Python);
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

    /// The Rust files parsed: `(name, source)` in, facts out.
    fn parsed(files: &[(&'static str, &str)]) -> Vec<(&'static str, Facts)> {
        use crate::lang::rust::Rust;
        let parse =
            |(name, src): &(&'static str, &str)| (*name, Rust.parse(name, src.as_bytes()).unwrap());
        files.iter().map(parse).collect()
    }

    /// A Rust resolver over files named after their modules: `crate` is `main.rs`, `mod x` is
    /// `x.rs`, `self` the file itself, and `super` the file one level up the chain `c.rs`,
    /// `b.rs`, `a.rs`, `main.rs`. A crate's name is no file.
    fn resolver<'a>(files: &'a BTreeMap<&'a str, &'a Facts>) -> Resolver<'a> {
        use crate::lang::rust::Rust;
        let modules: Modules = Box::new(|from, specifier| match (specifier, from) {
            ("crate", _) => vec!["main.rs".into()],
            ("self", from) => vec![from.into()],
            ("super", "c.rs") => vec!["b.rs".into()],
            ("super", "b.rs") => vec!["a.rs".into()],
            ("super", _) => vec!["main.rs".into()],
            (s, _) => (s.strip_prefix("mod ").into_iter())
                .map(|m| format!("{m}.rs"))
                .collect(),
        });
        Resolver::new(files, modules, &Rust)
    }

    /// What each call resolves to, by `file scope callee`.
    fn calls(files: &[(&'static str, &str)]) -> BTreeMap<String, String> {
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
        // Far past the limit: a walk that does not stop overflows the stack.
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
        // `x()` in `main.rs`, where the latest glob reaches `x` through `n` modules, each
        // re-exporting it from the next by a glob or by name, and an earlier glob gives `x` too.
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
            assert_eq!(x(3, by_name), "exact m3.rs::x", "by name: {by_name}");
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
        // `mod x; pub use x::x;`: the item is what a call reaches.
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
        // Inside `m`, `time` is a crate outside the file.
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
        // A private `use` is the module's own.
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
        // A result is followed inside `instance`, so `RECEIVER_DEPTH` receivers being resolved
        // around it leave it unknown, and the cut is recorded.
        let parsed = parsed(&[(
            "main.rs",
            "struct S;\nimpl S { fn new() -> Self { S } fn go(&self) {} }\n\
             fn f() { let s = S::new(); s.go(); }\n",
        )]);
        let map: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let resolver = resolver(&map);
        let s = |depth: usize| {
            let mut work = Work {
                busy: (0..depth)
                    .map(|_| Busy::Local("main.rs", "f", "y"))
                    .collect(),
                ..Work::default()
            };
            let found = resolver.instance(Binding::Local("main.rs", "f", "s"), &mut work);
            (
                matches!(found, Binding::Instance(t) if t.name == "S"),
                work.cut,
            )
        };
        assert_eq!(s(0), (true, false));
        assert_eq!(s(RECEIVER_DEPTH - 1), (true, false));
        assert_eq!(s(RECEIVER_DEPTH), (false, true));
    }

    #[test]
    fn a_class_annotated_deeper_than_the_limit_is_unknown() {
        // Whether `x0.m()` is exact when `n` parameters of `f` each take their class from the
        // next one's (`x0: x1.C`), quoted or not, and the last is `K`, whose member `C` is a
        // class that derives from `K` and so has it as a member too.
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
            let resolver = Resolver::new(&files, Box::new(|_, _| Vec::new()), &Python);
            let call = facts.calls.iter().find(|c| c.callee == "x0.m").unwrap();
            matches!(resolver.call("a.py", call), Resolution::Exact(t) if t.name == "K.C.m")
        };

        // Far past the limit: a recursion through receivers that does not stop overflows the
        // stack.
        let far = 5_000;
        for quoted in [false, true] {
            assert!(exact(RECEIVER_DEPTH, quoted), "quoted: {quoted}");
            assert!(!exact(RECEIVER_DEPTH + 1, quoted), "quoted: {quoted}");
            assert!(!exact(far, quoted), "quoted: {quoted}");
        }
    }

    #[test]
    fn a_family_does_not_depend_on_the_bases_of_the_walk_that_reaches_it() {
        // `K` (`C127`) has the longest family the limit keeps, `C0` the last of it. `Z` derives
        // from `K.D`, the class `D` that `C0` defines, found through `K`'s family: linearized on
        // the way from `Z`, it is one class past the limit; from `K` itself it is not.
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
            let resolver = Resolver::new(&files, Box::new(|_, _| Vec::new()), &Python);
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
        // `K` derives from `L.D`, named through two receivers (`y = w.D()`). `Z` derives from
        // `K.C`, named through `RECEIVER_DEPTH` receivers (`x0 = x1.C()`, ..., `x63 = K()`), so
        // `K`'s family, linearized on the way from `Z`, is two receivers past the limit; from
        // `K` itself it is not.
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
            let resolver = Resolver::new(&files, Box::new(|_, _| Vec::new()), &Python);
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

    // The rules that decide what an origin walk follows (spec #204), pinned at the `Code`
    // seam. Each test builds a `Code` over parsed files, asks one question and checks the
    // answer as one line per origin. The render helpers below own that line; a test asks them,
    // never the walks behind `Code::origins` and `Code::outside`.

    /// A `Code` of `sources` (path, text) parsed by `pack`, handed to `ask`.
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

    /// The source text of the call at `index` of `path`.
    fn written(code: &Code, sources: &[(&str, &str)], path: &str, index: usize) -> String {
        let src = sources.iter().find(|(p, _)| *p == path).unwrap().1;
        let call = &code.files()[path].calls[index];
        src[call.range.clone()].to_string()
    }

    /// What `origin_lines` shows of each route.
    #[derive(Clone, Copy)]
    enum Shown {
        /// Nothing: one line per origin.
        Nothing,
        /// The calls climbed: one line per origin and climb.
        Through,
        /// Everything (`route_line`): one line per origin and route.
        All,
    }

    /// One line per origin and route, as far as `shown` tells routes apart: its file and callee,
    /// `= name` when it names something outside the repository (as the file's imports spell
    /// it), then what `route_line` renders of what is shown of the route. Two routes that differ
    /// only in what is not shown give one line; two that differ in what is shown give two, even
    /// when they render alike (two calls written alike at different sites).
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

    /// ` via call` for each call of the repository a route climbed through, outermost first.
    fn via(code: &Code, sources: &[(&str, &str)], route: &Route) -> String {
        let calls = route.through.iter();
        calls
            .map(|&(at, index)| format!(" via {}", written(code, sources, at, index)))
            .collect()
    }

    /// Everything a route carries: ` steps=a,b` (constructor first), ` held=file:name`, then `via`.
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

    /// What the name `name` written in `scope` of `path` may hold: the `Values` origins.
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

    /// `held_by` with every route in full (`route_line`).
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

    /// The scope and the value as written, the first argument of the call of `probe(..)` in
    /// `path`.
    fn probed<'a>(code: &Code<'a>, path: &'a str) -> (&'a str, &'a Value) {
        let calls = code.files()[path].calls.iter();
        let call = calls.into_iter().find(|c| c.callee == "probe").unwrap();
        (call.scope.as_str(), &call.args[0])
    }

    /// The table the probes ask with, test-local as the kinds' tables are theirs: axum's builder
    /// methods and actix-web's `service`; no other method hands an object back.
    fn framework_table(origin: &str, method: &str) -> bool {
        (origin.starts_with("axum.") && ["route", "nest", "layer"].contains(&method))
            || (origin.starts_with("actix_web.") && method == "service")
    }
    const TABLE: Passes = framework_table;

    /// The `Values` origins of the value written as the argument of `probe(..)` in `path`, asked
    /// with the table `passes`, as `origin_lines` shows them.
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

    /// `probe_passing` with the test-local table (`TABLE`).
    fn probe_lines(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
        shown: Shown,
    ) -> Vec<String> {
        probe_passing(pack, sources, path, shown, TABLE)
    }

    /// What the value written as the argument of `probe(..)` in `path` may hold.
    fn held_by_value(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
    ) -> Vec<String> {
        probe_lines(pack, sources, path, Shown::Nothing)
    }

    /// The same with the calls climbed, one line per distinct climb.
    fn held_through(
        pack: &'static dyn LanguagePack,
        sources: &[(&'static str, &str)],
        path: &'static str,
    ) -> Vec<String> {
        probe_lines(pack, sources, path, Shown::Through)
    }

    /// The same with every route in full (`route_line`).
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

    /// One line per callee written in `path`, read by `Code::outside`: what the file's imports
    /// spell it as, the same read as a path, whether its head is the import itself, and the
    /// package and name the import binds (`imported`). `-` for a reading that gives nothing.
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

    /// `Code::outside` of the callee `callee` of `path` in full: `imported` (package and name),
    /// the spelling and `direct`.
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

    /// The `Constants` origin `chain` of `scope` in `a.ts` has: the package, how the code would
    /// write what it made (rebuilt from the form, the name and the steps), and the constant the
    /// name refers to (`held file:name`).
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

    /// What the Go source `src` declares `name` of `scope` to be outside the repository, the
    /// `Declarations` origin: spelled with its steps, and the written type (the form `Type` with
    /// no steps).
    fn declared(src: &str, scope: &'static str, name: &'static str) -> String {
        let (origin, written, _) = declared_form(src, scope, name);
        format!("origin={origin} type={written}")
    }

    /// `declared`, and the form of the origin (`-` for none).
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

    /// The wrapper calls `expand` climbs to each site of the `fetch` of `src`, as written,
    /// from the call in the wrapper to the site, one line per site.
    fn climbed(src: &str) -> Vec<String> {
        climbed_with(src, &|text, _| text)
    }

    /// `climbed`, each call followed by its position among the calls of the file, which tells
    /// two calls written alike apart.
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

    /// The sites `expand` finds for the constructed constant `held` of `a.ts`, on
    /// behalf of the call `at` (a callee of `a.ts`), each with the first argument of the `new`
    /// as read there.
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

    // Values.

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
        // `s` is an `S`, so `s.use` is exact.
        let exact = format!("{def}\ndef go():\n    s = S()\n    s.use(Router())\n");
        // `thing` is of no class Meada can tell, so `thing.use` is only a name match for the
        // method `use` (`Inferred`), which may be called on any object.
        let matched = "\ndef other(thing):\n    thing.use(Other())\n";
        let both = format!("{exact}{matched}");
        let param = |src: &str| held_by(py(), &[("a.py", src)], "a.py", "S.use", "r");
        assert_eq!(param(&exact), ["a.py Router"]);
        assert_eq!(param(&both), ["a.py Router"]);
        let only = format!("{def}{matched}");
        assert_eq!(param(&only), Vec::<String>::new());
    }

    #[test]
    fn a_parameter_bound_by_a_frame_takes_the_argument_of_that_call_alone() {
        let src = "def wrap(r):\n    return r\n\na = wrap(Alpha())\nb = wrap(Beta())\n";
        let sources = [("a.py", src)];
        // Two constructors through one factory: each is the factory's argument of its own call.
        assert_eq!(held_by(py(), &sources, "a.py", "", "a"), ["a.py Alpha"]);
        assert_eq!(held_by(py(), &sources, "a.py", "", "b"), ["a.py Beta"]);
        // No call binds the parameter when it is asked for itself: every exact caller does.
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
        // One call made both: the origin is one, and the climbed calls tell the clients apart.
        assert_eq!(held_by_value(py(), &sources, "a.py"), ["a.py Client"]);
        assert_eq!(
            held_through(py(), &sources, "a.py"),
            ["a.py Client via mk('x')", "a.py Client via mk('y')"]
        );
        // Two calls written alike at two sites are two climbs.
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
        // Returned directly, the same argument is the origin; the call that binds it is no
        // climb: the argument is read where the caller wrote it.
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

    // Values in a fluent pack (Rust builders).

    #[test]
    fn two_tables_over_one_file_each_keep_only_their_own_chains() {
        // One function returns an axum chain or an actix-web one. Each question carries one
        // library's table, test-local, and keeps only that library's chain.
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

    /// A `main.rs` with `defs` at the top and `body` in `main`, which ends by probing `r`.
    fn axum_main(defs: &str, body: &str) -> String {
        format!(
            "use axum::Router;\nuse actix_web::App;\n{defs}\nfn main() {{\n{body}\nprobe(r);\n}}\n"
        )
    }

    /// What `r` of `axum_main(defs, body)` may hold: `held_by_value`, then `held_through`.
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
        // The repository's `route` is the only method of that name it defines.
        let defs = "struct Mine;\nimpl Mine { fn route(&self) -> Mine { Mine } }\n";
        // `build` is no function Meada knows, so `route` on its result is a name match for the
        // repository's. The chain starts at no outside constructor, and the `passes` filter
        // empties it: an origin with no outside name does not survive a step above it.
        let (plain, _) = built(defs, "let r = build().route(\"/a\");");
        assert_eq!(plain, Vec::<String>::new());
        let (plain, _) = built(defs, "let r = build().layer(l).route(\"/a\");");
        assert_eq!(plain, Vec::<String>::new());
        // The method of an outside constructor is the library's, whatever the repository names.
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
        // One function returns an axum router or an actix app; `layer` is axum's alone. The
        // filter reads each origin's own library, not which return it came from.
        let defs =
            "fn make(f: bool) -> Router {\n    if f { return Router::new(); }\n    App::new()\n}\n";
        let (plain, _) = built(defs, "let r = make(true).layer(l);");
        assert_eq!(plain, [ROUTER]);
        let (plain, _) = built(defs, "let r = make(true).frobnicate();");
        assert_eq!(plain, Vec::<String>::new());
        let (plain, _) = built(defs, "let r = make(true);");
        assert_eq!(plain, [ROUTER, "main.rs App.new = actix_web.App.new"]);
    }

    // The name reading.

    #[test]
    fn a_path_written_callee_is_read_by_path_and_not_by_a_local_function_that_spells_its_head() {
        let src = "fn rocket() {}\nfn main() { rocket::build(); }\n";
        // Read as a value (`external=`), `rocket` is the function and so no crate; read as a path,
        // the head is the crate.
        assert_eq!(
            read_names(rust(), &[("main.rs", src)], "main.rs"),
            ["rocket.build: external=- by_path=rocket.build direct=false import=-"]
        );
    }

    #[test]
    fn a_use_imported_module_function_is_direct() {
        // An associated function of a type the `use` names.
        let src = "use reqwest::Client;\nfn main() { Client::new(); }\n";
        assert_eq!(
            read_names(rust(), &[("main.rs", src)], "main.rs"),
            [
                "Client.new: external=reqwest.Client.new by_path=reqwest.Client.new direct=true \
              import=reqwest Client.new"
            ]
        );
        // A function of a module the `use` names, and a function the `use` names itself.
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
                // Read as the local's own spelling: it is outside, and no import names its head.
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
        // A local of a class the repository defines is outside nothing.
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
        // The package and the name are the ones the repository file imports; the spelling is the
        // one the importing file gives it.
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

    // Constants.

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
        // A member after the constant, not called, is no step of what was made: no origin (the
        // old spelling wrote it `new Kafka().producer().send`; no form reads it).
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
        // Inside `f` the name is its parameter: no origin. Outside, the constant's.
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
        // A star import names no package member to spell; a named re-export does.
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

    // Declarations.

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
        // the repository is outside nothing.
        let local = format!(
            "{head}type T struct{{}}\nfunc mk() *T {{ return nil }}\nfunc f() {{\n\tw := mk()\n}}\n"
        );
        assert_eq!(declared(&local, "f", "w"), "origin=- type=-");
    }

    // What only the one origin question carries (spec #204).

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
        // Down a function of the repository: its chain's methods come first.
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
        // The items of a list asked as a value are each their own constant.
        let list = [("a.py", "a = Alpha()\nb = Beta()\nprobe([a, b])\n")];
        let found = probe_routes(py(), &list, "a.py");
        assert_eq!(found, ["a.py Alpha held=a.py:a", "a.py Beta held=a.py:b"]);
        // The same inputs give the same origins in the same order.
        assert_eq!(probe_routes(py(), &list, "a.py"), found);
        // Asked by name, the list is the constant.
        let named = [("a.py", "a = Alpha()\nb = Beta()\np = [b, a]\n")];
        assert_eq!(
            routes_of(py(), &named, "a.py", "", "p"),
            ["a.py Alpha held=a.py:p", "a.py Beta held=a.py:p"]
        );
        // A parameter is no constant, whatever its caller passes.
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
        // A method of a local made outside: no import names its head.
        assert_eq!(
            outside_of(go(), &go_files, "a.go", "r.GET", false).unwrap(),
            "-; r.GET; direct=false"
        );
        // A path-written callee whose head a local function spells is outside only by path.
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
        // A method of a value a package's function made: the form of that call, with the step.
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
        // An uncalled member after a constant head.
        none("const p = kafka.producer;\n", "p");
        none("const p = kafka.a.producer();\n", "p");
        // A `new` after steps, or after a call.
        none("const p = new kafka.producer();\n", "p");
        none("const p = kafka.producer();\nconst q = new p();\n", "q");
        // A `new` of an expression no name spells (`(kafka.producer())`) has no head to trace.
        none("const p = new (kafka.producer())();\n", "p");
        // What a call made, called again.
        none("const p = kafka.producer();\nconst q = p();\n", "q");
        // A member of the import itself is part of its name.
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

    // Expansion.

    #[test]
    fn a_constructed_constant_is_read_at_the_wrappers_call_site_and_in_place() {
        let head = "import { Queue } from 'bullmq';\n";
        // The name is a literal where the constant is made: read in place, the site is the `add`.
        let in_place = format!("{head}const q = new Queue('jobs');\nq.add('x', {{}});\n");
        assert_eq!(
            constructed_sites(&in_place, "q", "q.add"),
            ["q.add -> jobs"]
        );
        // The name is a parameter of the function that makes the constant: each call of that
        // wrapper is a site, read with its own argument.
        let wrapper = "function send(name) { const q = new Queue(name); q.add('x', {}); }\n\
                       send('jobs');\nsend('mail');\n";
        assert_eq!(
            constructed_sites(&format!("{head}{wrapper}"), "send.q", "q.add"),
            ["send -> jobs", "send -> mail"]
        );
    }

    /// What each call of one Java file resolves to, by `scope callee`: the level and target.
    fn java_calls(src: &str) -> BTreeMap<String, String> {
        use crate::lang::java::Java;
        let facts = Java.parse("A.java", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("A.java", &facts)]);
        let resolver = Resolver::new(&files, Box::new(|_, _| Vec::new()), &Java);
        let mut out = BTreeMap::new();
        for call in &facts.calls {
            let found = match resolver.call("A.java", call) {
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
        // `Remote` may declare `g(String)`: the family's own match is a name match.
        assert_eq!(calls["p.Own.run() this.g"], "inferred p.Own.g(int)");
        assert_eq!(calls["p.Own.run() g"], "inferred p.Own.g(int)");
        // A bare call name-matches as `this.` does.
        let both = "ambiguous p.Past.f(int) p.Port.f(String)";
        assert_eq!(calls["p.Past.run() this.f"], both);
        assert_eq!(calls["p.Past.run() f"], both);
        // Java calls `ArrayList.add(E)` here, never an exact `Names.add(String)`.
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
        // The typed receiver's family declares no `save`: `OrderService.save` is no candidate.
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
        // `Remote` may declare `repo`: the outer field is not read through it.
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
        // A receiver read sees the field, a call the method.
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
        // Python declares no overload families: the first class of the C3 order still wins.
        let src = "class A:\n    def f(self): pass\nclass B(A):\n    def f(self): pass\n\
                   class C(B, A):\n    def go(self): self.f()\n";
        let facts = Python.parse("a.py", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.py", &facts)]);
        let resolver = Resolver::new(&files, Box::new(|_, _| Vec::new()), &Python);
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
