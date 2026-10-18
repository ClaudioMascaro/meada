//! The values of the Rust pack: what a call is given and what it is called on, what a `let` or a
//! `const` binds, and what a function returns, as far as the syntax tells. Contract kinds read
//! them through the shared value flow (`Code::origins`). A value is built once the whole file
//! is read, since a path in it needs the file's `use` declarations (`normalize`).
//!
//! A call inside a value is a stub: its callee and its span, which the shared side matches to
//! the file's calls. Its own arguments and receiver are on that call (`Call::args`,
//! `Call::receiver`), so a chain of a thousand calls costs a thousand values, not a million.

use tree_sitter::Node;

use super::{DEFINITION_DEPTH, Heads, Path, Scan, field, levels, member, normalize, segments};
use crate::lang::{Invocation, Part, Value, named, text};
use crate::qualify;

/// How many levels of wrappers (`(x)`, `&x`, `x?`, `x.await`) and arrays a value is read
/// through; deeper is `Unknown`.
// ponytail: real code wraps a value a few levels; a crafted file must not recurse without end.
pub(super) const VALUE_DEPTH: usize = 64;

/// Where a value is written: the scope, the module written in place and whether `self::` and
/// `super::` can be read there, as a `Path` records them.
pub(super) struct Where {
    pub scope: String,
    pub module: String,
    pub pure: bool,
}

pub(super) struct Reader<'a> {
    pub src: &'a [u8],
    pub scan: &'a Scan,
    pub heads: &'a mut Heads,
}

impl Reader<'_> {
    /// The value of the expression `node`.
    pub fn value(&mut self, at: &Where, node: Node) -> Value {
        self.value_at(at, node, 0)
    }

    fn value_at(&mut self, at: &Where, node: Node, depth: usize) -> Value {
        let mut node = node;
        for level in depth..VALUE_DEPTH {
            match node.kind() {
                "parenthesized_expression" | "await_expression" | "try_expression" => {
                    match named(node).next() {
                        Some(inner) => node = inner,
                        None => return Value::Unknown,
                    }
                }
                "reference_expression" | "type_cast_expression" => match field(node, "value") {
                    Some(inner) => node = inner,
                    None => return Value::Unknown,
                },
                "generic_function" => match field(node, "function") {
                    Some(inner) => node = inner,
                    None => return Value::Unknown,
                },
                "array_expression" => {
                    let items = named(node).map(|n| self.value_at(at, n, level + 1));
                    return Value::List(items.collect());
                }
                _ => return self.leaf(at, node),
            }
        }
        Value::Unknown
    }

    fn leaf(&mut self, at: &Where, node: Node) -> Value {
        let src = self.src;
        match node.kind() {
            "string_literal" | "raw_string_literal" => string(node, src),
            "identifier" | "self" => Value::Ref(text(node, src).to_string()),
            "scoped_identifier" => match self.path(at, node) {
                Some(name) => Value::Ref(name),
                None => Value::Unknown,
            },
            "field_expression" => match member(node, src) {
                name if name.starts_with('(') => Value::Unknown,
                name => Value::Ref(name),
            },
            "call_expression" => match field(node, "function") {
                Some(function) => Value::Call(Invocation {
                    callee: self.callee(at, function),
                    receiver: None,
                    args: Vec::new(),
                    range: node.byte_range(),
                }),
                None => Value::Unknown,
            },
            _ => Value::Unknown,
        }
    }

    /// The normalized name of the path `node` (`crate.a.f`).
    fn path(&mut self, at: &Where, node: Node) -> Option<String> {
        let path = Path {
            scope: at.scope.clone(),
            module: at.module.clone(),
            pure: at.pure,
            segments: segments(node, self.src)?,
            binds: None,
        };
        normalize(&path, self.scan, self.heads, false)
    }

    /// The normalized callee of a call to `function`, as `parse` spells it.
    fn callee(&mut self, at: &Where, function: Node) -> String {
        let function = match function.kind() {
            "generic_function" => field(function, "function").unwrap_or(function),
            _ => function,
        };
        match function.kind() {
            "identifier" => text(function, self.src).to_string(),
            "scoped_identifier" => self
                .path(at, function)
                .unwrap_or_else(|| "(scoped_identifier)".into()),
            "field_expression" => member(function, self.src),
            kind => format!("({kind})"),
        }
    }

    /// The arguments of the call `node` and the value it is called on. A closure written as an
    /// argument is the `Kind::Inline` definition `parse` made of it.
    pub fn call(&mut self, at: &Where, node: Node) -> (Vec<Value>, Option<Value>) {
        let function = field(node, "function").map(|f| match f.kind() {
            "generic_function" => field(f, "function").unwrap_or(f),
            _ => f,
        });
        let receiver = function
            .filter(|f| f.kind() == "field_expression")
            .and_then(|f| field(f, "value"))
            .map(|v| self.value(at, v));
        let deep = levels(&at.scope) >= DEFINITION_DEPTH;
        let args = field(node, "arguments").into_iter().flat_map(named);
        let args = args.filter(|a| !matches!(a.kind(), "line_comment" | "block_comment"));
        let args = args.map(|arg| match arg.kind() {
            "closure_expression" if !deep => Value::Function(inline(&at.scope, arg)),
            _ => self.value(at, arg),
        });
        (args.collect(), receiver)
    }

    /// What a function with this `body` returns: its tail expression and each `return`.
    pub fn returns(&mut self, at: &Where, body: Node) -> Vec<Value> {
        let mut out = Vec::new();
        let tail = named(body).filter(|n| !is_comment(*n)).last();
        out.extend(tail.map(|t| self.value(at, t)));
        let mut stack = vec![body];
        while let Some(node) = stack.pop() {
            match node.kind() {
                // Another function's `return` is not this one's, and a macro body is a token
                // tree.
                "function_item" | "closure_expression" | "macro_invocation" | "impl_item"
                | "mod_item" | "trait_item" => {}
                "return_expression" => {
                    let value = named(node).next();
                    out.push(value.map_or(Value::Undefined, |v| self.value(at, v)));
                }
                _ => stack.extend(named(node).collect::<Vec<_>>().into_iter().rev()),
            }
        }
        out
    }
}

fn is_comment(node: Node) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

/// The name of the `Kind::Inline` definition the closure `node`, written as an argument within
/// `scope`, makes.
pub(super) fn inline(scope: &str, node: Node) -> String {
    qualify(scope, &format!("@{}", node.start_byte()))
}

/// A string literal as one text part; `Unknown` for one with an escape sequence, since its
/// bytes are not what the source spells.
fn string(node: Node, src: &[u8]) -> Value {
    let mut out = String::new();
    for part in named(node) {
        match part.kind() {
            "string_content" => out.push_str(text(part, src)),
            _ => return Value::Unknown,
        }
    }
    Value::Str(vec![Part::Text(out)])
}
