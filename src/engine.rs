//! The query engine: opens a workspace from a directory and answers over its indexes.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::channel::Routes;
use crate::contract::{self, Level, Link};
pub use crate::decisions::{Cause, Evaluation, Stale, Waiting};
use crate::index::{CallSite, Export, State, Symbol};
pub use crate::items::{Candidate, Enclosing, ItemKind, OpenItem};
use crate::msg::Msg;
use crate::render::{DOCUMENTED_ONLY, Files, REPOSITORY_LEVEL, callee, label, unresolved};
use crate::shape::{self, Shape};
pub use crate::trace::{Direction, Start};
use crate::workspace::{Missing, Repository, Workspace};
use crate::{
    Result, channel, decisions, document, http, index, items, package, render, repo, resource,
    trace,
};

/// SQLite attaches at most 10 databases to one connection (`SQLITE_MAX_ATTACHED`).
const MAX_PEERS: usize = 10;

pub struct Engine {
    /// The index of the repository where Meada runs, with every other repository of the
    /// workspace attached as `peer<n>`, `n` its position in `workspace.repos`.
    db: Connection,
    workspace: Workspace,
    root: PathBuf,
    identity: String,
}

impl Engine {
    /// Opens the workspace of the repository that contains `dir`. Every repository's index
    /// lives inside its own `.meada/`; every query refreshes them first.
    pub fn open(dir: &Path) -> Result<Self> {
        let workspace = Workspace::discover(dir)?;
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
        // Opening creates a missing index, or recreates one of another schema, before it is
        // attached.
        for peer in &workspace.repos[1..] {
            index::open(&peer.root)?;
        }
        let here = &workspace.repos[0];
        let db = index::open(&here.root)?;
        for (n, peer) in workspace.repos.iter().enumerate().skip(1) {
            let path = index::path(&peer.root);
            let path = path.to_str().ok_or("index path is not UTF-8")?;
            db.execute(&format!("ATTACH DATABASE ?1 AS peer{n}"), [path])?;
        }
        Ok(Self {
            db,
            root: here.root.clone(),
            identity: here.identity.clone(),
            workspace,
        })
    }

