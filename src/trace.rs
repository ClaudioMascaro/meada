//! `trace`: a chain followed from a start through calls inside repositories and links between
//! them, down or up, across the whole workspace (ADR 0005), rendered as indented lines under
//! the token budget and closed by the chain footer (ADR 0008).

use std::collections::{HashMap, HashSet};

use crate::contract::{self, Level, Link};
use crate::entry::EntryPoint;
use crate::index::{CallSite, Consumer, Export, Symbol};
use crate::render;
use crate::render::{
    DOCUMENTED_ONLY, Files, REPOSITORY_LEVEL, callee, label, line_of, mark, unresolved,
};
use meada_core::export::{Mark, Process, Step as ProcessStep, Via as StepVia};
use meada_core::trace::{Chain, Row, arrow, level_mark};

use crate::Result;
use crate::partial::Partial;
use crate::repo::Change;
use crate::workspace::{Missing, Workspace};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// From callers to callees, from consumers to the providers they reach, and from writers
    /// and producers to the readers and handlers of what they write or produce.
    Down,
    /// The reverse of `Down`.
    Up,
}

#[derive(Clone, Copy, Debug)]
pub enum Start<'a> {
    /// A symbol, a route or a call site, by identity or by name; or a process, as
    /// `process::<entry point identity>` (walked down only).
    From(&'a str),
    /// The symbols of the repository where Meada runs whose spans overlap the working-tree
    /// diff against `HEAD`, untracked files included.
    Uncommitted,
}

/// A call site, with the position of its repository in the workspace.
type Call<'a> = (usize, &'a CallSite);

/// The workspace as one graph of symbols, calls and links.
pub(crate) struct Graph<'a> {
    workspace: &'a Workspace,
    exports: &'a [Export],
    provisions: Vec<meada_core::export::Provision>,
    executions: Vec<meada_core::export::Execution>,
    symbols: HashMap<&'a str, (usize, &'a Symbol)>,
    iac: HashMap<String, (usize, &'a crate::iac::Node)>,
    calls: HashMap<&'a str, Call<'a>>,
    /// The calls each symbol (or file, for top-level calls) makes, in source order.
    callees: HashMap<&'a str, Vec<Call<'a>>>,
    /// The calls that may reach each symbol, ambiguous ones included, in workspace and source
    /// order.
    callers: HashMap<&'a str, Vec<Call<'a>>>,
    /// The consumers at each call site: a client wrapper's call holds one per URL it builds.
    consumers: HashMap<&'a str, Vec<&'a Consumer>>,
    /// `(contract, site)` of every provider.
    providers: Vec<(&'a str, &'a str)>,
    /// The sides only a document lists (spec #21), with the document's path.
    documented: HashMap<&'a str, &'a str>,
    /// The channel sides whose name Meada could not evaluate, by site (the producing call or
    /// the handler): `Produces` or `Consumes`, and the contract. They are never linked, so the
    /// chain would not otherwise show them.
    unresolved_sides: HashMap<&'a str, Vec<(Mark, &'a str)>>,
    /// Links by consumer and by provider, each sorted as the engine sorts links. A link's
    /// consumer is a call site, or, for a resource or a channel, the symbol that reads it or
    /// handles it: data goes from the provider (a writer, or the call that produces) to that
    /// consumer, so the chain crosses it the other way.
    out: HashMap<&'a str, Vec<&'a Link>>,
    into: HashMap<&'a str, Vec<&'a Link>>,
    /// The entry points of every repository by identity, with the position of its repository.
    entry_points: HashMap<&'a str, (usize, &'a EntryPoint)>,
}

