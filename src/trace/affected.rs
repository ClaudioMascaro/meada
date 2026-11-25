//! The affected set of a change (`meada trace --affected`): everything a change may affect,
//! file by file, for a merge queue to filter. A change roots the symbols, files, config sites,
//! IaC nodes and document sides it touches; a path that roots nothing is `open`, with its
//! reason, never dropped.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet};

use serde::Serialize;

use super::{Direction, Graph, calls_on_link, held, in_path};
use crate::contract::{self, Level, Link};
use crate::index::{CallSite, Export};
use crate::package::{self, Manifest};
use crate::render::Files;
use crate::repo::Change;

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
}

/// A changed path and what changed of it.
#[derive(Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Changed {
    repo: String,
    path: String,
    /// `lines`, `whole` or `deleted`.
    change: &'static str,
}

/// A path that roots nothing the walk can follow, or that the walk does not follow whole.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Open {
    repo: String,
    path: String,
    /// `deleted`, `iac`, `manifest`, `root` or `no-symbols`, the first that applies.
    reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<String>,
}

/// An affected file, with the evidence of the edge that set the key of its best node. A touched
/// file is a root: `certain`, at `exact`, with no edge.
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
    /// Whether the file's package is in the manifest's reverse closure of a changed package.
    manifest: bool,
}

/// A file a changed line reads, with the evidence of its strongest edge.
#[derive(Serialize, Debug, PartialEq)]
struct Read {
    repo: String,
    path: String,
    tier: Tier,
    level: Level,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidates: Option<usize>,
}

/// How certain an affected node is: the rank of the weakest edge of its strongest chain.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Tier {
    /// `exact`, `declared`, or `decided` by a human.
    Certain,
    /// A name match, narrowed or not.
    Inferred,
    /// `decided` by a model, or by no one named.
    Model,
    /// One of several candidates.
    Ambiguous,
}

impl Tier {
    /// The tier of an edge: its level, and for a decided link who decided it.
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

/// The key of an edge, a chain or a node: its tier, then its candidates (0 off `ambiguous`).
/// A chain's key is its largest edge key; a node's, the smallest chain key reaching it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key {
    pub(crate) tier: Tier,
    pub(crate) candidates: usize,
}

impl Key {
    /// A root's key, the smallest.
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

/// A node the fan settled: its key, the weakest edge of its strongest chain (none for a root),
/// and whether the walk ends there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settled<'g> {
    pub(crate) key: Key,
    pub(crate) evidence: Option<Evidence<'g>>,
    pub(crate) ends: bool,
}

/// A reached IaC node: a workload that runs the code, or a CI node that runs its program.
#[derive(Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Reached {
    id: String,
    level: Level,
}

/// A reached node with no file of its own (`Graph::ends`): a repository-level side, a side
/// only a document lists, or a config use.
#[derive(Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Side {
    id: String,
    /// `repository`, `document` or `config`.
    place: &'static str,
    level: Level,
}

/// The evidence of one edge: its level, who decided a decided link and the probability a model
/// gave, its candidate count (0 off an ambiguous edge), the link's reason, and whether a name
/// match was narrowed.
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
    /// A call or use row's evidence: a row with no level is held for the weakest.
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

/// A node that depends on another through one edge; `ends` when the walk stops there (an IaC
/// node that runs the code or its program).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dependent<'g> {
    pub(crate) to: &'g str,
    pub(crate) evidence: Evidence<'g>,
    pub(crate) ends: bool,
}

impl<'a> Graph<'a> {
    /// What depends on `id`, one edge each, in a fixed order: the calls and uses whose target
    /// it is (at their holder, else their caller), the calls that list it among their
    /// candidates, the definitions a decorator call of it decorates, the calls that consume a
    /// link into it, the methods of a declarative client that do, the consumers of the data
    /// that it or a call it makes provides, the workloads that run its code and the CI nodes
    /// that run it as a program. Not what it calls, nor what writes what it reads, nor what
    /// provisions what it uses: those are what it depends on.
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
                    // A call consuming a link into `id` itself is above.
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
            for x in (self.executions.iter())
                .filter(|x| x.repository == *repository && in_path(path, &x.path))
            {
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

    /// A link's evidence; an ambiguous link counts the links of its consumer and contract.
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

    /// The repository and path of a symbol, or of a file id (`<repository>::<path>`).
    fn path_of<'g>(&'g self, id: &'g str) -> Option<(usize, &'g str)> {
        if let Some(&(n, s)) = self.symbols.get(id) {
            return Some((n, &s.span.path));
        }
        self.workspace.repos.iter().enumerate().find_map(|(n, m)| {
            let path = id.strip_prefix(&m.identity)?.strip_prefix("::")?;
            (!path.contains("::") && !path.contains('#')).then_some((n, path))
        })
    }

