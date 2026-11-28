use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};

use super::{
    Binds, Call, Class, Const, Def, Export, Facts, Import, Imported, Invocation, Kind,
    LanguagePack, Modules, Param, Part, Receiver, Star, TYPE_DEPTH, Value, children, literal,
    named, text,
};
use crate::package::read_manifest;
use crate::{Result, parent, qualify};

pub mod pyproject;

// ponytail: real code nests a few levels; 128 is the depth the TypeScript pack allows.
const DEFINITION_DEPTH: usize = 128;

// ponytail: real code unpacks a few levels; the recursion must stay shallow, whatever the file.
const TARGET_DEPTH: usize = 64;

// ponytail: real code nests a few levels of options; 64 is far past any.
const VALUE_DEPTH: usize = 64;

// ponytail: real code concatenates and chains a few dozen terms; the recursion must stay
// shallow, whatever the file.
const FOLD_DEPTH: usize = 256;

const MAIN: &str = "__main__";

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
        let mut facts = Facts {
            member_writes: Some(Vec::new()),
            ..Facts::default()
        };
        let mut write_bytes = 0;
        let mut module = Module::default();
        let mut defined = HashSet::new();
        let mut locals = Locals::default();
        let mut functions: HashMap<String, usize> = HashMap::new();
        let mut decorators: HashMap<usize, String> = HashMap::new();
        let mut bare_names: HashMap<String, Vec<String>> = HashMap::new();
        let mut stack = vec![Visit {
            node: tree.root_node(),
            scope: String::new(),
            in_class: false,
            flat: false,
            awaited: false,
        }];
        while let Some(Visit {
            node,
            scope,
            in_class,
            flat,
            awaited,
        }) = stack.pop()
        {
            let mut inner = scope.clone();
            let mut inner_class = in_class;
            let mut inner_flat = flat;
            // ponytail: a name assigned in the guard is a local of `__main__`, so it no longer
            // rebinds the module's global of that name (it is no export or constant of the
            // file); a script keeps its globals outside the guard in practice.
            let mut main_body = None;
            match node.kind() {
                "if_statement" if scope.is_empty() && is_main_guard(node, src) => {
                    let body = node.child_by_field_name("consequence");
                    if let Some(body) = body.filter(|_| defined.insert(MAIN.to_string())) {
                        main_body = Some(body.id());
                        facts.defs.push(Def::new(
                            MAIN,
                            Kind::Function,
                            node.start_byte()..body.end_byte(),
                        ));
                        facts.programs.push(MAIN.to_string());
                    }
                }
                "function_definition" | "class_definition"
                    if levels(&scope) >= DEFINITION_DEPTH =>
                {
                    let name = node.child_by_field_name("name");
                    let params = parameters(node).into_iter().filter_map(|(name, _)| name);
                    for name in name.into_iter().chain(params) {
                        locals.bind(&scope, text(name, src), Class::Unknown, Evidence::Assigned);
                    }
                    inner_flat = true;
                }
                "function_definition" | "class_definition" => {
                    let is_class = node.kind() == "class_definition";
                    if let Some(name) = node.child_by_field_name("name") {
                        let name = text(name, src);
                        inner = qualify(&scope, name);
                        let in_place = in_place(node, &scope, src);
                        match scope.is_empty() {
                            true => module.def(name, in_place),
                            false => locals.def(&scope, name, in_place),
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
                                params: params(node, !is_class && in_class, src),
                                bases: bases(node, src),
                                decorators: bare_names.remove(&inner).unwrap_or_default(),
                                asynchronous: !is_class
                                    && children(node).any(|n| n.kind() == "async"),
                                return_values: if !is_class
                                    && !node
                                        .child_by_field_name("body")
                                        .is_some_and(|body| exits(body, 0))
                                {
                                    vec![Value::Unknown]
                                } else {
                                    Vec::new()
                                },
                                ..Def::new(inner.clone(), kind, span(node))
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
                "decorated_definition" if levels(&scope) < DEFINITION_DEPTH => {
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
                                    let names = bare_names.entry(def.clone()).or_default();
                                    names.push(callee(e, src));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "call" => {
                    if let Some(function) = node.child_by_field_name("function") {
                        let callee = callee(function, src);
                        if let Some(method) = callee.strip_prefix("__all__.") {
                            module.call_all(method, node, src);
                        }
                        let (args, kwargs) = arguments(node, src);
                        facts.calls.push(Call {
                            arity: None,
                            awaited,
                            labels: None,
                            constructs: false,
                            scope: scope.clone(),
                            callee,
                            range: node.byte_range(),
                            args,
                            kwargs,
                            decorates: decorators.remove(&node.id()),
                            by_path: false,
                            root: None,
                            on: None,
                            unwraps: false,
                            awaits: false,
                            receiver: None,
                            arg_types: Vec::new(),
                            uses: Vec::new(),
                        });
                    }
                }
                "return_statement" if !flat => {
                    if let Some(&at) = functions.get(&scope) {
                        let returned = named(node).next().map_or(Value::Unknown, |n| value(n, src));
                        facts.defs[at].return_values.push(returned);
                    }
                }
                "yield" if !flat => {
                    if let Some(&at) = functions.get(&scope) {
                        facts.defs[at].return_values.push(Value::Unknown);
                    }
                }
                "import_statement" | "import_from_statement" => {
                    let first = facts.imports.len();
                    imports(node, src, &mut facts.imports);
                    if scope.is_empty() {
                        module.import(&facts.imports[first..]);
                    } else {
                        for import in &facts.imports[first..] {
                            if !import.local.contains('.') && import.local != "*" {
                                locals.bind(
                                    &scope,
                                    &import.local,
                                    Class::Imported,
                                    Evidence::Assigned,
                                );
                            }
                        }
                        for import in &mut facts.imports[first..] {
                            import.local = qualify(&scope, &import.local);
                        }
                    }
                }
                "assignment" | "augmented_assignment" => {
                    let left = node.child_by_field_name("left");
                    for target in left.into_iter().flat_map(targets) {
                        if facts.member_writes.is_none() {
                            break;
                        }
                        if let Some(write) = selector_write(target, src, &scope, node.byte_range())
                        {
                            facts.member_write(write, &mut write_bytes);
                        }
                    }
                    module.items(&left.map(targets).unwrap_or_default(), src);
                    if scope.is_empty() {
                        module.assign(node, src);
                    }
                    locals.assign(node, &scope, src);
                }
                "as_pattern" if node.parent().is_some_and(|p| p.kind() == "with_item") => {
                    if let Some(alias) = node.child_by_field_name("alias")
                        && scope.is_empty()
                    {
                        module.targets(alias, src);
                    }
                    locals.with_item(node, &scope, src);
                }
                // ponytail: a comprehension's variables are bound in the enclosing scope, so
                // they hide a name there for the whole scope. At the top level, as a lambda's
                // parameters, they hide nothing (the module's hidden names are `Module`'s,
                // which takes neither): `[h() for h in hs]` beside `def h()` stays `exact` to
                // it. Hiding `h` there alone needs a scope of the comprehension's own.
                "for_statement" | "for_in_clause" | "as_pattern" | "except_clause"
                | "named_expression" => {
                    let field = match node.kind() {
                        "for_statement" | "for_in_clause" => "left",
                        "named_expression" => "name",
                        _ => "alias",
                    };
                    let target = node.child_by_field_name(field);
                    if let Some(target) = target
                        && scope.is_empty()
                        && node.kind() != "for_in_clause"
                    {
                        module.targets(target, src);
                    }
                    for target in target.map(targets).unwrap_or_default() {
                        locals.target(target, &scope, Class::Unknown, Evidence::Assigned, src);
                    }
                }
                "case_clause" => {
                    let mut found = Vec::new();
                    for pattern in named(node).filter(|n| n.kind() == "case_pattern") {
                        captures(pattern, 0, &mut found);
                    }
                    for name in found {
                        let name = text(name, src);
                        if scope.is_empty() {
                            module.name(name);
                        }
                        locals.bind(&scope, name, Class::Unknown, Evidence::Assigned);
                    }
                }
                "lambda" => {
                    for (name, _) in parameters(node) {
                        if let Some(name) = name {
                            locals.bind(
                                &scope,
                                text(name, src),
                                Class::Unknown,
                                Evidence::Assigned,
                            );
                        }
                    }
                }
                // ponytail: `del` of a `global` name in a function hides it in that function
                // alone.
                "delete_statement" => {
                    for target in named(node).flat_map(targets) {
                        if facts.member_writes.is_none() {
                            break;
                        }
                        if let Some(write) = selector_write(target, src, &scope, node.byte_range())
                        {
                            facts.member_write(write, &mut write_bytes);
                        }
                    }
                    let deleted: Vec<Node> = named(node).flat_map(targets).collect();
                    module.items(&deleted, src);
                    for name in deleted.into_iter().filter(|t| t.kind() == "identifier") {
                        let name = text(name, src);
                        match scope.is_empty() {
                            true => module.del(name),
                            false => facts.locals.push(qualify(&scope, name)),
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
                scope: match main_body == Some(child.id()) {
                    true => MAIN.to_string(),
                    false => inner.clone(),
                },
                in_class: inner_class,
                flat: inner_flat,
                awaited: node.kind() == "await"
                    || (node.kind() == "parenthesized_expression" && awaited),
            }));
        }
        for (scope, name, ..) in &locals.written {
            if locals.declared.get(&(scope.clone(), name.clone())) == Some(&true) {
                module.name(name);
            }
        }
        facts.locals.extend(module.hidden.iter().cloned());
        (facts.exports, facts.star) = module.exports();
        locals.finish(&mut facts);
        let settings = settings_consts(tree.root_node(), src);
        facts
            .consts
            .retain(|c| !settings.iter().any(|s| s.name == c.name));
        facts.consts.extend(settings);
        facts.loops = super::outermost(tree.root_node(), &["for_statement", "while_statement"]);
        Ok(facts)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["**/pyproject.toml"]
    }

    fn stars_bind(&self) -> bool {
        true
    }

    fn modules(&self, configs: &[(&str, &[u8])], _: &BTreeMap<&str, &Facts>) -> Result<Modules> {
        let mut by_dir: HashMap<String, Vec<String>> = (configs.iter())
            .map(|(path, src)| {
                let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
                let declared = read_manifest(path, src).roots;
                (dir.to_string(), pyproject::source_roots(dir, &declared))
            })
            .collect();
        by_dir
            .entry(String::new())
            .or_insert_with(|| pyproject::source_roots("", &[]));
        Ok(Box::new(move |importer, specifier| {
            let dots = specifier.bytes().take_while(|&b| b == b'.').count();
            let name = specifier[dots..].replace('.', "/");
            let bases = if dots == 0 {
                let mut dir = Some(importer.rsplit_once('/').map_or("", |(dir, _)| dir));
                let mut roots: Vec<String> = Vec::new();
                while let Some(d) = dir {
                    for root in by_dir.get(d).into_iter().flatten() {
                        if !roots.contains(root) {
                            roots.push(root.clone());
                        }
                    }
                    dir = d
                        .rsplit_once('/')
                        .map_or((!d.is_empty()).then_some(""), |(p, _)| Some(p));
                }
                roots
            } else {
                let dir = importer.rsplit_once('/').map_or("", |(dir, _)| dir);
                let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
                let Some(keep) = parts.len().checked_sub(dots - 1) else {
                    return Rc::default();
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
    in_class: bool,
    flat: bool,
    awaited: bool,
}

#[derive(Default)]
struct Module {
    bindings: Vec<Export>,
    bound: HashSet<String>,
    star: bool,
    defs: HashSet<String>,
    hidden: BTreeSet<String>,
    all: Option<Star>,
}

impl Module {
    fn def(&mut self, name: &str, in_place: bool) {
        let bound = self.star || self.bound.contains(name);
        self.name(name);
        self.defs.insert(name.to_string());
        match in_place || !bound {
            true => self.hidden.remove(name),
            false => self.hidden.insert(name.to_string()),
        };
    }

    fn del(&mut self, name: &str) {
        self.name(name);
        self.hidden.insert(name.to_string());
    }

    // ponytail: `__all__ = __all__ + [...]` is not folded; it is computed.
    fn grow_all(&mut self, added: Option<Vec<String>>) {
        self.all = Some(match (self.all.take(), added) {
            (Some(Star::Only(mut names)), Some(added)) => {
                names.extend(added);
                Star::Only(names)
            }
            _ => Star::Computed,
        });
    }

    fn import(&mut self, imports: &[Import]) {
        for import in imports {
            match import.name {
                Imported::All => {
                    self.star = true;
                    self.bindings.push(Export::All {
                        source: import.source.clone(),
                    });
                }
                _ if !import.local.contains('.') => self.name(&import.local),
                _ => {}
            }
        }
    }

    fn name(&mut self, name: &str) {
        self.bound.insert(name.to_string());
        self.bindings.push(Export::Local {
            exported: name.to_string(),
            local: name.to_string(),
        });
        match self.defs.contains(name) {
            true => self.hidden.insert(name.to_string()),
            false => self.hidden.remove(name),
        };
        if name == "__all__" {
            self.all = Some(Star::Computed);
        }
    }

    fn items(&mut self, targets: &[Node], src: &[u8]) {
        let item = |t: &Node| {
            t.kind() == "subscript"
                && (t.child_by_field_name("value")).is_some_and(|v| text(v, src) == "__all__")
        };
        if targets.iter().any(item) {
            self.all = Some(Star::Computed);
        }
    }

    fn targets(&mut self, node: Node, src: &[u8]) {
        for target in targets(node)
            .into_iter()
            .filter(|t| t.kind() == "identifier")
        {
            self.name(text(target, src));
        }
    }

    fn assign(&mut self, node: Node, src: &[u8]) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        let all = left.kind() == "identifier" && text(left, src) == "__all__";
        let before = all.then(|| self.all.take()).flatten();
        self.targets(left, src);
        if !all {
            return;
        }
        let right = node.child_by_field_name("right");
        let names = right.and_then(|right| string_list(right, src));
        self.all = before;
        match node.kind() {
            _ if !statement(node) => self.all = Some(Star::Computed),
            "assignment" => self.all = Some(names.map_or(Star::Computed, Star::Only)),
            _ => {
                let operator = node.child_by_field_name("operator").map(|o| text(o, src));
                self.grow_all(names.filter(|_| operator == Some("+=")));
            }
        }
    }

    fn call_all(&mut self, method: &str, call: Node, src: &[u8]) {
        let args = call.child_by_field_name("arguments");
        let args: Vec<Node> = args.map(|a| named(a).collect()).unwrap_or_default();
        let added = match (statement(call), method, &args[..]) {
            (true, "extend", [list]) => string_list(*list, src),
            (true, "append", [name]) => string(*name, src).map(|name| vec![name]),
            _ => None,
        };
        self.grow_all(added);
    }

    fn exports(self) -> (Vec<Export>, Star) {
        let mut seen = HashSet::new();
        let exports = (self.bindings.into_iter().rev())
            .filter(|e| match e {
                Export::Local { exported, .. } => seen.insert(exported.clone()),
                _ => true,
            })
            .collect();
        (exports, self.all.unwrap_or(Star::Public))
    }
}

pub(crate) fn files(base: &str, name: &str) -> Vec<String> {
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

fn exits(node: Node, depth: usize) -> bool {
    if depth >= VALUE_DEPTH {
        return false;
    }
    match node.kind() {
        "return_statement" | "raise_statement" => true,
        "block" => named(node)
            .filter(|n| n.kind() != "comment")
            .last()
            .is_some_and(|n| exits(n, depth + 1)),
        "if_statement" => {
            let alternatives: Vec<_> = named(node)
                .filter(|n| matches!(n.kind(), "elif_clause" | "else_clause"))
                .collect();
            node.child_by_field_name("consequence")
                .is_some_and(|n| exits(n, depth + 1))
                && alternatives
                    .last()
                    .is_some_and(|n| n.kind() == "else_clause")
                && alternatives.into_iter().all(|n| exits(n, depth + 1))
        }
        "elif_clause" => node
            .child_by_field_name("consequence")
            .is_some_and(|n| exits(n, depth + 1)),
        "else_clause" => node
            .child_by_field_name("body")
            .is_some_and(|n| exits(n, depth + 1)),
        _ => false,
    }
}

fn is_main_guard(node: Node, src: &[u8]) -> bool {
    let Some(condition) = node.child_by_field_name("condition") else {
        return false;
    };
    let operands: Vec<Node> = named(condition).collect();
    let [left, right] = operands.as_slice() else {
        return false;
    };
    let operator = &src[left.end_byte()..right.start_byte()];
    let (left, right) = (text(*left, src), text(*right, src));
    condition.kind() == "comparison_operator"
        && operator.trim_ascii() == b"=="
        && matches!(
            (left, right),
            ("__name__", "\"__main__\"" | "'__main__'")
                | ("\"__main__\"" | "'__main__'", "__name__")
        )
}

fn in_place(node: Node, scope: &str, src: &[u8]) -> bool {
    let mut parent = node.parent();
    if let Some(p) = parent.filter(|p| p.kind() == "decorated_definition") {
        parent = p.parent();
    }
    let Some(parent) = parent else {
        return false;
    };
    let owner = parent.parent().filter(|_| parent.kind() == "block");
    parent.kind() == "module"
        || owner.is_some_and(|o| match o.kind() {
            "function_definition" | "class_definition" => true,
            "if_statement" => {
                scope == MAIN
                    && o.parent().is_some_and(|m| m.kind() == "module")
                    && is_main_guard(o, src)
            }
            _ => false,
        })
}

fn statement(node: Node) -> bool {
    let parent = node.parent().filter(|p| p.kind() == "expression_statement");
    parent
        .and_then(|p| p.parent())
        .is_some_and(|m| m.kind() == "module")
}

fn levels(scope: &str) -> usize {
    match scope {
        "" => 0,
        scope => scope.matches('.').count() + 1,
    }
}

fn span(node: Node) -> Range<usize> {
    match node.parent() {
        Some(parent) if parent.kind() == "decorated_definition" => parent.byte_range(),
        _ => node.byte_range(),
    }
}

fn params(node: Node, method: bool, src: &[u8]) -> Vec<Param> {
    let Some(list) = node.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut receiver = match method && !decorated(node, "staticmethod", src) {
        false => None,
        true if decorated(node, "classmethod", src) => Some(Binds::Class),
        true => Some(Binds::Instance),
    };
    let mut keyword = false;
    let mut out = Vec::new();
    for p in named(list) {
        let inner = match p.kind() {
            "typed_parameter" => named(p).next().unwrap_or(p),
            _ => p,
        };
        let (name, default, binds) = match inner.kind() {
            "default_parameter" | "typed_default_parameter" => (
                p.child_by_field_name("name"),
                p.child_by_field_name("value"),
                None,
            ),
            "identifier" => (Some(inner), None, None),
            "list_splat_pattern" | "dictionary_splat_pattern" => {
                keyword = true;
                (Some(inner), None, Some(Binds::Rest))
            }
            "keyword_separator" => {
                keyword = true;
                continue;
            }
            _ => continue,
        };
        let Some(name) = name.and_then(identifier) else {
            continue;
        };
        let binds = binds.or(receiver.take()).unwrap_or(match keyword {
            true => Binds::Keyword,
            false => Binds::Any,
        });
        out.push(Param {
            name: text(name, src).to_string(),
            binds,
            label: None,
            default: default.map(|d| value(d, src)),
            fields: Vec::new(),
            closure: Default::default(),
        });
    }
    out
}

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
                _ => return None,
            };
            Some((identifier(pattern), p.child_by_field_name("type")))
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Evidence {
    Param,
    Assigned,
    Annotated,
}

#[derive(Default)]
struct Locals {
    written: Vec<(String, String, Class, Evidence)>,
    receivers: HashMap<String, String>,
    declared: HashMap<(String, String), bool>,
    classes: HashSet<String>,
    defs: Vec<(usize, String, String, bool)>,
    values: Vec<(usize, Value)>,
    latest: HashMap<(String, String), usize>,
    query_initial: HashSet<usize>,
    query_writes: Vec<(usize, super::QueryWrite)>,
    bound: HashSet<(String, String)>,
}

impl Locals {
    fn bind(&mut self, scope: &str, name: &str, class: Class, how: Evidence) {
        self.bound.insert((scope.to_string(), name.to_string()));
        (self.written).push((scope.to_string(), name.to_string(), class, how));
    }

    fn def(&mut self, scope: &str, name: &str, in_place: bool) {
        let key = (scope.to_string(), name.to_string());
        let bound = self.bound.contains(&key);
        let at = self.written.len();
        (self.defs).push((at, scope.to_string(), name.to_string(), !in_place && bound));
        self.bound.insert(key);
        if self
            .declared
            .contains_key(&(scope.to_string(), name.to_string()))
        {
            self.bind(scope, name, Class::Unknown, Evidence::Assigned);
        }
    }

    // ponytail: a write from outside the class (`obj.x = ...`) is not seen.
    fn target(&mut self, node: Node, scope: &str, class: Class, how: Evidence, src: &[u8]) {
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

    fn params(&mut self, node: Node, scope: &str, method: bool, src: &[u8]) {
        let receiver = method && !decorated(node, "staticmethod", src);
        for (n, (name, annotation)) in parameters(node).into_iter().enumerate() {
            let Some(name) = name.map(|name| text(name, src)) else {
                continue;
            };
            if n == 0 && receiver {
                self.receivers.insert(scope.to_string(), name.to_string());
                self.bind(scope, name, Class::Enclosing, Evidence::Annotated);
            } else if let Some(annotation) = annotation {
                self.bind(scope, name, class(annotation, src), Evidence::Annotated);
            } else {
                self.bind(scope, name, Class::Unknown, Evidence::Param);
            }
        }
    }

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
            self.target(
                left,
                scope,
                class(annotation, src),
                Evidence::Annotated,
                src,
            );
        } else {
            let constructed = match (node.kind(), left.kind(), right) {
                ("assignment", "identifier", Some(right)) if right.kind() == "call" => {
                    constructor(right, src)
                }
                _ => None,
            };
            let class = constructed.map_or(Class::Unknown, Class::Named);
            for target in targets(left) {
                self.target(target, scope, class.clone(), Evidence::Assigned, src);
            }
        }
        // ponytail: an attribute assigned something else (`self.client = client`) holds no
        // value, so its origins are not followed.
        if let (true, Some(right), true) =
            (node.kind() == "assignment", right, self.written.len() > at)
            && matches!(left.kind(), "identifier" | "attribute")
        {
            let value = value(right, src);
            if left.kind() == "identifier" || matches!(value, Value::Call(_)) {
                if left.kind() == "identifier" {
                    let change = super::QueryWrite::of(&value, text(left, src), "=");
                    if change != super::QueryWrite::Other {
                        self.query_writes.push((at, change));
                    }
                    if node
                        .parent()
                        .and_then(|s| s.parent())
                        .and_then(|b| b.parent())
                        .is_some_and(|p| p.kind() == "function_definition")
                    {
                        self.query_initial.insert(at);
                    }
                }
                self.assigned(at, value);
            }
        } else if node.kind() == "augmented_assignment"
            && left.kind() == "identifier"
            && let Some(right) = right
            && let Some(op) = node.child_by_field_name("operator")
        {
            let change = super::QueryWrite::of(&value(right, src), text(left, src), text(op, src));
            if change != super::QueryWrite::Other {
                self.query_writes.push((at, change));
            }
        }
    }

    fn assigned(&mut self, at: usize, value: Value) {
        let (scope, name, ..) = &self.written[at];
        let key = (scope.clone(), name.clone());
        self.latest.insert(key, self.values.len());
        self.values.push((at, value));
    }

    fn with_item(&mut self, node: Node, scope: &str, src: &[u8]) {
        let Some(alias) = node.child_by_field_name("alias") else {
            return;
        };
        let call = named(node).next().filter(|n| n.kind() == "call");
        let name = named(alias).next().filter(|n| n.kind() == "identifier");
        match (call, name) {
            (Some(call), Some(name)) => {
                let class = constructor(call, src).map_or(Class::Unknown, Class::Named);
                let (at, value) = (self.written.len(), value(call, src));
                self.bind(scope, text(name, src), class, Evidence::Assigned);
                self.assigned(at, value);
            }
            _ => {
                for target in targets(alias) {
                    self.target(target, scope, Class::Unknown, Evidence::Assigned, src);
                }
            }
        }
    }

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
        let latest = (self.latest.get(&(scope.to_string(), name.to_string()))).copied();
        let Some((_, Value::List(items))) = latest.map(|i| &mut self.values[i]) else {
            return false;
        };
        if let Value::List(more) = value(added, src) {
            items.extend(more);
        }
        true
    }

    fn finish(self, facts: &mut Facts) {
        let defs = (self.defs.iter()).map(|(_, scope, name, _)| (scope.as_str(), name.as_str()));
        let own: HashSet<(&str, &str)> = (self.written.iter())
            .map(|(scope, name, ..)| (scope.as_str(), name.as_str()))
            .chain(defs)
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
        let annotated = |how| how == Evidence::Annotated;
        let homes: Vec<(String, String)> = (self.written.iter())
            .map(|(scope, name, ..)| (home(scope, name), name.clone()))
            .collect();
        let mut times: HashMap<&(String, String), usize> = HashMap::new();
        let mut first = HashMap::new();
        for (at, key) in homes.iter().enumerate() {
            *times.entry(key).or_default() += 1;
            first.entry(key).or_insert(at);
        }
        // ponytail: so a class-body `m = None` after `def m` leaves `self.m()` linked to it;
        // `Resolver::member` would have to read hidden names, at the cost of
        // `m = staticmethod(m)`'s right link.
        let mut last: HashMap<(&str, &str), usize> = HashMap::new();
        for (at, (home, name)) in homes.iter().enumerate() {
            let at = match *home == self.written[at].0 {
                true => at,
                false => usize::MAX,
            };
            let latest = last.entry((home.as_str(), name.as_str())).or_default();
            *latest = (*latest).max(at);
        }
        let mut defined: BTreeMap<(&str, &str), (usize, bool)> = BTreeMap::new();
        for (at, scope, name, kept) in &self.defs {
            if !self.classes.contains(scope) {
                defined.insert((scope.as_str(), name.as_str()), (*at, *kept));
            }
        }
        for def in &mut facts.defs {
            def.reassigned_params = (def.params.iter())
                .filter(|p| {
                    times.get(&(def.name.clone(), p.name.clone())) > Some(&1)
                        || defined.contains_key(&(def.name.as_str(), p.name.as_str()))
                })
                .map(|p| p.name.clone())
                .collect();
        }
        let hidden = (defined.into_iter())
            .filter(|(key, (at, kept))| *kept || last.get(key).is_some_and(|latest| latest >= at))
            .map(|((scope, name), _)| qualify(scope, name));
        facts.locals.extend(hidden);
        let mut queries = HashMap::new();
        for (at, change) in self.query_writes {
            if first[&homes[at]] == at || homes[at].0 != self.written[at].0 {
                continue;
            }
            let entry = queries.entry(&homes[at]).or_insert((0, change));
            entry.0 += 1;
            entry.1 = entry.1.merge(change);
        }
        facts.consts = (self.values.into_iter())
            .filter_map(|(at, mut value)| {
                let home = &homes[at];
                let once = times[home] == 1;
                let is_first = first[home] == at;
                let mut query = || {
                    queries.get(home).is_some_and(|(writes, change)| {
                        *writes + 1 == times[home] && change.keep(&mut value)
                    })
                };
                if home.0 != self.written[at].0
                    || (!once && !(self.query_initial.contains(&at) && is_first && query()))
                {
                    return None;
                }
                Some(Const {
                    name: qualify(&home.0, &home.1),
                    value,
                    ty: None,
                    scope: None,
                })
            })
            .collect();
        let mut bound: BTreeMap<(String, String), (Class, Evidence)> = BTreeMap::new();
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
        facts.receivers = (bound.into_iter())
            .filter(|(_, (_, how))| *how != Evidence::Param)
            .map(|((scope, name), (class, _))| Receiver { scope, name, class })
            .collect();
    }
}

fn constructor(call: Node, src: &[u8]) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    Some(callee(function, src)).filter(|f| !f.starts_with('('))
}

fn targets(node: Node) -> Vec<Node> {
    let mut out = Vec::new();
    targets_at(node, 0, &mut out);
    out
}

fn selector_write(
    node: Node,
    src: &[u8],
    scope: &str,
    range: Range<usize>,
) -> Option<super::MemberWrite> {
    let (object, field) = match node.kind() {
        "attribute" => (
            node.child_by_field_name("object")?,
            Some(text(node.child_by_field_name("attribute")?, src).to_string()),
        ),
        "subscript" => (
            node.child_by_field_name("value")?,
            node.child_by_field_name("subscript")
                .and_then(|n| parts(n, src))
                .and_then(|p| literal(&p)),
        ),
        _ => return None,
    };
    Some(super::MemberWrite {
        scope: scope.into(),
        object: callee(object, src),
        field,
        range,
    })
}

fn targets_at<'t>(node: Node<'t>, depth: usize, out: &mut Vec<Node<'t>>) {
    match node.kind() {
        _ if depth >= TARGET_DEPTH => {}
        "identifier" | "attribute" | "subscript" => out.push(node),
        "pattern_list"
        | "tuple_pattern"
        | "list_pattern"
        | "tuple"
        | "list"
        | "as_pattern_target"
        | "expression_list"
        | "list_splat_pattern"
        | "list_splat"
        | "parenthesized_expression" => named(node).for_each(|n| targets_at(n, depth + 1, out)),
        _ => {}
    }
}

fn captures<'t>(node: Node<'t>, depth: usize, out: &mut Vec<Node<'t>>) {
    match node.kind() {
        _ if depth >= TARGET_DEPTH => {}
        "dotted_name" => {
            let parts: Vec<Node> = named(node).collect();
            if let [name] = parts[..] {
                out.push(name);
            }
        }
        "identifier" => out.push(node),
        "class_pattern" | "keyword_pattern" => {
            named(node)
                .skip(1)
                .for_each(|n| captures(n, depth + 1, out));
        }
        _ => named(node).for_each(|n| captures(n, depth + 1, out)),
    }
}

fn class(annotation: Node, src: &[u8]) -> Class {
    class_at(annotation, src, 0)
}

fn class_at(annotation: Node, src: &[u8], depth: usize) -> Class {
    if depth >= TYPE_DEPTH {
        return Class::Unknown;
    }
    let class = |n: Node| class_at(n, src, depth + 1);
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
                (Some("Optional" | "typing.Optional"), [one]) => class(*one),
                _ => Class::Unknown,
            }
        }
        "binary_operator" | "union_type" => {
            let operator = node.child_by_field_name("operator").map(|o| text(o, src));
            let sides: Vec<Node> = named(node).collect();
            match (operator, &sides[..]) {
                (None | Some("|"), [a, b]) if bare(*b).kind() == "none" => class(*a),
                (None | Some("|"), [a, b]) if bare(*a).kind() == "none" => class(*b),
                _ => Class::Unknown,
            }
        }
        _ => Class::Unknown,
    }
}

