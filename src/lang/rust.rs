//! The Rust language pack (spec #30). It reads `.rs` files and the `Cargo.toml` manifests of a
//! workspace with no Rust toolchain: crates come from `Cargo.toml` and the module tree from
//! `mod` declarations (`crates.rs`). Calls inside macro invocations are token trees to
//! tree-sitter and are not indexed, apart from the bodies of item macros (`macros.rs`), which
//! are read as items.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use tree_sitter::{Node, Parser};

use std::ops::Range;
use std::rc::Rc;

use super::{
    Binds, Call, Class, Const, Def, Export, Facts, Impl, Import, Imported, Kind, LanguagePack,
    Layout, Modules, Param, Receiver, Star, children, named, text,
};
use crate::{Result, qualify};

mod builders;
pub(crate) mod cfg;
pub(crate) mod crates;
mod macros;
mod mentions;
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

/// The HTTP methods the `Method` constants of the HTTP crates name, which an actix-web
/// `web::method(Method::X)` may be given. The pack folds them at parse time (`values.rs`), and
/// the HTTP kind's Rust reader reads the same list: the one pack constant a kind reads.
pub(crate) const VERBS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE",
];

/// The attributes that register the function they sit on as a route, in actix-web and Rocket:
/// `#[get("/x")]` and its kin, and actix-web's `#[route("/x", method = "GET")]`. The pack emits
/// them as calls that decorate the function (`Call::decorates`); any other attribute is not a
/// call.
const ROUTE_ATTRIBUTES: &[&str] = &[
    "get", "post", "put", "delete", "head", "connect", "options", "trace", "patch", "route",
];

/// How many route attributes one function is read with; the rest are not calls.
// ponytail: a handler serves a handful of routes; a crafted run of a million is not read.
const ROUTES_PER_FUNCTION: usize = 16;

pub struct Rust;

impl LanguagePack for Rust {
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn parse(&self, path: &str, src: &[u8]) -> Result<Facts> {
        self.parse_with(path, src, "")
    }

    fn parse_with(&self, _path: &str, src: &[u8], setting: &str) -> Result<Facts> {
        parse(src, setting)
    }

    fn settings(&self, configs: &[(&str, &[u8])]) -> Box<dyn Fn(&str) -> String> {
        let crates = crates::features(configs);
        Box::new(move |path| {
            let owns = |dir: &str| {
                dir.is_empty() || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
            };
            // The nearest package above the file.
            (crates.iter().filter(|(dir, _)| owns(dir)))
                .max_by_key(|(dir, _)| dir.len())
                .map(|(_, setting)| setting.clone())
                .unwrap_or_default()
        })
    }

    fn extend(&self, files: &BTreeMap<&str, &Facts>) -> BTreeMap<String, String> {
        macros::extend(files)
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

    fn strict(&self) -> bool {
        true
    }

    /// A type's name is its path in its file (`inner.Def`), never the module a glob names
    /// (`super`, `crate::x`): a glob takes its types as it takes its functions.
    fn namespaced(&self) -> bool {
        false
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        Ok(self.layout(configs, files)?.modules)
    }

    fn layout(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Result<Layout> {
        let tree = Rc::new(Tree::new(configs, files));
        let programs = (tree.bins())
            .filter(|bin| files[bin].defs.iter().any(|d| d.name == MAIN))
            .map(|bin| (bin.to_string(), MAIN.to_string()))
            .collect();
        let lookup = Rc::clone(&tree);
        Ok(Layout {
            members: Some(tree.members()),
            published: Some(tree.published()),
            programs,
            modules: Box::new(move |from, specifier| lookup.lookup(from, specifier).into()),
            reaches: Some(Box::new(move |from, to| tree.reaches(from, to))),
            shared: Vec::new(),
        })
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
    /// How many nodes are above it, and the items that may declare type parameters among them
    /// (the node itself too, when it is one), nearest first: `parameter` reads them from here.
    depth: usize,
    items: Option<Rc<Items<'t>>>,
}

/// An item around a node, with its depth in the tree.
struct Items<'t> {
    item: Node<'t>,
    depth: usize,
    up: Option<Rc<Items<'t>>>,
}

// Dropped one at a time: items nested thousands deep in a generated file would overflow the call
// stack if each one dropped the next.
impl Drop for Items<'_> {
    fn drop(&mut self) {
        let mut up = self.up.take();
        while let Some(items) = up {
            up = Rc::into_inner(items).and_then(|mut i| i.up.take());
        }
    }
}

/// Where a type is in the tree, for `parameter`: a parent lookup walks down from the root of
/// the tree, so it costs the depth of the node, and the walk already knows what it needs.
#[derive(Clone, Copy)]
struct Around<'a, 't> {
    depth: usize,
    items: Option<&'a Items<'t>>,
}

impl Around<'_, '_> {
    /// The place of a node `levels` below this one.
    fn down(self, levels: usize) -> Self {
        Around {
            depth: self.depth + levels,
            ..self
        }
    }
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
    /// The class of this receiver, a type the source writes.
    Named(usize),
    /// The class this function returns, a type the source writes.
    Result(usize),
}

/// What the file binds, for the paths: the qualified names of its non-method definitions
/// (`mod` declarations included), its locals (written once each), the names its `use`
/// declarations bind, qualified by the scope they are written in, and how many bind each.
#[derive(Default)]
struct Scan {
    defs: HashSet<String>,
    /// The functions among `defs`: a path (`rocket::build()`) does not name one by its head.
    functions: HashSet<String>,
    locals: HashSet<String>,
    uses: HashMap<String, usize>,
    /// The plain names the parameters of each function bind, as `(function, name)`.
    params: HashSet<(String, String)>,
    /// The path each name a `use` binds imports, as written (the last of them, when it binds the
    /// name more than once, which `uses` counts).
    targets: HashMap<String, Vec<String>>,
    /// The macros the file defines: a `format!` it defines itself is not the standard one.
    macros: HashSet<String>,
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
        let head = path.segments.len() > 1;
        let (module, mut s) = (path.module.as_str(), path.scope.as_str());
        loop {
            let qualified = qualify(s, name);
            let own = usize::from(path.binds.as_ref() == Some(&qualified));
            let used = self.uses.get(&qualified).copied().unwrap_or(0);
            let defined =
                self.defs.contains(&qualified) && !(head && self.functions.contains(&qualified));
            if defined || used > own {
                return true;
            }
            if s.len() <= module.len() {
                return false;
            }
            s = crate::parent(s);
        }
    }
}

