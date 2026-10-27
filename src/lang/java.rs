//! The Java language pack: one source file's syntax, with no compiler or build tool.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};

use super::{
    Annotated, Binds, Call, Class, Const, Def, Export, Facts, Import, Imported, Invocation, Kind,
    LanguagePack, Layout, Marked, Modules, Param, Part, Receiver, children, named, text,
};
use crate::{Result, qualify};

/// How many named types may enclose a definition. Deeper bodies keep their enclosing symbol.
// ponytail: real source nests a few types; this prevents quadratic qualified names.
const DEFINITION_DEPTH: usize = 128;
/// How deep arguments and concatenations are read before their value becomes unknown.
// ponytail: real expressions nest a few levels; this bounds hostile source recursion.
const VALUE_DEPTH: usize = 64;
/// How many segments of a callee are kept before it becomes an unknown expression.
// ponytail: real member paths are short; this keeps crafted chains cheap.
const PATH_DEPTH: usize = 256;
/// Enclosing-name bytes across syntax visits, before each visit's bounded metadata fanout.
// ponytail: a long package prefix must not multiply across a broad file without a budget.
const QUALIFICATION_BYTES: usize = 4 * 1024 * 1024;

pub struct Java;

impl LanguagePack for Java {
    fn extensions(&self) -> &'static [&'static str] {
        &["java"]
    }

    fn parse(&self, _path: &str, src: &[u8]) -> Result<Facts> {
        parse(src)
    }

    fn receiver(&self) -> Option<&'static str> {
        Some("this")
    }

    fn members_in_scope(&self) -> bool {
        true
    }

    fn unordered_globs(&self) -> bool {
        true
    }

    fn qualified_types(&self) -> bool {
        true
    }

    fn base_receiver(&self) -> Option<&'static str> {
        Some("super")
    }

    fn unordered_bases(&self) -> bool {
        true
    }

    // ponytail: an enum's implicit `values`, `valueOf`, `ordinal`, `name` and a record's
    // accessors are not listed; a bare call of one may still climb to an outer method.
    fn implicit_members(&self) -> &'static [&'static str] {
        &[
            "clone",
            "equals",
            "finalize",
            "getClass",
            "hashCode",
            "notify",
            "notifyAll",
            "toString",
            "wait",
        ]
    }

    /// What the kinds read through `Indexed::configs`: Spring's base configuration, which its
    /// reader lists (`jvm::spring::CONFIG_FILES`).
    fn config_files(&self) -> &'static [&'static str] {
        crate::jvm::spring::CONFIG_FILES
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        Ok(self.layout(configs, files)?.modules)
    }

    fn layout(&self, _configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Result<Layout> {
        let mut packages: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut types: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // What a Kotlin file declares at its top level: functions (by overload family) and
        // constants, which an import names by fully qualified name too.
        let mut tops: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (&path, facts) in files {
            // A Kotlin script's declarations are its own: no package or import reaches them.
            if facts.unbuilt || path.ends_with(".kts") {
                continue;
            }
            packages
                .entry(facts.package.clone())
                .or_default()
                .push(path.to_string());
            for def in facts.defs.iter().filter(|d| d.kind == Kind::Class) {
                types
                    .entry(def.name.clone())
                    .or_default()
                    .push(path.to_string());
            }
            let names = (facts.overloads.keys())
                .chain(facts.consts.iter().map(|c| &c.name))
                .filter(|n| !n.starts_with('.') && crate::parent(n) == facts.package);
            for name in names {
                let found = tops.entry(name.clone()).or_default();
                if found.last().map(String::as_str) != Some(path) {
                    found.push(path.to_string());
                }
            }
        }
        // A package keeps its files against a top-level declaration of its name (`fun util()`
        // beside package `util`), whose import then binds nothing; a type wins over both, as
        // before (Java forbids a type and a package of one name).
        let modules: BTreeMap<String, Vec<String>> = (tops.into_iter())
            .chain(
                packages
                    .iter()
                    .map(|(name, paths)| (name.clone(), paths.clone())),
            )
            .chain(types)
            .collect();
        Ok(Layout {
            members: None,
            modules: Box::new(move |_, name| modules.get(name).cloned().unwrap_or_default()),
            published: None,
            programs: Vec::new(),
            reaches: None,
            shared: packages.into_values().filter(|p| p.len() > 1).collect(),
        })
    }
}

struct Visit<'t> {
    node: Node<'t>,
    scope: Rc<String>,
    interface: bool,
    visible: bool,
    definitions: bool,
    depth: usize,
    /// The type parameters declared around the node: a type they name is no class.
    generics: Rc<Vec<String>>,
}

