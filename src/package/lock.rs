//! A Yarn Berry (2 and later) `yarn.lock`: the entries it resolves, the edges between them and
//! its workspace entries. `read` parses one, `diff` names what moved between two, `reached` walks
//! from what moved up to the workspaces that depend on it, and `direct` lists what each
//! workspace's own dependencies resolve to. Nothing here runs Yarn or reads a registry.
//!
//! An entry's key lists the descriptors (`name@range`) it resolves, comma-separated; its
//! `resolution` is the locator. A dependency `name: range` is the descriptor `name@range`,
//! matched against the keys after normalizing both sides: a `::<params>` suffix dropped, `npm:`
//! put before a range with no protocol, and the root manifest's `resolutions` applied. One that
//! still matches no key goes to every entry of that name, the safe side, and those entries are
//! marked as reached by name.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::document::events::{self, Syntax};

/// The most a lockfile may hold. A monorepo's is a few MiB, past `lang::MAX_BYTES`, which bounds
/// the other readers.
// ponytail: 16 MiB; a larger lockfile is `too-large`, which a trace reads as every package.
pub(crate) const MAX_LOCK_BYTES: usize = 16 << 20;

/// Why a lockfile was not read.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Unread {
    /// No version before the change: a new lockfile, or a change given as whole files.
    NoBase,
    /// The change deletes it.
    Deleted,
    /// Over `MAX_LOCK_BYTES`.
    TooLarge,
    /// Not a Berry lockfile: no `__metadata` (Yarn 1 and the rest).
    Format,
    /// Not YAML, or not a map of entries.
    Unparsable,
}

impl Unread {
    /// The kebab-case name the answer and the index spell it with.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Unread::NoBase => "no-base",
            Unread::Deleted => "deleted",
            Unread::TooLarge => "too-large",
            Unread::Format => "format",
            Unread::Unparsable => "unparsable",
        }
    }
}

/// One entry: what it resolved to and what it depends on.
#[derive(Debug, PartialEq)]
struct Entry {
    version: String,
    checksum: Option<String>,
    /// Each dependency's name and normalized range, as written.
    dependencies: BTreeMap<String, String>,
    /// The normalized descriptors of its key.
    keys: Vec<String>,
}

/// What one dependency edge reaches.
#[derive(Debug, PartialEq)]
enum Target {
    /// The keys it matched.
    Keys(Vec<String>),
    /// No key: every entry of its name.
    ByName,
}

/// A parsed lockfile.
#[derive(Debug, Default)]
pub(crate) struct Lock {
    /// Entries by locator.
    entries: BTreeMap<String, Entry>,
    /// Each normalized descriptor's locator.
    descriptors: BTreeMap<String, String>,
    /// The workspace entries: directory (relative to the lockfile's folder, `.` for its own)
    /// to locator.
    pub(crate) workspaces: BTreeMap<String, String>,
    /// The directory of each workspace locator.
    dirs: HashMap<String, String>,
    /// The locators of each name.
    named: HashMap<String, Vec<String>>,
    /// The names some dependency reached by name only: every entry of such a name is marked.
    by_name: BTreeSet<String>,
    /// Each dependency of each entry and what it reaches, by locator then dependency name.
    edges: BTreeMap<String, BTreeMap<String, Target>>,
    /// The reverse of the edges that matched a key: each descriptor's dependents, as locator
    /// and dependency name.
    dependents: HashMap<String, Vec<(String, String)>>,
    /// The reverse of the by-name edges, by the name they reach.
    by_name_dependents: HashMap<String, Vec<(String, String)>>,
}

/// A descriptor that moved between two lockfiles, with its version on each side.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Moved {
    pub descriptor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Whether an edge reached it by name only, so a dependent may not use it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub by_name: bool,
}

/// What one workspace's dependency resolves to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Resolved {
    pub dir: String,
    pub name: String,
    pub origin: String,
    pub version: String,
}

/// The name part of a descriptor or locator: up to the `@` after the first character, so a
/// scoped `@a/b@x` is `@a/b`.
fn name_of(descriptor: &str) -> &str {
    descriptor
        .get(1..)
        .and_then(|rest| rest.find('@'))
        .map_or(descriptor, |at| &descriptor[..at + 1])
}

