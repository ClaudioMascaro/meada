//! SDK-style MSBuild project files (`.csproj`), read as XML with no MSBuild evaluation:
//! whether one is a project, and the usings it adds to every file of it, the SDK's implicit
//! usings among them.

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

/// The usings an SDK-style project file adds to each of its files, in order: the implicit
/// usings it enables, then its `<Using>` items, less those it removes. `None` when the file
/// is no SDK-style project (no `Sdk` on `Project`, in an `Sdk` element or on an `Import`), or
/// cannot be read.
// ponytail: properties are read as written, with no `Condition`, import or
// `Directory.Build.props`; the last `ImplicitUsings` written decides.
pub fn read(src: &[u8]) -> Option<Vec<Using>> {
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
    let implicit = (elements(project, "PropertyGroup").flat_map(|g| elements(g, "ImplicitUsings")))
        .last()
        .and_then(|n| n.text())
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "enable" | "true"));
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
    Some(usings)
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
            read(src.as_bytes()).unwrap(),
            [
                Using::Namespace("A.B".into()),
                Using::Alias("F".into(), "D.E".into()),
                Using::Static("G.H".into()),
            ]
        );
    }

    #[test]
    fn an_sdk_is_named_in_any_case_or_on_an_import() {
        let lower = "<Project Sdk=\"microsoft.net.sdk.WEB\"><PropertyGroup>\
                     <ImplicitUsings>true</ImplicitUsings></PropertyGroup></Project>";
        let usings = read(lower.as_bytes()).unwrap();
        assert!(usings.contains(&Using::Namespace("Microsoft.AspNetCore.Http".into())));
        let import = "<Project><Import Project=\"Sdk.props\" Sdk=\"Microsoft.NET.Sdk\" />\
                      <ItemGroup><Using Include=\"A\" /></ItemGroup></Project>";
        assert_eq!(
            read(import.as_bytes()).unwrap(),
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
