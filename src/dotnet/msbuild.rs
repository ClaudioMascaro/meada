//! SDK-style MSBuild project files (`.csproj`), read as XML with no MSBuild evaluation:
//! whether one is a project, the usings it adds to every file of it (the SDK's implicit usings
//! among them), the properties that name its package and the references it declares, with the
//! `Directory.Build.props` files MSBuild imports into it (#381); `Directory.Build.targets` and
//! `Directory.Packages.props` are not read.

use std::collections::HashMap;

use roxmltree::{Document, Node, ParsingOptions};
use serde::{Deserialize, Serialize};

use crate::lang::MAX_BYTES;
use crate::repo::relative;
use crate::xml::{XML_NODES, within_limits};

/// A using directive: a namespace's types on demand, a type's static members and nested
/// types (`using static`), or an alias (`using X = A.B`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Using {
    Namespace(String),
    Static(String),
    Alias(String, String),
}

/// What an SDK-style project file says, as written: nothing is evaluated, so a value may hold
/// `$(Property)` text.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Project {
    /// The usings it adds to each of its files, in order.
    pub usings: Vec<Using>,
    /// The `PackageId` and `AssemblyName` properties, and the `PackageVersion` or else `Version`.
    pub package_id: Option<String>,
    pub assembly_name: Option<String>,
    pub version: Option<String>,
    /// Its `PackageReference` items, in order.
    pub packages: Vec<Reference>,
    /// Its `ProjectReference` items, in order.
    pub projects: Vec<Reference>,
}

/// A `PackageReference` or `ProjectReference` item, as written.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reference {
    pub include: String,
    pub version: Option<String>,
    /// It does not pass on to the projects that reference this one: its `PrivateAssets` names
    /// `all` or `compile`, or is computed (`$(Property)`), which the reader cannot tell.
    pub private: bool,
}

/// The namespaces implicit usings add in every SDK project (the SDK's own list).
const IMPLICIT: &[&str] = &[
    "System",
    "System.Collections.Generic",
    "System.IO",
    "System.Linq",
    "System.Net.Http",
    "System.Threading",
    "System.Threading.Tasks",
];
/// Those `Microsoft.NET.Sdk.Web` adds beside them.
const WEB: &[&str] = &[
    "System.Net.Http.Json",
    "Microsoft.AspNetCore.Builder",
    "Microsoft.AspNetCore.Hosting",
    "Microsoft.AspNetCore.Http",
    "Microsoft.AspNetCore.Routing",
    "Microsoft.Extensions.Configuration",
    "Microsoft.Extensions.DependencyInjection",
    "Microsoft.Extensions.Hosting",
    "Microsoft.Extensions.Logging",
];
/// Those `Microsoft.NET.Sdk.Worker` adds beside them.
const WORKER: &[&str] = &[
    "Microsoft.Extensions.Configuration",
    "Microsoft.Extensions.DependencyInjection",
    "Microsoft.Extensions.Hosting",
    "Microsoft.Extensions.Logging",
];

/// The name of the file MSBuild imports into every project below it.
pub const PROPS: &str = "Directory.Build.props";

/// The longest chain of props files `chain` follows; a file further up is not read.
// ponytail: real repositories nest a props file or two; this bounds the walk over a hostile one.
pub const PROPS_DEPTH: usize = 8;

/// The props file a props file imports, as a path of the repository (`""` is the root).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Import {
    /// The first props file at or above this directory (`GetPathOfFileAbove`).
    Above(String),
    /// This props file (a relative path).
    File(String),
}

/// A props file as it is read: what it adds to the projects below it, and the props file it
/// imports.
#[derive(Debug, PartialEq, Eq)]
pub struct Props {
    pub project: Project,
    pub import: Option<Import>,
}

/// One file of an evaluation, as written: a project file or a props file.
#[derive(Default)]
struct Layer {
    sdk: Option<String>,
    properties: HashMap<&'static str, String>,
    added: Vec<Using>,
    removed: Vec<String>,
    packages: Vec<Reference>,
    projects: Vec<Reference>,
    import: Option<Import>,
}

