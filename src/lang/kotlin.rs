use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};
use tree_sitter_language::LanguageFn;

use super::{
    Annotated, Binds, Bound, Call, Class, Const, Def, Export, Facts, Impl, Import, Imported,
    Invocation, Kind, LanguagePack, Marked, Modules, Param, Part, Receiver, Value, bare_calls,
    children, field_write, named, text, wildcard,
};
use crate::{Result, qualify};

// ponytail: real source nests a few declarations; this prevents quadratic qualified names.
const DEFINITION_DEPTH: usize = 128;
// ponytail: real expressions nest a few levels; this bounds hostile source recursion.
const VALUE_DEPTH: usize = 64;
// ponytail: real member paths are short; this keeps crafted chains cheap.
const PATH_DEPTH: usize = 256;
// ponytail: a long package prefix must not multiply across a broad file without a budget.
const QUALIFICATION_BYTES: usize = 4 * 1024 * 1024;

const DECLARATION_KEYWORDS: &[&str] = &[
    "class",
    "fun",
    "interface",
    "object",
    "typealias",
    "val",
    "var",
];

const DEFAULT_IMPORTS: &[&str] = &[
    "java.lang",
    "kotlin",
    "kotlin.annotation",
    "kotlin.collections",
    "kotlin.comparisons",
    "kotlin.io",
    "kotlin.jvm",
    "kotlin.ranges",
    "kotlin.sequences",
    "kotlin.text",
];

const COMPARABLE: &[&str] = &[
    "compareTo",
    "coerceAtLeast",
    "coerceAtMost",
    "coerceIn",
    "rangeTo",
    "rangeUntil",
];

const CLOSEABLE: &[&str] = &["close", "use"];

unsafe extern "C" {
    fn tree_sitter_kotlin() -> *const ();
}

// SAFETY: the function is the tree-sitter CLI's generated constructor of a static language.
const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_kotlin) };

pub struct Kotlin;

