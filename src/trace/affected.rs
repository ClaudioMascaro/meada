use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet};

use serde::Serialize;

use super::{Direction, Graph, calls_on_link, held, runs};
use crate::contract::{self, Level, Link};
use crate::index::{CallSite, Export};
use crate::lang::{self, Facts};
use crate::package::lock::{self, Unread};
use crate::package::{self, Manifest, npm};
use crate::render::Files;
use crate::repo::{self, Change};

/// The answer of `meada trace --affected`, every list sorted.
#[derive(Serialize, Debug, Default)]
pub struct Affected {
    changed: Vec<Changed>,
    open: Vec<Open>,
    files: Vec<File>,
    reads: Vec<Read>,
    packages: Packages,
    iac: Vec<Reached>,
    sides: Vec<Side>,
    lockfiles: Vec<Lockfile>,
}

#[derive(Serialize, Debug)]
pub(crate) struct Lockfile {
    repo: String,
    path: String,
    #[serde(flatten)]
    state: LockState,
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
enum LockState {
    Read {
        entries: Vec<lock::Moved>,
        packages: Vec<LockPackage>,
        manifests: Vec<String>,
    },
    Unread {
        unread: Unread,
    },
}

#[derive(Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LockPackage {
    package: String,
    via: Vec<String>,
    whole: bool,
}

#[derive(Default, Debug)]
pub(crate) struct Attributed {
    lockfiles: Vec<Lockfile>,
    pub(crate) files: BTreeMap<String, String>,
    all_packages: BTreeSet<String>,
    covered: BTreeSet<String>,
    reads: BTreeSet<(String, String)>,
}

impl Attributed {
    pub(crate) fn as_changes(&self) -> Vec<(String, Change)> {
        (self.files.keys())
            .map(|p| (p.clone(), Change::Untracked))
            .collect()
    }

    fn lockfile(&self, path: &str) -> Option<&Lockfile> {
        self.lockfiles.iter().find(|l| l.path == path)
    }
}

// ponytail: every package, unnarrowed (a `tsconfig.json` by its `include`, `.yarn/patches` by
// the lockfile's `patch:` entries); see `docs/agents/ceilings.md`.
fn root_global(folder: &str, path: &str) -> bool {
    let rel = match folder {
        "." => path,
        _ => match path.strip_prefix(folder).and_then(|r| r.strip_prefix('/')) {
            Some(rel) => rel,
            None => return false,
        },
    };
    matches!(
        rel,
        "package.json" | ".yarnrc.yml" | "tsconfig.json" | "backstage.json"
    ) || rel.starts_with(".yarn/")
}

#[derive(Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Changed {
    repo: String,
    path: String,
    change: &'static str,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Open {
    repo: String,
    path: String,
    reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