fn parse(src: &[u8], setting: &str) -> Result<Facts> {
    let features: BTreeSet<String> = (super::base(setting).split(','))
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect();
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let table = macros::table(super::extra(setting));
    let bodies = macros::bodies(tree.root_node(), src, &table)?;
    let expand = Expand {
        roots: (bodies.iter())
            .map(|(at, body)| (*at, body.root_node()))
            .collect(),
        table: &table,
    };
    let mut facts = Facts::default();
    let mut scan = Scan::default();
    let mut paths: Vec<(Slot, Path)> = Vec::new();
    // The receivers a `let` gives by the call it holds, with the call's span.
    let mut returned: Vec<(usize, Range<usize>)> = Vec::new();
    let mut imported: Vec<Use> = Vec::new();
    let mut bound = 0;
    let mut defined = HashSet::new();
    // The last `#n` given to the blocks of each scope.
    let mut suffixes: HashMap<String, usize> = HashMap::new();
    let mut public_names = Vec::new();
    // The values to read once the file is, and how many bindings each local name has.
    let mut held: Vec<Held> = Vec::new();
    // The calls made on another call (`Router::new().route(..)`) and the range of the inner one.
    let mut chains: Vec<(usize, Range<usize>)> = Vec::new();
    let mut bindings: HashMap<String, usize> = HashMap::new();
    // The names a `let mut` of a plain name declares.
    let mut mut_lets: HashSet<String> = HashSet::new();
    // The scope and name of each assignment: it changes the nearest binding of the name, which
    // an enclosing scope may hold (a closure given to a call is a scope of its own).
    let mut assigned: Vec<(String, String)> = Vec::new();
    // The Strings a function builds, by scope and name, and the names two of them share.
    let mut candidates: values::Builders = HashMap::new();
    let mut clash: HashSet<(String, String)> = HashSet::new();
    let mut spent = 0;
    // The `#[path]` value of each item that has one, by node id.
    let mut renamed: HashMap<usize, String> = HashMap::new();
    // The route attributes before each function, by node id.
    let mut routes: HashMap<usize, Routes> = HashMap::new();
    // Explicit stack instead of recursion: generated code can nest deeper than the call stack.
    let mut stack = vec![Visit {
        node: tree.root_node(),
        scope: String::new(),
        module: String::new(),
        pure: true,
        body: Body::Items,
        open: true,
        arg: false,
        depth: 0,
        items: None,
    }];
    while let Some(Visit {
        node,
        scope,
        module,
        pure,
        body,
        open,
        arg,
        depth,
        items,
    }) = stack.pop()
    {
        let items = match node.kind() {
            "function_item"
            | "function_signature_item"
            | "impl_item"
            | "trait_item"
            | "struct_item"
            | "enum_item" => Some(Rc::new(Items {
                item: node,
                depth,
                up: items,
            })),
            _ => items,
        };
        let place = Around {
            depth,
            items: items.as_deref(),
        };
        let mut inner = (scope.clone(), module.clone(), pure, body.clone(), open);
        // Directly in a module (the file's or one written in place): not in a function, an
        // `impl` or a trait.
        let in_module = pure && scope == module && matches!(body, Body::Items);
        let deep = levels(&scope) >= DEFINITION_DEPTH;
        let mut locals = Vec::new();
        match node.kind() {
            // Macro bodies are token trees; attributes are read by the item they sit on.
            "macro_definition" => {
                scan.macros
                    .extend(field(node, "name").map(|n| text(n, src).to_string()));
                continue;
            }
            "macro_invocation" | "attribute_item" | "inner_attribute_item" => continue,
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
                        scan.targets.insert(qualify(&scope, name), segments.clone());
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
            // A required method of a trait is a definition of its own; a foreign function is none.
            "function_signature_item" if deep || !matches!(body, Body::Trait(_)) => continue,
            "function_item" if deep => {
                locals.extend(field(node, "name").map(|n| text(n, src).to_string()));
                for param in parameters(node, src) {
                    locals.extend(param.names);
                }
            }
            "function_item" | "function_signature_item" => {
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
                scan.params.extend(
                    (params.iter())
                        .filter(|p| !p.name.is_empty())
                        .map(|p| (qualified.clone(), p.name.clone())),
                );
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
                        scan.functions.insert(qualified.clone());
                    }
                    let path = |segments| Path {
                        scope: qualified.clone(),
                        module: module.clone(),
                        pure,
                        segments,
                        binds: None,
                    };
                    // A type parameter as the result names no class: it is the caller's type.
                    let result = field(node, "return_type")
                        .and_then(|t| written(t, place.down(1), src, false));
                    if let Some(result) = result {
                        paths.push((Slot::Result(facts.defs.len()), path(result)));
                    }
                    if kind == Kind::Method && has_self(node) {
                        facts.receivers.push(Receiver {
                            scope: qualified.clone(),
                            name: "self".into(),
                            class: Class::Enclosing,
                        });
                    }
                    for (name, segments) in typed_parameters(node, place, src) {
                        paths.push((Slot::Named(facts.receivers.len()), path(segments)));
                        facts.receivers.push(Receiver {
                            scope: qualified.clone(),
                            name,
                            class: Class::Unknown,
                        });
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
                    if kind == Kind::Function {
                        let attributes = routes.remove(&node.id()).unwrap_or_default();
                        for (item, segments, tree) in attributes {
                            let (args, kwargs) = values::attribute(tree, src);
                            let by_path = segments.len() > 1;
                            let callee = segments.first().filter(|_| !by_path).cloned();
                            if by_path {
                                let at = facts.calls.len();
                                let path = Path {
                                    scope: scope.clone(),
                                    module: module.clone(),
                                    pure,
                                    segments,
                                    binds: None,
                                };
                                paths.push((Slot::Call(at), path));
                            }
                            facts.calls.push(Call {
                                arity: None,
                                labels: None,
                                constructs: false,
                                awaited: false,
                                scope: scope.clone(),
                                callee: callee.unwrap_or_default(),
                                range: item.byte_range(),
                                args,
                                kwargs,
                                decorates: Some(qualified.clone()),
                                by_path,
                                root: None,
                                on: None,
                                receiver: None,
                                arg_types: Vec::new(),
                                uses: Vec::new(),
                            });
                        }
                    }
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
                    // A `static mut` is written to elsewhere: it holds no known value.
                    if let Some(value) = field(node, "value").filter(|_| !mutable(node)) {
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
                // A function and a type of one name live in different namespaces.
                scan.defs.insert(qualified.clone());
                scan.functions.remove(&qualified);
                if defined.insert(qualified.clone()) {
                    facts.defs.push(def(&qualified, Kind::Class, node));
                    if node.kind() == "struct_item" {
                        for (name, segments) in fields(node, place, src) {
                            let path = Path {
                                scope: qualified.clone(),
                                module: module.clone(),
                                pure,
                                segments,
                                binds: None,
                            };
                            paths.push((Slot::Named(facts.receivers.len()), path));
                            facts.receivers.push(Receiver {
                                scope: qualified.clone(),
                                name,
                                class: Class::Unknown,
                            });
                        }
                    }
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
                    if in_module && !deep {
                        scan.defs.insert(qualified.clone());
                        scan.functions.remove(&qualified);
                    }
                    if in_module && !deep && defined.insert(qualified.clone()) {
                        declare(
                            &mut facts,
                            &mut public_names,
                            node,
                            renamed.get(&node.id()),
                            &module,
                            name,
                            src,
                        );
                    }
                    continue;
                }
                if deep {
                    // ponytail: past the depth limit, a module's items are named in the one
                    // around it.
                } else {
                    let qualified = qualify(&scope, name);
                    scan.defs.insert(qualified.clone());
                    scan.functions.remove(&qualified);
                    if defined.insert(qualified.clone()) {
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
                let root = chain_root(function, src);
                if root.is_none()
                    && callee.starts_with('(')
                    && let Some(inner) = inner_call(function)
                {
                    chains.push((facts.calls.len(), inner.byte_range()));
                }
                facts.calls.push(Call {
                    arity: None,
                    labels: None,
                    constructs: false,
                    awaited: false,
                    scope: scope.clone(),
                    callee,
                    range: node.byte_range(),
                    args: Vec::new(),
                    kwargs: Vec::new(),
                    decorates: None,
                    by_path,
                    root,
                    on: None,
                    receiver: None,
                    arg_types: Vec::new(),
                    uses: Vec::new(),
                });
            }
            // A `let` of a plain name whose type is written, or whose value is a call, is a
            // receiver (`Facts::receivers`), unless a parameter of the function binds the name
            // too: the two cannot tell which a call reaches, so the name is a local.
            "let_declaration"
                if !deep
                    && typed_let(node, place, src)
                        .is_some_and(|(name, _)| !scan.params.contains(&(scope.clone(), name))) =>
            {
                let Some((name, how)) = typed_let(node, place, src) else {
                    continue;
                };
                match how {
                    How::Type(segments) => {
                        let path = Path {
                            scope: scope.clone(),
                            module: module.clone(),
                            pure,
                            segments,
                            binds: None,
                        };
                        paths.push((Slot::Named(facts.receivers.len()), path));
                    }
                    How::Call(range) => returned.push((facts.receivers.len(), range)),
                }
                let class = Class::Unknown;
                // It is a constant too, if nothing else binds the name in its scope.
                let local = qualify(&scope, &name);
                *bindings.entry(local.clone()).or_default() += 1 + usize::from(mutable(node));
                if mutable(node) {
                    mut_lets.insert(local.clone());
                }
                if let Some(value) = field(node, "value") {
                    let at = Where {
                        scope: scope.clone(),
                        module: module.clone(),
                        pure,
                    };
                    let put = Put::Let(local);
                    held.push(Held {
                        at,
                        node: value,
                        put,
                    });
                }
                facts.receivers.push(Receiver {
                    scope: scope.clone(),
                    name,
                    class,
                });
            }
            "let_declaration" | "for_expression" | "let_condition" | "match_arm" => {
                if let Some(pattern) = field(node, "pattern") {
                    bind(pattern, 0, &mut locals, src);
                    // A `let` of a plain name is a constant if nothing else binds the name in
                    // its scope: a `let mut` is a second binding, as an assignment is.
                    let value = field(node, "value");
                    if let (true, Some(value), "identifier") =
                        (node.kind() == "let_declaration", value, pattern.kind())
                    {
                        let at = Where {
                            scope: scope.clone(),
                            module: module.clone(),
                            pure,
                        };
                        let local = qualify(&scope, text(pattern, src));
                        *bindings.entry(local.clone()).or_default() += usize::from(mutable(node));
                        if mutable(node) {
                            mut_lets.insert(local.clone());
                        }
                        let put = Put::Let(local);
                        held.push(Held {
                            at,
                            node: value,
                            put,
                        });
                    }
                }
            }
            // A name assigned again is not a constant.
            "assignment_expression" | "compound_assignment_expr" => {
                if let Some(left) = field(node, "left").filter(|n| n.kind() == "identifier") {
                    assigned.push((scope.clone(), text(left, src).to_string()));
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
        let kids = match node.kind() {
            "source_file"
                if inner_off(node, &features, src, false)
                    && inner_off(node, &features, src, true) =>
            {
                facts.unbuilt = true;
                Vec::new()
            }
            _ => built(node, &features, src, &mut renamed, &mut routes, &expand),
        };
        // A `let mut` String the rest of its block only reads and appends to: the statements
        // after it are the ones `kids` holds, read once (a sibling lookup costs the depth of the
        // node). Two in one scope under one name are told apart by no position the Reader has:
        // neither is kept.
        if node.kind() == "block" && !deep {
            for (at, kid) in kids.iter().enumerate() {
                if builders::candidate(*kid)
                    && let Some(name) = field(*kid, "pattern").filter(|p| p.kind() == "identifier")
                    && let Some(found) =
                        builders::builder(*kid, node, &kids[at + 1..], src, &mut spent)
                {
                    let key = (scope.clone(), text(name, src).to_string());
                    if !clash.contains(&key) && candidates.insert(key.clone(), found).is_some() {
                        candidates.remove(&key);
                        clash.insert(key);
                    }
                }
            }
        }
        stack.extend(kids.into_iter().rev().map(|child| Visit {
            node: child,
            scope: scope.clone(),
            module: module.clone(),
            pure,
            body: body.clone(),
            open,
            arg: node.kind() == "arguments",
            depth: depth + 1,
            items: items.clone(),
        }));
    }
    for (scope, name) in assigned {
        let mut at = scope.as_str();
        let home = loop {
            let local = qualify(at, &name);
            if bindings.contains_key(&local) || at.is_empty() {
                break local;
            }
            at = crate::parent(at);
        };
        // A name nothing binds is counted where it is written, as it always was.
        *bindings.entry(home).or_default() += 1;
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
            Slot::Named(at) => {
                facts.receivers[*at].class = callee.map_or(Class::Unknown, Class::Type);
            }
            Slot::Result(at) => facts.defs[*at].result = callee.map(Class::Type),
        }
    }
    let called: HashMap<(usize, usize), usize> = (facts.calls.iter().enumerate())
        .map(|(n, c)| ((c.range.start, c.range.end), n))
        .collect();
    for (outer, inner) in chains {
        facts.calls[outer].on = called.get(&(inner.start, inner.end)).copied();
    }
    // The values, now that the file's `use` declarations are known.
    let mut reader = Reader::new(src, &scan, &mut heads, &candidates);
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
                    scope: None,
                });
            }
            Put::Let(name) => {
                let value = reader.value(&at, node);
                // Declared `mut` once, and bound or assigned nowhere else.
                if bindings.get(&name) == Some(&2) && mut_lets.contains(&name) {
                    facts.mutable.insert(name.clone(), value.clone());
                }
                facts
                    .rebound
                    .push((crate::parent(&name).to_string(), value));
            }
            Put::Item(name) => {
                let value = reader.value(&at, node);
                facts.consts.push(Const {
                    name,
                    value,
                    ty: None,
                    scope: None,
                });
            }
            Put::Returns(def) => facts.defs[def].return_values = reader.returns(&at, node),
        }
    }
    // A constant is a name that holds its value, not a local hiding what it is bound to.
    facts.locals.retain(|l| !constants.contains(l));
    // A `let` holding a call: the function it names, when the call is a path or a bare name.
    let callees: HashMap<&Range<usize>, &str> = (facts.calls.iter())
        .filter(|c| c.by_path || !c.callee.contains('.'))
        .filter(|c| !c.callee.starts_with('('))
        .map(|c| (&c.range, c.callee.as_str()))
        .collect();
    for (at, range) in returned {
        facts.receivers[at].class = match callees.get(&range) {
            Some(callee) => Class::Returned(callee.to_string()),
            None => Class::Unknown,
        };
    }
    // One receiver per scope and name: bindings that disagree leave its class unknown.
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut receivers: Vec<Receiver> = Vec::new();
    for receiver in std::mem::take(&mut facts.receivers) {
        match seen.get(&(receiver.scope.clone(), receiver.name.clone())) {
            Some(&at) if receivers[at].class != receiver.class => {
                receivers[at].class = Class::Unknown;
            }
            Some(_) => {}
            None => {
                seen.insert(
                    (receiver.scope.clone(), receiver.name.clone()),
                    receivers.len(),
                );
                receivers.push(receiver);
            }
        }
    }
    facts.receivers = receivers;
    // A type or trait no path names reaches no class.
    facts.impls.retain(|i| !i.ty.is_empty());
    facts.externs = heads.externs.into_iter().collect();
    facts.macro_use = macro_use(tree.root_node(), src);
    facts.mentions = mentions::mentions(tree.root_node(), src);
    macros::record(tree.root_node(), src, &mut facts);
    for (local, source) in heads.imports {
        facts.imports.push(Import {
            local,
            source,
            name: Imported::Namespace,
        });
    }
    facts.star = Star::Only(public_names);
    facts.setting = setting.to_string();
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
        result: None,
        suspends: false,
        hash: String::new(),
    }
}

