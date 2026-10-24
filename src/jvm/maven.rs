//! Maven's literal project coordinates and direct dependencies. Properties stay unevaluated.

use crate::lang::MAX_BYTES;
use crate::package::{Entry, Summary};
use roxmltree::{Document, Node, ParsingOptions};

// ponytail: a POM is shallow; reject hostile trees before reading their project fields.
const XML_DEPTH: usize = 64;
// ponytail: bounds the in-memory tree independently of the manifest's byte limit.
const XML_NODES: u32 = 65_536;

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
    if doc
        .descendants()
        .any(|n| n.ancestors().take(XML_DEPTH + 1).count() > XML_DEPTH)
    {
        return Summary::default();
    }
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
            let range = field(dep, "version");
            let computed = group.contains("${")
                || artifact.contains("${")
                || range.as_ref().is_some_and(|v| v.contains("${"));
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
        ..Summary::default()
    }
}