fn parse(src: &[u8]) -> Result<Facts> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_java::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let package = named(tree.root_node())
        .find(|n| n.kind() == "package_declaration")
        .and_then(|n| named(n).find(|n| matches!(n.kind(), "identifier" | "scoped_identifier")))
        .map_or_else(String::new, |n| callee(n, src));
    let mut facts = Facts {
        package,
        ..Facts::default()
    };
    SYSTEM_BOUND.set(binds_system(tree.root_node(), src));
    // The current package and java.lang are implicitly visible before explicit imports.
    wildcard("java.lang", false, &mut facts);
    let package = facts.package.clone();
    wildcard(&package, true, &mut facts);
    for node in named(tree.root_node()).filter(|n| n.kind() == "import_declaration") {
        import(node, src, &mut facts);
    }
    let mut stack = vec![Visit {
        node: tree.root_node(),
        scope: Rc::new(facts.package.clone()),
        interface: false,
        visible: true,
        definitions: true,
        depth: 0,
        generics: Rc::new(Vec::new()),
    }];
    let mut fields = BTreeSet::new();
    let mut locals = Vec::new();
    // The types declared so far: a local whose scope is one sits in an initializer.
    let mut classes = BTreeSet::new();
    // Initializer calls of `var` locals (by `Class::Result` place) and chained calls (by place
    // in `facts.calls`), linked to the calls they name once every call is read.
    let mut results: Vec<Range<usize>> = Vec::new();
    let mut chains: Vec<(usize, Range<usize>)> = Vec::new();
    let mut outside = BTreeSet::new();
    // The instance `final` fields (class, name, initializer) and what constructors assign to
    // fields, by class and field.
    let mut finals: Vec<(String, String, Option<Value>)> = Vec::new();
    let mut assigned: BTreeMap<(String, String), Vec<(Value, String)>> = BTreeMap::new();
    // The names assigned anywhere in the file (`f = v`, `x.f = v`), marked as constants `.f`.
    let mut written = BTreeSet::new();
    let mut qualification_bytes = QUALIFICATION_BYTES;
    while let Some(Visit {
        node,
        scope,
        interface,
        visible,
        definitions,
        depth,
        mut generics,
    }) = stack.pop()
    {
        let Some(remaining) = qualification_bytes.checked_sub(scope.len()) else {
            // A partial overload family could turn an ambiguous call into a false exact one.
            return Ok(Facts {
                unbuilt: true,
                ..Facts::default()
            });
        };
        qualification_bytes = remaining;
        let (mut inner, mut in_interface, mut public, mut nested) =
            (scope.clone(), interface, visible, depth);
        let anonymous = node.kind() == "class_body"
            && node.parent().is_some_and(|p| {
                matches!(p.kind(), "object_creation_expression" | "enum_constant")
            });
        let defining = definitions && !anonymous && depth < DEFINITION_DEPTH;
        let kind = match node.kind() {
            "class_declaration"
            | "interface_declaration"
            | "annotation_type_declaration"
            | "enum_declaration"
            | "record_declaration" => Some(Kind::Class),
            "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration" => Some(Kind::Method),
            _ => None,
        };
        if let Some(kind) = kind.filter(|_| defining) {
            let Some(name) = node.child_by_field_name("name") else {
                continue;
            };
            let name = text(name, src);
            let constructor = matches!(
                node.kind(),
                "constructor_declaration" | "compact_constructor_declaration"
            );
            if let Some(declared) = node.child_by_field_name("type_parameters") {
                let mut names = generics.as_ref().clone();
                names.extend(
                    named(declared)
                        .filter_map(|p| named(p).find(|n| n.kind() == "type_identifier"))
                        .map(|n| text(n, src).to_string()),
                );
                generics = Rc::new(names);
            }
            let parameters = parameters(node, src, &generics);
            let signature_name = if kind == Kind::Method {
                let signature = format!(
                    "{name}({})",
                    parameters
                        .iter()
                        .map(|(_, ty, _)| ty.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                if !constructor && scope.rsplit('.').next() == Some(name) {
                    format!("{signature}#method")
                } else {
                    signature
                }
            } else {
                name.to_string()
            };
            inner = Rc::new(qualify(&scope, &signature_name));
            if node.kind() == "constructor_declaration" {
                let names: Vec<&str> = parameters.iter().map(|(n, _, _)| n.as_str()).collect();
                assignments(node, src, &scope, &inner, &names, &mut assigned);
            }
            if kind == Kind::Method {
                // The annotations of each parameter (`@Value("${key}") String base`).
                for (n, parameter) in formal_parameters(node).enumerate() {
                    for annotation in annotations(parameter, src, &scope) {
                        let on = Marked::Param(inner.as_ref().clone(), n);
                        facts.annotated.push(Annotated { on, annotation });
                    }
                }
                for (name, _, class) in &parameters {
                    facts.receivers.push(Receiver {
                        scope: inner.as_ref().clone(),
                        name: name.clone(),
                        class: class.clone(),
                    });
                }
            }
            let string_array = matches!(
                parameters.as_slice(),
                [(_, ty, _)]
                    if matches!(
                        ty.as_str(),
                        "String[]" | "java.lang.String[]" | "String..." | "java.lang.String..."
                    )
            );
            let params = parameters
                .into_iter()
                .map(|(name, _, _)| Param {
                    name,
                    binds: Binds::Any,
                    label: None,
                    default: None,
                    fields: Vec::new(),
                })
                .collect();
            let modifiers = modifiers(node);
            let own_public = modifiers
                .iter()
                .any(|m| matches!(*m, "public" | "protected"))
                || (interface && !modifiers.contains(&"private"));
            public = visible && own_public;
            if kind == Kind::Class {
                classes.insert(inner.as_ref().clone());
            }
            facts.defs.push(Def {
                name: inner.as_ref().clone(),
                kind,
                range: node.byte_range(),
                params,
                bases: if kind == Kind::Class {
                    bases(node, src, &generics)
                } else {
                    Vec::new()
                },
                decorators: Vec::new(),
                return_values: Vec::new(),
                returns: None,
                type_params: Vec::new(),
                result: match node
                    .child_by_field_name("type")
                    .map(|t| class_of(t, src, &generics))
                {
                    Some(Class::Type(ty)) if !constructor => Some(Class::Type(ty)),
                    _ => None,
                },
            });
            // `public static void main(String[])` (or `String...`) starts a program.
            if kind == Kind::Method
                && !constructor
                && name == "main"
                && (modifiers.contains(&"public") || (interface && !modifiers.contains(&"private")))
                && modifiers.contains(&"static")
                && node
                    .child_by_field_name("type")
                    .is_some_and(|t| t.kind() == "void_type")
                && string_array
            {
                facts.programs.push(inner.as_ref().clone());
            }
            if kind == Kind::Method {
                let callable = if constructor {
                    scope.as_ref().clone()
                } else {
                    qualify(&scope, name)
                };
                let families = if constructor {
                    &mut facts.constructors
                } else {
                    &mut facts.overloads
                };
                families
                    .entry(callable)
                    .or_default()
                    .push(inner.as_ref().clone());
                if modifiers.contains(&"static") {
                    facts.statics.push(inner.as_ref().clone());
                }
            } else {
                if scope.as_str() != facts.package
                    && (modifiers.contains(&"static")
                        || interface
                        || matches!(
                            node.kind(),
                            "interface_declaration"
                                | "annotation_type_declaration"
                                | "enum_declaration"
                                | "record_declaration"
                        ))
                {
                    facts.statics.push(inner.as_ref().clone());
                }
                nested += 1;
                let relative = inner
                    .strip_prefix(&format!("{}.", facts.package))
                    .unwrap_or(&inner);
                let aliases = if scope.as_str() == facts.package {
                    vec![name, inner.as_str()]
                } else {
                    vec![relative, inner.as_str()]
                };
                for alias in aliases {
                    facts.exports.push(Export::Local {
                        exported: alias.to_string(),
                        local: inner.as_ref().clone(),
                    });
                    if !public {
                        facts.private.push(alias.to_string());
                    }
                }
            }
            if !public {
                facts.hidden.push(inner.as_ref().clone());
            }
            for mut at in annotations(node, src, &scope) {
                at.decorates = Some(inner.as_ref().clone());
                facts.calls.push(at);
            }
            // An annotation type is an interface (JLS 9.6).
            in_interface = matches!(
                node.kind(),
                "interface_declaration" | "annotation_type_declaration"
            );
            if in_interface {
                facts.interfaces.push(inner.as_ref().clone());
            }
        }
        if kind.is_some() && !defining {
            if let Some(name) = node.child_by_field_name("name") {
                facts.locals.push(qualify(&scope, text(name, src)));
            }
            for (name, _, _) in parameters(node, src, &generics) {
                facts.locals.push(qualify(&scope, &name));
            }
        }
        match node.kind() {
            "method_invocation" => {
                let mut at = call(&scope, callee(node, src), node.byte_range());
                if !definitions && at.callee.starts_with("this.") {
                    at.callee = at.callee.replacen("this.", "(anonymous).", 1);
                }
                if !definitions && !at.callee.contains('.') {
                    at.callee = format!("(anonymous).{}", at.callee);
                }
                at.by_path = node.child_by_field_name("object").is_none() && definitions;
                at.args = arguments(node, src, 0);
                at.arity = Some(at.args.len());
                at.receiver = node.child_by_field_name("object").map(|n| value(n, src, 0));
                // `make().save()` and `make().x.save()`: the call the chain is made on.
                let mut object = node.child_by_field_name("object");
                while let Some(n) = object.filter(|n| n.kind() == "field_access") {
                    object = n.child_by_field_name("object");
                }
                if let Some(inner) = object.filter(|n| n.kind() == "method_invocation")
                    && definitions
                    && at.callee.starts_with("(method_invocation).")
                {
                    chains.push((facts.calls.len(), inner.byte_range()));
                }
                facts.calls.push(at);
            }
            "object_creation_expression" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    let ty = type_name(ty, src);
                    let callee = if definitions {
                        ty
                    } else {
                        format!("(opaque).{ty}")
                    };
                    let mut at = call(&scope, callee, node.byte_range());
                    at.by_path = definitions;
                    at.constructs = true;
                    at.args = arguments(node, src, 0);
                    at.arity = Some(at.args.len());
                    facts.calls.push(at);
                }
            }
            // `this(..)` calls a constructor of the class, `super(..)` one of its superclass.
            "explicit_constructor_invocation" if definitions => {
                let class = crate::parent(scope.as_str());
                let callee = match node.child_by_field_name("constructor").map(|c| c.kind()) {
                    Some("this") => class.rsplit('.').next().map(str::to_string),
                    Some("super") => superclass(node, src),
                    _ => None,
                };
                if let Some(callee) = callee {
                    let mut at = call(&scope, callee, node.byte_range());
                    at.by_path = true;
                    at.constructs = true;
                    at.args = arguments(node, src, 0);
                    at.arity = Some(at.args.len());
                    facts.calls.push(at);
                }
            }
            "local_variable_declaration" | "enhanced_for_statement" | "resource" if definitions => {
                // A declared type, or the class a `var` takes from `new T(..)` or a call.
                let declared = node.child_by_field_name("type");
                // An initializer block's or a field lambda's local is no field of the class.
                let initializer = classes.contains(scope.as_str());
                let mut typed = |value: Option<Node>, dimensions: bool| match declared {
                    _ if dimensions || initializer => Class::Unknown,
                    Some(ty) if text(ty, src) == "var" => match value {
                        Some(v)
                            if v.kind() == "object_creation_expression"
                                && v.child_by_field_name("type").is_some()
                                && named(v).all(|n| n.kind() != "class_body") =>
                        {
                            class_of(v.child_by_field_name("type").unwrap(), src, &generics)
                        }
                        Some(v) if v.kind() == "method_invocation" => {
                            results.push(v.byte_range());
                            Class::Result(results.len() - 1)
                        }
                        _ => Class::Unknown,
                    },
                    Some(ty) => class_of(ty, src, &generics),
                    None => Class::Unknown,
                };
                let mut bind = |name: Node, class: Class| {
                    locals.push(Receiver {
                        scope: scope.as_ref().clone(),
                        name: text(name, src).to_string(),
                        class,
                    });
                };
                if let Some(name) = node.child_by_field_name("name") {
                    let value = node.child_by_field_name("value");
                    let dimensions = node.child_by_field_name("dimensions").is_some();
                    // The element of a loop is no `var` initializer.
                    let value = value.filter(|_| node.kind() == "resource");
                    bind(name, typed(value, dimensions));
                }
                for declaration in named(node).filter(|n| n.kind() == "variable_declarator") {
                    if let Some(name) = declaration.child_by_field_name("name") {
                        let dimensions = declaration.child_by_field_name("dimensions").is_some();
                        bind(
                            name,
                            typed(declaration.child_by_field_name("value"), dimensions),
                        );
                    }
                }
            }
            "local_variable_declaration"
            | "catch_formal_parameter"
            | "enhanced_for_statement"
            | "resource"
            | "instanceof_expression" => {
                if let Some(name) = node.child_by_field_name("name") {
                    facts.locals.push(qualify(&scope, text(name, src)));
                }
                for declaration in named(node).filter(|n| n.kind() == "variable_declarator") {
                    if let Some(name) = declaration.child_by_field_name("name") {
                        facts.locals.push(qualify(&scope, text(name, src)));
                    }
                }
            }
            "type_pattern" | "record_pattern_component" => {
                if let Some(name) = named(node).filter(|n| n.kind() == "identifier").last() {
                    facts.locals.push(qualify(&scope, text(name, src)));
                }
            }
            "lambda_expression" => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let mut pending = vec![params];
                    while let Some(p) = pending.pop() {
                        if p.kind() == "identifier" {
                            facts.locals.push(qualify(&scope, text(p, src)));
                        } else if let Some(name) = p.child_by_field_name("name") {
                            facts.locals.push(qualify(&scope, text(name, src)));
                        } else {
                            pending.extend(named(p));
                        }
                    }
                }
            }
            "field_declaration" | "constant_declaration"
                if node.kind() == "constant_declaration"
                    || (modifiers(node).contains(&"final")
                        && modifiers(node).contains(&"static")) =>
            {
                for declaration in named(node).filter(|n| n.kind() == "variable_declarator") {
                    if let Some(name) = declaration.child_by_field_name("name") {
                        let name = qualify(&scope, text(name, src));
                        facts.statics.push(name.clone());
                        facts.consts.push(Const {
                            name,
                            value: declaration
                                .child_by_field_name("value")
                                .map_or(Value::Unknown, |n| value(n, src, 0)),
                            ty: None,
                            scope: None,
                        });
                    }
                }
            }
            "field_declaration"
                if definitions
                    && modifiers(node).contains(&"final")
                    && !modifiers(node).contains(&"static") =>
            {
                for declaration in named(node).filter(|n| n.kind() == "variable_declarator") {
                    if let Some(name) = declaration.child_by_field_name("name") {
                        let initial = declaration.child_by_field_name("value");
                        let initial = initial.map(|n| value(n, src, 0));
                        finals.push((scope.as_ref().clone(), text(name, src).into(), initial));
                    }
                }
            }
            "type_identifier" => {
                outside.insert(text(node, src).to_string());
            }
            "assignment_expression" => {
                let left = node.child_by_field_name("left");
                let name = match left {
                    Some(n) if n.kind() == "identifier" => Some(n),
                    Some(n) if n.kind() == "field_access" => n.child_by_field_name("field"),
                    _ => None,
                };
                written.extend(name.map(|n| text(n, src).to_string()));
            }
            _ => {}
        }
        if matches!(node.kind(), "field_declaration" | "constant_declaration") {
            let declared = node.child_by_field_name("type");
            for declaration in named(node).filter(|n| n.kind() == "variable_declarator") {
                if let Some(name) = declaration.child_by_field_name("name") {
                    // A field of an anonymous class sits in the enclosing scope: untyped there.
                    let class = match declared {
                        Some(ty)
                            if definitions
                                && declaration.child_by_field_name("dimensions").is_none() =>
                        {
                            class_of(ty, src, &generics)
                        }
                        _ => Class::Unknown,
                    };
                    fields.insert(text(name, src).to_string());
                    if definitions {
                        let is_static = modifiers(node).contains(&"static");
                        for annotation in annotations(node, src, &scope) {
                            let on = Marked::Field {
                                class: scope.as_ref().clone(),
                                name: text(name, src).into(),
                                is_static,
                            };
                            facts.annotated.push(Annotated { on, annotation });
                        }
                    }
                    facts.receivers.push(Receiver {
                        scope: scope.as_ref().clone(),
                        name: text(name, src).to_string(),
                        class,
                    });
                }
            }
        }
        if node.kind() == "enum_constant"
            && let Some(name) = node.child_by_field_name("name")
        {
            facts.receivers.push(Receiver {
                scope: scope.as_ref().clone(),
                name: text(name, src).to_string(),
                class: Class::Unknown,
            });
        }
        if node.kind() == "method_invocation"
            && let Some(receiver) = node.child_by_field_name("object")
            && receiver.kind() == "identifier"
            && text(receiver, src).starts_with(char::is_uppercase)
        {
            outside.insert(text(receiver, src).to_string());
        }
        for child in named(node).collect::<Vec<_>>().into_iter().rev() {
            // Calls in anonymous classes and lambdas remain owned by the enclosing symbol.
            stack.push(Visit {
                node: child,
                scope: inner.clone(),
                interface: in_interface,
                visible: public,
                definitions: definitions && !anonymous && (kind.is_none() || defining),
                depth: nested,
                generics: generics.clone(),
            });
        }
    }
    // A `final` field holds the one value its initializer or a constructor gives it: a constant
    // of the class, which no static context reads (it is no `statics`). Two values give none.
    // A value a constructor assigns is written in that constructor, its parameters in scope.
    for (class, name, initial) in finals {
        let given = assigned.remove(&(class.clone(), name.clone()));
        let initial = initial.map(|v| (v, None));
        let given = given.into_iter().flatten().map(|(v, at)| (v, Some(at)));
        let mut values = initial.into_iter().chain(given);
        if let (Some((value, scope)), None) = (values.next(), values.next()) {
            let name = qualify(&class, &name);
            facts.consts.push(Const {
                name,
                value,
                ty: None,
                scope,
            });
        }
    }
    // What `Code::field_written` reads: a field so assigned holds no one value.
    for name in written {
        facts.consts.push(Const {
            name: format!(".{name}"),
            value: Value::Unknown,
            ty: None,
            scope: None,
        });
    }
    // A local is typed only where it is the one binding of its name in its scope; one named
    // like a field may be read as the field before its declaration, so its class is unknown.
    let called: BTreeMap<(usize, usize), usize> = (facts.calls.iter().enumerate())
        .filter(|(_, c)| c.decorates.is_none())
        .map(|(n, c)| ((c.range.start, c.range.end), n))
        .collect();
    let call_at = |r: &Range<usize>| called.get(&(r.start, r.end)).copied();
    for (outer, inner) in chains {
        facts.calls[outer].on = call_at(&inner);
    }
    for mut local in locals {
        if let Class::Result(n) = local.class {
            local.class = call_at(&results[n]).map_or(Class::Unknown, Class::Result);
        }
        if fields.contains(&local.name) {
            local.class = Class::Unknown;
        }
        facts.receivers.push(local);
    }
    let mut at = BTreeMap::new();
    let mut receivers: Vec<Receiver> = Vec::new();
    for receiver in std::mem::take(&mut facts.receivers) {
        let key = (receiver.scope.clone(), receiver.name.clone());
        match at.get(&key) {
            Some(&n) => {
                let kept: &mut Receiver = &mut receivers[n];
                if kept.class != receiver.class {
                    kept.class = Class::Unknown;
                }
            }
            None => {
                at.insert(key, receivers.len());
                receivers.push(receiver);
            }
        }
    }
    facts.receivers = receivers;
    facts.externs = outside.into_iter().collect();
    let private: BTreeSet<_> = facts.private.iter().map(String::as_str).collect();
    facts.star = super::Star::Only(
        facts
            .exports
            .iter()
            .filter_map(|e| match e {
                Export::Local { exported, .. } if !private.contains(exported.as_str()) => {
                    Some(exported.clone())
                }
                _ => None,
            })
            .collect(),
    );
    facts.locals.sort();
    facts.locals.dedup();
    for names in facts.overloads.values_mut() {
        names.sort();
    }
    Ok(facts)
}