struct File {
    repo: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<String>,
    test: bool,
    tier: Tier,
    level: Level,
    #[serde(skip_serializing_if = "Option::is_none")]
    by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidates: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    narrowed: bool,
    manifest: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lockfile: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
struct Read {
    repo: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    tier: Tier,
    level: Level,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidates: Option<usize>,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Tier {
    Certain,
    Inferred,
    Model,
    Ambiguous,
}

impl Tier {
    pub(crate) fn of(evidence: &Evidence) -> Self {
        match evidence.level {
            Level::Exact | Level::Declared => Tier::Certain,
            Level::Decided if evidence.by == Some("human") => Tier::Certain,
            Level::Inferred => Tier::Inferred,
            Level::Decided => Tier::Model,
            Level::Ambiguous => Tier::Ambiguous,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key {
    pub(crate) tier: Tier,
    pub(crate) candidates: usize,
}

impl Key {
    const ROOT: Key = Key {
        tier: Tier::Certain,
        candidates: 0,
    };

    fn of(evidence: &Evidence) -> Self {
        let tier = Tier::of(evidence);
        let candidates = if tier == Tier::Ambiguous {
            evidence.candidates
        } else {
            0
        };
        Key { tier, candidates }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Settled<'g> {
    pub(crate) key: Key,
    pub(crate) evidence: Option<Evidence<'g>>,
    pub(crate) ends: bool,
}

#[derive(Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Reached {
    id: String,
    level: Level,
}

#[derive(Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Side {
    id: String,
    place: &'static str,
    level: Level,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Evidence<'g> {
    pub(crate) level: Level,
    pub(crate) by: Option<&'g str>,
    pub(crate) p: Option<f64>,
    pub(crate) candidates: usize,
    pub(crate) reason: Option<&'g str>,
    pub(crate) narrowed: bool,
}

impl<'g> Evidence<'g> {
    fn call(c: &'g CallSite) -> Self {
        Evidence {
            level: c.level.unwrap_or(Level::Ambiguous),
            by: None,
            p: None,
            candidates: 0,
            reason: None,
            narrowed: c.narrowed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dependent<'g> {
    pub(crate) to: &'g str,
    pub(crate) evidence: Evidence<'g>,
    pub(crate) ends: bool,
}

impl<'a> Graph<'a> {
    pub(crate) fn dependents(&self, id: &str) -> Vec<Dependent<'_>> {
        let mut out = Vec::new();
        let callers = self.callers.get(id).map_or(&[][..], Vec::as_slice);
        let targets = |c: &CallSite| c.target.as_deref() == Some(id);
        for &(_, c) in callers.iter().filter(|(_, c)| targets(c)) {
            out.push(Dependent {
                to: held(c),
                evidence: Evidence::call(c),
                ends: false,
            });
        }
        for &(_, c) in callers.iter().filter(|(_, c)| !targets(c)) {
            if c.candidates.iter().any(|t| t == id) {
                let evidence = Evidence {
                    level: Level::Ambiguous,
                    candidates: c.candidates.len(),
                    ..Evidence::call(c)
                };
                out.push(Dependent {
                    to: held(c),
                    evidence,
                    ends: false,
                });
            }
        }
        for &(_, c) in callers.iter().filter(|(_, c)| targets(c)) {
            if let Some(decorated) = &c.decorates {
                out.push(Dependent {
                    to: decorated,
                    evidence: Evidence::call(c),
                    ends: false,
                });
            }
        }
        let into = self.into.get(id).map_or(&[][..], Vec::as_slice);
        for &link in into {
            if let Some(&(_, c)) = self.calls.get(link.consumer.as_str()) {
                out.push(Dependent {
                    to: held(c),
                    evidence: self.evidence(link),
                    ends: false,
                });
            }
        }
        for &link in into {
            let method = link.consumer.as_str();
            if !self.calls.contains_key(method)
                && self.symbols.contains_key(method)
                && calls_on_link(link)
            {
                out.push(Dependent {
                    to: method,
                    evidence: self.evidence(link),
                    ends: false,
                });
            }
        }
        let made = (self.callees.get(id).into_iter().flatten()).map(|&(_, c)| c.id.as_str());
        for provider in std::iter::once(id).chain(made) {
            let written = self.into.get(provider).into_iter().flatten();
            for &link in written
                .filter(|l| !calls_on_link(l) && contract::flows_to_consumer(Some(&l.contract)))
            {
                let to = match self.calls.get(link.consumer.as_str()) {
                    Some(_) if provider == id => continue,
                    Some(&(_, reader)) => held(reader),
                    None => link.consumer.as_str(),
                };
                out.push(Dependent {
                    to,
                    evidence: self.evidence(link),
                    ends: false,
                });
            }
        }
        if let Some((n, path)) = self.path_of(id) {
            let repository = &self.exports[n].repository;
            for x in (self.executions.iter()).filter(|x| {
                x.repository == *repository && runs(x, &self.exports[n].manifests, path)
            }) {
                let evidence = Evidence {
                    level: x.level,
                    by: None,
                    p: None,
                    candidates: 0,
                    reason: Some(&x.reason),
                    narrowed: false,
                };
                out.push(Dependent {
                    to: &x.workload,
                    evidence,
                    ends: true,
                });
            }
        }
        if self.symbols.contains_key(id) {
            for &link in into.iter().filter(|l| calls_on_link(l)) {
                let Some(&(n, step)) = self.iac.get(link.consumer.as_str()) else {
                    continue;
                };
                let mut held = Vec::new();
                self.holders(n, step, &mut HashSet::new(), &mut held);
                for to in held {
                    out.push(Dependent {
                        to,
                        evidence: self.evidence(link),
                        ends: true,
                    });
                }
            }
        }
        out
    }

    fn evidence<'g>(&'g self, link: &'g Link) -> Evidence<'g> {
        let candidates = match link.level {
            Level::Ambiguous => (self.out.get(link.consumer.as_str()).into_iter().flatten())
                .filter(|l| l.contract == link.contract)
                .count(),
            _ => 0,
        };
        Evidence {
            level: link.level,
            by: link.by.as_deref(),
            p: link.p,
            candidates,
            reason: Some(&link.reason),
            narrowed: false,
        }
    }

    fn path_of<'g>(&'g self, id: &'g str) -> Option<(usize, &'g str)> {
        if let Some(&(n, s)) = self.symbols.get(id) {
            return Some((n, &s.span.path));
        }
        self.workspace.repos.iter().enumerate().find_map(|(n, m)| {
            let path = id.strip_prefix(&m.identity)?.strip_prefix("::")?;
            (!path.contains("::") && !path.contains('#')).then_some((n, path))
        })
    }

    fn holders<'g>(
        &'g self,
        n: usize,
        node: &crate::iac::Node,
        seen: &mut HashSet<&'g str>,
        out: &mut Vec<&'g str>,
    ) {
        let export = &self.exports[n];
        let identity = node.identity(&export.repository);
        let Some((key, _)) = self.iac.get_key_value(&identity) else {
            return;
        };
        if !seen.insert(key) {
            return;
        }
        out.push(key);
        for (m, caller, _) in self.node_calls(n, node, Direction::Up) {
            self.holders(m, &caller, seen, out);
        }
        for r in (export.iac_references.iter())
            .filter(|r| r.root == node.root && r.target == node.address)
        {
            let holds = |h: &&crate::iac::Node| h.root == r.root && h.address == r.source;
            if let Some(holder) = export.iac.iter().find(holds) {
                self.holders(n, holder, seen, out);
            }
        }
    }
}

#[derive(Serialize, Debug, Default)]
struct Packages {
    certain: Vec<String>,
    inferred: Vec<String>,
    manifest_only: Vec<String>,
}

struct Owners<'g> {
    graph: &'g Graph<'g>,
    workspaces: HashMap<(usize, &'g str), bool>,
}

impl<'g> Owners<'g> {
    fn new(graph: &'g Graph<'g>) -> Self {
        Self {
            graph,
            workspaces: HashMap::new(),
        }
    }

    fn is_workspace(&mut self, n: usize, path: &'g str) -> bool {
        let root = &self.graph.workspace.repos[n].root;
        *self.workspaces.entry((n, path)).or_insert_with(|| {
            let json = (path.rsplit('/').next() == Some("package.json"))
                .then(|| std::fs::read(root.join(path)).ok())
                .flatten()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            let members = json.as_ref().and_then(|j| j.get("workspaces"));
            members.is_some_and(|w| w.is_array() || w.get("packages").is_some_and(|p| p.is_array()))
        })
    }

    fn owner(&mut self, n: usize, path: &str) -> Option<(&'g Manifest, bool)> {
        let manifest = package::owner_of(&self.graph.exports[n].manifests, path)?;
        Some((manifest, self.is_workspace(n, &manifest.path)))
    }

