//! The Swift language pack: one source file's syntax, with no Swift toolchain or Xcode.
//! `Package.swift` says which files form each module, one per SwiftPM target (`manifest`); the
//! files no target holds, all of them in a repository without one, form one module. The files of
//! a module share its scope (`layout`), and `import Module` reaches the public declarations of
//! the module it names (`join`). A function's name carries its argument labels
//! (`place(order:quantity:)`), and so does each of its parameters (`Param::label`) and each call
//! (`Call::labels`): the shared resolution narrows overloads by them. An extension is no type:
//! its members are the extended type's (`Facts::impls`). The values a call, a local and a
//! function write are in `values`.

mod manifest;
mod values;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};

use super::kotlin::{bare_calls, wildcard};
use super::{
    Binds, Bound, Call, Class, Const, Def, Export, Facts, Impl, Import, Imported, Join, Kind,
    LanguagePack, Layout, Modules, Param, Receiver, Star, Value, children, named, text,
};
use crate::{Result, qualify};
use values::Ctx;

/// How many named types and functions may enclose a definition. Deeper bodies keep their
/// enclosing symbol.
// ponytail: real source nests a few types; this prevents quadratic qualified names.
const DEFINITION_DEPTH: usize = 128;
/// How many segments of a callee are kept before it becomes an unknown expression.
// ponytail: real member paths are short; this keeps crafted chains cheap.
const PATH_DEPTH: usize = 256;
/// Enclosing-name bytes the definitions, calls and bindings of one file may store. A file past
/// it keeps no facts: a partial overload family could turn an ambiguous call into an exact one.
// ponytail: a long nested name must not multiply across a broad file without a budget.
const QUALIFICATION_BYTES: usize = 4 * 1024 * 1024;

/// The module of the files no target of a `Package.swift` holds, which no import can name.
const NO_TARGET: &str = "(no target)";

pub struct Swift;

impl LanguagePack for Swift {
    fn extensions(&self) -> &'static [&'static str] {
        &["swift"]
    }

    /// `Package.swift` is the package's manifest, read as a config file (`manifest`).
    fn is_code(&self, path: &str) -> bool {
        path.rsplit('/').next() != Some("Package.swift")
    }

    fn parse(&self, _path: &str, src: &[u8]) -> Result<Facts> {
        parse(src)
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["**/Package.swift"]
    }

    /// Each file's module (`Facts::unit`, whose `internal` declarations no other module sees)
    /// and the module's own names, which come before what an import brings: the inside glob,
    /// read first as the last import.
    fn join(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Join {
        let targets = targets(configs);
        // The type parameters and associated types of each type, in whichever file declares it:
        // an extension's body sees those of the type it extends.
        let mut generics: HashMap<String, HashSet<String>> = HashMap::new();
        for def in files.values().flat_map(|f| &f.defs) {
            if !def.type_params.is_empty() {
                (generics.entry(def.name.clone()).or_default())
                    .extend(def.type_params.iter().cloned());
            }
        }
        Box::new(move |path, facts| {
            let module = module_of(&targets, path);
            wildcard(module, true, facts);
            facts.unit = module.to_string();
            for call in facts.calls.iter_mut().filter(|c| c.constructs) {
                let ty = head(&call.callee);
                let held =
                    enclosing(&call.scope).any(|t| generics.get(t).is_some_and(|g| g.contains(ty)));
                call.constructs = !held;
            }
            for receiver in &mut facts.receivers {
                let (Class::Type(ty) | Class::Named(ty)) = &receiver.class else {
                    continue;
                };
                let ty = head(ty);
                if enclosing(&receiver.scope)
                    .any(|t| generics.get(t).is_some_and(|g| g.contains(ty)))
                {
                    receiver.class = Class::Unknown;
                }
            }
        })
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        Ok(self.layout(configs, files)?.modules)
    }

    /// The files of a module share its scope, and an import of the module's name names them
    /// all through its first file. A module that is no target of the repository is outside it.
    fn layout(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Result<Layout> {
        let targets = targets(configs);
        let mut modules: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for path in files.keys() {
            let module = module_of(&targets, path).to_string();
            modules.entry(module).or_default().push(path.to_string());
        }
        let first: HashMap<String, String> = (modules.iter())
            .map(|(module, files)| (module.clone(), files[0].clone()))
            .collect();
        Ok(Layout {
            members: None,
            modules: Box::new(move |_, name| first.get(name).cloned().into_iter().collect()),
            published: None,
            programs: Vec::new(),
            reaches: None,
            shared: modules.into_values().filter(|f| f.len() > 1).collect(),
        })
    }

    fn receiver(&self) -> Option<&'static str> {
        Some("self")
    }

    fn base_receiver(&self) -> Option<&'static str> {
        Some("super")
    }

    fn members_in_scope(&self) -> bool {
        true
    }

    fn unordered_globs(&self) -> bool {
        true
    }

    fn namespaced(&self) -> bool {
        false
    }

    /// A type's conformances may be written in any order.
    fn unordered_bases(&self) -> bool {
        true
    }
}

/// The module of each target directory the manifests declare (empty at the root); of two
/// targets of one directory, the last by name.
fn targets(configs: &[(&str, &[u8])]) -> HashMap<String, String> {
    (configs.iter())
        .filter(|(path, _)| !Swift.is_code(path))
        .flat_map(|(path, src)| manifest::targets(path, src))
        .map(|t| (t.dir, t.name))
        .collect()
}

/// The module of the file `path`: the target whose directory holds it, the deepest one when
/// several do, else `NO_TARGET`. Its directories are looked up, so the cost is the path's depth.
// ponytail: two packages of the repository that name a target alike make one module of both.
fn module_of<'t>(targets: &'t HashMap<String, String>, path: &str) -> &'t str {
    let dirs = (path.rmatch_indices('/').map(|(at, _)| &path[..at])).chain([""]);
    dirs.into_iter()
        .find_map(|dir| targets.get(dir))
        .map_or(NO_TARGET, String::as_str)
}

/// Swift's access levels, least first (`open` is `public`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Access {
    Private,
    FilePrivate,
    Internal,
    Public,
}