impl<'a> Graph<'a> {
    /// A receiving operation keeps its call identity; its continuation is the owner's body.
    fn reader_call(&self, id: &str) -> Option<Call<'a>> {
        self.consumers
            .get(id)?
            .iter()
            .any(|c| contract::flows_to_consumer(c.contract.as_deref()))
            .then(|| self.calls.get(id).copied())
            .flatten()
    }

    fn callees_at(&self, id: &str) -> &[Call<'a>] {
        let owner = self
            .reader_call(id)
            .map_or(id, |(_, call)| call.caller.as_str());
        self.callees.get(owner).map_or(&[], Vec::as_slice)
    }

    /// `exports` in workspace order, and every link between them.
    pub(crate) fn new(workspace: &'a Workspace, exports: &'a [Export], links: &'a [Link]) -> Self {
        let mut graph = Self {
            workspace,
            exports,
            provisions: crate::iac::provision::links(exports),
            executions: contract::KINDS
                .iter()
                .flat_map(|k| k.executions(exports, links))
                .collect(),
            symbols: HashMap::new(),
            iac: HashMap::new(),
            calls: HashMap::new(),
            callees: HashMap::new(),
            callers: HashMap::new(),
            consumers: HashMap::new(),
            providers: Vec::new(),
            documented: HashMap::new(),
            unresolved_sides: HashMap::new(),
            out: HashMap::new(),
            into: HashMap::new(),
            entry_points: HashMap::new(),
        };
        for (n, export) in exports.iter().enumerate() {
            for node in &export.iac {
                graph
                    .iac
                    .entry(node.identity(&export.repository))
                    .or_insert((n, node));
            }
            for e in &export.entry_points {
                graph.entry_points.insert(&e.identity, (n, e));
            }
            for s in &export.symbols {
                graph.symbols.insert(&s.id, (n, s));
            }
            for c in &export.calls {
                graph.calls.insert(&c.id, (n, c));
                graph.callees.entry(&c.caller).or_default().push((n, c));
                for t in c.target.iter().chain(&c.candidates) {
                    graph.callers.entry(t).or_default().push((n, c));
                }
            }
            for c in &export.consumers {
                graph.consumers.entry(&c.site).or_default().push(c);
            }
            let provided = export.providers.iter();
            graph
                .providers
                .extend(provided.map(|p| (p.contract.as_str(), p.site.as_str())));
            let sites = (export.providers.iter().map(|p| &p.site))
                .chain(export.consumers.iter().map(|c| &c.site));
            for site in sites {
                if let Some(doc) = export.document_of(site) {
                    graph.documented.insert(site, doc);
                }
            }
            let produced = export.providers.iter();
            let consumed = export.consumers.iter();
            let sides = produced
                .map(|p| (Mark::Produces, Some(&p.contract), &p.site))
                .chain(consumed.map(|c| (Mark::Consumes, c.contract.as_ref(), &c.site)));
            for (side, contract, site) in sides {
                if let Some(contract) = contract.filter(|c| contract::unresolved(c)) {
                    graph
                        .unresolved_sides
                        .entry(site)
                        .or_default()
                        .push((side, contract));
                }
            }
        }
        let order = |(n, c): &Call| (*n, c.span.path.clone(), c.span.start, c.id.clone());
        for calls in graph.callees.values_mut().chain(graph.callers.values_mut()) {
            calls.sort_by_key(order);
        }
        for l in links {
            graph.out.entry(&l.consumer).or_default().push(l);
            graph.into.entry(&l.provider).or_default().push(l);
        }
        graph
    }

    /// The whole answer: the chain from `start` (`changes` holds the uncommitted changes when
    /// it starts from them), cut to `budget`, then the footer.
    pub(crate) fn answer(
        &self,
        start: Start,
        changes: Option<&[(String, Change)]>,
        direction: Direction,
        budget: Option<usize>,
    ) -> String {
        let mut walk = Walk {
            graph: self,
            direction,
            files: self
                .workspace
                .repos
                .iter()
                .map(|m| Files::new(&m.root))
                .collect(),
            rows: Vec::new(),
            ancestors: Vec::new(),
            expanded: HashSet::new(),
            executed: HashSet::new(),
        };
        match start {
            Start::Uncommitted => {
                let roots = self.changed(changes.unwrap_or_default(), &mut walk.files[0]);
                if roots.is_empty() {
                    return "no symbol overlaps the uncommitted changes\n".into();
                }
                for id in roots {
                    walk.root(id);
                }
            }
            Start::From(from) => match self.find(from, &mut walk) {
                Lookup::Nodes(ids) => ids.into_iter().for_each(|id| walk.root(id)),
                Lookup::Call(call) => walk.root_call(call),
                Lookup::Process(n, entry) => match direction {
                    Direction::Down => walk.process(n, entry),
                    Direction::Up => {
                        let lines = [
                            "a process is what its entry point sets in motion: trace it down"
                                .to_string(),
                        ];
                        return render::budgeted(&lines, budget);
                    }
                },
                Lookup::Answer(lines) => return render::budgeted(&lines, budget),
            },
        }
        walk.finish(budget)
    }

    /// Continue an IaC node through its ordinary links and downward provisioning/execution
    /// hops, without repeating the node or adding a second footer.
    pub(crate) fn attached_rows(
        &self,
        repository: usize,
        node: &crate::iac::Node,
        depth: usize,
        direction: Direction,
    ) -> Vec<Row> {
        let identity = format!(
            "{}::{}::{}",
            self.exports[repository].repository, node.path, node.address
        );
        let mut walk = Walk {
            graph: self,
            direction,
            files: self
                .workspace
                .repos
                .iter()
                .map(|r| Files::new(&r.root))
                .collect(),
            rows: Vec::new(),
            ancestors: Vec::new(),
            expanded: HashSet::new(),
            executed: HashSet::new(),
        };
        // The borrowed link endpoints carry the same identity as the IaC node. Entering
        // it uses the ordinary kind-directed traversal; omit the already-rendered root.
        if let Some((&site, _)) = self
            .into
            .get_key_value(identity.as_str())
            .or_else(|| self.out.get_key_value(identity.as_str()))
        {
            walk.enter(
                site,
                depth.saturating_sub(1),
                Row::call(String::new(), repository, None),
            );
            walk.rows.remove(0);
        }
        if direction == Direction::Up {
            return walk.rows;
        }
        for link in self
            .provisions
            .iter()
            .filter(|l| l.provisioner == identity && l.root == node.root)
        {
            let id = link.side.as_str();
            // The link is owned by this graph; find the same site in its borrowed exports.
            let call = self.calls.get(id).copied();
            let Some(site) = self
                .exports
                .iter()
                .flat_map(crate::iac::provision::uses)
                .map(|(site, _)| site)
                .find(|s| *s == id)
            else {
                continue;
            };
            let (n, text) = match call {
                Some(c) => (c.0, format!("{} {}", walk.site(c), walk.caller(c))),
                None => walk.node(site),
            };
            let mut row = Row::call(
                format!(
                    "provisions {} [{}: {}] {text}",
                    link.contract,
                    link.level.name(),
                    link.reason
                ),
                n,
                Some(link.level),
            );
            row.link = true;
            match call {
                Some(c) => {
                    walk.push(depth, row);
                    walk.call_down(c, depth + 1, &mut HashSet::new());
                }
                None if self.ends(site) && self.into.get(site).is_none_or(Vec::is_empty) => {
                    walk.push(depth, row)
                }
                None => walk.enter(site, depth, row),
            }
        }
        for execution in self
            .executions
            .iter()
            .filter(|x| x.workload == identity && x.root == node.root)
        {
            let Some(n) = self
                .exports
                .iter()
                .position(|e| e.repository == execution.repository)
            else {
                continue;
            };
            let mut row = Row::call(
                format!(
                    "runs code {} {} [{}: {}]",
                    execution.repository,
                    execution.path,
                    execution.level.name(),
                    execution.reason
                ),
                n,
                Some(execution.level),
            );
            row.link = true;
            walk.push(depth, row);
            if execution.level != Level::Ambiguous {
                for entry in self.exports[n].entry_points.iter().filter(|e| {
                    self.symbols
                        .get(e.site.as_str())
                        .is_some_and(|(_, s)| in_path(&s.span.path, &execution.path))
                }) {
                    let (n, text) = walk.node(&entry.site);
                    walk.enter(
                        &entry.site,
                        depth + 1,
                        Row::call(text, n, Some(execution.level)),
                    );
                }
            }
        }
        walk.rows
    }

    /// The starts `from` names: the providers of a route, else one symbol, else one call site.
    /// A name that fits several symbols or call sites lists them instead of guessing.
    fn find(&self, from: &str, walk: &mut Walk) -> Lookup<'a> {
        // A process, by its entry point's identity, and only in this form.
        if let Some(identity) = from.strip_prefix("process::") {
            return match self.entry_points.get(identity) {
                Some(&(n, entry)) => Lookup::Process(n, entry),
                None => Lookup::Answer(vec![format!(
                    "no process `{from}`: pass `process::<entry point identity>`"
                )]),
            };
        }
        // A route is a whole contract (`GET /users` names one too), or a path on any method.
        let mut sites: Vec<&str> = (contract::KINDS.iter())
            .flat_map(|kind| kind.starts(from, &self.providers))
            .collect();
        sites.sort();
        sites.dedup();
        if !sites.is_empty() {
            return Lookup::Nodes(sites);
        }
        if let Some(kind) = contract::kind_of(from)
            && let Some((provider, consumer)) = kind.sides()
        {
            return Lookup::Answer(vec![format!(
                "`{from}` is a {}, and a trace starts from a symbol, a route or a call \
                 site: run `context` on it for its {provider}s and {consumer}s, then trace \
                 from one",
                kind.prefix()
            )]);
        }
        let keys = config_starts(self.exports, from);
        if !keys.is_empty() {
            return Lookup::Nodes(keys);
        }
        if self.is_repo(from)
            && self
                .provisions
                .iter()
                .any(|l| crate::iac::provision::at_site(l, from))
        {
            let id = self
                .workspace
                .repos
                .iter()
                .find(|r| r.identity == from)
                .unwrap();
            return Lookup::Nodes(vec![&id.identity]);
        }
        let suffix = format!("::{from}");
        let fits = |id: &str| id == from || id.ends_with(&suffix);
        let mut symbols: Vec<&str> = self.symbols.keys().copied().filter(|id| fits(id)).collect();
        symbols.sort();
        if let [id] = symbols[..] {
            return Lookup::Nodes(vec![id]);
        }
        if !symbols.is_empty() {
            let mut lines = vec![format!(
                "`{from}` fits {} symbols; pass one of:",
                symbols.len()
            )];
            lines.extend(symbols.iter().map(|id| format!("{} {id}", walk.node(id).1)));
            return Lookup::Answer(lines);
        }
        let mut calls: Vec<Call> = (self.calls.iter())
            .filter(|(id, _)| fits(id))
            .map(|(_, &call)| call)
            .collect();
        calls.sort_by_key(|(_, c)| &c.id);
        match calls[..] {
            [call] => Lookup::Call(call),
            [] => Lookup::Answer(vec![no_start(from)]),
            _ => {
                let mut lines = vec![format!(
                    "`{from}` fits {} call sites; pass one of:",
                    calls.len()
                )];
                lines.extend(
                    calls
                        .iter()
                        .map(|&c| format!("{} {}", walk.site(c), c.1.id)),
                );
                Lookup::Answer(lines)
            }
        }
    }

    /// The symbols of the repository where Meada runs that the changes touch, in source order.
    fn changed(&self, changes: &[(String, Change)], files: &mut Files) -> Vec<&'a str> {
        let changes: HashMap<&str, &Change> =
            changes.iter().map(|(p, c)| (p.as_str(), c)).collect();
        let mut hits: Vec<(&str, i64, &str)> = Vec::new();
        for &(n, s) in self.symbols.values() {
            let Some(change) = changes.get(s.span.path.as_str()).filter(|_| n == 0) else {
                continue;
            };
            let hit = match change {
                Change::Untracked => true,
                Change::Lines(ranges) => files.get(&s.span.path).is_some_and(|src| {
                    let first = line_of(src, s.span.start);
                    let last = line_of(src, (s.span.end - 1).max(s.span.start));
                    ranges.iter().any(|&(a, b)| a <= last && first <= b)
                }),
            };
            if hit {
                hits.push((&s.span.path, s.span.start, &s.id));
            }
        }
        hits.sort();
        hits.into_iter().map(|(_, _, id)| id).collect()
    }

    /// The position in the workspace of the repository holding the symbol or file `id`.
    fn repo(&self, id: &str) -> usize {
        if let Some(&(n, _)) = self.symbols.get(id) {
            return n;
        }
        (self.workspace.repos.iter())
            .position(|m| {
                id == m.identity
                    || id
                        .strip_prefix(&m.identity)
                        .is_some_and(|r| r.starts_with("::"))
            })
            .unwrap_or(0)
    }

    /// Whether `id` is a whole repository: the side of a resource declared with no `via`.
    fn is_repo(&self, id: &str) -> bool {
        self.workspace.repos.iter().any(|m| m.identity == id)
    }

    /// Whether the chain ends at `id`, reached through a link: a whole repository, or a side
    /// only a document lists, since no code of it is indexed.
    fn ends(&self, id: &str) -> bool {
        self.is_repo(id)
            || self.documented.contains_key(id)
            || self.exports.iter().any(|e| {
                e.config_uses.iter().any(|u| u.site == id)
                    && !e.config_sites.iter().any(|s| s.id == id)
            })
    }
}