    fn package(&mut self, n: usize, path: &str) -> Option<String> {
        match self.owner(n, path)? {
            (_, true) => None,
            (manifest, false) => Some(directory(&manifest.path).to_string()),
        }
    }
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

fn manifest_bytes(abs: &std::path::Path) -> Option<Vec<u8>> {
    repo::regular(abs, lang::MAX_BYTES).filter(|b| b.len() <= lang::MAX_BYTES)
}

fn resolutions(path: &std::path::Path) -> BTreeMap<String, String> {
    manifest_bytes(path).map_or_else(BTreeMap::new, |b| lock::overrides(&b))
}

fn lockfile_reads(
    graph: &Graph<'_>,
    changes: &[(String, Change)],
    facts_at: impl FnOnce(&[&str]) -> crate::Result<BTreeMap<String, Facts>>,
) -> crate::Result<BTreeSet<(String, String)>> {
    let repo = &graph.workspace.repos[0];
    let provided: HashSet<&str> = (graph.exports.iter())
        .flat_map(|e| &e.packages)
        .map(|p| p.name.as_str())
        .collect();
    let paths: Vec<&str> = (changes.iter())
        .filter(|(_, c)| !matches!(c, Change::Deleted))
        .map(|(p, _)| p.as_str())
        .collect();
    let facts = facts_at(&paths)?;
    let mut out = BTreeSet::new();
    for (path, change) in changes {
        let Some(facts) = facts.get(path) else {
            continue;
        };
        let text = std::fs::read_to_string(repo.root.join(path)).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let written = |spec: &str| match change {
            Change::Lines(ranges) => ranges.iter().any(|&(a, b)| {
                let at = lines
                    .get(a.saturating_sub(1)..b.min(lines.len()))
                    .unwrap_or_default();
                at.iter().any(|l| {
                    ['\'', '"', '`']
                        .iter()
                        .any(|q| l.contains(&format!("{q}{spec}{q}")))
                })
            }),
            _ => true,
        };
        for spec in lang::loaded(facts) {
            let Some((name, _)) = npm::split(spec) else {
                continue;
            };
            if provided.contains(name) || !written(spec) {
                continue;
            }
            let lockfile = std::iter::successors(Some(path.as_str()), |p| {
                (!p.is_empty()).then(|| p.rsplit_once('/').map_or("", |(dir, _)| dir))
            })
            .skip(1)
            .map(|dir| repo::relative(dir, "yarn.lock").unwrap_or_default())
            .find(|lock| repo.root.join(lock).is_file());
            if let Some(lockfile) = lockfile {
                out.insert((lockfile, name.to_string()));
            }
        }
    }
    Ok(out)
}

fn loaded_names<'n>(facts: &Facts, names: &BTreeSet<&'n str>) -> BTreeSet<&'n str> {
    (lang::loaded(facts).into_iter())
        .filter_map(|s| names.get(npm::split(s)?.0).copied())
        .collect()
}

const DEPENDENCY_FIELDS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

fn covered(root: &std::path::Path, base: Option<&str>, path: &str, via: &BTreeSet<String>) -> bool {
    let json = |bytes: Option<Vec<u8>>| {
        bytes.and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    };
    let before = json(base.and_then(|rev| repo::show(root, rev, path).ok().flatten()));
    let after = json(manifest_bytes(&root.join(path)));
    let (Some(serde_json::Value::Object(before)), Some(serde_json::Value::Object(after))) =
        (before, after)
    else {
        return false;
    };
    let keys: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    keys.into_iter().all(|key| {
        let (b, a) = (before.get(key), after.get(key));
        if b == a {
            return true;
        }
        if !DEPENDENCY_FIELDS.contains(&key.as_str()) {
            return false;
        }
        let table = |v: Option<&'_ serde_json::Value>| -> Option<serde_json::Map<_, _>> {
            match v {
                None => Some(serde_json::Map::new()),
                Some(v) => v.as_object().cloned(),
            }
        };
        let (Some(b), Some(a)) = (table(b), table(a)) else {
            return false;
        };
        let names: BTreeSet<&String> = b.keys().chain(a.keys()).collect();
        (names.into_iter()).all(|n| b.get(n) == a.get(n) || via.contains(n))
    })
}

pub(crate) fn attribute<'g>(
    graph: &'g Graph<'g>,
    changes: &[(String, Change)],
    base: Option<&str>,
    mut facts: impl FnMut(&[&str]) -> crate::Result<BTreeMap<String, Facts>>,
    facts_at: impl FnOnce(&[&str]) -> crate::Result<BTreeMap<String, Facts>>,
) -> crate::Result<Attributed> {
    let repo = &graph.workspace.repos[0];
    let export = &graph.exports[0];
    let mut owners = Owners::new(graph);
    let mut out = Attributed {
        reads: lockfile_reads(graph, changes, facts_at)?,
        ..Attributed::default()
    };
    let mut dirs: Option<Vec<(&str, &str)>> = None;
    for (path, change) in changes {
        if path.rsplit('/').next() != Some("yarn.lock") {
            continue;
        }
        if matches!(change, Change::Deleted)
            && (changes.iter()).any(|(p, c)| p == path && !matches!(c, Change::Deleted))
        {
            continue;
        }
        let folder = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let manifest = repo::relative(folder, "package.json").unwrap_or_default();
        let at_root = (export.manifests.iter())
            .find(|m| m.path == manifest)
            .is_some_and(|m| owners.is_workspace(0, &m.path));
        let overrides = resolutions(&repo.root.join(&manifest));
        let head = || {
            let abs = repo.root.join(path);
            match repo::regular(&abs, lock::MAX_LOCK_BYTES) {
                Some(bytes) => lock::read(&bytes, &overrides),
                None if abs.symlink_metadata().is_ok() => Err(Unread::Unparsable),
                None => Err(Unread::Deleted),
            }
        };
        let read = match (change, base) {
            (Change::Deleted, _) => Err(Unread::Deleted),
            (_, None) => Err(Unread::NoBase),
            (_, Some(rev)) => match repo::show(&repo.root, rev, path) {
                Ok(Some(bytes)) => lock::read(&bytes, &overrides).and_then(|b| Ok((b, head()?))),
                _ => Err(Unread::NoBase),
            },
        };
        let (before, after) = match read {
            Ok(read) => read,
            Err(unread) => {
                if at_root {
                    out.lockfiles.push(Lockfile {
                        repo: repo.identity.clone(),
                        path: path.clone(),
                        state: LockState::Unread { unread },
                    });
                }
                continue;
            }
        };
        let moved = lock::diff(&before, &after);
        let reached = lock::reached(&before, &after, &moved);
        let outside: Vec<lock::Moved> = (moved.iter())
            .filter(|m| !after.is_workspace(&m.descriptor))
            .filter(|m| m.patched || lock::changed(&before, &after, &m.descriptor))
            .cloned()
            .collect();
        let hoisted: BTreeSet<String> = (lock::reached(&before, &after, &outside)
            .into_values()
            .flatten())
        .collect();
        let names: BTreeSet<&str> = reached.values().flatten().map(String::as_str).collect();
        let loaded = facts(&names.iter().copied().collect::<Vec<_>>())?;
        let inside = |p: &str| folder.is_empty() || p.starts_with(&format!("{folder}/"));
        let loads: BTreeMap<&str, BTreeSet<&str>> = (loaded.iter())
            .filter(|(p, _)| inside(p))
            .map(|(p, f)| (p.as_str(), loaded_names(f, &names)))
            .filter(|(_, n)| !n.is_empty())
            .collect();
        let dirs = dirs.get_or_insert_with(|| {
            (export.files.iter())
                .filter_map(|f| {
                    let manifest = package::owner_of(&export.manifests, &f.path)?;
                    Some((f.path.as_str(), directory(&manifest.path)))
                })
                .collect()
        });
        let mut packages = Vec::new();
        for (dir, via) in &reached {
            let Some(package) = repo::relative(folder, dir).filter(|p| !p.starts_with("..")) else {
                continue;
            };
            let package = if package.is_empty() { "." } else { &package };
            let owned: Vec<&str> = (dirs.iter())
                .filter(|(_, d)| *d == package)
                .map(|(p, _)| *p)
                .collect();
            let loading: Vec<&str> = (owned.iter().copied())
                .filter(|p| (loads.get(p)).is_some_and(|l| l.iter().any(|n| via.contains(*n))))
                .collect();
            let whole = loading.is_empty();
            for file in if whole { owned } else { loading } {
                out.files
                    .entry(file.to_string())
                    .or_insert_with(|| path.clone());
            }
            packages.push(LockPackage {
                package: package.to_string(),
                via: via.iter().cloned().collect(),
                whole,
            });
        }
        for (file, loading) in &loads {
            let Some(manifest) = package::owner_of(&export.manifests, file) else {
                continue;
            };
            let entries = &manifest.summary.dependencies;
            if (loading.iter())
                .any(|n| hoisted.contains(*n) && !entries.iter().any(|e| e.name == *n))
            {
                out.files
                    .entry(file.to_string())
                    .or_insert_with(|| path.clone());
            }
        }
        let loaded_anywhere: BTreeSet<&str> = loads.values().flatten().copied().collect();
        if (reached.get(".").into_iter().flatten()).any(|n| !loaded_anywhere.contains(n.as_str())) {
            out.all_packages.insert(path.clone());
        }
        let mut manifests = Vec::new();
        for (member, change) in changes {
            let Some(dir) = member.strip_suffix("package.json") else {
                continue;
            };
            let dir = dir.trim_end_matches('/');
            let at = if folder.is_empty() {
                dir
            } else if let Some(member) = dir.strip_prefix(folder).and_then(|d| d.strip_prefix('/'))
            {
                member
            } else {
                continue;
            };
            let via = reached.get(at).filter(|_| !at.is_empty() && at != "..");
            if let Some(via) = via
                && !matches!(change, Change::Deleted)
                && covered(&repo.root, base, member, via)
            {
                out.covered.insert(member.clone());
                manifests.push(member.clone());
            }
        }
        out.lockfiles.push(Lockfile {
            repo: repo.identity.clone(),
            path: path.clone(),
            state: LockState::Read {
                entries: moved,
                packages,
                manifests,
            },
        });
    }
    out.lockfiles
        .sort_by(|a, b| (&a.repo, &a.path).cmp(&(&b.repo, &b.path)));
    Ok(out)
}

