//! The TypeScript and JavaScript language pack.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use tree_sitter::{Language, Node, Parser};

use super::{
    Binds, Call, Const, Def, Export, Facts, Field, Import, Imported, Invocation, Kind,
    LanguagePack, Modules, Param, Part, Ty, Value, children, named, text,
};
use crate::{Result, package, qualify};

mod types;
mod uses;

/// How many definitions may nest in one file. Names are qualified by the definitions around
/// them and each call resolves its names up that chain, so unbounded nesting costs cubic time.
/// Past the limit a function is not a definition (its body belongs to the one around it, and
/// its name hides outer ones as a local).
// ponytail: real code nests a few dozen levels; 128 is the depth `document/schema.rs` reads
// (`MAX_DEPTH`).
const DEFINITION_DEPTH: usize = 128;

/// How many levels of arguments, objects, arrays and receivers of method chains `value` reads;
/// deeper is `Unknown`. A value is read again by every call or `new` around it, and the
/// recursion must stay shallow.
// ponytail: real code nests a few levels of options; 64 is far past any.
const VALUE_DEPTH: usize = 64;

/// How deep `parts`, `env_read` and `callee` follow a `+` chain, a chain of string method
/// calls or a member chain; the cut side is a hole (`parts`) or its node kind (`callee`).
// ponytail: real code concatenates and chains a few dozen terms; the recursion must stay
// shallow, whatever the file.
const FOLD_DEPTH: usize = 256;

pub struct TsJs;

impl LanguagePack for TsJs {
    fn extensions(&self) -> &'static [&'static str] {
        &EXTENSIONS
    }

    fn parse(&self, path: &str, src: &[u8]) -> Result<Facts> {
        let language: Language = if path.ends_with(".ts") {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        } else if path.ends_with(".tsx") {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            tree_sitter_javascript::LANGUAGE.into()
        };
        let mut parser = Parser::new();
        parser.set_language(&language)?;
        let tree = parser
            .parse(src, None)
            .ok_or("tree-sitter returned no tree")?;
        let parents = Parents::new(tree.root_node(), src);
        let mut facts = Facts::default();
        let mut uses = uses::Uses::new(src, &parents);
        // Explicit stack instead of recursion: generated code can nest deeper than the call
        // stack.
        let mut stack = vec![Visit {
            node: tree.root_node(),
            scope: String::new(),
            depth: 0,
            body: tree.root_node(),
            binding: None,
        }];
        while let Some(Visit {
            node,
            scope,
            depth,
            body,
            binding,
        }) = stack.pop()
        {
            let mut inner = scope.clone();
            let mut defined = definition(node, binding, src, &parents);
            if depth >= DEFINITION_DEPTH
                && let Some((name, kind, _)) = defined.take()
                && matches!(kind, Kind::Function | Kind::Class)
            {
                // Not a definition, but its name still hides the outer one, as a `let` does.
                facts.locals.push(qualify(&scope, &name));
            }
            let is_def = defined.is_some();
            if let Some((name, kind, range)) = defined {
                inner = qualify(&scope, &name);
                facts.defs.push(Def {
                    params: params(node, src, &inner, &parents),
                    returns: (node.child_by_field_name("return_type"))
                        .and_then(|t| types::returned(t, src, &parents)),
                    type_params: types::type_parameters(node, src),
                    result: None,
                    suspends: false,
                    hash: String::new(),
                    name: inner.clone(),
                    kind,
                    range,
                    bases: Vec::new(),
                    decorators: Vec::new(),
                    return_values: Vec::new(),
                });
            }
            match node.kind() {
                "call_expression" | "new_expression" => {
                    // `new X(...)` is a call of `X`: constructors are how some libraries are
                    // used (`new SendMessageCommand(...)`, `new Worker(...)`).
                    let arguments =
                        (node.child_by_field_name("arguments")).filter(|a| a.kind() == "arguments");
                    if let Some(function) = callee_node(node) {
                        // The artifact's `await` is outside the call: so is its span.
                        let start = if awaited_artifact(node) {
                            function.start_byte()
                        } else {
                            node.start_byte()
                        };
                        facts.calls.push(Call {
                            arity: None,
                            labels: None,
                            awaited: false,
                            constructs: false,
                            scope: scope.clone(),
                            callee: callee(function, src),
                            range: start..node.end_byte(),
                            args: arguments
                                .map(|a| {
                                    named(a)
                                        .map(|arg| value(arg, src, &scope, &parents))
                                        .collect()
                                })
                                .unwrap_or_default(),
                            kwargs: Vec::new(),
                            decorates: None,
                            by_path: false,
                            root: None,
                            on: None,
                            receiver: None,
                            arg_types: arguments
                                .map(|a| named(a).map(|arg| cast(arg, src, &parents)).collect())
                                .unwrap_or_default(),
                            uses: uses.of(node, body),
                        });
                    }
                }
                "interface_declaration" | "type_alias_declaration" => {
                    if let Some(name) = node.child_by_field_name("name") {
                        facts.consts.push(Const {
                            name: qualify(&scope, text(name, src)),
                            value: Value::Type(types::declaration(node, src, &parents, &mut 0)),
                            ty: None,
                            scope: None,
                        });
                    }
                }
                "import_statement" => imports(node, src, &mut facts.imports),
                "export_statement" => exports(node, src, &mut facts.exports),
                _ => {}
            }
            let named = binds(node, src, &parents);
            if node.kind() == "assignment_expression"
                && named.is_some()
                && let Some(Assigned {
                    name,
                    exported: true,
                    ..
                }) = assignment(node, src, &parents)
            {
                facts.exports.push(Export::Local {
                    exported: name.clone(),
                    local: name,
                });
            }
            let pattern = match node.kind() {
                kind if BODIES.contains(&kind) => node
                    .child_by_field_name("parameters")
                    .or_else(|| node.child_by_field_name("parameter")),
                "catch_clause" => node.child_by_field_name("parameter"),
                "for_in_statement" => node
                    .child_by_field_name("kind")
                    .and(node.child_by_field_name("left")),
                // A `const` holding one name is a constant or a definition.
                "variable_declarator" if named.is_none() => node.child_by_field_name("name"),
                _ => None,
            };
            // A definition's parameters are `Def::params`; only the names they leave out (a
            // nested, array or rest pattern) are locals.
            let params = (facts.defs.last())
                .filter(|_| is_def)
                .map_or(&[][..], |d| d.params.as_slice());
            let kept = |name: &str| {
                (params.iter()).any(|p| p.name == name || p.fields.iter().any(|f| f.name == name))
            };
            for name in pattern.map(|p| bound(p, src)).unwrap_or_default() {
                if !(BODIES.contains(&node.kind()) && kept(&name)) {
                    facts.locals.push(qualify(&inner, &name));
                }
            }
            if node.kind() == "variable_declarator"
                && let Some((value_node, name, _)) = &named
                && !FUNCTIONS.contains(&value_node.kind())
                && !matches!(value_node.kind(), "class")
            {
                let (value, ty) = declared(
                    node.child_by_field_name("type"),
                    *value_node,
                    src,
                    &scope,
                    &parents,
                );
                facts.consts.push(Const {
                    name: qualify(&scope, name),
                    value,
                    ty,
                    scope: None,
                });
            }
            let children: Vec<Node> = node.children(&mut node.walk()).collect();
            stack.extend(children.into_iter().rev().map(|child| {
                Visit {
                    binding: named
                        .as_ref()
                        .filter(|(value, _, _)| value.id() == child.id())
                        .map(|(_, name, range)| (name.clone(), range.clone())),
                    node: child,
                    scope: inner.clone(),
                    depth: depth + usize::from(is_def),
                    body: if BODIES.contains(&node.kind()) {
                        node
                    } else {
                        body
                    },
                }
            }));
        }
        Ok(facts)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["**/tsconfig.json", "**/jsconfig.json"]
    }

    fn receiver(&self) -> Option<&'static str> {
        Some("this")
    }

    fn base_receiver(&self) -> Option<&'static str> {
        Some("super")
    }

    fn assigned_members(&self) -> bool {
        true
    }

    fn globals(&self) -> &'static [&'static str] {
        &["fetch"]
    }

    fn modules(&self, configs: &[(&str, &[u8])], _: &BTreeMap<&str, &Facts>) -> Result<Modules> {
        // Read as `package::read_tsconfig` reads one: a config past the size limit, not UTF-8,
        // not JSON or not an object declares no paths, and indexing carries on.
        // The nearest config above an importer serves it; in one directory `tsconfig.json`
        // comes first in `configs`, so it wins. A config under `node_modules` declares nothing.
        let mut by_dir: HashMap<String, Aliases> = HashMap::new();
        for (path, src) in configs {
            let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
            if !path.split('/').any(|s| s == "node_modules") {
                by_dir
                    .entry(dir.to_string())
                    .or_insert_with(|| Aliases::parse(dir, src));
            }
        }
        Ok(Box::new(move |importer, specifier| {
            let dir = importer.rsplit_once('/').map_or("", |(dir, _)| dir);
            let bases: Vec<String> = if specifier.starts_with('.') {
                join(dir, specifier).into_iter().collect()
            } else {
                let mut up = Some(dir);
                let mut found = None;
                while let Some(d) = up {
                    if let Some(aliases) = by_dir.get(d) {
                        found = Some(aliases);
                        break;
                    }
                    up = d
                        .rsplit_once('/')
                        .map_or((!d.is_empty()).then_some(""), |(p, _)| Some(p));
                }
                found.map_or_else(Vec::new, |aliases| aliases.expand(specifier))
            };
            bases
                .iter()
                .flat_map(|base| with_extensions(base))
                .collect()
        }))
    }
}

