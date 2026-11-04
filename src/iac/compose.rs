//! Compose projects as IaC nodes (#652, ADR 0025): each service of a Compose file is a node
//! `services.<name>` of type `service`, with its `depends_on`, `links` and shared networks as
//! references, and its build context and image recorded for the image contract.
//!
//! A directory's Compose file (`compose.yaml`, `compose.yml`, `docker-compose.yaml` or
//! `docker-compose.yml`, the first that exists) and the override file of the same family
//! (`compose.override.*`, `docker-compose.override.*`) are one evaluation unit, rooted at the
//! directory (`.` for the repository root). The override is merged as Compose does for the fields
//! read: a scalar it sets replaces the base's, the lists add to it.
//!
//! `${VAR:-default}` and `${VAR-default}` resolve to the default and mark the field overridable;
//! `${VAR}`, `$VAR` and the other forms stay in the text as `${VAR}`, the unresolved part. A
//! build context is kept only when it is a path inside the repository, normalized from the root.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{NODES_MAX, Node, REFERENCES_MAX, ROOT_STORED_MAX, Reference, Resolved, local_source};
use crate::config_values;
use crate::document::events::{self, Step, Syntax};

/// How deep a default may nest another (`${A:-${B:-x}}`); a deeper one is left unresolved.
// ponytail: a default naming another variable nests once or twice; 8 stops a built chain.
const DEFAULT_DEPTH_MAX: usize = 8;

/// The names Compose looks for, in order, and the override file of each family.
const BASES: [(&str, &str); 4] = [
    ("compose.yaml", "compose.override.yaml"),
    ("compose.yml", "compose.override.yml"),
    ("docker-compose.yaml", "docker-compose.override.yaml"),
    ("docker-compose.yml", "docker-compose.override.yml"),
];

/// The names of the Compose files and of their override files.
pub(super) const NAMES: [&str; 8] = [
    "compose.yaml",
    "compose.override.yaml",
    "compose.yml",
    "compose.override.yml",
    "docker-compose.yaml",
    "docker-compose.override.yaml",
    "docker-compose.yml",
    "docker-compose.override.yml",
];

/// Whether the file at `path` is a Compose file or an override file by its name.
pub(crate) fn is_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    BASES
        .iter()
        .any(|(base, over)| *base == name || *over == name)
}

/// A text with its variables expanded.
#[derive(Debug, PartialEq)]
pub(crate) struct Expanded {
    pub text: String,
    /// Whether a default was used: a deployer's environment can replace the value.
    pub overridable: bool,
}

/// `text` with its Compose variables expanded: `$$` is `$`, `${VAR:-d}` and `${VAR-d}` are `d`
/// (itself expanded) and mark the result overridable, every other form stays as `${VAR}`.
pub(crate) fn expand(text: &str) -> Expanded {
    let mut out = Expanded {
        text: String::new(),
        overridable: false,
    };
    expand_into(text, &mut out, 0);
    out
}

fn expand_into(text: &str, out: &mut Expanded, depth: usize) {
    let bytes = text.as_bytes();
    let name_len = |s: &str| {
        let ok = |(i, c): (usize, char)| {
            c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
        };
        s.char_indices().take_while(|&p| ok(p)).count()
    };
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            let next = text[i..].find('$').map_or(text.len(), |n| i + n);
            out.text.push_str(&text[i..next]);
            i = next;
            continue;
        }
        let rest = &text[i + 1..];
        if rest.starts_with('$') {
            out.text.push('$');
            i += 2;
        } else if let Some(body) = rest.strip_prefix('{') {
            // The matching brace, counting the ones a nested default opens.
            let mut open = 1usize;
            let close = body.char_indices().find(|&(_, c)| {
                match c {
                    '{' => open += 1,
                    '}' => open -= 1,
                    _ => {}
                }
                open == 0
            });
            let Some((end, _)) = close else {
                out.text.push_str(&text[i..]);
                return;
            };
            let inner = &body[..end];
            let n = name_len(inner);
            let default = inner[n..]
                .strip_prefix(":-")
                .or_else(|| inner[n..].strip_prefix('-'));
            match default {
                Some(d) if n > 0 && depth < DEFAULT_DEPTH_MAX => {
                    out.overridable = true;
                    expand_into(d, out, depth + 1);
                }
                _ if n > 0 && n == inner.len() => out.text.push_str(&format!("${{{inner}}}")),
                // `${VAR:?err}`, `${VAR:+alt}` and the like: the variable is the unresolved part.
                _ if n > 0 => out.text.push_str(&format!("${{{}}}", &inner[..n])),
                _ => out.text.push_str(&format!("${{{inner}}}")),
            }
            i += 2 + end + 1;
        } else {
            let n = name_len(rest);
            if n == 0 {
                out.text.push('$');
                i += 1;
            } else {
                out.text.push_str(&format!("${{{}}}", &rest[..n]));
                i += 1 + n;
            }
        }
    }
}

