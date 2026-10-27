//! The values a Swift file writes (spec #29, #119): what the contract kinds read of a call, its
//! receiver and its arguments, and what a local holds (`Facts::bound`) or a function returns
//! (`Def::return_values`). A string literal is its parts, a name is `Value::Ref`, a call is
//! `Value::Call`, and a closure is the `Kind::Inline` definition the walk makes of it, when it
//! is written in code (`Ctx::inline`). A label before an argument makes it a keyword argument
//! (`get("users", use: list)`), the implicit member of an enum (`.GET`) is `Value::Member` with no type, and anything else is
//! `Unknown`.
//!
//! An environment read is `ProcessInfo.processInfo.environment["X"]` (also `Foundation.`-qualified),
//! the string `Str([Env(X)])`, also through `!`, parentheses, string methods that keep the text
//! (`trimmingCharacters`, `replacingOccurrences`, `lowercased`, `uppercased`) and a `??` whose
//! fallback is a plain string literal. A second read among a method's arguments or in a
//! fallback, and a fallback that is not a plain literal, make the whole a hole: which of them is
//! the base is unknown. Meada never reads the variable. Vapor's `Environment.get` is no read.

use tree_sitter::Node;

use super::{bound, call, callee, children, named, parts, qualify, text, unbacktick};
use crate::lang::{Call, Invocation, Part, Value};

/// How deep a value is read: a chain or a nest of calls past it is `Unknown`.
// ponytail: real source nests a handful of calls; this keeps a crafted chain linear.
const VALUE_DEPTH: usize = 32;

/// Where a value is written: the definition around it, and whether a closure there is a
/// definition of its own (code, not a type's property initializer).
#[derive(Clone, Copy)]
pub(super) struct Ctx<'s> {
    pub scope: &'s str,
    pub inline: bool,
}

/// The name of the `Kind::Inline` definition of the closure `node` written in `scope`.
pub(super) fn inline_name(scope: &str, node: Node) -> String {
    qualify(scope, &format!("@{}", node.start_byte()))
}

/// Gives the call `at`, made by `function` with `suffixes`, its receiver and its arguments.
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

/// The value the expression `node` writes.
pub(super) fn value(node: Node, src: &[u8], ctx: Ctx) -> Value {
    value_at(node, src, ctx, 0)
}

