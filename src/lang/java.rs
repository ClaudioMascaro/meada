//! The Java language pack: one source file's syntax, with no compiler or build tool.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};

use super::{
    Annotated, Binds, Bound, Call, Class, Const, Def, Export, Facts, Import, Imported, Invocation,
    Kind, LanguagePack, Layout, Marked, Mentioned, Modules, Param, Part, Receiver, children,
    field_write, named, text, wildcard,
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

    /// The JDK types whose members are few and fixed; the table grows with evidence from the
    /// dogfood repositories, never by guess (ADR 0019).
    fn known_outside(&self) -> super::KnownOutside {
        &[
            ("java.io.Serializable", &[]),
            ("java.lang.Cloneable", &[]),
            ("java.util.RandomAccess", &[]),
            ("java.util.EventListener", &[]),
            ("java.lang.Runnable", &["run"]),
            ("java.lang.Comparable", &["compareTo"]),
            ("java.lang.AutoCloseable", &["close"]),
            ("java.io.Closeable", &["close"]),
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
            // A file a pack dropped has no facts, and a Kotlin script's declarations are its
            // own: no package or import reaches them.
            if facts.unbuilt || facts.budget.is_some() || facts.misparsed || path.ends_with(".kts")
            {
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
        let modules: BTreeMap<String, Rc<[String]>> = (tops.into_iter())
            .chain(
                packages
                    .iter()
                    .map(|(name, paths)| (name.clone(), paths.clone())),
            )
            .chain(types)
            .map(|(name, paths)| (name, paths.into()))
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
    returning: bool,
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
        returning: false,
        depth: 0,
        generics: Rc::new(Vec::new()),
    }];
    // The fields by declaring scope: a local is hidden by those of its class or an enclosing one.
    let mut fields: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut locals = Vec::new();
    let mut returned: BTreeMap<String, Vec<Value>> = BTreeMap::new();
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
    // The writes of a name that can be a field (`f = v`, `this.f = v`, `x.f = v`, `f++`): where
    // each is, its name, and whether it may be a field of the class around it (`this.f`, or a
    // bare `f` no local or parameter declares).
    let mut written: Vec<(String, String, bool)> = Vec::new();
    // The locals declared with an initializer, and every name the file assigns: a local the
    // file assigns holds no one value, so its bound is dropped (`Facts::bound`).
    let mut initialized: Vec<Bound> = Vec::new();
    let mut reassigned: Vec<(String, String)> = Vec::new();
    let mut rebound: Vec<Bound> = Vec::new();
    // The method or constructor of each scope, and the names of the `final` fields: where their
    // mentions are counted (`Facts::mentioned`).
    let mut bodies: Vec<(String, Node, Vec<String>)> = Vec::new();
    let mut final_fields: BTreeSet<String> = BTreeSet::new();
    // The field names some declaration leaves open to another file (`Facts::private_fields`).
    let mut open_fields: BTreeSet<String> = BTreeSet::new();
    let mut seen = Declared::new(tree.root_node());
    let mut qualification_bytes = QUALIFICATION_BYTES;
    while let Some(Visit {
        node,
        scope,
        interface,
        visible,
        definitions,
        mut returning,
        depth,
        mut generics,
    }) = stack.pop()
    {
        let Some(remaining) = qualification_bytes.checked_sub(scope.len()) else {
            // A partial overload family could turn an ambiguous call into a false exact one.
            return Ok(Facts {
                budget: Some(QUALIFICATION_BYTES),
                // Read off the bytes: a file with no facts still says so (#775).
                reflects: spells(src, &REFLECTIVE),
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
        if node.kind() == "method_declaration"
            && !defining
            && let Some(name) = node.child_by_field_name("name")
        {
            // No definition stands for this method (an anonymous class's or an enum constant
            // body's, at any depth below one, or past the depth), yet a call through a base type
            // may dispatch to it (#772).
            facts.anonymous.push(text(name, src).to_string());
        }
        if anonymous || node.kind() == "lambda_expression" {
            returning = false;
        }
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
            returning = kind == Kind::Method;
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
            if kind == Kind::Method {
                let names = parameters.iter().map(|(n, _, _)| n.clone()).collect();
                bodies.push((inner.as_ref().clone(), node, names));
            }
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
                .map(|(name, ty, _)| Param {
                    name,
                    binds: if ty.ends_with("...") {
                        Binds::Rest
                    } else {
                        Binds::Any
                    },
                    label: None,
                    default: None,
                    fields: Vec::new(),
                    closure: Default::default(),
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
                params,
                bases: if kind == Kind::Class {
                    bases(node, src, &generics)
                } else {
                    Vec::new()
                },
                result: match node
                    .child_by_field_name("type")
                    .map(|t| class_of(t, src, &generics))
                {
                    Some(Class::Type(ty)) if !constructor => Some(Class::Type(ty)),
                    _ => None,
                },
                ..Def::new(inner.as_ref().clone(), kind, node.byte_range())
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
        // An interface's constant is public, whatever it writes.
        if matches!(node.kind(), "field_declaration" | "constant_declaration")
            && !modifiers(node).contains(&"private")
        {
            for declaration in named(node).filter(|n| n.kind() == "variable_declarator") {
                if let Some(name) = declaration.child_by_field_name("name") {
                    open_fields.insert(text(name, src).to_string());
                }
            }
        }
        match node.kind() {
            "return_statement" if returning && definitions => {
                if let Some(expression) = named(node).next() {
                    returned
                        .entry(scope.as_ref().clone())
                        .or_default()
                        .push(value(expression, src, 0));
                }
            }
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
                        let initial = declaration.child_by_field_name("value");
                        bind(name, typed(initial, dimensions));
                        // An initializer block's local sits in its class's scope, which a method
                        // of the class reads through: it is no bound.
                        if let Some(initial) = initial.filter(|_| !initializer) {
                            initialized.push(Bound {
                                scope: scope.as_ref().clone(),
                                within: scope.as_ref().clone(),
                                name: text(name, src).to_string(),
                                value: value(initial, src, 0),
                            });
                        }
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
                        final_fields.insert(text(name, src).to_string());
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
                        final_fields.insert(text(name, src).to_string());
                        let initial = declaration.child_by_field_name("value");
                        let initial = initial.map(|n| value(n, src, 0));
                        finals.push((scope.as_ref().clone(), text(name, src).into(), initial));
                    }
                }
            }
            "type_identifier" => {
                outside.insert(text(node, src).to_string());
            }
            "assignment_expression" | "update_expression" => {
                let left = match node.kind() {
                    "assignment_expression" => node.child_by_field_name("left"),
                    _ => named(node).next(),
                };
                if let Some(n) = left.filter(|n| n.kind() == "identifier") {
                    reassigned.push((scope.as_ref().clone(), text(n, src).to_string()));
                    if let Some(right) = node.child_by_field_name("right") {
                        rebound.push(Bound {
                            scope: scope.as_ref().clone(),
                            within: scope.as_ref().clone(),
                            name: text(n, src).to_string(),
                            value: value(right, src, 0),
                        });
                    }
                }
                let write = match left {
                    Some(n) if n.kind() == "identifier" => {
                        (!declared_before(n, src, &mut seen)).then_some((n, true))
                    }
                    Some(n) if n.kind() == "field_access" => {
                        let this = n
                            .child_by_field_name("object")
                            .is_some_and(|o| o.kind() == "this");
                        n.child_by_field_name("field").map(|f| (f, this))
                    }
                    _ => None,
                };
                if let Some((name, own)) = write {
                    // The `this` of an anonymous class is not the class around it.
                    let own = own && definitions;
                    written.push((scope.as_ref().clone(), text(name, src).to_string(), own));
                }
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
                    fields
                        .entry(text(name, src).to_string())
                        .or_default()
                        .insert(scope.as_ref().clone());
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
                returning,
                depth: nested,
                generics: generics.clone(),
            });
        }
    }
    for def in &mut facts.defs {
        def.return_values = returned.remove(&def.name).unwrap_or_default();
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
    // What `Code::field_written_in` reads: a field so assigned holds no one value. A write of
    // the field of the class around it, the innermost one, names that class; any other, a field
    // it inherits, one of an outer class its own class may inherit or one of another object,
    // names none.
    let written: BTreeSet<(String, Option<String>)> = (written.into_iter())
        .map(|(scope, name, own)| {
            let around = innermost(&classes, &scope);
            let owner = around.filter(|at| own && declares_field(&fields, &name, at));
            (name, owner.map(str::to_string))
        })
        .collect();
    for (name, class) in written {
        facts.consts.push(field_write(&name, class.as_deref()));
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
    // A local is bound to its initializer when it is the one binding of its name in its
    // definition, no field of an enclosing class shares the name and its binding is never assigned.
    let mut declared: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for local in &locals {
        *declared.entry((&local.scope, &local.name)).or_default() += 1;
    }
    let bindings: BTreeSet<(&str, &str)> = declared
        .keys()
        .copied()
        .chain(
            facts
                .defs
                .iter()
                .flat_map(|d| d.params.iter().map(|p| (d.name.as_str(), p.name.as_str()))),
        )
        .chain(fields.iter().flat_map(|(name, scopes)| {
            scopes
                .iter()
                .map(move |scope| (scope.as_str(), name.as_str()))
        }))
        .collect();
    // A bare write whose binding is not known stays conservative by spelling. Dropping
    // it would prove an initializer unchanged from incomplete assignment knowledge.
    let mut unknown_writes = BTreeSet::new();
    let reassigned: BTreeSet<(String, String)> = reassigned
        .into_iter()
        .filter_map(|(scope, name)| {
            let mut at = scope.as_str();
            loop {
                if bindings.contains(&(at, name.as_str())) {
                    return Some((at.to_string(), name));
                }
                if at.is_empty() {
                    unknown_writes.insert(name);
                    return None;
                }
                at = crate::parent(at);
            }
        })
        .collect();
    let field_hides = |name: &str, scope: &str| {
        (fields.get(name)).is_some_and(|at| at.iter().any(|at| encloses(at, scope)))
    };
    let (kept, dropped): (Vec<Bound>, Vec<Bound>) = initialized.into_iter().partition(|b| {
        declared.get(&(b.scope.as_str(), b.name.as_str())) == Some(&1)
            && !reassigned.contains(&(b.scope.clone(), b.name.clone()))
            && !unknown_writes.contains(&b.name)
            && !field_hides(&b.name, &b.scope)
    });
    facts.bound = kept;
    rebound.extend(dropped);
    facts.assigned = rebound;
    let mut named_in: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for b in &facts.bound {
        named_in.entry(&b.scope).or_default().push(&b.name);
    }
    for (scope, body, parameters) in &bodies {
        let mut names = named_in.get(scope.as_str()).cloned().unwrap_or_default();
        // A parameter's too: a reader that follows one passed on counts its mentions (#772).
        for parameter in parameters {
            if !parameter.is_empty() && !names.contains(&parameter.as_str()) {
                names.push(parameter);
            }
        }
        for (name, count) in names.iter().zip(mentions(*body, src, &names)) {
            facts.mentioned.push(Mentioned {
                scope: scope.clone(),
                name: name.to_string(),
                count,
            });
        }
    }
    let fields: Vec<&str> = final_fields.iter().map(String::as_str).collect();
    for (name, count) in fields.iter().zip(mentions(tree.root_node(), src, &fields)) {
        facts.mentioned.push(Mentioned {
            scope: String::new(),
            name: name.to_string(),
            count,
        });
    }
    // Lombok adds members the source does not show (a `@Getter`'s accessor hands a private field
    // out), so a file that spells its name anywhere, a comment included, lists none. Nor does a
    // file that holds a Unicode escape (JLS 3.3) or a character the compiler drops from a name
    // (JLS 3.8): one may spell that name, or a step on a field, where this pack reads other text.
    let holds = |needle: &[u8]| src.windows(needle.len()).any(|bytes| bytes == needle);
    if !holds(b"lombok") && !holds(b"\\u") && !dropped(src) {
        facts.private_fields = (final_fields.difference(&open_fields).cloned()).collect();
    }
    facts.reflects = spells(src, &REFLECTIVE);
    facts.loops = super::outermost(
        tree.root_node(),
        &[
            "for_statement",
            "enhanced_for_statement",
            "while_statement",
            "do_statement",
        ],
    );
    facts.deferred = deferred(tree.root_node());
    facts.repeated = repeated(tree.root_node());
    for mut local in locals {
        if let Class::Result(n) = local.class {
            local.class = call_at(&results[n]).map_or(Class::Unknown, Class::Result);
        }
        if field_hides(&local.name, &local.scope) {
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

/// What the children of each node declare (the node's id to each name with the offset of the
/// first child that declares it), and the parent of each node, both read once per file:
/// `Node::parent()` is found from the root, so a walk up by it costs the depth squared.
struct Declared<'t> {
    root: Node<'t>,
    names: BTreeMap<usize, BTreeMap<String, usize>>,
    parents: Option<BTreeMap<usize, Node<'t>>>,
}

impl<'t> Declared<'t> {
    fn new(root: Node<'t>) -> Self {
        Declared {
            root,
            names: BTreeMap::new(),
            parents: None,
        }
    }

    /// The parent of `node`, from an index of every node's parent built on first use.
    fn parent(&mut self, node: Node<'t>) -> Option<Node<'t>> {
        let root = self.root;
        let parents = self.parents.get_or_insert_with(|| {
            let mut parents = BTreeMap::new();
            let mut pending = vec![root];
            while let Some(at) = pending.pop() {
                for child in children(at) {
                    parents.insert(child.id(), at);
                    pending.push(child);
                }
            }
            parents
        });
        parents.get(&node.id()).copied()
    }
}

/// Whether the identifier `name` is a local or a parameter declared before it, in a block
/// around it, a loop, a `catch`, a `try` resource, a lambda or the method. `seen` holds what
/// each block declares, read once, so a block of thousands of statements costs its width.
// ponytail: a pattern variable (`o instanceof Foo base`) and a local of an earlier `case` group
// are not read, so an assignment to either counts as a write of a field of that name.
fn declared_before<'t>(name: Node<'t>, src: &[u8], seen: &mut Declared<'t>) -> bool {
    let wanted = text(name, src);
    let mut child = name;
    while let Some(parent) = seen.parent(child) {
        if parent.kind() == "class_body" {
            return false;
        }
        let names = seen
            .names
            .entry(parent.id())
            .or_insert_with(|| declared_in(parent, src));
        if names.get(wanted).is_some_and(|&at| at < child.start_byte()) {
            return true;
        }
        child = parent;
    }
    false
}

/// The names the children of `parent` declare, and a compact constructor's parameters.
fn declared_in(parent: Node, src: &[u8]) -> BTreeMap<String, usize> {
    let mut names = BTreeMap::new();
    if parent.kind() == "compact_constructor_declaration" {
        for p in formal_parameters(parent) {
            declares(parent, p, src, &mut |n| {
                names.insert(n.to_string(), 0);
            });
        }
    }
    for s in named(parent) {
        declares(parent, s, src, &mut |n| {
            names.entry(n.to_string()).or_insert(s.start_byte());
        });
    }
    names
}

/// Tells `out` each name `sibling`, a child of `parent`, declares.
fn declares(parent: Node, sibling: Node, src: &[u8], out: &mut dyn FnMut(&str)) {
    match sibling.kind() {
        "local_variable_declaration" | "spread_parameter" => {
            for d in named(sibling).filter(|n| n.kind() == "variable_declarator") {
                if let Some(n) = d.child_by_field_name("name") {
                    out(text(n, src));
                }
            }
        }
        "formal_parameter" | "catch_formal_parameter" | "resource" => {
            if let Some(n) = sibling.child_by_field_name("name") {
                out(text(n, src));
            }
        }
        "formal_parameters" | "inferred_parameters" | "resource_specification" => {
            for n in named(sibling) {
                declares(sibling, n, src, out);
            }
        }
        // A lambda's parameter, or a loop's variable.
        "identifier" => {
            let declared =
                (parent.child_by_field_name("name")).or(parent.child_by_field_name("parameters"));
            let declared = match parent.kind() {
                "lambda_expression" | "enhanced_for_statement" => {
                    declared.is_some_and(|d| d.id() == sibling.id())
                }
                "inferred_parameters" => true,
                _ => false,
            };
            if declared {
                out(text(sibling, src));
            }
        }
        _ => {}
    }
}

/// The byte ranges of the lambdas and of the local and anonymous classes under `root`, nested ones
/// included, in source order (`Facts::deferred`).
fn deferred(root: Node) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    // A node and whether it sits in the body of a method, constructor, initializer or lambda.
    let mut stack = vec![(root, false)];
    while let Some((node, inside)) = stack.pop() {
        let kind = node.kind();
        let anonymous = kind == "class_body"
            && node
                .parent()
                .is_some_and(|p| p.kind() == "object_creation_expression");
        let local = inside
            && matches!(
                kind,
                "class_declaration" | "record_declaration" | "enum_declaration"
            );
        if kind == "lambda_expression" || anonymous || local {
            out.push(node.byte_range());
        }
        let body = matches!(
            kind,
            "method_declaration"
                | "constructor_declaration"
                | "compact_constructor_declaration"
                | "static_initializer"
                | "lambda_expression"
        );
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            // An instance initializer is a block straight in a class body.
            let initializer = kind == "class_body" && child.kind() == "block";
            stack.push((child, inside || body || initializer));
        }
    }
    out
}

/// The byte ranges of the code each loop under `root` runs again, nested loops included, in
/// source order (`Facts::repeated`).
fn repeated(root: Node) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        if matches!(
            node.kind(),
            "for_statement" | "enhanced_for_statement" | "while_statement" | "do_statement"
        ) {
            let init = node.children_by_field_name("init", &mut cursor).last();
            out.push(init.map_or(node.start_byte(), |i| i.end_byte())..node.end_byte());
        }
        stack.extend(node.children(&mut cursor));
    }
    // The walk keeps no order, and a loop written in an init clause starts before the range of
    // the `for` that holds it.
    out.sort_by_key(|r| r.start);
    out
}

/// How often each of `names` is written as an identifier under `root`, in the order given. A
/// method's name is no mention: a local `entries` passed to `.entries(entries)` is written once
/// there (#773).
fn mentions(root: Node, src: &[u8], names: &[&str]) -> Vec<u32> {
    let index: BTreeMap<&str, usize> = names.iter().enumerate().map(|(n, s)| (*s, n)).collect();
    let mut counts = vec![0; names.len()];
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "identifier" {
            if let Some(&n) = index.get(text(node, src)) {
                counts[n] += 1;
            }
            continue;
        }
        let method = (node.kind() == "method_invocation")
            .then(|| node.child_by_field_name("name"))
            .flatten();
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor).filter(|c| Some(*c) != method));
    }
    counts
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
        awaited: false,
        range,
        args: Vec::new(),
        kwargs: Vec::new(),
        decorates: None,
        by_path: false,
        root: None,
        on: None,
        unwraps: false,
        awaits: false,
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

