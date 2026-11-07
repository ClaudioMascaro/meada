//! The TypeScript and JavaScript language pack.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Language, Node, Parser};

use super::{
    Binds, Call, ConfigInput, Const, Def, Export, Facts, Field, Import, Imported, Invocation, Kind,
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
        // The grammar gives `undefined` a dedicated token and recovers its bare arrow
        // parameter as `undefined = > body`. Reparse only that adjacent `=>` form with an
        // equal-length identifier token. All facts still read the original bytes and spans.
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
            Parents::new(root, src)
        } else {
            parents
        };
        let mut facts = Facts {
            member_writes: Some(Vec::new()),
            ..Facts::default()
        };
        let mut write_bytes = 0;
        let mut uses = uses::Uses::new(src, &parents);
        // The instance fields only their declaration or constructor may assign, or only their
        // class reach (`sealed_field`), by class and name, with their initializer; and every
        // member write, by name (`None`: any member of `this`) and scope (`member_writes`).
        let typescript = path.ends_with(".ts") || path.ends_with(".tsx");
        let mut sealed: Vec<(String, String, Option<Value>)> = Vec::new();
        let mut writes: BTreeMap<(Option<String>, String), Vec<Option<Value>>> = BTreeMap::new();
        // Each function definition by its node: the `return`s written in its body.
        let mut functions: HashMap<usize, usize> = HashMap::new();
        // Explicit stack instead of recursion: generated code can nest deeper than the call
        // stack.
        let mut stack = vec![Visit {
            node: root,
            scope: String::new(),
            depth: 0,
            body: root,
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
                functions.insert(node.id(), facts.defs.len());
                facts.defs.push(Def {
                    asynchronous: children(node).any(|n| n.kind() == "async"),
                    params: params(node, src, &inner, &parents),
                    returns: (node.child_by_field_name("return_type"))
                        .and_then(|t| types::returned(t, src, &parents)),
                    type_params: types::type_parameters(node, src),
                    lone_string: lone_string(node, src, &inner, &parents),
                    // An arrow function's expression body is what it returns.
                    return_values: (node.child_by_field_name("body"))
                        .filter(|b| {
                            node.kind() == "arrow_function" && b.kind() != "statement_block"
                        })
                        .map(|b| vec![value(b, src, &inner, &parents)])
                        .unwrap_or_default(),
                    ..Def::new(inner.clone(), kind, range)
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
                            unwraps: false,
                            awaits: false,
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
                                .map(|object| value(object, src, &scope, &parents)),
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
                "public_field_definition" if typescript => {
                    if let Some(name) = sealed_field(node, src) {
                        let initial = node.child_by_field_name("value");
                        let initial = initial.map(|v| value(v, src, &scope, &parents));
                        sealed.push((scope.clone(), name, initial));
                    }
                }
                "assignment_expression"
                | "augmented_assignment_expression"
                | "update_expression"
                    if typescript =>
                {
                    for (name, plain) in member_writes(node, src, &scope, &parents) {
                        let at = writes.entry((name, scope.clone())).or_default();
                        at.push(plain);
                    }
                }
                "import_statement" => imports(node, src, &mut facts.imports),
                "export_statement" => exports(node, src, &mut facts.exports),
                // A `return` of a function that is no definition (one returned in place, or
                // one past `DEFINITION_DEPTH`) is nobody's.
                "return_statement" => {
                    if let (Some(&at), Some(returned)) =
                        (functions.get(&body.id()), named(node).next())
                    {
                        let returned = value(returned, src, &scope, &parents);
                        facts.defs[at].return_values.push(returned);
                    }
                }
                _ => {}
            }
            let named = binds(node, src, &parents);
            selector_writes(node, src, &scope, &mut facts, &mut write_bytes);
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
                // A `new URL` whose path or host the file writes through its name is no longer
                // the string it was built from.
                let rewritten = is_url_new(*value_node, src, &parents)
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
        // A sealed field written once, by its initializer or by the plain `this.f = v` of its
        // class's constructor body, holds that value: a constant of the class, written in the
        // constructor, as Java's `final` and C#'s `readonly` fields are (`Const::scope`), which
        // `this.f` reads. Any other write of a member so named inside the class, or a computed
        // one of `this`, leaves it none.
        // ponytail: a write through an alias of `this` (`const self = this; self.f = v`) or
        // `Object.assign(this, ..)` is not seen, nor a subclass that redeclares a non-private
        // `readonly` field; see `docs/agents/ceilings/language-packs.md`.
        for (class, name, initial) in sealed {
            let inside = |scope: &str| {
                (scope.strip_prefix(class.as_str()))
                    .is_some_and(|r| r.is_empty() || r.starts_with('.'))
            };
            // The writes of `name` (or of any member) in scopes from `class` up to `class/`, which
            // holds every `class.*` scope; `inside` drops the others the range holds (`class-x`).
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
        Ok(facts)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &[
            "**/tsconfig.json",
            "**/jsconfig.json",
            // Backstage's configuration, which `http::ts` reads for `config.getString`.
            "**/app-config*.yaml",
        ]
    }

    /// The `$include` targets of Backstage's `app-config*.yaml` (#689).
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
        // Read as `package::read_tsconfig` reads one: a config past the size limit, not UTF-8,
        // not JSON or not an object declares no paths, and indexing carries on.
        // The nearest config above an importer serves it; in one directory `tsconfig.json`
        // comes first in `configs`, so it wins. A config under `node_modules` declares nothing.
        // Each config is read once; an `extends` is followed among these (`Aliases::of_chain`).
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

/// What one `tsconfig.json` (or `jsconfig.json`) in `dir` says of `baseUrl`, `paths` and
/// `extends`, each `None` when it does not say it.
struct Options {
    dir: String,
    /// The config an `extends` names, as a repository path: a relative path inside the
    /// repository (`.json` added when missing). A package, an absolute path or a path that leaves
    /// the repository is no `extends` here.
    extends: Option<String>,
    /// `baseUrl` joined to `dir`; `Some(None)` when it leaves the repository.
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

/// How many configs an `extends` chain may follow after the nearest one. A chain that goes
/// deeper, or back to a config it already holds, reads as the nearest config alone.
// ponytail: real monorepos extend one or two levels (package, then root); 8 is far past any.
const EXTENDS_DEPTH: usize = 8;

/// The `compilerOptions.baseUrl` and `paths` an importer's nearest config gives it: those of the
/// config itself, then of the chain of its `extends`, the nearest to declare each option winning
/// (the option as a whole, as TypeScript merges them).
// ponytail: project `references`, an `extends` array and a base that is not named `tsconfig.json`
// or `jsconfig.json` are not followed (only those files are read).
#[derive(Default)]
struct Aliases {
    /// What `paths` targets are relative to without `baseUrl`: the directory of the config
    /// that declares `paths`.
    dir: String,
    base_url: Option<String>,
    paths: Paths,
}

impl Aliases {
    /// The aliases of the config at `path`, among every config of the repository, read once.
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

    /// The paths a bare specifier names: the targets of the `paths` pattern with the
    /// longest prefix that matches, then the specifier under `baseUrl`. Without `baseUrl`,
    /// targets are relative to the config file's directory.
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
    /// The names a member is written through (`u.p = ..`, `u.p += ..`, `u["p"] = ..`), except
    /// `search` and `hash`, which keep the path of a URL (WHATWG URL Standard).
    // ponytail: per name across the whole file, whichever function writes it; see `docs/agents/ceilings.md`.
    written: HashSet<String>,
    /// Whether the file binds a `URL` other than the runtime constructor: `new URL(x)` is then
    /// no runtime global.
    url_declared: bool,
    /// Bare `undefined =>` parameters the grammar recovered as a comparison.
    arrow_parameters: Vec<usize>,
}

impl<'t> Parents<'t> {
    fn new(root: Node<'t>, src: &[u8]) -> Self {
        let mut up = HashMap::new();
        let mut assigned = HashSet::new();
        let mut written = HashSet::new();
        let mut url_declared = false;
        let mut url_required = false;
        let mut require_declared = false;
        let mut arrow_parameters = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
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
            url_declared |= shadows_runtime(node, src, "URL");
            url_required |= runtime_url_require(node, src);
            require_declared |= shadows_runtime(node, src, "require");
            written.extend(member_written(node, src));
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
        url_declared |= assigned.contains("URL")
            || (url_required && (require_declared || assigned.contains("require")));
        Self {
            up,
            assigned,
            written,
            url_declared,
            arrow_parameters,
        }
    }

    fn of(&self, node: Node) -> Option<Node<'t>> {
        self.up.get(&node.id()).copied()
    }
}

/// Whether `node` binds `name`, hiding the runtime name. Only the named `URL` export of Node's
/// `url` and `node:url` modules proves that a binding named `URL` is the runtime constructor.
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
        // A parameter or a `catch` binding of that name hides the global as well.
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

/// `const { URL } = require('url')` (or `node:url`), including an explicit `URL: URL` binding.
/// A module object, another export, a rest or nested pattern does not prove a constructor.
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

/// The callee of a `new URL(reference, base)` of the runtime global, as a `Value::New` names it.
pub const GLOBAL_URL: &str = "globalThis.URL";

/// Whether `node` is `new URL(..)` of the runtime global: no declaration of the file hides it.
fn is_url_new(node: Node, src: &[u8], up: &Parents) -> bool {
    node.kind() == "new_expression"
        && !up.url_declared
        && (node.child_by_field_name("constructor"))
            .is_some_and(|c| c.kind() == "identifier" && text(c, src) == "URL")
}

/// The name `node` writes a member of, when it is `u.p = ..` or `u.p += ..` (or `u["p"]`) and
/// `p` is neither `search` nor `hash`.
fn member_written(node: Node, src: &[u8]) -> Option<String> {
    if !matches!(
        node.kind(),
        "assignment_expression" | "augmented_assignment_expression"
    ) {
        return None;
    }
    let left = node.child_by_field_name("left")?;
    let object = left.child_by_field_name("object")?;
    let member = match left.kind() {
        "member_expression" => text(left.child_by_field_name("property")?, src).to_string(),
        "subscript_expression" => {
            let index = left.child_by_field_name("index")?;
            if index.kind() == "string" {
                unquote(index, src)
            } else {
                String::new()
            }
        }
        _ => return None,
    };
    (object.kind() == "identifier" && member != "search" && member != "hash")
        .then(|| text(object, src).to_string())
}

fn selector_writes(node: Node, src: &[u8], scope: &str, facts: &mut Facts, spent: &mut usize) {
    if facts.member_writes.is_none() {
        return;
    }
    let target = match node.kind() {
        "assignment_expression" | "augmented_assignment_expression" => {
            node.child_by_field_name("left")
        }
        "update_expression" => node
            .child_by_field_name("argument")
            .or_else(|| named(node).next()),
        "unary_expression" if children(node).any(|n| text(n, src) == "delete") => node
            .child_by_field_name("argument")
            .or_else(|| named(node).next()),
        _ => return,
    };
    let mut pending: Vec<_> = target.into_iter().collect();
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
            facts.member_write(
                super::MemberWrite {
                    scope: scope.into(),
                    object: callee(object, src),
                    field,
                    range: node.byte_range(),
                },
                spent,
            );
        }
    }
}

/// The string a function returns when that is all its body does: an arrow function's expression
/// body, or a body of one `return`. An `async` function returns a promise, not a string.
fn lone_string(node: Node, src: &[u8], scope: &str, up: &Parents) -> Option<Value> {
    if !(FUNCTIONS.contains(&node.kind()) || node.kind() == "function_declaration")
        || node.kind() == "generator_function"
        || node.children(&mut node.walk()).any(|c| c.kind() == "async")
    {
        return None;
    }
    let body = node.child_by_field_name("body")?;
    let returned = if body.kind() == "statement_block" {
        let mut statements = named(body).filter(|s| s.kind() != "comment");
        let (Some(only), None) = (statements.next(), statements.next()) else {
            return None;
        };
        if only.kind() != "return_statement" {
            return None;
        }
        named(only).next()?
    } else {
        body
    };
    Some(value(returned, src, scope, up)).filter(|v| matches!(v, Value::Str(_)))
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
    /// The name it defines: `obj.y`, `X.y` for `X.prototype.y`, `exports.y` for `exports.y` and
    /// `module.exports.y` (exported as `y`), `default` for `module.exports`.
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
        // The symbol is `exports.y`, not `y`: Node binds no local `y`, so a bare `y()` must not
        // reach it.
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
        "await_expression" => named(node)
            .next()
            .map_or(Value::Unknown, |n| Value::Await(Box::new(deeper(n)))),
        "number" => Value::Scalar(text(node, src).parse().ok()),
        "true" | "false" => Value::Scalar(None),
        // `undefined` is a shadowable name, including the grammar's dedicated node.
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
                        // The span of the call in `Facts::calls`.
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
                // `new URL(reference, base)` of the runtime global: the name no declaration of
                // the file can hide, for the HTTP kind to join (`http::join`).
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
fn parts(node: Node, src: &[u8], up: &Parents) -> Option<Vec<Part>> {
    parts_at(node, src, up, 0)
}

/// `parts` of a node `depth` levels inside the expression it started from; past `FOLD_DEPTH`
/// the rest is a hole.
fn parts_at(node: Node, src: &[u8], up: &Parents, depth: usize) -> Option<Vec<Part>> {
    if depth >= FOLD_DEPTH {
        return Some(vec![Part::Hole]);
    }
    let parts = |n: Node| parts_at(n, src, up, depth + 1);
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
        "member_expression" if let Some(name) = this_member(node, src, up) => {
            Some(vec![Part::Ref(name)])
        }
        "parenthesized_expression"
        | "non_null_expression"
        | "as_expression"
        | "satisfies_expression" => parts(named(node).next()?),
        _ => url_form(node, src, up, depth)
            .or_else(|| env_read(node, src, up, depth).map(|env| vec![env])),
    }
}

/// The URL forms that pass a string through, as the WHATWG URL Standard reads them: `new URL(x)`
/// with one argument parses `x` (#dom-url-url), and `u.toString()`, `u.href` and `String(u)` are
/// the URL's `href` (#dom-url-href). The receiver is a `new URL(..)` or a name no member write
/// reaches (`Parents::written`): a write to the path or the host changes what it holds.
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
                "identifier" if text(function, src) == "String" => {
                    let argument = one(arguments)?;
                    parts(argument).filter(|_| !written(argument))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// `process.env.X`, `process.env["X"]` or `import.meta.env.X`, also through string method
/// calls such as `.replace(...)` or `.trim()`.
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

/// The members whose call with one string literal reads a key of Backstage's configuration
/// (`config.getString('a.b')`); the kind decides whether the receiver is one and what the key
/// holds, as it does for C#'s `GetEnvironmentVariable`.
// ponytail: the one library form of this pack beside C#'s (`CODING_STANDARDS.md`); see `docs/agents/ceilings.md`.
const CONFIG_READS: &[&str] = &["getString", "getOptionalString"];

/// `config.getString('a.b')` as a `Part::Read` of the key, for the kind to decide.
fn config_read(call: Node, function: Node, src: &[u8]) -> Option<Part> {
    let args = call.child_by_field_name("arguments")?;
    let mut args = named(args);
    let key = args.next().filter(|k| k.kind() == "string")?;
    if args.next().is_some() {
        return None;
    }
    Some(Part::Read {
        scope: std::sync::Arc::from(""),
        callee: callee(function, src),
        key: unquote(key, src),
        then: Vec::new(),
    })
}

/// How many nodes `this_member` climbs to the class member around a `this`; past it the `this`
/// is read as no instance's.
// ponytail: real code nests a `this` a few dozen nodes inside its method; the climb must stay
// cheap whatever the file. See `docs/agents/ceilings/language-packs.md`.
const THIS_CLIMB: usize = 256;

/// `this.f` or `this.#f` as the name the resolver reads (`LanguagePack::receiver`), when the
/// `this` is an instance of the class around it: written in an instance method or field
/// initializer of a class body, through arrow functions only. A `function`, a method of an
/// object literal, a `static` member and a static block bind another `this`.
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
    let mut at = node;
    for _ in 0..THIS_CLIMB {
        let parent = up.of(at)?;
        match parent.kind() {
            "class_body" => {
                let shared = (at.children(&mut at.walk())).any(|c| c.kind() == "static");
                let instance = !shared && at.kind() != "class_static_block";
                return instance.then(|| format!("this.{}", text(property, src)));
            }
            "method_definition" if up.of(parent).is_none_or(|b| b.kind() != "class_body") => {
                return None;
            }
            kind if kind != "arrow_function" && FUNCTIONS.contains(&kind) => return None,
            "function_declaration" | "generator_function_declaration" => return None,
            _ => at = parent,
        }
    }
    None
}

/// The name of the instance field `node` (a `public_field_definition`) declares, when only its
/// declaration or a constructor of its class may assign it (`readonly`) or only its class may
/// reach it (`private`, `#x`): TypeScript's handbook
/// (https://www.typescriptlang.org/docs/handbook/2/classes.html#readonly) and ECMAScript's
/// private names (https://tc39.es/ecma262/#sec-private-names). A `static` or `abstract` field
/// is no instance's.
fn sealed_field(node: Node, src: &[u8]) -> Option<String> {
    let name = node.child_by_field_name("name")?;
    let mut sealed = name.kind() == "private_property_identifier";
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "readonly" => sealed = true,
            "accessibility_modifier" if text(child, src) == "private" => sealed = true,
            "static" | "abstract" => return None,
            _ => {}
        }
    }
    let plain = matches!(
        name.kind(),
        "property_identifier" | "private_property_identifier"
    );
    (sealed && plain).then(|| text(name, src).to_string())
}

