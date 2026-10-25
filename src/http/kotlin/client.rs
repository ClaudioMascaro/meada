//! Ktor client consumers (spec #27, #99). A call is a consumer when `Code::outside` says an
//! import from Ktor binds its verb (`get`, `post`, `put`, `patch`, `delete`, `head`, `options`
//! or `request`, of `io.ktor.client.request`) and its receiver is a Ktor `HttpClient`: one made
//! by a call to it that `Code::origins` reaches, or one the source writes the type of. The URL
//! is the call's first argument (or `urlString`), folded like any other value, with a template
//! hole that does not fold a parameter; a call with no such argument (`get { url { .. } }`) is
//! an unresolved consumer, as the builders of the request block are not read. `request` sends
//! the method its block assigns (`method = HttpMethod.X`), `GET` when it assigns none. A
//! `defaultRequest { url(base) }` in the configuration of the client the call comes from is the
//! base of its relative URLs. A client wrapper is read at each of its call sites
//! (`Code::expand`), through any number of levels.
//!
//! A method that depends on a parameter of a wrapper is not substituted (a block's assignment is
//! no argument): the consumer's method is unknown.

use super::{implicit, method};
use crate::contract::Found;
use crate::jvm::names::{one, spellings, typed};
use crate::lang::{Call, Part, Value};
use crate::resolve::{Code, Expand, Expanded, Follow, Of, passes_nothing};

use super::super::{based, consumer, fetch};

/// The package of the request functions.
const REQUEST: &str = "io.ktor.client.request.";
/// Ktor's `HttpClient`.
const CLIENT: &str = "io.ktor.client.HttpClient";
/// The spellings of `defaultRequest`: Ktor 2 and 3, then Ktor 1.
const DEFAULT_REQUEST: &[&str] = &[
    "io.ktor.client.plugins.defaultRequest",
    "io.ktor.client.features.defaultRequest.defaultRequest",
];
/// The request functions that name their method, and the one that takes it from its block.
const VERBS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];
const REQUEST_FUNCTION: &str = "request";

/// A client, as far as the source tells: the call that makes it, only its type, or one whose
/// configuration is not known.
#[derive(PartialEq)]
enum Client<'a> {
    Made(&'a str, usize),
    Typed,
    Unknown,
}

struct Clients<'c, 'a> {
    code: &'c Code<'a>,
}

