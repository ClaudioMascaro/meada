//! `Package.swift`, read with the Swift grammar and no Swift toolchain: the targets a SwiftPM
//! package declares, each a module of the files under its directory.

use tree_sitter::{Node, Parser};

use super::super::{named, text};

/// A target of a package: its module name and the directory of its files, relative to the
/// repository (`Sources/Store`, `pkg/Sources/Store`).
#[derive(Debug, PartialEq)]
pub struct Target {
    pub name: String,
    pub dir: String,
}

/// The targets the manifest at `path` declares: every `.target`, `.executableTarget`,
/// `.testTarget`, `.macro` and `.plugin` call written in it, wherever it sits (a variable may
/// hold the list), with a literal `name:`. Its `path:` names its directory when it is a literal
/// inside the package, else the default one (`Sources/<name>`, `Tests/<name>` for a test target,
/// `Plugins/<name>` for a plugin).
// ponytail: `sources:` and `exclude:` are not read; every file under the directory is the
// target's.
pub fn targets(path: &str, src: &[u8]) -> Vec<Target> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(src, None) else {
        return Vec::new();
    };
    let package = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut found = Vec::new();
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        pending.extend(named(node));
        if node.kind() != "call_expression" {
            continue;
        }
        let Some(defaults) = named(node).next().and_then(|f| defaults(f, src)) else {
            continue;
        };
        let arguments = named(node)
            .filter(|n| n.kind() == "call_suffix")
            .flat_map(named)
            .filter(|n| n.kind() == "value_arguments")
            .flat_map(named);
        let (mut name, mut dir) = (None, None);
        for argument in arguments {
            let label = argument.child_by_field_name("name").map(|l| text(l, src));
            let value = argument.child_by_field_name("value");
            match label {
                Some("name") => name = value.and_then(|v| literal(v, src)),
                Some("path") => dir = Some(value.and_then(|v| literal(v, src))),
                _ => {}
            }
        }
        let Some(name) = name else { continue };
        let dir = match dir {
            None => format!("{defaults}/{name}"),
            Some(Some(dir)) if inside(&dir) => dir,
            // A computed or escaping path: the target's files are not known.
            Some(_) => continue,
        };
        let segments = package.split('/').chain(dir.split('/'));
        let dir: Vec<&str> = segments.filter(|s| !s.is_empty() && *s != ".").collect();
        found.push(Target {
            name,
            dir: dir.join("/"),
        });
    }
    found.sort_by(|a, b| (&a.dir, &a.name).cmp(&(&b.dir, &b.name)));
    found
}

/// The directory a target call's kind (`.target`, `.testTarget`) holds its files in by
/// default, when `function` is one.
fn defaults(function: Node, src: &[u8]) -> Option<&'static str> {
    let member = match function.kind() {
        // `.target(...)`, the implicit member form.
        "prefix_expression" => function.child_by_field_name("target")?,
        // `Target.target(...)`.
        "navigation_expression" => function
            .child_by_field_name("suffix")?
            .child_by_field_name("suffix")?,
        _ => return None,
    };
    match text(member, src) {
        "target" | "executableTarget" | "macro" => Some("Sources"),
        "testTarget" => Some("Tests"),
        "plugin" => Some("Plugins"),
        _ => None,
    }
}

/// A string literal with no interpolation.
fn literal(node: Node, src: &[u8]) -> Option<String> {
    if node.kind() != "line_string_literal" {
        return None;
    }
    let mut out = String::new();
    for part in named(node) {
        match part.kind() {
            "line_str_text" => out.push_str(text(part, src)),
            _ => return None,
        }
    }
    Some(out)
}

/// Whether a relative directory stays inside the package: no root, no `..`.
fn inside(dir: &str) -> bool {
    !dir.starts_with('/') && !dir.split('/').any(|s| s == "..")
}