fn import(node: Node, src: &[u8], facts: &mut Facts) {
    let Some(path) = named(node).find(|n| matches!(n.kind(), "identifier" | "scoped_identifier"))
    else {
        return;
    };
    let mut written = callee(path, src);
    if named(node).any(|n| n.kind() == "asterisk") {
        written.push_str(".*");
    }
    if children(node).any(|n| n.kind() == "static") {
        let path = written.as_str();
        let Some((class, member)) = path.rsplit_once('.') else {
            return;
        };
        let alias = format!("@static{}", facts.imports.len());
        facts.imports.push(Import {
            local: alias.clone(),
            source: class.to_string(),
            name: Imported::Qualified,
        });
        facts.imports.push(if member == "*" {
            Import {
                local: "*".into(),
                source: String::new(),
                name: Imported::Glob {
                    path: alias,
                    inside: false,
                    public: false,
                },
            }
        } else {
            Import {
                local: member.to_string(),
                source: String::new(),
                name: Imported::Path(format!("{alias}.{member}")),
            }
        });
    } else if let Some(package) = written.strip_suffix(".*") {
        wildcard(package, false, facts);
    } else {
        facts.imports.push(Import {
            local: written.rsplit('.').next().unwrap_or(&written).to_string(),
            source: written.to_string(),
            name: Imported::Qualified,
        });
    }
}

