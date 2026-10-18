//! The `String` a function builds before it sends a request: `let mut url = String::from(BASE);
//! url.push_str("/users");`. A `let mut` whose name the rest of its block only reads, grows by
//! `push_str`, `push` and `+=` statements of that block, or hands on as a value, holds a known
//! text at each place it is read: what it began as, and the pushes that come before the place.
//! Any other use (a push in a branch or a loop, `&mut url`, `write!(url, ..)`, a shadowing
//! binding, a method that changes it) leaves the name unknown, as every `let mut` is.

use tree_sitter::Node;

use super::{field, mutable};
use crate::lang::{children, named, text};

/// How many syntax nodes the builders of one file may scan in all. Each `let mut` scans the rest
/// of its block, so a file of a hundred thousand of them in one function would cost the square.
// ponytail: real functions hold a few `let mut` Strings; past the budget the rest are unknown.
pub(super) const BUILDER_BUDGET: usize = 1 << 20;

/// How deep a builder is read through the builders its pushes read, and how big it grows, in
/// bytes of text plus parts (a push of the string to itself doubles it).
// ponytail: real code builds a URL from a few parts; a crafted file must not recurse or grow
// without end. Past either limit the read is a hole.
pub(super) const BUILDER_DEPTH: usize = 64;
pub(super) const BUILDER_SIZE: usize = 4096;

/// The size of a builder's `parts`, as `BUILDER_SIZE` counts it.
pub(super) fn size(parts: &[crate::lang::Part]) -> usize {
    use crate::lang::Part;
    (parts.iter())
        .map(|p| match p {
            Part::Text(t) => t.len() + 1,
            _ => 1,
        })
        .sum()
}

/// The methods that only read a `String`.
const READS: &[&str] = &[
    "as_str",
    "as_ref",
    "clone",
    "to_string",
    "to_owned",
    "len",
    "is_empty",
    "starts_with",
    "ends_with",
    "contains",
];

/// The macros that read the arguments they are given by reference.
const READING_MACROS: &[&str] = &["format", "print", "println", "eprint", "eprintln", "concat"];

/// A `let mut` String and what grows it.
pub(super) struct Builder<'t> {
    /// Where the `let` ends, and where its block does: the places it holds its text.
    pub from: usize,
    pub to: usize,
    pub init: Node<'t>,
    pub steps: Vec<Push<'t>>,
}

/// A statement of the builder's block that appends to it.
pub(super) struct Push<'t> {
    /// Where the statement ends: a read after it sees the push.
    pub end: usize,
    pub arg: Node<'t>,
    /// `push(c)` adds a char; `push_str(s)` and `+=` add a string.
    pub char: bool,
}

/// A node on the way down, with the two above it.
#[derive(Clone, Copy)]
struct Seen<'t> {
    node: Node<'t>,
    parent: Node<'t>,
    grand: Node<'t>,
    /// Whether the node is part of a pattern (a binding, not a use).
    pattern: bool,
}

/// Whether `node` is a `let mut` that may be a builder.
pub(super) fn candidate(node: Node) -> bool {
    node.kind() == "let_declaration" && mutable(node)
}