/// `compilerOptions.baseUrl` and `paths` of a `tsconfig.json` (or `jsconfig.json`) in `dir`.
// ponytail: `extends` and project `references` are not followed.
#[derive(Default)]
struct Aliases {
    dir: String,
    base_url: Option<String>,
    paths: Vec<(String, Vec<String>)>,
}

impl Aliases {
    fn parse(dir: &str, src: &[u8]) -> Self {
        let empty = Self {
            dir: dir.to_string(),
            ..Self::default()
        };
        let Some(config) = package::tsconfig_root(src) else {
            return empty;
        };
        let options = &config["compilerOptions"];
        let paths = options["paths"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(pattern, targets)| {
                let targets = targets.as_array().into_iter().flatten();
                let targets = targets.filter_map(|t| t.as_str().map(String::from));
                (pattern.clone(), targets.collect())
            })
            .collect();
        Self {
            base_url: options["baseUrl"].as_str().and_then(|b| join(dir, b)),
            paths,
            ..empty
        }
    }

    /// The paths a bare specifier names: the targets of the `paths` pattern with the
    /// longest prefix that matches, then the specifier under `baseUrl`. Without `baseUrl`,
    /// targets are relative to the config file's directory.
    fn expand(&self, specifier: &str) -> Vec<String> {
        let mut best: Option<(usize, &str, &[String])> = None;
        for (pattern, targets) in &self.paths {
            let matched = match pattern.split_once('*') {
                Some((prefix, suffix)) => specifier
                    .strip_prefix(prefix)
                    .and_then(|rest| rest.strip_suffix(suffix))
                    .map(|star| (prefix.len(), star)),
                None => (specifier == pattern).then_some((usize::MAX, "")),
            };
            if let Some((rank, star)) = matched
                && best.is_none_or(|(top, _, _)| rank > top)
            {
                best = Some((rank, star, targets));
            }
        }
        let root = self.base_url.as_deref().unwrap_or(&self.dir);
        let mut out: Vec<String> = best
            .into_iter()
            .flat_map(|(_, star, targets)| {
                targets
                    .iter()
                    .filter_map(move |t| join(root, &t.replacen('*', star, 1)))
            })
            .collect();
        out.extend(self.base_url.as_deref().and_then(|b| join(b, specifier)));
        out
    }
}

/// JSON with comments and trailing commas, as `tsconfig.json` allows, made plain JSON.
pub(crate) fn strip_jsonc(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                while let Some(c) = chars.next() {
                    out.push(c);
                    if c == '\\' {
                        out.extend(chars.next());
                    } else if c == '"' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => while chars.next_if(|&c| c != '\n').is_some() {},
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for c in chars.by_ref() {
                    if previous == '*' && c == '/' {
                        break;
                    }
                    previous = c;
                }
            }
            ',' if matches!(chars.clone().find(|c| !c.is_whitespace()), Some('}' | ']')) => {}
            c => out.push(c),
        }
    }
    out
}

pub(crate) const EXTENSIONS: [&str; 6] = ["ts", "tsx", "js", "jsx", "mjs", "cjs"];

/// The files a module path may name: itself, its `.ts` source when it names `.js` output,
/// itself with each extension, and its `index` file.
pub(crate) fn with_extensions(base: &str) -> Vec<String> {
    let mut out = vec![base.to_string()];
    if let Some(stem) = base.strip_suffix(".js") {
        out.extend([format!("{stem}.ts"), format!("{stem}.tsx")]);
    } else if let Some(stem) = base.strip_suffix(".jsx") {
        out.push(format!("{stem}.tsx"));
    }
    out.extend(EXTENSIONS.iter().map(|ext| format!("{base}.{ext}")));
    let dir = if base.is_empty() {
        String::new()
    } else {
        format!("{base}/")
    };
    out.extend(EXTENSIONS.iter().map(|ext| format!("{dir}index.{ext}")));
    out
}

/// `path` resolved against the repository-relative directory `dir`; `None` when it leaves
/// the repository.
fn join(dir: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

struct Visit<'t> {
    node: Node<'t>,
    scope: String,
    /// How many definitions enclose `node`.
    depth: usize,
    /// The function whose body holds `node`, or the root at the top level.
    body: Node<'t>,
    /// The name and span a binding gives an anonymous function or class: `const f = () => {}`,
    /// `export default () => {}`.
    binding: Option<(String, Range<usize>)>,
}

