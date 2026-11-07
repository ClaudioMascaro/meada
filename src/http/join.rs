//! The RFC 3986 reference resolution the HTTP consumers share (section 5.2): a relative URL
//! joined to a base, over the parts of a folded string. Swift's `URL(string:relativeTo:)` and
//! TypeScript's `new URL(reference, base)` (WHATWG URL Standard, #dom-url-url, whose basic URL
//! parser agrees with RFC 3986 for the references read here) resolve through `resolve`. A
//! reference this does not read (a query or a fragment alone, one that starts with a part that is
//! not text) and a base it cannot place (no visible scheme and authority) give `None`: the URL
//! stays unknown. A parameter on either side waits for the call sites (`depends`).
//!
//! `opaque_lead` is TypeScript's alone: a base that is an environment read or a hole is no URL
//! the resolver can place, yet an absolute-path reference replaces whatever path it has and keeps
//! its origin.

use crate::lang::{Part, Value};

/// Where in the parts of a string a position is: the part and the byte in it (a text).
pub(super) type At = (usize, usize);

/// The first of `set` at or after `from` in the texts of `parts`.
pub(super) fn find(parts: &[Part], from: At, set: &[char]) -> Option<At> {
    (from.0..parts.len()).find_map(|i| match &parts[i] {
        Part::Text(t) => {
            let start = if i == from.0 { from.1 } else { 0 };
            t[start..].find(set).map(|at| (i, start + at))
        }
        _ => None,
    })
}

/// The parts from `from` up to `to`.
pub(super) fn slice(parts: &[Part], from: At, to: At) -> Vec<Part> {
    let mut out = Vec::new();
    for (i, part) in parts.iter().enumerate().take(to.0 + 1).skip(from.0) {
        match part {
            Part::Text(t) => {
                let (low, high) = (if i == from.0 { from.1 } else { 0 }, t.len());
                let high = if i == to.0 { to.1 } else { high };
                if low < high {
                    out.push(Part::Text(t[low..high].to_string()));
                }
            }
            other => out.push(other.clone()),
        }
    }
    out
}

/// The parts of a string value; a name that is a wrapper's parameter is a part to be read later.
pub(super) fn text_parts(value: &Value) -> Option<Vec<Part>> {
    match value {
        Value::Str(parts) => Some(parts.clone()),
        Value::Ref(name) => Some(vec![Part::Ref(name.clone())]),
        _ => None,
    }
}

/// Whether a name of a wrapper's parameter is among the parts.
fn holds_ref(parts: &[Part]) -> bool {
    parts.iter().any(|p| matches!(p, Part::Ref(_)))
}

/// The two strings side by side, for a URL that depends on a parameter on either side: a
/// placeholder the wrapper's call sites replace with the real resolution.
pub(super) fn depends(a: &[Part], b: &[Part]) -> Option<Vec<Part>> {
    (holds_ref(a) || holds_ref(b)).then(|| merged([a, b].concat()))
}

/// How a reference is written, RFC 3986 section 4.2.
enum Reference {
    Absolute,
    NetworkPath,
    AbsolutePath,
    RelativePath,
}

/// The kind of a reference; none for one this does not read (a query or a fragment alone, or a
/// start that is not text, whose scheme and path it could change).
fn classify(relative: &[Part]) -> Option<Reference> {
    let Some(Part::Text(lead)) = relative.first() else {
        return None;
    };
    // A scheme is letters, digits, `+`, `-` and `.` from a letter, then `:`, before any `/?#`.
    for (at, c) in lead.char_indices() {
        match c {
            ':' if at > 0 => return Some(Reference::Absolute),
            '/' | '?' | '#' | ':' => break,
            c if c.is_ascii_alphabetic()
                || (at > 0 && (c.is_ascii_digit() || "+-.".contains(c))) =>
            {
                if at + c.len_utf8() == lead.len() && relative.len() > 1 {
                    return None;
                }
            }
            _ => break,
        }
    }
    match lead {
        l if l.starts_with("//") => Some(Reference::NetworkPath),
        l if l.starts_with('/') => Some(Reference::AbsolutePath),
        l if l.starts_with(['?', '#']) || l.is_empty() => None,
        _ => Some(Reference::RelativePath),
    }
}

