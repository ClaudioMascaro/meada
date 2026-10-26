//! Maven's literal project coordinates and direct dependencies. Properties stay unevaluated.

use crate::lang::MAX_BYTES;
use crate::package::{Entry, Summary};
use crate::xml::{XML_NODES, within_limits};
use roxmltree::{Document, Node, ParsingOptions};

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
    let at = child(node, name)?;
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
            let piece = |value: &str| match value.contains("${") {
                true => super::pattern(value),
                false => super::literal(value).then(|| value.to_string()),
            };
            let Some((group_pattern, artifact_pattern)) = piece(&group).zip(piece(&artifact))
            else {
                continue;
            };
            let name = format!("{group_pattern}:{artifact_pattern}");
            if name.len() > crate::package::MAX_NAME {
                continue;
            }
            // A computed version keeps the coordinate literal; its range stays as written.
            let range = field(dep, "version");
            let computed = group.contains("${") || artifact.contains("${");
            dependencies.push(Entry {
                field: "dependencies".into(),
                name,
                range,
                via: computed.then(|| "computed".into()),
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