/// Whether a range starts with a protocol (`npm:`, `workspace:`, `patch:`...).
fn has_protocol(range: &str) -> bool {
    range.split_once(':').is_some_and(|(p, _)| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// `name@range` normalized: no `::<params>` suffix, and `npm:` before a range with no protocol.
fn descriptor(name: &str, range: &str) -> String {
    let range = range.split_once("::").map_or(range, |(r, _)| r);
    match has_protocol(range) {
        true => format!("{name}@{range}"),
        false => format!("{name}@npm:{range}"),
    }
}

/// A key's descriptor normalized as `descriptor` does.
fn normalized(key: &str) -> String {
    let name = name_of(key);
    descriptor(name, key.get(name.len() + 1..).unwrap_or(""))
}

/// The protocol of a locator, after its name: `npm`, `workspace`, `patch`...
fn origin(locator: &str) -> String {
    let range = locator.get(name_of(locator).len() + 1..).unwrap_or("");
    range.split_once(':').map_or("npm", |(p, _)| p).to_string()
}

/// The descriptor a `patch:` descriptor wraps, percent-decoded and normalized:
/// `resolve@patch:resolve@npm%3A^1#~builtin<compat/resolve>` wraps `resolve@npm:^1`.
fn patched(descriptor: &str) -> Option<String> {
    let range = descriptor.get(name_of(descriptor).len() + 1..)?;
    let inner = range.strip_prefix("patch:")?;
    let inner = inner.split_once('#').map_or(inner, |(i, _)| i);
    let mut out = Vec::with_capacity(inner.len());
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| inner.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    Some(normalized(&String::from_utf8(out).ok()?))
}

/// The name a `resolutions` key applies to: its last package name, `parent/name` and
/// `**/name` taken by `name`, a scoped `@a/b` kept whole, a trailing `@range` dropped.
fn override_name(pattern: &str) -> &str {
    let segments: Vec<&str> = pattern.split('/').collect();
    let n = segments.len();
    let last = match n {
        _ if n >= 2 && segments[n - 2].starts_with('@') => {
            let start = pattern.len() - segments[n - 1].len() - segments[n - 2].len() - 1;
            &pattern[start..]
        }
        _ => segments[n - 1],
    };
    name_of(last)
}

/// A scalar as text: a string as written, a number or a boolean as YAML spells it.
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A Berry lockfile. `overrides` is the root manifest's `resolutions`, pattern to range.
pub(crate) fn read(bytes: &[u8], overrides: &BTreeMap<String, String>) -> Result<Lock, Unread> {
    if bytes.len() > MAX_LOCK_BYTES {
        return Err(Unread::TooLarge);
    }
    // Before any parse: only a Berry lockfile has `__metadata` at the top.
    let berry = bytes
        .split(|&b| b == b'\n')
        .any(|line| line.starts_with(b"__metadata:"));
    if !berry {
        return Err(Unread::Format);
    }
    let Ok(Value::Object(top)) = events::first(Syntax::Yaml, bytes) else {
        return Err(Unread::Unparsable);
    };
    let overrides: HashMap<&str, &str> = (overrides.iter())
        .map(|(pattern, range)| (override_name(pattern), range.as_str()))
        .collect();
    let mut lock = Lock::default();
    for (key, value) in top {
        if key == "__metadata" {
            continue;
        }
        let Value::Object(fields) = value else {
            return Err(Unread::Unparsable);
        };
        let Some(locator) = fields.get("resolution").and_then(scalar) else {
            return Err(Unread::Unparsable);
        };
        let dependencies = match fields.get("dependencies") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(deps)) => (deps.iter())
                .map(|(name, range)| Some((name.clone(), scalar(range)?)))
                .collect::<Option<_>>()
                .ok_or(Unread::Unparsable)?,
            Some(_) => return Err(Unread::Unparsable),
        };
        let keys: Vec<String> = key.split(',').map(|k| normalized(k.trim())).collect();
        for k in &keys {
            lock.descriptors.insert(k.clone(), locator.clone());
        }
        if origin(&locator) == "workspace" {
            // `<name>@workspace:<dir>`.
            let dir = locator.get(name_of(&locator).len() + 11..).unwrap_or(".");
            lock.workspaces.insert(dir.to_string(), locator.clone());
            lock.dirs.insert(locator.clone(), dir.to_string());
        }
        let name = name_of(&locator).to_string();
        lock.named.entry(name).or_default().push(locator.clone());
        let entry = Entry {
            version: fields.get("version").and_then(scalar).unwrap_or_default(),
            checksum: fields.get("checksum").and_then(scalar),
            dependencies,
            keys,
        };
        lock.entries.insert(locator, entry);
    }
    for (locator, entry) in &lock.entries {
        let mut out = BTreeMap::new();
        for (name, range) in &entry.dependencies {
            // An override may be scoped to a parent this edge is not under: both the overridden
            // and the written range are tried, and every key they match is an edge.
            let tried = (overrides.get(name.as_str()).map(|r| descriptor(name, r)))
                .into_iter()
                .chain([descriptor(name, range)]);
            let mut keys: Vec<String> =
                tried.filter(|d| lock.descriptors.contains_key(d)).collect();
            keys.dedup();
            let target = match keys.is_empty() {
                true => {
                    lock.by_name.insert(name.clone());
                    (lock.by_name_dependents.entry(name.clone()).or_default())
                        .push((locator.clone(), name.clone()));
                    Target::ByName
                }
                false => {
                    for d in &keys {
                        (lock.dependents.entry(d.clone()).or_default())
                            .push((locator.clone(), name.clone()));
                    }
                    Target::Keys(keys)
                }
            };
            out.insert(name.clone(), target);
        }
        lock.edges.insert(locator.clone(), out);
    }
    Ok(lock)
}

