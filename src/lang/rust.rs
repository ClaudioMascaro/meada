//! The Rust language pack (spec #30). It reads `.rs` files and the `Cargo.toml` manifests of a
//! workspace with no Rust toolchain: crates come from `Cargo.toml` and the module tree from
//! `mod` declarations (`crates.rs`). Calls inside macro invocations are token trees to
//! tree-sitter and are not indexed.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use tree_sitter::{Node, Parser};

use super::{
    Binds, Call, Def, Export, Facts, Impl, Import, Imported, Kind, LanguagePack, Modules, Param,
    Star, children, named, text,
};
use crate::{Result, qualify};

mod crates;

use crates::{MOD, PATH, Tree};

/// How many definitions may nest in one file. Names are qualified by the definitions around
/// them and each call resolves its names up that chain, so unbounded nesting costs cubic time.
/// Past the limit an item is not a definition (its body belongs to the one around it).
// ponytail: real code nests a few levels; 128 is the depth the other packs allow.
const DEFINITION_DEPTH: usize = 128;

/// How many levels deep a pattern or a `use` tree is read for the names it binds; a name
/// deeper binds nothing.
// ponytail: real code destructures a few levels; the recursion must stay shallow.
const BINDING_DEPTH: usize = 64;

/// How many segments of a path or a member chain a callee keeps; a longer one is its node kind.
// ponytail: real paths hold a handful of segments; the limit keeps a crafted one cheap.
const PATH_DEPTH: usize = 256;

pub struct Rust;

impl LanguagePack for Rust {
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn parse(&self, _path: &str, src: &[u8]) -> Result<Facts> {
        parse(src)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["**/Cargo.toml"]
    }

    fn receiver(&self) -> Option<&'static str> {
        Some("Self")
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        let tree = Tree::new(configs, files);
        Ok(Box::new(move |from, specifier| {
            tree.lookup(from, specifier)
        }))
    }

    fn members(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Option<BTreeSet<String>> {
        Some(Tree::new(configs, files).members())
    }
}

/// What the items around a node make of a function in it.
#[derive(Clone)]
enum Body {
    /// A module or a function body: a function is a free function.
    Items,
    /// An `impl` block: a function is a method under this scope (`Store`, `Store.Persist`).
    Impl(String),
    /// A trait: a function with a body is a default method under the trait.
    Trait(String),
}

struct Visit<'t> {
    node: Node<'t>,
    /// The qualified name of the enclosing definition.
    scope: String,
    /// The module written in place that encloses the node (`a.b`), empty at the top level.
    module: String,
    /// Whether every scope up to the top is a module: `self::` and `super::` are read only
    /// there.
    pure: bool,
    body: Body,
}

/// A path read once the whole file is: a call's callee or an `impl` block's type.
struct Path {
    scope: String,
    module: String,
    pure: bool,
    segments: Vec<String>,
}

/// Where a path's normalized callee goes.
enum Slot {
    Call(usize),
    Type(usize),
    Trait(usize),
}

/// What the file binds, for the paths: the qualified names of its non-method definitions, its
/// locals and its `mod` declarations, each function's parameters, and the names its `use`
/// declarations bind (or that one of them is a glob).
#[derive(Default)]
struct Scan {
    defs: HashSet<String>,
    locals: HashSet<String>,
    mods: HashSet<String>,
    params: HashMap<String, Vec<String>>,
    uses: HashSet<String>,
    glob: bool,
}

impl Scan {
    /// Whether `scope` binds `name` itself.
    fn binds(&self, scope: &str, name: &str) -> bool {
        let q = qualify(scope, name);
        self.defs.contains(&q)
            || self.locals.contains(&q)
            || self.mods.contains(&q)
            || self
                .params
                .get(scope)
                .is_some_and(|p| p.iter().any(|p| p == name))
    }

    /// Whether a scope from `scope` up to `target`, `target` left out, binds `name`.
    fn hides(&self, scope: &str, target: &str, name: &str) -> bool {
        let mut s = scope;
        while s.len() > target.len() {
            if self.binds(s, name) {
                return true;
            }
            s = crate::parent(s);
        }
        false
    }
}