/// Where a node sits: the file's top level, a type's body (a type, an extension, a protocol),
/// or code (a function's body, a closure, a property's initializer or accessors).
#[derive(Clone, Copy, PartialEq)]
enum Place {
    Top,
    Type,
    Code,
}

/// The type parameters and associated types a declaration brings, and those of the
/// declarations around it.
#[derive(Default)]
struct Generics {
    names: HashSet<String>,
    outer: Option<Rc<Generics>>,
}

impl Generics {
    fn holds(&self, name: &str) -> bool {
        self.names.contains(name) || self.outer.as_ref().is_some_and(|o| o.holds(name))
    }
}

struct Visit<'t> {
    node: Node<'t>,
    /// The definition around the node, empty at the top level.
    scope: Rc<String>,
    place: Place,
    /// The access a declaration has when it writes none, and at most.
    default: Access,
    ceiling: Access,
    /// How many definitions enclose the node.
    depth: usize,
    /// `scope` with every function named by its labels alone (`Signature::name`), which the
    /// first walk of `parse` names them all by.
    plain: Rc<String>,
    /// The type parameters and associated types of the declarations around the node.
    generics: Rc<Generics>,
    /// Whether the node is, or is directly in, the body of a function the walk defines: the
    /// statements of that body, not the ones nested in a block, a closure or an accessor.
    body: bool,
}

/// A function, method or initializer's parameters: each with its argument label, and the
/// type it is written with.
struct Signature {
    base: String,
    params: Vec<Param>,
    types: Vec<String>,
    /// What each parameter's written type says it holds.
    classes: Vec<Class>,
}

impl Signature {
    /// `base(label:label:)`, or with each label's type (`audit(_:Order)`) when another
    /// declaration of the same scope in the file (a type and its extensions alike) has the same
    /// labels and other types.
    fn name(&self, typed: bool) -> String {
        let labels = self
            .params
            .iter()
            .map(|p| p.label.as_deref().unwrap_or("_"));
        let inner: String = match typed {
            false => labels.map(|l| format!("{l}:")).collect(),
            true => (labels.zip(&self.types))
                .map(|(l, t)| format!("{l}:{t}"))
                .collect::<Vec<_>>()
                .join(","),
        };
        format!("{}({inner})", self.base)
    }
}

fn parse(src: &[u8]) -> Result<Facts> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_swift::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let root = tree.root_node();
    // The first walk finds the functions of one scope with the same labels and other types
    // (a type's body and its extensions in the file alike); the second names those with types.
    let mut signatures = HashMap::new();
    walk(root, src, &HashSet::new(), &mut signatures);
    let typed: HashSet<String> = (signatures.into_iter())
        .filter(|(_, types)| types.len() > 1)
        .map(|(name, _)| name)
        .collect();
    Ok(walk(root, src, &typed, &mut HashMap::new()))
}

