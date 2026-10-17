//! Entry points (spec #31): the doors into a repository, each with the process it sets in
//! motion. They are local facts, found from the repository's own contracts and calls when its
//! index is written and stored in `entry_points`; the process itself is walked on demand
//! (`trace::Graph::process`), never stored.
//!
//! An entry point's identity is its handler's identity plus, for a route or a channel, the contract
//! identifier: `<repository>::<path>::<handler>::http::GET::/users/{}`. An export's is the
//! symbol's own identity. No score or threshold picks them: every one gets its process.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::channel;
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
    /// A channel a consumer handler consumes, one entry point per channel.
    Channel,
    /// A program's start, as its language pack declares it (`Facts::programs`).
    Program,
}

impl EntryKind {
    /// The name the index stores and the export shows.
    pub fn name(self) -> &'static str {
        match self {
            EntryKind::Route => "route",
            EntryKind::Export => "export",
            EntryKind::Channel => "channel",
            EntryKind::Program => "program",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [
            EntryKind::Route,
            EntryKind::Export,
            EntryKind::Channel,
            EntryKind::Program,
        ]
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
    /// `(site, contract)` of each consumer the code gives.
    pub consumers: &'a [(String, String)],
    /// `(file, definition name)` of each program entry point the language packs declare. A name
    /// with no symbol is dropped silently: each pack's conformance golden catches a wrong one.
    pub programs: &'a [(String, String)],
}

/// The entry points of a repository, sorted by identity.
pub fn find(local: &Local) -> Vec<EntryPoint> {
    let routes = routes(local);
    // A program entry point comes first: dedup keeps it over an export of the same symbol.
    let mut found = programs(local);
    found.extend(routes);
    found.extend(channels(local));
    let served: HashSet<&str> = found.iter().map(|e| e.site.as_str()).collect();
    let exports = exports(local, &served);
    found.extend(exports);
    found.sort_by(|a, b| a.identity.cmp(&b.identity));
    // The same handler can be registered twice on one full path.
    found.dedup_by(|a, b| a.identity == b.identity);
    found
}

/// One entry point per program entry point a pack declares, whether or not anything calls it.
fn programs(local: &Local) -> Vec<EntryPoint> {
    let mut found = Vec::new();
    for (file, def) in local.programs {
        let Some(name) = local.names.def(file, def) else {
            continue;
        };
        let id = format!("{}::{file}::{name}", local.identity);
        if !local.symbols.contains(&id) {
            continue;
        }
        found.push(EntryPoint {
            identity: id.clone(),
            kind: EntryKind::Program,
            label: name,
            site: id,
        });
    }
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

/// One entry point per channel a consumer handler consumes, a symbol or a code file like a
/// route handler. An unresolved side (`channel::kafka`) is one too: the handler still consumes
/// from somewhere, and without its entry point it would show up nowhere (it would also be an
/// export entry point otherwise), so it is labelled as `channel::shown` prints it.
fn channels(local: &Local) -> Vec<EntryPoint> {
    let files: HashSet<String> = (local.files.keys())
        .map(|path| format!("{}::{path}", local.identity))
        .collect();
    let mut found = Vec::new();
    for (site, contract) in local.consumers {
        if channel::parse(contract).is_none() {
            continue;
        }
        if !local.symbols.contains(site) && !files.contains(site) {
            continue;
        }
        found.push(EntryPoint {
            identity: format!("{site}::{contract}"),
            kind: EntryKind::Channel,
            label: channel::shown(contract),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The `(label, kind)` of the entry points of a file with the symbols `main`, `run`
    /// (exported) and `__main__`.
    fn found(programs: &[&str], called: &[&str]) -> Vec<(String, &'static str)> {
        let id = |n: &str| format!("r::cmd.py::{n}");
        let symbols: HashSet<String> = ["main", "run", "__main__"].map(id).into();
        let called: HashSet<String> = called.iter().map(|n| id(n)).collect();
        let programs: Vec<(String, String)> = (programs.iter())
            .map(|d| ("cmd.py".to_string(), d.to_string()))
            .collect();
        let exported = [Exported {
            path: "cmd.py".into(),
            name: Some("run".into()),
            file: Some("cmd.py".into()),
            target: Some("run".into()),
        }];
        let mut facts = Facts::default();
        for name in ["main", "run", "__main__"] {
            facts.defs.push(crate::lang::Def {
                name: name.into(),
                kind: Kind::Function,
                range: 0..0,
                params: Vec::new(),
                returns: None,
                type_params: Vec::new(),
            });
        }
        let files = BTreeMap::from([("cmd.py", &facts)]);
        let local = Local {
            identity: "r",
            files: &files,
            names: &Names::new(&[]),
            symbols: &symbols,
            called: &called,
            exported: &exported,
            providers: &[],
            consumers: &[],
            programs: &programs,
        };
        (find(&local).into_iter())
            .map(|e| (e.label, e.kind.name()))
            .collect()
    }

    #[test]
    fn a_handler_of_two_channels_has_one_entry_each() {
        let symbols: HashSet<String> = ["r::a.ts::handle"].map(String::from).into();
        let facts = Facts::default();
        let files = BTreeMap::from([("a.ts", &facts), ("b.ts", &facts)]);
        let consumers: Vec<(String, String)> = [
            ("r::a.ts::handle", "channel::kafka::orders.created"),
            ("r::a.ts::handle", "channel::kafka::orders.paid"),
            ("r::a.ts::handle", "channel::kafka"),
            ("r::b.ts", "channel::rabbitmq::jobs"),
            ("r::a.ts::handle", "http::GET::/x"),
            ("r::a.ts::gone", "channel::kafka::ghost"),
        ]
        .map(|(s, c)| (s.to_string(), c.to_string()))
        .into();
        let local = Local {
            identity: "r",
            files: &files,
            names: &Names::new(&[]),
            symbols: &symbols,
            called: &HashSet::new(),
            exported: &[],
            providers: &[],
            consumers: &consumers,
            programs: &[],
        };

        let got: Vec<(String, String)> = (channels(&local).into_iter())
            .map(|e| (e.identity, e.label))
            .collect();

        let pair = |i: &str, l: &str| (i.to_string(), l.to_string());
        assert_eq!(
            got,
            [
                pair(
                    "r::a.ts::handle::channel::kafka::orders.created",
                    "channel::kafka::orders.created"
                ),
                pair(
                    "r::a.ts::handle::channel::kafka::orders.paid",
                    "channel::kafka::orders.paid"
                ),
                pair(
                    "r::a.ts::handle::channel::kafka",
                    "channel::kafka (unresolved)"
                ),
                pair(
                    "r::b.ts::channel::rabbitmq::jobs",
                    "channel::rabbitmq::jobs"
                ),
            ]
        );
    }

    #[test]
    fn a_declared_program_entry_is_a_door_even_when_called() {
        let got = found(&["main", "__main__"], &["main"]);
        let program = |l: &str| (l.to_string(), "program");
        assert_eq!(
            got,
            [
                program("__main__"),
                program("main"),
                ("run".into(), "export")
            ]
        );
    }

    #[test]
    fn a_program_that_is_also_an_export_is_one_program_entry() {
        assert_eq!(found(&["run"], &[]), [("run".into(), "program")]);
    }

    #[test]
    fn a_declared_name_without_a_symbol_is_no_entry() {
        assert_eq!(found(&["gone"], &[]), [("run".into(), "export")]);
    }

    #[test]
    fn no_declared_program_adds_nothing() {
        assert_eq!(found(&[], &[]), [("run".into(), "export")]);
    }
}
