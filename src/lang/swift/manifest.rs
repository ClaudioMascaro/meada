//! `Package.swift`, read with the Swift grammar and no Swift toolchain: the targets a SwiftPM
//! package declares, each a module of the files under its directory (`targets`), and what the
//! package kind reads of the manifest (`package`): the package's name (a `*` for each computed
//! part), its library products and
//! its `.package(url:)` and `.package(path:)` dependencies.

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
            dir: dir.join("/"),
        });
    }
    found.sort_by(|a, b| (&a.dir, &a.name).cmp(&(&b.dir, &b.name)));
    found
}

/// What the package kind reads of a `Package.swift`: only literal values count.
#[derive(Debug, Default, PartialEq)]
pub struct Package {
    /// The `name:` of the `Package(...)` call, with a `*` for each part that is computed.
    pub name: Option<String>,
    /// Whether any part of `name` is computed.
    pub name_computed: bool,
    /// The library products, as `(name, targets)`: the targets are the modules `import` can name.
    /// A target name that is not a literal is left out of its product.
    pub products: Vec<(String, Vec<String>)>,
    pub dependencies: Vec<Dependency>,
}

/// A `.package(url:)` or `.package(path:)` dependency.
#[derive(Debug, PartialEq)]
pub struct Dependency {
    /// Whether it names a git `url:` rather than a local `path:`.
    pub url: bool,
    /// The URL or path as written, with a `*` for each part that is computed (an interpolation,
    /// a variable, a concatenation).
    pub text: String,
    /// Whether any part of `text` is computed.
    pub computed: bool,
    /// The version requirement as written (`from 1.2.0`, `exact 2.0.0`, `1.0.0..<2.0.0`,
    /// `branch main`), when it is a literal one.
    pub range: Option<String>,
}

/// The name, library products and package dependencies `src` declares, wherever the calls sit
/// (a variable may hold a list). A `url:` or `path:` that is no string at all reads as a single
/// computed part.
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
    // Source order, so the first `Package(name:)` is the package's.
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

/// One argument: its label, when it has one, and its value.
type Argument<'t> = (Option<&'t str>, Node<'t>);

/// A call's function name (`Package`, or the member of `.library(...)` and `Product.library(...)`)
/// and its arguments, each with its label.
fn call<'t>(node: Node<'t>, src: &'t [u8]) -> Option<(&'t str, Vec<Argument<'t>>)> {
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
fn pattern(node: Node, src: &[u8]) -> (String, bool) {
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

/// The version requirement among a `.package(...)` call's `arguments`: `from:`, `exact:`,
/// `branch:` and `revision:` labels, `.upToNextMajor(from:)`-like members and a range of two
/// string literals, written compactly.
fn requirement(arguments: &[(Option<&str>, Node)], src: &[u8]) -> Option<String> {
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
