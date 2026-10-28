//! Maven's literal project coordinates and direct dependencies. A `${name}` in a dependency's
//! group or artifact reads as the literal the pom's `<properties>` or its `project.groupId` and
//! `project.artifactId` give it; the parent poms' are the seam's `inherit` (`package/maven.rs`).

use crate::lang::MAX_BYTES;
use crate::package::{Entry, Summary};
use crate::xml::{XML_NODES, within_limits};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::BTreeMap;

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    let mut children = node.children().filter(|n| {
        n.is_element()
            && n.tag_name().name() == name
            && n.tag_name().namespace() == node.tag_name().namespace()
    });
    let first = children.next()?;
    children.next().is_none().then_some(first)
}

fn field(node: Node<'_, '_>, name: &str) -> Option<String> {
    text(child(node, name)?)
}

fn text(at: Node<'_, '_>) -> Option<String> {
    (!at.children().any(|n| n.is_element()))
        .then(|| {
            at.children()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

/// The longest chain of properties naming properties a `${..}` follows.
// ponytail: real poms nest a few; this bounds a cycle (`a` is `${b}`, `b` is `${a}`).
const PROPERTY_DEPTH: usize = 16;

/// `value` with each `${name}` that `lookup` gives a literal replaced by it, through the
/// properties that name properties. A hole whose value has a hole left stays as written.
pub fn fill<'a>(value: &str, lookup: &impl Fn(&str) -> Option<&'a str>) -> String {
    fill_to(value, lookup, PROPERTY_DEPTH)
}

fn fill_to<'a>(value: &str, lookup: &impl Fn(&str) -> Option<&'a str>, depth: usize) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let Some(end) = rest[at..].find('}') else {
            break;
        };
        let hole = &rest[at..=at + end];
        let filled = (depth > 0)
            .then(|| lookup(&hole[2..hole.len() - 1]))
            .flatten()
            .map(|v| fill_to(v, lookup, depth - 1))
            .filter(|v| !v.contains("${"));
        out.push_str(filled.as_deref().unwrap_or(hole));
        rest = &rest[at + end + 1..];
    }
    out.push_str(rest);
    out
}

/// The coordinate a dependency's group and artifact name, and whether a `${..}` hole is left,
/// which makes it the pattern of the literal fragments.
pub fn named(group: &str, artifact: &str) -> Option<(String, bool)> {
    let piece = |value: &str| match value.contains("${") {
        true => super::pattern(value),
        false => super::literal(value).then(|| value.to_string()),
    };
    let name = format!("{}:{}", piece(group)?, piece(artifact)?);
    let computed = group.contains("${") || artifact.contains("${");
    (name.len() <= crate::package::MAX_NAME).then_some((name, computed))
}

/// The `<properties>` of a pom whose name is written once and holds text, as written.
fn properties(project: Node<'_, '_>) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let list = child(project, "properties").into_iter();
    for at in list.flat_map(|p| p.children()).filter(Node::is_element) {
        let name = at.tag_name().name().to_string();
        (found.entry(name))
            .and_modify(|v| *v = None)
            .or_insert(text(at));
    }
    (found.into_iter())
        .filter_map(|(name, value)| Some((name, value?)))
        .filter(|(_, value)| value.len() <= crate::package::MAX_NAME)
        .collect()
}