/// The facts of the file `root`, naming the functions of `typed` (by `Visit::plain` name) with
/// their parameters' types too, and recording in `signatures` the types of each function by
/// that name.
fn walk(
    root: Node,
    src: &[u8],
    typed: &HashSet<String>,
    signatures: &mut HashMap<String, HashSet<Vec<String>>>,
) -> Facts {
    let mut facts = Facts::default();
    for node in named(root).filter(|n| n.kind() == "import_declaration") {
        import(node, src, &mut facts);
    }
    // The function families (`Facts::overloads`) of the top level, each with the widest
    // access of its functions.
    let mut families: BTreeMap<String, Access> = BTreeMap::new();
    let mut defined: HashSet<String> = HashSet::new();
    // The types, whose scope a closure of their initializers is no definition in; and the values
    // each function returns (`Def::return_values`).
    let mut types: HashSet<String> = HashSet::new();
    let mut returns: HashMap<String, Vec<Value>> = HashMap::new();
    let mut budget = QUALIFICATION_BYTES;
    // The names assigned (`x = v`, `x += v`, `&x`) anywhere in the file, which hold no one value,
    // and the members assigned through a name (`request.httpMethod = v`).
    let mut assigned: HashSet<String> = HashSet::new();
    let mut members: Vec<Member> = Vec::new();
    let mut stack = vec![Visit {
        node: root,
        scope: Rc::new(String::new()),
        place: Place::Top,
        default: Access::Internal,
        ceiling: Access::Public,
        depth: 0,
        plain: Rc::new(String::new()),
        generics: Rc::default(),
        body: false,
    }];
    while let Some(Visit {
        node,
        scope,
        place,
        default,
        ceiling,
        depth,
        plain,
        generics,
        body,
    }) = stack.pop()
    {
        let charge = if stores(node.kind()) { scope.len() } else { 0 };
        let Some(left) = budget.checked_sub(charge) else {
            return Facts {
                unbuilt: true,
                ..Facts::default()
            };
        };
        budget = left;
        let defining = depth < DEFINITION_DEPTH;
        let (mut inner, mut inner_place, mut inner_plain) = (scope.clone(), place, plain.clone());
        let mut inner_generics = generics.clone();
        let mut defines_body = false;
        let (mut inner_default, mut inner_ceiling, mut nested) = (default, ceiling, depth);
        let mut pushed: Vec<Node> = named(node).collect();
        match node.kind() {
            "class_declaration" | "protocol_declaration" if defining => {
                let extension = children(node).any(|c| c.kind() == "extension");
                let Some(name) = node.child_by_field_name("name") else {
                    continue;
                };
                let name = type_name(name, src);
                let written = access(node, src);
                let bases = bases(node, src);
                inner_generics = with_generics(&generics, node, src);
                let type_params = own_generics(node, src);
                if extension {
                    // Members of an extension are the extended type's, wherever it is declared.
                    if place != Place::Top {
                        continue;
                    }
                    inner = Rc::new(name.clone());
                    types.insert(name.clone());
                    inner_plain = inner.clone();
                    inner_default = written.unwrap_or(Access::Internal);
                    inner_ceiling = Access::Public;
                    let blocks = std::iter::once(None).chain(bases.into_iter().map(Some));
                    for of in blocks {
                        facts.impls.push(Impl {
                            scope: name.clone(),
                            ty: name.clone(),
                            of,
                            within: String::new(),
                        });
                    }
                } else {
                    inner = Rc::new(qualify(&scope, &name));
                    types.insert(inner.as_ref().clone());
                    inner_plain = Rc::new(qualify(&plain, &name));
                    let own = written.unwrap_or(default).min(ceiling);
                    let protocol = node.kind() == "protocol_declaration";
                    // A protocol's requirements have its access.
                    (inner_default, inner_ceiling) = match protocol {
                        true => (own, own),
                        false => (Access::Internal, own),
                    };
                    if protocol {
                        facts.interfaces.push(inner.as_ref().clone());
                    }
                    if defined.insert(inner.as_ref().clone()) {
                        facts.defs.push(Def {
                            type_params,
                            ..definition(&inner, Kind::Class, node.byte_range(), bases)
                        });
                        publish(&mut facts, place, own, &inner, &name);
                    }
                }
                inner_place = Place::Type;
                nested += 1;
            }
            "function_declaration" | "protocol_function_declaration" | "init_declaration"
                if defining =>
            {
                inner_place = Place::Code;
                inner_generics = with_generics(&generics, node, src);
                let signature = signature(node, src, &inner_generics);
                let init = signature.as_ref().is_some_and(|s| s.base == "init");
                let Some(signature) = signature.filter(|_| !init || place == Place::Type) else {
                    stack_children(&mut stack, &pushed, &scope, &plain, &inner_generics, depth);
                    continue;
                };
                inner_plain = Rc::new(qualify(&plain, &signature.name(false)));
                (signatures.entry(inner_plain.as_ref().clone()).or_default())
                    .insert(signature.types.clone());
                let typed = typed.contains(inner_plain.as_str());
                inner = Rc::new(qualify(&scope, &signature.name(typed)));
                nested += 1;
                let own = access(node, src).unwrap_or(default).min(ceiling);
                // A protocol extension's default implementation shares the requirement's name.
                if !defined.insert(inner.as_ref().clone()) {
                    stack_children(
                        &mut stack,
                        &pushed,
                        &inner,
                        &inner_plain,
                        &inner_generics,
                        nested,
                    );
                    continue;
                }
                if let Some(value) =
                    values::implicit_return(node, src, ctx(&inner, defining, &types))
                {
                    returns
                        .entry(inner.as_ref().clone())
                        .or_default()
                        .push(value);
                }
                defines_body = true;
                let kind = match place {
                    Place::Type => Kind::Method,
                    _ => Kind::Function,
                };
                let family = qualify(&scope, &signature.base);
                facts
                    .overloads
                    .entry(family.clone())
                    .or_default()
                    .push(inner.as_ref().clone());
                if init {
                    // `Type(..)` constructs through one of them.
                    facts
                        .constructors
                        .entry(scope.as_ref().clone())
                        .or_default()
                        .push(inner.as_ref().clone());
                }
                for (param, class) in signature.params.iter().zip(signature.classes) {
                    if class != Class::Unknown {
                        facts.receivers.push(Receiver {
                            scope: inner.as_ref().clone(),
                            name: param.name.clone(),
                            class,
                        });
                    }
                }
                facts.defs.push(Def {
                    params: signature.params,
                    ..definition(&inner, kind, node.byte_range(), Vec::new())
                });
                if place == Place::Top {
                    let widest = families.entry(family).or_insert(Access::Private);
                    *widest = (*widest).max(own);
                }
                if place == Place::Type && own < Access::Public {
                    facts.hidden.push(inner.as_ref().clone());
                }
                if place != Place::Code && own < Access::Public {
                    facts.private.push(inner.as_ref().clone());
                }
            }
            // A deinitializer, a subscript and an accessor are code of the type around them.
            "deinit_declaration" | "subscript_declaration" | "computed_property" => {
                inner_place = Place::Code;
            }
            "property_declaration" => {
                let constant = children(node).any(|c| {
                    c.kind() == "value_binding_pattern"
                        && c.child_by_field_name("mutability")
                            .is_some_and(|m| text(m, src) == "let")
                });
                let own = access(node, src).unwrap_or(default).min(ceiling);
                let held = values::initializer(node, src, ctx(&scope, defining, &types));
                let names = bound(node, src);
                // A typed local is a receiver of its function alone when it is directly in the
                // function's body: in a nested block, a closure or an accessor its name would
                // reach code outside it (an accessor's and a closure's scope is the type's or the
                // file's), so it only hides the name.
                let top =
                    place == Place::Top && node.parent().is_some_and(|p| p.kind() == "source_file");
                // A property in a block of the top level is a local, not a global.
                let place = match place == Place::Top && !top {
                    true => Place::Code,
                    false => place,
                };
                let typed_here = top || (body && depth < DEFINITION_DEPTH);
                let class = match names.len() {
                    1 => declared(node, src, &generics),
                    _ => Class::Unknown,
                };
                for name in names {
                    let receiver = Receiver {
                        scope: scope.as_ref().clone(),
                        name: name.clone(),
                        class: class.clone(),
                    };
                    match place {
                        Place::Type => {
                            if constant {
                                facts.consts.push(Const {
                                    name: qualify(&scope, &name),
                                    value: held.clone().unwrap_or(Value::Unknown),
                                    ty: None,
                                    scope: None,
                                });
                            }
                            facts.receivers.push(receiver)
                        }
                        Place::Top if constant => {
                            facts.consts.push(Const {
                                name: name.clone(),
                                value: held.clone().unwrap_or(Value::Unknown),
                                ty: None,
                                scope: None,
                            });
                            publish(&mut facts, place, own, &name, &name);
                            if class != Class::Unknown && top {
                                facts.receivers.push(receiver);
                            }
                        }
                        // A typed local or global is a receiver, an untyped one hides its name.
                        // Either holds the call, the name or the string it is given (a `var`
                        // assigned later holds none: `assigned`).
                        _ => {
                            if let (
                                Some(value @ (Value::Call(_) | Value::Ref(_) | Value::Str(_))),
                                true,
                            ) = (&held, defining)
                            {
                                facts.bound.push(Bound {
                                    scope: scope.as_ref().clone(),
                                    within: scope.as_ref().clone(),
                                    name: name.clone(),
                                    value: value.clone(),
                                });
                            }
                            match class != Class::Unknown && typed_here {
                                true => facts.receivers.push(receiver),
                                false => facts.locals.push(qualify(&scope, &name)),
                            }
                        }
                    }
                }
                // Its names are no locals of the code its initializer and accessors are.
                pushed.retain(|n| n.kind() != "pattern");
                inner_place = Place::Code;
            }
            "lambda_parameter" | "pattern" | "catch_block" | "if_statement" | "guard_statement"
            | "while_statement"
                if place != Place::Type =>
            {
                for name in bound(node, src) {
                    facts.locals.push(qualify(&scope, &name));
                }
            }
            "lambda_literal" => {
                inner_place = Place::Code;
                // A closure written in code is a definition of its own, which a call's argument
                // names (`values`): a contract kind may claim it as a handler, and it folds into
                // the symbol around it otherwise.
                if ctx(&scope, defining, &types).inline {
                    inner = Rc::new(values::inline_name(&scope, node));
                    facts.defs.push(definition(
                        &inner,
                        Kind::Inline,
                        node.byte_range(),
                        Vec::new(),
                    ));
                    nested += 1;
                }
            }
            "control_transfer_statement" if text(node, src).starts_with("return") => {
                if let Some(result) = node.child_by_field_name("result") {
                    let value = values::value(result, src, ctx(&scope, defining, &types));
                    returns
                        .entry(scope.as_ref().clone())
                        .or_default()
                        .push(value);
                }
            }
            "assignment" => {
                if let Some(target) = node
                    .child_by_field_name("target")
                    .and_then(|t| named(t).next())
                {
                    let name = callee(target, src);
                    let plain = children(node).any(|c| c.kind() == "=");
                    let result = node.child_by_field_name("result");
                    match name.split_once('.') {
                        None => {
                            assigned.insert(name);
                        }
                        Some((head, rest)) if !name.contains('(') => {
                            // Only a plain `=` as a statement of the body runs once, in place.
                            let direct = plain
                                && !rest.contains('.')
                                && node.parent().is_some_and(|p| {
                                    p.kind() == "source_file"
                                        || (p.kind() == "statements"
                                            && p.parent().is_some_and(|b| {
                                                matches!(
                                                    b.kind(),
                                                    "function_body" | "lambda_literal"
                                                )
                                            }))
                                });
                            let value = match (direct, result) {
                                (true, Some(r)) => {
                                    values::value(r, src, ctx(&scope, defining, &types))
                                }
                                _ => Value::Unknown,
                            };
                            members.push(Member {
                                scope: scope.as_ref().clone(),
                                head: head.to_string(),
                                member: rest.split('.').next().unwrap_or(rest).to_string(),
                                value,
                                direct,
                            });
                        }
                        _ => {}
                    }
                }
            }
            "prefix_expression" if text(node, src).starts_with('&') => {
                if let Some(target) = node.child_by_field_name("target") {
                    assigned.insert(head(&callee(target, src)).to_string());
                }
            }
            "call_expression" | "constructor_expression" => {
                // `f(a) { }` may parse as a call of `f(a)`: it is one call of `f`.
                let (function, suffixes) = parts(node);
                if let Some(mut at) = call(function, &suffixes, node, src, &scope) {
                    values::fill(
                        &mut at,
                        function,
                        &suffixes,
                        src,
                        ctx(&scope, defining, &types),
                    );
                    // A type parameter is no type of the repository: `T()` constructs none.
                    at.constructs &= !generics.holds(head(&at.callee));
                    facts.calls.push(at);
                }
                pushed = std::iter::once(function)
                    .chain(suffixes.iter().copied())
                    .collect();
            }
            _ => {}
        }
        for &child in pushed.iter().rev() {
            let child_body = match node.kind() {
                "function_declaration" | "protocol_function_declaration" | "init_declaration" => {
                    defines_body && child.kind() == "function_body"
                }
                "function_body" => body && child.kind() == "statements",
                "statements" => body,
                _ => false,
            };
            stack.push(Visit {
                node: child,
                scope: inner.clone(),
                place: inner_place,
                default: inner_default,
                ceiling: inner_ceiling,
                depth: nested,
                plain: inner_plain.clone(),
                generics: inner_generics.clone(),
                body: child_body,
            });
        }
    }
    for (family, widest) in families {
        if widest < Access::Internal {
            continue;
        }
        if widest < Access::Public {
            facts.private.push(family.clone());
        }
        facts.exports.push(Export::Local {
            exported: family.clone(),
            local: family,
        });
    }
    for def in &mut facts.defs {
        if let Some(values) = returns.remove(&def.name)
            && def.kind != Kind::Class
        {
            def.return_values = values;
        }
    }
    bare_calls(&mut facts);
    facts.bound.retain(|b| !assigned.contains(&b.name));
    assign_members(&mut facts.bound, members);
    // A name another binding of the scope also declares (a loop variable, a `guard let`, a
    // `var`) holds no one value: its `let` bounds are dropped (Kotlin's rule). Two `let`s keep
    // both bounds.
    let held = facts.bound.iter().fold(HashMap::new(), |mut held, b| {
        *held.entry(qualify(&b.scope, &b.name)).or_insert(0) += 1;
        held
    });
    let declared = facts.locals.iter().fold(HashMap::new(), |mut n, l| {
        *n.entry(l.as_str()).or_insert(0) += 1;
        n
    });
    let shared: HashSet<String> = (held.iter())
        .filter(|(name, bounds)| declared.get(name.as_str()).is_some_and(|d| d > *bounds))
        .map(|(name, _)| name.clone())
        .collect();
    facts
        .bound
        .retain(|b| !shared.contains(&qualify(&b.scope, &b.name)));
    facts.locals.sort();
    facts.locals.dedup();
    facts.receivers = one_class_each(std::mem::take(&mut facts.receivers), &facts.locals);
    let private: HashSet<&str> = facts.private.iter().map(String::as_str).collect();
    facts.star = Star::Only(
        (facts.exports.iter())
            .filter_map(|e| match e {
                Export::Local { exported, .. } if !private.contains(exported.as_str()) => {
                    Some(exported.clone())
                }
                _ => None,
            })
            .collect(),
    );
    facts
}

