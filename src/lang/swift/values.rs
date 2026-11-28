use tree_sitter::Node;

use super::{bound, call, callee, children, named, parts, qualify, text, unbacktick};
use crate::lang::{Call, Invocation, Part, Value};

// ponytail: real source nests a handful of calls; this keeps a crafted chain linear.
const VALUE_DEPTH: usize = 32;

#[derive(Clone, Copy)]
pub(super) struct Ctx<'s> {
    pub scope: &'s str,
    pub inline: bool,
}

pub(super) fn inline_name(scope: &str, node: Node) -> String {
    qualify(scope, &format!("@{}", node.start_byte()))
}

pub(super) fn fill(at: &mut Call, function: Node, suffixes: &[Node], src: &[u8], ctx: Ctx) {
    fill_at(at, function, suffixes, src, ctx, 0);
}

fn fill_at(at: &mut Call, function: Node, suffixes: &[Node], src: &[u8], ctx: Ctx, depth: usize) {
    for suffix in suffixes {
        for child in children(*suffix) {
            match child.kind() {
                "value_arguments" => {
                    for argument in named(child).filter(|n| n.kind() == "value_argument") {
                        let value = argument
                            .child_by_field_name("value")
                            .map_or(Value::Unknown, |v| value_at(v, src, ctx, depth + 1));
                        match argument.child_by_field_name("name") {
                            Some(label) => {
                                let label = named(label).next().unwrap_or(label);
                                at.kwargs.push((unbacktick(text(label, src)), value));
                            }
                            None => at.args.push(value),
                        }
                    }
                }
                "lambda_literal" => at.args.push(value_at(child, src, ctx, depth + 1)),
                _ => {}
            }
        }
    }
    if function.kind() == "navigation_expression"
        && let Some(target) = function.child_by_field_name("target")
    {
        at.receiver = Some(value_at(target, src, ctx, depth + 1));
    }
}

pub(super) fn value(node: Node, src: &[u8], ctx: Ctx) -> Value {
    value_at(node, src, ctx, 0)
}

fn value_at(node: Node, src: &[u8], ctx: Ctx, depth: usize) -> Value {
    if depth > VALUE_DEPTH {
        return Value::Unknown;
    }
    match node.kind() {
        "ternary_expression" => {
            let side = |field| {
                node.child_by_field_name(field)
                    .map_or(Value::Unknown, |n| value_at(n, src, ctx, depth + 1))
            };
            crate::lang::query_alternative(side("if_true"), side("if_false"))
        }
        "simple_identifier" => Value::Ref(unbacktick(text(node, src))),
        "self_expression" => Value::Ref("self".into()),
        "postfix_expression"
            if (node.child_by_field_name("operation")).is_some_and(|o| o.kind() == "bang") =>
        {
            node.child_by_field_name("target")
                .map_or(Value::Unknown, |t| value_at(t, src, ctx, depth + 1))
        }
        "navigation_expression" | "postfix_expression" => {
            let name = callee(node, src);
            match name.contains('(') {
                true => Value::Unknown,
                false => Value::Ref(name),
            }
        }
        "line_string_literal" | "multi_line_string_literal" => {
            Value::Str(string(node, src, ctx, depth))
        }
        "additive_expression" if children(node).any(|c| c.kind() == "+") => {
            let mut out: Vec<Part> = Vec::new();
            for side in ["lhs", "rhs"] {
                let part = match node.child_by_field_name(side) {
                    Some(n) => match value_at(n, src, ctx, depth + 1) {
                        Value::Str(parts) => parts,
                        Value::Ref(name) => vec![Part::Ref(name)],
                        other => vec![Part::unread(&other)],
                    },
                    None => vec![Part::Hole],
                };
                for part in part {
                    push(&mut out, part);
                }
            }
            Value::Str(out)
        }
        "array_literal" => Value::List(
            (children(node))
                .filter(|c| c.is_named())
                .map(|c| value_at(c, src, ctx, depth + 1))
                .collect(),
        ),
        "lambda_literal" => match ctx.inline {
            true => Value::Function(inline_name(ctx.scope, node)),
            false => Value::Unknown,
        },
        "try_expression" | "await_expression" => node
            .child_by_field_name("expr")
            .map_or(Value::Unknown, |e| value_at(e, src, ctx, depth + 1)),
        "prefix_expression" if text(node, src).starts_with('.') => {
            match node.child_by_field_name("target") {
                Some(t) if t.kind() == "simple_identifier" => {
                    Value::Member(String::new(), unbacktick(text(t, src)))
                }
                _ => Value::Unknown,
            }
        }
        "tuple_expression" => match &named(node).collect::<Vec<_>>()[..] {
            [only] => {
                let value = value_at(*only, src, ctx, depth + 1);
                if matches!(&value, Value::Str(parts) if matches!(parts.as_slice(), [Part::QuerySuffix]))
                {
                    value
                } else {
                    env(value)
                }
            }
            _ => Value::Unknown,
        },
        "nil_coalescing_expression" => {
            let side = |field| match node.child_by_field_name(field) {
                Some(n) => value_at(n, src, ctx, depth + 1),
                None => Value::Unknown,
            };
            match (env(side("value")), side("if_nil")) {
                (Value::Str(read), Value::Str(p))
                    if p.iter().all(|p| matches!(p, Part::Text(_))) =>
                {
                    Value::Str(read)
                }
                (Value::Str(_), _) => Value::Str(vec![Part::Hole]),
                _ => Value::Unknown,
            }
        }
        "call_expression" | "constructor_expression" => {
            if let Some(read) = subscript_read(node, src, ctx, depth) {
                return Value::Str(vec![read]);
            }
            let (function, suffixes) = parts(node);
            let Some(mut at) = call(function, &suffixes, node, src, ctx.scope) else {
                return Value::Unknown;
            };
            fill_at(&mut at, function, &suffixes, src, ctx, depth);
            let method = at.callee.rsplit('.').next().unwrap_or(&at.callee);
            if let [_] = suffixes[..]
                && STRING_METHODS.contains(&method)
                && let Some(read) = at.receiver.as_ref().and_then(read)
            {
                let args = at.args.iter().chain(at.kwargs.iter().map(|(_, v)| v));
                return match args.clone().any(may_read) {
                    true => Value::Str(vec![Part::Hole]),
                    false => Value::Str(read.to_vec()),
                };
            }
            Value::Call(Invocation {
                callee: at.callee,
                receiver: at.receiver.map(Box::new),
                args: at.args,
                kwargs: at.kwargs,
                range: node.byte_range(),
            })
        }
        _ => Value::Unknown,
    }
}