pub(crate) fn roots<'g>(
    graph: &'g Graph<'g>,
    changes: &[(String, Change)],
    attributed: &Attributed,
) -> (Vec<String>, Vec<Open>) {
    let export = &graph.exports[0];
    let repo = &graph.workspace.repos[0];
    let mut files = Files::new(&repo.root);
    let mut owners = Owners::new(graph);
    let mut symbols: HashMap<&str, Vec<&crate::index::Symbol>> = HashMap::new();
    for &(n, s) in graph.symbols.values() {
        if n == 0 {
            symbols.entry(&s.span.path).or_default().push(s);
        }
    }
    let mut roots = BTreeSet::new();
    let mut open = Vec::new();
    let changed: HashSet<&str> = changes.iter().map(|(p, _)| p.as_str()).collect();
    let whole = Change::Untracked;
    let extra = (attributed.files.keys())
        .filter(|p| !changed.contains(p.as_str()))
        .map(|p| (p, &whole, true));
    let all = (changes.iter().map(|(p, c)| (p, c, false))).chain(extra);
    for (path, change, rooted_by_lockfile) in all {
        let file = format!("{}::{path}", repo.identity);
        let touched = |first: usize, last: usize| match change {
            Change::Untracked => true,
            Change::Deleted => false,
            Change::Lines(ranges) => ranges.iter().any(|&(a, b)| a <= last && first <= b),
        };
        let mut rooted = false;
        let mut iac = false;
        let spans: Vec<(&str, Option<(usize, usize)>)> = (symbols.get(path.as_str()))
            .into_iter()
            .flatten()
            .map(|s| {
                let first = files.line(path, s.span.start);
                let last = files.line(path, (s.span.end - 1).max(s.span.start));
                (s.id.as_str(), first.zip(last))
            })
            .collect();
        let uncovered = match change {
            Change::Untracked => true,
            Change::Deleted => false,
            Change::Lines(ranges) => ranges.iter().any(|&(a, b)| {
                (a..=b).any(|line| {
                    !spans
                        .iter()
                        .any(|(_, s)| s.is_some_and(|(f, l)| f <= line && line <= l))
                })
            }),
        };
        let code = !spans.is_empty()
            || graph.callees.contains_key(file.as_str())
            || graph.out.contains_key(file.as_str())
            || graph.into.contains_key(file.as_str());
        for &(id, span) in &spans {
            if uncovered || span.is_some_and(|(f, l)| touched(f, l)) {
                roots.insert(id.to_string());
                rooted = true;
            }
        }
        if uncovered && code {
            roots.insert(file.clone());
            rooted = true;
        }
        for site in export.config_sites.iter().filter(|s| s.path == *path) {
            if touched(site.line, site.line) {
                roots.insert(site.id.clone());
                rooted = true;
            }
        }
        for node in export.iac.iter().filter(|n| n.path == *path) {
            if touched(node.line, node.end_line) {
                roots.insert(node.identity(&export.repository));
                (rooted, iac) = (true, true);
            }
        }
        let prefix = format!("{}::", repo.identity);
        for (&side, &document) in &graph.documented {
            if document == path && side.starts_with(&prefix) && !matches!(change, Change::Deleted) {
                roots.insert(side.to_string());
                rooted = true;
            }
        }
        if rooted_by_lockfile {
            continue;
        }
        let owner = owners.owner(0, path);
        let global = match attributed.lockfile(path) {
            Some(l) => {
                matches!(l.state, LockState::Unread { .. })
                    || attributed.all_packages.contains(path)
            }
            None => owner.is_some_and(|(m, root)| root && root_global(directory(&m.path), path)),
        };
        let reason = match () {
            _ if global => Some("all-packages"),
            _ if attributed.lockfile(path).is_some() || attributed.covered.contains(path) => None,
            _ if matches!(change, Change::Deleted) => Some("deleted"),
            _ if iac => Some("iac"),
            _ if owner.is_some_and(|(m, root)| m.path == *path && !root) => Some("manifest"),
            _ if owner.is_some_and(|(_, root)| root) => Some("root"),
            _ if rooted => None,
            _ => Some("no-symbols"),
        };
        if let Some(reason) = reason {
            open.push(Open {
                repo: repo.identity.clone(),
                path: path.clone(),
                reason,
                package: (!global).then(|| owners.package(0, path)).flatten(),
            });
        }
    }
    open.sort();
    (roots.into_iter().collect(), open)
}