/// The route attributes of a function: the attribute item, the segments of its path (`get`,
/// `actix_web::get`) and its argument tokens, first first.
type Routes<'t> = Vec<(Node<'t>, Vec<String>, Node<'t>)>;

/// The route attributes written at the end of `before`, the children of a node that come before
/// a function. Comments between attributes are skipped, and only `ROUTES_PER_FUNCTION` are kept.
fn route_attributes<'t>(before: &[Node<'t>], src: &[u8]) -> Routes<'t> {
    let mut out = Vec::new();
    for &item in before.iter().rev() {
        match item.kind() {
            "line_comment" | "block_comment" => {}
            "attribute_item" => {
                if let Some((segments, tree)) = route_attribute(item, src)
                    && out.len() < ROUTES_PER_FUNCTION
                {
                    out.push((item, segments, tree));
                }
            }
            _ => break,
        }
    }
    out.reverse();
    out
}

/// The segments of the path and the argument tokens of a route attribute item.
fn route_attribute<'t>(item: Node<'t>, src: &[u8]) -> Option<(Vec<String>, Node<'t>)> {
    let attribute = named(item).find(|n| n.kind() == "attribute")?;
    let tree = field(attribute, "arguments")?;
    let segments = segments(named(attribute).next()?, src)?;
    (segments.last())
        .is_some_and(|s| ROUTE_ATTRIBUTES.contains(&s.as_str()))
        .then_some((segments, tree))
}

/// The crates the file declares with `#[macro_use] extern crate x;` at its top level.
fn macro_use(root: Node, src: &[u8]) -> Vec<String> {
    let (mut out, mut marked) = (Vec::new(), false);
    for item in named(root) {
        match item.kind() {
            "line_comment" | "block_comment" => {}
            "attribute_item" => marked |= text(item, src).replace(' ', "") == "#[macro_use]",
            "extern_crate_declaration" => {
                if let Some(name) = field(item, "name").filter(|_| marked) {
                    out.push(text(name, src).to_string());
                }
                marked = false;
            }
            _ => marked = false,
        }
    }
    out
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
    path: Option<&String>,
    module: &str,
    name: &str,
    src: &[u8],
) {
    let chain = module.replace('.', "/");
    let source = match path {
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

/// The children of `node` that are built, in one pass over the list (a sibling lookup per
/// child would be quadratic in a flat file): the `#[cfg(..)]` attributes before a child decide
/// it, and so does an inner `#![cfg(..)]` of a module written in place. A `#[path]` value
/// goes to `paths` under the id of the item it sits on, and the route attributes before a
/// function to `routes`. A `#[cfg_attr(..)]` only adds attributes, so it never leaves an item out.
/// An item macro's invocation that is built gives the items of its body instead (`expand`).
fn built<'t>(
    node: Node<'t>,
    features: &BTreeSet<String>,
    src: &[u8],
    paths: &mut HashMap<usize, String>,
    routes: &mut HashMap<usize, Routes<'t>>,
    expand: &Expand<'t, '_>,
) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let kids: Vec<Node> = children(node).collect();
    let mut reader = Built {
        features,
        src,
        paths,
        routes,
        expand,
    };
    reader.keep(&kids, (true, true), &mut out);
    out
}

/// The bodies of the item macros a file invokes in a module (`macros::bodies`), by the start of
/// the invocation, and what the file's setting says of each macro.
struct Expand<'t, 's> {
    roots: HashMap<usize, Node<'t>>,
    table: &'s macros::Table<'s>,
}

/// What `built` reads the children of a node with.
struct Built<'a, 't, 's> {
    features: &'a BTreeSet<String>,
    src: &'a [u8],
    paths: &'a mut HashMap<usize, String>,
    routes: &'a mut HashMap<usize, Routes<'t>>,
    expand: &'a Expand<'t, 's>,
}