const ENVIRONMENT: &str = "ProcessInfo.processInfo.environment";

const STRING_METHODS: &[&str] = &[
    "trimmingCharacters",
    "replacingOccurrences",
    "lowercased",
    "uppercased",
];

// ponytail: `ProcessInfo` is Foundation's by its name alone; a type of the repository named so
// would be read as it.
fn subscript_read(node: Node, src: &[u8], ctx: Ctx, depth: usize) -> Option<Part> {
    let (function, suffixes) = parts(node);
    let [suffix] = suffixes[..] else {
        return None;
    };
    let arguments = children(suffix).find(|c| c.kind() == "value_arguments")?;
    if node.kind() != "call_expression"
        || function.kind() != "navigation_expression"
        || children(arguments).next()?.kind() != "["
    {
        return None;
    }
    let name = callee(function, src);
    if name.strip_prefix("Foundation.").unwrap_or(&name) != ENVIRONMENT {
        return None;
    }
    let [key] = &named(arguments).collect::<Vec<_>>()[..] else {
        return None;
    };
    let literal = match key.child_by_field_name("value") {
        Some(v) if key.child_by_field_name("name").is_none() => value_at(v, src, ctx, depth + 1),
        _ => return None,
    };
    Some(Part::env_key(&literal).unwrap_or(Part::Hole))
}

fn env(value: Value) -> Value {
    match read(&value) {
        Some(_) => value,
        None => Value::Unknown,
    }
}

pub(super) fn read(value: &Value) -> Option<&[Part]> {
    match value {
        Value::Str(p)
            if matches!(
                p[..],
                [Part::Env(_) | Part::EnvKey { .. } | Part::Hole | Part::Limit(_)]
            ) =>
        {
            Some(p)
        }
        _ => None,
    }
}

fn may_read(value: &Value) -> bool {
    match value {
        Value::Str(parts) => (parts.iter())
            .any(|p| matches!(p, Part::Env(_) | Part::Hole | Part::Limit(_) | Part::Ref(_))),
        Value::Ref(name) => !name.starts_with(char::is_uppercase),
        Value::Call(call) => (call.args.iter())
            .chain(call.kwargs.iter().map(|(_, v)| v))
            .chain(call.receiver.as_deref())
            .any(may_read),
        Value::List(items) => items.iter().any(may_read),
        _ => false,
    }
}

fn string(node: Node, src: &[u8], ctx: Ctx, depth: usize) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for child in named(node) {
        let part = match child.kind() {
            "line_str_text" | "multi_line_str_text" => Part::Text(text(child, src).to_string()),
            "interpolated_expression" => match child
                .child_by_field_name("value")
                .map(|v| value_at(v, src, ctx, depth + 1))
            {
                Some(Value::Ref(name)) => Part::Ref(name),
                Some(Value::Str(mut read)) if matches!(read[..], [Part::Env(_)]) => read.remove(0),
                other => other.as_ref().map_or(Part::Hole, Part::unread),
            },
            _ => Part::Hole,
        };
        push(&mut out, part);
    }
    out
}

fn push(out: &mut Vec<Part>, part: Part) {
    match (out.last_mut(), part) {
        (Some(Part::Text(last)), Part::Text(more)) => last.push_str(&more),
        (_, part) => out.push(part),
    }
}

pub(super) fn implicit_return(function: Node, src: &[u8], ctx: Ctx) -> Option<Value> {
    if !children(function).any(|c| c.kind() == "->") {
        return None;
    }
    let body = function.child_by_field_name("body")?;
    let statements = named(body).find(|n| n.kind() == "statements")?;
    let [only] = named(statements).collect::<Vec<_>>()[..] else {
        return None;
    };
    let kind = only.kind();
    (kind.ends_with("_expression") || kind.ends_with("_literal") || kind == "simple_identifier")
        .then(|| value(only, src, ctx))
}

pub(super) fn initializer(node: Node, src: &[u8], ctx: Ctx) -> Option<Value> {
    let initial = node.child_by_field_name("value")?;
    (bound(node, src).len() == 1).then(|| value(initial, src, ctx))
}