/// A member assigned through a name (`request.httpMethod = v`): the scope written in, the name,
/// the member, its value, and whether the assignment runs once, in place (a statement of a body).
struct Member {
    scope: String,
    head: String,
    member: String,
    value: Value,
    direct: bool,
}

/// Gives the call a local holds the members assigned through its name, as `<member>=` keyword
/// arguments (Kotlin's `method=`): the value of the one assignment that runs in place, unknown
/// when it is written twice, may not run, or the name holds no call.
fn assign_members(bound: &mut [Bound], members: Vec<Member>) {
    for b in bound.iter_mut() {
        let mut written: Vec<(String, Value)> = Vec::new();
        for m in members.iter().filter(|m| m.head == b.name) {
            let inside = m.scope == b.scope || m.scope.starts_with(&format!("{}.", b.scope));
            if !inside {
                continue;
            }
            let key = format!("{}=", m.member);
            let value = match m.direct && m.scope == b.scope {
                true => m.value.clone(),
                false => Value::Unknown,
            };
            match written.iter_mut().find(|(k, _)| *k == key) {
                Some((_, held)) => *held = Value::Unknown,
                None => written.push((key, value)),
            }
        }
        if written.is_empty() {
            continue;
        }
        match &mut b.value {
            Value::Call(call) => call.kwargs.extend(written),
            other => *other = Value::Unknown,
        }
    }
}