fn value_at(node: Node, src: &[u8], ctx: Ctx, depth: usize) -> Value {
    if depth > VALUE_DEPTH {
        return Value::Unknown;
    }
    match node.kind() {
        "simple_identifier" => Value::Ref(unbacktick(text(node, src))),
        "self_expression" => Value::Ref("self".into()),
        // `URL(string: s)!`: the force unwrap is its operand.
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
                        _ => vec![Part::Hole],
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
        // `.GET`, an implicit member.
        "prefix_expression" if text(node, src).starts_with('.') => {
            match node.child_by_field_name("target") {
                Some(t) if t.kind() == "simple_identifier" => {
                    Value::Member(String::new(), unbacktick(text(t, src)))
                }
                _ => Value::Unknown,
            }
        }
        "tuple_expression" | "nil_coalescing_expression" => {
            env_read(node, src, ctx, depth).map_or(Value::Unknown, |p| Value::Str(vec![p]))
        }
        "call_expression" | "constructor_expression" => {
            if let Some(read) = env_read(node, src, ctx, depth) {
                return Value::Str(vec![read]);
            }
            let (function, suffixes) = parts(node);
            let Some(mut at) = call(function, &suffixes, node, src, ctx.scope) else {
                return Value::Unknown;
            };
            fill_at(&mut at, function, &suffixes, src, ctx, depth);
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

/// The expression `ProcessInfo.processInfo.environment` is subscripted on.
const ENVIRONMENT: &str = "ProcessInfo.processInfo.environment";

/// The methods of `String` an environment read goes through with its text kept.
const STRING_METHODS: &[&str] = &[
    "trimmingCharacters",
    "replacingOccurrences",
    "lowercased",
    "uppercased",
];

/// The environment read the expression `node` is (see the module), `Hole` when it reads a
/// second one or falls back on something but a literal, `None` when it is no read.
// ponytail: `ProcessInfo` is Foundation's by its name alone; a type of the repository named so
// would be read as it, and an argument is searched for the name `ProcessInfo`: one that only
// looks like a read makes a hole, which is never wrong, only less.
fn env_read(node: Node, src: &[u8], ctx: Ctx, depth: usize) -> Option<Part> {
    if depth > VALUE_DEPTH {
        return None;
    }
    let again = |n: Node| env_read(n, src, ctx, depth + 1);
    match node.kind() {
        "tuple_expression" => match &named(node).collect::<Vec<_>>()[..] {
            [only] => again(*only),
            _ => None,
        },
        "postfix_expression" => again(node.child_by_field_name("target")?).filter(|_| {
            node.child_by_field_name("operation")
                .is_some_and(|o| o.kind() == "bang")
        }),
        "nil_coalescing_expression" => {
            let read = again(node.child_by_field_name("value")?)?;
            let fallback = value_at(node.child_by_field_name("if_nil")?, src, ctx, depth + 1);
            let literal =
                matches!(&fallback, Value::Str(p) if p.iter().all(|p| matches!(p, Part::Text(_))));
            Some(if literal { read } else { Part::Hole })
        }
        "call_expression" => {
            let (function, suffixes) = parts(node);
            let [suffix] = suffixes[..] else {
                return None;
            };
            let arguments = children(suffix).find(|c| c.kind() == "value_arguments")?;
            if function.kind() != "navigation_expression" {
                return None;
            }
            if children(arguments).next()?.kind() == "[" {
                let name = callee(function, src);
                if name.strip_prefix("Foundation.").unwrap_or(&name) != ENVIRONMENT {
                    return None;
                }
                let [key] = &named(arguments).collect::<Vec<_>>()[..] else {
                    return None;
                };
                let literal = match key.child_by_field_name("value") {
                    Some(v) if key.child_by_field_name("name").is_none() => {
                        value_at(v, src, ctx, depth + 1)
                    }
                    _ => return None,
                };
                return match literal {
                    Value::Str(p) => match &p[..] {
                        [Part::Text(name)] if !name.is_empty() => Some(Part::Env(name.clone())),
                        _ => Some(Part::Hole),
                    },
                    _ => Some(Part::Hole),
                };
            }
            let method = function
                .child_by_field_name("suffix")?
                .child_by_field_name("suffix")?;
            if !STRING_METHODS.contains(&text(method, src)) {
                return None;
            }
            let read = again(function.child_by_field_name("target")?)?;
            Some(match text(arguments, src).contains("ProcessInfo") {
                true => Part::Hole,
                false => read,
            })
        }
        _ => None,
    }
}

/// The parts of a string literal: its text, a name interpolated, and a hole for any other
/// expression or an escape.
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
                _ => Part::Hole,
            },
            _ => Part::Hole,
        };
        push(&mut out, part);
    }
    out
}

/// Adds `part` to `out`, joining text to the text before it.
fn push(out: &mut Vec<Part>, part: Part) {
    match (out.last_mut(), part) {
        (Some(Part::Text(last)), Part::Text(more)) => last.push_str(&more),
        (_, part) => out.push(part),
    }
}

/// The value a function with a declared result returns when its body is one expression
/// (`func admin(_ r: RoutesBuilder) -> RoutesBuilder { r.grouped("admin") }`).
pub(super) fn implicit_return(function: Node, src: &[u8], ctx: Ctx) -> Option<Value> {
    if !children(function).any(|c| c.kind() == "->") {
        return None;
    }
    let body = function.child_by_field_name("body")?;
    let statements = named(body).find(|n| n.kind() == "statements")?;
    let [only] = named(statements).collect::<Vec<_>>()[..] else {
        return None;
    };
    // A declaration or a statement is no result; a closure's `return` is read where it stands.
    let kind = only.kind();
    (kind.ends_with("_expression") || kind.ends_with("_literal") || kind == "simple_identifier")
        .then(|| value(only, src, ctx))
}

/// The value a property or a local of one name is initialized with.
pub(super) fn initializer(node: Node, src: &[u8], ctx: Ctx) -> Option<Value> {
    let initial = node.child_by_field_name("value")?;
    (bound(node, src).len() == 1).then(|| value(initial, src, ctx))
}