impl LanguagePack for Kotlin {
    fn extensions(&self) -> &'static [&'static str] {
        &["kt", "kts"]
    }

    fn is_code(&self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        !matches!(name, "build.gradle.kts" | "settings.gradle.kts")
    }

    fn resolves_with(&self) -> Option<&'static dyn LanguagePack> {
        Some(&super::java::Java)
    }

    fn parse(&self, path: &str, src: &[u8]) -> Result<Facts> {
        parse(src, path.ends_with(".kts"))
    }

    fn extension_functions(&self) -> bool {
        true
    }

    fn known_outside(&self) -> super::KnownOutside {
        &[
            ("kotlin.Comparable", COMPARABLE),
            ("java.lang.Comparable", COMPARABLE),
            ("java.lang.AutoCloseable", CLOSEABLE),
            ("java.io.Closeable", CLOSEABLE),
            ("kotlin.AutoCloseable", CLOSEABLE),
        ]
    }

    fn config_files(&self) -> &'static [&'static str] {
        crate::jvm::spring::CONFIG_FILES
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        super::java::Java.modules(configs, files)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Access {
    Public,
    Internal,
    Private,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Top,
    Class,
    Code,
    Init,
}

struct Visit<'t> {
    node: Node<'t>,
    scope: Rc<String>,
    place: Place,
    access: Access,
    opaque: bool,
    implicit: Implicit,
    generics: Rc<Vec<String>>,
    typed_it: bool,
    depth: usize,
    named_len: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Implicit {
    Enclosing,
    Receiver,
    Unknown,
}

#[derive(Clone)]
enum Held {
    Class(Class),
    Name(String),
    Unknown,
}

struct Tracked {
    receiver: bool,
    parameter: bool,
    held: Held,
    on: Value,
}

struct Later {
    at: String,
    within: String,
    name: Option<String>,
    held: Held,
}

fn parse(src: &[u8], script: bool) -> Result<Facts> {
    let mut parser = Parser::new();
    parser.set_language(&LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let root = tree.root_node();
    SYSTEM_BOUND.set(binds_system(root, src));
    ENV_NAMES.set(env_names(root, src));
    let package = named(root)
        .find(|n| n.kind() == "package_header")
        .and_then(|n| named(n).find(|n| n.kind() == "qualified_identifier"))
        .map_or_else(String::new, |n| callee(n, src));
    let mut facts = Facts {
        package,
        ..Facts::default()
    };
    for package in DEFAULT_IMPORTS {
        wildcard(package, false, &mut facts);
    }
    for node in named(root).filter(|n| n.kind() == "import") {
        import(node, src, &mut facts);
    }
    let package = facts.package.clone();
    wildcard(&package, true, &mut facts);
    let mut stack = vec![Visit {
        node: root,
        scope: Rc::new(facts.package.clone()),
        place: Place::Top,
        access: Access::Public,
        opaque: false,
        implicit: Implicit::Enclosing,
        generics: Rc::new(Vec::new()),
        typed_it: false,
        depth: 0,
        named_len: facts.package.len(),
    }];
    let mut budget = QUALIFICATION_BYTES;
    let mut outside = BTreeSet::new();
    let mut fields: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut writes: Vec<(String, String, bool)> = Vec::new();
    let mut seen = Declared::new(root);
    let mut members: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut classes: BTreeSet<String> = BTreeSet::new();
    let mut query_locals = Vec::new();
    let mut query_writes = HashMap::new();
    let mut locals: Vec<Receiver> = Vec::new();
    let mut vals: Vec<String> = Vec::new();
    let mut chains: Vec<(usize, Range<usize>)> = Vec::new();
    let mut tracked: HashMap<usize, Tracked> = HashMap::new();
    let mut later: Vec<Later> = Vec::new();
    let mut extensions: BTreeSet<(String, String)> = BTreeSet::new();
    while let Some(Visit {
        node,
        scope,
        place,
        access,
        opaque,
        implicit,
        generics,
        typed_it,
        depth,
        named_len,
    }) = stack.pop()
    {
        let charge = if stores(node.kind()) {
            scope.len()
        } else {
            named_len
        };
        let Some(left) = budget.checked_sub(charge) else {
            return Ok(Facts {
                budget: Some(QUALIFICATION_BYTES),
                ..Facts::default()
            });
        };
        budget = left;
        if misparsed(node, src) {
            return Ok(Facts {
                misparsed: true,
                ..Facts::default()
            });
        }
        let (mut inner, mut inner_place, mut inner_access, mut inner_opaque, mut nested) =
            (scope.clone(), place, access, opaque, depth);
        let (mut inner_implicit, mut inner_generics) = (implicit, generics.clone());
        let mut inner_typed_it = typed_it;
        let mut skip_parameters = false;
        let mut inline = false;
        let defining = !opaque && depth < DEFINITION_DEPTH && place != Place::Init;
        match node.kind() {
            "class_declaration" | "object_declaration" | "companion_object" if defining => {
                let name = match node.child_by_field_name("name") {
                    Some(name) => text(name, src).to_string(),
                    None if node.kind() == "companion_object" => "Companion".into(),
                    None => continue,
                };
                inner = Rc::new(qualify(&scope, &name));
                classes.insert(inner.as_ref().clone());
                inner_access = access.max(own_access(node, src));
                inner_place = Place::Class;
                inner_generics = with_parameters(&generics, node, src);
                nested += 1;
                let mut bases = bases(node, src);
                if let Some(companion) = body(node)
                    .flat_map(named)
                    .find(|n| n.kind() == "companion_object")
                {
                    let own = companion.child_by_field_name("name");
                    let own = own.map_or("Companion", |n| text(n, src));
                    bases.push(format!("{name}.{own}"));
                }
                let params = constructor(node, src);
                if params.is_some() {
                    let family = facts.overloads.entry(inner.as_ref().clone()).or_default();
                    family.push(inner.as_ref().clone());
                    let primary = named(node).find(|n| n.kind() == "primary_constructor");
                    if primary.is_some_and(|p| own_access(p, src) == Access::Private) {
                        facts.sealed.push(inner.as_ref().clone());
                    }
                }
                let def = Def {
                    bases,
                    result: (node.kind() == "class_declaration").then(|| Class::Type(name.clone())),
                    ..definition(
                        &inner,
                        Kind::Class,
                        node.byte_range(),
                        params.unwrap_or_default(),
                    )
                };
                facts.defs.push(def);
                match platform(node, src).or_else(|| enclosing_expect(node, src)) {
                    Some("expect") => facts.expect.push(inner.as_ref().clone()),
                    Some("actual") => facts.actual.push(inner.as_ref().clone()),
                    _ => {}
                }
                if children(node).any(|c| c.kind() == "interface") {
                    facts.interfaces.push(inner.as_ref().clone());
                }
                if node.kind() == "object_declaration" {
                    facts.receivers.push(Receiver {
                        scope: inner.as_ref().clone(),
                        name: "INSTANCE".into(),
                        class: Class::Type(name.clone()),
                    });
                }
                if place != Place::Code {
                    let relative = (inner.strip_prefix(&format!("{}.", facts.package)))
                        .unwrap_or(&inner)
                        .to_string();
                    publish(&mut facts, inner_access, &relative, &inner);
                }
                annotate(&mut facts, node, src, &scope, &inner);
            }
            "function_declaration" if defining => {
                let Some(name) = node.child_by_field_name("name") else {
                    continue;
                };
                let on = receiver(node, src);
                let base = match &on {
                    Some(on) => format!("{on}.{}", text(name, src)),
                    None => text(name, src).to_string(),
                };
                inner_generics = with_parameters(&generics, node, src);
                let (params, types, classes) = parameters(node, src, &inner_generics, &mut outside);
                inner = Rc::new(qualify(&scope, &format!("{base}({})", types.join(","))));
                inner_access = access.max(own_access(node, src));
                inner_place = Place::Code;
                nested += 1;
                for (param, class) in params.iter().zip(classes) {
                    if class != Class::Unknown {
                        facts.receivers.push(Receiver {
                            scope: inner.as_ref().clone(),
                            name: param.name.clone(),
                            class,
                        });
                    }
                }
                annotate_parameters(&mut facts, node, src, &scope, &inner);
                if let Some(on) = on {
                    extension(
                        &mut facts,
                        &mut extensions,
                        &mut outside,
                        &scope,
                        &on,
                        &inner_generics,
                    );
                }
                let kind = if place == Place::Class {
                    Kind::Method
                } else {
                    Kind::Function
                };
                let result = (returned(node))
                    .map(|t| class_of(t, src, &inner_generics, &mut outside))
                    .filter(|c| matches!(c, Class::Type(_)));
                facts.defs.push(Def {
                    result,
                    return_values: function_returns(node, src),
                    private: kind == Kind::Method && own_access(node, src) == Access::Private,
                    ..definition(&inner, kind, node.byte_range(), params)
                });
                let family = qualify(&scope, &base);
                facts
                    .overloads
                    .entry(family.clone())
                    .or_default()
                    .push(inner.as_ref().clone());
                if place == Place::Top && !script && base == "main" {
                    facts.programs.push(inner.as_ref().clone());
                }
                if place == Place::Top {
                    publish(&mut facts, inner_access, &base, &family);
                } else if inner_access != Access::Public {
                    facts.hidden.push(inner.as_ref().clone());
                }
                annotate(&mut facts, node, src, &scope, &inner);
            }
            "function_declaration" if depth < DEFINITION_DEPTH => {
                inner = Rc::new(qualify(&scope, &format!("@{}", node.start_byte())));
                let range = node.byte_range();
                facts
                    .defs
                    .push(definition(&inner, Kind::Inline, range, Vec::new()));
                nested += 1;
                inline = true;
                inner_place = Place::Code;
            }
            "property_declaration" if matches!(place, Place::Top | Place::Class) && defining => {
                let names: Vec<Node> = (named(node))
                    .filter(|n| matches!(n.kind(), "variable_declaration"))
                    .filter_map(|n| named(n).find(|n| n.kind() == "identifier"))
                    .collect();
                let own = access.max(own_access(node, src));
                if named(node).any(|n| n.kind() == "getter")
                    && let [name] = names[..]
                {
                    let on = receiver(node, src);
                    let base = match &on {
                        Some(on) => format!("{on}.{}", text(name, src)),
                        None => text(name, src).to_string(),
                    };
                    if let Some(on) = on {
                        extension(
                            &mut facts,
                            &mut extensions,
                            &mut outside,
                            &scope,
                            &on,
                            &generics,
                        );
                    }
                    inner = Rc::new(qualify(&scope, &base));
                    inner_access = own;
                    inner_place = Place::Code;
                    nested += 1;
                    let kind = if place == Place::Class {
                        Kind::Method
                    } else {
                        Kind::Function
                    };
                    facts
                        .defs
                        .push(definition(&inner, kind, node.byte_range(), Vec::new()));
                    if place == Place::Top {
                        publish(&mut facts, own, &base, &inner);
                    } else if own != Access::Public {
                        facts.hidden.push(inner.as_ref().clone());
                    }
                    annotate(&mut facts, node, src, &scope, &inner);
                } else {
                    let constant = children(node).any(|c| c.kind() == "val");
                    let initial = (named(node))
                        .find(|n| !matches!(n.kind(), "modifiers" | "variable_declaration"))
                        .filter(|_| constant && names.len() == 1);
                    let class = match names.len() {
                        1 => declared(node, &generics, &mut outside, src),
                        _ => Class::Unknown,
                    };
                    for name in names {
                        let name = text(name, src);
                        fields
                            .entry(name.to_string())
                            .or_default()
                            .insert(owner(node, &scope));
                        let bound = Receiver {
                            scope: scope.as_ref().clone(),
                            name: name.to_string(),
                            class: class.clone(),
                        };
                        property(&mut facts, place, bound, constant, initial, src);
                        members
                            .entry(name.to_string())
                            .or_default()
                            .insert(scope.as_ref().clone());
                        let on = Marked::Field {
                            class: scope.as_ref().clone(),
                            name: name.to_string(),
                            is_static: place == Place::Top || in_object(node),
                        };
                        mark(&mut facts, on, node, src, &scope, FIELD_TARGETS);
                        if place == Place::Top && constant {
                            publish(&mut facts, own, name, &qualify(&scope, name));
                        }
                    }
                    inner_place = Place::Init;
                }
            }
            "class_parameter" if defining => {
                if let Some(name) = named(node).find(|n| n.kind() == "identifier") {
                    let written = children(node).any(|c| matches!(c.kind(), "val" | "var"));
                    let name = text(name, src);
                    let at = (node.parent().into_iter().flat_map(named))
                        .take_while(|n| n.id() != node.id())
                        .filter(|n| n.kind() == "class_parameter")
                        .count();
                    let on = Marked::Param(scope.as_ref().clone(), at);
                    mark(&mut facts, on, node, src, &scope, PARAM_TARGETS);
                    if written {
                        members
                            .entry(name.to_string())
                            .or_default()
                            .insert(scope.as_ref().clone());
                        let on = Marked::Field {
                            class: scope.as_ref().clone(),
                            name: name.to_string(),
                            is_static: false,
                        };
                        let targets = [FIELD_TARGETS, &["param"]].concat();
                        mark(&mut facts, on, node, src, &scope, &targets);
                    }
                    let ty = named(node).skip_while(|n| n.kind() != "identifier").nth(1);
                    let class = ty.map_or(Class::Unknown, |ty| {
                        class_of(ty, src, &generics, &mut outside)
                    });
                    if written {
                        let constant = children(node).any(|c| c.kind() == "val")
                            && annotations(node, src, &scope).is_empty();
                        fields
                            .entry(name.to_string())
                            .or_default()
                            .insert(scope.as_ref().clone());
                        let bound = Receiver {
                            scope: scope.as_ref().clone(),
                            name: name.to_string(),
                            class,
                        };
                        property(&mut facts, Place::Class, bound, constant, None, src);
                    } else if class != Class::Unknown {
                        let (scope, name) = (scope.as_ref().clone(), name.to_string());
                        locals.push(Receiver { scope, name, class });
                    } else {
                        facts.locals.push(qualify(&scope, name));
                    }
                }
                inner_place = Place::Init;
            }
            "secondary_constructor" if defining => {
                let (params, types, _) = parameters(node, src, &generics, &mut outside);
                let class = scope.rsplit('.').next().unwrap_or_default();
                let ctor = qualify(&scope, &format!("{class}({})", types.join(",")));
                facts.defs.push(Def {
                    result: Some(Class::Type(class.to_string())),
                    ..definition(&ctor, Kind::Method, node.byte_range(), params)
                });
                if access.max(own_access(node, src)) != Access::Public {
                    facts.hidden.push(ctor.clone());
                }
                if own_access(node, src) == Access::Private {
                    facts.sealed.push(ctor.clone());
                }
                (facts.overloads.entry(scope.as_ref().clone()).or_default()).push(ctor);
                inner_place = Place::Init;
            }
            "anonymous_initializer" | "secondary_constructor" => inner_place = Place::Init,
            "enum_entry" => {
                if let Some(name) = named(node).find(|n| n.kind() == "identifier") {
                    facts.receivers.push(Receiver {
                        scope: scope.as_ref().clone(),
                        name: text(name, src).to_string(),
                        class: Class::Unknown,
                    });
                }
            }
            "class_body" if node.parent().is_some_and(|p| p.kind() == "enum_entry") => {
                inner_opaque = true;
                inner_implicit = Implicit::Unknown;
            }
            "lambda_literal" | "anonymous_function" | "object_literal" => {
                inner_opaque = true;
                inner_place = Place::Code;
                let given = tracked
                    .remove(&node.id())
                    .filter(|_| depth < DEFINITION_DEPTH);
                if let Some(Tracked {
                    receiver,
                    parameter,
                    held,
                    on,
                }) = given
                {
                    inner = Rc::new(qualify(&scope, &format!("@{}", node.start_byte())));
                    let range = node.byte_range();
                    facts
                        .defs
                        .push(definition(&inner, Kind::Inline, range, Vec::new()));
                    nested += 1;
                    inline = true;
                    let (at, within) = (inner.as_ref().clone(), scope.as_ref().clone());
                    if receiver {
                        inner_implicit = Implicit::Receiver;
                        let held = held.clone();
                        later.push(Later {
                            at: at.clone(),
                            within: within.clone(),
                            name: None,
                            held,
                        });
                    }
                    if parameter && let Some((name, ty)) = lambda_parameter(node, src) {
                        skip_parameters = true;
                        inner_typed_it |= name == "it";
                        let written = ty.map_or(Class::Unknown, |t| {
                            class_of(t, src, &generics, &mut outside)
                        });
                        if written == Class::Unknown {
                            facts.bound.push(Bound {
                                scope: at.clone(),
                                within: within.clone(),
                                name: name.clone(),
                                value: on,
                            });
                        }
                        match held {
                            _ if written != Class::Unknown => locals.push(Receiver {
                                scope: at,
                                name,
                                class: written,
                            }),
                            Held::Class(class) => locals.push(Receiver {
                                scope: at,
                                name,
                                class,
                            }),
                            Held::Name(_) => later.push(Later {
                                at,
                                within,
                                name: Some(name),
                                held,
                            }),
                            Held::Unknown => facts.locals.push(qualify(&at, &name)),
                        }
                    }
                } else {
                    inner_implicit = Implicit::Unknown;
                    let lambda = node.kind() == "lambda_literal";
                    let it = lambda && named(node).all(|n| n.kind() != "lambda_parameters");
                    if (argument(node) || it && typed_it) && depth < DEFINITION_DEPTH {
                        inner = Rc::new(qualify(&scope, &format!("@{}", node.start_byte())));
                        let range = node.byte_range();
                        facts
                            .defs
                            .push(definition(&inner, Kind::Inline, range, Vec::new()));
                        nested += 1;
                        inline = true;
                        inner_typed_it &= !it;
                    }
                    if it {
                        facts.locals.push(qualify(&inner, "it"));
                    }
                }
            }
            "assignment" | "unary_expression" => {
                let target = if node.kind() == "assignment" {
                    assigned(&mut facts.calls, node, src, &mut seen);
                    node.child_by_field_name("left")
                } else {
                    let step = text(node, src);
                    let step = ["++", "--"]
                        .iter()
                        .any(|op| step.starts_with(op) || step.ends_with(op));
                    node.child_by_field_name("argument").filter(|_| step)
                };
                if let Some(target) = target.filter(|n| n.kind() == "identifier") {
                    let name = text(target, src);
                    let op = node
                        .child_by_field_name("operator")
                        .map_or("", |n| text(n, src));
                    let change = node
                        .child_by_field_name("right")
                        .map_or(super::QueryWrite::Other, |right| {
                            super::QueryWrite::of(&value(right, src, 0), name, op)
                        });
                    let mut at = scope.as_str();
                    while !at.is_empty() {
                        query_writes
                            .entry(qualify(at, name))
                            .and_modify(|old: &mut super::QueryWrite| *old = old.merge(change))
                            .or_insert(change);
                        at = crate::parent(at);
                    }
                }
                if let Some((name, own)) = target.and_then(|t| write_target(t, src, &mut seen)) {
                    let own = own && !opaque && implicit == Implicit::Enclosing;
                    writes.push((scope.as_ref().clone(), text(name, src).to_string(), own));
                }
            }
            "call_expression" => {
                if let Some((lambda, found)) = tracked_call(node, src) {
                    tracked.insert(lambda.id(), found);
                }
                let lambdas = depth < DEFINITION_DEPTH;
                if let Some(at) = call(node, src, &scope, implicit, lambdas) {
                    if defining
                        && at.callee.starts_with("(call_expression).")
                        && let Some(inner) = chained(node)
                    {
                        chains.push((facts.calls.len(), inner.byte_range()));
                    }
                    facts.calls.push(at);
                }
            }
            _ => {}
        }
        let binds = match node.kind() {
            "variable_declaration" if matches!(place, Place::Code | Place::Init) => {
                named(node).next()
            }
            "catch_block" => named(node).find(|n| n.kind() == "identifier"),
            "parameter" if !defining => named(node).next(),
            "function_declaration" | "class_declaration" | "object_declaration" if !defining => {
                node.child_by_field_name("name")
            }
            _ => None,
        };
        if let Some(name) = binds.filter(|n| n.kind() == "identifier") {
            let class = match (node.kind(), node.parent()) {
                ("variable_declaration", Some(p)) if p.kind() == "property_declaration" => {
                    declared(p, &generics, &mut outside, src)
                }
                ("variable_declaration", _) => written(node, &generics, &mut outside, src),
                ("parameter", _) if !vararg(node, src) => {
                    written(node, &generics, &mut outside, src)
                }
                _ => Class::Unknown,
            };
            let property = node
                .parent()
                .filter(|p| children(*p).any(|c| c.kind() == "val"));
            if let Some(initial) = property.and_then(initial) {
                facts.bound.push(Bound {
                    scope: scope.to_string(),
                    within: scope.to_string(),
                    name: text(name, src).to_string(),
                    value: value(initial, src, 0),
                });
                if class == Class::Unknown {
                    vals.push(qualify(&scope, text(name, src)));
                }
            }
            if place == Place::Code
                && node.kind() == "variable_declaration"
                && let Some(initializer) = node
                    .parent()
                    .filter(|p| children(*p).any(|c| c.kind() == "var"))
                    .and_then(initial)
            {
                query_locals.push((scope.to_string(), text(name, src).to_owned(), initializer));
            }
            if class == Class::Unknown {
                facts.locals.push(qualify(&scope, text(name, src)));
            } else {
                let (scope, name) = (scope.as_ref().clone(), text(name, src).to_string());
                locals.push(Receiver { scope, name, class });
            }
        }
        let inner_named = match inline || Rc::ptr_eq(&inner, &scope) {
            true => named_len,
            false => inner.len(),
        };
        let mut pushed: Vec<Node> = named(node).collect();
        if node.kind() == "property_declaration" && matches!(place, Place::Top | Place::Class) {
            pushed.retain(|n| {
                !matches!(
                    n.kind(),
                    "variable_declaration" | "multi_variable_declaration"
                )
            });
        }
        if node.kind() == "call_expression"
            && let Some(first) = pushed.first().copied()
            && trailing(node, first)
        {
            pushed.splice(0..1, named(first));
        }
        if skip_parameters {
            pushed.retain(|n| n.kind() != "lambda_parameters");
        }
        for child in pushed.into_iter().rev() {
            stack.push(Visit {
                node: child,
                scope: inner.clone(),
                place: inner_place,
                access: inner_access,
                opaque: inner_opaque,
                implicit: inner_implicit,
                generics: inner_generics.clone(),
                typed_it: inner_typed_it,
                depth: nested,
                named_len: inner_named,
            });
        }
    }
    if script {
        facts.exports.clear();
        facts.private.clear();
    }
    for local in &mut locals {
        let hidden = fields.get(&local.name);
        if hidden.is_some_and(|at| at.iter().any(|at| encloses(at, &local.scope))) {
            local.class = Class::Unknown;
        }
    }
    let writes: BTreeSet<(String, Option<String>)> = (writes.into_iter())
        .map(|(scope, name, own)| {
            let around = innermost(&classes, &scope);
            let owner = around.filter(|at| {
                own && (members.get(name.as_str())).is_some_and(|owners| owners.contains(*at))
            });
            (name, owner.map(str::to_string))
        })
        .collect();
    for (name, class) in writes {
        facts.consts.push(field_write(&name, class.as_deref()));
    }
    facts.receivers.extend(locals);
    facts.receivers = merge(std::mem::take(&mut facts.receivers));
    scoped(&mut facts, later);
    let called: BTreeMap<(usize, usize), usize> = (facts.calls.iter().enumerate())
        .filter(|(_, c)| c.decorates.is_none())
        .map(|(n, c)| ((c.range.start, c.range.end), n))
        .collect();
    for (outer, inner) in chains {
        facts.calls[outer].on = called.get(&(inner.start, inner.end)).copied();
    }
    facts.externs = outside.into_iter().collect();
    bare_calls(&mut facts);
    let private: std::collections::BTreeSet<&str> =
        facts.private.iter().map(String::as_str).collect();
    facts.star = super::Star::Only(
        (facts.exports.iter())
            .filter_map(|e| match e {
                Export::Local { exported, .. } if !private.contains(exported.as_str()) => {
                    Some(exported.clone())
                }
                _ => None,
            })
            .collect(),
    );
    let taken = |name: &String| {
        let (held, declared) = (vals.iter().filter(|v| *v == name).count(), &facts.locals);
        declared.iter().filter(|l| *l == name).count() > held
    };
    let shared: HashSet<String> = vals.iter().filter(|v| taken(v)).cloned().collect();
    facts
        .bound
        .retain(|b| !shared.contains(&qualify(&b.scope, &b.name)));
    let mut declarations = HashMap::new();
    for name in facts
        .locals
        .iter()
        .cloned()
        .chain(facts.receivers.iter().map(|r| qualify(&r.scope, &r.name)))
    {
        *declarations.entry(name).or_insert(0) += 1;
    }
    for (scope, name, initializer) in query_locals {
        let qualified = qualify(&scope, &name);
        if declarations.get(&qualified) != Some(&1) {
            continue;
        }
        let Some(change) = query_writes.get(&qualified) else {
            continue;
        };
        let mut value = value(initializer, src, 0);
        if change.keep(&mut value) {
            facts.bound.push(Bound {
                within: scope.clone(),
                scope,
                name,
                value,
            });
        }
    }
    facts.locals.sort();
    facts.locals.dedup();
    for names in facts.overloads.values_mut() {
        names.sort();
    }
    Ok(facts)
}

fn definition(name: &str, kind: Kind, range: Range<usize>, params: Vec<Param>) -> Def {
    Def {
        params,
        ..Def::new(name, kind, range)
    }
}

fn publish(facts: &mut Facts, access: Access, exported: &str, local: &str) {
    if access != Access::Public {
        facts.hidden.push(local.to_string());
    }
    if access == Access::Private {
        return;
    }
    let full = qualify(&facts.package, exported);
    let mut aliases = vec![exported.to_string()];
    if full != exported {
        aliases.push(full);
    }
    for alias in aliases {
        if access == Access::Internal {
            facts.private.push(alias.clone());
        }
        facts.exports.push(Export::Local {
            exported: alias,
            local: local.to_string(),
        });
    }
}

fn property(
    facts: &mut Facts,
    place: Place,
    bound: Receiver,
    constant: bool,
    initial: Option<Node>,
    src: &[u8],
) {
    if constant {
        facts.consts.push(Const {
            name: qualify(&bound.scope, &bound.name),
            value: initial.map_or(Value::Unknown, |n| value(n, src, 0)),
            ty: None,
            scope: None,
        });
    }
    if place == Place::Class || bound.class != Class::Unknown {
        facts.receivers.push(bound);
    } else if !constant {
        facts.locals.push(qualify(&bound.scope, &bound.name));
    }
}

fn own_access(node: Node, src: &[u8]) -> Access {
    let visibility = (named(node).filter(|n| n.kind() == "modifiers"))
        .flat_map(named)
        .filter(|n| n.kind() == "visibility_modifier")
        .map(|n| text(n, src))
        .next();
    match visibility {
        Some("private") => Access::Private,
        Some("internal") => Access::Internal,
        _ => Access::Public,
    }
}

fn platform<'s>(node: Node, src: &'s [u8]) -> Option<&'s str> {
    (named(node).filter(|n| n.kind() == "modifiers"))
        .flat_map(named)
        .find(|n| n.kind() == "platform_modifier")
        .map(|n| text(n, src))
}