impl<'t> Built<'_, 't, '_> {
    /// Adds the `kids` that are built to `out`, each judged by the attributes before it and by
    /// `outer`, what the attributes an item macro's arm writes on every item make of it (with
    /// `test` false and true). An item macro's body is read in its place, as items.
    fn keep(&mut self, kids: &[Node<'t>], outer: (bool, bool), out: &mut Vec<Node<'t>>) {
        let (features, src) = (self.features, self.src);
        let (mut on, mut on_test, mut path) = (outer.0, outer.1, None);
        for (at, &child) in kids.iter().enumerate() {
            match child.kind() {
                "attribute_item" => {
                    let (plain, test) = attribute_holds(child, features, src);
                    (on, on_test) = (on && plain, on_test && test);
                    path = path_value(child, src).or(path);
                }
                "line_comment" | "block_comment" | "inner_attribute_item" => out.push(child),
                _ => {
                    // A module a test build compiles is indexed; any other item is judged with
                    // `test` false. Its inner `#![cfg(..)]` is judged in the same build as its
                    // outer one.
                    let kept = (on && !inner_off(child, features, src, false))
                        || (on_test
                            && child.kind() == "mod_item"
                            && !inner_off(child, features, src, true));
                    let body = (child.kind() == "macro_invocation")
                        .then(|| self.expand.roots.get(&child.start_byte()))
                        .flatten();
                    match body {
                        // An item macro's items are judged by the attributes on the invocation,
                        // then by those the arm writes on each: a module in it may be one a
                        // test build compiles.
                        Some(&body) if on || on_test => {
                            let name = field(child, "macro").map(|m| text(m, src));
                            let predicates = name.and_then(|n| self.expand.table.get(n));
                            let holds = |test| {
                                (predicates.into_iter().flatten())
                                    .all(|p| cfg::holds(p, features, test))
                            };
                            let items: Vec<Node> = children(body).collect();
                            let gate = (on && holds(false), on_test && holds(true));
                            self.keep(&items, gate, out);
                        }
                        None if kept => {
                            if let Some(value) = path {
                                self.paths.insert(child.id(), value);
                            }
                            if child.kind() == "function_item" {
                                let found = route_attributes(&kids[..at], src);
                                if !found.is_empty() {
                                    self.routes.insert(child.id(), found);
                                }
                            }
                            out.push(child);
                        }
                        _ => {}
                    }
                    (on, on_test, path) = (outer.0, outer.1, None);
                }
            }
        }
    }
}

/// Whether an attribute item is no `#[cfg(..)]`, or one that holds, with `test` false and with
/// it true.
fn attribute_holds(item: Node, features: &BTreeSet<String>, src: &[u8]) -> (bool, bool) {
    let attribute = named(item).find(|n| n.kind() == "attribute");
    let condition = attribute.and_then(|a| text(a, src).strip_prefix("cfg"));
    let condition = condition.filter(|c| c.trim_start().starts_with('('));
    let condition = condition.map(str::trim_start);
    let holds = |test| condition.is_none_or(|c| cfg::holds(c, features, test));
    (holds(false), holds(true))
}

/// Whether the file or the module written in place that `node` is (or holds as its body) opens
/// with an inner `#![cfg(..)]` that does not hold, with `test` as given.
fn inner_off(node: Node, features: &BTreeSet<String>, src: &[u8], test: bool) -> bool {
    let body = match node.kind() {
        "mod_item" => field(node, "body"),
        _ => Some(node),
    };
    let Some(body) = body.filter(|b| matches!(b.kind(), "source_file" | "declaration_list")) else {
        return false;
    };
    let mut cursor = body.walk();
    for inner in body.named_children(&mut cursor) {
        match inner.kind() {
            "inner_attribute_item" => {
                let attribute = named(inner).find(|n| n.kind() == "attribute");
                let condition = attribute.and_then(|a| text(a, src).strip_prefix("cfg"));
                let condition = condition.filter(|c| c.trim_start().starts_with('('));
                if condition.is_some_and(|c| !cfg::holds(c.trim_start(), features, test)) {
                    return true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => return false,
        }
    }
    false
}

/// The value of a `#[path = "..."]` attribute item, if it is a plain string.
fn path_value(item: Node, src: &[u8]) -> Option<String> {
    let attribute = named(item).find(|n| n.kind() == "attribute")?;
    let is_path = named(attribute)
        .next()
        .is_some_and(|n| text(n, src) == "path");
    let value = field(attribute, "value").filter(|v| is_path && v.kind() == "string_literal")?;
    let parts: Vec<Node> = named(value).collect();
    match parts[..] {
        [part] if part.kind() == "string_content" => Some(text(part, src).to_string()),
        _ => None,
    }
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
            label: None,
            default: None,
            fields: Vec::new(),
            closure: Default::default(),
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

/// Whether the function takes `self`.
fn has_self(function: Node) -> bool {
    let list = field(function, "parameters");
    list.is_some_and(|l| named(l).any(|p| p.kind() == "self_parameter"))
}

/// The parameters named by a plain identifier whose type the source writes, with the path of
/// that type.
fn typed_parameters(function: Node, place: Around, src: &[u8]) -> Vec<(String, Vec<String>)> {
    let Some(list) = field(function, "parameters") else {
        return Vec::new();
    };
    let typed = named(list).filter(|p| p.kind() == "parameter");
    typed
        .filter_map(|p| {
            let name = field(p, "pattern").filter(|n| n.kind() == "identifier")?;
            Some((
                text(name, src).to_string(),
                written(field(p, "type")?, place.down(3), src, true)?,
            ))
        })
        .collect()
}

/// The named fields of a struct whose type the source writes, with the path of that type.
fn fields(item: Node, place: Around, src: &[u8]) -> Vec<(String, Vec<String>)> {
    let Some(list) = field(item, "body").filter(|b| b.kind() == "field_declaration_list") else {
        return Vec::new();
    };
    let declared = named(list).filter(|f| f.kind() == "field_declaration");
    declared
        .filter_map(|f| {
            let name = text(field(f, "name")?, src).to_string();
            Some((name, written(field(f, "type")?, place.down(3), src, true)?))
        })
        .collect()
}

/// What a `let` of a plain name gives its receiver: the type it writes, or the call it holds.
enum How {
    Type(Vec<String>),
    /// The span of the call (`Call::range`).
    Call(Range<usize>),
}

/// Whether a `let` or `static` is `mut`.
fn mutable(node: Node) -> bool {
    children(node).any(|c| c.kind() == "mutable_specifier")
}

fn typed_let(node: Node, place: Around, src: &[u8]) -> Option<(String, How)> {
    let pattern = field(node, "pattern").filter(|p| p.kind() == "identifier")?;
    let name = text(pattern, src).to_string();
    if let Some(ty) = field(node, "type") {
        return Some((name, How::Type(written(ty, place.down(1), src, true)?)));
    }
    // Only a call of a name or a path: the function it calls is what the call's own callee says.
    let value = field(node, "value").filter(|v| v.kind() == "call_expression")?;
    let function = field(value, "function")?;
    let function = match function.kind() {
        "generic_function" => field(function, "function")?,
        _ => function,
    };
    matches!(function.kind(), "identifier" | "scoped_identifier")
        .then(|| (name, How::Call(value.byte_range())))
}

/// The path of the item a written type names: `&T` and `&mut T` name `T`; `dyn Trait`, `impl
/// Trait` and a type parameter bounded by one trait name that trait, so a call on it stops at
/// the trait's method. `None` for a type that names no single item: a tuple, a slice, a
/// function, a parameter bounded by several traits or by none. With `generic` false a type
/// parameter names nothing (a function's result: it is whatever its caller passes). `place` is
/// where `ty` is.
fn written(ty: Node, mut place: Around, src: &[u8], generic: bool) -> Option<Vec<String>> {
    let mut at = ty;
    for _ in 0..PATH_DEPTH {
        return match at.kind() {
            "reference_type" => {
                at = field(at, "type")?;
                place = place.down(1);
                continue;
            }
            "dynamic_type" | "abstract_type" | "bounded_type" => only_bound(&[at], src),
            // `(dyn A + Send)`: a parenthesized type is a tuple of one.
            "tuple_type" if at.child_count() == 3 && at.named_child_count() == 1 => {
                at = at.named_child(0)?;
                place = place.down(1);
                continue;
            }
            "type_identifier" => match parameter(place, text(at, src), src) {
                Some(bound) => bound.filter(|_| generic),
                None => Some(vec![text(at, src).to_string()]),
            },
            "primitive_type" => Some(vec![text(at, src).to_string()]),
            "scoped_type_identifier" | "generic_type" => {
                let path = segments(at, src)?;
                // `T::Item`: a member of a type parameter names nothing here.
                match parameter(place, &path[0], src) {
                    Some(_) => None,
                    None => Some(path),
                }
            }
            _ => None,
        };
    }
    None
}

/// The path of the one trait among `bounds` (a `+` list is a `bounded_type`), ignoring
/// lifetimes, `?Sized` and the marker traits `Send`, `Sync`, `Unpin` and `Sized`, which add no
/// method; `None` for none or several.
fn only_bound(bounds: &[Node], src: &[u8]) -> Option<Vec<String>> {
    // `A + B + 'c` nests: `(A + B) + 'c`.
    let mut found = Vec::new();
    let mut pending: Vec<Node> = bounds.to_vec();
    while let Some(bound) = pending.pop() {
        if found.len() + pending.len() >= PATH_DEPTH {
            return None;
        }
        match bound.kind() {
            "bounded_type" => pending.extend(named(bound)),
            // The first bound of `impl A + B` and `dyn A + B` is the type's own `trait`.
            "dynamic_type" | "abstract_type" => pending.extend(field(bound, "trait")),
            _ => found.push(bound),
        }
    }
    found.retain(|b| !matches!(b.kind(), "lifetime" | "removed_trait_bound" | "use_bounds"));
    let marker = |b: &Node| {
        let path = segments(*b, src).unwrap_or_default();
        let std =
            path.len() == 1 || matches!(path.first().map(String::as_str), Some("std" | "core"));
        let last = path.last().map(String::as_str);
        std && matches!(last, Some("Send" | "Sync" | "Unpin" | "Sized"))
    };
    found.retain(|b| !marker(b));
    match found[..] {
        [one]
            if matches!(
                one.kind(),
                "type_identifier" | "scoped_type_identifier" | "generic_type"
            ) =>
        {
            segments(one, src)
        }
        _ => None,
    }
}

/// The type parameter `name` as the items around the type at `place` declare it: `Some` of the
/// one trait it is bounded by (in `<T: Trait>` or a `where` clause), or of `None` when it has no
/// single bound; `None` when no item around declares it, so the name is a type.
fn parameter(place: Around, name: &str, src: &[u8]) -> Option<Option<Vec<String>>> {
    let mut up = place.items;
    // ponytail: blocks nest deeper than items; past the limit the name is taken as a type
    // parameter with no bound, so no repository type is guessed.
    while let Some(Items {
        item,
        depth,
        up: next,
    }) = up
        && place.depth - depth <= PATH_DEPTH
    {
        up = next.as_deref();
        let declared = field(*item, "type_parameters").into_iter().flat_map(named);
        let mut declared = declared
            .filter(|p| p.kind() == "type_parameter")
            .filter(|p| field(*p, "name").is_some_and(|n| text(n, src) == name));
        let Some(declared) = declared.next() else {
            continue;
        };
        let mut bounds: Vec<Node> = (field(declared, "bounds").into_iter())
            .flat_map(named)
            .collect();
        let clause = named(*item).find(|c| c.kind() == "where_clause");
        for predicate in clause.into_iter().flat_map(named) {
            let left = field(predicate, "left");
            if left.is_some_and(|l| l.kind() == "type_identifier" && text(l, src) == name) {
                bounds.extend(field(predicate, "bounds").into_iter().flat_map(named));
            }
        }
        return Some(only_bound(&bounds, src));
    }
    (place.depth >= PATH_DEPTH).then_some(None)
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

/// The call a method call is made on (`Router::new()` in `Router::new().route(..)`), through
/// `.await` and parentheses. Not through `?`: it unwraps the written result (`Result<T>`,
/// `Option<T>`), so the receiver is not of that type and the call stays a name match.
// ponytail: `.await` takes the written result as the awaited value, which holds for an `async
// fn`; a plain `fn` that returns `impl Future<Output = T>` leaves the call unresolved.
fn inner_call(function: Node) -> Option<Node> {
    if function.kind() != "field_expression" {
        return None;
    }
    let mut at = field(function, "value")?;
    for _ in 0..PATH_DEPTH {
        match at.kind() {
            "call_expression" => return Some(at),
            "await_expression" | "parenthesized_expression" => at = at.named_child(0)?,
            _ => return None,
        }
    }
    None
}

/// The name a method call on a call chain starts at (`Call::root`): `client` for
/// `client.get(u).send()` and `client.get(u)?.send()`, `None` for a call on anything else.
fn chain_root(function: Node, src: &[u8]) -> Option<String> {
    if function.kind() != "field_expression" {
        return None;
    }
    let mut at = field(function, "value")?;
    if !matches!(
        at.kind(),
        "call_expression" | "try_expression" | "await_expression"
    ) {
        return None;
    }
    for _ in 0..PATH_DEPTH {
        at = match at.kind() {
            "try_expression" | "await_expression" | "parenthesized_expression" => {
                at.named_child(0)?
            }
            "call_expression" => match field(at, "function")? {
                f if f.kind() == "field_expression" => field(f, "value")?,
                _ => return None,
            },
            _ => break,
        };
    }
    let root = match at.kind() {
        "identifier" | "self" => text(at, src).to_string(),
        "field_expression" => member(at, src),
        _ => return None,
    };
    (!root.starts_with('(')).then_some(root)
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
    use std::time::{Duration, Instant};

    use super::*;
    use crate::lang::Value;

    pub(super) fn facts(src: &str) -> Facts {
        parse(src.as_bytes(), "").unwrap()
    }

    /// How many empty blocks the timing tests put side by side, and how many they nest.
    const DEPTH: usize = 2000;

    /// Times `run` on two files of the same nodes, `fn f() { BODY }`: one with `body` after
    /// `DEPTH` empty blocks side by side, one with `body` inside `DEPTH` nested blocks. It must
    /// take less than four times as long deep as at the top.
    pub(super) fn same_time_at_any_depth(
        body: &str,
        run: impl Fn(&str, &tree_sitter::Tree) -> Duration,
    ) {
        let top = format!("fn f() {{ {}{body} }}\n", "{} ".repeat(DEPTH));
        let deep = format!(
            "fn f() {{ {}{body}{} }}\n",
            "{ ".repeat(DEPTH),
            "} ".repeat(DEPTH)
        );
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language).unwrap();
        let [top, deep] = [top, deep].map(|src| {
            let tree = parser.parse(&src, None).unwrap();
            assert!(!tree.root_node().has_error());
            (src, tree)
        });
        // A parent or sibling lookup walks down from the root of the tree, so it costs the depth
        // of the node; each node must cost the same anywhere. The least of interleaved runs
        // keeps the machine's speed and load out of the ratio.
        let (mut at_top, mut at_depth) = (Duration::MAX, Duration::MAX);
        for _ in 0..5 {
            at_top = at_top.min(run(&top.0, &top.1));
            at_depth = at_depth.min(run(&deep.0, &deep.1));
        }
        assert!(
            at_depth < at_top * 4,
            "{at_top:?} at the top, {at_depth:?} {DEPTH} deep"
        );
    }

    fn callees(src: &str) -> Vec<(String, bool)> {
        let calls = facts(src).calls.into_iter();
        calls.map(|c| (c.callee, c.by_path)).collect()
    }

    #[test]
    fn a_value_nested_past_the_depth_limit_is_unknown() {
        let read = |n: usize| {
            let src = format!("const X: &str = {}\"a\"{};\n", "(".repeat(n), ")".repeat(n));
            let facts = facts(&src);
            let x = facts.consts.into_iter().find(|c| c.name == "X").unwrap();
            matches!(x.value, crate::lang::Value::Str(_))
        };
        assert!(read(values::VALUE_DEPTH - 1));
        assert!(!read(values::VALUE_DEPTH));
    }

    /// The first argument of each call to `g` in `src`: a string as its text (`<x>` for a name in
    /// it, `{}` for a hole), a name as `@x`, anything else as `?`.
    fn read(src: &str) -> Vec<String> {
        use crate::lang::{Part, Value};
        let show = |value: &Value| match value {
            Value::Str(parts) => (parts.iter())
                .map(|p| match p {
                    Part::Text(t) => t.clone(),
                    Part::Ref(r) => format!("<{r}>"),
                    Part::Env(e) => format!("env:{e}"),
                    Part::EnvOr(..) | Part::Read { .. } | Part::Hole => "{}".into(),
                })
                .collect::<String>(),
            Value::Ref(name) => format!("@{name}"),
            _ => "?".into(),
        };
        let calls = facts(src).calls.into_iter();
        let calls = calls.filter(|c| c.callee == "g");
        calls.map(|c| show(&c.args[0])).collect()
    }

    #[test]
    fn a_string_a_function_builds_holds_the_pushes_before_each_read() {
        let src = "fn f() {\n    let mut u = String::from(\"/a\");\n    g(&u);\n\
                   u.push_str(\"/b\");\n    u.push('/');\n    u += \"c\";\n    g(&u);\n\
                   g(format!(\"{u}!\"));\n    g(u);\n}\n";
        assert_eq!(read(src), ["/a", "/a/b/c", "/a/b/c!", "/a/b/c"]);
    }

    #[test]
    fn a_string_something_else_may_change_is_not_a_builder() {
        // A loop, a branch, a `&mut`, a macro that writes, another binding of the name, a
        // method that changes it, and a closure that shadows it.
        for body in [
            "for p in ps { u.push_str(p); }",
            "if c { u.push_str(\"/b\"); }",
            "h(&mut u);",
            "write!(u, \"/b\");",
            "let u = \"/x\";",
            "u.insert(0, 'x');",
            "u[..].make_ascii_uppercase();",
            "(*u).make_ascii_uppercase();",
            "u = String::new();",
        ] {
            let src = format!("fn f() {{ let mut u = String::from(\"/a\"); {body} g(&u); }}");
            assert_eq!(read(&src), ["@u"], "{body}");
        }
        // Two in one scope under one name: neither is told apart.
        let twice = "fn f(c: bool) { if c { let mut u = String::from(\"/a\"); g(&u); } \
                     else { let mut u = String::from(\"/b\"); g(&u); } }";
        assert_eq!(read(twice), ["@u", "@u"]);
    }

    #[test]
    fn a_string_that_doubles_or_chains_is_read_up_to_its_limits() {
        let doubling = (0..40).map(|_| "u.push_str(&u);").collect::<String>();
        let src = format!("fn f() {{ let mut u = String::from(\"a\"); {doubling} g(&u); }}");
        // Past the size limit what it holds is holes, not the text.
        let found = read(&src);
        assert!(
            found[0].starts_with("{}") && !found[0].contains('a'),
            "{found:?}"
        );
        // Each String pushes the one before it: past the depth limit the chain is a hole.
        let chain = |n: usize| {
            let mut src = String::from("fn f() { let mut s0 = String::from(\"a\");\n");
            for i in 1..=n {
                let before = i - 1;
                // A `+=` is no call, so no earlier read has folded the String it adds.
                src += &format!("let mut s{i} = String::new(); s{i} += &s{before};\n");
            }
            src + &format!("g(&s{n}); }}")
        };
        assert_eq!(read(&chain(builders::BUILDER_DEPTH - 2)), ["a"]);
        assert!(read(&chain(builders::BUILDER_DEPTH + 2))[0].contains("{}"));
    }

    #[test]
    fn a_push_the_configuration_leaves_out_is_not_in_the_string() {
        // The default configuration of a fixed Linux target: `windows` is false, `unix` true, a
        // comment between the attribute and the statement changes nothing.
        let src = "fn f() {\n let mut u = String::from(\"/a\");\n\
                   #[cfg(windows)]\n // not here\n u.push_str(\"/w\");\n\
                   #[cfg(feature = \"x\")]\n u.push_str(\"/x\");\n\
                   #[cfg(unix)]\n // here\n u.push_str(\"/u\");\n g(&u);\n}\n";
        assert_eq!(read(src), ["/a/u"]);
    }

    #[test]
    fn a_static_mut_holds_no_known_value() {
        let facts = facts("static mut X: &str = \"/a\";\nstatic Y: &str = \"/b\";\n");
        let names: Vec<_> = facts.consts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Y"]);
    }

    #[test]
    fn many_reads_of_a_long_builder_are_charged_to_the_work_budget() {
        // 400 pushes of 8 bytes: a read copies about 3600 units, so the 4M budget lasts about a
        // thousand of the 5000 reads, and the rest are holes. Walking every push for each read
        // would be 2M steps; a prefix is folded once.
        let mut src = String::from("fn f() {\n let mut u = String::from(\"a\");\n");
        for _ in 0..400 {
            src += " u.push_str(\"abcdefgh\");\n";
        }
        for _ in 0..5000 {
            src += " g(&u);\n";
        }
        let found = read(&(src + "}"));
        assert_eq!(found.len(), 5000);
        assert!(found[0].starts_with("aabcdefgh"));
        assert_eq!(found[4999], "{}");
    }

    #[test]
    fn a_format_past_its_limits_is_unknown() {
        let many = |n: usize| {
            format!(
                "format!(\"{}\", {})",
                "{}".repeat(n),
                vec!["\"a\""; n].join(", ")
            )
        };
        let args = values::FORMAT_ARGS;
        assert_eq!(
            read(&format!("fn f() {{ g({}); }}", many(args))),
            ["a".repeat(args)]
        );
        assert_eq!(read(&format!("fn f() {{ g({}); }}", many(args + 1))), ["?"]);
        // Captures of a builder: each copies its text, and the output is bounded.
        let captures = "{u}".repeat(values::FORMAT_PLACEHOLDERS + 1);
        let src =
            format!("fn f() {{ let mut u = String::from(\"a\"); g(format!(\"{captures}\")); }}");
        assert_eq!(read(&src), ["?"]);
        let big = "x".repeat(100);
        let src = format!(
            "fn f() {{ let mut u = String::from(\"{big}\"); g(format!(\"{}\")); }}",
            "{u}".repeat(60)
        );
        assert_eq!(read(&src), ["?"]);
        // Named arguments are looked up by name, however many.
        let keyed: Vec<String> = (0..args).map(|i| format!("k{i} = \"v\"")).collect();
        let template = (0..args).map(|i| format!("{{k{i}}}")).collect::<String>();
        let src = format!(
            "fn f() {{ g(format!(\"{template}\", {})); }}",
            keyed.join(", ")
        );
        assert_eq!(read(&src), ["v".repeat(args)]);
    }

    #[test]
    fn the_builders_of_a_file_scan_up_to_a_budget() {
        let n = 3000;
        let mut src = String::from("fn f() {\n");
        for i in 0..n {
            src += &format!("let mut s{i} = String::from(\"/{i}\"); g(&s{i});\n");
        }
        let found = read(&(src + "}"));
        assert_eq!(found.len(), n);
        assert_eq!(found[0], "/0");
        assert_eq!(found[n - 1], format!("@s{}", n - 1));
    }

    #[test]
    fn format_and_concat_fold_literals_and_plain_names() {
        for (call, expected) in [
            ("format!(\"/a/{}/{}\", \"b\", c)", "/a/b/<c>"),
            ("format!(\"/{0}/{1}/{0}\", \"x\", y)", "/x/<y>/x"),
            ("format!(\"/{p}/{q}\", q = \"z\", p = r)", "/<r>/z"),
            ("format!(\"/{p}\")", "/<p>"),
            ("format!(\"{{/{p}}}\")", "{/<p>}"),
            ("format!(\"/{}\", x.y)", "/{}"),
            ("format!(\"/{}\", h())", "/{}"),
            ("format!(\"/{:?}\", z)", "/{}"),
            ("format!(\"/{z:>8}\")", "/{}"),
            ("format!(\"/{}\", 5)", "/5"),
            ("format!(\"/{}\", 5u8)", "/{}"),
            ("format!(\"/{}\", true)", "/true"),
            ("format!(\"/{}\", \"a\",)", "/a"),
            ("std::format!(\"/{}\", \"a\")", "/a"),
            ("concat!(\"/a\", \"/b\", 7, y)", "/a/b7{}"),
            // Not a format string, a count that does not match, or a macro of the file.
            ("format!(\"/{}\")", "?"),
            ("format!(\"/{}/{}\", a)", "?"),
            ("format!(\"/{.*}\", 1, 2)", "?"),
            ("format!(\"/}\", 1)", "?"),
            ("format!(x, 1)", "?"),
            ("format!()", "?"),
            ("format!(\"/a\",, \"b\")", "?"),
            ("other!(\"/a\")", "?"),
        ] {
            let src = format!("fn f() {{ g({call}); }}");
            assert_eq!(read(&src), [expected], "{call}");
        }
        let own = "macro_rules! format { ($($t:tt)*) => { String::new() } }\n\
                   fn f() { g(format!(\"/a\")); }";
        assert_eq!(read(own), ["?"]);
    }

    #[test]
    fn a_string_conversion_keeps_the_text_and_a_sum_needs_a_string() {
        for (value, expected) in [
            ("\"/a\".to_string()", "/a"),
            ("String::from(\"/a\")", "/a"),
            ("String::new()", ""),
            ("A.to_string() + \"/b\"", "<A>/b"),
            ("\"/a\".to_owned() + \"/b\" + b", "/a/b<b>"),
            ("\"/a\".to_string() + h()", "/a{}"),
            ("a + b", "?"),
            ("A.into()", "?"),
            ("\"/a\".replace(\"a\", \"b\")", "?"),
        ] {
            let src = format!("fn f() {{ g({value}); }}");
            assert_eq!(read(&src), [expected], "{value}");
        }
    }

    #[test]
    fn a_method_constant_is_text_only_from_an_http_crate() {
        let name = |head: &str, call: &str| {
            let src = format!("{head}\nfn f() {{ g({call}); }}");
            read(&src)
        };
        assert_eq!(name("use reqwest::Method;", "Method::PUT"), ["PUT"]);
        assert_eq!(name("use http::Method;", "Method::PUT"), ["PUT"]);
        assert_eq!(name("", "reqwest::Method::DELETE"), ["DELETE"]);
        // The file's own `Method`, another crate's, and a name that is no verb.
        for (head, call) in [
            ("enum Method { PUT }", "Method::PUT"),
            ("use mine::Method;", "Method::PUT"),
            ("use reqwest::Method;", "Method::PURGE"),
            ("use reqwest::Method;\nuse mine::Method;", "Method::PUT"),
        ] {
            let found = name(head, call);
            assert!(found[0].starts_with('@'), "{head} {call}: {found:?}");
        }
    }

    #[test]
    fn a_let_bound_once_and_never_assigned_is_a_constant() {
        let src = "fn f(c: bool) {\n    let once = \"/a\";\n    let mut mutable = \"/b\";\n\
                   let mut again = \"/c\";\n    again = \"/d\";\n    let plain = \"/e\";\n\
                   let twice = \"/f\";\n    if c { twice = \"/g\"; }\n}\n";
        let mut found: Vec<String> = facts(src).consts.into_iter().map(|c| c.name).collect();
        found.sort();
        assert_eq!(found, ["f.once", "f.plain"]);
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
        let found = Rust.layout(&configs, &files).unwrap().programs;
        assert_eq!(found, [("src/main.rs".to_string(), "main".to_string())]);
    }

    #[test]
    fn a_main_in_an_inline_module_is_no_program() {
        let inline = facts("mod x { pub fn main() {} }\n");
        let files = BTreeMap::from([("src/main.rs", &inline)]);
        let configs: [(&str, &[u8]); 1] = [("Cargo.toml", b"[package]\nname = \"p\"\n")];
        assert!(Rust.layout(&configs, &files).unwrap().programs.is_empty());
    }

    #[test]
    fn a_false_cfg_leaves_the_item_and_a_declared_file_out() {
        let src = "#[cfg(windows)]\nfn a() {}\n#[cfg(unix)]\n#[inline]\nfn b() {}\n\
                   #[cfg(feature = \"x\")]\nmod m;\n#[cfg_attr(windows, inline)]\nfn c() {}\n\
                   struct S;\nimpl S {\n#[cfg(not(test))]\nfn d() {}\nfn e() {}\n}\n\
                   #[cfg(test)]\nmod t { fn f() {} }\n";
        let names = |setting: &str| -> Vec<String> {
            let facts = parse(src.as_bytes(), setting).unwrap();
            facts.defs.into_iter().map(|d| d.name).collect()
        };
        assert_eq!(names(""), ["b", "c", "S", "S.d", "S.e", "t", "t.f"]);
        assert_eq!(names("x"), ["b", "m", "c", "S", "S.d", "S.e", "t", "t.f"]);
    }

    #[test]
    fn a_test_build_keeps_its_modules_and_nothing_else() {
        let src = "#[cfg(test)]\nuse crate::mock::client;\n#[cfg(test)]\nfn helper() {}\n\
                   #[cfg(test)]\nmod t { fn f() {} }\n#[cfg(all(test, unix))]\nmod u {}\n\
                   #[cfg(any(test, feature = \"p\"))]\nmod v {}\n#[cfg(any(test, feature = \"p\"))]\nfn g() {}\n\
                   #[cfg(all(test, feature = \"p\"))]\nmod w {}\n#[cfg(not(test))]\nmod x {}\n";
        let facts = facts(src);
        let names: Vec<&str> = facts.defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["t", "t.f", "u", "v", "x"]);
        assert!(facts.imports.iter().all(|i| i.local != "client"));
    }

    #[test]
    fn an_inner_cfg_that_is_false_leaves_out_the_file_or_the_module() {
        let file = facts("#![cfg(windows)]\nfn a() {}\n");
        assert!(file.unbuilt && file.defs.is_empty());
        let file = facts("// note\n#![cfg(unix)]\nfn a() {}\n");
        assert!(!file.unbuilt && file.defs.len() == 1);
        // A file is a module: one a test build compiles stays.
        let file = facts("#![cfg(any(test, feature = \"p\"))]\nfn a() {}\n");
        assert!(!file.unbuilt);
        let src = "mod m {\n#![cfg(windows)]\nfn a() {}\n}\nmod n {\n#![cfg(unix)]\nfn b() {}\n}\n";
        let names: Vec<String> = facts(src).defs.into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["n", "n.b"]);
        // The outer and the inner cfg are judged in the same build: no build compiles both.
        let src = "#[cfg(any(test, feature = \"p\"))]\nmod m {\n#![cfg(not(test))]\nfn a() {}\n}\n";
        assert!(facts(src).defs.is_empty());
        let src = "#[cfg(any(test, unix))]\nmod m {\n#![cfg(not(test))]\nfn a() {}\n}\n";
        assert_eq!(facts(src).defs.len(), 2);
    }

    #[test]
    fn a_flat_file_and_a_run_of_attributes_parse_in_linear_time() {
        let mut src = String::new();
        for n in 0..30_000 {
            src += &format!("#[cfg(unix)]\nfn f{n}() {{}}\n");
        }
        src += &"#[a]\n".repeat(30_000);
        src += "#[cfg(windows)]\nfn last() {}\n";
        let start = std::time::Instant::now();
        let facts = facts(&src);
        assert_eq!(facts.defs.len(), 30_000);
        // Quadratic sibling lookups took minutes here.
        assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
    }

    #[test]
    fn a_file_takes_the_default_features_of_the_nearest_package() {
        let configs: [(&str, &[u8]); 2] = [
            (
                "Cargo.toml",
                b"[package]\nname = \"p\"\n[features]\ndefault = [\"a\"]\n",
            ),
            (
                "sub/Cargo.toml",
                b"[package]\nname = \"q\"\n[features]\ndefault = [\"b\"]\n",
            ),
        ];
        let setting = Rust.settings(&configs);
        assert_eq!(setting("src/lib.rs"), "a,default");
        assert_eq!(setting("sub/src/lib.rs"), "b,default");
        assert_eq!(setting("subway/x.rs"), "a,default");
        assert_eq!(setting("elsewhere/x.rs"), "a,default");
    }

    #[test]
    fn a_feature_with_a_control_character_is_no_setting() {
        let manifest = "[package]\nname = \"p\"\n[features]\ndefault = [\"a\", \"b\\nc\"]\na = []\n\"b\\nc\" = []\n";
        let configs: [(&str, &[u8]); 1] = [("Cargo.toml", manifest.as_bytes())];
        // A line break would end the setting's first line (`lang::setting`).
        assert_eq!(Rust.settings(&configs)("src/lib.rs"), "a,default");
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
    fn a_function_does_not_hide_the_crate_its_path_begins_with() {
        // `rocket::build()` names the crate in a file that has a `fn rocket`; `rocket()` does not.
        let src = "fn rocket() { rocket::build(); }\n";
        assert_eq!(facts(src).externs, ["rocket"]);
        let src = "fn rocket() {}\nfn f() { rocket(); }\n";
        assert!(facts(src).externs.is_empty());
    }

    #[test]
    fn route_attributes_decorate_their_function_up_to_the_limit() {
        let attrs = "#[get(\"/x\")]\n".repeat(ROUTES_PER_FUNCTION + 5);
        let src = format!("{attrs}#[derive(Debug)]\nfn h() {{}}\n");
        let calls = facts(&src).calls;
        let decorating = calls.iter().filter(|c| c.decorates.as_deref() == Some("h"));
        assert_eq!(decorating.count(), ROUTES_PER_FUNCTION);
    }

    /// The value the second argument of the `mount` call of `src` holds.
    fn mounted(src: &str) -> Value {
        let calls = facts(src).calls;
        let call = calls.into_iter().find(|c| c.callee.ends_with("mount"));
        call.unwrap().args.into_iter().nth(1).unwrap()
    }

    #[test]
    fn routes_and_methods_past_their_limits_are_not_read() {
        let routes = |n: usize| {
            let items: Vec<String> = (0..n).map(|i| format!("h{i}")).collect();
            format!(
                "fn f(b: B) {{ b.mount(\"/\", routes![{}]); }}\n",
                items.join(", ")
            )
        };
        assert!(matches!(
            mounted(&routes(values::ROUTE_ITEMS)),
            Value::List(_)
        ));
        assert!(matches!(
            mounted(&routes(values::ROUTE_ITEMS + 1)),
            Value::Unknown
        ));
        let methods = |n: usize| {
            let given = vec!["method = \"GET\""; n].join(", ");
            let src = format!("#[route(\"/x\", {given})]\nfn h() {{}}\n");
            let calls = facts(&src).calls;
            let unread = calls[0]
                .kwargs
                .iter()
                .filter(|(_, v)| matches!(v, Value::Unknown));
            unread.count()
        };
        assert_eq!(methods(values::ROUTE_METHODS), 0);
        assert_eq!(methods(values::ROUTE_METHODS + 3), 3);
    }

    #[test]
    fn a_macro_use_extern_crate_is_declared_only_when_marked() {
        let src = "#[macro_use]\nextern crate rocket;\nextern crate other;\n";
        assert_eq!(facts(src).macro_use, ["rocket"]);
        let src = "#[macro_use] // here\nextern crate rocket;\nuse x::y;\n#[macro_use]\nmod m;\nextern crate z;\n";
        assert_eq!(facts(src).macro_use, ["rocket"]);
    }

    #[test]
    fn a_module_hides_the_crate_of_its_name_whatever_function_shares_it() {
        let src = "fn store() {}\nmod store { pub fn get() {} }\nfn f() { store::get(); }\n";
        assert!(facts(src).externs.is_empty());
        let src = "mod store { pub fn get() {} }\nfn store() {}\nfn f() { store::get(); }\n";
        assert!(facts(src).externs.is_empty());
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

    /// The receivers as `scope name = class`, sorted.
    fn receivers(src: &str) -> Vec<String> {
        let mut found: Vec<String> = (facts(src).receivers.into_iter())
            .map(|r| {
                let class = match r.class {
                    Class::Enclosing => "Self".to_string(),
                    Class::Named(n) | Class::Type(n) | Class::Member(n) => n,
                    Class::Returned(n) | Class::Called(n) => format!("{n}()"),
                    Class::Result(n) => format!("#{n}()"),
                    Class::Imported => "imported".into(),
                    Class::Unknown => "?".into(),
                };
                format!("{} {} = {class}", r.scope, r.name)
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn written_types_make_receivers() {
        let src = "struct S { a: A, b: (A, B), c: &'a mut m::C }\n\
                   impl S { fn f(&self, x: &mut X, (y, z): (Y, Z), n: u8) -> Self { \
                   let w: W = make(); let v = Y::new(); let u = self.g(); let t = z(); } }\n\
                   trait T { fn r(&self) -> impl Q; }\n";
        let expected = [
            "S a = A",
            "S c = m.C",
            "S.f n = u8",
            "S.f self = Self",
            "S.f t = z()",
            "S.f v = Y.new()",
            "S.f w = W",
            "S.f x = X",
            "T.r self = Self",
        ];
        assert_eq!(receivers(src), expected);
        let results: Vec<String> = (facts(src).defs.into_iter())
            .filter_map(|d| match d.result? {
                Class::Type(n) => Some(format!("{} -> {n}", d.name)),
                _ => None,
            })
            .collect();
        assert_eq!(results, ["S.f -> Self", "T.r -> Q"]);
    }

    #[test]
    fn type_parameters_name_their_one_bound_and_hide_a_type_of_their_name() {
        let src = "struct A;\n\
                   fn a<A: P>(x: &A) {}\n\
                   fn b<T: P + Q>(x: T) {}\n\
                   fn c<T: ?Sized + 'a + P>(x: &T) {}\n\
                   fn d<T>(x: T) where T: P {}\n\
                   fn e<T: P>(x: T::Item) {}\n\
                   fn f(x: &dyn m::P, y: impl P + Send) {}\n\
                   struct W<T: P> { inner: T }\n\
                   impl<T: P> W<T> { fn g(&self, t: T) {} }\n\
                   fn h<T>(x: T) {}\n\
                   fn i<T: P + Send + 'a>(x: &T, y: &(dyn P + Sync)) {}\n\
                   fn j(x: impl P + Send + 'static) {}\n\
                   fn k<T: P>(x: T) -> T { x }\n\
                   fn l(x: &[u8]) { let x: Y = make(); let z: Z = make(); }\n";
        // Several bounds, none and a member of a parameter name no type; marker traits and
        // lifetimes are no bound; a `let` of a parameter's name is a local, not a receiver.
        let expected = [
            "W inner = P",
            "W.g self = Self",
            "W.g t = P",
            "a x = P",
            "c x = P",
            "d x = P",
            "f x = m.P",
            "f y = P",
            "i x = P",
            "i y = P",
            "j x = P",
            "k x = P",
            "l z = Z",
        ];
        assert_eq!(receivers(src), expected);
        let results = (facts(src).defs.into_iter()).filter(|d| d.result.is_some());
        assert_eq!(results.count(), 0, "a type parameter is no result class");
        assert_eq!(facts(src).locals, ["l.x"]);
    }

    #[test]
    fn a_parameter_declared_past_the_limit_is_no_type() {
        let blocks = "{ ".repeat(PATH_DEPTH + 8);
        let src = format!(
            "struct T;\nfn f<T: P>() {{ {blocks} let x: T = y(); {} }}\n",
            "}".repeat(PATH_DEPTH + 8)
        );
        // The walk gives up before it finds the declaration: `T` is no type, so no receiver.
        assert!(receivers(&src).is_empty());
    }

    #[test]
    fn a_parameter_is_declared_for_the_types_up_to_the_limit_below_it() {
        // Each nested block is two nodes (an expression statement and a block). The type of a
        // `let` in `k` of them is `3 + 2k` levels below the function that declares `T`, one more
        // behind a `&`; the type of a nested function's parameter or a nested struct's field is
        // `5 + 2k`, and one more.
        let found = |head: &str, inner: &str, k: usize| {
            let (open, close) = ("{ ".repeat(k), "}".repeat(k));
            receivers(&format!("{head} {{ {open}{inner}{close} }}\n"))
        };
        let cases = [
            ("let x: T = y();", 3, "f x"),
            ("let x: &T = y();", 4, "f x"),
            ("fn g(x: T) {}", 5, "f.g x"),
            ("fn g(x: &T) {}", 6, "f.g x"),
            ("struct S { a: T }", 5, "f.S a"),
        ];
        for (inner, levels, receiver) in cases {
            let last = (PATH_DEPTH - levels) / 2;
            let class = [format!("{receiver} = P")];
            assert_eq!(found("fn f<T: P>()", inner, last), class, "{inner}");
            assert!(found("fn f<T: P>()", inner, last + 1).is_empty(), "{inner}");
        }
        // A name nothing declares is a type, until the type is `PATH_DEPTH` levels below the
        // root of the tree: there the walk gives up and the name is no type.
        for (inner, depth) in [("let x: U = y();", 4), ("let x: &U = y();", 5)] {
            let last = (PATH_DEPTH - 1 - depth) / 2;
            assert_eq!(found("fn f()", inner, last), ["f x = U"], "{inner}");
            assert!(found("fn f()", inner, last + 1).is_empty(), "{inner}");
        }
        // So is the result of a nested function.
        let result = |k: usize| {
            let (open, close) = ("{ ".repeat(k), "}".repeat(k));
            let src = format!("fn f() {{ {open}fn g() -> U {{ y() }}{close} }}\n");
            let defs = facts(&src).defs.into_iter();
            defs.filter(|d| d.name == "f.g").find_map(|d| d.result)
        };
        let last = (PATH_DEPTH - 1 - 4) / 2;
        assert!(matches!(result(last), Some(Class::Type(u)) if u == "U"));
        assert!(result(last + 1).is_none());
    }

    #[test]
    fn a_long_chain_of_items_is_dropped_without_recursion() {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language).unwrap();
        let tree = parser.parse("fn f() {}", None).unwrap();
        let mut items = None;
        for depth in 0..1_000_000 {
            let item = tree.root_node();
            items = Some(Rc::new(Items {
                item,
                depth,
                up: items,
            }));
        }
        drop(items);
    }

    #[test]
    fn a_type_parameter_is_read_in_the_same_time_at_any_depth() {
        // Looking for the declaration of the `T` of each `let`, the walk went up the tree, a
        // lookup per level, up to `PATH_DEPTH` of them.
        same_time_at_any_depth(&"let x: T = y();\n".repeat(50), |src, _| {
            let start = Instant::now();
            facts(src);
            start.elapsed()
        });
    }

    #[test]
    fn the_route_attributes_of_a_function_are_read_in_the_same_time_at_any_depth() {
        // Reading the attributes before each function, the walk looked up a sibling per attribute.
        let functions =
            (0..300).map(|i| format!("#[get(\"/a\")] #[post(\"/b\")] fn h{i}() {{}}\n"));
        same_time_at_any_depth(&functions.collect::<String>(), |src, _| {
            let start = Instant::now();
            let calls = facts(src).calls;
            let took = start.elapsed();
            assert_eq!(calls.iter().filter(|c| c.decorates.is_some()).count(), 600);
            took
        });
    }
}
