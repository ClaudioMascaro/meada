//! The Python language pack.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;

use tree_sitter::{Node, Parser};

use super::{
    Binds, Call, Class, Const, Def, Export, Facts, Import, Imported, Invocation, Kind,
    LanguagePack, Message, Modules, Param, Part, Receiver, Star, TYPE_DEPTH, Value, children,
    literal, named, text,
};
use crate::package::read_manifest;
use crate::resolve::Code;
use crate::{Result, parent, qualify};

mod messages;
pub mod pyproject;

/// How many definitions may nest in one file. Names are qualified by the definitions around
/// them and each call resolves its names up that chain, so unbounded nesting costs cubic time.
/// Past the limit a function or class is not a definition (its body belongs to the one around
/// it, where its name and parameters are locals that hide outer ones).
// ponytail: real code nests a few levels; 128 is the depth the TypeScript pack allows.
const DEFINITION_DEPTH: usize = 128;

/// How many nodes deep `targets` and `captures` read an unpacking target or a `case` pattern;
/// a name deeper is not bound.
// ponytail: real code unpacks a few levels; the recursion must stay shallow, whatever the file.
const TARGET_DEPTH: usize = 64;

/// How many levels of lists, tuples, dict literals, parentheses and `await`s `value` reads;
/// deeper is `Unknown`. A value is read again by every call around it, and the recursion must
/// stay shallow, whatever the file.
// ponytail: real code nests a few levels of options; 64 is far past any.
const VALUE_DEPTH: usize = 64;

/// How deep `parts`, `env_read` and `callee` follow a `+` or `or` chain, a chain of string
/// method calls or `getenv` defaults, nested f-strings or an attribute chain; the cut side is a
/// hole (`parts`) or its node kind (`callee`).
// ponytail: real code concatenates and chains a few dozen terms; the recursion must stay
// shallow, whatever the file.
const FOLD_DEPTH: usize = 256;