/// The builder the `let mut` `node` makes; `None` when anything else may change the name, or the
/// budget `spent` is used up.
pub(super) fn builder<'t>(node: Node<'t>, src: &[u8], spent: &mut usize) -> Option<Builder<'t>> {
    let pattern = field(node, "pattern").filter(|p| p.kind() == "identifier")?;
    let name = text(pattern, src);
    let init = field(node, "value")?;
    let block = node.parent().filter(|b| b.kind() == "block")?;
    let from = node.end_byte();
    let is_name = |n: Node| n.kind() == "identifier" && text(n, src) == name;
    let mut steps = Vec::new();
    let mut stack: Vec<Seen> = Vec::new();
    for stmt in std::iter::successors(node.next_sibling(), |s| s.next_sibling()) {
        *spent += 1;
        if *spent > BUILDER_BUDGET {
            return None;
        }
        let appended = (stmt.kind() == "expression_statement")
            .then(|| named(stmt).next())
            .flatten()
            .and_then(|e| append(e, &is_name, src));
        let (node, parent) = match appended {
            // A `#[cfg]` on the statement may leave it out.
            Some(_)
                if stmt
                    .prev_sibling()
                    .is_some_and(|p| p.kind() == "attribute_item") =>
            {
                return None;
            }
            Some((arg, char)) => {
                steps.push(Push {
                    end: stmt.end_byte(),
                    arg,
                    char,
                });
                (arg, stmt)
            }
            None => (stmt, block),
        };
        stack.push(Seen {
            node,
            parent,
            grand: block,
            pattern: false,
        });
    }
    while let Some(seen) = stack.pop() {
        *spent += 1;
        if *spent > BUILDER_BUDGET {
            return None;
        }
        let Seen {
            node,
            parent,
            pattern,
            ..
        } = seen;
        match node.kind() {
            // Another function's names are its own, and an attribute is no use.
            "function_item" | "impl_item" | "mod_item" | "trait_item" | "macro_definition"
            | "attribute_item" => continue,
            "identifier" if is_name(node) && (pattern || !reads(seen, src)) => return None,
            _ => {}
        }
        let binds = |child: Node| {
            pattern
                || node.kind().ends_with("_pattern")
                || matches!(node.kind(), "closure_parameters" | "parameter")
                || (matches!(
                    node.kind(),
                    "let_declaration" | "let_condition" | "for_expression"
                ) && field(node, "pattern").is_some_and(|p| p.id() == child.id()))
        };
        let kids = match node.kind() {
            // The separators of a macro's arguments tell a lone name from an expression.
            "token_tree" => children(node).collect::<Vec<_>>(),
            _ => named(node).collect(),
        };
        stack.extend(kids.into_iter().map(|child| Seen {
            node: child,
            parent: node,
            grand: parent,
            pattern: binds(child),
        }));
    }
    Some(Builder {
        from,
        to: block.end_byte(),
        init,
        steps,
    })
}

/// The argument and whether it is a char, of an append to the name: `name.push_str(arg)`,
/// `name.push(arg)` or `name += arg`.
fn append<'t>(
    expr: Node<'t>,
    is_name: &dyn Fn(Node) -> bool,
    src: &[u8],
) -> Option<(Node<'t>, bool)> {
    match expr.kind() {
        "compound_assignment_expr" => {
            let left = field(expr, "left").filter(|l| is_name(*l))?;
            left.next_sibling().filter(|o| o.kind() == "+=")?;
            Some((field(expr, "right")?, false))
        }
        "call_expression" => {
            let function = field(expr, "function").filter(|f| f.kind() == "field_expression")?;
            field(function, "value").filter(|v| is_name(*v))?;
            let method = text(field(function, "field")?, src);
            let args: Vec<Node> = named(field(expr, "arguments")?)
                .filter(|a| !matches!(a.kind(), "line_comment" | "block_comment"))
                .collect();
            match (method, args.as_slice()) {
                ("push_str", [arg]) => Some((*arg, false)),
                ("push", [arg]) => Some((*arg, true)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether the use of the name at `seen` only reads it.
fn reads(seen: Seen, src: &[u8]) -> bool {
    let Seen {
        node,
        parent,
        grand,
        ..
    } = seen;
    let is = |n: Option<Node>, other: Node| n.is_some_and(|n| n.id() == other.id());
    match parent.kind() {
        "assignment_expression" | "compound_assignment_expr" => !is(field(parent, "left"), node),
        "reference_expression" => !children(parent).any(|c| c.kind() == "mutable_specifier"),
        // A method call that only reads.
        "field_expression" => {
            let called = grand.kind() == "call_expression" && is(field(grand, "function"), parent);
            called && field(parent, "field").is_some_and(|f| READS.contains(&text(f, src)))
        }
        // The name alone as an argument of a macro that reads its arguments.
        "token_tree" => {
            let lone =
                |n: Option<Node>, kinds: &[&str]| n.is_some_and(|n| kinds.contains(&n.kind()));
            lone(node.prev_sibling(), &["(", ",", "&"])
                && lone(node.next_sibling(), &[")", ","])
                && grand.kind() == "macro_invocation"
                && field(grand, "macro").is_some_and(|m| READING_MACROS.contains(&text(m, src)))
        }
        _ => true,
    }
}
