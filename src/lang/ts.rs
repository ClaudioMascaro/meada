use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Language, Node, Parser};

use super::{
    Binds, Call, ConfigInput, Const, Declaration, Def, Export, Facts, Field, FieldWrites, Import,
    Imported, Invocation, Kind, LanguagePack, Modules, Param, Part, Ty, Value, children, named,
    text,
};
use crate::{Result, package, qualify};

mod globals;
mod receivers;
mod references;
mod types;
mod uses;

// ponytail: real code nests a few dozen levels; 128 is the depth `document/schema.rs` reads
// (`MAX_DEPTH`).
const DEFINITION_DEPTH: usize = 128;

// ponytail: real code nests a few levels of options; 64 is far past any.
const VALUE_DEPTH: usize = 64;

// ponytail: real code concatenates and chains a few dozen terms; the recursion must stay
// shallow, whatever the file.
const FOLD_DEPTH: usize = 256;

// ponytail: real files read a few kilobytes of receivers; one budget per file keeps a crafted
// chain from costing the square of its length: a file costs its length plus at most the budget.
const RECEIVER_BUDGET: usize = 1 << 20;

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
        let module = path.ends_with(".mjs");
        let parents = Parents::new(tree.root_node(), src, module);
        let repaired = if parents.arrow_parameters.is_empty() {
            None
        } else {
            let mut syntax = src.to_vec();
            for &at in &parents.arrow_parameters {
                syntax[at] = b'_';
            }
            Some(
                parser
                    .parse(&syntax, None)
                    .ok_or("tree-sitter returned no tree")?,
            )
        };
        let root = repaired.as_ref().unwrap_or(&tree).root_node();
        let parents = if repaired.is_some() {
            Parents::new(root, src, module)
        } else {
            parents
        };
        let mut facts = Facts {
            member_writes: Some(Vec::new()),
            ..Facts::default()
        };
        let mut write_bytes = [0; 2];
        let mut receiver_bytes = 0;
        let mut uses = uses::Uses::new(src, &parents);
        let mut references = references::References::default();
        let typescript = path.ends_with(".ts") || path.ends_with(".tsx");
        let mut sealed: Vec<(String, String, Option<Value>)> = Vec::new();
        let mut writes: BTreeMap<(Option<String>, String), Vec<Option<Value>>> = BTreeMap::new();
        let mut private: Vec<(String, String, Stated)> = Vec::new();
        let mut stated: BTreeMap<(Option<String>, String), Vec<Stated>> = BTreeMap::new();
        let mut assigned = HashSet::new();
        let mut query_writes = HashMap::new();
        let mut query_locals = Vec::new();
        let mut functions: HashMap<usize, usize> = HashMap::new();
        let mut results: HashMap<usize, usize> = HashMap::new();
        let mut stack = vec![Visit {
            node: root,
            block: root,
            scope: String::new(),
            depth: 0,
            body: root,
            binding: None,
        }];
        while let Some(Visit {
            node,
            block,
            scope,
            depth,
            body,
            binding,
        }) = stack.pop()
        {
            let block = match node.kind() {
                "statement_block" | "switch_body" | "for_statement" | "for_in_statement"
                | "catch_clause" | "class_static_block" => node,
                _ => block,
            };
            let mut inner = scope.clone();
            let mut defined = definition(node, binding, src, &parents);
            if depth >= DEFINITION_DEPTH
                && let Some((name, kind, _)) = defined.take()
                && matches!(kind, Kind::Function | Kind::Class)
            {
                facts.locals.push(qualify(&scope, &name));
            }
            let is_def = defined.is_some();
            if let Some((name, kind, range)) = defined {
                inner = qualify(&scope, &name);
                functions.insert(node.id(), facts.defs.len());
                facts.defs.push(Def {
                    asynchronous: children(node).any(|n| n.kind() == "async"),
                    params: params(node, src, &inner, &parents),
                    returns: (node.child_by_field_name("return_type"))
                        .and_then(|t| types::returned(t, src, &parents)),
                    type_params: types::type_parameters(node, src),
                    single_return: single_return(node),
                    terminal_return: (node.child_by_field_name("body")).is_some_and(|b| {
                        if b.kind() == "statement_block" {
                            named(b)
                                .filter(|n| n.kind() != "comment")
                                .last()
                                .is_some_and(|n| n.kind() == "return_statement")
                        } else {
                            node.kind() == "arrow_function"
                        }
                    }),
                    return_values: (node.child_by_field_name("body"))
                        .filter(|b| {
                            node.kind() == "arrow_function" && b.kind() != "statement_block"
                        })
                        .map(|b| vec![value(b, src, &inner, &parents)])
                        .unwrap_or_default(),
                    ..Def::new(inner.clone(), kind, range)
                });
                receivers::definition(node, src, &inner, &parents, &mut facts);
            }
            references.visit(node, &scope, src, &parents);
            match node.kind() {
                "call_expression" | "new_expression" => {
                    facts.loads.extend(loads_from(node, src, &parents));
                    let arguments =
                        (node.child_by_field_name("arguments")).filter(|a| a.kind() == "arguments");
                    if let Some(function) = callee_node(node) {
                        if let Some(receiver) = results.remove(&node.id()) {
                            facts.receivers[receiver].class =
                                super::Class::Result(facts.calls.len());
                        }
                        let start = if awaited_artifact(node) {
                            function.start_byte()
                        } else {
                            node.start_byte()
                        };
                        if function.kind() == "member_expression"
                            && let Some(head) =
                                function.child_by_field_name("object").map(unwrapped)
                            && head.kind() == "identifier"
                            && globals::namespace(text(head, src))
                            && parents.global(text(head, src), head.start_byte())
                        {
                            facts.ts_global_calls.insert((start, node.end_byte()));
                        }
                        let callee = callee(function, src);
                        let (root, awaits) = receivers::wrapped(function, src)
                            .map_or((None, false), |(name, awaited)| (Some(name), awaited));
                        let head = root
                            .as_deref()
                            .or_else(|| callee.rsplit_once('.').map(|(h, _)| h));
                        if head.is_some_and(|h| h.starts_with("this."))
                            && !instance_context(function, &parents)
                        {
                            facts.ts_rebound_this.insert((start, node.end_byte()));
                        }
                        facts.calls.push(Call {
                            arity: None,
                            labels: None,
                            awaited: receivers::awaited(node, &parents),
                            constructs: node.kind() == "new_expression",
                            scope: scope.clone(),
                            callee,
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
                            root,
                            on: None,
                            unwraps: false,
                            awaits,
                            receiver: (function.kind() == "member_expression")
                                .then(|| function.child_by_field_name("object"))
                                .flatten()
                                .filter(|object| {
                                    matches!(
                                        object.kind(),
                                        "call_expression"
                                            | "new_expression"
                                            | "await_expression"
                                            | "parenthesized_expression"
                                    )
                                })
                                .map(|object| {
                                    receiver_bytes += object.byte_range().len();
                                    match receiver_bytes <= RECEIVER_BUDGET {
                                        true => value(object, src, &scope, &parents),
                                        false => Value::Unknown,
                                    }
                                }),
                            arg_types: arguments
                                .map(|a| named(a).map(|arg| cast(arg, src, &parents)).collect())
                                .unwrap_or_default(),
                            uses: uses.of(node, body),
                        });
                    }
                }
                "object" => {
                    if let Some(owner) = literal_name(node, src, &parents) {
                        let methods = literal_methods(node, src);
                        facts.ts_literal_methods.insert(owner, methods);
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
                "public_field_definition" if typescript => {
                    if let Some(name) = private_field(node, src) {
                        let initial = node.child_by_field_name("value");
                        private.push((
                            scope.clone(),
                            name,
                            stated_value(initial, src, &scope, &parents),
                        ));
                    }
                    if let Some(name) = sealed_field(node, src) {
                        let initial = node.child_by_field_name("value");
                        let rewritten = (initial)
                            .is_some_and(|v| is_url_new(unwrapped(v), src, &parents))
                            && parents.written.contains(&format!("this.{name}"));
                        let initial = initial.map(|v| match rewritten {
                            true => Value::Unknown,
                            false => value(v, src, &scope, &parents),
                        });
                        sealed.push((scope.clone(), name, initial));
                    }
                }
                "assignment_expression"
                | "augmented_assignment_expression"
                | "update_expression"
                    if typescript =>
                {
                    for (name, plain, value) in member_writes(node, src, &scope, &parents) {
                        let at = writes.entry((name.clone(), scope.clone())).or_default();
                        at.push(plain);
                        stated.entry((name, scope.clone())).or_default().push(value);
                    }
                }
                "import_statement" => {
                    imports(node, src, &mut facts.imports);
                    if !children(node).any(|c| c.kind() == "import_clause")
                        && let Some(source) = node.child_by_field_name("source")
                    {
                        facts.loads.push(unquote(source, src));
                    }
                }
                "export_statement" => exports(node, src, &mut facts.exports),
                "return_statement" => {
                    if let Some(&at) = functions.get(&body.id()) {
                        let generator = body.kind().starts_with("generator_function")
                            || children(body).any(|n| n.kind() == "*");
                        let returned = match generator {
                            true => Value::Unknown,
                            false => named(node)
                                .next()
                                .map_or(Value::Undefined, |r| value(r, src, &scope, &parents)),
                        };
                        facts.defs[at].return_values.push(returned);
                    }
                }
                _ => {}
            }
            let named = binds(node, src, &parents);
            if let Some(target) = written_name_target(node) {
                for name in bound(target, src) {
                    let change = if target.kind() == "identifier" {
                        let op = node
                            .child_by_field_name("operator")
                            .map_or("=", |n| text(n, src));
                        node.child_by_field_name("right")
                            .filter(|right| src[right.byte_range()].contains(&b'?'))
                            .map_or(super::QueryWrite::Other, |right| {
                                super::QueryWrite::of(
                                    &value(right, src, &scope, &parents),
                                    &name,
                                    op,
                                )
                            })
                    } else {
                        super::QueryWrite::Other
                    };
                    let mut at = scope.as_str();
                    while !at.is_empty() {
                        let local = qualify(at, &name);
                        query_writes
                            .entry(local.clone())
                            .and_modify(|old: &mut super::QueryWrite| *old = old.merge(change))
                            .or_insert(change);
                        assigned.insert(local);
                        at = crate::parent(at);
                    }
                }
            }
            if node.kind() == "variable_declarator"
                && named.is_none()
                && let Some(declaration) = parents.of(node)
                && declaration.kind() == "lexical_declaration"
                && parents.of(declaration).is_some_and(|block| {
                    block.kind() == "statement_block" && parents.of(block) == Some(body)
                })
                && let Some(name) = node
                    .child_by_field_name("name")
                    .filter(|n| n.kind() == "identifier")
                && let Some(initial) = node.child_by_field_name("value")
                && !scope.is_empty()
            {
                query_locals.push((qualify(&scope, text(name, src)), scope.clone(), initial));
            }
            selector_writes(node, src, &scope, &parents, &mut facts, &mut write_bytes);
            if node.kind() == "assignment_expression"
                && named.is_some()
                && let Some(Assigned {
                    name,
                    exported: true,
                    ..
                }) = assignment(node, src, &parents)
            {
                facts.exports.push(Export::Local {
                    exported: name.strip_prefix("exports.").unwrap_or(&name).to_string(),
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
                "variable_declarator" if named.is_none() => node.child_by_field_name("name"),
                _ => None,
            };
            let params = (facts.defs.last())
                .filter(|_| is_def)
                .map_or(&[][..], |d| d.params.as_slice());
            let kept = |name: &str| {
                (params.iter()).any(|p| p.name == name || p.fields.iter().any(|f| f.name == name))
            };
            for name in pattern.map(|p| bound(p, src)).unwrap_or_default() {
                if !(BODIES.contains(&node.kind()) && kept(&name)) {
                    let qualified = qualify(&inner, &name);
                    if matches!(
                        node.kind(),
                        "variable_declarator" | "catch_clause" | "for_in_statement"
                    ) {
                        let start = pattern.map_or(node.start_byte(), |p| p.end_byte());
                        let binding_block = if parents
                            .of(node)
                            .is_some_and(|p| p.kind() == "variable_declaration")
                        {
                            body
                        } else {
                            block
                        };
                        receivers::record(
                            &mut facts.ts_locals,
                            qualified.clone(),
                            binding_block,
                            start,
                            None,
                            None,
                        );
                    }
                    facts.locals.push(qualified);
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
                let receiver =
                    receivers::local(node, name, src, &scope, &parents, &mut facts, &mut results);
                receivers::record(
                    &mut facts.ts_locals,
                    qualify(&scope, name),
                    block,
                    node.end_byte(),
                    receiver,
                    Some(facts.consts.len()),
                );
                let rewritten = is_url_new(unwrapped(*value_node), src, &parents)
                    && parents.written.contains(name.as_str());
                facts.consts.push(Const {
                    name: qualify(&scope, name),
                    value: if rewritten { Value::Unknown } else { value },
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
                    block,
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
        // ponytail: a write through an alias of `this` (`const self = this; self.f = v`) or
        // `Object.assign(this, ..)` is not seen, nor a subclass that redeclares a non-private
        // `readonly` field; see `docs/agents/ceilings/language-packs.md`.
        facts.field_writes = field_writes(private, &stated);
        for (class, name, initial) in sealed {
            let inside = |scope: &str| {
                (scope.strip_prefix(class.as_str()))
                    .is_some_and(|r| r.is_empty() || r.starts_with('.'))
            };
            let mut given = [Some(name.clone()), None].into_iter().flat_map(|name| {
                let (from, to) = (class.clone(), format!("{class}/"));
                (writes.range((name.clone(), from)..(name, to)))
                    .filter(|((_, scope), _)| inside(scope))
                    .flat_map(|((_, scope), values)| values.iter().map(move |v| (scope, v)))
            });
            let ctor = qualify(&class, "constructor");
            let held = match (initial, given.next(), given.next()) {
                (Some(value), None, _) => Some((value, None)),
                (None, Some((scope, Some(value))), None) if *scope == ctor => {
                    Some((value.clone(), Some(ctor)))
                }
                _ => None,
            };
            if let Some((value, scope)) = held {
                facts.consts.push(Const {
                    name: qualify(&class, &name),
                    value,
                    ty: None,
                    scope,
                });
            }
        }
        let mut names = HashMap::new();
        for name in &facts.locals {
            *names.entry(name.as_str()).or_insert(0) += 1;
        }
        let mut kept = HashSet::new();
        let constants: HashSet<_> = facts.consts.iter().map(|c| c.name.clone()).collect();
        for (name, scope, initial) in query_locals {
            if names.get(name.as_str()) != Some(&1) || constants.contains(&name) {
                continue;
            }
            let Some(change) = query_writes.get(&name) else {
                continue;
            };
            let mut value = value(initial, src, &scope, &parents);
            if change.keep(&mut value) {
                kept.insert(name.clone());
                facts.consts.push(Const {
                    name,
                    value,
                    ty: None,
                    scope: None,
                });
            }
        }
        facts.locals.retain(|name| !kept.contains(name));
        for def in &mut facts.defs {
            let names = (def.params.iter())
                .flat_map(|p| std::iter::once(&p.name).chain(p.fields.iter().map(|f| &f.name)));
            def.reassigned_params = names
                .filter(|name| assigned.contains(&qualify(&def.name, name)))
                .cloned()
                .collect();
        }
        facts.declarations = declarations(root, src, &facts);
        facts.references = references.into_vec();
        facts.loads.sort();
        facts.loads.dedup();
        facts.loops = super::outermost(
            root,
            &[
                "for_statement",
                "for_in_statement",
                "while_statement",
                "do_statement",
            ],
        );
        let typed: HashSet<_> = facts.receivers.iter().map(|r| r.name.as_str()).collect();
        for (name, bindings) in std::mem::take(&mut facts.ts_locals) {
            if typed.contains(name.rsplit('.').next().unwrap_or(&name)) {
                facts.ts_locals.insert(name, bindings);
            } else {
                facts.ts_record_locals.insert(name, bindings);
            }
        }
        Ok(facts)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &[
            "**/tsconfig.json",
            "**/jsconfig.json",
            "**/app-config*.yaml",
        ]
    }

    fn config_inputs<'b>(&self, configs: &[(&str, &[u8])], read: &dyn Fn(&str) -> ConfigInput<'b>) {
        crate::http::config_inputs(configs, read);
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
        let mut options: HashMap<String, Options> = HashMap::new();
        let mut nearest: HashMap<&str, &str> = HashMap::new();
        for (path, src) in configs {
            let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
            if !path.ends_with("tsconfig.json") && !path.ends_with("jsconfig.json") {
                continue;
            }
            if !path.split('/').any(|s| s == "node_modules") {
                nearest.entry(dir).or_insert(path);
                options.insert(path.to_string(), Options::read(dir, src));
            }
        }
        let by_dir: HashMap<String, Aliases> = nearest
            .into_iter()
            .map(|(dir, path)| (dir.to_string(), Aliases::of_chain(path, &options)))
            .collect();
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

type Paths = Rc<Vec<(String, Vec<String>)>>;

struct Options {
    dir: String,
    extends: Option<String>,
    base_url: Option<Option<String>>,
    paths: Option<Paths>,
}

impl Options {
    fn read(dir: &str, src: &[u8]) -> Self {
        let mut out = Self {
            dir: dir.to_string(),
            extends: None,
            base_url: None,
            paths: None,
        };
        let Some(config) = package::tsconfig_root(src) else {
            return out;
        };
        let options = &config["compilerOptions"];
        out.extends = config["extends"]
            .as_str()
            .filter(|e| e.starts_with("./") || e.starts_with("../"))
            .and_then(|e| join(dir, e))
            .map(|e| if e.ends_with(".json") { e } else { e + ".json" });
        out.base_url = options["baseUrl"].as_str().map(|b| join(dir, b));
        out.paths = options["paths"].as_object().map(|paths| {
            let paths = paths.iter().map(|(pattern, targets)| {
                let targets = targets.as_array().into_iter().flatten();
                let targets = targets.filter_map(|t| t.as_str().map(String::from));
                (pattern.clone(), targets.collect())
            });
            Rc::new(paths.collect())
        });
        out
    }
}

// ponytail: real monorepos extend one or two levels (package, then root); 8 is far past any.
const EXTENDS_DEPTH: usize = 8;

// ponytail: project `references`, an `extends` array and a base that is not named `tsconfig.json`
// or `jsconfig.json` are not followed (only those files are read).
#[derive(Default)]
struct Aliases {
    dir: String,
    base_url: Option<String>,
    paths: Paths,
}

impl Aliases {
    fn of_chain(path: &str, configs: &HashMap<String, Options>) -> Self {
        let mut chain: Vec<&Options> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        let mut next = Some(path);
        while let Some(at) = next {
            let Some((at, options)) = configs.get_key_value(at) else {
                break;
            };
            if chain.len() > EXTENDS_DEPTH || seen.contains(&at.as_str()) {
                chain.truncate(1);
                break;
            }
            seen.push(at);
            chain.push(options);
            next = options.extends.as_deref();
        }
        let Some(nearest) = chain.first() else {
            return Self::default();
        };
        let declared = chain.iter().find(|o| o.paths.is_some());
        Self {
            dir: declared.unwrap_or(nearest).dir.clone(),
            base_url: chain.iter().find_map(|o| o.base_url.clone()).flatten(),
            paths: declared.and_then(|o| o.paths.clone()).unwrap_or_default(),
        }
    }

    fn expand(&self, specifier: &str) -> Vec<String> {
        let mut best: Option<(usize, &str, &[String])> = None;
        for (pattern, targets) in self.paths.iter() {
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
    block: Node<'t>,
    scope: String,
    depth: usize,
    body: Node<'t>,
    binding: Option<(String, Range<usize>)>,
}

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
        "method_definition" if up.of(node)?.kind() == "class_body" => (Kind::Method, own()?),
        "method_definition" if in_options(node, up) => {
            (Kind::Inline, (inline(node), node.byte_range()))
        }
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

struct Parents<'t> {
    up: HashMap<usize, Node<'t>>,
    assigned: HashSet<String>,
    // ponytail: per name across the whole file, whichever function writes it; see `docs/agents/ceilings.md`.
    written: HashSet<String>,
    url_declared: bool,
    // ponytail: per file, not per scope; a file that shadows `require` once records none.
    require_declared: bool,
    local_literals: HashMap<String, HashMap<usize, Option<usize>>>,
    global_shadows: HashMap<String, Vec<Range<usize>>>,
    imported_types: HashSet<String>,
    promise_shadows: Vec<Range<usize>>,
    string_shadows: Vec<Range<usize>>,
    process_shadows: Vec<Range<usize>>,
    strict_scopes: HashSet<usize>,
    arrow_parameters: Vec<usize>,
}

impl<'t> Parents<'t> {
    fn new(root: Node<'t>, src: &[u8], module: bool) -> Self {
        let mut up = HashMap::new();
        let mut assigned = HashSet::new();
        let mut written = HashSet::new();
        let mut url_declared = false;
        let mut url_required = false;
        let mut require_declared = false;
        let mut arrow_parameters = Vec::new();
        let mut globals = globals::Bindings::default();
        let mut bindings = HashMap::<String, Vec<Node>>::new();
        let mut promise_bindings = Vec::new();
        let mut string_bindings = Vec::new();
        let mut string_writes = Vec::new();
        let mut process_bindings = Vec::new();
        let mut process_writes = Vec::new();
        let mut strict_scopes = HashSet::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let body = if node.kind() == "program" {
                Some(node)
            } else if BODIES.contains(&node.kind()) {
                node.child_by_field_name("body")
            } else {
                None
            };
            if node.kind() == "class_body"
                || body.is_some_and(|body| strict_directive(body, src))
                || node.kind() == "program"
                    && (module
                        || named(node)
                            .any(|n| matches!(n.kind(), "import_statement" | "export_statement")))
            {
                strict_scopes.insert(node.id());
            }
            if node.kind() == "binary_expression"
                && let Some(left) = node.child_by_field_name("left")
                && left.kind() == "undefined"
                && let Some(operator) = node.child_by_field_name("operator")
                && operator.kind() == ">"
                && let Some(at) = operator.start_byte().checked_sub(1)
                && src.get(at..operator.end_byte()) == Some(b"=>")
                && children(node).any(|child| {
                    child.is_error() && child.start_byte() == at && child.end_byte() == at + 1
                })
            {
                arrow_parameters.push(left.start_byte());
            }
            globals.visit(node, src, |name| {
                bindings.entry(name.into()).or_default().push(node);
            });
            if declares_promise(node, src) {
                promise_bindings.push(node);
            }
            url_declared |= shadows_runtime(node, src, "URL");
            url_required |= runtime_url_require(node, src);
            require_declared |= shadows_runtime(node, src, "require");
            if !(node.kind() == "for_in_statement" && node.child_by_field_name("kind").is_none()) {
                if shadows_runtime(node, src, "String") {
                    string_bindings.push(node);
                }
                if shadows_runtime(node, src, "process") {
                    process_bindings.push(node);
                }
            }
            members_written(node, src, &mut written);
            let target = written_name_target(node);
            let names = target.map(|t| bound(t, src)).unwrap_or_default();
            for name in &names {
                globals.write(name, node.start_byte());
            }
            if names.iter().any(|name| name == "String") {
                string_writes.push(node.start_byte());
            }
            if names.iter().any(|name| name == "process") {
                process_writes.push(node.start_byte());
            }
            assigned.extend(names);
            for child in children(node) {
                up.insert(child.id(), node);
                stack.push(child);
            }
        }
        url_declared |= assigned.contains("URL")
            || (url_required && (require_declared || assigned.contains("require")));
        let require_declared = require_declared || assigned.contains("require");
        let mut parents = Self {
            up,
            require_declared,
            assigned,
            written,
            url_declared,
            local_literals: HashMap::new(),
            global_shadows: HashMap::new(),
            imported_types: std::mem::take(&mut globals.imported_types),
            promise_shadows: Vec::new(),
            string_shadows: Vec::new(),
            process_shadows: Vec::new(),
            strict_scopes,
            arrow_parameters,
        };
        for (name, nodes) in bindings {
            let fresh = |node: Node<'t>| {
                let declaration = parents.of(node)?;
                (node.kind() == "variable_declarator"
                    && node.child_by_field_name("name")?.kind() == "identifier"
                    && declaration
                        .child_by_field_name("kind")
                        .is_some_and(|k| text(k, src) == "const")
                    && unwrapped(node.child_by_field_name("value")?).kind() == "object"
                    && literal_name(unwrapped(node.child_by_field_name("value")?), src, &parents)
                        .is_none())
                .then_some(node.end_byte())
            };
            if !nodes.iter().any(|n| fresh(*n).is_some()) {
                continue;
            }
            let mut scopes = HashMap::new();
            for node in nodes {
                let Some(scope) = parents.binding_scope(node, src) else {
                    scopes.clear();
                    break;
                };
                scopes
                    .entry(scope.id())
                    .and_modify(|v| *v = None)
                    .or_insert_with(|| fresh(node));
            }
            parents.local_literals.insert(name, scopes);
        }
        parents.global_shadows = globals.scopes(&parents, root, src);
        parents.promise_shadows = parents.runtime_shadows(promise_bindings, Vec::new(), root, src);
        parents.string_shadows = parents.runtime_shadows(string_bindings, string_writes, root, src);
        parents.process_shadows =
            parents.runtime_shadows(process_bindings, process_writes, root, src);
        parents
    }

    fn global(&self, name: &str, at: usize) -> bool {
        self.global_shadows
            .get(name)
            .is_none_or(|scopes| !runtime_hidden(scopes, at))
    }

    fn runtime_shadows(
        &self,
        bindings: Vec<Node<'t>>,
        writes: Vec<usize>,
        root: Node<'t>,
        src: &[u8],
    ) -> Vec<Range<usize>> {
        let mut scopes: Vec<_> = bindings
            .into_iter()
            .map(|node| self.runtime_scope(node, src, root))
            .collect();
        scopes.sort_by_key(|scope| scope.start);
        let mut shadows: Vec<Range<usize>> = Vec::new();
        for scope in scopes {
            if let Some(last) = shadows.last_mut()
                && last.end >= scope.start
            {
                last.end = last.end.max(scope.end);
            } else {
                shadows.push(scope);
            }
        }
        if writes.into_iter().any(|at| !runtime_hidden(&shadows, at)) {
            return vec![root.byte_range()];
        }
        shadows
    }

    fn of(&self, node: Node) -> Option<Node<'t>> {
        self.up.get(&node.id()).copied()
    }

    fn string_hidden(&self, at: usize) -> bool {
        runtime_hidden(&self.string_shadows, at)
    }

    fn local_literal(&self, object: Node<'t>, src: &[u8]) -> bool {
        let object = unwrapped(object);
        if object.kind() != "identifier" {
            return false;
        }
        let Some(scopes) = self.local_literals.get(text(object, src)) else {
            return false;
        };
        let mut scope = Some(object);
        for _ in 0..FOLD_DEPTH {
            let Some(node) = scope else { break };
            if node.kind() == "with_statement" {
                return false;
            }
            if let Some(declaration) = scopes.get(&node.id()) {
                return declaration.is_some_and(|end| end <= object.start_byte());
            }
            scope = self.of(node);
        }
        false
    }

    fn runtime_scope(&self, node: Node<'t>, src: &[u8], root: Node<'t>) -> Range<usize> {
        self.binding_scope(node, src).unwrap_or(root).byte_range()
    }

    fn binding_scope(&self, node: Node<'t>, src: &[u8]) -> Option<Node<'t>> {
        match node.kind() {
            "formal_parameters" => {
                return self.of(node);
            }
            "catch_clause"
            | "arrow_function"
            | "function_expression"
            | "function"
            | "generator_function"
            | "class" => return Some(node),
            "for_in_statement"
                if node
                    .child_by_field_name("kind")
                    .is_none_or(|kind| text(kind, src) != "var") =>
            {
                return Some(node);
            }
            _ => {}
        }
        let function = self
            .of(node)
            .is_some_and(|parent| parent.kind() == "variable_declaration")
            || node.kind() == "for_in_statement"
            || node.kind() == "function_declaration"
                && !children(node).any(|child| child.kind() == "async")
                && !self.enclosing_strict(node);
        let mut at = self.of(node);
        for _ in 0..FOLD_DEPTH {
            let Some(parent) = at else { break };
            if parent.kind() == "program"
                || (function && BODIES.contains(&parent.kind()))
                || (!function
                    && matches!(
                        parent.kind(),
                        "statement_block" | "for_statement" | "for_in_statement" | "switch_body"
                    ))
            {
                return Some(parent);
            }
            at = self.of(parent);
        }
        None
    }

    fn enclosing_strict(&self, node: Node) -> bool {
        let mut at = self.of(node);
        for _ in 0..FOLD_DEPTH {
            let Some(parent) = at else { break };
            if self.strict_scopes.contains(&parent.id()) {
                return true;
            }
            at = self.of(parent);
        }
        false
    }
}

fn runtime_hidden(shadows: &[Range<usize>], at: usize) -> bool {
    let next = shadows.partition_point(|scope| scope.start <= at);
    next > 0 && at < shadows[next - 1].end
}

fn strict_directive(body: Node, src: &[u8]) -> bool {
    for statement in named(body) {
        if matches!(statement.kind(), "comment" | "hash_bang_line") {
            continue;
        }
        if statement.kind() != "expression_statement" {
            return false;
        }
        let Some(literal) = named(statement).next().filter(|n| n.kind() == "string") else {
            return false;
        };
        if matches!(text(literal, src), "\"use strict\"" | "'use strict'") {
            return true;
        }
    }
    false
}

fn loads_from(node: Node, src: &[u8], up: &Parents) -> Option<String> {
    let function = node.child_by_field_name("function")?;
    let loads = match function.kind() {
        "import" => true,
        "identifier" => text(function, src) == "require" && !up.require_declared,
        _ => false,
    };
    let mut args = named(node.child_by_field_name("arguments")?);
    let arg = args.next().filter(|a| a.kind() == "string")?;
    (loads && args.next().is_none()).then(|| unquote(arg, src))
}

fn declares_promise(node: Node, src: &[u8]) -> bool {
    match node.kind() {
        "type_alias_declaration"
        | "interface_declaration"
        | "class_declaration"
        | "abstract_class_declaration"
        | "class"
        | "enum_declaration" => node
            .child_by_field_name("name")
            .is_some_and(|name| text(name, src) == "Promise"),
        "internal_module" | "module" => node.child_by_field_name("name").is_some_and(|name| {
            let start = name.start_byte();
            name.named_descendant_for_byte_range(start, start + 1)
                .is_some_and(|first| first.kind() == "identifier" && text(first, src) == "Promise")
        }),
        "import_statement" | "import_alias" | "import_require_clause" => {
            shadows_runtime(node, src, "Promise")
        }
        _ => false,
    }
}

fn shadows_runtime(node: Node, src: &[u8], name: &str) -> bool {
    match node.kind() {
        "class_declaration"
        | "abstract_class_declaration"
        | "class"
        | "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "function"
        | "generator_function"
        | "enum_declaration" => node
            .child_by_field_name("name")
            .is_some_and(|n| text(n, src) == name),
        "variable_declarator" => {
            !(name == "URL" && runtime_url_require(node, src))
                && (node.child_by_field_name("name"))
                    .is_some_and(|n| bound(n, src).iter().any(|n| n == name))
        }
        "formal_parameters" => bound(node, src).iter().any(|n| n == name),
        "for_in_statement" => node
            .child_by_field_name("left")
            .is_some_and(|n| bound(n, src).iter().any(|n| n == name)),
        "import_alias" | "import_require_clause" => named(node)
            .next()
            .is_some_and(|n| n.kind() == "identifier" && text(n, src) == name),
        "catch_clause" => (node.child_by_field_name("parameter"))
            .is_some_and(|n| bound(n, src).iter().any(|n| n == name)),
        "arrow_function" => {
            (node.child_by_field_name("parameter")).is_some_and(|n| text(n, src) == name)
        }
        "import_statement" => {
            let mut bindings = Vec::new();
            imports(node, src, &mut bindings);
            bindings.iter().any(|i| {
                i.local == name
                    && !(name == "URL"
                        && matches!(i.source.as_str(), "url" | "node:url")
                        && matches!(&i.name, Imported::Name(n) if n == "URL"))
            })
        }
        _ => false,
    }
}

fn runtime_url_require(node: Node, src: &[u8]) -> bool {
    if node.kind() != "variable_declarator" {
        return false;
    }
    let Some(pattern) = node
        .child_by_field_name("name")
        .filter(|n| n.kind() == "object_pattern")
    else {
        return false;
    };
    let binds_url = named(pattern).any(|n| match n.kind() {
        "shorthand_property_identifier_pattern" => text(n, src) == "URL",
        "pair_pattern" => {
            n.child_by_field_name("key")
                .is_some_and(|k| text(k, src) == "URL")
                && n.child_by_field_name("value")
                    .is_some_and(|v| v.kind() == "identifier" && text(v, src) == "URL")
        }
        _ => false,
    });
    binds_url
        && node.child_by_field_name("value").is_some_and(|v| {
            v.kind() == "call_expression"
                && v.child_by_field_name("function")
                    .is_some_and(|f| f.kind() == "identifier" && text(f, src) == "require")
                && v.child_by_field_name("arguments").is_some_and(|args| {
                    let mut args = named(args);
                    args.next().is_some_and(|a| {
                        a.kind() == "string"
                            && matches!(unquote(a, src).as_str(), "url" | "node:url")
                    }) && args.next().is_none()
                })
        })
}

pub const GLOBAL_URL: &str = "globalThis.URL";

fn is_url_new(node: Node, src: &[u8], up: &Parents) -> bool {
    node.kind() == "new_expression"
        && !up.url_declared
        && (node.child_by_field_name("constructor"))
            .is_some_and(|c| c.kind() == "identifier" && text(c, src) == "URL")
}

fn unwrapped(mut node: Node) -> Node {
    while let Some(inner) = match node.kind() {
        "parenthesized_expression"
        | "non_null_expression"
        | "as_expression"
        | "satisfies_expression" => named(node).next(),
        "type_assertion" => named(node).last(),
        _ => None,
    } {
        node = inner;
    }
    node
}

fn members_written(node: Node, src: &[u8], out: &mut HashSet<String>) {
    let object = |n: Node| match n.kind() {
        "identifier" => Some(text(n, src).to_string()),
        "member_expression" => {
            let this = n.child_by_field_name("object")?;
            let field = n.child_by_field_name("property")?;
            (this.kind() == "this").then(|| format!("this.{}", text(field, src)))
        }
        _ => None,
    };
    let mut targets = Vec::new();
    match node.kind() {
        "assignment_expression" | "augmented_assignment_expression" => {
            let left = node.child_by_field_name("left");
            targets.extend(left);
            if node.kind() == "assignment_expression"
                && left.is_some_and(|l| l.kind() == "identifier")
            {
                out.extend((node.child_by_field_name("right")).and_then(|r| object(unwrapped(r))));
            }
        }
        "update_expression" => targets.extend(node.child_by_field_name("argument")),
        "for_in_statement" if node.child_by_field_name("kind").is_none() => {
            targets.extend(node.child_by_field_name("left"));
        }
        "variable_declarator"
            if (node.child_by_field_name("name")).is_some_and(|n| n.kind() == "identifier") =>
        {
            out.extend((node.child_by_field_name("value")).and_then(|v| object(unwrapped(v))));
        }
        "call_expression"
            if (node.child_by_field_name("function"))
                .is_some_and(|f| text(f, src) == "Object.assign") =>
        {
            let first = (node.child_by_field_name("arguments")).and_then(|a| named(a).next());
            out.extend(first.and_then(|a| object(unwrapped(a))));
        }
        _ => {}
    }
    while let Some(target) = targets.pop() {
        match target.kind() {
            "member_expression" | "subscript_expression" => {
                let member = match target.kind() {
                    "member_expression" => target.child_by_field_name("property"),
                    _ => target.child_by_field_name("index"),
                };
                let member = member.map_or(String::new(), |m| match m.kind() {
                    "string" => unquote(m, src),
                    "property_identifier" | "private_property_identifier" => {
                        text(m, src).to_string()
                    }
                    _ => String::new(),
                });
                if member != "search" && member != "hash" {
                    let written = target.child_by_field_name("object").map(unwrapped);
                    out.extend(written.and_then(object));
                }
            }
            "pair_pattern" => targets.extend(target.child_by_field_name("value")),
            "assignment_pattern" | "object_assignment_pattern" => {
                targets.extend(target.child_by_field_name("left"));
            }
            "array_pattern"
            | "object_pattern"
            | "rest_pattern"
            | "parenthesized_expression"
            | "non_null_expression"
            | "as_expression"
            | "satisfies_expression" => targets.extend(named(target)),
            _ => {}
        }
    }
}

fn written_name_target(node: Node) -> Option<Node> {
    match node.kind() {
        "assignment_expression" | "augmented_assignment_expression" => {
            node.child_by_field_name("left")
        }
        "update_expression" => node
            .child_by_field_name("argument")
            .or_else(|| named(node).next()),
        "for_in_statement" if node.child_by_field_name("kind").is_none() => {
            node.child_by_field_name("left")
        }
        _ => None,
    }
}

fn selector_writes(
    node: Node,
    src: &[u8],
    scope: &str,
    up: &Parents,
    facts: &mut Facts,
    spent: &mut [usize; 2],
) {
    if facts.member_writes.is_none() {
        return;
    }
    let target =
        if node.kind() == "unary_expression" && children(node).any(|n| text(n, src) == "delete") {
            node.child_by_field_name("argument")
                .or_else(|| named(node).next())
        } else {
            written_name_target(node)
        };
    let Some(target) = target else { return };
    let mut pending = vec![target];
    while let Some(target) = pending.pop() {
        if facts.member_writes.is_none() {
            break;
        }
        let field = match target.kind() {
            "member_expression" => target
                .child_by_field_name("property")
                .map(|p| text(p, src).to_string()),
            "subscript_expression" => target
                .child_by_field_name("index")
                .filter(|n| n.kind() == "string")
                .map(|n| unquote(n, src)),
            "identifier" => continue,
            "assignment_pattern" => {
                pending.extend(target.child_by_field_name("left"));
                continue;
            }
            "pair_pattern" => {
                pending.extend(target.child_by_field_name("value"));
                continue;
            }
            _ => {
                pending.extend(named(target));
                continue;
            }
        };
        if let Some(object) = target.child_by_field_name("object") {
            let owner = |object| {
                let name = callee(object, src);
                if (name == "this" || name.starts_with("this.")) && !instance_context(target, up) {
                    "(this)".into()
                } else {
                    name
                }
            };
            let mut depth = 0;
            let mut inner = unwrapped(object);
            while !facts.ts_record_writes_incomplete
                && matches!(inner.kind(), "subscript_expression" | "member_expression")
            {
                depth += 1;
                let Some(receiver) = inner.child_by_field_name("object") else {
                    break;
                };
                let name = inner
                    .child_by_field_name("property")
                    .map(|p| text(p, src).into())
                    .or_else(|| {
                        inner
                            .child_by_field_name("index")
                            .filter(|n| n.kind() == "string")
                            .map(|n| unquote(n, src))
                    });
                if up.local_literal(receiver, src) {
                    facts
                        .ts_local_literal_writes
                        .entry(owner(receiver))
                        .or_default()
                        .insert(node.start_byte());
                }
                let write = super::MemberWrite {
                    scope: scope.into(),
                    object: owner(receiver),
                    field: name,
                    range: node.byte_range(),
                };
                spent[1] = spent[1].saturating_add(write.bytes());
                if depth > FOLD_DEPTH || spent[1] > super::MAX_BYTES {
                    facts.ts_record_writes_incomplete = true;
                    facts.ts_record_writes.clear();
                    break;
                }
                facts.ts_record_writes.push(write);
                inner = unwrapped(receiver);
            }
            if up.local_literal(object, src) {
                facts
                    .ts_local_literal_writes
                    .entry(owner(object))
                    .or_default()
                    .insert(node.start_byte());
            }
            let object = owner(object);
            facts.member_write(
                super::MemberWrite {
                    scope: scope.into(),
                    object,
                    field,
                    range: node.byte_range(),
                },
                &mut spent[0],
            );
        }
    }
}

fn single_return(node: Node) -> bool {
    if !(FUNCTIONS.contains(&node.kind()) || node.kind() == "function_declaration")
        || node.kind() == "generator_function"
        || children(node).any(|n| n.kind() == "async")
    {
        return false;
    }
    node.child_by_field_name("body").is_some_and(|body| {
        if body.kind() != "statement_block" {
            return true;
        }
        let mut statements = named(body).filter(|n| n.kind() != "comment");
        statements
            .next()
            .is_some_and(|n| n.kind() == "return_statement")
            && statements.next().is_none()
    })
}

const FUNCTIONS: &[&str] = &[
    "arrow_function",
    "function_expression",
    "function",
    "generator_function",
];

const BODIES: &[&str] = &[
    "function_declaration",
    "generator_function_declaration",
    "method_definition",
    "arrow_function",
    "function_expression",
    "function",
    "generator_function",
];

fn bound(pattern: Node, src: &[u8]) -> Vec<String> {
    (bound_nodes(pattern).into_iter())
        .map(|n| text(n, src).to_string())
        .collect()
}

fn bound_nodes(pattern: Node) -> Vec<Node> {
    let mut out = Vec::new();
    let mut stack = vec![pattern];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "identifier" | "undefined" | "shorthand_property_identifier_pattern" => out.push(node),
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

fn in_options(property: Node, up: &Parents) -> bool {
    up.of(property)
        .filter(|o| o.kind() == "object")
        .and_then(|o| up.of(o))
        .is_some_and(|a| a.kind() == "arguments")
}

fn inline(node: Node) -> String {
    format!("@{}", node.start_byte())
}

fn params(node: Node, src: &[u8], scope: &str, up: &Parents) -> Vec<Param> {
    let Some(list) = node
        .child_by_field_name("parameters")
        .or_else(|| node.child_by_field_name("parameter"))
    else {
        return Vec::new();
    };
    if matches!(list.kind(), "identifier" | "undefined") {
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
                    .filter(|n| matches!(n.kind(), "identifier" | "undefined"))
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

struct Assigned {
    name: String,
    method: bool,
    exported: bool,
}

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
        ["exports", y] | ["module", "exports", y] => (format!("exports.{y}"), false, true),
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

fn literal_owner(property: Node, src: &[u8], up: &Parents) -> Option<String> {
    literal_name(up.of(property).filter(|o| o.kind() == "object")?, src, up)
}

fn literal_name(object: Node, src: &[u8], up: &Parents) -> Option<String> {
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

fn literal_methods(object: Node, src: &[u8]) -> BTreeMap<String, bool> {
    let mut methods = BTreeMap::new();
    let mut wildcard = 0;
    for member in named(object) {
        let key = member
            .child_by_field_name("name")
            .or_else(|| member.child_by_field_name("key"))
            .or_else(|| (member.kind() == "shorthand_property_identifier").then_some(member));
        if member.kind() == "spread_element"
            || key.is_some_and(|k| k.kind() == "computed_property_name")
        {
            wildcard = member.end_byte();
            continue;
        }
        let Some(key) = key else { continue };
        let name = if key.kind() == "string" {
            unquote(key, src)
        } else {
            text(key, src).to_string()
        };
        let method = key.kind() == "property_identifier"
            && match member.kind() {
                "method_definition" => !children(member).any(|n| matches!(n.kind(), "get" | "set")),
                "pair" => member
                    .child_by_field_name("value")
                    .is_some_and(|v| FUNCTIONS.contains(&v.kind())),
                _ => false,
            };
        if let Some(position) = methods.get_mut(&name) {
            *position = 0;
        } else if method {
            methods.insert(name, member.end_byte());
        }
    }
    methods
        .into_iter()
        .map(|(name, at)| (name, at <= wildcard))
        .collect()
}

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

fn declarations(root: Node, src: &[u8], facts: &Facts) -> Vec<Declaration> {
    let mut found: Vec<(&str, Range<usize>, bool)> = Vec::new();
    let not_value = |v: Node| v.kind() == "class" || FUNCTIONS.contains(&v.kind());
    for child in named(root) {
        let mut node = child;
        if child.kind() == "export_statement" {
            if let Some(declaration) = child.child_by_field_name("declaration") {
                node = declaration;
            } else {
                if let Some(value) = child.child_by_field_name("value")
                    && children(child).any(|c| c.kind() == "default")
                    && value.kind() != "identifier"
                    && !not_value(value)
                {
                    found.push(("default", value.byte_range(), true));
                }
                continue;
            }
        }
        match node.kind() {
            "lexical_declaration"
                if node
                    .child_by_field_name("kind")
                    .is_some_and(|k| k.kind() == "const") =>
            {
                for declarator in children(node).filter(|c| c.kind() == "variable_declarator") {
                    let Some(name) = (declarator.child_by_field_name("name"))
                        .filter(|n| n.kind() == "identifier")
                    else {
                        continue;
                    };
                    if !declarator
                        .child_by_field_name("value")
                        .is_some_and(not_value)
                    {
                        found.push((text(name, src), declarator.byte_range(), true));
                    }
                }
            }
            "interface_declaration" | "type_alias_declaration" | "enum_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let value = node.kind() == "enum_declaration";
                    found.push((text(name, src), node.byte_range(), value));
                }
            }
            _ => {}
        }
    }
    let defined: HashSet<&str> = facts.defs.iter().map(|d| d.name.as_str()).collect();
    let mut kept: HashMap<&str, (Range<usize>, bool)> = HashMap::new();
    for (name, range, value) in found {
        if defined.contains(name) {
            continue;
        }
        let entry = kept.entry(name).or_insert((range.clone(), value));
        if value && !entry.1 {
            *entry = (range, value);
        }
    }
    let mut kept: Vec<_> = kept.into_iter().collect();
    kept.sort_by_key(|(_, (range, _))| range.start);
    (kept.into_iter())
        .map(|(name, (range, _))| Declaration {
            name: name.to_string(),
            range,
            hash: String::new(),
        })
        .collect()
}

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

fn value(node: Node, src: &[u8], scope: &str, up: &Parents) -> Value {
    value_at(node, src, scope, up, 0)
}

fn value_at(node: Node, src: &[u8], scope: &str, up: &Parents, depth: usize) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    let deeper = |n: Node| value_at(n, src, scope, up, depth + 1);
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
                    let value = property.child_by_field_name("value")?;
                    if key.kind() == "computed_property_name" {
                        let key = named(key).next().map_or(Value::Unknown, deeper);
                        return Some(("[]".into(), Value::List(vec![key, deeper(value)])));
                    }
                    let key = match key.kind() {
                        "string" => unquote(key, src),
                        _ => text(key, src).to_string(),
                    };
                    let value = deeper(value);
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
                "method_definition" => {
                    let key = text(property.child_by_field_name("name")?, src).to_string();
                    Some((key, Value::Unknown))
                }
                _ => None,
            });
            Value::Object(properties.collect())
        }
        "array" => Value::List(named(node).map(deeper).collect()),
        "await_expression" => named(node)
            .next()
            .map_or(Value::Unknown, |n| Value::Await(Box::new(deeper(n)))),
        "number" => Value::Scalar(text(node, src).parse().ok()),
        "true" | "false" => Value::Scalar(None),
        "identifier" | "undefined" => Value::Ref(text(node, src).to_string()),
        "member_expression" if let Some(name) = this_member(node, src, up) => Value::Ref(name),
        kind if FUNCTIONS.contains(&kind) && written_in_place(node, up) => {
            function(scope, &inline(node))
        }
        kind => match parts(node, src, up) {
            Some(parts) => Value::Str(parts),
            None if kind == "call_expression" => {
                let args = node.child_by_field_name("arguments");
                let args = args.filter(|a| a.kind() == "arguments");
                callee_node(node).map_or(Value::Unknown, |f| {
                    let receiver = (f.kind() == "member_expression")
                        .then(|| f.child_by_field_name("object"))
                        .flatten()
                        .map(|object| Box::new(deeper(object)));
                    let awaited = awaited_artifact(node);
                    let call = Value::Call(Invocation {
                        callee: callee(f, src),
                        receiver,
                        args: args.map_or(Vec::new(), |a| named(a).map(deeper).collect()),
                        kwargs: Vec::new(),
                        range: match awaited {
                            true => f.start_byte()..node.end_byte(),
                            false => node.byte_range(),
                        },
                    });
                    if awaited {
                        Value::Await(Box::new(call))
                    } else {
                        call
                    }
                })
            }
            None if kind == "new_expression" => callee_node(node).map_or(Value::Unknown, |f| {
                let args = node.child_by_field_name("arguments");
                let args = args.filter(|a| a.kind() == "arguments");
                let args: Vec<Value> = args
                    .map(|a| named(a).map(deeper))
                    .into_iter()
                    .flatten()
                    .collect();
                let name = match args.len() == 2 && is_url_new(node, src, up) {
                    true => GLOBAL_URL.to_string(),
                    false => callee(f, src),
                };
                Value::New(name, args)
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

fn cast(node: Node, src: &[u8], up: &Parents) -> Option<Ty> {
    let declared = named(node)
        .nth(1)
        .filter(|_| node.kind() == "as_expression")?;
    Some(types::ty(declared, src, up, &mut 0)).filter(|t| !matches!(t, Ty::Unknown | Ty::Schema(_)))
}

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

fn parts(node: Node, src: &[u8], up: &Parents) -> Option<Vec<Part>> {
    parts_at(node, src, up, 0)
}

fn trailing_slash<'a>(call: Node<'a>, src: &[u8]) -> Option<(Node<'a>, super::SlashStrip)> {
    let function = call.child_by_field_name("function")?;
    if text(function.child_by_field_name("property")?, src) != "replace" {
        return None;
    }
    let mut args = named(call.child_by_field_name("arguments")?).filter(|a| a.kind() != "comment");
    let pattern = args.next().filter(|a| a.kind() == "regex")?;
    let replacement = args.next()?;
    if args.next().is_some() || !matches!(text(replacement, src), "''" | "\"\"" | "``") {
        return None;
    }
    let strip = match text(pattern, src) {
        r"/\/$/" | r"/\/$/g" => super::SlashStrip::One,
        r"/\/+$/" | r"/\/+$/g" | r"/\/*$/" | r"/\/*$/g" => super::SlashStrip::All,
        _ => return None,
    };
    Some((function.child_by_field_name("object")?, strip))
}

fn parts_at(node: Node, src: &[u8], up: &Parents, depth: usize) -> Option<Vec<Part>> {
    if depth >= FOLD_DEPTH {
        return Some(vec![Part::Hole]);
    }
    let parts = |n: Node| parts_at(n, src, up, depth + 1);
    let inside = |n: Node| parts(n).unwrap_or_else(|| vec![unread(n, src)]);
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
        "ternary_expression" => {
            let yes = Value::Str(parts(node.child_by_field_name("consequence")?)?);
            let no = Value::Str(parts(node.child_by_field_name("alternative")?)?);
            match super::query_alternative(yes, no) {
                Value::Str(parts) => Some(parts),
                _ => None,
            }
        }
        "binary_expression" => {
            let left = node.child_by_field_name("left")?;
            let right = node.child_by_field_name("right")?;
            match text(node.child_by_field_name("operator")?, src) {
                "+" => {
                    let (l, r) = (parts(left), parts(right));
                    if l.is_none() && r.is_none() {
                        return None;
                    }
                    let l = l.unwrap_or_else(|| vec![unread(left, src)]);
                    let r = r.unwrap_or_else(|| vec![unread(right, src)]);
                    Some(l.into_iter().chain(r).collect())
                }
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
        "member_expression"
            if let Some(indexed) = node.child_by_field_name("object")
                && indexed.kind() == "subscript_expression"
                && let Some(field) =
                    this_member(indexed.child_by_field_name("object")?, src, up) =>
        {
            let property = text(node.child_by_field_name("property")?, src).to_owned();
            let key = indexed.child_by_field_name("index")?;
            let mut out = vec![Part::RecordStart {
                at: key.start_byte(),
                field,
                property,
            }];
            if key.kind() == "number"
                && let Ok(n) = text(key, src).parse::<u32>()
            {
                out.push(Part::Text(n.to_string()));
            } else {
                out.extend(inside(key));
            }
            out.push(Part::RecordEnd);
            Some(out)
        }
        "member_expression" if let Some(name) = this_member(node, src, up) => {
            Some(vec![Part::Ref(name)])
        }
        "parenthesized_expression"
        | "non_null_expression"
        | "as_expression"
        | "satisfies_expression" => parts(named(node).next()?),
        "call_expression" if let Some((object, strip)) = trailing_slash(node, src) => {
            Some(strip.wrap(inside(object)))
        }
        _ => url_form(node, src, up, depth)
            .or_else(|| env_read(node, src, up, depth).map(|env| vec![env])),
    }
}

fn unread(mut node: Node, src: &[u8]) -> Part {
    let mut awaited = false;
    while matches!(
        node.kind(),
        "await_expression" | "parenthesized_expression" | "non_null_expression"
    ) {
        awaited |= node.kind() == "await_expression";
        let Some(inner) = named(node).next() else {
            return Part::Hole;
        };
        node = inner;
    }
    if node.kind() != "call_expression" {
        return Part::Hole;
    }
    let Some(function) = callee_node(node) else {
        return Part::Hole;
    };
    let artifact = awaited_artifact(node);
    let start = if artifact {
        function.start_byte()
    } else {
        node.start_byte()
    };
    Part::invoke(
        callee(function, src),
        start..node.end_byte(),
        awaited || artifact,
    )
}

fn url_form(node: Node, src: &[u8], up: &Parents, depth: usize) -> Option<Vec<Part>> {
    let parts = |n: Node| parts_at(n, src, up, depth + 1);
    let written = |n: Node| n.kind() == "identifier" && up.written.contains(text(n, src));
    let url = |n: Node| match n.kind() {
        "identifier" if !written(n) => parts(n),
        "new_expression" => parts(n),
        _ => None,
    };
    fn one(arguments: Option<Node<'_>>) -> Option<Node<'_>> {
        let mut args = named(arguments.filter(|a| a.kind() == "arguments")?);
        let first = args.next()?;
        args.next().is_none().then_some(first)
    }
    match node.kind() {
        "new_expression" => {
            let argument = one(node.child_by_field_name("arguments"))?;
            parts(argument).filter(|_| is_url_new(node, src, up))
        }
        "member_expression" => {
            let property = node.child_by_field_name("property")?;
            let object = node.child_by_field_name("object")?;
            if text(property, src) == "href" {
                url(object)
            } else {
                None
            }
        }
        "call_expression" => {
            let function = callee_node(node)?;
            let arguments = node.child_by_field_name("arguments");
            match function.kind() {
                "member_expression"
                    if (function.child_by_field_name("property"))
                        .is_some_and(|p| text(p, src) == "toString")
                        && arguments.is_some_and(|a| named(a).next().is_none()) =>
                {
                    url(function.child_by_field_name("object")?)
                }
                "identifier"
                    if text(function, src) == "String"
                        && !up.string_hidden(function.start_byte()) =>
                {
                    let argument = one(arguments)?;
                    parts(argument).filter(|_| !written(argument))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn env_read(node: Node, src: &[u8], up: &Parents, depth: usize) -> Option<Part> {
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
                Some(index) if index.kind() == "identifier" => {
                    if object == "process.env"
                        && runtime_hidden(&up.process_shadows, node.start_byte())
                    {
                        return None;
                    }
                    return Some(Part::EnvKey {
                        name: text(index, src).into(),
                        reader: None,
                    });
                }
                Some(_) => return None,
                None => text(node.child_by_field_name("property")?, src).to_string(),
            };
            Some(Part::Env(name))
        }
        "call_expression" => {
            let function = callee_node(node)?;
            let method = function.child_by_field_name("property")?;
            if CONFIG_READS.contains(&text(method, src)) {
                return config_read(node, function, src);
            }
            if !STRING_METHODS.contains(&text(method, src)) {
                return None;
            }
            single_env(parts_at(
                function.child_by_field_name("object")?,
                src,
                up,
                depth + 1,
            )?)
        }
        _ => None,
    }
}

// ponytail: the one library form of this pack beside C#'s (`CODING_STANDARDS.md`); see `docs/agents/ceilings.md`.
const CONFIG_READS: &[&str] = &["getString", "getOptionalString"];

fn config_read(call: Node, function: Node, src: &[u8]) -> Option<Part> {
    let key = |call: Node| {
        let mut args =
            named(call.child_by_field_name("arguments")?).filter(|n| n.kind() != "comment");
        let arg = args.next().filter(|k| k.kind() == "string")?;
        args.next().is_none().then(|| unquote(arg, src))
    };
    let mut keys = vec![key(call)?];
    let mut base = function.child_by_field_name("object")?;
    while base.kind() == "call_expression" {
        let method = callee_node(base)?;
        if !method
            .child_by_field_name("property")
            .is_some_and(|p| matches!(text(p, src), "getConfig" | "getOptionalConfig"))
        {
            break;
        }
        if keys.len() >= FOLD_DEPTH {
            return None;
        }
        keys.push(key(base)?);
        base = method.child_by_field_name("object")?;
    }
    keys.reverse();
    Some(Part::Read {
        scope: std::sync::Arc::from(""),
        callee: callee(function, src),
        key: keys.join("."),
        parameter: false,
        then: Vec::new(),
    })
}

// ponytail: real code nests a `this` a few dozen nodes inside its method; the climb must stay
// cheap whatever the file. See `docs/agents/ceilings/language-packs.md`.
const THIS_CLIMB: usize = 256;

fn this_member(node: Node, src: &[u8], up: &Parents) -> Option<String> {
    let object = node.child_by_field_name("object")?;
    let property = node.child_by_field_name("property")?;
    let named = matches!(
        property.kind(),
        "property_identifier" | "private_property_identifier"
    );
    if object.kind() != "this" || !named {
        return None;
    }
    instance_context(node, up).then(|| format!("this.{}", text(property, src)))
}

fn instance_context(node: Node, up: &Parents) -> bool {
    let mut at = node;
    for _ in 0..THIS_CLIMB {
        let Some(parent) = up.of(at) else {
            return false;
        };
        match parent.kind() {
            "class_body" => {
                return at.kind() != "class_static_block"
                    && !children(at).any(|c| c.kind() == "static");
            }
            "method_definition" if up.of(parent).is_none_or(|b| b.kind() != "class_body") => {
                return false;
            }
            kind if kind != "arrow_function" && FUNCTIONS.contains(&kind) => return false,
            "function_declaration" | "generator_function_declaration" => return false,
            _ => at = parent,
        }
    }
    false
}

fn sealed_field(node: Node, src: &[u8]) -> Option<String> {
    let (name, private, readonly) = instance_field(node, src)?;
    (private || readonly).then_some(name)
}

fn private_field(node: Node, src: &[u8]) -> Option<String> {
    let (name, private, _) = instance_field(node, src)?;
    private.then_some(name)
}

fn instance_field(node: Node, src: &[u8]) -> Option<(String, bool, bool)> {
    let name = node.child_by_field_name("name")?;
    let mut private = name.kind() == "private_property_identifier";
    let mut readonly = false;
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "readonly" => readonly = true,
            "accessibility_modifier" if text(child, src) == "private" => private = true,
            "static" | "abstract" => return None,
            _ => {}
        }
    }
    let plain = matches!(
        name.kind(),
        "property_identifier" | "private_property_identifier"
    );
    plain.then(|| (text(name, src).to_string(), private, readonly))
}

type Stated = Option<Option<Value>>;

fn stated_value(right: Option<Node>, src: &[u8], scope: &str, up: &Parents) -> Stated {
    match right {
        None => Some(None),
        Some(v) if v.kind() == "null" => Some(None),
        Some(v) if FUNCTIONS.contains(&v.kind()) || v.kind() == "class" => None,
        Some(v) => Some(Some(value(v, src, scope, up))),
    }
}

fn field_writes(
    private: Vec<(String, String, Stated)>,
    stated: &BTreeMap<(Option<String>, String), Vec<Stated>>,
) -> Vec<FieldWrites> {
    let mut out = Vec::new();
    for (class, name, initial) in private {
        let inside = |scope: &str| {
            (scope.strip_prefix(class.as_str())).is_some_and(|r| r.is_empty() || r.starts_with('.'))
        };
        let written = [Some(name.clone()), None].into_iter().flat_map(|key| {
            let (from, to) = (class.clone(), format!("{class}/"));
            (stated.range((key.clone(), from)..(key, to)))
                .filter(|((_, scope), _)| inside(scope))
                .flat_map(|((_, scope), values)| values.iter().map(move |v| (scope, v)))
        });
        let (mut values, mut known) = (Vec::new(), true);
        for (scope, given) in std::iter::once((&class, &initial)).chain(written) {
            match given {
                None => known = false,
                Some(None) => {}
                Some(Some(value)) => values.push((scope.clone(), value.clone())),
            }
        }
        if known && !values.is_empty() {
            let name = qualify(&class, &name);
            out.push(FieldWrites { name, values });
        }
    }
    out
}

fn member_writes(
    node: Node,
    src: &[u8],
    scope: &str,
    up: &Parents,
) -> Vec<(Option<String>, Option<Value>, Stated)> {
    let target = match node.kind() {
        "update_expression" => node.child_by_field_name("argument"),
        _ => node.child_by_field_name("left"),
    };
    let mut pending: Vec<Node> = target.into_iter().collect();
    let mut out = Vec::new();
    while let Some(t) = pending.pop() {
        let object = t.child_by_field_name("object");
        let on_this = object.is_some_and(|o| o.kind() == "this");
        let name = match t.kind() {
            "member_expression" => t.child_by_field_name("property").map(|p| text(p, src)),
            "subscript_expression" => match t.child_by_field_name("index") {
                Some(index) if index.kind() == "string" => {
                    out.push((Some(unquote(index, src)), None, None));
                    continue;
                }
                _ if on_this => None,
                _ => continue,
            },
            _ => {
                pending.extend(named(t));
                continue;
            }
        };
        let plain = (node.kind() == "assignment_expression"
            && Some(t) == target
            && on_this
            && in_constructor_body(node, src, up))
        .then(|| node.child_by_field_name("right"))
        .flatten()
        .filter(|v| !FUNCTIONS.contains(&v.kind()) && v.kind() != "class")
        .map(|v| {
            let rewritten = is_url_new(unwrapped(v), src, up)
                && name.is_some_and(|n| up.written.contains(&format!("this.{n}")));
            match rewritten {
                true => Value::Unknown,
                false => value(v, src, scope, up),
            }
        });
        let statement = node.kind() == "assignment_expression"
            && Some(t) == target
            && on_this
            && up
                .of(node)
                .is_some_and(|s| s.kind() == "expression_statement");
        let stated = match node.child_by_field_name("right") {
            Some(right) if statement && name.is_some() => stated_value(Some(right), src, scope, up),
            _ => None,
        };
        out.push((name.map(str::to_string), plain, stated));
    }
    out
}

fn in_constructor_body(node: Node, src: &[u8], up: &Parents) -> bool {
    let statement = up.of(node).filter(|s| s.kind() == "expression_statement");
    let block = statement
        .and_then(|s| up.of(s))
        .filter(|b| b.kind() == "statement_block");
    let method = block.and_then(|b| up.of(b)).filter(|m| {
        m.kind() == "method_definition"
            && m.child_by_field_name("name")
                .is_some_and(|n| text(n, src) == "constructor")
    });
    method
        .and_then(|m| up.of(m))
        .is_some_and(|c| c.kind() == "class_body")
}

fn single_env(parts: Vec<Part>) -> Option<Part> {
    match <[Part; 1]>::try_from(parts) {
        Ok([env @ Part::Env(_)]) => Some(env),
        _ => None,
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn unquote(node: Node, src: &[u8]) -> String {
    let quoted = text(node, src);
    quoted
        .get(1..quoted.len().saturating_sub(1))
        .unwrap_or("")
        .to_string()
}

pub(crate) fn dynamic_import(value: &Value) -> Option<&str> {
    let Value::Call(call) = value else {
        return None;
    };
    match (call.callee.as_str(), call.args.as_slice()) {
        ("(import)", [Value::Str(parts)]) => match parts.as_slice() {
            [Part::Text(specifier)] => Some(specifier),
            _ => None,
        },
        _ => None,
    }
}

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

fn awaited_artifact(node: Node) -> bool {
    node.child_by_field_name("type_arguments").is_some()
        && node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("constructor"))
            .is_some_and(|f| f.kind() == "await_expression")
}

fn callee(node: Node, src: &[u8]) -> String {
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
    use crate::lang::Use;

    #[test]
    fn only_the_global_promise_supplies_an_awaited_output() {
        for (declaration, global) in [
            ("interface Promise<T> {}", false),
            ("abstract class Promise<T> {}", false),
            ("enum Promise { A }", false),
            ("namespace Promise {}", false),
            ("namespace Promise.Inner.Other {}", false),
            ("import type { Other as Promise } from 'some-lib';", false),
            ("import * as Promise from 'some-lib';", false),
            ("import Promise = require('some-lib');", false),
            ("import { Promise as Other } from 'some-lib';", true),
            ("function other() { type Promise<T> = T; }", true),
        ] {
            let source = format!("function open(): Promise<Client> {{}} {declaration}");
            let facts = TsJs.parse("a.ts", source.as_bytes()).unwrap();
            let open = facts.defs.iter().find(|def| def.name == "open").unwrap();
            assert_eq!(open.output.is_some(), global, "{declaration}");
        }
        let source = b"function open<Promise>(): Promise<Client> {}";
        assert!(TsJs.parse("a.ts", source).unwrap().defs[0].output.is_none());
    }

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
        assert!(modules("bad/a.ts", "@/b").is_empty());
        assert!(!has("app/node_modules/p/a.ts", "app/node_modules/p/x/b.ts"));
    }

    fn aliased(configs: &[(&str, &str)], from: &str) -> Vec<String> {
        let configs: Vec<(&str, &[u8])> = configs.iter().map(|(p, j)| (*p, j.as_bytes())).collect();
        let modules = TsJs.modules(&configs, &BTreeMap::new()).unwrap();
        modules(from, "@/b").iter().cloned().collect()
    }

    #[test]
    fn an_extends_inside_the_repository_lends_its_paths_and_the_nearest_option_wins() {
        let root = r#"{"compilerOptions": {"paths": {"@/*": ["lib/*"]}}}"#;
        let base = r#"{"extends": "../../tsconfig", "compilerOptions": {"baseUrl": "."}}"#;
        let configs = [
            ("tsconfig.json", root),
            (
                "packages/a/tsconfig.json",
                r#"{"extends": "../../tsconfig.json"}"#,
            ),
            ("packages/b/tsconfig.json", base),
            (
                "packages/c/tsconfig.json",
                r#"{"extends": "../a/tsconfig.json", "compilerOptions": {"paths": {"@/*": ["c/*"]}}}"#,
            ),
            (
                "packages/d/tsconfig.json",
                r#"{"extends": "@tsconfig/node18"}"#,
            ),
            (
                "packages/e/tsconfig.json",
                r#"{"extends": "../../../tsconfig.json"}"#,
            ),
            (
                "packages/f/tsconfig.json",
                r#"{"extends": "/tsconfig.json"}"#,
            ),
        ];
        let has = |from: &str, file: &str| aliased(&configs, from).contains(&file.to_string());
        assert!(has("packages/a/src/x.ts", "lib/b.ts"));
        assert!(has("packages/b/src/x.ts", "packages/b/lib/b.ts"));
        assert!(has("packages/b/src/x.ts", "packages/b/@/b.ts"));
        assert!(has("packages/c/src/x.ts", "packages/c/c/b.ts"));
        assert!(!has("packages/c/src/x.ts", "lib/b.ts"));
        for dir in ["d", "e", "f"] {
            assert!(aliased(&configs, &format!("packages/{dir}/x.ts")).is_empty());
        }
    }

    #[test]
    fn an_extends_chain_past_the_limit_or_in_a_cycle_reads_as_the_nearest_config_alone() {
        let root = r#"{"compilerOptions": {"paths": {"@/*": ["lib/*"]}}}"#.to_string();
        let chain = |links: usize| {
            let mut configs = vec![("tsconfig.json".to_string(), root.clone())];
            for i in 0..links {
                let up = if i + 1 == links {
                    "../tsconfig.json".into()
                } else {
                    format!("../c{}/tsconfig.json", i + 1)
                };
                configs.push((
                    format!("c{i}/tsconfig.json"),
                    format!(r#"{{"extends": "{up}"}}"#),
                ));
            }
            configs
        };
        let reaches = |configs: &[(String, String)]| {
            let configs: Vec<(&str, &str)> = configs
                .iter()
                .map(|(p, j)| (p.as_str(), j.as_str()))
                .collect();
            aliased(&configs, "c0/x.ts").contains(&"lib/b.ts".to_string())
        };
        assert!(reaches(&chain(EXTENDS_DEPTH)));
        assert!(!reaches(&chain(EXTENDS_DEPTH + 1)));
        assert!(!reaches(&chain(EXTENDS_DEPTH * 1000)));
        let own = r#"{"extends": "../c1/tsconfig.json", "compilerOptions": {"paths": {"@/*": ["own/*"]}}}"#;
        let cycle = [
            ("c0/tsconfig.json", own),
            (
                "c1/tsconfig.json",
                r#"{"extends": "../c0/tsconfig.json", "compilerOptions": {"baseUrl": "x"}}"#,
            ),
        ];
        let found = aliased(&cycle, "c0/x.ts");
        assert!(found.contains(&"c0/own/b.ts".to_string()), "{found:?}");
        assert!(!found.iter().any(|f| f.starts_with("c1/")), "{found:?}");
        let self_extends = [("c0/tsconfig.json", r#"{"extends": "./tsconfig.json"}"#)];
        assert!(aliased(&self_extends, "c0/x.ts").is_empty());
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
        assert_eq!(held.ty, None);
        let args: Vec<_> = facts.calls.iter().filter(|c| c.callee == "fetch").collect();
        assert!(matches!(args[0].args.as_slice(), [Value::Ref(name)] if name == "url"));
        assert_eq!(args[0].arg_types, [None]);
        assert!(matches!(args[1].args.as_slice(), [Value::Str(_)]));
    }

    fn fetched_parts(path: &'static str, src: &str) -> Vec<Part> {
        use crate::resolve::{Code, Resolver};
        let facts = TsJs.parse(path, src.as_bytes()).unwrap();
        let files = BTreeMap::from([(path, &facts)]);
        let modules = TsJs.modules(&[], &BTreeMap::new()).unwrap();
        let resolver = Resolver::new(&files, modules, &TsJs);
        let code = Code::new(&resolver, &files, &TsJs);
        let [call] = &facts
            .calls
            .iter()
            .filter(|c| c.callee == "fetch")
            .collect::<Vec<_>>()[..]
        else {
            panic!("one fetch in {src}");
        };
        match code.value(path, &call.scope, &call.args[0]) {
            Value::Str(parts) => parts,
            _ => panic!("no string in {src}"),
        }
    }

    #[test]
    fn a_readonly_or_private_field_written_once_is_a_constant_of_its_class() {
        let env = || vec![Part::Env("API".into()), Part::Text("/o".into())];
        let class = |members: &str, field: &str| {
            let method = "list() { return fetch(`${this.F}/o`); }";
            format!("class C {{\n{members}\n{method}\n}}\n").replace('F', field)
        };
        let assigned = "constructor() { this.F = process.env.API; }";
        for (members, field) in [
            (format!("private readonly F: string;\n{assigned}"), "base"),
            (format!("readonly F: string;\n{assigned}"), "base"),
            (format!("private F: string;\n{assigned}"), "base"),
            (format!("F: string;\n{assigned}"), "#base"),
            ("private readonly F = process.env.API;".to_string(), "base"),
            ("F = process.env.API;".to_string(), "#base"),
        ] {
            let src = class(&members, field);
            assert_eq!(fetched_parts("a.ts", &src), env(), "{src}");
        }
        let src = "class C {\nprivate readonly base: string;\n\
                   constructor(config: Config) { this.base = config.getString('a.b'); }\n\
                   list() { return fetch(this.base + '/o'); }\n}\n";
        let parts = fetched_parts("a.ts", src);
        assert!(
            matches!(&parts[..], [Part::Read { key, .. }, Part::Text(t)] if key == "a.b" && t == "/o"),
            "{parts:?}"
        );

        let hole = || vec![Part::Hole, Part::Text("/o".into())];
        for members in [
            "base = process.env.API;",
            "public base: string;\nconstructor() { this.base = process.env.API; }",
            "private readonly base = process.env.API;\nconstructor() { this.base = process.env.API; }",
            "private base: string;\nconstructor() { this.base = process.env.API; this.base = process.env.API; }",
            "private base: string;\nconstructor(x: boolean) { if (x) { this.base = process.env.API; } }",
            "private base = process.env.API;\nreset() { this.base += '/v2'; }",
            "private base = process.env.API;\nreset(o: C) { o.base = 'x'; }",
            "private base = process.env.API;\nreset() { [this.base] = ['x']; }",
            "private base = process.env.API;\nreset() { this['base'] = 'x'; }",
            "private base: string;\ninit() { this.base = process.env.API; }",
            "private base: string;\nconstructor() { const f = () => { this.base = process.env.API; }; f(); }",
            "constructor(private readonly base: string) {}",
            "private readonly base: string;\nconstructor(base: string) { this.base = base; }",
            "static readonly base = process.env.API;",
        ] {
            let src = class(members, "base");
            assert_eq!(fetched_parts("a.ts", &src), hole(), "{src}");
        }
        let sealed = "private readonly base = process.env.API;";
        let with = |method: &str| format!("class C {{\n{sealed}\n{method}\n}}\n");
        let arrow = with("list() { return [1].map(() => fetch(`${this.base}/o`)); }");
        assert_eq!(fetched_parts("a.ts", &arrow), env(), "{arrow}");
        for method in [
            "list() { return [1].map(function () { return fetch(`${this.base}/o`); }); }",
            "list() { return run({ go() { return fetch(`${this.base}/o`); } }); }",
            "static list() { return fetch(`${this.base}/o`); }",
            "list() { class D { go() { return fetch(`${this.base}/o`); } } }",
        ] {
            let src = with(method);
            assert_eq!(fetched_parts("a.ts", &src), hole(), "{src}");
        }
        let js = class("F = process.env.API;", "#base");
        assert_eq!(fetched_parts("a.js", &js), hole(), "{js}");
    }

    #[test]
    fn a_this_nested_past_the_climb_reads_nothing() {
        let nested = |n: usize| {
            let arrows = "() => ".repeat(n);
            format!(
                "class C {{\nprivate readonly base = process.env.API;\n\
                 list() {{ return {arrows}fetch(`${{this.base}}/o`); }}\n}}\n"
            )
        };
        let env = vec![Part::Env("API".into()), Part::Text("/o".into())];
        assert_eq!(fetched_parts("a.ts", &nested(THIS_CLIMB / 4)), env);
        let hole = vec![Part::Hole, Part::Text("/o".into())];
        assert_eq!(fetched_parts("a.ts", &nested(THIS_CLIMB * 2)), hole);
    }

    #[test]
    fn sealed_fields_of_many_classes_are_read_in_linear_time() {
        let source: String = (0..8192)
            .map(|n| {
                format!(
                    "class C{n} {{ private readonly base: string; \
                     constructor() {{ this.base = process.env.API; }} }}\n"
                )
            })
            .collect();
        let start = std::time::Instant::now();
        let facts = TsJs.parse("a.ts", source.as_bytes()).unwrap();
        let took = start.elapsed();
        assert_eq!(
            facts.consts.iter().filter(|c| c.scope.is_some()).count(),
            8192
        );
        assert!(took.as_secs() < 40, "{took:?}");
    }

    fn folded(value: &Value) -> Option<String> {
        let Value::Str(parts) = value else {
            return None;
        };
        Some(
            parts
                .iter()
                .map(|p| match p {
                    Part::Text(t) => t.clone(),
                    Part::Env(e) => format!("<{e}>"),
                    Part::Ref(_) => "?".into(),
                    _ => "_".into(),
                })
                .collect(),
        )
    }

    fn fetched(src: &str) -> Vec<Option<String>> {
        let facts = parse(src);
        let calls = facts.calls.iter().filter(|c| c.callee == "fetch");
        calls.map(|c| folded(&c.args[0])).collect()
    }

    #[test]
    fn a_url_of_one_argument_is_the_string_it_parses() {
        let got = fetched(
            "fetch(new URL('https://h.example.com/a'));\n\
             fetch(new URL(`${process.env.API_URL}/a`).toString());\n\
             fetch(new URL('https://h.example.com/b').href);\n\
             fetch(String(new URL('https://h.example.com/c')));\n\
             fetch(`${new URL('https://h.example.com/d')}/e`);\n\
             const u = new URL(base); fetch(u.toString()); fetch(u.href); fetch(String(u));\n\
             fetch(`${u}`);\n",
        );
        let host = |p: &str| Some(format!("https://h.example.com/{p}"));
        assert_eq!(got[0], host("a"));
        assert_eq!(got[1], Some("<API_URL>/a".into()));
        assert_eq!(got[2], host("b"));
        assert_eq!(got[3], host("c"));
        assert_eq!(got[4], Some("https://h.example.com/d/e".into()));
        assert!(
            got[5..].iter().all(|g| g.as_deref() == Some("?")),
            "{got:?}"
        );
    }

    #[test]
    fn other_shapes_of_those_names_stay_calls() {
        let facts = parse(
            "fetch(n.toString(16));\n\
             fetch(new URL('/a', base));\n\
             fetch(new URL(...parts));\n\
             fetch(o.p.href);\n\
             fetch(String());\n\
             fetch(String(a, b));\n",
        );
        let args: Vec<_> = (facts.calls.iter())
            .filter(|c| c.callee == "fetch")
            .map(|c| &c.args[0])
            .collect();
        assert!(matches!(args[0], Value::Call(i) if i.callee == "n.toString"));
        assert!(matches!(args[1], Value::New(c, a) if c == GLOBAL_URL && a.len() == 2));
        assert!(matches!(args[2], Value::New(c, _) if c == "URL"));
        assert!(matches!(args[3], Value::Unknown));
        assert!(matches!(args[4], Value::Call(_)));
        assert!(matches!(args[5], Value::Call(_)));
    }

    #[test]
    fn a_member_write_other_than_the_query_leaves_the_url_unknown() {
        let unknown = |src: &str| {
            let facts = parse(src);
            let c = facts.consts.iter().find(|c| c.name == "u").unwrap();
            matches!(c.value, Value::Unknown)
        };
        let url = "const u = new URL('https://h.example.com/a');\n";
        assert!(!unknown(url));
        assert!(!unknown(&format!("{url}u.searchParams.set('k', 'v');")));
        assert!(!unknown(&format!("{url}u.searchParams.append('k', 'v');")));
        assert!(!unknown(&format!(
            "{url}u.search = '?k=v'; u.hash += '#h'; u['hash'] = 'x';"
        )));
        for write in [
            "u.pathname = '/b';",
            "u.pathname += '/b';",
            "u.host = 'other';",
            "u.href = 'https://o.example.com/';",
            "u['pathname'] = '/b';",
            "u[key] = '/b';",
            "[u.pathname] = ['/b'];",
            "({ p: u.pathname } = o);",
            "({ p: u.pathname = '/b' } = o);",
            "for (u.pathname of paths) {}",
            "u.port++;",
            "Object.assign(u, { pathname: '/b' });",
            "const v = u; v.pathname = '/b';",
            "let v; v = u; v.pathname = '/b';",
            "(u).pathname = '/b';",
            "u!.pathname = '/b';",
            "(u as URL).port++;",
        ] {
            assert!(unknown(&format!("{url}{write}")), "{write}");
        }
        for read in ["const { pathname } = u;", "({ pathname: p } = u);"] {
            assert!(!unknown(&format!("{url}{read}")), "{read}");
        }
        for made in [
            "(new URL('https://h.example.com/a'))",
            "new URL('https://h.example.com/a') as URL",
            "new URL('https://h.example.com/a')!",
            "new URL('https://h.example.com/a') satisfies URL",
            "<URL>new URL('https://h.example.com/a')",
        ] {
            assert!(
                unknown(&format!("const u = {made};\nu.pathname = '/b';")),
                "{made}"
            );
        }
        let field = |write: &str| {
            let facts = parse(&format!(
                "class C {{\n  private readonly url = new URL('https://h.example.com/a');\n  \
                 go() {{ {write} fetch(this.url.href); }}\n}}\n"
            ));
            (facts.consts.iter())
                .find(|c| c.name == "C.url")
                .is_some_and(|c| !matches!(c.value, Value::Unknown))
        };
        assert!(field(""));
        assert!(field("this.url.search = '?k';"));
        assert!(!field("this.url.pathname = '/b';"));
        assert!(!field("Object.assign(this.url, { pathname: '/b' });"));
        assert!(!field("(this.url).pathname = '/b';"));
        let built = |write: &str| {
            let facts = parse(&format!(
                "class C {{\n  private readonly url: URL;\n  constructor() {{ this.url = \
                 new URL('https://h.example.com/a'); }}\n  go() {{ {write} fetch(this.url.href); \
                 }}\n}}\n"
            ));
            (facts.consts.iter())
                .find(|c| c.name == "C.url")
                .is_some_and(|c| !matches!(c.value, Value::Unknown))
        };
        assert!(built(""));
        assert!(!built("this.url.pathname = '/b';"));
        let facts = parse(&format!(
            "{url}u.pathname = '/b'; fetch(u.toString()); fetch(u.href);"
        ));
        let args: Vec<_> = (facts.calls.iter())
            .filter(|c| c.callee == "fetch")
            .map(|c| &c.args[0])
            .collect();
        assert!(matches!(args[0], Value::Call(i) if i.callee == "u.toString"));
        assert!(matches!(args[1], Value::Member(o, p) if o == "u" && p == "href"));
    }

    #[test]
    fn a_url_the_file_declares_or_imports_from_the_repository_is_no_global() {
        for head in [
            "class URL { constructor(p) {} }\n",
            "function URL(p) {}\n",
            "const URL = makeUrl();\n",
            "import { URL } from './url';\n",
            "import { Other as URL } from '../url';\n",
            "import URL from './url';\n",
            "function f(URL) { return URL; }\n",
            "const f = (URL) => URL;\n",
            "try { run(); } catch (URL) {}\n",
        ] {
            let got = fetched(&format!("{head}fetch(new URL('https://h.example.com/a'));"));
            assert_eq!(got, [None], "{head}");
        }
        let got = fetched(
            "const { URL } = require('node:url');\nfetch(new URL('https://h.example.com/a'));",
        );
        assert_eq!(got, [Some("https://h.example.com/a".into())]);
        let got =
            fetched("import { URL } from 'node:url';\nfetch(new URL('https://h.example.com/a'));");
        assert_eq!(got, [Some("https://h.example.com/a".into())]);
    }

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

    #[test]
    fn a_bare_import_is_a_load() {
        let facts = parse("import 'x/polyfill';\nimport \"./side.css\";\n");
        assert_eq!(facts.loads, ["./side.css", "x/polyfill"]);
        assert!(facts.imports.is_empty());
    }

    #[test]
    fn a_require_of_a_string_is_a_load() {
        assert_eq!(parse("const a = require('lodash');").loads, ["lodash"]);
        assert!(parse("const a = require(name);").loads.is_empty());
        assert!(parse("const a = require('a', 'b');").loads.is_empty());
        assert!(parse("const a = require(`a${b}`);").loads.is_empty());
        assert!(parse("const a = x.require('a');").loads.is_empty());
    }

    #[test]
    fn a_shadowed_require_is_no_load() {
        assert!(
            parse("function f(require) { require('x') }")
                .loads
                .is_empty()
        );
        assert!(parse("require = load; require('x');").loads.is_empty());
    }

    #[test]
    fn a_load_makes_no_binding() {
        let rows = |src: &str| {
            let facts = parse(src);
            let calls: Vec<(String, String)> = (facts.calls.iter())
                .map(|c| (c.scope.clone(), c.callee.clone()))
                .collect();
            let imports: Vec<(String, String)> = (facts.imports.iter())
                .map(|i| (i.local.clone(), i.source.clone()))
                .collect();
            (calls, imports)
        };
        assert_eq!(
            rows("import y from 'y';\nimport 'x';\nfunction f() { x(); y(); }\n"),
            rows("import y from 'y';\nfunction f() { x(); y(); }\n"),
        );
    }

    #[test]
    fn loaded_unites_every_form() {
        let facts = parse(
            "import a from 'a'; export * from 'b'; export { q } from 'q'; import('c');\n\
             import 'd'; require('e'); import('d');\n",
        );
        let loaded: Vec<&str> = crate::lang::loaded(&facts).into_iter().collect();
        assert_eq!(loaded, ["a", "b", "c", "d", "e", "q"]);
        let stored: Facts = serde_json::from_str(&serde_json::to_string(&facts).unwrap()).unwrap();
        assert_eq!(crate::lang::loaded(&stored), crate::lang::loaded(&facts));
    }

    fn uses_of(path: &str, src: &str) -> Vec<(String, String, &'static str)> {
        let facts = TsJs.parse(path, src.as_bytes()).unwrap();
        let mut got: Vec<_> = (facts.references.iter())
            .map(|r| (r.scope.clone(), r.name.clone(), r.kind.as_str()))
            .collect();
        got.sort();
        got
    }

    fn expected(rows: &[(&str, &str, &'static str)]) -> Vec<(String, String, &'static str)> {
        let mut rows: Vec<_> = (rows.iter())
            .map(|(s, n, k)| (s.to_string(), n.to_string(), *k))
            .collect();
        rows.sort();
        rows
    }

    #[test]
    fn non_call_uses_are_recorded_once_per_scope_name_and_kind() {
        let src = "function Header() { return null }\n\
                   export function LoginPage(t: Theme) {\n\
                     config.url;\n\
                     client.get();\n\
                     Button();\n\
                     const v = 1; f(v);\n\
                     const o = { plugin };\n\
                     return <div><Header/><Header/><Ui.Card/><Button onClick={handler}/></div>;\n\
                   }\n\
                   class Page extends Base implements Shape {}\n\
                   interface P extends Q {}\n\
                   type T = typeof tokens;\n\
                   export { X } from '@scope/b';\n\
                   export * from '@scope/s';\n";
        assert_eq!(
            uses_of("a.tsx", src),
            expected(&[
                ("", "*", "reexport"),
                ("", "X", "reexport"),
                ("", "Q", "extends"),
                ("", "tokens", "type"),
                ("LoginPage", "Theme", "type"),
                ("LoginPage", "Header", "jsx"),
                ("LoginPage", "Ui.Card", "jsx"),
                ("LoginPage", "Button", "jsx"),
                ("LoginPage", "handler", "value"),
                ("LoginPage", "config.url", "member"),
                ("LoginPage", "client", "member"),
                ("LoginPage", "plugin", "value"),
                ("LoginPage", "v", "value"),
                ("Page", "Base", "extends"),
                ("Page", "Shape", "extends"),
            ])
        );
        let facts = TsJs.parse("a.tsx", src.as_bytes()).unwrap();
        let header = (facts.references.iter())
            .find(|r| r.name == "Header" && r.kind == Use::Jsx)
            .unwrap();
        assert_eq!(header.range.start, src.find("Header/>").unwrap());
    }

    #[test]
    fn type_parameters_and_written_names_are_no_uses() {
        let src = "type Box<T> = { v: T };\n\
                   function id<U>(u: U): U { return u }\n\
                   type Inner<X> = X extends Array<infer E> ? E : never;\n\
                   type Keys<O> = { [K in keyof O]: K };\n\
                   function w() { target.field = 1; count = 2; for (const i of list) {} }\n\
                   type Q = Ns.Model;\n";
        assert_eq!(
            uses_of("a.ts", src),
            expected(&[
                ("", "Array", "type"),
                ("", "Ns.Model", "type"),
                ("id", "u", "value"),
                ("w", "list", "value"),
            ])
        );
    }

    #[test]
    fn a_type_declared_inside_a_block_hides_the_top_level_one() {
        let src = "interface Props { a: number }\n\
                   enum E { A }\n\
                   function f() { interface Props { b: string }; const p: Props = null; return p }\n\
                   function h() { enum E { B }; return E.B }\n\
                   function g(q: Props) { return E.A }\n";
        assert_eq!(
            uses_of("a.ts", src),
            expected(&[
                ("f", "p", "value"),
                ("g", "E.A", "member"),
                ("g", "Props", "type"),
            ])
        );
    }

    #[test]
    fn dynamic_imports_record_the_member_they_take() {
        let src = "const Lazy = lazy(() => import('@scope/a').then(m => ({ default: m.Page })));\n\
                   import('./local').then(m => m.helper);\n\
                   async function g(s: string) {\n\
                     const { X, Y: z } = await import('@scope/a');\n\
                     (await import('@scope/a')).W;\n\
                     import(`@scope/a${s}`).then(m => m.Q);\n\
                   }\n";
        let facts = TsJs.parse("a.tsx", src.as_bytes()).unwrap();
        let mut got: Vec<_> = (facts.references.iter())
            .filter(|r| r.kind == Use::Dynamic)
            .map(|r| (r.name.as_str(), r.from.as_deref().unwrap()))
            .collect();
        got.sort();
        assert_eq!(
            got,
            [
                ("Page", "@scope/a"),
                ("W", "@scope/a"),
                ("X", "@scope/a"),
                ("Y", "@scope/a"),
                ("helper", "./local"),
            ]
        );
        assert!(
            (facts.references.iter())
                .all(|r| matches!(r.kind, Use::Dynamic | Use::Reexport) == r.from.is_some())
        );
    }

    #[test]
    fn js_and_jsx_files_record_their_uses() {
        let src = "class A extends Base {}\nfunction C() { return <Card/> }\n";
        assert_eq!(
            uses_of("a.jsx", src),
            expected(&[("A", "Base", "extends"), ("C", "Card", "jsx")])
        );
        assert_eq!(
            uses_of("a.js", "class A extends ns.Base {}\n"),
            expected(&[("A", "ns.Base", "extends")])
        );
    }

    #[test]
    fn dynamic_import_reads_only_a_plain_literal() {
        let src = "f(import('x'));\nf(import(`x${y}`));\nf(import('x', o));\nf(g('x'));\n";
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let got: Vec<_> = (facts.calls.iter())
            .filter(|c| c.callee == "f")
            .map(|c| dynamic_import(&c.args[0]))
            .collect();
        assert_eq!(got, [Some("x"), None, None, None]);
    }

    #[test]
    fn a_file_records_its_top_level_declarations_exported_or_not() {
        let src = "const limit = 3; type Shape = { a: number }; interface Props { s: Shape } enum Mode { A }\n\
                   export const shown = 1; let later = 2; var old = 3; const run = () => 1;\n\
                   if (x) { const inner = 4 }\n";
        let mut names: Vec<_> = (parse(src).declarations.into_iter())
            .map(|d| d.name)
            .collect();
        names.sort();
        assert_eq!(names, ["Mode", "Props", "Shape", "limit", "shown"]);
    }

    #[test]
    fn top_level_declarations_are_recorded_once_each() {
        let src = "export const aPlugin = createX({ id: 'a' });\n\
                   export type APlugin = typeof aPlugin;\n\
                   export interface P { a: string }\n\
                   export interface P { b: string }\n\
                   export enum S { A = 'a' }\n\
                   export let l = f();\n\
                   export const Foo = z.object({});\n\
                   export type Foo = z.infer<typeof Foo>;\n\
                   class C {}\n\
                   export interface C {}\n\
                   namespace N { export interface X {} }\n\
                   namespace M { export const z = 1; }\n\
                   const z = 2;\n\
                   if (ok) { const y = 1; }\n\
                   const y = g();\n\
                   export { y as why };\n\
                   const hidden = 1;\n\
                   export const run = () => 1;\n\
                   export default makeX({});\n";
        let mut facts = parse(src);
        crate::lang::hash_spans(&mut facts, src.as_bytes());
        let got: Vec<_> = (facts.declarations.iter())
            .map(|d| (d.name.as_str(), &src[d.range.clone()]))
            .collect();
        assert_eq!(
            got,
            [
                ("aPlugin", "aPlugin = createX({ id: 'a' })"),
                ("APlugin", "type APlugin = typeof aPlugin;"),
                ("P", "interface P { a: string }"),
                ("S", "enum S { A = 'a' }"),
                ("Foo", "Foo = z.object({})"),
                ("z", "z = 2"),
                ("y", "y = g()"),
                ("hidden", "hidden = 1"),
                ("default", "makeX({})"),
            ]
        );
        assert_eq!(
            facts.declarations[0].hash,
            blake3::hash(b"aPlugin = createX({ id: 'a' })")
                .to_hex()
                .to_string()
        );
        let js = TsJs
            .parse(
                "a.js",
                b"export const a = 1;\nconst z = 3;\nexport default z;\n",
            )
            .unwrap();
        let names: Vec<_> = js.declarations.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["a", "z"]);
        let src = "export type Bar = z.infer<typeof Bar>;\nexport const Bar = z.object({});\n";
        let bar = parse(src);
        let got: Vec<_> = (bar.declarations.iter())
            .map(|d| (d.name.as_str(), &src[d.range.clone()]))
            .collect();
        assert_eq!(got, [("Bar", "Bar = z.object({})")]);
    }

    #[test]
    fn a_bare_undefined_arrow_retains_its_parameter_scope_and_original_spans() {
        let src = "const bare = undefined => read(undefined);\n\
                   const commented = undefined /* parameter */ => read(undefined);\n\
                   const parenthesized = (undefined) => read(undefined);\n\
                   const asynchronous = async undefined => read(undefined);\n\
                   const ordinary = value => read(value);\n\
                   const comparison = undefined > read(undefined);\n\
                   const malformed = undefined = > read(undefined);\n\
                   const text = 'undefined => read(undefined)';\n\
                   // const comment = undefined => read(undefined);\n";
        for path in ["a.js", "a.ts"] {
            let facts = TsJs.parse(path, src.as_bytes()).unwrap();
            let functions = [
                ("bare", "undefined"),
                ("commented", "undefined"),
                ("parenthesized", "undefined"),
                ("asynchronous", "undefined"),
                ("ordinary", "value"),
            ];
            assert_eq!(facts.defs.len(), functions.len(), "{path}");
            for (name, parameter) in functions {
                let def = facts.defs.iter().find(|d| d.name == name).unwrap();
                assert_eq!(def.params.len(), 1, "{path}: {name}");
                assert_eq!(def.params[0].name, parameter, "{path}: {name}");
                let call = facts.calls.iter().find(|c| c.scope == name).unwrap();
                assert_eq!(&src[call.range.clone()], format!("read({parameter})"));
                assert!(matches!(&call.args[0], Value::Ref(name) if name == parameter));
                assert!(src[def.range.clone()].contains(parameter));
            }
            assert_eq!(facts.calls.iter().filter(|c| c.scope.is_empty()).count(), 2);
        }
    }

    fn depth(value: &Value) -> usize {
        match value {
            Value::New(_, args) => 1 + args.iter().map(depth).max().unwrap_or(0),
            _ => 1,
        }
    }

    #[test]
    fn a_function_lists_the_values_it_returns() {
        let facts = parse(
            "function make() {\n  const r = Router();\n  const inner = () => { return 2; };\n  \
             if (x) { return r; }\n  return () => { return 1; };\n}\n",
        );
        let returned = |name: &str| -> Vec<String> {
            let def = facts.defs.iter().find(|d| d.name == name).unwrap();
            (def.return_values.iter())
                .map(|v| match v {
                    Value::Ref(r) => r.clone(),
                    Value::Scalar(_) => "scalar".into(),
                    Value::Unknown => "?".into(),
                    _ => "other".into(),
                })
                .collect()
        };
        assert_eq!(returned("make"), ["r", "?"]);
        assert_eq!(returned("make.inner"), ["scalar"]);
        let facts = parse(
            "function* make() {\n  return new Storage();\n}\nclass A {\n  *each() { return 1; }\n}\n",
        );
        let generators = (facts.defs.iter())
            .filter(|d| d.name == "make" || d.name == "A.each")
            .map(|d| {
                let unknown = |v: &Value| matches!(v, Value::Unknown);
                !d.return_values.is_empty() && d.return_values.iter().all(unknown)
            })
            .collect::<Vec<_>>();
        assert_eq!(generators, [true, true]);
        let facts = parse("const f = () => Router();\nxs.map((x) => { return x; });\n");
        let values = |d: &Def| d.return_values.len();
        assert_eq!(facts.defs.iter().map(values).collect::<Vec<_>>(), [1, 1]);
        let n = DEFINITION_DEPTH + 1;
        let src = format!("{}return 1;{}", "f(() => { ".repeat(n), " });".repeat(n));
        let facts = parse(&src);
        assert!(facts.defs.iter().all(|d| d.return_values.is_empty()));
    }

    #[test]
    fn definitions_nest_to_a_limit() {
        let n = 512;
        let facts = parse(&format!("{}1{};", "f(() => ".repeat(n), ")".repeat(n)));
        assert_eq!(facts.calls.len(), n);
        let deepest = (facts.defs.iter()).map(|d| d.name.matches('.').count() + 1);
        assert_eq!(deepest.max(), Some(DEFINITION_DEPTH.min(n)));
    }

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
                ("app.use", "function"),
                ("default", "function"),
                ("exports.a", "function"),
                ("exports.b", "function"),
                ("f", "function"),
                ("x.y.z", "function"),
            ]
        );
        let exported: Vec<_> = (facts.exports.iter())
            .filter_map(|e| match e {
                Export::Local { exported, local } => Some((exported.as_str(), local.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            exported,
            [
                ("a", "exports.a"),
                ("b", "exports.b"),
                ("default", "default")
            ]
        );
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
    fn a_long_method_chain_reads_its_receivers_within_one_budget() {
        let n = 30_000;
        let source = format!("const a = process.env.X{};", ".trim()".repeat(n));
        let start = std::time::Instant::now();
        let facts = TsJs.parse("a.ts", source.as_bytes()).unwrap();
        let took = start.elapsed();
        assert_eq!(facts.calls.len(), n);
        let unknown = (facts.calls.iter())
            .filter(|c| matches!(c.receiver, Some(Value::Unknown)))
            .count();
        assert!(0 < unknown && unknown < n, "{unknown}");
        assert!(took.as_secs() < 25, "{took:?}");
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
        let source = "async function f() { g(await get<Row>(\"/x\"), await get(\"/y\")); }";
        let facts = parse(source);
        let call = facts.calls.iter().find(|c| c.callee == "g").unwrap();
        assert_eq!(call.args.len(), 2);
        for (value, written) in call.args.iter().zip(["get<Row>(\"/x\")", "get(\"/y\")"]) {
            let Value::Await(value) = value else {
                panic!("await syntax is retained");
            };
            let Value::Call(call) = value.as_ref() else {
                panic!("awaited get call");
            };
            assert_eq!(call.callee, "get");
            assert_eq!(call.args.len(), 1);
            assert_eq!(&source[call.range.clone()], written);
        }
    }

    #[test]
    fn field_writes_keep_what_a_class_writes_its_private_fields() {
        let src = "class C {\n\
                   private a: X;\n\
                   #b: X | null = null;\n\
                   private c = make();\n\
                   d: X;\n\
                   private static e = make();\n\
                   private f: X | null = null;\n\
                   private g: X;\n\
                   constructor(p) { this.a = p.one(); this.g = p; }\n\
                   open() { this.#b = make(); this.f = null; this.d = make(); }\n\
                   bump() { this.g += 1; }\n\
                   }\n\
                   class D { private a: X; m() { this.a = other(); } }\n\
                   class E { private a = make(); m(k) { this[k] = 1; } }\n";
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let found: Vec<(&str, Vec<&str>)> = (facts.field_writes.iter())
            .map(|w| {
                (
                    w.name.as_str(),
                    w.values.iter().map(|(at, _)| at.as_str()).collect(),
                )
            })
            .collect();
        let expected: [(&str, Vec<&str>); 4] = [
            ("C.a", vec!["C.constructor"]),
            ("C.#b", vec!["C.open"]),
            ("C.c", vec!["C"]),
            ("D.a", vec!["D.m"]),
        ];
        assert_eq!(found, expected);
    }
}
