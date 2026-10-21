//! The query engine: opens a workspace from a directory and answers over its indexes.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::contract::{self, ContractKind, Hit, KINDS, Level, Link, Role, Search, View};
pub use crate::decisions::{Cause, Evaluation, Stale, Waiting};
use crate::entry::{self, EntryPoint};
use crate::index::{Export, State, Symbol, schema};
pub use crate::items::{Candidate, Enclosing, ItemKind, OpenItem};
use crate::msg::Msg;
use crate::render::{
    DOCUMENTED_ONLY, Files, REPOSITORY_LEVEL, callee, label, mark, rank, section, unresolved,
};
use crate::shape::{self, Shape};
pub use crate::trace::{Direction, Start};
use crate::workspace::{Manifest, Missing, Repository, Workspace};
use crate::{Result, decisions, document, drift_view, index, items, render, repo, trace};

/// SQLite attaches at most 10 databases to one connection (`SQLITE_MAX_ATTACHED`).
const MAX_PEERS: usize = 10;

pub struct Engine {
    /// The index of the repository where Meada runs, with every other repository of the
    /// workspace attached as `peer<n>`, `n` its position in `workspace.repos`.
    db: Connection,
    workspace: Workspace,
    root: PathBuf,
    identity: String,
    /// Opened from a manifest (server mode): every index read-only, never refreshed.
    read_only: bool,
}

impl Engine {
    /// Opens the workspace of the repository that contains `dir`. Every repository's index
    /// lives inside its own `.meada/`; every query refreshes them first.
    pub fn open(dir: &Path) -> Result<Self> {
        Self::over(Workspace::discover(dir)?)
    }

    /// Opens the workspace the manifest at `manifest` lists (server mode, ADR 0011), `repo`
    /// first, for one answer: every index lives under the manifest's `index_dir`, keyed by
    /// identity, and is opened read-only. Nothing is stat'ed, refreshed, created or written;
    /// only `meada index --workspace` writes. One read transaction lasts as long as the engine,
    /// so open one per answer: it reads each index as one commit left it, and a commit waits
    /// for it. A missing or unreadable index, or one a commit in progress locks past the busy
    /// timeout, is an error naming the repository (`index::open_read`).
    pub fn open_manifest(manifest: &Path, repo: &str) -> Result<Self> {
        Self::read(Manifest::read(manifest)?.workspace(repo)?, manifest)
    }

    /// `open_manifest` over `listed`, the manifest at `manifest` as `served` read it when the
    /// server started: only `repo`'s mirror is checked again (`Repository::recheck`), and every
    /// index is opened as `open_manifest` opens it. A call of `meada mcp --listen`.
    pub(crate) fn open_listed(listed: &Manifest, repo: &str, manifest: &Path) -> Result<Self> {
        let workspace = listed.clone().workspace(repo)?;
        (workspace.repos[0].recheck()).map_err(|problem| format!("{repo}: {problem}"))?;
        Self::read(workspace, manifest)
    }

    /// Opens every index of `workspace`, listed by the manifest at `manifest`, read-only.
    fn read(workspace: Workspace, manifest: &Path) -> Result<Self> {
        fits(&workspace)?;
        let manifest = manifest.display().to_string();
        let repos: Vec<(&Path, &str)> = (workspace.repos.iter())
            .map(|r| (r.index.as_path(), r.identity.as_str()))
            .collect();
        let db = index::open_read(&repos, &manifest)?;
        let here = &workspace.repos[0];
        Ok(Self {
            db,
            root: here.root.clone(),
            identity: here.identity.clone(),
            workspace,
            read_only: true,
        })
    }

    /// Opens every index of `workspace` where its repository says it lives
    /// (`Repository::index`).
    fn over(workspace: Workspace) -> Result<Self> {
        fits(&workspace)?;
        // Opening creates a missing index, or recreates one of another schema, before it is
        // attached.
        for peer in &workspace.repos[1..] {
            index::open(&peer.index, &peer.root)?;
        }
        let here = &workspace.repos[0];
        let db = index::open(&here.index, &here.root)?;
        for (n, peer) in workspace.repos.iter().enumerate().skip(1) {
            let path = peer.index.to_str().ok_or("index path is not UTF-8")?;
            db.execute(&format!("ATTACH DATABASE ?1 AS peer{n}"), [path])?;
        }
        Ok(Self {
            db,
            root: here.root.clone(),
            identity: here.identity.clone(),
            workspace,
            read_only: false,
        })
    }

    /// Brings every index of the workspace up to date with the files on disk (ADR 0004), with
    /// no watcher: only files whose mtime or size changed are hashed. Each repository is
    /// refreshed on its own, through its own connection; links are derived from the stored
    /// contracts at query time, so links that touch a changed file follow it. Does nothing on
    /// an engine opened from a manifest: there the operator's `meada index` refreshes.
    pub fn refresh(&self) -> Result<()> {
        if self.read_only {
            return Ok(());
        }
        for peer in &self.workspace.repos[1..] {
            let db = index::open(&peer.index, &peer.root)?;
            index::refresh(&db, &peer.root, &peer.identity, &peer.config.serve)?;
        }
        let here = &self.workspace.repos[0];
        index::refresh(&self.db, &self.root, &self.identity, &here.config.serve)
    }

    /// The canonical export of the repository where Meada runs: its whole index, plus the
    /// links from its consumers and to its providers, as sorted, pretty-printed JSON.
    pub fn export(&self) -> Result<String> {
        self.refresh()?;
        let mut exports = self.exports()?;
        let here = format!("{}::", self.identity);
        let (all, mut evaluations) = self.links_and_decisions(&exports);
        // The processes cross repositories: they walk every link, not only the local ones.
        let graph = trace::Graph::new(&self.workspace, &exports, &all);
        let processes = (exports[0].entry_points.iter())
            .map(|e| graph.export_process(e))
            .collect();
        let links = all.into_iter();
        let links =
            links.filter(|l| l.consumer.starts_with(&here) || l.provider.starts_with(&here));
        let mut export = exports.swap_remove(0);
        export.processes = processes;
        export.links = links.collect();
        export.stale_decisions = evaluations.swap_remove(0).stale;
        Ok(serde_json::to_string_pretty(&export)? + "\n")
    }

    /// The index content of every repository of the workspace, with the contracts its
    /// `meada.toml` declares, in workspace order.
    fn exports(&self) -> Result<Vec<Export>> {
        let repos = self.workspace.repos.iter().enumerate();
        let mut exports = repos
            .map(|(n, m)| {
                let mut export = index::export(&self.db, &schema(n), &m.identity)?;
                export.root = m.root.clone();
                for kind in contract::KINDS {
                    kind.declare(m, &mut export);
                }
                Ok(export)
            })
            .collect::<Result<Vec<_>>>()?;
        // Each kind's step over the whole workspace, once every repository has declared.
        for kind in contract::KINDS {
            kind.workspace_step(&mut exports);
        }
        Ok(exports)
    }