impl Lock {
    /// The entry a descriptor resolves to, with its locator.
    fn resolve(&self, descriptor: &str) -> Option<(&str, &Entry)> {
        let (locator, entry) = self
            .entries
            .get_key_value(self.descriptors.get(descriptor)?)?;
        Some((locator, entry))
    }

    /// Whether `descriptor` resolves to a workspace.
    pub(crate) fn is_workspace(&self, descriptor: &str) -> bool {
        (self.resolve(descriptor)).is_some_and(|(locator, _)| self.dirs.contains_key(locator))
    }

    /// Whether `locator` is an entry some dependency reached by name only.
    fn by_name(&self, locator: &str) -> bool {
        self.by_name.contains(name_of(locator))
    }
}

/// The descriptors that moved from `base` to `head`, sorted: a locator, version, checksum or
/// dependency list that differs, or a descriptor on one side only. A moved `patch:` descriptor
/// moves the descriptor it wraps too, so a patch's new hash reaches what depends on the original.
pub(crate) fn diff(base: &Lock, head: &Lock) -> Vec<Moved> {
    let every: BTreeSet<&String> = base
        .descriptors
        .keys()
        .chain(head.descriptors.keys())
        .collect();
    let by_name = |l: Option<(&str, &Entry)>| {
        l.is_some_and(|(locator, _)| base.by_name(locator) || head.by_name(locator))
    };
    let mut moved = BTreeMap::new();
    let mut wrapped = Vec::new();
    // Each locator's entries compared once, however many keys it has.
    let mut compared: HashMap<&str, bool> = HashMap::new();
    for d in every {
        let (b, h) = (base.resolve(d), head.resolve(d));
        let unchanged = match (b, h) {
            (Some((bl, be)), Some((hl, he))) if bl == hl => {
                *compared.entry(hl).or_insert_with(|| same(be, he))
            }
            _ => false,
        };
        if unchanged {
            continue;
        }
        wrapped.extend(patched(d));
        moved.insert(
            d.clone(),
            Moved {
                descriptor: d.clone(),
                before: b.map(|(_, e)| e.version.clone()),
                after: h.map(|(_, e)| e.version.clone()),
                by_name: by_name(b) || by_name(h),
            },
        );
    }
    for d in wrapped {
        let (b, h) = (base.resolve(&d), head.resolve(&d));
        moved.entry(d.clone()).or_insert_with(|| Moved {
            descriptor: d,
            before: b.map(|(_, e)| e.version.clone()),
            after: h.map(|(_, e)| e.version.clone()),
            by_name: by_name(b) || by_name(h),
        });
    }
    moved.into_values().collect()
}

