//! `trace`: a chain followed from a start through calls inside repositories and links between
//! them, down or up, across the whole workspace (ADR 0005), rendered as indented lines under
//! the token budget and closed by the chain footer (ADR 0008).

use std::collections::{HashMap, HashSet};

use crate::contract::{self, Level, Link};
use crate::index::{CallSite, Consumer, Export, Symbol};
use crate::render::{DOCUMENTED_ONLY, Files, REPOSITORY_LEVEL, callee, label, line_of, unresolved};
use crate::repo::Change;
use crate::workspace::{Missing, Workspace};
use crate::{channel, http, render};

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
    /// A symbol, a route or a call site, by identity or by name.
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
    symbols: HashMap<&'a str, (usize, &'a Symbol)>,
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
    /// the handler): `produces` or `consumes`, and the contract. They are never linked, so the
    /// chain would not otherwise show them.
    unresolved_sides: HashMap<&'a str, Vec<(&'static str, &'a str)>>,
    /// Links by consumer and by provider, each sorted as the engine sorts links. A link's
    /// consumer is a call site, or, for a resource or a channel, the symbol that reads it or
    /// handles it: data goes from the provider (a writer, or the call that produces) to that
    /// consumer, so the chain crosses it the other way.
    out: HashMap<&'a str, Vec<&'a Link>>,
    into: HashMap<&'a str, Vec<&'a Link>>,
}