fn wildcard(source: &str, inside: bool, facts: &mut Facts) {
    let alias = format!("@glob{}", facts.imports.len());
    facts.imports.push(Import {
        local: alias.clone(),
        source: source.to_string(),
        name: Imported::Namespace,
    });
    facts.imports.push(Import {
        local: "*".into(),
        source: String::new(),
        name: Imported::Glob {
            path: alias,
            inside,
            public: false,
        },
    });
}

/// What the constructor `node` of `class` assigns to fields (`this.f = v`, or `f = v` when no
/// parameter is `f`), by class and field. Lambdas and class bodies inside assign none of them.
// ponytail: a local of the constructor named like a field and assigned with `f = v` counts as
// the field; the field then holds two values or a wrong one only in such code.
fn assignments(
    node: Node,
    src: &[u8],
    class: &str,
    constructor: &str,
    params: &[&str],
    out: &mut BTreeMap<(String, String), Vec<(Value, String)>>,
) {
    let mut pending: Vec<Node> = node.child_by_field_name("body").into_iter().collect();
    while let Some(n) = pending.pop() {
        match n.kind() {
            "lambda_expression" | "class_body" => continue,
            "assignment_expression"
                if n.child_by_field_name("operator")
                    .is_some_and(|o| text(o, src) == "=") =>
            {
                let field = n
                    .child_by_field_name("left")
                    .and_then(|left| match left.kind() {
                        "identifier" if !params.contains(&text(left, src)) => Some(text(left, src)),
                        "field_access"
                            if left
                                .child_by_field_name("object")
                                .is_some_and(|o| o.kind() == "this") =>
                        {
                            left.child_by_field_name("field").map(|f| text(f, src))
                        }
                        _ => None,
                    });
                if let (Some(field), Some(right)) = (field, n.child_by_field_name("right")) {
                    let at = (class.to_string(), field.to_string());
                    let assigned = (value(right, src, 0), constructor.to_string());
                    out.entry(at).or_default().push(assigned);
                }
            }
            _ => {}
        }
        pending.extend(named(n));
    }
}