fn parse(src: &[u8]) -> Result<Facts> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let mut facts = Facts::default();
    let mut scan = Scan::default();
    let mut paths: Vec<(Slot, Path)> = Vec::new();
    let mut defined = HashSet::new();
    let mut public = Vec::new();
    // Explicit stack instead of recursion: generated code can nest deeper than the call stack.
    let mut stack = vec![Visit {
        node: tree.root_node(),
        scope: String::new(),
        module: String::new(),
        pure: true,
        body: Body::Items,
    }];
    while let Some(Visit {
        node,
        scope,
        module,
        pure,
        body,
    }) = stack.pop()
    {
        let mut inner = (scope.clone(), module.clone(), pure, body.clone());
        let top = scope.is_empty() && matches!(body, Body::Items);
        let deep = levels(&scope) >= DEFINITION_DEPTH;
        let mut locals = Vec::new();
        match node.kind() {
            // Macro bodies are token trees; attributes are read by the item they sit on.
            "macro_invocation" | "macro_definition" | "attribute_item" | "inner_attribute_item" => {
                continue;
            }
            "use_declaration" => {
                uses(node, src, 0, &mut scan);
                continue;
            }
            "function_item" if deep => {
                locals.extend(field(node, "name").map(|n| text(n, src).to_string()));
                for param in parameters(node, src) {
                    locals.extend(param.names);
                }
            }
            "function_item" => {
                let Some(name) = field(node, "name").map(|n| text(n, src)) else {
                    continue;
                };
                let (qualified, kind) = match &body {
                    Body::Items => (qualify(&scope, name), Kind::Function),
                    Body::Impl(of) | Body::Trait(of) => (qualify(of, name), Kind::Method),
                };
                let (params, bound): (Vec<Param>, Vec<Vec<String>>) = (parameters(node, src)
                    .into_iter())
                .map(|p| (p.param, p.names))
                .unzip();
                for local in bound.iter().flatten().map(|n| qualify(&qualified, n)) {
                    if scan.locals.insert(local.clone()) {
                        facts.locals.push(local);
                    }
                }
                if defined.insert(qualified.clone()) {
                    if kind == Kind::Function {
                        scan.defs.insert(qualified.clone());
                    }
                    let names = params.iter().map(|p| p.name.clone()).collect();
                    scan.params.insert(qualified.clone(), names);
                    facts.defs.push(Def {
                        params,
                        ..def(&qualified, kind, node)
                    });
                    if top {
                        export(&mut facts, &mut public, node, name, src);
                    }
                }
                inner = (qualified, module.clone(), pure, Body::Items);
            }
            "struct_item" | "enum_item" | "union_item" | "trait_item" if !deep => {
                let Some(name) = field(node, "name").map(|n| text(n, src)) else {
                    continue;
                };
                let qualified = qualify(&scope, name);
                if defined.insert(qualified.clone()) {
                    scan.defs.insert(qualified.clone());
                    facts.defs.push(def(&qualified, Kind::Class, node));
                    if top {
                        export(&mut facts, &mut public, node, name, src);
                    }
                }
                if node.kind() == "trait_item" {
                    inner.3 = Body::Trait(qualified);
                }
            }
            "mod_item" => {
                let Some(name) = field(node, "name").map(|n| text(n, src)) else {
                    continue;
                };
                // Directly in a module: not in a function, an `impl` or a trait.
                let in_module = pure && scope == module && matches!(body, Body::Items);
                if field(node, "body").is_none() {
                    if in_module {
                        declare(&mut facts, &mut scan, &mut public, node, &module, name, src);
                    }
                    continue;
                }
                if deep {
                    // ponytail: past the depth limit, a module's items are named in the one
                    // around it.
                } else {
                    let qualified = qualify(&scope, name);
                    if defined.insert(qualified.clone()) {
                        scan.defs.insert(qualified.clone());
                        facts.defs.push(def(&qualified, Kind::Module, node));
                        if top {
                            export(&mut facts, &mut public, node, name, src);
                        }
                    }
                    inner = (qualified.clone(), qualified, in_module, Body::Items);
                }
            }
            "impl_item" if !deep => {
                let ty = field(node, "type").and_then(|t| type_path(t, src));
                let of = field(node, "trait").map(|t| type_path(t, src));
                let last = |p: &Option<Vec<String>>, n: Node| match p {
                    Some(segments) => segments.last().unwrap().clone(),
                    None => format!("({})", n.kind()),
                };
                let (Some(ty_node), trait_node) = (field(node, "type"), field(node, "trait"))
                else {
                    continue;
                };
                let mut block = qualify(&scope, &last(&ty, ty_node));
                if let (Some(of), Some(n)) = (&of, trait_node) {
                    block = qualify(&block, &last(of, n));
                }
                if let Some(ty) = ty {
                    let at = facts.impls.len();
                    let path = |segments| Path {
                        scope: scope.clone(),
                        module: module.clone(),
                        pure,
                        segments,
                    };
                    paths.push((Slot::Type(at), path(ty)));
                    if let Some(Some(of)) = of.clone() {
                        paths.push((Slot::Trait(at), path(of)));
                    }
                    facts.impls.push(Impl {
                        scope: block.clone(),
                        ty: String::new(),
                        // A trait that cannot be read is still a trait's block.
                        of: of.as_ref().map(|_| String::new()),
                        within: scope.clone(),
                    });
                }
                inner.3 = Body::Impl(block);
            }
            "call_expression" => {
                let Some(function) = field(node, "function") else {
                    continue;
                };
                let function = match function.kind() {
                    "generic_function" => field(function, "function").unwrap_or(function),
                    _ => function,
                };
                let (callee, path) = match function.kind() {
                    "identifier" => (text(function, src).to_string(), None),
                    "scoped_identifier" => match segments(function, src) {
                        Some(segments) => (String::new(), Some(segments)),
                        None => (format!("({})", function.kind()), Some(Vec::new())),
                    },
                    "field_expression" => (member(function, src), None),
                    kind => (format!("({kind})"), None),
                };
                let by_path = path.is_some();
                if let Some(segments) = path.filter(|s| !s.is_empty()) {
                    let path = Path {
                        scope: scope.clone(),
                        module: module.clone(),
                        pure,
                        segments,
                    };
                    paths.push((Slot::Call(facts.calls.len()), path));
                }
                facts.calls.push(Call {
                    scope: scope.clone(),
                    callee,
                    range: node.byte_range(),
                    args: Vec::new(),
                    kwargs: Vec::new(),
                    decorates: None,
                    by_path,
                    arg_types: Vec::new(),
                    uses: Vec::new(),
                });
            }
            "let_declaration" | "for_expression" | "let_condition" | "match_arm" => {
                if let Some(pattern) = field(node, "pattern") {
                    bind(pattern, 0, &mut locals, src);
                }
            }
            // A closure is no definition: its calls belong to the enclosing one, where its
            // parameters bind.
            "closure_expression" => {
                if let Some(params) = field(node, "parameters") {
                    bind(params, 0, &mut locals, src);
                }
            }
            _ => {}
        }
        for name in locals {
            let local = qualify(&scope, &name);
            if scan.locals.insert(local.clone()) {
                facts.locals.push(local);
            }
        }
        let (scope, module, pure, body) = inner;
        let kids: Vec<Node> = children(node).collect();
        stack.extend(kids.into_iter().rev().map(|child| Visit {
            node: child,
            scope: scope.clone(),
            module: module.clone(),
            pure,
            body: body.clone(),
        }));
    }
    let mut synthetic = BTreeSet::new();
    for (slot, path) in &paths {
        let callee = normalize(path, &scan, &mut synthetic);
        match slot {
            Slot::Call(at) => {
                facts.calls[*at].callee = callee.unwrap_or_else(|| "(scoped_identifier)".into())
            }
            Slot::Type(at) => facts.impls[*at].ty = callee.unwrap_or_default(),
            Slot::Trait(at) => facts.impls[*at].of = Some(callee.unwrap_or_default()),
        }
    }
    // A type or trait no path names reaches no class.
    facts.impls.retain(|i| !i.ty.is_empty());
    for (local, source) in synthetic {
        facts.imports.push(Import {
            local,
            source,
            name: Imported::Namespace,
        });
    }
    facts.star = Star::Only(public);
    Ok(facts)
}

