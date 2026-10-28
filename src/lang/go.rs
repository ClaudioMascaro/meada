//! The Go language pack (spec #25). It reads `.go` files and the `go.mod` files of a repository
//! with no Go toolchain: an import names a package directory through the module path of the
//! `go.mod` that owns it (`modfile.rs`), the files of one package share their top-level scope
//! (`Layout::shared`), and build constraints are evaluated for one fixed target (`build.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};

use super::{
    Binds, Call, Class, Const, Def, Export, Facts, Import, Imported, Kind, LanguagePack, Layout,
    Modules, Param, Receiver, Value, children, named, text,
};
use crate::resolve::Code;
use crate::{Result, parent, qualify};

/// What `name`, written in `scope` of `path`, names in a package outside the repository when the
/// head of the name is the import itself (`http.Get`, `kafka.NewReader`), spelled as the imports
/// name it. `Code::outside` filtered on `direct`: a method of a typed local (`c.Get` with
/// `c *http.Client`) is not a function of the package.
pub(crate) fn package_call<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    name: &'a str,
) -> Option<String> {
    let outside = code.outside(path, scope, name, false)?;
    outside.direct.then_some(outside.spelled)
}

mod build;
pub mod modfile;
mod values;
use values::Keep;

/// How many segments of a member chain a callee keeps, and how many wrappers (`*`, `[T]`,
/// parentheses) a receiver's type is read through; a longer one is its node kind, or names no
/// type.
// ponytail: real chains hold a handful of segments; the limit keeps a crafted one cheap.
const PATH_DEPTH: usize = 256;

/// How many function literals may nest in one another as definitions of their own. Names are
/// qualified by the literals around them and each call resolves its names up that chain, so
/// unbounded nesting costs cubic time. Past the limit a literal is no definition: its
/// parameters and calls belong to the one around it.
// ponytail: real code nests a few levels; 128 is the depth the other packs allow.
const DEFINITION_DEPTH: usize = 128;

pub struct Go;

impl LanguagePack for Go {
    fn extensions(&self) -> &'static [&'static str] {
        &["go"]
    }

    fn parse(&self, path: &str, src: &[u8]) -> Result<Facts> {
        parse(path, src)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["**/go.mod"]
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        Ok(self.layout(configs, files)?.modules)
    }

    fn layout(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Result<Layout> {
        let built: BTreeMap<&str, &Facts> = (files.iter())
            .filter(|(_, f)| !f.unbuilt)
            .map(|(p, f)| (*p, *f))
            .collect();
        // The files of one directory that declare one package share a scope.
        let mut packages: BTreeMap<(&str, &str), Vec<String>> = BTreeMap::new();
        for (path, facts) in &built {
            let key = (dir(path), facts.package.as_str());
            packages.entry(key).or_default().push(path.to_string());
        }
        // What an import of a directory names: its first file but the tests (which no other
        // package imports), which stands for the package (`Layout::shared`).
        let mut importable: BTreeMap<&str, &str> = BTreeMap::new();
        for path in built.keys().filter(|p| !p.ends_with("_test.go")) {
            importable.entry(dir(path)).or_insert(path);
        }
        let importable: BTreeMap<String, String> = (importable.into_iter())
            .map(|(d, p)| (d.to_string(), p.to_string()))
            .collect();
        let modules = modfile::Modules::new(configs);
        Ok(Layout {
            members: Some(built.keys().map(|p| p.to_string()).collect()),
            modules: Box::new(move |from, specifier| {
                (modules.dir(dir(from), specifier))
                    .and_then(|d| importable.get(&d))
                    .into_iter()
                    .cloned()
                    .collect()
            }),
            published: None,
            programs: Vec::new(),
            reaches: None,
            shared: packages.into_values().filter(|p| p.len() > 1).collect(),
        })
    }
}

/// The directory of a repository-relative path, empty at the root.
fn dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// What the walk of a declaration's body records.
struct Walk<'s> {
    src: &'s [u8],
    facts: Facts,
    /// The locals recorded so far, so each is listed once.
    locals: BTreeSet<String>,
    /// Every binding of a name in a scope, with the class the source writes for it (`None` when
    /// it writes none) and the bytes it holds for, when that is less than the whole scope (a
    /// `:=` or `var`, from its end to the end of its block). A name all its bindings give one
    /// class is a receiver (`finish`).
    bound: BTreeMap<(String, String), Vec<Binding>>,
    /// The type parameters of the type declarations, as `Type.Param`: they hide a name in the
    /// declaration alone, not in the methods.
    type_scoped: BTreeSet<String>,
    /// The plain names a statement of a function's own block binds to one value (`r :=
    /// gin.Default()`), by scope: constants when nothing else binds or assigns the name.
    held: Vec<(String, String, Value)>,
    /// The names assigned again, with the scope the assignment is written in and what it does
    /// to a name bound to an environment read (`keeps_env`).
    assigned: Vec<(String, String, Keep)>,
    /// The single value each `return` of a definition gives.
    returns: HashMap<String, Vec<Value>>,
    /// The standard-library packages whose calls and constants the values fold.
    std: values::Std,
    /// The package-level `var`s, which hold their value only if nothing assigns them.
    vars: BTreeSet<String>,
    /// The field names written through a selector (`cfg.Base = ..`, `cfg.Base++`, `&cfg.Base`),
    /// whichever package the selector reaches.
    selected: BTreeSet<String>,
    /// The scopes that write through a dereference (`*p = ..`, `*p.Topic = ..`), with every
    /// scope around them.
    derefs: HashSet<String>,
    /// The second name of each `v, ok := os.LookupEnv(..)`, by the scope and the first name.
    lookups: HashMap<(String, String), String>,
}

/// A binding of a name: the class the source writes for it, and where it holds.
type Binding = (Option<Class>, Option<Range<usize>>);