fn modifiers(node: Node<'_>) -> Vec<&str> {
    named(node)
        .find(|n| n.kind() == "modifiers")
        .map_or_else(Vec::new, |n| children(n).map(|c| c.kind()).collect())
}

/// The parameter nodes of the method or constructor `node`, in order.
fn formal_parameters(node: Node) -> impl Iterator<Item = Node> {
    let list = node.child_by_field_name("parameters").or_else(|| {
        (node.kind() == "compact_constructor_declaration")
            .then(|| node.parent()?.parent()?.child_by_field_name("parameters"))
            .flatten()
    });
    (list.into_iter().flat_map(named))
        .filter(|p| matches!(p.kind(), "formal_parameter" | "spread_parameter"))
}

/// Each parameter's name, its type as written and the class that type names.
fn parameters(node: Node, src: &[u8], generics: &[String]) -> Vec<(String, String, Class)> {
    formal_parameters(node)
        .map(|p| {
            let name = p.child_by_field_name("name").or_else(|| {
                named(p)
                    .find(|n| n.kind() == "variable_declarator")
                    .and_then(|n| n.child_by_field_name("name"))
            });
            let ty = p.child_by_field_name("type").or_else(|| {
                named(p).find(|n| n.kind() != "modifiers" && n.kind() != "variable_declarator")
            });
            let mut ty_class = ty.map_or(Class::Unknown, |t| class_of(t, src, generics));
            let mut ty = ty.map_or_else(String::new, |t| type_spelling(t, src));
            // The grammar recovers an annotation before a varargs ellipsis as an ERROR
            // child of a formal parameter. Only the ellipsis tokens survive this check.
            let spread = p.kind() == "spread_parameter"
                || named(p).filter(Node::is_error).any(|n| {
                    children(n)
                        .map(|c| type_spelling(c, src))
                        .collect::<String>()
                        == "..."
                });
            if spread {
                ty.push_str("...");
                ty_class = Class::Unknown;
            }
            if let Some(dimensions) = p.child_by_field_name("dimensions") {
                ty.push_str(&type_spelling(dimensions, src));
                ty_class = Class::Unknown;
            }
            (name.map_or("", |n| text(n, src)).to_string(), ty, ty_class)
        })
        .collect()
}

