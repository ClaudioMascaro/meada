//! The shared resolution (ADR 0007): connects each call a language pack found to the
//! definitions it reaches, with Meada's own scope and import resolution and no types.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::lang::{
    Call, Class, Def, Export, Facts, Imported, Kind, LanguagePack, Modules, Param, Part, Star,
    Value,
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
    /// A name bound in a scope (`Facts::locals`): its file, scope and name.
    Local(&'a str, &'a str, &'a str),
    /// Something an import brings from outside the repository.
    External,
    /// Neither scope nor import resolution knows it.
    Unknown,
}

/// Seen `(file, exported name)` pairs: re-exports may form cycles.
type Seen<'a> = HashSet<(&'a str, &'a str)>;

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
/// C3 rejects the hierarchy, or the bases form a cycle.
type Family<'a> = Option<Vec<Ancestor<'a>>>;

/// The files of one language pack, resolved together.
pub struct Resolver<'a> {
    files: &'a BTreeMap<&'a str, &'a Facts>,
    defs: HashMap<&'a str, HashMap<&'a str, &'a Def>>,
    consts: HashMap<&'a str, HashSet<&'a str>>,
    /// Each file's `Facts::locals`, by scope and name.
    locals: HashMap<&'a str, HashMap<(&'a str, &'a str), &'a Class>>,
    /// Every class's family, linearized once.
    families: HashMap<Target<'a>, Family<'a>>,
    /// The pack's keyword for the instance of the enclosing class (`LanguagePack::receiver`).
    receiver: Option<&'static str>,
    /// The pack's names bound everywhere (`LanguagePack::globals`).
    globals: &'static [&'static str],
    /// The call sites whose single target (exact or inferred) is each definition, sorted.
    callers: HashMap<Target<'a>, Vec<(&'a str, usize)>>,
    /// Name inference candidates, sorted: methods by their last name (`true`), top-level
    /// functions and classes by their name (`false`).
    by_name: HashMap<(bool, &'a str), Vec<Target<'a>>>,
    modules: Modules,
}