/// Pushes the `children` of a function's declaration to be visited in source order, as its
/// code, under `scope` (`plain`).
fn stack_children<'t>(
    stack: &mut Vec<Visit<'t>>,
    children: &[Node<'t>],
    scope: &Rc<String>,
    plain: &Rc<String>,
    generics: &Rc<Generics>,
    depth: usize,
) {
    for &node in children.iter().rev() {
        stack.push(Visit {
            node,
            scope: scope.clone(),
            place: Place::Code,
            default: Access::Private,
            ceiling: Access::Private,
            depth,
            plain: plain.clone(),
            generics: generics.clone(),
            body: false,
        });
    }
}

/// Whether visiting a node of this kind may store its scope's name (a definition, a call, a
/// binding), which `QUALIFICATION_BYTES` charges.
fn stores(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "protocol_declaration"
            | "function_declaration"
            | "protocol_function_declaration"
            | "init_declaration"
            | "property_declaration"
            | "call_expression"
            | "constructor_expression"
            | "lambda_literal"
            | "lambda_parameter"
            | "pattern"
            | "catch_block"
            | "if_statement"
            | "guard_statement"
            | "while_statement"
    )
}

/// Records a top-level type or constant of `access` as the file's export `name`; one only its
/// module sees is `private`, and one only its file sees is no export. A function's family is
/// exported once, after the walk.
fn publish(facts: &mut Facts, place: Place, access: Access, qualified: &str, name: &str) {
    if access < Access::Public {
        facts.private.push(qualified.to_string());
    }
    if place != Place::Top || access < Access::Internal {
        return;
    }
    facts.exports.push(Export::Local {
        exported: name.to_string(),
        local: qualified.to_string(),
    });
}

/// `import Module` (or `import struct Module.Type`): the module's public names on demand,
/// and its name as a qualifier (`Module.f()`).
// ponytail: `@testable import` sees no `internal` declarations here, and an import of one
// declaration takes the whole module.
fn import(node: Node, src: &[u8], facts: &mut Facts) {
    let Some(path) = named(node).find(|n| n.kind() == "identifier") else {
        return;
    };
    let Some(module) = named(path).next().map(|n| unbacktick(text(n, src))) else {
        return;
    };
    facts.imports.push(Import {
        local: module.clone(),
        source: module.clone(),
        name: Imported::Namespace,
    });
    wildcard(&module, false, facts);
}

/// A function, method or initializer's name and parameters. `None` for an operator.
fn signature(node: Node, src: &[u8], generics: &Generics) -> Option<Signature> {
    let base = match node.kind() {
        "init_declaration" => "init".to_string(),
        _ => {
            let name = node.child_by_field_name("name")?;
            if name.kind() != "simple_identifier" {
                return None;
            }
            unbacktick(text(name, src))
        }
    };
    let (mut params, mut types, mut classes) = (Vec::new(), Vec::new(), Vec::new());
    for child in children(node) {
        match child.kind() {
            "parameter" => {
                let mut cursor = child.walk();
                let names: Vec<Node> = child.children_by_field_name("name", &mut cursor).collect();
                let name = names
                    .first()
                    .map_or_else(String::new, |n| unbacktick(text(*n, src)));
                let label = (child.child_by_field_name("external_name"))
                    .map_or_else(|| name.clone(), |n| unbacktick(text(n, src)));
                let variadic = children(child).any(|c| c.kind() == "...");
                params.push(Param {
                    name,
                    binds: if variadic { Binds::Rest } else { Binds::Any },
                    label: Some(label),
                    default: None,
                    fields: Vec::new(),
                });
                let ty = names.get(1).map_or("", |t| text(*t, src));
                types.push(ty.split_whitespace().collect());
                classes.push(match variadic {
                    true => Class::Unknown,
                    false => names
                        .get(1)
                        .map_or(Class::Unknown, |t| class_of(*t, src, generics)),
                });
            }
            // A default value follows its parameter.
            "=" => {
                if let Some(last) = params.last_mut().filter(|p| p.binds == Binds::Any) {
                    last.binds = Binds::Optional;
                }
            }
            _ => {}
        }
    }
    Some(Signature {
        base,
        params,
        types,
        classes,
    })
}

/// The access a declaration writes (`public`, `open`, `internal`, `fileprivate`, `private`),
/// not a setter's (`private(set)`).
fn access(node: Node, src: &[u8]) -> Option<Access> {
    let modifiers = named(node).find(|n| n.kind() == "modifiers")?;
    named(modifiers)
        .filter(|n| n.kind() == "visibility_modifier")
        .find_map(|n| match text(n, src) {
            "public" | "open" => Some(Access::Public),
            "internal" => Some(Access::Internal),
            "fileprivate" => Some(Access::FilePrivate),
            "private" => Some(Access::Private),
            _ => None,
        })
}