    /// The CI step `node` and what holds it, as the walk up from a step writes them
    /// (`Walk::holders_up`): its CI job, its pipeline, and the CI jobs that call that pipeline,
    /// each once, by the identities the graph keeps.
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

/// The packages per tier, by manifest directory.
#[derive(Serialize, Debug, Default)]
struct Packages {
    certain: Vec<String>,
    inferred: Vec<String>,
    manifest_only: Vec<String>,
}

/// The package that owns each path, per repository of the workspace.
struct Owners<'g> {
    graph: &'g Graph<'g>,
    /// Whether each manifest read is a workspace root (it lists workspace members), by
    /// repository and path.
    workspaces: HashMap<(usize, &'g str), bool>,
}

impl<'g> Owners<'g> {
    fn new(graph: &'g Graph<'g>) -> Self {
        Self {
            graph,
            workspaces: HashMap::new(),
        }
    }

    /// Whether the manifest at `path` in repository `n` lists workspace members: an npm
    /// `package.json` whose `workspaces` is an array or an object with `packages`. The index
    /// stores no such field, so it is read from disk.
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

    /// The owning manifest of `path` in repository `n`, and whether it is a workspace root.
    fn owner(&mut self, n: usize, path: &str) -> Option<(&'g Manifest, bool)> {
        let manifest = package::owner_of(&self.graph.exports[n].manifests, path)?;
        Some((manifest, self.is_workspace(n, &manifest.path)))
    }

    /// The directory of the manifest that owns `path` in repository `n`, unless a workspace
    /// root owns it.
    fn package(&mut self, n: usize, path: &str) -> Option<String> {
        match self.owner(n, path)? {
            (_, true) => None,
            (manifest, false) => Some(directory(&manifest.path).to_string()),
        }
    }
}

/// The directory of a manifest path: `.` for the repository root.
fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

/// The ids a change roots in the repository where Meada runs, sorted, and the paths it leaves
/// open. A symbol roots when its span overlaps a changed line; the file's own id and every
/// symbol of the file root when the whole file changed or a changed line sits in no symbol (an
/// `import` line rebinds calls in unchanged functions). Config sites and IaC nodes root by
/// their lines, and a changed document roots every side it alone lists. An IaC or CI path is
/// also `open` (`iac`): the walk does not follow IaC references nor pipeline calls.
pub(crate) fn roots<'g>(
    graph: &'g Graph<'g>,
    changes: &[(String, Change)],
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
    for (path, change) in changes {
        let file = format!("{}::{path}", repo.identity);
        // Whether a 1-based line range overlaps the change.
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
        let owner = owners.owner(0, path);
        let reason = match () {
            _ if matches!(change, Change::Deleted) => Some("deleted"),
            _ if iac => Some("iac"),
            _ if rooted => None,
            _ if owner.is_some_and(|(m, root)| m.path == *path && !root) => Some("manifest"),
            _ if owner.is_some_and(|(_, root)| root) => Some("root"),
            _ => Some("no-symbols"),
        };
        if let Some(reason) = reason {
            open.push(Open {
                repo: repo.identity.clone(),
                path: path.clone(),
                reason,
                package: owners.package(0, path),
            });
        }
    }
    open.sort();
    (roots.into_iter().collect(), open)
}