pub(crate) fn answer<'g>(
    graph: &'g Graph<'g>,
    changes: &[(String, Change)],
    roots: &[String],
    open: Vec<Open>,
    attributed: Attributed,
) -> Affected {
    let repo = &graph.workspace.repos[0].identity;
    let mut owners = Owners::new(graph);
    let mut changed: Vec<Changed> = (changes.iter())
        .map(|(path, change)| Changed {
            repo: repo.clone(),
            path: path.clone(),
            change: match change {
                Change::Lines(_) => "lines",
                Change::Untracked => "whole",
                Change::Deleted => "deleted",
            },
        })
        .collect();
    changed.sort();
    let mut files: BTreeMap<(usize, &str), (Key, &str, Option<Evidence>)> = BTreeMap::new();
    let touched =
        (changed.iter().map(|c| c.path.as_str())).chain(open.iter().map(|o| o.path.as_str()));
    for path in touched.chain(attributed.files.keys().map(String::as_str)) {
        files.insert((0, path), (Key::ROOT, "", None));
    }
    let mut iac = BTreeMap::new();
    let mut sides = Vec::new();
    for (id, node) in graph.fan(roots) {
        if node.ends {
            iac.insert(id, node.evidence);
        }
        if graph.ends(id) {
            let place = match () {
                _ if graph.is_repo(id) => "repository",
                _ if graph.documented.contains_key(id) => "document",
                _ => "config",
            };
            let level = node.evidence.map_or(Level::Exact, |e| e.level);
            let id = id.to_string();
            sides.push(Side { id, place, level });
        }
        let Some(place) = graph.file_of(id) else {
            continue;
        };
        let entry = (node.key, id, node.evidence);
        let best = files.entry(place).or_insert(entry);
        if (entry.0, entry.1) < (best.0, best.1) {
            *best = entry;
        }
    }
    let seeds = (changed.iter().map(|c| c.path.as_str()))
        .chain(open.iter().map(|o| o.path.as_str()))
        .chain(attributed.files.keys().map(String::as_str))
        .filter_map(|path| owners.package(0, path))
        .collect();
    let closure = closure(&graph.exports[0], seeds);
    let files: Vec<File> = (files.into_iter())
        .map(|((n, path), (key, _, evidence))| {
            let root = Evidence {
                level: Level::Exact,
                by: None,
                p: None,
                candidates: 0,
                reason: None,
                narrowed: false,
            };
            let e = evidence.unwrap_or(root);
            let package = owners.package(n, path);
            File {
                repo: graph.workspace.repos[n].identity.clone(),
                path: path.to_string(),
                manifest: n == 0 && package.as_ref().is_some_and(|p| closure.contains(p)),
                package,
                test: crate::http::is_test_file(path),
                tier: key.tier,
                level: e.level,
                by: e.by.map(str::to_string),
                p: e.p,
                candidates: (e.candidates > 0).then_some(e.candidates),
                reason: e.reason.map(str::to_string),
                narrowed: e.narrowed,
                lockfile: (n == 0)
                    .then(|| attributed.files.get(path).cloned())
                    .flatten(),
            }
        })
        .collect();
    let iac = (iac.into_iter())
        .filter_map(|(id, e)| {
            Some(Reached {
                id: id.to_string(),
                level: e?.level,
            })
        })
        .collect();
    let (mut certain, mut weaker) = (BTreeSet::new(), BTreeSet::new());
    for f in files.iter().filter(|f| f.repo == *repo) {
        let tier = if f.tier == Tier::Certain {
            &mut certain
        } else {
            &mut weaker
        };
        tier.extend(f.package.clone());
    }
    let packages = Packages {
        inferred: weaker.difference(&certain).cloned().collect(),
        manifest_only: closure.difference(&certain).cloned().collect(),
        certain: certain.into_iter().collect(),
    };
    sides.sort();
    let mut read: BTreeMap<(usize, &str, Option<&str>), (Key, Evidence)> = BTreeMap::new();
    for (id, evidence) in reads(graph, changes, roots) {
        let document = || {
            let path = *graph.documented.get(id)?;
            let mut repos = graph.workspace.repos.iter();
            let n = repos.position(|m| {
                id.strip_prefix(&m.identity)
                    .is_some_and(|rest| rest.starts_with("::"))
            })?;
            Some((n, path))
        };
        let Some((n, path)) = graph.file_of(id).or_else(document) else {
            continue;
        };
        let key = Key::of(&evidence);
        let best = read.entry((n, path, None)).or_insert((key, evidence));
        if key < best.0 {
            *best = (key, evidence);
        }
    }
    let exact = Evidence {
        level: Level::Exact,
        by: None,
        p: None,
        candidates: 0,
        reason: None,
        narrowed: false,
    };
    for (lockfile, name) in &attributed.reads {
        read.insert((0, lockfile, Some(name)), (Key::ROOT, exact));
    }
    let reads = (read.into_iter())
        .map(|((n, path, name), (key, e))| Read {
            repo: graph.workspace.repos[n].identity.clone(),
            path: path.to_string(),
            name: name.map(str::to_string),
            tier: key.tier,
            level: e.level,
            candidates: (e.candidates > 0).then_some(e.candidates),
        })
        .collect();
    Affected {
        changed,
        open,
        files,
        reads,
        packages,
        iac,
        sides,
        lockfiles: attributed.lockfiles,
    }
}