/// The types a type declaration or an extension writes after its name, as callee-like names.
/// `AnyObject` is a constraint on the kind of type, not one with members.
fn bases(node: Node, src: &[u8]) -> Vec<String> {
    named(node)
        .filter(|n| n.kind() == "inheritance_specifier")
        .filter_map(|n| n.child_by_field_name("inherits_from"))
        .map(|t| type_name(t, src))
        .filter(|t| !t.is_empty() && t != "AnyObject")
        .collect()
}

/// A written type as a callee-like name, without its type arguments: `Outer.Inner` for
/// `Outer<T>.Inner`; empty for a type that is no name (a tuple, a function type).
fn type_name(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "type_identifier" | "simple_identifier" => unbacktick(text(node, src)),
        "user_type" => (named(node))
            .filter(|n| n.kind() == "type_identifier")
            .map(|n| unbacktick(text(n, src)))
            .collect::<Vec<_>>()
            .join("."),
        _ => String::new(),
    }
}

/// What a written type says a value holds: an instance of the type it names, through `?`,
/// `any` and `some`. An array, a tuple or a function type holds no instance of a type's members.
fn class_of(node: Node, src: &[u8], generics: &Generics) -> Class {
    match node.kind() {
        "user_type" => match type_name(node, src) {
            name if name.is_empty() => Class::Unknown,
            // A type parameter is no type of the repository, whatever a type is called there.
            name if generics.holds(head(&name)) => Class::Unknown,
            name => Class::Type(name),
        },
        "optional_type" => (node.child_by_field_name("wrapped"))
            .map_or(Class::Unknown, |t| class_of(t, src, generics)),
        "existential_type" | "opaque_type" => named(node)
            .next()
            .map_or(Class::Unknown, |t| class_of(t, src, generics)),
        _ => Class::Unknown,
    }
}

/// The type parameters a declaration brings (`<T: P>`) and, for a protocol, its associated
/// types.
fn own_generics(node: Node, src: &[u8]) -> Vec<String> {
    let own = children(node).filter(|c| c.kind() == "type_parameters");
    let parameters = own.flat_map(named).filter_map(|p| named(p).next());
    let body = named(node).find(|n| n.kind() == "protocol_body");
    let associated = (body.into_iter().flat_map(named))
        .filter(|n| n.kind() == "associatedtype_declaration")
        .filter_map(|n| n.child_by_field_name("name"));
    parameters
        .chain(associated)
        .map(|n| unbacktick(text(n, src)))
        .collect()
}

/// `generics` with those of the declaration `node`.
fn with_generics(generics: &Rc<Generics>, node: Node, src: &[u8]) -> Rc<Generics> {
    let names: HashSet<String> = own_generics(node, src).into_iter().collect();
    match names.is_empty() {
        true => generics.clone(),
        false => Rc::new(Generics {
            names,
            outer: Some(generics.clone()),
        }),
    }
}

/// What a property or a local holds as far as its declaration says: its written type, else the
/// type its initializer constructs (`Type(..)`, through `try` and `await`).
fn declared(node: Node, src: &[u8], generics: &Generics) -> Class {
    let annotation = named(node).find(|n| n.kind() == "type_annotation");
    if let Some(annotation) = annotation {
        return (annotation.child_by_field_name("name"))
            .map_or(Class::Unknown, |t| class_of(t, src, generics));
    }
    let mut value = node.child_by_field_name("value");
    while let Some(inner) =
        value.filter(|v| matches!(v.kind(), "try_expression" | "await_expression"))
    {
        value = inner.child_by_field_name("expr");
    }
    let Some(value) =
        value.filter(|v| matches!(v.kind(), "call_expression" | "constructor_expression"))
    else {
        return Class::Unknown;
    };
    let (function, suffixes) = parts(value);
    match call(function, &suffixes, value, src, "") {
        Some(made) if made.constructs && !generics.holds(head(&made.callee)) => {
            Class::Named(made.callee)
        }
        _ => Class::Unknown,
    }
}

/// The receivers with one class per scope and name: a name bound twice to other classes, or
/// also by a binding with no written class (`locals`), holds no known one.
fn one_class_each(receivers: Vec<Receiver>, locals: &[String]) -> Vec<Receiver> {
    let mut at: HashMap<(String, String), usize> = HashMap::new();
    let mut kept: Vec<Receiver> = Vec::new();
    for mut receiver in receivers {
        if locals
            .binary_search(&qualify(&receiver.scope, &receiver.name))
            .is_ok()
        {
            receiver.class = Class::Unknown;
        }
        match at.get(&(receiver.scope.clone(), receiver.name.clone())) {
            Some(&n) if kept[n].class != receiver.class => kept[n].class = Class::Unknown,
            Some(_) => {}
            None => {
                at.insert((receiver.scope.clone(), receiver.name.clone()), kept.len());
                kept.push(receiver);
            }
        }
    }
    kept
}

/// `scope` and each definition around it: its prefixes cut at a dot outside the parentheses
/// of a function's labels (`Page.show(_:Outer.Item)` is cut at `Page`).
fn enclosing(scope: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0usize;
    let cuts = scope.char_indices().filter_map(move |(at, c)| {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '.' if depth == 0 => return Some(at),
            _ => {}
        }
        None
    });
    cuts.map(|at| &scope[..at]).chain([scope])
}

/// The first segment of a dotted name.
fn head(name: &str) -> &str {
    name.split('.').next().unwrap_or_default()
}

fn unbacktick(name: &str) -> String {
    name.trim_matches('`').to_string()
}

