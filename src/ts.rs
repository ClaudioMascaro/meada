//! TypeScript parsing: what one file defines and calls. It never touches the index or other
//! files; resolution lives in `index`.

use std::ops::Range;

use tree_sitter::{Language, Node, Parser};

use crate::Result;

/// A function definition. `name` is qualified by its enclosing functions: `outer.inner`.
pub struct Def {
    pub name: String,
    pub range: Range<usize>,
}

/// A call site. `scope` is the qualified name of the enclosing function, empty at the top
/// level; `callee` is the normalized call expression.
pub struct Call {
    pub scope: String,
    pub callee: String,
    pub range: Range<usize>,
}

/// The grammar for a repository-relative path, or `None` when the file is not TypeScript.
pub fn language(path: &str) -> Option<Language> {
    if path.ends_with(".tsx") {
        Some(tree_sitter_typescript::LANGUAGE_TSX.into())
    } else if path.ends_with(".ts") {
        Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
    } else {
        None
    }
}

/// Definitions and calls of one file, each in source order.
pub fn parse(language: &Language, src: &[u8]) -> Result<(Vec<Def>, Vec<Call>)> {
    let mut parser = Parser::new();
    parser.set_language(language)?;
    let tree = parser
        .parse(src, None)
        .ok_or("tree-sitter returned no tree")?;
    let (mut defs, mut calls) = (Vec::new(), Vec::new());
    // Explicit stack instead of recursion: generated code can nest deeper than the call stack.
    let mut stack = vec![(tree.root_node(), String::new())];
    while let Some((node, scope)) = stack.pop() {
        let mut inner = scope.clone();
        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    inner = qualify(&scope, text(name, src));
                    defs.push(Def {
                        name: inner.clone(),
                        range: node.byte_range(),
                    });
                }
            }
            "call_expression" => {
                if let Some(function) = node.child_by_field_name("function") {
                    calls.push(Call {
                        scope: scope.clone(),
                        callee: callee(function, src),
                        range: node.byte_range(),
                    });
                }
            }
            _ => {}
        }
        let children: Vec<Node> = node.children(&mut node.walk()).collect();
        stack.extend(children.into_iter().rev().map(|c| (c, inner.clone())));
    }
    Ok((defs, calls))
}

/// `a.b.c` for identifier and member chains; any other expression becomes `(<node kind>)`,
/// so no source text beyond names reaches the index.
fn callee(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "identifier" | "this" | "super" | "property_identifier" | "private_property_identifier" => {
            text(node, src).to_string()
        }
        "member_expression" => match (
            node.child_by_field_name("object"),
            node.child_by_field_name("property"),
        ) {
            (Some(object), Some(property)) => {
                format!("{}.{}", callee(object, src), text(property, src))
            }
            _ => format!("({})", node.kind()),
        },
        kind => format!("({kind})"),
    }
}

fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}.{name}")
    }
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}