/// The properties a layer is asked for.
const PROPERTIES: [&str; 5] = [
    "ImplicitUsings",
    "PackageId",
    "AssemblyName",
    "PackageVersion",
    "Version",
];

/// What an SDK-style project file says, with the props files `props` (the bytes of the
/// `Directory.Build.props` files MSBuild imports into it, outer first, from `chain`) evaluated
/// before it: the usings it adds to each of its files, in order (the implicit usings it enables,
/// then the `<Using>` items, less those removed), its package properties and its references.
/// A props file that cannot be read adds nothing. `None` when the file is no SDK-style project
/// (no `Sdk` on `Project`, in an `Sdk` element or on an `Import`), or cannot be read.
// ponytail: properties are read as written, with no `Condition` and only a props file's parent
// imported; the last one written decides. A `ProjectReference` in a props file is relative to
// the project, as MSBuild evaluates it; one built on `$(MSBuildThisFileDirectory)` is computed.
pub fn read(src: &[u8], props: &[&[u8]]) -> Option<Project> {
    let own = layer(src, "")?;
    own.sdk.as_ref()?;
    let mut layers: Vec<Layer> = props.iter().filter_map(|src| layer(src, "")).collect();
    layers.push(own);
    Some(merge(layers))
}

/// What the props file at `path` says, as a project file does; `None` when it cannot be read.
pub fn props(path: &str, src: &[u8]) -> Option<Props> {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let layer = layer(src, dir)?;
    let import = layer.import.clone();
    Some(Props {
        project: merge(vec![layer]),
        import,
    })
}

/// The props files MSBuild imports into a project in the directory `dir` (`a/b`, empty at the
/// root), outer first: the first one at or above it, then each it imports, up to `PROPS_DEPTH`
/// files and not one twice. `props` gives the props file at a path as the file it imports, or
/// `None` when there is no such file; a file that cannot be read imports nothing.
pub fn chain(dir: &str, props: impl Fn(&str) -> Option<Option<Import>>) -> Vec<String> {
    let above = |mut dir: &str| loop {
        let path = join(dir, PROPS);
        if props(&path).is_some() {
            return Some(path);
        }
        dir = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
        if path == PROPS {
            return None;
        }
    };
    let mut found: Vec<String> = Vec::new();
    let mut next = above(dir);
    while let Some(path) = next.take().filter(|p| !found.contains(p)) {
        if found.len() == PROPS_DEPTH {
            break;
        }
        next = match props(&path).flatten() {
            Some(Import::Above(dir)) => above(&dir),
            Some(Import::File(file)) => props(&file).map(|_| file),
            None => None,
        };
        found.push(path);
    }
    found.reverse();
    found
}

fn join(dir: &str, name: &str) -> String {
    match dir {
        "" => name.to_string(),
        dir => format!("{dir}/{name}"),
    }
}

/// The props file the `Import` of `project` (its `Project` attribute) names, written in the file
/// in `dir`: `GetPathOfFileAbove` of the file's name, or a relative path to one.
fn import_of(dir: &str, project: &str) -> Option<Import> {
    const HERE: &str = "$(MSBuildThisFileDirectory)";
    let value = project.trim().replace('\\', "/");
    let inside = |path: String| (!path.starts_with("..")).then_some(path);
    if let Some((_, args)) = value.split_once("GetPathOfFileAbove(") {
        // `('Directory.Build.props', '$(MSBuildThisFileDirectory)../')`
        let mut quoted = args.split('\'').skip(1).step_by(2);
        let (name, start) = (quoted.next(), quoted.next());
        let start = start.map_or(Some(""), |start| start.strip_prefix(HERE))?;
        let above = relative(dir, start).filter(|_| name == Some(PROPS) && !start.contains("$("));
        return above.and_then(inside).map(Import::Above);
    }
    let path = value.strip_prefix(HERE).unwrap_or(&value);
    let named = path == PROPS || path.ends_with(&format!("/{PROPS}"));
    let path = relative(dir, path).filter(|_| named && !path.contains("$("));
    path.and_then(inside).map(Import::File)
}