pub(crate) fn reads<'g>(
    graph: &'g Graph<'g>,
    changes: &[(String, Change)],
    roots: &[String],
) -> Vec<(&'g str, Evidence<'g>)> {
    let repo = &graph.workspace.repos[0];
    let mut files = Files::new(&repo.root);
    let changed: HashMap<&str, &Change> = changes.iter().map(|(p, c)| (p.as_str(), c)).collect();
    let roots_in_order = roots;
    let roots: HashSet<&str> = roots.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for c in &graph.exports[0].calls {
        let path = c.span.path.as_str();
        let on_line = match changed.get(path) {
            None | Some(Change::Deleted) => false,
            _ if roots.contains(format!("{}::{path}", repo.identity).as_str()) => true,
            Some(Change::Untracked) => true,
            Some(Change::Lines(ranges)) => {
                let first = files.line(path, c.span.start);
                let last = files.line(path, (c.span.end - 1).max(c.span.start));
                first
                    .zip(last)
                    .is_some_and(|(f, l)| ranges.iter().any(|&(a, b)| a <= l && f <= b))
            }
        };
        if !on_line {
            continue;
        }
        match &c.target {
            Some(target) => out.push((target.as_str(), Evidence::call(c))),
            None => out.extend(c.candidates.iter().map(|t| {
                let evidence = Evidence {
                    level: Level::Ambiguous,
                    candidates: c.candidates.len(),
                    ..Evidence::call(c)
                };
                (t.as_str(), evidence)
            })),
        }
        for &link in graph.out.get(c.id.as_str()).into_iter().flatten() {
            out.push((link.provider.as_str(), graph.evidence(link)));
        }
    }
    for root in roots_in_order {
        for &link in graph.out.get(root.as_str()).into_iter().flatten() {
            out.push((link.provider.as_str(), graph.evidence(link)));
        }
    }
    out
}

fn closure(export: &Export, mut packages: BTreeSet<String>) -> BTreeSet<String> {
    let named: HashMap<&str, &str> = (export.packages.iter())
        .filter(|p| p.repository == export.repository)
        .map(|p| (p.name.as_str(), p.directory.as_str()))
        .collect();
    loop {
        let before = packages.len();
        for d in &export.dependencies {
            if named
                .get(d.name.as_str())
                .is_some_and(|&dir| packages.contains(dir))
            {
                packages.insert(directory(&d.manifest).to_string());
            }
        }
        if packages.len() == before {
            return packages;
        }
    }
}

