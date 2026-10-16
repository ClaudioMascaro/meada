//! The TypeScript and JavaScript language pack.

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::{Language, Node, Parser};

use super::{
    Call, Const, Def, Export, Facts, Field, Import, Imported, Kind, LanguagePack, Message, Modules,
    Param, Part, Value,
};
use crate::resolve::Code;
use crate::{Result, qualify};

mod messages;

/// How many definitions may nest in one file, and how many levels of arguments, objects and
/// arrays `value` reads. Names are qualified by the definitions around them and each call
/// resolves its names up that chain, so unbounded nesting costs cubic time; a value is read
/// again by every `new` around it. Past the limit a function is no definition (its body belongs
/// to the one around it) and a value is `Unknown`.
// ponytail: real code nests a few dozen levels; 128 matches the document readers.
const MAX_NESTING: usize = 128;

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
        let parents = Parents::new(tree.root_node());
        let mut facts = Facts::default();
        // Explicit stack instead of recursion: generated code can nest deeper than the call
        // stack.
        let mut stack = vec![Visit {
            node: tree.root_node(),
            scope: String::new(),
            depth: 0,
            binding: None,
        }];
        while let Some(Visit {
            node,
            scope,
            depth,
            binding,
        }) = stack.pop()
        {
            let mut inner = scope.clone();
            let defined = definition(node, binding, src, &parents).filter(|_| depth < MAX_NESTING);
            let is_def = defined.is_some();
            if let Some((name, kind, range)) = defined {
                inner = qualify(&scope, &name);
                facts.defs.push(Def {
                    params: params(node, src, &inner, &parents),
                    name: inner.clone(),
                    kind,
                    range,
                });
            }
            match node.kind() {
                "call_expression" | "new_expression" => {
                    // `new X(...)` is a call of `X`: constructors are how some libraries are
                    // used (`new SendMessageCommand(...)`, `new Worker(...)`).
                    let function = node
                        .child_by_field_name("function")
                        .or_else(|| node.child_by_field_name("constructor"));
                    if let Some(function) = function {
                        facts.calls.push(Call {
                            scope: scope.clone(),
                            callee: callee(function, src),
                            range: node.byte_range(),
                            args: node
                                .child_by_field_name("arguments")
                                .filter(|a| a.kind() == "arguments")
                                .map(|a| {
                                    named(a)
                                        .map(|arg| value(arg, src, &scope, &parents))
                                        .collect()
                                })
                                .unwrap_or_default(),
                        });
                    }
                }
                "import_statement" => imports(node, src, &mut facts.imports),
                "export_statement" => exports(node, src, &mut facts.exports),
                _ => {}
            }
            let named = binds(node, src, &parents);
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
                facts.consts.push(Const {
                    name: qualify(&scope, name),
                    value: value(*value_node, src, &scope, &parents),
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
                }
            }));
        }
        Ok(facts)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["tsconfig.json", "jsconfig.json"]
    }

    fn messages(&self, code: &Code) -> Vec<Message> {
        messages::messages(code)
    }

    fn modules(&self, configs: &[(&str, &[u8])]) -> Result<Modules> {
        let aliases = match configs.first() {
            Some((name, src)) => Aliases::parse(src).map_err(|e| format!("{name}: {e}"))?,
            None => Aliases::default(),
        };
        Ok(Box::new(move |importer, specifier| {
            let bases: Vec<String> = if specifier.starts_with('.') {
                let dir = importer.rsplit_once('/').map_or("", |(dir, _)| dir);
                join(dir, specifier).into_iter().collect()
            } else {
                aliases.expand(specifier)
            };
            bases
                .iter()
                .flat_map(|base| with_extensions(base))
                .collect()
        }))
    }
}

/// `compilerOptions.baseUrl` and `paths` of the root `tsconfig.json` (or `jsconfig.json`).
// ponytail: `extends` and nested project configs are not followed.
#[derive(Default)]
struct Aliases {
    base_url: Option<String>,
    paths: Vec<(String, Vec<String>)>,
}

impl Aliases {
    fn parse(src: &[u8]) -> Result<Self> {
        let config: serde_json::Value =
            serde_json::from_str(&strip_jsonc(std::str::from_utf8(src)?))?;
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
        Ok(Self {
            base_url: options["baseUrl"].as_str().and_then(|b| join("", b)),
            paths,
        })
    }