/// The annotations of the declaration `node` (its `modifiers`), each as a call written in
/// `scope`: its name the callee, its arguments positional or named.
fn annotations(node: Node, src: &[u8], scope: &str) -> Vec<Call> {
    let mut out = Vec::new();
    for annotation in named(node)
        .filter(|n| n.kind() == "modifiers")
        .flat_map(named)
        .filter(|n| matches!(n.kind(), "annotation" | "marker_annotation"))
    {
        let Some(name) = annotation.child_by_field_name("name") else {
            continue;
        };
        let mut at = call(scope, callee(name, src), annotation.byte_range());
        (at.args, at.kwargs) = elements(annotation, src, 0);
        out.push(at);
    }
    out
}

/// The elements of the annotation `node` as written: the unnamed one and the named ones
/// (`key = value`).
fn elements(node: Node, src: &[u8], depth: usize) -> (Vec<Value>, Vec<(String, Value)>) {
    let (mut args, mut kwargs) = (Vec::new(), Vec::new());
    for argument in node
        .child_by_field_name("arguments")
        .into_iter()
        .flat_map(named)
    {
        if argument.kind() == "element_value_pair" {
            if let (Some(key), Some(v)) = (
                argument.child_by_field_name("key"),
                argument.child_by_field_name("value"),
            ) {
                kwargs.push((text(key, src).to_string(), value(v, src, depth)));
            }
        } else {
            args.push(value(argument, src, depth));
        }
    }
    (args, kwargs)
}