fn bare(mut node: Node) -> Node {
    while node.kind() == "type"
        && let Some(inner) = named(node).next()
    {
        node = inner;
    }
    node
}

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

fn decorated(node: Node, name: &str, src: &[u8]) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == "decorated_definition")
        .is_some_and(|parent| {
            children(parent)
                .filter(|c| c.kind() == "decorator")
                .any(|d| named(d).next().is_some_and(|e| text(e, src) == name))
        })
}

fn identifier(mut node: Node) -> Option<Node> {
    while node.kind() != "identifier" {
        node = named(node).next()?;
    }
    Some(node)
}

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
        let join = if source.ends_with('.') { "" } else { "." };
        let member = Imported::Member {
            name: name.to_string(),
            module: format!("{source}{join}{name}"),
        };
        add(local, source, member);
    }
}

// ponytail: only a top-level base class in the same file, a literal prefix or alias; `AliasChoices`,
// v1 `Field(env=)`, a dict `model_config`, `case_sensitive` and inherited settings are not read.
fn settings_consts(root: Node, src: &[u8]) -> Vec<Const> {
    let keyword = |call: Node, key: &str| -> Option<String> {
        let list = call.child_by_field_name("arguments")?;
        named(list)
            .filter(|a| a.kind() == "keyword_argument")
            .find(|a| (a.child_by_field_name("name")).is_some_and(|n| text(n, src) == key))
            .and_then(|a| string(a.child_by_field_name("value")?, src))
    };
    let mut consts = Vec::new();
    for class in named(root).filter(|n| n.kind() == "class_definition") {
        let bases = class.child_by_field_name("superclasses");
        let is_settings = bases.is_some_and(|b| {
            named(b).any(|b| {
                matches!(
                    callee(b, src).as_str(),
                    "BaseSettings" | "pydantic_settings.BaseSettings" | "pydantic.BaseSettings"
                )
            })
        });
        let (Some(name), Some(body), true) = (
            class.child_by_field_name("name"),
            class.child_by_field_name("body"),
            is_settings,
        ) else {
            continue;
        };
        let mut prefix = String::new();
        let mut fields = Vec::new();
        for stmt in named(body).filter(|s| s.kind() == "expression_statement") {
            let Some((left, right, assign)) = assignment(stmt) else {
                continue;
            };
            let key = text(left, src);
            let call = right.filter(|r| r.kind() == "call");
            if key == "model_config" {
                prefix = call
                    .and_then(|c| keyword(c, "env_prefix"))
                    .unwrap_or(prefix);
            } else if left.kind() == "identifier"
                && !key.starts_with('_')
                && (assign.child_by_field_name("type"))
                    .is_some_and(|t| !text(t, src).contains("ClassVar"))
            {
                let alias = call
                    .and_then(|c| keyword(c, "validation_alias").or_else(|| keyword(c, "alias")))
                    .map(|a| a.to_uppercase());
                fields.push((key.to_string(), alias));
            }
        }
        let is_config = |c: &Node| {
            c.kind() == "class_definition"
                && (c.child_by_field_name("name")).is_some_and(|n| text(n, src) == "Config")
        };
        for config in named(body).filter(is_config) {
            let inner = config.child_by_field_name("body");
            for stmt in inner.into_iter().flat_map(named) {
                if let Some((left, right, _)) = assignment(stmt)
                    && text(left, src) == "env_prefix"
                    && let Some(p) = right.and_then(|r| string(r, src))
                {
                    prefix = p;
                }
            }
        }
        let class = text(name, src);
        for (field, alias) in fields {
            let var = alias.unwrap_or_else(|| format!("{prefix}{field}").to_uppercase());
            consts.push(Const {
                name: format!("{class}.{field}"),
                value: Value::Str(vec![Part::Env(var)]),
                ty: None,
                scope: None,
            });
        }
    }
    consts
}

