//! The workspace: the repositories one query can see. It holds the repository where Meada
//! runs, its peers through their `meada.toml` files (transitively), and the sibling clones
//! whose own `meada.toml` names a repository already in it. Discovery reads only local files.
//! In server mode (ADR 0011) the workspace is instead every repository a workspace manifest
//! (`meada.workspace.toml`) lists, and nothing is discovered.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::config::{self, Config};
use crate::decisions::{self, Decisions};
use crate::msg::Msg;
use crate::{Result, index, modules, repo};

pub struct Workspace {
    /// The repository where Meada runs first, then the others in discovery order.
    pub repos: Vec<Repository>,
    /// Peers named in a `meada.toml` but not found on disk, sorted.
    pub missing: Vec<Missing>,
}

#[derive(Clone)]
pub struct Repository {
    pub root: PathBuf,
    pub identity: String,
    pub config: Config,
    pub decisions: Decisions,
    /// Where its index lives: `<root>/.meada/index.db` locally, `<index_dir>/<identity>/index.db`
    /// in server mode. The one answer every open of an index asks for.
    pub index: PathBuf,
}

impl Repository {
    /// The repository checked out at `root`, read locally, its index in its own `.meada/`.
    fn local(root: PathBuf, identity: String) -> Result<Self> {
        Ok(Self {
            config: config::load(&root)?,
            decisions: decisions::load(&root)?,
            index: index::path(&root),
            root,
            identity,
        })
    }

