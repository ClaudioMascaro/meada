use super::{Parents, callee, children, named, text, types};
use crate::lang::{Class, Facts, Kind, Receiver, TsClass, TsLocal, Ty};
use tree_sitter::Node;

/// Only a named type gives a receiver. The type reader rejects a shadowing type parameter.
fn annotation(node: Node, src: &[u8], up: &Parents) -> Option<Class> {
    let ty = named(node).next()?;
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
    value: Node,
    name: &str,
    src: &[u8],
    scope: &str,
    up: &Parents,
    facts: &mut Facts,
) -> Option<usize> {
    let class = match node.child_by_field_name("type") {
        Some(ty) => annotation(ty, src, up),
        None if value.kind() == "new_expression" => value
            .child_by_field_name("constructor")
            .map(|c| callee(c, src))
            .filter(|c| !c.starts_with('('))
            .map(Class::Named),
        None => None,
    };
    let index = facts.receivers.len();
    put(facts, scope, name, class?);
    Some(index)
}

fn property(param: Node) -> bool {
    children(param).any(|c| {
        matches!(
            c.kind(),
            "accessibility_modifier" | "readonly" | "override_modifier"
        )
    })
}

pub(super) fn definition(node: Node, src: &[u8], scope: &str, up: &Parents, facts: &mut Facts) {
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
            } else if member.kind() == "method_definition" && text(name, src) == "constructor" {
                info.fields.extend(
                    member
                        .child_by_field_name("parameters")
                        .into_iter()
                        .flat_map(named)
                        .filter(|p| property(*p))
                        .filter_map(|p| p.child_by_field_name("pattern"))
                        .filter(|p| p.kind() == "identifier")
                        .map(|p| text(p, src).to_string()),
                );
            }
        }
        if !info.fields.is_empty() || !info.implements.is_empty() {
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
