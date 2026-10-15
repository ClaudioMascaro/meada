//! The Python language pack.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use tree_sitter::{Node, Parser};

use super::{
    Call, Class, Const, Def, Export, Facts, Import, Imported, Kind, LanguagePack, Local, Modules,
    Param, Part, Star, Value, children, named, text,
};
use crate::{Result, qualify};

pub struct Python;

impl LanguagePack for Python {
    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn parse(&self, _path: &str, src: &[u8]) -> Result<Facts> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
        let tree = parser
            .parse(src, None)
            .ok_or("tree-sitter returned no tree")?;
        let mut facts = Facts::default();
        let mut module = Module::default();
        // A name defined twice (`@property` and its setter, `try`/`except` fallbacks) is one
        // symbol: the first definition.
        let mut defined = HashSet::new();
        let mut locals = Locals::default();
        // Each function's position in `facts.defs`, for its `return`s.
        let mut functions: HashMap<String, usize> = HashMap::new();
        // The decorator calls not yet met, by node, and the definition each one decorates.
        let mut decorators: HashMap<usize, String> = HashMap::new();
        // The bare-name decorators of each definition not yet met.
        let mut bare: HashMap<String, Vec<String>> = HashMap::new();
        // Explicit stack instead of recursion: generated code can nest deeper than the call
        // stack.
        let mut stack = vec![Visit {
            node: tree.root_node(),
            scope: String::new(),
            in_class: false,
        }];
        while let Some(Visit {
            node,
            scope,
            in_class,
        }) = stack.pop()
        {
            let mut inner = scope.clone();
            let mut inner_class = in_class;
            match node.kind() {
                "function_definition" | "class_definition" => {
                    let is_class = node.kind() == "class_definition";
                    if let Some(name) = node.child_by_field_name("name") {
                        let name = text(name, src);
                        inner = qualify(&scope, name);
                        if scope.is_empty() {
                            module.names.push(name.to_string());
                        }
                        if defined.insert(inner.clone()) {
                            if !is_class {
                                functions.insert(inner.clone(), facts.defs.len());
                            }
                            let kind = match (is_class, in_class) {
                                (true, _) => Kind::Class,
                                (false, true) => Kind::Method,
                                (false, false) => Kind::Function,
                            };
                            facts.defs.push(Def {
                                name: inner.clone(),
                                kind,
                                range: span(node),
                                params: params(node, src),
                                bases: bases(node, src),
                                decorators: bare.remove(&inner).unwrap_or_default(),
                                returns: Vec::new(),
                            });
                        }
                        if !is_class {
                            locals.params(node, &inner, in_class, src);
                        }
                    }
                    if is_class {
                        locals.classes.insert(inner.clone());
                    }
                    inner_class = is_class;
                }
                "decorated_definition" => {
                    let name = (node.child_by_field_name("definition"))
                        .and_then(|d| d.child_by_field_name("name"));
                    if let Some(name) = name {
                        let def = qualify(&scope, text(name, src));
                        for decorator in named(node).filter(|n| n.kind() == "decorator") {
                            match named(decorator).next() {
                                Some(call) if call.kind() == "call" => {
                                    decorators.insert(call.id(), def.clone());
                                }
                                Some(e) if matches!(e.kind(), "identifier" | "attribute") => {
                                    let names = bare.entry(def.clone()).or_default();
                                    names.push(callee(e, src));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "call" => {
                    if let Some(function) = node.child_by_field_name("function") {
                        let (args, kwargs) = arguments(node, src);
                        facts.calls.push(Call {
                            scope: scope.clone(),
                            callee: callee(function, src),
                            range: node.byte_range(),
                            args,
                            kwargs,
                            decorates: decorators.remove(&node.id()),
                        });
                    }
                }
                "return_statement" => {
                    if let (Some(&at), Some(returned)) = (functions.get(&scope), named(node).next())
                    {
                        facts.defs[at].returns.push(value(returned, src));
                    }
                }
                // ponytail: an import binds its names for the whole file, wherever it sits; a
                // function-local one also hides an outer binding there, but other functions
                // see it too.
                "import_statement" | "import_from_statement" => {
                    let first = facts.imports.len();
                    imports(node, src, &mut facts.imports);
                    if scope.is_empty() {
                        module.import(&facts.imports[first..]);
                    } else {
                        // In a function or a class, the import also hides an outer binding.
                        for import in &facts.imports[first..] {
                            if !import.local.contains('.') && import.local != "*" {
                                locals.bind(&scope, &import.local, Class::Imported, How::Assigned);
                            }
                        }
                    }
                }
                "assignment" | "augmented_assignment" => {
                    if scope.is_empty() && node.kind() == "assignment" {
                        module.assign(node, src);
                    }
                    locals.assign(node, &scope, src);
                }
                "as_pattern" if node.parent().is_some_and(|p| p.kind() == "with_item") => {
                    locals.with_item(node, &scope, src);
                }
                // Other bindings say nothing of a class, but hide an outer binding of the name.
                // ponytail: a comprehension's variables are bound in the enclosing scope, so
                // they hide a name there for the whole scope.
                "for_statement" | "for_in_clause" | "as_pattern" | "except_clause"
                | "named_expression" => {
                    let field = match node.kind() {
                        "for_statement" | "for_in_clause" => "left",
                        "named_expression" => "name",
                        _ => "alias",
                    };
                    let target = node.child_by_field_name(field);
                    for target in target.map(targets).unwrap_or_default() {
                        locals.target(target, &scope, Class::Unknown, How::Assigned, src);
                    }
                }
                "case_clause" => {
                    let mut found = Vec::new();
                    for pattern in named(node).filter(|n| n.kind() == "case_pattern") {
                        captures(pattern, &mut found);
                    }
                    for name in found {
                        locals.bind(&scope, text(name, src), Class::Unknown, How::Assigned);
                    }
                }
                // A lambda's calls belong to the enclosing scope, which its parameters bind.
                "lambda" => {
                    for (name, _) in parameters(node) {
                        if let Some(name) = name {
                            locals.bind(&scope, text(name, src), Class::Unknown, How::Assigned);
                        }
                    }
                }
                "global_statement" | "nonlocal_statement" => {
                    let global = node.kind() == "global_statement";
                    for name in named(node).filter(|n| n.kind() == "identifier") {
                        let key = (scope.clone(), text(name, src).to_string());
                        locals.declared.insert(key, global);
                    }
                }
                _ => {}
            }
            let kids: Vec<Node> = children(node).collect();
            stack.extend(kids.into_iter().rev().map(|child| Visit {
                node: child,
                scope: inner.clone(),
                in_class: inner_class,
            }));
        }
        (facts.exports, facts.star) = module.exports();
        (facts.locals, facts.consts) = locals.finish();
        Ok(facts)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["pyproject.toml"]
    }

    fn modules(&self, configs: &[(&str, &[u8])]) -> Result<Modules> {
        let declared = match configs.first() {
            Some((name, src)) => source_roots(src).map_err(|e| format!("{name}: {e}"))?,
            None => Vec::new(),
        };
        // The repository root, then the declared roots; a `src` directory when none is
        // declared.
        // ponytail: `src` is a root whether or not it holds packages, since a pack sees no
        // file list.
        let mut roots = vec![String::new()];
        if declared.is_empty() {
            roots.push("src".to_string());
        }
        roots.extend(declared);
        Ok(Box::new(move |importer, specifier| {
            let dots = specifier.bytes().take_while(|&b| b == b'.').count();
            let name = specifier[dots..].replace('.', "/");
            let bases = if dots == 0 {
                roots.clone()
            } else {
                // One dot names the importer's own package, its directory; each more goes one
                // level up.
                let dir = importer.rsplit_once('/').map_or("", |(dir, _)| dir);
                let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
                let Some(keep) = parts.len().checked_sub(dots - 1) else {
                    return Vec::new();
                };
                parts.truncate(keep);
                vec![parts.join("/")]
            };
            bases.iter().flat_map(|base| files(base, &name)).collect()
        }))
    }
}

struct Visit<'t> {
    node: Node<'t>,
    scope: String,
    /// Whether the node sits in a class body, outside any function in it: a function defined
    /// here is a method.
    in_class: bool,
}

/// What a module binds at its top level, which makes up its exports.
#[derive(Default)]
struct Module {
    names: Vec<String>,
    /// The names of a literal `__all__`; `None` when there is none, or it is computed.
    all: Option<Vec<String>>,
    /// The modules `from m import *` brings in.
    stars: Vec<String>,
}

impl Module {
    fn import(&mut self, imports: &[Import]) {
        for import in imports {
            match import.name {
                Imported::All => self.stars.push(import.source.clone()),
                _ if !import.local.contains('.') => self.names.push(import.local.clone()),
                _ => {}
            }
        }
    }

    fn assign(&mut self, node: Node, src: &[u8]) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        if left.kind() != "identifier" {
            return;
        }
        let name = text(left, src);
        self.names.push(name.to_string());
        if name == "__all__" {
            self.all = node
                .child_by_field_name("right")
                .and_then(|right| string_list(right, src));
        }
    }

    /// What an explicit import or an attribute access names: every module-level name (imports
    /// included, as that is how a package re-exports) and its star imports' names. What
    /// `import *` takes is a literal `__all__`, else the public names; a computed `__all__` is
    /// ignored.
    fn exports(self) -> (Vec<Export>, Star) {
        let mut seen = HashSet::new();
        let exports = (self.names.into_iter())
            .filter(|n| seen.insert(n.clone()))
            .map(|name| Export::Local {
                exported: name.clone(),
                local: name,
            })
            .chain(self.stars.into_iter().map(|source| Export::All { source }))
            .collect();
        (exports, self.all.map_or(Star::Public, Star::Only))
    }
}

/// The repository-relative files that module `name`, a `/`-separated path, may be under
/// `base`: the `__init__.py` of the package `name` (`base` itself when `name` is empty), which
/// Python prefers, or `name.py`.
fn files(base: &str, name: &str) -> Vec<String> {
    let path = [base, name]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    let init = if path.is_empty() {
        "__init__.py".to_string()
    } else {
        format!("{path}/__init__.py")
    };
    if name.is_empty() {
        vec![init]
    } else {
        vec![init, format!("{path}.py")]
    }
}

/// The source roots `pyproject.toml` declares besides the repository root: setuptools'
/// `package-dir` and `packages.find` `where`, and Poetry's `packages` with `from`.
// ponytail: other build backends (hatch, pdm, flit) and nested projects are not read.
fn source_roots(src: &[u8]) -> Result<Vec<String>> {
    let config: toml::Table = std::str::from_utf8(src)?.parse()?;
    let tool = config.get("tool");
    let setuptools = tool.and_then(|t| t.get("setuptools"));
    let mut roots: Vec<String> = Vec::new();
    // `"" = "src"` roots every package in `src`; `"pkg" = "lib/pkg"` roots `pkg` in `lib`, and
    // `"a.b" = "lib/a/b"` roots `a.b` in `lib`. A directory not named after its package says
    // nothing about a root.
    let dirs = setuptools.and_then(|s| s.get("package-dir"));
    for (package, dir) in dirs.and_then(toml::Value::as_table).into_iter().flatten() {
        let (Some(dir), Some(package)) = (
            dir.as_str().and_then(clean),
            clean(&package.replace('.', "/")),
        ) else {
            continue;
        };
        if package.is_empty() {
            roots.push(dir);
        } else if let Some(root) = dir.strip_suffix(&format!("/{package}")) {
            roots.push(root.to_string());
        }
    }
    let find = setuptools.and_then(|s| s.get("packages")?.get("find"));
    let wheres = find.and_then(|f| f.get("where")?.as_array());
    roots.extend(
        wheres
            .into_iter()
            .flatten()
            .filter_map(|w| clean(w.as_str()?)),
    );
    let poetry = tool.and_then(|t| t.get("poetry")?.get("packages")?.as_array());
    roots.extend(
        poetry
            .into_iter()
            .flatten()
            .filter_map(|p| clean(p.get("from")?.as_str()?)),
    );
    let mut seen = HashSet::new();
    roots.retain(|root| !root.is_empty() && seen.insert(root.clone()));
    Ok(roots)
}

/// A repository-relative directory in its plain form, `None` when it leaves the repository.
fn clean(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// The span of a definition: a decorated one includes its decorators.
fn span(node: Node) -> Range<usize> {
    match node.parent() {
        Some(parent) if parent.kind() == "decorated_definition" => parent.byte_range(),
        _ => node.byte_range(),
    }
}

/// The parameters of a function `node` in order, `self` included, with their defaults. `*args`
/// and `**kwargs` keep their stars in the name, and so does the bare `*`: such a parameter
/// binds no argument, and the ones after it are keyword-only (`Param::name`).
fn params(node: Node, src: &[u8]) -> Vec<Param> {
    let Some(list) = node.child_by_field_name("parameters") else {
        return Vec::new();
    };
    named(list)
        .filter_map(|p| {
            let (name, default) = match p.kind() {
                "default_parameter" | "typed_default_parameter" => (
                    text(identifier(p.child_by_field_name("name")?)?, src).to_string(),
                    p.child_by_field_name("value"),
                ),
                "identifier" | "typed_parameter" => (text(identifier(p)?, src).to_string(), None),
                "list_splat_pattern" => (format!("*{}", text(identifier(p)?, src)), None),
                "dictionary_splat_pattern" => (format!("**{}", text(identifier(p)?, src)), None),
                "keyword_separator" => ("*".to_string(), None),
                // The `/` separator binds nothing.
                _ => return None,
            };
            Some(Param {
                name,
                default: default.map(|d| value(d, src)),
                fields: Vec::new(),
            })
        })
        .collect()
}

/// Each parameter of a function `node` in order: the identifier it binds and its annotation.
fn parameters(node: Node) -> Vec<(Option<Node>, Option<Node>)> {
    let Some(list) = node.child_by_field_name("parameters") else {
        return Vec::new();
    };
    named(list)
        .filter_map(|p| {
            let pattern = match p.kind() {
                "default_parameter" | "typed_default_parameter" => p.child_by_field_name("name")?,
                "identifier"
                | "typed_parameter"
                | "list_splat_pattern"
                | "dictionary_splat_pattern" => p,
                // The bare `*` and `/` separators bind nothing.
                _ => return None,
            };
            Some((identifier(pattern), p.child_by_field_name("type")))
        })
        .collect()
}

/// How a name got its class. An annotation decides over the other ways; two other bindings
/// that disagree leave the class unknown.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum How {
    /// A parameter without an annotation: its argument is not known.
    Param,
    Assigned,
    /// An annotation, or the receiver of a method.
    Annotated,
}

/// The names each scope binds and what the syntax says of their class.
#[derive(Default)]
struct Locals {
    /// Every binding as written, in source order: its scope, name, class and how.
    written: Vec<(String, String, Class, How)>,
    /// The receiver parameter (`self`, `cls`) of each method, by the method's scope.
    receivers: HashMap<String, String>,
    /// The names a scope declares `global` (`true`) or `nonlocal` (`false`).
    declared: HashMap<(String, String), bool>,
    /// The scopes that are class bodies.
    classes: HashSet<String>,
    /// The value each `name = value` assigns, by the position of its binding in `written`.
    values: Vec<(usize, Value)>,
}

impl Locals {
    fn bind(&mut self, scope: &str, name: &str, class: Class, how: How) {
        (self.written).push((scope.to_string(), name.to_string(), class, how));
    }

    /// Binds a target: a name in `scope`, or `self.x` as the attribute `x` of the class whose
    /// method binds `self`, also from a function nested in the method. An attribute of any
    /// other object is not a binding.
    // ponytail: a write from outside the class (`obj.x = ...`) is not seen.
    fn target(&mut self, node: Node, scope: &str, class: Class, how: How, src: &[u8]) {
        match node.kind() {
            "identifier" => self.bind(scope, text(node, src), class, how),
            "attribute" => {
                let object = node.child_by_field_name("object");
                let attribute = node.child_by_field_name("attribute");
                let (Some(object), Some(attribute)) = (object, attribute) else {
                    return;
                };
                let mut method = scope;
                while !self.receivers.contains_key(method) {
                    if method.is_empty() || self.classes.contains(method) {
                        return;
                    }
                    method = parent(method);
                }
                if self.receivers[method] == text(object, src) {
                    let owner = parent(method).to_string();
                    self.bind(&owner, text(attribute, src), class, how);
                }
            }
            _ => {}
        }
    }

    /// The parameters of the function `node`, qualified `scope`: the first one of a method
    /// that is not static is its receiver.
    fn params(&mut self, node: Node, scope: &str, method: bool, src: &[u8]) {
        let receiver = method && !decorated(node, "staticmethod", src);
        for (n, (name, annotation)) in parameters(node).into_iter().enumerate() {
            let Some(name) = name.map(|name| text(name, src)) else {
                continue;
            };
            if n == 0 && receiver {
                self.receivers.insert(scope.to_string(), name.to_string());
                self.bind(scope, name, Class::Enclosing, How::Annotated);
            } else if let Some(annotation) = annotation {
                self.bind(scope, name, class(annotation, src), How::Annotated);
            } else {
                self.bind(scope, name, Class::Unknown, How::Param);
            }
        }
    }

    /// An assignment in `scope`: `x = C(...)` gives `x` what `C` names, `x: T` annotates `x`,
    /// `self.x: T` in a method annotates the class's attribute `x`, and any other target,
    /// `self.x` included, is bound to something unknown.
    fn assign(&mut self, node: Node, scope: &str, src: &[u8]) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        if self.extend(node, left, scope, src) {
            return;
        }
        let at = self.written.len();
        let right = node.child_by_field_name("right");
        if let Some(annotation) = node.child_by_field_name("type") {
            self.target(left, scope, class(annotation, src), How::Annotated, src);
        } else {
            let constructed = match (node.kind(), left.kind(), right) {
                ("assignment", "identifier", Some(right)) if right.kind() == "call" => {
                    constructor(right, src)
                }
                _ => None,
            };
            let class = constructed.map_or(Class::Unknown, Class::Named);
            for target in targets(left) {
                self.target(target, scope, class.clone(), How::Assigned, src);
            }
        }
        // A name bound by `name = value` holds that value. So does a `self` attribute, but
        // only a call: the attribute's constant is the class's, and a name in the value would
        // be read from the class, not from the method that writes it.
        // ponytail: an attribute assigned something else (`self.client = client`) holds no
        // value, so its origins are not followed.
        if let (true, Some(right), true) =
            (node.kind() == "assignment", right, self.written.len() > at)
            && matches!(left.kind(), "identifier" | "attribute")
        {
            let value = value(right, src);
            if left.kind() == "identifier" || matches!(value, Value::Call(_)) {
                self.values.push((at, value));
            }
        }
    }

    /// `with C(...) as x`: `x` holds what `C(...)` gives, as `x = C(...)` does.
    fn with_item(&mut self, node: Node, scope: &str, src: &[u8]) {
        let Some(alias) = node.child_by_field_name("alias") else {
            return;
        };
        let call = named(node).next().filter(|n| n.kind() == "call");
        let name = named(alias).next().filter(|n| n.kind() == "identifier");
        match (call, name) {
            (Some(call), Some(name)) => {
                let class = constructor(call, src).map_or(Class::Unknown, Class::Named);
                self.values.push((self.written.len(), value(call, src)));
                self.bind(scope, text(name, src), class, How::Assigned);
            }
            _ => {
                for target in targets(alias) {
                    self.target(target, scope, Class::Unknown, How::Assigned, src);
                }
            }
        }
    }

    /// `xs += ...` or `xs = xs + ...`, when the latest value `scope` assigned `xs` is a list
    /// literal: a list literal added joins its items, anything else added is left out, and
    /// `xs` is not bound again, so it stays a constant. `false` for any other assignment.
    // ponytail: an addition that is not a list literal (`+= static(...)`) is left out of the
    // list rather than making it unknown.
    fn extend(&mut self, node: Node, left: Node, scope: &str, src: &[u8]) -> bool {
        if left.kind() != "identifier" {
            return false;
        }
        let name = text(left, src);
        let operator = |n: Node| n.child_by_field_name("operator").map(|o| text(o, src));
        let Some(right) = node.child_by_field_name("right") else {
            return false;
        };
        let added = match node.kind() {
            "augmented_assignment" if operator(node) == Some("+=") => right,
            "assignment"
                if right.kind() == "binary_operator"
                    && operator(right) == Some("+")
                    && (right.child_by_field_name("left"))
                        .is_some_and(|l| l.kind() == "identifier" && text(l, src) == name) =>
            {
                match right.child_by_field_name("right") {
                    Some(added) => added,
                    None => return false,
                }
            }
            _ => return false,
        };
        let Locals {
            written, values, ..
        } = self;
        let latest = (values.iter_mut().rev())
            .find(|(at, _)| written[*at].0 == scope && written[*at].1 == name);
        let Some((_, Value::List(items))) = latest else {
            return false;
        };
        if let Value::List(more) = value(added, src) {
            items.extend(more);
        }
        true
    }

    /// The facts: one per scope and name, every binding but a plain parameter (its definition
    /// already lists it). A name declared `global` is bound at the module; one declared
    /// `nonlocal`, in the nearest enclosing function that binds it, class bodies skipped. An
    /// annotation decides; other bindings that disagree make the class unknown. A name its
    /// scope binds once, by `name = value`, is also a constant holding that value.
    fn finish(self) -> (Vec<Local>, Vec<Const>) {
        let own: HashSet<(&str, &str)> = (self.written.iter())
            .map(|(scope, name, ..)| (scope.as_str(), name.as_str()))
            .filter(|&(scope, name)| !self.declared.contains_key(&(scope.into(), name.into())))
            .collect();
        let home = |scope: &str, name: &str| -> String {
            match self.declared.get(&(scope.to_string(), name.to_string())) {
                None => scope.to_string(),
                Some(true) => String::new(),
                Some(false) => {
                    let mut at = parent(scope);
                    while !at.is_empty() {
                        if !self.classes.contains(at) && own.contains(&(at, name)) {
                            return at.to_string();
                        }
                        at = parent(at);
                    }
                    scope.to_string()
                }
            }
        };
        let annotated = |how| how == How::Annotated;
        let homes: Vec<(String, String)> = (self.written.iter())
            .map(|(scope, name, ..)| (home(scope, name), name.clone()))
            .collect();
        let mut times: HashMap<&(String, String), usize> = HashMap::new();
        for key in &homes {
            *times.entry(key).or_default() += 1;
        }
        let consts = (self.values.into_iter())
            .filter(|(at, _)| times[&homes[*at]] == 1 && homes[*at].0 == self.written[*at].0)
            .map(|(at, value)| Const {
                name: qualify(&homes[at].0, &homes[at].1),
                value,
            })
            .collect();
        let mut bound: BTreeMap<(String, String), (Class, How)> = BTreeMap::new();
        for (key, (_, _, class, how)) in homes.iter().zip(&self.written) {
            let key = key.clone();
            let merged = match bound.remove(&key) {
                None => (class.clone(), *how),
                Some(old) if annotated(old.1) && !annotated(*how) => old,
                Some(old) if annotated(*how) && !annotated(old.1) => (class.clone(), *how),
                Some((old, old_how)) if old == *class => (old, old_how.max(*how)),
                Some((_, old_how)) => (Class::Unknown, old_how.max(*how)),
            };
            bound.insert(key, merged);
        }
        let locals = (bound.into_iter())
            .filter(|(_, (_, how))| *how != How::Param)
            .map(|((scope, name), (class, _))| Local { scope, name, class })
            .collect();
        (locals, consts)
    }
}

/// What a `call` node constructs, as a callee-like name; `None` when its callee is no name.
fn constructor(call: Node, src: &[u8]) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    Some(callee(function, src)).filter(|f| !f.starts_with('('))
}