impl Walk<'_> {
    /// A name bound in `scope` to a value of no class the source writes.
    fn local(&mut self, scope: &str, name: &str) {
        self.bind_as(scope, name, None, true, None);
    }

    /// A name bound in `scope`, holding `class` when the source writes it. A `local` hides the
    /// outer names it spells; a parameter or a field is no local.
    fn bind_as(
        &mut self,
        scope: &str,
        name: &str,
        class: Option<Class>,
        local: bool,
        region: Option<Range<usize>>,
    ) {
        if name == "_" {
            return;
        }
        if local && self.locals.insert(qualify(scope, name)) {
            self.facts.locals.push(qualify(scope, name));
        }
        (self.bound.entry((scope.into(), name.into())).or_default()).push((class, region));
    }

    /// The names a parameter list binds, with the type each writes: parameters (`local` false,
    /// which `Def::params` lists), or the receiver and the named results of a function.
    fn typed_params(&mut self, scope: &str, list: Option<Node>, local: bool) {
        for param in list.into_iter().flat_map(named) {
            let ty = param.child_by_field_name("type");
            let ty = ty.filter(|_| param.kind() == "parameter_declaration");
            let class = ty.and_then(|t| type_name(t, self.src)).map(Class::Type);
            for name in names(param) {
                self.bind_as(scope, text(name, self.src), class.clone(), local, None);
            }
        }
    }

    /// The names a `var` or `:=` binds, each with the class its type or its value writes: the
    /// type, else the value at its place (`a, b := T{}, U{}`), else, of one call giving several
    /// values, the first.
    fn assigned(
        &mut self,
        scope: &str,
        left: &[Node],
        ty: Option<Node>,
        values: Option<Node>,
        region: Option<(usize, usize)>,
    ) {
        let src = self.src;
        let values: Vec<Node> = values.into_iter().flat_map(named).collect();
        let written = ty.and_then(|t| type_name(t, src)).map(Class::Type);
        for (at, name) in left.iter().enumerate() {
            let class = match values.len() {
                _ if ty.is_some() => written.clone(),
                n if n == left.len() => value_class(values[at], src),
                1 if at == 0 => value_class(values[0], src),
                _ => None,
            };
            let region = region.map(|(from, to)| from..to);
            self.bind_as(scope, text(*name, src), class, region.is_some(), region);
        }
    }

    /// Notes that `scope` writes through a dereference when one of `targets` is `*p`.
    fn dereference(&mut self, scope: &str, targets: &[Node]) {
        let src = self.src;
        let deref = |t: &&Node| {
            t.kind() == "unary_expression"
                && (t.child_by_field_name("operator")).is_some_and(|o| text(o, src) == "*")
        };
        if targets.iter().any(|t| deref(&t)) {
            let mut at = scope;
            while !at.is_empty() && self.derefs.insert(at.to_string()) {
                at = crate::parent(at);
            }
        }
    }

    /// Records the writes to `targets`: a name assigned again in `scope`, and the field name of
    /// a selector, which any package may write (`config.Base = ..`).
    fn write(&mut self, scope: &str, targets: &[Node], keeps: Keep) {
        for target in targets {
            let name = match target.kind() {
                "identifier" => text(*target, self.src),
                "selector_expression" => {
                    let Some(field) = target.child_by_field_name("field") else {
                        continue;
                    };
                    self.selected.insert(text(field, self.src).to_string());
                    continue;
                }
                _ => continue,
            };
            let name = name.to_string();
            self.assigned.push((scope.to_string(), name, keeps.clone()));
        }
    }

    /// Whether `scope`, or a scope around or inside it, writes through a dereference.
    fn writes_through(&self, scope: &str) -> bool {
        let mut at = scope;
        while !at.is_empty() {
            if self.derefs.contains(at) {
                return true;
            }
            at = crate::parent(at);
        }
        false
    }

    /// Makes the names held by one binding constants, and gives each definition what it
    /// returns. A name bound again anywhere in its function, or assigned in it or in a literal
    /// inside it, holds no one value.
    fn finish_values(&mut self) {
        let mut assigned = HashSet::new();
        // The names assigned that no scope of the file binds: a variable another file of the
        // package declares.
        let mut elsewhere = BTreeSet::new();
        // The names bound to an environment read: an assignment that keeps the read leaves them
        // constants.
        let env: HashSet<String> = (self.held.iter())
            .filter(|(_, _, value)| values::is_env(value))
            .map(|(scope, name, _)| qualify(scope, name))
            .collect();
        // The binding a name written in a scope refers to: the innermost scope that binds it.
        let bound = &self.bound;
        let binder = |scope: &str, name: &str| {
            let mut at = scope;
            loop {
                if bound.contains_key(&(at.to_string(), name.to_string())) {
                    break Some(at.to_string());
                }
                if at.is_empty() {
                    break None;
                }
                at = crate::parent(at);
            }
        };
        // The fallbacks of names bound to a read: the binding, the name, the scope, the default
        // and its guard.
        let mut defaults = Vec::new();
        for (scope, name, keeps) in std::mem::take(&mut self.assigned) {
            let found = binder(&scope, &name).map(|at| qualify(&at, &name));
            // The address of a topic: a write only when the scope writes through a pointer.
            if matches!(keeps, Keep::Pointer) && !self.writes_through(&scope) {
                continue;
            }
            match (found, keeps) {
                (Some(found), Keep::Trim) if env.contains(&found) => {}
                (Some(found), Keep::Default(value, guard)) if env.contains(&found) => {
                    defaults.push((found, name, scope, value, guard));
                }
                (Some(found), _) => _ = assigned.insert(found),
                (None, _) if name != "_" => _ = elsewhere.insert(name),
                (None, _) => {}
            }
        }
        // A guard holds when no binding of the file hides `len`, and when `ok` is the second
        // result of the `os.LookupEnv` that bound the read, bound once and never assigned.
        // ponytail: a package-level `len` another file of the package declares is not seen.
        let hides_len = (self.bound.keys()).any(|(_, n)| n == "len")
            || (self.facts.defs.iter()).any(|d| d.name == "len")
            || (self.facts.consts.iter()).any(|c| c.name == "len");
        let guarded = |found: &str, name: &str, scope: &str, guard: &values::Guard| {
            let Some(ok) = &guard.ok else {
                return !(guard.len && hides_len);
            };
            let at = binder(scope, ok);
            let paired = (at.as_deref()).filter(|at| qualify(at, name) == found);
            let lookup = paired.and_then(|at| self.lookups.get(&(at.to_string(), name.into())));
            !(guard.len && hides_len)
                && lookup == Some(ok)
                && paired.is_some_and(|at| {
                    let once = bound.get(&(at.to_string(), ok.clone())).map(Vec::len) == Some(1);
                    once && !assigned.contains(&qualify(at, ok))
                })
        };
        let (kept, broken): (Vec<_>, Vec<_>) = (defaults.into_iter())
            .partition(|(found, name, scope, _, guard)| guarded(found, name, scope, guard));
        assigned.extend(broken.into_iter().map(|(found, ..)| found));
        let defaults: Vec<(String, String, Value)> = (kept.into_iter())
            .map(|(found, _, scope, value, _)| (found, scope, value))
            .collect();
        // A `var` assigned again holds no one value. One assigned in another file is declared
        // there and here (as a constant of no value), so the package binds the name to neither.
        for constant in &mut self.facts.consts {
            if self.vars.contains(&constant.name) && assigned.contains(&constant.name) {
                constant.value = Value::Unknown;
            }
        }
        // The markers `Resolver::redeclared` reads: a package variable, and a field written
        // through a selector, which makes a variable of that name no constant in any package.
        let marks = (self.vars.iter()).map(|v| format!("{v}.var"));
        let marks = marks.chain(self.selected.iter().map(|f| format!(".{f}")));
        for name in marks {
            let value = Value::Unknown;
            self.facts.consts.push(Const {
                name,
                value,
                ty: None,
                scope: None,
            });
        }
        for name in elsewhere {
            let value = Value::Unknown;
            self.facts.consts.push(Const {
                name,
                value,
                ty: None,
                scope: None,
            });
        }
        let mut constants = HashSet::new();
        for (scope, name, value) in std::mem::take(&mut self.held) {
            let local = qualify(&scope, &name);
            if self.bound.get(&(scope.clone(), name.clone())).map(Vec::len) == Some(1)
                && !assigned.contains(&local)
            {
                constants.insert(local.clone());
                self.facts.consts.push(Const {
                    name: local,
                    value,
                    ty: None,
                    scope: None,
                });
            }
        }
        // A fallback leaves the name its read only when the default is plain, which the
        // constants of the file now tell.
        if !defaults.is_empty() {
            let table: HashMap<&str, &Value> = (self.facts.consts.iter())
                .filter(|c| !self.vars.contains(&c.name))
                .map(|c| (c.name.as_str(), &c.value))
                .collect();
            let rejected: HashSet<String> = (defaults.into_iter())
                .filter(|(_, scope, value)| !values::plain(value, scope, &table))
                .map(|(found, _, _)| found)
                .collect();
            self.facts.consts.retain(|c| !rejected.contains(&c.name));
            constants.retain(|c| !rejected.contains(c));
        }
        // A constant is a name that holds its value, not a local hiding what it is bound to.
        self.facts.locals.retain(|l| !constants.contains(l));
        let mut returns = std::mem::take(&mut self.returns);
        for def in &mut self.facts.defs {
            def.return_values = returns.remove(&def.name).unwrap_or_default();
        }
    }

    /// The identifiers of an expression list on the left of `:=`, or of a name field.
    fn bind(&mut self, scope: &str, node: Node) {
        let src = self.src;
        match node.kind() {
            "identifier" => self.local(scope, text(node, src)),
            _ => {
                for n in named(node).filter(|n| n.kind() == "identifier") {
                    self.local(scope, text(n, src));
                }
            }
        }
    }

    /// The names a parameter list binds, of no class the source writes.
    fn bind_params(&mut self, scope: &str, list: Option<Node>) {
        for param in list.into_iter().flat_map(named) {
            for name in names(param) {
                self.local(scope, text(name, self.src));
            }
        }
    }

    /// The type parameters a generic function declares (`[T any]`).
    fn bind_type_params(&mut self, scope: &str, node: Node) {
        for name in type_params(node) {
            self.local(scope, text(name, self.src));
        }
    }

    /// The type parameters a type declaration declares: names within the declaration alone.
    fn bind_type_scope(&mut self, ty: &str, node: Node) {
        for name in type_params(node) {
            self.type_scoped.insert(qualify(ty, text(name, self.src)));
        }
    }

    /// Records the calls and the bindings under `node`, which belong to `scope`. A function
    /// literal is a `Kind::Inline` definition (`@<offset>` under `scope`) that the index names
    /// or folds into the one around it: its parameters bind in it, and its calls are its own.
    fn body(&mut self, scope: &str, node: Node) {
        let src = self.src;
        // Explicit stack instead of recursion: generated code can nest deeper than the stack.
        let mut stack: Vec<(Node, Rc<str>, usize)> = vec![(node, scope.into(), 0)];
        while let Some((node, scope, depth)) = stack.pop() {
            let mut inner_scope = Rc::clone(&scope);
            let mut inner_depth = depth;
            let scope = &*scope;
            match node.kind() {
                "call_expression" => {
                    if let Some(function) = node.child_by_field_name("function") {
                        let mut at = call(scope, callee(function, src), node.byte_range());
                        at.args = values::arguments(node)
                            .map(|a| values::value(a, src, &self.std, scope, depth))
                            .collect();
                        at.receiver = values::receiver(function, src, &self.std, scope, depth);
                        self.facts.calls.push(at);
                    }
                }
                "return_statement" => {
                    let results: Vec<Node> = named(node).flat_map(named).collect();
                    if let [result] = results[..] {
                        let value = values::value(result, src, &self.std, scope, depth);
                        (self.returns.entry(scope.to_string()).or_default()).push(value);
                    }
                }
                "short_var_declaration" => {
                    if let Some(left) = node.child_by_field_name("left") {
                        let left = identifiers(left);
                        let right = node.child_by_field_name("right");
                        let region = Some(region(node));
                        self.assigned(scope, &left, None, right, region);
                        if in_body(node) {
                            self.hold(scope, &left, right, depth);
                        }
                    }
                }
                "range_clause" | "receive_statement"
                    if children(node).any(|c| c.kind() == ":=") =>
                {
                    if let Some(left) = node.child_by_field_name("left") {
                        self.bind(scope, left);
                    }
                }
                // `r = gin.New()`, `for r = range rs`: the name is assigned again.
                "assignment_statement" | "range_clause" | "receive_statement" => {
                    if let Some(left) = node.child_by_field_name("left") {
                        let targets: Vec<Node> = named(left).chain([left]).collect();
                        let keeps = match node.kind() {
                            "assignment_statement" => {
                                values::keeps_env(node, src, &self.std, scope, depth)
                            }
                            _ => Keep::No,
                        };
                        self.dereference(scope, &targets);
                        self.write(scope, &targets, keeps);
                    }
                }
                "inc_statement" | "dec_statement" => {
                    let targets: Vec<Node> = named(node).collect();
                    self.dereference(scope, &targets);
                    self.write(scope, &targets, Keep::No);
                }
                "var_spec" | "const_spec" => {
                    let ty = node.child_by_field_name("type");
                    let value = node.child_by_field_name("value");
                    self.assigned(scope, &names(node), ty, value, Some(region(node)));
                    if node.kind() == "var_spec" && in_body(node) {
                        self.hold(scope, &names(node), value, depth);
                    }
                }
                "type_spec" | "type_alias" => {
                    for name in names(node) {
                        self.local(scope, text(name, src));
                    }
                    if node.kind() == "var_spec" && in_body(node) {
                        let right = node.child_by_field_name("value");
                        self.hold(scope, &names(node), right, depth);
                    }
                }
                // `&p`: the name may be written through the pointer, so it holds no one value.
                // ponytail: except as the `Topic` of a `TopicPartition` literal (confluent-kafka-go's
                // `*string`), unless the function writes through a dereference; no alias analysis.
                "composite_literal" => {
                    let ty = node.child_by_field_name("type");
                    if let Some(body) = node.child_by_field_name("body") {
                        self.literal(scope, ty, body, depth, 0);
                    }
                }
                "unary_expression" => {
                    let operand = node.child_by_field_name("operand");
                    let address = node.child_by_field_name("operator");
                    if let (Some(operand), Some(address)) = (operand, address)
                        && text(address, src) == "&"
                    {
                        let keeps = match is_topic_pointer(node, src) {
                            true => Keep::Pointer,
                            false => Keep::No,
                        };
                        self.write(scope, &[operand], keeps);
                    }
                }
                "type_switch_statement" => {
                    if let Some(alias) = node.child_by_field_name("alias") {
                        self.bind(scope, alias);
                    }
                }
                "func_literal" if depth < DEFINITION_DEPTH => {
                    let name = self.inline(scope, node);
                    (inner_scope, inner_depth) = (name.into(), depth + 1);
                }
                // Too deep to be a definition: it lends its names to the one around it.
                "func_literal" => {
                    self.bind_params(scope, node.child_by_field_name("parameters"));
                    self.bind_params(scope, result_list(node));
                }
                _ => {}
            }
            let mut inner: Vec<Node> = named(node).collect();
            inner.reverse();
            stack.extend((inner.into_iter()).map(|n| (n, Rc::clone(&inner_scope), inner_depth)));
        }
    }

    /// Turns the names whose every binding writes one class into receivers, and drops the written
    /// results and receivers whose type a local hides: the shared side reads a type among types
    /// and modules, so a local type of the same name (or a type parameter) would be missed.
    fn finish(&mut self) {
        let type_scoped = std::mem::take(&mut self.type_scoped);
        let hides = |locals: &BTreeSet<String>, scope: &str, class: &Class| {
            let Class::Type(ty) = class else {
                return false;
            };
            let head = ty.split('.').next().unwrap_or(ty);
            let mut scope = scope;
            if type_scoped.contains(&qualify(scope, head)) {
                return true;
            }
            loop {
                if locals.contains(&qualify(scope, head)) {
                    return true;
                }
                if scope.is_empty() {
                    return false;
                }
                scope = parent(scope);
            }
        };
        // The method calls by the name they start at, with their scope and place, in order of
        // scope: a name held for part of its scope is a receiver only if every call it starts
        // in the scope (or in a literal in it) is inside that part.
        let mut called: HashMap<&str, Vec<(&str, usize)>> = HashMap::new();
        for call in &self.facts.calls {
            if let Some((head, _)) = call.callee.split_once('.')
                && !head.starts_with('(')
            {
                (called.entry(head).or_default()).push((&call.scope, call.range.start));
            }
        }
        called.values_mut().for_each(|v| v.sort_unstable());
        let mut taken = BTreeSet::new();
        for ((scope, name), events) in std::mem::take(&mut self.bound) {
            let Some((Some(class), _)) = events.first() else {
                continue;
            };
            if events.iter().any(|e| e.0.as_ref() != Some(class))
                || hides(&self.locals, &scope, class)
            {
                continue;
            }
            if events.iter().all(|e| e.1.is_some()) {
                // The farthest end reached by the regions starting at or before each place.
                let mut held: Vec<(usize, usize)> = (events.iter())
                    .filter_map(|e| e.1.as_ref().map(|r| (r.start, r.end)))
                    .collect();
                held.sort_unstable();
                let mut reach = 0;
                let held: Vec<(usize, usize)> = (held.into_iter())
                    .map(|(from, to)| {
                        reach = reach.max(to);
                        (from, reach)
                    })
                    .collect();
                let holds = |place: usize| {
                    let at = held.partition_point(|h| h.0 <= place);
                    at > 0 && held[at - 1].1 > place
                };
                // The calls of the scope and of the literals in it come together in order.
                let calls = called.get(name.as_str()).map_or(&[][..], Vec::as_slice);
                let from = calls.partition_point(|c| c.0 < scope.as_str());
                let own = |at: &str| {
                    at == scope || (at.starts_with(&scope) && at[scope.len()..].starts_with('.'))
                };
                let calls = calls[from..].iter().take_while(|c| own(c.0));
                if calls.into_iter().any(|c| !holds(c.1)) {
                    continue;
                }
            }
            taken.insert(qualify(&scope, &name));
            let class = class.clone();
            self.facts.receivers.push(Receiver { scope, name, class });
        }
        self.facts.locals.retain(|l| !taken.contains(l));
        for def in &mut self.facts.defs {
            if def
                .result
                .as_ref()
                .is_some_and(|c| hides(&self.locals, &def.name, c))
            {
                def.result = None;
            }
        }
    }

    /// Records what the literal `body` of the type `ty` (`None`: written without it inside
    /// another literal, `[]T{{..}}`), written in `scope`, sets each field to, for
    /// `Code::field_values`: a marker constant `{field}` holding `New(type, [value])`, `{*}` for
    /// the fields a struct sets by position, and the type `?` when it is not read. The literals
    /// written without their type inside a slice, an array or a map are of its element, key or
    /// value type; inside any other literal, of a type not read.
    fn literal<'t>(
        &mut self,
        scope: &str,
        ty: Option<Node<'t>>,
        body: Node<'t>,
        depth: usize,
        level: usize,
    ) {
        let src = self.src;
        let marker = |ty: Option<&str>, field: Option<&str>, value: Value| Const {
            name: qualify(scope, &format!("{{{}}}", field.unwrap_or("*"))),
            value: Value::New(ty.unwrap_or("?").to_string(), vec![value]),
            ty: None,
            scope: None,
        };
        // ponytail: a literal nested deeper than the value depth sets fields of a type not read.
        if level >= values::VALUE_DEPTH {
            self.facts.consts.push(marker(None, None, Value::Unknown));
            return;
        }
        let child = |n: Option<Node<'t>>, name| n.and_then(|n| n.child_by_field_name(name));
        // The literals written without their type inside this one, with the type each is of.
        let mut inside: Vec<(Option<Node<'t>>, Node<'t>)> = Vec::new();
        let elided = |n: Option<Node<'t>>| -> Option<Node<'t>> {
            (n.and_then(|n| named(n).next())).filter(|n| n.kind() == "literal_value")
        };
        let (keys, values) = match ty.map(|t| t.kind()) {
            Some("slice_type" | "array_type" | "implicit_length_array_type") => {
                (None, child(ty, "element"))
            }
            Some("map_type") => (child(ty, "key"), child(ty, "value")),
            // An anonymous struct: its fields are no named type's.
            Some("struct_type") => return,
            _ => {
                let name = ty.and_then(|t| type_name(t, src));
                for e in named(body) {
                    let keyed = e.kind() == "keyed_element";
                    let key = (keyed.then(|| e.child_by_field_name("key"))).flatten();
                    let key = key.and_then(|k| named(k).next());
                    let value = if keyed {
                        e.child_by_field_name("value")
                    } else {
                        Some(e)
                    };
                    if let Some(v) = elided(value) {
                        inside.push((None, v));
                        continue;
                    }
                    let value = value.and_then(|v| named(v).next());
                    let set = match (key, value) {
                        (Some(k), Some(v)) if k.kind() == "identifier" => {
                            let value = values::value(v, src, &self.std, scope, depth);
                            marker(name.as_deref(), Some(text(k, src)), value)
                        }
                        (Some(_), _) => continue,
                        (None, _) => marker(name.as_deref(), None, Value::Unknown),
                    };
                    self.facts.consts.push(set);
                }
                (None, None)
            }
        };
        if keys.is_some() || values.is_some() {
            for e in named(body) {
                let (key, value) = match e.kind() {
                    "keyed_element" => {
                        (e.child_by_field_name("key"), e.child_by_field_name("value"))
                    }
                    _ => (None, Some(e)),
                };
                inside.extend(elided(key).map(|k| (keys, k)));
                inside.extend(elided(value).map(|v| (values, v)));
            }
        }
        for (ty, body) in inside {
            self.literal(scope, ty, body, depth, level + 1);
        }
    }

    /// Holds the value of `right` for the name `left` binds, when one name is bound to one
    /// value.
    fn hold(&mut self, scope: &str, left: &[Node], right: Option<Node>, depth: usize) {
        let src = self.src;
        let left: Vec<Node> = match left {
            [list] if list.kind() == "expression_list" => named(*list).collect(),
            left => left.to_vec(),
        };
        let right: Vec<Node> = (right.into_iter())
            .flat_map(|r| match r.kind() {
                "expression_list" => named(r).collect(),
                _ => vec![r],
            })
            .collect();
        // `v, ok := os.LookupEnv("X")`: the first result is the read, the other the name `ok`.
        let lookup = match (&left[..], &right[..]) {
            ([name, ok], [call]) => {
                let read = values::lookup_env(*call, src, &self.std, scope, depth);
                if read.is_some() && ok.kind() == "identifier" {
                    let key = (scope.to_string(), text(*name, src).to_string());
                    self.lookups.insert(key, text(*ok, src).to_string());
                }
                read.map(|value| (name, value))
            }
            _ => None,
        };
        let one = match (&left[..], &right[..]) {
            ([name], [value]) => Some((name, values::value(*value, src, &self.std, scope, depth))),
            _ => None,
        };
        if let Some((name, value)) = lookup.or(one)
            && name.kind() == "identifier"
            && text(*name, src) != "_"
        {
            let name = text(*name, src).to_string();
            self.std.note(scope, &name, &value);
            self.held.push((scope.to_string(), name, value));
        }
    }

    /// The `Kind::Inline` definition of the function `node` (a literal, or a blank `func _`)
    /// written in `scope`, with its parameters, and the names its results bind: its name.
    fn inline(&mut self, scope: &str, node: Node) -> String {
        let name = qualify(scope, &format!("@{}", node.start_byte()));
        let list = node.child_by_field_name("parameters");
        let params = params(list, self.src);
        (self.facts.defs).push(def(name.clone(), Kind::Inline, node.byte_range(), params));
        self.typed_params(&name, list, false);
        self.typed_params(&name, result_list(node), true);
        self.bind_type_params(&name, node);
        name
    }
}