/// The answer: what `changes` touched, the paths `roots` left `open`, and what the walk from
/// `roots` reaches.
pub(crate) fn answer<'g>(
    graph: &'g Graph<'g>,
    changes: &[(String, Change)],
    roots: &[String],
    open: Vec<Open>,
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
    // A file's entry comes from its best node, smallest by `(key, id)`; a touched file is a
    // root.
    let mut files: BTreeMap<(usize, &str), (Key, &str, Option<Evidence>)> = BTreeMap::new();
    let touched =
        (changed.iter().map(|c| c.path.as_str())).chain(open.iter().map(|o| o.path.as_str()));
    for path in touched {
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
    // The packages the change touches, and what depends on them through the manifests.
    let seeds = (changed.iter().map(|c| c.path.as_str()))
        .chain(open.iter().map(|o| o.path.as_str()))
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
    // The packages are those of the repository where Meada runs.
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
    let mut read: BTreeMap<(usize, &str), (Key, Evidence)> = BTreeMap::new();
    for (id, evidence) in reads(graph, changes, roots) {
        let Some(place) = graph.file_of(id) else {
            continue;
        };
        let key = Key::of(&evidence);
        let best = read.entry(place).or_insert((key, evidence));
        if key < best.0 {
            *best = (key, evidence);
        }
    }
    let reads = (read.into_iter())
        .map(|((n, path), (key, e))| Read {
            repo: graph.workspace.repos[n].identity.clone(),
            path: path.to_string(),
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
    }
}

/// What the rows on changed lines read, one hop down, with each edge's evidence: a row's
/// target or candidates, and the provider of every link whose consumer it is (a package link
/// to a bare specifier, HTTP, the writer of what it reads). A row is on a changed line when its
/// span overlaps one; every row of a file whose own id roots is (a whole file, or a changed
/// line in no symbol, which rebinds the file's calls). Rows are read by span from the export,
/// uses included, not through `Graph::callees`.
pub(crate) fn reads<'g>(
    graph: &'g Graph<'g>,
    changes: &[(String, Change)],
    roots: &[String],
) -> Vec<(&'g str, Evidence<'g>)> {
    let repo = &graph.workspace.repos[0];
    let mut files = Files::new(&repo.root);
    let changed: HashMap<&str, &Change> = changes.iter().map(|(p, c)| (p.as_str(), c)).collect();
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
    out
}

/// `seeds` and the packages of `export`'s repository that depend on them through its
/// manifests, by directory: a `Dependency` makes the package of its manifest depend on the
/// workspace package it names.
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
    /// Every node reached from `roots` over `dependents`, each with its key: the smallest,
    /// over the chains reaching it, of the largest edge key on the chain. Settles each node
    /// once from a priority queue, equal keys in id order, so the first predecessor to offer a
    /// key is the smallest by `(key, id)`; a root is `Certain` whatever reaches it. No depth
    /// limit and no budget: the settled set ends cycles.
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
                    // The edge sets the chain's key unless an earlier edge is weaker.
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
    /// The repository and file a reached node lives in: a symbol's, a file id's, a config
    /// site's or an IaC node's. A repository, a side only a document lists and a config use
    /// have none.
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

    /// `r::a.ts::<name>`.
    fn id(name: &str) -> String {
        format!("r::a.ts::{name}")
    }

    /// A workspace of one repository `r` whose file `a.ts` defines `names` and makes `calls`,
    /// `(caller, target, level, candidates)` by name: an ambiguous call has no target.
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
        // root -inferred-> x, and root -exact-> y -exact-> x: the inferred route is shorter.
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
        // x lists root among 3 candidates; y among 2, and x calls y.
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
        // The weakest edge of the chain is y's call, not x's exact one.
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

    /// What a breadth-first walk over certain edges only reaches from `roots`: a node an
    /// ending edge reaches is not walked on.
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

    /// `tests/fixtures/<fixture>` copied under `case` as a git repository with `origin`.
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

    /// The nodes `fan` settles `certain` from what `files` roots in `repo` are the nodes a
    /// walk over certain edges reaches.
    fn assert_certain_filter_is_a_certain_walk(repo: &Path, files: &[&str]) {
        let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
        let engine = Engine::open(repo).unwrap();
        let changes = [Changes::Files(&files).read(repo).unwrap()];
        let (certain, walked, all) = (engine.walked(&changes, |graph, _, roots, _| {
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
        // The filter leaves something out, or the equivalence says nothing.
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