    /// Every link of the workspace, sorted.
    fn links(&self, exports: &[Export]) -> Vec<Link> {
        self.links_and_decisions(exports).0
    }

    /// Every link of the workspace, sorted, with the decisions replayed and the passes of the
    /// kinds after them (`ContractKind::after_links`), and the decisions of each repository
    /// that did not apply, in workspace order.
    fn links_and_decisions(&self, exports: &[Export]) -> (Vec<Link>, Vec<Evaluation>) {
        let (mut links, evaluations) =
            decisions::replay(self.core_links(exports), &self.workspace, exports);
        for kind in contract::KINDS {
            kind.after_links(&mut links, exports);
        }
        (links, evaluations)
    }

    /// Every link of the workspace as the core derives it from the stored contracts, before
    /// any decision.
    fn core_links(&self, exports: &[Export]) -> Vec<Link> {
        let repos = &self.workspace.repos;
        let providers: Vec<_> = exports
            .iter()
            .zip(repos)
            .flat_map(|(e, m)| e.providers.iter().map(move |p| (m, p)))
            .collect();
        let mut links = Vec::new();
        for (export, from) in exports.iter().zip(repos) {
            for consumer in &export.consumers {
                let Some(kind) = consumer.contract.as_deref().and_then(contract::kind_of) else {
                    continue;
                };
                links.extend(kind.link(consumer, from, &providers, exports));
            }
        }
        links
    }

    /// For each repository of the workspace, in workspace order, the decisions that do not
    /// apply with their cause (stale), and those that wait on a missing peer.
    pub fn decision_report(&self) -> Result<Vec<Evaluation>> {
        self.refresh()?;
        Ok(self.links_and_decisions(&self.exports()?).1)
    }

    /// `decision_report`, with, for each repository of the workspace, in workspace order, the
    /// status lines: each kind's own (a `via` that names no symbol, a resource no link reaches),
    /// its documents' (one ignored, a route listed twice) and the shape drift's.
    pub fn status_report(&self) -> Result<(Vec<Evaluation>, Vec<Vec<String>>)> {
        self.refresh()?;
        let exports = self.exports()?;
        let (links, evaluations) = self.links_and_decisions(&exports);
        let repos = &self.workspace.repos;
        let entries = drift_view::entries(repos, &exports, &links);
        let drift = drift_view::counts(repos, &entries);
        let lines = (repos.iter().zip(&exports).zip(drift))
            .map(|((repo, export), drift)| {
                let mut lines: Vec<String> = (contract::KINDS.iter())
                    .flat_map(|kind| kind.status(repo, export, &links))
                    .collect();
                lines.extend(document::report(repo, export));
                if drift > 0 {
                    lines.push(Msg::Drift(drift).text());
                }
                lines
            })
            .collect();
        Ok((evaluations, lines))
    }

    /// The decisions of the repository where Meada runs that are stale or waiting.
    pub fn local_decisions(&self) -> Result<Evaluation> {
        let all = self.decision_report()?;
        Ok(all
            .into_iter()
            .find(|e| e.repository == self.identity)
            .unwrap_or_else(|| Evaluation {
                repository: self.identity.clone(),
                stale: vec![],
                waiting: vec![],
            }))
    }

    /// The root of the repository where Meada runs.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The open items whose call lives in the repository where Meada runs, in id order: the
    /// ones `meada decide` works on.
    pub fn local_items(&self) -> Result<Vec<OpenItem>> {
        let mut items = self.open_items()?;
        items.retain(|i| i.repository == self.identity);
        Ok(items)
    }

    /// The item `id` of the repository where Meada runs, open or with a decision that applies,
    /// as the core sees it (every candidate, as if undecided): what `meada decide --item`
    /// redoes. `None` when the call is no such item.
    pub fn local_item(&self, id: &str) -> Result<Option<OpenItem>> {
        Ok((self.items()?.into_iter())
            .map(|(item, _)| item)
            .find(|i| i.id == id && i.repository == self.identity))
    }

    /// The open items of the workspace, ambiguous links and unresolved consumers that no
    /// decision resolves, in id order, each with its sorted candidates. Callers keep the ones
    /// whose `repository` they own.
    pub fn open_items(&self) -> Result<Vec<OpenItem>> {
        Ok((self.items()?.into_iter())
            .filter_map(|(item, decided)| (!decided).then_some(item))
            .collect())
    }

    /// The items of the workspace as the core sees them, in id order, each with whether a
    /// decision applies to it (its call has a `decided` link once the decisions replay).
    fn items(&self) -> Result<Vec<(OpenItem, bool)>> {
        self.refresh()?;
        let exports = self.exports()?;
        let core = self.core_links(&exports);
        let items = items::list(&self.workspace.repos, &exports, &core);
        let (links, _) = decisions::replay(core, &self.workspace, &exports);
        let decided: HashSet<&str> = (links.iter())
            .filter(|l| l.level == Level::Decided)
            .map(|l| l.consumer.as_str())
            .collect();
        Ok((items.into_iter())
            .map(|item| {
                let applied = decided.contains(item.id.as_str());
                (item, applied)
            })
            .collect())
    }

    /// The chain from `start`, followed `direction` through calls inside repositories and
    /// links between them, across the whole workspace, until it ends, a cycle closes or the
    /// budget runs out; one indented line per call or hop, then the chain footer and any missing
    /// peer. `start` names a symbol, a route (`http::GET::/users/{}`, `GET /users/{}` or
    /// `/users/{}`) or a call site, or takes the symbols the uncommitted changes touch.
    pub fn trace(
        &self,
        start: Start,
        direction: Direction,
        budget: Option<usize>,
    ) -> Result<String> {
        self.refresh()?;
        let exports = self.exports()?;
        let links = self.links(&exports);
        let changes = match start {
            Start::Uncommitted => Some(repo::uncommitted(&self.root)?),
            Start::From(_) => None,
        };
        let graph = trace::Graph::new(&self.workspace, &exports, &links);
        Ok(graph.answer(start, changes.as_deref(), direction, budget))
    }

