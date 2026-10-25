//! The Kotlin language pack: one source file's syntax, with no compiler, JDK or build tool.
//! Its files resolve with the Java pack's (`LanguagePack::resolves_with`), so the two share one
//! layout of packages and types and call each other by fully qualified name.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use tree_sitter::{Node, Parser};

use super::{
    Binds, Call, Class, Const, Def, Export, Facts, Import, Imported, Invocation, Kind,
    LanguagePack, Modules, Param, Part, Receiver, Value, children, named, text,
};
use crate::{Result, qualify};

/// How many named declarations may enclose a definition. Deeper bodies keep their enclosing
/// symbol.
// ponytail: real source nests a few declarations; this prevents quadratic qualified names.
const DEFINITION_DEPTH: usize = 128;
/// How deep arguments and concatenations are read before their value becomes unknown.
// ponytail: real expressions nest a few levels; this bounds hostile source recursion.
const VALUE_DEPTH: usize = 64;
/// How many segments of a callee are kept before it becomes an unknown expression.
// ponytail: real member paths are short; this keeps crafted chains cheap.
const PATH_DEPTH: usize = 256;
/// Enclosing-name bytes across syntax visits, before a file yields no facts.
// ponytail: a long package prefix must not multiply across a broad file without a budget.
const QUALIFICATION_BYTES: usize = 4 * 1024 * 1024;

/// The packages every Kotlin file on the JVM imports on demand without writing it.
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

pub struct Kotlin;

impl LanguagePack for Kotlin {
    fn extensions(&self) -> &'static [&'static str] {
        &["kt", "kts"]
    }

    /// Gradle's Kotlin DSL files are build manifests, not code.
    fn is_code(&self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        !matches!(name, "build.gradle.kts" | "settings.gradle.kts")
    }

    /// Calls between Kotlin and Java files resolve by fully qualified name, through the Java
    /// pack's layout of packages and types and its flags (`this`, `super`, class members in
    /// scope, on-demand imports).
    fn resolves_with(&self) -> Option<&'static dyn LanguagePack> {
        Some(&super::java::Java)
    }

    fn parse(&self, path: &str, src: &[u8]) -> Result<Facts> {
        parse(src, path.ends_with(".kts"))
    }

    fn config_files(&self) -> &'static [&'static str] {
        &[]
    }

    fn modules(
        &self,
        configs: &[(&str, &[u8])],
        files: &BTreeMap<&str, &Facts>,
    ) -> Result<Modules> {
        super::java::Java.modules(configs, files)
    }
}

/// Who sees a declaration: Kotlin's default is public; `internal` is the repository's module
/// only, `private` its file or class only.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Access {
    Public,
    Internal,
    Private,
}

/// Where a node sits: at a file's top level, directly in a class body, in code (a function
/// body, a getter), or in code whose scope is a class or the file (an `init` block, a secondary
/// constructor, a property's initializer), where a declaration is no definition.
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
    /// Inside a lambda, an object literal or an enum entry's body: whatever the source calls
    /// there may be a member of an implicit receiver no syntax names, so a bare or `this` call
    /// is read as a name match, and declarations there are no definitions.
    opaque: bool,
    depth: usize,
}