/// The names a declaration or a statement binds in its own node: a property's or a
/// condition's `let x`, a pattern of one name (`for (i, item) in`, `case let .some(q)`), a
/// closure's parameter, a `catch`'s implicit `error`.
fn bound(node: Node, src: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for name in node.children_by_field_name("bound_identifier", &mut cursor) {
        out.push(unbacktick(text(name, src)));
    }
    match node.kind() {
        "property_declaration" => {
            for pattern in named(node).filter(|n| n.kind() == "pattern") {
                out.extend(bound(pattern, src));
            }
        }
        "pattern" if out.is_empty() => {
            if let [one] = named(node).collect::<Vec<_>>()[..]
                && one.kind() == "simple_identifier"
                && !text(node, src).starts_with('.')
            {
                out.push(unbacktick(text(one, src)));
            }
        }
        "lambda_parameter" => {
            if let Some(name) = node.child_by_field_name("name") {
                out.push(unbacktick(text(name, src)));
            }
        }
        "catch_block" if node.child_by_field_name("error").is_none() => {
            out.push("error".to_string());
        }
        _ => {}
    }
    out
}

/// A call's function and suffixes (`call_suffix`, `constructor_suffix`), in order: a call
/// whose suffixes pass only trailing closures to a call (`f(a) { }`) is that call with them.
fn parts(node: Node) -> (Node, Vec<Node>) {
    // The suffixes of each call absorbed, the outermost first.
    let mut layers: Vec<Vec<Node>> = Vec::new();
    let mut at = node;
    let function = loop {
        let mut all = named(at);
        let Some(function) = all.next() else {
            break at;
        };
        let suffixes: Vec<Node> = all.collect();
        let closures_only =
            (suffixes.iter()).all(|s| named(*s).all(|n| n.kind() != "value_arguments"));
        layers.push(suffixes);
        if function.kind() != "call_expression" || !closures_only {
            break function;
        }
        at = function;
    };
    (function, layers.into_iter().rev().flatten().collect())
}

/// The call a call or constructor expression makes, `None` for a subscript (`items[0]`).
fn call(function: Node, suffixes: &[Node], node: Node, src: &[u8], scope: &str) -> Option<Call> {
    let mut labels = Vec::new();
    let mut label = None;
    for suffix in suffixes {
        for child in children(*suffix) {
            match child.kind() {
                "value_arguments" => {
                    if children(child).next().is_some_and(|c| c.kind() == "[") {
                        return None;
                    }
                    for argument in named(child).filter(|n| n.kind() == "value_argument") {
                        let written = argument.child_by_field_name("name");
                        labels.push(written.map_or("_".to_string(), |l| unbacktick(text(l, src))));
                    }
                }
                "lambda_literal" => labels.push(label.take().unwrap_or_default()),
                "simple_identifier" => label = Some(unbacktick(text(child, src))),
                _ => {}
            }
        }
    }
    let (callee, constructs) = match node.kind() {
        "constructor_expression" => {
            let ty = node.child_by_field_name("constructed_type")?;
            (type_name(ty, src), true)
        }
        _ => {
            let callee = callee(function, src);
            let last = callee.rsplit('.').next().unwrap_or(&callee);
            let constructs = !callee.starts_with('(') && last.starts_with(char::is_uppercase);
            (callee, constructs)
        }
    };
    if callee.is_empty() {
        return None;
    }
    Some(Call {
        scope: scope.to_string(),
        callee,
        arity: Some(labels.len()),
        labels: Some(labels),
        constructs,
        range: node.byte_range(),
        args: Vec::new(),
        kwargs: Vec::new(),
        decorates: None,
        by_path: false,
        root: None,
        on: None,
        receiver: None,
        arg_types: Vec::new(),
        uses: Vec::new(),
    })
}

/// A called expression as a dotted name (`a.b.c`, `self.f`, `super.init`); a part that is no
/// name is `(<node kind>)`, and so is a chain longer than `PATH_DEPTH`. `x!` and `x?` are `x`.
fn callee(node: Node, src: &[u8]) -> String {
    let mut segments: Vec<String> = Vec::new();
    let mut at = node;
    loop {
        if segments.len() >= PATH_DEPTH {
            return format!("({})", node.kind());
        }
        match at.kind() {
            "simple_identifier" => {
                segments.push(unbacktick(text(at, src)));
                break;
            }
            "self_expression" => {
                segments.push("self".into());
                break;
            }
            "super_expression" => {
                segments.push("super".into());
                break;
            }
            "navigation_expression" => {
                let name = (at.child_by_field_name("suffix"))
                    .and_then(|s| s.child_by_field_name("suffix"))
                    .filter(|n| n.kind() == "simple_identifier");
                let (Some(name), Some(target)) = (name, at.child_by_field_name("target")) else {
                    segments.push(format!("({})", at.kind()));
                    break;
                };
                segments.push(unbacktick(text(name, src)));
                at = target;
            }
            "postfix_expression" => match at.child_by_field_name("target") {
                Some(target) => at = target,
                None => {
                    segments.push(format!("({})", at.kind()));
                    break;
                }
            },
            kind => {
                segments.push(format!("({kind})"));
                break;
            }
        }
    }
    segments.reverse();
    segments.join(".")
}

/// Where the code of `scope` writes a value: a closure there is a definition unless the scope is
/// a type (a property's initializer) or the walk no longer names definitions.
fn ctx<'s>(scope: &'s str, defining: bool, types: &HashSet<String>) -> Ctx<'s> {
    Ctx {
        scope,
        inline: defining && !types.contains(scope),
    }
}