    /// Symbols of the workspace whose name matches `query` case-insensitively, and contracts
    /// whose identifier or route path does, one line each: exact first, then prefix, then
    /// substring, ties by identity. `kind` keeps only `symbol`, `contract`, `route` (HTTP
    /// contract), `resource` (a declared resource, matched by kind and id) or `channel` (matched
    /// by broker kind and name) matches; `document` lists the documents read, matched by path
    /// or title (see `search_documents`); `package` lists the packages, matched by name (see
    /// `ContractKind::own_search`); `community` lists the communities, matched by name, and `process`
    /// the processes, matched by their entry point's label (`search_communities`,
    /// `search_processes`); `drift` lists the fields consumers read that a route lacks (see
    /// `search_drift`); `ambiguous` and `unresolved` list the open items whose enclosing
    /// symbol or call expression matches, each with the `meada decide` command that resolves
    /// it. The answer is cut at `budget` tokens.
    pub fn search(&self, query: &str, kind: Option<&str>, budget: Option<usize>) -> Result<String> {
        if let Some(item_kind) = kind.and_then(ItemKind::parse) {
            return self.search_items(query, item_kind, budget);
        }
        if kind == Some("document") {
            return self.search_documents(query, budget);
        }
        let selected = kind.and_then(contract::searched);
        if let Some((own, Search::Own(_))) = selected {
            self.refresh()?;
            let exports = self.exports()?;
            let links = self.links(&exports);
            let hits = own.own_search(&exports, &links, &query.to_lowercase());
            return Ok(hits_answer(query, hits, budget));
        }
        if kind == Some("drift") {
            return self.search_drift(query, budget);
        }
        if kind == Some("community") {
            return self.search_communities(query, budget);
        }
        if kind == Some("process") {
            return self.search_processes(query, budget);
        }
        // Whether symbols match, and the prefix of the contracts that do.
        let (symbols, contracts) = match (kind, selected) {
            (None, _) => (true, Some(String::new())),
            (Some("symbol"), _) => (true, None),
            (Some("contract"), _) => (false, Some(String::new())),
            (_, Some((filter, _))) => (false, Some(format!("{}::", filter.prefix()))),
            (Some(other), None) => {
                let mut valid: Vec<_> = kind_words().map(|w| format!("`{w}`")).collect();
                let last = valid.pop().unwrap_or_default();
                let valid = format!("{} or {last}", valid.join(", "));
                return Err(format!("`kind` takes {valid}, not `{other}`").into());
            }
        };
        self.refresh()?;
        let exports = self.exports()?;
        let needle = query.to_lowercase();
        // (rank, identity to break ties, line)
        let mut hits: Vec<Hit> = Vec::new();
        for (export, repo) in exports.iter().zip(&self.workspace.repos) {
            let mut files = Files::new(&repo.root);
            if symbols {
                // ponytail: scans every symbol; an indexed lookup waits for a workspace that
                // makes this slow.
                for symbol in &export.symbols {
                    let name = label(&symbol.id).to_lowercase();
                    let last = name.rsplit('.').next().unwrap_or(&name);
                    if let Some(r) = [name.as_str(), last]
                        .iter()
                        .filter_map(|n| rank(n, &needle))
                        .min()
                    {
                        let line = line(&repo.identity, &mut files, symbol, label(&symbol.id));
                        hits.push((r, symbol.id.clone(), line));
                    }
                }
            }
            let Some(prefix) = &contracts else {
                continue;
            };
            let provided = export
                .providers
                .iter()
                .map(|p| (p.contract.clone(), &p.site));
            // An unresolved consumer has no contract: it shows as `(unresolved) <callee>`.
            let consumed = export.consumers.iter().map(|c| {
                let contract = c.contract.clone();
                (contract.unwrap_or_else(|| unresolved(&c.site)), &c.site)
            });
            for (contract, site) in provided.chain(consumed) {
                if !contract.starts_with(prefix) {
                    continue;
                }
                // The identifier ranks whole and by the parts its kind gives: `/users` is exact
                // for `http::GET::/users`.
                let lower = contract.to_lowercase();
                let (kind, id) = contract::parts(&lower);
                let Some(r) = [Some(lower.as_str()), kind, id]
                    .into_iter()
                    .flatten()
                    .filter_map(|n| rank(n, &needle))
                    .min()
                else {
                    continue;
                };
                let span = export
                    .symbols
                    .iter()
                    .find(|s| s.id == *site)
                    .map(|s| &s.span);
                let span =
                    span.or_else(|| export.calls.iter().find(|c| c.id == *site).map(|c| &c.span));
                let location = match span {
                    Some(span) => files.at(&span.path, span.start),
                    None if *site == repo.identity => REPOSITORY_LEVEL.into(),
                    None => match export.document_of(site) {
                        // A route or channel side only a document lists.
                        Some(doc) => format!("{doc} {DOCUMENTED_ONLY}"),
                        None => contract::place(&repo.identity, site).unwrap_or_else(|| {
                            // A route file that serves the contract with no function of its own.
                            site.rsplit_once("::")
                                .map_or(site.as_str(), |(_, p)| p)
                                .to_string()
                        }),
                    },
                };
                let line = format!(
                    "{} {location} {}",
                    repo.identity,
                    contract::shown(&contract)
                );
                hits.push((r, format!("{contract} {site}"), line));
            }
        }
        Ok(hits_answer(query, hits, budget))
    }

    /// The documents read (spec #21) whose path or `info.title` matches `query`, one line each,
    /// as `<repository> <path> (<format> <version>, routes: <n>) <title>`.
    fn search_documents(&self, query: &str, budget: Option<usize>) -> Result<String> {
        self.refresh()?;
        let needle = query.to_lowercase();
        // (rank, path to break ties, line)
        let mut hits: Vec<Hit> = Vec::new();
        for (export, repo) in self.exports()?.iter().zip(&self.workspace.repos) {
            for doc in &export.documents {
                let Some(shown) = doc.summary.shown() else {
                    continue;
                };
                let title = doc.summary.title.as_deref();
                let path = doc.path.to_lowercase();
                let title_lower = title.map(str::to_lowercase);
                let Some(r) = [Some(path.as_str()), title_lower.as_deref()]
                    .into_iter()
                    .flatten()
                    .filter_map(|n| rank(n, &needle))
                    .min()
                else {
                    continue;
                };
                let title = title.map_or(String::new(), |t| format!(" {t}"));
                let line = format!("{} {} ({shown}){title}", repo.identity, doc.path);
                hits.push((r, format!("{} {}", repo.identity, doc.path), line));
            }
        }
        Ok(hits_answer(query, hits, budget))
    }

    /// The drift (spec #23) whose route path or missing field path matches `query`, one line
    /// per missing path, as `<repository> <path>:<line> <caller> <route> missing <path>
    /// (<level>)`.
    fn search_drift(&self, query: &str, budget: Option<usize>) -> Result<String> {
        self.refresh()?;
        let exports = self.exports()?;
        let links = self.links(&exports);
        let entries = drift_view::entries(&self.workspace.repos, &exports, &links);
        let hits = drift_view::matches(&self.workspace.repos, &entries, &query.to_lowercase());
        Ok(hits_answer(query, hits, budget))
    }

