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

pub(crate) fn every_row() -> bool {
    EVERY_ROW.get()
}

pub(crate) struct Partial<'d> {
    dbs: &'d [Connection],
    pub(crate) exports: Vec<Export>,
    every_symbol: Vec<bool>,
}

impl<'d> Partial<'d> {
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

    fn at(&self, at: Option<usize>) -> Vec<usize> {
        match at {
            Some(n) => vec![n],
            None => (0..self.exports.len()).collect(),
        }
    }

    pub(crate) fn every_symbol(&mut self, n: usize) -> Result<()> {
        if !self.every_symbol[n] {
            let mut all = Vec::new();
            index::each_symbol(&self.dbs[n], |s| all.push(s))?;
            self.exports[n].symbols = all;
            self.every_symbol[n] = true;
        }
        Ok(())
    }

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

    pub(crate) fn calls<S: AsRef<str>>(
        &mut self,
        at: Option<usize>,
        ids: &[S],
    ) -> Result<Vec<(usize, CallSite)>> {
        self.read_calls(at, ids, index::calls_of)
    }

    pub(crate) fn callees<S: AsRef<str>>(
        &mut self,
        at: Option<usize>,
        callers: &[S],
    ) -> Result<Vec<(usize, CallSite)>> {
        self.read_calls(at, callers, index::calls_by)
    }

    pub(crate) fn uses_held<S: AsRef<str>>(
        &mut self,
        at: Option<usize>,
        holders: &[S],
    ) -> Result<Vec<(usize, CallSite)>> {
        self.read_calls(at, holders, index::uses_held)
    }

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

    pub(crate) fn add_calls(&mut self, n: usize, found: Vec<CallSite>) {
        if found.is_empty() {
            return;
        }
        let calls = &mut self.exports[n].calls;
        merge(calls, found, call_key);
        debug_assert!(
            calls.windows(2).all(|w| w[0].id != w[1].id),
            "two call sites share an identity"
        );
    }

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

    pub(crate) fn drift(&mut self, links: &[Link]) -> Result<()> {
        let sites = drift_view::sites(&self.exports, links);
        let calls = self.calls(None, &sites)?;
        self.callers_of(&calls)
    }

    pub(crate) fn db(&self, n: usize) -> &'d Connection {
        &self.dbs[n]
    }
}

fn whole_export(db: &Connection) -> Result<Export> {
    index::export(db, "")
}

pub(crate) fn each_symbol(db: &Connection, f: impl FnMut(Symbol)) -> Result<()> {
    match every_row() {
        true => whole_export(db)?.symbols.into_iter().for_each(f),
        false => index::each_symbol(db, f)?,
    }
    Ok(())
}

pub(crate) fn symbols_hashed<S: AsRef<str>>(db: &Connection, hashes: &[S]) -> Result<Vec<Symbol>> {
    match every_row() {
        true => Ok(whole_export(db)?.symbols),
        false => index::symbols_hashed(db, hashes),
    }
}

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

pub(crate) fn community_sizes(db: &Connection) -> Result<Vec<(String, usize)>> {
    match every_row() {
        true => Ok((whole_export(db)?.communities.into_iter())
            .map(|c| (c.name, c.members.len()))
            .collect()),
        false => index::community_sizes(db),
    }
}

pub(crate) fn community_of(db: &Connection, member: &str) -> Result<Option<String>> {
    match every_row() {
        true => Ok((whole_export(db)?.communities.into_iter())
            .find(|c| c.members.iter().any(|m| m == member))
            .map(|c| c.name)),
        false => index::community_of(db, member),
    }
}

fn call_key(c: &CallSite) -> (&str, &str, i64, i64) {
    (
        c.id.as_str(),
        c.span.path.as_str(),
        c.span.start,
        c.span.end,
    )
}

fn merge<T>(rows: &mut Vec<T>, found: Vec<T>, key: impl Fn(&T) -> (&str, &str, i64, i64)) {
    let mut fresh: Vec<T> = (found.into_iter())
        .filter(|row| rows.binary_search_by(|r| key(r).cmp(&key(row))).is_err())
        .collect();
    fresh.sort_by(|a, b| key(a).cmp(&key(b)));
    fresh.dedup_by(|a, b| key(a) == key(b));
    rows.extend(fresh);
    rows.sort_by(|a, b| key(a).cmp(&key(b)));
}