/// Whether the declaration `node` is a statement of a function's own body, not of a block
/// inside it: a name it binds is visible in the rest of the function, whatever else the function
/// does.
fn in_body(node: Node) -> bool {
    let mut at = node.parent();
    while let Some(parent) = at.filter(|p| matches!(p.kind(), "var_spec_list" | "var_declaration"))
    {
        at = parent.parent();
    }
    let block = (at.filter(|p| p.kind() == "statement_list"))
        .and_then(|p| p.parent())
        .filter(|b| b.kind() == "block");
    let function = block.and_then(|b| b.parent());
    function.is_some_and(|f| {
        matches!(
            f.kind(),
            "function_declaration" | "method_declaration" | "func_literal"
        )
    })
}

/// Whether the `&name` `node` is the value of the `Topic:` field of a `TopicPartition` literal.
fn is_topic_pointer(node: Node, src: &[u8]) -> bool {
    let keyed = (node.parent())
        .and_then(|element| element.parent())
        .filter(|keyed| keyed.kind() == "keyed_element");
    let key = keyed.and_then(|keyed| keyed.child_by_field_name("key"));
    let literal = (keyed.and_then(|keyed| keyed.parent())).and_then(|body| body.parent());
    let ty = literal.and_then(|l| l.child_by_field_name("type"));
    key.is_some_and(|key| text(key, src) == "Topic")
        && ty.is_some_and(|ty| text(ty, src).ends_with("TopicPartition"))
}