/// A service as read from one file.
#[derive(Default)]
struct Service {
    path: String,
    line: usize,
    end_line: usize,
    image: Option<Resolved>,
    build: Option<Resolved>,
    depends_on: BTreeSet<String>,
    links: BTreeSet<String>,
    networks: BTreeSet<String>,
    /// Whether the file sets `image`, `build` or `network_mode`: a merge takes the override's
    /// even when it recorded nothing (a context outside the repository).
    image_set: bool,
    build_set: bool,
    network_mode: bool,
}

/// The names a `depends_on`, `links` or `networks` holds: a list's strings or a mapping's keys,
/// a link without its alias.
fn names(value: Option<&Value>, links: bool) -> BTreeSet<String> {
    let items: Vec<&str> = match value {
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect(),
        Some(Value::Object(map)) => map.keys().map(String::as_str).collect(),
        _ => Vec::new(),
    };
    (items.into_iter())
        .map(|s| match links {
            true => s.split(':').next().unwrap_or(s).to_string(),
            false => s.to_string(),
        })
        .collect()
}

/// The build context of a service in the directory `dir`: the text after expansion, normalized
/// from the repository root, or `None` when it is not a path inside it (absolute, a URL, climbing
/// out of the root, or still holding an unresolved part).
fn context(dir: &str, build: &Value) -> Option<Resolved> {
    let written = match build {
        Value::String(s) => s.as_str(),
        Value::Object(map) => map.get("context").and_then(Value::as_str).unwrap_or("."),
        _ => return None,
    };
    let Expanded { text, overridable } = expand(written);
    if text.contains('$') || text.contains("://") || text.starts_with(['/', '~', '\\']) {
        return None;
    }
    let local = match text.starts_with("./") || text.starts_with("../") {
        true => text,
        false => format!("./{text}"),
    };
    let path = local_source(dir, &local)?;
    let text = if path.is_empty() { ".".into() } else { path };
    Some(Resolved { text, overridable })
}

/// `value` with every YAML merge key (`<<: *anchor`, or a list of anchors) resolved, wherever it
/// is and however the anchors chain: a key a mapping sets itself wins, and an earlier mapping of
/// a list wins over a later. The reader bounds the nesting, so the recursion is shallow.
fn unmerge(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map.iter().filter(|(k, _)| *k != "<<") {
                out.insert(k.clone(), unmerge(v));
            }
            let from: Vec<&Value> = match map.get("<<") {
                Some(Value::Array(list)) => list.iter().collect(),
                Some(one) => vec![one],
                None => Vec::new(),
            };
            for source in from {
                if let Value::Object(merged) = unmerge(source) {
                    for (k, v) in merged {
                        out.entry(k).or_insert(v);
                    }
                }
            }
            Value::Object(out)
        }
        Value::Array(list) => Value::Array(list.iter().map(unmerge).collect()),
        other => other.clone(),
    }
}