impl<'a> Graph<'a> {
    pub(crate) fn fan<'g>(&'g self, roots: &'g [String]) -> BTreeMap<&'g str, Settled<'g>> {
        let root = |r: &'g String| {
            let settled = Settled {
                key: Key::ROOT,
                evidence: None,
                ends: false,
            };
            (r.as_str(), settled)
        };
        let mut best: HashMap<&str, Settled> = roots.iter().map(root).collect();
        let mut heap: BinaryHeap<_> = best.keys().map(|&r| Reverse((Key::ROOT, r))).collect();
        let mut settled = BTreeMap::new();
        while let Some(Reverse((key, id))) = heap.pop() {
            if settled.contains_key(id) {
                continue;
            }
            let node = best[id];
            settled.insert(id, node);
            if node.ends {
                continue;
            }
            for d in self.dependents(id) {
                let edge = Key::of(&d.evidence);
                let reached = Settled {
                    key: key.max(edge),
                    evidence: if edge >= key {
                        Some(d.evidence)
                    } else {
                        node.evidence
                    },
                    ends: d.ends,
                };
                if settled.contains_key(d.to)
                    || best.get(d.to).is_some_and(|b| b.key <= reached.key)
                {
                    continue;
                }
                best.insert(d.to, reached);
                heap.push(Reverse((reached.key, d.to)));
            }
        }
        settled
    }
}

