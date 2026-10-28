//! `Package.swift`, read with the Swift grammar and no Swift toolchain: the targets a SwiftPM
//! package declares, each a module of the files under its directory (`targets`), and the
//! grammar readers the package kind's own reader (`package::swiftpm::manifest`) builds on: a
//! call with its labelled arguments (`call`), a string with its computed parts (`pattern`) and a
//! literal one (`literal`).

use tree_sitter::{Node, Parser};

use super::super::{named, text};

/// Where a package's manifest sits, as a config file's glob, and its file name.
pub(crate) const MANIFESTS: &str = "**/Package.swift";
pub(crate) const MANIFEST: &str = MANIFESTS.split_at(3).1;

/// A target of a package: its module name, the directory of its package and the directory of
/// its files, both relative to the repository (`pkg`, `pkg/Sources/Store`).
#[derive(Debug, PartialEq)]
pub struct Target {
    pub name: String,
    pub package: String,
    pub dir: String,
}

/// The targets the manifest at `path` declares: every `.target`, `.executableTarget`,
/// `.testTarget`, `.macro` and `.plugin` call written in it, wherever it sits (a variable may
/// hold the list) but in a `dependencies:` argument (`.target(name:)` there names a
/// dependency), with a literal `name:`. Its `path:` names its directory when it is a literal
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
    // Each node with whether a `dependencies:` argument holds it.
    let mut pending = vec![(tree.root_node(), false)];
    while let Some((node, dependency)) = pending.pop() {
        let label = (node.kind() == "value_argument")
            .then(|| node.child_by_field_name("name"))
            .flatten();
        let inner = dependency || label.is_some_and(|l| text(l, src) == "dependencies");
        pending.extend(named(node).map(|n| (n, inner)));
        if node.kind() != "call_expression" || dependency {
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
            package: package.to_string(),
            dir: dir.join("/"),
        });
    }
    found.sort_by(|a, b| (&a.dir, &a.name).cmp(&(&b.dir, &b.name)));
    found
}

/// One argument: its label, when it has one, and its value.
pub(crate) type Argument<'t> = (Option<&'t str>, Node<'t>);

/// A call's function name (`Package`, or the member of `.library(...)` and `Product.library(...)`)
/// and its arguments, each with its label.
pub(crate) fn call<'t>(node: Node<'t>, src: &'t [u8]) -> Option<(&'t str, Vec<Argument<'t>>)> {
    let function = named(node).next()?;
    let name = match function.kind() {
        "simple_identifier" => function,
        "prefix_expression" => function.child_by_field_name("target")?,
        "navigation_expression" => function
            .child_by_field_name("suffix")?
            .child_by_field_name("suffix")?,
        _ => return None,
    };
    let arguments = named(node)
        .filter(|n| n.kind() == "call_suffix")
        .flat_map(named)
        .filter(|n| n.kind() == "value_arguments")
        .flat_map(named)
        .filter(|n| n.kind() == "value_argument")
        .filter_map(|a| {
            let label = a.child_by_field_name("name").map(|l| text(l, src));
            Some((label, a.child_by_field_name("value")?))
        })
        .collect();
    Some((text(name, src), arguments))
}

/// A string written with its computed parts as `*`, and whether it has any. Anything that is no
/// string literal is one computed part.
pub(crate) fn pattern(node: Node, src: &[u8]) -> (String, bool) {
    if node.kind() != "line_string_literal" {
        return ("*".to_string(), true);
    }
    let mut out = String::new();
    let mut computed = false;
    for part in named(node) {
        match part.kind() {
            "line_str_text" => out.push_str(text(part, src)),
            _ => {
                computed = true;
                out.push('*');
            }
        }
    }
    (out, computed)
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
pub(crate) fn literal(node: Node, src: &[u8]) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &str, src: &str) -> Vec<(String, String)> {
        (targets(path, src.as_bytes()).into_iter())
            .map(|t| (t.name, t.dir))
            .collect()
    }

    #[test]
    fn each_target_kind_has_its_default_directory_under_its_package() {
        let src = r#"let package = Package(name: "P", targets: [
            .target(name: "Core", dependencies: [.target(name: "Base"), "Kit"]),
            .executableTarget(name: "App"),
            .testTarget(name: "CoreTests", dependencies: ["Core"]),
            .plugin(name: "Gen", capability: .buildTool()),
            .macro(name: "Macros"),
        ])"#;
        let owned = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            (pairs.iter())
                .map(|(n, d)| (n.to_string(), d.to_string()))
                .collect()
        };
        assert_eq!(
            read("pkg/Package.swift", src),
            owned(&[
                ("Gen", "pkg/Plugins/Gen"),
                ("App", "pkg/Sources/App"),
                ("Core", "pkg/Sources/Core"),
                ("Macros", "pkg/Sources/Macros"),
                ("CoreTests", "pkg/Tests/CoreTests"),
            ])
        );
    }

    #[test]
    fn a_literal_path_inside_the_package_names_the_directory() {
        let src = r#"let targets: [Target] = [
            .target(name: "Root", path: "."),
            .target(name: "Lib", path: "./Code/Lib/"),
            .target(name: "Up", path: "../Other"),
            .target(name: "Abs", path: "/tmp/x"),
            .target(name: "Computed", path: base + "/x"),
            .target(name: base + "Name"),
        ]"#;
        let found = read("Package.swift", src);
        let owned = |n: &str, d: &str| (n.to_string(), d.to_string());
        assert_eq!(found, [owned("Root", ""), owned("Lib", "Code/Lib")]);
        assert_eq!(read("a/Package.swift", src)[0], owned("Root", "a"));
    }
}