fn levels(scope: &str) -> usize {
    match scope {
        "" => 0,
        scope => scope.matches('.').count() + 1,
    }
}

fn field<'t>(node: Node<'t>, name: &str) -> Option<Node<'t>> {
    node.child_by_field_name(name)
}

fn def(name: &str, kind: Kind, node: Node) -> Def {
    Def {
        name: name.to_string(),
        kind,
        range: node.byte_range(),
        params: Vec::new(),
        bases: Vec::new(),
        decorators: Vec::new(),
        return_values: Vec::new(),
        returns: None,
        type_params: Vec::new(),
    }
}

/// Whether the item is `pub`: `pub(crate)` and narrower stay inside the crate.
fn is_pub(node: Node, src: &[u8]) -> bool {
    named(node).any(|n| n.kind() == "visibility_modifier" && text(n, src) == "pub")
}

/// A top-level item: the file exports it, and a glob import takes it when it is `pub`.
fn export(facts: &mut Facts, public: &mut Vec<String>, node: Node, name: &str, src: &[u8]) {
    facts.exports.push(Export::Local {
        exported: name.to_string(),
        local: name.to_string(),
    });
    if is_pub(node, src) {
        public.push(name.to_string());
    }
}

/// A `mod x;` declaration in `module` (empty at the top level): an import of the file it names
/// under `module.x`, which the file exports when it is at the top level.
fn declare(
    facts: &mut Facts,
    scan: &mut Scan,
    public: &mut Vec<String>,
    node: Node,
    module: &str,
    name: &str,
    src: &[u8],
) {
    let chain = module.replace('.', "/");
    let source = match path_attribute(node, src) {
        Some(value) => format!("{PATH}{chain}|{value}"),
        None => format!("{MOD}{}", qualify(&chain, name).replace('.', "/")),
    };
    let local = qualify(module, name);
    scan.mods.insert(local.clone());
    if module.is_empty() {
        facts.exports.push(Export::From {
            exported: name.to_string(),
            source: source.clone(),
            name: Imported::Namespace,
        });
        if is_pub(node, src) {
            public.push(name.to_string());
        }
    }
    facts.imports.push(Import {
        local,
        source,
        name: Imported::Namespace,
    });
}