/// The definition `node` makes, named by itself or by the binding that holds it.
fn definition(
    node: Node,
    binding: Option<(String, Range<usize>)>,
    src: &[u8],
    up: &Parents,
) -> Option<(String, Kind, Range<usize>)> {
    let own = || {
        let name = node.child_by_field_name("name")?;
        Some((text(name, src).to_string(), node.byte_range()))
    };
    let (kind, (name, range)) = match node.kind() {
        "function_declaration" | "generator_function_declaration" => (Kind::Function, own()?),
        "class_declaration" | "abstract_class_declaration" => (Kind::Class, own()?),
        // Methods of object literals are definitions only in a class body, in an options
        // object (written in place like an argument) and in a top-level `const` (below).
        "method_definition" if up.of(node)?.kind() == "class_body" => (Kind::Method, own()?),
        "method_definition" if in_options(node, up) => {
            (Kind::Inline, (inline(node), node.byte_range()))
        }
        // A method of an object literal bound by a top-level `const` is `o.m`.
        "method_definition" => {
            let name = node.child_by_field_name("name")?;
            let owner =
                literal_owner(node, src, up).filter(|_| name.kind() == "property_identifier")?;
            (
                Kind::Function,
                (format!("{owner}.{}", text(name, src)), node.byte_range()),
            )
        }
        kind if FUNCTIONS.contains(&kind) => match binding {
            Some(binding) => {
                let method = (up.of(node))
                    .and_then(|p| assignment(p, src, up))
                    .is_some_and(|a| a.method);
                (if method { Kind::Method } else { Kind::Function }, binding)
            }
            None if written_in_place(node, up) => (Kind::Inline, (inline(node), node.byte_range())),
            None => return None,
        },
        "class" => (Kind::Class, binding?),
        _ => return None,
    };
    Some((name, kind, range))
}

/// The parent of every node, read once: `Node::parent` walks down from the root, so asking it
/// for each function in deeply nested code costs time quadratic in the file. It also holds the
/// names the file assigns after declaring them (`f = other`, `[f] = list`, `for (f in o)`).
struct Parents<'t> {
    up: HashMap<usize, Node<'t>>,
    assigned: HashSet<String>,
}

impl<'t> Parents<'t> {
    fn new(root: Node<'t>, src: &[u8]) -> Self {
        let mut up = HashMap::new();
        let mut assigned = HashSet::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let target = match node.kind() {
                "assignment_expression" | "augmented_assignment_expression" => {
                    node.child_by_field_name("left")
                }
                "for_in_statement" => node
                    .child_by_field_name("kind")
                    .is_none()
                    .then(|| node.child_by_field_name("left"))
                    .flatten(),
                _ => None,
            };
            assigned.extend(target.map(|t| bound(t, src)).unwrap_or_default());
            for child in children(node) {
                up.insert(child.id(), node);
                stack.push(child);
            }
        }
        Self { up, assigned }
    }

    fn of(&self, node: Node) -> Option<Node<'t>> {
        self.up.get(&node.id()).copied()
    }
}

/// The kinds of anonymous function expressions.
const FUNCTIONS: &[&str] = &[
    "arrow_function",
    "function_expression",
    "function",
    "generator_function",
];

/// The kinds of functions with a body, whose parameters bind names; a signature's do not.
const BODIES: &[&str] = &[
    "function_declaration",
    "generator_function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
    "function",
    "generator_function",
];

/// The names a binding pattern binds: an identifier, or each name an object, array, rest or
/// default pattern (or a parameter list) holds, never a key or a default value.
fn bound(pattern: Node, src: &[u8]) -> Vec<String> {
    (bound_nodes(pattern).into_iter())
        .map(|n| text(n, src).to_string())
        .collect()
}

/// The identifier nodes `bound` reads the names of.
fn bound_nodes(pattern: Node) -> Vec<Node> {
    let mut out = Vec::new();
    // Explicit stack: a pattern can nest deeper than the call stack.
    let mut stack = vec![pattern];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => out.push(node),
            "pair_pattern" => stack.extend(node.child_by_field_name("value")),
            "assignment_pattern" | "object_assignment_pattern" => {
                stack.extend(node.child_by_field_name("left"))
            }
            "required_parameter" | "optional_parameter" => {
                stack.extend(node.child_by_field_name("pattern"))
            }
            "formal_parameters" | "object_pattern" | "array_pattern" | "rest_pattern" => {
                stack.extend(named(node))
            }
            _ => {}
        }
    }
    out
}

/// Whether the anonymous function `node` is written in place as an argument, or as a property
/// of an options object given as one: `run({ eachMessage: async () => {} })`.
fn written_in_place(node: Node, up: &Parents) -> bool {
    match up.of(node) {
        Some(p) if p.kind() == "arguments" => true,
        Some(p) if p.kind() == "pair" => {
            p.child_by_field_name("value")
                .is_some_and(|v| v.id() == node.id())
                && in_options(p, up)
        }
        _ => false,
    }
}

/// Whether `property` (a pair or a method) belongs to an object literal given as an argument.
fn in_options(property: Node, up: &Parents) -> bool {
    up.of(property)
        .filter(|o| o.kind() == "object")
        .and_then(|o| up.of(o))
        .is_some_and(|a| a.kind() == "arguments")
}

/// The last name segment of the `Kind::Inline` definition an anonymous function argument
/// makes.
fn inline(node: Node) -> String {
    format!("@{}", node.start_byte())
}

/// The parameters of a function `node`; `scope` is the function's own qualified name, where
/// default values are read.
fn params(node: Node, src: &[u8], scope: &str, up: &Parents) -> Vec<Param> {
    let Some(list) = node
        .child_by_field_name("parameters")
        .or_else(|| node.child_by_field_name("parameter"))
    else {
        return Vec::new();
    };
    if list.kind() == "identifier" {
        return vec![Param {
            name: text(list, src).to_string(),
            binds: Binds::Any,
            label: None,
            default: None,
            fields: Vec::new(),
            closure: Default::default(),
        }];
    }
    named(list)
        .filter(|p| p.kind() != "comment")
        .map(|p| {
            let (pattern, default) = match p.kind() {
                "required_parameter" | "optional_parameter" => (
                    p.child_by_field_name("pattern"),
                    p.child_by_field_name("value"),
                ),
                "assignment_pattern" => (
                    p.child_by_field_name("left"),
                    p.child_by_field_name("right"),
                ),
                _ => (Some(p), None),
            };
            Param {
                name: pattern
                    .filter(|n| n.kind() == "identifier")
                    .map_or("", |n| text(n, src))
                    .to_string(),
                binds: Binds::Any,
                label: None,
                default: default.map(|d| value(d, src, scope, up)),
                fields: pattern
                    .filter(|n| n.kind() == "object_pattern")
                    .map_or_else(Vec::new, |n| fields(n, src, scope, up)),
                closure: Default::default(),
            }
        })
        .collect()
}