impl<'a> Resolver<'a> {
    pub fn new(
        files: &'a BTreeMap<&'a str, &'a Facts>,
        modules: Modules,
        language: &dyn LanguagePack,
    ) -> Self {
        let defs = files
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
            .map(|(path, facts)| {
                let names = (facts.locals.iter())
                    .map(|l| ((l.scope.as_str(), l.name.as_str()), &l.class))
                    .collect();
                (*path, names)
            })
            .collect();
        let mut by_name: HashMap<_, Vec<Target>> = HashMap::new();
        for (path, facts) in files {
            for def in &facts.defs {
                let name = def.name.as_str();
                let key = match (def.kind, name.rsplit_once('.')) {
                    (Kind::Method, Some((_, last))) => (true, last),
                    (Kind::Function | Kind::Class, None) => (false, name),
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
            families: HashMap::new(),
            receiver: language.receiver(),
            globals: language.globals(),
            callers: HashMap::new(),
            by_name,
            modules,
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
        let mut callers: HashMap<Target, Vec<(&str, usize)>> = HashMap::new();
        for (path, facts) in files {
            for (n, call) in facts.calls.iter().enumerate() {
                if let Resolution::Exact(t) | Resolution::Inferred(t) = resolver.call(path, call) {
                    callers.entry(t).or_default().push((path, n));
                }
            }
        }
        resolver.callers = callers;
        resolver
    }

    pub fn call(&self, path: &'a str, call: &'a Call) -> Resolution<'a> {
        let work = &mut Work::default();
        match self.resolve(path, &call.scope, &call.callee, &mut Seen::new(), work) {
            Binding::Def(t) => Resolution::Exact(t),
            Binding::External => Resolution::Unresolved,
            Binding::Instance(_)
            | Binding::Module(_)
            | Binding::Const(..)
            | Binding::Param(..)
            | Binding::Local(..)
            | Binding::Unknown => self.infer(&call.callee),
        }
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
        let (mut binding, members) = self.dotted(path, callee, seen).unwrap_or_else(|| {
            let (head, members) = callee.split_once('.').unwrap_or((callee, ""));
            let binding = match head {
                _ if Some(head) == self.receiver => self.class(path, scope),
                "super" => Binding::Unknown,
                _ if head.starts_with('(') => Binding::Unknown,
                _ => self.lexical(path, scope, head, seen),
            };
            (binding, members)
        });
        for member in members.split('.').filter(|m| !m.is_empty()) {
            binding = match self.instance(binding, work) {
                Binding::Def(t) | Binding::Instance(t) if self.is_class(t) => {
                    self.member(t, member, work)
                }
                Binding::Def(t) => self.def(t.path, &format!("{}.{member}", t.name)),
                Binding::Module(m) => self.export(m, member, seen),
                other => other,
            };
        }
        binding
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
        let Some(class) = self.locals[path].get(&(scope, name)) else {
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
            Class::Imported | Class::Unknown => Binding::Unknown,
        };
        work.busy.pop();
        match found {
            Binding::Def(t) if self.is_class(t) => Binding::Instance(t),
            Binding::External => Binding::External,
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
            let qualified = format!("{}.{member}", c.name);
            if let Some((name, _)) = self.defs[c.path].get_key_value(qualified.as_str()) {
                return Binding::Def(Target { path: c.path, name });
            }
            if self.locals[c.path].contains_key(&(c.name, member)) {
                return Binding::Local(c.path, c.name, member);
            }
        }
        Binding::Unknown
    }

    /// `class` and its ancestors in C3 order, from the bases each class writes: Python's
    /// method resolution order. Without written bases (TypeScript) it is `class` alone.
    fn family(&self, class: Target<'a>, work: &mut Work<'a>) -> Family<'a> {
        if let Some(family) = self.families.get(&class).or(work.families.get(&class)) {
            return family.clone();
        }
        let key = Busy::Family(class);
        if work.busy.contains(&key) {
            return None;
        }
        work.busy.push(key);
        let bases = self.bases(class, work);
        let mut lists = Vec::new();
        for base in &bases {
            lists.push(match base {
                Ancestor::Class(base) => self.family(*base, work),
                Ancestor::Opaque(_) => Some(vec![base.clone()]),
            });
        }
        lists.push(Some(bases));
        let family = (lists.into_iter().collect::<Option<Vec<_>>>())
            .and_then(merge)
            .map(|rest| [Ancestor::Class(class)].into_iter().chain(rest).collect());
        work.busy.pop();
        work.families.insert(class, family.clone());
        family
    }

    /// What each base `class` writes is, in order, resolved from the scope enclosing it.
    fn bases(&self, class: Target<'a>, work: &mut Work<'a>) -> Vec<Ancestor<'a>> {
        let scope = class.name.rsplit_once('.').map_or("", |(scope, _)| scope);
        let bases = self.defs[class.path][class.name].bases.iter().enumerate();
        bases
            .map(
                |(n, base)| match self.resolve(class.path, scope, base, &mut Seen::new(), work) {
                    Binding::Def(t) if self.is_class(t) => Ancestor::Class(t),
                    _ if base.starts_with('(') => {
                        Ancestor::Opaque(format!("{}::{}#{n}", class.path, class.name))
                    }
                    _ => Ancestor::Opaque(self.imported_name(class.path, base)),
                },
            )
            .collect()
    }

    /// `name` as the file's imports spell it: the module and name its head imports, then the
    /// rest (`ABC` imported from `abc` is `abc.ABC`); as written when no import binds it.
    fn imported_name(&self, path: &str, name: &str) -> String {
        let (head, rest) = name.split_at(name.find('.').unwrap_or(name.len()));
        let imports = self.files[path].imports.iter().rev();
        match imports.map(|i| (i, &i.name)).find(|(i, _)| i.local == head) {
            Some((i, Imported::Namespace)) => format!("{}{rest}", i.source),
            Some((i, Imported::Name(n) | Imported::Member { name: n, .. })) => {
                format!("{}.{n}{rest}", i.source)
            }
            _ => name.to_string(),
        }
    }