/// The `name` fields of a declaration (`a, b int`, `x, y = 1, 2`).
fn names(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("name", &mut cursor).collect()
}

/// The names of the type parameters a declaration lists.
fn type_params(node: Node) -> Vec<Node> {
    let list = node.child_by_field_name("type_parameters");
    (list.into_iter().flat_map(named)).flat_map(names).collect()
}

/// What a `:=` or a `var` in `node` holds for: from the end of the statement (its own value
/// still sees the outer name) to the end of the block, clause or statement that encloses it.
fn region(node: Node) -> (usize, usize) {
    let mut at = node.parent();
    while let Some(parent) = at {
        let ends = matches!(
            parent.kind(),
            "block"
                | "if_statement"
                | "for_statement"
                | "expression_switch_statement"
                | "type_switch_statement"
                | "select_statement"
                | "expression_case"
                | "type_case"
                | "default_case"
                | "communication_case"
                | "source_file"
        );
        if ends {
            return (node.end_byte(), parent.end_byte());
        }
        at = parent.parent();
    }
    (node.end_byte(), usize::MAX)
}

/// The identifiers of the left side of a `:=`.
fn identifiers(node: Node) -> Vec<Node> {
    match node.kind() {
        "identifier" => vec![node],
        _ => named(node).filter(|n| n.kind() == "identifier").collect(),
    }
}