impl<'a> Graph<'a> {
    /// `exports` in workspace order, and every link between them.
    pub(crate) fn new(workspace: &'a Workspace, exports: &'a [Export], links: &'a [Link]) -> Self {
        let mut graph = Self {
            workspace,
            symbols: HashMap::new(),
            calls: HashMap::new(),
            callees: HashMap::new(),
            callers: HashMap::new(),
            consumers: HashMap::new(),
            providers: Vec::new(),
            documented: HashMap::new(),
            unresolved_sides: HashMap::new(),
            out: HashMap::new(),
            into: HashMap::new(),
        };
        for (n, export) in exports.iter().enumerate() {
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
                .map(|p| ("produces", Some(&p.contract), &p.site))
                .chain(consumed.map(|c| ("consumes", c.contract.as_ref(), &c.site)));
            for (verb, contract, site) in sides {
                if let Some(contract) = contract.filter(|c| channel::is_unresolved(c)) {
                    graph
                        .unresolved_sides
                        .entry(site)
                        .or_default()
                        .push((verb, contract));
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
                Lookup::Answer(lines) => return render::budgeted(&lines, budget),
            },
        }
        walk.finish(budget)
    }

    /// The starts `from` names: the providers of a route, else one symbol, else one call site.
    /// A name that fits several symbols or call sites lists them instead of guessing.
    fn find(&self, from: &str, walk: &mut Walk) -> Lookup<'a> {
        // A route is a whole contract (`GET /users` names one too), or a path on any method.
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
        let (contract, path) = if http::route(from).is_some() {
            (Some(from.to_string()), None)
        } else if let Some((method, path)) = from.split_once(' ')
            && path.starts_with('/')
            && method.chars().all(|c| c.is_ascii_alphabetic())
        {
            (Some(http::contract(method, path)), None)
        } else if from.starts_with('/') {
            (None, Some(format!("::{}", http::normalize_path(from))))
        } else {
            (None, None)
        };
        let route = |c: &str| {
            contract.as_deref() == Some(c) || path.as_deref().is_some_and(|p| c.ends_with(p))
        };
        let mut sites: Vec<&str> = (self.providers.iter())
            .filter(|(c, _)| route(c))
            .map(|&(_, s)| s)
            .collect();
        sites.sort();
        sites.dedup();
        if !sites.is_empty() {
            return Lookup::Nodes(sites);
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
            [] => Lookup::Answer(vec![format!(
                "no symbol, route or call site matches `{from}`"
            )]),
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
        self.is_repo(id) || self.documented.contains_key(id)
    }
}

/// What a `from` names.
enum Lookup<'a> {
    /// Symbols, or files serving a route, to start from.
    Nodes(Vec<&'a str>),
    Call(Call<'a>),
    /// No start: the answer says why.
    Answer(Vec<String>),
}

/// One line of the chain.
struct Row {
    text: String,
    repo: usize,
    /// The level of the call or link that reached it.
    level: Option<Level>,
    link: bool,
    /// What ends the line once it is complete: ` drift: missing <paths>` after a link whose
    /// consumer reads fields its provider lacks (spec #23).
    end: String,
}

impl Row {
    /// A line reached through a call at `level`, or a line with no level.
    fn call(text: String, repo: usize, level: Option<Level>) -> Self {
        Self {
            text,
            repo,
            level,
            link: false,
            end: String::new(),
        }
    }

    /// A line reached through `link`.
    fn link(text: String, repo: usize, link: &Link) -> Self {
        let missing = link.drift.iter().flat_map(|d| &d.missing);
        let missing: Vec<&str> = missing.map(String::as_str).collect();
        Self {
            text,
            repo,
            level: Some(link.level),
            link: true,
            end: match missing.is_empty() {
                true => String::new(),
                false => format!(" drift: missing {}", missing.join(", ")),
            },
        }
    }
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
}

impl<'a> Walk<'_, 'a> {
    fn root(&mut self, id: &'a str) {
        let (n, text) = self.node(id);
        self.enter(id, 0, Row::call(text, n, None));
    }

    /// A call site as the start: the call, then what it reaches or where it sits.
    fn root_call(&mut self, call: Call<'a>) {
        let text = format!("{} {}", self.site(call), callee(&call.1.id));
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
                for &call in graph.callees.get(id).into_iter().flatten() {
                    self.call_down(call, depth + 1, &mut shown);
                }
                // A writer goes on to the readers of what it writes; a repository-level reader
                // ends the branch.
                for link in graph.into.get(id).into_iter().flatten() {
                    let reader = link.consumer.as_str();
                    let ends = graph.ends(reader);
                    if (ends || graph.symbols.contains_key(reader)) && shown.insert((reader, true))
                    {
                        self.reach(reader, depth + 1, link);
                    }
                }
            }
            Direction::Up => {
                for &call @ (n, c) in graph.callers.get(id).into_iter().flatten() {
                    if !shown.insert((c.caller.as_str(), false)) {
                        continue;
                    }
                    let text =
                        format!("{} {}{}", self.site(call), self.caller(call), mark(c.level));
                    self.enter(&c.caller, depth + 1, Row::call(text, n, c.level));
                }
                for link in graph.into.get(id).into_iter().flatten() {
                    let Some(&call @ (n, c)) = graph.calls.get(link.consumer.as_str()) else {
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
                for link in graph.out.get(id).into_iter().flatten() {
                    let writer = link.provider.as_str();
                    if let Some(&call @ (n, c)) = graph.calls.get(writer) {
                        if shown.insert((c.caller.as_str(), true)) {
                            let text = format!(
                                "{} {} {}",
                                arrow(link),
                                self.site(call),
                                self.caller(call)
                            );
                            self.enter(&c.caller, depth + 1, Row::link(text, n, link));
                        }
                        continue;
                    }
                    if shown.insert((writer, true)) {
                        self.reach(writer, depth + 1, link);
                    }
                }
            }
        }
        self.ancestors.pop();
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
        for t in c.target.iter().chain(&c.candidates) {
            let Some(&(m, s)) = graph.symbols.get(t.as_str()) else {
                continue;
            };
            if !shown.insert((&s.id, false)) {
                continue;
            }
            let text = self.node(&s.id).1 + &mark(c.level);
            self.enter(&s.id, depth, Row::call(text, m, c.level));
        }
        let consumers = graph.consumers.get(c.id.as_str());
        for consumer in consumers.into_iter().flatten() {
            let site = self.site(call);
            let out = graph.out.get(c.id.as_str()).into_iter().flatten();
            let links: Vec<&Link> = match &consumer.contract {
                Some(contract) => out.filter(|l| l.contract == *contract).copied().collect(),
                // An unresolved consumer has links only through a decision, on a contract no
                // other consumer of the site has.
                None => {
                    let others = |l: &&&Link| {
                        let mut all = consumers.into_iter().flatten();
                        all.any(|o| o.contract.as_deref() == Some(l.contract.as_str()))
                    };
                    out.filter(|l| !others(l)).copied().collect()
                }
            };
            if links.is_empty() {
                let text = match &consumer.contract {
                    Some(contract) => format!("{site} consumes {contract}, no provider"),
                    None => format!("{site} {}", unresolved(&c.id)),
                };
                self.push(depth, Row::call(text, n, None));
            }
            for link in links {
                if shown.insert((&link.provider, true)) {
                    self.reach(&link.provider, depth, link);
                }
            }
        }
        let place = self.site(call);
        self.unresolved_sides(&c.id, depth, n, &place);
        // A call that produces to a channel goes on to the handlers of its consumers.
        for link in graph.into.get(c.id.as_str()).into_iter().flatten() {
            let handler = link.consumer.as_str();
            if shown.insert((handler, true)) {
                self.reach(handler, depth, link);
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
        for &(verb, contract) in sides {
            let text = format!("{place} {verb} {}", channel::shown(contract));
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

    /// The rows that fit the budget with room for the footer, the cut note if any, then the
    /// footer and the missing peers.
    fn finish(self, budget: Option<usize>) -> String {
        let budget = budget.unwrap_or(render::DEFAULT_BUDGET);
        let missing: Vec<String> = (self.graph.workspace.missing.iter())
            .map(Missing::text)
            .collect();
        let footer = |weakest: Option<Level>, hops: usize, repos: usize, tokens: usize| {
            let weakest = weakest.map_or("none", Level::name);
            let mut text =
                format!("chain: {weakest} · {hops} hops · {repos} repos · {tokens} tokens\n");
            for line in &missing {
                text += line;
                text += "\n";
            }
            text
        };
        let stats = |rows: &[Row]| {
            let weakest = rows.iter().filter_map(|r| r.level).max();
            let hops = rows.iter().filter(|r| r.link).count();
            let repos = rows.iter().map(|r| r.repo).collect::<HashSet<_>>().len();
            (weakest, hops, repos)
        };
        // Room for the widest footer the shown rows can give: an answer within budget uses at
        // most `budget` tokens.
        let (_, hops, repos) = stats(&self.rows);
        let widest = footer(Some(Level::Ambiguous), hops, repos, budget);
        let lines: Vec<String> = self.rows.iter().map(|r| r.text.clone()).collect();
        let room = budget.saturating_sub(render::tokens(&widest));
        let (shown, body) = render::cut(&lines, room, budget);
        let (weakest, hops, repos) = stats(&self.rows[..shown]);
        // The footer counts its own tokens: settle on the count that includes its digits.
        let mut tokens = 0;
        loop {
            let answer = body.clone() + &footer(weakest, hops, repos, tokens);
            match render::tokens(&answer) {
                t if t == tokens => return answer,
                t => tokens = t,
            }
        }
    }
}

/// ` [<level>]` after a call that is not exact.
fn mark(level: Option<Level>) -> String {
    match level {
        Some(Level::Exact) | None => String::new(),
        Some(level) => format!(" [{}]", level.name()),
    }
}

/// `→[<level>: <reason>]`.
fn arrow(link: &Link) -> String {
    format!("→[{}: {}]", link.level.name(), link.reason)
}