    /// Brings every index of the workspace up to date with the files on disk (ADR 0004), with
    /// no watcher: only files whose mtime or size changed are hashed. Each repository is
    /// refreshed on its own, through its own connection; links are derived from the stored
    /// contracts at query time, so links that touch a changed file follow it.
    pub fn refresh(&self) -> Result<()> {
        for peer in &self.workspace.repos[1..] {
            let db = index::open(&peer.root)?;
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
        let (links, mut evaluations) = self.links_and_decisions(&exports);
        let links = links.into_iter();
        let links =
            links.filter(|l| l.consumer.starts_with(&here) || l.provider.starts_with(&here));
        let mut export = exports.swap_remove(0);
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
                for kind in contract::KINDS {
                    kind.declare(m, &mut export);
                }
                Ok(export)
            })
            .collect::<Result<Vec<_>>>()?;
        // Dependency entries need every repository's packages, so they come after all declares.
        let providers = package::Providers::new(&exports);
        package::consume(&mut exports, &providers);
        // Imports of a package resolve into its provider, wherever it lives.
        package::import(&mut exports, &providers);
        Ok(exports)
    }

    /// Every link of the workspace, sorted.
    fn links(&self, exports: &[Export]) -> Vec<Link> {
        self.links_and_decisions(exports).0
    }

    /// Every link of the workspace, sorted, with the decisions replayed and the drift of each
    /// HTTP link (`http::drift`), and the decisions of each repository that did not apply, in
    /// workspace order.
    fn links_and_decisions(&self, exports: &[Export]) -> (Vec<Link>, Vec<Evaluation>) {
        let (mut links, evaluations) =
            decisions::replay(self.core_links(exports), &self.workspace, exports);
        http::drift(&mut links, exports);
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
        let routes = Routes::new(exports);
        let mut links = Vec::new();
        for (export, from) in exports.iter().zip(repos) {
            for consumer in &export.consumers {
                let Some(kind) = consumer.contract.as_deref().and_then(contract::kind_of) else {
                    continue;
                };
                links.extend(kind.link(consumer, from, &providers, &routes));
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
    /// status lines about its resources (a `via` that names no symbol, a resource no link
    /// reaches) and its documents (one ignored, a route listed twice).
    pub fn status_report(&self) -> Result<(Vec<Evaluation>, Vec<Vec<String>>)> {
        self.refresh()?;
        let exports = self.exports()?;
        let (links, evaluations) = self.links_and_decisions(&exports);
        let repos = self.workspace.repos.iter();
        let lines = repos
            .zip(&exports)
            .map(|(repo, export)| {
                let mut lines = resource::report(repo, export, &links);
                lines.extend(document::report(repo, export));
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
    /// `search_packages`); `ambiguous` and `unresolved` list the open items whose enclosing
    /// symbol or call expression matches, each with the `meada decide` command that resolves
    /// it. The answer is cut at `budget` tokens.
    pub fn search(&self, query: &str, kind: Option<&str>, budget: Option<usize>) -> Result<String> {
        if let Some(item_kind) = kind.and_then(ItemKind::parse) {
            return self.search_items(query, item_kind, budget);
        }
        if kind == Some("document") {
            return self.search_documents(query, budget);
        }
        if kind == Some("package") {
            return self.search_packages(query, budget);
        }
        // Whether symbols match, and the prefix of the contracts that do.
        let (symbols, contracts) = match kind {
            None => (true, Some("")),
            Some("symbol") => (true, None),
            Some("contract") => (false, Some("")),
            Some("route") => (false, Some("http::")),
            Some("resource") => (false, Some(resource::PREFIX)),
            Some("channel") => (false, Some(channel::PREFIX)),
            Some(other) => {
                let valid = "`symbol`, `contract`, `route`, `resource`, `channel`, `document`, `package`, \
                             `ambiguous` or `unresolved`";
                return Err(format!("`kind` takes {valid}, not `{other}`").into());
            }
        };
        self.refresh()?;
        let exports = self.exports()?;
        let needle = query.to_lowercase();
        // (rank, identity to break ties, line)
        let mut hits: Vec<(u8, String, String)> = Vec::new();
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
            let Some(prefix) = contracts else {
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
                // A route path ranks against the path part: `/users` is exact for
                // `http::GET::/users`. A resource ranks against its kind and its id, a channel
                // against its broker kind and its name.
                let lower = contract.to_lowercase();
                let (kind, id) = match lower.strip_prefix(resource::PREFIX) {
                    Some(rest) => rest.split_once("::").unzip(),
                    None => match channel::parse(&lower) {
                        Some((broker, name)) => (Some(broker), name),
                        None => (None, lower.rsplit_once("::").map(|(_, p)| p)),
                    },
                };
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
                        None => package::place(&repo.identity, site).unwrap_or_else(|| {
                            // A route file that serves the contract with no function of its own.
                            site.rsplit_once("::")
                                .map_or(site.as_str(), |(_, p)| p)
                                .to_string()
                        }),
                    },
                };
                let line = format!("{} {location} {}", repo.identity, channel::shown(&contract));
                hits.push((r, format!("{contract} {site}"), line));
            }
        }
        hits.sort();
        if hits.is_empty() {
            return Ok(format!("no matches for `{query}`\n"));
        }
        let lines: Vec<String> = hits.into_iter().map(|(_, _, line)| line).collect();
        Ok(render::budgeted(&lines, budget))
    }

    /// The documents read (spec #21) whose path or `info.title` matches `query`, one line each,
    /// as `<repository> <path> (<format> <version>, routes: <n>) <title>`.
    fn search_documents(&self, query: &str, budget: Option<usize>) -> Result<String> {
        self.refresh()?;
        let needle = query.to_lowercase();
        // (rank, path to break ties, line)
        let mut hits: Vec<(u8, String, String)> = Vec::new();
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
        hits.sort();
        if hits.is_empty() {
            return Ok(format!("no matches for `{query}`\n"));
        }
        let lines: Vec<String> = hits.into_iter().map(|(_, _, line)| line).collect();
        Ok(render::budgeted(&lines, budget))
    }

    /// The packages (spec #22) whose name matches `query`, one line each, as `<repository>
    /// <directory> <name>@<version> (consumers: <n>)`, the version left out when the manifest
    /// has none. The consumers are the dependency entries linked to the package, at any level.
    fn search_packages(&self, query: &str, budget: Option<usize>) -> Result<String> {
        self.refresh()?;
        let exports = self.exports()?;
        let links = self.links(&exports);
        let mut entries: HashMap<&str, usize> = HashMap::new();
        for link in &links {
            *entries.entry(&link.provider).or_default() += 1;
        }
        let needle = query.to_lowercase();
        // (rank, repository and directory to break ties, line)
        let mut hits: Vec<(u8, String, String)> = Vec::new();
        for export in &exports {
            for p in &export.packages {
                let Some(r) = rank(&p.name.to_lowercase(), &needle) else {
                    continue;
                };
                let name = match &p.version {
                    Some(version) => format!("{}@{version}", p.name),
                    None => p.name.clone(),
                };
                let n = entries.get(p.site().as_str()).copied().unwrap_or(0);
                let line = format!("{} {} {name} (consumers: {n})", p.repository, p.directory);
                hits.push((r, format!("{} {}", p.repository, p.directory), line));
            }
        }
        hits.sort();
        if hits.is_empty() {
            return Ok(format!("no matches for `{query}`\n"));
        }
        let lines: Vec<String> = hits.into_iter().map(|(_, _, line)| line).collect();
        Ok(render::budgeted(&lines, budget))
    }

    /// The open items of `kind` whose enclosing symbol or call expression matches `query`.
    fn search_items(&self, query: &str, kind: ItemKind, budget: Option<usize>) -> Result<String> {
        let needle = query.to_lowercase();
        // (rank, id, line)
        let mut hits: Vec<(u8, String, String)> = Vec::new();
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
        hits.sort();
        if hits.is_empty() {
            return Ok(format!("no matches for `{query}`\n"));
        }
        let lines: Vec<String> = hits.into_iter().map(|(_, _, line)| line).collect();
        Ok(render::budgeted(&lines, budget))
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
    /// the top level of a file `<repository> <role> <path> (top level)`. A channel also lists
    /// the sides linked to it through another name, each followed by `[<level>: <reason>]`:
    /// the producers a consumer's wildcard pattern matches, the wildcard consumers that match
    /// it, and the publishes its bindings route to it, then the bindings themselves.
    fn contract_sides(
        &self,
        exports: &[Export],
        contract: &str,
        (provider, consumer): (&str, Option<&str>),
        what: &str,
    ) -> Vec<String> {
        let carried = match http::route(contract) {
            Some(_) => "response",
            None => "payload",
        };
        let routes = Routes::new(exports);
        // How a side reaches `contract`: its own (no tag), a match through another name, or
        // not at all.
        let tag = |own: bool, matched: Option<(Level, String)>| match own {
            true => Some(String::new()),
            false => matched.map(|(level, reason)| format!(" [{}: {reason}]", level.name())),
        };
        // Providers, then consumers, then the bindings that reach a queue; each with the lines
        // of the payload shape a document gives it.
        let mut sides: Vec<(u8, String, Vec<String>)> = Vec::new();
        for (export, repo) in exports.iter().zip(&self.workspace.repos) {
            let mut files = Files::new(&repo.root);
            // Each side that reaches `contract`: its site, order, role words, tag and shape.
            let provided = export.providers.iter().filter_map(|p| {
                let tag = tag(p.contract == contract, channel::fits(&p.contract, contract))?;
                Some((&p.site, 0, String::from(provider), tag, &p.shape))
            });
            let published = export.publishes.iter().filter_map(|q| {
                let tag = tag(false, routes.reached(q).remove(contract))?;
                Some((&q.site, 0, String::from(provider), tag, &None))
            });
            let consumed = export.consumers.iter().filter_map(|c| {
                let (own, consumer) = (c.contract.as_ref()?, consumer?);
                let tag = tag(own == contract, channel::fits(contract, own))?;
                Some((&c.site, 1, String::from(consumer), tag, &c.shape))
            });
            let bound = (export.bindings.iter().filter(|b| b.contract == contract)).map(|b| {
                let role = format!("binding {} {}", b.exchange, b.pattern);
                (&b.site, 2, role, String::new(), &None)
            });
            let all = provided.chain(published).chain(consumed).chain(bound);
            for (site, order, role, tag, shape) in all {
                let place = if let Some(s) = export.symbols.iter().find(|s| s.id == *site) {
                    let at = files.at(&s.span.path, s.span.start);
                    format!("{at} {}", label(&s.id))
                } else if let Some(c) = export.calls.iter().find(|c| c.id == *site) {
                    let at = files.at(&c.span.path, c.span.start);
                    let name = export.symbols.iter().find(|s| s.id == c.caller);
                    format!("{at} {}", name.map_or("(top level)", |s| label(&s.id)))
                } else if *site == repo.identity {
                    REPOSITORY_LEVEL.into()
                } else if let Some(doc) = export.document_of(site) {
                    // A side only a document lists.
                    format!("{doc} {DOCUMENTED_ONLY}")
                } else if let Some(place) = package::place(&repo.identity, site) {
                    // A dependency entry of a manifest.
                    place
                } else {
                    format!("{} (top level)", label(site))
                };
                let shape = shape.as_ref();
                let shape =
                    shape.map_or_else(Vec::new, |s| shape_lines(&repo.identity, s, carried));
                let line = format!("{} {role} {place}{tag}", repo.identity);
                sides.push((order, line, shape));
            }
        }
        if sides.is_empty() {
            return vec![format!("no {what} matches `{contract}`")];
        }
        sides.sort();
        let mut lines = vec![channel::shown(contract)];
        for (_, line, shape) in sides {
            lines.push(format!("  {line}"));
            lines.extend(shape.into_iter().map(|l| format!("    {l}")));
        }
        lines
    }

    /// One symbol of the workspace: its span text read from disk (only while the file still
    /// has the indexed hash), its callers and its callees. `symbol` is an identity,
    /// `<path>::<name>` or a name; when it fits several symbols, they are listed instead of
    /// guessed. The answer is cut at `budget` tokens.
    pub fn context(&self, symbol: &str, budget: Option<usize>) -> Result<String> {
        self.refresh()?;
        self.context_fresh(symbol, budget, true)
    }

    fn context_fresh(&self, symbol: &str, budget: Option<usize>, retry: bool) -> Result<String> {
        let exports = self.exports()?;
        let repos = &self.workspace.repos;
        // A contract listed by its sides, as `search` prints it; a route by its providers.
        let contract = channel::typed(symbol);
        if let Some(kind) = contract::kind_of(contract) {
            let roles = match (kind.sides(), http::route(contract)) {
                (Some((provider, consumer)), _) => Some((provider, Some(consumer))),
                (None, Some(_)) => Some(("provider", None)),
                (None, None) => None,
            };
            if let Some(roles) = roles {
                let lines = self.contract_sides(&exports, contract, roles, kind.prefix());
                return Ok(render::budgeted(&lines, budget));
            }
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
            index::forget(&index::open(&repo.root)?, &symbol_found.span.path)?;
            self.refresh()?;
            return self.context_fresh(symbol, budget, false);
        }
        let mut files = Files::new(&repo.root);
        let identity = &repo.identity;
        let name = label(&symbol_found.id);
        let mut lines = vec![line(identity, &mut files, symbol_found, name)];
        lines.extend(text.unwrap_or_else(|| {
            vec![format!(
                "(span text unavailable: {} changed since indexing)",
                symbol_found.span.path
            )]
        }));
        lines.extend(relations(identity, export, &mut files, &symbol_found.id));
        let links = self.links(&exports);
        lines.extend(consumers(&exports, repos, &links, &symbol_found.id));
        let routes = Routes::new(&exports);
        let workspace = (links.as_slice(), &routes);
        lines.extend(contracts(
            identity,
            export,
            workspace,
            &mut files,
            symbol_found,
        ));
        Ok(render::budgeted(&lines, budget))
    }
}

/// `contracts:` with what `symbol` provides, the channels it produces to (a publish to a named
/// exchange, then each queue the bindings route it to) and the contracts its calls consume,
/// unresolved ones included, each followed by its decided links; `links` and `routes` are the
/// workspace's. Nothing when it has none.
fn contracts(
    identity: &str,
    export: &Export,
    (links, routes): (&[Link], &Routes),
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
    // A channel producer is a call site: the symbol around it produces.
    for call in export.calls.iter().filter(|c| c.caller == symbol.id) {
        let produced = export
            .providers
            .iter()
            .filter(|p| p.site == call.id && p.contract.starts_with(channel::PREFIX));
        let location = files.at(&call.span.path, call.span.start);
        for provider in produced {
            let contract = channel::shown(&provider.contract);
            lines.push(format!("{identity} {location} produces {contract}"));
        }
        for publish in export.publishes.iter().filter(|q| q.site == call.id) {
            let (exchange, key) = (&publish.exchange, &publish.key);
            lines.push(format!("{identity} {location} publishes {exchange} {key}"));
            for (queue, (level, reason)) in routes.reached(publish) {
                let level = level.name();
                lines.push(format!(
                    "{identity} {location} produces {queue} [{level}: {reason}]"
                ));
            }
        }
    }
    // A resource reader or a channel handler is a symbol, not a call site.
    let readers = export.consumers.iter().filter(|c| c.site == symbol.id);
    for contract in readers.filter_map(|c| c.contract.as_ref()) {
        let contract = channel::shown(contract);
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

/// The span's lines, or `None` when its file under `root` no longer has the indexed hash.
fn span_text(root: &Path, export: &Export, symbol: &Symbol) -> Option<Vec<String>> {
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
    Some(
        String::from_utf8_lossy(span)
            .lines()
            .map(str::to_string)
            .collect(),
    )
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
                ambiguous(c)
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
                .map(|t| line(identity, files, t, label(&t.id)) + &ambiguous(c))
                .collect()
        })
        .collect();
    callees.dedup();
    let mut lines = section("callers", callers);
    lines.extend(section("callees", callees));
    lines
}

/// `consumers:` with the calls that reach `symbol` through a package import (spec #22), from
/// any repository of the workspace, as `<repository> <path>:<line> <caller>`, then ` [<level>,
/// <range flag>]` when the link is not exact or the dependency's range does not plainly accept
/// the package's version. Nothing when there are none.
fn consumers(
    exports: &[Export],
    repos: &[Repository],
    links: &[Link],
    symbol: &str,
) -> Vec<String> {
    let mut reached = package::reached(exports, links, symbol);
    reached.sort_by_key(|r| {
        (
            &repos[r.repository].identity,
            &r.call.span.path,
            r.call.span.start,
        )
    });
    let lines: Vec<String> = reached
        .iter()
        .map(|r| {
            let repo = &repos[r.repository];
            let location = Files::new(&repo.root).at(&r.call.span.path, r.call.span.start);
            let caller = &r.call.caller;
            let caller = match exports[r.repository]
                .symbols
                .iter()
                .any(|s| s.id == *caller)
            {
                true => label(caller),
                false => "(top level)",
            };
            let level = (r.level != Level::Exact).then(|| r.level.name());
            let flag = r.flag.map(package::Flag::words);
            let tags: Vec<&str> = level.into_iter().chain(flag).collect();
            let tags = match tags.is_empty() {
                true => String::new(),
                false => format!(" [{}]", tags.join(", ")),
            };
            format!("{} {location} {caller}{tags}", repo.identity)
        })
        .collect();
    match lines.is_empty() {
        true => lines,
        false => section("consumers", lines),
    }
}

/// `<repository> <path>:<line> <name>` for a symbol's definition.
fn line(identity: &str, files: &mut Files, symbol: &Symbol, name: &str) -> String {
    let location = files.at(&symbol.span.path, symbol.span.start);
    format!("{identity} {location} {name}")
}

/// ` [ambiguous]` for an ambiguous call, empty otherwise.
fn ambiguous(call: &CallSite) -> String {
    match call.level {
        Some(level @ Level::Ambiguous) => format!(" [{}]", level.name()),
        _ => String::new(),
    }
}

/// The workspace of the repository containing `dir`: each repository with where it is,
/// relative to the one Meada runs in, and its index state, each followed by the decisions of
/// its `meada.decisions.toml` that are stale or wait on a missing peer; then every peer not
/// found. It never creates an index: only when every repository is indexed at the current
/// schema does it refresh them, like a query, to evaluate the decisions against the files as they
/// are; otherwise it reports the states and no decision.
pub fn status(dir: &Path) -> Result<String> {
    let workspace = Workspace::discover(dir)?;
    let here = &workspace.repos[0].root;
    let indexed =
        |m: &crate::workspace::Repository| matches!(index::state(&m.root), State::Indexed { .. });
    // A failure to open or refresh drops the decision and resource lines, not the whole status.
    let engine = workspace
        .repos
        .iter()
        .all(indexed)
        .then(|| Engine::open(dir));
    let (evaluations, resources) = match engine {
        Some(Ok(engine)) => engine.status_report().unwrap_or_default(),
        _ => Default::default(),
    };
    let mut lines = Vec::new();
    for (n, m) in workspace.repos.iter().enumerate() {
        let state = match index::state(&m.root) {
            State::Missing => Msg::NotIndexed.text(),
            State::OtherSchema => Msg::OtherVersion.text(),
            State::Indexed { files } => Msg::Indexed(files).text(),
        };
        lines.push(format!(
            "{} ({}) {state}",
            m.identity,
            relative(here, &m.root)
        ));
        for e in evaluations.iter().filter(|e| e.repository == m.identity) {
            lines.extend(e.lines().map(|l| format!("  {l}")));
        }
        lines.extend((resources.get(n).into_iter().flatten()).map(|l| format!("  {l}")));
    }
    lines.extend(workspace.missing.iter().map(Missing::text));
    Ok(lines.iter().map(|l| format!("{l}\n")).collect())
}

/// 0 when `name` is `needle`, 1 when it starts with it, 2 when it contains it.
fn rank(name: &str, needle: &str) -> Option<u8> {
    match () {
        _ if name == needle => Some(0),
        _ if name.starts_with(needle) => Some(1),
        _ if name.contains(needle) => Some(2),
        _ => None,
    }
}

/// The schema a workspace repository's index is attached as.
fn schema(n: usize) -> String {
    if n == 0 {
        "main".into()
    } else {
        format!("peer{n}")
    }
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

/// `name:` followed by the items, or `name: none`.
fn section(name: &str, items: Vec<String>) -> Vec<String> {
    if items.is_empty() {
        return vec![format!("{name}: none")];
    }
    let mut lines = vec![format!("{name}:")];
    lines.extend(items);
    lines
}
