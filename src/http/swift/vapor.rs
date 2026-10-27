//! Vapor providers (spec #29, #119): `get`, `post`, `put`, `patch`, `delete` and `on(.METHOD, ..)`
//! called on the application or on a route builder serve their path components (variadic
//! strings, or slash-separated ones) under every prefix of the `grouped(..)` calls around them,
//! at any depth. Vapor's closure form `group("api") { }` is not read.
//!
//! A builder is followed through the shared value flow (`Code::origins` with `Follow::Values`),
//! as the minimal APIs of C# are (`http/csharp/minimal.rs`, whose design this follows): locals,
//! function arguments and return values. Each origin is a node of the mounts: a `grouped` call
//! is mounted on the origins of its receiver under its components, and the application
//! (`Application(..)`, `Application.make(..)`) is the root; any other origin is a mount Meada
//! does not read. A builder one function returns keeps the calls the walk climbed to reach it
//! (`Route::through`), so two groups one factory makes from two builders keep their own
//! prefixes, and a builder used under two prefixes serves each route under both.
//!
//! Vapor calls `boot(routes:)` of a `RouteCollection`, so no call of the repository binds its
//! parameter: `register(collection: C())` mounts the `routes` parameter of `C.boot(routes:)` on
//! the origins of the builder it is called on (`Node::Boot`), with no segment, and a route
//! written on `routes` there is served under every builder the collection is registered on.
//!
//! A closure handler is an inline handler named for its route, and a function given as `use:`
//! is that function (`handler_site`). A method of the repository named like one of the
//! framework's (`get`, `grouped`) hides it where a call reaches it. A `grouped` argument that
//! constructs a value (`grouped(AuthMiddleware())`) is a middleware and adds no segment, unless
//! it constructs a path (`PATHS`); one else that is no string literal serves nothing.

use std::collections::{BTreeMap, BTreeSet};

use super::normalized;
use crate::contract::{Found, Site};
use crate::http::{
    CallAt, METHODS, Mounts, contract, handler_site, join, path_literal, prefixes, provider,
};
use crate::lang::{Call, Value};
use crate::names::{one, spellings};
use crate::resolve::{Code, Of, Target, passes_nothing};

/// The route methods of one HTTP method, with the method each serves.
const VERBS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
];
const ON: &str = "on";
const REGISTER: &str = "register";
const SELF: &str = "self.";
/// The function Vapor calls to have a `RouteCollection` register its routes.
const BOOT: &str = "boot(routes:)";

/// What differs between the frameworks that share this router value flow: Vapor
/// (`VAPOR`) and Hummingbird (`hummingbird::HUMMINGBIRD`).
pub(super) struct Dialect {
    /// What makes the root builder, as the file's imports spell it.
    pub roots: &'static [&'static str],
    /// The method that makes a child builder, mounted under its path components.
    pub group: &'static str,
    /// Whether `on` takes its method first (`on(.GET, "p")`) or after the path
    /// (`on("p", method: .get)`).
    pub method_first: bool,
    /// Whether `register(collection:)` mounts a `RouteCollection`.
    pub collections: bool,
}

pub(super) const VAPOR: Dialect = Dialect {
    roots: &["Vapor.Application", "Vapor.Application.make"],
    group: "grouped",
    method_first: true,
    collections: true,
};

/// An origin of a builder: the call that made it and the calls the walk climbed through to
/// reach it, outermost first (`Route::through`), or the `routes` parameter of a collection's
/// `boot(routes:)`, by its file and name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Node<'a> {
    Call(CallAt<'a>, Vec<CallAt<'a>>),
    Boot(&'a str, &'a str),
}

/// The method a callee calls: `get` of `app.get`.
fn method(callee: &str) -> &str {
    callee.rsplit('.').next().unwrap_or(callee)
}

/// The types whose construction is a path component, not a middleware.
const PATHS: [&str; 3] = ["String", "Substring", "PathComponent"];

/// How many nodes the mounts of one repository may hold.
// ponytail: real apps make tens of groups; past this (a crafted web of factories) nothing is
// served, rather than routes under part of their prefixes.
const VAPOR_NODES: usize = 4096;

struct Vapor<'c, 'a> {
    code: &'c Code<'a>,
    dialect: &'c Dialect,
    /// Whether a node was dropped past `VAPOR_NODES`.
    overflow: bool,
    /// The nodes whose origins went round a factory that calls itself (`origins`): what is
    /// mounted above them is not known, so a route reaching one is served nowhere.
    cycles: BTreeSet<usize>,
    /// Each node by its place in `nodes`.
    ids: BTreeMap<Node<'a>, usize>,
    nodes: Vec<Node<'a>>,
    /// The nodes not yet read.
    pending: Vec<usize>,
    roots: BTreeSet<usize>,
    mounts: Mounts<'a, usize>,
    /// The builders each collection's `boot(routes:)` is registered on, by the method.
    registered: BTreeMap<(&'a str, &'a str), Vec<Node<'a>>>,
}