fn assignment(stmt: Node) -> Option<(Node, Option<Node>, Node)> {
    let a = named(stmt).next().filter(|a| a.kind() == "assignment")?;
    Some((
        a.child_by_field_name("left")?,
        a.child_by_field_name("right"),
        a,
    ))
}

fn string_list(node: Node, src: &[u8]) -> Option<Vec<String>> {
    if !matches!(node.kind(), "list" | "tuple") {
        return None;
    }
    named(node).map(|item| string(item, src)).collect()
}

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
            "dictionary_splat" => kwargs.push(("...".into(), Value::Unknown)),
            "list_splat" => {
                args.push(Value::Unknown);
                kwargs.push(("*".into(), Value::Unknown));
            }
            "comment" => {}
            _ => args.push(value(arg, src)),
        }
    }
    (args, kwargs)
}

fn value(node: Node, src: &[u8]) -> Value {
    value_at(node, src, 0)
}

fn value_at(node: Node, src: &[u8], depth: usize) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    let deeper = |n: Node| value_at(n, src, depth + 1);
    match node.kind() {
        "identifier" => Value::Ref(text(node, src).to_string()),
        "none" => Value::Undefined,
        "attribute" => match callee(node, src) {
            name if name.starts_with('(') => Value::Unknown,
            name => Value::Ref(name),
        },
        "subscript" if parts(node, src).is_none() => {
            match (
                node.child_by_field_name("value"),
                node.child_by_field_name("subscript")
                    .and_then(|n| parts(n, src))
                    .and_then(|p| literal(&p)),
            ) {
                (Some(object), Some(field)) => Value::Member(callee(object, src), field),
                _ => Value::Unknown,
            }
        }
        "call" => parts(node, src).map_or_else(
            || {
                Value::Call(Invocation {
                    callee: node
                        .child_by_field_name("function")
                        .map_or_else(String::new, |f| callee(f, src)),
                    receiver: None,
                    args: Vec::new(),
                    kwargs: Vec::new(),
                    range: node.byte_range(),
                })
            },
            Value::Str,
        ),
        "list" | "tuple" => Value::List(named(node).map(deeper).collect()),
        "dictionary" => {
            let entries = named(node).filter_map(|entry| pair(entry, src, depth + 1));
            Value::Object(entries.collect())
        }
        "await" => named(node)
            .next()
            .map_or(Value::Unknown, |n| Value::Await(Box::new(deeper(n)))),
        "parenthesized_expression" => named(node).next().map_or(Value::Unknown, deeper),
        _ => parts(node, src).map_or(Value::Unknown, Value::Str),
    }
}