/// The symbol of an `if __name__ == "__main__":` block, the program entry point of its file.
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
        let mut facts = Facts::default();
        let mut module = Module::default();
        // A name defined twice (`@property` and its setter, `try`/`except` fallbacks) is one
        // symbol: the first definition.
        let mut defined = HashSet::new();
        let mut locals = Locals::default();
        // Each function's position in `facts.defs`, for its `return`s.
        let mut functions: HashMap<String, usize> = HashMap::new();
        // The decorator calls not yet met, by node, and the definition each one decorates.
        let mut decorators: HashMap<usize, String> = HashMap::new();
        // The bare-name decorators of each definition not yet met.
        let mut bare_names: HashMap<String, Vec<String>> = HashMap::new();
        // Explicit stack instead of recursion: generated code can nest deeper than the call
        // stack.
        let mut stack = vec![Visit {
            node: tree.root_node(),
            scope: String::new(),
            in_class: false,
            flat: false,
        }];
        while let Some(Visit {
            node,
            scope,
            in_class,
            flat,
        }) = stack.pop()
        {
            let mut inner = scope.clone();
            let mut inner_class = in_class;
            let mut inner_flat = flat;
            // The body of a main guard, whose calls and bindings belong to the `__main__`
            // symbol.
            // ponytail: a name assigned in the guard is a local of `__main__`, so it no longer
            // rebinds the module's global of that name (it is no export or constant of the
            // file); a script keeps its globals outside the guard in practice.
            let mut main_body = None;
            match node.kind() {
                "if_statement" if scope.is_empty() && is_main_guard(node, src) => {
                    let body = node.child_by_field_name("consequence");
                    // Only the first `__main__` counts, a guard or a `def __main__()`: the
                    // calls of a later guard stay at the top level of the file.
                    if let Some(body) = body.filter(|_| defined.insert(MAIN.to_string())) {
                        main_body = Some(body.id());
                        facts.defs.push(Def {
                            name: MAIN.to_string(),
                            kind: Kind::Function,
                            // The `if` and its body, not an `else` after it.
                            range: node.start_byte()..body.end_byte(),
                            params: Vec::new(),
                            bases: Vec::new(),
                            decorators: Vec::new(),
                            return_values: Vec::new(),
                            returns: None,
                            type_params: Vec::new(),
                            result: None,
                        });
                        facts.programs.push(MAIN.to_string());
                    }
                }
                "function_definition" | "class_definition"
                    if levels(&scope) >= DEFINITION_DEPTH =>
                {
                    // No definition: its body belongs to the scope around it, where its name
                    // and parameters hide outer ones, as a lambda's do; its `return`s are no
                    // one's.
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
                                name: inner.clone(),
                                kind,
                                range: span(node),
                                params: params(node, !is_class && in_class, src),
                                bases: bases(node, src),
                                decorators: bare_names.remove(&inner).unwrap_or_default(),
                                return_values: Vec::new(),
                                returns: None,
                                type_params: Vec::new(),
                                result: None,
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
                            module.call_all(method, node, scope.is_empty(), src);
                        }
                        let (args, kwargs) = arguments(node, src);
                        facts.calls.push(Call {
                            scope: scope.clone(),
                            callee,
                            range: node.byte_range(),
                            args,
                            kwargs,
                            decorates: decorators.remove(&node.id()),
                            by_path: false,
                            root: None,
                            receiver: None,
                            arg_types: Vec::new(),
                            uses: Vec::new(),
                        });
                    }
                }
                "return_statement" if !flat => {
                    if let (Some(&at), Some(returned)) = (functions.get(&scope), named(node).next())
                    {
                        facts.defs[at].return_values.push(value(returned, src));
                    }
                }
                // ponytail: an import binds its names for the whole file, wherever it sits; a
                // function-local one also hides an outer binding there, but other functions
                // see it too. A star import that rebinds a name is not placed against the code
                // that reads it either (`Resolver::rebound`): code run while the module loads
                // gets the name unknown, a function the star import's.
                "import_statement" | "import_from_statement" => {
                    let first = facts.imports.len();
                    imports(node, src, &mut facts.imports);
                    if scope.is_empty() {
                        module.import(&facts.imports[first..]);
                    } else {
                        // In a function or a class, the import also hides an outer binding.
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
                    }
                }
                "assignment" | "augmented_assignment" => {
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
                // Other bindings say nothing of a class, but hide an outer binding of the name.
                // ponytail: a comprehension's variables are bound in the enclosing scope, so
                // they hide a name there for the whole scope.
                "for_statement" | "for_in_clause" | "as_pattern" | "except_clause"
                | "named_expression" => {
                    let field = match node.kind() {
                        "for_statement" | "for_in_clause" => "left",
                        "named_expression" => "name",
                        _ => "alias",
                    };
                    let target = node.child_by_field_name(field);
                    // A comprehension's variables are its own, not the module's.
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
                // A lambda's calls belong to the enclosing scope, which its parameters bind.
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
                // A deleted name is bound to nothing: at the top level until bound again, in a
                // function in its whole body.
                // ponytail: `del` of a `global` name in a function hides it in that function
                // alone.
                "delete_statement" => {
                    let deleted = named(node).flat_map(targets);
                    for name in deleted.filter(|t| t.kind() == "identifier") {
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
            }));
        }
        // A function's `global x` may run at any time after the module's own bindings: it is
        // bound last, after every star import.
        for (scope, name, ..) in &locals.written {
            if locals.declared.get(&(scope.clone(), name.clone())) == Some(&true) {
                module.name(name);
            }
        }
        facts.locals.extend(module.hidden.iter().cloned());
        (facts.exports, facts.star) = module.exports();
        let hidden;
        (facts.receivers, facts.consts, hidden) = locals.finish();
        facts.locals.extend(hidden);
        Ok(facts)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["pyproject.toml"]
    }

    fn messages(&self, code: &Code) -> Vec<Message> {
        messages::messages(code)
    }

    fn stars_bind(&self) -> bool {
        true
    }

    fn modules(&self, configs: &[(&str, &[u8])], _: &BTreeMap<&str, &Facts>) -> Result<Modules> {
        // Read as a PyPI manifest is: one past the size limit, not UTF-8 or not TOML declares
        // nothing, and indexing carries on.
        let declared =
            (configs.first()).map_or_else(Vec::new, |(name, src)| read_manifest(name, src).roots);
        let roots = pyproject::source_roots("", &declared);
        Ok(Box::new(move |importer, specifier| {
            let dots = specifier.bytes().take_while(|&b| b == b'.').count();
            let name = specifier[dots..].replace('.', "/");
            let bases = if dots == 0 {
                roots.clone()
            } else {
                // One dot names the importer's own package, its directory; each more goes one
                // level up.
                let dir = importer.rsplit_once('/').map_or("", |(dir, _)| dir);
                let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
                let Some(keep) = parts.len().checked_sub(dots - 1) else {
                    return Vec::new();
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
    /// Whether the node sits in a class body, outside any function in it: a function defined
    /// here is a method.
    in_class: bool,
    /// Whether the node sits in a function or class past `DEFINITION_DEPTH`, which is no
    /// definition: a `return` there is not the enclosing definition's.
    flat: bool,
}

/// What a module binds at its top level, which makes up its exports.
#[derive(Default)]
struct Module {
    /// Every top-level binding in statement order: a name (`Export::Local`, `del` included) or
    /// a star import (`Export::All`).
    bindings: Vec<Export>,
    /// The names a top-level `def` or `class` binds.
    defs: HashSet<String>,
    /// The names whose last top-level binding leaves no call reaching what they were bound to:
    /// a `del`, or any binding after a `def` or `class` of the name, whose definition stays a
    /// symbol. They become `Facts::locals`, which hide them.
    hidden: BTreeSet<String>,
    /// What a star import takes by `__all__`: the names of a literal one, or `Star::Computed`;
    /// `None` when there is none.
    all: Option<Star>,
}

impl Module {
    /// A top-level `def` or `class`; one not `in_place` (under `if`, `try`, a loop) may not run,
    /// so a name bound before it, a star import included, stays hidden.
    fn def(&mut self, name: &str, in_place: bool) {
        let bound = self.bindings.iter().any(|b| match b {
            Export::Local { exported, .. } => exported == name,
            _ => true,
        });
        self.name(name);
        self.defs.insert(name.to_string());
        match in_place || !bound {
            true => self.hidden.remove(name),
            false => self.hidden.insert(name.to_string()),
        };
    }

    /// `del name` at the top level: the name is bound to nothing.
    fn del(&mut self, name: &str) {
        self.name(name);
        self.hidden.insert(name.to_string());
    }

    /// `__all__` grown by the literal names `added`, or changed some other way (`None`): a
    /// literal one adds them, any other is computed.
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
                Imported::All => self.bindings.push(Export::All {
                    source: import.source.clone(),
                }),
                _ if !import.local.contains('.') => self.name(&import.local),
                _ => {}
            }
        }
    }

    fn name(&mut self, name: &str) {
        self.bindings.push(Export::Local {
            exported: name.to_string(),
            local: name.to_string(),
        });
        match self.defs.contains(name) {
            true => self.hidden.insert(name.to_string()),
            false => self.hidden.remove(name),
        };
    }

    /// The names an unpacking or `as` target binds; `self.x` binds none at the top level.
    fn targets(&mut self, node: Node, src: &[u8]) {
        for target in targets(node)
            .into_iter()
            .filter(|t| t.kind() == "identifier")
        {
            self.name(text(target, src));
        }
    }

    /// A top-level assignment, `x += ...` and `a, b = ...` included, and a literal `__all__`,
    /// which `+=` may grow.
    fn assign(&mut self, node: Node, src: &[u8]) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        self.targets(left, src);
        if left.kind() != "identifier" || text(left, src) != "__all__" {
            return;
        }
        let right = node.child_by_field_name("right");
        let names = right.and_then(|right| string_list(right, src));
        match node.kind() {
            "assignment" => self.all = Some(names.map_or(Star::Computed, Star::Only)),
            _ => {
                let operator = node.child_by_field_name("operator").map(|o| text(o, src));
                self.grow_all(names.filter(|_| operator == Some("+=")));
            }
        }
    }

    /// A call of a method of `__all__`: at the top level (`top`), `.extend` of a literal list
    /// or `.append` of a literal name grows it; any other, or one in a function, which may run
    /// at any time, makes it computed.
    fn call_all(&mut self, method: &str, call: Node, top: bool, src: &[u8]) {
        let args = call.child_by_field_name("arguments");
        let args: Vec<Node> = args.map(|a| named(a).collect()).unwrap_or_default();
        let added = match (top, method, &args[..]) {
            (true, "extend", [list]) => string_list(*list, src),
            (true, "append", [name]) => string(*name, src).map(|name| vec![name]),
            _ => None,
        };
        self.grow_all(added);
    }

    /// What an explicit import or an attribute access names: every module-level name (imports
    /// included, as that is how a package re-exports) and its star imports' names, the last
    /// binding first, as it rebinds a name the bindings before it gave (`stars_bind`): a name
    /// once, where it was last bound, and every star import. What `import *` takes is a literal
    /// `__all__`, else the public names, which a computed `__all__` may take or not.
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

/// The repository-relative files that module `name`, a `/`-separated path, may be under
/// `base`: the `__init__.py` of the package `name` (`base` itself when `name` is empty), which
/// Python prefers, or `name.py`.
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

/// Whether the `if` statement `node` tests `__name__ == "__main__"`, in either order and with
/// either quote. Any other condition (`and`, parentheses, `!=`) is no main guard.
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

/// Whether the definition `node` is a statement of its scope's own body (the module, a function,
/// a class, the main guard `scope` is), which runs whenever that body does: not one under `if`,
/// `try`, a loop, `with` or `match`.
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

/// How many definitions the qualified name `scope` nests: none at the top level.
fn levels(scope: &str) -> usize {
    match scope {
        "" => 0,
        scope => scope.matches('.').count() + 1,
    }
}

/// The span of a definition: a decorated one includes its decorators.
fn span(node: Node) -> Range<usize> {
    match node.parent() {
        Some(parent) if parent.kind() == "decorated_definition" => parent.byte_range(),
        _ => node.byte_range(),
    }
}

/// The parameters of a function `node` in order, with their defaults: the receiver of a
/// `method` (`self`, `cls`) first, unless it is static. `*args` and `**kwargs` bind no single
/// argument, and the parameters after them or after the bare `*` only a keyword argument.
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
        // A typed `*args: T` holds its splat.
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
            // The `/` separator binds nothing.
            _ => continue,
        };
        let Some(name) = name.and_then(identifier) else {
            continue;
        };
        // The first parameter takes the receiver, if any.
        let binds = binds.or(receiver.take()).unwrap_or(match keyword {
            true => Binds::Keyword,
            false => Binds::Any,
        });
        out.push(Param {
            name: text(name, src).to_string(),
            binds,
            default: default.map(|d| value(d, src)),
            fields: Vec::new(),
        });
    }
    out
}