fn enclosing_expect<'s>(node: Node, src: &'s [u8]) -> Option<&'s str> {
    let mut up = node.parent();
    while let Some(p) = up {
        if matches!(p.kind(), "class_declaration" | "object_declaration")
            && let Some(m @ "expect") = platform(p, src)
        {
            return Some(m);
        }
        up = p.parent();
    }
    None
}

fn constructor(node: Node, src: &[u8]) -> Option<Vec<Param>> {
    let class = node.kind() == "class_declaration"
        && children(node).all(|c| c.kind() != "interface")
        && (named(node).any(|n| n.kind() == "primary_constructor")
            || body(node)
                .flat_map(named)
                .all(|n| n.kind() != "secondary_constructor"));
    if !class {
        return None;
    }
    let params = (named(node).filter(|n| n.kind() == "primary_constructor"))
        .flat_map(named)
        .filter(|n| n.kind() == "class_parameters")
        .flat_map(named)
        .filter(|n| n.kind() == "class_parameter")
        .map(|p| Param {
            name: named(p)
                .find(|n| n.kind() == "identifier")
                .map_or("", |n| text(n, src))
                .to_string(),
            binds: if children(p).any(|c| c.kind() == "=") {
                Binds::Optional
            } else {
                Binds::Any
            },
            label: None,
            default: None,
            fields: Vec::new(),
            closure: Default::default(),
        });
    Some(params.collect())
}

fn body(node: Node) -> impl Iterator<Item = Node> {
    named(node).filter(|n| matches!(n.kind(), "class_body" | "enum_class_body"))
}

fn bases(node: Node, src: &[u8]) -> Vec<String> {
    (named(node).filter(|n| n.kind() == "delegation_specifiers"))
        .flat_map(named)
        .filter(|n| n.kind() == "delegation_specifier")
        .map(|specifier| {
            let mut ty = named(specifier).next();
            if let Some(n) = ty.filter(|n| n.kind() == "constructor_invocation") {
                ty = named(n).next();
            }
            ty.filter(|t| t.kind() == "user_type")
                .and_then(|t| type_name(t, src))
                .unwrap_or_else(|| "(type)".into())
        })
        .collect()
}

fn receiver(node: Node, src: &[u8]) -> Option<String> {
    let mut found = None;
    for child in children(node) {
        match child.kind() {
            "user_type" | "nullable_type" | "parenthesized_type" => found = Some(child),
            "identifier" | "variable_declaration" | "function_value_parameters" => break,
            _ => {}
        }
    }
    let mut ty = found?;
    while ty.kind() != "user_type" {
        ty = named(ty).find(|n| matches!(n.kind(), "user_type" | "nullable_type"))?;
    }
    type_name(ty, src)
}

fn type_name(ty: Node, src: &[u8]) -> Option<String> {
    let parts: Vec<&str> = (named(ty).filter(|n| n.kind() == "identifier"))
        .map(|n| text(n, src))
        .collect();
    (!parts.is_empty()).then(|| parts.join("."))
}

fn parameters(
    node: Node,
    src: &[u8],
    generics: &[String],
    outside: &mut BTreeSet<String>,
) -> (Vec<Param>, Vec<String>, Vec<Class>) {
    let (mut params, mut types, mut classes) = (Vec::new(), Vec::new(), Vec::new());
    let Some(list) = named(node).find(|n| n.kind() == "function_value_parameters") else {
        return (params, types, classes);
    };
    let mut vararg = false;
    for child in children(list) {
        match child.kind() {
            "parameter_modifiers" => vararg |= text(child, src).contains("vararg"),
            "parameter" => {
                let name = named(child).find(|n| n.kind() == "identifier");
                let ty = named(child).find(|n| n.kind() != "identifier");
                let mut spelled = ty.map_or_else(String::new, |t| spelling(t, src));
                if vararg {
                    spelled = format!("vararg {spelled}");
                }
                types.push(spelled);
                classes.push(match ty {
                    Some(t) if !vararg => class_of(t, src, generics, outside),
                    _ => Class::Unknown,
                });
                params.push(Param {
                    name: name.map_or("", |n| text(n, src)).to_string(),
                    binds: if vararg { Binds::Rest } else { Binds::Any },
                    label: None,
                    default: None,
                    fields: Vec::new(),
                    closure: Default::default(),
                });
                vararg = false;
            }
            "=" => {
                if let Some(last) = params.last_mut().filter(|p| p.binds == Binds::Any) {
                    last.binds = Binds::Optional;
                }
            }
            _ => {}
        }
    }
    (params, types, classes)
}

fn misparsed(node: Node, src: &[u8]) -> bool {
    node.kind() == "infix_expression"
        && named(node)
            .any(|n| n.kind() == "identifier" && DECLARATION_KEYWORDS.contains(&text(n, src)))
}

fn spelling(node: Node, src: &[u8]) -> String {
    let mut spelled = String::new();
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if node.is_error()
            || matches!(
                node.kind(),
                "annotation" | "line_comment" | "block_comment" | "type_modifiers"
            )
        {
            continue;
        }
        if node.child_count() == 0 {
            let token = text(node, src);
            let word = |c: char| c.is_alphanumeric() || c == '_';
            if spelled.ends_with(word) && token.starts_with(word) {
                spelled.push(' ');
            }
            spelled.push_str(token);
        } else {
            pending.extend(children(node).collect::<Vec<_>>().into_iter().rev());
        }
    }
    spelled
}

fn with_parameters(outer: &Rc<Vec<String>>, node: Node, src: &[u8]) -> Rc<Vec<String>> {
    let Some(list) = named(node).find(|n| n.kind() == "type_parameters") else {
        return outer.clone();
    };
    let mut all = outer.as_ref().clone();
    all.extend(
        (named(list).filter(|n| n.kind() == "type_parameter"))
            .filter_map(|p| named(p).find(|n| n.kind() == "identifier"))
            .map(|n| text(n, src).to_string()),
    );
    Rc::new(all)
}

