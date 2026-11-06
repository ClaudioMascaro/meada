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
    builds: bool,
    environment: BTreeMap<String, super::environment::Entry>,
    depends_on: BTreeSet<String>,
    links: BTreeSet<String>,
    networks: BTreeSet<String>,
    /// Whether the file sets `image`, `build` or `network_mode`: a merge takes the override's
    /// even when it recorded nothing (a context outside the repository).
    image_set: bool,
    build_set: bool,
    network_mode: bool,
}

/// Map and list environments share Compose's merge-by-variable rule. Bare names stay unknown.
fn environment(value: Option<&Value>) -> BTreeMap<String, super::environment::Entry> {
    let entries = match value {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                };
                (name.clone(), value)
            })
            .collect::<Vec<_>>(),
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(Value::as_str)
            .map(|entry| {
                entry.split_once('=').map_or_else(
                    || (entry.into(), None),
                    |(name, value)| (name.into(), Some(value.into())),
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .map(|(name, value)| {
            let expanded = value
                .as_deref()
                .map(expand)
                .filter(|v| !v.text.contains("${"));
            let target = match &expanded {
                Some(v) if v.overridable => "Compose default",
                Some(_) => "Compose literal",
                None => "Compose environment (unresolved)",
            };
            let value = expanded
                .as_ref()
                .and_then(|v| super::environment::literal(&v.text));
            let entry = super::environment::Entry {
                // Shared service fields are added only after their full expansion fits.
                container: String::new(),
                image: None,
                name: name.clone(),
                target: target.into(),
                value,
            };
            (name, entry)
        })
        .collect()
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

/// How many lines `KeyLines::end_of` steps back one by one before it looks the block's end up.
const WALK_BACK: usize = 64;

/// The indent of `line`, the leading blanks (a blank line has none).
fn indent(line: &str) -> usize {
    if line.trim().is_empty() {
        0
    } else {
        line.len() - line.trim_start().len()
    }
}

/// `table[p][i]` is the greatest indent among the `2^p` lines from line `i` (0-based) on; a level
/// has one entry per window that fits. `log(lines)` levels, each as long as the text.
fn windows(indents: &[u32]) -> Vec<Vec<u32>> {
    let mut table: Vec<Vec<u32>> = vec![indents.to_vec()];
    while let Some(last) = table.last() {
        let half = 1usize << (table.len() - 1);
        if last.len() <= half {
            break;
        }
        let next: Vec<u32> = (0..last.len() - half)
            .map(|i| last[i].max(last[i + half]))
            .collect();
        table.push(next);
    }
    table
}

/// A plain key at the start of `line` and what follows it: up to the first `:` that ends the line
/// or is followed by a blank (`a:b:` is the key `a:b`).
fn plain_key(line: &str) -> Option<(&str, &str)> {
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == ':' && chars.peek().is_none_or(|(_, n)| n.is_whitespace()) {
            return Some((line[..i].trim_end(), &line[i..]));
        }
    }
    None
}

/// The line of each entry of a section's mapping (`services`, `Resources`): the key line the event
/// reader gave, else, for an entry that came by an alias (`services: *all`), the nearest line
/// above the first scalar that starts with the key, found in one pass over the text.
pub(super) struct KeyLines<'a> {
    lines: &'a [&'a str],
    read: BTreeMap<&'a str, usize>,
    /// The 1-based lines that start a key, by the key's text, built when an entry has no read line.
    by_text: std::cell::OnceCell<BTreeMap<String, Vec<usize>>>,
    /// The indent of each line, found once: a line of blanks costs its length to trim.
    indents: Vec<u32>,
    /// The maximum indent over each window of `2^p` lines (`windows`), built when `end_of` needs it.
    windows: std::cell::OnceCell<Vec<Vec<u32>>>,
}