/// One file's facts; a `script`'s declarations are its own, exported to no other file.
fn parse(src: &[u8], script: bool) -> Result<Facts> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let root = tree.root_node();
    let package = named(root)
        .find(|n| n.kind() == "package_header")
        .and_then(|n| named(n).find(|n| n.kind() == "qualified_identifier"))
        .map_or_else(String::new, |n| callee(n, src));
    let mut facts = Facts {
        package,
        ..Facts::default()
    };
    // Globs are read the latest first, and one `inside` stops the reading: the file's own
    // package comes first, then the star imports, which see `internal` declarations, then the
    // default imports. Explicit imports come before any glob.
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
        depth: 0,
    }];
    let mut budget = QUALIFICATION_BYTES;
    while let Some(Visit {
        node,
        scope,
        place,
        access,
        opaque,
        depth,
    }) = stack.pop()
    {
        let Some(left) = budget.checked_sub(scope.len()) else {
            // A partial overload family could turn an ambiguous call into a false exact one.
            return Ok(Facts {
                unbuilt: true,
                ..Facts::default()
            });
        };
        budget = left;
        let (mut inner, mut inner_place, mut inner_access, mut inner_opaque, mut nested) =
            (scope.clone(), place, access, opaque, depth);
        let defining = !opaque && depth < DEFINITION_DEPTH && place != Place::Init;
        match node.kind() {
            "class_declaration" | "object_declaration" | "companion_object" if defining => {
                let name = match node.child_by_field_name("name") {
                    Some(name) => text(name, src).to_string(),
                    None if node.kind() == "companion_object" => "Companion".into(),
                    None => continue,
                };
                inner = Rc::new(qualify(&scope, &name));
                inner_access = access.max(own_access(node, src));
                inner_place = Place::Class;
                nested += 1;
                let mut bases = bases(node, src);
                // A companion's members are called on the class's name (`Point.origin()`) and
                // by bare name inside it: the companion reads as the class's last base.
                if let Some(companion) = body(node)
                    .flat_map(named)
                    .find(|n| n.kind() == "companion_object")
                {
                    let own = companion.child_by_field_name("name");
                    let own = own.map_or("Companion", |n| text(n, src));
                    bases.push(format!("{name}.{own}"));
                }
                // A class's name called constructs it: the class joins the callables of its
                // name (`Facts::overloads`, with a factory function of that name) with its
                // primary constructor's parameters, when it declares no other constructor.
                let params = constructor(node, src);
                if params.is_some() {
                    let family = facts.overloads.entry(inner.as_ref().clone()).or_default();
                    family.push(inner.as_ref().clone());
                }
                let def = Def {
                    bases,
                    ..definition(
                        &inner,
                        Kind::Class,
                        node.byte_range(),
                        params.unwrap_or_default(),
                    )
                };
                facts.defs.push(def);
                if children(node).any(|c| c.kind() == "interface") {
                    facts.interfaces.push(inner.as_ref().clone());
                }
                // Java reaches an object through its `INSTANCE` field.
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
                let base = match receiver(node, src) {
                    Some(on) => format!("{on}.{}", text(name, src)),
                    None => text(name, src).to_string(),
                };
                let (params, types) = parameters(node, src);
                inner = Rc::new(qualify(&scope, &format!("{base}({})", types.join(","))));
                inner_access = access.max(own_access(node, src));
                inner_place = Place::Code;
                nested += 1;
                let kind = if place == Place::Class {
                    Kind::Method
                } else {
                    Kind::Function
                };
                facts
                    .defs
                    .push(definition(&inner, kind, node.byte_range(), params));
                let family = qualify(&scope, &base);
                facts
                    .overloads
                    .entry(family.clone())
                    .or_default()
                    .push(inner.as_ref().clone());
                // A top-level `fun main` (not a script's, not an extension) starts a program.
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
            "property_declaration" if matches!(place, Place::Top | Place::Class) && defining => {
                let names: Vec<Node> = (named(node))
                    .filter(|n| matches!(n.kind(), "variable_declaration"))
                    .filter_map(|n| named(n).find(|n| n.kind() == "identifier"))
                    .collect();
                let own = access.max(own_access(node, src));
                if named(node).any(|n| n.kind() == "getter")
                    && let [name] = names[..]
                {
                    // A property with a getter is a definition, its calls the getter's.
                    let base = match receiver(node, src) {
                        Some(on) => format!("{on}.{}", text(name, src)),
                        None => text(name, src).to_string(),
                    };
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
                    for name in names {
                        let name = text(name, src);
                        property(&mut facts, &scope, place, name, constant, initial, src);
                        if place == Place::Top && constant {
                            publish(&mut facts, own, name, &qualify(&scope, name));
                        }
                    }
                    // The initializer's calls belong to the declaration around it.
                    inner_place = Place::Init;
                }
            }
            // A constructor's `val` and `var` parameters are properties of the class; the others
            // are seen by its initializers.
            "class_parameter" if defining => {
                if let Some(name) = named(node).find(|n| n.kind() == "identifier") {
                    let declared = children(node).any(|c| matches!(c.kind(), "val" | "var"));
                    let name = text(name, src);
                    if declared {
                        let constant = children(node).any(|c| c.kind() == "val");
                        property(&mut facts, &scope, Place::Class, name, constant, None, src);
                    } else {
                        facts.locals.push(qualify(&scope, name));
                    }
                }
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
            // An enum entry's arguments are read in the enum's scope, its body as an object's.
            "class_body" if node.parent().is_some_and(|p| p.kind() == "enum_entry") => {
                inner_opaque = true;
            }
            "lambda_literal" | "anonymous_function" | "object_literal" => {
                inner_opaque = true;
                inner_place = Place::Code;
                // `it` is a lambda's one parameter when it declares none.
                if node.kind() == "lambda_literal"
                    && named(node).all(|n| n.kind() != "lambda_parameters")
                {
                    facts.locals.push(qualify(&scope, "it"));
                }
            }
            "call_expression" => {
                if let Some(at) = call(node, src, &scope, opaque) {
                    facts.calls.push(at);
                }
            }
            _ => {}
        }
        // Names code binds: locals, loop and lambda variables, `catch` parameters and the
        // parameters of functions that are no definitions.
        let binds = match node.kind() {
            "variable_declaration" if matches!(place, Place::Code | Place::Init) => {
                named(node).next()
            }
            "catch_block" => named(node).find(|n| n.kind() == "identifier"),
            "parameter" if !defining => named(node).next(),
            // A function or class declared where it is no definition binds its name.
            "function_declaration" | "class_declaration" | "object_declaration" if !defining => {
                node.child_by_field_name("name")
            }
            _ => None,
        };
        if let Some(name) = binds.filter(|n| n.kind() == "identifier") {
            facts.locals.push(qualify(&scope, text(name, src)));
        }
        let mut pushed: Vec<Node> = named(node).collect();
        // A property's own names are no locals of the code its initializer or getter is.
        if node.kind() == "property_declaration" && matches!(place, Place::Top | Place::Class) {
            pushed.retain(|n| {
                !matches!(
                    n.kind(),
                    "variable_declaration" | "multi_variable_declaration"
                )
            });
        }
        // `f(a) { }` is one call whose callee and arguments the inner node holds.
        if node.kind() == "call_expression"
            && let Some(first) = pushed.first().copied()
            && trailing(node, first)
        {
            pushed.splice(0..1, named(first));
        }
        for child in pushed.into_iter().rev() {
            stack.push(Visit {
                node: child,
                scope: inner.clone(),
                place: inner_place,
                access: inner_access,
                opaque: inner_opaque,
                depth: nested,
            });
        }
    }
    if script {
        facts.exports.clear();
        facts.private.clear();
    }
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
    facts.locals.sort();
    facts.locals.dedup();
    for names in facts.overloads.values_mut() {
        names.sort();
    }
    Ok(facts)
}

/// Marks each bare call (`f()`, not in a lambda) as Java's are (`Call::by_path`): the class
/// members around it, inherited and companion ones included, come before the names outside the
/// class. Unless a binding of the code around it holds what it invokes: a local, a parameter, a
/// property, a constant of its name in any scope from the call's out.
fn bare_calls(facts: &mut Facts) {
    let locals: HashSet<&str> = facts.locals.iter().map(String::as_str).collect();
    let consts: HashSet<&str> = facts.consts.iter().map(|c| c.name.as_str()).collect();
    let properties: HashSet<(&str, &str)> = (facts.receivers.iter())
        .map(|r| (r.scope.as_str(), r.name.as_str()))
        .collect();
    let params: HashMap<&str, Vec<&str>> = (facts.defs.iter())
        .map(|d| {
            (
                d.name.as_str(),
                d.params.iter().map(|p| p.name.as_str()).collect(),
            )
        })
        .collect();
    let bound = |scope: &str, name: &str| {
        let mut at = Some(scope);
        while let Some(s) = at {
            let qualified = qualify(s, name);
            if locals.contains(qualified.as_str())
                || consts.contains(qualified.as_str())
                || properties.contains(&(s, name))
                || params.get(s).is_some_and(|p| p.contains(&name))
            {
                return true;
            }
            at = (!s.is_empty()).then(|| crate::parent(s));
        }
        false
    };
    let bare: Vec<bool> = (facts.calls.iter())
        .map(|c| {
            c.decorates.is_none()
                && !c.callee.contains('.')
                && !c.callee.starts_with('(')
                && !bound(&c.scope, &c.callee)
        })
        .collect();
    for (call, bare) in facts.calls.iter_mut().zip(bare) {
        call.by_path = bare;
    }
}

fn definition(name: &str, kind: Kind, range: Range<usize>, params: Vec<Param>) -> Def {
    Def {
        name: name.to_string(),
        kind,
        range,
        params,
        bases: Vec::new(),
        decorators: Vec::new(),
        return_values: Vec::new(),
        returns: None,
        type_params: Vec::new(),
        result: None,
    }
}

/// Exports `local` under `exported` and under its fully qualified name, unless it is
/// `private`: an `internal` one only the repository sees. A declaration that is not public is
/// no export door either.
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

/// A property without a getter: a `val` is a constant (its value read when it is the one
/// declared), and a class's property hides an outer name in the class.
fn property(
    facts: &mut Facts,
    scope: &str,
    place: Place,
    name: &str,
    constant: bool,
    initial: Option<Node>,
    src: &[u8],
) {
    if constant {
        facts.consts.push(Const {
            name: qualify(scope, name),
            value: initial.map_or(Value::Unknown, |n| value(n, src, 0)),
            ty: None,
            scope: None,
        });
    }
    if place == Place::Class {
        facts.receivers.push(Receiver {
            scope: scope.to_string(),
            name: name.to_string(),
            class: Class::Unknown,
        });
    } else if !constant {
        facts.locals.push(qualify(scope, name));
    }
}

/// The declaration's own visibility, from its modifiers.
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

/// The primary constructor's parameters of a class that declares no other constructor; `None`
/// for an interface, an object, a companion or a class with secondary constructors, whose
/// argument counts are not one list.
fn constructor(node: Node, src: &[u8]) -> Option<Vec<Param>> {
    let class = node.kind() == "class_declaration"
        && children(node).all(|c| c.kind() != "interface")
        && body(node)
            .flat_map(named)
            .all(|n| n.kind() != "secondary_constructor");
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
            default: None,
            fields: Vec::new(),
        });
    Some(params.collect())
}