/// The names an object pattern binds: `{ a, b = 1, c: d, e: f = 2 }`.
// ponytail: a rest (`...rest`) or nested (`{ a: { b } }`) pattern binds nothing, so a name it
// holds stays unknown; bind them when a wrapper reads through one.
fn fields(pattern: Node, src: &[u8], scope: &str, up: &Parents) -> Vec<Field> {
    named(pattern)
        .filter_map(|p| {
            let (key, target, default) = match p.kind() {
                "shorthand_property_identifier_pattern" => (p, p, None),
                "object_assignment_pattern" => (
                    p.child_by_field_name("left")?,
                    p.child_by_field_name("left")?,
                    p.child_by_field_name("right"),
                ),
                "pair_pattern" => {
                    let target = p.child_by_field_name("value")?;
                    match target.kind() {
                        "assignment_pattern" => (
                            p.child_by_field_name("key")?,
                            target.child_by_field_name("left")?,
                            target.child_by_field_name("right"),
                        ),
                        _ => (p.child_by_field_name("key")?, target, None),
                    }
                }
                _ => return None,
            };
            (target.kind() == "identifier"
                || target.kind() == "shorthand_property_identifier_pattern")
                .then(|| Field {
                    name: text(target, src).to_string(),
                    key: match key.kind() {
                        "string" => unquote(key, src),
                        _ => text(key, src).to_string(),
                    },
                    default: default.map(|d| value(d, src, scope, up)),
                })
        })
        .collect()
}

/// The child of `node` that a binding names, with that name and the binding's span:
/// the value of a `const` declarator, of a `let` or `var` one that holds a function or class
/// and is assigned nowhere else, of an `export default`, of a top-level assignment to a member
/// (`assignment`) or of a function-valued property of an object literal bound by a top-level
/// `const`.
fn binds<'t>(node: Node<'t>, src: &[u8], up: &Parents) -> Option<(Node<'t>, String, Range<usize>)> {
    match node.kind() {
        "variable_declarator" => {
            let value = node.child_by_field_name("value")?;
            let name = node.child_by_field_name("name")?;
            let declaration = up.of(node)?;
            let kind = declaration
                .child_by_field_name("kind")
                .map(|k| text(k, src));
            let name = (name.kind() == "identifier").then(|| text(name, src))?;
            let held = FUNCTIONS.contains(&value.kind()) || value.kind() == "class";
            (kind == Some("const") || (held && !up.assigned.contains(name)))
                .then(|| (value, name.to_string(), node.byte_range()))
        }
        "export_statement" => {
            let value = node.child_by_field_name("value")?;
            let is_default = node
                .children(&mut node.walk())
                .any(|c| c.kind() == "default");
            is_default.then(|| (value, "default".to_string(), node.byte_range()))
        }
        "assignment_expression" => {
            let a = assignment(node, src, up)?;
            Some((
                node.child_by_field_name("right")?,
                a.name,
                node.byte_range(),
            ))
        }
        "pair" => {
            let key = node.child_by_field_name("key")?;
            let value = node.child_by_field_name("value")?;
            let owner = literal_owner(node, src, up).filter(|_| {
                key.kind() == "property_identifier" && FUNCTIONS.contains(&value.kind())
            })?;
            Some((
                value,
                format!("{owner}.{}", text(key, src)),
                node.byte_range(),
            ))
        }
        _ => None,
    }
}

/// A top-level assignment of a function or class to a member, as `binds` and `exports` read it.
struct Assigned {
    /// The name it defines: `obj.y`, `X.y` for `X.prototype.y`, `y` for `exports.y` and
    /// `module.exports.y`, `default` for `module.exports`.
    name: String,
    /// `X.prototype.y`: a method of `X`.
    method: bool,
    /// A CommonJS export.
    exported: bool,
}

/// What the statement `node` (an assignment) defines, when it is `<member> = <function or
/// class>` at the top level of the file. Other chains than `a.b.c` of plain names define
/// nothing, and neither does anything under `exports` or `module` but the CommonJS forms.
fn assignment(node: Node, src: &[u8], up: &Parents) -> Option<Assigned> {
    let (left, right) = (
        node.child_by_field_name("left")?,
        node.child_by_field_name("right")?,
    );
    let statement = up.of(node).filter(|s| s.kind() == "expression_statement")?;
    up.of(statement).filter(|p| p.kind() == "program")?;
    if left.kind() != "member_expression"
        || !(FUNCTIONS.contains(&right.kind()) || right.kind() == "class")
    {
        return None;
    }
    let path = callee(left, src);
    let names: Vec<&str> = path.split('.').collect();
    let plain = names.iter().all(|n| !n.is_empty() && !n.starts_with('('))
        && !matches!(names[0], "this" | "super");
    if !plain {
        return None;
    }
    let (name, method, exported) = match names.as_slice() {
        ["module", "exports"] => ("default".to_string(), false, true),
        ["exports", y] | ["module", "exports", y] => (y.to_string(), false, true),
        ["exports" | "module", ..] => return None,
        [.., "prototype"] => return None,
        ["prototype", _] => return None,
        [x @ .., "prototype", y] => (format!("{}.{y}", x.join(".")), true, false),
        _ => (path.clone(), false, false),
    };
    Some(Assigned {
        name,
        method,
        exported,
    })
}

/// The name of the top-level `const` whose object literal holds `property` (a pair or a
/// method): `o` for the `litM` of `const o = { litM() {} }`.
fn literal_owner(property: Node, src: &[u8], up: &Parents) -> Option<String> {
    let object = up.of(property).filter(|o| o.kind() == "object")?;
    let declarator = (up.of(object)).filter(|d| d.kind() == "variable_declarator")?;
    let name = declarator.child_by_field_name("name")?;
    let declaration = up.of(declarator)?;
    let top = match up.of(declaration)? {
        p if p.kind() == "export_statement" => up.of(p)?,
        p => p,
    };
    (name.kind() == "identifier"
        && declaration.kind() == "lexical_declaration"
        && declaration
            .child_by_field_name("kind")
            .is_some_and(|k| text(k, src) == "const")
        && top.kind() == "program")
        .then(|| text(name, src).to_string())
}

/// The bindings of `import d, { a, b as c } from 's'` and `import * as ns from 's'`.
fn imports(node: Node, src: &[u8], out: &mut Vec<Import>) {
    let (Some(source), Some(clause)) = (
        node.child_by_field_name("source"),
        children(node).find(|c| c.kind() == "import_clause"),
    ) else {
        return;
    };
    let source = unquote(source, src);
    let mut add = |local: Node, name| {
        out.push(Import {
            local: text(local, src).to_string(),
            source: source.clone(),
            name,
        })
    };
    for part in children(clause) {
        match part.kind() {
            "identifier" => add(part, Imported::Name("default".to_string())),
            "namespace_import" => {
                if let Some(local) = children(part).find(|c| c.kind() == "identifier") {
                    add(local, Imported::Namespace);
                }
            }
            "named_imports" => {
                for spec in children(part).filter(|c| c.kind() == "import_specifier") {
                    if let Some(name) = spec.child_by_field_name("name") {
                        let local = spec.child_by_field_name("alias").unwrap_or(name);
                        add(local, Imported::Name(text(name, src).to_string()));
                    }
                }
            }
            _ => {}
        }
    }
}