/// The type a type expression names, through a pointer, type arguments and parentheses
/// (`Store` for `*Store` and `Store[T]`), qualified by its package (`shapes.Circle`). A
/// slice, a map, a function or any other type literal names none.
fn type_name(node: Node, src: &[u8]) -> Option<String> {
    let mut ty = node;
    for _ in 0..PATH_DEPTH {
        ty = match ty.kind() {
            "type_identifier" => return Some(text(ty, src).to_string()),
            "qualified_type" => {
                let package = ty.child_by_field_name("package")?;
                let name = ty.child_by_field_name("name")?;
                return Some(format!("{}.{}", text(package, src), text(name, src)));
            }
            "generic_type" => ty.child_by_field_name("type")?,
            "pointer_type" | "parenthesized_type" => named(ty).next()?,
            _ => return None,
        };
    }
    None
}

/// The class an expression gives, as far as it writes it: a composite literal (`T{}`, `&T{}`)
/// is a `T`; a call is whatever the function it names returns (`Class::Called`).
fn value_class(node: Node, src: &[u8]) -> Option<Class> {
    let (mut node, mut address) = (node, false);
    for _ in 0..PATH_DEPTH {
        match node.kind() {
            "parenthesized_expression" => node = named(node).next()?,
            "unary_expression" if !address => {
                let operator = node.child_by_field_name("operator")?;
                if text(operator, src) != "&" {
                    return None;
                }
                (node, address) = (node.child_by_field_name("operand")?, true);
            }
            "composite_literal" => {
                return type_name(node.child_by_field_name("type")?, src).map(Class::Type);
            }
            "call_expression" if !address => {
                let callee = callee(node.child_by_field_name("function")?, src);
                return (!callee.starts_with('(')).then_some(Class::Called(callee));
            }
            _ => return None,
        }
    }
    None
}

/// The class a function's first result writes (`*Store` of `(*Store, error)`).
fn result_class(node: Node, src: &[u8]) -> Option<Class> {
    let result = node.child_by_field_name("result")?;
    let ty = match result.kind() {
        "parameter_list" => named(result).next()?.child_by_field_name("type")?,
        _ => result,
    };
    type_name(ty, src).map(Class::Type)
}

/// A function's named results, when it names them.
fn result_list(node: Node) -> Option<Node> {
    node.child_by_field_name("result")
        .filter(|r| r.kind() == "parameter_list")
}

fn call(scope: &str, callee: String, range: Range<usize>) -> Call {
    Call {
        arity: None,
        labels: None,
        constructs: false,
        awaited: false,
        scope: scope.to_string(),
        callee,
        range,
        args: Vec::new(),
        kwargs: Vec::new(),
        decorates: None,
        by_path: false,
        root: None,
        on: None,
        receiver: None,
        arg_types: Vec::new(),
        uses: Vec::new(),
    }
}

fn def(name: String, kind: Kind, range: Range<usize>, params: Vec<Param>) -> Def {
    Def {
        name,
        kind,
        range,
        params,
        bases: Vec::new(),
        decorators: Vec::new(),
        return_values: Vec::new(),
        returns: None,
        type_params: Vec::new(),
        result: None,
        suspends: false,
    }
}

