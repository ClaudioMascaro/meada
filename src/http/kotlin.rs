//! Ktor server providers (spec #27, #98); the Ktor client's consumers are in `client` (#99). A routing scope is the lambda of a `routing { }` or a
//! `route(prefix) { }` call, or the body of an extension function on Ktor's `Route` or `Routing`
//! (`fun Route.orders()`). A `route` block is mounted on the routing scope around it under its
//! prefix, and such an extension function called there with the implicit receiver is mounted
//! with no segment, through the same mounts and prefixes as the other frameworks (`prefixes`):
//! one called under two prefixes serves under each, and one no routing scope calls serves
//! nowhere. `get(path) { }` and the other methods (a path-less one serves its block's path), and
//! `route(path, HttpMethod.X) { }`, register a route whose handler is the lambda
//! (`handler_site`); the path and the method are read positionally or by name. A routing scope
//! is reached from a call through the lambda arguments around it, but not through a handler or a
//! local function. An extension on `Routing` applies only where the receiver is one.
//!
//! A Ktor name counts only when `Code::outside` says an import from Ktor binds it; a function of
//! the repository with the same name hides it. Type-safe routing (`get<Resource> { }`, from
//! `io.ktor.server.resources`) is not read, and a name its star import may bind is no route.

use std::collections::{BTreeMap, BTreeSet, HashMap};

mod client;
pub(super) use client::consumers;

use super::rust::actix_path;
use super::{METHODS, Mounts, contract, handler_site, join, normalize_path, path_literal};
use super::{prefixes, provider};
use crate::contract::Found;
use crate::jvm::names::{kotlin, one, spellings};
use crate::lang::{Call, Facts, Kind, Value};
use crate::resolve::Code;

/// The packages of Ktor's routing functions and types: Ktor 2 and 3, then Ktor 1.
const ROUTING: &[&str] = &["io.ktor.server.routing.", "io.ktor.routing."];
/// The package of type-safe routing, whose `get` and others a star import may bind too.
const RESOURCES: &str = "io.ktor.server.resources.";
const HTTP_METHOD: &str = "io.ktor.http.HttpMethod";
/// Ktor's functions of one method, by their lower-case method: a server route's, and a client
/// request's (`kotlin/client.rs`).
const VERBS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

/// A definition (file, qualified name): a routing scope, a node of the mounts.
type Scope<'a> = (&'a str, &'a str);

/// The type of a routing scope's implicit receiver: an extension on `Routing` applies in a
/// `routing { }` block or another one on `Routing`, never in a `route` block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Receiver {
    Route,
    Routing,
}

/// The calls of each scope of a file, by start, each with the furthest end of the calls that
/// start before it or with it (whether a call of the scope holds a range is one search), and
/// the range of each lambda.
#[derive(Default)]
struct Spans<'a> {
    calls: HashMap<&'a str, Vec<(usize, usize)>>,
    lambdas: HashMap<&'a str, std::ops::Range<usize>>,
}

/// What a Ktor call is given: its path, its method and its lambda, the path and the method
/// written positionally or by name. `None` for any other named argument or one more.
fn given(call: &Call) -> Option<(Option<&Value>, Option<&Value>, &str)> {
    let (Some(Value::Function(lambda)), rest) = (call.args.last(), call.args.split_last()) else {
        return None;
    };
    let named = |key: &str| call.kwargs.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    // A key ending in `=` is an assignment in the lambda (`Call::kwargs`), no argument.
    if (call.kwargs.iter()).any(|(k, _)| k != "path" && k != "method" && !k.ends_with('=')) {
        return None;
    }
    let mut rest = rest.map_or(&[][..], |(_, rest)| rest).iter();
    let path = rest.next().or(named("path"));
    let method = rest.next().or(named("method"));
    rest.next().is_none().then_some((path, method, lambda))
}

/// A route a method call or `route(path, HttpMethod.X)` registers in a routing scope.
struct Route<'a> {
    path: &'a str,
    call: &'a Call,
    parent: Scope<'a>,
    route: String,
    method: String,
}