/// Each parameter of a function `node` in order: the identifier it binds and its annotation.
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
                // The bare `*` and `/` separators bind nothing.
                _ => return None,
            };
            Some((identifier(pattern), p.child_by_field_name("type")))
        })
        .collect()
}

/// What a binding tells of the class of its name. An annotation decides over the other ways;
/// two other bindings that disagree leave the class unknown.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Evidence {
    /// A parameter without an annotation: its argument is not known.
    Param,
    Assigned,
    /// An annotation, or the receiver of a method.
    Annotated,
}

/// The names each scope binds and what the syntax says of their class.
#[derive(Default)]
struct Locals {
    /// Every binding as written, in source order: its scope, name, class and how.
    written: Vec<(String, String, Class, Evidence)>,
    /// The receiver parameter (`self`, `cls`) of each method, by the method's scope.
    receivers: HashMap<String, String>,
    /// The names a scope declares `global` (`true`) or `nonlocal` (`false`).
    declared: HashMap<(String, String), bool>,
    /// The scopes that are class bodies.
    classes: HashSet<String>,
    /// Every `def` and `class` in a function or class body, in source order: the position in
    /// `written` it comes at, its scope and name, and whether it may not run (not in place)
    /// after a binding of the name in its scope, which then stays the name's.
    defs: Vec<(usize, String, String, bool)>,
    /// The value each `name = value` assigns, by the position of its binding in `written`.
    values: Vec<(usize, Value)>,
}