/// The scope enclosing `scope`, empty at the top level.
fn parent(scope: &str) -> &str {
    scope.rsplit_once('.').map_or("", |(parent, _)| parent)
}

/// The targets an assignment, a `for` or an `as` binds: names, and `self.x` attributes, also
/// inside `a, (b, c)`.
fn targets(node: Node) -> Vec<Node> {
    match node.kind() {
        "identifier" | "attribute" => vec![node],
        "pattern_list" | "tuple_pattern" | "list_pattern" | "tuple" | "list"
        | "as_pattern_target" => named(node).flat_map(targets).collect(),
        _ => Vec::new(),
    }
}

/// The names a `case` pattern captures: a bare name, the name after `as` or `*`, and those
/// inside sequences, mappings and class patterns; not a class's name or a dotted value.
fn captures<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
    match node.kind() {
        "dotted_name" => {
            let parts: Vec<Node> = named(node).collect();
            if let [name] = parts[..] {
                out.push(name);
            }
        }
        "identifier" => out.push(node),
        "class_pattern" | "keyword_pattern" => {
            named(node).skip(1).for_each(|n| captures(n, out));
        }
        _ => named(node).for_each(|n| captures(n, out)),
    }
}

/// What an annotation says, read as written: a name, or a string holding one, through
/// `Optional[...]` and `... | None`.
fn class(annotation: Node, src: &[u8]) -> Class {
    let node = bare(annotation);
    match node.kind() {
        "identifier" | "attribute" | "member_type" => dotted(text(node, src)),
        "string" => string(node, src).map_or(Class::Unknown, |s| dotted(&s)),
        "subscript" | "generic_type" => {
            let (name, inside): (_, Vec<Node>) = if node.kind() == "subscript" {
                let mut cursor = node.walk();
                let inside = node.children_by_field_name("subscript", &mut cursor);
                (node.child_by_field_name("value"), inside.collect())
            } else {
                let parameter = named(node).find(|n| n.kind() == "type_parameter");
                (
                    named(node).next(),
                    parameter.into_iter().flat_map(named).collect(),
                )
            };
            match (name.map(|n| text(n, src)), &inside[..]) {
                (Some("Optional" | "typing.Optional"), [one]) => class(*one, src),
                _ => Class::Unknown,
            }
        }
        "binary_operator" | "union_type" => {
            let operator = node.child_by_field_name("operator").map(|o| text(o, src));
            let sides: Vec<Node> = named(node).collect();
            match (operator, &sides[..]) {
                (None | Some("|"), [a, b]) if bare(*b).kind() == "none" => class(*a, src),
                (None | Some("|"), [a, b]) if bare(*a).kind() == "none" => class(*b, src),
                _ => Class::Unknown,
            }
        }
        _ => Class::Unknown,
    }
}