/// The reference `relative` resolved against `base` as RFC 3986 does, dot segments removed;
/// `None` when either does not say enough (a base with no visible scheme and authority, a
/// reference that starts with a parameter, one that is a query or a fragment alone).
pub(super) fn resolve(base: &[Part], relative: &[Part]) -> Option<Vec<Part>> {
    let kind = classify(relative)?;
    if let Reference::Absolute = kind {
        return Some(relative.to_vec());
    }
    let end = (base.len(), 0);
    // The scheme (up to its `:`), the authority and the path of the base.
    let scheme = base
        .iter()
        .position(|p| matches!(p, Part::Text(t) if t.contains("://")))?;
    let Part::Text(t) = &base[scheme] else {
        return None;
    };
    let colon = t.find("://")?;
    let path = find(base, (scheme, colon + 3), &['/', '?', '#']).unwrap_or(end);
    let stop = find(base, path, &['?', '#']).unwrap_or(end);
    if let Reference::NetworkPath = kind {
        let mut out = slice(base, (0, 0), (scheme, colon + 1));
        out.extend(relative.iter().cloned());
        return Some(merged(out));
    }
    let mut route = match kind {
        Reference::AbsolutePath => Vec::new(),
        // The base's path up to its last `/`, or the root when it has none.
        _ => {
            let slash = (path.0..=stop.0.min(base.len().saturating_sub(1)))
                .rev()
                .find_map(|i| {
                    let Part::Text(t) = &base[i] else {
                        return None;
                    };
                    let low = if i == path.0 { path.1 } else { 0 };
                    let high = if i == stop.0 { stop.1 } else { t.len() };
                    t.get(low..high)?.rfind('/').map(|at| (i, low + at + 1))
                });
            match slash {
                Some(to) => slice(base, path, to),
                None => vec![Part::Text("/".into())],
            }
        }
    };
    route.extend(relative.iter().cloned());
    let mut out = slice(base, (0, 0), path);
    out.extend(without_dots(&route)?);
    Some(merged(out))
}

/// The path `path` (which starts with `/`, and may end in a query or a fragment) without its `.`
/// and `..` segments; none when a `..` would remove a segment that holds a parameter, which may
/// be several.
fn without_dots(path: &[Part]) -> Option<Vec<Part>> {
    let end = (path.len(), 0);
    let cut = find(path, (0, 0), &['?', '#']).unwrap_or(end);
    let mut segments: Vec<Vec<Part>> = vec![Vec::new()];
    for part in slice(path, (0, 0), cut) {
        match part {
            Part::Text(t) => {
                for (k, piece) in t.split('/').enumerate() {
                    if k > 0 {
                        segments.push(Vec::new());
                    }
                    if !piece.is_empty() {
                        segments.last_mut()?.push(Part::Text(piece.into()));
                    }
                }
            }
            other => segments.last_mut()?.push(other),
        }
    }
    let last = segments.len() - 1;
    let mut kept: Vec<Vec<Part>> = vec![Vec::new()];
    for (k, segment) in segments.into_iter().enumerate().skip(1) {
        let dot = matches!(&segment[..], [Part::Text(t)] if t == ".");
        let up = matches!(&segment[..], [Part::Text(t)] if t == "..");
        if up && kept.len() > 1 && kept.pop()?.iter().any(|p| !matches!(p, Part::Text(_))) {
            return None;
        }
        match dot || up {
            // A trailing dot segment leaves the slash.
            true if k == last => kept.push(Vec::new()),
            true => {}
            false => kept.push(segment),
        }
    }
    let mut out = Vec::new();
    for segment in kept.into_iter().skip(1) {
        out.push(Part::Text("/".into()));
        out.extend(segment);
    }
    out.extend(slice(path, cut, end));
    Some(merged(out))
}

/// `parts` with the neighbouring texts joined.
pub(super) fn merged(parts: Vec<Part>) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for part in parts {
        match (out.last_mut(), part) {
            (Some(Part::Text(last)), Part::Text(more)) => last.push_str(&more),
            (_, part) => out.push(part),
        }
    }
    out
}