impl<'c, 'a> Clients<'c, 'a> {
    /// The Ktor clients the receiver of a call may be, `None` when it is not one.
    fn of(&self, path: &'a str, scope: &'a str, receiver: &'a Value) -> Option<Vec<Client<'a>>> {
        let found = self.code.origins(
            path,
            scope,
            Of::Value(receiver),
            Follow::Values {
                passes: passes_nothing,
            },
        );
        let (mut out, mut other) = (Vec::new(), false);
        for origin in found {
            let made = origin.outside.map(|o| o.spellings()).unwrap_or_default();
            match origin.call {
                Some((p, i)) if made.iter().any(|s| s == CLIENT) => out.push(Client::Made(p, i)),
                _ => other = true,
            }
        }
        if out.is_empty() {
            let Value::Ref(name) = receiver else {
                return None;
            };
            let written = typed(self.code, path, scope, name);
            return written
                .iter()
                .any(|s| s == CLIENT)
                .then(|| vec![Client::Typed]);
        }
        if other {
            out.push(Client::Unknown);
        }
        Some(out)
    }

    /// The base URL `clients` share, `None` when none is set. A hole when they differ or one is
    /// not known.
    fn base(&self, clients: &[Client<'a>]) -> Option<Vec<Part>> {
        let mut bases: Vec<Option<Vec<Part>>> = Vec::new();
        for client in clients {
            let base = match client {
                Client::Made(path, index) => self.configured(path, *index),
                Client::Typed => None,
                Client::Unknown => Some(vec![Part::Hole]),
            };
            if !bases.contains(&base) {
                bases.push(base);
            }
        }
        match bases.len() {
            0 => None,
            1 => bases.pop().flatten(),
            _ => Some(vec![Part::Hole]),
        }
    }

    /// The base URL the `defaultRequest { url(..) }` in the configuration block of the call
    /// that makes a client sets: a hole when it is set more than once or not by a string.
    fn configured(&self, path: &'a str, index: usize) -> Option<Vec<Part>> {
        let code = self.code;
        let Some(Value::Function(block)) = code.call(path, index)?.args.last() else {
            return None;
        };
        let calls = |scope: &str, name: &str| -> Vec<(usize, &'a Call)> {
            let found = (code.files()[path].calls.iter().enumerate())
                .filter(|(_, c)| c.scope == scope && implicit(&c.callee) == Some(name));
            found
                .filter_map(|(n, _)| Some((n, code.call(path, n)?)))
                .collect()
        };
        let mut bases: Vec<Vec<Part>> = Vec::new();
        for (_, default) in calls(block, "defaultRequest") {
            let known = spellings(code, path, &default.scope, "defaultRequest");
            if one(&known, |s| DEFAULT_REQUEST.contains(&s)).is_none() {
                continue;
            }
            let Some(Value::Function(lambda)) = default.args.last() else {
                continue;
            };
            let urls = calls(lambda, "url");
            let hole = || vec![Part::Hole];
            let base = match &urls[..] {
                [] => continue,
                [(_, url)] => match url.args.first() {
                    Some(Value::Function(_)) | None => hole(),
                    Some(value) => match code.value(path, &url.scope, value) {
                        Value::Str(parts) => parts,
                        _ => hole(),
                    },
                },
                _ => hole(),
            };
            bases.push(base);
        }
        match &mut bases[..] {
            [] => None,
            [one] => Some(std::mem::take(one)),
            _ => Some(vec![Part::Hole]),
        }
    }

    /// The method a `request` call sends: the one its block assigns, `GET` when it assigns none,
    /// unknown when it is no literal Ktor method, or the call passes its block some other way.
    fn requested(&self, path: &'a str, call: &'a Call) -> Value {
        let unknown = Value::Unknown;
        let text = |m: String| Value::Str(vec![Part::Text(m)]);
        let rest = call.args.get(1..).unwrap_or_default();
        match rest {
            [] => return text("GET".into()),
            [Value::Function(block)] => {
                let set: Vec<_> = (self.code.assigned(path).iter())
                    .filter(|a| a.scope == *block && a.name == "method")
                    .collect();
                return match set[..] {
                    [] => text("GET".into()),
                    [one] => method(self.code, path, block, &one.value).map_or(unknown, text),
                    _ => unknown,
                };
            }
            _ => {}
        }
        unknown
    }
}

/// Ktor client consumers: see the module.
pub(in crate::http) fn consumers(code: &Code) -> Vec<Found> {
    let clients = Clients { code };
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        if !path.ends_with(".kt") && !path.ends_with(".kts") {
            continue;
        }
        for (n, stored) in facts.calls.iter().enumerate() {
            let Some((_, member)) = stored.callee.rsplit_once('.') else {
                continue;
            };
            if !VERBS.contains(&member) && member != REQUEST_FUNCTION {
                continue;
            }
            let spelled = spellings(code, path, &stored.scope, member);
            if one(&spelled, |s| s.strip_prefix(REQUEST) == Some(member)).is_none() {
                continue;
            }
            // The values of the call: `files` holds none, only the facts the index keeps.
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let Some(receiver) = code.receiver(path, n) else {
                continue;
            };
            let Some(of) = clients.of(path, &call.scope, receiver) else {
                continue;
            };
            let base = clients.base(&of);
            let method = match member {
                REQUEST_FUNCTION => clients.requested(path, call),
                verb => Value::Str(vec![Part::Text(verb.to_uppercase())]),
            };
            let read = |args: &[Value], kwargs: &[(String, Value)]| {
                let url = (args.first().filter(|v| !matches!(v, Value::Function(_))))
                    .or_else(|| {
                        kwargs
                            .iter()
                            .find(|(k, _)| k == "urlString")
                            .map(|(_, v)| v)
                    })
                    .cloned()
                    .unwrap_or(Value::Unknown);
                fetch(&[based(base.as_deref(), &url), method.clone()])
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
