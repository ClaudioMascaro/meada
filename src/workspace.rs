//! The workspace: the repositories one query can see. It holds the repository where Meada
//! runs, its peers through their `meada.toml` files (transitively), and the sibling clones
//! whose own `meada.toml` names a repository already in it. Discovery reads only local files.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::{Result, repo};

pub struct Workspace {
    /// The repository where Meada runs first, then the others in discovery order.
    pub repos: Vec<Repository>,
    /// Peers named in a `meada.toml` but not found on disk, sorted.
    pub missing: Vec<Missing>,
}

pub struct Repository {
    pub root: PathBuf,
    pub identity: String,
    pub config: Config,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Missing {
    /// The missing peer's identity.
    pub repo: String,
    /// The repository whose `meada.toml` names it.
    pub named_by: String,
}

impl Workspace {
    /// The workspace of the repository containing `dir`.
    pub fn discover(dir: &Path) -> Result<Self> {
        let start = repo::root(dir)?;
        let mut found = Finder::default();
        let identity = found.identity(&start);
        let mut repos = vec![Repository {
            config: config::load(&start)?,
            root: start,
            identity,
        }];
        let mut missing = Vec::new();
        let mut next = 0;
        loop {
            // Peers, transitively.
            while next < repos.len() {
                let wanted: Vec<_> = repos[next]
                    .config
                    .peers
                    .iter()
                    .map(|p| (p.repo.clone(), p.path.clone()))
                    .collect();
                for (identity, hint) in wanted {
                    if repos.iter().any(|m| m.identity == identity) {
                        continue;
                    }
                    let from = repos[next].root.clone();
                    match found.peer(&from, &identity, hint.as_deref()) {
                        Some(root) => repos.push(Repository {
                            config: config::load(&root)?,
                            root,
                            identity,
                        }),
                        None => missing.push(Missing {
                            repo: identity,
                            named_by: repos[next].identity.clone(),
                        }),
                    }
                }
                next += 1;
            }
            // Sibling clones that consume from the workspace; their peers come next round.
            let mut joined = false;
            let roots: Vec<PathBuf> = repos.iter().map(|m| m.root.clone()).collect();
            for root in &roots {
                for sibling in found.siblings(root) {
                    if repos.iter().any(|m| m.root == sibling) {
                        continue;
                    }
                    // ponytail: a sibling with an invalid `meada.toml` is skipped silently; it
                    // cannot be told apart from one that names no repository of the workspace.
                    let Ok(config) = config::load(&sibling) else {
                        continue;
                    };
                    if config
                        .peers
                        .iter()
                        .any(|p| repos.iter().any(|m| m.identity == p.repo))
                    {
                        let identity = found.identity(&sibling);
                        // Another clone of a repository already in: one index per identity.
                        if repos.iter().any(|m| m.identity == identity) {
                            continue;
                        }
                        repos.push(Repository {
                            root: sibling,
                            identity,
                            config,
                        });
                        joined = true;
                    }
                }
            }
            if !joined {
                break;
            }
        }
        missing.retain(|m| !repos.iter().any(|r| r.identity == m.repo));
        missing.sort();
        missing.dedup();
        Ok(Self { repos, missing })
    }
}

/// Repository identities and sibling listings, each read from disk once.
#[derive(Default)]
struct Finder {
    identities: HashMap<PathBuf, String>,
    siblings: HashMap<PathBuf, Vec<PathBuf>>,
}

impl Finder {
    fn identity(&mut self, root: &Path) -> String {
        self.identities
            .entry(root.to_path_buf())
            .or_insert_with(|| repo::identity(root))
            .clone()
    }

    /// The root of the peer `identity` named from the repository at `from`: at its `path`
    /// hint, or among the clones next to `from`. A repository with no remote (`local/<dir>`)
    /// is only found at its hint: any clone may share its directory name.
    fn peer(&mut self, from: &Path, identity: &str, hint: Option<&str>) -> Option<PathBuf> {
        if let Some(hint) = hint {
            let dir = from.join(hint);
            if dir.is_dir()
                && let Ok(root) = repo::root(&dir)
                && self.identity(&root) == identity
            {
                return Some(root);
            }
        }
        if identity.starts_with("local/") {
            return None;
        }
        self.siblings(from)
            .into_iter()
            .find(|sibling| self.identity(sibling) == identity)
    }

    /// The git clones next to `root`, sorted.
    fn siblings(&mut self, root: &Path) -> Vec<PathBuf> {
        let Some(parent) = root.parent() else {
            return Vec::new();
        };
        let all = self
            .siblings
            .entry(parent.to_path_buf())
            .or_insert_with(|| {
                let mut dirs: Vec<PathBuf> = fs::read_dir(parent)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.join(".git").exists())
                    .collect();
                dirs.sort();
                dirs
            });
        all.iter().filter(|p| *p != root).cloned().collect()
    }
}
