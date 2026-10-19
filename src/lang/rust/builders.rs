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

/// What folding the pushes of a file's builders and reading them may cost in all, in `size`
/// units: each prefix folded and each read copies its text. Past it a read is a hole.
// ponytail: real files read a few builders a few times; 2000 pushes and 100k reads of one
// builder would otherwise cost the product.
pub(super) const BUILDER_WORK: usize = 1 << 22;

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
    /// The nodes listed before and after it among its parent's children (every token, in a
    /// token tree): a sibling lookup costs the depth of the node.
    before: Option<Node<'t>>,
    after: Option<Node<'t>>,
}

/// Whether `node` is a `let mut` that may be a builder.
pub(super) fn candidate(node: Node) -> bool {
    node.kind() == "let_declaration" && mutable(node)
}

/// The builder the `let mut` `node` of `block` makes; `None` when anything else may change the
/// name, or the budget `spent` is used up. `rest` is what follows it in the block, as `parse`
/// reads the block's children once: a statement the file's configuration leaves out
/// (`#[cfg(..)]` false) is not there, and neither is an attribute.
pub(super) fn builder<'t>(
    node: Node<'t>,
    block: Node<'t>,
    rest: &[Node<'t>],
    src: &[u8],
    spent: &mut usize,
) -> Option<Builder<'t>> {
    let pattern = field(node, "pattern").filter(|p| p.kind() == "identifier")?;
    let name = text(pattern, src);
    let init = field(node, "value")?;
    let from = node.end_byte();
    let is_name = |n: Node| n.kind() == "identifier" && text(n, src) == name;
    let mut steps = Vec::new();
    let mut stack: Vec<Seen> = Vec::new();
    for &stmt in rest {
        *spent += 1;
        if *spent > BUILDER_BUDGET {
            return None;
        }
        let appended = (stmt.kind() == "expression_statement")
            .then(|| named(stmt).next())
            .flatten()
            .and_then(|e| append(e, &is_name, src));
        let (node, parent) = match appended {
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
            before: None,
            after: None,
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
        stack.extend(kids.iter().enumerate().map(|(at, &child)| Seen {
            node: child,
            parent: node,
            grand: parent,
            pattern: binds(child),
            before: at.checked_sub(1).map(|at| kids[at]),
            after: kids.get(at + 1).copied(),
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
            field(expr, "left").filter(|l| is_name(*l))?;
            field(expr, "operator").filter(|o| o.kind() == "+=")?;
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
        before,
        after,
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
            lone(before, &["(", ",", "&"])
                && lone(after, &[")", ","])
                && grand.kind() == "macro_invocation"
                && field(grand, "macro").is_some_and(|m| READING_MACROS.contains(&text(m, src)))
        }
        // Handed on, compared or held: the String is moved or copied, and not changed in place.
        // An index (`u[..].make_ascii_uppercase()`) or a deref (`*u`) reaches it for writing,
        // and any other use is not known to read.
        kind => matches!(
            kind,
            "arguments"
                | "let_declaration"
                | "return_expression"
                | "binary_expression"
                | "expression_statement"
                | "block"
                | "tuple_expression"
                | "array_expression"
                | "field_initializer"
                | "shorthand_field_initializer"
                | "parenthesized_expression"
                | "try_expression"
                | "await_expression"
                | "type_cast_expression"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use tree_sitter::{Node, Parser, Tree};

    use super::builder;
    use crate::lang::children;

    /// A `let mut` String that `n` statements push to, add to and print, `depth` blocks deep.
    fn block(n: usize, depth: usize) -> (String, Tree) {
        let body = "s.push_str(\"a\"); s += \"b\"; println!(\"{}\", s);\n".repeat(n);
        let src = format!(
            "fn f() {}{{ let mut s = String::new();\n{body}}}{}\n",
            "{ ".repeat(depth),
            " }".repeat(depth)
        );
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&src, None).unwrap();
        (src, tree)
    }

    /// How long the builder of the `let mut` in `src` takes to read its block.
    fn read(src: &str, tree: &Tree, n: usize) -> Duration {
        let at = src.find("let mut").unwrap();
        let mut node = tree.root_node().descendant_for_byte_range(at, at).unwrap();
        while node.kind() != "let_declaration" {
            node = node.parent().unwrap();
        }
        let block = node.parent().unwrap();
        let kids: Vec<Node> = children(block).collect();
        let rest = &kids[kids.iter().position(|k| k.id() == node.id()).unwrap() + 1..];
        let start = Instant::now();
        let found = builder(node, block, rest, src.as_bytes(), &mut 0);
        let took = start.elapsed();
        assert_eq!(found.map(|b| b.steps.len()), Some(2 * n));
        took
    }

    #[test]
    fn a_builder_reads_a_block_in_the_same_time_at_any_depth() {
        // A parent or sibling lookup walks down from the root of the tree (which balances a long
        // list), so it costs the depth of the node: 2000 deep, the lookups this walk made took 90
        // times as long as at the top, and one per print alone 30 times. Each statement must
        // cost the same anywhere. The least of interleaved runs keeps the machine and its load
        // out of the ratio.
        let (n, depth) = (1000, 2000);
        let (top, deep) = (block(n, 0), block(n, depth));
        let (mut at_top, mut at_depth) = (Duration::MAX, Duration::MAX);
        for _ in 0..5 {
            at_top = at_top.min(read(&top.0, &top.1, n));
            at_depth = at_depth.min(read(&deep.0, &deep.1, n));
        }
        assert!(
            at_depth < at_top * 4,
            "{at_top:?} at the top, {at_depth:?} {depth} deep"
        );
    }
}