fn pair(node: Node, src: &[u8], depth: usize) -> Option<(String, Value)> {
    match node.kind() {
        "pair" => {
            let key = node.child_by_field_name("key")?;
            let item = value_at(node.child_by_field_name("value")?, src, depth);
            Some(match parts(key, src).and_then(|p| literal(&p)) {
                Some(key) => (key, item),
                None => (
                    "[]".into(),
                    Value::List(vec![value_at(key, src, depth), item]),
                ),
            })
        }
        "dictionary_splat" => Some(("...".into(), value_at(named(node).next()?, src, depth))),
        _ => None,
    }
}

fn parts(node: Node, src: &[u8]) -> Option<Vec<Part>> {
    parts_at(node, src, 0)
}

fn unread(mut node: Node, src: &[u8]) -> Part {
    let mut awaited = false;
    while matches!(node.kind(), "await" | "parenthesized_expression") {
        awaited |= node.kind() == "await";
        let Some(inner) = named(node).next() else {
            return Part::Hole;
        };
        node = inner;
    }
    match node
        .child_by_field_name("function")
        .filter(|_| node.kind() == "call")
    {
        Some(function) => Part::invoke(callee(function, src), node.byte_range(), awaited),
        None => Part::Hole,
    }
}

fn parts_at(node: Node, src: &[u8], depth: usize) -> Option<Vec<Part>> {
    if depth >= FOLD_DEPTH {
        return Some(vec![Part::Hole]);
    }
    let parts = |n: Node| parts_at(n, src, depth + 1);
    let inside = |n: Node| parts(n).unwrap_or_else(|| vec![unread(n, src)]);
    match node.kind() {
        "string" => Some(
            children(node)
                .filter_map(|part| match part.kind() {
                    "string_content" => Some(Part::Text(text(part, src).to_string())),
                    "interpolation" => Some(match part.child_by_field_name("expression") {
                        Some(e) => (name(e, src).or_else(|| parts(e).and_then(single_env)))
                            .unwrap_or_else(|| unread(e, src)),
                        None => Part::Hole,
                    }),
                    _ => None,
                })
                .collect(),
        ),
        "concatenated_string" => Some(named(node).flat_map(inside).collect()),
        "conditional_expression" => {
            let mut terms = named(node).filter(|n| n.kind() != "comment");
            let yes = Value::Str(parts(terms.next()?)?);
            let no = Value::Str(parts(terms.nth(1)?)?);
            match super::query_alternative(yes, no) {
                Value::Str(parts) => Some(parts),
                _ => None,
            }
        }
        "binary_operator" => {
            let left = node.child_by_field_name("left")?;
            let right = node.child_by_field_name("right")?;
            if text(node.child_by_field_name("operator")?, src) != "+" {
                return None;
            }
            let (l, r) = (parts(left), parts(right));
            if l.is_none() && r.is_none() {
                return None;
            }
            let (l, r) = (
                l.unwrap_or_else(|| vec![unread(left, src)]),
                r.unwrap_or_else(|| vec![unread(right, src)]),
            );
            Some(l.into_iter().chain(r).collect())
        }
        "boolean_operator" => {
            let left = node.child_by_field_name("left")?;
            let right = node.child_by_field_name("right")?;
            if text(node.child_by_field_name("operator")?, src) != "or" {
                return None;
            }
            let env = single_env(parts(left)?)?;
            let more = inside(right).iter().any(|p| matches!(p, Part::Env(_)));
            Some(vec![if more { Part::Hole } else { env }])
        }
        "identifier" | "attribute" => name(node, src).map(|name| vec![name]),
        "parenthesized_expression" => parts(named(node).next()?),
        "subscript" | "call" => env_read(node, src, depth).map(|env| vec![env]),
        _ => None,
    }
}