    /// The communities (spec #31) whose name matches `query`, one line each, as `<repository>
    /// <name> (members: <n>)`.
    fn search_communities(&self, query: &str, budget: Option<usize>) -> Result<String> {
        self.refresh()?;
        let needle = query.to_lowercase();
        // (rank, repository and name to break ties, line)
        let mut hits: Vec<Hit> = Vec::new();
        for export in self.exports()? {
            for c in &export.communities {
                let Some(r) = rank(&c.name.to_lowercase(), &needle) else {
                    continue;
                };
                let (repository, n) = (&export.repository, c.members.len());
                let line = format!("{repository} {} (members: {n})", c.name);
                hits.push((r, format!("{repository} {}", c.name), line));
            }
        }
        Ok(hits_answer(query, hits, budget))
    }

    /// The processes (spec #31) whose entry point's label matches `query`, one line each, as
    /// `<repository> <label> process::<identity> (<kind>)`: the identity `trace` starts from.
    fn search_processes(&self, query: &str, budget: Option<usize>) -> Result<String> {
        self.refresh()?;
        let needle = query.to_lowercase();
        // (rank, identity to break ties, line)
        let mut hits: Vec<Hit> = Vec::new();
        for export in self.exports()? {
            for e in &export.entry_points {
                let Some(r) = rank(&e.label.to_lowercase(), &needle) else {
                    continue;
                };
                let (repository, kind) = (&export.repository, e.kind.name());
                let line = format!("{repository} {} process::{} ({kind})", e.label, e.identity);
                hits.push((r, e.identity.clone(), line));
            }
        }
        Ok(hits_answer(query, hits, budget))
    }

    /// The open items of `kind` whose enclosing symbol or call expression matches `query`.
    fn search_items(&self, query: &str, kind: ItemKind, budget: Option<usize>) -> Result<String> {
        let needle = query.to_lowercase();
        // (rank, id, line)
        let mut hits: Vec<Hit> = Vec::new();
        for item in self.open_items()?.into_iter().filter(|i| i.kind == kind) {
            let name = item.enclosing.as_ref().map(|e| e.name.to_lowercase());
            let last = name.as_deref().and_then(|n| n.rsplit('.').next());
            let Some(r) = [
                name.as_deref(),
                last,
                Some(&*item.expression.to_lowercase()),
            ]
            .into_iter()
            .flatten()
            .filter_map(|n| rank(n, &needle))
            .min() else {
                continue;
            };
            let line = format!(
                "{} {} ({}) candidates: {}; run `meada decide --item {}`",
                item.repository,
                item.location(),
                kind.name(),
                item.candidates.len(),
                item.id
            );
            hits.push((r, item.id, line));
        }
        Ok(hits_answer(query, hits, budget))
    }

    /// The resource, channel or route `contract`: one line per provider and consumer across the
    /// workspace, providers first, as `<repository> <role> <path>:<line> <name>`, where `role`
    /// is `writer` or `reader` (a resource, `<name>` its `via` symbol), `producer` or
    /// `consumer` (a channel, `<name>` the symbol around the producing call, or the handler) or
    /// `provider` (a route, `<name>` its handler; its consumers are calls, which `trace` and
    /// the `contracts:` of their symbols show, so `consumer` is `None` and they are left out).
    /// A side only a document lists is `<repository> <role> <document> (documented only)`;
    /// the shape a document gives a side follows it, `response` for a route and `payload`
    /// for a channel.
    /// A side declared with no `via` is `<repository> <role> (repository-level)`, a handler at
    /// the top level of a file `<repository> <role> <path> (top level)`. The kind also lists the
    /// sides linked to the contract through another name (`ContractKind::related_sides`), each
    /// followed by `[<level>: <reason>]`; a channel's are the producers a consumer's wildcard
    /// pattern matches, the wildcard consumers that match it, and the publishes its bindings
    /// route to it, then the bindings themselves.
    fn contract_sides(
        &self,
        exports: &[Export],
        contract: &str,
        View {
            provider,
            consumer,
            carried,
            ..
        }: View,
        kind: &dyn ContractKind,
    ) -> Vec<String> {
        // Providers, then consumers, then the bindings that reach a queue; each with the lines
        // of the payload shape a document gives it.
        let mut sides: Vec<(u8, String, Vec<String>)> = Vec::new();
        for (export, repo) in exports.iter().zip(&self.workspace.repos) {
            let mut files = Files::new(&repo.root);
            // Each side that reaches `contract`: its site, order, role words, tag and shape.
            // The sides that hold it come from the engine, the others from the kind.
            let provided = export.providers.iter().filter(|p| p.contract == contract);
            let provided =
                provided.map(|p| (p.site.as_str(), 0, provider.into(), p.shape.as_ref()));
            let consumed = export.consumers.iter().filter_map(|c| {
                let words = consumer.filter(|_| c.contract.as_deref() == Some(contract))?;
                Some((c.site.as_str(), 1, words.into(), c.shape.as_ref()))
            });
            let own = provided
                .chain(consumed)
                .map(|(site, order, role, shape)| (site, order, role, String::new(), shape));
            let related = kind.related_sides(exports, export, contract);
            let related = related.into_iter().map(|r| {
                let tag = r.matched.map_or_else(String::new, |(level, reason)| {
                    format!(" [{}: {reason}]", level.name())
                });
                let order = match r.role {
                    Role::Provider => 0,
                    Role::Consumer => 1,
                    _ => 2,
                };
                (r.site, order, r.words, tag, r.shape)
            });
            for (site, order, role, tag, shape) in own.chain(related) {
                let place = if let Some(s) = export.symbols.iter().find(|s| s.id == site) {
                    let at = files.at(&s.span.path, s.span.start);
                    format!("{at} {}", label(&s.id))
                } else if let Some(c) = export.calls.iter().find(|c| c.id == site) {
                    let at = files.at(&c.span.path, c.span.start);
                    let name = export.symbols.iter().find(|s| s.id == c.caller);
                    format!("{at} {}", name.map_or("(top level)", |s| label(&s.id)))
                } else if site == repo.identity {
                    REPOSITORY_LEVEL.into()
                } else if let Some(doc) = export.document_of(site) {
                    // A side only a document lists.
                    format!("{doc} {DOCUMENTED_ONLY}")
                } else if let Some(place) = contract::place(&repo.identity, site) {
                    // A dependency entry of a manifest.
                    place
                } else {
                    format!("{} (top level)", label(site))
                };
                let shape =
                    shape.map_or_else(Vec::new, |s| shape_lines(&repo.identity, s, carried));
                let line = format!("{} {role} {place}{tag}", repo.identity);
                sides.push((order, line, shape));
            }
        }
        if sides.is_empty() {
            return vec![format!("no {} matches `{contract}`", kind.prefix())];
        }
        sides.sort();
        let mut lines = vec![contract::shown(contract)];
        for (_, line, shape) in sides {
            lines.push(format!("  {line}"));
            lines.extend(shape.into_iter().map(|l| format!("    {l}")));
        }
        lines
    }