/// What an `export` statement adds to the file's exports.
fn exports(node: Node, src: &[u8], out: &mut Vec<Export>) {
    let local = |exported: &str, local: &str| Export::Local {
        exported: exported.to_string(),
        local: local.to_string(),
    };
    let declaration = node.child_by_field_name("declaration");
    let own_name = declaration
        .and_then(|d| d.child_by_field_name("name"))
        .map(|n| text(n, src));
    if children(node).any(|c| c.kind() == "default") {
        let value = node.child_by_field_name("value");
        let name = match (own_name, value) {
            (Some(name), _) => name,
            (None, Some(v)) if v.kind() == "identifier" => text(v, src),
            // An anonymous function or class: its definition is named `default`.
            _ => "default",
        };
        out.push(local("default", name));
        return;
    }
    if let Some(source) = node.child_by_field_name("source") {
        let source = unquote(source, src);
        let from = |exported: Node, name| Export::From {
            exported: text(exported, src).to_string(),
            source: source.clone(),
            name,
        };
        match children(node).find(|c| matches!(c.kind(), "export_clause" | "namespace_export")) {
            Some(clause) if clause.kind() == "export_clause" => {
                for spec in children(clause).filter(|c| c.kind() == "export_specifier") {
                    if let Some(name) = spec.child_by_field_name("name") {
                        let exported = spec.child_by_field_name("alias").unwrap_or(name);
                        out.push(from(exported, Imported::Name(text(name, src).to_string())));
                    }
                }
            }
            Some(namespace) => {
                if let Some(exported) = children(namespace).find(|c| c.kind() == "identifier") {
                    out.push(from(exported, Imported::Namespace));
                }
            }
            None => out.push(Export::All { source }),
        }
        return;
    }
    if let Some(name) = own_name {
        out.push(local(name, name));
    } else if let Some(declaration) = declaration {
        // `export const a = ..., b = ...`
        for declarator in children(declaration).filter(|c| c.kind() == "variable_declarator") {
            if let Some(name) = declarator
                .child_by_field_name("name")
                .filter(|n| n.kind() == "identifier")
            {
                out.push(local(text(name, src), text(name, src)));
            }
        }
    } else if let Some(clause) = children(node).find(|c| c.kind() == "export_clause") {
        for spec in children(clause).filter(|c| c.kind() == "export_specifier") {
            if let Some(name) = spec.child_by_field_name("name") {
                let exported = spec.child_by_field_name("alias").unwrap_or(name);
                out.push(local(text(exported, src), text(name, src)));
            }
        }
    }
}

/// The function written in place as `inline` within `scope`: `Unknown` when `scope` is as deep
/// as definitions go, since no definition stands behind it.
fn function(scope: &str, inline: &str) -> Value {
    let nested = if scope.is_empty() {
        0
    } else {
        scope.matches('.').count() + 1
    };
    if nested >= DEFINITION_DEPTH {
        return Value::Unknown;
    }
    Value::Function(qualify(scope, inline))
}

/// What an expression is, as far as its own syntax tells; `scope` is the qualified name of
/// the enclosing definition.
fn value(node: Node, src: &[u8], scope: &str, up: &Parents) -> Value {
    value_at(node, src, scope, up, 0)
}

/// `value` of a node `depth` levels inside the expression it started from.
fn value_at(node: Node, src: &[u8], scope: &str, up: &Parents, depth: usize) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    let deeper = |n: Node| value_at(n, src, scope, up, depth + 1);
    // `x as T` is `x`, or the schema `T` infers; the type of a cast is `cast`'s.
    if node.kind() == "as_expression"
        && let Some(inner) = named(node).next()
    {
        return match named(node).nth(1).and_then(|t| types::inferred(t, src)) {
            Some(schema) => Value::Schema(schema),
            None => deeper(inner),
        };
    }
    match node.kind() {
        "object" => {
            let properties = named(node).filter_map(|property| match property.kind() {
                "pair" => {
                    let key = property.child_by_field_name("key")?;
                    let key = match key.kind() {
                        "string" => unquote(key, src),
                        _ => text(key, src).to_string(),
                    };
                    let value = deeper(property.child_by_field_name("value")?);
                    Some((key, value))
                }
                "shorthand_property_identifier" => {
                    let name = text(property, src).to_string();
                    Some((name.clone(), Value::Ref(name)))
                }
                "spread_element" => {
                    let spread = named(property).next()?;
                    Some(("...".into(), deeper(spread)))
                }
                "method_definition" if in_options(property, up) => {
                    let key = text(property.child_by_field_name("name")?, src).to_string();
                    Some((key, function(scope, &inline(property))))
                }
                // A getter or method of a literal that is no options object: a key whose value
                // Meada does not read, not a key that is absent.
                "method_definition" => {
                    let key = text(property.child_by_field_name("name")?, src).to_string();
                    Some((key, Value::Unknown))
                }
                _ => None,
            });
            Value::Object(properties.collect())
        }
        "array" => Value::List(named(node).map(deeper).collect()),
        "await_expression" => named(node).next().map_or(Value::Unknown, deeper),
        "number" => Value::Scalar(text(node, src).parse().ok()),
        "true" | "false" => Value::Scalar(None),
        "identifier" if text(node, src) == "undefined" => Value::Undefined,
        "identifier" => Value::Ref(text(node, src).to_string()),
        kind if FUNCTIONS.contains(&kind) && written_in_place(node, up) => {
            function(scope, &inline(node))
        }
        kind => match parts(node, src) {
            Some(parts) => Value::Str(parts),
            None if kind == "call_expression" => {
                let args = node.child_by_field_name("arguments");
                let args = args.filter(|a| a.kind() == "arguments");
                callee_node(node).map_or(Value::Unknown, |f| {
                    let receiver = (f.kind() == "member_expression")
                        .then(|| f.child_by_field_name("object"))
                        .flatten()
                        .map(|object| Box::new(deeper(object)));
                    Value::Call(Invocation {
                        callee: callee(f, src),
                        receiver,
                        args: args.map_or(Vec::new(), |a| named(a).map(deeper).collect()),
                        kwargs: Vec::new(),
                        // The span of the call in `Facts::calls`.
                        range: match awaited_artifact(node) {
                            true => f.start_byte()..node.end_byte(),
                            false => node.byte_range(),
                        },
                    })
                })
            }
            None if kind == "new_expression" => callee_node(node).map_or(Value::Unknown, |f| {
                let args = node.child_by_field_name("arguments");
                let args = args.filter(|a| a.kind() == "arguments");
                let args = args.map(|a| named(a).map(deeper));
                Value::New(callee(f, src), args.into_iter().flatten().collect())
            }),
            None if kind == "member_expression" => {
                let object = node.child_by_field_name("object");
                let property = node.child_by_field_name("property");
                match (object, property) {
                    (Some(o), Some(p)) if o.kind() == "identifier" => {
                        Value::Member(text(o, src).to_string(), text(p, src).to_string())
                    }
                    _ => Value::Unknown,
                }
            }
            None => Value::Unknown,
        },
    }
}