/// The answer's line when `from` names no symbol, route or call site.
pub(crate) fn no_start(from: &str) -> String {
    format!("no symbol, route or call site matches `{from}`")
}

/// A key can carry declared sides, automatic uses, or both. An unqualified query keeps
/// every visible repository's match, with each canonical key entering the trace once.
fn config_starts<'a>(exports: &'a [Export], from: &str) -> Vec<&'a str> {
    let declared = crate::config_values::sites::found(exports, from);
    let uses = crate::iac::config_uses::found(exports, from);
    let mut seen = HashSet::new();
    declared
        .into_iter()
        .map(|(_, s)| s.id.as_str())
        .chain(uses.into_iter().map(|(_, u)| u.site.as_str()))
        .filter(|site| seen.insert(*site))
        .collect()
}

fn in_path(file: &str, directory: &str) -> bool {
    directory == "."
        || file == directory
        || file
            .strip_prefix(directory)
            .is_some_and(|s| s.starts_with('/'))
}

/// Read the entry points that workload execution can reach before constructing the graph.
pub(crate) fn read_executions(partial: &mut Partial, links: &[Link]) -> Result<()> {
    let executions = contract::KINDS
        .iter()
        .flat_map(|k| k.executions(&partial.exports, links))
        .collect::<Vec<_>>();
    let mut starts = Vec::new();
    for execution in executions {
        let Some(n) = partial
            .exports
            .iter()
            .position(|e| e.repository == execution.repository)
        else {
            continue;
        };
        let sites = partial.exports[n]
            .entry_points
            .iter()
            .map(|e| e.site.clone())
            .collect::<Vec<_>>();
        partial.symbols(Some(n), &sites)?;
        starts.extend(
            partial.exports[n]
                .symbols
                .iter()
                .filter(|s| in_path(&s.span.path, &execution.path))
                .map(|s| s.id.clone()),
        );
    }
    read(partial, links, starts, Direction::Down, true)?;
    Ok(())
}

/// The nodes `start` names, as `Graph::find` and `Graph::changed` find them, read into
/// `partial` with every row `find` looks up (#396): the providers a kind starts from, else the
/// symbols the name fits, else the call sites it fits. Empty when it names none, or several
/// (the answer lists them, and nothing is walked), or a process `direction` up.
pub(crate) fn starts(
    partial: &mut Partial,
    start: Start,
    changes: Option<&[(String, Change)]>,
    direction: Direction,
) -> Result<Vec<String>> {
    let from = match start {
        Start::Uncommitted => {
            let changed = changes.unwrap_or_default().iter();
            let paths: Vec<&str> = changed.map(|(p, _)| p.as_str()).collect();
            let found = crate::index::symbols_in(partial.db(0), &paths)?;
            let ids = found.iter().map(|s| s.id.clone()).collect();
            partial.add_symbols(0, found);
            return Ok(ids);
        }
        Start::From(from) => from,
    };
    if let Some(identity) = from.strip_prefix("process::") {
        let entries = partial.exports.iter().flat_map(|e| &e.entry_points);
        let sites = entries
            .filter(|e| e.identity == identity)
            .map(|e| e.site.clone());
        return Ok(match direction {
            Direction::Down => sites.collect(),
            Direction::Up => Vec::new(),
        });
    }
    let providers: Vec<(&str, &str)> = (partial.exports.iter())
        .flat_map(|e| &e.providers)
        .map(|p| (p.contract.as_str(), p.site.as_str()))
        .collect();
    let sites: Vec<String> = (contract::KINDS.iter())
        .flat_map(|kind| kind.starts(from, &providers))
        .map(String::from)
        .collect();
    if !sites.is_empty() {
        return Ok(sites);
    }
    if contract::kind_of(from).is_some_and(|kind| kind.sides().is_some()) {
        return Ok(Vec::new());
    }
    let keys = config_starts(&partial.exports, from);
    if !keys.is_empty() {
        return Ok(keys.into_iter().map(str::to_string).collect());
    }
    if partial.exports.iter().any(|e| e.repository == from)
        && crate::iac::provision::links(&partial.exports)
            .iter()
            .any(|l| crate::iac::provision::at_site(l, from))
    {
        return Ok(vec![from.to_string()]);
    }
    let mut symbols = HashSet::new();
    for n in 0..partial.exports.len() {
        let found = crate::index::symbols_named(partial.db(n), from)?;
        symbols.extend(found.iter().map(|s| s.id.clone()));
        partial.add_symbols(n, found);
    }
    if !symbols.is_empty() {
        return Ok(match symbols.len() {
            1 => symbols.into_iter().collect(),
            _ => Vec::new(),
        });
    }
    let mut calls = HashSet::new();
    for n in 0..partial.exports.len() {
        let found = crate::index::calls_named(partial.db(n), from)?;
        calls.extend(found.iter().map(|c| c.id.clone()));
        partial.add_calls(n, found);
    }
    Ok(match calls.len() {
        1 => calls.into_iter().collect(),
        _ => Vec::new(),
    })
}