/// The workspaces of `head` that depend on what moved, each with the names of its own
/// dependencies that lead there. The walk starts at each moved descriptor's dependents (a key
/// added to an unchanged entry reaches only what uses that key), goes up the dependency edges of
/// `head` through every key of each entry it reaches, and stops at a workspace: one reached
/// through another workspace is the code graph's to follow. A workspace whose own entry changed
/// is reached through the dependencies it added, removed or changed since `base`.
/// `peerDependencies` are no edges: the entry that provides a peer lists it. Each descriptor's
/// dependents and each name's by-name dependents are taken once.
pub(crate) fn reached(
    base: &Lock,
    head: &Lock,
    moved: &[Moved],
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let (mut descriptors, mut names): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    for m in moved {
        let Some((locator, entry)) = head.resolve(&m.descriptor) else {
            continue;
        };
        let own = base.entries.get(locator);
        match head.dirs.get(locator) {
            Some(dir) => {
                let none = BTreeMap::new();
                let before = own.map_or(&none, |e| &e.dependencies);
                let after = &entry.dependencies;
                let changed =
                    (after.keys().chain(before.keys())).filter(|n| before.get(*n) != after.get(*n));
                out.entry(dir.clone()).or_default().extend(changed.cloned());
            }
            None => {
                descriptors.push(&m.descriptor);
                // A by-name edge gets whichever entry of the name: one that changed reaches it.
                if !own.is_some_and(|b| same(b, entry)) {
                    names.push(name_of(locator));
                }
            }
        }
    }
    let (mut taken, mut named, mut queued) = (HashSet::new(), HashSet::new(), HashSet::new());
    loop {
        let dependents: Vec<&(String, String)> = if let Some(d) = descriptors.pop() {
            if !taken.insert(d) {
                continue;
            }
            head.dependents.get(d).into_iter().flatten().collect()
        } else if let Some(name) = names.pop() {
            if !named.insert(name) {
                continue;
            }
            head.by_name_dependents
                .get(name)
                .into_iter()
                .flatten()
                .collect()
        } else {
            break;
        };
        for (dependent, dependency) in dependents {
            match head.dirs.get(dependent) {
                Some(dir) => {
                    out.entry(dir.clone())
                        .or_default()
                        .insert(dependency.clone());
                }
                None if queued.insert(dependent.as_str()) => {
                    descriptors.extend(head.entries[dependent].keys.iter().map(String::as_str));
                    names.push(name_of(dependent));
                }
                None => {}
            }
        }
    }
    out.retain(|_, names| !names.is_empty());
    out
}

/// The `resolutions` of a `package.json`'s bytes, pattern to range; none when it does not parse.
pub(crate) fn overrides(manifest: &[u8]) -> BTreeMap<String, String> {
    let json = serde_json::from_slice::<serde_json::Value>(manifest).ok();
    let found = json
        .as_ref()
        .and_then(|j| j.get("resolutions")?.as_object());
    (found.into_iter().flatten())
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
        .collect()
}

/// Whether two entries of one locator read the same.
fn same(a: &Entry, b: &Entry) -> bool {
    a.version == b.version && a.checksum == b.checksum && a.dependencies == b.dependencies
}

/// Whether what `descriptor` resolves to changed from `base` to `head`: another locator, or one
/// whose entry reads otherwise. A key added to an entry that did not change is no change.
pub(crate) fn changed(base: &Lock, head: &Lock, descriptor: &str) -> bool {
    let Some((locator, entry)) = head.resolve(descriptor) else {
        return true;
    };
    match base.resolve(descriptor) {
        Some((before, _)) if before != locator => true,
        _ => !base.entries.get(locator).is_some_and(|b| same(b, entry)),
    }
}