impl<'a> KeyLines<'a> {
    pub(super) fn new(lines: &'a [&'a str], section: &str, keys: &'a [events::KeyLine]) -> Self {
        KeyLines {
            lines,
            read: (keys.iter())
                .filter(|k| k.section == section)
                .map(|k| (k.key.as_str(), k.line))
                .collect(),
            by_text: std::cell::OnceCell::new(),
            indents: lines.iter().map(|l| indent(l) as u32).collect(),
            windows: std::cell::OnceCell::new(),
        }
    }

    /// The last line of the block that starts at line `line` and whose last scalar is at `last`:
    /// the last line at or before it that is indented deeper than the key (a null value reports
    /// the line after it; a blank line, or one no deeper, ends nothing). A block ends within a few
    /// lines of `last`, except for the entries of an aliased section, which all begin at the
    /// alias's line, and for flat text: those look it up in a table of the maximum indent over
    /// the windows of the text, in `log(lines)` steps.
    pub(super) fn end_of(&self, line: usize, last: usize) -> usize {
        let key_indent = self.indents.get(line - 1).map_or(0, |&i| i as usize);
        let from = last.max(line).min(self.lines.len().max(line));
        let deeper =
            |at: usize| (self.indents.get(at - 1)).is_some_and(|&i| i as usize > key_indent);
        let mut end = from;
        for _ in 0..WALK_BACK {
            if end <= line {
                return line;
            }
            if deeper(end) {
                return end;
            }
            end -= 1;
        }
        // Every window of `2^p` lines ending at `pos` that holds nothing deeper is skipped, the
        // widest first: what is left is the first line from `end` back that is deeper.
        let table = self.windows.get_or_init(|| windows(&self.indents));
        let mut pos = end.min(self.lines.len()); // 1-based, the last line not yet ruled out
        for (p, level) in table.iter().enumerate().rev() {
            let width = 1usize << p;
            if pos >= width && level[pos - width] as usize <= key_indent {
                pos -= width;
            }
        }
        pos.max(line)
    }

    /// The line of the entry `name` whose first scalar is at line `first`.
    pub(super) fn of(&self, name: &str, first: usize) -> usize {
        if let Some(&line) = self.read.get(name) {
            return line;
        }
        let by_text = self.by_text.get_or_init(|| {
            let mut by: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for (i, l) in self.lines.iter().enumerate() {
                let l = l.trim_start();
                let key = match l.chars().next() {
                    Some(q @ ('"' | '\'')) => l[1..].split_once(q),
                    _ => plain_key(l),
                };
                if let Some((key, rest)) = key
                    && rest.trim_start().starts_with(':')
                {
                    by.entry(key.to_string()).or_default().push(i + 1);
                }
            }
            by
        });
        let nearest = by_text
            .get(name)
            .and_then(|at| at.iter().rev().find(|&&l| l <= first));
        nearest.copied().unwrap_or(first)
    }
}

