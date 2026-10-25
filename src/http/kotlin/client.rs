//! Ktor client consumers (spec #27, #99). A call is a consumer when `Code::outside` says an
//! import from Ktor binds its verb (`get`, `post`, `put`, `patch`, `delete`, `head`, `options`
//! or `request`, of `io.ktor.client.request`) and its receiver is a Ktor `HttpClient`: one made
//! by a call to it that `Code::origins` reaches, or that a local holds (`val c = HttpClient(..)`,
//! or the receiver `use` passes on, found through `Facts::bound`), or one the source writes the
//! type of. A local, a parameter or a loop variable of the name hides a client of the file.
//! The URL is the call's first argument (or `urlString`), folded like any other value, with a
//! template hole that does not fold a parameter; a call with no such argument (`get { url { ..
//! } }`) is an unresolved consumer, as the builders of the request block are not read.
//! `request` sends the method its block assigns (`method = HttpMethod.X`, or `this.method`),
//! `GET` when it assigns none: a block that is no lambda written in the call, an assignment
//! that may not run (an `if`, a lambda inside the block) or a method that is no `HttpMethod`
//! member is an unknown method, and the consumer is unresolved. A `defaultRequest { url(base)
//! }` in the configuration of the client the call comes from is the base of its relative URLs.
//! A client wrapper is read at each of its call sites (`Code::expand`, through any number of
//! levels), the URL and the method it takes from its parameters substituted there.

use super::{external, implicit};
use crate::contract::Found;
use crate::jvm::names::{one, spellings, typed};
use crate::lang::{Bound, Call, Class, Part, Value};
use crate::qualify;
use crate::resolve::{Code, Expand, Expanded, Follow, Of, passes_nothing};

use super::super::{based, consumer, fetch};

/// The package of the request functions.
const REQUEST: &str = "io.ktor.client.request.";
/// Ktor's `HttpClient`.
const CLIENT: &str = "io.ktor.client.HttpClient";
/// The spellings of `defaultRequest`: Ktor 2 and 3, then Ktor 1.
const DEFAULT_REQUEST: &[&str] = &[
    "io.ktor.client.plugins.defaultRequest",
    "io.ktor.client.features.defaultRequest",
];
/// The request functions that name their method, and the one that takes it from its block.
const VERBS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];
const REQUEST_FUNCTION: &str = "request";
/// The key the pack records `method = value` under in `Call::kwargs`.
const METHOD: &str = "method=";

/// How many locals a client is followed through (`c.use { it.get(..) }` on a local `c`).
const LOCAL_DEPTH: usize = 8;

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
        self.reach(path, scope, receiver, 0)
    }

    fn reach(
        &self,
        path: &'a str,
        scope: &'a str,
        receiver: &'a Value,
        depth: usize,
    ) -> Option<Vec<Client<'a>>> {
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
                Some((p, i)) if made.iter().any(|s| s == CLIENT) || self.makes(p, i) => {
                    out.push(Client::Made(p, i));
                }
                _ => other = true,
            }
        }
        if out.is_empty() {
            let Value::Ref(name) = receiver else {
                return None;
            };
            let written = typed(self.code, path, scope, name);
            if written.iter().any(|s| s == CLIENT) {
                return Some(vec![Client::Typed]);
            }
            // A local with no written type holds what the pack recorded for it.
            let Bound { within, value, .. } = self.local(path, scope, name)?;
            return match depth < LOCAL_DEPTH {
                true => self.reach(path, within, value, depth + 1),
                false => None,
            };
        }
        if other {
            out.push(Client::Unknown);
        }
        Some(out)
    }

    /// Whether the call at `index` of `path` is Ktor's `HttpClient(..)` as its own imports spell
    /// it: the on-demand ones too, which the origin's import does not name.
    fn makes(&self, path: &'a str, index: usize) -> bool {
        let call = &self.code.files()[path].calls[index];
        let spelled = spellings(self.code, path, &call.scope, &call.callee);
        one(&spelled, |s| s == CLIENT).is_some()
    }

    /// The value the nearest binding of `name` around `scope` holds, when the pack recorded it
    /// and no other binding of the name is in the same scope (a name declared twice, a loop
    /// variable, a parameter, a constant, a property).
    fn local(&self, path: &'a str, scope: &'a str, name: &str) -> Option<&'a Bound> {
        let facts = self.code.files()[path];
        let mut at = Some(scope);
        while let Some(s) = at {
            let mut bound =
                (self.code.bound(path).iter()).filter(|b| b.scope == s && b.name == name);
            let held = bound.next();
            let qualified = qualify(s, name);
            let others = bound.next().is_some()
                || facts.locals.binary_search(&qualified).is_ok()
                || (facts.consts.iter()).any(|c| c.name == qualified)
                || (facts.defs.iter())
                    .any(|d| d.name == s && d.params.iter().any(|p| p.name == name))
                || (facts.receivers.iter())
                    .any(|r| r.scope == s && r.name == name && r.class == Class::Unknown);
            match (held, others) {
                (Some(found), false) => return Some(found),
                (None, false)
                    if !facts
                        .receivers
                        .iter()
                        .any(|r| r.scope == s && r.name == name) =>
                {
                    at = (!s.is_empty()).then(|| crate::parent(s));
                }
                _ => return None,
            }
        }
        None
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
}

/// The method a `request` call sends, from the `method=` pseudo-keyword the pack gives the
/// call whose block assigns it: `GET` when it assigns none, a `Ref` while it depends on a
/// parameter of a wrapper (read again at the wrapper's call sites), unknown when it is no
/// `HttpMethod` member or the call is given its block some other way (`given`).
fn requested(kwargs: &[(String, Value)], given: bool) -> Value {
    match kwargs.iter().find(|(k, _)| k == METHOD) {
        None if given => Value::Unknown,
        None => text("GET".into()),
        Some((_, Value::Ref(name))) => Value::Ref(name.clone()),
        Some((_, Value::External(spelled))) => external(spelled).map_or(Value::Unknown, text),
        Some(_) => Value::Unknown,
    }
}

fn text(method: String) -> Value {
    Value::Str(vec![Part::Text(method)])
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
            // The call whole, its names kept as the caller's imports spell them: an enum member
            // a wrapper takes keeps the evidence of the caller (`Expand::Chain`).
            let read = |args: &[Value], kwargs: &[(String, Value)]| {
                let [Value::Call(request)] = args else {
                    return None;
                };
                let first = (request.args.first()).filter(|v| !matches!(v, Value::Function(_)));
                let named = kwargs.iter().find(|(k, _)| k == "urlString");
                let url = (first.or(named.map(|(_, v)| v)))
                    .cloned()
                    .unwrap_or(Value::Unknown);
                // What else the call is given: a block that is no lambda written there.
                let rest = request.args.iter().skip(usize::from(first.is_some()));
                let given = rest.into_iter().any(|v| !matches!(v, Value::Function(_)))
                    || (kwargs.iter()).any(|(k, v)| {
                        k != "urlString" && k != METHOD && !matches!(v, Value::Function(_))
                    });
                let method = match member {
                    REQUEST_FUNCTION => requested(kwargs, given),
                    verb => text(verb.to_uppercase()),
                };
                fetch(&[based(base.as_deref(), &url), method])
            };
            for Expanded {
                site: (site, at),
                found: read,
                ..
            } in code.expand(Expand::Chain(path, n), &read)
            {
                found.push(consumer(site, at, read, None));
            }
        }
    }
    found
}
