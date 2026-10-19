//! The Go language pack (spec #25). It reads `.go` files and the `go.mod` files of a repository
//! with no Go toolchain: an import names a package directory through the module path of the
//! `go.mod` that owns it (`modfile.rs`), the files of one package share their top-level scope
//! (`Layout::shared`), and build constraints are evaluated for one fixed target (`build.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};

use super::{
    Binds, Call, Const, Def, Export, Facts, Import, Imported, Kind, LanguagePack, Layout, Modules,
    Param, Value, children, named, text,
};
use crate::{Result, qualify};

mod build;
mod modfile;
mod values;

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
                    .map_or_else(Vec::new, |first| vec![first.clone()])
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
    /// How many times each local is bound.
    bindings: HashMap<String, usize>,
    /// The plain names a statement of a function's own block binds to one value (`r :=
    /// gin.Default()`), by scope: constants when nothing else binds or assigns the name.
    held: Vec<(String, String, Value)>,
    /// The names assigned again, with the scope the assignment is written in.
    assigned: Vec<(String, String)>,
    /// The single value each `return` of a definition gives.
    returns: HashMap<String, Vec<Value>>,
}

impl Walk<'_> {
    fn local(&mut self, scope: &str, name: &str) {
        if name == "_" {
            return;
        }
        *self.bindings.entry(qualify(scope, name)).or_default() += 1;
        if self.locals.insert(qualify(scope, name)) {
            self.facts.locals.push(qualify(scope, name));
        }
    }

    /// Makes the names held by one binding constants, and gives each definition what it
    /// returns. A name bound again anywhere in its function, or assigned in it or in a literal
    /// inside it, holds no one value.
    fn finish(&mut self) {
        let mut assigned = HashSet::new();
        for (scope, name) in std::mem::take(&mut self.assigned) {
            let mut at = scope.as_str();
            // The binding the name refers to: the innermost scope that binds it.
            let found = loop {
                if self.bindings.contains_key(&qualify(at, &name)) {
                    break Some(qualify(at, &name));
                }
                if at.is_empty() {
                    break None;
                }
                at = crate::parent(at);
            };
            assigned.extend(found);
        }
        let mut constants = HashSet::new();
        for (scope, name, value) in std::mem::take(&mut self.held) {
            let local = qualify(&scope, &name);
            if self.bindings.get(&local) == Some(&1) && !assigned.contains(&local) {
                constants.insert(local.clone());
                self.facts.consts.push(Const {
                    name: local,
                    value,
                    ty: None,
                });
            }
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

    /// The names a parameter list binds.
    fn bind_params(&mut self, scope: &str, list: Option<Node>) {
        for param in list.into_iter().flat_map(named) {
            for name in names(param) {
                self.local(scope, text(name, self.src));
            }
        }
    }

    /// The type parameters a generic function declares (`[T any]`).
    fn bind_type_params(&mut self, scope: &str, node: Node) {
        let list = node.child_by_field_name("type_parameters");
        for declaration in list.into_iter().flat_map(named) {
            for name in names(declaration) {
                self.local(scope, text(name, self.src));
            }
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
                            .map(|a| values::value(a, src, scope, depth))
                            .collect();
                        at.receiver = values::receiver(function, src, scope, depth);
                        self.facts.calls.push(at);
                    }
                }
                "return_statement" => {
                    let results: Vec<Node> = named(node).flat_map(named).collect();
                    if let [result] = results[..] {
                        let value = values::value(result, src, scope, depth);
                        (self.returns.entry(scope.to_string()).or_default()).push(value);
                    }
                }
                "short_var_declaration" | "range_clause" | "receive_statement"
                    if children(node).any(|c| c.kind() == ":=") =>
                {
                    if let Some(left) = node.child_by_field_name("left") {
                        self.bind(scope, left);
                        if node.kind() == "short_var_declaration" && in_body(node) {
                            let right = node.child_by_field_name("right");
                            self.hold(scope, &[left], right, depth);
                        }
                    }
                }
                // `r = gin.New()`, `for r = range rs`: the name is assigned again.
                "assignment_statement" | "range_clause" | "receive_statement" => {
                    if let Some(left) = node.child_by_field_name("left") {
                        let targets = named(left).chain([left]);
                        for name in targets.filter(|n| n.kind() == "identifier") {
                            let name = text(name, src).to_string();
                            self.assigned.push((scope.to_string(), name));
                        }
                    }
                }
                "var_spec" | "const_spec" | "type_spec" | "type_alias" => {
                    for name in names(node) {
                        self.local(scope, text(name, src));
                    }
                    if node.kind() == "var_spec" && in_body(node) {
                        let right = node.child_by_field_name("value");
                        self.hold(scope, &names(node), right, depth);
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
        if let ([name], [value]) = (&left[..], &right[..])
            && name.kind() == "identifier"
            && text(*name, src) != "_"
        {
            let value = values::value(*value, src, scope, depth);
            let name = text(*name, src).to_string();
            self.held.push((scope.to_string(), name, value));
        }
    }

    /// The `Kind::Inline` definition of the function `node` (a literal, or a blank `func _`)
    /// written in `scope`, with its parameters, and the names its results bind: its name.
    fn inline(&mut self, scope: &str, node: Node) -> String {
        let name = qualify(scope, &format!("@{}", node.start_byte()));
        let params = params(node.child_by_field_name("parameters"), self.src);
        (self.facts.defs).push(def(name.clone(), Kind::Inline, node.byte_range(), params));
        self.bind_params(&name, result_list(node));
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

/// The `name` fields of a declaration (`a, b int`, `x, y = 1, 2`).
fn names(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("name", &mut cursor).collect()
}

/// A function's named results, when it names them.
fn result_list(node: Node) -> Option<Node> {
    node.child_by_field_name("result")
        .filter(|r| r.kind() == "parameter_list")
}

fn call(scope: &str, callee: String, range: Range<usize>) -> Call {
    Call {
        scope: scope.to_string(),
        callee,
        range,
        args: Vec::new(),
        kwargs: Vec::new(),
        decorates: None,
        by_path: false,
        root: None,
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
        bindings: HashMap::new(),
        held: Vec::new(),
        assigned: Vec::new(),
        returns: HashMap::new(),
    };
    let mut top = Vec::new();
    let mut inits: HashMap<&str, usize> = HashMap::new();
    for node in named(tree.root_node()) {
        match node.kind() {
            "package_clause" => {
                walk.facts.package = named(node).next().map_or("", |n| text(n, src)).into();
            }
            "import_declaration" => imports(node, src, &mut walk.facts),
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
                let params = params(node.child_by_field_name("parameters"), src);
                let range = node.byte_range();
                walk.facts
                    .defs
                    .push(def(name.clone(), Kind::Function, range, params));
                walk.bind_params(&name, result_list(node));
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
                let params = params(node.child_by_field_name("parameters"), src);
                let method = def(qualified.clone(), Kind::Method, node.byte_range(), params);
                walk.facts.defs.push(method);
                walk.bind_params(&qualified, receiver);
                walk.bind_params(&qualified, result_list(node));
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
                    let ty = spec.child_by_field_name("type");
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
                        walk.facts.defs.push(def(
                            qualified,
                            Kind::Method,
                            elem.byte_range(),
                            params,
                        ));
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
                    for name in names(spec).into_iter().map(|n| text(n, src)) {
                        if name == "_" {
                            continue;
                        }
                        walk.facts.consts.push(Const {
                            name: name.to_string(),
                            value: Value::Unknown,
                            ty: None,
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

    #[test]
    fn a_name_one_statement_of_the_function_binds_once_is_a_constant() {
        let consts = |body: &str| -> Vec<String> {
            let src = format!("package x\n\nfunc f() {{ {body} }}\n");
            facts(&src).consts.into_iter().map(|c| c.name).collect()
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