fn class_of(ty: Node, src: &[u8], generics: &[String], outside: &mut BTreeSet<String>) -> Class {
    let mut ty = ty;
    while matches!(ty.kind(), "nullable_type" | "parenthesized_type") {
        let inner = named(ty).find(|n| {
            matches!(
                n.kind(),
                "user_type" | "nullable_type" | "parenthesized_type"
            )
        });
        match inner {
            Some(inner) => ty = inner,
            None => return Class::Unknown,
        }
    }
    if ty.kind() != "user_type" {
        return Class::Unknown;
    }
    let (mut path, mut arguments) = (Vec::new(), false);
    for part in named(ty) {
        match part.kind() {
            "identifier" if !arguments => path.push(text(part, src)),
            "type_arguments" => arguments = true,
            "line_comment" | "block_comment" => {}
            _ => return Class::Unknown,
        }
    }
    match path.first() {
        Some(head) if !generics.iter().any(|g| g == head) => {
            outside.insert(head.to_string());
            Class::Type(path.join("."))
        }
        _ => Class::Unknown,
    }
}

fn returned(node: Node) -> Option<Node> {
    children(node)
        .skip_while(|c| c.kind() != "function_value_parameters")
        .find(|c| {
            matches!(
                c.kind(),
                "user_type" | "nullable_type" | "parenthesized_type" | "function_type"
            )
        })
}

fn chained(node: Node) -> Option<Node> {
    let mut at = parts(node).0?;
    while let Some(next) = match at.kind() {
        "navigation_expression" if children(at).all(|c| c.kind() != "::") => named(at).next(),
        "unary_expression" if children(at).any(|c| c.kind() == "!!") => {
            at.child_by_field_name("argument")
        }
        _ => None,
    } {
        at = next;
    }
    (at.kind() == "call_expression").then_some(at)
}

fn written(var: Node, generics: &[String], outside: &mut BTreeSet<String>, src: &[u8]) -> Class {
    (named(var).nth(1)).map_or(Class::Unknown, |ty| class_of(ty, src, generics, outside))
}

fn vararg(node: Node, src: &[u8]) -> bool {
    (node.prev_named_sibling())
        .is_some_and(|p| p.kind() == "parameter_modifiers" && text(p, src).contains("vararg"))
}

fn declared(
    property: Node,
    generics: &[String],
    outside: &mut BTreeSet<String>,
    src: &[u8],
) -> Class {
    let mut vars = named(property).filter(|n| n.kind() == "variable_declaration");
    let (Some(var), None) = (vars.next(), vars.next()) else {
        return Class::Unknown;
    };
    if named(var).nth(1).is_some() {
        return written(var, generics, outside, src);
    }
    match initial(property).map(|child| held(child, src, 0)) {
        Some(Held::Class(class)) => class,
        _ => Class::Unknown,
    }
}

fn initial(node: Node) -> Option<Node> {
    let mut children = children(node).skip_while(|c| c.kind() != "=");
    children.find(|c| c.is_named())
}

fn held(node: Node, src: &[u8], depth: usize) -> Held {
    let mut node = node;
    while node.kind() == "parenthesized_expression" {
        match named(node).next() {
            Some(inner) => node = inner,
            None => return Held::Unknown,
        }
    }
    if depth >= VALUE_DEPTH {
        return Held::Unknown;
    }
    match node.kind() {
        "identifier" => Held::Name(text(node, src).to_string()),
        "call_expression" => {
            let (function, args) = parts(node);
            let Some(function) = function else {
                return Held::Unknown;
            };
            match function.kind() {
                "identifier" => Held::Class(Class::Named(text(function, src).to_string())),
                "navigation_expression" => {
                    let last = named(function).last().map(|n| text(n, src));
                    let on = named(function).next();
                    if let (Some("apply" | "also"), [lambda], Some(on)) = (last, &args[..], on)
                        && lambda.kind() == "annotated_lambda"
                    {
                        return held(on, src, depth + 1);
                    }
                    let path = callee(function, src);
                    let class = last.is_some_and(|l| l.starts_with(char::is_uppercase));
                    if path.contains('(') || !class {
                        Held::Unknown
                    } else {
                        Held::Class(Class::Named(path))
                    }
                }
                _ => Held::Unknown,
            }
        }
        _ => Held::Unknown,
    }
}

fn tracked_call<'t>(node: Node<'t>, src: &[u8]) -> Option<(Node<'t>, Tracked)> {
    let (function, args) = parts(node);
    let function = function?;
    let lambda = args.last().filter(|a| a.kind() == "annotated_lambda")?;
    let lambda = named(*lambda).find(|n| n.kind() == "lambda_literal")?;
    let (receiver, parameter, held, receiving) = match function.kind() {
        "navigation_expression"
            if args.len() == 1 && children(function).all(|c| c.kind() != "::") =>
        {
            let on = named(function).next()?;
            let name = named(function).last().map(|n| text(n, src))?;
            let held = held(on, src, 0);
            match name {
                "apply" | "run" => (true, false, held, Value::Unknown),
                "let" | "also" | "use" => (false, true, held, value(on, src, 0)),
                _ => return None,
            }
        }
        "identifier" => match (text(function, src), &args[..]) {
            ("with", [arg, _]) if arg.kind() == "value_argument" => {
                let mut parts = named(*arg);
                match (parts.next(), parts.next()) {
                    (Some(on), None) => (true, false, held(on, src, 0), Value::Unknown),
                    _ => return None,
                }
            }
            ("run", [_]) => (false, false, Held::Unknown, Value::Unknown),
            _ => return None,
        },
        _ => return None,
    };
    if receiver && matches!(held, Held::Unknown) {
        return None;
    }
    Some((
        lambda,
        Tracked {
            receiver,
            parameter,
            held,
            on: receiving,
        },
    ))
}

fn lambda_parameter<'t>(lambda: Node<'t>, src: &[u8]) -> Option<(String, Option<Node<'t>>)> {
    let params: Vec<Node> = (named(lambda).filter(|n| n.kind() == "lambda_parameters"))
        .flat_map(named)
        .collect();
    match params[..] {
        [] => Some(("it".into(), None)),
        [var] if var.kind() == "variable_declaration" => {
            let name = named(var).find(|n| n.kind() == "identifier")?;
            Some((text(name, src).to_string(), named(var).nth(1)))
        }
        _ => None,
    }
}

/// An extension function or property extends its receiver's type: a block of that type
fn extension(
    facts: &mut Facts,
    seen: &mut BTreeSet<(String, String)>,
    outside: &mut BTreeSet<String>,
    scope: &str,
    on: &str,
    generics: &[String],
) {
    let head = on.split('.').next().unwrap_or(on);
    let ty = if generics.iter().any(|g| g == head) {
        format!("({on})")
    } else {
        outside.insert(head.to_string());
        on.to_string()
    };
    if seen.insert((scope.to_string(), on.to_string())) {
        facts.extensions.push(Impl {
            scope: qualify(scope, on),
            ty,
            of: None,
            within: scope.to_string(),
        });
    }
}

fn merge(all: Vec<Receiver>) -> Vec<Receiver> {
    let mut at = HashMap::new();
    let mut receivers: Vec<Receiver> = Vec::new();
    for receiver in all {
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
    receivers
}

fn scoped(facts: &mut Facts, later: Vec<Later>) {
    let mut bound: HashMap<(&str, &str), Class> = HashMap::new();
    for r in &facts.receivers {
        bound.insert((r.scope.as_str(), r.name.as_str()), r.class.clone());
    }
    for d in &facts.defs {
        for p in &d.params {
            bound
                .entry((d.name.as_str(), p.name.as_str()))
                .or_insert(Class::Unknown);
        }
    }
    let hidden: HashSet<&str> = (facts.locals.iter().map(String::as_str))
        .chain(facts.consts.iter().map(|c| c.name.as_str()))
        .collect();
    let find = |within: &str, name: &str| {
        let mut at = Some(within);
        while let Some(s) = at {
            if let Some(class) = bound.get(&(s, name)) {
                return class.clone();
            }
            if hidden.contains(qualify(s, name).as_str()) {
                break;
            }
            at = (!s.is_empty()).then(|| crate::parent(s));
        }
        Class::Unknown
    };
    let (mut receivers, mut impls, mut locals) = (Vec::new(), Vec::new(), Vec::new());
    for Later {
        at,
        within,
        name,
        held,
    } in later
    {
        let class = match held {
            Held::Class(class) => class,
            Held::Name(held) => find(&within, &held),
            Held::Unknown => Class::Unknown,
        };
        match (name, class) {
            (None, class) => impls.push(Impl {
                scope: at,
                ty: match class {
                    Class::Type(ty) | Class::Named(ty) => ty,
                    _ => "(unknown)".into(),
                },
                of: None,
                within,
            }),
            (Some(name), Class::Unknown) => locals.push(qualify(&at, &name)),
            (Some(name), class) => receivers.push(Receiver {
                scope: at,
                name,
                class,
            }),
        }
    }
    facts.receivers.extend(receivers);
    facts.impls.extend(impls);
    facts.locals.extend(locals);
}

fn import(node: Node, src: &[u8], facts: &mut Facts) {
    let Some(path) = named(node).find(|n| n.kind() == "qualified_identifier") else {
        return;
    };
    let written = callee(path, src);
    if children(node).any(|n| n.kind() == "*") {
        wildcard(&written, true, facts);
        return;
    }
    let alias = (named(node))
        .filter(|n| n.kind() == "identifier")
        .last()
        .map(|n| text(n, src).to_string());
    facts.imports.push(Import {
        local: alias.unwrap_or_else(|| written.rsplit('.').next().unwrap_or(&written).into()),
        source: written,
        name: Imported::Qualified,
    });
}

fn trailing(node: Node, first: Node) -> bool {
    first.kind() == "call_expression"
        && named(node).skip(1).all(|n| n.kind() == "annotated_lambda")
        && named(first).all(|n| n.kind() != "annotated_lambda")
}

fn parts<'t>(node: Node<'t>) -> (Option<Node<'t>>, Vec<Node<'t>>) {
    let mut first = named(node).next();
    let mut args = Vec::new();
    let mut at = node;
    if let Some(inner) = first.filter(|&f| trailing(node, f)) {
        args.extend(named(node).skip(1));
        at = inner;
        first = named(inner).next();
    }
    let mut own: Vec<Node> = Vec::new();
    for child in named(at).skip(1) {
        match child.kind() {
            "value_arguments" => own.extend(named(child).filter(|n| n.kind() == "value_argument")),
            "annotated_lambda" => own.push(child),
            _ => {}
        }
    }
    own.extend(args);
    (first, own)
}

fn call(node: Node, src: &[u8], scope: &str, implicit: Implicit, lambdas: bool) -> Option<Call> {
    let (function, args) = parts(node);
    let function = function?;
    let mut callee = callee(function, src);
    match implicit {
        Implicit::Unknown => {
            if let Some(rest) = callee.strip_prefix("this.") {
                callee = format!("(anonymous).{rest}");
            } else if !callee.contains('.') {
                callee = format!("(anonymous).{callee}");
            }
        }
        Implicit::Receiver => {
            if let Some(rest) = callee.strip_prefix("super.") {
                callee = format!("(anonymous).{rest}");
            }
        }
        Implicit::Enclosing => {}
    }
    let mut at = Call {
        scope: scope.to_string(),
        callee,
        arity: Some(args.len()),
        awaited: false,
        labels: None,
        constructs: false,
        range: node.byte_range(),
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
    };
    (at.args, at.kwargs) = arguments(
        &args,
        src,
        0,
        lambdas.then_some(scope),
        pairs(function, src),
    );
    if let Some(ty) = type_argument(node, src) {
        at.kwargs
            .push((TYPE_ARGUMENT.into(), Value::Member(ty, "class".into())));
    }
    if function.kind() == "navigation_expression" {
        at.receiver = named(function).next().map(|n| value(n, src, 0));
    }
    Some(at)
}