    /// The paths a bare specifier names: the targets of the `paths` pattern with the
    /// longest prefix that matches, then the specifier under `baseUrl`. Without `baseUrl`,
    /// targets are relative to the config file, at the repository root.
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
        let root = self.base_url.as_deref().unwrap_or("");
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
        // Methods of object literals are not definitions: only those of a class body, and
        // those of an options object, which are written in place like an argument.
        "method_definition" if up.of(node)?.kind() == "class_body" => (Kind::Method, own()?),
        "method_definition" if in_options(node, up) => {
            (Kind::Inline, (inline(node), node.byte_range()))
        }
        kind if FUNCTIONS.contains(&kind) => match binding {
            Some(binding) => (Kind::Function, binding),
            None if written_in_place(node, up) => (Kind::Inline, (inline(node), node.byte_range())),
            None => return None,
        },
        "class" => (Kind::Class, binding?),
        _ => return None,
    };
    Some((name, kind, range))
}

/// The parent of every node, read once: `Node::parent` walks down from the root, so asking it
/// for each function in deeply nested code costs time quadratic in the file.
struct Parents<'t>(HashMap<usize, Node<'t>>);

impl<'t> Parents<'t> {
    fn new(root: Node<'t>) -> Self {
        let mut parents = HashMap::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            for child in children(node) {
                parents.insert(child.id(), node);
                stack.push(child);
            }
        }
        Self(parents)
    }

    fn of(&self, node: Node) -> Option<Node<'t>> {
        self.0.get(&node.id()).copied()
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
    let mut out = Vec::new();
    // Explicit stack: a pattern can nest deeper than the call stack.
    let mut stack = vec![pattern];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                out.push(text(node, src).to_string())
            }
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
            default: None,
            fields: Vec::new(),
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
                default: default.map(|d| value(d, src, scope, up)),
                fields: pattern
                    .filter(|n| n.kind() == "object_pattern")
                    .map_or_else(Vec::new, |n| fields(n, src, scope, up)),
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
/// the value of a `const` declarator, or of an `export default`.
fn binds<'t>(node: Node<'t>, src: &[u8], up: &Parents) -> Option<(Node<'t>, String, Range<usize>)> {
    let value = node.child_by_field_name("value")?;
    match node.kind() {
        "variable_declarator" => {
            let declaration = up.of(node)?;
            let is_const = declaration
                .child_by_field_name("kind")
                .is_some_and(|k| text(k, src) == "const");
            let name = node.child_by_field_name("name")?;
            (is_const && name.kind() == "identifier")
                .then(|| (value, text(name, src).to_string(), node.byte_range()))
        }
        "export_statement" => {
            let is_default = node
                .children(&mut node.walk())
                .any(|c| c.kind() == "default");
            is_default.then(|| (value, "default".to_string(), node.byte_range()))
        }
        _ => None,
    }
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

/// What an expression is, as far as its own syntax tells; `scope` is the qualified name of
/// the enclosing definition.
fn value(node: Node, src: &[u8], scope: &str, up: &Parents) -> Value {
    value_at(node, src, scope, up, 0)
}

/// `value` of a node `depth` levels inside the expression it started from.
fn value_at(node: Node, src: &[u8], scope: &str, up: &Parents, depth: usize) -> Value {
    if depth >= MAX_NESTING {
        return Value::Unknown;
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
                    let value = value_at(
                        property.child_by_field_name("value")?,
                        src,
                        scope,
                        up,
                        depth + 1,
                    );
                    Some((key, value))
                }
                "shorthand_property_identifier" => {
                    let name = text(property, src).to_string();
                    Some((name.clone(), Value::Ref(name)))
                }
                "spread_element" => {
                    let spread = named(property).next()?;
                    Some(("...".into(), value_at(spread, src, scope, up, depth + 1)))
                }
                "method_definition" if in_options(property, up) => {
                    let key = text(property.child_by_field_name("name")?, src).to_string();
                    Some((key, Value::Function(qualify(scope, &inline(property)))))
                }
                _ => None,
            });
            Value::Object(properties.collect())
        }
        "array" => Value::List(
            named(node)
                .map(|item| value_at(item, src, scope, up, depth + 1))
                .collect(),
        ),
        "await_expression" => named(node)
            .next()
            .map_or(Value::Unknown, |n| value_at(n, src, scope, up, depth + 1)),
        "identifier" if text(node, src) == "undefined" => Value::Undefined,
        "identifier" => Value::Ref(text(node, src).to_string()),
        kind if FUNCTIONS.contains(&kind) && written_in_place(node, up) => {
            Value::Function(qualify(scope, &inline(node)))
        }
        kind => match parts(node, src) {
            Some(parts) => Value::Str(parts),
            None if kind == "call_expression" => node
                .child_by_field_name("function")
                .map_or(Value::Unknown, |f| Value::Call(callee(f, src))),
            None if kind == "new_expression" => {
                node.child_by_field_name("constructor")
                    .map_or(Value::Unknown, |f| {
                        let args = node.child_by_field_name("arguments");
                        let args = args.filter(|a| a.kind() == "arguments");
                        let args = args
                            .map(|a| named(a).map(|arg| value_at(arg, src, scope, up, depth + 1)));
                        Value::New(callee(f, src), args.into_iter().flatten().collect())
                    })
            }
            None => Value::Unknown,
        },
    }
}

