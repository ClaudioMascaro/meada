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
        self.resolve(path, scope, callee, &mut Seen::new(), &mut Work::default())
    }

    /// `binding` of a path (Rust's `store::Store::new`, `Call::by_path`): its head names a
    /// module or a type, which a local or a parameter of the same name does not hide.
    fn path_binding(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Binding<'a> {
        let (seen, work) = (&mut Seen::new(), &mut Work::default());
        self.resolve_as(path, scope, callee, seen, work, true)
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
        self.resolve_as(path, scope, callee, seen, work, false)
    }

    /// `resolve`, its head read among types and modules alone when `types` (a path).
    fn resolve_as(
        &self,
        path: &'a str,
        scope: &'a str,
        callee: &'a str,
        seen: &mut Seen<'a>,
        work: &mut Work<'a>,
        types: bool,
    ) -> Binding<'a> {
        let (mut binding, members) = self.dotted(path, callee, seen).unwrap_or_else(|| {
            let (head, members) = callee.split_once('.').unwrap_or((callee, ""));
            let binding = match head {
                _ if Some(head) == self.receiver => self.class(path, scope),
                _ if Some(head) == self.base_receiver => Binding::Unknown,
                _ if head.starts_with('(') => Binding::Unknown,
                _ => self.lexical_as(path, scope, head, seen, types),
            };
            (binding, members)
        });
        for member in members.split('.').filter(|m| !m.is_empty()) {
            binding = match self.instance(binding, work) {
                Binding::Def(t) | Binding::Instance(t) if self.is_class(t) => {
                    self.member(t, member, work)
                }
                Binding::Def(t) => match self.def(t.path, &format!("{}.{member}", t.name)) {
                    // A module declared in its file (Rust's `mod store;`) is the file an
                    // import of its name binds: its members are what that file exports.
                    Binding::Unknown if self.defs[t.path][t.name].kind == Kind::Module => {
                        let mut imports = self.files[t.path].imports.iter().rev();
                        match imports.find(|i| i.local == t.name) {
                            Some(i) => match self.import(t.path, &i.source, &i.name, seen) {
                                Binding::Module(m) => self.export(m, member, seen),
                                _ => Binding::Unknown,
                            },
                            None => Binding::Unknown,
                        }
                    }
                    found => found,
                },
                Binding::Module(m) => self.export(m, member, seen),
                Binding::Several(_) => Binding::Unknown,
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
            Class::Imported | Class::Unknown => Binding::Unknown,
        };
        work.busy.pop();
        match found {
            Binding::Def(t) if self.is_class(t) => Binding::Instance(t),
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
        self.lexical_as(path, scope, name, seen, false)
    }

    /// `lexical`, among types and modules alone when `types`: no local, parameter or receiver
    /// (a value) hides a path's head.
    fn lexical_as(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
        seen: &mut Seen<'a>,
        types: bool,
    ) -> Binding<'a> {
        let defs = &self.defs[path];
        let mut scope = Some(scope);
        let mut first = true;
        while let Some(s) = scope {
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
            if types {
                match defs.get_key_value(candidate.as_str()) {
                    Some((name, def)) if def.kind != Kind::Method => {
                        return Binding::Def(Target { path, name });
                    }
                    _ => continue,
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
        source: &'a str,
        name: &'a Imported,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let found = match (name, self.module(path, source)) {
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

    /// What each file exports, for an import of the repository as a package (spec #22), as
    /// `export` reads it: for each name a file exports, the definition, the module-level
    /// constant or type (issue #175) or the whole file it names, or nothing when that is outside
    /// the repository or not known (an import from another package stops there); for each `export * from` a file of the repository, that
    /// file. Sorted.
    pub fn exported(&self) -> Vec<Exported> {
        let mut rows = BTreeSet::new();
        for (&path, facts) in self.files {
            for export in &facts.exports {
                let row = match export {
                    Export::Local { exported, .. } | Export::From { exported, .. } => {
                        match self.export(path, exported, &mut Seen::new()) {
                            Binding::Def(t) => {
                                (Some(exported.as_str()), Some(t.path), Some(t.name))
                            }
                            Binding::Module(m) => (Some(exported.as_str()), Some(m), None),
                            // A constant or a type: the shape a package keeps of it (issue
                            // #175) is found through it; it is no symbol a call reaches.
                            Binding::Const(at, c) => (Some(exported.as_str()), Some(at), Some(c)),
                            _ => (Some(exported.as_str()), None, None),
                        }
                    }
                    Export::All { source } => match self.module(path, source) {
                        Some(m) => (None, Some(m), None),
                        None => continue,
                    },
                };
                rows.insert((path, row));
            }
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
                    Imported::All => return None,
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
        let mut spelled = match name {
            Imported::Namespace => String::new(),
            Imported::Name(name) | Imported::Member { name, .. } => name.clone(),
            Imported::All => return None,
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

    /// Whether the pack binds `name` everywhere, as JavaScript does `fetch`.
    pub fn binds_everywhere(&self, name: &str) -> bool {
        self.resolver.globals.contains(&name)
    }

    /// What `callee`, written in `scope` of `path`, names outside the repository: the module
    /// and name the file's imports give it (`fastapi.APIRouter`), when an import from outside
    /// the repository binds it.
    pub fn external(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Option<String> {
        let binding = self.resolver.binding(path, scope, callee);
        matches!(binding, Binding::External(..)).then(|| self.resolver.imported_name(path, callee))
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
                let Some(index) = calls.iter().position(|c| c.range == call.range) else {
                    return;
                };
                let function = match self.resolver.call(path, &calls[index]) {
                    Resolution::Exact(t) if !self.resolver.is_class(t) => t,
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
            Value::List(items) => {
                for item in items {
                    self.flow(path, scope, item, trail);
                }
            }
            _ => {}
        }
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
}