#[derive(Clone, Copy)]
enum Args {
    Plain,
    Pairs,
    Elements(bool),
}

fn pairs(function: Node, src: &[u8]) -> Args {
    match function.kind() == "identifier" && text(function, src) == "mapOf" {
        true => Args::Pairs,
        false => Args::Plain,
    }
}

fn arguments(
    nodes: &[Node],
    src: &[u8],
    depth: usize,
    scope: Option<&str>,
    how: Args,
) -> (Vec<Value>, Vec<(String, Value)>) {
    let (mut args, mut kwargs) = (Vec::new(), Vec::new());
    let read = |v: Node| match (v.kind(), scope, how) {
        ("lambda_literal", Some(scope), _) => {
            Value::Function(qualify(scope, &format!("@{}", v.start_byte())))
        }
        (_, _, Args::Elements(nested)) => element(v, src, depth + 1, nested),
        ("infix_expression", _, Args::Pairs) => {
            let parts: Vec<Node> = named(v).collect();
            match parts[..] {
                [key, _, _] => value(key, src, depth + 1),
                _ => Value::Unknown,
            }
        }
        _ => value(v, src, depth + 1),
    };
    for &node in nodes {
        if node.kind() == "annotated_lambda" {
            let lambda = named(node).find(|n| n.kind() == "lambda_literal");
            args.push(lambda.map_or(Value::Unknown, read));
            continue;
        }
        let parts: Vec<Node> = named(node).collect();
        let named_argument = children(node).any(|c| c.kind() == "=");
        match parts[..] {
            [key, v] if named_argument && key.kind() == "identifier" => {
                kwargs.push((text(key, src).to_string(), read(v)));
            }
            [.., v] => args.push(read(v)),
            [] => args.push(Value::Unknown),
        }
    }
    (args, kwargs)
}

fn stores(kind: &str) -> bool {
    matches!(
        kind,
        "call_expression"
            | "annotation"
            | "lambda_literal"
            | "anonymous_function"
            | "object_literal"
            | "function_declaration"
            | "class_declaration"
            | "object_declaration"
            | "companion_object"
            | "property_declaration"
            | "variable_declaration"
            | "parameter"
            | "class_parameter"
            | "catch_block"
            | "enum_entry"
    )
}

fn append(parts: &mut Vec<Part>, found: Vec<Part>) {
    for part in found {
        match (parts.last_mut(), part) {
            (_, Part::Text(next)) if next.is_empty() => {}
            (Some(Part::Text(last)), Part::Text(next)) => last.push_str(&next),
            (_, part) => parts.push(part),
        }
    }
}

fn template(parts: &mut Vec<Part>, text: &str) {
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        let name = &rest[at + 1..];
        let end = name
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(name.len());
        let starts = name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_');
        if !starts {
            append(parts, vec![Part::Text(rest[..=at].into())]);
            rest = name;
            continue;
        }
        append(parts, vec![Part::Text(rest[..at].into())]);
        parts.push(Part::Ref(name[..end].into()));
        rest = &name[end..];
    }
    append(parts, vec![Part::Text(rest.into())]);
}

fn write_target<'t>(
    target: Node<'t>,
    src: &[u8],
    seen: &mut Declared<'t>,
) -> Option<(Node<'t>, bool)> {
    match target.kind() {
        "identifier" => (!declared_before(target, src, seen)).then_some((target, true)),
        "navigation_expression" => {
            let parts: Vec<Node> = named(target).collect();
            let name = parts.last().filter(|n| n.kind() == "identifier")?;
            let this = parts.len() == 2 && text(parts[0], src) == "this";
            Some((*name, this))
        }
        _ => None,
    }
}

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

// ponytail: a local of the script around it is not read, so an assignment to one counts as a
// write of a property of that name.
fn declared_before<'t>(name: Node<'t>, src: &[u8], seen: &mut Declared<'t>) -> bool {
    let wanted = text(name, src);
    let mut child = name;
    while let Some(parent) = seen.parent(child) {
        let outer = |n: Node| matches!(n.kind(), "class_body" | "enum_class_body" | "source_file");
        let member =
            parent.kind() == "property_declaration" && seen.parent(parent).is_some_and(outer);
        if outer(parent) {
            return false;
        }
        if !member {
            let names = seen
                .names
                .entry(parent.id())
                .or_insert_with(|| declared_in(parent, src));
            if names.get(wanted).is_some_and(|&at| at < child.start_byte()) {
                return true;
            }
        }
        child = parent;
    }
    false
}

fn declared_in(parent: Node, src: &[u8]) -> BTreeMap<String, usize> {
    let mut names = BTreeMap::new();
    for s in named(parent) {
        declares(parent, s, src, &mut |n| {
            names.entry(n.to_string()).or_insert(s.start_byte());
        });
    }
    names
}

fn declares(parent: Node, sibling: Node, src: &[u8], out: &mut dyn FnMut(&str)) {
    match sibling.kind() {
        "variable_declaration" | "parameter" => {
            if let Some(i) = named(sibling).next().filter(|i| i.kind() == "identifier") {
                out(text(i, src));
            }
        }
        "property_declaration"
        | "multi_variable_declaration"
        | "function_value_parameters"
        | "lambda_parameters" => {
            for n in named(sibling) {
                declares(sibling, n, src, out);
            }
        }
        "identifier" if parent.kind() == "catch_block" => out(text(sibling, src)),
        _ => {}
    }
}

fn assigned<'t>(calls: &mut [Call], node: Node<'t>, src: &[u8], seen: &mut Declared<'t>) {
    let (Some(left), Some(right), Some(operator)) = (
        node.child_by_field_name("left"),
        node.child_by_field_name("right"),
        node.child_by_field_name("operator"),
    ) else {
        return;
    };
    let property = match left.kind() {
        "identifier" => Some(left),
        "navigation_expression" => {
            let mut parts = named(left);
            let this = parts.next().is_some_and(|n| n.kind() == "this_expression");
            let name = parts.next().filter(|n| n.kind() == "identifier");
            name.filter(|_| this && parts.next().is_none())
        }
        _ => None,
    };
    let Some(property) = property.filter(|_| text(operator, src) == "=") else {
        return;
    };
    let key = format!("{}=", text(property, src));
    let (mut at, mut own) = (node, true);
    while let Some(parent) = seen.parent(at) {
        match parent.kind() {
            "lambda_literal" if argument(parent) => {
                let owner = std::iter::successors(seen.parent(parent), |n| seen.parent(*n))
                    .find(|n| n.kind() == "call_expression");
                let Some(owner) = owner else { return };
                let given = match own {
                    true => value(right, src, 0),
                    false => Value::Unknown,
                };
                let range = owner.byte_range();
                let Some(call) = calls.iter_mut().rev().find(|c| c.range == range) else {
                    return;
                };
                match call.kwargs.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, set)) => *set = Value::Unknown,
                    None => call.kwargs.push((key.clone(), given)),
                }
                own = false;
            }
            "lambda_literal"
            | "anonymous_function"
            | "function_declaration"
            | "class_body"
            | "object_literal" => return,
            _ => own = false,
        }
        at = parent;
    }
}

fn argument(node: Node) -> bool {
    node.kind() == "lambda_literal"
        && (node.parent())
            .is_some_and(|p| matches!(p.kind(), "annotated_lambda" | "value_argument"))
}

fn annotate(facts: &mut Facts, node: Node, src: &[u8], scope: &str, inner: &str) {
    for mut annotation in annotations(node, src, scope) {
        annotation.decorates = Some(inner.to_string());
        facts.calls.push(annotation);
    }
}

fn mark(facts: &mut Facts, on: Marked, node: Node, src: &[u8], scope: &str, targets: &[&str]) {
    let modifiers = named(node).filter(|n| n.kind() == "modifiers");
    for (annotation, target) in modifiers.flat_map(|m| annotations_of(m, src, scope)) {
        if targets.contains(&target) {
            let on = on.clone();
            facts.annotated.push(Annotated { on, annotation });
        }
    }
}

const FIELD_TARGETS: &[&str] = &["", "field", "set", "setparam"];
const PARAM_TARGETS: &[&str] = &["", "param"];

fn annotate_parameters(facts: &mut Facts, node: Node, src: &[u8], scope: &str, inner: &str) {
    let Some(list) = named(node).find(|n| n.kind() == "function_value_parameters") else {
        return;
    };
    let (mut at, mut pending) = (0, Vec::new());
    for child in named(list) {
        match child.kind() {
            "parameter_modifiers" => pending.extend(annotations_of(child, src, scope)),
            "parameter" => {
                for (annotation, target) in std::mem::take(&mut pending) {
                    if PARAM_TARGETS.contains(&target) {
                        let on = Marked::Param(inner.to_string(), at);
                        facts.annotated.push(Annotated { on, annotation });
                    }
                }
                at += 1;
            }
            _ => {}
        }
    }
}

fn in_object(node: Node) -> bool {
    let owner = node.parent().and_then(|body| body.parent());
    owner.is_some_and(|o| matches!(o.kind(), "object_declaration" | "companion_object"))
}

fn annotations<'t>(node: Node<'t>, src: &[u8], scope: &str) -> Vec<Call> {
    (named(node).filter(|n| n.kind() == "modifiers"))
        .flat_map(|modifiers| annotations_of(modifiers, src, scope))
        .map(|(annotation, _)| annotation)
        .collect()
}

fn annotations_of<'s>(modifiers: Node, src: &'s [u8], scope: &str) -> Vec<(Call, &'s str)> {
    let mut found = Vec::new();
    for annotation in named(modifiers).filter(|n| n.kind() == "annotation") {
        let Some(written) =
            named(annotation).find(|n| matches!(n.kind(), "user_type" | "constructor_invocation"))
        else {
            continue;
        };
        let (ty, args) = match written.kind() {
            "constructor_invocation" => {
                let args: Vec<Node> = (named(written))
                    .filter(|n| n.kind() == "value_arguments")
                    .flat_map(named)
                    .filter(|n| n.kind() == "value_argument")
                    .collect();
                (named(written).next(), args)
            }
            _ => (Some(written), Vec::new()),
        };
        let Some(name) = ty.and_then(|t| type_name(t, src)) else {
            continue;
        };
        let (positional, keywords) = arguments(&args, src, 0, None, Args::Elements(false));
        let target = (named(annotation).find(|n| n.kind() == "use_site_target"))
            .map_or("", |t| text(t, src).trim_end_matches(':').trim());
        let call = Call {
            scope: scope.to_string(),
            callee: name,
            arity: None,
            awaited: false,
            labels: None,
            constructs: false,
            range: annotation.byte_range(),
            args: positional,
            kwargs: keywords,
            decorates: None,
            by_path: false,
            root: None,
            on: None,
            unwraps: false,
            awaits: false,
            receiver: None,
            arg_types: Vec::new(),
            uses: Vec::new(),
        };
        found.push((call, target));
    }
    found
}