/// The value of a `#[path = "..."]` attribute written on the item, if it is a plain string.
fn path_attribute(node: Node, src: &[u8]) -> Option<String> {
    let mut at = node.prev_named_sibling();
    while let Some(sibling) = at {
        match sibling.kind() {
            "attribute_item" => {
                let attribute = named(sibling).find(|n| n.kind() == "attribute")?;
                let is_path = named(attribute)
                    .next()
                    .is_some_and(|n| text(n, src) == "path");
                let value = field(attribute, "value").filter(|_| is_path);
                if let Some(value) = value.filter(|v| v.kind() == "string_literal") {
                    let parts: Vec<Node> = named(value).collect();
                    return match parts[..] {
                        [part] if part.kind() == "string_content" => {
                            Some(text(part, src).to_string())
                        }
                        _ => None,
                    };
                }
            }
            "line_comment" | "block_comment" => {}
            _ => return None,
        }
        at = sibling.prev_named_sibling();
    }
    None
}

/// A parameter, and the names its pattern binds when it is no plain name.
struct Parameter {
    param: Param,
    names: Vec<String>,
}

fn parameters(function: Node, src: &[u8]) -> Vec<Parameter> {
    let Some(list) = field(function, "parameters") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in named(list) {
        let param = |name: &str, binds| Param {
            name: name.to_string(),
            binds,
            default: None,
            fields: Vec::new(),
        };
        match node.kind() {
            "self_parameter" => out.push(Parameter {
                param: param("self", Binds::Instance),
                names: Vec::new(),
            }),
            "parameter" => match field(node, "pattern") {
                Some(p) if p.kind() == "identifier" => out.push(Parameter {
                    param: param(text(p, src), Binds::Any),
                    names: Vec::new(),
                }),
                pattern => {
                    let mut names = Vec::new();
                    if let Some(p) = pattern {
                        bind(p, 0, &mut names, src);
                    }
                    out.push(Parameter {
                        param: param("", Binds::Any),
                        names,
                    });
                }
            },
            _ => {}
        }
    }
    out
}

/// The names a pattern binds, `BINDING_DEPTH` levels deep: not the type a struct or tuple
/// struct pattern names, nor a match arm's guard.
fn bind(pattern: Node, depth: usize, out: &mut Vec<String>, src: &[u8]) {
    if depth >= BINDING_DEPTH {
        return;
    }
    match pattern.kind() {
        "identifier" | "shorthand_field_identifier" => out.push(text(pattern, src).to_string()),
        "parameter" => {
            if let Some(p) = field(pattern, "pattern") {
                bind(p, depth + 1, out, src);
            }
        }
        _ => {
            let skip = ["type", "condition"].map(|f| field(pattern, f).map(|n| n.id()));
            for child in named(pattern).filter(|c| !skip.contains(&Some(c.id()))) {
                if !matches!(child.kind(), "scoped_identifier" | "type_identifier") {
                    bind(child, depth + 1, out, src);
                }
            }
        }
    }
}