/// The body of a class or object declaration.
fn body(node: Node) -> impl Iterator<Item = Node> {
    named(node).filter(|n| matches!(n.kind(), "class_body" | "enum_class_body"))
}

/// The supertypes a declaration writes after `:`, in order; one whose name cannot be read
/// stays as an unread base (`(type)`), whose members are not known.
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

/// The receiver type an extension function or property writes before its name (`Route` of
/// `fun Route.orders()`), by name without type arguments: a type before the `name` field (or
/// the variable) of the declaration, not its result type after it.
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

/// A `user_type`'s dotted name, without type arguments (`Map.Entry` of `Map.Entry<K, V>`).
fn type_name(ty: Node, src: &[u8]) -> Option<String> {
    let parts: Vec<&str> = (named(ty).filter(|n| n.kind() == "identifier"))
        .map(|n| text(n, src))
        .collect();
    (!parts.is_empty()).then(|| parts.join("."))
}

/// A function's parameters and their types as written: a `vararg` one takes any number of
/// arguments (`Binds::Rest`, spelled `vararg T`), one with a default value may be left out.
fn parameters(node: Node, src: &[u8]) -> (Vec<Param>, Vec<String>) {
    let (mut params, mut types) = (Vec::new(), Vec::new());
    let Some(list) = named(node).find(|n| n.kind() == "function_value_parameters") else {
        return (params, types);
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
                params.push(Param {
                    name: name.map_or("", |n| text(n, src)).to_string(),
                    binds: if vararg { Binds::Rest } else { Binds::Any },
                    default: None,
                    fields: Vec::new(),
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
    (params, types)
}

/// A type's tokens as written, without annotations or comments, a space only between words.
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

/// Whether the call `node` passes a trailing lambda to the call `first` it is written after:
/// `f(a) { }` is one call of `f`, not a call of what `f(a)` returns.
fn trailing(node: Node, first: Node) -> bool {
    first.kind() == "call_expression"
        && named(node).skip(1).all(|n| n.kind() == "annotated_lambda")
        && named(first).all(|n| n.kind() != "annotated_lambda")
}

/// The arguments and trailing lambdas of a call, and the expression called.
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

/// The call `node` is, written in `scope`.
fn call(node: Node, src: &[u8], scope: &str, opaque: bool) -> Option<Call> {
    let (function, args) = parts(node);
    let function = function?;
    let mut callee = callee(function, src);
    // A lambda may have a receiver no syntax names, whose members a bare or `this` call
    // reaches first: such a call is read as a name match.
    if opaque {
        if let Some(rest) = callee.strip_prefix("this.") {
            callee = format!("(anonymous).{rest}");
        } else if !callee.contains('.') {
            callee = format!("(anonymous).{callee}");
        }
    }
    let mut at = Call {
        scope: scope.to_string(),
        callee,
        arity: Some(args.len()),
        constructs: false,
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
    };
    (at.args, at.kwargs) = arguments(&args, src, 0);
    if function.kind() == "navigation_expression" {
        at.receiver = named(function).next().map(|n| value(n, src, 0));
    }
    Some(at)
}

/// The arguments as written: positional ones, then named ones (`key = value`).
fn arguments(nodes: &[Node], src: &[u8], depth: usize) -> (Vec<Value>, Vec<(String, Value)>) {
    let (mut args, mut kwargs) = (Vec::new(), Vec::new());
    for &node in nodes {
        if node.kind() == "annotated_lambda" {
            args.push(Value::Unknown);
            continue;
        }
        let parts: Vec<Node> = named(node).collect();
        let named_argument = children(node).any(|c| c.kind() == "=");
        match parts[..] {
            [key, v] if named_argument && key.kind() == "identifier" => {
                kwargs.push((text(key, src).to_string(), value(v, src, depth + 1)));
            }
            [.., v] => args.push(value(v, src, depth + 1)),
            [] => args.push(Value::Unknown),
        }
    }
    (args, kwargs)
}

/// The annotations written on the declaration `node`, each a call decorating `inner`.
fn annotate(facts: &mut Facts, node: Node, src: &[u8], scope: &str, inner: &str) {
    for annotation in (named(node).filter(|n| n.kind() == "modifiers"))
        .flat_map(named)
        .filter(|n| n.kind() == "annotation")
    {
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
        let (positional, keywords) = arguments(&args, src, 0);
        facts.calls.push(Call {
            scope: scope.to_string(),
            callee: name,
            arity: None,
            constructs: false,
            range: annotation.byte_range(),
            args: positional,
            kwargs: keywords,
            decorates: Some(inner.to_string()),
            by_path: false,
            root: None,
            on: None,
            receiver: None,
            arg_types: Vec::new(),
            uses: Vec::new(),
        });
    }
}

/// The normalized callee of an expression: `a.b.c` for names and navigation, `this` and `super`
/// for the plain keywords, `(<node kind>)` for anything else.
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
            // `a.b`, `a?.b`; a callable reference (`a::b`) is no member.
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

fn value(node: Node, src: &[u8], depth: usize) -> Value {
    if depth >= VALUE_DEPTH {
        return Value::Unknown;
    }
    match node.kind() {
        // A literal without templates or escapes; the rest is read by the kinds that need it.
        "string_literal" => {
            let mut text_parts = String::new();
            for part in named(node) {
                let content = text(part, src);
                if part.kind() != "string_content" || content.contains('$') {
                    return Value::Unknown;
                }
                text_parts.push_str(content);
            }
            Value::Str(vec![Part::Text(text_parts)])
        }
        "identifier" | "navigation_expression" => Value::Ref(callee(node, src)),
        "this_expression" => Value::Ref("this".into()),
        "call_expression" => {
            let (function, args) = parts(node);
            let Some(function) = function else {
                return Value::Unknown;
            };
            Value::Call(Invocation {
                callee: callee(function, src),
                receiver: (function.kind() == "navigation_expression")
                    .then(|| named(function).next())
                    .flatten()
                    .map(|n| Box::new(value(n, src, depth + 1))),
                args: arguments(&args, src, depth + 1).0,
                range: node.byte_range(),
            })
        }
        "number_literal" => Value::Scalar(text(node, src).parse().ok()),
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