fn definition(name: &str, kind: Kind, range: Range<usize>, bases: Vec<String>) -> Def {
    Def {
        name: name.to_string(),
        kind,
        range,
        params: Vec::new(),
        bases,
        decorators: Vec::new(),
        return_values: Vec::new(),
        returns: None,
        type_params: Vec::new(),
        result: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Part;

    fn facts(src: &str) -> Facts {
        parse(src.as_bytes()).unwrap()
    }

    #[test]
    fn definitions_past_the_depth_limit_keep_their_enclosing_symbol() {
        let depth = DEFINITION_DEPTH + 2;
        let src = "struct S {\n".repeat(depth) + &"}\n".repeat(depth);
        assert_eq!(facts(&src).defs.len(), DEFINITION_DEPTH);
    }

    #[test]
    fn a_callee_past_the_path_limit_is_an_unknown_expression() {
        let chain = vec!["a"; PATH_DEPTH + 1].join(".");
        let calls = facts(&format!("func f() {{ {chain}() }}\n")).calls;
        assert_eq!(calls[0].callee, "(navigation_expression)");
        let chain = vec!["a"; PATH_DEPTH].join(".");
        let calls = facts(&format!("func f() {{ {chain}() }}\n")).calls;
        assert_eq!(calls[0].callee, chain);
    }

    #[test]
    fn a_file_past_the_qualification_budget_keeps_no_facts() {
        let name = "N".repeat(64 * 1024);
        let calls = "f()\n".repeat(QUALIFICATION_BYTES / name.len() + 1);
        let src = format!("struct {name} {{ func g() {{\n{calls}}} }}\n");
        let facts = facts(&src);
        assert!(facts.unbuilt && facts.defs.is_empty());
        let src = format!("struct {name} {{ func g() {{\nf()\n}} }}\n");
        assert!(!super::parse(src.as_bytes()).unwrap().unbuilt);
    }

    #[test]
    fn a_function_is_named_with_its_argument_labels() {
        let names: Vec<String> = facts(
            "func place(order: Int, quantity q: Int) {}\nfunc place(_ id: String) {}\n\
             func run() {}\nfunc retry(times: Int = 3, _ body: () -> Void) {}\n",
        )
        .defs
        .into_iter()
        .map(|d| d.name)
        .collect();
        assert_eq!(
            names,
            [
                "place(order:quantity:)",
                "place(_:)",
                "run()",
                "retry(times:_:)"
            ]
        );
    }

    #[test]
    fn overloads_with_the_same_labels_are_named_with_their_types() {
        let names: Vec<String> = facts(
            "struct S {\n  func audit(_ order: Order) {}\n  func audit(_ id: [String: Int]) {}\n}\n",
        )
        .defs
        .into_iter()
        .map(|d| d.name)
        .collect();
        assert_eq!(names, ["S", "S.audit(_:Order)", "S.audit(_:[String:Int])"]);
    }

    #[test]
    fn a_type_and_its_extension_in_one_file_are_one_scope_for_names() {
        let names =
            |src: &str| -> Vec<String> { facts(src).defs.into_iter().map(|d| d.name).collect() };
        assert_eq!(
            names(
                "class S { func audit(_ o: Order) {} }\nextension S { func audit(_ id: String) {} }\n"
            ),
            ["S", "S.audit(_:Order)", "S.audit(_:String)"]
        );
        // A protocol extension's default body of a requirement is that requirement.
        assert_eq!(
            names("protocol P { func f(x: Int) }\nextension P { func f(x: Int) {} }\n"),
            ["P", "P.f(x:)"]
        );
    }

    #[test]
    fn a_call_writes_its_labels_and_trailing_closures() {
        let calls = facts(
            "func f() {\n  place(order: o, 2)\n  retry { }\n  run(a: 1) { } other: { }\n  \
             items[0]\n}\n",
        )
        .calls;
        let labels: Vec<(String, Vec<String>)> = calls
            .into_iter()
            .map(|c| (c.callee, c.labels.unwrap()))
            .collect();
        let owned = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                ("place".to_string(), owned(&["order", "_"])),
                ("retry".to_string(), owned(&[""])),
                ("run".to_string(), owned(&["a", "", "other"])),
            ]
        );
    }

    #[test]
    fn a_call_writes_its_receiver_its_arguments_and_its_closure_as_a_definition() {
        let facts = facts(
            "func routes(_ app: Application) -> Builder {\n  let api = app.grouped(\"api\", \"v1\")\n  \
             api.get(\"users\", use: list) { req in 1 }\n  return api\n}\n",
        );
        let get = facts.calls.iter().find(|c| c.callee == "api.get").unwrap();
        assert!(matches!(&get.receiver, Some(Value::Ref(r)) if r == "api"));
        assert!(
            matches!(&get.args[..], [Value::Str(_), Value::Function(f)] if f.starts_with("routes(_:).@"))
        );
        assert!(matches!(&get.kwargs[..], [(k, Value::Ref(r))] if k == "use" && r == "list"));
        let inline = facts.defs.iter().filter(|d| d.kind == Kind::Inline).count();
        assert_eq!(inline, 1);
        assert!(
            matches!(&facts.bound[..], [b] if b.name == "api" && matches!(b.value, Value::Call(_)))
        );
        let routes = facts.defs.iter().find(|d| d.name == "routes(_:)").unwrap();
        assert!(matches!(&routes.return_values[..], [Value::Ref(r)] if r == "api"));
    }

    #[test]
    fn a_local_holds_the_members_assigned_through_it_in_place() {
        let facts = facts(
            "func f(flag: Bool) {\n  var a = URLRequest(url: u)\n  a.httpMethod = \"POST\"\n  \
             var b = URLRequest(url: u)\n  if flag { b.httpMethod = \"PUT\" }\n  \
             var c = URLRequest(url: u)\n  c.httpMethod = \"PUT\"\n  c.httpMethod = \"GET\"\n  \
             var d = URLRequest(url: u)\n  d = other\n  let e = \"x\" + y\n}\n",
        );
        let method = |name: &str| {
            let held = facts.bound.iter().find(|b| b.name == name)?;
            let Value::Call(call) = &held.value else {
                return None;
            };
            let set = call.kwargs.iter().find(|(k, _)| k == "httpMethod=");
            Some(match set {
                Some((_, Value::Str(parts))) => format!("{parts:?}"),
                Some(_) => "unknown".to_string(),
                None => "none".to_string(),
            })
        };
        assert_eq!(method("a").as_deref(), Some("[Text(\"POST\")]"));
        // May not run, written twice, and assigned again as a whole: no one method.
        assert_eq!(method("b").as_deref(), Some("unknown"));
        assert_eq!(method("c").as_deref(), Some("unknown"));
        assert!(method("d").is_none());
        let e = facts.bound.iter().find(|b| b.name == "e").unwrap();
        assert!(
            matches!(&e.value, Value::Str(p) if matches!(&p[..], [Part::Text(t), Part::Ref(r)] if t == "x" && r == "y"))
        );
    }

    #[test]
    fn a_type_level_let_is_a_constant_of_the_type() {
        let facts = facts("enum Api {\n  static let host = \"h\"\n  var other = \"o\"\n}\n");
        let names: Vec<&str> = facts.consts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Api.host"]);
    }
}
