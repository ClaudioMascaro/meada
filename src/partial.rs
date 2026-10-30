//! A partial export (#396): what `search`, `context` and `trace` read of the indexes. Each
//! repository's export holds its contract tables whole (`index::contracts`: providers,
//! consumers, documents, manifests, entry points and the rows imports resolve through), which
//! links are derived from at every query, and of its code rows (symbols, calls with their
//! candidates) only those a query asked for, read with SQL filters. Nothing outlives the query.
//!
//! The rows asked for are kept as `index::export` sorts them, so any code that looks a row up in
//! an export finds what it would find in the whole one, as long as the query asked for it first;
//! code that scans every symbol or call of an export reads the index instead (`index::each_symbol`
//! and the other filtered readers).

use rusqlite::Connection;

use crate::Result;
use crate::index::{self, CallSite, Export, Symbol};

pub(crate) struct Partial<'d> {
    /// The index of each repository, in workspace order.
    dbs: &'d [Connection],
    /// One export per repository, in workspace order.
    pub(crate) exports: Vec<Export>,
    /// The repositories whose every symbol is read.
    every_symbol: Vec<bool>,
}

impl<'d> Partial<'d> {
    /// `exports` (`index::contracts`, one per index of `dbs`) with no code row yet.
    pub(crate) fn new(dbs: &'d [Connection], exports: Vec<Export>) -> Self {
        let every_symbol = vec![false; exports.len()];
        Self {
            dbs,
            exports,
            every_symbol,
        }
    }

    /// `exports`, each the whole of its index (`index::export`): every row already read.
    pub(crate) fn whole(dbs: &'d [Connection], exports: Vec<Export>) -> Self {
        let every_symbol = vec![true; exports.len()];
        Self {
            dbs,
            exports,
            every_symbol,
        }
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
    /// repository (`index::call_ids` numbers them), so a held call is whole. Returns the rows read as the export holds them,
    /// candidates included, with the position of their repository.
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

    /// The index of the repository at `n`.
    pub(crate) fn db(&self, n: usize) -> &'d Connection {
        &self.dbs[n]
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