/// The parameters of a parameter list in order; an unnamed or blank one has an empty name.
fn params(list: Option<Node>, src: &[u8]) -> Vec<Param> {
    let param = |name: &str| Param {
        name: if name == "_" {
            String::new()
        } else {
            name.to_string()
        },
        binds: Binds::Any,
        label: None,
        default: None,
        fields: Vec::new(),
    };
    let mut out = Vec::new();
    for declaration in list.into_iter().flat_map(named) {
        match names(declaration)[..] {
            [] => out.push(param("")),
            ref several => out.extend(several.iter().map(|n| param(text(*n, src)))),
        }
    }
    out
}

/// The normalized call expression: `a.b.c` for identifier and selector chains, `(<node kind>)`
/// for any other head.
fn callee(node: Node, src: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut at = node;
    loop {
        match at.kind() {
            "identifier" => {
                parts.push(text(at, src).to_string());
                break;
            }
            "selector_expression" if parts.len() < PATH_DEPTH => {
                let field = at.child_by_field_name("field");
                let operand = at.child_by_field_name("operand");
                let (Some(field), Some(operand)) = (field, operand) else {
                    return format!("({})", node.kind());
                };
                parts.push(text(field, src).to_string());
                at = operand;
            }
            "selector_expression" => return format!("({})", node.kind()),
            kind => {
                parts.push(format!("({kind})"));
                break;
            }
        }
    }
    parts.reverse();
    parts.join(".")
}

/// The type a method's receiver names, through pointers, type arguments and parentheses
/// (`Store` for `*Store`, `Store[T]` and `(*Store)`), and the type parameters its type
/// arguments name (`T`), which the method binds.
fn receiver_type<'s>(receiver: Node, src: &'s [u8]) -> Option<(&'s str, Vec<&'s str>)> {
    let declaration = named(receiver).next()?;
    let mut ty = declaration.child_by_field_name("type")?;
    let mut bound = Vec::new();
    for _ in 0..PATH_DEPTH {
        ty = match ty.kind() {
            "type_identifier" => return Some((text(ty, src), bound)),
            "generic_type" => {
                let arguments = ty.child_by_field_name("type_arguments");
                let elems = arguments.into_iter().flat_map(named);
                bound.extend(
                    (elems.filter_map(|e| named(e).next()))
                        .filter(|t| t.kind() == "type_identifier")
                        .map(|t| text(t, src)),
                );
                ty.child_by_field_name("type")?
            }
            "pointer_type" | "parenthesized_type" => named(ty).next()?,
            _ => return None,
        };
    }
    None
}

/// The name an import binds when it names none: the package name the Go tools assume for the
/// path, its last element (the one before a major version `vN`), without a `go-` prefix and up
/// to the first character no identifier holds (`gopkg.in/yaml.v3` is `yaml`, `kafka-go` is
/// `kafka`).
fn assumed_name(path: &str) -> &str {
    let mut elements = path.rsplit('/');
    let last = elements.next().unwrap_or(path);
    let major = last
        .strip_prefix('v')
        .is_some_and(|n| n.parse::<u32>().is_ok());
    let base = match elements.next() {
        Some(before) if major => before,
        _ => last,
    };
    let base = base.strip_prefix("go-").unwrap_or(base);
    let end = base
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(base.len());
    &base[..end]
}

/// Whether Go exports the name: it starts with an upper-case letter.
fn exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn imports(node: Node, src: &[u8], facts: &mut Facts) {
    let specs: Vec<Node> = match named(node).next() {
        Some(list) if list.kind() == "import_spec_list" => named(list).collect(),
        Some(spec) => vec![spec],
        None => Vec::new(),
    };
    for spec in specs.into_iter().filter(|s| s.kind() == "import_spec") {
        let Some(path) = spec.child_by_field_name("path") else {
            continue;
        };
        let source = text(path, src);
        let source = source.get(1..source.len().saturating_sub(1)).unwrap_or("");
        let (local, name) = match spec.child_by_field_name("name") {
            // A blank import binds no name.
            Some(n) if n.kind() == "blank_identifier" => continue,
            // A dot import: the package's exports join the file scope.
            Some(n) if n.kind() == "dot" => (String::new(), Imported::All),
            Some(n) => (text(n, src).to_string(), Imported::Namespace),
            None => (assumed_name(source).to_string(), Imported::Namespace),
        };
        facts.imports.push(Import {
            local,
            source: source.to_string(),
            name,
        });
    }
}

/// Leaves a name two imports of the file give unknown: a plain import binds the name assumed
/// from its path (`assumed_name`), which may not be its package's, so neither is sure to bind
/// it. They become one import of the name from the empty specifier, which names nothing of the
/// repository: a call through it links to nothing.
fn unbind_clashes(imports: &mut Vec<Import>) {
    let mut count: HashMap<String, usize> = HashMap::new();
    for import in imports.iter().filter(|i| !i.local.is_empty()) {
        *count.entry(import.local.clone()).or_default() += 1;
    }
    let mut seen = HashSet::new();
    imports.retain_mut(|import| {
        if count.get(&import.local).is_none_or(|n| *n < 2) {
            return true;
        }
        import.source.clear();
        seen.insert(import.local.clone())
    });
}

