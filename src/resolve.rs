//! The shared resolution (ADR 0007): connects each call a language pack found to the
//! definitions it reaches, with Meada's own scope and import resolution and no types.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::lang::{
    Binds, Call, Class, Def, Export, Facts, Imported, Kind, LanguagePack, Modules, Param, Part,
    Step, Ty, Value,
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
    /// Several definitions fit, sorted.
    Ambiguous(Vec<Target<'a>>),
    Unresolved,
}

/// The scope a qualified name is written in: `outer` for `outer.inner`, the empty top-level
/// scope for a top-level name.
pub fn scope_of(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(scope, _)| scope)
}

/// What a crate outside the repository is imported as.
static NAMESPACE: Imported = Imported::Namespace;

/// An `exports` row: the name, the file and the definition in it.
type Row<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>);

/// Whether the export names `name`.
fn exports_name(export: &Export, name: &str) -> bool {
    matches!(export, Export::Local { exported, .. } | Export::From { exported, .. } if exported == name)
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

/// Whether `local` is `name` bound inside `scope` (`scope.name`, or `name` at the top level).
fn bound_in(local: &str, scope: &str, name: &str) -> bool {
    match scope {
        "" => local == name,
        scope => local.strip_prefix(scope).and_then(|l| l.strip_prefix('.')) == Some(name),
    }
}

/// What a name or a member chain refers to.
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

/// A call site whose single target is a definition. `by_name` when only a name match found it
/// (`data.get(...)`, or a bare name no import binds): it may reach any object or function.
#[derive(Clone, Copy)]
struct Caller<'a> {
    path: &'a str,
    index: usize,
    by_name: bool,
}

/// What one resolution has met, so that imports and re-exports forming a cycle give nothing:
/// the `(file, exported name)` pairs, the glob imports asked for a name (their file, place in
/// its imports and the name), and how many aliases and globs deep the walk is.
#[derive(Default)]
struct Seen<'a> {
    names: HashSet<(&'a str, &'a str)>,
    globs: HashSet<(&'a str, usize, &'a str)>,
    links: usize,
}

impl<'a> Seen<'a> {
    fn new() -> Self {
        Self::default()
    }

    fn insert(&mut self, key: (&'a str, &'a str)) -> bool {
        self.names.insert(key)
    }

    fn contains(&self, key: &(&str, &str)) -> bool {
        self.names.contains(key)
    }
}

/// How many aliases and globs one resolution follows one into the other. A longer chain of
/// `use` gives nothing.
// ponytail: real code chains a handful of `use`s and `pub use`s; a crafted chain of thousands
// must not overflow the stack. A pack with longer chains raises the limit with the stack.
const LINK_DEPTH: usize = 128;

/// How many calls whose result a receiver takes (`Class::Returned`) one resolution follows into
/// one another. A deeper chain gives nothing.
// ponytail: a result is the class its function writes, so real chains are one or two deep; the
// limit keeps a crafted repository from recursing through thousands of functions.
const RETURN_DEPTH: usize = 64;

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
    /// A `use` path: its head among types and modules, its last member of any kind.
    Alias,
}

impl Reading {
    fn types(self) -> bool {
        matches!(self, Reading::Path | Reading::Alias)
    }

    fn value(self) -> bool {
        matches!(self, Reading::Value | Reading::Path)
    }
}