/// The name a call reaches through the implicit receiver: `f()`, `this.f()`, or `f()` in a
/// lambda whose receiver no syntax names (`(anonymous).f`).
fn implicit(callee: &str) -> Option<&str> {
    let name = (callee.strip_prefix("(anonymous)."))
        .or_else(|| callee.strip_prefix("this."))
        .unwrap_or(callee);
    (!name.contains('.')).then_some(name)
}

/// Whether `name`, written in `scope` of `path`, is Ktor's routing `name`.
fn ktor<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> bool {
    let spelled = spellings(code, path, scope, name);
    !spelled.iter().any(|s| s.starts_with(RESOURCES))
        && one(&spelled, |s| {
            ROUTING.iter().any(|p| s.strip_prefix(p) == Some(name))
        })
        .is_some()
}

/// Whether the definition `def` is a lambda (`@<offset>`).
fn is_lambda(def: &str) -> bool {
    let parent = crate::parent(def);
    def[parent.len()..].trim_start_matches('.').starts_with('@')
}

/// The method `HttpMethod.X`, written in `scope` of `path`, names, upper-case, when `HttpMethod`
/// is Ktor's.
fn method<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, value: &'a Value) -> Option<String> {
    let Value::Ref(name) = value else {
        return None;
    };
    let (head, member) = name.rsplit_once('.')?;
    one(&spellings(code, path, scope, head), |s| s == HTTP_METHOD)?;
    let method = member.to_uppercase();
    METHODS.contains(&method.as_str()).then_some(method)
}

/// The method `HttpMethod.X` names, upper-case, when one of the `spelled` names of an outside
/// value (`Value::External`) is Ktor's.
fn external(spelled: &[String]) -> Option<String> {
    let known = |s: &str| {
        s.strip_prefix(HTTP_METHOD)
            .is_some_and(|m| m.starts_with('.'))
    };
    let method = one(spelled, known)?[HTTP_METHOD.len() + 1..].to_uppercase();
    METHODS.contains(&method.as_str()).then_some(method)
}

struct Ktor<'c, 'a> {
    code: &'c Code<'a>,
    /// The lambdas of `routing` calls: served at the root.
    roots: BTreeSet<Scope<'a>>,
    /// The lambdas of `routing` and `route` calls.
    blocks: BTreeSet<Scope<'a>>,
    /// The handler lambdas of routes, where no routing scope is.
    handlers: BTreeSet<Scope<'a>>,
    /// The scopes that hold a `route` call with no lambda Meada reads (one nested past the
    /// pack's definition depth): what they register may be inside it, under a prefix unread.
    unread: BTreeSet<Scope<'a>>,
    /// The Ktor type each extension function asked about extends, `Route` or `Routing`.
    routers: BTreeMap<Scope<'a>, Option<Receiver>>,
    /// The call spans of each file asked about.
    spans: HashMap<&'a str, Spans<'a>>,
    /// The routing scope each scope asked about registers in (`scope_of`).
    scopes: BTreeMap<Scope<'a>, Option<Scope<'a>>>,
    mounts: Mounts<'a, Scope<'a>>,
    routes: Vec<Route<'a>>,
}

impl<'c, 'a> Ktor<'c, 'a> {
    /// The Ktor type `def` of `path` extends, when it is an extension function on `Route` or
    /// `Routing`.
    fn router(&mut self, path: &'a str, def: &'a str) -> Option<Receiver> {
        let code = self.code;
        *self.routers.entry((path, def)).or_insert_with(|| {
            let facts: &Facts = code.files()[path];
            let block = crate::parent(def);
            let e = facts.extensions.iter().find(|e| e.scope == block)?;
            let spelled = spellings(code, path, &e.within, &e.ty);
            let ty = |s: &str| ROUTING.iter().any(|p| s.strip_prefix(p) == Some(&e.ty));
            match one(&spelled, ty)? {
                s if s.ends_with(".Routing") => Some(Receiver::Routing),
                s if s.ends_with(".Route") => Some(Receiver::Route),
                _ => None,
            }
        })
    }