impl Locals {
    fn bind(&mut self, scope: &str, name: &str, class: Class, how: Evidence) {
        (self.written).push((scope.to_string(), name.to_string(), class, how));
    }

    /// A `def` or `class` of `name` in the function or class body `scope`, `in_place` or under
    /// `if`, `try`, a loop.
    fn def(&mut self, scope: &str, name: &str, in_place: bool) {
        let same = |s: &String, n: &String| s == scope && n == name;
        let bound = self.written.iter().any(|(s, n, ..)| same(s, n))
            || self.defs.iter().any(|(_, s, n, _)| same(s, n));
        let at = self.written.len();
        (self.defs).push((at, scope.to_string(), name.to_string(), !in_place && bound));
    }

    /// Binds a target: a name in `scope`, or `self.x` as the attribute `x` of the class whose
    /// method binds `self`, also from a function nested in the method. An attribute of any
    /// other object is not a binding.
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

    /// The parameters of the function `node`, qualified `scope`: the first one of a method
    /// that is not static is its receiver.
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

    /// An assignment in `scope`: `x = C(...)` gives `x` what `C` names, `x: T` annotates `x`,
    /// `self.x: T` in a method annotates the class's attribute `x`, and any other target,
    /// `self.x` included, is bound to something unknown.
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
        // A name bound by `name = value` holds that value. So does a `self` attribute, but
        // only a call: the attribute's constant is the class's, and a name in the value would
        // be read from the class, not from the method that writes it.
        // ponytail: an attribute assigned something else (`self.client = client`) holds no
        // value, so its origins are not followed.
        if let (true, Some(right), true) =
            (node.kind() == "assignment", right, self.written.len() > at)
            && matches!(left.kind(), "identifier" | "attribute")
        {
            let value = value(right, src);
            if left.kind() == "identifier" || matches!(value, Value::Call(_)) {
                self.values.push((at, value));
            }
        }
    }

    /// `with C(...) as x`: `x` holds what `C(...)` gives, as `x = C(...)` does.
    fn with_item(&mut self, node: Node, scope: &str, src: &[u8]) {
        let Some(alias) = node.child_by_field_name("alias") else {
            return;
        };
        let call = named(node).next().filter(|n| n.kind() == "call");
        let name = named(alias).next().filter(|n| n.kind() == "identifier");
        match (call, name) {
            (Some(call), Some(name)) => {
                let class = constructor(call, src).map_or(Class::Unknown, Class::Named);
                self.values.push((self.written.len(), value(call, src)));
                self.bind(scope, text(name, src), class, Evidence::Assigned);
            }
            _ => {
                for target in targets(alias) {
                    self.target(target, scope, Class::Unknown, Evidence::Assigned, src);
                }
            }
        }
    }

    /// `xs += ...` or `xs = xs + ...`, when the latest value `scope` assigned `xs` is a list
    /// literal: a list literal added joins its items, anything else added is left out, and
    /// `xs` is not bound again, so it stays a constant. `false` for any other assignment.
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
        let Locals {
            written, values, ..
        } = self;
        let latest = (values.iter_mut().rev())
            .find(|(at, _)| written[*at].0 == scope && written[*at].1 == name);
        let Some((_, Value::List(items))) = latest else {
            return false;
        };
        if let Value::List(more) = value(added, src) {
            items.extend(more);
        }
        true
    }

    /// The facts: one per scope and name, every binding but a plain parameter (its definition
    /// already lists it). A name declared `global` is bound at the module; one declared
    /// `nonlocal`, in the nearest enclosing function that binds it, class bodies skipped. An
    /// annotation decides; other bindings that disagree make the class unknown. A name its
    /// scope binds once, by `name = value`, is also a constant holding that value. Last, the
    /// definitions in a function whose name its last binding there is not (`Facts::locals`).
    fn finish(self) -> (Vec<Receiver>, Vec<Const>, Vec<String>) {
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
        for key in &homes {
            *times.entry(key).or_default() += 1;
        }
        // The last binding of each name in its function decides: a definition is hidden when the
        // name is bound after it there, or from a nested function through `nonlocal`, which may
        // run at any time. A class body's is not: `self.m()` reads a method through its class.
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
        let hidden = (defined.into_iter())
            .filter(|(key, (at, kept))| *kept || last.get(key).is_some_and(|latest| latest >= at))
            .map(|((scope, name), _)| qualify(scope, name))
            .collect();
        let consts = (self.values.into_iter())
            .filter(|(at, _)| times[&homes[*at]] == 1 && homes[*at].0 == self.written[*at].0)
            .map(|(at, value)| Const {
                name: qualify(&homes[at].0, &homes[at].1),
                value,
                ty: None,
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
        let locals = (bound.into_iter())
            .filter(|(_, (_, how))| *how != Evidence::Param)
            .map(|((scope, name), (class, _))| Receiver { scope, name, class })
            .collect();
        (locals, consts, hidden)
    }
}

/// What a `call` node constructs, as a callee-like name; `None` when its callee is no name.
fn constructor(call: Node, src: &[u8]) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    Some(callee(function, src)).filter(|f| !f.starts_with('('))
}