/// Reads into `partial` every code row a walk `direction` from `starts` may read (#396), and
/// returns the nodes reached. From each node: its own rows (a symbol, or a call site named by
/// its identity), then down the calls it makes, the targets and candidates of each and what
/// the links at those calls and at the node lead to, or up its call site's caller, the calls
/// whose target is the node (or, with `candidates`, that list it among theirs) and what the
/// links at the node lead to; until nothing new is reached. That is every edge `Walk::enter`
/// and `Graph::edges` may follow and some they do not (an overload set, a link of another
/// contract), so a `Graph` over `partial` answers from the reached nodes as the one over every
/// row does. `links` are the workspace's.
pub(crate) fn read(
    partial: &mut Partial,
    links: &[Link],
    starts: Vec<String>,
    direction: Direction,
    candidates: bool,
) -> Result<HashSet<String>> {
    let receiving: HashSet<String> = partial
        .exports
        .iter()
        .flat_map(|export| &export.consumers)
        .filter(|consumer| contract::flows_to_consumer(consumer.contract.as_deref()))
        .map(|consumer| consumer.site.clone())
        .collect();
    let mut by_consumer: HashMap<&str, Vec<&Link>> = HashMap::new();
    let mut by_provider: HashMap<&str, Vec<&Link>> = HashMap::new();
    for l in links {
        by_consumer.entry(&l.consumer).or_default().push(l);
        by_provider.entry(&l.provider).or_default().push(l);
    }
    // The far end of each link at `id`, either way: a link is crossed both ways, by its kind.
    let across = |id: &str, next: &mut Vec<String>| {
        let out = by_consumer.get(id).into_iter().flatten();
        next.extend(out.map(|l| l.provider.clone()));
        let inward = by_provider.get(id).into_iter().flatten();
        next.extend(inward.map(|l| l.consumer.clone()));
    };
    let mut reached: HashSet<String> = HashSet::new();
    let mut frontier = starts;
    while !frontier.is_empty() {
        let new: Vec<String> = (frontier.into_iter())
            .filter(|id| reached.insert(id.clone()))
            .collect();
        partial.symbols(None, &new)?;
        let sites = partial.calls(None, &new)?;
        partial.callers_of(&sites)?;
        let mut next = Vec::new();
        for id in &new {
            across(id, &mut next);
        }
        match direction {
            Direction::Down => {
                for (_, call) in &sites {
                    if receiving.contains(&call.id) {
                        next.push(call.caller.clone());
                    }
                }
                let mut calls = sites;
                calls.extend(partial.callees(None, &new)?);
                for (_, c) in &calls {
                    next.extend(c.target.iter().chain(&c.candidates).cloned());
                    across(&c.id, &mut next);
                }
            }
            Direction::Up => {
                // Provisioning sides at producing calls belong to their enclosing symbol.
                for (_, call) in partial.callees(None, &new)? {
                    next.extend(
                        by_consumer
                            .get(call.id.as_str())
                            .into_iter()
                            .flatten()
                            .filter(|link| contract::flows_to_consumer(Some(&link.contract)))
                            .map(|link| link.provider.clone()),
                    );
                }
                let mut calls = sites;
                calls.extend(partial.callers(None, &new, candidates)?);
                next.extend(calls.into_iter().map(|(_, c)| c.caller));
            }
        }
        frontier = next;
    }
    Ok(reached)
}

/// Reads into `partial` what `Graph::processes_of(id)` reads (#396): the nodes upstream of
/// `id` through the edges a process follows (calls by their target, links), with the calls of
/// those that are no symbol (a file making top-level calls), whose edges come from their calls
/// alone. `links` are the workspace's.
pub(crate) fn read_processes(partial: &mut Partial, links: &[Link], id: &str) -> Result<()> {
    let reached = read(partial, links, vec![id.to_string()], Direction::Up, false)?;
    let symbols: HashSet<&str> = (partial.exports.iter())
        .flat_map(|e| &e.symbols)
        .map(|s| s.id.as_str())
        .collect();
    let files: Vec<String> = (reached.into_iter())
        .filter(|id| !symbols.contains(id.as_str()))
        .collect();
    partial.callees(None, &files)?;
    Ok(())
}

/// Whether the consumer of `link` calls its provider, so the provider is not a writer that goes
/// on to a reader.
fn calls_on_link(link: &Link) -> bool {
    contract::calls_from_definition(Some(&link.contract))
}