fn parse(path: &str, src: &[u8]) -> Result<Facts> {
    if !build::builds(path, src) {
        return Ok(Facts {
            unbuilt: true,
            ..Facts::default()
        });
    }
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_go::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let mut walk = Walk {
        src,
        facts: Facts::default(),
        locals: BTreeSet::new(),
        bound: BTreeMap::new(),
        type_scoped: BTreeSet::new(),
        held: Vec::new(),
        assigned: Vec::new(),
        returns: HashMap::new(),
        std: values::Std::default(),
        vars: BTreeSet::new(),
        selected: BTreeSet::new(),
        derefs: HashSet::new(),
        lookups: HashMap::new(),
    };
    // Imports come before every other declaration, and the values read the names they give.
    for node in named(tree.root_node()).filter(|n| n.kind() == "import_declaration") {
        imports(node, src, &mut walk.facts);
    }
    // A cgo file (`import "C"`): the target builds without cgo, so it is left out like a file
    // its constraints exclude.
    if walk.facts.imports.iter().any(|i| i.source == "C") {
        return Ok(Facts {
            unbuilt: true,
            ..Facts::default()
        });
    }
    walk.std = values::Std::new(tree.root_node(), src, &walk.facts.imports);
    let mut top = Vec::new();
    let mut inits: HashMap<&str, usize> = HashMap::new();
    for node in named(tree.root_node()) {
        match node.kind() {
            "package_clause" => {
                walk.facts.package = named(node).next().map_or("", |n| text(n, src)).into();
            }
            "import_declaration" => {}
            "function_declaration" => {
                let Some(name) = node.child_by_field_name("name").map(|n| text(n, src)) else {
                    continue;
                };
                let body = node.child_by_field_name("body");
                // A blank function names no symbol: it folds into the file, as a literal does.
                if name == "_" {
                    let inline = walk.inline("", node);
                    body.into_iter().for_each(|b| walk.body(&inline, b));
                    continue;
                }
                // Each `init` (the one name a file may declare twice) is a symbol of its own.
                let n = inits.entry(name).or_insert(0);
                *n += 1;
                let name = match *n {
                    1 => name.to_string(),
                    n => format!("{name}#{n}"),
                };
                let list = node.child_by_field_name("parameters");
                let range = node.byte_range();
                let mut function = def(name.clone(), Kind::Function, range, params(list, src));
                function.result = result_class(node, src);
                walk.facts.defs.push(function);
                // `func main` of package `main` starts a program.
                if name == "main" && walk.facts.package == "main" {
                    walk.facts.programs.push(name.clone());
                }
                walk.typed_params(&name, list, false);
                walk.typed_params(&name, result_list(node), true);
                walk.bind_type_params(&name, node);
                if let Some(body) = body {
                    walk.body(&name, body);
                }
                top.push(name);
            }
            "method_declaration" => {
                let receiver = node.child_by_field_name("receiver");
                let ty = receiver.and_then(|r| receiver_type(r, src));
                let name = node.child_by_field_name("name").map(|n| text(n, src));
                let (Some((ty, type_params)), Some(name)) = (ty, name) else {
                    continue;
                };
                let qualified = format!("{ty}.{name}");
                let list = node.child_by_field_name("parameters");
                let range = node.byte_range();
                let mut method = def(qualified.clone(), Kind::Method, range, params(list, src));
                method.result = result_class(node, src);
                if !exported(name) {
                    walk.facts.hidden.push(qualified.clone());
                }
                walk.facts.defs.push(method);
                walk.typed_params(&qualified, list, false);
                walk.typed_params(&qualified, receiver, true);
                walk.typed_params(&qualified, result_list(node), true);
                for type_param in type_params {
                    walk.local(&qualified, type_param);
                }
                if let Some(body) = node.child_by_field_name("body") {
                    walk.body(&qualified, body);
                }
            }
            "type_declaration" => {
                for spec in named(node).filter(|s| matches!(s.kind(), "type_spec" | "type_alias")) {
                    let Some(name) = spec.child_by_field_name("name").map(|n| text(n, src)) else {
                        continue;
                    };
                    walk.facts.defs.push(def(
                        name.into(),
                        Kind::Class,
                        spec.byte_range(),
                        Vec::new(),
                    ));
                    top.push(name.to_string());
                    walk.bind_type_scope(name, spec);
                    let ty = spec.child_by_field_name("type");
                    for field in (ty.filter(|t| t.kind() == "struct_type").into_iter())
                        .flat_map(named)
                        .flat_map(named)
                        .filter(|f| f.kind() == "field_declaration")
                    {
                        let class = field.child_by_field_name("type");
                        let class = class.and_then(|t| type_name(t, src)).map(Class::Type);
                        for field in names(field) {
                            walk.bind_as(name, text(field, src), class.clone(), false, None);
                        }
                    }
                    for elem in ty
                        .filter(|t| t.kind() == "interface_type")
                        .into_iter()
                        .flat_map(named)
                    {
                        let method = elem
                            .child_by_field_name("name")
                            .filter(|_| elem.kind() == "method_elem");
                        let Some(method) = method else { continue };
                        let params = params(elem.child_by_field_name("parameters"), src);
                        let qualified = format!("{name}.{}", text(method, src));
                        if !exported(text(method, src)) {
                            walk.facts.hidden.push(qualified.clone());
                        }
                        let range = elem.byte_range();
                        let mut method = def(qualified, Kind::Method, range, params);
                        method.result = result_class(elem, src);
                        walk.facts.defs.push(method);
                    }
                }
            }
            "var_declaration" | "const_declaration" => {
                let mut specs: Vec<Node> = named(node).collect();
                if let [list] = &specs[..]
                    && list.kind() == "var_spec_list"
                {
                    specs = named(*list).collect();
                }
                for spec in specs {
                    let ty = spec.child_by_field_name("type");
                    let value = spec.child_by_field_name("value");
                    walk.assigned("", &names(spec), ty, value, None);
                    // Each name holds the value written at its place, when the names and the
                    // values pair up. A `var` holds it only if nothing assigns it (`finish_values`).
                    let given: Vec<Node> = value.into_iter().flat_map(named).collect();
                    let listed = names(spec);
                    let paired = given.len() == listed.len();
                    for (at, name) in listed.into_iter().enumerate() {
                        let name = text(name, src);
                        if name == "_" {
                            continue;
                        }
                        let value = match given.get(at) {
                            Some(v) if paired => values::value(*v, src, &walk.std, "", 0),
                            _ => Value::Unknown,
                        };
                        if node.kind() == "var_declaration" {
                            walk.vars.insert(name.to_string());
                        }
                        walk.std.note("", name, &value);
                        walk.facts.consts.push(Const {
                            name: name.to_string(),
                            value,
                            ty: None,
                            scope: None,
                        });
                        top.push(name.to_string());
                    }
                    if let Some(value) = spec.child_by_field_name("value") {
                        walk.body("", value);
                    }
                }
            }
            _ => walk.body("", node),
        }
    }
    // The values read `bound`, which `finish` takes.
    walk.finish_values();
    walk.finish();
    let mut facts = walk.facts;
    unbind_clashes(&mut facts.imports);
    facts.exports = (top.into_iter().filter(|n| exported(n)))
        .map(|name| Export::Local {
            exported: name.clone(),
            local: name,
        })
        .collect();
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(src: &str) -> Facts {
        parse("x.go", src.as_bytes()).unwrap()
    }

    #[test]
    fn literals_nested_past_the_limit_set_fields_of_a_type_not_read() {
        let nested = |n: usize| {
            let ty = format!("{}T", "[]".repeat(n));
            let value = format!("{}a: 1{}", "{".repeat(n + 1), "}".repeat(n + 1));
            let markers = facts(&format!("package x\n\nvar v = {ty}{value}\n")).consts;
            let marked = |ty: &str| {
                (markers.iter()).any(|c| matches!(&c.value, Value::New(t, _) if t == ty))
            };
            (marked("T"), marked("?"))
        };
        assert_eq!(nested(3), (true, false));
        assert_eq!(nested(values::VALUE_DEPTH + 10), (false, true));
    }

    #[test]
    fn a_cgo_file_is_left_out() {
        assert!(facts("package x\n\nimport \"C\"\n\nfunc F() {}\n").unbuilt);
        assert!(facts("package x\n\nimport (\n\t\"fmt\"\n\t\"C\"\n)\n").unbuilt);
        assert!(!facts("package x\n\nimport c \"example.com/C\"\n").unbuilt);
    }

    #[test]
    fn a_selector_chain_longer_than_the_limit_is_its_node_kind() {
        let callee = |n: usize| {
            let src = format!("package x\n\nfunc f() {{ a{}() }}\n", ".b".repeat(n));
            facts(&src).calls[0].callee.clone()
        };
        assert_eq!(callee(PATH_DEPTH), format!("a{}", ".b".repeat(PATH_DEPTH)));
        assert_eq!(callee(PATH_DEPTH + 1), "(selector_expression)");
    }

    #[test]
    fn a_receiver_type_wrapped_deeper_than_the_limit_names_no_method() {
        let methods = |n: usize| {
            let src = format!("package x\n\nfunc (s {}S) M() {{}}\n", "*".repeat(n));
            facts(&src)
                .defs
                .into_iter()
                .map(|d| d.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(methods(PATH_DEPTH - 1), ["S.M"]);
        assert!(methods(PATH_DEPTH).is_empty());
    }

    #[test]
    fn literals_nested_deeper_than_the_limit_are_no_definitions() {
        let n = DEFINITION_DEPTH + 10;
        let src = format!(
            "package x\n\nfunc f() {{ {} g() {} }}\n",
            "func() {".repeat(n),
            "}()".repeat(n)
        );
        let facts = facts(&src);
        let inline = facts.defs.iter().filter(|d| d.kind == Kind::Inline).count();
        assert_eq!(inline, DEFINITION_DEPTH);
        let scope = &facts.calls.iter().find(|c| c.callee == "g").unwrap().scope;
        assert_eq!(scope.matches('@').count(), DEFINITION_DEPTH);
    }

    /// The receivers as `scope name = class`, sorted.
    fn receivers(src: &str) -> Vec<String> {
        let mut found: Vec<String> = (facts(src).receivers.into_iter())
            .map(|r| match r.class {
                Class::Type(n) => format!("{} {} = {n}", r.scope, r.name),
                Class::Called(n) => format!("{} {} = {n}()", r.scope, r.name),
                _ => format!("{} {} = ?", r.scope, r.name),
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn written_types_make_receivers() {
        let src = "package x\n\n\
            type T struct {\n\ta, b *p.A\n\tc []B\n\tC\n}\n\n\
            func (t *T) M(x U, y ...V) (z W, err error) {\n\
            \tvar v X\n\tq := &Y{}\n\tr, s := f()\n\tu := (g.H{})\n}\n";
        assert_eq!(
            receivers(src),
            [
                "T a = p.A",
                "T b = p.A",
                "T.M err = error",
                "T.M q = Y",
                "T.M r = f()",
                "T.M t = T",
                "T.M u = g.H",
                "T.M v = X",
                "T.M x = U",
                "T.M z = W",
            ]
        );
    }

    #[test]
    fn a_name_bound_to_different_types_or_hidden_types_make_none() {
        let src = "package x\n\n\
            func f[T any](a *T, b B) {\n\ttype B int\n\tc := C{}\n\t{\n\t\tc := D{}\n\t}\n\
            \td := E{}\n\td, e := g()\n}\n";
        assert!(receivers(src).is_empty());
    }

    #[test]
    fn a_name_held_for_part_of_its_scope_is_a_receiver_only_if_every_call_is_inside() {
        let src = |body: &str| format!("package x\n\nfunc f() {{\n{body}\n}}\n");
        let held = |body: &str| receivers(&src(body));
        assert_eq!(held("\ta := S{}\n\ta.M()"), ["f a = S"]);
        assert_eq!(held("\ta := S{}\n\tg(func() { a.M() })"), ["f a = S"]);
        // Before the binding, after the block that holds it, or in its own value.
        assert!(held("\ta.M()\n\ta := S{}").is_empty());
        assert!(held("\tg(func() { a.M() })\n\ta := S{}").is_empty());
        assert!(held("\t{\n\t\ta := S{}\n\t\ta.M()\n\t}\n\ta.M()").is_empty());
        assert!(held("\ta := a.N()").is_empty());
    }

    #[test]
    fn a_type_declarations_parameters_hide_a_type_in_the_declaration_alone() {
        let src = "package x\n\ntype B[T any] struct{ v T; w U }\n\n\
                   func (b B[U]) M(t T, u U) {}\n";
        assert_eq!(receivers(src), ["B w = U", "B.M b = B", "B.M t = T"]);
    }

    #[test]
    fn a_type_wrapped_in_more_pointers_than_the_limit_is_no_receiver() {
        let receivers = |n: usize| {
            let src = format!("package x\n\nfunc f(a {}S) {{}}\n", "*".repeat(n));
            receivers(&src)
        };
        assert_eq!(receivers(PATH_DEPTH - 1), ["f a = S"]);
        assert!(receivers(PATH_DEPTH).is_empty());
    }

    #[test]
    fn a_name_one_statement_of_the_function_binds_once_is_a_constant() {
        let consts = |body: &str| -> Vec<String> {
            let src = format!("package x\n\nfunc f() {{ {body} }}\n");
            let names = facts(&src).consts.into_iter().map(|c| c.name);
            // The markers of the fields literals set are no constants.
            names.filter(|n| !n.ends_with('}')).collect()
        };
        assert_eq!(consts("r := mk()"), ["f.r"]);
        assert_eq!(consts("var r = mk()"), ["f.r"]);
        assert_eq!(consts("r := mk(); r.Use()"), ["f.r"]);
        // Assigned again, in the function or in a literal in it, bound again in a block, in an
        // initializer, in a loop, or bound with others: no one value.
        for body in [
            "r := mk(); r = other()",
            "r := mk(); func() { r = other() }()",
            "r := mk(); for r = range xs {}",
            "r := mk(); keep(&r)",
            "r := mk(); func() { keep(&r) }()",
            "r := mk(); { r := other(); _ = r }",
            "r := mk(); if r := other(); r != nil {}",
            "if r := mk(); r != nil {}",
            "{ r := mk(); _ = r }",
            "a, b := mk()",
            "var a, b = mk(), mk()",
            "var r *T",
            "_ := mk()",
        ] {
            assert_eq!(consts(body), Vec::<String>::new(), "{body}");
        }
        // The address of a name as the `Topic` of a `TopicPartition` keeps it a constant, unless
        // the function writes through a dereference; any other literal does not.
        let kept = "r := \"x\"; _ = kafka.TopicPartition{Topic: &r}";
        assert_eq!(consts(kept), ["f.r"]);
        for body in [
            "r := \"x\"; _ = T{Topic: &r}",
            "r := \"x\"; _ = kafka.TopicPartition{Other: &r}",
            "r := \"x\"; p := kafka.TopicPartition{Topic: &r}; *p.Topic = \"y\"",
            "r := \"x\"; p := kafka.TopicPartition{Topic: &r}; func() { *p.Topic = \"y\" }()",
            "r := \"x\"; p := kafka.TopicPartition{Topic: &r}; *q++",
        ] {
            assert!(!consts(body).contains(&"f.r".to_string()), "{body}");
        }
        // A literal's own statement binds in the literal.
        let inner = consts("func() { q := mk(); _ = q }()");
        assert_eq!(inner.len(), 1);
        assert!(inner[0].starts_with("f.@") && inner[0].ends_with(".q"));
    }

    #[test]
    fn a_function_returns_the_one_value_a_return_gives() {
        let returns = |src: &str| -> usize {
            let src = format!("package x\n\n{src}\n");
            facts(&src).defs[0].return_values.len()
        };
        assert_eq!(returns("func f() *T { return mk() }"), 1);
        assert_eq!(
            returns("func f(c bool) *T { if c { return a() }; return b() }"),
            2
        );
        assert_eq!(returns("func f() (*T, error) { return mk(), nil }"), 0);
        assert_eq!(returns("func f() (r *T) { return }"), 0);
    }

    #[test]
    fn a_plain_import_binds_the_name_the_go_tools_assume() {
        let cases = [
            ("fmt", "fmt"),
            ("net/http", "http"),
            ("github.com/go-chi/chi/v5", "chi"),
            ("gopkg.in/yaml.v3", "yaml"),
            ("github.com/segmentio/kafka-go", "kafka"),
            ("github.com/redis/go-redis/v9", "redis"),
            ("github.com/nats-io/nats.go", "nats"),
        ];
        for (path, name) in cases {
            assert_eq!(assumed_name(path), name, "{path}");
        }
    }
}
