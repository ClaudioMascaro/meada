//! The names a file mentions (`Facts::mentions`, spec #30): those it writes where they refer to
//! something, in an expression, a receiver, a path, a `use` or a macro's tokens. A name a
//! binding pattern, a parameter or a field declares is no mention, and neither is a reference
//! a local of the name hides (a `let` before it in its block, a parameter of the function or the
//! closure around it, a `match`, `if let`, `while let` or `for` pattern). The HTTP kind reads
//! them to tell a handler no code may register (`http/rust.rs`).

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;

use tree_sitter::Node;

use super::{bind, field, named, route_attribute, text};

/// How many distinct names a file may mention before `Facts::mentions` tells nothing of it.
// ponytail: a generated file with more is read as mentioning every name (none of its crate's
// handlers is then taken as unregistered); raise it if a real file reaches it.
pub(super) const MENTIONS: usize = 1 << 14;

/// The owner of what no function encloses.
const TOP: usize = usize::MAX;

/// The names `root` mentions, apart from the functions it defines and its route attributes;
/// `None` past `MENTIONS` names.
pub(super) fn mentions(root: Node, src: &[u8]) -> Option<BTreeSet<String>> {
    // Where the locals of each name are in scope, by the function that owns them (its start):
    // a function written inside another sees none of the outer one's locals.
    let mut scopes: HashMap<usize, HashMap<String, Vec<Range<usize>>>> = HashMap::new();
    let mut bind_in = |pattern: Node, scope: Range<usize>, owner: usize| {
        let mut names = Vec::new();
        bind(pattern, 0, &mut names, src);
        for name in names {
            let at = scopes.entry(owner).or_default();
            at.entry(name).or_default().push(scope.clone());
        }
    };
    // Each reference: its name, owner and start, and whether a local can hide it (a path's
    // segments and a `use` are read among modules and types).
    let mut references: Vec<(&str, usize, usize, bool)> = Vec::new();
    // Explicit stack instead of recursion: generated code can nest deeper than the call stack.
    let mut stack = vec![(root, TOP)];
    while let Some((node, owner)) = stack.pop() {
        match node.kind() {
            "attribute_item" if route_attribute(node, src).is_some() => {}
            "function_item" | "function_signature_item" => {
                let inner = node.start_byte();
                let Some(body) = field(node, "body") else {
                    continue;
                };
                let parameters = field(node, "parameters").into_iter().flat_map(named);
                for parameter in parameters.filter(|p| p.kind() == "parameter") {
                    bind_in(parameter, body.byte_range(), inner);
                }
                stack.push((body, inner));
            }
            "closure_expression" => {
                let Some(body) = field(node, "body") else {
                    continue;
                };
                for parameter in field(node, "parameters").into_iter().flat_map(named) {
                    bind_in(parameter, body.byte_range(), owner);
                }
                stack.push((body, owner));
            }
            "let_declaration" => {
                let block = node.parent().map_or(node.end_byte(), |b| b.end_byte());
                if let Some(pattern) = field(node, "pattern") {
                    bind_in(pattern, node.end_byte()..block, owner);
                }
                stack.extend(field(node, "value").map(|c| (c, owner)));
                stack.extend(field(node, "alternative").map(|c| (c, owner)));
            }
            "let_condition" => {
                if let Some(pattern) = field(node, "pattern") {
                    bind_in(pattern, node.end_byte()..guarded(node), owner);
                }
                stack.extend(field(node, "value").map(|c| (c, owner)));
            }
            "match_arm" => {
                if let Some(pattern) = field(node, "pattern") {
                    bind_in(pattern, pattern.start_byte()..node.end_byte(), owner);
                    stack.extend(field(pattern, "condition").map(|c| (c, owner)));
                }
                stack.extend(field(node, "value").map(|c| (c, owner)));
            }
            "for_expression" => {
                let body = field(node, "body");
                if let (Some(pattern), Some(body)) = (field(node, "pattern"), body) {
                    bind_in(pattern, body.byte_range(), owner);
                }
                stack.extend(field(node, "value").map(|c| (c, owner)));
                stack.extend(body.map(|c| (c, owner)));
            }
            "identifier" if !declares(node) => {
                let hideable = match node.parent().map(|p| p.kind()) {
                    Some(
                        "scoped_identifier" | "use_declaration" | "use_list" | "scoped_use_list"
                        | "use_as_clause" | "use_wildcard",
                    ) => false,
                    Some("token_tree") => node.prev_sibling().is_none_or(|p| p.kind() != "::"),
                    _ => true,
                };
                references.push((text(node, src), owner, node.start_byte(), hideable));
            }
            _ => stack.extend(named(node).map(|c| (c, owner))),
        }
    }
    for ranges in scopes.values_mut().flat_map(|s| s.values_mut()) {
        *ranges = merged(std::mem::take(ranges));
    }
    let mut out = BTreeSet::new();
    for (name, owner, at, hideable) in references {
        let ranges = scopes.get(&owner).and_then(|s| s.get(name));
        if hideable && ranges.is_some_and(|r| covers(r, at)) {
            continue;
        }
        out.insert(name.to_string());
        if out.len() > MENTIONS {
            return None;
        }
    }
    Some(out)
}