impl<'c, 'a> Vapor<'c, 'a> {
    /// Whether the call at `index` of `path` is the framework's: no function of the repository
    /// answers it.
    fn framework(&self, path: &'a str, index: usize) -> bool {
        self.code.reaches(path, index).is_none()
    }

    /// The place of `node` in `nodes`, read later when it is new; `None` past `VAPOR_NODES`.
    fn intern(&mut self, node: Node<'a>) -> Option<usize> {
        if let Some(&id) = self.ids.get(&node) {
            return Some(id);
        }
        if self.nodes.len() >= VAPOR_NODES {
            self.overflow = true;
            return None;
        }
        let id = self.nodes.len();
        self.ids.insert(node.clone(), id);
        self.nodes.push(node);
        self.pending.push(id);
        Some(id)
    }

    /// The origins of `value`, written in `scope` of `path` inside the functions the calls of
    /// `context` reach, which bind their parameters (`Code::origins_within`); `None` when one
    /// climbs a call twice, round a factory that calls itself.
    fn origins(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        context: &[CallAt<'a>],
    ) -> Option<Vec<Node<'a>>> {
        let mut out = BTreeSet::new();
        let (found, unbound) =
            (self.code).origins_unbound(path, scope, Of::Value(value), passes_nothing, context);
        // Vapor calls `boot(routes:)`, so no call binds its parameter: a builder that comes from
        // it, through arguments and aliases, is the collection's `routes`.
        for (function, at) in unbound {
            if at == 0 && function.name.ends_with(&format!(".{BOOT}")) {
                out.insert(Node::Boot(function.path, function.name));
            }
        }
        for origin in found {
            let Some(call) = origin.call else {
                continue;
            };
            for route in origin.routes {
                let distinct: BTreeSet<&CallAt<'a>> = route.through.iter().collect();
                if distinct.len() < route.through.len() {
                    return None;
                }
                out.insert(Node::Call(call, route.through));
            }
        }
        Some(out.into_iter().collect())
    }

    /// Whether the call at `index` of `path` makes an application, as the file's imports spell
    /// it.
    fn application(&self, path: &'a str, index: usize) -> bool {
        let call = &self.code.files()[path].calls[index];
        let spelled = spellings(self.code, path, &call.scope, &call.callee);
        one(&spelled, |s| self.dialect.roots.contains(&s)).is_some()
    }

    /// The path components `call` is given, joined with `/`: its string arguments, a value that
    /// constructs one a middleware (`middleware`) adding none. `None` when one cannot be read.
    fn components(&self, path: &'a str, call: &'a Call, args: &[&'a Value]) -> Option<String> {
        let mut out = Vec::new();
        for &arg in args {
            let middleware = matches!(arg, Value::Call(inv)
                if method(&inv.callee).starts_with(char::is_uppercase)
                    && !PATHS.contains(&method(&inv.callee)));
            if middleware && call.callee.ends_with(self.dialect.group) {
                continue;
            }
            out.push(path_literal(self.code.value(path, &call.scope, arg))?);
        }
        Some(out.join("/"))
    }

    /// Reads the node `id`: a `grouped` call is mounted on the origins of its receiver under its
    /// components, the `routes` of a collection's `boot` on the builders it is registered on,
    /// the application is a root, and anything else is left unread.
    fn read(&mut self, id: usize) {
        let code = self.code;
        let (call_at, context) = match self.nodes[id].clone() {
            Node::Boot(path, name) => {
                let parents = self
                    .registered
                    .get(&(path, name))
                    .cloned()
                    .unwrap_or_default();
                let parents = (parents.into_iter())
                    .filter_map(|node| Some((self.intern(node)?, Some(String::new()))))
                    .collect();
                self.mounts.insert(id, parents);
                return;
            }
            Node::Call(call_at, context) => (call_at, context),
        };
        let (path, index) = call_at;
        let Some(call) = code.call(path, index) else {
            return;
        };
        if self.application(path, index) {
            self.roots.insert(id);
            return;
        }
        let Some(receiver) = call.receiver.as_ref() else {
            return;
        };
        if method(&call.callee) != self.dialect.group || !self.framework(path, index) {
            return;
        }
        let args: Vec<&Value> = call.args.iter().collect();
        let prefix = match call.kwargs.is_empty() {
            true => self.components(path, call, &args),
            false => None,
        };
        let Some(parents) = self.origins(path, &call.scope, receiver, &context) else {
            self.cycles.insert(id);
            return;
        };
        let parents: Vec<(usize, Option<String>)> = (parents.into_iter())
            .filter_map(|node| Some((self.intern(node)?, prefix.clone())))
            .collect();
        self.mounts.insert(id, parents);
    }

    /// Whether a node mounted under `nodes`, at any depth, went round a factory that calls
    /// itself (`cycles`).
    fn round(&self, nodes: &[usize]) -> bool {
        let (mut seen, mut work) = (BTreeSet::new(), nodes.to_vec());
        while let Some(id) = work.pop() {
            if !seen.insert(id) {
                continue;
            }
            if self.cycles.contains(&id) {
                return true;
            }
            let parents = self.mounts.get(&id).into_iter().flatten();
            work.extend(parents.map(|(parent, _)| *parent));
        }
        false
    }

    /// The method `value` names, upper-case: `.GET` or `HTTPMethod.GET`.
    fn http_method(value: &Value) -> Option<String> {
        let name = match value {
            Value::Ref(name) => method(name),
            Value::Member(ty, name) if ty.is_empty() => name,
            _ => return None,
        };
        let method = name.to_uppercase();
        METHODS.contains(&method.as_str()).then_some(method)
    }

    /// The collection class a `register(collection:)` call is given: the type constructed in
    /// the argument, or in the local it names.
    fn collection(&self, path: &'a str, call: &'a Call) -> Option<Target<'a>> {
        let arg = call.keyword("collection").or_else(|| call.args.first())?;
        let (scope, arg) = match arg {
            Value::Ref(name) => {
                let bound = self.code.local(path, &call.scope, name)?;
                (bound.within.as_str(), &bound.value)
            }
            other => (call.scope.as_str(), other),
        };
        let Value::Call(constructed) = arg else {
            return None;
        };
        self.code.method(path, scope, &constructed.callee, BOOT)
    }
}