impl<'a> Graph<'a> {
    fn file_of<'g>(&'g self, id: &'g str) -> Option<(usize, &'g str)> {
        if let Some(found) = self.path_of(id) {
            return Some(found);
        }
        if let Some(&(n, node)) = self.iac.get(id) {
            return Some((n, &node.path));
        }
        if let Some(&(n, call)) = self.calls.get(id) {
            return Some((n, &call.span.path));
        }
        (self.exports.iter().enumerate()).find_map(|(n, e)| {
            let site = e.config_sites.iter().find(|s| s.id == id)?;
            Some((n, site.path.as_str()))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet, VecDeque};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::*;
    use crate::config::Config;
    use crate::decisions::Decisions;
    use crate::engine::{Changes, Engine};
    use crate::index::{Export, Span, Symbol};
    use crate::workspace::{Repository, Workspace};

    fn id(name: &str) -> String {
        format!("r::a.ts::{name}")
    }

    fn repository(
        names: &[&str],
        calls: &[(&str, Option<&str>, Level, &[&str])],
    ) -> (Workspace, [Export; 1]) {
        let span = || {
            let mut span = Span::default();
            span.path = "a.ts".into();
            span
        };
        let export = Export {
            repository: "r".into(),
            symbols: (names.iter())
                .map(|name| {
                    let mut symbol = Symbol::default();
                    symbol.id = id(name);
                    symbol.span = span();
                    symbol
                })
                .collect(),
            calls: (calls.iter().enumerate())
                .map(|(n, (caller, target, level, candidates))| {
                    let mut call = CallSite::default();
                    call.id = format!("{}@{n}", id(caller));
                    call.caller = id(caller);
                    call.target = target.map(id);
                    call.candidates = candidates.iter().map(|c| id(c)).collect();
                    call.level = Some(*level);
                    call.span = span();
                    call
                })
                .collect(),
            ..Export::default()
        };
        let workspace = Workspace {
            repos: vec![Repository {
                root: "r".into(),
                identity: "r".into(),
                config: Config::default(),
                decisions: Decisions::default(),
                index: "".into(),
            }],
            missing: Vec::new(),
        };
        (workspace, [export])
    }

    fn fanned(graph: &Graph, roots: &[&str]) -> BTreeMap<String, Key> {
        let roots: Vec<String> = roots.iter().map(|r| id(r)).collect();
        (graph.fan(&roots).into_iter())
            .map(|(node, s)| (node.to_string(), s.key))
            .collect()
    }

    const CERTAIN: Key = Key {
        tier: Tier::Certain,
        candidates: 0,
    };

    #[test]
    fn a_node_reached_by_exact_and_inferred_chains_is_certain() {
        let calls = [
            ("x", Some("root"), Level::Inferred, &[][..]),
            ("y", Some("root"), Level::Exact, &[]),
            ("x", Some("y"), Level::Exact, &[]),
        ];
        let (workspace, exports) = repository(&["root", "x", "y"], &calls);
        let graph = Graph::new(&workspace, &exports, &[]);

        let fan = fanned(&graph, &["root"]);

        assert_eq!(fan[&id("x")], CERTAIN);
    }

    #[test]
    fn an_ambiguous_chain_keeps_the_fewest_candidates() {
        let calls = [
            ("x", None, Level::Ambiguous, &["root", "o1", "o2"][..]),
            ("y", None, Level::Ambiguous, &["root", "o1"]),
            ("x", Some("y"), Level::Exact, &[]),
        ];
        let (workspace, exports) = repository(&["root", "x", "y", "o1", "o2"], &calls);
        let graph = Graph::new(&workspace, &exports, &[]);
        let roots = [id("root")];

        let fan = graph.fan(&roots);

        let x = &fan[id("x").as_str()];
        let two = Key {
            tier: Tier::Ambiguous,
            candidates: 2,
        };
        assert_eq!(x.key, two);
        let evidence = x.evidence.unwrap();
        assert_eq!((evidence.level, evidence.candidates), (Level::Ambiguous, 2));
    }

    #[test]
    fn a_cycle_settles_each_node_once() {
        let calls = [
            ("a", Some("root"), Level::Exact, &[][..]),
            ("b", Some("a"), Level::Exact, &[]),
            ("a", Some("b"), Level::Exact, &[]),
        ];
        let (workspace, exports) = repository(&["root", "a", "b"], &calls);
        let graph = Graph::new(&workspace, &exports, &[]);

        let fan = fanned(&graph, &["root"]);

        let expected = BTreeMap::from([
            (id("a"), CERTAIN),
            (id("b"), CERTAIN),
            (id("root"), CERTAIN),
        ]);
        assert_eq!(fan, expected);
    }

    #[test]
    fn a_root_is_certain_whatever_reaches_it() {
        let calls = [("r2", None, Level::Ambiguous, &["r1", "o1"][..])];
        let (workspace, exports) = repository(&["r1", "r2", "o1"], &calls);
        let graph = Graph::new(&workspace, &exports, &[]);
        let roots = [id("r1"), id("r2")];

        let fan = graph.fan(&roots);

        let r2 = &fan[id("r2").as_str()];
        assert_eq!((r2.key, r2.evidence), (CERTAIN, None));
    }

    #[test]
    fn a_decided_link_is_certain_only_when_a_human_decided() {
        let decided = |by| Evidence {
            level: Level::Decided,
            by,
            p: None,
            candidates: 0,
            reason: None,
            narrowed: false,
        };
        assert_eq!(Tier::of(&decided(Some("human"))), Tier::Certain);
        assert_eq!(Tier::of(&decided(Some("a-model"))), Tier::Model);
        assert_eq!(Tier::of(&decided(None)), Tier::Model);
        let declared = Evidence {
            level: Level::Declared,
            ..decided(None)
        };
        assert_eq!(Tier::of(&declared), Tier::Certain);
    }

    #[test]
    fn an_ambiguous_link_counts_the_links_of_its_consumer_and_contract() {
        let (workspace, exports) =
            repository(&["x", "p1", "p2", "p3"], &[("x", None, Level::Exact, &[])]);
        let link = |provider: &str, contract: &str| {
            let (consumer, provider) = (format!("{}@0", id("x")), id(provider));
            let reason = "2 providers fit".to_string();
            Link::new(
                consumer,
                provider,
                contract.into(),
                Level::Ambiguous,
                reason,
                None,
                None,
                None,
            )
        };
        let links = [
            link("p1", "http::GET::/a"),
            link("p2", "http::GET::/a"),
            link("p3", "http::GET::/b"),
        ];
        let graph = Graph::new(&workspace, &exports, &links);

        let dependents = graph.dependents(&id("p1"));

        let x = dependents.iter().find(|d| d.to == id("x")).expect("x");
        assert_eq!(
            (x.evidence.level, x.evidence.candidates),
            (Level::Ambiguous, 2)
        );
    }

    fn certain_walk<'g>(graph: &'g Graph<'g>, roots: &'g [String]) -> BTreeSet<&'g str> {
        let mut seen: HashSet<&str> = roots.iter().map(String::as_str).collect();
        let mut queue: VecDeque<&str> = roots.iter().map(String::as_str).collect();
        while let Some(node) = queue.pop_front() {
            for d in graph.dependents(node) {
                if Tier::of(&d.evidence) == Tier::Certain && seen.insert(d.to) && !d.ends {
                    queue.push_back(d.to);
                }
            }
        }
        seen.into_iter().collect()
    }

    fn fixture(case: &str, fixture: &str, origin: &str) -> PathBuf {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let target = to.join(entry.file_name());
                match entry.file_type().unwrap().is_dir() {
                    true => copy(&entry.path(), &target),
                    false => drop(std::fs::copy(entry.path(), target).unwrap()),
                }
            }
        }
        let scratch = std::env::temp_dir().join(format!("meada-affected-{}", std::process::id()));
        let dir = scratch.join(case).join(fixture);
        let _ = std::fs::remove_dir_all(&dir);
        copy(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(fixture),
            &dir,
        );
        for args in [
            &["-c", "init.defaultBranch=main", "init", "-q"][..],
            &["remote", "add", "origin", origin],
            &["add", "-A"],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "-m",
                "f",
            ],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        }
        dir
    }

    fn assert_certain_filter_is_a_certain_walk(repo: &Path, files: &[&str]) {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        let engine = Engine::open(repo).unwrap();
        let changes = [Changes::Files(&files).read(repo).unwrap()];
        let (certain, walked, all) = (engine.walked(&changes, None, |graph, _, roots, _, _| {
            let fan = graph.fan(roots);
            let certain: BTreeSet<String> = (fan.iter())
                .filter(|(_, s)| s.key.tier == Tier::Certain)
                .map(|(node, _)| node.to_string())
                .collect();
            let walked = certain_walk(graph, roots)
                .into_iter()
                .map(str::to_string)
                .collect();
            (certain, walked, fan.len())
        }))
        .unwrap()
        .remove(0);
        assert_eq!(certain, walked);
        assert!(all > certain.len(), "{all} nodes, all certain");
    }

    #[test]
    fn the_certain_filter_equals_a_certain_only_walk() {
        let mono = fixture(
            "certain_mono",
            "trace-affected",
            "https://example.com/scope/mono.git",
        );
        let changed = [
            "packages/a/src/f.ts",
            "packages/a/src/named.ts",
            "packages/e/src/runners.ts",
        ];
        assert_certain_filter_is_a_certain_walk(&mono, &changed);

        let chain = |name: &str, origin: &str| fixture("certain_chain", name, origin);
        chain("chain-worker", "https://example.com/chain/worker.git");
        chain("chain-web", "https://example.com/chain/web.git");
        chain("chain-catalog", "https://example.com/chain/catalog.git");
        let gateway = chain("chain-gateway", "git@example.com:chain/gateway.git");
        let sources = [
            "orders.ts",
            "checkout.ts",
            "routes.ts",
            "catalog.ts",
            "server.ts",
        ];
        let changed: Vec<String> = sources.iter().map(|s| format!("src/{s}")).collect();
        let changed: Vec<&str> = changed.iter().map(String::as_str).collect();
        assert_certain_filter_is_a_certain_walk(&gateway, &changed);
    }
}