/// The targets an assignment, a `for` or an `as` binds, or a `del` unbinds: names, and `self.x`
/// attributes, also inside `a, (b, c)`, `a, *b` and `(a)`, to `TARGET_DEPTH` nodes deep.
fn targets(node: Node) -> Vec<Node> {
    let mut out = Vec::new();
    targets_at(node, 0, &mut out);
    out
}

/// `targets` of a node `depth` nodes inside the target it started from.
fn targets_at<'t>(node: Node<'t>, depth: usize, out: &mut Vec<Node<'t>>) {
    match node.kind() {
        _ if depth >= TARGET_DEPTH => {}
        "identifier" | "attribute" => out.push(node),
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

/// The names a `case` pattern `depth` nodes deep captures: a bare name, the name after `as` or
/// `*`, and those inside sequences, mappings and class patterns, to `TARGET_DEPTH` nodes deep;
/// not a class's name or a dotted value.
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

/// What an annotation says, read as written: a name, or a string holding one, through
/// `Optional[...]` and `... | None`, to `TYPE_DEPTH` levels; deeper is `Unknown`.
fn class(annotation: Node, src: &[u8]) -> Class {
    class_at(annotation, src, 0)
}

/// `class` of an annotation `depth` levels inside the one it started from.
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

/// The expression a `type` node wraps.
fn bare(mut node: Node) -> Node {
    while node.kind() == "type"
        && let Some(inner) = named(node).next()
    {
        node = inner;
    }
    node
}

/// `Named` for a dotted name (`Circle`, `shapes.Circle`), whitespace aside.
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

/// The bases a class `node` lists, as callee-like names; a generic base `B[T]` is `B`, and any
/// other expression is `(<node kind>)`, a base whose members are not known. Keywords such as
/// `metaclass=` are not bases.
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

/// Whether the definition `node` carries the decorator `name`.
fn decorated(node: Node, name: &str, src: &[u8]) -> bool {
    node.parent()
        .filter(|parent| parent.kind() == "decorated_definition")
        .is_some_and(|parent| {
            children(parent)
                .filter(|c| c.kind() == "decorator")
                .any(|d| named(d).next().is_some_and(|e| text(e, src) == name))
        })
}

/// The identifier a parameter pattern binds: `*args` and `x: int` bind `args` and `x`.
fn identifier(mut node: Node) -> Option<Node> {
    while node.kind() != "identifier" {
        node = named(node).next()?;
    }
    Some(node)
}

/// The bindings of `import a.b.c` (which binds `a`, `a.b` and `a.b.c`, each to its module),
/// `import a.b as c`, `from a.b import c as d` and `from a import *`. `source` is the module
/// as written: leading dots make it relative to the importing file's package.
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
    // The name an `aliased_import` imports, and the one it binds.
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
        // `from p import m` also names the submodule `m` of `p`, when `p` exports no `m`.
        let join = if source.ends_with('.') { "" } else { "." };
        let member = Imported::Member {
            name: name.to_string(),
            module: format!("{source}{join}{name}"),
        };
        add(local, source, member);
    }
}