fn element(node: Node, src: &[u8], depth: usize, nested: bool) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    match node.kind() {
        "collection_literal" => Value::List(
            named(node)
                .map(|n| element(n, src, depth + 1, nested))
                .collect(),
        ),
        "call_expression" => {
            let (function, args) = parts(node);
            let Some(function) = function.filter(|f| {
                matches!(f.kind(), "identifier" | "navigation_expression")
                    && args.iter().all(|a| a.kind() == "value_argument")
            }) else {
                return value(node, src, depth);
            };
            let callee = callee(function, src);
            if callee == "arrayOf" {
                let (items, keywords) =
                    arguments(&args, src, depth + 1, None, Args::Elements(nested));
                return match (nested, keywords.is_empty()) {
                    (true, true) => Value::List(items),
                    (false, true) => Value::Call(Invocation {
                        callee,
                        receiver: None,
                        args: items,
                        kwargs: Vec::new(),
                        range: node.byte_range(),
                    }),
                    _ => Value::Unknown,
                };
            }
            let (positional, keywords) =
                arguments(&args, src, depth + 1, None, Args::Elements(true));
            let mut elements: Vec<(String, Value)> = Vec::new();
            elements.extend(positional.into_iter().take(1).map(|v| ("value".into(), v)));
            elements.extend(keywords);
            Value::New(callee, vec![Value::Object(elements)])
        }
        _ => value(node, src, depth),
    }
}

fn callee(node: Node, src: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut pending = vec![node];
    while let Some(n) = pending.pop() {
        if parts.len() + pending.len() >= PATH_DEPTH {
            return format!("({})", node.kind());
        }
        match n.kind() {
            "identifier" => parts.push(text(n, src)),
            "this_expression" if text(n, src) == "this" => parts.push("this"),
            "super_expression" if text(n, src) == "super" => parts.push("super"),
            "qualified_identifier" => {
                let ids: Vec<Node> = named(n).filter(|n| n.kind() == "identifier").collect();
                pending.extend(ids.into_iter().rev());
            }
            "unary_expression" if children(n).any(|c| c.kind() == "!!") => {
                match n.child_by_field_name("argument") {
                    Some(operand) => pending.push(operand),
                    None => parts.push("(expression)"),
                }
            }
            "navigation_expression" if children(n).all(|c| c.kind() != "::") => {
                let nodes: Vec<Node> = named(n)
                    .filter(|n| !matches!(n.kind(), "line_comment" | "block_comment"))
                    .collect();
                pending.extend(nodes.into_iter().rev());
            }
            kind => parts.push(match kind {
                "call_expression" => "(call_expression)",
                _ => "(expression)",
            }),
        }
    }
    parts.join(".")
}

pub const TYPE_ARGUMENT: &str = "<type>=";

fn type_argument(node: Node, src: &[u8]) -> Option<String> {
    let inner = named(node).next().filter(|f| f.kind() == "call_expression");
    let arguments = named(inner.unwrap_or(node)).find(|n| n.kind() == "type_arguments")?;
    let mut projections = named(arguments);
    let (only, rest) = (projections.next()?, projections.next());
    let ty = named(only)
        .find(|n| n.kind() == "user_type")
        .filter(|_| rest.is_none())?;
    let path: Vec<&str> = (named(ty).take_while(|n| n.kind() == "identifier"))
        .map(|n| text(n, src))
        .collect();
    (!path.is_empty()).then(|| path.join("."))
}

fn java_class(node: Node, src: &[u8]) -> Option<String> {
    let mut outer = named(node);
    let (literal, java) = (outer.next()?, outer.next()?);
    if outer.next().is_some()
        || text(java, src) != "java"
        || literal.kind() != "navigation_expression"
    {
        return None;
    }
    let mut inner = named(literal);
    let (on, class) = (inner.next()?, inner.next()?);
    let literal_of_class =
        children(literal).any(|c| c.kind() == "::") && text(class, src) == "class";
    (inner.next().is_none() && literal_of_class).then(|| callee(on, src))
}

const STRING_METHODS: &[&str] = &[
    "trim",
    "trimEnd",
    "trimStart",
    "removePrefix",
    "removeSuffix",
    "replace",
    "lowercase",
    "toString",
];

thread_local! {
    static ENV_NAMES: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
    static SYSTEM_BOUND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the file binds `System` to another type: an import of one (also under that alias),
fn binds_system(root: Node, src: &[u8]) -> bool {
    let imported = named(root).filter(|n| n.kind() == "import").any(|n| {
        let import = text(n, src).trim_start_matches("import").trim();
        import != "java.lang.System"
            && (import.ends_with(".System") || import.ends_with(" as System"))
    });
    let mut words =
        (src.split(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))).filter(|w| !w.is_empty());
    let mut before: &[u8] = b"";
    let declared = words.any(|word| {
        let declares = matches!(before, b"class" | b"interface" | b"object" | b"typealias");
        before = word;
        declares && word == b"System"
    });
    imported || declared
}

// ponytail: a `System` type another file of the package declares is not seen (`binds_system`).
fn getenv(callee: &str) -> bool {
    callee == "java.lang.System.getenv" || callee == "System.getenv" && !SYSTEM_BOUND.get()
}

fn read_variable(value: &Value) -> bool {
    matches!(value, Value::Str(parts) if matches!(parts[..], [Part::Env(_) | Part::EnvKey { .. }]))
}

fn reads_environment(value: &Value) -> bool {
    let holds = |name: &str| {
        let name = name.rsplit('.').next().unwrap_or(name);
        ENV_NAMES.with_borrow(|names| names.contains(name))
    };
    match value {
        Value::Ref(name) => holds(name),
        Value::Str(parts) => parts.iter().any(|part| match part {
            Part::Env(_) => true,
            Part::Ref(name) => holds(name),
            _ => false,
        }),
        _ => false,
    }
}

fn env_names(root: Node, src: &[u8]) -> HashSet<String> {
    let mut names = HashSet::new();
    if !src.windows(6).any(|w| w == b"getenv") {
        return names;
    }
    let mut pending = vec![(root, None)];
    while let Some((node, owner)) = pending.pop() {
        let owner = match node.kind() {
            "property_declaration" => named(node)
                .find(|n| n.kind() == "variable_declaration")
                .and_then(|d| named(d).find(|n| n.kind() == "identifier"))
                .map(|n| text(n, src)),
            "function_declaration" | "class_declaration" => None,
            _ => owner,
        };
        pending.extend(named(node).map(|n| (n, owner)));
        let function = named(node)
            .next()
            .filter(|f| f.kind() == "navigation_expression");
        if let (Some(owner), "call_expression", Some(function)) = (owner, node.kind(), function)
            && getenv(&callee(function, src))
        {
            names.insert(owner.to_string());
        }
    }
    names
}

fn function_returns(function: Node, src: &[u8]) -> Vec<Value> {
    let Some(body) = named(function).find(|n| n.kind() == "function_body") else {
        return Vec::new();
    };
    if children(body).any(|n| n.kind() == "=") {
        return named(body)
            .last()
            .map(|n| value(n, src, 0))
            .into_iter()
            .collect();
    }
    let mut out = Vec::new();
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "return_expression" => out.push(
                named(node)
                    .last()
                    .map_or(Value::Unknown, |n| value(n, src, 0)),
            ),
            "lambda_literal" => return Vec::new(),
            "function_declaration"
            | "anonymous_function"
            | "object_literal"
            | "class_declaration" => {}
            _ => stack.extend(named(node)),
        }
    }
    out
}

fn value(node: Node, src: &[u8], depth: usize) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    match node.kind() {
        "string_literal" | "multiline_string_literal" => {
            let (mut parts, mut pending) = (Vec::new(), String::new());
            for part in named(node) {
                match part.kind() {
                    "string_content" => pending.push_str(text(part, src)),
                    "escape_sequence" if text(part, src) == "\\$" => {
                        template(&mut parts, &std::mem::take(&mut pending));
                        append(&mut parts, vec![Part::Text("$".into())]);
                    }
                    "interpolation" => {
                        template(&mut parts, &std::mem::take(&mut pending));
                        match named(part).next().map(|n| value(n, src, depth + 1)) {
                            Some(Value::Str(found)) => append(&mut parts, found),
                            Some(Value::Ref(name)) => parts.push(Part::Ref(name)),
                            other => parts.push(other.as_ref().map_or(Part::Hole, Part::unread)),
                        }
                    }
                    _ => return Value::Unknown,
                }
            }
            template(&mut parts, &pending);
            if parts.is_empty() {
                parts.push(Part::Text(String::new()));
            }
            Value::Str(parts)
        }
        "navigation_expression" => match java_class(node, src) {
            Some(class) => Value::Member(class, "class".into()),
            None => Value::Ref(callee(node, src)),
        },
        "identifier" => Value::Ref(callee(node, src)),
        "this_expression" => Value::Ref("this".into()),
        "call_expression" => {
            let (function, args) = parts(node);
            let Some(function) = function else {
                return Value::Unknown;
            };
            let callee = callee(function, src);
            let (args, keywords) = arguments(&args, src, depth + 1, None, pairs(function, src));
            let navigation = function.kind() == "navigation_expression";
            if navigation
                && getenv(&callee)
                && keywords.is_empty()
                && let [arg] = &args[..]
                && let Some(read) = Part::env_key(arg)
            {
                return Value::Str(vec![read]);
            }
            let receiver = navigation
                .then(|| named(function).next())
                .flatten()
                .map(|n| value(n, src, depth + 1));
            let method = named(function).last().map(|n| text(n, src));
            if let Some(receiver) = receiver.as_ref().filter(|r| read_variable(r))
                && navigation
                && method.is_some_and(|m| STRING_METHODS.contains(&m))
            {
                return receiver.clone();
            }
            Value::Call(Invocation {
                callee,
                receiver: receiver.map(Box::new),
                args,
                kwargs: Vec::new(),
                range: node.byte_range(),
            })
        }
        "index_expression" => {
            let mut parts = named(node);
            let (Some(map), Some(key)) = (parts.next(), parts.next()) else {
                return Value::Unknown;
            };
            let (function, args) = self::parts(map);
            let function = function.filter(|f| f.kind() == "navigation_expression");
            match function {
                Some(function)
                    if map.kind() == "call_expression"
                        && args.is_empty()
                        && getenv(&callee(function, src)) =>
                {
                    match Part::env_key(&value(key, src, depth + 1)) {
                        Some(read) => Value::Str(vec![read]),
                        None => Value::Unknown,
                    }
                }
                _ => Value::Unknown,
            }
        }
        "unary_expression" if children(node).any(|c| c.kind() == "!!") => {
            let inner = node.child_by_field_name("argument");
            match inner.map(|n| value(n, src, depth + 1)) {
                Some(inner) if read_variable(&inner) => inner,
                _ => Value::Unknown,
            }
        }
        "binary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|n| text(n, src) == "?:") =>
        {
            let left = node.child_by_field_name("left");
            let left = left.map(|n| value(n, src, depth + 1));
            let right = node.child_by_field_name("right");
            match (left, right) {
                (Some(left), Some(right)) if read_variable(&left) => {
                    match reads_environment(&value(right, src, depth + 1)) {
                        true => Value::Str(vec![Part::Hole]),
                        false => left,
                    }
                }
                _ => Value::Unknown,
            }
        }
        "if_expression" => {
            let terms: Vec<_> = named(node)
                .filter(|n| !n.kind().ends_with("comment"))
                .collect();
            let side = |branch: Node| match branch.kind() {
                "block" => {
                    let mut terms = named(branch).filter(|n| !n.kind().ends_with("comment"));
                    match (terms.next(), terms.next()) {
                        (Some(one), None) => value(one, src, depth + 1),
                        _ => Value::Unknown,
                    }
                }
                _ => value(branch, src, depth + 1),
            };
            match terms.as_slice() {
                [_, yes, no] => super::query_alternative(side(*yes), side(*no)),
                _ => Value::Unknown,
            }
        }
        "number_literal" => Value::Scalar(text(node, src).parse().ok()),
        "collection_literal" => {
            Value::List(named(node).map(|n| value(n, src, depth + 1)).collect())
        }
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
                    Some(Value::Str(found)) => append(&mut parts, found),
                    Some(Value::Ref(name)) => parts.push(Part::Ref(name)),
                    other => parts.push(other.as_ref().map_or(Part::Hole, Part::unread)),
                }
            }
            Value::Str(parts)
        }
        _ => Value::Unknown,
    }
}