/// What each dependency of each workspace resolves to, sorted by directory and name. A
/// dependency that matched no key gives a row only when its name has one entry.
pub(crate) fn direct(lock: &Lock) -> Vec<Resolved> {
    let mut out = BTreeSet::new();
    for (dir, locator) in &lock.workspaces {
        for (name, target) in lock.edges.get(locator).into_iter().flatten() {
            let found: Vec<&str> = match target {
                // Two locators (a parent-scoped override beside the written range): which one
                // this member gets is not certain, so no row.
                Target::Keys(keys) => {
                    let found: BTreeSet<&str> = (keys.iter())
                        .filter_map(|d| lock.descriptors.get(d))
                        .map(String::as_str)
                        .collect();
                    match found.len() {
                        1 => found.into_iter().collect(),
                        _ => Vec::new(),
                    }
                }
                Target::ByName => match lock.named.get(name).map(Vec::as_slice) {
                    Some([one]) => vec![one.as_str()],
                    _ => Vec::new(),
                },
            };
            for target in found {
                out.insert(Resolved {
                    dir: dir.clone(),
                    name: name.clone(),
                    origin: origin(target),
                    version: lock.entries[target].version.clone(),
                });
            }
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "# generated\n\n__metadata:\n  version: 8\n  cacheKey: 10c0\n\n";

    /// An entry: key, resolution, version and dependencies.
    type Row = (
        &'static str,
        &'static str,
        &'static str,
        &'static [(&'static str, &'static str)],
    );

    /// A lockfile of `__metadata` and `entries`, each `(key, resolution, version, deps)`.
    fn text(entries: &[Row]) -> String {
        let mut out = HEAD.to_string();
        for (key, resolution, version, deps) in entries {
            out += &format!("\"{key}\":\n  version: {version}\n  resolution: \"{resolution}\"\n");
            if !deps.is_empty() {
                out += "  dependencies:\n";
                for (n, r) in *deps {
                    out += &format!("    \"{n}\": \"{r}\"\n");
                }
            }
            out +=
                &format!("  checksum: 10c0/{version}\n  languageName: node\n  linkType: hard\n\n");
        }
        out
    }

    fn lock(entries: &[Row]) -> Lock {
        read(text(entries).as_bytes(), &BTreeMap::new()).unwrap()
    }

    const ROOT: Row = (
        "root@workspace:.",
        "root@workspace:.",
        "0.0.0-use.local",
        &[],
    );

    fn a(lodash: &'static str) -> Vec<Row> {
        vec![
            ROOT,
            (
                "@scope/a@workspace:^, @scope/a@workspace:packages/a",
                "@scope/a@workspace:packages/a",
                "0.0.0-use.local",
                &[("lodash", "npm:^4.17.0")],
            ),
            (
                "@scope/b@workspace:packages/b",
                "@scope/b@workspace:packages/b",
                "0.0.0-use.local",
                &[("@scope/a", "workspace:^"), ("@scope/kit", "npm:^2.0.0")],
            ),
            (
                "@scope/kit@npm:^2.0.0",
                "@scope/kit@npm:2.0.0",
                "2.0.0",
                &[("left-pad", "npm:^1")],
            ),
            ("left-pad@npm:^1", "left-pad@npm:1.3.0", "1.3.0", &[]),
            ("lodash@npm:^4.17.0", lodash_locator(lodash), lodash, &[]),
        ]
    }

    fn lodash_locator(version: &'static str) -> &'static str {
        match version {
            "4.17.20" => "lodash@npm:4.17.20",
            "4.17.21" => "lodash@npm:4.17.21",
            _ => unreachable!(),
        }
    }

    fn names(pairs: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        (pairs.iter())
            .map(|(dir, n)| (dir.to_string(), n.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    fn walk(base: &Lock, head: &Lock) -> BTreeMap<String, BTreeSet<String>> {
        reached(base, head, &diff(base, head))
    }

    #[test]
    fn a_yarn_1_file_is_format() {
        let v1 = "# yarn lockfile v1\n\nlodash@^4:\n  version \"4.17.21\"\n";
        assert_eq!(
            read(v1.as_bytes(), &BTreeMap::new()).unwrap_err(),
            Unread::Format
        );
    }

    #[test]
    fn over_the_cap_is_too_large() {
        let mut bytes = b"__metadata:\n".to_vec();
        bytes.resize(MAX_LOCK_BYTES + 1, b' ');
        assert_eq!(
            read(&bytes, &BTreeMap::new()).unwrap_err(),
            Unread::TooLarge
        );
    }

    #[test]
    fn garbage_is_unparsable() {
        assert_eq!(
            read(b"__metadata: [\n", &BTreeMap::new()).unwrap_err(),
            Unread::Unparsable
        );
        // An entry that is no map, and one with no resolution.
        let scalar = format!("{HEAD}\"x@npm:1\": 3\n");
        assert_eq!(
            read(scalar.as_bytes(), &BTreeMap::new()).unwrap_err(),
            Unread::Unparsable
        );
        let bare = format!("{HEAD}\"x@npm:1\":\n  version: 1.0.0\n");
        assert_eq!(
            read(bare.as_bytes(), &BTreeMap::new()).unwrap_err(),
            Unread::Unparsable
        );
    }

    #[test]
    fn a_direct_bump_moves_one_descriptor_and_reaches_its_member() {
        let (base, head) = (lock(&a("4.17.20")), lock(&a("4.17.21")));
        let moved = diff(&base, &head);
        assert_eq!(
            moved,
            [Moved {
                descriptor: "lodash@npm:^4.17.0".into(),
                before: Some("4.17.20".into()),
                after: Some("4.17.21".into()),
                by_name: false,
            }]
        );
        assert_eq!(
            reached(&base, &head, &moved),
            names(&[("packages/a", &["lodash"])])
        );
    }

    #[test]
    fn a_transitive_bump_reaches_through_the_direct_dependency() {
        let base = lock(&a("4.17.20"));
        let mut entries = a("4.17.20");
        entries[4] = ("left-pad@npm:^1", "left-pad@npm:1.3.1", "1.3.1", &[]);
        let head = lock(&entries);
        assert_eq!(
            walk(&base, &head),
            names(&[("packages/b", &["@scope/kit"])])
        );
    }

    #[test]
    fn a_workspace_reached_through_another_is_not_continued() {
        let walked = walk(&lock(&a("4.17.20")), &lock(&a("4.17.21")));
        assert!(!walked.contains_key("packages/b"), "{walked:?}");
    }

    #[test]
    fn a_framework_protocol_key_matches_after_normalizing() {
        let entries: Vec<Row> = vec![
            ROOT,
            (
                "@scope/a@workspace:packages/a",
                "@scope/a@workspace:packages/a",
                "0.0.0-use.local",
                &[("@x/core", "fw:^1.0.0")],
            ),
            (
                "@x/core@fw:^1.0.0::v=1.2.0&r=a",
                "@x/core@npm:1.2.0",
                "1.2.0",
                &[],
            ),
        ];
        let base = lock(&entries);
        let mut bumped = entries.clone();
        bumped[2] = (
            "@x/core@fw:^1.0.0::v=1.2.1&r=a",
            "@x/core@npm:1.2.1",
            "1.2.1",
            &[],
        );
        let head = lock(&bumped);
        assert!(head.by_name.is_empty());
        assert_eq!(walk(&base, &head), names(&[("packages/a", &["@x/core"])]));
        assert_eq!(
            direct(&head)[0],
            Resolved {
                dir: "packages/a".into(),
                name: "@x/core".into(),
                origin: "npm".into(),
                version: "1.2.1".into(),
            }
        );
    }

    #[test]
    fn a_range_without_protocol_gets_npm() {
        let entries: Vec<Row> = vec![
            ROOT,
            (
                "@scope/a@workspace:packages/a",
                "@scope/a@workspace:packages/a",
                "0.0.0-use.local",
                &[("lodash", "^4.17.0")],
            ),
            ("lodash@npm:^4.17.0", "lodash@npm:4.17.20", "4.17.20", &[]),
        ];
        let head = lock(&entries);
        assert!(head.by_name.is_empty());
        assert_eq!(
            head.edges["@scope/a@workspace:packages/a"]["lodash"],
            Target::Keys(vec!["lodash@npm:^4.17.0".into()])
        );
    }

    #[test]
    fn a_new_key_on_a_workspace_reaches_only_its_new_dependent() {
        let rows = |key: &'static str, deps: &'static [(&'static str, &'static str)]| -> Vec<Row> {
            vec![
                ROOT,
                (
                    key,
                    "@scope/a@workspace:packages/a",
                    "0.0.0-use.local",
                    &[("lodash", "npm:^4.17.0")],
                ),
                (
                    "@scope/b@workspace:packages/b",
                    "@scope/b@workspace:packages/b",
                    "0.0.0-use.local",
                    deps,
                ),
                ("lodash@npm:^4.17.0", "lodash@npm:4.17.21", "4.17.21", &[]),
            ]
        };
        let base = lock(&rows("@scope/a@workspace:packages/a", &[]));
        let head = lock(&rows(
            "@scope/a@workspace:^, @scope/a@workspace:packages/a",
            &[("@scope/a", "workspace:^")],
        ));
        assert_eq!(walk(&base, &head), names(&[("packages/b", &["@scope/a"])]));
    }

    #[test]
    fn a_new_range_of_an_existing_entry_reaches_only_its_new_dependent() {
        let rows = |key: &'static str, deps: &'static [(&'static str, &'static str)]| -> Vec<Row> {
            vec![
                ROOT,
                (
                    "@scope/a@workspace:packages/a",
                    "@scope/a@workspace:packages/a",
                    "0.0.0-use.local",
                    &[("lodash", "npm:^4.17.0")],
                ),
                (
                    "@scope/c@workspace:packages/c",
                    "@scope/c@workspace:packages/c",
                    "0.0.0-use.local",
                    deps,
                ),
                (key, "lodash@npm:4.17.21", "4.17.21", &[]),
            ]
        };
        let base = lock(&rows("lodash@npm:^4.17.0", &[]));
        let head = lock(&rows(
            "lodash@npm:^4.0.0, lodash@npm:^4.17.0",
            &[("lodash", "npm:^4.0.0")],
        ));
        assert_eq!(walk(&base, &head), names(&[("packages/c", &["lodash"])]));
        // The entry itself did not change: what a hoisted import gets stays the same.
        let moved = diff(&base, &head);
        let outside: Vec<&Moved> = (moved.iter())
            .filter(|m| !head.is_workspace(&m.descriptor))
            .collect();
        assert_eq!(outside.len(), 1, "{moved:?}");
        assert!(!changed(&base, &head, &outside[0].descriptor), "{moved:?}");
    }

    #[test]
    fn direct_gives_no_row_for_two_resolutions() {
        let rows: Vec<Row> = vec![
            ROOT,
            (
                "@scope/b@workspace:packages/b",
                "@scope/b@workspace:packages/b",
                "0.0.0-use.local",
                &[("left-pad", "npm:^1")],
            ),
            ("left-pad@npm:^1", "left-pad@npm:1.3.2", "1.3.2", &[]),
            ("left-pad@npm:1.3.0", "left-pad@npm:1.3.0", "1.3.0", &[]),
        ];
        // A parent-scoped override names `left-pad` too: both keys match the member's edge.
        let overrides = BTreeMap::from([("@scope/kit/left-pad".to_string(), "1.3.0".to_string())]);
        let lock = read(text(&rows).as_bytes(), &overrides).unwrap();
        assert!(
            direct(&lock).iter().all(|r| r.name != "left-pad"),
            "{:?}",
            direct(&lock)
        );
    }

    #[test]
    fn a_resolutions_override_redirects_the_range() {
        let entries = |checksum| -> Vec<Row> {
            vec![
                ROOT,
                (
                    "@scope/b@workspace:packages/b",
                    "@scope/b@workspace:packages/b",
                    "0.0.0-use.local",
                    &[("@scope/kit", "npm:^2.0.0")],
                ),
                (
                    "@scope/kit@npm:^2.0.0",
                    "@scope/kit@npm:2.0.0",
                    "2.0.0",
                    &[("left-pad", "^1")],
                ),
                ("left-pad@npm:1.3.0", "left-pad@npm:1.3.0", checksum, &[]),
            ]
        };
        let overrides = BTreeMap::from([("left-pad".to_string(), "1.3.0".to_string())]);
        let base = read(text(&entries("1.3.0")).as_bytes(), &overrides).unwrap();
        // The checksum is written from the version; a new one moves the entry.
        let head = read(text(&entries("1.3.0-r1")).as_bytes(), &overrides).unwrap();
        assert!(head.by_name.is_empty());
        assert_eq!(
            walk(&base, &head),
            names(&[("packages/b", &["@scope/kit"])])
        );
        assert_eq!(override_name("parent/left-pad"), "left-pad");
        assert_eq!(override_name("**/@x/y"), "@x/y");
        assert_eq!(override_name("@x/y@npm:1"), "@x/y");
    }

    #[test]
    fn an_unmatched_edge_goes_to_every_entry_of_that_name() {
        let entries = |v1| -> Vec<Row> {
            vec![
                ROOT,
                (
                    "@scope/a@workspace:packages/a",
                    "@scope/a@workspace:packages/a",
                    "0.0.0-use.local",
                    &[("ghost", "weird:1")],
                ),
                ("ghost@npm:^1", v1, "1.0.0", &[]),
                ("ghost@npm:^2", "ghost@npm:2.0.0", "2.0.0", &[]),
            ]
        };
        let base = lock(&entries("ghost@npm:1.0.0"));
        let head = lock(&entries("ghost@npm:1.0.1"));
        assert_eq!(
            head.edges["@scope/a@workspace:packages/a"]["ghost"],
            Target::ByName
        );
        let moved = diff(&base, &head);
        assert_eq!(moved.len(), 1);
        assert!(moved[0].by_name);
        assert_eq!(
            reached(&base, &head, &moved),
            names(&[("packages/a", &["ghost"])])
        );
    }

    #[test]
    fn a_patch_rehash_reaches_the_dependents() {
        let entries = |hash| -> Vec<Row> {
            vec![
                ROOT,
                (
                    "@scope/a@workspace:packages/a",
                    "@scope/a@workspace:packages/a",
                    "0.0.0-use.local",
                    &[("resolve", "npm:^1.20.0")],
                ),
                ("resolve@npm:^1.20.0", "resolve@npm:1.22.8", "1.22.8", &[]),
                (
                    "resolve@patch:resolve@npm%3A^1.20.0#~builtin<compat/resolve>",
                    hash,
                    "1.22.8",
                    &[],
                ),
            ]
        };
        let base = lock(&entries(
            "resolve@patch:resolve@npm%3A1.22.8#~builtin<compat/resolve>::version=1.22.8&hash=c3c19d",
        ));
        let head = lock(&entries(
            "resolve@patch:resolve@npm%3A1.22.8#~builtin<compat/resolve>::version=1.22.8&hash=07638b",
        ));
        let moved = diff(&base, &head);
        let descriptors: Vec<&str> = moved.iter().map(|m| m.descriptor.as_str()).collect();
        assert_eq!(
            descriptors,
            [
                "resolve@npm:^1.20.0",
                "resolve@patch:resolve@npm%3A^1.20.0#~builtin<compat/resolve>"
            ]
        );
        assert_eq!(
            reached(&base, &head, &moved),
            names(&[("packages/a", &["resolve"])])
        );
    }

    #[test]
    fn a_member_using_an_existing_descriptor_moves() {
        let mut entries = a("4.17.20");
        entries.push((
            "@scope/c@workspace:packages/c",
            "@scope/c@workspace:packages/c",
            "0.0.0-use.local",
            &[],
        ));
        let base = lock(&entries);
        let last = entries.len() - 1;
        entries[last].3 = &[("lodash", "npm:^4.17.0")];
        let head = lock(&entries);
        assert_eq!(walk(&base, &head), names(&[("packages/c", &["lodash"])]));
    }

    #[test]
    fn peers_are_not_edges() {
        let ui = |react: &'static str| {
            format!(
                "{}\"@scope/ui@npm:^1\":\n  version: 1.0.0\n  resolution: \"@scope/ui@npm:1.0.0\"\n  \
                 peerDependencies:\n    react: \"*\"\n  checksum: 10c0/ui\n\n{}",
                text(&[
                    ROOT,
                    (
                        "@scope/a@workspace:packages/a",
                        "@scope/a@workspace:packages/a",
                        "0.0.0-use.local",
                        &[("@scope/ui", "npm:^1")],
                    ),
                ]),
                text(&[("react@npm:^18", react, "18.0.0", &[])]).replace(HEAD, ""),
            )
        };
        let base = read(ui("react@npm:18.0.0").as_bytes(), &BTreeMap::new()).unwrap();
        let head = read(ui("react@npm:18.0.1").as_bytes(), &BTreeMap::new()).unwrap();
        assert_eq!(diff(&base, &head).len(), 1);
        assert!(walk(&base, &head).is_empty());
    }

    #[test]
    fn an_alias_is_reached_by_its_key() {
        let entries = |v| -> Vec<Row> {
            vec![
                ROOT,
                (
                    "@scope/a@workspace:packages/a",
                    "@scope/a@workspace:packages/a",
                    "0.0.0-use.local",
                    &[("util", "npm:lodash@^4")],
                ),
                ("util@npm:lodash@^4", lodash_locator(v), v, &[]),
            ]
        };
        let walked = walk(&lock(&entries("4.17.20")), &lock(&entries("4.17.21")));
        assert_eq!(walked, names(&[("packages/a", &["util"])]));
    }

    #[test]
    fn direct_lists_each_member_dependency_with_origin_and_version() {
        let mut entries = a("4.17.21");
        entries[2].3 = &[
            ("@scope/a", "workspace:^"),
            (
                "resolve",
                "patch:resolve@npm%3A^1.20.0#~builtin<compat/resolve>",
            ),
        ];
        entries.push((
            "resolve@patch:resolve@npm%3A^1.20.0#~builtin<compat/resolve>",
            "resolve@patch:resolve@npm%3A1.22.8#~builtin<compat/resolve>::version=1.22.8&hash=c3c19d",
            "1.22.8",
            &[],
        ));
        let rows: Vec<(String, String, String, String)> = (direct(&lock(&entries)).into_iter())
            .map(|r| (r.dir, r.name, r.origin, r.version))
            .collect();
        let expected = [
            ("packages/a", "lodash", "npm", "4.17.21"),
            ("packages/b", "@scope/a", "workspace", "0.0.0-use.local"),
            ("packages/b", "resolve", "patch", "1.22.8"),
        ]
        .map(|(d, n, o, v)| (d.into(), n.into(), o.into(), v.into()));
        assert_eq!(rows, expected);
    }

    #[test]
    fn workspaces_are_read_by_directory() {
        let head = lock(&a("4.17.20"));
        let dirs: Vec<&str> = head.workspaces.keys().map(String::as_str).collect();
        assert_eq!(dirs, [".", "packages/a", "packages/b"]);
        for kind in [
            Unread::NoBase,
            Unread::Deleted,
            Unread::TooLarge,
            Unread::Format,
            Unread::Unparsable,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.name()));
        }
        assert_eq!(
            serde_json::to_string(&Unread::TooLarge).unwrap(),
            "\"too-large\""
        );
    }
}
