//! The shared resolution (ADR 0007): connects each call a language pack found to the
//! definitions it reaches, with Meada's own scope and import resolution and no types.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::lang::{
    Call, Def, Export, Facts, Imported, Kind, LanguagePack, Modules, Param, Part, Value,
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
    /// A whole file, through a namespace import or re-export.
    Module(&'a str),
    /// A `const` that holds no function: its file and qualified name.
    Const(&'a str, &'a str),
    /// The parameter at this position of a function, and the field of its object pattern
    /// when the name is one of those it destructures.
    Param(Target<'a>, usize, Option<usize>),
    /// Something an import brings from outside the repository: the specifier imported from
    /// and what it names there.
    External(&'a str, &'a Imported),
    /// Neither scope nor import resolution knows it.
    Unknown,
}

/// Seen `(file, exported name)` pairs: re-exports may form cycles.
type Seen<'a> = HashSet<(&'a str, &'a str)>;

/// The files of one language pack, resolved together.
pub struct Resolver<'a> {
    files: &'a BTreeMap<&'a str, &'a Facts>,
    defs: HashMap<&'a str, HashMap<&'a str, &'a Def>>,
    consts: HashMap<&'a str, HashSet<&'a str>>,
    /// The call sites whose single target (exact or inferred) is each definition, sorted.
    callers: HashMap<Target<'a>, Vec<(&'a str, usize)>>,
    /// Name inference candidates, sorted: methods by their last name (`true`), top-level
    /// functions and classes by their name (`false`).
    by_name: HashMap<(bool, &'a str), Vec<Target<'a>>>,
    modules: Modules,
}

impl<'a> Resolver<'a> {
    pub fn new(files: &'a BTreeMap<&'a str, &'a Facts>, modules: Modules) -> Self {
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
            callers: HashMap::new(),
            by_name,
            modules,
        };
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
        let mut segments = call.callee.split('.');
        let head = segments.next().unwrap_or("");
        let seen = &mut Seen::new();
        let mut binding = match head {
            "this" => self.class(path, &call.scope),
            "super" => Binding::Unknown,
            _ if head.starts_with('(') => Binding::Unknown,
            _ => self.lexical(path, &call.scope, head, seen),
        };
        for member in segments {
            binding = match binding {
                Binding::Def(t) => self.def(t.path, &format!("{}.{member}", t.name)),
                Binding::Module(m) => self.export(m, member, seen),
                other => other,
            };
        }
        match binding {
            Binding::Def(t) => Resolution::Exact(t),
            Binding::External(..) => Resolution::Unresolved,
            Binding::Module(_) | Binding::Const(..) | Binding::Param(..) | Binding::Unknown => {
                self.infer(&call.callee)
            }
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
    /// defines, its constants, its parameters), then the top level, then the file's imports.
    /// Methods are not lexical bindings.
    fn lexical(
        &self,
        path: &'a str,
        scope: &str,
        name: &'a str,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let defs = &self.defs[path];
        let mut scope = Some(scope);
        while let Some(s) = scope {
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
            scope = (!s.is_empty()).then(|| s.rsplit_once('.').map_or("", |(parent, _)| parent));
        }
        match self.files[path].imports.iter().find(|i| i.local == name) {
            Some(import) => self.import(path, &import.source, &import.name, seen),
            None => Binding::Unknown,
        }
    }

    fn import(
        &self,
        path: &'a str,
        source: &'a str,
        name: &'a Imported,
        seen: &mut Seen<'a>,
    ) -> Binding<'a> {
        let Some(module) = self.module(path, source) else {
            return Binding::External(source, name);
        };
        match name {
            Imported::Namespace => Binding::Module(module),
            Imported::Name(name) => self.export(module, name, seen),
        }
    }

    /// What `path` exports as `name`.
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
        if name != "default" {
            for export in exports {
                if let Export::All { source } = export
                    && let Some(module) = self.module(path, source)
                    && let found @ (Binding::Def(_) | Binding::Module(_)) =
                        self.export(module, name, seen)
                {
                    return found;
                }
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

/// A call whose arguments bind the parameters of `def`, the function it reaches.
#[derive(Clone, Copy)]
struct Frame<'a> {
    def: Target<'a>,
    path: &'a str,
    index: usize,
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
                    Imported::Name(name) => name.clone(),
                },
                binding: None,
            },
            Binding::Const(at, constant) => {
                // `const a = b; const b = a;` comes from nowhere.
                if seen.contains(&(at, constant)) {
                    return None;
                }
                seen.push((at, constant));
                let value = &self
                    .full(at)?
                    .consts
                    .iter()
                    .find(|c| c.name == constant)?
                    .value;
                let within = constant.rsplit_once('.').map_or("", |(s, _)| s);
                let mut origin = match value {
                    Value::Ref(name) => self.trace(at, within, name, seen)?,
                    Value::Call(callee) => {
                        let mut origin = self.trace(at, within, callee, seen)?;
                        origin.expr += "()";
                        origin
                    }
                    Value::New(callee) => {
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
        Some(self.full(path)?.calls.get(index)?.args.as_slice())
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
            Binding::Const(at, constant) => {
                let facts = self.full(at)?;
                let value = &facts.consts.iter().find(|c| c.name == constant)?.value;
                Some(((at, constant), value))
            }
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

    /// The definition `name` refers to from `scope` in `path`.
    pub fn definition(&self, path: &'a str, scope: &'a str, name: &'a str) -> Option<Target<'a>> {
        match self.resolver.lexical(path, scope, name, &mut Seen::new()) {
            Binding::Def(t) => Some(t),
            _ => None,
        }
    }

    /// What `read` makes of the arguments of the call at `index` of `path`, at each call site
    /// that determines them. `read` answers `None` when what it needs holds a `Ref`: a
    /// parameter of the enclosing function, which makes that function a client wrapper. Each
    /// call site of the wrapper is then read with its own arguments substituted, through any
    /// number of wrapper levels. A wrapper reached again through its own callers (a cycle)
    /// adds nothing; a wrapper nobody calls is read where it stands, its parameters unknown.
    // ponytail: every simple path up the wrappers' callers is walked; memoize per (wrapper,
    // call site) if a dense call graph makes this slow.
    pub fn expand<R>(
        &self,
        path: &'a str,
        index: usize,
        read: &dyn Fn(&[Value]) -> Option<R>,
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
        read: &dyn Fn(&[Value]) -> Option<R>,
        out: &mut Vec<(&'a str, usize, R)>,
    ) {
        let call = self.resolver.files[path].calls.get(index);
        let (Some(call), Some(args)) = (call, self.args(path, index)) else {
            return;
        };
        let mut wants = None;
        let values: Vec<Value> = args
            .iter()
            .map(|a| self.eval(path, &call.scope, a, frames, 0, &mut Vec::new(), &mut wants))
            .collect();
        let (site, at) = frames.last().map_or((path, index), |f| (f.path, f.index));
        if let Some(found) = read(&values) {
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
            out.extend(read(&values).map(|found| (site, at, found)));
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
                items
                    .iter()
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
        match self.resolver.lexical(path, scope, name, &mut Seen::new()) {
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
                    let Some(args) = self.args(frame.path, frame.index) else {
                        return Value::Unknown;
                    };
                    let param = self.param(function, n);
                    let bound = match (args.get(n), param.and_then(|p| p.default.as_ref())) {
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
