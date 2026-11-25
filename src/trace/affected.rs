//! The affected set of a change (`meada trace --affected`): everything a change may affect,
//! file by file, for a merge queue to filter. A change roots the symbols, files, config sites,
//! IaC nodes and document sides it touches; a path that roots nothing is `open`, with its
//! reason, never dropped.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde::Serialize;

use super::Graph;
use crate::contract::Level;
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
    iac: Vec<File>,
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
    /// Whether the file's package is in the manifest's reverse closure of a changed package.
    manifest: bool,
}

/// How certain an affected node is: the rank of the weakest edge of its strongest chain.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
enum Tier {
    /// `exact`, `declared`, or `decided` by a human.
    Certain,
}

/// The packages per tier, by manifest directory.
#[derive(Serialize, Debug, Default)]
struct Packages {
    certain: Vec<String>,
    inferred: Vec<String>,
    manifest_only: Vec<String>,
}

/// The package that owns each path of the repository where Meada runs.
struct Owners<'a> {
    manifests: &'a [Manifest],
    root: &'a Path,
    /// Whether each manifest read is a workspace root (it lists workspace members), by path.
    workspaces: HashMap<&'a str, bool>,
}

impl<'a> Owners<'a> {
    fn new(graph: &Graph<'a>) -> Self {
        Self {
            manifests: &graph.exports[0].manifests,
            root: &graph.workspace.repos[0].root,
            workspaces: HashMap::new(),
        }
    }

    /// Whether the manifest at `path` lists workspace members: an npm `package.json` whose
    /// `workspaces` is an array or an object with `packages`. The index stores no such field,
    /// so it is read from disk.
    fn is_workspace(&mut self, path: &'a str) -> bool {
        let root = self.root;
        *self.workspaces.entry(path).or_insert_with(|| {
            let json = (path.rsplit('/').next() == Some("package.json"))
                .then(|| std::fs::read(root.join(path)).ok())
                .flatten()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            let members = json.as_ref().and_then(|j| j.get("workspaces"));
            members.is_some_and(|w| w.is_array() || w.get("packages").is_some_and(|p| p.is_array()))
        })
    }

    /// The owning manifest of `path`, and whether it is a workspace root.
    fn owner(&mut self, path: &str) -> Option<(&'a Manifest, bool)> {
        let manifest = package::owner_of(self.manifests, path)?;
        Some((manifest, self.is_workspace(&manifest.path)))
    }

    /// The directory of the manifest that owns `path`, unless a workspace root owns it.
    fn package(&mut self, path: &str) -> Option<String> {
        match self.owner(path)? {
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
pub(crate) fn roots(graph: &Graph, changes: &[(String, Change)]) -> (Vec<String>, Vec<Open>) {
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
        let owner = owners.owner(path);
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
                package: owners.package(path),
            });
        }
    }
    open.sort();
    (roots.into_iter().collect(), open)
}

/// The answer from what the change touched: `changes`, and the paths `roots` left `open`.
pub(crate) fn answer(graph: &Graph, changes: &[(String, Change)], open: Vec<Open>) -> Affected {
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
    let touched: BTreeSet<&str> = (changed.iter().map(|c| c.path.as_str()))
        .chain(open.iter().map(|o| o.path.as_str()))
        .collect();
    let files = (touched.into_iter())
        .map(|path| File {
            repo: repo.clone(),
            path: path.to_string(),
            package: owners.package(path),
            test: crate::http::is_test_file(path),
            tier: Tier::Certain,
            level: Level::Exact,
            manifest: false,
        })
        .collect();
    Affected {
        changed,
        open,
        files,
        ..Affected::default()
    }
}