/// A route a call registers: its call, its pattern, its methods and its handler.
struct Route<'a> {
    path: &'a str,
    call: &'a Call,
    pattern: String,
    method: String,
    handler: Option<&'a Value>,
    nodes: Vec<usize>,
}

/// Vapor providers: see the module.
pub(super) fn providers(code: &Code, dialect: &Dialect) -> Vec<Found> {
    let files: Vec<_> = (code.files().iter())
        .filter(|(p, _)| p.ends_with(".swift"))
        .map(|(p, f)| (*p, *f))
        .collect();
    let mut vapor = Vapor {
        code,
        dialect,
        overflow: false,
        cycles: BTreeSet::new(),
        ids: BTreeMap::new(),
        nodes: Vec::new(),
        pending: Vec::new(),
        roots: BTreeSet::new(),
        mounts: Mounts::new(),
        registered: BTreeMap::new(),
    };
    let mut routes = Vec::new();
    for &(path, facts) in &files {
        for (n, call) in facts.calls.iter().enumerate() {
            let name = method(&call.callee);
            let verb = VERBS.iter().find(|(m, _)| *m == name).map(|(_, v)| *v);
            let known = verb.is_some() || (name == ON || (dialect.collections && name == REGISTER));
            if !known || !vapor.framework(path, n) {
                continue;
            }
            // The index keeps no argument: they come from the full facts of the file.
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let Some(receiver) = call.receiver.as_ref() else {
                continue;
            };
            let Some(nodes) = vapor.origins(path, &call.scope, receiver, &[]) else {
                continue;
            };
            if name == REGISTER {
                if let Some(collection) = vapor.collection(path, call) {
                    (vapor.registered.entry((collection.path, collection.name)))
                        .or_default()
                        .extend(nodes);
                }
                continue;
            }
            let positional: Vec<&Value> = (call.args.iter())
                .filter(|v| !matches!(v, Value::Function(_)))
                .collect();
            let (method, components) = match (verb, dialect.method_first) {
                (Some(verb), _) => (Some(verb.to_string()), &positional[..]),
                (None, true) => match positional.split_first() {
                    Some((first, rest)) => (Vapor::http_method(first), rest),
                    None => (None, &positional[..]),
                },
                (None, false) => (
                    call.keyword("method").and_then(Vapor::http_method),
                    &positional[..],
                ),
            };
            let trailing = call.args.last().filter(|v| matches!(v, Value::Function(_)));
            let handler = call.keyword("use").or(trailing);
            let pattern = vapor.components(path, call, components);
            let (Some(method), Some(pattern)) = (method, pattern) else {
                continue;
            };
            let nodes = nodes.into_iter().filter_map(|n| vapor.intern(n)).collect();
            routes.push(Route {
                path,
                call,
                pattern,
                method,
                handler,
                nodes,
            });
        }
    }
    while let Some(id) = vapor.pending.pop() {
        vapor.read(id);
    }
    if vapor.overflow {
        return Vec::new();
    }
    let root = |id: usize| vapor.roots.contains(&id).then(String::new);
    let mut found = Vec::new();
    for route in &routes {
        if vapor.round(&route.nodes) {
            continue;
        }
        let mut served = BTreeSet::new();
        for &node in &route.nodes {
            served.extend(prefixes(node, &vapor.mounts, &root, &mut Vec::new()));
        }
        for prefix in served {
            let full = normalized(&join(&prefix, &route.pattern));
            let scope = &route.call.scope;
            let site = match route.handler {
                // `use: self.show` is `show` of the type around the route.
                Some(Value::Ref(name)) if name.starts_with(SELF) => {
                    match code.definition(route.path, scope, &name[SELF.len()..]) {
                        Some(t) => Site::Def {
                            path: t.path.into(),
                            name: t.name.into(),
                        },
                        None => Site::File(route.path.into()),
                    }
                }
                handler => handler_site(code, route.path, scope, handler, &route.method, &full),
            };
            found.push(provider(site, contract(&route.method, &full)));
        }
    }
    found
}
