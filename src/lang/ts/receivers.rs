use super::{Parents, callee, children, named, text, types};
use crate::lang::{Class, Facts, Kind, Receiver, TsClass, TsLocal, Ty};
use std::collections::HashMap;
use tree_sitter::Node;

/// Only a named type gives a receiver. The type reader rejects a shadowing type parameter.
fn annotation(node: Node, src: &[u8], up: &Parents) -> Option<Class> {
    let ty = named(node).find(|n| n.kind() != "comment")?;
    if ty.kind() != "type_identifier" {
        return None;
    }
    match types::ty(ty, src, up, &mut 0) {
        Ty::Named(name) => Some(Class::Type(name)),
        _ => None,
    }
}

fn put(facts: &mut Facts, scope: &str, name: &str, class: Class) {
    facts.receivers.push(Receiver {
        scope: scope.to_string(),
        name: name.to_string(),
        class,
    });
}

fn constructed(value: Node, src: &[u8]) -> Option<Class> {
    (value.kind() == "new_expression")
        .then(|| value.child_by_field_name("constructor"))
        .flatten()
        .map(|c| callee(c, src))
        .filter(|c| !c.starts_with('('))
        .map(Class::Named)
}

pub(super) fn record(
    locals: &mut std::collections::BTreeMap<String, Vec<TsLocal>>,
    name: String,
    block: Node,
    declared: usize,
    receiver: Option<usize>,
) {
    locals.entry(name).or_default().push(TsLocal {
        block: block.byte_range(),
        declared,
        receiver,
    });
}

pub(super) fn local(
    node: Node,
    name: &str,
    src: &[u8],
    scope: &str,
    up: &Parents,
    facts: &mut Facts,
    results: &mut HashMap<usize, usize>,
) -> Option<usize> {
    let value = node.child_by_field_name("value")?;
    let index = facts.receivers.len();
    let class = match node.child_by_field_name("type") {
        Some(ty) => annotation(ty, src, up),
        None => constructed(value, src).or_else(|| {
            let mut call = super::unwrapped(value);
            if call.kind() == "await_expression" {
                call = super::unwrapped(named(call).find(|n| n.kind() != "comment")?);
            }
            if call.kind() != "call_expression"
                || callee(super::callee_node(call)?, src).starts_with('(')
            {
                return None;
            }
            // The call's visit fills in its final index, after the declarator's visit.
            results.insert(call.id(), index);
            Some(Class::Unknown)
        }),
    };
    put(facts, scope, name, class?);
    Some(index)
}

/// A Promise's output becomes a receiver only when the call was awaited.
fn result(typed: Node, src: &[u8], up: &Parents) -> (Option<Class>, Option<Class>) {
    let inner = named(typed).find(|n| n.kind() != "comment");
    if let Some(generic) = inner.filter(|n| n.kind() == "generic_type")
        && let Some(head) = generic.child_by_field_name("name")
        && text(head, src) == "Promise"
        && !super::runtime_hidden(&up.promise_shadows, head.start_byte())
        && matches!(types::ty(head, src, up, &mut 0), Ty::Named(n) if n == "Promise")
    {
        let output = generic
            .child_by_field_name("type_arguments")
            .filter(|args| named(*args).filter(|n| n.kind() != "comment").count() == 1)
            .and_then(|args| annotation(args, src, up));
        (None, output)
    } else {
        (annotation(typed, src, up), None)
    }
}

pub(super) fn awaited(call: Node, up: &Parents) -> bool {
    let mut node = call;
    while let Some(parent) = up.of(node) {
        match parent.kind() {
            "await_expression" => return true,
            "parenthesized_expression"
            | "non_null_expression"
            | "as_expression"
            | "satisfies_expression" => node = parent,
            _ => break,
        }
    }
    super::awaited_artifact(call)
}

fn property(param: Node) -> bool {
    children(param).any(|c| {
        matches!(
            c.kind(),
            "accessibility_modifier" | "readonly" | "override_modifier"
        )
    })
}

/// A plain assignment in the constructor body can supply an untyped field's receiver.
fn constructor_receivers(
    ctor: Node,
    scope: &str,
    src: &[u8],
    declared: &std::collections::HashSet<String>,
    info: &mut TsClass,
    facts: &mut Facts,
) {
    for statement in ctor.child_by_field_name("body").into_iter().flat_map(named) {
        let Some(assign) = named(statement).find(|n| n.kind() != "comment") else {
            continue;
        };
        if statement.kind() != "expression_statement" || assign.kind() != "assignment_expression" {
            continue;
        }
        let Some(left) = assign.child_by_field_name("left") else {
            continue;
        };
        let Some(name) = left.child_by_field_name("property") else {
            continue;
        };
        let name = text(name, src);
        if !left
            .child_by_field_name("object")
            .is_some_and(|o| o.kind() == "this")
            || declared.contains(name)
            || info.constructed.contains_key(name)
        {
            continue;
        }
        if let Some(class) = assign
            .child_by_field_name("right")
            .and_then(|v| constructed(v, src))
        {
            info.constructed.insert(name.into(), assign.start_byte());
            put(facts, scope, name, class);
        }
    }
}