/// The origin of a base that is an environment read or a hole, for a reference that is an
/// absolute path: `/orders` replaces whatever path the base has, so `${API_URL}` + `/orders` is
/// `${API_URL}/orders` and keeps the lead (`via env:API_URL`). `None` for anything else: a
/// relative or network-path reference (the base's path or host would matter), a query or a
/// fragment alone, a reference that starts with a part that is not text, and a base whose opaque
/// part is not its whole origin (a port or a host written after it).
// ponytail: TypeScript's rule alone, so an `EnvOr` lead (a default `fetches` reads) is not one;
// Swift keeps its answers. Another pack takes it by calling this after `resolve` fails.
pub(super) fn opaque_lead(base: &[Part], reference: &[Part]) -> Option<Vec<Part>> {
    let lead = base.first()?;
    if !matches!(lead, Part::Env(_) | Part::Hole | Part::Read { .. }) {
        return None;
    }
    match base.get(1) {
        None => {}
        Some(Part::Text(t)) if t.starts_with(['/', '?', '#']) => {}
        Some(_) => return None,
    }
    let Some(Reference::AbsolutePath) = classify(reference) else {
        return None;
    };
    let mut out = vec![lead.clone()];
    out.extend(without_dots(reference)?);
    Some(merged(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(parts: &[Part]) -> String {
        let one = |p: &Part| match p {
            Part::Text(t) => t.clone(),
            Part::Env(v) => format!("${{{v}}}"),
            _ => "{}".to_string(),
        };
        parts.iter().map(one).collect()
    }

    fn joined(base: &str, relative: &str) -> Option<String> {
        let (base, relative) = (
            vec![Part::Text(base.into())],
            vec![Part::Text(relative.into())],
        );
        resolve(&base, &relative).map(|p| written(&p))
    }

    #[test]
    fn a_relative_reference_resolves_against_its_base_as_rfc_3986_does() {
        for (base, relative, url) in [
            (
                "https://h.example.com",
                "/status",
                "https://h.example.com/status",
            ),
            (
                "https://h.example.com",
                "status",
                "https://h.example.com/status",
            ),
            (
                "https://h.example.com/api/",
                "status",
                "https://h.example.com/api/status",
            ),
            (
                "https://h.example.com/api",
                "status",
                "https://h.example.com/status",
            ),
            (
                "https://h.example.com/api/v1/",
                "/x",
                "https://h.example.com/x",
            ),
            (
                "https://a.example.com/x",
                "https://b.example.com/y",
                "https://b.example.com/y",
            ),
        ] {
            assert_eq!(
                joined(base, relative).as_deref(),
                Some(url),
                "{base} + {relative}"
            );
        }
        // The scheme ends at a `:` before any `/?#`; `//` is a network path; dot segments go;
        // the base's query is not its path.
        for (base, relative, url) in [
            (
                "https://h.example.com/api/",
                "users?next=https://x",
                "https://h.example.com/api/users?next=https://x",
            ),
            (
                "https://h.example.com/api/",
                "//cdn.example.com/x",
                "https://cdn.example.com/x",
            ),
            (
                "https://h.example.com/api/v1/",
                "../users",
                "https://h.example.com/api/users",
            ),
            (
                "https://h.example.com/api/v1/",
                "./a/../b",
                "https://h.example.com/api/v1/b",
            ),
            (
                "https://h.example.com/api/",
                "/x/../../users",
                "https://h.example.com/users",
            ),
            (
                "https://h.example.com/api/v1?next=/a/b",
                "status",
                "https://h.example.com/api/status",
            ),
            ("https://h.example.com", "mailto:a@b", "mailto:a@b"),
        ] {
            assert_eq!(
                joined(base, relative).as_deref(),
                Some(url),
                "{base} + {relative}"
            );
        }
        // A query or a fragment alone, and a base with no scheme, are not read.
        assert_eq!(joined("https://h.example.com", "?q=1"), None);
        assert_eq!(joined("/api", "status"), None);
    }

    #[test]
    fn a_base_with_a_hole_before_its_path_keeps_the_hole() {
        let base = [
            Part::Text("https://".into()),
            Part::Hole,
            Part::Text("/api/".into()),
        ];
        let status = [Part::Text("status".into())];
        assert_eq!(
            written(&resolve(&base, &status).unwrap()),
            "https://{}/api/status"
        );
        let absolute = [Part::Text("/x".into())];
        assert_eq!(written(&resolve(&base, &absolute).unwrap()), "https://{}/x");
    }

    #[test]
    fn a_parameter_on_either_side_waits_for_the_call_sites() {
        let (base, users) = (
            [Part::Text("https://h.example.com/v1/".into())],
            [Part::Ref("p".into())],
        );
        assert!(resolve(&base, &users).is_none());
        assert_eq!(
            written(&depends(&base, &users).unwrap()),
            "https://h.example.com/v1/{}"
        );
        let (base, users) = ([Part::Ref("b".into())], [Part::Text("users".into())]);
        assert!(resolve(&base, &users).is_none());
        assert!(depends(&base, &users).is_some());
        // A string with no parameter that cannot be resolved stays unread.
        assert!(depends(&users, &users).is_none());
    }

    #[test]
    fn an_absolute_path_over_an_opaque_base_keeps_the_opaque_lead() {
        let text = |t: &str| Part::Text(t.into());
        let lead = |base: Vec<Part>, reference: &str| {
            opaque_lead(&base, &[text(reference)]).map(|p| written(&p))
        };
        let env = || Part::Env("API_URL".into());
        // The base's own path is replaced, its origin is the read.
        assert_eq!(
            lead(vec![env()], "/orders").as_deref(),
            Some("${API_URL}/orders")
        );
        assert_eq!(
            lead(vec![env(), text("/v1/")], "/orders").as_deref(),
            Some("${API_URL}/orders")
        );
        assert_eq!(lead(vec![Part::Hole], "/a/../b").as_deref(), Some("{}/b"));
        let read = Part::Read {
            scope: "s".into(),
            callee: "config.getString".into(),
            key: "a.b".into(),
            then: vec![],
        };
        assert_eq!(lead(vec![read], "/orders").as_deref(), Some("{}/orders"));
        // A default (`Part::EnvOr`) has a path of its own that `fetches` would append to.
        let defaulted = Part::EnvOr("API_URL".into(), vec![text("http://h/api")]);
        assert_eq!(lead(vec![defaulted], "/orders"), None);
        // A relative reference, a network path, a query and a fragment need the base's path or
        // host: unknown. So does a reference that starts with a part that is not text.
        for reference in ["orders", "//h/orders", "?q=1", "#top", "../x"] {
            assert_eq!(lead(vec![env()], reference), None, "{reference}");
        }
        assert!(opaque_lead(&[env()], &[Part::Env("P".into()), text("/orders")]).is_none());
        // A base whose opaque part is not its whole origin, and a literal base, are not this.
        assert_eq!(lead(vec![env(), text(":8080/api")], "/orders"), None);
        assert_eq!(lead(vec![env(), Part::Hole], "/orders"), None);
        assert_eq!(lead(vec![text("https://h")], "/orders"), None);
    }

    #[test]
    fn a_query_or_a_fragment_alone_and_a_reference_led_by_a_read_are_not_read() {
        let base = [Part::Text("https://h/api/".into())];
        for reference in ["?q=1", "#top"] {
            assert_eq!(joined("https://h/api/", reference), None, "{reference}");
        }
        let read = [Part::Env("PREFIX".into()), Part::Text("/orders".into())];
        assert!(resolve(&base, &read).is_none());
        assert!(depends(&base, &read).is_none());
        let hole = [Part::Hole, Part::Text("/orders".into())];
        assert!(resolve(&base, &hole).is_none());
        // Over an opaque base the reference's lead matters as much.
        assert!(opaque_lead(&[Part::Env("API_URL".into())], &hole).is_none());
    }

    #[test]
    fn a_new_url_joins_a_literal_base_as_the_url_standard_reads_it() {
        // The cases of the language rule (#dom-url-url): an absolute reference is itself,
        // `//host/x` takes the host, `/orders` replaces the path, `orders` joins the base's
        // directory (the last `/` ends it).
        for (reference, base, url) in [
            ("https://a.test/x", "https://h/api/", "https://a.test/x"),
            ("//a.test/x", "https://h/api/", "https://a.test/x"),
            ("/orders", "https://h/api/v1", "https://h/orders"),
            ("orders", "https://h/api/", "https://h/api/orders"),
            ("orders", "https://h/api", "https://h/orders"),
            ("orders", "https://h", "https://h/orders"),
        ] {
            assert_eq!(
                joined(base, reference).as_deref(),
                Some(url),
                "{base} + {reference}"
            );
        }
    }
}
