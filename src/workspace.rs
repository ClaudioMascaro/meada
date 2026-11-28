use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[cfg(feature = "snapshot")]
pub(crate) use meada_core::manifest::identity_problem;
use meada_core::manifest::{EntryFile, entry_problems};
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
    fn local(root: PathBuf, identity: String) -> Result<Self> {
        Ok(Self {
            config: config::load(&root)?,
            decisions: decisions::load(&root)?,
            index: index::path(&root),
            root,
            identity,
        })
    }

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
    pub fn text(&self) -> String {
        Msg::MissingPeer {
            repo: &self.repo,
            named_by: &self.named_by,
        }
        .text()
    }
}

impl Workspace {
    pub fn discover(dir: &Path) -> Result<Self> {
        let start = repo::root(dir)?;
        let mut found = Finder::default();
        let identity = found.identity(&start);
        let mut repos = vec![Repository::local(start, identity)?];
        let mut missing = Vec::new();
        let mut next = 0;
        loop {
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
    /// Where every index lives (a pulled generation's directory for `of_generation`).
    pub index_dir: PathBuf,
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
    /// The `[ui]` section, when the build has the module (ADR 0014).
    #[cfg(feature = "ui")]
    pub ui: Option<crate::ui::Config>,
    /// The `[postgres]` section, when the build has the module (ADR 0016).
    #[cfg(feature = "postgres")]
    pub postgres: Option<crate::postgres::Config>,
    /// The `[directory]` section, when the build has the module (ADR 0016); `read` checks that
    /// `[postgres]` and `[oidc]` are there too.
    #[cfg(feature = "directory")]
    pub directory: Option<crate::directory::Config>,
    /// The `[catalog]` section, when the build has the module (#341, ADR 0021); `read` checks
    /// that `[oidc.github]` is there too.
    #[cfg(feature = "workspaces")]
    pub catalog: Option<crate::catalog::Config>,
    /// The `[workspaces]` section, when the build has the module (#342, ADR 0021); `read` checks
    /// the sections it needs.
    #[cfg(feature = "workspaces")]
    pub workspaces: Option<crate::workspaces::Config>,
    /// With `[workspaces]` and a pulled version (`read_listed`), the named workspaces the last
    /// `meada workspace pull` read, each without the entries the role check excluded and only
    /// while one is left: what `snapshot publish` records as contexts.
    #[cfg(feature = "workspaces")]
    pub contexts: Vec<crate::workspaces::Context>,
    /// The `[proposals]` section, when the build has the module (ADR 0022); `read` checks that
    /// `[workspaces]` is there too.
    #[cfg(feature = "proposals")]
    pub proposals: Option<crate::proposals::Config>,
    /// The `[accounts]` section, when the build has the module (ADR 0024); `read` checks that
    /// `[postgres]` is there and `[oidc]` is not, and gives it `[postgres]`.
    #[cfg(feature = "accounts")]
    pub accounts: Option<crate::accounts::Config>,
    /// `[workspace] source = "store"` (ADR 0014): the repositories come from the current
    /// workspace version: `entries` is empty, or with `read_listed` the pulled version's.
    #[cfg(feature = "generation-pointer")]
    pub store: bool,
    /// With `read_listed` and `source = "store"`, the id of the pulled version read.
    #[cfg(feature = "generation-pointer")]
    pub version: Option<String>,
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
    /// Whether only named workspaces select it (#342): a member is fetched, indexed and bundled
    /// once for all of them, at `<[workspaces] mirrors>/<identity>`, and a generation records it
    /// apart from the deployment workspace's repositories.
    pub member: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
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
    #[cfg(feature = "ui")]
    ui: Option<crate::ui::File>,
    #[cfg(feature = "postgres")]
    postgres: Option<crate::postgres::File>,
    #[cfg(feature = "directory")]
    directory: Option<crate::directory::File>,
    #[cfg(feature = "workspaces")]
    catalog: Option<crate::catalog::File>,
    #[cfg(feature = "workspaces")]
    workspaces: Option<crate::workspaces::File>,
    #[cfg(feature = "proposals")]
    proposals: Option<crate::proposals::File>,
    #[cfg(feature = "accounts")]
    accounts: Option<crate::accounts::File>,
    #[cfg(feature = "github-app")]
    #[serde(default, rename = "github_app")]
    github_apps: Vec<crate::github_app::File>,
    workspace: Option<WorkspaceFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceFile {
    #[serde(default)]
    source: Source,
}

#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Source {
    #[default]
    File,
    Store,
}

fn config_of(entry: EntryFile) -> Config {
    let peers = entry.peers.into_iter().map(|p| config::Peer {
        repo: p.repo,
        path: None,
        base_url_env: p.base_url_env,
    });
    Config {
        peers: peers.collect(),
        serve: entry.serve,
        resources: entry.resources,
        test_routes: false,
        iac: Default::default(),
        base_urls: Vec::new(),
        helm_env: Vec::new(),
        channel_clients: Vec::new(),
    }
}

impl Manifest {
    pub fn read(file: &Path) -> Result<Self> {
        Self::read_from(file, false)
    }

    fn read_from(file: &Path, pulled: bool) -> Result<Self> {
        let src = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        modules::check(file, &src)?;
        #[cfg(feature = "accounts")]
        if let Ok(table) = toml::from_str::<toml::Table>(&src)
            && let Some(dotted) = crate::accounts::under_break_glass(&table)
        {
            let refused = Msg::AccountsUnderBreakGlass(dotted).text();
            return Err(config::error_at(file, &src, dotted, &refused));
        }
        #[cfg_attr(not(feature = "generation-pointer"), expect(unused_mut))]
        let mut parsed: ManifestFile = config::parse(file, &src, "meada.workspace.toml")?;
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
        #[cfg(feature = "ui")]
        let ui = crate::ui::validated(file, &src, base, parsed.ui)?;
        #[cfg(feature = "postgres")]
        let postgres = modules::validated(file, &src, parsed.postgres, |p| p.validate(base))?;
        #[cfg(all(feature = "oidc", feature = "postgres"))]
        let mut oidc = oidc;
        #[cfg(all(feature = "oidc", feature = "postgres"))]
        if let Some(github) = oidc.as_mut().and_then(|o| o.github.as_mut()) {
            if postgres.is_none() {
                let needed = Msg::NeedsSections {
                    section: "oidc.github",
                    needed: &["postgres"],
                };
                return Err(config::error_at(file, &src, "oidc.github", &needed.text()));
            }
            github.postgres = postgres.clone();
        }
        #[cfg(feature = "accounts")]
        let accounts = {
            let mut section =
                modules::validated(file, &src, parsed.accounts, |a| a.validate(base))?;
            if let Some(accounts) = section.as_mut() {
                if postgres.is_none() {
                    modules::needs(file, &src, "accounts", &["postgres"])?;
                }
                if oidc.is_some() && !accounts.break_glass {
                    let beside = Msg::AccountsBesideOidc.text();
                    return Err(config::error_at(file, &src, "accounts", &beside));
                }
                if oidc.is_none() && accounts.break_glass {
                    let needs = Msg::AccountsBreakGlassNeedsOidc.text();
                    let key = "accounts.break_glass";
                    return Err(config::error_at(file, &src, key, &needs));
                }
                accounts.postgres = postgres.clone();
                if let Some(github) = accounts.github.as_mut() {
                    github.postgres = postgres.clone();
                }
            }
            section
        };
        #[cfg(feature = "oidc")]
        let github = oidc.as_ref().is_some_and(|o| o.github.is_some());
        #[cfg(feature = "accounts")]
        let github = github || accounts.as_ref().is_some_and(|a| a.github.is_some());
        #[cfg(not(feature = "oidc"))]
        let github = false;
        #[cfg(all(feature = "workspaces", feature = "accounts"))]
        let mut accounts = accounts;
        #[cfg(feature = "workspaces")]
        let catalog = {
            let provider = oidc.as_mut().and_then(|o| o.github.as_mut());
            #[cfg(feature = "accounts")]
            let provider = provider.or(accounts.as_mut().and_then(|a| a.github.as_mut()));
            let host = provider.as_ref().map(|g| g.host.clone());
            if parsed.catalog.is_some() && host.is_none() {
                #[cfg(feature = "accounts")]
                {
                    let what = Msg::NeedsGithubProvider("catalog").text();
                    return Err(modules::at_section(file, &src, "catalog", &what));
                }
                #[cfg(not(feature = "accounts"))]
                modules::needs(file, &src, "catalog", &["oidc.github"])?;
            }
            let host = host.unwrap_or_default();
            let catalog = modules::validated(file, &src, parsed.catalog, |c| c.validate(&host))?;
            if let Some(github) = provider {
                github.catalog = catalog.is_some();
            }
            catalog
        };
        #[cfg(feature = "directory")]
        let directory = modules::validated(file, &src, parsed.directory, |d| d.validate(base))?;
        #[cfg(feature = "directory")]
        if directory.is_some() {
            #[cfg(feature = "oidc")]
            let no_oidc = oidc.is_none();
            #[cfg(not(feature = "oidc"))]
            let no_oidc = true;
            let missing = [("postgres", postgres.is_none()), ("oidc", no_oidc)];
            let missing: Vec<&str> = (missing.iter()).filter(|m| m.1).map(|m| m.0).collect();
            modules::needs(file, &src, "directory", &missing)?;
        }
        #[cfg(all(feature = "oidc", feature = "directory"))]
        if let Some(overage) = oidc.as_mut().and_then(|o| o.overage.as_mut()) {
            match (&directory, &postgres) {
                (Some(directory), Some(postgres)) => {
                    overage.lookup = Some((directory.clone(), postgres.clone()));
                }
                _ => {
                    let needed = Msg::NeedsSections {
                        section: "oidc.entra",
                        needed: &["directory"],
                    };
                    let key = "oidc.entra.overage";
                    return Err(config::error_at(file, &src, key, &needed.text()));
                }
            }
        }
        #[cfg(feature = "github-app")]
        crate::github_app::validated(file, &src, base, parsed.github_apps, valkey.is_some())?;
        let store = parsed.workspace.is_some_and(|w| w.source == Source::Store);
        #[cfg(not(feature = "generation-pointer"))]
        if store {
            let needs = Msg::WorkspaceStoreNeedsFeature.text();
            return Err(config::error_at(file, &src, "workspace.source", &needs));
        }
        #[cfg(feature = "generation-pointer")]
        if store {
            let missing = [
                ("snapshot", snapshot.is_none()),
                ("generations", generations.is_none()),
            ];
            let missing: Vec<&str> = (missing.iter()).filter(|m| m.1).map(|m| m.0).collect();
            modules::needs(file, &src, "workspace", &missing)?;
            if !parsed.repositories.is_empty() {
                let beside = Msg::WorkspaceStoreBeside.text();
                return Err(config::error_at(file, &src, "repository", &beside));
            }
        }
        #[cfg(feature = "workspaces")]
        let workspaces = {
            let section = modules::validated(file, &src, parsed.workspaces, |w| w.validate())?;
            if section.is_some() {
                let missing = [("ui", ui.is_none()), ("catalog", catalog.is_none())];
                let missing: Vec<&str> = (missing.iter()).filter(|m| m.1).map(|m| m.0).collect();
                modules::needs(file, &src, "workspaces", &missing)?;
                if ui.as_ref().is_some_and(|u| !u.operations) {
                    let why = Msg::WorkspacesNeedsOperations.text();
                    return Err(config::error_at(file, &src, "workspaces", &why));
                }
                if !store {
                    let why = Msg::WorkspacesNeedsStore.text();
                    return Err(config::error_at(file, &src, "workspaces", &why));
                }
                if let Some(entry) =
                    (parsed.repositories.iter()).find(|e| e.repo.split('/').next() == Some("-"))
                {
                    let why = Msg::WorkspacesReservedSegment(&entry.repo).text();
                    return Err(config::error_at(file, &src, "repository.repo", &why));
                }
            }
            section
        };
        #[cfg(feature = "proposals")]
        let proposals = {
            let section = modules::validated(file, &src, parsed.proposals, |p| p.validate())?;
            if section.is_some() && workspaces.is_none() {
                modules::needs(file, &src, "proposals", &["workspaces"])?;
            }
            section
        };
        if parsed.repositories.iter().any(|e| e.owner.is_some()) {
            let owner = Msg::WorkspaceOwnerStatic.text();
            return Err(config::error_at(file, &src, "repository.owner", &owner));
        }
        let attributed = (parsed.repositories.iter()).find_map(|e| match e {
            EntryFile {
                added_by: Some(_), ..
            } => Some("repository.added_by"),
            EntryFile {
                added_by_time: Some(_),
                ..
            } => Some("repository.added_by_time"),
            _ => None,
        });
        if let Some(key) = attributed {
            let added_by = Msg::WorkspaceAddedByStatic.text();
            return Err(config::error_at(file, &src, key, &added_by));
        }
        let index_dir = base.join(index_dir);
        #[cfg(feature = "snapshot")]
        #[cfg_attr(not(feature = "generation-pointer"), expect(unused_mut))]
        let mut entries_src = src;
        #[cfg(feature = "generation-pointer")]
        let mut version = None;
        #[cfg(feature = "workspaces")]
        let mut deployment_version = String::new();
        #[cfg(feature = "generation-pointer")]
        if store && pulled {
            let dir = index_dir.join(crate::snapshot::versions::DIR);
            let read = match github {
                true => crate::snapshot::versions::eligible(&dir)?,
                false => crate::snapshot::versions::pulled(&dir)?,
            };
            let not_pulled = || crate::snapshot::versions::not_pulled(file, &dir);
            #[cfg(feature = "workspaces")]
            let read = match (read, workspaces.is_some()) {
                (None, true) => Some((String::new(), Vec::new(), String::new())),
                (read, _) => read,
            };
            let (id, repositories, text) = read.ok_or_else(not_pulled)?;
            #[cfg(feature = "workspaces")]
            if workspaces.is_some()
                && let Some(entry) =
                    (repositories.iter()).find(|e| e.repo.split('/').next() == Some("-"))
            {
                return Err(Msg::WorkspacesReservedSegment(&entry.repo).text().into());
            }
            #[cfg(feature = "workspaces")]
            {
                deployment_version = id.clone();
            }
            parsed.repositories = repositories;
            entries_src = text;
            version = Some(id).filter(|id| !id.is_empty());
        }
        #[cfg(feature = "workspaces")]
        let mut contexts = Vec::new();
        #[cfg(feature = "workspaces")]
        let deployed = parsed.repositories.len();
        #[cfg(feature = "workspaces")]
        if let (Some(section), true, true) = (&workspaces, store, pulled) {
            let dir = index_dir.join(crate::snapshot::versions::DIR);
            let deployment: Vec<String> =
                parsed.repositories.iter().map(|e| e.repo.clone()).collect();
            let found = crate::workspaces::membership(&dir, &deployment_version, &deployment)?;
            contexts = found.contexts;
            let members = found.members;
            for identity in members {
                let path = crate::workspaces::member_path(&section.mirrors, &identity);
                parsed.repositories.push(EntryFile {
                    repo: identity,
                    path: path.display().to_string(),
                    owner: None,
                    added_by: None,
                    added_by_time: None,
                    serve: Default::default(),
                    resources: Vec::new(),
                    peers: Vec::new(),
                });
            }
        }
        #[cfg(not(feature = "generation-pointer"))]
        let _ = (pulled, github);
        #[cfg(feature = "snapshot")]
        let mut written = crate::snapshot::entry_blocks(&entries_src).into_iter();
        (parsed.repositories.iter()).for_each(|e| repo::trust(&base.join(&e.path)));
        let problems = entry_problems(parsed.repositories.iter().map(|e| e.repo.as_str()));
        #[cfg(feature = "workspaces")]
        let is_member = |at: usize| at >= deployed;
        #[cfg(not(feature = "workspaces"))]
        let is_member = |_: usize| false;
        let mut entries: Vec<Listed> = (parsed.repositories.into_iter().zip(problems))
            .enumerate()
            .map(|(at, (e, problem))| {
                let (repo, path, blocks) = (e.repo.clone(), e.path.clone(), e.blocks());
                let (repository, ignored) =
                    match mirror(&index_dir, &base.join(&path), &repo, problem, config_of(e)) {
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
                    blocks: match is_member(at) {
                        true => serde_json::json!({}),
                        false => written.next().unwrap_or_default(),
                    },
                    member: is_member(at),
                }
            })
            .collect();
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
            listed.ignored.clear();
            listed.repository = Err(Msg::IndexInMirror {
                index: &index,
                mirror,
            }
            .text());
        }
        Ok(Self {
            entries,
            index_dir,
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
            #[cfg(feature = "ui")]
            ui,
            #[cfg(feature = "postgres")]
            postgres,
            #[cfg(feature = "directory")]
            directory,
            #[cfg(feature = "workspaces")]
            catalog,
            #[cfg(feature = "workspaces")]
            workspaces,
            #[cfg(feature = "workspaces")]
            contexts,
            #[cfg(feature = "proposals")]
            proposals,
            #[cfg(feature = "accounts")]
            accounts,
            #[cfg(feature = "generation-pointer")]
            store,
            #[cfg(feature = "generation-pointer")]
            version,
        })
    }

    #[cfg(feature = "proposals")]
    pub fn read_only(mut self) -> Self {
        use crate::postgres::Config;
        self.postgres = self.postgres.map(Config::read_only);
        if let Some(github) = self.github_mut() {
            github.postgres = github.postgres.take().map(Config::read_only);
        }
        #[cfg(feature = "directory")]
        if let Some((_, postgres)) = (self.oidc.as_mut())
            .and_then(|oidc| oidc.overage.as_mut())
            .and_then(|overage| overage.lookup.as_mut())
        {
            *postgres = postgres.clone().read_only();
        }
        self
    }

    pub fn read_listed(file: &Path) -> Result<Self> {
        Self::read_from(file, true)
    }

    #[cfg(feature = "generation-pointer")]
    pub(crate) fn excludes(&self) -> bool {
        #[cfg(feature = "oidc")]
        let github = self.github().is_some();
        #[cfg(not(feature = "oidc"))]
        let github = false;
        self.store && github
    }

    #[cfg(feature = "oidc")]
    pub(crate) fn github(&self) -> Option<&crate::grant::Github> {
        let github = self.oidc.as_ref().and_then(|oidc| oidc.github.as_ref());
        #[cfg(feature = "accounts")]
        let github = github.or_else(|| self.accounts.as_ref()?.github.as_ref());
        github
    }

    #[cfg(feature = "proposals")]
    fn github_mut(&mut self) -> Option<&mut crate::grant::Github> {
        if let Some(github) = self.oidc.as_mut().and_then(|oidc| oidc.github.as_mut()) {
            return Some(github);
        }
        #[cfg(feature = "accounts")]
        if let Some(github) = self.accounts.as_mut().and_then(|a| a.github.as_mut()) {
            return Some(github);
        }
        None
    }

    #[cfg(feature = "github-app")]
    pub(crate) fn github_apps(
        file: &Path,
    ) -> Result<(
        Option<crate::valkey::Config>,
        Vec<crate::github_app::Config>,
    )> {
        let src = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        modules::check(file, &src)?;
        let parsed: ManifestFile = config::parse(file, &src, "meada.workspace.toml")?;
        let base = file.parent().unwrap_or(Path::new(""));
        let valkey = modules::validated(file, &src, parsed.valkey, |v| v.validate(base))?;
        let has_valkey = valkey.is_some();
        let apps = crate::github_app::validated(file, &src, base, parsed.github_apps, has_valkey)?;
        Ok((valkey, apps))
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(file: &Path) -> Result<Option<crate::postgres::Config>> {
        let src = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        modules::check(file, &src)?;
        let parsed: ManifestFile = config::parse(file, &src, "meada.workspace.toml")?;
        let base = file.parent().unwrap_or(Path::new(""));
        modules::validated(file, &src, parsed.postgres, |p| p.validate(base))
    }

    #[cfg(feature = "snapshot")]
    pub(crate) fn of_generation(dir: &Path, document: &crate::snapshot::Document) -> Result<Self> {
        let entries = (document.repositories.iter())
            .map(|entry| (entry, entry.blocks.clone()))
            .collect();
        Self::of_entries(dir, entries)
    }

    #[cfg(feature = "snapshot")]
    pub(crate) fn of_context(
        dir: &Path,
        document: &crate::snapshot::Document,
        context: &crate::snapshot::Context,
    ) -> Result<Self> {
        let held = document.repositories.iter().chain(&document.members);
        let mut entries = Vec::new();
        for recorded in &context.repositories {
            let found = held.clone().find(|e| e.identity == recorded.repo);
            let shown = dir
                .join(crate::snapshot::pull::DOCUMENT)
                .display()
                .to_string();
            let found = found.ok_or_else(|| Msg::SnapshotNotDocument(&shown).text())?;
            entries.push((found, recorded.blocks.clone()));
        }
        Self::of_entries(dir, entries)
    }

    #[cfg(feature = "snapshot")]
    fn of_entries(
        dir: &Path,
        entries: Vec<(&crate::snapshot::Entry, serde_json::Value)>,
    ) -> Result<Self> {
        let shown = dir
            .join(crate::snapshot::pull::DOCUMENT)
            .display()
            .to_string();
        let not = || Msg::SnapshotNotDocument(&shown).text();
        let mut listed_entries = Vec::new();
        for (entry, recorded) in entries {
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
            let serde_json::Value::Object(mut written) = recorded.clone() else {
                return Err(not().into());
            };
            written.insert("repo".into(), repo.into());
            written.insert("path".into(), "".into());
            let file: EntryFile =
                serde_json::from_value(written.into()).map_err(|e| bad(e.to_string()))?;
            let blocks = file.blocks();
            let max = Some(config::GENERATION_CONFIG_MAX);
            let own = config::read_config_up_to(&root, max);
            let (config, ignored) = match own.map_err(|e| bad(e.to_string()))? {
                Some(own) => (own, blocks),
                None => (config_of(file), Vec::new()),
            };
            listed_entries.push(Listed {
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
                blocks: recorded,
                member: false,
            });
        }
        Ok(Self {
            entries: listed_entries,
            index_dir: dir.to_path_buf(),
            #[cfg(feature = "valkey")]
            valkey: None,
            snapshot: None,
            #[cfg(feature = "generation-pointer")]
            generations: None,
            #[cfg(feature = "refresh-lock")]
            refresh_lock: None,
            #[cfg(feature = "oidc")]
            oidc: None,
            #[cfg(feature = "ui")]
            ui: None,
            #[cfg(feature = "postgres")]
            postgres: None,
            #[cfg(feature = "directory")]
            directory: None,
            #[cfg(feature = "workspaces")]
            catalog: None,
            #[cfg(feature = "workspaces")]
            workspaces: None,
            #[cfg(feature = "workspaces")]
            contexts: Vec::new(),
            #[cfg(feature = "proposals")]
            proposals: None,
            #[cfg(feature = "accounts")]
            accounts: None,
            #[cfg(feature = "generation-pointer")]
            store: false,
            #[cfg(feature = "generation-pointer")]
            version: None,
        })
    }

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

    pub fn workspace(mut self, first: &str, reads: &dyn Fn(&str) -> bool) -> Result<Workspace> {
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

fn mirror(
    index_dir: &Path,
    path: &Path,
    repo: &str,
    problem: Option<String>,
    listed: Config,
) -> std::result::Result<(Repository, bool), String> {
    if let Some(problem) = problem {
        return Err(problem);
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
    let max = Some(config::GENERATION_CONFIG_MAX);
    let own = config::read_config_up_to(&root, max).map_err(|e| e.to_string())?;
    let has_own = own.is_some();
    let repository = Repository {
        config: own.unwrap_or(listed),
        decisions: decisions::load_up_to(&root, max).map_err(|e| e.to_string())?,
        index: index_dir.join(repo).join("index.db"),
        root,
        identity: found,
    };
    Ok((repository, has_own))
}

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
