//! The affected set of a change (`meada trace --affected`): everything a change may affect,
//! file by file, for a merge queue to filter. A change roots the symbols, files, config sites,
//! IaC nodes and document sides it touches; a path that roots nothing is `open`, with its
//! reason, never dropped.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use serde::Serialize;

use super::{Direction, Graph, calls_on_link, held, in_path};
use crate::contract::{self, Level, Link};
use crate::index::CallSite;
use crate::package::{self, Manifest};
use crate::render::Files;
use crate::repo::Change;

/// The answer of `meada trace --affected`, every list sorted.
#[derive(Serialize, Debug, Default)]
pub struct Affected {
    changed: Vec<Changed>,
    open: Vec<Open>,
    files: Vec<File>,
    reads: Vec<File>,
    packages: Packages,
    iac: Vec<Reached>,
    sides: Vec<File>,
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

/// A reached IaC node: a workload that runs the code, or a CI node that runs its program.
#[derive(Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Reached {
    id: String,
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
    // A file's entry comes from the first of its nodes the walk settles.
    let mut files: BTreeMap<(usize, &str), Option<Evidence>> = BTreeMap::new();
    let touched =
        (changed.iter().map(|c| c.path.as_str())).chain(open.iter().map(|o| o.path.as_str()));
    for path in touched {
        files.insert((0, path), None);
    }
    let mut iac = BTreeMap::new();
    for (id, (evidence, ends)) in walk(graph, roots) {
        if ends {
            iac.entry(id).or_insert(evidence);
        }
        if let Some(place) = graph.file_of(id) {
            files.entry(place).or_insert(evidence);
        }
    }
    let files = (files.into_iter())
        .map(|((n, path), evidence)| {
            let root = Evidence {
                level: Level::Exact,
                by: None,
                p: None,
                candidates: 0,
                reason: None,
                narrowed: false,
            };
            let e = evidence.unwrap_or(root);
            File {
                repo: graph.workspace.repos[n].identity.clone(),
                path: path.to_string(),
                package: owners.package(n, path),
                test: crate::http::is_test_file(path),
                tier: Tier::of(&e),
                level: e.level,
                by: e.by.map(str::to_string),
                p: e.p,
                candidates: (e.candidates > 0).then_some(e.candidates),
                reason: e.reason.map(str::to_string),
                narrowed: e.narrowed,
                manifest: false,
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
    Affected {
        changed,
        open,
        files,
        iac,
        ..Affected::default()
    }
}

/// Every node reached from `roots` over `Graph::dependents`, breadth first, with the edge that
/// first reached it (none for a root) and whether the walk ends there.
fn walk<'g>(
    graph: &'g Graph<'g>,
    roots: &'g [String],
) -> Vec<(&'g str, (Option<Evidence<'g>>, bool))> {
    let mut seen: HashSet<&str> = roots.iter().map(String::as_str).collect();
    let mut reached: Vec<_> = roots.iter().map(|r| (r.as_str(), (None, false))).collect();
    let mut queue: VecDeque<&str> = roots.iter().map(String::as_str).collect();
    while let Some(id) = queue.pop_front() {
        for d in graph.dependents(id) {
            if seen.insert(d.to) {
                reached.push((d.to, (Some(d.evidence), d.ends)));
                if !d.ends {
                    queue.push_back(d.to);
                }
            }
        }
    }
    reached
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
        (self.exports.iter().enumerate()).find_map(|(n, e)| {
            let site = e.config_sites.iter().find(|s| s.id == id)?;
            Some((n, site.path.as_str()))
        })
    }
}