fn name(node: Node, src: &[u8]) -> Option<Part> {
    match node.kind() {
        "identifier" | "attribute" => Some(callee(node, src))
            .filter(|name| !name.starts_with('('))
            .map(Part::Ref),
        _ => None,
    }
}

// ponytail: the names are matched as written, not through the file's imports, so `import os
// as o` is not read and a local `getenv` function is.
fn env_key(node: Node, src: &[u8], reader: String) -> Option<Part> {
    if node.kind() == "identifier" {
        return Some(Part::EnvKey {
            name: text(node, src).into(),
            reader: Some(reader),
        });
    }
    string(node, src).map(Part::Env)
}

fn env_read(node: Node, src: &[u8], depth: usize) -> Option<Part> {
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
            let key = node.child_by_field_name("subscript")?;
            let environ = callee(node.child_by_field_name("value")?, src);
            matches!(environ.as_str(), "os.environ" | "environ")
                .then(|| env_key(key, src, environ))?
        }
        "call" => {
            let function = node.child_by_field_name("function")?;
            let reader = callee(function, src);
            if matches!(
                reader.as_str(),
                "os.getenv" | "getenv" | "os.environ.get" | "environ.get"
            ) {
                let list = node.child_by_field_name("arguments")?;
                let key = env_key(named(list).next()?, src, reader)?;
                let mut positional = named(list).filter(|a| {
                    !matches!(
                        a.kind(),
                        "keyword_argument" | "comment" | "dictionary_splat"
                    )
                });
                let keyword = named(list).find(|a| {
                    a.kind() == "keyword_argument"
                        && (a.child_by_field_name("name"))
                            .is_some_and(|n| text(n, src) == "default")
                });
                let default = (positional.nth(1)).or_else(|| keyword?.child_by_field_name("value"));
                let reads = default.and_then(|d| parts_at(d, src, depth + 1));
                return Some(match reads {
                    Some(p) if p.iter().any(|p| matches!(p, Part::Env(_))) => Part::Hole,
                    _ => key,
                });
            }
            let method = function.child_by_field_name("attribute")?;
            if !STRING_METHODS.contains(&text(method, src)) {
                return None;
            }
            single_env(parts_at(
                function.child_by_field_name("object")?,
                src,
                depth + 1,
            )?)
        }
        _ => None,
    }
}