/// The services of the Compose file `src` at `path`, in name order.
fn read(path: &str, src: &[u8]) -> BTreeMap<String, Service> {
    let dir = config_values::parent(path);
    // The first and last line of the scalars under each service.
    let mut spans: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let documents = events::read(Syntax::Yaml, src, |scalar| {
        if let [Step::Key(top), Step::Key(name), ..] = scalar.path
            && top == "services"
        {
            let span = spans
                .entry(name.clone())
                .or_insert((scalar.line, scalar.line));
            span.0 = span.0.min(scalar.line);
            span.1 = span.1.max(scalar.line);
        }
    });
    let Ok(documents) = documents else {
        return BTreeMap::new();
    };
    let Some(Value::Object(services)) = documents
        .first()
        .and_then(|d| d.get("services"))
        .map(unmerge)
    else {
        return BTreeMap::new();
    };
    let text = String::from_utf8_lossy(src);
    let lines: Vec<&str> = text.lines().collect();
    let mut found = BTreeMap::new();
    for (name, body) in &services {
        let Value::Object(body) = body else {
            continue;
        };
        let (first, last) = spans.get(name).copied().unwrap_or((1, 1));
        // The key's own line: the nearest one at or above the first scalar that starts with it.
        let keyed = |l: &&str| {
            let l = l.trim_start();
            [name.clone(), format!("\"{name}\""), format!("'{name}'")]
                .iter()
                .any(|k| {
                    l.strip_prefix(k.as_str())
                        .is_some_and(|r| r.trim_start().starts_with(':'))
                })
        };
        let line = (lines.iter().enumerate().take(first).rev())
            .find(|(_, l)| keyed(l))
            .map_or(first, |(i, _)| i + 1);
        // A null value reports the line after it: step back over lines that are not nested in
        // the service (blank, or indented no deeper than its key).
        let indent = |l: &str| l.len() - l.trim_start().len();
        let key_indent = lines.get(line - 1).map_or(0, |l| indent(l));
        let mut end = last.max(line).min(lines.len().max(line));
        while end > line
            && lines
                .get(end - 1)
                .is_none_or(|l| l.trim().is_empty() || indent(l) <= key_indent)
        {
            end -= 1;
        }
        let image = body.get("image").and_then(Value::as_str).map(|s| {
            let Expanded { text, overridable } = expand(s);
            Resolved { text, overridable }
        });
        found.insert(
            name.clone(),
            Service {
                path: path.to_string(),
                line,
                end_line: end,
                image,
                build: body.get("build").and_then(|b| context(dir, b)),
                depends_on: names(body.get("depends_on"), false),
                links: names(body.get("links"), true),
                networks: names(body.get("networks"), false),
                image_set: body.contains_key("image"),
                build_set: body.contains_key("build"),
                network_mode: body.contains_key("network_mode"),
            },
        );
    }
    found
}

/// The services of a unit: the base's, then the override's merged in.
fn merge(
    base: BTreeMap<String, Service>,
    over: BTreeMap<String, Service>,
) -> BTreeMap<String, Service> {
    let mut merged = base;
    for (name, s) in over {
        match merged.get_mut(&name) {
            Some(m) => {
                if s.image_set {
                    m.image = s.image;
                }
                if s.build_set {
                    m.build = s.build;
                }
                m.network_mode |= s.network_mode;
                m.depends_on.extend(s.depends_on);
                m.links.extend(s.links);
                m.networks.extend(s.networks);
            }
            None => {
                merged.insert(name, s);
            }
        }
    }
    merged
}

