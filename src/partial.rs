//! A partial export (#396): what `search`, `context` and `trace` read of the indexes. Each
//! repository's export holds its contract tables whole (`index::contracts`: providers,
//! consumers, documents, manifests, entry points and the rows imports resolve through), which
//! links are derived from at every query, and of its code rows (symbols, calls with their
//! candidates) only those a query asked for, read with SQL filters. Nothing outlives the query.
//!
//! The rows asked for are kept as `index::export` sorts them, so any code that looks a row up in
//! an export finds what it would find in the whole one, as long as the query asked for it first;
//! code that scans every symbol of an export, or counts or places rows without holding them,
//! reads the index through the functions at the end of this file.
//!
//! `with_every_row` is the one switch to the whole export, for the test that compares both
//! answers: under it `Engine::partial` reads `index::export`, and each function at the end of this
//! file answers from it as the code before #396 did.

use std::cell::Cell;
use std::collections::HashMap;

use rusqlite::Connection;

use crate::Result;
use crate::contract::Link;
use crate::drift_view;
use crate::index::{self, CallSite, Export, Span, Symbol};
use crate::workspace::Repository;

thread_local! {
    static EVERY_ROW: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` with this thread's queries (`search`, `context`, `trace`, open items) reading every
/// row of the indexes, as `meada export` does, instead of the rows of their answer (#396): the
/// test that compares both answers runs them under it. Test-only: no command, option or setting
/// reaches it.
#[doc(hidden)]
pub fn with_every_row<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            EVERY_ROW.set(self.0);
        }
    }
    let _restore = Restore(EVERY_ROW.replace(true));
    f()
}

/// Whether this thread's queries read every row (`with_every_row`).
pub(crate) fn every_row() -> bool {
    EVERY_ROW.get()
}

pub(crate) struct Partial<'d> {
    /// The index of each repository, in workspace order.
    dbs: &'d [Connection],
    /// One export per repository, in workspace order.
    pub(crate) exports: Vec<Export>,
    /// The repositories whose every symbol is read.
    every_symbol: Vec<bool>,
}

impl<'d> Partial<'d> {
    /// Each repository's contract rows (`index::contracts`), one per index of `dbs` (those of
    /// `repos`), with no code row yet; under `with_every_row`, each whole index
    /// (`index::export`), every row already read.
    pub(crate) fn open(dbs: &'d [Connection], repos: &[Repository]) -> Result<Self> {
        let every_row = every_row();
        let exports = (dbs.iter().zip(repos))
            .map(|(db, m)| {
                let mut export = match every_row {
                    true => index::export(db, &m.identity)?,
                    false => index::contracts(db, &m.identity)?,
                };
                export.root = m.root.clone();
                Ok(export)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            dbs,
            every_symbol: vec![every_row; exports.len()],
            exports,
        })
    }

    /// The positions `at` names: one repository, or all of them.
    fn at(&self, at: Option<usize>) -> Vec<usize> {
        match at {
            Some(n) => vec![n],
            None => (0..self.exports.len()).collect(),
        }
    }

    /// Reads every symbol of the repository at `n`.
    pub(crate) fn every_symbol(&mut self, n: usize) -> Result<()> {
        if !self.every_symbol[n] {
            let mut all = Vec::new();
            index::each_symbol(&self.dbs[n], |s| all.push(s))?;
            self.exports[n].symbols = all;
            self.every_symbol[n] = true;
        }
        Ok(())
    }

    /// Reads the symbols `ids` names, in the repository at `at` or in every one.
    pub(crate) fn symbols<S: AsRef<str>>(&mut self, at: Option<usize>, ids: &[S]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        for n in self.at(at) {
            if !self.every_symbol[n] {
                let found = index::symbols_of(&self.dbs[n], ids)?;
                self.add_symbols(n, found);
            }
        }
        Ok(())
    }

    /// Adds `found` to the symbols of the repository at `n`, each row once.
    pub(crate) fn add_symbols(&mut self, n: usize, found: Vec<Symbol>) {
        if found.is_empty() || self.every_symbol[n] {
            return;
        }
        merge(&mut self.exports[n].symbols, found, |s| {
            (
                s.id.as_str(),
                s.span.path.as_str(),
                s.span.start,
                s.span.end,
            )
        });
    }

    /// Reads the call sites `ids` names; returns the rows read.
    pub(crate) fn calls<S: AsRef<str>>(
        &mut self,
        at: Option<usize>,
        ids: &[S],
    ) -> Result<Vec<(usize, CallSite)>> {
        self.read_calls(at, ids, index::calls_of)
    }

    /// Reads the calls the symbols or files `callers` make; returns the rows read.
    pub(crate) fn callees<S: AsRef<str>>(
        &mut self,
        at: Option<usize>,
        callers: &[S],
    ) -> Result<Vec<(usize, CallSite)>> {
        self.read_calls(at, callers, index::calls_by)
    }

    /// Reads the calls whose target is one of `targets`, or, with `candidates`, that list one
    /// among their candidates; returns the rows read.
    pub(crate) fn callers<S: AsRef<str>>(
        &mut self,
        at: Option<usize>,
        targets: &[S],
        candidates: bool,
    ) -> Result<Vec<(usize, CallSite)>> {
        self.read_calls(at, targets, |db, ids, held| {
            index::calls_to(db, ids, candidates, held)
        })
    }

    /// Reads with `read` in the repositories `at` names, the candidates of the calls already
    /// held left unread (`index::calls_where`): an identity names one call site of a
    /// repository (`index::call_ids` numbers them), so a held call is whole. Returns the rows
    /// read as the export holds them, candidates included, with the position of their
    /// repository.
    fn read_calls<S: AsRef<str>>(
        &mut self,
        at: Option<usize>,
        ids: &[S],
        read: impl Fn(&Connection, &[S], &dyn Fn(&str) -> bool) -> Result<Vec<CallSite>>,
    ) -> Result<Vec<(usize, CallSite)>> {
        let mut read_all = Vec::new();
        if ids.is_empty() {
            return Ok(read_all);
        }
        for n in self.at(at) {
            let calls = &self.exports[n].calls;
            let held = |id: &str| calls.binary_search_by(|c| c.id.as_str().cmp(id)).is_ok();
            let found = read(&self.dbs[n], ids, &held)?;
            let keys: Vec<(String, String, i64, i64)> = (found.iter())
                .map(|c| (c.id.clone(), c.span.path.clone(), c.span.start, c.span.end))
                .collect();
            self.add_calls(n, found);
            let calls = &self.exports[n].calls;
            for (id, path, start, end) in keys {
                let key = (id.as_str(), path.as_str(), start, end);
                if let Ok(at) = calls.binary_search_by(|c| call_key(c).cmp(&key)) {
                    read_all.push((n, calls[at].clone()));
                }
            }
        }
        Ok(read_all)
    }

    /// Adds `found` to the calls of the repository at `n`, each row once.
    pub(crate) fn add_calls(&mut self, n: usize, found: Vec<CallSite>) {
        if found.is_empty() {
            return;
        }
        let calls = &mut self.exports[n].calls;
        merge(calls, found, call_key);
        // An identity names one call site (`index::call_ids`), so `read_calls` may leave a
        // held call's candidates unread.
        debug_assert!(
            calls.windows(2).all(|w| w[0].id != w[1].id),
            "two call sites share an identity"
        );
    }

    /// Reads the symbols that make the calls `calls` (the caller of each), in their repository.
    pub(crate) fn callers_of(&mut self, calls: &[(usize, CallSite)]) -> Result<()> {
        for n in 0..self.exports.len() {
            let ids: Vec<&str> = (calls.iter())
                .filter(|(m, _)| *m == n)
                .map(|(_, c)| c.caller.as_str())
                .collect();
            self.symbols(Some(n), &ids)?;
        }
        Ok(())
    }

    /// Reads the call sites and symbols `sites` names in the repository at each position, and
    /// the symbols making those calls: what places a side or an item.
    pub(crate) fn sites(&mut self, sites: Vec<(usize, String)>) -> Result<()> {
        for n in 0..self.exports.len() {
            let ids: Vec<&str> = (sites.iter())
                .filter(|(m, _)| *m == n)
                .map(|(_, id)| id.as_str())
                .collect();
            self.symbols(Some(n), &ids)?;
            let calls = self.calls(Some(n), &ids)?;
            self.callers_of(&calls)?;
        }
        Ok(())
    }

    /// Reads the calls `drift_view::entries` places for `links`, and the symbols making them.
    pub(crate) fn drift(&mut self, links: &[Link]) -> Result<()> {
        let sites = drift_view::sites(&self.exports, links);
        let calls = self.calls(None, &sites)?;
        self.callers_of(&calls)
    }

    /// The index of the repository at `n`.
    pub(crate) fn db(&self, n: usize) -> &'d Connection {
        &self.dbs[n]
    }
}

/// The whole export of `db`, which the functions below answer from under `with_every_row`.
fn whole_export(db: &Connection) -> Result<Export> {
    index::export(db, "")
}

/// Every symbol of `db`, one at a time, as `export` sorts them (`index::each_symbol`).
pub(crate) fn each_symbol(db: &Connection, f: impl FnMut(Symbol)) -> Result<()> {
    match every_row() {
        true => whole_export(db)?.symbols.into_iter().for_each(f),
        false => index::each_symbol(db, f)?,
    }
    Ok(())
}

/// The symbols of `db` whose hash is one of `hashes` (`index::symbols_hashed`); reading every
/// row, every symbol, the caller keeping the groups a match belongs to.
pub(crate) fn symbols_hashed<S: AsRef<str>>(db: &Connection, hashes: &[S]) -> Result<Vec<Symbol>> {
    match every_row() {
        true => Ok(whole_export(db)?.symbols),
        false => index::symbols_hashed(db, hashes),
    }
}

/// The span of each of `sites` in `db`: the symbol's, else the call site's, the first row of the
/// identity as `export` sorts them. Only the span is placed, so no candidate is read (every call
/// counts as held).
pub(crate) fn spans(db: &Connection, sites: &[&str]) -> Result<HashMap<String, Span>> {
    let mut spans: HashMap<String, Span> = HashMap::new();
    if every_row() {
        let export = whole_export(db)?;
        for site in sites {
            let symbol = export
                .symbols
                .iter()
                .find(|s| s.id == *site)
                .map(|s| &s.span);
            let span =
                symbol.or_else(|| export.calls.iter().find(|c| c.id == *site).map(|c| &c.span));
            if let Some(span) = span {
                spans.insert(site.to_string(), span.clone());
            }
        }
        return Ok(spans);
    }
    for s in index::symbols_of(db, sites)? {
        spans.entry(s.id).or_insert(s.span);
    }
    let calls: Vec<&str> = (sites.iter().copied())
        .filter(|s| !spans.contains_key(*s))
        .collect();
    for c in index::calls_of(db, &calls, &|_| true)? {
        spans.entry(c.id).or_insert(c.span);
    }
    Ok(spans)
}

/// Every community of `db` by name, with its number of members (`index::community_sizes`).
pub(crate) fn community_sizes(db: &Connection) -> Result<Vec<(String, usize)>> {
    match every_row() {
        true => Ok((whole_export(db)?.communities.into_iter())
            .map(|c| (c.name, c.members.len()))
            .collect()),
        false => index::community_sizes(db),
    }
}

/// The name of the community `member` belongs to in `db`, the first by name
/// (`index::community_of`).
pub(crate) fn community_of(db: &Connection, member: &str) -> Result<Option<String>> {
    match every_row() {
        true => Ok((whole_export(db)?.communities.into_iter())
            .find(|c| c.members.iter().any(|m| m == member))
            .map(|c| c.name)),
        false => index::community_of(db, member),
    }
}

/// A call row's place in the order of `index::export`.
fn call_key(c: &CallSite) -> (&str, &str, i64, i64) {
    (
        c.id.as_str(),
        c.span.path.as_str(),
        c.span.start,
        c.span.end,
    )
}

/// Adds to `rows`, sorted by `key`, the rows of `found` it does not hold yet, keeping it sorted:
/// a query reads in many small steps, so each step costs the rows it adds and one merge.
fn merge<T>(rows: &mut Vec<T>, found: Vec<T>, key: impl Fn(&T) -> (&str, &str, i64, i64)) {
    let mut fresh: Vec<T> = (found.into_iter())
        .filter(|row| rows.binary_search_by(|r| key(r).cmp(&key(row))).is_err())
        .collect();
    fresh.sort_by(|a, b| key(a).cmp(&key(b)));
    fresh.dedup_by(|a, b| key(a) == key(b));
    rows.extend(fresh);
    // Two sorted runs: the sort merges them.
    rows.sort_by(|a, b| key(a).cmp(&key(b)));
}