/// The innermost class of `classes` that is `scope` or encloses it, found by the prefixes of
/// `scope` rather than a scan of the classes.
fn innermost<'a>(classes: &'a BTreeSet<String>, scope: &str) -> Option<&'a str> {
    let ends = scope
        .match_indices('.')
        .map(|(n, _)| n)
        .chain([scope.len()]);
    ends.rev()
        .find_map(|end| classes.get(&scope[..end]))
        .map(String::as_str)
}

/// Whether `class` declares a field `name`.
fn declares_field(fields: &BTreeMap<String, BTreeSet<String>>, name: &str, class: &str) -> bool {
    fields.get(name).is_some_and(|at| at.contains(class))
}

/// Whether the class scope `class` is `scope` or encloses it.
fn encloses(class: &str, scope: &str) -> bool {
    scope == class
        || scope
            .strip_prefix(class)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// The methods of `Class` that read a field by its name (`Facts::reflects`).
const REFLECTIVE: [&str; 4] = [
    "getDeclaredField",
    "getField",
    "getDeclaredFields",
    "getFields",
];

/// Whether `src` may hold a character the compiler drops from a name (JLS 3.8,
/// `Character.isIdentifierIgnorable`): a control that is no white space, or a format character
/// (a soft hyphen, a zero-width space, a byte order mark). Text that is not UTF-8 may, and any
/// character past the Basic Multilingual Plane counts, as do the unassigned ones beside the
/// format characters of today's Unicode: a later version may make one of them.
fn dropped(src: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(src) else {
        return true;
    };
    text.chars().any(|c| {
        matches!(
            u32::from(c),
            0x00..=0x08 | 0x0e..=0x1b | 0x7f..=0x9f | 0xad | 0x600..=0x605 | 0x61c | 0x6dd
                | 0x70f | 0x890..=0x89f | 0x8e2 | 0x180e | 0x200b..=0x200f | 0x202a..=0x202e
                | 0x2060..=0x206f | 0xfeff | 0xfff0..=0xfffb | 0x1_0000..
        )
    })
}

/// Whether `src` spells one of `names` as a whole identifier, anywhere: a call, a method
/// reference, a comment, a string. A Unicode escape (`\u0046`, JLS 3.3) is read as the character
/// it stands for, as the compiler reads it before anything else; one after an escaped backslash
/// is read so too, which can only find a name the file does not spell. A control that is no
/// white space and every character past ASCII, escaped or not, is read as not there: the
/// compiler drops some from a name (JLS 3.8), and a letter among them makes a longer name, which
/// this takes for the listed one, never the reverse. Bytes are read one by one, so the encoding
/// of the file does not matter.
fn spells(src: &[u8], names: &[&str]) -> bool {
    let mut token: Vec<u8> = Vec::new();
    let mut at = 0;
    while at <= src.len() {
        // One character: `\uXXXX`, with any number of `u`, or a byte.
        let escape = src[at..].strip_prefix(b"\\u").and_then(|rest| {
            let hex = &rest[rest.iter().take_while(|&&b| b == b'u').count()..];
            let digits = hex
                .get(..4)
                .filter(|d| d.iter().all(u8::is_ascii_hexdigit))?;
            let code = u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()?;
            Some((Some(code), src.len() - at - hex.len() + 4))
        });
        let (character, length) = escape.unwrap_or((src.get(at).map(|&b| u32::from(b)), 1));
        let named = (character.and_then(|c| u8::try_from(c).ok()))
            .filter(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$'));
        match (named, character) {
            (Some(c), _) => token.push(c),
            (None, Some(0x00..=0x08 | 0x0e..=0x1b | 0x7f..)) => {}
            // Any other character, and the end of the file, end a name.
            _ => {
                if names.iter().any(|name| name.as_bytes() == token) {
                    return true;
                }
                token.clear();
            }
        }
        at += length;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long enclosing name repeated by every node under it exhausts the budget: the file keeps
    /// no definitions and carries the budget `meada status` names, not `unbuilt`.
    #[test]
    fn a_file_past_the_qualification_budget_keeps_no_facts() {
        let name = "N".repeat(64 * 1024);
        let methods = "void m() { f(); }\n".repeat(QUALIFICATION_BYTES / name.len() + 1);
        let src = format!("class {name} {{\n{methods}}}\n");
        let facts = Java.parse("C.java", src.as_bytes()).unwrap();
        assert!(facts.defs.is_empty() && facts.calls.is_empty());
        assert_eq!(facts.budget, Some(QUALIFICATION_BYTES));
        assert!(!facts.unbuilt && !facts.reflects);
        // Whether it reads fields by reflection is read off its bytes, which no budget bounds.
        let reflective = src.replacen("f();", "getClass().getDeclaredField(\"b\");", 1);
        let facts = Java.parse("C.java", reflective.as_bytes()).unwrap();
        assert!(facts.budget.is_some() && facts.reflects);
        let src = format!("class {name} {{\nvoid m() {{ f(); }}\n}}\n");
        let facts = Java.parse("C.java", src.as_bytes()).unwrap();
        assert_eq!(facts.budget, None);
        assert!(!facts.defs.is_empty());
    }

    /// The methods no definition stands for (#772): an anonymous class's or an enum constant
    /// body's, and those of a class nested in one at any depth; a member or local class's
    /// elsewhere are definitions.
    #[test]
    fn methods_with_no_definition_are_listed() {
        let src = "class C { enum E { A { void a() {} } } void h() { \
            Object o = new Object() { void hold(int x) { class K { void k() {} } } \
            class M { void m() {} } }; class L { void l() {} } } }";
        let facts = Java.parse("C.java", src.as_bytes()).unwrap();
        assert_eq!(facts.anonymous, ["a", "hold", "k", "m"]);
    }

    /// A `final` field is private when every field declaration of the file that spells its name
    /// is (#775): a name another class declares without `private`, final or not, an interface's
    /// constant and a field that is not `final` are not. A file that names Lombok lists none.
    #[test]
    fn final_fields_private_wherever_the_file_declares_them_are_listed() {
        let src = "class C { private static final B kept = b(); private final B own = b(); \
            static final B open = b(); protected final B guarded = b(); \
            public final B shown = b(); private final B twin = b(); \
            private final B loose = b(); private B plain; } \
            class D { B twin; static B loose; } interface I { B shared = b(); }";
        let facts = Java.parse("C.java", src.as_bytes()).unwrap();
        assert_eq!(facts.private_fields, ["kept", "own"]);
        // Lombok adds members the source does not show: its file lists none.
        let lombok = format!("import lombok.Getter; {src}");
        let facts = Java.parse("C.java", lombok.as_bytes()).unwrap();
        assert_eq!(facts.private_fields, Vec::<String>::new());
        // A Unicode escape may spell Lombok's name, or a step on a field: its file lists none.
        let escaped = format!("{src} class E {{ String e = \"\\u00e9\"; }}");
        let facts = Java.parse("C.java", escaped.as_bytes()).unwrap();
        assert_eq!(facts.private_fields, Vec::<String>::new());
        // So may a character the compiler drops from a name (JLS 3.8), which this pack reads as
        // other text: a file that holds one, or is not UTF-8, lists none.
        for dropped in [
            "\u{ad}",
            "\u{200b}",
            "\u{7f}",
            "\u{85}",
            "\u{feff}",
            "\u{1f600}",
        ] {
            let hidden = format!("{src} class E {{ String e = \"{dropped}\"; }}");
            let facts = Java.parse("C.java", hidden.as_bytes()).unwrap();
            assert_eq!(facts.private_fields, Vec::<String>::new(), "{dropped:?}");
        }
        let latin = [src.as_bytes(), b" // caf\xe9"].concat();
        let facts = Java.parse("C.java", &latin).unwrap();
        assert_eq!(facts.private_fields, Vec::<String>::new());
        // Other text past ASCII is no such character.
        let accented = format!("{src} // \u{e9} \u{201c}ok\u{201d} \u{e7}\u{e3}o \u{d7}");
        let facts = Java.parse("C.java", accented.as_bytes()).unwrap();
        assert_eq!(facts.private_fields, ["kept", "own"]);
    }

    /// A file reads fields by reflection where it spells one of the four methods of `Class` as a
    /// whole name (#775): in a call, a method reference, a name written with a Unicode escape, a
    /// comment. A longer name is another method.
    #[test]
    fn a_file_that_spells_a_reflective_field_read_says_so() {
        let reflects = |body: &str| {
            let src = format!("class C {{ void f() throws Exception {{ {body} }} }}");
            Java.parse("C.java", src.as_bytes()).unwrap().reflects
        };
        assert!(reflects("Holder.class.getDeclaredField(\"b\");"));
        assert!(reflects("o.getClass().getFields();"));
        assert!(reflects("Lookup l = Holder.class::getDeclaredField;"));
        assert!(reflects("Holder.class.getDeclared\\u0046ield(\"b\");"));
        assert!(reflects("Holder.class.\\uu0067etField(\"b\");"));
        assert!(reflects("/* through getField */ lookup.find(\"b\");"));
        assert!(!reflects(
            "errors.getFieldErrors(); form.getFieldName(); my_getField();"
        ));
        assert!(!reflects("holder.build(); String e = \"\\u00e9\";"));
        // A character the compiler drops from a name (JLS 3.8) is read as not there: a soft
        // hyphen, raw or escaped, a control, a zero-width space, in any encoding.
        assert!(reflects("Holder.class.getDeclared\u{ad}Field(\"b\");"));
        assert!(reflects("Holder.class.getDeclared\\u00adField(\"b\");"));
        assert!(reflects("Holder.class.getDeclared\u{7f}Field(\"b\");"));
        assert!(reflects("Holder.class.getF\\u200bield(\"b\");"));
        assert!(spells(b"x.getDeclared\xadField()", &REFLECTIVE));
        // White space and punctuation end a name, escaped or not.
        assert!(!reflects(
            "get Field(); getDeclared.Field(); getDeclared\\u000aField();"
        ));
        // The name ends the file.
        assert!(spells(b"x::getFields", &REFLECTIVE));
        assert!(!spells(b"x::getFieldsOf", &REFLECTIVE));
    }

    /// Code that runs when its caller says: every lambda and local or anonymous class, nested ones
    /// too. A member class, a method and a field initializer without one are not.
    #[test]
    fn lambdas_and_local_and_anonymous_classes_are_deferred() {
        let src = "class C { Runnable f = () -> g(); class M { void m() {} } void h() { \
            class L { void l() { Runnable n = () -> 1; } } Object o = new Object() { }; \
            for (int i = 0; i < 2; i++) { run(); } } { Runnable b = () -> 2; } }";
        let facts = Java.parse("C.java", src.as_bytes()).unwrap();
        let text: Vec<&str> = (facts.deferred.iter()).map(|r| &src[r.clone()]).collect();
        assert_eq!(
            text,
            [
                "() -> g()",
                "class L { void l() { Runnable n = () -> 1; } }",
                "() -> 1",
                "{ }",
                "() -> 2",
            ]
        );
        assert!(facts.loops.len() == 1 && src[facts.loops[0].clone()].starts_with("for"));
    }

    /// Every loop, nested ones too, from where each turn starts: a `for` after its init clause.
    #[test]
    fn every_loop_is_repeated_from_the_end_of_its_init_clause() {
        let src = "class C { void h() { for (int i = 0; i < 2; i++) { while (go()) { run(); } } \
            do { run(); } while (go()); for (var x : xs) { run(); } } }";
        let facts = Java.parse("C.java", src.as_bytes()).unwrap();
        let text: Vec<&str> = (facts.repeated.iter()).map(|r| &src[r.clone()]).collect();
        assert_eq!(
            text,
            [
                " i < 2; i++) { while (go()) { run(); } }",
                "while (go()) { run(); }",
                "do { run(); } while (go());",
                "for (var x : xs) { run(); }",
            ]
        );
    }

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

    /// The markers `Code::field_written_in` reads (`Const`s named `.f` or `.f@class`) of a source.
    fn writes(src: &str) -> Vec<String> {
        let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
        let mut marks: Vec<_> = (facts.consts.iter())
            .filter(|c| c.name.starts_with('.'))
            .map(|c| c.name.clone())
            .collect();
        marks.sort();
        marks
    }

    /// A block of thousands of assignments costs its width: what a block declares is read once.
    #[test]
    fn a_block_of_many_assignments_is_read_in_linear_time() {
        let body = "a = 1;\n".repeat(50_000);
        let src = format!("class C {{ int a; void f() {{ {body} }} }}");
        let start = std::time::Instant::now();
        let mut marks = writes(&src);
        marks.dedup();
        assert_eq!(marks, [".a@C"]);
        // 1.8 s measured in a local debug build; the bound is over twenty times that.
        assert!(start.elapsed().as_secs() < 40, "{:?}", start.elapsed());
    }

    /// Thousands of fields and thousands of locals cost their sum: a local finds the fields of
    /// its name by a lookup, not a scan of every field.
    #[test]
    fn many_fields_and_locals_are_read_in_linear_time() {
        let fields = (0..5_000).fold(String::new(), |s, n| s + &format!("int f{n};\n"));
        let locals = (0..5_000).fold(String::new(), |s, n| {
            s + &format!("Foo f{n} = new Foo();\n")
        }) + "Foo control = new Foo();\n";
        let src = format!("class C {{ {fields} void f() {{ {locals} }} }}");
        let start = std::time::Instant::now();
        let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
        // Every local shares its name with a field, so none is bound to its initializer; the
        // control local, whose name no field has, is the one that is.
        assert_eq!(facts.bound.len(), 1);
        assert_eq!(facts.bound[0].name, "control");
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
    }

    /// Writes at the bottom of deeply nested blocks cost the depth each: a node's parent is read
    /// from an index, not found from the root.
    #[test]
    fn deeply_nested_writes_are_read_in_linear_time() {
        let depth = 1_000;
        let src = format!(
            "class C {{ int a; void f() {{ {} {} {} }} }}",
            "{ ".repeat(depth),
            "a = 1; ".repeat(200),
            "} ".repeat(depth)
        );
        let start = std::time::Instant::now();
        assert_eq!(writes(&src), [".a@C"]);
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
    }

    #[test]
    fn an_assignment_to_a_local_or_a_parameter_is_no_write() {
        let src = "class C { String base; String url;
            void a(String url) { String base = \"a\"; base = \"b\"; url = \"c\"; }
            void b() { for (String base : list) { base = \"d\"; } }
            void c() { { String base; } base = \"e\"; }
            void d() { Runnable r = () -> { String url = \"f\"; url += \"g\"; }; }
        }";
        assert_eq!(writes(src), [".base@C"]);
    }

    #[test]
    fn a_catch_parameter_a_varargs_one_and_a_compact_constructors_are_no_write() {
        let src = "class C { String base; String url;
            void a() { try { } catch (Exception base) { base = null; } }
            void b(String... url) { url = null; }
            void c() { try (Res base = open()) { base = null; } }
        }";
        assert_eq!(writes(src), Vec::<String>::new());
        let src = "record R(String base) { R { base = base.trim(); } }";
        assert_eq!(writes(src), Vec::<String>::new());
    }

    #[test]
    fn a_write_names_its_class_when_it_is_known() {
        let src = "class C { String base; String url; int n;
            void a() { this.base = \"a\"; n++; }
            void b(C other) { other.url = \"b\"; }
            class D { String base; void c() { base = \"c\"; } }
            void d() { new Object() { void e() { this.url = \"d\"; } }; }
        }";
        assert_eq!(writes(src), [".base@C", ".base@C.D", ".n@C", ".url"]);
    }

    #[test]
    fn a_write_of_a_name_only_an_outer_class_declares_names_no_class() {
        // `D` may inherit a `base` of its own.
        let src = "class C { String base; class D extends B { void s() { base = \"y\"; } } }";
        assert_eq!(writes(src), [".base"]);
        let src = "class C { void m() { java.util.function.BiConsumer<String, String> f = (url, x) -> { url = null; }; } }";
        assert_eq!(writes(src), Vec::<String>::new());
    }

    /// A local with no initializer, a loop variable, a `try` resource, a `catch` parameter and a
    /// lambda parameter are no bounds, and one sharing the name of a bound leaves it unread.
    #[test]
    fn a_local_is_bound_only_when_declared_once_with_an_initializer() {
        let held = |body: &str| {
            let src = format!("package a;\nclass C {{ void f() {{\n{body}\n}} }}\n");
            let facts = Java.parse("a/C.java", src.as_bytes()).unwrap();
            facts.bound.into_iter().map(|b| b.name).collect::<Vec<_>>()
        };
        assert_eq!(held("String a = x(); int b = 1, c;"), ["a", "b"]);
        // An initializer block's local is keyed by its class, which methods read through.
        let src = "class C { { String a = x(); } }";
        assert!(
            Java.parse("a/C.java", src.as_bytes())
                .unwrap()
                .bound
                .is_empty()
        );
        assert_eq!(held("String a; a = x();"), Vec::<String>::new());
        assert_eq!(
            held("for (String a : xs) { use(a); }"),
            Vec::<String>::new()
        );
        assert_eq!(held("try (R a = open()) { use(a); }"), Vec::<String>::new());
        assert_eq!(
            held("try { x(); } catch (E a) { use(a); }"),
            Vec::<String>::new()
        );
        assert_eq!(held("xs.forEach(a -> use(a));"), Vec::<String>::new());
        assert_eq!(
            held("if (o instanceof T a) { use(a); }"),
            Vec::<String>::new()
        );
        assert_eq!(held("String a = x(); a += y();"), Vec::<String>::new());
        assert_eq!(held("int a = 0; a++;"), Vec::<String>::new());
        assert_eq!(held("int a = 0; --a;"), Vec::<String>::new());
        assert_eq!(
            held("String a = x(); run(() -> { a = y(); });"),
            Vec::<String>::new()
        );
        assert_eq!(
            held("String a = x(); for (String a : xs) {}"),
            Vec::<String>::new()
        );
    }
}