    /// The implicit receiver of a routing scope.
    fn receiver(&mut self, (path, def): Scope<'a>) -> Option<Receiver> {
        match self.roots.contains(&(path, def)) {
            true => Some(Receiver::Routing),
            false if self.blocks.contains(&(path, def)) => Some(Receiver::Route),
            false => self.router(path, def),
        }
    }

    /// Whether the lambda `def` of `path` is an argument of a call of the scope around it, not
    /// the body of a local function.
    fn argument(&mut self, path: &'a str, def: &'a str) -> bool {
        let code = self.code;
        let facts: &'a Facts = code.files()[path];
        let spans = self.spans.entry(path).or_insert_with(|| {
            let mut spans = Spans::default();
            for call in &facts.calls {
                let span = (call.range.start, call.range.end);
                spans
                    .calls
                    .entry(call.scope.as_str())
                    .or_default()
                    .push(span);
            }
            for def in facts.defs.iter().filter(|d| d.kind == Kind::Inline) {
                spans.lambdas.insert(def.name.as_str(), def.range.clone());
            }
            for calls in spans.calls.values_mut() {
                calls.sort();
                let mut furthest = 0;
                for (_, end) in calls.iter_mut() {
                    furthest = furthest.max(*end);
                    *end = furthest;
                }
            }
            spans
        });
        let Some(range) = spans.lambdas.get(def) else {
            return false;
        };
        let calls = (spans.calls.get(crate::parent(def))).map_or(&[][..], Vec::as_slice);
        let before = calls.partition_point(|&(start, _)| start <= range.start);
        before > 0 && calls[before - 1].1 >= range.end
    }