/// The names a `use` declaration binds, or that it is a glob.
fn uses(node: Node, src: &[u8], depth: usize, scan: &mut Scan) {
    if depth >= BINDING_DEPTH {
        return;
    }
    match node.kind() {
        "identifier" => {
            scan.uses.insert(text(node, src).to_string());
        }
        "scoped_identifier" => {
            if let Some(name) = field(node, "name") {
                scan.uses.insert(text(name, src).to_string());
            }
        }
        "use_as_clause" => {
            if let Some(alias) = field(node, "alias") {
                scan.uses.insert(text(alias, src).to_string());
            }
        }
        "use_wildcard" => scan.glob = true,
        // `use a::b::{self}` binds `b`: every path before a list counts.
        "scoped_use_list" => {
            for part in ["path", "list"].into_iter().filter_map(|f| field(node, f)) {
                uses(part, src, depth + 1, scan);
            }
        }
        "use_declaration" | "use_list" => {
            for child in named(node) {
                uses(child, src, depth + 1, scan);
            }
        }
        _ => {}
    }
}

/// The segments of a path (`crate::a::f`, `Vec::<u8>::new`), or `None` for a form that names
/// no item by its segments (`<T as Trait>::f`, `::f`, a path longer than `PATH_DEPTH`).
fn segments(node: Node, src: &[u8]) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut at = node;
    loop {
        if out.len() >= PATH_DEPTH {
            return None;
        }
        match at.kind() {
            "scoped_identifier" | "scoped_type_identifier" => {
                out.push(text(field(at, "name")?, src).to_string());
                at = field(at, "path")?;
            }
            "generic_type" => at = field(at, "type")?,
            "identifier" | "type_identifier" | "crate" | "self" | "super" => {
                out.push(text(at, src).to_string());
                break;
            }
            _ => return None,
        }
    }
    out.reverse();
    Some(out)
}

/// The path an `impl` block's type or trait is written as (`Store`, `super::Store`, `&Store`,
/// `Store<T>`); `None` for a type that names no item (a tuple, a slice, a function).
fn type_path(node: Node, src: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "reference_type" | "pointer_type" => type_path(field(node, "type")?, src),
        _ => segments(node, src),
    }
}

/// A method call's callee: the member chain of identifiers (`self.store.save`), any other
/// receiver its node kind (`(call_expression).amount`).
fn member(node: Node, src: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut at = node;
    loop {
        if parts.len() >= PATH_DEPTH {
            return format!("({})", node.kind());
        }
        match at.kind() {
            "field_expression" => {
                let (Some(name), Some(value)) = (field(at, "field"), field(at, "value")) else {
                    return format!("({})", node.kind());
                };
                parts.push(text(name, src).to_string());
                at = value;
            }
            "identifier" | "self" => {
                parts.push(text(at, src).to_string());
                break;
            }
            kind => {
                parts.push(format!("({kind})"));
                break;
            }
        }
    }
    parts.reverse();
    parts.join(".")
}

/// A path as a callee-like name the shared resolution reads (`crate.a.f`, `super.f`,
/// `Self.f`, `inner.x.f`), with the imports it needs (`crate`, `super`, an outside crate).
/// `self::` and `super::` within the file read the path from the module they name, as a plain
/// name; `None` when a scope between hides its first segment, or it is read where modules are
/// not all the way up.
fn normalize(
    path: &Path,
    scan: &Scan,
    synthetic: &mut BTreeSet<(String, String)>,
) -> Option<String> {
    let segments = &path.segments;
    let head = segments.first()?.as_str();
    match head {
        "crate" => {
            synthetic.insert(("crate".into(), "crate".into()));
            Some(segments.join("."))
        }
        "Self" => Some(segments.join(".")),
        "self" | "super" => {
            if !path.pure {
                return None;
            }
            let skip = usize::from(head == "self");
            let ups = segments[skip..]
                .iter()
                .take_while(|s| *s == "super")
                .count();
            let rest = &segments[skip + ups..];
            rest.first()?;
            let module: Vec<&str> = path.module.split('.').filter(|m| !m.is_empty()).collect();
            if ups > module.len() {
                // Out of the file: the module that declares it, and so on up.
                let up = ups - module.len();
                let local = vec!["super"; up].join(".");
                synthetic.insert((local.clone(), vec!["super"; up].join("::")));
                return Some(format!("{local}.{}", rest.join(".")));
            }
            let target = module[..module.len() - ups].join(".");
            if scan.hides(&path.scope, &target, &rest[0]) {
                return None;
            }
            Some(within(&target, rest, scan))
        }
        _ => {
            let module = &path.module;
            let declared = path.pure && scan.mods.contains(&qualify(module, head));
            if declared && !scan.hides(&path.scope, module, head) {
                return Some(within(module, segments, scan));
            }
            let visible = scan.hides(&path.scope, "", head) || scan.binds("", head);
            if !visible && !scan.uses.contains(head) && !scan.glob {
                // Neither the file nor a `use` binds it: a crate outside the repository.
                synthetic.insert((head.to_string(), head.to_string()));
            }
            Some(segments.join("."))
        }
    }
}