/// What a `from` names.
enum Lookup<'a> {
    /// Symbols, or files serving a route, to start from.
    Nodes(Vec<&'a str>),
    Call(Call<'a>),
    /// The process of an entry point of the repository at that position.
    Process(usize, &'a EntryPoint),
    /// No start: the answer says why.
    Answer(Vec<String>),
}

/// A depth-first walk of the graph that writes the chain as it goes.
struct Walk<'g, 'a> {
    graph: &'g Graph<'a>,
    direction: Direction,
    files: Vec<Files<'a>>,
    rows: Vec<Row>,
    /// The nodes from the start down to the one being followed: meeting one again closes a
    /// cycle.
    ancestors: Vec<&'a str>,
    /// Nodes already followed once; the chain shows them again but does not repeat what
    /// follows them.
    expanded: HashSet<&'a str>,
    /// The workloads and roots whose `runs code` line the answer wrote (#744): a workload that
    /// runs several of the nodes reached, or several paths of one, is written once.
    executed: HashSet<(String, String)>,
}

impl<'a> Walk<'_, 'a> {
    fn root(&mut self, id: &'a str) {
        let (n, text) = self.node(id);
        self.enter(id, 0, Row::call(text, n, None));
    }

    /// A call site as the start: the call, then what it reaches or where it sits.
    fn root_call(&mut self, call: Call<'a>) {
        let text = format!("{} {}", self.site(call), callee(&call.1.id));
        if self.direction == Direction::Down && self.graph.reader_call(&call.1.id).is_some() {
            self.enter(&call.1.id, 0, Row::call(text, call.0, None));
            return;
        }
        self.push(0, Row::call(text, call.0, None));
        match self.direction {
            Direction::Down => self.call_down(call, 1, &mut HashSet::new()),
            Direction::Up => {
                let site = self.site(call);
                self.unresolved_sides(&call.1.id, 1, call.0, &site);
                let text = format!("{site} {}", self.caller(call));
                self.enter(&call.1.caller, 1, Row::call(text, call.0, None));
            }
        }
    }

    /// Writes the line reaching `id`, then follows `id` unless that closes a cycle or repeats
    /// a node already followed.
    fn enter(&mut self, id: &'a str, depth: usize, mut row: Row) {
        let seen = match () {
            _ if self.ancestors.contains(&id) => " (cycle)",
            _ if self.expanded.contains(id) => " (see above)",
            _ => "",
        };
        row.text += seen;
        self.push(depth, row);
        if !seen.is_empty() {
            return;
        }
        self.expanded.insert(id);
        self.ancestors.push(id);
        let graph = self.graph;
        if let Some(&(n, s)) = graph.symbols.get(id) {
            let place = format!(
                "{} {}",
                graph.workspace.repos[n].identity,
                self.files[n].at(&s.span.path, s.span.start)
            );
            self.unresolved_sides(id, depth + 1, n, &place);
        }
        // Each node reached once per line it hangs from, however many calls reach it.
        let mut shown = HashSet::new();
        match self.direction {
            Direction::Down => {
                for &call in graph.callees_at(id) {
                    self.call_down(call, depth + 1, &mut shown);
                }
                // A method of a declarative client is a consumer with no call of its own.
                if graph.symbols.contains_key(id) && graph.consumers.contains_key(id) {
                    let (n, place) = (graph.repo(id), self.place(id));
                    self.consumers_down(id, n, &place, depth + 1, true, &mut shown);
                }
                // A writer goes on to the readers of what it writes; a repository-level reader
                // ends the branch.
                for link in (graph.into.get(id).into_iter().flatten()).filter(|l| !calls_on_link(l))
                {
                    let reader = link.consumer.as_str();
                    let ends = graph.ends(reader);
                    if (ends
                        || graph
                            .exports
                            .iter()
                            .any(|e| e.config_sites.iter().any(|s| s.id == reader))
                        || graph.iac.contains_key(reader)
                        || graph.symbols.contains_key(reader)
                        || graph.reader_call(reader).is_some())
                        && shown.insert((reader, true))
                    {
                        self.reach(reader, depth + 1, link);
                    }
                }
            }
            Direction::Up => {
                self.executors_up(id, depth + 1);
                self.provisioners_up(id, depth + 1);
                for &(_, c) in graph.callees.get(id).into_iter().flatten() {
                    self.provisioners_up(&c.id, depth + 1);
                    for link in graph
                        .out
                        .get(c.id.as_str())
                        .into_iter()
                        .flatten()
                        .filter(|link| contract::flows_to_consumer(Some(&link.contract)))
                    {
                        self.writer_up(link, depth + 1, &mut shown);
                    }
                }
                for &call @ (n, c) in graph.callers.get(id).into_iter().flatten() {
                    if !shown.insert((c.caller.as_str(), false)) {
                        continue;
                    }
                    let text = format!("{} {}{}", self.site(call), self.caller(call), mark(c));
                    self.enter(&c.caller, depth + 1, Row::call(text, n, c.level));
                }
                for link in graph.into.get(id).into_iter().flatten() {
                    let Some(&call @ (n, c)) = graph.calls.get(link.consumer.as_str()) else {
                        // A method of a declarative client consumes the route.
                        let method = link.consumer.as_str();
                        if graph.symbols.contains_key(method)
                            && calls_on_link(link)
                            && shown.insert((method, true))
                        {
                            self.reach(method, depth + 1, link);
                        }
                        continue;
                    };
                    if !shown.insert((c.caller.as_str(), true)) {
                        continue;
                    }
                    let text = format!("{} {} {}", arrow(link), self.site(call), self.caller(call));
                    self.enter(&c.caller, depth + 1, Row::link(text, n, link));
                }
                // A reader goes back to the writers of what it reads, and a channel handler to
                // the calls that produce what it handles, then to their callers; a
                // repository-level writer ends the branch.
                for link in (graph.out.get(id).into_iter().flatten()).filter(|l| !calls_on_link(l))
                {
                    self.writer_up(link, depth + 1, &mut shown);
                }
            }
        }
        self.ancestors.pop();
    }

    fn writer_up(&mut self, link: &'a Link, depth: usize, shown: &mut HashSet<(&'a str, bool)>) {
        let writer = link.provider.as_str();
        if let Some(&call @ (n, c)) = self.graph.calls.get(writer) {
            if shown.insert((c.caller.as_str(), true)) {
                let text = format!("{} {} {}", arrow(link), self.site(call), self.caller(call));
                self.enter(&c.caller, depth, Row::link(text, n, link));
            }
        } else if shown.insert((writer, true)) {
            self.reach(writer, depth, link);
        }
    }

    /// Upstream of a contract side is every root that provisions it.
    fn provisioners_up(&mut self, site: &str, depth: usize) {
        let graph = self.graph;
        for link in graph
            .provisions
            .iter()
            .filter(|l| crate::iac::provision::at_site(l, site))
        {
            if let Some((n, p)) = crate::iac::provision::target(graph.exports, link) {
                let text = format!(
                    "provisions {} [{}: {}] {}",
                    link.contract,
                    link.level.name(),
                    link.reason,
                    p.place(&graph.exports[n].repository)
                );
                let mut row = Row::call(text, n, Some(link.level));
                row.link = true;
                self.push(depth, row);
            }
        }
    }

    /// A code symbol is upstream of the workload that runs its repository or code directory.
    fn executors_up(&mut self, id: &str, depth: usize) {
        let graph = self.graph;
        let Some(&(n, symbol)) = graph.symbols.get(id) else {
            return;
        };
        for execution in graph.executions.iter().filter(|x| {
            x.repository == graph.exports[n].repository && in_path(&symbol.span.path, &x.path)
        }) {
            for (m, export) in graph.exports.iter().enumerate() {
                for node in &export.iac {
                    if node.identity(&export.repository) == execution.workload
                        && node.root == execution.root
                        && self
                            .executed
                            .insert((execution.workload.clone(), execution.root.clone()))
                    {
                        let text = format!(
                            "runs code [{}: {}] {} {}:{} {} (root {})",
                            execution.level.name(),
                            execution.reason,
                            export.repository,
                            node.path,
                            node.line,
                            node.address,
                            node.root
                        );
                        let mut row = Row::call(text, m, Some(execution.level));
                        row.link = true;
                        self.push(depth, row);
                    }
                }
            }
        }
    }

    /// What one call reaches: its targets in its repository, and, when it consumes a
    /// contract, the providers it links to, or that it is unresolved or links to none.
    /// `shown` holds the nodes already written under the same line, with whether a link
    /// reached them.
    fn call_down(
        &mut self,
        call @ (n, c): Call<'a>,
        depth: usize,
        shown: &mut HashSet<(&'a str, bool)>,
    ) {
        let graph = self.graph;
        let targets = (c.target.iter().chain(&c.candidates))
            .filter_map(|t| graph.symbols.get(t.as_str()).copied())
            .collect();
        for set in render::overload_sets(targets, |(_, s)| &s.id) {
            let &(m, s) = &set[0];
            if !shown.insert((&s.id, false)) {
                continue;
            }
            let mut text = self.node(&s.id).1;
            if set.len() > 1 {
                // Overloads of one method are one guess: a leaf, as an ambiguous step is.
                let (name, n) = (render::overload_name(&s.id), set.len());
                text.truncate(text.len() - label(&s.id).len());
                text += &format!("{name} {}", render::overloads(n, false));
                self.push(depth, Row::call(text + &mark(c), m, c.level));
                continue;
            }
            self.enter(&s.id, depth, Row::call(text + &mark(c), m, c.level));
        }
        let place = self.site(call);
        self.consumers_down(&c.id, n, &place, depth, false, shown);
        self.unresolved_sides(&c.id, depth, n, &place);
        // A call that produces to a channel goes on to the handlers of its consumers.
        for link in graph.into.get(c.id.as_str()).into_iter().flatten() {
            let handler = link.consumer.as_str();
            if shown.insert((handler, true)) {
                self.reach(handler, depth, link);
            }
        }
    }

    /// What the consumers at `site` (in repository `n`, written `place`) reach: the providers
    /// they link to, or a line saying one is unresolved or links to none. At a definition (a
    /// method of a declarative client, which has no call) only the consumers whose kind calls
    /// on from one (`ContractKind::calls_from_definition`) count.
    fn consumers_down(
        &mut self,
        site: &'a str,
        n: usize,
        place: &str,
        depth: usize,
        at_definition: bool,
        shown: &mut HashSet<(&'a str, bool)>,
    ) {
        let graph = self.graph;
        for (consumer, links) in graph.consumer_links(site, at_definition) {
            if links.is_empty() {
                let text = match &consumer.contract {
                    Some(contract) => format!("{place} consumes {contract}, no provider"),
                    None => format!("{place} {}", unresolved(site)),
                };
                self.push(depth, Row::call(text, n, None));
            }
            for link in links {
                if shown.insert((&link.provider, true)) {
                    self.reach(&link.provider, depth, link);
                }
            }
        }
    }

    /// Writes the line `link` reaches `id` by, then follows `id`, unless the chain ends there
    /// (`Graph::ends`): that line closes the branch.
    fn reach(&mut self, id: &'a str, depth: usize, link: &Link) {
        let (m, text) = self.node(id);
        let row = Row::link(format!("{} {text}", arrow(link)), m, link);
        match self.graph.ends(id) {
            true => self.push(depth, row),
            false => self.enter(id, depth, row),
        }
    }

    /// The unresolved channel sides at `site`, one row each: they link to nothing, and say so.
    fn unresolved_sides(&mut self, site: &str, depth: usize, repo: usize, place: &str) {
        let sides = self.graph.unresolved_sides.get(site).into_iter().flatten();
        for &(side, contract) in sides {
            let text = format!("{place} {} {}", side.verb(), contract::shown(contract));
            self.push(depth, Row::call(text, repo, None));
        }
    }

    /// Writes `row`, indented `depth` levels.
    fn push(&mut self, depth: usize, mut row: Row) {
        row.text.insert_str(0, &"  ".repeat(depth));
        row.text += &std::mem::take(&mut row.end);
        self.rows.push(row);
    }

    /// `<repository> <path>:<line> <name>` at a symbol's definition; a file (the caller of
    /// top-level calls, or a route file serving with no function of its own) is
    /// `<repository> <path> (top level)`, and a route only a document lists
    /// `<repository> <document> GET /users/{} (documented only)`.
    fn node(&mut self, id: &str) -> (usize, String) {
        let graph = self.graph;
        let n = graph.repo(id);
        let identity = &graph.workspace.repos[n].identity;
        let document = graph.documented.get(id);
        let text = match graph.symbols.get(id) {
            Some((_, s)) => {
                let at = self.files[n].at(&s.span.path, s.span.start);
                format!("{identity} {at} {}", label(id))
            }
            None if graph.calls.contains_key(id) => {
                let call = graph.calls[id];
                format!("{} {}", self.site(call), self.caller(call))
            }
            None if graph.iac.contains_key(id) => {
                let (_, node) = graph.iac[id];
                format!("{identity} {}:{} {}", node.path, node.line, node.address)
            }
            None if graph.exports[n].config_sites.iter().any(|s| s.id == id) => {
                let site = graph.exports[n]
                    .config_sites
                    .iter()
                    .find(|s| s.id == id)
                    .unwrap();
                format!("{identity} {}", site.place())
            }
            None if graph.exports[n].config_uses.iter().any(|u| u.site == id) => {
                let u = graph.exports[n]
                    .config_uses
                    .iter()
                    .find(|u| u.site == id)
                    .unwrap();
                format!("{identity} use {} [{}]", u.place(), u.level.name())
            }
            None if graph.is_repo(id) => format!("{identity} {REPOSITORY_LEVEL}"),
            None => match document {
                Some(doc) => format!("{identity} {doc} {} {DOCUMENTED_ONLY}", label(id)),
                None => format!("{identity} {} (top level)", label(id)),
            },
        };
        (n, text)
    }

    /// `<repository> <path>:<line>` of a call site.
    fn site(&mut self, (n, c): Call) -> String {
        let at = self.files[n].at(&c.span.path, c.span.start);
        format!("{} {at}", self.graph.workspace.repos[n].identity)
    }

    /// The name of the symbol making a call, `(top level)` outside any.
    fn caller(&self, (_, c): Call) -> &'a str {
        match self.graph.symbols.get(c.caller.as_str()) {
            Some((_, s)) => label(&s.id),
            None => "(top level)",
        }
    }

    /// The chain as `meada_core::trace::Chain` answers it, with the missing peers.
    fn finish(self, budget: Option<usize>) -> String {
        let missing = (self.graph.workspace.missing.iter())
            .map(Missing::text)
            .collect();
        let chain = Chain {
            rows: self.rows,
            missing,
        };
        chain.answer(budget)
    }
}

/// How a step of a process was reached.
#[derive(Clone, Copy)]
enum Via<'a> {
    /// It is the entry point itself.
    Entry,
    /// A call inside a repository, or a note under one.
    Call,
    Link(&'a Link),
}

/// How deep a process walk follows one branch before it stops and says so.
// ponytail: a fixed depth keeps the recursion off the stack limit however long a call chain is;
// a walk with its own stack lifts it.
const PROCESS_DEPTH: usize = 256;

/// One step of a process: the structured list `trace` renders and the export carries.
pub(crate) struct Step<'a> {
    depth: usize,
    /// The position of the repository holding `node`.
    repo: usize,
    via: Via<'a>,
    /// The level of the call or link that reached it.
    level: Option<Level>,
    /// What the step is at: a symbol, a file, a repository or a side only a document lists, or
    /// for a note, the call site or symbol it is about.
    node: &'a str,
    mark: Option<Mark>,
    /// The candidates of an ambiguous call or link.
    candidates: Vec<&'a str>,
    contract: Option<&'a str>,
}

