//! Python HTTP consumers (spec #24): requests, httpx and aiohttp. A consumer is a function of a
//! library's module (`requests.get(url)`) or a method of a client it constructs
//! (`session.get(url)`); its HTTP method is its name or, for `request(method, url)`, its first
//! argument. The client's `base_url=` is joined by the root's `based`, a wrapper is read at each
//! of its call sites (`Code::expand`), and the URL and method are read by the root's `fetch`.
//! The rows of `LIBRARIES` also list reqwest and ureq (spec #30), so the Rust pack's clients are
//! read by the same rules (the Rust pack gives the receiver's value, so a receiver that is a
//! call is read there). The receiver's client is found with `Code::origins` (`Follow::Values`:
//! assignments, `with`, `self` attributes, arguments and return values), and a constructor is
//! named by `Code::outside` read as a name (`by_path` false, also for the Rust rows), not by the
//! `outside` an origin carries, which a Rust path call reads as a path.

use super::super::{HTTP_METHODS, based, consumer, fetch};
use crate::contract::Found;
use crate::lang::rust::frameworks::{REQWEST_CLIENTS, UREQ_AGENTS};
use crate::lang::{Facts, Imported, Part, Value};
use crate::resolve::{Code, Expand, Expanded, Follow, Of, Origin};

/// An HTTP client library of Python or Rust, by what it names outside the repository.
struct Library {
    /// The modules whose functions send a request (`requests.get`, `reqwest.blocking.get`).
    modules: &'static [&'static str],
    /// What constructs a client, an object whose methods send requests. A Rust builder chain
    /// (`Client::builder().build()?`) is the constructor that starts it.
    clients: &'static [&'static str],
}

const LIBRARIES: &[Library] = &[
    Library {
        modules: &["requests"],
        clients: &["requests.Session"],
    },
    Library {
        modules: &["httpx"],
        clients: &["httpx.Client", "httpx.AsyncClient"],
    },
    Library {
        modules: &[],
        clients: &["aiohttp.ClientSession"],
    },
    Library {
        modules: &["reqwest", "reqwest.blocking"],
        clients: REQWEST_CLIENTS,
    },
    Library {
        modules: &["ureq"],
        clients: UREQ_AGENTS,
    },
];

/// Whether the file names a library of `LIBRARIES` outside the repository: Python imports its
/// module, Rust uses a path under its crate or writes one with no `use`.
fn names_library(facts: &Facts) -> bool {
    let roots = LIBRARIES
        .iter()
        .flat_map(|l| l.modules.iter().chain(l.clients))
        .filter_map(|n| n.split('.').next());
    let roots: Vec<&str> = roots.collect();
    let head = |path: &str| roots.contains(&path.split('.').next().unwrap_or_default());
    (facts.imports.iter()).any(|i| match &i.name {
        Imported::Path(target) => head(target),
        _ => roots.contains(&i.source.as_str()),
    }) || facts.externs.iter().any(|e| roots.contains(&e.as_str()))
}

/// Whether `member` sends a request: one of the methods, or `request(method, url)`.
fn sends(member: &str) -> bool {
    HTTP_METHODS.contains(&member) || member == "request"
}

/// The base URL the clients that may be the receiver with these `origins` were built
/// with, `None` when it is not a client: `Some(None)` when it has no base URL, an unknown one
/// when the clients differ. A client is the call of a library's constructor that the receiver
/// traces back to, by assignment, `with`, a `self` attribute or an argument.
// ponytail: a Python receiver that is the result of a call (`Session().get(...)`,
// `make().get(...)`) is not read, a Rust one is (the pack gives the receiver's value); a
// parameter given a client takes the base of every caller that passes one.
fn client_base<'a>(code: &Code<'a>, origins: Vec<Origin<'a>>) -> Option<Option<Vec<Part>>> {
    let mut bases: Vec<Option<Vec<Part>>> = Vec::new();
    for (at, i) in origins.iter().filter_map(|o| o.call) {
        let Some(ctor) = code.call(at, i) else {
            continue;
        };
        let name = code.outside(at, &ctor.scope, &ctor.callee, false);
        if !name.is_some_and(|n| {
            LIBRARIES
                .iter()
                .any(|l| l.clients.contains(&n.spelled.as_str()))
        }) {
            continue;
        }
        let base = match ctor.keyword("base_url") {
            None => None,
            Some(url) => match code.value(at, &ctor.scope, url) {
                Value::Undefined => None,
                Value::Str(parts) => Some(parts),
                _ => Some(vec![Part::Hole]),
            },
        };
        if !bases.contains(&base) {
            bases.push(base);
        }
    }
    match bases.len() {
        0 => None,
        1 => bases.pop(),
        _ => Some(Some(vec![Part::Hole])),
    }
}

/// Python HTTP consumers: a function of a library's module (`requests.get(url)`) or a method
/// of a client it constructs (`session.get(url)`), whose HTTP method is its name or, for
/// `request(method, url)`, its first argument. A wrapper around one is read at each of its call
/// sites, as `fetch` wrappers are.
pub(in crate::http) fn consumers(code: &Code) -> Vec<Found> {
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        let imports = names_library(facts);
        for (n, call) in facts.calls.iter().enumerate() {
            let function = imports
                .then(|| code.outside(path, &call.scope, &call.callee, false))
                .flatten()
                .and_then(|outside| {
                    let (module, function) = outside.spelled.rsplit_once('.')?;
                    let library = LIBRARIES.iter().any(|l| l.modules.contains(&module));
                    (library && sends(function)).then(|| (function.to_string(), None))
                });
            let method =
                (call.callee.rsplit_once('.')).filter(|(_, m)| function.is_none() && sends(m));
            let (member, base) = match (function, method) {
                (Some(function), _) => function,
                (None, Some((receiver, member))) => {
                    // A pack that gives the receiver's value reads a receiver that is a call;
                    // Python's is a name.
                    let origins = match code.receiver(path, n) {
                        Some(value) => code.origins(
                            path,
                            &call.scope,
                            Of::Value(value),
                            Follow::Values {
                                passes: crate::lang::rust::passes,
                            },
                        ),
                        None if receiver.starts_with('(') => continue,
                        None => code.origins(
                            path,
                            &call.scope,
                            Of::Name(receiver),
                            Follow::Values {
                                passes: crate::lang::rust::passes,
                            },
                        ),
                    };
                    match client_base(code, origins) {
                        Some(base) => (member.to_string(), base),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let read = |args: &[Value], kwargs: &[(String, Value)]| {
                let arg = |n: usize, key: &str| {
                    let named = kwargs.iter().find(|(k, _)| k == key).map(|(_, v)| v);
                    args.get(n).or(named)
                };
                let (method, url) = match member.as_str() {
                    "request" => (arg(0, "method").cloned(), arg(1, "url")?),
                    name => (
                        Some(Value::Str(vec![Part::Text(name.to_uppercase())])),
                        arg(0, "url")?,
                    ),
                };
                let url = based(base.as_deref(), url);
                fetch(&[url, method.unwrap_or(Value::Unknown)])
            };
            for Expanded {
                site: (site, at),
                found: read,
                ..
            } in code.expand(Expand::Call(path, n), &read)
            {
                found.push(consumer(site, at, read, None));
            }
        }
    }
    found
}