    /// The definitions whose name matches the callee's last name anywhere in the repository:
    /// methods for a member call (`x.area()`), top-level functions and classes for a plain
    /// call (`area()`).
    fn infer(&self, callee: &str) -> Resolution<'a> {
        let (member, last) = match callee.rsplit_once('.') {
            Some((_, last)) => (true, last),
            None => (false, callee),
        };
        let found = self
            .by_name
            .get(&(member, last))
            .map_or(&[][..], Vec::as_slice);
        match found {
            [] => Resolution::Unresolved,
            [one] => Resolution::Inferred(*one),
            all => Resolution::Ambiguous(all.to_vec()),
        }
    }

    fn def(&self, path: &'a str, name: &str) -> Binding<'a> {
        match self.defs[path].get_key_value(name) {
            Some((name, _)) => Binding::Def(Target { path, name }),
            None => Binding::Unknown,
        }
    }

    /// The class whose body encloses `scope`, for `this`.
    fn class(&self, path: &'a str, scope: &str) -> Binding<'a> {
        let defs = &self.defs[path];
        let mut scope = scope;
        while !scope.is_empty() {
            if let Some((name, def)) = defs.get_key_value(scope)
                && def.kind == Kind::Class
            {
                return Binding::Def(Target { path, name });
            }
            scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
        }
        Binding::Unknown
    }

    /// What `name` refers to from `scope`: the innermost enclosing definition first (what it
    /// defines, its constants, its parameters, its other locals), then the top level, then the
    /// file's imports. Methods are not lexical bindings, and a class body's names are in scope
    /// only for the statements written directly in it, not for its methods or nested classes.
    fn lexical(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let defs = &self.defs[path];
        let mut scope = Some(scope);
        let mut first = true;
        while let Some(s) = scope {
            scope = (!s.is_empty()).then(|| s.rsplit_once('.').map_or("", |(parent, _)| parent));
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
            if let Some((name, def)) = defs.get_key_value(candidate.as_str())
                && def.kind != Kind::Method
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
            match self.locals[path].get(&(s, name)) {
                Some(Class::Imported) => return self.imported(path, name, seen),
                Some(_) => return Binding::Local(path, s, name),
                None => {}
            }
        }
        self.imported(path, name, seen)
    }

    /// What the file's imports bind `name` to. Several imports may bind one name: the latest
    /// in source order decides, whatever it resolves to. An import of every name binds
    /// `name` only if the module takes it.
    fn imported(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        for import in self.files[path].imports.iter().rev() {
            if import.local == name {
                return self.import(path, &import.source, &import.name, seen);
            }
            if matches!(import.name, Imported::All)
                && let Some(module) = self.module(path, &import.source)
                && let found @ (Binding::Def(_) | Binding::Module(_)) =
                    self.star(module, name, seen)
            {
                return found;
            }
        }
        Binding::Unknown
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

    fn import(
        &self,
        path: &'a str,
        source: &str,
        name: &'a Imported,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let found = match (name, self.module(path, source)) {
            (_, None) => Binding::External,
            (Imported::Namespace, Some(module)) => Binding::Module(module),
            (Imported::Name(name) | Imported::Member { name, .. }, Some(module)) => {
                self.export(module, name, seen)
            }
            (Imported::All, Some(_)) => Binding::Unknown,
        };
        if let Imported::Member { module, .. } = name
            && matches!(found, Binding::Unknown | Binding::External)
            && let Some(module) = self.module(path, module)
        {
            return Binding::Module(module);
        }
        found
    }

    /// What a star import of `path` takes as `name`.
    fn star(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        let taken = match &self.files[path].star {
            Star::ExceptDefault => name != "default",
            Star::Public => !name.starts_with('_'),
            Star::Only(names) => names.iter().any(|n| n == name),
        };
        if taken {
            self.export(path, name, seen)
        } else {
            Binding::Unknown
        }
    }

    /// What `path` exports as `name`: all it lets an explicit import name.
    fn export(&self, path: &'a str, name: &'a str, seen: &mut Seen<'a>) -> Binding<'a> {
        if !seen.insert((path, name)) {
            return Binding::Unknown;
        }
        let exports = &self.files[path].exports;
        for export in exports {
            match export {
                Export::Local { exported, local } if exported == name => {
                    return self.lexical(path, "", local, seen);
                }
                Export::From {
                    exported,
                    source,
                    name: imported,
                } if exported == name => return self.import(path, source, imported, seen),
                _ => {}
            }
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
        Binding::Unknown
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
/// none is.
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
    }
}

/// The keyword arguments of a call, as `(name, value)`.
pub type Keywords = [(String, Value)];

/// A call whose arguments bind the parameters of `def`, the function it reaches.
#[derive(Clone, Copy)]
struct Frame<'a> {
    def: Target<'a>,
    path: &'a str,
    index: usize,
}

/// What `Code::origins` follows: a constant, a parameter, a function's return values.
#[derive(PartialEq)]
enum Flow<'a> {
    Const(&'a str, &'a str),
    Param(Target<'a>, usize),
    Returns(Target<'a>),
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
}

impl<'a> Code<'a> {
    pub fn new(resolver: &'a Resolver<'a>, full: &'a BTreeMap<&'a str, &'a Facts>) -> Self {
        Self {
            resolver,
            full,
            misses: RefCell::new(BTreeSet::new()),
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

    /// Whether the pack binds `name` everywhere, as JavaScript does `fetch`.
    pub fn is_global(&self, name: &str) -> bool {
        self.resolver.globals.contains(&name)
    }

    /// What `callee`, written in `scope` of `path`, names outside the repository: the module
    /// and name the file's imports give it (`fastapi.APIRouter`), when an import from outside
    /// the repository binds it.
    pub fn external(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Option<String> {
        let binding =
            (self.resolver).resolve(path, scope, callee, &mut Seen::new(), &mut Work::default());
        matches!(binding, Binding::External).then(|| self.resolver.imported_name(path, callee))
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
        let locals = &self.resolver.locals[function.path];
        let bound = call.callee.contains('.')
            && params.first().is_some_and(|p| {
                locals.get(&(function.name, p.name.as_str())) == Some(&&Class::Enclosing)
            });
        let position = match bound {
            true => n.checked_sub(1),
            false => Some(n),
        };
        position
            .and_then(|at| call.args.get(at))
            .or_else(|| call.keyword(&params.get(n)?.name))
    }

    /// The calls whose result `name`, written in `scope` of `path`, may hold, sorted: followed
    /// through constants and imports, the arguments callers pass to parameters, the values
    /// functions of the repository return and the items of a list or tuple. A call that reaches no such function is where the
    /// value comes from, such as a constructor (`APIRouter()`); whatever else the name may
    /// hold (an assignment that is not its only binding, an attribute) is left out.
    pub fn origins(&self, path: &'a str, scope: &'a str, name: &'a str) -> Vec<(&'a str, usize)> {
        let mut out = BTreeSet::new();
        self.flow_name(
            path,
            scope,
            name,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut out,
        );
        out.into_iter().collect()
    }

    /// `origins` of a value as written.
    pub fn value_origins(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
    ) -> Vec<(&'a str, usize)> {
        let mut out = BTreeSet::new();
        self.flow(
            path,
            scope,
            value,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut out,
        );
        out.into_iter().collect()
    }

    /// `frames` bind the parameters of the functions whose return values are being followed,
    /// innermost last; `busy` is what is being followed, met again only through a cycle.
    fn flow(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        frames: &mut Vec<Frame<'a>>,
        busy: &mut Vec<Flow<'a>>,
        out: &mut BTreeSet<(&'a str, usize)>,
    ) {
        match value {
            Value::Ref(name) => self.flow_name(path, scope, name, frames, busy, out),
            Value::Call(range) => {
                let calls = &self.resolver.files[path].calls;
                let Some(index) = calls.iter().position(|c| c.range == *range) else {
                    return;
                };
                let function = match self.resolver.call(path, &calls[index]) {
                    Resolution::Exact(t) if !self.resolver.is_class(t) => t,
                    _ => {
                        out.insert((path, index));
                        return;
                    }
                };
                let Some(def) = (self.full(function.path))
                    .and_then(|f| f.defs.iter().find(|d| d.name == function.name))
                else {
                    return;
                };
                if busy.contains(&Flow::Returns(function)) {
                    return;
                }
                busy.push(Flow::Returns(function));
                frames.push(Frame {
                    def: function,
                    path,
                    index,
                });
                for returned in &def.returns {
                    self.flow(function.path, function.name, returned, frames, busy, out);
                }
                frames.pop();
                busy.pop();
            }
            Value::List(items) => {
                for item in items {
                    self.flow(path, scope, item, frames, busy, out);
                }
            }
            _ => {}
        }
    }

    fn flow_name(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        frames: &mut Vec<Frame<'a>>,
        busy: &mut Vec<Flow<'a>>,
        out: &mut BTreeSet<(&'a str, usize)>,
    ) {
        let work = &mut Work::default();
        let binding = self
            .resolver
            .resolve(path, scope, name, &mut Seen::new(), work);
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
                if busy.contains(&Flow::Const(at, constant)) {
                    return;
                }
                busy.push(Flow::Const(at, constant));
                let within = constant.rsplit_once('.').map_or("", |(s, _)| s);
                self.flow(at, within, &found.value, frames, busy, out);
                busy.pop();
            }
            Binding::Param(function, n, None) => {
                // Inside a function whose return value is followed, its call binds the
                // parameter; else every call of it does.
                let sites = match frames.last() {
                    Some(frame) if frame.def == function => vec![(frame.path, frame.index)],
                    // ponytail: a parameter of an outer function, captured by a closure, is
                    // not followed.
                    Some(_) => return,
                    None => self
                        .resolver
                        .callers
                        .get(&function)
                        .cloned()
                        .unwrap_or_default(),
                };
                if busy.contains(&Flow::Param(function, n)) {
                    return;
                }
                busy.push(Flow::Param(function, n));
                let frame = frames.pop();
                for (caller, index) in sites {
                    let scope = &self.resolver.files[caller].calls[index].scope;
                    if let Some(arg) = self.arg(caller, index, function, n) {
                        self.flow(caller, scope, arg, frames, busy, out);
                    }
                }
                frames.extend(frame);
                busy.pop();
            }
            _ => {}
        }
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

    /// The definition `name` (also a member chain, such as `views.index`) refers to from
    /// `scope` in `path`.
    pub fn definition(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<Target<'a>> {
        let work = &mut Work::default();
        match self
            .resolver
            .resolve(path, scope, name, &mut Seen::new(), work)
        {
            Binding::Def(t) => Some(t),
            _ => None,
        }
    }

    /// The file `name`, written in `scope` of `path`, refers to as a whole module.
    pub fn module(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<&'a str> {
        let work = &mut Work::default();
        match self
            .resolver
            .resolve(path, scope, name, &mut Seen::new(), work)
        {
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
        let mut out = Vec::new();
        self.walk(path, index, &mut Vec::new(), read, &mut out);
        out
    }

    fn walk<R>(
        &self,
        path: &'a str,
        index: usize,
        frames: &mut Vec<Frame<'a>>,
        read: &dyn Fn(&[Value], &Keywords) -> Option<R>,
        out: &mut Vec<(&'a str, usize, R)>,
    ) {
        // The full call: the resolver's files may be kept from the index, without values.
        let Some(call) = self.call(path, index) else {
            return;
        };
        let mut wants = None;
        let values: Vec<Value> = call
            .args
            .iter()
            .map(|a| self.eval(path, &call.scope, a, frames, 0, &mut Vec::new(), &mut wants))
            .collect();
        let kwargs: Vec<(String, Value)> = (call.kwargs.iter())
            .map(|(k, v)| {
                let v = self.eval(path, &call.scope, v, frames, 0, &mut Vec::new(), &mut wants);
                (k.clone(), v)
            })
            .collect();
        let (site, at) = frames.last().map_or((path, index), |f| (f.path, f.index));
        if let Some(found) = read(&values, &kwargs) {
            out.push((site, at, found));
            return;
        }
        let Some(wrapper) = wants else {
            return;
        };
        if frames.iter().any(|f| f.def == wrapper) {
            return;
        }
        let callers = self.resolver.callers.get(&wrapper);
        let callers = callers.map_or(&[][..], Vec::as_slice);
        if callers.is_empty() {
            let values: Vec<Value> = values.into_iter().map(unbind).collect();
            let kwargs: Vec<(String, Value)> =
                kwargs.into_iter().map(|(k, v)| (k, unbind(v))).collect();
            out.extend(read(&values, &kwargs).map(|found| (site, at, found)));
            return;
        }
        for &(caller, n) in callers {
            frames.push(Frame {
                def: wrapper,
                path: caller,
                index: n,
            });
            self.walk(path, index, frames, read, out);
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
        let binding =
            (self.resolver).resolve(path, scope, name, &mut Seen::new(), &mut Work::default());
        match binding {
            Binding::Const(at, constant) => {
                // `const a = b; const b = a;` folds to nothing.
                if seen.contains(&(at, constant)) {
                    return Value::Unknown;
                }
                let found = self
                    .full(at)
                    .and_then(|f| f.consts.iter().find(|c| c.name == constant));
                let Some(found) = found else {
                    return Value::Unknown;
                };
                let within = constant.rsplit_once('.').map_or("", |(s, _)| s);
                seen.push((at, constant));
                let folded = self.eval(at, within, &found.value, frames, depth, seen, wants);
                seen.pop();
                folded
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