/// The expression a `type` node wraps.
fn bare(mut node: Node) -> Node {
    while node.kind() == "type"
        && let Some(inner) = named(node).next()
    {
        node = inner;
    }
    node
}

/// `Named` for a dotted name (`Circle`, `shapes.Circle`), whitespace aside.
fn dotted(name: &str) -> Class {
    let name: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let valid = name
        .split('.')
        .all(|part| !part.is_empty() && part.chars().all(|c| c == '_' || c.is_alphanumeric()));
    if valid {
        Class::Named(name)
    } else {
        Class::Unknown
    }
}

/// The bases a class `node` lists, as callee-like names; a generic base `B[T]` is `B`, and any
/// other expression is `(<node kind>)`, a base whose members are not known. Keywords such as
/// `metaclass=` are not bases.
fn bases(node: Node, src: &[u8]) -> Vec<String> {
    let Some(list) = node.child_by_field_name("superclasses") else {
        return Vec::new();
    };
    named(list)
        .filter(|base| !matches!(base.kind(), "keyword_argument" | "comment"))
        .map(|base| match base.kind() {
            "subscript" => base.child_by_field_name("value").unwrap_or(base),
            _ => base,
        })
        .map(|base| callee(base, src))
        .collect()
}

/// Whether the definition `node` carries the decorator `name`.
fn decorated(node: Node, name: &str, src: &[u8]) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == "decorated_definition")
        .is_some_and(|parent| {
            children(parent)
                .filter(|c| c.kind() == "decorator")
                .any(|d| named(d).next().is_some_and(|e| text(e, src) == name))
        })
}

