//! Entry points (spec #31): the doors into a repository, each with the process it sets in
//! motion. They are local facts, found from the repository's own contracts and calls when its
//! index is written and stored in `entry_points`; the process itself is walked on demand
//! (`trace::Graph::process`), never stored.
//!
//! An entry point's identity is its handler's identity plus, for a route, the contract
//! identifier: `<repository>::<path>::<handler>::http::GET::/users/{}`. An export's is the
//! symbol's own identity. No score or threshold picks them: every one gets its process.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::contract::Names;
use crate::http;
use crate::lang::{Facts, Kind};
use crate::resolve::Exported;

/// What kind of door an entry point is. A new kind adds a variant, its `name` and `parse`
/// arms and its finder in `find`; the walk and the export need nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    /// A route an HTTP handler serves.
    Route,
    /// An exported function or method nothing in its repository calls.
    Export,
}

impl EntryKind {
    /// The name the index stores and the export shows.
    pub fn name(self) -> &'static str {
        match self {
            EntryKind::Route => "route",
            EntryKind::Export => "export",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [EntryKind::Route, EntryKind::Export]
            .into_iter()
            .find(|k| k.name() == name)
    }
}

/// One entry point: its identity, kind and label (what names its process), and the `site`
/// the process starts from, a symbol or a code file of the repository.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryPoint {
    pub identity: String,
    pub kind: EntryKind,
    pub label: String,
    pub site: String,
}

/// What `find` reads, all of one repository.
pub struct Local<'a> {
    pub identity: &'a str,
    pub files: &'a BTreeMap<&'a str, &'a Facts>,
    pub names: &'a Names,
    /// Every symbol identity of the repository.
    pub symbols: &'a HashSet<String>,
    /// Every symbol some call reaches, an ambiguous candidate included.
    pub called: &'a HashSet<String>,
    pub exported: &'a [Exported],
    /// `(site, contract)` of each provider the code gives.
    pub providers: &'a [(String, String)],
}

/// The entry points of a repository, sorted by identity.
pub fn find(local: &Local) -> Vec<EntryPoint> {
    let routes = routes(local);
    let served: HashSet<&str> = routes.iter().map(|e| e.site.as_str()).collect();
    let mut found = routes.clone();
    found.extend(exports(local, &served));
    found.sort_by(|a, b| a.identity.cmp(&b.identity));
    // The same handler can be registered twice on one full path.
    found.dedup_by(|a, b| a.identity == b.identity);
    found
}

/// One entry point per route a handler serves: a symbol, or a code file when no function of its
/// own serves it (its process starts from the file's top-level calls). A route only a document
/// lists is no entry point.
fn routes(local: &Local) -> Vec<EntryPoint> {
    let files: HashSet<String> = (local.files.keys())
        .map(|path| format!("{}::{path}", local.identity))
        .collect();
    let mut found = Vec::new();
    for (site, contract) in local.providers {
        let Some((method, path)) = http::route(contract) else {
            continue;
        };
        if !local.symbols.contains(site) && !files.contains(site) {
            continue;
        }
        found.push(EntryPoint {
            identity: format!("{site}::{contract}"),
            kind: EntryKind::Route,
            label: format!("{method} {path}"),
            site: site.clone(),
        });
    }
    found
}

/// The exported functions and methods no call reaches, but for route handlers.
fn exports(local: &Local, served: &HashSet<&str>) -> Vec<EntryPoint> {
    let mut defs = BTreeSet::new();
    for e in local.exported {
        let (Some(file), Some(target)) = (&e.file, &e.target) else {
            continue;
        };
        let Some(facts) = local.files.get(file.as_str()) else {
            continue;
        };
        let class = (facts.defs.iter()).any(|d| d.name == *target && d.kind == Kind::Class);
        for def in &facts.defs {
            let own = match def.kind {
                Kind::Function => def.name == *target,
                // A method of an exported class; a constructor is reached as `new Class`.
                Kind::Method => {
                    let method = def.name.strip_prefix(target.as_str());
                    let method = method.and_then(|m| m.strip_prefix('.'));
                    class && method.is_some_and(|m| !m.contains('.') && m != "constructor")
                }
                _ => false,
            };
            if let Some(name) = local.names.def(file, &def.name).filter(|_| own) {
                defs.insert((file.as_str(), name));
            }
        }
    }
    let mut found = Vec::new();
    for (file, name) in defs {
        let id = format!("{}::{file}::{name}", local.identity);
        if local.called.contains(&id) || served.contains(id.as_str()) {
            continue;
        }
        found.push(EntryPoint {
            identity: id.clone(),
            kind: EntryKind::Export,
            label: name,
            site: id,
        });
    }
    found
}
