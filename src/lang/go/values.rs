//! The values the Go pack reads off an expression: what a call is given, what a function returns
//! and what a `:=` holds. Only what the syntax says outright: a string literal, a name, a call, a
//! function literal. Anything else is `Unknown`, so no kind reads a guess.

use tree_sitter::Node;

use super::{DEFINITION_DEPTH, callee};
use crate::lang::{Invocation, Part, Value, named, text};
use crate::qualify;

/// How many nested expressions a value is read through (`f(g(h(..)))`); a deeper one is
/// `Unknown`.
// ponytail: real arguments nest a handful of calls; the limit keeps a crafted one cheap.
pub(super) const VALUE_DEPTH: usize = 64;

/// The value of `node`, written in `scope` at function literal nesting `depth` (a literal past
/// `DEFINITION_DEPTH` is no definition, so it is no function value).
pub(super) fn value(node: Node, src: &[u8], scope: &str, depth: usize) -> Value {
    read(node, src, scope, depth, 0)
}

fn read(node: Node, src: &[u8], scope: &str, depth: usize, level: usize) -> Value {
    if level >= VALUE_DEPTH {
        return Value::Unknown;
    }
    let again = |n: Node| read(n, src, scope, depth, level + 1);
    match node.kind() {
        "raw_string_literal" => {
            let raw = text(node, src);
            string(raw.get(1..raw.len().saturating_sub(1)))
        }
        "interpreted_string_literal" => {
            let quoted = text(node, src);
            string(
                quoted
                    .get(1..quoted.len().saturating_sub(1))
                    .filter(|s| !s.contains('\\')),
            )
        }
        "identifier" => Value::Ref(text(node, src).to_string()),
        "selector_expression" => match callee(node, src) {
            name if name.starts_with('(') => Value::Unknown,
            name => Value::Ref(name),
        },
        "parenthesized_expression" => named(node).next().map_or(Value::Unknown, again),
        "unary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|o| text(o, src) == "&") =>
        {
            node.child_by_field_name("operand")
                .map_or(Value::Unknown, again)
        }
        "func_literal" if depth < DEFINITION_DEPTH => {
            Value::Function(qualify(scope, &format!("@{}", node.start_byte())))
        }
        "call_expression" => {
            let Some(function) = node.child_by_field_name("function") else {
                return Value::Unknown;
            };
            Value::Call(invocation(node, function, src, again))
        }
        _ => Value::Unknown,
    }
}

/// The text of a string literal's content as a value, `Unknown` when it has none to read.
fn string(content: Option<&str>) -> Value {
    content.map_or(Value::Unknown, |c| {
        Value::Str(vec![Part::Text(c.to_string())])
    })
}

/// The call `node` of `function`, with the value it is made on and its arguments.
fn invocation(node: Node, function: Node, src: &[u8], read: impl Fn(Node) -> Value) -> Invocation {
    let operand = (function.kind() == "selector_expression")
        .then(|| function.child_by_field_name("operand"))
        .flatten();
    Invocation {
        callee: callee(function, src),
        receiver: operand.map(|o| Box::new(read(o))),
        args: arguments(node).map(&read).collect(),
        range: node.byte_range(),
    }
}

/// The arguments of the call `node`.
pub(super) fn arguments(node: Node) -> impl Iterator<Item = Node> {
    let list = node.child_by_field_name("arguments");
    list.into_iter().flat_map(named)
}

/// The value a call is made on, when it is a method or a function of a package
/// (`r.Group("/x")`, `http.HandleFunc(..)`).
pub(super) fn receiver(function: Node, src: &[u8], scope: &str, depth: usize) -> Option<Value> {
    if function.kind() != "selector_expression" {
        return None;
    }
    let operand = function.child_by_field_name("operand")?;
    Some(value(operand, src, scope, depth))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;

    /// How many calls deep the argument of the call `f` is read.
    fn read_depth(nesting: usize) -> usize {
        let src = format!(
            "package x\n\nfunc f() {{ {}1{} }}\n",
            "g(".repeat(nesting),
            ")".repeat(nesting)
        );
        let facts = crate::lang::go::Go.parse("x.go", src.as_bytes()).unwrap();
        let outer = facts
            .calls
            .iter()
            .find(|c| c.range.start == src.find("g(").unwrap())
            .unwrap();
        let mut value = outer.args.first();
        let mut depth = 0;
        while let Some(Value::Call(call)) = value {
            depth += 1;
            value = call.args.first();
        }
        depth
    }

    #[test]
    fn calls_nested_past_the_limit_are_unknown() {
        assert_eq!(read_depth(10), 9);
        // The outermost call is the fact; its argument is read to the limit.
        assert_eq!(read_depth(VALUE_DEPTH + 10), VALUE_DEPTH);
    }
}