pub fn read(src: &[u8]) -> Summary {
    if src.len() > MAX_BYTES {
        return Summary::default();
    }
    let Ok(text) = std::str::from_utf8(src) else {
        return Summary::default();
    };
    if !within_limits(text) {
        return Summary::default();
    }
    let Ok(doc) = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: XML_NODES,
            ..ParsingOptions::default()
        },
    ) else {
        return Summary::default();
    };
    let project = doc.root_element();
    if project.tag_name().name() != "project"
        || !matches!(
            project.tag_name().namespace(),
            None | Some("http://maven.apache.org/POM/4.0.0")
        )
    {
        return Summary::default();
    }
    let inherited = |key| child(project, "parent").and_then(|p| field(p, key));
    let own_or_parent = |key| {
        let present = project.children().any(|n| {
            n.is_element()
                && n.tag_name().name() == key
                && n.tag_name().namespace() == project.tag_name().namespace()
        });
        match present {
            true => field(project, key),
            false => inherited(key),
        }
    };
    let group = own_or_parent("groupId");
    let artifact = field(project, "artifactId");
    let name = group
        .as_deref()
        .zip(artifact.as_deref())
        .and_then(|(g, a)| super::coordinate(g, a));
    let version = own_or_parent("version").filter(|v| !v.contains("${"));
    let properties = properties(project);
    let parent = child(project, "parent")
        .and_then(|p| field(p, "groupId").zip(field(p, "artifactId")))
        .and_then(|(g, a)| super::coordinate(&g, &a));
    let lookup = |name: &str| match name {
        "project.groupId" => group.as_deref(),
        "project.artifactId" => artifact.as_deref(),
        _ => properties.get(name).map(String::as_str),
    };
    let mut dependencies = Vec::new();
    if let Some(list) = child(project, "dependencies") {
        for dep in list.children().filter(|n| {
            n.is_element()
                && n.tag_name().name() == "dependency"
                && n.tag_name().namespace() == list.tag_name().namespace()
        }) {
            let Some((group, artifact)) = field(dep, "groupId").zip(field(dep, "artifactId"))
            else {
                continue;
            };
            let (group, artifact) = (fill(&group, &lookup), fill(&artifact, &lookup));
            let Some((name, computed)) = named(&group, &artifact) else {
                continue;
            };
            // A computed version keeps the coordinate literal; its range stays as written.
            let range = field(dep, "version");
            // Only a `compile` dependency, the default, passes on to the module's dependents,
            // and only when not `optional`; a value the reader cannot tell does not.
            let unset_or = |key, value: &str| {
                let written = dep.children().any(|n| {
                    n.is_element()
                        && n.tag_name().name() == key
                        && n.tag_name().namespace() == dep.tag_name().namespace()
                });
                !written || field(dep, key).is_some_and(|v| v.eq_ignore_ascii_case(value))
            };
            dependencies.push(Entry {
                field: "dependencies".into(),
                name,
                range,
                via: computed.then(|| "computed".into()),
                // Only a pom under a parent has one whose properties could fill the rest.
                template: (computed && parent.is_some()).then_some((group, artifact)),
                private: !unset_or("scope", "compile") || !unset_or("optional", "false"),
                ..Entry::default()
            });
        }
    }
    dependencies.sort_by(|a, b| (&a.field, &a.name).cmp(&(&b.field, &b.name)));
    dependencies.dedup();
    Summary {
        name,
        version,
        dependencies,
        parent,
        properties,
        roots: super::ROOTS.map(Into::into).into(),
        test_roots: super::TEST_ROOTS.map(Into::into).into(),
        ..Summary::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::{XML_ATTRIBUTES, XML_DEPTH, XML_NAMESPACES};

    fn name(src: &str) -> Option<String> {
        read(src.as_bytes()).name
    }

    fn project(attributes: &str, body: &str) -> String {
        format!(
            "<project{attributes}><groupId>example</groupId><artifactId>web</artifactId>{body}</project>"
        )
    }

    fn dependencies(src: &str) -> Vec<(String, Option<String>)> {
        (read(src.as_bytes()).dependencies.into_iter())
            .map(|e| (e.name, e.via))
            .collect()
    }

    fn dependency(group: &str, artifact: &str) -> String {
        format!(
            "<dependencies><dependency><groupId>{group}</groupId><artifactId>{artifact}</artifactId>\
             </dependency></dependencies>"
        )
    }

    #[test]
    fn a_property_the_pom_declares_as_a_literal_fills_a_group_or_an_artifact() {
        let computed = |name: &str| (name.to_string(), Some("computed".to_string()));
        let properties = "<properties><shared.group>example</shared.group><kit>k${suffix}</kit>\
            <suffix>it</suffix><deep>${deeper}</deep><deeper>${deep}</deeper>\
            <twice>a</twice><twice>b</twice><space>a b</space></properties>";
        for (group, artifact, expected) in [
            (
                "${shared.group}",
                "models",
                ("example:models".to_string(), None),
            ),
            ("example", "${kit}", ("example:kit".to_string(), None)),
            (
                "${project.groupId}",
                "${project.artifactId}",
                ("example:web".to_string(), None),
            ),
            // Any hole without a literal leaves the coordinate computed.
            ("${undeclared}", "models", computed("*:models")),
            ("example", "${deep}", computed("example:*")),
            ("example", "${twice}", computed("example:*")),
            (
                "example",
                "m${suffix}${undeclared}",
                computed("example:mit*"),
            ),
            ("example", "${project.version}", computed("example:*")),
        ] {
            let body = format!("{properties}{}", dependency(group, artifact));
            assert_eq!(
                dependencies(&project("", &body)),
                [expected],
                "{group}:{artifact}"
            );
        }
        // A value that is no name skips the entry, as a written one does.
        let body = format!("{properties}{}", dependency("example", "${space}"));
        assert!(dependencies(&project("", &body)).is_empty());
        // The parent's group fills `project.groupId` of a pom that writes none.
        let pom = format!(
            "<project><parent><groupId>example</groupId><artifactId>p</artifactId></parent>\
             <artifactId>web</artifactId>{}</project>",
            dependency("${project.groupId}", "models")
        );
        assert_eq!(dependencies(&pom), [("example:models".to_string(), None)]);
    }

    #[test]
    fn a_computed_dependency_under_a_parent_keeps_its_template_and_the_parent() {
        let pom = "<project><parent><groupId>example</groupId><artifactId>p</artifactId></parent>\
                   <artifactId>web</artifactId><dependencies><dependency>\
                   <groupId>${shared}</groupId><artifactId>m${suffix}</artifactId></dependency>\
                   </dependencies></project>";
        let summary = read(pom.as_bytes());
        assert_eq!(summary.parent.as_deref(), Some("example:p"));
        assert_eq!(
            summary.dependencies[0].template,
            Some(("${shared}".to_string(), "m${suffix}".to_string()))
        );
        // Without a parent no pom could fill it.
        let alone = dependency("${shared}", "models");
        assert_eq!(
            read(project("", &alone).as_bytes()).dependencies[0].template,
            None
        );
    }

    #[test]
    fn the_source_roots_are_main_and_test_and_the_test_one_is_marked() {
        let summary = read(project("", "").as_bytes());
        assert_eq!(
            summary.roots,
            [
                "src/main/java",
                "src/main/kotlin",
                "src/test/java",
                "src/test/kotlin"
            ]
        );
        assert_eq!(summary.test_roots, ["src/test/java", "src/test/kotlin"]);
    }

    #[test]
    fn only_a_compile_dependency_that_is_not_optional_passes_on() {
        let dep = |n: &str, more: &str| {
            format!(
                "<dependency><groupId>g</groupId><artifactId>{n}</artifactId>{more}</dependency>"
            )
        };
        let deps = [
            dep("a", ""),
            dep("b", "<scope>compile</scope><optional>false</optional>"),
            dep("c", "<scope>test</scope>"),
            dep("d", "<scope>provided</scope>"),
            dep("e", "<optional>true</optional>"),
            dep("f", "<scope>${scope}</scope>"),
            dep("g", "<scope>compile</scope><scope>test</scope>"),
        ];
        let body = format!("<dependencies>{}</dependencies>", deps.concat());
        let summary = read(project("", &body).as_bytes());
        let private: Vec<_> = (summary.dependencies.iter())
            .map(|e| (e.name.as_str(), e.private))
            .collect();
        assert_eq!(
            private,
            [
                ("g:a", false),
                ("g:b", false),
                ("g:c", true),
                ("g:d", true),
                ("g:e", true),
                ("g:f", true),
                ("g:g", true)
            ]
        );
    }

    #[test]
    fn namespace_declarations_past_the_limit_read_as_nothing() {
        let declared =
            |n: usize| -> String { (0..n).map(|i| format!(" xmlns:p{i}='u'")).collect() };
        assert_eq!(
            name(&project(&declared(XML_NAMESPACES), "")).as_deref(),
            Some("example:web")
        );
        assert_eq!(name(&project(&declared(XML_NAMESPACES + 1), "")), None);
        // Each nested element declaring its own namespace: the shape that costs roxmltree most.
        let nested: String = (0..XML_NAMESPACES + 1)
            .map(|i| format!("<a xmlns:p{i}='u'>"))
            .collect();
        assert_eq!(name(&project("", &nested)), None);
    }

    #[test]
    fn attributes_past_the_limit_on_one_tag_read_as_nothing() {
        let attributes = |n: usize| -> String { (0..n).map(|i| format!(" a{i}='x=y'")).collect() };
        let body = |n: usize| format!("<x{}/>", attributes(n));
        assert_eq!(
            name(&project("", &body(XML_ATTRIBUTES))).as_deref(),
            Some("example:web")
        );
        assert_eq!(name(&project("", &body(XML_ATTRIBUTES + 1))), None);
        assert_eq!(name(&project(&attributes(XML_ATTRIBUTES + 1), "")), None);
    }

    #[test]
    fn elements_nested_past_the_limit_read_as_nothing() {
        // The project element is one level; comments, CDATA and self-closing tags are none.
        let nested = |n: usize| {
            format!(
                "{}<!-- <a> --><![CDATA[<a>]]><b/>{}",
                "<x a='>'>".repeat(n),
                "</x>".repeat(n)
            )
        };
        assert_eq!(
            name(&project("", &nested(XML_DEPTH - 1))).as_deref(),
            Some("example:web")
        );
        assert_eq!(name(&project("", &nested(XML_DEPTH))), None);
    }

    #[test]
    fn an_unterminated_tag_reads_as_nothing() {
        let open: String = (0..10).map(|i| format!(" a{i}='1'")).collect();
        assert_eq!(name(&format!("{}<x{open}", project("", ""))), None);
        assert_eq!(name(&format!("{}<!-- open", project("", ""))), None);
    }
}
