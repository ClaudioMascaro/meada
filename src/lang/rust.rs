//! The Rust language pack (spec #30). It reads `.rs` files and the `Cargo.toml` manifests of a
//! workspace with no Rust toolchain: crates come from `Cargo.toml` and the module tree from
//! `mod` declarations (`crates.rs`). Calls inside macro invocations are token trees to
//! tree-sitter and are not indexed.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use tree_sitter::{Node, Parser};

use super::{
    Binds, Call, Const, Def, Export, Facts, Impl, Import, Imported, Kind, LanguagePack, Modules,
    Param, Star, children, named, text,
};
use crate::{Result, qualify};

pub(crate) mod crates;
mod values;

use crates::{MOD, PATH, Tree};
use values::{Reader, Where};

/// How many definitions may nest in one file. Names are qualified by the definitions around
/// them and each call resolves its names up that chain, so unbounded nesting costs cubic time.
/// Past the limit an item is not a definition (its body belongs to the one around it).
// ponytail: real code nests a few levels; 128 is the depth the other packs allow.
const DEFINITION_DEPTH: usize = 128;

/// How many levels deep a pattern or a `use` tree is read for the names it binds; a name
/// deeper binds nothing.
// ponytail: real code destructures a few levels; the recursion must stay shallow.
const BINDING_DEPTH: usize = 64;