    /// The routing scope a call written in `scope` of `path` registers in: the nearest lambda of
    /// a `routing` or `route` call or routing extension function around it, through lambdas
    /// alone, a handler's excluded.
    fn scope_of(&mut self, path: &'a str, scope: &'a str) -> Option<Scope<'a>> {
        if self.unread.contains(&(path, scope)) {
            return None;
        }
        // Each scope is walked once: many calls share one, deep in nested lambdas.
        let (mut at, mut walked) = (scope, Vec::new());
        let found = loop {
            if let Some(&found) = self.scopes.get(&(path, at)) {
                break found;
            }
            walked.push(at);
            if self.blocks.contains(&(path, at))
                || !is_lambda(at) && self.router(path, at).is_some()
            {
                break Some((path, at));
            }
            if !is_lambda(at) || self.handlers.contains(&(path, at)) || !self.argument(path, at) {
                break None;
            }
            at = crate::parent(at);
        };
        for at in walked {
            self.scopes.insert((path, at), found);
        }
        found
    }

    /// The method `route(path, HttpMethod.X)` names, upper-case.
    fn method(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Option<String> {
        method(self.code, path, scope, value)
    }

    /// The path a call's argument `value` gives, a Ktor parameter (`{id}`, `{id?}`, `{...}`)
    /// as one.
    fn path(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Option<String> {
        path_literal(self.code.value(path, scope, value)).and_then(|p| actix_path(&p))
    }

    fn read(&mut self) {
        let code = self.code;
        // The Ktor calls, with their name, and the other calls through the implicit receiver.
        let (mut ktor_calls, mut others) = (Vec::new(), Vec::new());
        for (&path, facts) in code.files() {
            if !kotlin(path) {
                continue;
            }
            for (n, call) in facts.calls.iter().enumerate() {
                let Some(name) = implicit(&call.callee).filter(|_| call.decorates.is_none()) else {
                    continue;
                };
                let known = name == "routing" || name == "route" || VERBS.contains(&name);
                if !(known && ktor(code, path, &call.scope, name)) {
                    others.push((path, call, name));
                    continue;
                }
                // The index keeps no argument: they come from the full facts of the file.
                let Some(call) = code.call(path, n) else {
                    continue;
                };
                let Some((route, method, lambda)) = given(call) else {
                    if name == "route" && !matches!(call.args.last(), Some(Value::Function(_))) {
                        self.unread.insert((path, &call.scope));
                    }
                    continue;
                };
                match (name, route, method) {
                    ("routing", None, None) => {
                        self.roots.insert((path, lambda));
                        self.blocks.insert((path, lambda));
                    }
                    ("route", Some(_), None) => {
                        self.blocks.insert((path, lambda));
                    }
                    ("route", Some(_), Some(_)) => {
                        self.handlers.insert((path, lambda));
                    }
                    (verb, _, None) if VERBS.contains(&verb) => {
                        self.handlers.insert((path, lambda));
                    }
                    _ => continue,
                }
                ktor_calls.push((path, call, name, route, method, lambda));
            }
        }
        for (path, call, name, route, method, lambda) in ktor_calls {
            let scope = call.scope.as_str();
            let Some(parent) = self.scope_of(path, scope) else {
                continue;
            };
            let (route, method) = match (name, route, method) {
                ("routing", ..) => continue,
                ("route", Some(prefix), None) => {
                    let segment = self.path(path, scope, prefix);
                    let mounted = self.mounts.entry((path, lambda)).or_default();
                    mounted.push((parent, segment));
                    continue;
                }
                ("route", Some(route), Some(method)) => (
                    self.path(path, scope, route),
                    self.method(path, scope, method),
                ),
                (verb, None, _) => (Some(String::new()), Some(verb.to_uppercase())),
                (verb, Some(route), _) => {
                    (self.path(path, scope, route), Some(verb.to_uppercase()))
                }
            };
            let (Some(route), Some(method)) = (route, method) else {
                continue;
            };
            self.routes.push(Route {
                path,
                call,
                parent,
                route,
                method,
            });
        }
        // An extension function on `Route` or `Routing` called in a routing scope: the nearest
        // ones in scope that extend one are mounted there.
        for (path, call, name) in others {
            let Some(parent) = self.scope_of(path, &call.scope) else {
                continue;
            };
            let found = code.extensions(path, &call.scope, name, call.arity);
            // An extension on `Routing` does not apply to the `Route` of a `route` block.
            let here = self.receiver(parent);
            let found: Vec<_> = (found.into_iter())
                .filter(|(_, t)| match self.router(t.path, t.name) {
                    Some(Receiver::Routing) => here == Some(Receiver::Routing),
                    applies => applies.is_some(),
                })
                .collect();
            let Some(nearest) = found.iter().map(|f| f.0).min() else {
                continue;
            };
            for (_, t) in found.into_iter().filter(|f| f.0 == nearest) {
                let mounted = self.mounts.entry((t.path, t.name)).or_default();
                mounted.push((parent, Some(String::new())));
            }
        }
    }

    fn found(&self) -> Vec<Found> {
        let root = |scope: Scope<'a>| self.roots.contains(&scope).then(String::new);
        let mut found = Vec::new();
        for route in &self.routes {
            for prefix in prefixes(route.parent, &self.mounts, &root, &mut Vec::new()) {
                let full = normalize_path(&join(&prefix, &route.route));
                let (call, method) = (route.call, &route.method);
                let site = handler_site(
                    self.code,
                    route.path,
                    &call.scope,
                    call.args.last(),
                    method,
                    &full,
                );
                found.push(provider(site, contract(method, &full)));
            }
        }
        found
    }
}

/// Ktor server providers: see the module.
pub(super) fn providers(code: &Code) -> Vec<Found> {
    let mut ktor = Ktor {
        code,
        roots: BTreeSet::new(),
        blocks: BTreeSet::new(),
        handlers: BTreeSet::new(),
        unread: BTreeSet::new(),
        routers: BTreeMap::new(),
        spans: HashMap::new(),
        scopes: BTreeMap::new(),
        mounts: Mounts::new(),
        routes: Vec::new(),
    };
    ktor.read();
    ktor.found()
}