fn owner(node: Node, scope: &str) -> String {
    let companion = node.parent().and_then(|b| b.parent());
    match scope.rsplit_once('.') {
        Some((class, _)) if companion.is_some_and(|c| c.kind() == "companion_object") => {
            class.to_string()
        }
        _ => scope.to_string(),
    }
}

fn innermost<'a>(classes: &'a BTreeSet<String>, scope: &str) -> Option<&'a str> {
    let ends = [0]
        .into_iter()
        .chain(scope.match_indices('.').map(|(n, _)| n))
        .chain([scope.len()]);
    ends.rev()
        .find_map(|end| classes.get(&scope[..end]))
        .map(String::as_str)
}

fn encloses(class: &str, scope: &str) -> bool {
    class.is_empty()
        || scope == class
        || scope
            .strip_prefix(class)
            .is_some_and(|rest| rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_past_the_qualification_budget_keeps_no_facts() {
        let name = "N".repeat(64 * 1024);
        let funs = "fun m() { f() }\n".repeat(QUALIFICATION_BYTES / name.len() + 1);
        let src = format!("package a\nclass {name} {{\n{funs}}}\n");
        let facts = parse(src.as_bytes(), false).unwrap();
        assert!(facts.defs.is_empty() && facts.calls.is_empty());
        assert_eq!(facts.budget, Some(QUALIFICATION_BYTES));
        assert!(!facts.unbuilt);
        let src = format!("package a\nclass {name} {{\nfun m() {{ f() }}\n}}\n");
        let facts = parse(src.as_bytes(), false).unwrap();
        assert_eq!(facts.budget, None);
        assert!(!facts.defs.is_empty());
    }

    #[test]
    fn a_secondary_constructor_joins_its_classs_overloads() {
        let facts = parse(
            b"package a\nclass C {\n constructor(x: Int) {}\n private constructor() {}\n}\n\
              enum class E(val n: Int) {\n A(1);\n internal constructor() : this(0)\n}\n",
            false,
        )
        .unwrap();
        assert_eq!(facts.overloads["a.C"], ["a.C.C()", "a.C.C(Int)"]);
        assert_eq!(facts.overloads["a.E"], ["a.E", "a.E.E()"]);
        assert_eq!(facts.hidden, ["a.C.C()", "a.E.E()"]);
    }

    #[test]
    fn a_declaration_read_as_an_infix_call_leaves_the_file_out() {
        let facts = parse(b"package a\nfun f() {}\nval a = b class C\n", false).unwrap();
        assert!(facts.misparsed && facts.defs.is_empty());
        assert!(!facts.unbuilt);
        let facts = parse(
            b"package a\nval p = value to open\nval q = x `class` y\n",
            false,
        );
        let facts = facts.unwrap();
        assert!(!facts.misparsed);
        assert!(!serde_json::to_string(&facts).unwrap().contains("misparsed"));
        let facts = parse(b"package a\nval a = b class C\n", false).unwrap();
        assert!(
            serde_json::to_string(&facts)
                .unwrap()
                .contains("\"misparsed\":true")
        );
    }

    #[test]
    fn a_string_template_is_text_names_and_holes() {
        let text = |s: &str| Part::Text(s.into());
        let name = |s: &str| Part::Ref(s.into());
        let value_of = |literal: &str| {
            let facts = parse(
                format!("package a\nconst val x = {literal}\n").as_bytes(),
                false,
            );
            facts.unwrap().consts.remove(0).value
        };
        let Value::Str(parts) = value_of("\"$base/u/${id}/${a.b}/${f(1)}/$5 and $\"") else {
            panic!("a template folds");
        };
        let expected = [
            name("base"),
            text("/u/"),
            name("id"),
            text("/"),
            name("a.b"),
            text("/"),
            text("/$5 and $"),
        ];
        let [before @ .., invocation, tail] = parts.as_slice() else {
            panic!("a template has literal parts");
        };
        assert_eq!(before, &expected[..6]);
        assert!(matches!(invocation, Part::Invoke { callee, awaited: false, .. } if callee == "f"));
        assert_eq!(*tail, expected[6]);
        assert!(matches!(value_of("\"a\\nb\""), Value::Unknown));
        let Value::Str(parts) = value_of("\"\"") else {
            panic!("an empty literal folds");
        };
        assert_eq!(parts, [text("")]);
    }

    fn parts_of(source: &str) -> Option<Vec<Part>> {
        let facts = parse(source.as_bytes(), false).unwrap();
        match facts.consts.into_iter().next()?.value {
            Value::Str(parts) => Some(parts),
            _ => None,
        }
    }

    #[test]
    fn a_class_literal_is_the_member_class_of_the_type() {
        let of = |literal: &str| {
            let facts = parse(format!("package a\nval x = {literal}\n").as_bytes(), false);
            facts.unwrap().consts.remove(0).value
        };
        for literal in ["Api::class.java", "a.Api::class.java"] {
            let name = literal.split("::").next().unwrap();
            let Value::Member(ty, class) = of(literal) else {
                panic!("{literal} is a class literal");
            };
            assert_eq!((ty.as_str(), class.as_str()), (name, "class"), "{literal}");
        }
        assert!(!matches!(of("Api::create"), Value::Member(..)));
    }

    #[test]
    fn a_type_argument_is_the_class_of_a_keyword() {
        let of = |call: &str| {
            let src = format!("package a\nfun f() {{ {call} }}\n");
            let mut calls = parse(src.as_bytes(), false).unwrap().calls;
            let call = calls.swap_remove(0);
            (call.keyword(TYPE_ARGUMENT)).map(|v| match v {
                Value::Member(ty, class) => format!("{ty}.{class}"),
                _ => "?".into(),
            })
        };
        assert_eq!(of("one.create<Api>()").as_deref(), Some("Api.class"));
        assert_eq!(of("create<a.Api>()").as_deref(), Some("a.Api.class"));
        assert_eq!(of("create<List<Api>>()").as_deref(), Some("List.class"));
        assert_eq!(of("create<A, B>()"), None);
        assert_eq!(of("create(1)"), None);
    }

    #[test]
    fn a_local_val_is_bound_when_no_other_binding_shares_its_name() {
        let bound = |body: &str| {
            let src = format!("package a\nfun f(xs: List<Any>) {{ {body} }}\n");
            let facts = parse(src.as_bytes(), false).unwrap();
            facts.bound.into_iter().map(|b| b.name).collect::<Vec<_>>()
        };
        assert_eq!(bound("val a = xs.make()"), ["a"]);
        assert_eq!(
            bound("val a = xs.make(); for (a in xs) { use(a) }"),
            [] as [&str; 0]
        );
        assert_eq!(bound("val a = xs.make(); val a = xs.other()"), ["a", "a"]);
    }

    #[test]
    fn a_not_null_assertion_calls_on_its_operand() {
        let src = "package a\nfun f(x: Foo?) { x!!.ext(); x?.ext(); x!!!!.ext(); (x).ext() }\n";
        let facts = parse(src.as_bytes(), false).unwrap();
        let callees: Vec<&str> = facts.calls.iter().map(|c| c.callee.as_str()).collect();
        assert_eq!(callees, ["x.ext", "x.ext", "x.ext", "(expression).ext"]);
    }

    #[test]
    fn a_property_assigned_in_a_lambda_is_a_keyword_of_its_call() {
        let src =
            "package a\nfun f() { run { size = 3; this.label = \"x\"; if (c) { kind = 1 } } }\n";
        let facts = parse(src.as_bytes(), false).unwrap();
        let run = facts.calls.iter().find(|c| c.callee == "run").unwrap();
        let keys: Vec<(&str, String)> = (run.kwargs.iter())
            .map(|(k, v)| (k.as_str(), show(v)))
            .collect();
        assert_eq!(
            keys,
            [
                ("size=", "?".into()),
                ("label=", "\"x\"".into()),
                ("kind=", "?".into())
            ]
        );
    }

    fn show(v: &Value) -> String {
        let all = |vs: &[Value]| vs.iter().map(show).collect::<Vec<_>>().join(", ");
        match v {
            Value::Str(parts) => {
                let text: Vec<String> = (parts.iter())
                    .map(|p| match p {
                        Part::Text(t) => t.clone(),
                        _ => "?".into(),
                    })
                    .collect();
                format!("{:?}", text.concat())
            }
            Value::Ref(name) => name.clone(),
            Value::List(items) => format!("[{}]", all(items)),
            Value::New(name, args) => format!("new {name}({})", all(args)),
            Value::Object(entries) => {
                let entries: Vec<String> = (entries.iter())
                    .map(|(k, v)| format!("{k}: {}", show(v)))
                    .collect();
                format!("{{{}}}", entries.join(", "))
            }
            Value::Call(c) => {
                let receiver = c.receiver.as_deref().map(show).unwrap_or_default();
                format!("{receiver}.{}({})", c.callee, all(&c.args))
            }
            _ => "?".into(),
        }
    }

    #[test]
    fn an_annotation_element_value_is_a_new_with_its_elements() {
        let src = "package a\nclass C {\n\
            @Outer(items = [Inner(\"x\", n = arrayOf(\"p\", \"q\")), p.Inner(value = \"y\")],\n\
            list = arrayOf(Inner(\"z\")), plain = [\"a\"], one = Inner())\n\
            fun f() {}\n}\n";
        let facts = parse(src.as_bytes(), false).unwrap();
        let outer = facts.calls.iter().find(|c| c.callee == "Outer").unwrap();
        let of = |key: &str| show(outer.keyword(key).unwrap());
        assert_eq!(
            of("items"),
            "[new Inner({value: \"x\", n: [\"p\", \"q\"]}), new p.Inner({value: \"y\"})]"
        );
        assert_eq!(of("plain"), "[\"a\"]");
        assert_eq!(of("one"), "new Inner({})");
        assert_eq!(of("list"), ".arrayOf(new Inner({value: \"z\"}))");
    }

    #[test]
    fn a_mapof_entry_is_its_key() {
        let facts = parse(
            b"package a\nval x = \"k\" to v\nval m = mapOf(\"a\" to 1, K to 2, p)\n",
            false,
        )
        .unwrap();
        let shown: Vec<_> = facts.consts.iter().map(|c| show(&c.value)).collect();
        assert_eq!(shown[0], "?");
        let call = facts.calls.iter().find(|c| c.callee == "mapOf").unwrap();
        assert_eq!(
            call.args.iter().map(show).collect::<Vec<_>>(),
            ["\"a\"", "K", "p"]
        );
    }

    #[test]
    fn an_escaped_dollar_is_text() {
        let of = |literal: &str| parts_of(&format!("package a\nval x = {literal}\n"));
        let text = |s: &str| Some(vec![Part::Text(s.into())]);
        assert_eq!(of(r#""\${web.url}""#), text("${web.url}"));
        assert_eq!(
            of(r#""\${A:x}/$b""#),
            Some(vec![Part::Text("${A:x}/".into()), Part::Ref("b".into())])
        );
        assert_eq!(of(r#""\$name""#), text("$name"));
    }

    #[test]
    fn an_environment_read_is_one_variable_through_fallbacks_and_string_methods() {
        let of = |expression: &str, header: &str| {
            parts_of(&format!("package a\n{header}\nval x = {expression}\n"))
        };
        let env = Some(vec![Part::Env("X".into())]);
        for read in [
            r#"System.getenv("X")"#,
            r#"java.lang.System.getenv("X")"#,
            r#"System.getenv()["X"]"#,
            r#"System.getenv("X")!!"#,
            r#"System.getenv("X") ?: "http://localhost""#,
            r#"System.getenv("X") ?: error("unset")"#,
            r#"(System.getenv("X") ?: "http://localhost").removeSuffix("/")"#,
            r#"System.getenv()["X"]?.trimEnd('/') ?: "d""#,
        ] {
            assert_eq!(of(read, ""), env, "{read}");
        }
        let after = Some(vec![Part::Env("X".into()), Part::Text("/api".into())]);
        assert_eq!(of(r#"System.getenv("X") + "/api""#, ""), after);
        assert_eq!(of(r#""${System.getenv("X")}/api""#, ""), after);
        let two = r#"System.getenv("X") ?: System.getenv("Y")"#;
        assert_eq!(of(two, ""), Some(vec![Part::Hole]));
        for (read, header) in [
            (r#"System.getenv(name)"#, ""),
            (r#"System.getenv("X", "Y")"#, ""),
            (r#"System.getProperty("X")"#, ""),
            (r#"System.getenv("X")"#, "import example.System"),
            (r#"System.getenv("X")"#, "import example.Sys as System"),
            (r#"System.getenv("X")"#, "object System"),
            (r#"base.removeSuffix("/")"#, ""),
        ] {
            let found = of(read, header);
            assert!(
                found
                    .as_ref()
                    .is_none_or(|p| !matches!(p[..], [Part::Env(_)])),
                "{read}"
            );
        }
    }

    #[test]
    fn a_deeply_nested_environment_read_is_bounded() {
        let chain = format!("System.getenv(\"X\"){}", ".trim()".repeat(VALUE_DEPTH + 8));
        let nest = format!(
            "{}System.getenv(\"X\"){}",
            "(".repeat(VALUE_DEPTH * 4),
            ")".repeat(VALUE_DEPTH * 4)
        );
        for read in [chain, nest] {
            let found = parts_of(&format!("package a\nval x = {read}\n"));
            assert!(found.is_none_or(|p| !matches!(p[..], [Part::Env(_)])));
        }
    }

    #[test]
    fn a_long_chain_of_string_methods_is_read_in_linear_time() {
        for head in [r#"System.getenv("X")"#, "base", r#"foo("X")"#] {
            let source = format!("package a\nval x = {head}{}\n", ".trim()".repeat(2048));
            let start = std::time::Instant::now();
            let found = parts_of(&source);
            let took = start.elapsed();
            assert!(found.is_none(), "{head}");
            assert!(took.as_secs() < 40, "{head}: {took:?}");
        }
    }

    #[test]
    fn literals_joined_by_plus_make_one_text() {
        let found = parts_of(r#"val x = "\$" + "{key}" + "/a""#);
        assert_eq!(found, Some(vec![Part::Text("${key}/a".into())]));
    }

    #[test]
    fn a_fallback_that_reads_the_environment_through_a_property_is_a_hole() {
        let source = |fallback: &str| {
            let body = format!("val x = System.getenv(\"A\") ?: {fallback}\n");
            format!("package a\n{body}val other = System.getenv(\"B\")\nval plain = \"u\"\n")
        };
        let hole = Some(vec![Part::Hole]);
        assert_eq!(parts_of(&source("other")), hole);
        assert_eq!(parts_of(&source("this.other")), hole);
        assert_eq!(parts_of(&source("\"$other/a\"")), hole);
        let env = Some(vec![Part::Env("A".into())]);
        assert_eq!(parts_of(&source("plain")), env);
    }

    #[test]
    fn every_assignment_to_a_name_that_can_be_a_property_is_recorded_as_a_write() {
        let source = br#"package a
class C {
    @Value("\${k}") lateinit var base: String
    var other = 1
    fun f(o: C) { base = "x"; o.other += 2; this.third = 3; val l = 1 }
}
"#;
        let facts = parse(source, false).unwrap();
        let mut written: Vec<&str> = (facts.consts.iter())
            .filter_map(|c| c.name.strip_prefix('.'))
            .collect();
        written.sort();
        assert_eq!(written, ["base@a.C", "other", "third"]);
    }

    #[test]
    fn a_use_site_target_decides_what_the_annotation_marks() {
        let source = br#"package a
class C(
    @Value("1") val a: String,
    @param:Value("2") val b: String,
    @field:Value("3") val c: String,
    @get:Value("4") val d: String,
    @property:Value("5") val e: String,
) {
    @Value("6") lateinit var f: String
    @field:Value("7") lateinit var g: String
    @get:Value("8") lateinit var h: String
    @set:Value("9") lateinit var i: String
    @property:Value("10") lateinit var j: String
    fun m(@Value("11") x: String, @get:Value("12") y: String) {}
}
"#;
        let facts = parse(source, false).unwrap();
        let marks: Vec<String> = (facts.annotated.iter())
            .map(|a| {
                let n = match &a.annotation.args[0] {
                    Value::Str(p) => format!("{:?}", p[0]),
                    _ => String::new(),
                };
                match &a.on {
                    Marked::Field { name, .. } => format!("{n} field {name}"),
                    Marked::Param(_, at) => format!("{n} param {at}"),
                }
            })
            .collect();
        let expected = [
            r#"Text("1") param 0"#,
            r#"Text("1") field a"#,
            r#"Text("2") param 1"#,
            r#"Text("2") field b"#,
            r#"Text("3") field c"#,
            r#"Text("6") field f"#,
            r#"Text("7") field g"#,
            r#"Text("9") field i"#,
            r#"Text("11") param 0"#,
        ];
        assert_eq!(marks, expected);
    }

    #[test]
    fn annotations_on_fields_and_parameters_are_recorded_with_what_they_mark() {
        let source = br#"package a
class C(@Value("\${a}") private val p: String, q: String, @param:Value("b") r: Int) {
    @field:Value("\${c}") private lateinit var f: String
    fun m(x: Int, @Value("\${d}") y: String) {}
    companion object {
        @Value("\${e}") lateinit var s: String
    }
}
@Value("\${g}") lateinit var top: String
"#;
        let facts = parse(source, false).unwrap();
        let marks: Vec<String> = (facts.annotated.iter())
            .map(|a| match &a.on {
                Marked::Field {
                    class,
                    name,
                    is_static,
                } => format!("field {class}.{name} {is_static}"),
                Marked::Param(function, at) => format!("param {function} {at}"),
            })
            .collect();
        assert_eq!(
            marks,
            [
                "param a.C 0",
                "field a.C.p false",
                "param a.C 2",
                "field a.C.f false",
                "param a.C.m(Int,String) 1",
                "field a.C.Companion.s true",
                "field a.top true",
            ]
        );
        assert!(
            facts
                .annotated
                .iter()
                .all(|a| a.annotation.decorates.is_none())
        );
    }

    #[test]
    fn accessors_stay_accessors_beside_the_scanner_patch() {
        let src = b"package a
class A {
    val a: Int
        get() = 1
    val b: Int
        get(): Int = 2
    val c: Int
        get() { return 3 }
    var d: Int = 0
        private set
    var e: Int = 0
        get
        set(value) { field = value }
    val f: Int
        @JvmName(\"ff\") get() = 4
    val g: Int
        get ( ) = 5
    var h = 0
        internal get
    val i: Int
        get /* c */ () = 6
    var j = 1
        set(v) = Unit
        get() = 2
    fun z() {}
}
val top: Int
    get() = 7
fun after() {}
";
        let facts = parse(src, false).unwrap();
        let mut names: Vec<&str> = facts.defs.iter().map(|d| d.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "a.A",
                "a.A.a",
                "a.A.b",
                "a.A.c",
                "a.A.e",
                "a.A.f",
                "a.A.g",
                "a.A.h",
                "a.A.i",
                "a.A.j",
                "a.A.z()",
                "a.after()",
                "a.top"
            ]
        );
    }

    #[test]
    fn a_setter_parameter_is_any_name() {
        let src = "package a
class A {
    var p: Int = 0
        get() = field
        set(значение) { field = значение }
    var q: Int = 0
        get() = field
        set(ñ) { field = ñ }
    var r: Int = 0
        get() = field
        set(_v) { field = _v }
    var s: Int = 0
        get() = field
        set(`odd name`) { field = `odd name` }
    var t: Int = 0
        get() = field
        set (v) { field = v }
    fun z() {}
}
";
        let facts = parse(src.as_bytes(), false).unwrap();
        let mut names: Vec<&str> = facts.defs.iter().map(|d| d.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "a.A", "a.A.p", "a.A.q", "a.A.r", "a.A.s", "a.A.t", "a.A.z()"
            ]
        );
    }

    fn writes(src: &str) -> Vec<String> {
        let facts = parse(src.as_bytes(), false).unwrap();
        let mut marks: Vec<_> = (facts.consts.iter())
            .filter(|c| c.name.starts_with('.'))
            .map(|c| c.name.clone())
            .collect();
        marks.sort();
        marks
    }

    #[test]
    fn a_block_of_many_assignments_is_read_in_linear_time() {
        let body = "a = 1\n".repeat(50_000);
        let src = format!("package a\nclass C {{ var a = 0\n fun f() {{\n{body} }} }}");
        let start = std::time::Instant::now();
        let mut marks = writes(&src);
        marks.dedup();
        assert_eq!(marks, [".a@a.C"]);
        assert!(start.elapsed().as_secs() < 40, "{:?}", start.elapsed());
    }

    #[test]
    fn many_properties_and_locals_are_read_in_linear_time() {
        let fields = (0..5_000).fold(String::new(), |s, n| s + &format!("var f{n} = 0\n"));
        let locals = (0..5_000).fold(String::new(), |s, n| {
            s + &format!("val f{n}: Foo = Foo()\n")
        }) + "val control: Foo = Foo()\n";
        let src = format!("package a\nclass C {{\n{fields} fun f() {{\n{locals} }} }}");
        let start = std::time::Instant::now();
        let facts = parse(src.as_bytes(), false).unwrap();
        let typed: Vec<_> = (facts.receivers.iter())
            .filter(|r| r.class != Class::Unknown)
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(typed, ["control"]);
        assert!(start.elapsed().as_secs() < 15, "{:?}", start.elapsed());
    }

    #[test]
    fn deeply_nested_writes_are_read_in_linear_time() {
        let depth = 1_000;
        let src = format!(
            "package a\nclass C {{ var a = 0\n fun f() {{ {} {} {} }} }}",
            "if (true) { ".repeat(depth),
            "a = 1; ".repeat(200),
            "} ".repeat(depth)
        );
        let start = std::time::Instant::now();
        assert_eq!(writes(&src), [".a@a.C"]);
        assert!(start.elapsed().as_secs() < 15, "{:?}", start.elapsed());
    }

    #[test]
    fn an_assignment_to_a_local_or_a_parameter_is_no_write() {
        let src = "package a
            class C(var base: String) {
                var url = \"u\"
                fun f(host: String) { var base = \"a\"; base = \"b\"; var h = host; h = \"c\" }
                fun g() { for (url in list) { var x = url; x += \"d\" } }
                fun h() { try { } catch (base: Exception) { var y = 1; y++ } }
                fun i() { list.forEach { url -> var z = url; z = \"e\" } }
            }";
        assert_eq!(writes(src), Vec::<String>::new());
    }

    #[test]
    fn a_write_names_its_class_when_it_is_known() {
        let src = "package a
            class C(var base: String) {
                var url = \"u\"
                var n = 0
                fun f(o: C) { this.base = \"a\"; n++; o.url = \"b\" }
                fun g() { list.forEach { url = it } }
                class D { var base = \"\"; fun h() { base = \"c\" } }
            }";
        assert_eq!(writes(src), [".base@a.C", ".base@a.C.D", ".n@a.C", ".url"]);
    }

    #[test]
    fn a_write_of_a_name_only_an_outer_class_or_the_package_declares_names_no_class() {
        let src = "package a
            var base = \"\"
            fun C.reset() { this.base = \"x\" }
            class C { var url = \"\"; class D : B() { fun s() { url = \"y\" } } }";
        assert_eq!(writes(src), [".base", ".url"]);
    }
}