/// The identifier a parameter pattern binds: `*args` and `x: int` bind `args` and `x`.
fn identifier(mut node: Node) -> Option<Node> {
    while node.kind() != "identifier" {
        node = named(node).next()?;
    }
    Some(node)
}

/// The bindings of `import a.b.c` (which binds `a`, `a.b` and `a.b.c`, each to its module),
/// `import a.b as c`, `from a.b import c as d` and `from a import *`. `source` is the module
/// as written: leading dots make it relative to the importing file's package.
fn imports(node: Node, src: &[u8], out: &mut Vec<Import>) {
    let mut add = |local: &str, source: &str, name| {
        out.push(Import {
            local: local.to_string(),
            source: source.to_string(),
            name,
        })
    };
    let mut cursor = node.walk();
    let names = node.children_by_field_name("name", &mut cursor);
    // The name an `aliased_import` imports, and the one it binds.
    let aliased = |item: Node| {
        let name = text(item.child_by_field_name("name")?, src);
        Some((name, text(item.child_by_field_name("alias")?, src)))
    };
    if node.kind() == "import_statement" {
        for item in names {
            if item.kind() == "aliased_import" {
                if let Some((name, alias)) = aliased(item) {
                    add(alias, name, Imported::Namespace);
                }
            } else {
                let name = text(item, src);
                for (end, _) in name.match_indices('.') {
                    add(&name[..end], &name[..end], Imported::Namespace);
                }
                add(name, name, Imported::Namespace);
            }
        }
        return;
    }
    let Some(module) = node.child_by_field_name("module_name") else {
        return;
    };
    let source = text(module, src);
    if children(node).any(|c| c.kind() == "wildcard_import") {
        add("*", source, Imported::All);
    }
    for item in names {
        let (name, local) = match item.kind() {
            "aliased_import" => match aliased(item) {
                Some(found) => found,
                None => continue,
            },
            _ => (text(item, src), text(item, src)),
        };
        // `from p import m` also names the submodule `m` of `p`, when `p` exports no `m`.
        let join = if source.ends_with('.') { "" } else { "." };
        let member = Imported::Member {
            name: name.to_string(),
            module: format!("{source}{join}{name}"),
        };
        add(local, source, member);
    }
}

