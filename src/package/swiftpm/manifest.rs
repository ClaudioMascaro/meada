use tree_sitter::{Node, Parser};

use crate::lang::named;
use crate::lang::swift::manifest::{Argument, call, literal, pattern};

#[derive(Debug, Default, PartialEq)]
pub struct Package {
    pub name: Option<String>,
    pub name_computed: bool,
    pub products: Vec<(String, Vec<String>)>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, PartialEq)]
pub struct Dependency {
    pub url: bool,
    pub text: String,
    pub computed: bool,
    pub range: Option<String>,
}

// ponytail: `.package(name:url:)`'s deprecated `name:` and a product whose `targets:` is not a
// list literal are not read; `.executable`, `.plugin` products expose no module.
pub fn package(src: &[u8]) -> Package {
    let mut parser = Parser::new();
    let parsed = (parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .ok())
    .and_then(|()| parser.parse(src, None));
    let Some(tree) = parsed else {
        return Package::default();
    };
    let mut found = Package::default();
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        let children: Vec<Node> = named(node).collect();
        pending.extend(children.into_iter().rev());
        if node.kind() != "call_expression" {
            continue;
        }
        let Some((function, arguments)) = call(node, src) else {
            continue;
        };
        let argument = |label: &str| {
            arguments
                .iter()
                .find(|(l, _)| *l == Some(label))
                .map(|(_, v)| *v)
        };
        match function {
            "Package" if found.name.is_none() => {
                if let Some((name, computed)) = argument("name").map(|v| pattern(v, src)) {
                    (found.name, found.name_computed) = (Some(name), computed);
                }
            }
            "library" => {
                let Some(name) = argument("name").and_then(|v| literal(v, src)) else {
                    continue;
                };
                let list = argument("targets").filter(|v| v.kind() == "array_literal");
                let targets = (list.into_iter().flat_map(named))
                    .filter_map(|v| literal(v, src))
                    .collect();
                found.products.push((name, targets));
            }
            "package" => {
                let (url, place) = match (argument("url"), argument("path")) {
                    (Some(url), None) => (true, url),
                    (None, Some(path)) => (false, path),
                    _ => continue,
                };
                let (text, computed) = pattern(place, src);
                found.dependencies.push(Dependency {
                    url,
                    text,
                    computed,
                    range: requirement(&arguments, src),
                });
            }
            _ => {}
        }
    }
    found
}

fn requirement(arguments: &[Argument], src: &[u8]) -> Option<String> {
    for &(label, value) in arguments {
        match (label, value.kind()) {
            (Some(l @ ("from" | "exact" | "branch" | "revision")), _) => {
                return Some(format!("{l} {}", literal(value, src)?));
            }
            (None, "call_expression") => {
                let (member, inner) = call(value, src)?;
                let (_, version) = inner.first()?;
                if !matches!(
                    member,
                    "upToNextMajor" | "upToNextMinor" | "exact" | "branch" | "revision"
                ) {
                    return None;
                }
                return Some(format!("{member} {}", literal(*version, src)?));
            }
            (None, "range_expression") => {
                let (start, end) = (
                    value.child_by_field_name("start")?,
                    value.child_by_field_name("end")?,
                );
                let operator = src.get(start.end_byte()..end.start_byte())?;
                let operator = std::str::from_utf8(operator).ok()?.trim();
                if !matches!(operator, "..<" | "...") {
                    return None;
                }
                return Some(format!(
                    "{}{operator}{}",
                    literal(start, src)?,
                    literal(end, src)?
                ));
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_gives_its_name_library_products_and_literal_dependencies() {
        let src = r#"let vendor = "kit"
        let package = Package(
            name: "Shop",
            products: [
                .library(name: "Store", type: .dynamic, targets: ["Store", "Money", computed]),
                .executable(name: "tool", targets: ["App"]),
            ],
            dependencies: [
                .package(url: "https://example.com/a/kit.git", from: "1.2.0"),
                .package(url: "https://example.com/a/exact.git", exact: "2.0.0"),
                .package(url: "https://example.com/a/up.git", .upToNextMinor(from: "1.1.0")),
                .package(url: "https://example.com/a/range.git", "1.0.0"..<"2.0.0"),
                .package(url: "https://example.com/a/main.git", branch: "main"),
                .package(path: "../local"),
                .package(url: "https://example.com/a/\(vendor).git", from: "1.0.0"),
                .package(url: base + "x.git", from: version),
            ],
            targets: []
        )"#;
        let found = package(src.as_bytes());
        assert_eq!(found.name.as_deref(), Some("Shop"));
        assert_eq!(
            found.products,
            [(
                "Store".to_string(),
                vec!["Store".to_string(), "Money".to_string()]
            )]
        );
        let rows: Vec<_> = (found.dependencies.iter())
            .map(|d| (d.url, d.text.as_str(), d.computed, d.range.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    true,
                    "https://example.com/a/kit.git",
                    false,
                    Some("from 1.2.0")
                ),
                (
                    true,
                    "https://example.com/a/exact.git",
                    false,
                    Some("exact 2.0.0")
                ),
                (
                    true,
                    "https://example.com/a/up.git",
                    false,
                    Some("upToNextMinor 1.1.0")
                ),
                (
                    true,
                    "https://example.com/a/range.git",
                    false,
                    Some("1.0.0..<2.0.0")
                ),
                (
                    true,
                    "https://example.com/a/main.git",
                    false,
                    Some("branch main")
                ),
                (false, "../local", false, None),
                (
                    true,
                    "https://example.com/a/*.git",
                    true,
                    Some("from 1.0.0")
                ),
                (true, "*", true, None),
            ]
        );
        assert_eq!(package(b"not swift {{{"), Package::default());
    }
}