/// The type a cast `node` (`x as T`) declares, when it lists fields (`z.infer` types are the
/// value's, see `value_at`).
fn cast(node: Node, src: &[u8], up: &Parents) -> Option<Ty> {
    let declared = named(node)
        .nth(1)
        .filter(|_| node.kind() == "as_expression")?;
    Some(types::ty(declared, src, up, &mut 0)).filter(|t| !matches!(t, Ty::Unknown | Ty::Schema(_)))
}

/// What a `const` holds and the type it is declared to have: by the annotation of the variable
/// or by a cast of its initializer. A `z.infer` type gives the value the schema it names.
fn declared(
    annotation: Option<Node>,
    init: Node,
    src: &[u8],
    scope: &str,
    up: &Parents,
) -> (Value, Option<Ty>) {
    let annotated = annotation.map(|t| types::ty(t, src, up, &mut 0));
    if let Some(Ty::Schema(schema)) = annotated {
        return (Value::Schema(schema), None);
    }
    let ty = annotated.or_else(|| cast(init, src, up));
    (
        value(init, src, scope, up),
        ty.filter(|t| *t != Ty::Unknown),
    )
}

/// A string expression folded into literal text, environment reads and holes; `None` when
/// `node` is not a string expression Meada folds.
fn parts(node: Node, src: &[u8]) -> Option<Vec<Part>> {
    parts_at(node, src, 0)
}

/// `parts` of a node `depth` levels inside the expression it started from; past `FOLD_DEPTH`
/// the rest is a hole.
fn parts_at(node: Node, src: &[u8], depth: usize) -> Option<Vec<Part>> {
    if depth >= FOLD_DEPTH {
        return Some(vec![Part::Hole]);
    }
    let parts = |n: Node| parts_at(n, src, depth + 1);
    // Anything inside a string that is not itself foldable is a hole.
    let inside = |n: Node| parts(n).unwrap_or_else(|| vec![Part::Hole]);
    match node.kind() {
        "string" => Some(vec![Part::Text(unquote(node, src))]),
        "template_string" => {
            let mut out = Vec::new();
            let mut at = node.start_byte() + 1;
            for hole in named(node).filter(|c| c.kind() == "template_substitution") {
                out.push(Part::Text(lossy(&src[at..hole.start_byte()])));
                out.extend(named(hole).next().map_or(vec![Part::Hole], inside));
                at = hole.end_byte();
            }
            out.push(Part::Text(lossy(
                &src[at..node.end_byte().saturating_sub(1)],
            )));
            out.retain(|p| !matches!(p, Part::Text(t) if t.is_empty()));
            Some(out)
        }
        "binary_expression" => {
            let left = node.child_by_field_name("left")?;
            let right = node.child_by_field_name("right")?;
            match text(node.child_by_field_name("operator")?, src) {
                // At least one side must be a string for `+` to concatenate.
                "+" => {
                    let (left, right) = (parts(left), parts(right));
                    if left.is_none() && right.is_none() {
                        return None;
                    }
                    let hole = || vec![Part::Hole];
                    let (left, right) = (left.unwrap_or_else(hole), right.unwrap_or_else(hole));
                    Some(left.into_iter().chain(right).collect())
                }
                // A fallback keeps the environment read on its left, if it is the only one.
                "||" | "??" => {
                    let env = single_env(parts(left)?)?;
                    let more = inside(right).iter().any(|p| matches!(p, Part::Env(_)));
                    Some(vec![if more { Part::Hole } else { env }])
                }
                _ => None,
            }
        }
        "identifier" if text(node, src) != "undefined" => {
            Some(vec![Part::Ref(text(node, src).to_string())])
        }
        "parenthesized_expression"
        | "non_null_expression"
        | "as_expression"
        | "satisfies_expression" => parts(named(node).next()?),
        _ => env_read(node, src, depth).map(|env| vec![env]),
    }
}