    /// One symbol of the workspace: its community (`community: <name>` or `none`) and the
    /// processes it belongs to (`processes: none`, or a summary `processes: <n> (<k> route, <k>
    /// export; <r> repositories)` and the entries that fit, `<label> (<repository>), ..., +<m>
    /// more`, found by walking upstream), both right after its first line; then its span text
    /// read from disk (only while the file still has the indexed hash), its callers and its
    /// callees. The first line is `<repository> <path>:<start>-<end> <name>`, the span's range
    /// in its file; `<path>:<start>` when the span text is unavailable, `<path>` when the file
    /// cannot be read. `symbol` is an identity, `<path>::<name>` or a name; when it fits several
    /// symbols, they are listed instead of guessed. The answer is cut at `budget` tokens, from
    /// the last to give up: the first line, the community, the processes line (its summary, then
    /// as many entries as fit), the callers, callees, consumers and contracts, and the span
    /// text, which keeps its leading lines and ends with the cut note of `render::cut`, or is
    /// left out when not even that note fits (the agent reads it from the lines the first
    /// line's range names).
    pub fn context(&self, symbol: &str, budget: Option<usize>) -> Result<String> {
        self.refresh()?;
        // A read-only engine never refreshes, so it has nothing to retry with.
        self.context_fresh(symbol, budget, !self.read_only)
    }

    fn context_fresh(&self, symbol: &str, budget: Option<usize>, retry: bool) -> Result<String> {
        let exports = self.exports()?;
        let repos = &self.workspace.repos;
        // A contract listed by its sides, as `search` prints it. One its kind lists by its
        // providers alone (its consumers are calls) is followed by the linked consumers that
        // read fields of the carried shape, with their drift, placed at their call (`View`).
        let contract = contract::typed(symbol);
        if let Some(kind) = contract::kind_of(contract)
            && let Some(view) = kind.view(contract)
        {
            let mut lines = self.contract_sides(&exports, contract, view, kind);
            if view.follows_drift() {
                let links = self.links(&exports);
                let entries = drift_view::entries(repos, &exports, &links);
                lines.extend(drift_view::consumers(repos, &entries, contract));
            }
            return Ok(render::budgeted(&lines, budget));
        }
        let suffix = format!("::{symbol}");
        let found: Vec<(usize, &Symbol)> = (exports.iter().enumerate())
            .flat_map(|(n, e)| e.symbols.iter().map(move |s| (n, s)))
            .filter(|(_, s)| s.id == symbol || s.id.ends_with(&suffix))
            .collect();
        let [(n, symbol_found)] = found[..] else {
            if found.is_empty() {
                return Ok(format!("no symbol matches `{symbol}`\n"));
            }
            let short = |s: &Symbol| format!("{}::{}", s.span.path, label(&s.id));
            let mut lines = vec![format!(
                "`{symbol}` fits {} symbols; pass one of:",
                found.len()
            )];
            lines.extend(found.iter().map(|&(n, s)| {
                // `<path>::<name>`, or the identity when another repository has the same one.
                let name = match found.iter().filter(|(_, o)| short(o) == short(s)).count() {
                    1 => short(s),
                    _ => s.id.clone(),
                };
                let mut files = Files::new(&repos[n].root);
                line(&repos[n].identity, &mut files, s, &name)
            }));
            return Ok(render::budgeted(&lines, budget));
        };
        let (repo, export) = (&repos[n], &exports[n]);
        let text = span_text(&repo.root, export, symbol_found);
        if text.is_none() && retry {
            // The file changed without its mtime or size showing it: hash it again.
            index::forget(
                &index::open(&repo.index, &repo.root)?,
                &symbol_found.span.path,
            )?;
            self.refresh()?;
            return self.context_fresh(symbol, budget, false);
        }
        let mut files = Files::new(&repo.root);
        let identity = &repo.identity;
        let name = label(&symbol_found.id);
        let header = match &text {
            // The range holds exactly the lines the cut note counts (`render::cut` takes its
            // `<m>` from `span.len()`), so a cut or omitted span is read from those lines.
            Some((first, span)) => format!(
                "{identity} {}:{first}-{} {name}",
                symbol_found.span.path,
                first + span.len().saturating_sub(1)
            ),
            // The stored offsets no longer describe the file.
            None => line(identity, &mut files, symbol_found, name),
        };
        let links = self.links(&exports);
        let community = (export.communities.iter())
            .find(|c| c.members.contains(&symbol_found.id))
            .map_or("none", |c| c.name.as_str());
        let community = format!("community: {community}");
        // Without the span text there is no span: the note is a line of the tail, kept in its
        // output position and ahead of the callers, so the cut never rewrites it.
        // In server mode nothing refreshes, so the file moved ahead of the index (ADR 0011).
        let unavailable = text.is_none().then(|| match self.read_only {
            true => "index behind the mirror: run meada index --workspace".to_string(),
            false => format!(
                "(span text unavailable: {} changed since indexing)",
                symbol_found.span.path
            ),
        });
        let span = text.map(|(_, span)| span).unwrap_or_default();
        let mut tail = Vec::from_iter(unavailable);
        tail.extend(relations(identity, export, &mut files, &symbol_found.id));
        let sections = (KINDS.iter())
            .flat_map(|k| k.context_sections(&exports, repos, &links, &symbol_found.id));
        tail.extend(sections);
        let workspace = (exports.as_slice(), links.as_slice());
        tail.extend(contracts(
            identity,
            export,
            workspace,
            &mut files,
            symbol_found,
        ));
        let graph = trace::Graph::new(&self.workspace, &exports, &links);
        let found = graph.processes_of(&symbol_found.id);
        let bytes = |lines: &[String]| lines.iter().map(|l| render::line_bytes(l)).sum::<usize>();
        let cap = render::bytes(budget.unwrap_or(render::DEFAULT_BUDGET));
        let head = [header, community];
        // An answer that fits is cut nowhere, with every process named.
        let full = processes_line(&found, repos, usize::MAX);
        let whole: Vec<String> = (head.iter().cloned())
            .chain([full])
            .chain(span.iter().cloned())
            .chain(tail.iter().cloned())
            .collect();
        if bytes(&whole) <= cap {
            return Ok(render::budgeted(&whole, budget));
        }
        // Otherwise the cut order, from the last to give up: header, community, the processes
        // line (its summary, then the entries that fit), the sections after the span, the span
        // text. The processes line may take what the head leaves less the widest note the final
        // cut can write, so that cut never reaches it.
        let total = head.len() + 1 + span.len() + 1 + tail.len();
        let widest = render::note(total, total, budget.unwrap_or(render::DEFAULT_BUDGET));
        let room = cap.saturating_sub(bytes(&head) + 1 + widest.len());
        let processes = processes_line(&found, repos, room);
        let left = cap.saturating_sub(bytes(&head) + render::line_bytes(&processes));
        // The span gives up first: it keeps the lines the tail leaves, then its own note. With no
        // room even for the note it is left out, and the agent reads it from the lines the
        // header's range names.
        let span_room = left.saturating_sub(bytes(&tail));
        let (_, span) = render::cut(&span, span_room, budget.unwrap_or(render::DEFAULT_BUDGET));
        let span = if span.len() > span_room {
            String::new()
        } else {
            span
        };
        let mut lines = Vec::from(head);
        lines.push(processes);
        lines.extend(span.lines().map(str::to_string));
        lines.extend(tail);
        Ok(render::budgeted(&lines, budget))
    }
}