/// One file as written. `dir` is its directory, to resolve the props file it imports.
fn layer(src: &[u8], dir: &str) -> Option<Layer> {
    if src.len() > MAX_BYTES {
        return None;
    }
    let text = std::str::from_utf8(src).ok()?;
    if !within_limits(text) {
        return None;
    }
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: XML_NODES,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(text, options).ok()?;
    let project = doc.root_element();
    if project.tag_name().name() != "Project" {
        return None;
    }
    let sdk = (project.attribute("Sdk"))
        .or_else(|| elements(project, "Sdk").find_map(|n| n.attribute("Name")))
        .or_else(|| elements(project, "Import").find_map(|n| n.attribute("Sdk")));
    let sdk = sdk.map(|sdk| {
        let name = sdk.split(';').next().unwrap_or("");
        // MSBuild reads SDK names in any case.
        name.split('/')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    });
    let mut layer = Layer {
        sdk,
        ..Layer::default()
    };
    for name in PROPERTIES {
        let last = (elements(project, "PropertyGroup").flat_map(|g| elements(g, name))).last();
        if let Some(node) = last {
            let value = node.text().map(str::trim).unwrap_or("");
            layer.properties.insert(name, value.to_string());
        }
    }
    for item in elements(project, "ItemGroup").flat_map(|g| elements(g, "Using")) {
        if let Some(names) = item.attribute("Remove") {
            let names = names.split(';').map(str::trim).filter(|n| !n.is_empty());
            layer.removed.extend(names.map(str::to_string));
        }
        let Some(names) = item.attribute("Include") else {
            continue;
        };
        let alias = item
            .attribute("Alias")
            .map(str::trim)
            .filter(|a| !a.is_empty());
        let is_static = item
            .attribute("Static")
            .is_some_and(|s| s.trim().eq_ignore_ascii_case("true"));
        for name in names.split(';').map(str::trim).filter(|n| !n.is_empty()) {
            layer.added.push(match alias {
                Some(alias) => Using::Alias(alias.to_string(), name.to_string()),
                None if is_static => Using::Static(name.to_string()),
                None => Using::Namespace(name.to_string()),
            });
        }
    }
    let items = |name: &'static str| {
        elements(project, "ItemGroup")
            .flat_map(move |g| elements(g, name))
            .filter_map(|n| Some((n, n.attribute("Include")?.trim())))
            .filter(|(_, include)| !include.is_empty())
    };
    // An attribute, else a child element of the item.
    let metadata = |item: Node<'_, '_>, name: &'static str| {
        let child = elements(item, name).next().and_then(|v| v.text());
        (item.attribute(name).or(child))
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let reference = |(item, include): (Node, &str)| Reference {
        include: include.to_string(),
        version: metadata(item, "Version"),
        private: metadata(item, "PrivateAssets").is_some_and(|assets| {
            assets.contains("$(")
                || (assets.split(';').map(str::trim))
                    .any(|a| a.eq_ignore_ascii_case("all") || a.eq_ignore_ascii_case("compile"))
        }),
    };
    layer.packages = items("PackageReference").map(reference).collect();
    layer.projects = items("ProjectReference").map(reference).collect();
    layer.import = (elements(project, "Import"))
        .filter_map(|n| n.attribute("Project"))
        .find_map(|project| import_of(dir, project));
    Some(layer)
}

/// The layers, outer first, evaluated in order: a property written by a later one wins, items
/// add up, and a `Using` removed drops those the layers before it and its own file add (and the
/// implicit ones).
fn merge(layers: Vec<Layer>) -> Project {
    let mut properties: HashMap<&str, &str> = HashMap::new();
    for layer in &layers {
        properties.extend(layer.properties.iter().map(|(k, v)| (*k, v.as_str())));
    }
    let property = |name: &str| {
        let value = properties.get(name).copied().filter(|v| !v.is_empty());
        value.map(str::to_string)
    };
    let sdk = layers.iter().rev().find_map(|l| l.sdk.as_deref());
    let implicit = property("ImplicitUsings")
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "enable" | "true"));
    let mut usings: Vec<Using> = Vec::new();
    if implicit {
        let more = match sdk.unwrap_or("") {
            "microsoft.net.sdk.web" => WEB,
            "microsoft.net.sdk.worker" => WORKER,
            _ => &[],
        };
        let all = IMPLICIT.iter().chain(more);
        usings.extend(all.map(|ns| Using::Namespace(ns.to_string())));
    }
    let mut project = Project {
        package_id: property("PackageId"),
        assembly_name: property("AssemblyName"),
        version: property("PackageVersion").or_else(|| property("Version")),
        ..Project::default()
    };
    for layer in layers {
        usings.extend(layer.added);
        usings.retain(|u| match u {
            Using::Namespace(n) | Using::Static(n) | Using::Alias(_, n) => {
                !layer.removed.contains(n)
            }
        });
        project.packages.extend(layer.packages);
        project.projects.extend(layer.projects);
    }
    project.usings = usings;
    project
}

