//! The bounds every XML build file reader of the repository's files shares (Maven's POM in
//! `jvm/maven.rs`, MSBuild project files in `dotnet/msbuild.rs`): a document past them is
//! unreadable, checked in one linear pass before roxmltree parses it with `XML_NODES` nodes at
//! most and no DTD.

// ponytail: a build file is shallow; deeper elements make it unreadable.
pub(crate) const XML_DEPTH: usize = 64;
// ponytail: bounds the in-memory tree independently of the manifest's byte limit.
pub(crate) const XML_NODES: u32 = 65_536;
// ponytail: a build file declares two or three namespaces; each nested declaration copies the
// enclosing ones in roxmltree.
pub(crate) const XML_NAMESPACES: usize = 16;
// ponytail: a build file's tag has a few attributes; roxmltree checks duplicates pairwise.
pub(crate) const XML_ATTRIBUTES: usize = 64;

/// Whether the document stays within the limits roxmltree's cost depends on, read in one linear
/// pass before parsing: namespace declarations, attributes per tag and element depth. Comments,
/// CDATA, processing instructions and declarations are skipped; an unterminated one, or an
/// unterminated tag, is unreadable.
pub(crate) fn within_limits(text: &str) -> bool {
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