fn single_env(parts: Vec<Part>) -> Option<Part> {
    match <[Part; 1]>::try_from(parts) {
        Ok([env @ Part::Env(_)]) => Some(env),
        _ => None,
    }
}

fn callee(node: Node, src: &[u8]) -> String {
    let (mut node, mut names) = (node, Vec::new());
    let base = loop {
        match node.kind() {
            "identifier" => break text(node, src).to_string(),
            "attribute" if names.len() < FOLD_DEPTH => match (
                node.child_by_field_name("object"),
                node.child_by_field_name("attribute"),
            ) {
                (Some(object), Some(attribute)) => {
                    names.push(text(attribute, src));
                    node = object;
                }
                _ => break format!("({})", node.kind()),
            },
            kind => break format!("({kind})"),
        }
    };
    (names.iter().rev()).fold(base, |path, name| path + "." + name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consts(src: &str) -> Vec<Const> {
        Python.parse("a.py", src.as_bytes()).unwrap().consts
    }

    fn held(consts: &[Const], name: &str) -> Value {
        (consts.iter().find(|c| c.name == name).unwrap().value).clone()
    }

    fn nesting(value: &Value) -> usize {
        match value {
            Value::Await(value) => 1 + nesting(value),
            Value::List(items) => 1 + items.first().map_or(0, nesting),
            Value::Object(entries) => 1 + entries.first().map_or(0, |(_, v)| nesting(v)),
            _ => 0,
        }
    }

    fn facts(src: &str) -> Facts {
        Python.parse("a.py", src.as_bytes()).unwrap()
    }

    #[test]
    fn only_an_inner_class_config_gives_a_settings_prefix() {
        let src = "from pydantic_settings import BaseSettings\n\
            class Legacy(BaseSettings):\n    url: str = ''\n    class Config:\n        env_prefix = 'old_'\n\
            class Other(BaseSettings):\n    url: str = ''\n    class Meta:\n        env_prefix = 'no_'\n";
        let consts = consts(src);
        let env = |name: &str| match held(&consts, name) {
            Value::Str(parts) => match parts.as_slice() {
                [Part::Env(v)] => v.clone(),
                _ => panic!("{name} is no env read"),
            },
            _ => panic!("{name} is no string"),
        };
        assert_eq!(env("Legacy.url"), "OLD_URL");
        assert_eq!(env("Other.url"), "URL");
    }

    #[test]
    fn a_main_guard_is_a_program_whose_symbol_holds_its_calls() {
        let src = "def run():\n    pass\n\nif __name__ == \"__main__\":\n    run()\n    x = 1\nelse:\n    other()\n";
        let f = facts(src);

        assert_eq!(f.programs, ["__main__"]);
        let main = f.defs.iter().find(|d| d.name == "__main__").unwrap();
        assert_eq!(
            &src[main.range.clone()],
            "if __name__ == \"__main__\":\n    run()\n    x = 1"
        );
        let scopes: Vec<(&str, &str)> = (f.calls.iter())
            .map(|c| (c.callee.as_str(), c.scope.as_str()))
            .collect();
        assert_eq!(scopes, [("run", "__main__"), ("other", "")]);
        assert!(
            !f.exports
                .iter()
                .any(|e| matches!(e, Export::Local { exported, .. } if exported == "__main__"))
        );
    }

    #[test]
    fn a_main_guard_is_read_in_its_other_spellings_and_nothing_else() {
        for guard in [
            "if '__main__' == __name__:",
            "if __name__ == '__main__':",
            "if \"__main__\" == __name__:",
        ] {
            let f = facts(&format!("{guard}\n    run()\n"));
            assert_eq!(f.programs, ["__main__"], "{guard}");
        }
        for guard in [
            "if __name__ != \"__main__\":",
            "if __name__ == \"other\":",
            "if __file__ == \"__main__\":",
            "if __name__ == \"__main__\" and ok:",
            "if (__name__ == \"__main__\"):",
            "if __name__ in [\"__main__\"]:",
        ] {
            let f = facts(&format!("{guard}\n    run()\n"));
            assert!(f.programs.is_empty(), "{guard}");
            assert!(f.defs.is_empty(), "{guard}");
        }
        let f = facts("def f():\n    if __name__ == \"__main__\":\n        run()\n");
        assert!(f.programs.is_empty());
    }

    #[test]
    fn only_the_first_main_symbol_of_a_file_counts() {
        let two =
            "if __name__ == \"__main__\":\n    one()\n\nif __name__ == \"__main__\":\n    two()\n";
        let f = facts(two);

        assert_eq!(f.programs, ["__main__"]);
        assert_eq!(f.defs.len(), 1);
        assert_eq!(
            &two[f.defs[0].range.clone()],
            "if __name__ == \"__main__\":\n    one()"
        );
        let scopes: Vec<(&str, &str)> = (f.calls.iter())
            .map(|c| (c.callee.as_str(), c.scope.as_str()))
            .collect();
        assert_eq!(scopes, [("one", "__main__"), ("two", "")]);

        let f = facts("def __main__():\n    one()\n\nif __name__ == \"__main__\":\n    two()\n");
        assert!(f.programs.is_empty());
        assert_eq!(f.defs.len(), 1);
        let scopes: Vec<(&str, &str)> = (f.calls.iter())
            .map(|c| (c.callee.as_str(), c.scope.as_str()))
            .collect();
        assert_eq!(scopes, [("one", "__main__"), ("two", "")]);
    }

    #[test]
    fn a_function_defined_in_a_main_guard_is_nested_in_its_symbol() {
        let f = facts("if __name__ == \"__main__\":\n    def go():\n        run()\n    go()\n");

        let names: Vec<&str> = f.defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["__main__", "__main__.go"]);
    }

    #[test]
    fn an_await_is_the_value_it_awaits() {
        let consts = consts("async def f():\n    nc = await connect()\n");

        let Value::Await(value) = held(&consts, "f.nc") else {
            unreachable!("await syntax is retained");
        };
        let Value::Call(call) = value.as_ref() else {
            unreachable!("not a call");
        };

        assert_eq!(call.callee, "connect");
    }

    #[test]
    fn a_dict_literal_is_an_object_with_a_key_for_each_kind_of_entry() {
        let src = "streams = {\n    # A comment is no entry.\n    \"orders\": \">\",\n    \
                   ORDERS: \">\",\n    f\"{X}.eu\": 1,\n    **rest,\n}\n";

        let Value::Object(entries) = held(&consts(src), "streams") else {
            unreachable!("not an object");
        };

        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["orders", "[]", "[]", "..."]);
        let Value::List(pair) = &entries[1].1 else {
            unreachable!("not a pair");
        };
        assert!(matches!(pair.as_slice(), [Value::Ref(k), Value::Str(_)] if k == "ORDERS"));
        let Value::List(pair) = &entries[2].1 else {
            unreachable!("not a pair");
        };
        let [Value::Str(key), Value::Unknown] = pair.as_slice() else {
            unreachable!("not an f-string and a number");
        };
        assert!(key.iter().any(|p| matches!(p, Part::Ref(x) if x == "X")));
        assert!(matches!(&entries[3].1, Value::Ref(name) if name == "rest"));
    }

    #[test]
    fn what_is_no_dict_literal_stays_unknown() {
        let consts = consts("a = {1, 2}\nb = {k: v for k, v in c}\n");

        assert!(matches!(held(&consts, "a"), Value::Unknown));
        assert!(matches!(held(&consts, "b"), Value::Unknown));
    }

    #[test]
    fn values_nested_past_the_depth_limit_are_unknown() {
        let n = 20_000;
        let dict = format!("x = {}1{}\n", "{\"k\": ".repeat(n), "}".repeat(n));
        let list = format!("x = {}1{}\n", "[".repeat(n), "]".repeat(n));
        let tuple = format!("x = {}1{}\n", "(".repeat(n), ",)".repeat(n));
        let parenthesized = format!("x = {}[1]{}\n", "(".repeat(n), ")".repeat(n));
        let awaited = format!("async def f():\n    x = {}[1]\n", "await ".repeat(n));

        for (src, name) in [
            (dict, "x"),
            (list, "x"),
            (tuple, "x"),
            (parenthesized, "x"),
            (awaited, "f.x"),
        ] {
            let depth = nesting(&held(&consts(&src), name));
            assert!(depth <= VALUE_DEPTH, "{depth} levels in `{}`", &src[..20]);
        }
    }

    #[test]
    fn concatenation_folds_once_per_term() {
        let src = format!("x = \"/x\" + {}\"y\"\n", "b + ".repeat(64));

        let Value::Str(parts) = held(&consts(&src), "x") else {
            unreachable!("not a string");
        };

        assert_eq!(parts.len(), 66);
    }

    #[test]
    fn long_chains_are_cut() {
        let n = 30_000;
        let sum = consts(&format!("a = '' + {}b\n", "b + ".repeat(n)));
        let Value::Str(parts) = held(&sum, "a") else {
            unreachable!("not a string");
        };
        assert!(parts.len() <= FOLD_DEPTH + 2, "{}", parts.len());

        let fallbacks = format!("a = {}os.getenv(\"X\")\n", "os.getenv(\"X\") or ".repeat(n));
        let defaults = format!(
            "a = {}\"/\"{}\n",
            "os.getenv(\"X\", ".repeat(4 * FOLD_DEPTH),
            ")".repeat(4 * FOLD_DEPTH)
        );
        let methods = format!("a = os.environ[\"X\"]{}\n", ".strip()".repeat(n));
        for src in [fallbacks, defaults, methods] {
            let value = held(&consts(&src), "a");
            assert!(
                matches!(value, Value::Str(_) | Value::Call(_) | Value::Unknown),
                "{}",
                &src[..20]
            );
        }

        let member = facts(&format!("a{}.f()\n", ".b".repeat(n)));
        assert_eq!(member.calls.len(), 1);
        assert!(member.calls[0].callee.len() < 10 * FOLD_DEPTH);
    }

    fn bound<'f>(facts: &'f Facts, scope: &str, name: &str) -> Option<&'f Class> {
        (facts.receivers.iter())
            .find(|r| r.scope == scope && r.name == name)
            .map(|r| &r.class)
    }

    #[test]
    fn definitions_nest_to_a_limit() {
        let n = 2 * DEFINITION_DEPTH;
        let src: String = (0..n)
            .map(|i| {
                let pad = "    ".repeat(i);
                let head = match i % 2 {
                    0 => format!("class d{i}"),
                    _ => format!("def d{i}()"),
                };
                format!("{pad}{head}:\n{pad}    g()\n")
            })
            .collect();

        let f = facts(&src);

        assert_eq!(f.calls.len(), n);
        let deepest = (f.defs.iter()).map(|d| levels(&d.name)).max();
        assert_eq!(deepest, Some(DEFINITION_DEPTH));
    }

    #[test]
    fn a_definition_past_the_limit_hides_its_name_and_keeps_its_returns() {
        let mut src = String::from("def g():\n    pass\n");
        for i in 0..DEFINITION_DEPTH {
            src += &format!("{}def f{i}():\n", "    ".repeat(i));
        }
        let pad = "    ".repeat(DEFINITION_DEPTH);
        src += &format!("{pad}def g(x):\n{pad}    return x\n{pad}g()\n{pad}return \"/r\"\n");

        let f = facts(&src);

        let call = f.calls.iter().find(|c| c.callee == "g").unwrap();
        assert_eq!(levels(&call.scope), DEFINITION_DEPTH);
        assert!(f.defs.iter().all(|d| d.name != qualify(&call.scope, "g")));
        assert!(matches!(bound(&f, &call.scope, "g"), Some(Class::Unknown)));
        assert!(matches!(bound(&f, &call.scope, "x"), Some(Class::Unknown)));
        let outer = f.defs.iter().find(|d| d.name == call.scope).unwrap();
        assert!(matches!(outer.return_values[..], [Value::Str(_)]));
    }

    #[test]
    fn targets_and_captures_nested_past_the_limit_bind_nothing_below_it() {
        let n = 20_000;
        let deep = |open: &str, close: &str| format!("{}b{}", open.repeat(n), close.repeat(n));
        let unpack = facts(&format!("a, {} = v\n", deep("(", ",)")));
        let case = facts(&format!(
            "match v:\n    case [a, {}]:\n        pass\n",
            deep("[", "]")
        ));
        for f in [&unpack, &case] {
            assert!(bound(f, "", "a").is_some());
            assert!(bound(f, "", "b").is_none());
        }
        let unpack = facts(&format!("a, {}b{} = v\n", "(".repeat(8), ",)".repeat(8)));
        assert!(bound(&unpack, "", "b").is_some());
    }

    #[test]
    fn annotations_nested_past_the_type_depth_are_unknown() {
        let param = |annotation: String| {
            let f = facts(&format!("def f(x: {annotation}):\n    pass\n"));
            bound(&f, "f", "x").cloned()
        };
        let optional = |n| format!("{}C{}", "Optional[".repeat(n), "]".repeat(n));
        let union = |n| format!("C{}", " | None".repeat(n));

        assert!(matches!(param(optional(3)), Some(Class::Named(c)) if c == "C"));
        assert!(matches!(param(union(3)), Some(Class::Named(c)) if c == "C"));
        for annotation in [optional(20_000), union(20_000)] {
            assert!(matches!(param(annotation), Some(Class::Unknown)));
        }
    }

    #[test]
    fn a_value_just_inside_the_depth_limit_is_read() {
        let n = VALUE_DEPTH - 1;
        let src = format!("x = {}1{}\n", "[".repeat(n), "]".repeat(n));

        assert_eq!(nesting(&held(&consts(&src), "x")), n);
    }

    #[test]
    fn a_module_of_many_names_binds_each_definition_in_linear_time() {
        let (names, defs) = (400_000, 30_000);
        let mut module = Module::default();
        for i in 0..names {
            module.name(&format!("z{i}"));
        }
        let start = std::time::Instant::now();
        for i in 0..defs {
            module.def(&format!("f{i}"), true);
        }
        let took = start.elapsed();
        assert_eq!(module.defs.len(), defs);
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn a_function_of_many_names_binds_each_definition_in_linear_time() {
        let (names, defs) = (300_000, 40_000);
        let mut locals = Locals::default();
        for i in 0..names {
            locals.bind("g", &format!("z{i}"), Class::Unknown, Evidence::Assigned);
        }
        let start = std::time::Instant::now();
        for i in 0..defs {
            locals.def("g", &format!("h{i}"), true);
        }
        let took = start.elapsed();
        assert_eq!(locals.defs.len(), defs);
        assert!(took.as_secs() < 20, "{took:?}");
    }

    #[test]
    fn a_scope_of_many_values_extends_each_list_in_linear_time() {
        let (values, more) = (300_000, 40_000);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let mut parse = |src: &'static str| (parser.parse(src, None).unwrap(), src.as_bytes());
        let (list, filler, add) = (parse("c0 = []\n"), parse("z = []\n"), parse("c0 += [1]\n"));
        let mut locals = Locals::default();
        let mut run = |(tree, src): &(tree_sitter::Tree, &[u8]), times| {
            let statement = tree.root_node().named_child(0).unwrap().named_child(0);
            for _ in 0..times {
                locals.assign(statement.unwrap(), "", src);
            }
        };
        run(&list, 1);
        run(&filler, values);
        let start = std::time::Instant::now();
        run(&add, more);
        let took = start.elapsed();
        let (written, held) = (&locals.written, &locals.values);
        let list = held.iter().find(|(at, _)| written[*at].1 == "c0");
        assert!(matches!(list, Some((_, Value::List(items))) if items.len() == more));
        assert!(took.as_secs() < 20, "{took:?}");
    }
}