/// A string expression folded into literal text, environment reads and holes; `None` when
/// `node` is not a string expression Meada folds.
fn parts(node: Node, src: &[u8]) -> Option<Vec<Part>> {
    // Anything inside a string that is not itself foldable is a hole.
    let inside = |n: Node| parts(n, src).unwrap_or_else(|| vec![Part::Hole]);
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
                    let (left, right) = (parts(left, src), parts(right, src));
                    if left.is_none() && right.is_none() {
                        return None;
                    }
                    let hole = || vec![Part::Hole];
                    let (left, right) = (left.unwrap_or_else(hole), right.unwrap_or_else(hole));
                    Some(left.into_iter().chain(right).collect())
                }
                // A fallback keeps the environment read on its left, if it is the only one.
                "||" | "??" => {
                    let env = single_env(parts(left, src)?)?;
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
        | "satisfies_expression" => parts(named(node).next()?, src),
        _ => env_read(node, src).map(|env| vec![env]),
    }
}

/// `process.env.X`, `process.env["X"]` or `import.meta.env.X`, also through string method
/// calls such as `.replace(...)` or `.trim()`.
fn env_read(node: Node, src: &[u8]) -> Option<Part> {
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
            let function = node.child_by_field_name("function")?;
            let method = function.child_by_field_name("property")?;
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

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn named<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .collect::<Vec<_>>()
        .into_iter()
}

fn children<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect::<Vec<_>>().into_iter()
}

/// A string literal's content: `'./m'` is `./m`.
fn unquote(node: Node, src: &[u8]) -> String {
    let quoted = text(node, src);
    quoted
        .get(1..quoted.len().saturating_sub(1))
        .unwrap_or("")
        .to_string()
}

/// `a.b.c` for identifier and member chains; any other expression becomes `(<node kind>)`,
/// so no source text beyond names reaches the index.
fn callee(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "identifier" | "this" | "super" | "property_identifier" | "private_property_identifier" => {
            text(node, src).to_string()
        }
        "member_expression" => match (
            node.child_by_field_name("object"),
            node.child_by_field_name("property"),
        ) {
            (Some(object), Some(property)) => {
                format!("{}.{}", callee(object, src), text(property, src))
            }
            _ => format!("({})", node.kind()),
        },
        kind => format!("({kind})"),
    }
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(src: &str) -> Facts {
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
        let facts = facts(&format!("{}1{};", "f(() => ".repeat(n), ")".repeat(n)));
        assert_eq!(facts.calls.len(), n);
        let deepest = (facts.defs.iter()).map(|d| d.name.matches('.').count() + 1);
        assert_eq!(deepest.max(), Some(MAX_NESTING.min(n)));
    }

    #[test]
    fn values_nest_to_a_limit() {
        let n = 512;
        let facts = facts(&format!("{}{};", "new F(".repeat(n), ")".repeat(n)));
        assert_eq!(facts.calls.len(), n);
        let deepest = (facts.calls.iter().flat_map(|c| &c.args)).map(depth);
        assert_eq!(deepest.max(), Some(MAX_NESTING + 1));
    }

    #[test]
    fn concatenation_folds_once_per_term() {
        let facts = facts(&format!("const a = '/x' + {}'y';", "b + ".repeat(64)));
        let Value::Str(parts) = &facts.consts[0].value else {
            panic!("a string");
        };
        assert_eq!(parts.len(), 66);
    }
}