/// `process.env.X`, `process.env["X"]` or `import.meta.env.X`, also through string method
/// calls such as `.replace(...)` or `.trim()`.
fn env_read(node: Node, src: &[u8], depth: usize) -> Option<Part> {
    const STRING_METHODS: &[&str] = &[
        "replace",
        "replaceAll",
        "trim",
        "trimEnd",
        "trimStart",
        "toString",
        "toLowerCase",
    ];
    match node.kind() {
        "member_expression" | "subscript_expression" => {
            let object = text(node.child_by_field_name("object")?, src);
            if object != "process.env" && object != "import.meta.env" {
                return None;
            }
            let name = match node.child_by_field_name("index") {
                Some(index) if index.kind() == "string" => unquote(index, src),
                Some(_) => return None,
                None => text(node.child_by_field_name("property")?, src).to_string(),
            };
            Some(Part::Env(name))
        }
        "call_expression" => {
            let function = callee_node(node)?;
            let method = function.child_by_field_name("property")?;
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

/// The environment read that makes up all of `parts`, if that is what they are.
fn single_env(parts: Vec<Part>) -> Option<Part> {
    match <[Part; 1]>::try_from(parts) {
        Ok([env @ Part::Env(_)]) => Some(env),
        _ => None,
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A string literal's content: `'./m'` is `./m`.
fn unquote(node: Node, src: &[u8]) -> String {
    let quoted = text(node, src);
    quoted
        .get(1..quoted.len().saturating_sub(1))
        .unwrap_or("")
        .to_string()
}

/// What a call or `new` expression calls. With type arguments under `await`, the grammar
/// reads `await get<Row>(x)` as a call whose `function` is `await get`: the awaited call is
/// `get<Row>(x)`, so the callee is the operand of that `await`.
fn callee_node(node: Node) -> Option<Node> {
    let function = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"))?;
    if awaited_artifact(node) {
        named(function).next()
    } else {
        Some(function)
    }
}

/// Whether `node` is the parse artifact `await f<T>(x)`: type arguments, and an `await` as
/// the `function`.
fn awaited_artifact(node: Node) -> bool {
    node.child_by_field_name("type_arguments").is_some()
        && node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("constructor"))
            .is_some_and(|f| f.kind() == "await_expression")
}

/// `a.b.c` for identifier and member chains; any other expression becomes `(<node kind>)`,
/// so no source text beyond names reaches the index.
fn callee(node: Node, src: &[u8]) -> String {
    // The properties of the chain, outermost first; a chain longer than `FOLD_DEPTH` ends in
    // the kind of the node where it is cut.
    let (mut node, mut names) = (node, Vec::new());
    let base = loop {
        match node.kind() {
            "identifier"
            | "this"
            | "super"
            | "property_identifier"
            | "private_property_identifier" => break text(node, src).to_string(),
            "member_expression" if names.len() < FOLD_DEPTH => match (
                node.child_by_field_name("object"),
                node.child_by_field_name("property"),
            ) {
                (Some(object), Some(property)) => {
                    names.push(text(property, src));
                    node = object;
                }
                _ => break format!("({})", node.kind()),
            },
            kind => break format!("({kind})"),
        }
    };
    names
        .iter()
        .rev()
        .fold(base, |path, name| path + "." + name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tsconfig_that_is_invalid_or_past_the_size_limit_declares_no_paths() {
        let resolve = |config: &[u8]| {
            let modules = TsJs
                .modules(&[("tsconfig.json", config)], &BTreeMap::new())
                .unwrap();
            (modules("src/a.ts", "@/b"), modules("src/a.ts", "./c"))
        };
        let valid = r#"{"compilerOptions": {"paths": {"@/*": ["src/*"]}}}"#;
        let (aliased, relative) = resolve(valid.as_bytes());
        assert!(aliased.contains(&"src/b.ts".to_string()), "{aliased:?}");
        let padded = format!("{valid}{}", " ".repeat(crate::lang::MAX_BYTES));
        let deep = "[".repeat(100_000);
        for config in [
            padded.as_bytes(),
            b"{ not json",
            deep.as_bytes(),
            b"{\"a\": \"\xff\"}",
        ] {
            assert_eq!(resolve(config), (std::rc::Rc::default(), relative.clone()));
        }
    }

    #[test]
    fn the_nearest_config_serves_an_importer_and_node_modules_declares_nothing() {
        let config = |t: &str| format!(r#"{{"compilerOptions": {{"paths": {{"@/*": ["{t}"]}}}}}}"#);
        let (root, app, js, hidden) = (
            config("lib/*"),
            config("./src/*"),
            config("./js/*"),
            config("./x/*"),
        );
        let modules = TsJs
            .modules(
                &[
                    ("tsconfig.json", root.as_bytes()),
                    ("app/tsconfig.json", app.as_bytes()),
                    ("app/node_modules/p/tsconfig.json", hidden.as_bytes()),
                    ("app/jsconfig.json", js.as_bytes()),
                    ("web/jsconfig.json", js.as_bytes()),
                    ("bad/tsconfig.json", b"{ not json"),
                ],
                &BTreeMap::new(),
            )
            .unwrap();
        let has = |from: &str, file: &str| modules(from, "@/b").contains(&file.to_string());
        assert!(has("x.ts", "lib/b.ts"));
        assert!(has("app/src/a.ts", "app/src/b.ts"));
        assert!(!has("app/src/a.ts", "app/js/b.ts"));
        assert!(has("web/a.ts", "web/js/b.ts"));
        // A hostile nested config declares nothing, and hides the root's aliases from its subtree.
        assert!(modules("bad/a.ts", "@/b").is_empty());
        assert!(!has("app/node_modules/p/a.ts", "app/node_modules/p/x/b.ts"));
    }

    #[test]
    fn a_cast_is_the_value_it_casts() {
        let src = "const BASE = process.env.API;\n\
                   const u = BASE as string;\n\
                   fetch(url as string);\n\
                   fetch(`${BASE as string}/x`);\n";
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let held = facts.consts.iter().find(|c| c.name == "u").unwrap();
        assert!(matches!(&held.value, Value::Ref(name) if name == "BASE"));
        // A type that lists no fields is no type of the constant.
        assert_eq!(held.ty, None);
        let args: Vec<_> = facts.calls.iter().filter(|c| c.callee == "fetch").collect();
        assert!(matches!(args[0].args.as_slice(), [Value::Ref(name)] if name == "url"));
        assert_eq!(args[0].arg_types, [None]);
        assert!(matches!(args[1].args.as_slice(), [Value::Str(_)]));
    }

    /// How many calls deep the first argument of `value` goes.
    fn nesting(value: &Value) -> usize {
        let mut value = value;
        let mut depth = 0;
        while let Value::Call(call) = value {
            depth += 1;
            match call.args.first() {
                Some(inner) => value = inner,
                None => break,
            }
        }
        depth
    }

    #[test]
    fn expressions_nested_past_the_depth_limit_are_unknown() {
        let n = VALUE_DEPTH * 2;
        let src = format!("const x = {}1{};\n", "f(".repeat(n), ")".repeat(n));

        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();

        let depth = nesting(&facts.consts[0].value);
        assert!((VALUE_DEPTH - 1..=VALUE_DEPTH).contains(&depth), "{depth}");
    }

    #[test]
    fn a_method_chain_keeps_the_receiver_and_the_arguments_of_each_call() {
        let src = "const x = z.object({ a: 1 }).optional();\n";

        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();

        let Value::Call(outer) = &facts.consts[0].value else {
            unreachable!("not a call");
        };
        assert_eq!(outer.callee, "(call_expression).optional");
        let Some(Value::Call(inner)) = outer.receiver.as_deref() else {
            unreachable!("no receiver call");
        };
        assert_eq!(inner.callee, "z.object");
        assert!(matches!(inner.receiver.as_deref(), Some(Value::Ref(z)) if z == "z"));
        assert!(matches!(inner.args.as_slice(), [Value::Object(p)] if p.len() == 1));
    }

    fn parse(src: &str) -> Facts {
        TsJs.parse("a.ts", src.as_bytes()).unwrap()
    }

    fn depth(value: &Value) -> usize {
        match value {
            Value::New(_, args) => 1 + args.iter().map(depth).max().unwrap_or(0),
            _ => 1,
        }
    }

    #[test]
    fn definitions_nest_to_a_limit() {
        let n = 512;
        let facts = parse(&format!("{}1{};", "f(() => ".repeat(n), ")".repeat(n)));
        assert_eq!(facts.calls.len(), n);
        let deepest = (facts.defs.iter()).map(|d| d.name.matches('.').count() + 1);
        assert_eq!(deepest.max(), Some(DEFINITION_DEPTH.min(n)));
    }

    /// The definitions of `facts` as `(name, kind)`, sorted by name.
    fn defined(facts: &Facts) -> Vec<(&str, &'static str)> {
        let kind = |k: Kind| match k {
            Kind::Function => "function",
            Kind::Method => "method",
            Kind::Class => "class",
            _ => "other",
        };
        let mut out: Vec<_> = (facts.defs.iter())
            .map(|d| (d.name.as_str(), kind(d.kind)))
            .collect();
        out.sort_by_key(|(name, _)| *name);
        out
    }

    #[test]
    fn a_function_assigned_to_a_member_at_the_top_level_is_a_definition() {
        let facts = parse(
            "exports.a = function () {};\nmodule.exports.b = () => {};\n\
             V.prototype.m = function m() {};\napp.use = function use() {};\n\
             x.y.z = () => {};\nthis.t = () => {};\nmodule.exports.a.b = () => {};\n\
             f().g = () => {};\nfunction f() { o.p = () => {}; }\n\
             module.exports = function () {};\n",
        );

        assert_eq!(
            defined(&facts),
            [
                ("V.m", "method"),
                ("a", "function"),
                ("app.use", "function"),
                ("b", "function"),
                ("default", "function"),
                ("f", "function"),
                ("x.y.z", "function"),
            ]
        );
        let exported: Vec<_> = (facts.exports.iter())
            .filter_map(|e| match e {
                Export::Local { exported, local } if exported == local => Some(exported.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(exported, ["a", "b", "default"]);
    }

    #[test]
    fn a_let_or_var_function_is_a_definition_unless_assigned_again() {
        let facts = parse(
            "let a = () => {}; var b = function () {}; let c = () => {}; c = a;\n\
             var d = () => {}; [d] = [a]; let e = 1; var g = class {}; var h = () => {};\n\
             for (h in a) {}\nlet i = () => {}; i += 1;\n",
        );

        assert_eq!(
            defined(&facts),
            [("a", "function"), ("b", "function"), ("g", "class")]
        );
        for name in ["c", "d", "e", "h", "i"] {
            assert!(facts.locals.contains(&name.to_string()), "{name}");
        }
    }

    #[test]
    fn the_functions_of_a_top_level_const_object_are_definitions() {
        let facts = parse(
            "const o = { m() {}, p: () => {}, q: 1, 'k-x': () => {}, n: { deep() {} } };\n\
             let l = { z() {} };\nexport const e = { y: function () {} };\n\
             function f() { const inner = { w() {} }; }\nrun({ eachMessage() {} });\n",
        );

        let names: Vec<_> = defined(&facts).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names.len(), 5, "{names:?}");
        assert!(names[0].starts_with('@'));
        assert_eq!(&names[1..], ["e.y", "f", "o.m", "o.p"]);
    }

    #[test]
    fn let_definitions_nest_to_a_limit() {
        let n = DEFINITION_DEPTH * 2;
        let facts = parse(&format!(
            "{}1{}",
            "let f = () => {".repeat(n),
            "};".repeat(n)
        ));

        let deepest = (facts.defs.iter()).map(|d| d.name.matches('.').count() + 1);
        assert_eq!(deepest.max(), Some(DEFINITION_DEPTH));
    }

    #[test]
    fn values_nest_to_a_limit() {
        let n = 512;
        let facts = parse(&format!("{}{};", "new F(".repeat(n), ")".repeat(n)));
        assert_eq!(facts.calls.len(), n);
        let deepest = (facts.calls.iter().flat_map(|c| &c.args)).map(depth);
        assert_eq!(deepest.max(), Some(VALUE_DEPTH + 1));
    }

    #[test]
    fn a_definition_past_the_limit_hides_its_name() {
        let n = DEFINITION_DEPTH;
        let src = format!(
            "function g() {{}}\n{}function g() {{}} g();{}",
            "f(() => {".repeat(n),
            "});".repeat(n)
        );
        let facts = parse(&src);
        let call = facts.calls.iter().find(|c| c.callee == "g").unwrap();
        assert!(facts.locals.contains(&qualify(&call.scope, "g")));
        assert!(
            facts
                .defs
                .iter()
                .all(|d| d.name != qualify(&call.scope, "g"))
        );
    }

    #[test]
    fn a_handler_past_the_limit_is_unknown() {
        let n = DEFINITION_DEPTH;
        let src = format!("{}h(() => 1){}", "f(() => {".repeat(n), "});".repeat(n));
        let facts = parse(&src);
        let call = facts.calls.iter().find(|c| c.callee == "h").unwrap();
        assert!(matches!(call.args[..], [Value::Unknown]));
        let options = format!(
            "{}h({{ a() {{}} }}){}",
            "f(() => {".repeat(n),
            "});".repeat(n)
        );
        let options = parse(&options);
        let call = options.calls.iter().find(|c| c.callee == "h").unwrap();
        assert!(
            matches!(&call.args[..], [Value::Object(p)] if matches!(p[..], [(_, Value::Unknown)]))
        );
    }

    #[test]
    fn long_chains_are_cut() {
        let n = 30_000;
        let sum = parse(&format!("const a = '' + {}b;", "b + ".repeat(n)));
        let Value::Str(parts) = &sum.consts[0].value else {
            panic!("a string");
        };
        assert!(parts.len() <= FOLD_DEPTH + 2);
        let env = parse(&format!("const a = process.env.X{};", ".trim()".repeat(n)));
        assert!(matches!(
            env.consts[0].value,
            Value::Str(_) | Value::Call(_)
        ));
        let member = parse(&format!("a{}.f();", ".b".repeat(n)));
        assert_eq!(member.calls.len(), 1);
        assert!(member.calls[0].callee.len() < 10 * FOLD_DEPTH);
    }

    #[test]
    fn concatenation_folds_once_per_term() {
        let facts = parse(&format!("const a = '/x' + {}'y';", "b + ".repeat(64)));
        let Value::Str(parts) = &facts.consts[0].value else {
            panic!("a string");
        };
        assert_eq!(parts.len(), 66);
    }

    #[test]
    fn type_arguments_under_await_leave_the_call_intact() {
        // The grammar reads `await get<Row>("/x")` as a call of `await get`.
        for (typed, plain) in [
            ("await get<Row>(\"/x\")", "await get(\"/x\")"),
            ("await api.get<Row>(\"/x\")", "await api.get(\"/x\")"),
            ("await new Client<Row>(\"/x\")", "await new Client(\"/x\")"),
        ] {
            let facts = |call: &str| parse(&format!("async function f() {{ const d = {call}; }}"));
            let (typed, plain) = (facts(typed), facts(plain));
            assert_eq!(typed.calls.len(), 1);
            assert_eq!(typed.calls[0].callee, plain.calls[0].callee);
            assert!(!typed.calls[0].callee.starts_with('('));
            assert_eq!(typed.calls[0].args.len(), 1);
        }
    }

    #[test]
    fn a_typed_call_under_await_spans_like_the_untyped_one() {
        let range = |call: &str| {
            let src = format!("async function f() {{ await {call}; }}");
            let at = src.find("await").unwrap() + "await ".len();
            let r = parse(&src).calls[0].range.clone();
            (r.start - at, r.end - at, call.len())
        };
        let (start, end, len) = range("get<Row>(\"/x\")");
        assert_eq!((start, end), (0, len));
        assert_eq!(range("get(\"/x\")").0, 0);
    }

    #[test]
    fn a_parenthesized_await_is_not_unwrapped() {
        for src in ["(await f)(x)", "(await getFn())(x)"] {
            let facts = parse(&format!("async function f() {{ {src}; }}"));
            let callees: Vec<_> = facts.calls.iter().map(|c| c.callee.as_str()).collect();
            assert!(callees.contains(&"(parenthesized_expression)"), "{src}");
        }
    }

    #[test]
    fn an_awaited_typed_call_is_a_call_value() {
        let facts = parse("async function f() { g(await get<Row>(\"/x\"), await get(\"/y\")); }");
        let call = facts.calls.iter().find(|c| c.callee == "g").unwrap();
        assert_eq!(call.args.len(), 2);
        assert!(
            call.args
                .iter()
                .all(|a| matches!(a, Value::Call(call) if call.callee == "get"))
        );
    }
}