/// The strings of a list or tuple literal of string literals; `None` for anything else.
fn string_list(node: Node, src: &[u8]) -> Option<Vec<String>> {
    if !matches!(node.kind(), "list" | "tuple") {
        return None;
    }
    named(node).map(|item| string(item, src)).collect()
}

/// The content of a string literal without prefix or quotes; `None` for another expression
/// or an f-string with interpolations.
fn string(node: Node, src: &[u8]) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let (mut start, mut end) = (None, None);
    for part in children(node) {
        match part.kind() {
            "string_start" => start = Some(part.end_byte()),
            "string_end" => end = Some(part.start_byte()),
            "interpolation" => return None,
            _ => {}
        }
    }
    let content = src.get(start?..end?)?;
    Some(String::from_utf8_lossy(content).into_owned())
}

/// The positional and keyword arguments of a call `node`, as written. A splat (`*a`, `**k`)
/// is not read.
// ponytail: a `*a` splat shifts the positions after it, which are read as if it held one value.
fn arguments(node: Node, src: &[u8]) -> (Vec<Value>, Vec<(String, Value)>) {
    let (mut args, mut kwargs) = (Vec::new(), Vec::new());
    let list = node.child_by_field_name("arguments");
    for arg in list
        .filter(|l| l.kind() == "argument_list")
        .into_iter()
        .flat_map(named)
    {
        match arg.kind() {
            "keyword_argument" => {
                let name = arg.child_by_field_name("name");
                if let (Some(name), Some(v)) = (name, arg.child_by_field_name("value")) {
                    kwargs.push((text(name, src).to_string(), value(v, src)));
                }
            }
            "comment" | "dictionary_splat" => {}
            _ => args.push(value(arg, src)),
        }
    }
    (args, kwargs)
}