/// `rest` read from `module`: a module it declares with `mod` is imported under
/// `module.<name>`, anything else is found by name from the call's scope.
fn within(module: &str, rest: &[String], scan: &Scan) -> String {
    let joined = rest.join(".");
    match !module.is_empty() && scan.mods.contains(&qualify(module, &rest[0])) {
        true => format!("{module}.{joined}"),
        false => joined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(src: &str) -> Facts {
        parse(src.as_bytes()).unwrap()
    }

    fn callees(src: &str) -> Vec<(String, bool)> {
        let calls = facts(src).calls.into_iter();
        calls.map(|c| (c.callee, c.by_path)).collect()
    }

    #[test]
    fn paths_normalize_to_dotted_callees() {
        let src = "mod a;\nmod inner { mod x; fn g() { x::f(); self::h(); super::k(); } }\n\
                   fn f() { crate::a::f(); super::super::k(); a::f(); Vec::<u8>::new(); \
                   <T as Tr>::f(); g::<u8>(); s.m.n(); f().m(); }\n";
        let found = callees(src);
        let expected = [
            ("inner.x.f", true),
            ("h", true),
            ("k", true),
            ("crate.a.f", true),
            ("super.super.k", true),
            ("a.f", true),
            ("Vec.new", true),
            ("(scoped_identifier)", true),
            ("g", false),
            ("s.m.n", false),
            ("(call_expression).m", false),
            ("f", false),
        ];
        let expected: Vec<(String, bool)> =
            expected.iter().map(|(c, p)| (c.to_string(), *p)).collect();
        assert_eq!(found, expected);
        let imports: Vec<(String, String)> = (facts(src).imports.into_iter())
            .map(|i| (i.local, i.source))
            .collect();
        let pairs = [
            ("a", "mod a"),
            ("inner.x", "mod inner/x"),
            ("Vec", "Vec"),
            ("crate", "crate"),
            ("super.super", "super::super"),
        ];
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(l, s)| (l.to_string(), s.to_string()))
            .collect();
        assert_eq!(imports, pairs);
    }

    #[test]
    fn a_hidden_first_segment_leaves_the_path_unresolved() {
        let src = "fn k() {}\nmod t { fn k() {} fn g() { super::k(); } }\n";
        assert_eq!(callees(src)[0].0, "(scoped_identifier)");
    }

    #[test]
    fn items_nested_past_the_depth_limit_are_no_definitions() {
        let depth = DEFINITION_DEPTH + 72;
        let src = format!(
            "{}fn f() {{}}{}",
            "mod m { ".repeat(depth),
            " }".repeat(depth)
        );
        let defs = facts(&src).defs;
        assert_eq!(defs.len(), DEFINITION_DEPTH);
        assert!(defs.iter().all(|d| d.kind == Kind::Module));
    }

    #[test]
    fn deep_patterns_and_paths_stay_within_their_limits() {
        let pattern = format!("{}x{}", "(".repeat(200), ")".repeat(200));
        let src = format!("fn f() {{ let {pattern} = 1; }}\n");
        assert!(facts(&src).locals.is_empty());
        let path = vec!["a"; PATH_DEPTH + 1].join("::");
        let src = format!("fn f() {{ {path}(); }}\n");
        assert_eq!(callees(&src)[0].0, "(scoped_identifier)");
    }
}