/// A local whose class, or a class whose family, is being resolved: one met again while
/// resolving itself (`a: b.C` and `b: a.D`, `class A(B)` and `class B(A)`) resolves to
/// nothing.
#[derive(PartialEq)]
enum Busy<'a> {
    Local(&'a str, &'a str, &'a str),
    Family(Target<'a>),
}

/// What resolving one name carries along: what is being resolved, and the families
/// linearized so far (while `Resolver::new` linearizes them all).
#[derive(Default)]
struct Work<'a> {
    busy: Vec<Busy<'a>>,
    families: HashMap<Target<'a>, Family<'a>>,
    /// How many `returned` are being resolved.
    returns: usize,
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
/// families being resolved: no family is kept on the way up, since the cut depends on where
/// the walk started.
struct Cut;

/// The files of one language pack, resolved together.
pub struct Resolver<'a> {
    files: &'a BTreeMap<&'a str, &'a Facts>,
    defs: HashMap<&'a str, HashMap<&'a str, &'a Def>>,
    consts: HashMap<&'a str, HashSet<&'a str>>,
    /// The other names bindings make (`Facts::locals`): each hides an outer name.
    locals: HashMap<&'a str, HashSet<&'a str>>,
    /// Each file's `Facts::receivers`, by scope and name.
    receivers: HashMap<&'a str, HashMap<(&'a str, &'a str), &'a Class>>,
    /// Every class's family, linearized once.
    families: HashMap<Target<'a>, Family<'a>>,
    /// The pack's keyword for the instance of the enclosing class (`LanguagePack::receiver`).
    receiver: Option<&'static str>,
    /// The pack's keyword for the base of the enclosing class (`LanguagePack::base_receiver`).
    base_receiver: Option<&'static str>,
    /// The pack's names bound everywhere (`LanguagePack::globals`).
    globals: &'static [&'static str],
    /// The call sites whose single target (exact or inferred) is each definition, sorted.
    callers: HashMap<Target<'a>, Vec<Caller<'a>>>,
    /// Name inference candidates, sorted: methods by their last name (`true`), top-level
    /// functions and classes, and those of modules written in place, by their name (`false`).
    by_name: HashMap<(bool, &'a str), Vec<Target<'a>>>,
    modules: Modules,
    /// The blocks that add members to each class (`Facts::impls`), the inherent ones first.
    impls: HashMap<Target<'a>, Vec<Block<'a>>>,
    /// The class each block adds members to, by its file and scope: what `Self` names in it.
    blocks: HashMap<(&'a str, &'a str), Target<'a>>,
    /// The files whose exports other packages can import, when the pack says which
    /// (`LanguagePack::published`).
    published: Option<BTreeSet<String>>,
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
    pub fn new(
        files: &'a BTreeMap<&'a str, &'a Facts>,
        modules: Modules,
        language: &dyn LanguagePack,
    ) -> Self {
        let defs: HashMap<&str, HashMap<&str, &Def>> = files
            .iter()
            .map(|(path, facts)| {
                let names = facts.defs.iter().map(|d| (d.name.as_str(), d)).collect();
                (*path, names)
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
        }
        by_name.values_mut().for_each(|targets| targets.sort());
        let mut resolver = Self {
            files,
            defs,
            consts,
            locals,
            receivers,
            families: HashMap::new(),
            receiver: language.receiver(),
            base_receiver: language.base_receiver(),
            globals: language.globals(),
            callers: HashMap::new(),
            by_name,
            modules,
            impls: HashMap::new(),
            blocks: HashMap::new(),
            published: None,
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
        let mut impls: HashMap<Target, Vec<Block>> = HashMap::new();
        let mut blocks = HashMap::new();
        for (path, facts) in files {
            for block in &facts.impls {
                let class = |name: &'a str| match resolver.path_binding(path, &block.within, name) {
                    Binding::Def(t) if resolver.is_class(t) => Some(t),
                    _ => None,
                };
                let Some(ty) = class(&block.ty) else {
                    continue;
                };
                blocks.insert((*path, block.scope.as_str()), ty);
                impls.entry(ty).or_default().push(Block {
                    path,
                    scope: &block.scope,
                    of_trait: block.of.is_some(),
                    of: block.of.as_deref().and_then(class),
                });
            }
        }
        impls
            .values_mut()
            .for_each(|b| b.sort_by_key(|b| b.of_trait));
        (resolver.impls, resolver.blocks) = (impls, blocks);
        let mut callers: HashMap<Target, Vec<Caller>> = HashMap::new();
        for (path, facts) in files {
            for (index, call) in facts.calls.iter().enumerate() {
                let (target, by_name) = match resolver.call(path, call) {
                    Resolution::Exact(t) => (t, false),
                    Resolution::Inferred(t) => (t, true),
                    _ => continue,
                };
                let caller = Caller {
                    path,
                    index,
                    by_name,
                };
                callers.entry(target).or_default().push(caller);
            }
        }
        resolver.callers = callers;
        resolver
    }

    /// Limits `exported` to these files (`LanguagePack::published`); `None` keeps every file.
    pub fn publishing(mut self, published: Option<BTreeSet<String>>) -> Self {
        self.published = published;
        self
    }

    pub fn call(&self, path: &'a str, call: &'a Call) -> Resolution<'a> {
        let binding = match call.by_path {
            true => self.path_binding(path, &call.scope, &call.callee),
            false => self.binding(path, &call.scope, &call.callee),
        };
        match binding {
            Binding::Def(t) => Resolution::Exact(t),
            Binding::Several(all) => Resolution::Ambiguous(all),
            Binding::External(..) => Resolution::Unresolved,
            Binding::Instance(_)
            | Binding::Module(_)
            | Binding::Const(..)
            | Binding::Param(..)
            | Binding::Local(..)
            | Binding::Hidden
            | Binding::Unknown => self.infer(&call.callee, call.by_path),
        }
    }

    /// `resolve` of one name on its own, with nothing seen or being resolved yet.
    fn binding(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut Seen::new(), &mut Work::default());
        self.resolve_as(path, scope, callee, seen, work, Reading::Value)
    }

    /// `binding` of a path (Rust's `store::Store::new`, `Call::by_path`): its head names a
    /// module or a type, which a local or a parameter of the same name does not hide.
    fn path_binding(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut Seen::new(), &mut Work::default());
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
        let (mut binding, members) = self.dotted(path, callee, seen).unwrap_or_else(|| {
            let (head, members) = callee.split_once('.').unwrap_or((callee, ""));
            let binding = match head {
                _ if Some(head) == self.receiver => self.class(path, scope),
                _ if Some(head) == self.base_receiver => Binding::Unknown,
                _ if head.starts_with('(') => Binding::Unknown,
                _ => self.lexical_as(path, scope, head, seen, reading),
            };
            (binding, members)
        });
        let members: Vec<&str> = members.split('.').filter(|m| !m.is_empty()).collect();
        for (n, member) in members.iter().enumerate() {
            let value = reading.value() && n + 1 == members.len();
            binding = match self.instance(binding, work) {
                Binding::Def(t) | Binding::Instance(t) if self.is_class(t) => {
                    self.member(t, member, work)
                }
                Binding::Def(t) => match self.def(t.path, &format!("{}.{member}", t.name)) {
                    Binding::Unknown if self.defs[t.path][t.name].kind == Kind::Module => {
                        self.module_member(t, member, value, seen)
                    }
                    found => found,
                },
                Binding::Module(m) => self.export_as(m, member, seen, value),
                Binding::Several(_) => Binding::Unknown,
                other => other,
            };
        }
        binding
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
        let exports = &self.files[path].exports;
        if let Some(Export::Local { exported, .. } | Export::From { exported, .. }) =
            exports.iter().find(|e| exports_name(e, &dotted))
        {
            return self.export_as(path, exported, seen, value);
        }
        let mut imports = self.files[path].imports.iter().rev();
        match imports.find(|i| i.local == module.name && matches!(i.name, Imported::Namespace)) {
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

    /// What a parameter or a local holds, as far as the source writes its class: an instance
    /// of a class of the repository, something from outside it, or unknown. Other bindings
    /// stay as they are.
    fn instance(&self, binding: Binding<'a>, work: &mut Work<'a>) -> Binding<'a> {
        let (path, scope, name) = match binding {
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
            other => return other,
        };
        let Some(class) = self.receivers[path].get(&(scope, name)) else {
            return Binding::Unknown;
        };
        if let Class::Imported = class {
            return self.imported(path, name, &mut Seen::new());
        }
        let key = Busy::Local(path, scope, name);
        if work.busy.contains(&key) {
            return Binding::Unknown;
        }
        work.busy.push(key);
        let found = match class {
            Class::Enclosing => self.class(path, scope),
            Class::Named(class) => self.resolve(path, scope, class, &mut Seen::new(), work),
            Class::Type(class) => {
                let (seen, reading) = (&mut Seen::new(), Reading::Path);
                self.resolve_as(path, scope, class, seen, work, reading)
            }
            Class::Returned(callee) => self.returned(path, scope, callee, work),
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
    fn returned(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        if work.returns >= RETURN_DEPTH {
            return Binding::Unknown;
        }
        work.returns += 1;
        let found = self.result(path, scope, callee, work);
        work.returns -= 1;
        found
    }

    fn result(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        work: &mut Work<'a>,
    ) -> Binding<'a> {
        let (seen, path_reading) = (&mut Seen::new(), Reading::Path);
        let by_path = callee.rsplit_once('.');
        let reading = if by_path.is_some() {
            path_reading
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
                    match self.resolve_as(path, scope, ty, &mut Seen::new(), work, path_reading) {
                        Binding::Def(c) if self.is_class(c) => Binding::Def(c),
                        _ => Binding::Unknown,
                    }
                }
                Some(Class::Type(class)) => {
                    let seen = &mut Seen::new();
                    self.resolve_as(t.path, t.name, class, seen, work, path_reading)
                }
                _ => Binding::Unknown,
            },
            external @ Binding::External(..) => external,
            _ => Binding::Unknown,
        }
    }

    /// `member` of `class`: the first class of its family that defines it, or binds it as an
    /// attribute. A base whose members are not known, met first, leaves it unknown, and so
    /// does a family C3 rejects.
    fn member(&self, class: Target<'a>, member: &'a str, work: &mut Work<'a>) -> Binding<'a> {
        let Some(family) = self.family(class, work) else {
            return Binding::Unknown;
        };
        for ancestor in family {
            let Ancestor::Class(c) = ancestor else {
                return Binding::Unknown;
            };
            if let Some(found) = self.block_member(c, member) {
                return found;
            }
            let qualified = format!("{}.{member}", c.name);
            if let Some((name, _)) = self.defs[c.path].get_key_value(qualified.as_str()) {
                return Binding::Def(Target { path: c.path, name });
            }
            if self.receivers[c.path].contains_key(&(c.name, member)) {
                return Binding::Local(c.path, c.name, member);
            }
        }
        Binding::Unknown
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
        self.lineage(class, work).unwrap_or_else(|Cut| {
            // From no other family, the cut is the class's own: longer than the limit.
            if first {
                work.families.insert(class, None);
            }
            None
        })
    }

    /// `family`, or `Cut` when the bases nest deeper than `FAMILY_DEPTH` below the families
    /// `work` is resolving.
    fn lineage(&self, class: Target<'a>, work: &mut Work<'a>) -> Result<Family<'a>, Cut> {
        if let Some(family) = self.families.get(&class).or(work.families.get(&class)) {
            return Ok(family.clone());
        }
        let key = Busy::Family(class);
        if work.busy.contains(&key) {
            return Ok(None);
        }
        if (work.busy.iter())
            .filter(|b| matches!(b, Busy::Family(_)))
            .count()
            >= FAMILY_DEPTH
        {
            return Err(Cut);
        }
        work.busy.push(key);
        let bases = self.bases(class, work);
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
        lists.push(Some(bases));
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
            .map(
                |(n, base)| match self.resolve(class.path, scope, base, &mut Seen::new(), work) {
                    Binding::Def(t) if self.is_class(t) => Ancestor::Class(t),
                    _ if base.starts_with('(') => {
                        Ancestor::Opaque(format!("{}::{}#{n}", class.path, class.name))
                    }
                    _ => Ancestor::Opaque(self.imported_name(class.path, scope, base)),
                },
            )
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
                let found = imports.iter().rev().find_map(|i| match &i.name {
                    Imported::Path(t) if bound_in(&i.local, s, head) => Some(t),
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
        let imports = imports.iter().rev();
        match imports.map(|i| (i, &i.name)).find(|(i, _)| i.local == head) {
            Some((i, Imported::Namespace)) => format!("{}{rest}", i.source),
            Some((i, Imported::Name(n) | Imported::Member { name: n, .. })) => {
                format!("{}.{n}{rest}", i.source)
            }
            _ => name,
        }
    }

    /// The definitions whose name matches the callee's last name anywhere in the repository:
    /// methods for a member call (`x.area()`), top-level functions and classes for a plain
    /// call (`area()`), both for a path (`a::area()`, `by_path`).
    fn infer(&self, callee: &str, by_path: bool) -> Resolution<'a> {
        let (member, last) = match callee.rsplit_once('.') {
            Some((_, last)) => (true, last),
            None => (false, callee),
        };
        let found = |member| (self.by_name.get(&(member, last))).map_or(&[][..], Vec::as_slice);
        let mut found = found(member).to_vec();
        if member && by_path {
            found.extend(self.by_name.get(&(false, last)).into_iter().flatten());
            found.sort();
        }
        match found[..] {
            [] => Resolution::Unresolved,
            [one] => Resolution::Inferred(one),
            _ => Resolution::Ambiguous(found),
        }
    }

    fn def(&self, path: &'a str, name: &str) -> Binding<'a> {
        match self.defs[path].get_key_value(name) {
            Some((name, _)) => Binding::Def(Target { path, name }),
            None => Binding::Unknown,
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

    /// What `name` refers to from `scope`: the innermost enclosing definition first (a local
    /// binding, which hides the name; else what it defines, its constants, its parameters, its
    /// receivers), then the top level, then the file's imports. Methods are not lexical
    /// bindings, and a class body's names are in scope only for the statements written directly
    /// in it, not for its methods or nested classes. A module written in place (`Kind::Module`)
    /// ends the climb: the names around it are not its own, only the file's imports are.
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
            if class_body {
                continue;
            }
            let candidate = if s.is_empty() {
                name.to_string()
            } else {
                format!("{s}.{name}")
            };
            if reading.types() {
                match defs.get_key_value(candidate.as_str()) {
                    Some((name, def)) if def.kind != Kind::Method => {
                        return Binding::Def(Target { path, name });
                    }
                    _ => {
                        if let Some(found) = self.imported_at(path, s, name, seen) {
                            return found;
                        }
                        continue;
                    }
                }
            }
            // A `let`, a `catch` binding, a destructured name: what it holds is not known, and
            // it hides every name it spells, outer ones and those of its own definition (a
            // definition, constant or parameter it may shadow in a block alike).
            if self.locals[path].contains(candidate.as_str()) {
                return Binding::Hidden;
            }
            if let Some((name, def)) = defs.get_key_value(candidate.as_str())
                && def.kind != Kind::Method
                && !(def.kind == Kind::Module && reading == Reading::Value)
            {
                return Binding::Def(Target { path, name });
            }
            if let Some(constant) = self.consts[path].get(candidate.as_str()) {
                return Binding::Const(path, constant);
            }
            if let Some((function, def)) = defs.get_key_value(s)
                && let Some((n, field)) = def.params.iter().enumerate().find_map(|(n, p)| {
                    if p.name == name {
                        return Some((n, None));
                    }
                    let field = p.fields.iter().position(|f| f.name == name)?;
                    Some((n, Some(field)))
                })
            {
                let function = Target {
                    path,
                    name: function,
                };
                return Binding::Param(function, n, field);
            }
            match self.receivers[path].get(&(s, name)) {
                Some(Class::Imported) => return self.imported_in(path, name, seen, true),
                Some(_) => return Binding::Local(path, s, name),
                None => {}
            }
            if let Some(found) = self.imported_at(path, s, name, seen) {
                return found;
            }
        }
        self.imported_in(path, name, seen, top)
    }

    /// What the file's imports bind `name` to. Several imports may bind one name: the latest
    /// in source order decides, whatever it resolves to. An import of every name binds
    /// `name` only if the module takes it. The imports a pack scopes (`Imported::Path` and
    /// `Glob`, Rust's `use`) are not these: `imported_at` reads them where they are written,
    /// and a name no import binds is a crate outside the file when the pack says so
    /// (`Facts::externs`).
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
        for import in self.files[path].imports.iter().rev() {
            let scoped = matches!(import.name, Imported::Path(_) | Imported::Glob { .. });
            // A module declared in the file (Rust's `mod x;`) is in scope where its
            // definition is.
            let declared = matches!(import.name, Imported::Namespace)
                && (self.defs[path].get(import.local.as_str()))
                    .is_some_and(|d| d.kind == Kind::Module);
            if import.local == name && !scoped && (top || !declared) {
                let scope = scope_of(&import.local);
                return self.import(path, scope, &import.source, &import.name, seen);
            }
            if matches!(import.name, Imported::All)
                && let Some(module) = self.module(path, &import.source)
                && let found @ (Binding::Def(_) | Binding::Module(_)) =
                    self.star(module, name, seen)
            {
                return found;
            }
        }
        if top && let Some(found) = self.globs(path, "", name, false, seen) {
            return found;
        }
        if !self.files[path].externs.iter().any(|n| n == name) {
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
        for import in self.files[path].imports.iter().rev() {
            if let Imported::Path(target) = &import.name
                && bound_in(&import.local, scope, name)
                && !seen.contains(&(path, target))
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

    /// What the glob imports written in `scope` give `name`, the latest first; `public` limits
    /// them to the re-exporting ones (`pub use`). A glob is asked for a name once: it may reach
    /// itself.
    fn globs(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        public: bool,
        seen: &mut Seen<'a>,
    ) -> Option<Binding<'a>> {
        let imports = self.files[path].imports.iter().enumerate();
        for (at, import) in imports.rev() {
            let Imported::Glob {
                path: target,
                inside,
                public: re_exports,
            } = &import.name
            else {
                continue;
            };
            if (public && !re_exports)
                || !bound_in(&import.local, scope, "*")
                || seen.links >= LINK_DEPTH
                || !seen.globs.insert((path, at, name))
            {
                continue;
            }
            seen.links += 1;
            let work = &mut Work::default();
            let found = match self.resolve_as(path, scope, target, seen, work, Reading::Alias) {
                Binding::Module(m) if *inside => self.export(m, name, seen),
                Binding::Module(m) => self.star(m, name, seen),
                // A module written in place states no visibility of its items: only a glob
                // from inside it, which sees them all, is read.
                Binding::Def(t) if *inside && self.defs[t.path][t.name].kind == Kind::Module => {
                    let member = format!("{}.{name}", t.name);
                    match self.def(t.path, &member) {
                        Binding::Unknown => self
                            .imported_at(t.path, t.name, name, seen)
                            .unwrap_or(Binding::Unknown),
                        found => found,
                    }
                }
                _ => {
                    seen.links -= 1;
                    continue;
                }
            };
            seen.links -= 1;
            if matches!(found, Binding::Def(_) | Binding::Module(_)) {
                return Some(found);
            }
        }
        None
    }

    /// What the callee-like name `target` written in `scope` gives (a `use` path): its head
    /// read among types and modules, so no local hides it. An alias met again while resolving
    /// itself gives nothing.
    fn alias(
        &self,
        path: &'a str,
        scope: &'a str,
        target: &'a str,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        if seen.links >= LINK_DEPTH || !seen.insert((path, target)) {
            return Binding::Unknown;
        }
        seen.links += 1;
        let work = &mut Work::default();
        let found = self.resolve_as(path, scope, target, seen, work, Reading::Alias);
        seen.links -= 1;
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
                Binding::Unknown => None,
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
            (Imported::Name(name) | Imported::Member { name, .. }, Some(module)) => {
                self.export(module, name, seen)
            }
            (Imported::All, Some(_)) => Binding::Unknown,
        };
        if let Imported::Member { module, .. } = name
            && matches!(found, Binding::Unknown | Binding::External(..))
            && let Some(module) = self.module(path, module)
        {
            return Binding::Module(module);
        }
        found
    }

    /// What a star import of `path` takes as `name`.
    fn star(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        if self.files[path].star.takes(name) {
            self.export(path, name, seen)
        } else {
            (self.globs(path, "", name, true, seen)).unwrap_or(Binding::Unknown)
        }
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
        if !seen.insert((path, name)) {
            return Binding::Unknown;
        }
        let exports = &self.files[path].exports;
        let mut named = exports.iter().filter(|e| exports_name(e, name));
        let namespace = |e: &&Export| {
            matches!(
                e,
                Export::From {
                    name: Imported::Namespace,
                    ..
                }
            )
        };
        let found = match value {
            true => (named.clone().find(|e| !namespace(e))).or_else(|| named.next()),
            false => named.next(),
        };
        match found {
            Some(Export::Local { local, .. }) => {
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
                return self.lexical_as(path, scope, bare, seen, reading);
            }
            Some(Export::From {
                source,
                name: imported,
                ..
            }) => return self.import(path, "", source, imported, seen),
            _ => {}
        }
        for export in exports {
            if let Export::All { source } = export
                && let Some(module) = self.module(path, source)
                && let found @ (Binding::Def(_) | Binding::Module(_)) =
                    self.star(module, name, seen)
            {
                return found;
            }
        }
        (self.globs(path, "", name, false, seen)).unwrap_or(Binding::Unknown)
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
                        if private.contains(exported.as_str()) && !facts.star.takes(exported) {
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
            names.retain(|n| facts.exports.iter().all(|e| !exports_name(e, n)));
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
                out.extend(named.filter(|n| facts.star.takes(n)));
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

/// A call whose arguments bind the parameters of `def`, the function it reaches.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub def: Target<'a>,
    pub path: &'a str,
    pub index: usize,
}

/// What `Code::expand_frames` finds at one site.
pub struct Expanded<'a, R> {
    /// The site's file and call position.
    pub site: (&'a str, usize),
    /// The wrapper calls climbed to reach the site, innermost first; the last is the site.
    pub frames: Vec<Frame<'a>>,
    /// What the reader made of the arguments there.
    pub found: R,
}

/// Arguments as written in `path` within `scope`, read on behalf of the call `at`.
struct Written<'a> {
    path: &'a str,
    scope: &'a str,
    args: &'a [Value],
    kwargs: &'a Keywords,
    at: (&'a str, usize),
}

/// How many calls a builder chain is read through, down to what starts it.
// ponytail: a chain of more builder calls than this is not read to its start; its calls past
// the limit have no origin.
pub const FLUENT_CHAIN: usize = 1024;

/// Where a builder chain stops (`Code::chain_tail`): the call, whether a step above it was
/// found only by a name match, and how many steps lead to it.
#[derive(Clone, Copy)]
struct Tail {
    stop: usize,
    matched: bool,
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
/// again only through a cycle; `out` holds the origins found.
#[derive(Default)]
struct Trail<'a> {
    frames: Vec<Frame<'a>>,
    busy: Vec<Flow<'a>>,
    out: BTreeSet<(&'a str, usize)>,
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
    pack: &'static dyn LanguagePack,
}

/// Where a value comes from when a package outside the repository made it: the specifier
/// imported, and what the code did with what it imports, written as JavaScript would write
/// it from that name. `const kafka = new Kafka(...)` with `Kafka` imported from `kafkajs`,
/// then `const producer = kafka.producer()`, makes `producer` the origin `kafkajs`,
/// `new Kafka().producer()`. A namespace import adds no name; a default import is `default`.
pub struct Origin<'a> {
    pub package: &'a str,
    pub expr: String,
    /// The constant the traced name refers to, as its file and qualified name; `None` when
    /// the name is the import itself.
    pub binding: Option<(&'a str, &'a str)>,
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
            pack,
        }
    }

    /// The language pack whose files these are.
    pub fn pack(&self) -> &'static dyn LanguagePack {
        self.pack
    }

    /// The origin of `chain`, a name or member chain (`a.b`) written in `path` within
    /// `scope`, through constants in the same file and imported ones: `None` unless it comes
    /// from an import of something outside the repository. Only `const` bindings are
    /// followed; a parameter has no origin.
    pub fn origin(&self, path: &'a str, scope: &'a str, chain: &'a str) -> Option<Origin<'a>> {
        self.trace(path, scope, chain, &mut Vec::new())
    }

    fn trace(
        &self,
        path: &'a str,
        scope: &'a str,
        chain: &'a str,
        seen: &mut Vec<(&'a str, &'a str)>,
    ) -> Option<Origin<'a>> {
        let mut members = chain.split('.');
        let head = members.next()?;
        let mut origin = match self.resolver.lexical(path, scope, head, &mut Seen::new()) {
            Binding::External(package, name) => Origin {
                package,
                expr: match name {
                    Imported::Namespace => String::new(),
                    Imported::Name(name) | Imported::Member { name, .. } => name.clone(),
                    // A star import spells no name of its own.
                    Imported::All | Imported::Path(_) | Imported::Glob { .. } => return None,
                },
                binding: None,
            },
            Binding::Const(at, constant) => {
                // `const a = b; const b = a;` comes from nowhere.
                if seen.contains(&(at, constant)) {
                    return None;
                }
                seen.push((at, constant));
                let (value, within) = self.held(at, constant)?;
                let mut origin = match value {
                    Value::Ref(name) => self.trace(at, within, name, seen)?,
                    Value::Call(call) => {
                        let callee = &call.callee;
                        let mut origin = self.trace(at, within, callee, seen)?;
                        origin.expr += "()";
                        origin
                    }
                    Value::New(callee, _) => {
                        let mut origin = self.trace(at, within, callee, seen)?;
                        origin.expr = format!("new {}()", origin.expr);
                        origin
                    }
                    _ => return None,
                };
                origin.binding = Some((at, constant));
                origin
            }
            _ => return None,
        };
        for member in members {
            if !origin.expr.is_empty() {
                origin.expr.push('.');
            }
            origin.expr += member;
        }
        Some(origin)
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
        let (head, members) = chain
            .split_once('.')
            .map_or((chain, None), |(h, m)| (h, Some(m)));
        let Binding::External(specifier, name) =
            self.resolver.lexical(path, scope, head, &mut Seen::new())
        else {
            return None;
        };
        // A crate a pack's path import (Rust's `use`) names: the whole path, as `external` spells it.
        let pathed = self.resolver.files[path]
            .imports
            .iter()
            .any(|i| matches!(i.name, Imported::Path(_)));
        if pathed && matches!(name, Imported::Namespace) {
            let full = self.resolver.imported_name(path, scope, chain);
            let rest = full.strip_prefix(specifier)?;
            let rest = rest
                .strip_prefix('.')
                .or_else(|| rest.is_empty().then_some(rest))?;
            return Some((specifier, rest.to_string()));
        }
        let mut spelled = match name {
            Imported::Namespace => String::new(),
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

    /// The values the call at `index` of `path` may return, when a function of the repository
    /// answers it (`Exact`) or a name match does (`Inferred`, `Ambiguous`): the file and name
    /// of each function with each value it returns.
    pub fn returned(&self, path: &'a str, index: usize) -> Vec<(&'a str, &'a str, &'a Value)> {
        let call = &self.resolver.files[path].calls[index];
        let targets = match self.resolver.call(path, call) {
            Resolution::Exact(t) | Resolution::Inferred(t) => vec![t],
            Resolution::Ambiguous(ts) => ts,
            Resolution::Unresolved => return Vec::new(),
        };
        let mut out = Vec::new();
        for t in targets {
            let defs = self.full(t.path).map(|f| f.defs.iter());
            let def = defs.and_then(|mut d| d.find(|d| d.name == t.name));
            let values = def.map(|d| d.return_values.iter()).into_iter().flatten();
            out.extend(values.map(|v| (t.path, t.name, v)));
        }
        out
    }

    /// The index among the calls of `path` of the one spanning `range` (an `Invocation`'s).
    pub fn call_index(&self, path: &'a str, range: &std::ops::Range<usize>) -> Option<usize> {
        call_at(&self.resolver.files[path].calls, range)
    }

    /// Whether the pack binds `name` everywhere, as JavaScript does `fetch`.
    pub fn binds_everywhere(&self, name: &str) -> bool {
        self.resolver.globals.contains(&name)
    }

    /// What `callee`, written in `scope` of `path`, names outside the repository: the module
    /// and name the file's imports give it (`fastapi.APIRouter`), when an import from outside
    /// the repository binds it.
    pub fn external(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Option<String> {
        let binding = self.resolver.binding(path, scope, callee);
        matches!(binding, Binding::External(..))
            .then(|| self.resolver.imported_name(path, scope, callee))
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
            Binds::Any | Binds::Instance | Binds::Class => (position)
                .and_then(|at| call.args.get(at))
                .or_else(|| call.keyword(&param.name)),
        }
    }

    /// The calls whose result `name`, written in `scope` of `path`, may hold, sorted: followed
    /// through constants and imports, the arguments callers pass to parameters, the values
    /// functions of the repository return and the items of a list or tuple. A call that
    /// reaches no such function is where the value comes from, such as a constructor
    /// (`APIRouter()`); whatever else the name may hold (an assignment that is not its only
    /// binding, an attribute) is left out.
    pub fn origins(&self, path: &'a str, scope: &'a str, name: &'a str) -> Vec<(&'a str, usize)> {
        let mut trail = Trail::default();
        self.flow_name(path, scope, name, &mut trail);
        trail.out.into_iter().collect()
    }

    /// `origins` of a value as written.
    pub fn value_origins(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
    ) -> Vec<(&'a str, usize)> {
        let mut trail = Trail::default();
        self.flow(path, scope, value, &mut trail);
        trail.out.into_iter().collect()
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
                // answers. A step only a name match found must start at an outside constructor,
                // whose method it is then (a method of the library wins over a name).
                let tail = match self.pack.fluent() {
                    true => self.chain_tail(path, index),
                    false => Some(Tail {
                        stop: index,
                        matched: false,
                        depth: 0,
                    }),
                };
                let Some(Tail { stop, matched, .. }) = tail else {
                    return;
                };
                let kept = matched.then(|| std::mem::take(&mut trail.out));
                self.flow_from(path, stop, trail);
                if let Some(mut kept) = kept {
                    let outside = (trail.out.iter()).all(|&(p, n)| {
                        let call = &self.resolver.files[p].calls[n];
                        self.external(p, &call.scope, &call.callee).is_some()
                    });
                    if outside {
                        kept.append(&mut trail.out);
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

    /// The origins of the call at `index` of `path`, where a chain stops: what a function of
    /// the repository returns, the value the call is made on, or the call itself.
    fn flow_from(&self, path: &'a str, index: usize, trail: &mut Trail<'a>) {
        let calls = &self.resolver.files[path].calls;
        let function = match self.resolver.call(path, &calls[index]) {
            Resolution::Exact(t) if !self.resolver.is_class(t) => t,
            _ if self.pack.fluent() => match self.receiver(path, index) {
                None => {
                    trail.out.insert((path, index));
                    return;
                }
                Some(Value::Call(_)) => return,
                Some(other) => return self.flow(path, &calls[index].scope, other, trail),
            },
            _ => {
                trail.out.insert((path, index));
                return;
            }
        };
        let Some(def) = (self.full(function.path))
            .and_then(|f| f.defs.iter().find(|d| d.name == function.name))
        else {
            return;
        };
        if trail.busy.contains(&Flow::Returns(function)) {
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

    /// Where the builder chain ending in the call at `index` of `path` stops: the first call
    /// down its receivers that a function of the repository answers or that is made on no
    /// call. Each call is walked once, whatever the chain.
    fn chain_tail(&self, path: &'a str, index: usize) -> Option<Tail> {
        let calls = &self.resolver.files[path].calls;
        let mut walked: Vec<(usize, bool)> = Vec::new();
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
                    walked.push((at, !matches!(resolution, Resolution::Unresolved)));
                    match call_at(calls, &next.range) {
                        Some(next) => at = next,
                        None => break None,
                    }
                }
                _ => {
                    break Some(Tail {
                        stop: at,
                        matched: false,
                        depth: 0,
                    });
                }
            }
        };
        for &(step, matched) in walked.iter().rev() {
            tail = tail.filter(|t| t.depth < FLUENT_CHAIN).map(|t| Tail {
                matched: t.matched || matched,
                depth: t.depth + 1,
                ..t
            });
            self.chains.borrow_mut().insert((path, step), tail);
        }
        tail
    }

    fn flow_name(&self, path: &'a str, scope: &'a str, name: &'a str, trail: &mut Trail<'a>) {
        let binding = self.resolver.binding(path, scope, name);
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
                let found = self
                    .full(at)
                    .and_then(|f| f.consts.iter().find(|c| c.name == constant));
                let Some(found) = found else {
                    return;
                };
                if trail.busy.contains(&Flow::Const(at, constant)) {
                    return;
                }
                trail.busy.push(Flow::Const(at, constant));
                self.flow(at, crate::parent(constant), &found.value, trail);
                trail.busy.pop();
            }
            Binding::Param(function, n, None) => {
                // Inside a function whose return value is followed, its call binds the
                // parameter; else every call of it does.
                let sites = match trail.frames.last() {
                    Some(frame) if frame.def == function => vec![(frame.path, frame.index)],
                    // ponytail: a parameter of an outer function, captured by a closure, is
                    // not followed.
                    Some(_) => return,
                    // A caller found only by a name match may pass any object: not climbed.
                    None => (self.resolver.callers.get(&function).into_iter().flatten())
                        .filter(|c| !c.by_name)
                        .map(|c| (c.path, c.index))
                        .collect(),
                };
                if trail.busy.contains(&Flow::Param(function, n)) {
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
        let consts = &self.full(path)?.consts;
        consts
            .iter()
            .find(|c| c.name == constant && c.ty.is_some())?
            .ty
            .as_ref()
    }

    /// What the function making the call at `index` of `path` does with its result
    /// (`Call::uses`).
    pub fn uses(&self, path: &'a str, index: usize) -> Option<&'a [Vec<Step>]> {
        Some(self.full(path)?.calls.get(index)?.uses.as_slice())
    }

    /// The declared return type of the definition `name` of `path` (`Def::returns`).
    pub fn returns(&self, path: &'a str, name: &str) -> Option<&'a Ty> {
        let def = self.full(path)?.defs.iter().find(|d| d.name == name)?;
        def.returns.as_ref()
    }

    /// The names of the type parameters of the definition `name` of `path`
    /// (`Def::type_params`).
    pub fn type_params(&self, path: &'a str, name: &str) -> &'a [String] {
        let def = self
            .full(path)
            .and_then(|f| f.defs.iter().find(|d| d.name == name));
        def.map_or(&[], |d| d.type_params.as_slice())
    }

    fn full(&self, path: &'a str) -> Option<&'a Facts> {
        let found = self.full.get(path).copied();
        if found.is_none() {
            self.misses.borrow_mut().insert(path);
        }
        found
    }

    /// The parameter at position `n` of `function`.
    fn param(&self, function: Target<'a>, n: usize) -> Option<&'a Param> {
        let file = self.full(function.path)?;
        file.defs
            .iter()
            .find(|d| d.name == function.name)?
            .params
            .get(n)
    }

    /// `value`, written in `path` within `scope`, with its constants folded. Parameters are
    /// unknown.
    pub fn value(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Value {
        unbind(self.eval(path, scope, value, &[], 0, &mut Vec::new(), &mut None))
    }

    /// The constant `name` refers to from `scope` in `path`: its file and qualified name, and
    /// its value as written.
    pub fn constant(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<((&'a str, &'a str), &'a Value)> {
        match self.resolver.lexical(path, scope, name, &mut Seen::new()) {
            Binding::Const(at, constant) => Some(((at, constant), self.held(at, constant)?.0)),
            _ => None,
        }
    }

    /// The value the `const` `constant` of `path` holds, as written, and the scope it is
    /// written in.
    fn held(&self, path: &'a str, constant: &'a str) -> Option<(&'a Value, &'a str)> {
        let value = &self
            .full(path)?
            .consts
            .iter()
            .find(|c| c.name == constant && !matches!(c.value, Value::Type(_)))?
            .value;
        Some((value, scope_of(constant)))
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
            self.resolver.lexical(path, scope, name, &mut Seen::new())
        else {
            return None;
        };
        // Namespaces open no scope: two declarations of one name leave it unknown.
        let mut declared = (self.full(at)?.consts.iter())
            .filter(|c| c.name == constant && matches!(c.value, Value::Type(_)));
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
            self.resolver.lexical(path, scope, name, &mut Seen::new())
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
            self.resolver.lexical(path, scope, name, &mut Seen::new()),
            Binding::Param(f, at, None) if f == function && at == n
        )
    }

    /// Whether `name`, written in `path` within `scope`, is no binding of the file at all: a
    /// global such as `Response`. A local binding of the name hides the global.
    pub fn is_global(&self, path: &'a str, scope: &'a str, name: &'a str) -> bool {
        matches!(
            self.resolver.lexical(path, scope, name, &mut Seen::new()),
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

    /// The file `name`, written in `scope` of `path`, refers to as a whole module.
    pub fn module(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<&'a str> {
        match self.resolver.binding(path, scope, name) {
            Binding::Module(m) => Some(m),
            _ => None,
        }
    }

    /// The file an import of `specifier` from `path` names, if the pack parses it.
    pub fn import_file(&self, path: &str, specifier: &str) -> Option<&'a str> {
        self.resolver.module(path, specifier)
    }

    /// What `read` makes of the positional and keyword arguments of the call at `index` of
    /// `path`, at each call site that determines them. `read` answers `None` when what it
    /// needs holds a `Ref`: a parameter of the enclosing function, which makes that function a
    /// client wrapper. Each call site of the wrapper is then read with its own arguments
    /// substituted, through any number of wrapper levels. A wrapper reached again through its
    /// own callers (a cycle) adds nothing; a wrapper nobody calls is read where it stands, its
    /// parameters unknown.
    // ponytail: every simple path up the wrappers' callers is walked; memoize per (wrapper,
    // call site) if a dense call graph makes this slow.
    pub fn expand<R>(
        &self,
        path: &'a str,
        index: usize,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<(&'a str, usize, R)> {
        (self.expand_frames(path, index, read).into_iter())
            .map(|e| (e.site.0, e.site.1, e.found))
            .collect()
    }

    /// `expand`, with the wrapper calls climbed from the call to each site.
    pub fn expand_frames<R>(
        &self,
        path: &'a str,
        index: usize,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<Expanded<'a, R>> {
        // The full call: the resolver's files may be kept from the index, without values.
        let Some(call) = self.call(path, index) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let written = Written {
            path,
            scope: &call.scope,
            args: &call.args,
            kwargs: &call.kwargs,
            at: (path, index),
        };
        self.walk(&written, &mut Vec::new(), read, &mut out);
        out
    }

    /// `expand` for the arguments of the `new` expression the `const` `binding` holds
    /// (`Origin::binding`), on behalf of the call `at` made on that binding: `at` is the site
    /// when they evaluate where the binding stands, and the function enclosing the binding is
    /// the client wrapper when they depend on its parameters.
    pub fn expand_constructed<R>(
        &self,
        (file, constant): (&'a str, &'a str),
        at: (&'a str, usize),
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
    ) -> Vec<(&'a str, usize, R)> {
        let Some((Value::New(_, args), scope)) = self.held(file, constant) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let written = Written {
            path: file,
            scope,
            args,
            kwargs: &[],
            at,
        };
        self.walk(&written, &mut Vec::new(), read, &mut out);
        (out.into_iter())
            .map(|e| (e.site.0, e.site.1, e.found))
            .collect()
    }

    fn walk<R>(
        &self,
        written: &Written<'a>,
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
        let mut wants = None;
        let values: Vec<Value> = args
            .iter()
            .map(|a| self.eval(path, scope, a, frames, 0, &mut Vec::new(), &mut wants))
            .collect();
        let kwargs: Vec<(String, Value)> = (kwargs.iter())
            .map(|(k, v)| {
                let v = self.eval(path, scope, v, frames, 0, &mut Vec::new(), &mut wants);
                (k.clone(), v)
            })
            .collect();
        let (site, at) = frames.last().map_or(written.at, |f| (f.path, f.index));
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
            .filter(|c| !c.by_name)
            .copied()
            .collect();
        if callers.is_empty() {
            let values: Vec<Value> = values.into_iter().map(unbind).collect();
            let kwargs: Vec<(String, Value)> =
                kwargs.into_iter().map(|(k, v)| (k, unbind(v))).collect();
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

    /// `value` with its names folded. `frames[depth..]` bind the parameters of the function
    /// enclosing `scope` and of its callers, innermost first; a parameter no frame binds stays
    /// a `Ref`, and its function is recorded in `wants`.
    #[allow(clippy::too_many_arguments)]
    fn eval(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &[Frame<'a>],
        depth: usize,
        seen: &mut Vec<(&'a str, &'a str)>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        match value {
            Value::Str(parts) => {
                let mut out = Vec::new();
                for part in parts {
                    let Part::Ref(name) = part else {
                        out.push(part.clone());
                        continue;
                    };
                    match self.name(path, scope, name, frames, depth, seen, wants) {
                        Value::Str(parts) => out.extend(parts),
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
                        let v = self.eval(path, scope, v, frames, depth, seen, wants);
                        (k.clone(), v)
                    })
                    .collect(),
            ),
            Value::List(items) => Value::List(
                (items.iter())
                    .map(|v| self.eval(path, scope, v, frames, depth, seen, wants))
                    .collect(),
            ),
            Value::Ref(name) => self.name(path, scope, name, frames, depth, seen, wants),
            other => other.clone(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn name(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        frames: &[Frame<'a>],
        depth: usize,
        seen: &mut Vec<(&'a str, &'a str)>,
        wants: &mut Option<Target<'a>>,
    ) -> Value {
        match self.resolver.binding(path, scope, name) {
            Binding::Const(at, constant) => {
                // `const a = b; const b = a;` folds to nothing.
                if seen.contains(&(at, constant)) {
                    return Value::Unknown;
                }
                let Some((value, within)) = self.held(at, constant) else {
                    return Value::Unknown;
                };
                seen.push((at, constant));
                let folded = self.eval(at, within, value, frames, depth, seen, wants);
                seen.pop();
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
                            self.eval(frame.path, &call.scope, arg, frames, depth + 1, seen, wants)
                        }
                        // A left-out argument takes the parameter's default.
                        (None, Some(default)) => self.eval(
                            function.path,
                            function.name,
                            default,
                            frames,
                            depth,
                            seen,
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
                            seen,
                            wants,
                        ),
                        (Some(read), _) => read,
                        (None, None) => Value::Undefined,
                    }
                }
                // ponytail: a parameter of an outer function, captured by a closure, is
                // unknown; bind it when closures that escape their function matter.
                Some(_) => Value::Unknown,
                None => {
                    wants.get_or_insert(function);
                    Value::Ref(name.to_string())
                }
            },
            _ => Value::Unknown,
        }
    }
}

/// `value` with every parameter left in it unknown.
fn unbind(value: Value) -> Value {
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
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::python::Python;

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
                    Resolution::Inferred(t) => format!("inferred {}::{}", t.path, t.name),
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
        let parsed = parsed(&[(
            "main.rs",
            "struct S;\nimpl S { fn new() -> Self { S } fn go(&self) {} }\n\
             fn f() { let s = S::new(); s.go(); }\n",
        )]);
        let map: BTreeMap<&str, &Facts> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let resolver = resolver(&map);
        let new = |returns| {
            let mut work = Work {
                returns,
                ..Work::default()
            };
            resolver.returned("main.rs", "f", "S.new", &mut work)
        };
        assert!(matches!(new(0), Binding::Def(t) if t.name == "S"));
        assert!(matches!(new(RETURN_DEPTH - 1), Binding::Def(t) if t.name == "S"));
        assert!(matches!(new(RETURN_DEPTH), Binding::Unknown));
    }
}