/// `contracts:` with what `symbol` provides, what its calls produce (a provider at a call, then
/// what the kinds add for the call) and the contracts its calls consume, unresolved ones
/// included, each followed by its decided links; `exports` and `links` are the workspace's.
/// Nothing when it has none.
fn contracts(
    identity: &str,
    export: &Export,
    (exports, links): (&[Export], &[Link]),
    files: &mut Files,
    symbol: &Symbol,
) -> Vec<String> {
    let mut lines = Vec::new();
    for p in export.providers.iter().filter(|p| p.site == symbol.id) {
        lines.push(line(
            identity,
            files,
            symbol,
            &format!("provides {}", p.contract),
        ));
        lines.extend(
            p.shape
                .as_ref()
                .map_or_else(Vec::new, |s| shape_lines(identity, s, "response")),
        );
    }
    // A provider can sit at a call (a channel producer): the symbol around it produces. A
    // kind may add lines for the call.
    for call in export.calls.iter().filter(|c| c.caller == symbol.id) {
        let location = files.at(&call.span.path, call.span.start);
        for provider in export.providers.iter().filter(|p| p.site == call.id) {
            let contract = contract::shown(&provider.contract);
            lines.push(format!("{identity} {location} produces {contract}"));
        }
        let added = KINDS
            .iter()
            .flat_map(|k| k.at_call(exports, export, &call.id));
        lines.extend(added.map(|line| format!("{identity} {location} {line}")));
    }
    // A resource reader or a channel handler is a symbol, not a call site.
    let readers = export.consumers.iter().filter(|c| c.site == symbol.id);
    for contract in readers.filter_map(|c| c.contract.as_ref()) {
        let contract = contract::shown(contract);
        lines.push(line(
            identity,
            files,
            symbol,
            &format!("consumes {contract}"),
        ));
    }
    for consumer in &export.consumers {
        let Some(call) = export
            .calls
            .iter()
            .find(|c| c.id == consumer.site && c.caller == symbol.id)
        else {
            continue;
        };
        let location = files.at(&call.span.path, call.span.start);
        let decided = links
            .iter()
            .filter(|l| l.consumer == consumer.site && l.level == Level::Decided)
            .filter(|l| consumer.contract.as_ref().is_none_or(|c| *c == l.contract));
        let mut any = false;
        for link in decided {
            any = true;
            lines.push(format!(
                "{identity} {location} consumes {} →[{}: {}] {}",
                link.contract,
                link.level.name(),
                link.reason,
                link.provider
            ));
        }
        if !any {
            let contract = consumer.contract.clone();
            let contract = contract.unwrap_or_else(|| unresolved(&consumer.site));
            lines.push(format!("{identity} {location} consumes {contract}"));
        }
    }
    if lines.is_empty() {
        return lines;
    }
    section("contracts", lines)
}

/// What a route's response (or a channel's payload: `what`) carries, after its `provides` line:
/// the fields and the parts left open, each line naming where the shape comes from.
fn shape_lines(identity: &str, shape: &Shape, what: &str) -> Vec<String> {
    let at = shape.source.at();
    let list = |paths: &BTreeSet<String>| {
        let paths: Vec<&str> = paths.iter().map(|p| shape::shown(p)).collect();
        paths.join(", ")
    };
    let fields = list(&shape.fields);
    let mut lines = vec![
        format!("{identity} {at} {what} fields: {fields}")
            .trim_end()
            .to_string(),
    ];
    if !shape.open.is_empty() {
        lines.push(format!(
            "{identity} {at} {what} open: {}",
            list(&shape.open)
        ));
    }
    lines
}

/// The span's first line in its file and its lines, or `None` when its file under `root` no
/// longer has the indexed hash.
fn span_text(root: &Path, export: &Export, symbol: &Symbol) -> Option<(usize, Vec<String>)> {
    let path = &symbol.span.path;
    let indexed = export
        .files
        .iter()
        .find(|f| f.path == *path)
        .map(|f| &f.hash);
    let text = fs::read(root.join(path))
        .ok()
        .filter(|src| indexed.is_some_and(|h| blake3::hash(src).to_hex().as_str() == h.as_str()));
    let src = text?;
    let span = src.get(symbol.span.start as usize..symbol.span.end as usize)?;
    let lines = String::from_utf8_lossy(span)
        .lines()
        .map(str::to_string)
        .collect();
    Some((render::line_of(&src, symbol.span.start), lines))
}

/// `callers:` and `callees:` of the symbol `id`, inside the repository `identity`.
fn relations(identity: &str, export: &Export, files: &mut Files, id: &str) -> Vec<String> {
    let symbols: HashMap<&str, &Symbol> =
        export.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut callers: Vec<_> = export
        .calls
        .iter()
        .filter(|c| c.target.as_deref() == Some(id) || c.candidates.iter().any(|t| t == id))
        .collect();
    callers.sort_by_key(|c| (&c.span.path, c.span.start));
    let callers: Vec<String> = callers
        .iter()
        .map(|c| {
            let name = symbols
                .contains_key(c.caller.as_str())
                .then(|| label(&c.caller));
            let location = files.at(&c.span.path, c.span.start);
            format!(
                "{identity} {location} {}{}",
                name.unwrap_or("(top level)"),
                mark(c)
            )
        })
        .collect();
    let mut callees: Vec<_> = export.calls.iter().filter(|c| c.caller == id).collect();
    callees.sort_by_key(|c| (c.target.is_none(), &c.id));
    let mut callees: Vec<String> = callees
        .iter()
        .flat_map(|c| {
            // An ambiguous call lists every candidate, marked, instead of a guess.
            let targets: Vec<&Symbol> = c
                .target
                .iter()
                .chain(&c.candidates)
                .filter_map(|t| symbols.get(t.as_str()).copied())
                .collect();
            if targets.is_empty() {
                let callee = callee(&c.id);
                let location = files.at(&c.span.path, c.span.start);
                return vec![format!("{identity} {location} (unresolved) {callee}")];
            }
            targets
                .iter()
                .map(|t| line(identity, files, t, label(&t.id)) + &mark(c))
                .collect()
        })
        .collect();
    callees.dedup();
    let mut lines = section("callers", callers);
    lines.extend(section("callees", callees));
    lines
}