/// What an expression is, as far as its own syntax tells.
fn value(node: Node, src: &[u8]) -> Value {
    match node.kind() {
        "identifier" => Value::Ref(text(node, src).to_string()),
        "none" => Value::Undefined,
        "attribute" => match callee(node, src) {
            name if name.starts_with('(') => Value::Unknown,
            name => Value::Ref(name),
        },
        "call" => parts(node, src).map_or(Value::Call(node.byte_range()), Value::Str),
        "list" | "tuple" => Value::List(named(node).map(|item| value(item, src)).collect()),
        "parenthesized_expression" => named(node).next().map_or(Value::Unknown, |n| value(n, src)),
        _ => parts(node, src).map_or(Value::Unknown, Value::Str),
    }
}

/// A string expression folded into literal text, names and holes: string literals, f-strings,
/// implicit and `+` concatenation; `None` when `node` is not one.
fn parts(node: Node, src: &[u8]) -> Option<Vec<Part>> {
    let inside = |n: Node| parts(n, src).unwrap_or_else(|| vec![Part::Hole]);
    match node.kind() {
        "string" => Some(
            children(node)
                .filter_map(|part| match part.kind() {
                    "string_content" => Some(Part::Text(text(part, src).to_string())),
                    "interpolation" => Some(match part.child_by_field_name("expression") {
                        Some(e) if e.kind() == "identifier" => Part::Ref(text(e, src).into()),
                        Some(e) => parts(e, src).and_then(single_env).unwrap_or(Part::Hole),
                        None => Part::Hole,
                    }),
                    _ => None,
                })
                .collect(),
        ),
        "concatenated_string" => Some(named(node).flat_map(inside).collect()),
        "binary_operator" => {
            let left = node.child_by_field_name("left")?;
            let right = node.child_by_field_name("right")?;
            let plus = text(node.child_by_field_name("operator")?, src) == "+";
            (plus && (parts(left, src).is_some() || parts(right, src).is_some()))
                .then(|| inside(left).into_iter().chain(inside(right)).collect())
        }
        // A fallback keeps the environment read on its left, if it is the only one.
        "boolean_operator" => {
            let left = node.child_by_field_name("left")?;
            let right = node.child_by_field_name("right")?;
            if text(node.child_by_field_name("operator")?, src) != "or" {
                return None;
            }
            let env = single_env(parts(left, src)?)?;
            let more = inside(right).iter().any(|p| matches!(p, Part::Env(_)));
            Some(vec![if more { Part::Hole } else { env }])
        }
        "identifier" => Some(vec![Part::Ref(text(node, src).to_string())]),
        "parenthesized_expression" => parts(named(node).next()?, src),
        "subscript" | "call" => env_read(node, src).map(|env| vec![env]),
        _ => None,
    }
}