/// How many names the `use` declarations of one file bind; the rest bind nothing.
// ponytail: real files import a few hundred names; every lookup walks the imports, so a crafted
// file of a million cannot be left to make each call a scan.
const USE_BINDINGS: usize = 4096;

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

    // Types are strict: a builder method hands back the type it was called on, so
    // `Router::new().route(..)` is the router `Router::new()` made.
    fn fluent(&self) -> bool {
        true
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

    fn published(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Option<BTreeSet<String>> {
        Some(Tree::new(configs, files).published())
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
    /// Whether every module written in place around the node is `pub`: its `pub` items are
    /// then exported under their dotted names.
    open: bool,
    /// Whether the node is an argument of a call: a closure there is a definition.
    arg: bool,
}

/// What a value is read for, once the whole file is.
enum Put {
    /// The arguments and the receiver of the call at this index of `Facts::calls`.
    Call(usize),
    /// A `let` binding a plain name once in its scope, by this qualified name: a constant.
    Let(String),
    /// A `const` or a `static`, by this qualified name.
    Item(String),
    /// What the definition at this index of `Facts::defs` returns: the node is its body.
    Returns(usize),
}

struct Held<'t> {
    at: Where,
    node: Node<'t>,
    put: Put,
}

/// A name a `use` binds (or, with no name, the glob of its path), read once the whole file is.
struct Use {
    path: Path,
    name: Option<String>,
    public: bool,
}

/// A path read once the whole file is: a call's callee or an `impl` block's type, or a `use`
/// (`binds` is then the qualified name it binds, which its own head does not name: `use
/// tokio::{self}` begins with the crate).
struct Path {
    scope: String,
    module: String,
    pure: bool,
    segments: Vec<String>,
    binds: Option<String>,
}

/// Where a path's normalized callee goes.
enum Slot {
    Call(usize),
    Type(usize),
    Trait(usize),
}

/// What the file binds, for the paths: the qualified names of its non-method definitions
/// (`mod` declarations included), its locals (written once each), the names its `use`
/// declarations bind, qualified by the scope they are written in, and how many bind each.
#[derive(Default)]
struct Scan {
    defs: HashSet<String>,
    locals: HashSet<String>,
    uses: HashMap<String, usize>,
}

/// What the paths of a file need besides its own items: imports of the file itself and of
/// the modules around it, and the crates outside it that a path names with no `use`.
#[derive(Default)]
struct Heads {
    imports: BTreeSet<(String, String)>,
    externs: BTreeSet<String>,
}

impl Scan {
    /// Whether a scope from `scope` up to `module`, `module` included, defines or imports
    /// `name`: what a path's head may name there (a module ends the names in scope). The
    /// `use` that `path` is does not import its own head.
    fn defines(&self, path: &Path, name: &str) -> bool {
        let (module, mut s) = (path.module.as_str(), path.scope.as_str());
        loop {
            let qualified = qualify(s, name);
            let own = usize::from(path.binds.as_ref() == Some(&qualified));
            let used = self.uses.get(&qualified).copied().unwrap_or(0);
            if self.defs.contains(&qualified) || used > own {
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
    let mut imported: Vec<Use> = Vec::new();
    let mut bound = 0;
    let mut defined = HashSet::new();
    // The last `#n` given to the blocks of each scope.
    let mut suffixes: HashMap<String, usize> = HashMap::new();
    let mut public_names = Vec::new();
    // The values to read once the file is, and how many bindings each local name has.
    let mut held: Vec<Held> = Vec::new();
    let mut bindings: HashMap<String, usize> = HashMap::new();
    // Explicit stack instead of recursion: generated code can nest deeper than the call stack.
    let mut stack = vec![Visit {
        node: tree.root_node(),
        scope: String::new(),
        module: String::new(),
        pure: true,
        body: Body::Items,
        open: true,
        arg: false,
    }];
    while let Some(Visit {
        node,
        scope,
        module,
        pure,
        body,
        open,
        arg,
    }) = stack.pop()
    {
        let mut inner = (scope.clone(), module.clone(), pure, body.clone(), open);
        // Directly in a module (the file's or one written in place): not in a function, an
        // `impl` or a trait.
        let in_module = pure && scope == module && matches!(body, Body::Items);
        let deep = levels(&scope) >= DEFINITION_DEPTH;
        let mut locals = Vec::new();
        match node.kind() {
            // Macro bodies are token trees; attributes are read by the item they sit on.
            "macro_invocation" | "macro_definition" | "attribute_item" | "inner_attribute_item" => {
                continue;
            }
            "use_declaration" => {
                let public = is_pub(node, src);
                let mut found = Vec::new();
                if let Some(argument) = field(node, "argument") {
                    let room = USE_BINDINGS.saturating_sub(bound);
                    leaves(argument, &mut Vec::new(), 0, room, src, &mut found);
                }
                bound += found.len();
                for Leaf { segments, name } in found {
                    if let Some(name) = &name {
                        *scan.uses.entry(qualify(&scope, name)).or_default() += 1;
                        if in_module {
                            export(&mut facts, &mut public_names, public, name, &module, open);
                        }
                    }
                    let path = Path {
                        scope: scope.clone(),
                        module: module.clone(),
                        pure,
                        segments,
                        binds: name.as_ref().map(|n| qualify(&scope, n)),
                    };
                    imported.push(Use { path, name, public });
                }
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
                    *bindings.entry(local.clone()).or_default() += 1;
                    if scan.locals.insert(local.clone()) {
                        facts.locals.push(local);
                    }
                }
                for param in params.iter().filter(|p| !p.name.is_empty()) {
                    *bindings
                        .entry(qualify(&qualified, &param.name))
                        .or_default() += 1;
                }
                if defined.insert(qualified.clone()) {
                    if kind == Kind::Function {
                        scan.defs.insert(qualified.clone());
                    }
                    if let Some(body) = field(node, "body") {
                        let at = Where {
                            scope: qualified.clone(),
                            module: module.clone(),
                            pure,
                        };
                        let put = Put::Returns(facts.defs.len());
                        held.push(Held {
                            at,
                            node: body,
                            put,
                        });
                    }
                    facts.defs.push(Def {
                        params,
                        ..def(&qualified, kind, node)
                    });
                    if in_module {
                        let is_pub = is_pub(node, src);
                        export(&mut facts, &mut public_names, is_pub, name, &module, open);
                    }
                }
                inner = (qualified, module.clone(), pure, Body::Items, open);
            }
            "const_item" | "static_item" if !deep && matches!(body, Body::Items) => {
                let name = field(node, "name").map(|n| text(n, src));
                if let Some(name) = name
                    && defined.insert(qualify(&scope, name))
                {
                    let at = Where {
                        scope: scope.clone(),
                        module: module.clone(),
                        pure,
                    };
                    if let Some(value) = field(node, "value") {
                        let put = Put::Item(qualify(&scope, name));
                        held.push(Held {
                            at,
                            node: value,
                            put,
                        });
                    }
                    if in_module {
                        let is_pub = is_pub(node, src);
                        export(&mut facts, &mut public_names, is_pub, name, &module, open);
                    }
                }
            }
            "struct_item" | "enum_item" | "union_item" | "trait_item" if !deep => {
                let Some(name) = field(node, "name").map(|n| text(n, src)) else {
                    continue;
                };
                let qualified = qualify(&scope, name);
                if defined.insert(qualified.clone()) {
                    scan.defs.insert(qualified.clone());
                    facts.defs.push(def(&qualified, Kind::Class, node));
                    if in_module {
                        let is_pub = is_pub(node, src);
                        export(&mut facts, &mut public_names, is_pub, name, &module, open);
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
                if field(node, "body").is_none() {
                    let qualified = qualify(&module, name);
                    if in_module && !deep && defined.insert(qualified.clone()) {
                        scan.defs.insert(qualified);
                        declare(&mut facts, &mut public_names, node, &module, name, src);
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
                        if in_module {
                            let is_pub = is_pub(node, src);
                            export(&mut facts, &mut public_names, is_pub, name, &module, open);
                        }
                    }
                    let open = open && in_module && is_pub(node, src);
                    inner = (qualified.clone(), qualified, in_module, Body::Items, open);
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
                        binds: None,
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
                        binds: None,
                    };
                    paths.push((Slot::Call(facts.calls.len()), path));
                }
                let at = Where {
                    scope: scope.clone(),
                    module: module.clone(),
                    pure,
                };
                let put = Put::Call(facts.calls.len());
                held.push(Held { at, node, put });
                facts.calls.push(Call {
                    scope: scope.clone(),
                    callee,
                    range: node.byte_range(),
                    args: Vec::new(),
                    kwargs: Vec::new(),
                    decorates: None,
                    by_path,
                    receiver: None,
                    arg_types: Vec::new(),
                    uses: Vec::new(),
                });
            }
            "let_declaration" | "for_expression" | "let_condition" | "match_arm" => {
                if let Some(pattern) = field(node, "pattern") {
                    bind(pattern, 0, &mut locals, src);
                    // A `let` of a plain name is a constant if nothing else binds the name in
                    // its scope (`let mut` is a `mut_pattern`, and is not).
                    let value = field(node, "value");
                    if let (true, Some(value), "identifier") =
                        (node.kind() == "let_declaration", value, pattern.kind())
                    {
                        let at = Where {
                            scope: scope.clone(),
                            module: module.clone(),
                            pure,
                        };
                        let put = Put::Let(qualify(&scope, text(pattern, src)));
                        held.push(Held {
                            at,
                            node: value,
                            put,
                        });
                    }
                }
            }
            // A closure given to a call is a definition of its own: the shared side names it
            // when a contract kind says it is a route handler, and folds it into the one
            // around it otherwise. Its parameters bind in it.
            "closure_expression" if arg && !deep => {
                let name = values::inline(&scope, node);
                facts.defs.push(def(&name, Kind::Inline, node));
                let mut names = Vec::new();
                if let Some(params) = field(node, "parameters") {
                    bind(params, 0, &mut names, src);
                }
                for local in names.into_iter().map(|n| qualify(&name, &n)) {
                    *bindings.entry(local.clone()).or_default() += 1;
                    if scan.locals.insert(local.clone()) {
                        facts.locals.push(local);
                    }
                }
                inner = (name, module.clone(), pure, Body::Items, open);
            }
            // Any other closure is no definition: its calls belong to the enclosing one,
            // where its parameters bind.
            "closure_expression" => {
                if let Some(params) = field(node, "parameters") {
                    bind(params, 0, &mut locals, src);
                }
            }
            _ => {}
        }
        for name in locals {
            let local = qualify(&scope, &name);
            *bindings.entry(local.clone()).or_default() += 1;
            if scan.locals.insert(local.clone()) {
                facts.locals.push(local);
            }
        }
        let (scope, module, pure, body, open) = inner;
        let kids: Vec<Node> = children(node).collect();
        stack.extend(kids.into_iter().rev().map(|child| Visit {
            node: child,
            scope: scope.clone(),
            module: module.clone(),
            pure,
            body: body.clone(),
            open,
            arg: node.kind() == "arguments",
        }));
    }
    let mut heads = Heads::default();
    for Use { path, name, public } in &imported {
        let Some(target) = normalize(path, &scan, &mut heads, true) else {
            continue;
        };
        let (local, name) = match name {
            Some(name) => (qualify(&path.scope, name), Imported::Path(target)),
            None => {
                let inside = path.segments.iter().all(|s| s == "self" || s == "super");
                let inside = inside || path.segments == ["crate"];
                let glob = Imported::Glob {
                    path: target,
                    inside,
                    public: *public,
                };
                (qualify(&path.scope, "*"), glob)
            }
        };
        let source = String::new();
        facts.imports.push(Import {
            local,
            source,
            name,
        });
    }
    for (slot, path) in &paths {
        let callee = normalize(path, &scan, &mut heads, false);
        match slot {
            Slot::Call(at) => {
                facts.calls[*at].callee = callee.unwrap_or_else(|| "(scoped_identifier)".into())
            }
            Slot::Type(at) => facts.impls[*at].ty = callee.unwrap_or_default(),
            Slot::Trait(at) => facts.impls[*at].of = Some(callee.unwrap_or_default()),
        }
    }
    // The values, now that the file's `use` declarations are known.
    let mut reader = Reader {
        src,
        scan: &scan,
        heads: &mut heads,
    };
    let mut constants = HashSet::new();
    for Held { at, node, put } in held {
        match put {
            Put::Call(at_call) => {
                let (args, receiver) = reader.call(&at, node);
                let call = &mut facts.calls[at_call];
                call.args = args;
                call.receiver = receiver;
            }
            Put::Let(name) if bindings.get(&name) == Some(&1) => {
                let value = reader.value(&at, node);
                constants.insert(name.clone());
                facts.consts.push(Const {
                    name,
                    value,
                    ty: None,
                });
            }
            Put::Let(_) => {}
            Put::Item(name) => {
                let value = reader.value(&at, node);
                facts.consts.push(Const {
                    name,
                    value,
                    ty: None,
                });
            }
            Put::Returns(def) => facts.defs[def].return_values = reader.returns(&at, node),
        }
    }
    // A constant is a name that holds its value, not a local hiding what it is bound to.
    facts.locals.retain(|l| !constants.contains(l));
    // A type or trait no path names reaches no class.
    facts.impls.retain(|i| !i.ty.is_empty());
    facts.externs = heads.externs.into_iter().collect();
    for (local, source) in heads.imports {
        facts.imports.push(Import {
            local,
            source,
            name: Imported::Namespace,
        });
    }
    facts.star = Star::Only(public_names);
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

/// A top-level name: the file exports it to its own crate, and to others and to a glob import
/// when it is `pub`. In a module written in place, whose modules around are all `pub` (`open`),
/// a `pub` name is exported under its dotted name (`inner.helper`).
fn export(
    facts: &mut Facts,
    public: &mut Vec<String>,
    is_pub: bool,
    name: &str,
    module: &str,
    open: bool,
) {
    if module.is_empty() {
        facts.exports.push(Export::Local {
            exported: name.to_string(),
            local: name.to_string(),
        });
        match is_pub {
            true => public.push(name.to_string()),
            false => facts.private.push(name.to_string()),
        }
    } else if open && is_pub {
        let dotted = qualify(module, name);
        facts.exports.push(Export::Local {
            exported: dotted.clone(),
            local: dotted,
        });
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
        match is_pub(node, src) {
            true => public.push(name.to_string()),
            false => facts.private.push(name.to_string()),
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

/// A name a `use` tree binds, with the path it names; no name for a glob of the path.
struct Leaf {
    segments: Vec<String>,
    name: Option<String>,
}

/// The leaves of a `use` tree below the path `prefix`, `BINDING_DEPTH` levels deep and `room`
/// of them in all (`use a::{b, c as d, e::*}` has three). `use serde;` names nothing a path
/// could not reach already, and `as _` binds no name.
fn leaves(
    node: Node,
    prefix: &mut Vec<String>,
    depth: usize,
    room: usize,
    src: &[u8],
    out: &mut Vec<Leaf>,
) {
    if depth >= BINDING_DEPTH || out.len() >= room {
        return;
    }
    let joined = |prefix: &[String], tail: Vec<String>| {
        let mut all = prefix.to_vec();
        all.extend(tail);
        all
    };
    match node.kind() {
        "scoped_identifier" | "identifier" | "crate" | "super" => {
            let Some(tail) = segments(node, src) else {
                return;
            };
            let all = joined(prefix, tail);
            if all.len() > 1 {
                let name = all.last().cloned();
                out.push(Leaf {
                    segments: all,
                    name,
                });
            }
        }
        // `use a::b::{self}` binds `b`.
        "self" => {
            if let Some(name) = prefix.last().cloned() {
                let segments = prefix.clone();
                let name = Some(name);
                out.push(Leaf { segments, name });
            }
        }
        "use_as_clause" => {
            let alias = field(node, "alias").map(|a| text(a, src).to_string());
            let tail = field(node, "path").and_then(|p| segments(p, src));
            if let (Some(alias), Some(tail)) = (alias, tail)
                && alias != "_"
            {
                let tail = if tail == ["self"] && !prefix.is_empty() {
                    Vec::new()
                } else {
                    tail
                };
                let segments = joined(prefix, tail);
                let name = Some(alias);
                out.push(Leaf { segments, name });
            }
        }
        "scoped_use_list" => {
            let tail = match field(node, "path") {
                Some(path) => segments(path, src),
                None => Some(Vec::new()),
            };
            if let (Some(tail), Some(list)) = (tail, field(node, "list")) {
                let at = prefix.len();
                prefix.extend(tail);
                leaves(list, prefix, depth + 1, room, src, out);
                prefix.truncate(at);
            }
        }
        "use_list" => {
            for child in named(node) {
                leaves(child, prefix, depth + 1, room, src, out);
            }
        }
        "use_wildcard" => {
            let tail = match named(node).next() {
                Some(path) => segments(path, src),
                None => Some(Vec::new()),
            };
            let segments = tail.map(|tail| joined(prefix, tail));
            if let Some(segments) = segments.filter(|s| !s.is_empty()) {
                out.push(Leaf {
                    segments,
                    name: None,
                });
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
/// module inside a function, whose place in the file no path names. A `use` path (`module`)
/// may be the module itself (`super`, `self`).
fn normalize(path: &Path, scan: &Scan, heads: &mut Heads, module: bool) -> Option<String> {
    let segments = &path.segments;
    let head = segments.first()?.as_str();
    match head {
        "crate" => {
            heads.imports.insert(("crate".into(), "crate".into()));
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
            if rest.is_empty() && !module {
                return None;
            }
            let module: Vec<&str> = path.module.split('.').filter(|m| !m.is_empty()).collect();
            if ups > module.len() {
                // Out of the file: the module that declares it, and so on up.
                let up = ups - module.len();
                let local = vec!["super"; up].join(".");
                heads
                    .imports
                    .insert((local.clone(), vec!["super"; up].join("::")));
                let anchored = [local.as_str()]
                    .into_iter()
                    .chain(rest.iter().map(String::as_str));
                return Some(anchored.collect::<Vec<_>>().join("."));
            }
            heads.imports.insert((SELF.into(), "self".into()));
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
            if !scan.defines(path, head) {
                // Neither the module nor a `use` binds it: a crate outside the repository.
                heads.externs.insert(head.to_string());
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
            ("crate", "crate"),
            ("mod", "self"),
            ("super.super", "super::super"),
        ];
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(l, s)| (l.to_string(), s.to_string()))
            .collect();
        assert_eq!(imports, pairs);
        // A crate no item of the file names is left to the resolver, after the file's imports.
        assert_eq!(facts(src).externs, ["Vec"]);
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
    fn a_main_in_an_inline_module_is_no_program() {
        let inline = facts("mod x { pub fn main() {} }\n");
        let files = BTreeMap::from([("src/main.rs", &inline)]);
        let configs: [(&str, &[u8]); 1] = [("Cargo.toml", b"[package]\nname = \"p\"\n")];
        assert!(Rust.programs(&configs, &files).is_empty());
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

    /// The imports as `local = path`, a glob as `local * path` (`^` marks one inside the module
    /// it globs), anything else as `local source`.
    fn bindings(src: &str) -> Vec<String> {
        let imports = facts(src).imports.into_iter();
        imports
            .map(|i| match i.name {
                Imported::Path(p) => format!("{} = {p}", i.local),
                Imported::Glob { path, inside, .. } => {
                    format!("{} * {}{path}", i.local, if inside { "^ " } else { "" })
                }
                _ => format!("{} {}", i.local, i.source),
            })
            .collect()
    }

    #[test]
    fn use_trees_bind_each_name_they_spell() {
        let src = "use a::b;\nuse a::{c as d, e::*, f::{self, g}, h::{self as i}};\n\
                   use std::fmt as _;\nuse serde;\nuse super::*;\nuse self::m::n;\n\
                   mod m { use super::*; use crate::z; fn f() { use super::y; } }\n";
        let expected = [
            "b = a.b",
            "d = a.c",
            "* * a.e",
            "f = a.f",
            "g = a.f.g",
            "i = a.h",
            "* * ^ super",
            "n = mod.m.n",
            "m.* * ^ mod",
            "m.z = crate.z",
            "m.f.y = mod.y",
            "crate crate",
            "mod self",
            "super super",
        ];
        assert_eq!(bindings(src), expected);
        // `as _` and a bare crate bind nothing; the first segment of a path is a crate outside
        // the file when nothing binds it.
        assert_eq!(facts(src).externs, ["a"]);
    }

    #[test]
    fn names_a_use_binds_hide_in_their_scope_alone() {
        let src = "mod m { use y::x; }\nfn f() { x::g(); }\n";
        assert_eq!(facts(src).externs, ["x", "y"]);
        let src = "use y::x;\nfn f() { x::g(); }\n";
        assert_eq!(facts(src).externs, ["y"]);
    }

    #[test]
    fn a_use_does_not_import_the_crate_its_own_path_begins_with() {
        // `use tokio::{self}` and `use foo::foo` bind the name their path begins with, which
        // is still a crate outside the file.
        let src = "use tokio::{self, select};\nuse foo::foo;\nfn f() { tokio::spawn(); }\n";
        assert_eq!(facts(src).externs, ["foo", "tokio"]);
        // Another `use` of the name does bind it.
        let src = "use a::tokio;\nuse tokio::{self};\n";
        assert_eq!(facts(src).externs, ["a"]);
    }

    #[test]
    fn exports_are_the_pub_names_reachable_through_pub_modules() {
        let src = "pub fn a() {}\nfn b() {}\npub(crate) fn c() {}\n\
                   pub mod m { pub fn d() {} fn e() {} pub mod k { pub fn l() {} } }\n\
                   mod n { pub fn f() {} }\npub use x::Y;\nuse z::W;\n";
        let facts = facts(src);
        let exported: Vec<&str> = (facts.exports.iter())
            .map(|e| match e {
                Export::Local { exported, .. } | Export::From { exported, .. } => exported.as_str(),
                Export::All { .. } => "*",
            })
            .collect();
        assert_eq!(
            exported,
            ["a", "b", "c", "m", "m.d", "m.k", "m.k.l", "n", "Y", "W"]
        );
        assert_eq!(facts.private, ["b", "c", "n", "W"]);
        assert!(facts.star.takes("a"));
        assert!(!facts.star.takes("W"));
    }

    #[test]
    fn uses_past_the_limit_bind_nothing() {
        let names: Vec<String> = (0..USE_BINDINGS + 5).map(|i| format!("n{i}")).collect();
        let src = format!("use a::{{{}}};\n", names.join(", "));
        assert_eq!(facts(&src).imports.len(), USE_BINDINGS);
        let nested = format!(
            "{}b{}",
            "a::{".repeat(BINDING_DEPTH + 8),
            "}".repeat(BINDING_DEPTH + 8)
        );
        assert!(facts(&format!("use {nested};\n")).imports.is_empty());
    }
}