/// `<repository> <path>:<line> <name>` for a symbol's definition.
fn line(identity: &str, files: &mut Files, symbol: &Symbol, name: &str) -> String {
    let location = files.at(&symbol.span.path, symbol.span.start);
    format!("{identity} {location} {name}")
}

/// The workspace of the repository containing `dir`: each repository with where it is,
/// relative to the one Meada runs in, and its index state, each followed by the decisions of
/// its `meada.decisions.toml` that are stale or wait on a missing peer; then every peer not
/// found. It never creates an index: only when every repository is indexed at the current
/// schema does it refresh them, like a query, to evaluate the decisions against the files as they
/// are; otherwise it reports the states and no decision.
pub fn status(dir: &Path) -> Result<String> {
    let workspace = Workspace::discover(dir)?;
    let here = workspace.repos[0].root.clone();
    let indexed =
        (workspace.repos.iter()).all(|m| matches!(index::state(&m.index), State::Indexed { .. }));
    // A failure to open or refresh drops the decision and status lines, not the whole status.
    let report = match indexed.then(|| Engine::open(dir)) {
        Some(Ok(engine)) => engine.status_report().unwrap_or_default(),
        _ => Default::default(),
    };
    let heads = (workspace.repos.iter()).map(|m| {
        let state = state_text(index::state(&m.index), Msg::OtherVersion);
        let line = format!("{} ({}) {state}", m.identity, relative(&here, &m.root));
        (m.identity.clone(), line)
    });
    Ok(status_lines(heads, report, &workspace.missing))
}

/// `status` in server mode (ADR 0011): every repository the manifest at `manifest` lists, in
/// manifest order, with its `path` as the manifest writes it and its index state, or why it
/// cannot be served; then every peer a `meada.toml` names that the manifest does not list, and
/// every entry block a mirror's own `meada.toml` overrides. The decision and status lines come,
/// as locally, only when every repository is indexed at the current schema. It only reads, as
/// `Engine::open_manifest` does.
pub fn status_workspace(manifest: &Path) -> Result<String> {
    let listed = Manifest::read(manifest)?;
    let missing = listed.missing();
    let ignored: String = (listed.entries.iter())
        .flat_map(|l| l.ignored.iter().map(|block| (block, &l.repo)))
        .map(|(block, repo)| Msg::IgnoredBlock { block, repo }.text() + "\n")
        .collect();
    let rows: Vec<_> = (listed.entries.iter())
        .map(|l| {
            let index = (l.repository.as_ref())
                .map(|r| r.index.clone())
                .map_err(String::clone);
            (l.repo.clone(), l.path.clone(), index)
        })
        .collect();
    let indexed = (rows.iter()).all(|(_, _, index)| {
        matches!(
            index.as_ref().map(|i| index::state(i)),
            Ok(State::Indexed { .. })
        )
    });
    let first = rows.first().map(|(repo, _, _)| repo.clone());
    let report = match (indexed, first) {
        (true, Some(first)) => (listed.workspace(&first))
            .and_then(|workspace| Engine::read(workspace, manifest))
            .and_then(|engine| engine.status_report())
            .unwrap_or_default(),
        _ => Default::default(),
    };
    let heads = rows.into_iter().map(|(repo, path, index)| {
        let state = match index {
            Ok(index) => state_text(index::state(&index), Msg::OtherVersionWorkspace),
            Err(problem) => problem,
        };
        let line = format!("{repo} ({path}) {state}");
        (repo, line)
    });
    Ok(status_lines(heads, report, &missing) + &ignored)
}

/// `meada index --workspace` (ADR 0011): refreshes every repository the manifest at `manifest`
/// lists into its index under `index_dir`, one transaction each, creating or rebuilding a
/// missing index or one of another schema. Returns the problems, one `<repo>: <problem>` per
/// repository that could not be refreshed, in manifest order; one does not stop the others.
pub fn index_workspace(manifest: &Path) -> Result<Vec<String>> {
    let listed = Manifest::read(manifest)?;
    let refresh = |r: &Repository| {
        let db = index::open(&r.index, &r.root)?;
        index::refresh(&db, &r.root, &r.identity, &r.config.serve)
    };
    Ok((listed.entries.iter())
        .filter_map(|l| {
            let problem = match &l.repository {
                Ok(r) => refresh(r).err()?.to_string(),
                Err(problem) => problem.clone(),
            };
            Some(format!("{}: {problem}", l.repo))
        })
        .collect())
}

/// The manifest at `manifest` as `meada mcp --listen` serves it, once every repository is
/// servable: its mirror is a checkout of the identity listed and its index opens read-only at
/// the current schema version. Otherwise every problem, one per line, each index problem naming
/// `meada index --workspace`. It only reads.
pub fn served(manifest: &Path) -> Result<Manifest> {
    let listed = Manifest::read(manifest)?;
    let shown = manifest.display().to_string();
    let problems: Vec<String> = (listed.entries.iter())
        .filter_map(|l| match &l.repository {
            Ok(r) => (index::open_read(&[(&r.index, &r.identity)], &shown))
                .err()
                .map(|e| e.to_string()),
            Err(problem) => Some(format!("{}: {problem}", l.repo)),
        })
        .collect();
    if !problems.is_empty() {
        return Err(problems.join("\n").into());
    }
    // The whole workspace opens too: what a call opens, whichever repository it names.
    let first = (listed.entries.first()).ok_or_else(|| format!("{shown}: no repository listed"))?;
    Engine::open_listed(&listed, &first.repo, manifest)?;
    Ok(listed)
}

/// How long the readiness probe waits for a commit in progress: a writer must not stall it.
pub const PROBE_BUSY: std::time::Duration = std::time::Duration::from_millis(50);

/// Whether a node can serve, for `status --workspace` (exit 0 or 1): `served` passes, or it fails
/// only because an index is locked by a commit in progress (the file exists and the running
/// server answers once the lock clears). Run it under `with_busy_timeout(PROBE_BUSY, ..)`.
pub fn ready(manifest: &Path) -> bool {
    if served(manifest).is_ok() {
        return true;
    }
    let Ok(listed) = Manifest::read(manifest) else {
        return false;
    };
    let states: Vec<_> = (listed.entries.iter())
        .map(|l| l.repository.as_ref().map(|r| index::state(&r.index)))
        .collect();
    (states.iter()).any(|s| matches!(s, Ok(State::Locked)))
        && (states.iter()).all(|s| matches!(s, Ok(State::Indexed { .. } | State::Locked)))
}