/// The strings of a list or tuple literal of string literals; `None` for anything else.
fn string_list(node: Node, src: &[u8]) -> Option<Vec<String>> {
    if !matches!(node.kind(), "list" | "tuple") {
        return None;
    }
    named(node).map(|item| string(item, src)).collect()
}

/// The content of a string literal without prefix or quotes; `None` for another expression
/// or an f-string with interpolations.
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

/// The positional and keyword arguments of a call `node`, as written. A splat (`*a`, `**k`)
/// is not read.
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
            "comment" | "dictionary_splat" => {}
            _ => args.push(value(arg, src)),
        }
    }
    (args, kwargs)
}

/// What an expression is, as far as its own syntax tells.
fn value(node: Node, src: &[u8]) -> Value {
    value_at(node, src, 0)
}

/// `value` of a node `depth` levels inside the expression it started from.
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
        "call" => parts(node, src).map_or_else(
            || {
                Value::Call(Invocation {
                    callee: node
                        .child_by_field_name("function")
                        .map_or_else(String::new, |f| callee(f, src)),
                    receiver: None,
                    args: Vec::new(),
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
        "await" | "parenthesized_expression" => named(node).next().map_or(Value::Unknown, deeper),
        _ => parts(node, src).map_or(Value::Unknown, Value::Str),
    }
}

/// An entry of a dict literal as a property of `Value::Object`: a string literal key is the
/// key, any other key is `[]` holding the list of the key and the value; `**d` is a spread.
/// `depth` is the level of the values it reads.
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

/// A string expression folded into literal text, names and holes: string literals, f-strings,
/// implicit and `+` concatenation; `None` when `node` is not one.
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
    let inside = |n: Node| parts(n).unwrap_or_else(|| vec![Part::Hole]);
    match node.kind() {
        "string" => Some(
            children(node)
                .filter_map(|part| match part.kind() {
                    "string_content" => Some(Part::Text(text(part, src).to_string())),
                    "interpolation" => Some(match part.child_by_field_name("expression") {
                        Some(e) => (name(e, src).or_else(|| parts(e).and_then(single_env)))
                            .unwrap_or(Part::Hole),
                        None => Part::Hole,
                    }),
                    _ => None,
                })
                .collect(),
        ),
        "concatenated_string" => Some(named(node).flat_map(inside).collect()),
        "binary_operator" => {
            let left = node.child_by_field_name("left")?;
            let right = node.child_by_field_name("right")?;
            if text(node.child_by_field_name("operator")?, src) != "+" {
                return None;
            }
            // Each side once: a left-associated chain would cost twice per term otherwise.
            let (left, right) = (parts(left), parts(right));
            if left.is_none() && right.is_none() {
                return None;
            }
            let hole = || vec![Part::Hole];
            let (left, right) = (left.unwrap_or_else(hole), right.unwrap_or_else(hole));
            Some(left.into_iter().chain(right).collect())
        }
        // A fallback keeps the environment read on its left, if it is the only one.
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

/// A name or a member chain (`config.BASE`) inside a string, folded like `Value::Ref`.
fn name(node: Node, src: &[u8]) -> Option<Part> {
    match node.kind() {
        "identifier" | "attribute" => Some(callee(node, src))
            .filter(|name| !name.starts_with('('))
            .map(Part::Ref),
        _ => None,
    }
}

/// `os.environ["X"]`, `os.environ.get("X")` or `os.getenv("X")` (also `environ` and `getenv`
/// imported from `os`), with or without a default, also through string method calls such as
/// `.rstrip("/")`. Meada never reads the value, so a default is ignored; one that reads
/// another variable makes the whole read a hole. `depth` is the level of `node` in `parts`.
// ponytail: the names are matched as written, not through the file's imports, so `import os
// as o` is not read and a local `getenv` function is.
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
            let name = string(node.child_by_field_name("subscript")?, src)?;
            let environ = callee(node.child_by_field_name("value")?, src);
            matches!(environ.as_str(), "os.environ" | "environ").then_some(Part::Env(name))
        }
        "call" => {
            let function = node.child_by_field_name("function")?;
            if let "os.getenv" | "getenv" | "os.environ.get" | "environ.get" =
                callee(function, src).as_str()
            {
                let list = node.child_by_field_name("arguments")?;
                let name = string(named(list).next()?, src)?;
                // The second positional argument, else the `default` keyword, read as `parts`
                // at the next level rather than as a whole value.
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
                    _ => Part::Env(name),
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

/// The environment read that makes up all of `parts`, if that is what they are.
fn single_env(parts: Vec<Part>) -> Option<Part> {
    match <[Part; 1]>::try_from(parts) {
        Ok([env @ Part::Env(_)]) => Some(env),
        _ => None,
    }
}

/// `a.b.c` for identifier and attribute chains; any other expression becomes `(<node kind>)`,
/// so no source text beyond names reaches the index.
fn callee(node: Node, src: &[u8]) -> String {
    // The attributes of the chain, outermost first; a chain longer than `FOLD_DEPTH` ends in
    // the kind of the node where it is cut.
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

    /// How many levels deep the first item of `value` goes.
    fn nesting(value: &Value) -> usize {
        match value {
            Value::List(items) => 1 + items.first().map_or(0, nesting),
            Value::Object(entries) => 1 + entries.first().map_or(0, |(_, v)| nesting(v)),
            _ => 0,
        }
    }

    fn facts(src: &str) -> Facts {
        Python.parse("a.py", src.as_bytes()).unwrap()
    }

    #[test]
    fn a_main_guard_is_a_program_whose_symbol_holds_its_calls() {
        let src = "def run():\n    pass\n\nif __name__ == \"__main__\":\n    run()\n    x = 1\nelse:\n    other()\n";
        let f = facts(src);

        assert_eq!(f.programs, ["__main__"]);
        let main = f.defs.iter().find(|d| d.name == "__main__").unwrap();
        // The `if` block up to its last body line: the `else` is not part of it.
        assert_eq!(
            &src[main.range.clone()],
            "if __name__ == \"__main__\":\n    run()\n    x = 1"
        );
        let scopes: Vec<(&str, &str)> = (f.calls.iter())
            .map(|c| (c.callee.as_str(), c.scope.as_str()))
            .collect();
        assert_eq!(scopes, [("run", "__main__"), ("other", "")]);
        // The block binds no export: a module-level name stays what its file exports.
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
        // Only top-level code starts the program: not a guard inside a function or a class.
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

        // A function of that name defined first wins: the guard is no program.
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

        let Value::Call(call) = held(&consts, "f.nc") else {
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
        // A key that is no string literal travels with its value, to be folded with it.
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
        // Far past the limit: a recursion that does not stop overflows the stack.
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
        // Left-associated: reading each side twice would take 2^64 steps.
        let src = format!("x = \"/x\" + {}\"y\"\n", "b + ".repeat(64));

        let Value::Str(parts) = held(&consts(&src), "x") else {
            unreachable!("not a string");
        };

        assert_eq!(parts.len(), 66);
    }

    #[test]
    fn long_chains_are_cut() {
        // Far past the limit: a recursion that does not stop overflows the stack.
        let n = 30_000;
        let sum = consts(&format!("a = '' + {}b\n", "b + ".repeat(n)));
        let Value::Str(parts) = held(&sum, "a") else {
            unreachable!("not a string");
        };
        assert!(parts.len() <= FOLD_DEPTH + 2, "{}", parts.len());

        let fallbacks = format!("a = {}os.getenv(\"X\")\n", "os.getenv(\"X\") or ".repeat(n));
        // Every call's arguments are folded again, up to `FOLD_DEPTH` each: fewer levels keep
        // the test fast.
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

    /// The class `facts` binds `name` to in `scope`, if it binds it.
    fn bound<'f>(facts: &'f Facts, scope: &str, name: &str) -> Option<&'f Class> {
        (facts.receivers.iter())
            .find(|r| r.scope == scope && r.name == name)
            .map(|r| &r.class)
    }

    #[test]
    fn definitions_nest_to_a_limit() {
        // Classes and functions in turn, each with a call in its body.
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
        // Its name and parameters are locals of the definition around it.
        assert!(matches!(bound(&f, &call.scope, "g"), Some(Class::Unknown)));
        assert!(matches!(bound(&f, &call.scope, "x"), Some(Class::Unknown)));
        // Its `return x` is not the one of the definition around it.
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
        // Just inside the limit, the name is bound.
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
}
