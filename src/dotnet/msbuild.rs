//! SDK-style MSBuild project files (`.csproj`), read as XML with no MSBuild evaluation:
//! whether one is a project, the usings it adds to every file of it (the SDK's implicit usings
//! among them), the properties that name its package and the references it declares.

use roxmltree::{Document, Node, ParsingOptions};

use crate::lang::MAX_BYTES;
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
    /// Its `PackageReference` items, as `(Include, Version)`, in order.
    pub packages: Vec<(String, Option<String>)>,
    /// The `Include` of its `ProjectReference` items, in order, as written.
    pub projects: Vec<String>,
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

/// What an SDK-style project file says: the usings it adds to each of its files, in order (the
/// implicit usings it enables, then its `<Using>` items, less those it removes), its package
/// properties and its references. `None` when the file is no SDK-style project (no `Sdk` on
/// `Project`, in an `Sdk` element or on an `Import`), or cannot be read.
// ponytail: properties are read as written, with no `Condition`, import or
// `Directory.Build.props`; the last one written decides.
pub fn read(src: &[u8]) -> Option<Project> {
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
        .or_else(|| elements(project, "Import").find_map(|n| n.attribute("Sdk")))?;
    let sdk = sdk
        .split(';')
        .next()
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let property = |name: &'static str| {
        let last = (elements(project, "PropertyGroup").flat_map(|g| elements(g, name))).last();
        let value = last.and_then(|n| n.text()).map(str::trim);
        value.filter(|v| !v.is_empty()).map(str::to_string)
    };
    let implicit = property("ImplicitUsings")
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "enable" | "true"));
    let mut usings: Vec<Using> = Vec::new();
    if implicit {
        // MSBuild reads SDK names in any case.
        let more = match sdk.as_str() {
            "microsoft.net.sdk.web" => WEB,
            "microsoft.net.sdk.worker" => WORKER,
            _ => &[],
        };
        let all = IMPLICIT.iter().chain(more);
        usings.extend(all.map(|ns| Using::Namespace(ns.to_string())));
    }
    let mut removed = Vec::new();
    for item in elements(project, "ItemGroup").flat_map(|g| elements(g, "Using")) {
        if let Some(names) = item.attribute("Remove") {
            removed.extend(names.split(';').map(str::trim).filter(|n| !n.is_empty()));
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
            usings.push(match alias {
                Some(alias) => Using::Alias(alias.to_string(), name.to_string()),
                None if is_static => Using::Static(name.to_string()),
                None => Using::Namespace(name.to_string()),
            });
        }
    }
    usings.retain(|u| match u {
        Using::Namespace(n) | Using::Static(n) | Using::Alias(_, n) => {
            !removed.contains(&n.as_str())
        }
    });
    let items = |name: &'static str| {
        elements(project, "ItemGroup")
            .flat_map(move |g| elements(g, name))
            .filter_map(|n| Some((n, n.attribute("Include")?.trim())))
            .filter(|(_, include)| !include.is_empty())
    };
    let version = |item: Node| {
        let child = elements(item, "Version").next().and_then(|v| v.text());
        (item.attribute("Version").or(child))
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    Some(Project {
        usings,
        package_id: property("PackageId"),
        assembly_name: property("AssemblyName"),
        version: property("PackageVersion").or_else(|| property("Version")),
        packages: items("PackageReference")
            .map(|(n, include)| (include.to_string(), version(n)))
            .collect(),
        projects: items("ProjectReference")
            .map(|(_, include)| include.to_string())
            .collect(),
    })
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
            read(src.as_bytes()).unwrap().usings,
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
        let project = read(src.as_bytes()).unwrap();
        assert_eq!(project.package_id.as_deref(), Some("Kit"));
        assert_eq!(project.assembly_name, None);
        assert_eq!(project.version.as_deref(), Some("2.0"));
        let packages = [("A", Some("1.*")), ("B", Some("[2,3)")), ("C", None)];
        let packages = packages.map(|(n, v)| (n.to_string(), v.map(String::from)));
        assert_eq!(project.packages, packages);
        assert_eq!(project.projects, ["..\\X\\X.csproj"]);
    }

    #[test]
    fn an_sdk_is_named_in_any_case_or_on_an_import() {
        let lower = "<Project Sdk=\"microsoft.net.sdk.WEB\"><PropertyGroup>\
                     <ImplicitUsings>true</ImplicitUsings></PropertyGroup></Project>";
        let usings = read(lower.as_bytes()).unwrap().usings;
        assert!(usings.contains(&Using::Namespace("Microsoft.AspNetCore.Http".into())));
        let import = "<Project><Import Project=\"Sdk.props\" Sdk=\"Microsoft.NET.Sdk\" />\
                      <ItemGroup><Using Include=\"A\" /></ItemGroup></Project>";
        assert_eq!(
            read(import.as_bytes()).unwrap().usings,
            [Using::Namespace("A".into())]
        );
    }

    #[test]
    fn a_project_without_an_sdk_or_past_the_limit_is_none() {
        assert!(read(b"<Project ToolsVersion=\"15.0\"></Project>").is_none());
        let deep = format!(
            "<Project Sdk=\"S\">{}{}</Project>",
            "<a>".repeat(100),
            "</a>".repeat(100)
        );
        assert!(read(deep.as_bytes()).is_none());
        let big = format!(
            "<Project Sdk=\"S\"><!--{}--></Project>",
            "x".repeat(MAX_BYTES)
        );
        assert!(read(big.as_bytes()).is_none());
    }
}