pub(super) fn definition(node: Node, src: &[u8], scope: &str, up: &Parents, facts: &mut Facts) {
    if let Some(ty) = node.child_by_field_name("return_type")
        && let Some(def) = facts.defs.last_mut()
    {
        (def.result, def.output) = result(ty, src, up);
    }
    if facts.defs.last().is_some_and(|d| d.kind == Kind::Class) {
        let mut info = TsClass::default();
        let heritage = named(node).find(|n| n.kind() == "class_heritage");
        for clause in heritage.into_iter().flat_map(named) {
            if clause.kind() == "extends_clause" {
                let bases = clause.child_by_field_name("value").map(|n| callee(n, src));
                facts.defs.last_mut().unwrap().bases.extend(bases);
            } else if clause.kind() == "implements_clause" {
                info.implements.extend(named(clause).filter_map(|ty| {
                    let name = ty.child_by_field_name("name").unwrap_or(ty);
                    matches!(name.kind(), "type_identifier" | "nested_type_identifier")
                        .then(|| text(name, src).to_string())
                }));
            }
        }
        let mut declared = std::collections::HashSet::new();
        let mut ctor = None;
        for member in node.child_by_field_name("body").into_iter().flat_map(named) {
            let Some(name) = member.child_by_field_name("name") else {
                continue;
            };
            if member.kind() == "public_field_definition" {
                let field = match name.kind() {
                    "computed_property_name" => "*".to_string(),
                    "string" => super::unquote(name, src),
                    _ => text(name, src).to_string(),
                };
                info.fields.push(field);
                if let Some((field, private, _)) = super::instance_field(member, src) {
                    if private {
                        info.private.push(field.clone());
                    }
                    let init = member.child_by_field_name("value");
                    let ty = member.child_by_field_name("type");
                    if let Some(value) = init {
                        info.initialized.insert(field.clone(), value.start_byte());
                    }
                    if ty.is_some() || init.is_some() {
                        declared.insert(field.clone());
                    }
                    let class = match ty {
                        Some(ty) => annotation(ty, src, up),
                        None => init.and_then(|v| constructed(v, src)),
                    };
                    if let Some(class) = class {
                        put(facts, scope, &field, class);
                    }
                }
            } else if member.kind() == "method_definition" && text(name, src) == "constructor" {
                ctor = Some(member);
                for p in member
                    .child_by_field_name("parameters")
                    .into_iter()
                    .flat_map(named)
                    .filter(|p| property(*p))
                {
                    let Some(n) = p
                        .child_by_field_name("pattern")
                        .filter(|n| n.kind() == "identifier")
                    else {
                        continue;
                    };
                    let field = text(n, src);
                    info.fields.push(field.into());
                    declared.insert(field.into());
                    info.initialized.insert(field.into(), p.start_byte());
                    if children(p)
                        .any(|c| c.kind() == "accessibility_modifier" && text(c, src) == "private")
                    {
                        info.private.push(field.into());
                    }
                    if let Some(class) = p
                        .child_by_field_name("type")
                        .and_then(|t| annotation(t, src, up))
                    {
                        put(facts, scope, field, class);
                    }
                }
            }
        }
        if let Some(ctor) = ctor {
            constructor_receivers(ctor, scope, src, &declared, &mut info, facts);
        }
        if !info.fields.is_empty() || !info.implements.is_empty() || !info.constructed.is_empty() {
            facts.ts_classes.insert(scope.to_string(), info);
        }
    }
    for param in node
        .child_by_field_name("parameters")
        .into_iter()
        .flat_map(named)
    {
        let name = param
            .child_by_field_name("pattern")
            .filter(|n| n.kind() == "identifier");
        let ty = param
            .child_by_field_name("type")
            .and_then(|t| annotation(t, src, up));
        if let (Some(name), Some(class)) = (name, ty) {
            put(facts, scope, text(name, src), class);
        }
    }
}

/// Preserve the written call id while resolving parentheses or await around a named receiver.
pub(super) fn wrapped(function: Node, src: &[u8]) -> Option<String> {
    let mut object = function.child_by_field_name("object")?;
    if object.kind() != "parenthesized_expression" {
        return None;
    }
    for _ in 0..super::FOLD_DEPTH {
        object = match object.kind() {
            "parenthesized_expression" | "await_expression" => {
                named(object).find(|n| n.kind() != "comment")?
            }
            "identifier" | "member_expression" | "this" => {
                let name = callee(object, src);
                return (!name.starts_with('(')).then_some(name);
            }
            _ => return None,
        };
    }
    None
}
