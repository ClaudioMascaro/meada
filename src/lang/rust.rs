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

/// The head of a path read from the file's own top (`self::`, and `super::` that stays in the
/// file): an import of the file itself. A keyword, so no name of the code is spelled so.
const SELF: &str = "mod";

/// How many segments of a path or a member chain a callee keeps, and how many references an
/// `impl` block's type is read through; a longer one is its node kind, or names nothing.
// ponytail: real paths hold a handful of segments; the limit keeps a crafted one cheap.
const PATH_DEPTH: usize = 256;

const MAIN: &str = "main";

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

    fn programs(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Vec<(String, String)> {
        (Tree::new(configs, files).bins())
            .filter(|bin| files[bin].defs.iter().any(|d| d.name == MAIN))
            .map(|bin| (bin.to_string(), MAIN.to_string()))
            .collect()
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

/// What the file binds, for the paths: the qualified names of its non-method definitions
/// (`mod` declarations included), its locals (written once each), and the names its `use`
/// declarations bind (or that one of them is a glob).
#[derive(Default)]
struct Scan {
    defs: HashSet<String>,
    locals: HashSet<String>,
    uses: HashSet<String>,
    glob: bool,
}

impl Scan {
    /// Whether a scope from `scope` up to `module`, `module` included, defines `name`: what a
    /// path's head may name there (a module ends the names in scope).
    fn defines(&self, scope: &str, module: &str, name: &str) -> bool {
        let mut s = scope;
        loop {
            if self.defs.contains(&qualify(s, name)) {
                return true;
            }
            if s.len() <= module.len() {
                return false;
            }
            s = crate::parent(s);
        }
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
    // The last `#n` given to the blocks of each scope.
    let mut suffixes: HashMap<String, usize> = HashMap::new();
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
                    let qualified = qualify(&module, name);
                    if in_module && !deep && defined.insert(qualified.clone()) {
                        scan.defs.insert(qualified);
                        declare(&mut facts, &mut public, node, &module, name, src);
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
                let mut base = qualify(&scope, &last(&ty, ty_node));
                if let (Some(of), Some(n)) = (&of, trait_node) {
                    base = qualify(&base, &last(of, n));
                }
                // Two blocks giving one name a method (`impl From<A> for E`, `impl From<B>
                // for E`) are two scopes: the later one is `E.From#2`.
                let methods: Vec<&str> = (field(node, "body").into_iter())
                    .flat_map(named)
                    .filter(|n| n.kind() == "function_item")
                    .filter_map(|f| field(f, "name").map(|n| text(n, src)))
                    .collect();
                let mut block = base.clone();
                let next = suffixes.entry(base.clone()).or_insert(1);
                while methods
                    .iter()
                    .any(|m| defined.contains(&qualify(&block, m)))
                {
                    *next += 1;
                    block = format!("{base}#{next}");
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
    // The module is a definition of its own, the declaration's span (`search` finds it).
    facts.defs.push(def(&local, Kind::Module, node));
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
/// References past `PATH_DEPTH` (`& & & Store`) name nothing.
fn type_path(node: Node, src: &[u8]) -> Option<Vec<String>> {
    let mut at = node;
    for _ in 0..PATH_DEPTH {
        match at.kind() {
            "reference_type" | "pointer_type" => at = field(at, "type")?,
            _ => return segments(at, src),
        }
    }
    None
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
/// `Self.f`, `mod.inner.f`), with the imports it needs (`crate`, `super`, `mod` for the file
/// itself, an outside crate). `self::` and `super::` within the file are read from the file's
/// top (`mod.<module>.<rest>`), so no name between hides them; `None` where they are read in a
/// module inside a function, whose place in the file no path names.
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
            let ups = (segments[skip..].iter())
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
            synthetic.insert((SELF.into(), "self".into()));
            let target = &module[..module.len() - ups];
            let anchored = [SELF].iter().chain(target).map(|s| s.to_string());
            Some(
                anchored
                    .chain(rest.iter().cloned())
                    .collect::<Vec<_>>()
                    .join("."),
            )
        }
        _ => {
            let defined = scan.defines(&path.scope, &path.module, head);
            if !defined && !scan.uses.contains(head) && !scan.glob {
                // Neither the module nor a `use` binds it: a crate outside the repository.
                synthetic.insert((head.to_string(), head.to_string()));
            }
            Some(segments.join("."))
        }
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
            ("x.f", true),
            ("mod.inner.h", true),
            ("mod.k", true),
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
            ("mod", "self"),
            ("super.super", "super::super"),
        ];
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(l, s)| (l.to_string(), s.to_string()))
            .collect();
        assert_eq!(imports, pairs);
    }

    #[test]
    fn super_out_of_an_inline_module_is_read_from_the_file_top() {
        let src = "fn k() {}\nmod t { fn k() {} fn g() { super::k(); } }\n";
        assert_eq!(callees(src)[0].0, "mod.k");
    }

    #[test]
    fn main_starts_a_program_only_in_a_binary_root() {
        let main = facts("fn main() {}\nmod m;\n");
        let other = facts("pub fn main() {}\n");
        let files = BTreeMap::from([
            ("src/lib.rs", &main),
            ("src/main.rs", &main),
            ("src/m.rs", &other),
        ]);
        // `src/lib.rs` is a library root and `src/m.rs` a module: neither starts a program.
        let configs: [(&str, &[u8]); 1] = [("Cargo.toml", b"[package]\nname = \"p\"\n")];
        let found = Rust.programs(&configs, &files);
        assert_eq!(found, [("src/main.rs".to_string(), "main".to_string())]);
    }

    #[test]
    fn modules_in_files_are_definitions() {
        let src = "mod a;\nmod inner { #[path = \"p.rs\"] mod x; }\n";
        let defs: Vec<(String, bool)> = (facts(src).defs.into_iter())
            .map(|d| (d.name, d.kind == Kind::Module))
            .collect();
        let expected = [("a", true), ("inner", true), ("inner.x", true)];
        let expected: Vec<(String, bool)> =
            expected.iter().map(|(n, m)| (n.to_string(), *m)).collect();
        assert_eq!(defs, expected);
        assert_eq!(facts(src).imports[1].source, "path inner|p.rs");
    }

    #[test]
    fn blocks_giving_one_name_a_method_are_scopes_of_their_own() {
        let src = "struct E;\nimpl From<A> for E { fn from() {} }\n\
                   impl From<B> for E { fn from() {} }\nimpl E { fn a() {} }\nimpl E { fn b() {} }\n";
        let facts = facts(src);
        let names: Vec<&str> = facts.defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["E", "E.From.from", "E.From#2.from", "E.a", "E.b"]);
        let scopes: Vec<&str> = facts.impls.iter().map(|i| i.scope.as_str()).collect();
        assert_eq!(scopes, ["E.From", "E.From#2", "E", "E"]);
    }

    #[test]
    fn references_past_the_path_limit_name_no_type() {
        let refs = "& ".repeat(PATH_DEPTH + 1);
        let src = format!("struct S;\nimpl {refs}S {{ fn m() {{}} }}\n");
        assert!(facts(&src).impls.is_empty());
        let src = format!("struct S;\nimpl {} S {{ fn m() {{}} }}\n", "& ".repeat(3));
        assert_eq!(facts(&src).impls[0].ty, "S");
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