/// An index state as `status` shows it, `other` for an index of another schema.
fn state_text(state: State, other: Msg) -> String {
    match state {
        State::Missing => Msg::NotIndexed.text(),
        State::OtherSchema => other.text(),
        State::Locked => Msg::StateLocked.text(),
        State::Indexed { files } => Msg::Indexed(files).text(),
    }
}

/// `status`'s text: each repository's line, by identity in workspace order, followed by its
/// decisions that are stale or waiting and its status lines (`Engine::status_report`, empty
/// when it did not run), then the missing peers.
fn status_lines(
    heads: impl Iterator<Item = (String, String)>,
    (evaluations, status_lines): (Vec<Evaluation>, Vec<Vec<String>>),
    missing: &[Missing],
) -> String {
    let mut lines = Vec::new();
    for (n, (identity, head)) in heads.enumerate() {
        lines.push(head);
        for e in evaluations.iter().filter(|e| e.repository == identity) {
            lines.extend(e.lines().map(|l| format!("  {l}")));
        }
        lines.extend((status_lines.get(n).into_iter().flatten()).map(|l| format!("  {l}")));
    }
    lines.extend(missing.iter().map(Missing::text));
    lines.iter().map(|l| format!("{l}\n")).collect()
}

/// The `kind` words of `search` the engine answers itself: the first two come before the kinds'
/// filter words, the rest after the kinds' own renderers (`document` sits between the two).
const WORDS_BEFORE: [&str; 2] = ["symbol", "contract"];
const WORDS_AFTER: [&str; 5] = ["drift", "community", "process", "ambiguous", "unresolved"];

/// Every `kind` word of `search`, in the order its error names them: the engine's own, the
/// kinds' filter words in kinds order, `document`, the kinds' own-renderer words, then the rest
/// of the engine's.
pub(crate) fn kind_words() -> impl Iterator<Item = &'static str> {
    let kinds = |own: bool| {
        (KINDS.iter().map(|k| k.search_word()))
            .filter(move |s| matches!(s, Search::Own(_)) == own)
            .map(Search::word)
    };
    (WORDS_BEFORE.into_iter())
        .chain(kinds(false))
        .chain(["document"])
        .chain(kinds(true))
        .chain(WORDS_AFTER)
}

/// Refuses a workspace larger than one connection attaches.
fn fits(workspace: &Workspace) -> Result<()> {
    if workspace.repos.len() > MAX_PEERS + 1 {
        // ponytail: one connection attaches every peer, so a workspace reads at most 10
        // peers; batched ATTACH (detach one batch of peers, attach the next) lifts it.
        return Err(Msg::TooManyRepos {
            found: workspace.repos.len(),
            max: MAX_PEERS + 1,
        }
        .text()
        .into());
    }
    Ok(())
}

/// `to` as a path from `from`, with `/` separators: `.`, `../api`.
fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// `processes: none`, or `processes: <n> (<k> <kind>, ...; <r> repositories)` for the processes
/// `found`, a summary present whenever there is one, then `: ` and the entries that fit in
/// `room` bytes, in order, and `, +<m> more` for the rest. When not even the first entry fits
/// the summary stands alone. An entry is `<label> (<repository>)`, or its `process::` identity
/// when another process of the same repository has the same label.
fn processes_line(found: &[(usize, &EntryPoint)], repos: &[Repository], room: usize) -> String {
    if found.is_empty() {
        return "processes: none".into();
    }
    let kinds: Vec<String> = (entry::EntryKind::ALL.iter())
        .filter_map(|k| {
            let n = found.iter().filter(|(_, e)| e.kind == *k).count();
            (n > 0).then(|| format!("{n} {}", k.name()))
        })
        .collect();
    let repositories = (found.iter().map(|(n, _)| n).collect::<HashSet<_>>()).len();
    let summary = format!(
        "processes: {} ({}; {repositories} {})",
        found.len(),
        kinds.join(", "),
        if repositories == 1 {
            "repository"
        } else {
            "repositories"
        }
    );
    let entries: Vec<String> = (found.iter())
        .map(|&(n, e)| {
            let same = found.iter().filter(|(m, o)| *m == n && o.label == e.label);
            match same.count() {
                1 => format!("{} ({})", e.label, repos[n].identity),
                _ => format!("process::{}", e.identity),
            }
        })
        .collect();
    let mut kept = Vec::new();
    // The length of the summary and the entries kept, joined.
    let mut used = summary.len() + ": ".len();
    for (i, entry) in entries.iter().enumerate() {
        let len = used + if kept.is_empty() { 0 } else { ", ".len() } + entry.len();
        let rest = entries.len() - i - 1;
        let more = if rest > 0 {
            format!(", +{rest} more").len()
        } else {
            0
        };
        if len + more > room {
            break;
        }
        kept.push(entry.clone());
        used = len;
    }
    if kept.is_empty() {
        return summary;
    }
    let rest = entries.len() - kept.len();
    if rest > 0 {
        kept.push(format!("+{rest} more"));
    }
    format!("{summary}: {}", kept.join(", "))
}

/// The `hits` `(rank, tie-break key, line)` sorted and cut at `budget`, or the answer for none.
fn hits_answer(query: &str, mut hits: Vec<Hit>, budget: Option<usize>) -> String {
    hits.sort();
    if hits.is_empty() {
        return format!("no matches for `{query}`\n");
    }
    let lines: Vec<String> = hits.into_iter().map(|(_, _, line)| line).collect();
    render::budgeted(&lines, budget)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;

    use super::{Engine, kind_words};

    /// No kind's search word is an engine word or another kind's: each `kind` selects one answer.
    #[test]
    fn no_search_word_of_a_kind_collides_with_an_engine_word() {
        let words: Vec<_> = kind_words().collect();
        let distinct: HashSet<_> = words.iter().collect();
        assert_eq!(distinct.len(), words.len(), "{words:?}");
    }

    #[test]
    fn a_span_changed_since_indexing_keeps_the_start_alone_in_the_first_line() {
        let dir = std::env::temp_dir().join(format!("meada-engine-stale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.ts"),
            "export function add() {\n  return 1;\n}\n",
        )
        .unwrap();
        let engine = Engine::open(&dir).unwrap();
        engine.refresh().unwrap();
        // The retry of `context` would refresh the index: ask without it, as a race would.
        fs::write(
            dir.join("a.ts"),
            "export function add() {\n  return 2;\n}\n",
        )
        .unwrap();

        let answer = engine.context_fresh("add", None, false).unwrap();

        let first = format!("{} a.ts:1 add", engine.identity);
        assert_eq!(answer.lines().next(), Some(first.as_str()), "{answer}");
        assert!(
            answer.contains("(span text unavailable: a.ts changed since indexing)"),
            "{answer}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