fn call(scope: &str, callee: String, range: Range<usize>) -> Call {
    Call {
        scope: scope.to_string(),
        callee,
        arity: None,
        labels: None,
        constructs: false,
        range,
        args: Vec::new(),
        kwargs: Vec::new(),
        decorates: None,
        by_path: false,
        root: None,
        on: None,
        receiver: None,
        arg_types: Vec::new(),
        uses: Vec::new(),
    }
}

/// The class a written type names, as a callee-like name: unknown for a primitive, an array,
/// `var`, a type parameter or a type with arguments inside its path (`Outer<T>.Inner`).
fn class_of(ty: Node, src: &[u8], generics: &[String]) -> Class {
    let ty = match ty.kind() {
        "generic_type" => named(ty).find(|n| n.kind() != "type_arguments"),
        _ => Some(ty),
    };
    let Some(ty) = ty.filter(|t| matches!(t.kind(), "type_identifier" | "scoped_type_identifier"))
    else {
        return Class::Unknown;
    };
    let name = callee(ty, src);
    let head = name.split('.').next().unwrap_or(&name);
    if name == "var" || name.contains('(') || generics.iter().any(|g| g == head) {
        return Class::Unknown;
    }
    Class::Type(name)
}

/// The supertypes a type declaration writes in `extends` and `implements`, in order; one whose
/// class cannot be read stays as an unread base (`(type)`), whose members are not known.
fn bases(node: Node, src: &[u8], generics: &[String]) -> Vec<String> {
    let superclass = node
        .child_by_field_name("superclass")
        .into_iter()
        .flat_map(named);
    let lists = (node.child_by_field_name("interfaces").into_iter())
        .chain(named(node).filter(|n| n.kind() == "extends_interfaces"))
        .flat_map(named)
        .filter(|n| n.kind() == "type_list")
        .flat_map(named);
    superclass
        .chain(lists)
        .filter(|n| !matches!(n.kind(), "line_comment" | "block_comment"))
        .map(|ty| match class_of(ty, src, generics) {
            Class::Type(name) => name,
            _ => "(type)".to_string(),
        })
        .collect()
}

/// The superclass the class around `node` writes in `extends`, as a type name; `None` without
/// one (`java.lang.Object`'s constructor is outside).
fn superclass(node: Node, src: &[u8]) -> Option<String> {
    let mut at = node.parent();
    while let Some(n) = at {
        if matches!(
            n.kind(),
            "class_declaration" | "enum_declaration" | "record_declaration"
        ) {
            let ty = named(n.child_by_field_name("superclass")?).next()?;
            return Some(type_name(ty, src));
        }
        at = n.parent();
    }
    None
}

fn type_name(node: Node, src: &[u8]) -> String {
    let ty = type_spelling(node, src);
    ty.split('<').next().unwrap_or(&ty).trim().to_string()
}

/// Keep type tokens, never annotation arguments, comments or literals recovered from errors.
fn type_spelling(node: Node, src: &[u8]) -> String {
    let mut spelling = String::new();
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if node.is_error()
            || node.kind().ends_with("_literal")
            || matches!(node.kind(), "true" | "false")
            || matches!(
                node.kind(),
                "annotation" | "marker_annotation" | "line_comment" | "block_comment"
            )
        {
            continue;
        }
        if node.child_count() == 0 {
            let token = text(node, src);
            let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '$');
            if spelling.ends_with(word) && token.starts_with(word) {
                spelling.push(' ');
            }
            spelling.push_str(token);
        } else {
            pending.extend(children(node).collect::<Vec<_>>().into_iter().rev());
        }
    }
    spelling
}

fn callee(node: Node, src: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut pending = vec![node];
    while let Some(n) = pending.pop() {
        if parts.len() + pending.len() >= PATH_DEPTH {
            return format!("({})", node.kind());
        }
        match n.kind() {
            "identifier" | "type_identifier" | "this" | "super" => parts.push(text(n, src)),
            "method_invocation" if n.id() != node.id() => parts.push("(method_invocation)"),
            "method_invocation" => {
                if let Some(name) = n.child_by_field_name("name") {
                    pending.push(name);
                }
                if let Some(object) = n.child_by_field_name("object") {
                    pending.push(object);
                }
            }
            "field_access" | "scoped_identifier" | "scoped_type_identifier" => {
                let nodes: Vec<_> = named(n)
                    .filter(|n| !matches!(n.kind(), "line_comment" | "block_comment"))
                    .collect();
                pending.extend(nodes.into_iter().rev());
            }
            _ => parts.push("(expression)"),
        }
    }
    parts.join(".")
}