/// The child elements of `node` named `name`, in order.
fn elements<'a, 'i>(node: Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children()
        .filter(move |n| n.is_element() && n.tag_name().name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn using_items_add_remove_alias_and_static() {
        let src = r#"<Project Sdk="Microsoft.NET.Sdk">
            <ItemGroup>
              <Using Include="A.B;C" />
              <Using Include="D.E" Alias="F" />
              <Using Include="G.H" Static="true" />
              <Using Remove="C" />
            </ItemGroup>
          </Project>"#;
        assert_eq!(
            read(src.as_bytes(), &[]).unwrap().usings,
            [
                Using::Namespace("A.B".into()),
                Using::Alias("F".into(), "D.E".into()),
                Using::Static("G.H".into()),
            ]
        );
    }

    #[test]
    fn package_properties_and_references_are_read_as_written() {
        let src = r#"<Project Sdk="Microsoft.NET.Sdk">
            <PropertyGroup>
              <PackageId>First</PackageId><PackageId> Kit </PackageId>
              <AssemblyName></AssemblyName><Version>1.0</Version><PackageVersion>2.0</PackageVersion>
            </PropertyGroup>
            <ItemGroup>
              <PackageReference Include="A" Version=" 1.* " />
              <PackageReference Include="B"><Version>[2,3)</Version></PackageReference>
              <PackageReference Include="C" />
              <PackageReference Update="D" Version="1" />
              <ProjectReference Include="..\X\X.csproj" />
            </ItemGroup>
          </Project>"#;
        let project = read(src.as_bytes(), &[]).unwrap();
        assert_eq!(project.package_id.as_deref(), Some("Kit"));
        assert_eq!(project.assembly_name, None);
        assert_eq!(project.version.as_deref(), Some("2.0"));
        let packages = [("A", Some("1.*")), ("B", Some("[2,3)")), ("C", None)];
        let packages = packages.map(|(n, v)| Reference {
            include: n.to_string(),
            version: v.map(String::from),
            private: false,
        });
        assert_eq!(project.packages, packages);
        let projects: Vec<_> = project.projects.iter().map(|r| &r.include).collect();
        assert_eq!(projects, ["..\\X\\X.csproj"]);
    }

    #[test]
    fn a_reference_with_private_compile_assets_or_computed_ones_does_not_pass_on() {
        let src = r#"<Project Sdk="S"><ItemGroup>
            <PackageReference Include="A" PrivateAssets="All" />
            <PackageReference Include="B"><PrivateAssets>contentfiles; Compile</PrivateAssets></PackageReference>
            <PackageReference Include="C" PrivateAssets="$(Assets)" />
            <PackageReference Include="D" PrivateAssets="analyzers;build" />
            <PackageReference Include="E" />
            <ProjectReference Include="X.csproj" PrivateAssets="all" />
            <ProjectReference Include="Y.csproj" PrivateAssets="none" />
          </ItemGroup></Project>"#;
        let project = read(src.as_bytes(), &[]).unwrap();
        let private = |refs: &[Reference]| -> Vec<_> {
            refs.iter()
                .map(|r| (r.include.clone(), r.private))
                .collect()
        };
        let row = |n: &str, p| (n.to_string(), p);
        assert_eq!(
            private(&project.packages),
            [
                row("A", true),
                row("B", true),
                row("C", true),
                row("D", false),
                row("E", false)
            ]
        );
        assert_eq!(
            private(&project.projects),
            [row("X.csproj", true), row("Y.csproj", false)]
        );
    }

    #[test]
    fn an_sdk_is_named_in_any_case_or_on_an_import() {
        let lower = "<Project Sdk=\"microsoft.net.sdk.WEB\"><PropertyGroup>\
                     <ImplicitUsings>true</ImplicitUsings></PropertyGroup></Project>";
        let usings = read(lower.as_bytes(), &[]).unwrap().usings;
        assert!(usings.contains(&Using::Namespace("Microsoft.AspNetCore.Http".into())));
        let import = "<Project><Import Project=\"Sdk.props\" Sdk=\"Microsoft.NET.Sdk\" />\
                      <ItemGroup><Using Include=\"A\" /></ItemGroup></Project>";
        assert_eq!(
            read(import.as_bytes(), &[]).unwrap().usings,
            [Using::Namespace("A".into())]
        );
    }

    #[test]
    fn a_project_without_an_sdk_or_past_the_limit_is_none() {
        assert!(read(b"<Project ToolsVersion=\"15.0\"></Project>", &[]).is_none());
        let deep = format!(
            "<Project Sdk=\"S\">{}{}</Project>",
            "<a>".repeat(100),
            "</a>".repeat(100)
        );
        assert!(read(deep.as_bytes(), &[]).is_none());
        let big = format!(
            "<Project Sdk=\"S\"><!--{}--></Project>",
            "x".repeat(MAX_BYTES)
        );
        assert!(read(big.as_bytes(), &[]).is_none());
    }

    fn project_under(props: &[&str], src: &str) -> Project {
        let props: Vec<&[u8]> = props.iter().map(|p| p.as_bytes()).collect();
        read(src.as_bytes(), &props).unwrap()
    }

    #[test]
    fn a_props_file_is_evaluated_before_the_project_and_the_project_wins() {
        let outer = r#"<Project>
            <PropertyGroup><ImplicitUsings>enable</ImplicitUsings><Version>1.0</Version>
              <AssemblyName>Outer</AssemblyName></PropertyGroup>
            <ItemGroup><Using Include="A" /><Using Include="B" />
              <PackageReference Include="P" /><ProjectReference Include="..\X\X.csproj" />
            </ItemGroup></Project>"#;
        let inner = r#"<Project><PropertyGroup><AssemblyName>Inner</AssemblyName></PropertyGroup>
            <ItemGroup><Using Include="C" /><Using Remove="B" /></ItemGroup></Project>"#;
        let own = r#"<Project Sdk="Microsoft.NET.Sdk">
            <PropertyGroup><Version>2.0</Version></PropertyGroup>
            <ItemGroup><Using Include="D" /><Using Remove="System.IO;C" />
              <PackageReference Include="Q" /></ItemGroup></Project>"#;
        let project = project_under(&[outer, inner], own);
        let names: Vec<_> = (project.usings.iter())
            .map(|u| match u {
                Using::Namespace(n) => n.as_str(),
                _ => "?",
            })
            .collect();
        // The SDK's implicit usings come first, less `System.IO`; `B` and `C` are removed by
        // the file after the one that adds them, `D` is the project's own.
        assert_eq!(names[..2], ["System", "System.Collections.Generic"]);
        assert!(!names.contains(&"System.IO"));
        assert_eq!(names[names.len() - 2..], ["A", "D"]);
        assert_eq!(project.assembly_name.as_deref(), Some("Inner"));
        assert_eq!(project.version.as_deref(), Some("2.0"));
        let includes =
            |refs: &[Reference]| refs.iter().map(|r| r.include.clone()).collect::<Vec<_>>();
        assert_eq!(includes(&project.packages), ["P", "Q"]);
        assert_eq!(includes(&project.projects), ["..\\X\\X.csproj"]);
        // An unreadable props file adds nothing; a project is still needed to have an SDK.
        let big = format!("<Project><!--{}--></Project>", "x".repeat(MAX_BYTES));
        assert_eq!(
            project_under(&[&big, "not xml"], own),
            project_under(&[], own)
        );
        assert!(read(outer.as_bytes(), &[outer.as_bytes()]).is_none());
    }

    fn found(files: &[(&str, &str)], dir: &str) -> Vec<String> {
        let imports: HashMap<&str, Option<Import>> = (files.iter())
            .map(|(path, src)| (*path, props(path, src.as_bytes()).and_then(|p| p.import)))
            .collect();
        chain(dir, |path| imports.get(path).cloned())
    }

    fn importing(project: &str) -> String {
        format!("<Project><Import Project=\"{project}\" /></Project>")
    }

    #[test]
    fn the_nearest_props_file_hides_the_ones_above_unless_it_imports_them() {
        let above = importing(
            "$([MSBuild]::GetPathOfFileAbove('Directory.Build.props', '$(MSBuildThisFileDirectory)../'))",
        );
        let plain = "<Project></Project>";
        let files = [
            (PROPS, plain),
            ("a/Directory.Build.props", plain),
            ("a/b/Directory.Build.props", plain),
        ];
        assert_eq!(found(&files, "a/b/c"), ["a/b/Directory.Build.props"]);
        assert_eq!(found(&files, ""), [PROPS]);
        assert_eq!(found(&files[..1], "a/b"), [PROPS]);
        assert!(found(&[("a/Directory.Build.props", plain)], "b").is_empty());
        let files = [
            (PROPS, plain),
            ("a/Directory.Build.props", above.as_str()),
            ("a/b/Directory.Build.props", above.as_str()),
        ];
        assert_eq!(
            found(&files, "a/b"),
            [
                PROPS,
                "a/Directory.Build.props",
                "a/b/Directory.Build.props"
            ]
        );
        // A relative path to the file above, with either slash and the property in front.
        let by_path = importing("$(MSBuildThisFileDirectory)..\\Directory.Build.props");
        let files = [
            (PROPS, plain),
            ("a/Directory.Build.props", by_path.as_str()),
        ];
        assert_eq!(found(&files, "a"), [PROPS, "a/Directory.Build.props"]);
        // The file imported is the one that is read, not the first above the importing one.
        let skip = importing("../../Directory.Build.props");
        let files = [
            (PROPS, plain),
            ("a/Directory.Build.props", plain),
            ("a/b/Directory.Build.props", skip.as_str()),
        ];
        assert_eq!(found(&files, "a/b"), [PROPS, "a/b/Directory.Build.props"]);
    }

    #[test]
    fn a_self_import_a_cycle_and_a_long_chain_end() {
        let own = importing("$([MSBuild]::GetPathOfFileAbove('Directory.Build.props'))");
        assert_eq!(found(&[(PROPS, own.as_str())], ""), [PROPS]);
        let named = importing("Directory.Build.props");
        assert_eq!(found(&[(PROPS, named.as_str())], ""), [PROPS]);
        let up = importing("../Directory.Build.props");
        let down = importing("a/Directory.Build.props");
        let files = [
            (PROPS, down.as_str()),
            ("a/Directory.Build.props", up.as_str()),
        ];
        assert_eq!(found(&files, "a"), [PROPS, "a/Directory.Build.props"]);
        // An import past the root, or of another file, goes nowhere.
        let outside = importing("../Directory.Build.props");
        assert_eq!(found(&[(PROPS, outside.as_str())], ""), [PROPS]);
        let other = importing("common.props");
        assert_eq!(found(&[(PROPS, other.as_str())], ""), [PROPS]);
        let above = importing(
            "$([MSBuild]::GetPathOfFileAbove('Directory.Build.props', '$(MSBuildThisFileDirectory)../'))",
        );
        let dirs: Vec<String> = (0..PROPS_DEPTH * 2)
            .map(|n| format!("{}Directory.Build.props", "d/".repeat(n)))
            .collect();
        let files: Vec<(&str, &str)> = dirs.iter().map(|d| (d.as_str(), above.as_str())).collect();
        let deepest = "d/".repeat(PROPS_DEPTH * 2 - 1);
        assert_eq!(
            found(&files, deepest.trim_end_matches('/')).len(),
            PROPS_DEPTH
        );
    }
}
