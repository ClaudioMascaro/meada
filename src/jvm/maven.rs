//! Maven's literal project coordinates and direct dependencies. Properties stay unevaluated.

use crate::lang::MAX_BYTES;
use crate::package::{Entry, Summary};
use roxmltree::{Document, Node, ParsingOptions};

// ponytail: a POM is shallow; deeper elements make the file no POM.
const XML_DEPTH: usize = 64;
// ponytail: bounds the in-memory tree independently of the manifest's byte limit.
const XML_NODES: u32 = 65_536;
// ponytail: a POM declares two or three namespaces; each nested declaration copies the
// enclosing ones in roxmltree.
const XML_NAMESPACES: usize = 16;
// ponytail: a POM tag has a few attributes; roxmltree checks duplicates pairwise.
const XML_ATTRIBUTES: usize = 64;

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

/// Whether the POM stays within the limits roxmltree's cost depends on, read in one linear pass
/// before parsing: namespace declarations, attributes per tag and element depth. Comments,
/// CDATA, processing instructions and declarations are skipped; an unterminated one, or an
/// unterminated tag, is no POM.
fn within_limits(text: &str) -> bool {
    if text.matches("xmlns").count() > XML_NAMESPACES {
        return false;
    }
    const SKIPPED: [(&str, &str); 4] = [
        ("<!--", "-->"),
        ("<![CDATA[", "]]>"),
        ("<?", "?>"),
        ("<!", ">"),
    ];
    let mut depth = 0usize;
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        if let Some((open, close)) = SKIPPED.iter().find(|(open, _)| rest.starts_with(open)) {
            let Some(end) = rest[open.len()..].find(close) else {
                return false;
            };
            rest = &rest[open.len() + end + close.len()..];
            continue;
        }
        let tag = rest.as_bytes();
        let (mut quote, mut attributes, mut end) = (None, 0usize, None);
        for (i, &b) in tag.iter().enumerate().skip(1) {
            match (quote, b) {
                (Some(q), _) if b == q => quote = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(b),
                (None, b'=') => attributes += 1,
                (None, b'>') => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            return false;
        };
        if attributes > XML_ATTRIBUTES {
            return false;
        }
        if tag.get(1) == Some(&b'/') {
            depth = depth.saturating_sub(1);
        } else if tag[end - 1] != b'/' {
            depth += 1;
            if depth > XML_DEPTH {
                return false;
            }
        }
        rest = &rest[end + 1..];
    }
    true
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
        roots: vec!["src/main/java".into(), "src/test/java".into()],
        test_roots: vec!["src/test/java".into()],
        ..Summary::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(summary.roots, ["src/main/java", "src/test/java"]);
        assert_eq!(summary.test_roots, ["src/test/java"]);
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