/// Whether `node` is the name an item other than a function declares (a function's is never
/// visited): a `const`, a `static`, a module, an enum variant, a crate.
fn declares(node: Node) -> bool {
    node.parent().is_some_and(|p| {
        matches!(
            p.kind(),
            "const_item" | "static_item" | "mod_item" | "enum_variant" | "extern_crate_declaration"
        ) && field(p, "name") == Some(node)
    })
}

/// Where the bindings of an `if let`, `while let` or guard's pattern end: with the block they
/// guard (the `else` sees none of them), or with the arm.
fn guarded(condition: Node) -> usize {
    let mut at = condition.parent();
    while let Some(node) = at {
        let end = match node.kind() {
            "if_expression" => field(node, "consequence"),
            "while_expression" => field(node, "body"),
            "match_arm" => Some(node),
            "let_chain" | "match_pattern" => {
                at = node.parent();
                continue;
            }
            _ => None,
        };
        return end.map_or(condition.end_byte(), |n| n.end_byte());
    }
    condition.end_byte()
}

/// `ranges` sorted and joined where they meet.
fn merged(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// Whether one of `ranges` (sorted and apart) holds `at`.
fn covers(ranges: &[Range<usize>], at: usize) -> bool {
    let after = ranges.partition_point(|r| r.start <= at);
    after > 0 && ranges[after - 1].end > at
}

#[cfg(test)]
mod tests {
    use super::super::tests::facts;
    use super::*;

    fn names(src: &str) -> Vec<String> {
        let found = facts(src).mentions.unwrap();
        found.into_iter().collect()
    }

    #[test]
    fn mentions_are_the_names_written_outside_definitions_and_route_attributes() {
        let src = "use a::used as alias;\n#[get(\"/x\")]\nfn handler() {}\nfn other() { f(local, m![inside]); }\n";
        assert_eq!(
            names(src),
            ["a", "alias", "f", "inside", "local", "m", "used"]
        );
        let many = (0..MENTIONS + 1)
            .map(|i| format!("fn f() {{ n{i}; }}\n"))
            .collect::<String>();
        assert!(facts(&many).mentions.is_none());
    }

    #[test]
    fn a_name_bound_or_hidden_by_a_local_is_no_mention() {
        let src = "struct S { h: u8 }\n\
                   fn p(h: u8) { g(h); let k = |h: u8| h; }\n\
                   fn l() { let h = 1; g(h); }\n\
                   fn m(x: Option<u8>) { match x { Some(h) => g(h), None => {} } }\n\
                   fn i(x: Option<u8>) { if let Some(h) = x { g(h) } }\n\
                   fn r(v: Vec<u8>) { for h in v { g(h); } }\n";
        assert_eq!(names(src), ["g"]);
    }

    #[test]
    fn a_reference_a_local_does_not_reach_is_a_mention() {
        // Before the `let`, in the `else`, in a function written inside, as a path segment, in
        // a `use` and after `::` in a macro's tokens.
        let cases = [
            "fn a() { g(h); let h = 1; }",
            "fn b(x: Option<u8>) { if let Some(h) = x {} else { g(h) } }",
            "fn c(h: u8) { fn inner() { g(h) } }",
            "fn d(h: u8) { g(m::h) }",
            "fn e(h: u8) { use m::h; }",
            "fn f(h: u8) { g![m::h] }",
            "fn i(h: u8) { g![h] }",
        ];
        for (n, src) in cases.into_iter().enumerate() {
            let found = names(src).contains(&"h".to_string());
            assert_eq!(found, n < 6, "{src}");
        }
    }
}