impl<'a> Step<'a> {
    fn new(depth: usize, repo: usize, via: Via<'a>, level: Option<Level>, node: &'a str) -> Self {
        Self {
            depth,
            repo,
            via,
            level,
            node,
            mark: None,
            candidates: Vec::new(),
            contract: None,
        }
    }

    fn marked(mut self, mark: Mark) -> Self {
        self.mark = Some(mark);
        self
    }

    fn about(mut self, contract: &'a str) -> Self {
        self.contract = Some(contract);
        self
    }
}

/// What a step of a process goes on to.
enum Edge<'a> {
    /// An unresolved channel side at `site`, `Produces` or `Consumes` `contract`.
    Side {
        site: &'a str,
        repo: usize,
        mark: Mark,
        contract: &'a str,
    },
    /// A call to the symbol `to`.
    Call { to: &'a str, level: Option<Level> },
    /// An ambiguous call or link: a guess, so one step listing `candidates`, not followed.
    /// `at` is the call site.
    Guess {
        at: &'a str,
        repo: usize,
        level: Option<Level>,
        candidates: Vec<&'a str>,
    },
    /// A consumer at `at` that nothing serves: `contract` is `None` when Meada could not read it.
    Dead {
        at: &'a str,
        repo: usize,
        contract: Option<&'a str>,
    },
    /// A link followed to its far end `to`.
    Link { to: &'a str, link: &'a Link },
}