/// The services of the Compose file `src` at `path`, in name order.
fn read(path: &str, src: &[u8]) -> BTreeMap<String, Service> {
    let dir = config_values::parent(path);
    // The first and last line of the scalars under each service.
    let mut spans: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let documents = events::read_keyed(Syntax::Yaml, src, false, |scalar| {
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
    let Ok((documents, key_lines)) = documents else {
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
    let key_line = KeyLines::new(&lines, "services", &key_lines);
    let mut found = BTreeMap::new();
    for (name, body) in &services {
        let Value::Object(body) = body else {
            continue;
        };
        let (first, last) = spans.get(name).copied().unwrap_or((1, 1));
        let line = key_line.of(name, first);
        let end = key_line.end_of(line, last);
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
                environment: environment(body.get("environment")),
                build: body.get("build").and_then(|b| context(dir, b)),
                builds: body
                    .get("build")
                    .is_some_and(|b| b.is_object() || b.as_str().is_some_and(|s| !s.is_empty())),
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
                    m.builds = s.builds;
                }
                m.environment.extend(s.environment);
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
            let image_size = s.image.as_ref().map_or(0, |i| i.text.len());
            let environment_size = s.environment.values().try_fold(0usize, |size, entry| {
                let size = size
                    .checked_add(entry.size())?
                    .checked_add(name.len())?
                    .checked_add(image_size)?;
                (size <= super::environment::ENVIRONMENT_BYTES_MAX).then_some(size)
            });
            // Check repeated names and images before making any owned copies.
            let environment = if environment_size.is_some() {
                s.environment
                    .values()
                    .cloned()
                    .map(|mut e| {
                        e.container.clone_from(name);
                        e.image = s.image.as_ref().map(|i| i.text.clone());
                        e
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let size = address.len() + image_size + environment_size.unwrap_or(0);
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
                build_image: s.builds,
                role: None,
                handler: None,
                events: Vec::new(),
                subscription: None,
                environment,
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

    #[test]
    fn environment_expansion_accounts_for_long_images_and_service_names() {
        let entries = (0..512)
            .map(|i| format!("      Q{i}: orders\n"))
            .collect::<String>();
        for (name, image) in [
            ("worker".into(), "i".repeat(400 << 10)),
            ("w".repeat(400 << 10), "image".into()),
        ] {
            // Explicit keys keep this valid YAML past the implicit-key length limit.
            let text = format!(
                "services:\n  ? {name}\n  :\n    image: {image}\n    environment:\n{entries}"
            );
            assert!(text.len() < crate::lang::MAX_BYTES);
            let (nodes, references) = unit(&[("compose.yaml", &text)]);
            assert_eq!(nodes.len(), 1);
            assert!(references.is_empty());
            assert_eq!(nodes[0].address, format!("services.{name}"));
            assert!(nodes[0].environment.is_empty());
            assert_eq!(nodes[0].image.as_ref().unwrap().text, image);
        }
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

    #[test]
    fn a_nested_key_spelled_like_its_service_does_not_move_its_lines() {
        let text = "services:\n  db: {image: p}\n  web:\n    environment:\n      web: 1\n      db: 2\n    image: w\n";
        let (nodes, _) = unit(&[("compose.yaml", text)]);
        let web = nodes.iter().find(|n| n.address == "services.web").unwrap();
        assert_eq!((web.line, web.end_line), (3, 7));
    }

    #[test]
    fn a_services_section_that_is_an_alias_starts_at_the_key_lines_of_its_anchor() {
        let text = "x-all: &all\n  app:\n    image: app:1\n  worker:\n    image: w:1\n    depends_on: [app]\nservices: *all\n";
        let (nodes, _) = unit(&[("compose.yaml", text)]);
        let at: Vec<_> = nodes
            .iter()
            .map(|n| (n.address.as_str(), n.line, n.end_line))
            .collect();
        assert_eq!(at, [("services.app", 2, 6), ("services.worker", 4, 6)]);
    }

    #[test]
    fn an_aliased_entry_whose_key_holds_a_colon_is_found_by_its_whole_key() {
        let text = "x-all: &all\n  a:b:\n    image: colon\n  nb:\n    image: nb\nservices: *all\n";
        let (nodes, _) = unit(&[("compose.yaml", text)]);
        let at: Vec<_> = nodes.iter().map(|n| (n.address.as_str(), n.line)).collect();
        assert_eq!(at, [("services.a:b", 2), ("services.nb", 4)]);
    }

    #[test]
    fn an_aliased_section_of_one_line_entries_is_walked_once_not_per_entry() {
        let entries: String = (0..40_000)
            .map(|i| format!("  s{i}: {{image: a}}\n"))
            .collect();
        let text = format!("x: &all\n{entries}services: *all\n");
        let start = std::time::Instant::now();
        let (nodes, _) = unit(&[("compose.yaml", &text)]);
        assert!(nodes.len() > 1000);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn flat_text_and_entries_at_every_indent_are_walked_in_linear_time() {
        // JSON with nothing indented: no line is deeper than any key.
        let entries: String = (0..20_000)
            .map(|i| format!("\"s{i}\": {{\n\"image\": \"a\"\n}},\n"))
            .collect();
        let flat = format!("{{\n\"services\": {{\n{entries}\"z\": {{}}\n}}\n}}\n");
        // Flow entries of an aliased section, each at a smaller indent than the one before.
        let deep: String = (0..1_400)
            .map(|i| format!("{}s{i}: {{image: a}},\n", " ".repeat(1_500 - i)))
            .collect();
        let deep = format!("x: &all {{\n{deep}}}\nservices: *all\n");
        for text in [flat, deep] {
            let start = std::time::Instant::now();
            let (nodes, _) = unit(&[("compose.yaml", &text)]);
            assert!(nodes.len() > 1000);
            assert!(
                start.elapsed() < std::time::Duration::from_secs(10),
                "{:?}",
                start.elapsed()
            );
        }
    }

    #[test]
    fn a_block_ends_at_its_last_deeper_line_however_far_the_walk_goes() {
        // The entry's last scalar is far below it (an alias), past a long run of lines no deeper: the
        // walk back finds the entry's own last line.
        let run = "  other: 1\n".repeat(500);
        let text = format!("x: &all\n  a:\n    image: a\n{run}services: *all\n");
        let (nodes, _) = unit(&[("compose.yaml", &text)]);
        let a = nodes.iter().find(|n| n.address == "services.a").unwrap();
        assert_eq!((a.line, a.end_line), (2, 3));
    }

    #[test]
    fn a_long_line_of_blanks_above_an_alias_is_read_once() {
        // Every entry of the aliased section walks back from the alias line over the blank line.
        let entries: String = (0..10_000)
            .map(|i| format!("  s{i}: {{image: a}}\n"))
            .collect();
        let blanks = " ".repeat(600_000);
        let text = format!("x: &all\n{entries}{blanks}\nservices: *all\n");
        let start = std::time::Instant::now();
        let (nodes, _) = unit(&[("compose.yaml", &text)]);
        assert!(nodes.len() > 1000);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            start.elapsed()
        );
    }
}