/// The variable `System.getenv("X")` (or `java.lang.System.getenv("X")`) reads, unless the file
/// binds `System` to another type: by a single-type import, or by declaring one.
// ponytail: a `System` type another file of the package declares is not seen; it hides
// `java.lang.System` there, and its `getenv` still reads as an environment read.
fn getenv(node: Node, src: &[u8]) -> Option<String> {
    let callee = callee(node, src);
    if callee != "System.getenv" && callee != "java.lang.System.getenv" {
        return None;
    }
    if callee == "System.getenv" && SYSTEM_BOUND.get() {
        return None;
    }
    let [Value::Str(parts)] = &arguments(node, src, 0)[..] else {
        return None;
    };
    match &parts[..] {
        [Part::Text(name)] => Some(name.clone()),
        _ => None,
    }
}

thread_local! {
    /// Whether the file `parse` reads binds `System` to a type other than `java.lang.System`
    /// (`binds_system`), for `getenv`.
    static SYSTEM_BOUND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the file binds `System` to another type: a single-type import of one, or a type it
/// may declare by that name.
fn binds_system(root: Node, src: &[u8]) -> bool {
    let imported = named(root)
        .filter(|n| n.kind() == "import_declaration")
        .any(|n| {
            let import = text(n, src).trim_end_matches(';').trim_end();
            let named = import.trim_start_matches("import").trim_start();
            import.ends_with(".System") && named != "java.lang.System"
        });
    imported || declares_system(src)
}

/// Whether the source may declare a type named `System`: the word after `class`, `interface`,
/// `enum` or `record`, wherever it stands (a comment that reads so counts too).
fn declares_system(src: &[u8]) -> bool {
    let mut words = (src.split(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))))
        .filter(|w| !w.is_empty());
    let mut before: &[u8] = b"";
    words.any(|word| {
        let declared = matches!(before, b"class" | b"interface" | b"enum" | b"record");
        before = word;
        declared && word == b"System"
    })
}

fn arguments(node: Node, src: &[u8], depth: usize) -> Vec<Value> {
    node.child_by_field_name("arguments")
        .into_iter()
        .flat_map(named)
        .filter(|n| !matches!(n.kind(), "line_comment" | "block_comment"))
        .map(|n| value(n, src, depth + 1))
        .collect()
}

use super::Value;

fn value(node: Node, src: &[u8], depth: usize) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    match node.kind() {
        "string_literal" => serde_json::from_slice::<String>(&src[node.byte_range()])
            .map_or(Value::Unknown, |s| Value::Str(vec![Part::Text(s)])),
        "identifier" | "field_access" | "scoped_identifier" => Value::Ref(callee(node, src)),
        "this" => Value::Ref("this".into()),
        "method_invocation" if let Some(name) = getenv(node, src) => {
            Value::Str(vec![Part::Env(name)])
        }
        "method_invocation" => Value::Call(Invocation {
            callee: callee(node, src),
            receiver: node
                .child_by_field_name("object")
                .map(|n| Box::new(value(n, src, depth + 1))),
            args: arguments(node, src, depth + 1),
            kwargs: Vec::new(),
            range: node.byte_range(),
        }),
        "object_creation_expression" => node
            .child_by_field_name("type")
            .map_or(Value::Unknown, |ty| {
                Value::New(type_name(ty, src), arguments(node, src, depth + 1))
            }),
        // `Api.class`: the member `class` of the type, which a call such as Retrofit's
        // `create(Api.class)` names its interface by.
        "class_literal" => named(node).next().map_or(Value::Unknown, |ty| {
            Value::Member(type_name(ty, src), "class".into())
        }),
        "array_initializer" | "element_value_array_initializer" => {
            Value::List(named(node).map(|n| value(n, src, depth + 1)).collect())
        }
        // An annotation written as an element value (`@Queue("orders")` inside `@QueueBinding`):
        // its name and its elements, the unnamed one as `value`, as Java reads it.
        "annotation" | "marker_annotation" => {
            let Some(name) = node.child_by_field_name("name") else {
                return Value::Unknown;
            };
            let (args, kwargs) = elements(node, src, depth + 1);
            let named = args.into_iter().map(|v| ("value".to_string(), v));
            Value::New(
                callee(name, src),
                vec![Value::Object(named.chain(kwargs).collect())],
            )
        }
        "decimal_integer_literal" => Value::Scalar(text(node, src).parse().ok()),
        "true" | "false" => Value::Scalar(None),
        "null_literal" => Value::Undefined,
        "parenthesized_expression" => named(node)
            .next()
            .map_or(Value::Unknown, |n| value(n, src, depth + 1)),
        "binary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|n| text(n, src) == "+") =>
        {
            let mut parts = Vec::new();
            for side in ["left", "right"] {
                match node
                    .child_by_field_name(side)
                    .map(|n| value(n, src, depth + 1))
                {
                    Some(Value::Str(found)) => parts.extend(found),
                    Some(Value::Ref(name)) => parts.push(Part::Ref(name)),
                    _ => parts.push(Part::Hole),
                }
            }
            Value::Str(parts)
        }
        _ => Value::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_annotated_field_is_recorded_with_whether_it_is_static() {
        // The kind decides what a `static` field takes (Spring injects none).
        let src = "class C { @Value(\"a\") static String s; @Value(\"b\") String t; }";
        let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
        let fields: Vec<_> = (facts.annotated.iter())
            .filter_map(|a| match &a.on {
                Marked::Field {
                    name, is_static, ..
                } => Some((name.as_str(), *is_static)),
                Marked::Param(..) => None,
            })
            .collect();
        assert_eq!(fields, [("s", true), ("t", false)]);
    }
}