impl<'a> Graph<'a> {
    /// The edges the symbol or file `id` expands into, in order: its unresolved channel sides,
    /// then each outgoing call in source order of its call site (then identity) with what it
    /// reaches, then, for a writer or producer, the readers of what it writes. The one place
    /// that says which edges a step has.
    fn edges(&self, id: &'a str) -> Vec<Edge<'a>> {
        let mut edges = Vec::new();
        self.sides_at(id, self.repo(id), &mut edges);
        let mut shown = HashSet::new();
        for &call in self.callees_at(id) {
            self.call_edges(call, &mut shown, &mut edges);
        }
        // A method of a declarative client is a consumer with no call of its own.
        if self.symbols.contains_key(id) {
            self.consumer_edges(id, self.repo(id), true, &mut shown, &mut edges);
        }
        // A writer goes on to the readers of what it writes; a repository-level reader ends
        // the branch.
        for link in (self.into.get(id).into_iter().flatten()).filter(|l| !calls_on_link(l)) {
            let reader = link.consumer.as_str();
            let reaches = self.ends(reader)
                || self.symbols.contains_key(reader)
                || self.reader_call(reader).is_some();
            if reaches && shown.insert((reader, true)) {
                edges.push(Edge::Link { to: reader, link });
            }
        }
        edges
    }

    /// What one call reaches, as `Walk::call_down` finds it, but an ambiguous call or link is
    /// one guess.
    fn call_edges(
        &self,
        (n, c): Call<'a>,
        shown: &mut HashSet<(&'a str, bool)>,
        edges: &mut Vec<Edge<'a>>,
    ) {
        if c.target.is_none() && !c.candidates.is_empty() && shown.insert((&c.id, false)) {
            edges.push(Edge::Guess {
                at: &c.id,
                repo: n,
                level: c.level,
                candidates: c.candidates.iter().map(String::as_str).collect(),
            });
        }
        if let Some(t) = &c.target
            && let Some(&(_, s)) = self.symbols.get(t.as_str())
            && shown.insert((&s.id, false))
        {
            edges.push(Edge::Call {
                to: &s.id,
                level: c.level,
            });
        }
        self.consumer_edges(&c.id, n, false, shown, edges);
        self.sides_at(&c.id, n, edges);
        // A call that produces to a channel goes on to the handlers of its consumers.
        for link in self.into.get(c.id.as_str()).into_iter().flatten() {
            let handler = link.consumer.as_str();
            if shown.insert((handler, true)) {
                edges.push(Edge::Link { to: handler, link });
            }
        }
    }

    /// What the consumers at `site` reach, as `Walk::consumers_down` finds it, but an ambiguous
    /// link is one guess.
    fn consumer_edges(
        &self,
        site: &'a str,
        n: usize,
        at_definition: bool,
        shown: &mut HashSet<(&'a str, bool)>,
        edges: &mut Vec<Edge<'a>>,
    ) {
        for (consumer, links) in self.consumer_links(site, at_definition) {
            if links.is_empty() {
                edges.push(Edge::Dead {
                    at: site,
                    repo: n,
                    contract: consumer.contract.as_deref(),
                });
            }
            let (guesses, links): (Vec<&Link>, Vec<&Link>) =
                links.into_iter().partition(|l| l.level == Level::Ambiguous);
            let candidates: Vec<&str> = (guesses.iter())
                .map(|l| l.provider.as_str())
                .filter(|p| shown.insert((p, true)))
                .collect();
            if !candidates.is_empty() {
                edges.push(Edge::Guess {
                    at: site,
                    repo: n,
                    level: Some(Level::Ambiguous),
                    candidates,
                });
            }
            for link in links {
                if shown.insert((&link.provider, true)) {
                    edges.push(Edge::Link {
                        to: &link.provider,
                        link,
                    });
                }
            }
        }
    }

    /// The consumers at `site`, each with the links it has: those on its contract, and for an
    /// unresolved one those of a decision, on a contract no other consumer of the site has. At
    /// a definition (a method of a declarative client, which has no call) only the consumers
    /// whose kind calls on from one (`ContractKind::calls_from_definition`) count.
    fn consumer_links(
        &self,
        site: &str,
        at_definition: bool,
    ) -> Vec<(&'a Consumer, Vec<&'a Link>)> {
        let consumers = self.consumers.get(site).into_iter().flatten();
        let consumers = consumers.copied().filter(|c| {
            if at_definition {
                contract::calls_from_definition(c.contract.as_deref())
            } else {
                !contract::flows_to_consumer(c.contract.as_deref())
            }
        });
        consumers
            .map(|consumer| {
                let out = self.out.get(site).into_iter().flatten().copied();
                let links = match &consumer.contract {
                    Some(contract) => out.filter(|l| l.contract == *contract).collect(),
                    None => {
                        let others = |l: &Link| {
                            let mut all = self.consumers.get(site).into_iter().flatten();
                            all.any(|o| o.contract.as_deref() == Some(l.contract.as_str()))
                        };
                        out.filter(|l| !others(l)).collect()
                    }
                };
                (consumer, links)
            })
            .collect()
    }

    /// The unresolved channel sides at `site`, one edge each.
    fn sides_at(&self, site: &'a str, repo: usize, edges: &mut Vec<Edge<'a>>) {
        for &(mark, contract) in self.unresolved_sides.get(site).into_iter().flatten() {
            edges.push(Edge::Side {
                site,
                repo,
                mark,
                contract,
            });
        }
    }

    /// The entry points whose process reaches the symbol `id`, sorted by identity, each with the
    /// position of its repository. Found by walking upstream: the walk inverts `edges`, the one
    /// set of edge rules, so it crosses exactly the calls and links a process expands (decided
    /// links included, ambiguous calls and links not) and membership agrees with the process
    /// tree, but for chains deeper than `PROCESS_DEPTH`, which only this walk ignores: a node
    /// past depth 256 is no step of the tree, and the limit can hide a node shallower than
    /// that, through `Seen`. E reaches X at depth 255, so X expands and each of its callees, Y
    /// among them, is a `depth-limit` step at 256 whose own callee Z is no step; a later direct
    /// E to X shows X as seen, so Y is not expanded there either, yet `processes_of(Z)` lists E.
    pub(crate) fn processes_of(&self, id: &'a str) -> Vec<(usize, &'a EntryPoint)> {
        // What reaches each node through one edge.
        let mut up: HashMap<&str, Vec<&str>> = HashMap::new();
        let nodes: HashSet<&str> = (self.symbols.keys().chain(self.callees.keys()))
            .copied()
            .chain(
                self.consumers
                    .keys()
                    .copied()
                    .filter(|id| self.reader_call(id).is_some()),
            )
            .collect();
        for from in nodes {
            for edge in self.edges(from) {
                if let Edge::Call { to, .. } | Edge::Link { to, .. } = edge {
                    up.entry(to).or_default().push(from);
                }
            }
        }
        let mut reached = HashSet::from([id]);
        reached.extend(
            self.callees
                .get(id)
                .into_iter()
                .flatten()
                .filter(|(_, call)| self.reader_call(&call.id).is_some())
                .map(|(_, call)| call.id.as_str()),
        );
        let mut todo: Vec<_> = reached.iter().copied().collect();
        while let Some(node) = todo.pop() {
            for &caller in up.get(node).into_iter().flatten() {
                if reached.insert(caller) {
                    todo.push(caller);
                }
            }
        }
        let mut found: Vec<_> = (self.entry_points.values())
            .filter(|(_, e)| reached.contains(e.site.as_str()))
            .copied()
            .collect();
        found.sort_by(|(_, a), (_, b)| a.identity.cmp(&b.identity));
        found
    }

    /// The steps of the process `entry` starts, depth first: a step's children are its
    /// `edges`. The walk follows what `trace` down follows, but an ambiguous call or link is
    /// one step that is not expanded, and a symbol is expanded once
    /// per process, at its first occurrence.
    pub(crate) fn process(&self, entry: &'a EntryPoint) -> Vec<Step<'a>> {
        let mut descent = Descent {
            graph: self,
            steps: Vec::new(),
            ancestors: Vec::new(),
            expanded: HashSet::new(),
        };
        descent.enter(&entry.site, 0, Via::Entry, None);
        descent.steps
    }

    /// `entry`'s process as the export carries it.
    pub(crate) fn export_process(&self, entry: &'a EntryPoint) -> Process {
        let steps = self.process(entry).into_iter().map(|s| {
            let via = match s.via {
                Via::Entry => StepVia::Entry,
                Via::Call => StepVia::Call,
                Via::Link(_) => StepVia::Link,
            };
            let reason = match s.via {
                Via::Link(link) => Some(link.reason.clone()),
                _ => None,
            };
            ProcessStep::new(
                s.depth,
                self.workspace.repos[s.repo].identity.clone(),
                s.level,
                via,
                s.node.to_string(),
                reason,
                s.mark,
                s.candidates.iter().map(|c| c.to_string()).collect(),
                s.contract.map(String::from),
            )
        });
        let (identity, label) = (entry.identity.clone(), entry.label.clone());
        Process::new(identity, entry.kind, label, steps.collect())
    }
}

/// The depth-first walk of one process.
struct Descent<'g, 'a> {
    graph: &'g Graph<'a>,
    steps: Vec<Step<'a>>,
    /// The nodes from the entry point down to the one being followed.
    ancestors: Vec<&'a str>,
    /// Nodes already followed once.
    expanded: HashSet<&'a str>,
}

impl<'a> Descent<'_, 'a> {
    /// Writes the step at `id`, then follows `id` unless that closes a cycle, repeats a node
    /// already followed or is `PROCESS_DEPTH` deep.
    fn enter(&mut self, id: &'a str, depth: usize, via: Via<'a>, level: Option<Level>) {
        let graph = self.graph;
        let mut step = Step::new(depth, graph.repo(id), via, level, id);
        match () {
            _ if self.ancestors.contains(&id) => step = step.marked(Mark::Cycle),
            _ if self.expanded.contains(id) => step = step.marked(Mark::Seen),
            _ if depth >= PROCESS_DEPTH => step = step.marked(Mark::DepthLimit),
            _ => {}
        }
        let seen = step.mark.is_some();
        self.steps.push(step);
        if seen {
            return;
        }
        self.expanded.insert(id);
        self.ancestors.push(id);
        for edge in graph.edges(id) {
            self.follow(edge, depth + 1);
        }
        self.ancestors.pop();
    }

    /// The step or steps `edge` makes at `depth`.
    fn follow(&mut self, edge: Edge<'a>, depth: usize) {
        match edge {
            Edge::Side {
                site,
                repo,
                mark,
                contract,
            } => {
                let step = Step::new(depth, repo, Via::Call, None, site);
                self.steps.push(step.marked(mark).about(contract));
            }
            Edge::Call { to, level } => self.enter(to, depth, Via::Call, level),
            Edge::Guess {
                at,
                repo,
                level,
                candidates,
            } => {
                let mut step = Step::new(depth, repo, Via::Call, level, at);
                step.candidates = candidates;
                self.steps.push(step.marked(Mark::Ambiguous));
            }
            Edge::Dead { at, repo, contract } => {
                let step = Step::new(depth, repo, Via::Call, None, at);
                self.steps.push(match contract {
                    Some(contract) => step.marked(Mark::NoProvider).about(contract),
                    None => step.marked(Mark::Unresolved),
                });
            }
            Edge::Link { to, link } => self.reach(to, depth, link),
        }
    }

    /// The step `link` reaches `id` by, then `id` followed, unless the chain ends there.
    fn reach(&mut self, id: &'a str, depth: usize, link: &'a Link) {
        let level = Some(link.level);
        match self.graph.ends(id) {
            true => {
                let step = Step::new(depth, self.graph.repo(id), Via::Link(link), level, id);
                self.steps.push(step);
            }
            false => self.enter(id, depth, Via::Link(link), level),
        }
    }
}

impl<'a> Walk<'_, 'a> {
    /// The process of `entry`, whose repository is at `n`: its label first, then its steps as
    /// the chain shows them.
    fn process(&mut self, n: usize, entry: &'a EntryPoint) {
        self.push(0, Row::call(format!("process {}", entry.label), n, None));
        for step in self.graph.process(entry) {
            self.show(&step);
        }
    }

    /// `<repository> <path>:<line>` of the call site or symbol a note is about.
    fn place(&mut self, id: &str) -> String {
        let graph = self.graph;
        match (graph.calls.get(id), graph.symbols.get(id)) {
            (Some(&call), _) => self.site(call),
            (None, Some(&(n, s))) => {
                let at = self.files[n].at(&s.span.path, s.span.start);
                format!("{} {at}", graph.workspace.repos[n].identity)
            }
            (None, None) => self.node(id).1,
        }
    }

    /// Writes `step` in the chain's line grammar.
    fn show(&mut self, step: &Step<'a>) {
        let contract = step.contract.unwrap_or_default();
        let row = match step.mark {
            Some(Mark::Ambiguous) => {
                let sets = render::overload_sets(step.candidates.clone(), |c| c);
                let candidates = (sets.iter())
                    .map(|set| match set.as_slice() {
                        [one] => one.to_string(),
                        [first, ..] => {
                            format!(
                                "{} {}",
                                render::overload_name(first),
                                render::overloads(set.len(), false)
                            )
                        }
                        [] => unreachable!(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let text = format!(
                    "{} {}{}: {candidates}",
                    self.place(step.node),
                    callee(step.node),
                    level_mark(step.level)
                );
                Row::call(text, step.repo, step.level)
            }
            Some(Mark::Unresolved) => {
                let text = format!("{} {}", self.place(step.node), unresolved(step.node));
                Row::call(text, step.repo, None)
            }
            Some(Mark::NoProvider) => {
                let text = format!("{} consumes {contract}, no provider", self.place(step.node));
                Row::call(text, step.repo, None)
            }
            Some(side @ (Mark::Produces | Mark::Consumes)) => {
                let text = format!(
                    "{} {} {}",
                    self.place(step.node),
                    side.verb(),
                    contract::shown(contract)
                );
                Row::call(text, step.repo, None)
            }
            seen => {
                let text = self.node(step.node).1;
                let mut row = match step.via {
                    Via::Entry => Row::call(text, step.repo, None),
                    Via::Call => Row::call(text + &level_mark(step.level), step.repo, step.level),
                    Via::Link(link) => {
                        Row::link(format!("{} {text}", arrow(link)), step.repo, link)
                    }
                };
                row.text += match seen {
                    Some(Mark::Cycle) => " (cycle)",
                    Some(Mark::DepthLimit) => " (depth limit)",
                    Some(_) => " (see above)",
                    None => "",
                };
                row
            }
        };
        self.push(step.depth, row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::decisions::Decisions;
    use crate::entry::EntryKind;
    use crate::index::Span;
    use crate::workspace::Repository;

    #[test]
    fn a_chain_past_the_depth_limit_ends_at_it() {
        let id = |n: usize| format!("r::a.ts::f{n}");
        let span = || {
            let mut span = Span::default();
            span.path = "a.ts".into();
            span
        };
        let length = PROCESS_DEPTH + 50;
        let export = Export {
            repository: "r".into(),
            symbols: (0..=length)
                .map(|n| {
                    let mut symbol = Symbol::default();
                    symbol.id = id(n);
                    symbol.span = span();
                    symbol
                })
                .collect(),
            calls: (0..length)
                .map(|n| {
                    let mut call = CallSite::default();
                    call.id = format!("r::a.ts::f{n}@{n}");
                    call.caller = id(n);
                    call.target = Some(id(n + 1));
                    call.level = Some(Level::Exact);
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
        let entry = EntryPoint {
            identity: id(0),
            kind: EntryKind::Export,
            label: "f0".into(),
            site: id(0),
        };
        let exports = [export];
        let graph = Graph::new(&workspace, &exports, &[]);

        let steps = graph.process(&entry);

        assert_eq!(steps.len(), PROCESS_DEPTH + 1);
        let last = steps.last().unwrap();
        assert_eq!(last.depth, PROCESS_DEPTH);
        assert!(last.mark == Some(Mark::DepthLimit));
        assert!(steps[..PROCESS_DEPTH].iter().all(|s| s.mark.is_none()));
    }
}