    /// A mirror `Manifest::read` served, checked again (server mode, at query time): it still
    /// exists, as the root of a checkout whose remote is its identity.
    pub fn recheck(&self) -> std::result::Result<(), String> {
        let shown = self.root.display().to_string();
        if !self.root.exists() {
            return Err(Msg::NoMirror(&shown).text());
        }
        if !repo::is_checkout(&self.root) {
            if repo::refused_as_dubious(&self.root) {
                return Err(Msg::DubiousOwner(&shown).text());
            }
            return Err(Msg::NotACheckout(&shown).text());
        }
        let found = repo::identity(&self.root);
        if found != self.identity {
            return Err(Msg::IdentityMismatch {
                found: &found,
                repo: &self.identity,
            }
            .text());
        }
        Ok(())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Missing {
    /// The missing peer's identity.
    pub repo: String,
    /// The repository whose `meada.toml` names it.
    pub named_by: String,
}

impl Missing {
    /// `missing peer: <repo>, named by <repo>`, as `status` and the trace footer show it.
    pub fn text(&self) -> String {
        Msg::MissingPeer {
            repo: &self.repo,
            named_by: &self.named_by,
        }
        .text()
    }
}

impl Workspace {
    /// The workspace of the repository containing `dir`.
    pub fn discover(dir: &Path) -> Result<Self> {
        let start = repo::root(dir)?;
        let mut found = Finder::default();
        let identity = found.identity(&start);
        let mut repos = vec![Repository::local(start, identity)?];
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
                        Some(root) => repos.push(Repository::local(root, identity)?),
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
                    // ponytail: a sibling with an invalid `meada.toml` or decisions file is
                    // skipped silently; it cannot be told apart from one that names no
                    // repository of the workspace.
                    let (Ok(config), Ok(decisions)) =
                        (config::load(&sibling), decisions::load(&sibling))
                    else {
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
                            index: index::path(&sibling),
                            root: sibling,
                            identity,
                            config,
                            decisions,
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

/// A workspace manifest (`meada.workspace.toml`, ADR 0011), read only from the path
/// `--workspace` gives: every `[[repository]]` in manifest order.
#[derive(Clone)]
pub struct Manifest {
    pub entries: Vec<Listed>,
    /// The `[valkey]` section, when the build has the module.
    #[cfg(feature = "valkey")]
    pub valkey: Option<crate::valkey::Config>,
    /// The `[snapshot]` section, when the build has the module.
    #[cfg(feature = "snapshot")]
    pub snapshot: Option<crate::snapshot::Config>,
    /// The `[generations]` section, when the build has the module; `read` checks that
    /// `[snapshot]` and `[valkey]` are there too.
    #[cfg(feature = "generation-pointer")]
    pub generations: Option<crate::snapshot::generations::Config>,
    /// The `[refresh_lock]` section, when the build has the module; `read` checks that
    /// `[generations]` is there too.
    #[cfg(feature = "refresh-lock")]
    pub refresh_lock: Option<crate::snapshot::lock::Config>,
    /// The `[oidc]` section, when the build has the module.
    #[cfg(feature = "oidc")]
    pub oidc: Option<crate::oidc::Config>,
}

/// One `[[repository]]` of a manifest.
#[derive(Clone)]
pub struct Listed {
    /// The identity it names.
    pub repo: String,
    /// Its `path`, as the manifest writes it.
    pub path: String,
    /// The repository at that path, or why it cannot be served.
    pub repository: std::result::Result<Repository, String>,
    /// The entry's configuration blocks the mirror's own `meada.toml` overrides, as `status`
    /// names them: `[repository.serve]`, `[[repository.resource]] <kind> <id>`,
    /// `[[repository.peer]] <repo>`.
    pub ignored: Vec<String>,
    /// The entry's `serve`, `resource` and `peer` blocks as written, canonical
    /// (`snapshot::blocks`): what a bundle id hashes, and what a generation records.
    #[cfg(feature = "snapshot")]
    pub blocks: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    // Required, but checked by `read`: serde would name it after the last table of the file.
    index_dir: Option<String>,
    #[serde(default, rename = "repository")]
    repositories: Vec<EntryFile>,
    #[cfg(feature = "valkey")]
    valkey: Option<crate::valkey::File>,
    #[cfg(feature = "snapshot")]
    snapshot: Option<crate::snapshot::File>,
    #[cfg(feature = "generation-pointer")]
    generations: Option<crate::snapshot::generations::File>,
    #[cfg(feature = "refresh-lock")]
    refresh_lock: Option<crate::snapshot::lock::File>,
    #[cfg(feature = "oidc")]
    oidc: Option<crate::oidc::File>,
}

/// A `[[repository]]`: `serve` and `resource` take the shapes of `meada.toml`'s, `peer` that
/// of its `[[peer]]` without `path` (the manifest locates every repository).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryFile {
    repo: String,
    path: String,
    #[serde(default, deserialize_with = "config::declared")]
    serve: config::Serve,
    #[serde(default, rename = "resource")]
    resources: Vec<config::Resource>,
    #[serde(default, rename = "peer")]
    peers: Vec<EntryPeer>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryPeer {
    repo: String,
    #[serde(default)]
    base_url_env: Vec<String>,
}

impl EntryFile {
    /// Its blocks, as `Listed::ignored` names them.
    fn blocks(&self) -> Vec<String> {
        let serve = self
            .serve
            .declared
            .then(|| "[repository.serve]".to_string());
        let resources =
            (self.resources.iter()).map(|r| format!("[[repository.resource]] {} {}", r.kind, r.id));
        let peers = (self.peers.iter()).map(|p| format!("[[repository.peer]] {}", p.repo));
        serve.into_iter().chain(resources).chain(peers).collect()
    }

    /// Its blocks as the configuration of a mirror with no `meada.toml`.
    fn config(self) -> Config {
        let peers = self.peers.into_iter().map(|p| config::Peer {
            repo: p.repo,
            path: None,
            base_url_env: p.base_url_env,
        });
        Config {
            peers: peers.collect(),
            serve: self.serve,
            resources: self.resources,
        }
    }
}

impl Manifest {
    /// The manifest at `file`; `index_dir` and each `path` are absolute or relative to its
    /// directory. A parse error names the file, the line and the key, as `meada.toml`'s do.
    pub fn read(file: &Path) -> Result<Self> {
        let src = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        modules::check(file, &src)?;
        let parsed: ManifestFile = config::parse(file, &src, "meada.workspace.toml")?;
        let Some(index_dir) = parsed.index_dir else {
            let missing = Msg::KeyMissing("index_dir").text();
            return Err(format!("{}: {missing}", file.display()).into());
        };
        let base = file.parent().unwrap_or(Path::new(""));
        #[cfg(feature = "valkey")]
        let valkey = modules::validated(file, &src, parsed.valkey, |v| v.validate(base))?;
        #[cfg(feature = "snapshot")]
        let snapshot = modules::validated(file, &src, parsed.snapshot, |s| s.validate(base))?;
        #[cfg(feature = "generation-pointer")]
        let generations = modules::validated(file, &src, parsed.generations, |g| g.validate())?;
        #[cfg(feature = "generation-pointer")]
        if generations.is_some() {
            let missing = [
                ("snapshot", snapshot.is_none()),
                ("valkey", valkey.is_none()),
            ];
            let missing: Vec<&str> = (missing.iter()).filter(|m| m.1).map(|m| m.0).collect();
            modules::needs(file, &src, "generations", &missing)?;
        }
        #[cfg(feature = "refresh-lock")]
        let refresh_lock = modules::validated(file, &src, parsed.refresh_lock, |l| l.validate())?;
        #[cfg(feature = "refresh-lock")]
        if refresh_lock.is_some() && generations.is_none() {
            modules::needs(file, &src, "refresh_lock", &["generations"])?;
        }
        #[cfg(feature = "oidc")]
        let oidc = modules::validated(file, &src, parsed.oidc, |o| o.validate())?;
        // Each entry's blocks as written, in manifest order: the typed read checked them.
        #[cfg(feature = "snapshot")]
        let mut written = crate::snapshot::entry_blocks(&src).into_iter();
        let index_dir = base.join(index_dir);
        // Before any `git` call: a mirror another user owns is read all the same (ADR 0011).
        (parsed.repositories.iter()).for_each(|e| repo::trust(&base.join(&e.path)));
        let mut seen = HashSet::new();
        let mut entries: Vec<Listed> = (parsed.repositories.into_iter())
            .map(|e| {
                let twice = !seen.insert(e.repo.clone());
                let (repo, path, blocks) = (e.repo.clone(), e.path.clone(), e.blocks());
                let (repository, ignored) =
                    match mirror(&index_dir, &base.join(&path), &repo, twice, e.config()) {
                        Ok((repository, true)) => (Ok(repository), blocks),
                        Ok((repository, false)) => (Ok(repository), Vec::new()),
                        Err(problem) => (Err(problem), Vec::new()),
                    };
                Listed {
                    repo,
                    path,
                    repository,
                    ignored,
                    #[cfg(feature = "snapshot")]
                    blocks: written.next().unwrap_or_default(),
                }
            })
            .collect();
        // No index lands inside a listed directory: mirrors may be read-only mounts.
        let roots: Vec<(PathBuf, String)> = (entries.iter())
            .filter_map(|l| {
                let path = base.join(&l.path);
                Some((path.canonicalize().ok()?, path.display().to_string()))
            })
            .collect();
        for listed in &mut entries {
            let Ok(repo) = &listed.repository else {
                continue;
            };
            let dir = repo.index.parent().unwrap_or(&repo.index);
            let at = resolved(dir);
            let Some((_, mirror)) = roots.iter().find(|(root, _)| at.starts_with(root)) else {
                continue;
            };
            let index = dir.display().to_string();
            // Not served, so nothing of it is ignored either.
            listed.ignored.clear();
            listed.repository = Err(Msg::IndexInMirror {
                index: &index,
                mirror,
            }
            .text());
        }
        Ok(Self {
            entries,
            #[cfg(feature = "valkey")]
            valkey,
            #[cfg(feature = "snapshot")]
            snapshot,
            #[cfg(feature = "generation-pointer")]
            generations,
            #[cfg(feature = "refresh-lock")]
            refresh_lock,
            #[cfg(feature = "oidc")]
            oidc,
        })
    }

    /// The workspace of a pulled generation (`meada snapshot pull`, ADR 0012), which `dir`
    /// (`<local_dir>/<generation id>`) holds and `document` records: per entry, the mirror at
    /// `<bundle id>/mirror` and the index at `<bundle id>/index.db`, the entry's blocks as the
    /// document records them, or the mirror's own `meada.toml` whole. It touches no mirror
    /// with `git`: the pull checked each one. The paths are built only from ids and identities
    /// checked here, since the document came from a store.
    #[cfg(feature = "snapshot")]
    pub(crate) fn of_generation(dir: &Path, document: &crate::snapshot::Document) -> Result<Self> {
        let shown = dir
            .join(crate::snapshot::pull::DOCUMENT)
            .display()
            .to_string();
        let not = || Msg::SnapshotNotDocument(&shown).text();
        let mut entries = Vec::new();
        for entry in &document.repositories {
            let repo = entry.identity.as_str();
            let bad = |problem: String| format!("{shown}: {repo}: {problem}");
            if let Some(problem) = identity_problem(repo) {
                return Err(bad(problem).into());
            }
            let bundle = crate::snapshot::bundle_of(&entry.bundle);
            if !crate::snapshot::is_id(bundle) {
                return Err(not().into());
            }
            let root = dir.join(bundle).join("mirror");
            let serde_json::Value::Object(mut written) = entry.blocks.clone() else {
                return Err(not().into());
            };
            written.insert("repo".into(), repo.into());
            written.insert("path".into(), "".into());
            let file: EntryFile =
                serde_json::from_value(written.into()).map_err(|e| bad(e.to_string()))?;
            let blocks = file.blocks();
            let max = Some(config::GENERATION_CONFIG_MAX);
            let own = config::read_own_up_to(&root, "meada.toml", max);
            let (config, ignored) = match own.map_err(|e| bad(e.to_string()))? {
                Some(own) => (own, blocks),
                None => (file.config(), Vec::new()),
            };
            entries.push(Listed {
                repo: repo.to_string(),
                path: root.display().to_string(),
                repository: Ok(Repository {
                    config,
                    decisions: (decisions::load_up_to(&root, max))
                        .map_err(|e| bad(e.to_string()))?,
                    index: dir.join(bundle).join("index.db"),
                    root,
                    identity: repo.to_string(),
                }),
                ignored,
                blocks: entry.blocks.clone(),
            });
        }
        Ok(Self {
            entries,
            #[cfg(feature = "valkey")]
            valkey: None,
            snapshot: None,
            #[cfg(feature = "generation-pointer")]
            generations: None,
            #[cfg(feature = "refresh-lock")]
            refresh_lock: None,
            #[cfg(feature = "oidc")]
            oidc: None,
        })
    }

    /// The peers a servable repository's `meada.toml` names that the manifest does not list,
    /// sorted: server mode looks for nothing by path hint or sibling clone.
    pub fn missing(&self) -> Vec<Missing> {
        let listed: HashSet<&str> = self.entries.iter().map(|l| l.repo.as_str()).collect();
        let mut missing: Vec<Missing> = (self.entries.iter())
            .filter_map(|l| l.repository.as_ref().ok())
            .flat_map(|r| {
                (r.config.peers.iter())
                    .filter(|p| !listed.contains(p.repo.as_str()))
                    .map(|p| Missing {
                        repo: p.repo.clone(),
                        named_by: r.identity.clone(),
                    })
            })
            .collect();
        missing.sort();
        missing.dedup();
        missing
    }

    /// The workspace of a query about `first`: every listed repository that `reads` (a token's
    /// grant, #290), `first` first, then manifest order. One it does not read is absent, as if
    /// the manifest did not list it, and is not missing either. Refused while any entry kept
    /// cannot be served, naming each problem.
    pub fn workspace(mut self, first: &str, reads: &dyn Fn(&str) -> bool) -> Result<Workspace> {
        // Before `missing`, so neither a problem nor a missing peer names one left out.
        self.entries.retain(|l| reads(&l.repo));
        let mut missing = self.missing();
        missing.retain(|m| reads(&m.repo));
        let mut repos = Vec::new();
        let mut problems = Vec::new();
        for listed in self.entries {
            match listed.repository {
                Ok(repo) => repos.push(repo),
                Err(problem) => problems.push(format!("{}: {problem}", listed.repo)),
            }
        }
        if !problems.is_empty() {
            return Err(problems.join("\n").into());
        }
        let Some(at) = repos.iter().position(|r| r.identity == first) else {
            let served: Vec<&str> = repos.iter().map(|r| r.identity.as_str()).collect();
            let served = served.join(", ");
            return Err(Msg::NotListed {
                repo: first,
                served: &served,
            }
            .text()
            .into());
        };
        let named = repos.remove(at);
        repos.insert(0, named);
        Ok(Workspace { repos, missing })
    }
}

/// Where `path` is, even before it exists: its deepest existing ancestor canonicalized, the
/// components that do not exist yet folded onto it (they hold no symbolic link).
fn resolved(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut rest = Vec::new();
    let mut at = absolute.as_path();
    let real = loop {
        if let Ok(real) = at.canonicalize() {
            break real;
        }
        match (at.parent(), at.components().next_back()) {
            (Some(parent), Some(last)) => {
                rest.push(last);
                at = parent;
            }
            _ => break PathBuf::new(),
        }
    };
    rest.into_iter().rev().fold(real, |mut path, part| {
        match part {
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            part => path.push(part),
        }
        path
    })
}

/// Why `repo` cannot be the identity of a served repository, checked before any path is built
/// from it: a segment that would leave the directory it is joined to (empty, `.` or `..`), or a
/// `local/` identity. A manifest entry and a generation document (`meada snapshot pull`) share
/// it; each also refuses an identity listed twice.
pub(crate) fn identity_problem(repo: &str) -> Option<String> {
    if repo.split('/').any(|s| matches!(s, "" | "." | "..")) {
        return Some(Msg::UnsafeIdentity.text());
    }
    if repo.starts_with("local/") {
        return Some(Msg::LocalIdentity.text());
    }
    None
}

/// The repository a manifest entry lists as `repo` at `path`, its index under `index_dir`, or
/// why it cannot be served. `twice` when an earlier entry names the same identity. Its
/// configuration is its own `meada.toml`, whole, or the entry's blocks (`listed`) when it has
/// none; the flag says it has one.
fn mirror(
    index_dir: &Path,
    path: &Path,
    repo: &str,
    twice: bool,
    listed: Config,
) -> std::result::Result<(Repository, bool), String> {
    if let Some(problem) = identity_problem(repo) {
        return Err(problem);
    }
    if twice {
        return Err(Msg::ListedTwice.text());
    }
    let shown = path.display().to_string();
    if !path.exists() {
        return Err(Msg::NoMirror(&shown).text());
    }
    if !repo::is_checkout(path) {
        if repo::refused_as_dubious(path) {
            return Err(Msg::DubiousOwner(&shown).text());
        }
        return Err(Msg::NotACheckout(&shown).text());
    }
    let root = path.canonicalize().map_err(|e| format!("{shown}: {e}"))?;
    let found = repo::identity(&root);
    if found != repo {
        return Err(Msg::IdentityMismatch {
            found: &found,
            repo,
        }
        .text());
    }
    let own = config::load_own(&root).map_err(|e| e.to_string())?;
    let has_own = own.is_some();
    let repository = Repository {
        config: own.unwrap_or(listed),
        decisions: decisions::load(&root).map_err(|e| e.to_string())?,
        index: index_dir.join(repo).join("index.db"),
        root,
        identity: found,
    };
    Ok((repository, has_own))
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