/// The IaC nodes and references of the Compose files `files` (path and bytes), one unit per
/// directory, in directory order. `left` is the budget of stored bytes the whole repository still
/// has; it shrinks by what is stored.
pub(super) fn evaluate(
    files: &[(String, Vec<u8>)],
    left: &mut usize,
) -> (Vec<Node>, Vec<Reference>) {
    let mut dirs: BTreeMap<&str, BTreeMap<&str, &[u8]>> = BTreeMap::new();
    for (path, src) in files {
        let name = path.rsplit('/').next().unwrap_or(path);
        dirs.entry(config_values::parent(path))
            .or_default()
            .insert(name, src);
    }
    let (mut nodes, mut references) = (Vec::new(), Vec::new());
    for (dir, named) in dirs {
        let Some((base, over)) = BASES.iter().find(|(b, _)| named.contains_key(b)) else {
            continue;
        };
        let at = |name: &str| match dir.is_empty() {
            true => name.to_string(),
            false => format!("{dir}/{name}"),
        };
        let mut services = read(&at(base), named[base]);
        if let Some(src) = named.get(over) {
            services = merge(services, read(&at(over), src));
        }
        // A service with no `networks:` after the merge is on the project's `default` network, unless it sets a `network_mode`.
        for s in (services.values_mut()).filter(|s| s.networks.is_empty() && !s.network_mode) {
            s.networks.insert("default".into());
        }
        let root = if dir.is_empty() { "." } else { dir }.to_string();
        let mut room = ROOT_STORED_MAX.min(*left);
        let mut take = |bytes: usize| match room.checked_sub(bytes) {
            Some(r) => {
                room = r;
                true
            }
            None => false,
        };
        let by_network =
            services
                .iter()
                .fold(BTreeMap::<&str, Vec<&str>>::new(), |mut m, (name, s)| {
                    s.networks
                        .iter()
                        .for_each(|n| m.entry(n).or_default().push(name));
                    m
                });
        for (name, s) in services.iter().take(NODES_MAX) {
            let address = format!("services.{name}");
            let size = address.len() + s.image.as_ref().map_or(0, |i| i.text.len());
            if !take(size) {
                break;
            }
            nodes.push(Node {
                address: address.clone(),
                root: root.clone(),
                ty: "service".into(),
                name: None,
                pattern: false,
                overridable: Vec::new(),
                path: s.path.clone(),
                line: s.line,
                end_line: s.end_line,
                block: config_values::key_path(&[
                    Step::Key("services".into()),
                    Step::Key(name.clone()),
                ]),
                image: s.image.clone(),
                build: s.build.clone(),
            });
            let mut targets: BTreeSet<(String, String)> = BTreeSet::new();
            for (list, via) in [(&s.depends_on, "depends_on"), (&s.links, "links")] {
                for t in list
                    .iter()
                    .filter(|t| *t != name && services.contains_key(*t))
                {
                    targets.insert((format!("services.{t}"), via.into()));
                }
            }
            for net in &s.networks {
                for t in by_network[net.as_str()].iter().filter(|t| **t != name) {
                    if targets.len() >= REFERENCES_MAX {
                        break;
                    }
                    targets.insert((format!("services.{t}"), format!("networks.{net}")));
                }
            }
            for (target, via) in targets.into_iter().take(REFERENCES_MAX) {
                if !take(address.len() + target.len() + via.len()) {
                    break;
                }
                references.push(Reference {
                    root: root.clone(),
                    source: address.clone(),
                    target,
                    via: Some(via),
                });
            }
        }
        *left -= ROOT_STORED_MAX.min(*left) - room;
    }
    (nodes, references)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(list: &[(&str, &str)]) -> (Vec<Node>, Vec<Reference>) {
        let files: Vec<_> = (list.iter())
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect();
        evaluate(&files, &mut usize::MAX.clone())
    }

    fn x(text: &str) -> (String, bool) {
        let e = expand(text);
        (e.text, e.overridable)
    }

    #[test]
    fn a_default_resolves_and_marks_the_text_overridable() {
        assert_eq!(x("app:${TAG:-1.2}"), ("app:1.2".into(), true));
        assert_eq!(x("app:${TAG-1.2}"), ("app:1.2".into(), true));
        assert_eq!(x("${A:-${B:-x}}-y"), ("x-y".into(), true));
        assert_eq!(x("app:latest"), ("app:latest".into(), false));
        assert_eq!(x("a$$b"), ("a$b".into(), false));
    }

    #[test]
    fn a_variable_with_no_default_stays_an_unresolved_part() {
        assert_eq!(x("app:${TAG}"), ("app:${TAG}".into(), false));
        assert_eq!(x("app:$TAG"), ("app:${TAG}".into(), false));
        assert_eq!(x("app:${TAG:?needed}"), ("app:${TAG}".into(), false));
        assert_eq!(x("app:${TAG:+x}"), ("app:${TAG}".into(), false));
        assert_eq!(x("app:${TAG"), ("app:${TAG".into(), false));
        assert_eq!(x("cost $ 5"), ("cost $ 5".into(), false));
    }

    #[test]
    fn a_default_nested_past_the_ceiling_is_left_unresolved() {
        let nested = |n: usize| "${A:-".repeat(n) + "x" + &"}".repeat(n);
        assert_eq!(x(&nested(DEFAULT_DEPTH_MAX)).0, "x");
        assert_ne!(x(&nested(DEFAULT_DEPTH_MAX + 2)).0, "x");
    }

    const BASE: &str = "services:\n  web:\n    image: web:${TAG:-1}\n    build: ./web\n    depends_on: [api]\n    networks: [front]\n  api:\n    image: api\n    links:\n      - db:database\n    networks:\n      front:\n  db:\n    image: postgres\n";

    #[test]
    fn services_are_nodes_with_their_references_image_and_build() {
        let (nodes, references) = unit(&[("compose.yaml", BASE)]);
        let shown: Vec<_> = (nodes.iter())
            .map(|n| (n.address.as_str(), n.line, n.end_line, n.root.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("services.api", 7, 12, "."),
                ("services.db", 13, 14, "."),
                ("services.web", 2, 6, "."),
            ]
        );
        let web = &nodes[2];
        assert_eq!(
            web.image.as_ref().map(|i| (i.text.as_str(), i.overridable)),
            Some(("web:1", true))
        );
        assert_eq!(
            web.build.as_ref().map(|i| (i.text.as_str(), i.overridable)),
            Some(("web", false))
        );
        let refs: Vec<_> = (references.iter())
            .map(|r| {
                format!(
                    "{} -> {} via {}",
                    r.source,
                    r.target,
                    r.via.as_deref().unwrap()
                )
            })
            .collect();
        assert_eq!(
            refs,
            [
                "services.api -> services.db via links",
                "services.api -> services.web via networks.front",
                "services.web -> services.api via depends_on",
                "services.web -> services.api via networks.front",
            ]
        );
    }

    #[test]
    fn an_override_file_replaces_scalars_and_adds_to_lists() {
        let over = "services:\n  web:\n    image: web:dev\n    depends_on: [db]\n  cache:\n    image: redis\n";
        let (nodes, references) =
            unit(&[("p/compose.yaml", BASE), ("p/compose.override.yaml", over)]);
        let web = nodes.iter().find(|n| n.address == "services.web").unwrap();
        assert_eq!(
            web.image.as_ref().map(|i| (i.text.as_str(), i.overridable)),
            Some(("web:dev", false))
        );
        assert_eq!(web.build.as_ref().map(|b| b.text.as_str()), Some("p/web"));
        assert_eq!(web.path, "p/compose.yaml");
        let cache = nodes
            .iter()
            .find(|n| n.address == "services.cache")
            .unwrap();
        assert_eq!(
            (cache.path.as_str(), cache.line),
            ("p/compose.override.yaml", 5)
        );
        assert_eq!(web.root, "p");
        assert!(
            references
                .iter()
                .any(|r| r.source == "services.web" && r.target == "services.db")
        );
        // An override with no base of its family is no project; the family picks its override.
        assert!(unit(&[("compose.override.yaml", over)]).0.is_empty());
        let (nodes, _) = unit(&[
            ("docker-compose.yml", BASE),
            ("compose.override.yaml", over),
        ]);
        assert!(nodes.iter().all(|n| n.address != "services.cache"));
    }

    #[test]
    fn a_build_context_stays_inside_the_repository() {
        let build = |ctx: &str| {
            let text = format!("services:\n  a:\n    build: {{context: \"{ctx}\"}}\n");
            unit(&[("x/compose.yaml", &text)]).0[0]
                .build
                .clone()
                .map(|b| b.text)
        };
        assert_eq!(build("."), Some("x".into()));
        assert_eq!(build("../shared"), Some("shared".into()));
        assert_eq!(build("../.."), None);
        assert_eq!(build("/abs"), None);
        assert_eq!(build("https://github.com/a/b.git"), None);
        assert_eq!(build("${DIR}/app"), None);
        assert_eq!(build("${DIR:-app}"), Some("x/app".into()));
    }

    #[test]
    fn a_merge_key_adds_the_anchor_and_a_service_with_no_networks_is_on_default() {
        let text = "x-b: &b\n  image: a:1\n  depends_on: [db]\nservices:\n  db: {image: p}\n  s:\n    <<: *b\n    image: s:2\n";
        let (nodes, references) = unit(&[("compose.yaml", text)]);
        let s = nodes.iter().find(|n| n.address == "services.s").unwrap();
        assert_eq!(s.image.as_ref().map(|i| i.text.as_str()), Some("s:2"));
        let via: Vec<_> = (references.iter())
            .map(|r| format!("{}>{} {}", r.source, r.target, r.via.as_deref().unwrap()))
            .collect();
        assert_eq!(
            via,
            [
                "services.db>services.s networks.default",
                "services.s>services.db depends_on",
                "services.s>services.db networks.default",
            ]
        );
    }

    #[test]
    fn a_chain_of_merge_keys_and_one_inside_build_are_followed() {
        let text = concat!(
            "x: &a {image: first:1, depends_on: [db]}\nx2: &n {<<: *a, networks: [front]}\n",
            "services:\n  db: {image: p}\n  deep: {<<: *n}\n",
            "  built: {build: {<<: {context: ./ctxm}}}\n"
        );
        let (nodes, references) = unit(&[("m/compose.yaml", text)]);
        let deep = nodes.iter().find(|n| n.address == "services.deep").unwrap();
        assert_eq!(
            deep.image.as_ref().map(|i| i.text.as_str()),
            Some("first:1")
        );
        let built = nodes
            .iter()
            .find(|n| n.address == "services.built")
            .unwrap();
        assert_eq!(
            built.build.as_ref().map(|b| b.text.as_str()),
            Some("m/ctxm")
        );
        assert!(references.iter().any(|r| r.source == "services.deep"
            && r.target == "services.db"
            && r.via.as_deref() == Some("depends_on")));
    }

    #[test]
    fn an_override_with_a_rejected_build_replaces_it_and_network_mode_leaves_default() {
        let base =
            "services:\n  a: {build: ./a, image: x}\n  b: {network_mode: host}\n  c: {}\n  d: {}\n";
        let over = "services:\n  a: {build: /abs}\n";
        let (nodes, references) = unit(&[("compose.yaml", base), ("compose.override.yaml", over)]);
        assert!(nodes[0].build.is_none() && nodes[0].image.is_some());
        let refs: Vec<_> = references.iter().map(|r| r.source.as_str()).collect();
        assert_eq!(refs.len(), 6);
        assert!(!refs.contains(&"services.b"));
    }

    #[test]
    fn units_past_the_ceilings_are_cut() {
        // More dependencies than REFERENCES_MAX keep the first REFERENCES_MAX in address order.
        let n = REFERENCES_MAX + 5;
        let mut text = String::from("services:\n  hub:\n    networks: [n]\n");
        for i in 0..n {
            text += &format!("  s{i:03}:\n    networks: [n]\n");
        }
        let (nodes, references) = unit(&[("compose.yaml", &text)]);
        assert_eq!(nodes.len(), n + 1);
        assert_eq!(
            references
                .iter()
                .filter(|r| r.source == "services.hub")
                .count(),
            REFERENCES_MAX
        );
        // More services than NODES_MAX keep the first NODES_MAX in name order.
        let many: String = (0..NODES_MAX + 3)
            .map(|i| format!("  s{i:06}: {{networks: [n{i}]}}\n"))
            .collect();
        let (nodes, _) = unit(&[("compose.yaml", &format!("services:\n{many}"))]);
        assert_eq!(nodes.len(), NODES_MAX);
        // A budget that holds a node but not its references stops there.
        let files = [("compose.yaml".to_string(), BASE.as_bytes().to_vec())];
        let (nodes, _) = evaluate(&files, &mut 40);
        assert!(nodes.len() < 3);
    }
}