/// `os.environ["X"]`, `os.environ.get("X")` or `os.getenv("X")` (also `environ` and `getenv`
/// imported from `os`), with or without a default, also through string method calls such as
/// `.rstrip("/")`. Meada never reads the value, so a default is ignored; one that reads
/// another variable makes the whole read a hole.
// ponytail: the names are matched as written, not through the file's imports, so `import os
// as o` is not read and a local `getenv` function is.
fn env_read(node: Node, src: &[u8]) -> Option<Part> {
    const STRING_METHODS: &[&str] = &[
        "strip",
        "lstrip",
        "rstrip",
        "replace",
        "removeprefix",
        "removesuffix",
        "lower",
    ];
    match node.kind() {
        "subscript" => {
            let name = string(node.child_by_field_name("subscript")?, src)?;
            let environ = callee(node.child_by_field_name("value")?, src);
            matches!(environ.as_str(), "os.environ" | "environ").then_some(Part::Env(name))
        }
        "call" => {
            let function = node.child_by_field_name("function")?;
            if let "os.getenv" | "getenv" | "os.environ.get" | "environ.get" =
                callee(function, src).as_str()
            {
                let (args, kwargs) = arguments(node, src);
                let name = string(named(node.child_by_field_name("arguments")?).next()?, src)?;
                let default = args
                    .get(1)
                    .or(kwargs.iter().find(|(k, _)| k == "default").map(|(_, v)| v));
                return Some(match default {
                    Some(Value::Str(p)) if p.iter().any(|p| matches!(p, Part::Env(_))) => {
                        Part::Hole
                    }
                    _ => Part::Env(name),
                });
            }
            let method = function.child_by_field_name("attribute")?;
            if !STRING_METHODS.contains(&text(method, src)) {
                return None;
            }
            single_env(parts(function.child_by_field_name("object")?, src)?)
        }
        _ => None,
    }
}

/// The environment read that makes up all of `parts`, if that is what they are.
fn single_env(parts: Vec<Part>) -> Option<Part> {
    match <[Part; 1]>::try_from(parts) {
        Ok([env @ Part::Env(_)]) => Some(env),
        _ => None,
    }
}

/// `a.b.c` for identifier and attribute chains; any other expression becomes `(<node kind>)`,
/// so no source text beyond names reaches the index.
fn callee(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "identifier" => text(node, src).to_string(),
        "attribute" => match (
            node.child_by_field_name("object"),
            node.child_by_field_name("attribute"),
        ) {
            (Some(object), Some(attribute)) => {
                format!("{}.{}", callee(object, src), text(attribute, src))
            }
            _ => format!("({})", node.kind()),
        },
        kind => format!("({kind})"),
    }
}