/// The members the write `node` (`=`, `op=`, `++`) assigns: each one's name, `None` for a
/// computed member of `this` (it may be any), with the value of a plain `this.f = v` written
/// as a statement of a constructor's body, the one write a sealed field may have. A pattern
/// (`[this.a] = ..`) writes every member it holds.
fn member_writes(
    node: Node,
    src: &[u8],
    scope: &str,
    up: &Parents,
) -> Vec<(Option<String>, Option<Value>)> {
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
                    out.push((Some(unquote(index, src)), None));
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
        .map(|v| value(v, src, scope, up));
        out.push((name.map(str::to_string), plain));
    }
    out
}

/// Whether the expression `node` is a statement of the body of a class's `constructor`.
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

    /// The files `@/b` names from `from`, given `configs` as `(path, json)`.
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
            // A package, a path outside the repository and an absolute path are unread.
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
        // `paths` are relative to the config that declares them.
        assert!(has("packages/a/src/x.ts", "lib/b.ts"));
        // The nearest `baseUrl` is the base of the root's `paths`, and also resolves the bare name.
        assert!(has("packages/b/src/x.ts", "packages/b/lib/b.ts"));
        assert!(has("packages/b/src/x.ts", "packages/b/@/b.ts"));
        // A nested `paths` replaces the base's, through the chain.
        assert!(has("packages/c/src/x.ts", "packages/c/c/b.ts"));
        assert!(!has("packages/c/src/x.ts", "lib/b.ts"));
        for dir in ["d", "e", "f"] {
            assert!(aliased(&configs, &format!("packages/{dir}/x.ts")).is_empty());
        }
    }

    #[test]
    fn an_extends_chain_past_the_limit_or_in_a_cycle_reads_as_the_nearest_config_alone() {
        let root = r#"{"compilerOptions": {"paths": {"@/*": ["lib/*"]}}}"#.to_string();
        // `c0` extends `c1` and so on, the last extending the root: `links` configs in between.
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
        // The nearest config and `EXTENDS_DEPTH` more reach the root's paths; one more does not.
        assert!(reaches(&chain(EXTENDS_DEPTH)));
        assert!(!reaches(&chain(EXTENDS_DEPTH + 1)));
        assert!(!reaches(&chain(EXTENDS_DEPTH * 1000)));
        // A cycle reads as the nearest config alone, which here declares paths of its own.
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
        // A type that lists no fields is no type of the constant.
        assert_eq!(held.ty, None);
        let args: Vec<_> = facts.calls.iter().filter(|c| c.callee == "fetch").collect();
        assert!(matches!(args[0].args.as_slice(), [Value::Ref(name)] if name == "url"));
        assert_eq!(args[0].arg_types, [None]);
        assert!(matches!(args[1].args.as_slice(), [Value::Str(_)]));
    }

    /// What the first argument of the one `fetch` in `path` folds to, `src` alone in the
    /// repository.
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

    /// A field TypeScript lets only its declaration or its constructor assign (`readonly`), or
    /// only its class reach (`private`, ECMAScript's `#x`), written once holds that value: a
    /// constant of the class, written in the constructor, which `this.f` in a method reads.
    #[test]
    fn a_readonly_or_private_field_written_once_is_a_constant_of_its_class() {
        let env = || vec![Part::Env("API".into()), Part::Text("/o".into())];
        // `F` is the field's name.
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
        // Through a concatenation, and a Backstage read the HTTP kind decides.
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
            // Public and mutable.
            "base = process.env.API;",
            "public base: string;\nconstructor() { this.base = process.env.API; }",
            // Written twice: by the initializer and the constructor, twice in the constructor,
            // or under a condition (written, or not).
            "private readonly base = process.env.API;\nconstructor() { this.base = process.env.API; }",
            "private base: string;\nconstructor() { this.base = process.env.API; this.base = process.env.API; }",
            "private base: string;\nconstructor(x: boolean) { if (x) { this.base = process.env.API; } }",
            "private base = process.env.API;\nreset() { this.base += '/v2'; }",
            "private base = process.env.API;\nreset(o: C) { o.base = 'x'; }",
            "private base = process.env.API;\nreset() { [this.base] = ['x']; }",
            "private base = process.env.API;\nreset() { this['base'] = 'x'; }",
            // Written outside the constructor, or in a nested function of it.
            "private base: string;\ninit() { this.base = process.env.API; }",
            "private base: string;\nconstructor() { const f = () => { this.base = process.env.API; }; f(); }",
            // A parameter property, or a constructor's parameter: its callers give the value.
            "constructor(private readonly base: string) {}",
            "private readonly base: string;\nconstructor(base: string) { this.base = base; }",
            // Static: no instance holds it.
            "static readonly base = process.env.API;",
        ] {
            let src = class(members, "base");
            assert_eq!(fetched_parts("a.ts", &src), hole(), "{src}");
        }
        // Only an instance method's `this` is the instance, also through an arrow function: a
        // `function`, a method of an object literal, a `static` method and a nested class bind
        // another `this`.
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
        // A JavaScript file declares no `readonly` or `private`, and its `#x` is left as today.
        let js = class("F = process.env.API;", "#base");
        assert_eq!(fetched_parts("a.js", &js), hole(), "{js}");
    }

    /// A `this` nested past `THIS_CLIMB` nodes inside its method is no instance's.
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
        // 1.4 s measured in a local debug build; the bound is over twenty times that.
        assert!(took.as_secs() < 40, "{took:?}");
    }

    /// The text of the literal parts of a folded value, `<env>` for an environment read, `?`
    /// for a name and `_` for a hole; `None` when the value is no string.
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

    /// WHATWG URL Standard: `new URL(x)` parses `x` (#dom-url-url), and `toString()`, `href` and
    /// `String(u)` give its `href` (#dom-url-href), also inside a template string.
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
        // A name folds through the existing name folding at the shared side.
        assert!(
            got[5..].iter().all(|g| g.as_deref() == Some("?")),
            "{got:?}"
        );
    }

    /// Only a call with the shape of the standard's reads: `toString(16)`, `new URL(x, base)` and
    /// a `href` of anything but a name or a `new URL(..)` stay as they were.
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

    /// WHATWG URL Standard: `searchParams` changes only the query (#dom-url-searchparams), as
    /// `search` and `hash` do; a write to any other member changes the path or the host.
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
        ] {
            assert!(unknown(&format!("{url}{write}")), "{write}");
        }
        // The pass-through forms of a written name stay calls and members, as they were.
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

    /// A file that declares a `URL`, or imports one from a relative path, has no runtime global
    /// of that name: its `new URL(x)` is the repository's constructor.
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
        // An import from a package, or a `require` of one, is the same global.
        let got = fetched(
            "const { URL } = require('node:url');\nfetch(new URL('https://h.example.com/a'));",
        );
        assert_eq!(got, [Some("https://h.example.com/a".into())]);
        let got =
            fetched("import { URL } from 'node:url';\nfetch(new URL('https://h.example.com/a'));");
        assert_eq!(got, [Some("https://h.example.com/a".into())]);
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

    /// Each `return` of a function is one of its values; a nested function's are its own, and
    /// those of a function that is no definition (one returned in place) belong to nobody.
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
        // A callback written in place, and an arrow's expression body.
        let facts = parse("const f = () => Router();\nxs.map((x) => { return x; });\n");
        let values = |d: &Def| d.return_values.len();
        assert_eq!(facts.defs.iter().map(values).collect::<Vec<_>>(), [1, 1]);
        // Past `DEFINITION_DEPTH` the innermost function is no definition: its `return` is
        // nobody's, not the deepest definition's.
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
}
