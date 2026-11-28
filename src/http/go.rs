mod clients;

pub(super) use clients::consumers;

use super::rust::actix_path;
use super::{
    ANY, CallAt, Mounts, PARAM, PREFIX_DEPTH, PREFIX_WORK, handler_site, join, normalize_path,
    path_literal, prefixes, provider,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::contract::Found;
use crate::lang::go::{modfile::unversioned, package_call};
use crate::lang::{Call, Value, literal};
use crate::resolve::{Code, Follow, Of, Target, method as member};

#[derive(Clone, Copy)]
enum Serves {
    Method(&'static str),
    Any,
    Given,
    Pattern,
    Chained,
}

#[derive(Clone, Copy)]
enum Form {
    Prefix,
    Callback,
    Route,
    Subrouter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Params {
    Braces,
    Star,
    Colon,
}

#[derive(Clone, Copy)]
enum Handler {
    Last,
    Next,
}

struct Router {
    package: &'static str,
    made: &'static [&'static str],
    groups: &'static [(&'static str, Form)],
    mount: Option<&'static str>,
    routes: &'static [(&'static str, Serves)],
    handler: Handler,
    params: Params,
    default: bool,
}

const GIN_ROUTES: &[(&str, Serves)] = &[
    ("GET", Serves::Method("GET")),
    ("POST", Serves::Method("POST")),
    ("PUT", Serves::Method("PUT")),
    ("PATCH", Serves::Method("PATCH")),
    ("DELETE", Serves::Method("DELETE")),
    ("HEAD", Serves::Method("HEAD")),
    ("OPTIONS", Serves::Method("OPTIONS")),
    ("Any", Serves::Any),
    ("Handle", Serves::Given),
];

static ROUTERS: &[Router] = &[
    Router {
        package: "net/http",
        made: &["NewServeMux"],
        groups: &[],
        mount: None,
        routes: &[("Handle", Serves::Pattern), ("HandleFunc", Serves::Pattern)],
        handler: Handler::Next,
        params: Params::Braces,
        default: true,
    },
    Router {
        package: "github.com/gin-gonic/gin",
        made: &["Default", "New"],
        groups: &[("Group", Form::Prefix)],
        mount: None,
        routes: GIN_ROUTES,
        handler: Handler::Last,
        params: Params::Colon,
        default: false,
    },
    Router {
        package: "github.com/labstack/echo",
        made: &["New"],
        groups: &[("Group", Form::Prefix)],
        mount: None,
        routes: &[
            ("GET", Serves::Method("GET")),
            ("POST", Serves::Method("POST")),
            ("PUT", Serves::Method("PUT")),
            ("PATCH", Serves::Method("PATCH")),
            ("DELETE", Serves::Method("DELETE")),
            ("HEAD", Serves::Method("HEAD")),
            ("OPTIONS", Serves::Method("OPTIONS")),
            ("Any", Serves::Any),
        ],
        handler: Handler::Next,
        params: Params::Colon,
        default: false,
    },
    Router {
        package: "github.com/go-chi/chi",
        made: &["NewRouter", "NewMux"],
        groups: &[("Group", Form::Callback), ("Route", Form::Route)],
        mount: Some("Mount"),
        routes: &[
            ("Get", Serves::Method("GET")),
            ("Post", Serves::Method("POST")),
            ("Put", Serves::Method("PUT")),
            ("Patch", Serves::Method("PATCH")),
            ("Delete", Serves::Method("DELETE")),
            ("Head", Serves::Method("HEAD")),
            ("Options", Serves::Method("OPTIONS")),
            ("Connect", Serves::Method("CONNECT")),
            ("Trace", Serves::Method("TRACE")),
            ("Method", Serves::Given),
            ("MethodFunc", Serves::Given),
            ("Handle", Serves::Any),
            ("HandleFunc", Serves::Any),
        ],
        handler: Handler::Next,
        params: Params::Star,
        default: false,
    },
    Router {
        package: "github.com/gorilla/mux",
        made: &["NewRouter"],
        groups: &[("Subrouter", Form::Subrouter)],
        mount: None,
        routes: &[("Handle", Serves::Chained), ("HandleFunc", Serves::Chained)],
        handler: Handler::Next,
        params: Params::Braces,
        default: false,
    },
];

const RESTRICT: &str = "Methods";
const KEEPS_ROUTE: &[&str] = &["Name", "Schemes", "Headers", "Queries", "Host", RESTRICT];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place<'a> {
    Made(CallAt<'a>),
    Default,
}

struct Registration<'a> {
    path: &'a str,
    n: usize,
    at: Place<'a>,
    member: &'a str,
}

fn split(external: &str) -> Option<(String, &str)> {
    external
        .rsplit_once('.')
        .map(|(package, name)| (unversioned(package), name))
}

fn go_path(path: &str, params: Params) -> Option<String> {
    let (colon, star) = (params == Params::Colon, params != Params::Braces);
    let path = path
        .strip_suffix("{$}")
        .filter(|p| p.ends_with('/'))
        .unwrap_or(path);
    if path.contains("{$}") {
        return None;
    }
    let path = actix_path(path)?;
    // ponytail: a literal `:name` segment where `:` is no parameter (`net/http`, chi, gorilla)
    // would read as one in a contract (`normalize_path`), so the route yields no provider.
    if !colon && path.split('/').any(|s| s.starts_with(':')) {
        return None;
    }
    let segments = path
        .split('/')
        .map(|s| if star && s.starts_with('*') { PARAM } else { s });
    Some(segments.collect::<Vec<_>>().join("/"))
}

#[derive(Clone)]
struct Pattern {
    method: Option<String>,
    path: String,
    subtree: bool,
}

impl Pattern {
    fn holds(&self, route: &str) -> bool {
        let segments = |p: &str| -> Vec<String> {
            p.split('/')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };
        let (outer, inner) = (segments(&self.path), segments(route));
        let fits = outer.iter().zip(&inner).all(|(o, i)| o == i || o == PARAM);
        fits && match self.subtree {
            true => inner.len() >= outer.len(),
            false => inner.len() == outer.len(),
        }
    }

    fn serves(&self, method: Option<&str>) -> Option<Option<String>> {
        match (&self.method, method) {
            (Some(m), Some(r)) if m != r => None,
            (Some(m), _) => Some(Some(m.clone())),
            (None, r) => Some(r.map(str::to_string)),
        }
    }
}

fn pattern(pattern: &str) -> Option<Pattern> {
    let (method, rest) = match pattern.split_once([' ', '\t']) {
        Some((method, rest)) => (Some(method), rest.trim_start_matches([' ', '\t'])),
        None => (None, pattern),
    };
    if method.is_some_and(|m| !upper(m)) {
        return None;
    }
    let path = &rest[rest.find('/')?..];
    Some(Pattern {
        method: method.map(str::to_string),
        path: normalize_path(&go_path(path, Params::Braces)?),
        subtree: path.ends_with('/') && !path.ends_with("{$}") || path.ends_with("...}"),
    })
}

const STDLIB: &Router = &ROUTERS[0];

struct Go<'c, 'a> {
    code: &'c Code<'a>,
    mounts: Mounts<'a, Place<'a>>,
    patterns: BTreeMap<Place<'a>, Vec<(Place<'a>, Option<Pattern>)>>,
    callbacks: BTreeMap<Target<'a>, Vec<Place<'a>>>,
    restricted: BTreeMap<CallAt<'a>, Option<Vec<String>>>,
}

struct Group<'a> {
    parent: &'a Value,
    prefix: Option<String>,
    callback: Option<Target<'a>>,
}

impl<'c, 'a> Go<'c, 'a> {
    fn makes(&self, router: &Router, (path, n): CallAt<'a>) -> bool {
        let Some(call) = self.code.call(path, n) else {
            return false;
        };
        let external = package_call(self.code, path, &call.scope, &call.callee);
        match external.as_deref().and_then(split) {
            Some((package, name)) => package == router.package && router.made.contains(&name),
            None => false,
        }
    }

    fn places(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Vec<Place<'a>> {
        let origins = self.code.origins(
            path,
            scope,
            Of::Value(value),
            Follow::Values {
                passes: crate::resolve::passes_nothing,
            },
        );
        let mut places: Vec<Place> = origins
            .into_iter()
            .filter_map(|o| o.call)
            .map(Place::Made)
            .collect();
        let (mut groups, mut seen) = (Vec::new(), Vec::new());
        if self.reach(path, scope, value, 0, &mut seen, &mut groups) {
            places.extend(groups);
        }
        places.sort();
        places.dedup();
        places
    }

    // ponytail: a group passed on through more than `PREFIX_DEPTH` exact calls and names, or
    // reached through more than `PREFIX_WORK` parameters, is served nowhere rather than under a
    // part of its callers.
    fn reach(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        depth: usize,
        seen: &mut Vec<(Target<'a>, usize)>,
        out: &mut Vec<Place<'a>>,
    ) -> bool {
        let Value::Ref(name) = value else {
            return true;
        };
        if depth >= PREFIX_DEPTH || seen.len() >= PREFIX_WORK {
            return false;
        }
        if let Some(((at, constant), held)) = self.code.constant(path, scope, name) {
            return match held {
                Value::Ref(_) => {
                    self.reach(at, crate::parent(constant), held, depth + 1, seen, out)
                }
                _ => true,
            };
        }
        let Some((function, n)) = self.code.unchanged_param_of(path, scope, name) else {
            return true;
        };
        if let Some(groups) = self.callbacks.get(&function)
            && n == 0
        {
            out.extend(groups);
        }
        if seen.contains(&(function, n)) {
            return true;
        }
        seen.push((function, n));
        let args = self.code.param_args(function, n);
        let mut read = true;
        for (path, scope, arg) in args {
            read &= self.reach(path, scope, arg, depth + 1, seen, out);
        }
        read
    }

    fn routers(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        router: &Router,
    ) -> Vec<Place<'a>> {
        let places = self.places(path, scope, value).into_iter();
        let made = places.filter(|p| matches!(p, Place::Made(at) if self.makes(router, *at)));
        made.collect()
    }

    fn muxes(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Vec<Place<'a>> {
        self.routers(path, scope, value, STDLIB)
    }

    fn group(&self, path: &'a str, call: &'a Call) -> Option<Group<'a>> {
        let scope = call.scope.as_str();
        let member = member(&call.callee);
        let function = |value: &'a Value| match value {
            Value::Function(name) => Some(Target { path, name }),
            Value::Ref(name) => self.code.definition(path, scope, name),
            _ => None,
        };
        let read = |value: Option<&'a Value>, params| {
            let written = value.and_then(|p| path_literal(self.code.value(path, scope, p)));
            written.and_then(|p| go_path(&p, params))
        };
        for router in ROUTERS {
            let prefix = |value| read(value, router.params);
            for &(name, form) in router.groups {
                if name != member {
                    continue;
                }
                let args = call.args.as_slice();
                let receiver = call.receiver.as_ref();
                let (parent, prefix, callback) = match (form, args) {
                    (Form::Prefix, [first, ..]) if function(first).is_none() => {
                        (receiver?, prefix(Some(first)), None)
                    }
                    (Form::Callback, [callback]) if function(callback).is_some() => {
                        (receiver?, Some(String::new()), function(callback))
                    }
                    (Form::Route, [first, callback]) if function(callback).is_some() => {
                        (receiver?, prefix(Some(first)), function(callback))
                    }
                    (Form::Subrouter, []) => match receiver? {
                        Value::Call(inner) if inner.callee.ends_with(".PathPrefix") => {
                            (inner.receiver.as_deref()?, prefix(inner.args.first()), None)
                        }
                        _ => continue,
                    },
                    _ => continue,
                };
                return Some(Group {
                    parent,
                    prefix,
                    callback,
                });
            }
        }
        None
    }

    fn mount(
        &mut self,
        at: (&'a str, &'a str),
        value: &'a Value,
        parents: &[Place<'a>],
        outer: Option<Pattern>,
    ) -> bool {
        let muxes = self.muxes(at.0, at.1, value);
        for mux in &muxes {
            let mounted = self.patterns.entry(*mux).or_default();
            mounted.extend(parents.iter().map(|&p| (p, outer.clone())));
        }
        !muxes.is_empty() || self.unread(at, value)
    }

    fn mount_router(
        &mut self,
        at: (&'a str, &'a str),
        value: &'a Value,
        parents: &[Place<'a>],
        prefix: Option<String>,
        router: &Router,
    ) {
        for sub in self.routers(at.0, at.1, value, router) {
            let mounted = self.mounts.entry(sub).or_default();
            mounted.extend(parents.iter().map(|&p| (p, prefix.clone())));
        }
        self.unread(at, value);
    }

    fn unread(&mut self, (path, scope): (&'a str, &'a str), value: &'a Value) -> bool {
        let mut found = false;
        for place in self.places(path, scope, value) {
            let Place::Made(at) = place else { continue };
            let Some(router) = ROUTERS.iter().find(|r| self.makes(r, at)) else {
                continue;
            };
            found = true;
            if std::ptr::eq(router, STDLIB) {
                self.patterns.entry(place).or_default();
            } else {
                self.mounts.entry(place).or_default();
            }
        }
        if let Value::Call(call) = value {
            for arg in &call.args {
                found |= self.unread((path, scope), arg);
            }
        }
        found
    }

    fn chains(
        &self,
        at: Place<'a>,
        seen: &mut Vec<Place<'a>>,
        work: &mut usize,
    ) -> Option<Vec<Vec<&Pattern>>> {
        let Place::Made(made) = at else {
            return Some(vec![Vec::new()]);
        };
        if !self.makes(STDLIB, made) {
            return Some(Vec::new());
        }
        let Some(parents) = self.patterns.get(&at) else {
            return Some(vec![Vec::new()]);
        };
        if seen.contains(&at) {
            return Some(Vec::new());
        }
        if seen.len() >= PREFIX_DEPTH {
            return None;
        }
        seen.push(at);
        let mut out = Vec::new();
        for (parent, outer) in parents {
            *work = work.checked_sub(1)?;
            let Some(outer) = outer else { continue };
            for mut chain in self.chains(*parent, seen, work)? {
                chain.push(outer);
                out.push(chain);
            }
        }
        seen.pop();
        Some(out)
    }

    fn method(&self, path: &'a str, scope: &'a str, arg: &'a Value) -> Option<String> {
        let method = match arg {
            Value::Ref(name) => {
                let external = package_call(self.code, path, scope, name)?;
                let (package, name) = split(&external)?;
                let name = name
                    .strip_prefix("Method")
                    .filter(|_| package == "net/http")?;
                name.to_uppercase()
            }
            arg => match self.code.value(path, scope, arg) {
                Value::Str(parts) => literal(&parts)?.to_uppercase(),
                _ => return None,
            },
        };
        upper(&method).then_some(method)
    }

    fn restrict(&mut self, path: &'a str, n: usize) {
        let Some(call) = self.code.call(path, n) else {
            return;
        };
        let scope = call.scope.as_str();
        let methods = (call.args.iter())
            .map(|arg| self.method(path, scope, arg))
            .collect::<Option<Vec<_>>>()
            .filter(|methods| !methods.is_empty());
        let mut value = call.receiver.as_ref();
        while let Some(Value::Call(inner)) = value {
            let Some(at) = self.code.call_index(path, &inner.range) else {
                return;
            };
            let name = member(&inner.callee);
            if chained(name) {
                let first = !self.restricted.contains_key(&(path, at));
                let methods = methods.filter(|_| first);
                self.restricted.insert((path, at), methods);
                return;
            }
            if !KEEPS_ROUTE.contains(&name) {
                return;
            }
            value = inner.receiver.as_deref();
        }
    }

    fn unwrapped(&self, path: &'a str, scope: &'a str, value: &'a Value) -> &'a Value {
        if let Value::Call(call) = value
            && let [inner] = &call.args[..]
        {
            let external = package_call(self.code, path, scope, &call.callee);
            let split = external.as_deref().and_then(split);
            if split.is_some_and(|(package, name)| package == "net/http" && name == "HandlerFunc") {
                return inner;
            }
        }
        value
    }

    fn provide(
        &self,
        Registration {
            path,
            n,
            at,
            member,
        }: Registration<'a>,
        out: &mut Vec<Found>,
    ) {
        let code = self.code;
        let Some(call) = code.call(path, n) else {
            return;
        };
        let scope = call.scope.as_str();
        let read = |i: usize| {
            let arg = call.args.get(i)?;
            path_literal(code.value(path, scope, arg))
        };
        for router in ROUTERS {
            let Some(&(_, serves)) = router.routes.iter().find(|(name, _)| *name == member) else {
                continue;
            };
            let root = |place: Place<'a>| match place {
                Place::Default => router.default.then(String::new),
                Place::Made(at) => self.makes(router, at).then(String::new),
            };
            let (mut work, stdlib) = (PREFIX_WORK, std::ptr::eq(router, STDLIB));
            let chains = match stdlib {
                true => self
                    .chains(at, &mut Vec::new(), &mut work)
                    .unwrap_or_default(),
                false => Vec::new(),
            };
            let prefixes = match stdlib {
                true => Vec::new(),
                false => prefixes(at, &self.mounts, &root, &mut Vec::new()),
            };
            if prefixes.is_empty() && chains.is_empty() {
                continue;
            }
            let (methods, route, at) = match serves {
                Serves::Method(m) => (
                    vec![Some(m.to_string())],
                    read(0).and_then(|p| go_path(&p, router.params)),
                    0,
                ),
                Serves::Any => (
                    vec![None],
                    read(0).and_then(|p| go_path(&p, router.params)),
                    0,
                ),
                Serves::Given => {
                    match call.args.first().and_then(|a| self.method(path, scope, a)) {
                        Some(m) => (
                            vec![Some(m)],
                            read(1).and_then(|p| go_path(&p, router.params)),
                            1,
                        ),
                        None => continue,
                    }
                }
                Serves::Pattern => match read(0).and_then(|p| pattern(&p)) {
                    Some(p) => (vec![p.method], Some(p.path), 0),
                    None => continue,
                },
                Serves::Chained => {
                    let methods = match self.restricted.get(&(path, n)) {
                        None => vec![None],
                        Some(Some(methods)) => methods.iter().cloned().map(Some).collect(),
                        Some(None) => continue,
                    };
                    (methods, read(0).and_then(|p| go_path(&p, router.params)), 0)
                }
            };
            let Some(route) = route else { continue };
            let handler = match router.handler {
                Handler::Last => call.args.last().filter(|_| call.args.len() > at + 1),
                Handler::Next => call.args.get(at + 1),
            };
            let Some(handler) = handler else { continue };
            let handler = self.unwrapped(path, scope, handler);
            let route = normalize_path(&route);
            let mut served = BTreeSet::new();
            for method in &methods {
                for chain in &chains {
                    let method = (chain.iter()).try_fold(method.clone(), |m, outer| {
                        outer
                            .holds(&route)
                            .then(|| outer.serves(m.as_deref()))
                            .flatten()
                    });
                    served.extend(method.map(|m| (m, route.clone())));
                }
                for prefix in &prefixes {
                    served.insert((method.clone(), normalize_path(&join(prefix, &route))));
                }
            }
            for (method, full) in served {
                let method = method.unwrap_or_else(|| ANY.to_string());
                let site = handler_site(code, path, scope, Some(handler), &method, &full);
                out.push(provider(site, format!("http::{method}::{full}")));
            }
        }
    }
}

pub(super) fn providers<'a>(code: &Code<'a>) -> Vec<Found> {
    if !code.pack().extensions().contains(&"go") {
        return Vec::new();
    }
    let mut go = Go {
        code,
        mounts: Mounts::new(),
        patterns: BTreeMap::new(),
        callbacks: BTreeMap::new(),
        restricted: BTreeMap::new(),
    };
    for (&path, facts) in code.files() {
        for (n, stored) in facts.calls.iter().enumerate() {
            let member = member(&stored.callee);
            let groups = ROUTERS.iter().flat_map(|r| r.groups);
            if member == RESTRICT {
                go.restrict(path, n);
            } else if groups.clone().any(|(m, _)| *m == member)
                && let Some(call) = code.call(path, n)
                && let Some(callback) = go.group(path, call).and_then(|g| g.callback)
            {
                go.callbacks
                    .entry(callback)
                    .or_default()
                    .push(Place::Made((path, n)));
            }
        }
    }
    let mut registrations = Vec::new();
    for (&path, facts) in code.files() {
        for (n, stored) in facts.calls.iter().enumerate() {
            let member = member(&stored.callee);
            let groups = ROUTERS
                .iter()
                .any(|r| r.groups.iter().any(|(m, _)| *m == member));
            let mounting = ROUTERS.iter().find(|r| r.mount == Some(member));
            let routes = ROUTERS
                .iter()
                .any(|r| r.routes.iter().any(|(m, _)| *m == member));
            if !(groups || routes || mounting.is_some()) {
                continue;
            }
            let Some(call) = code.call(path, n) else {
                continue;
            };
            if groups && let Some(group) = go.group(path, call) {
                let parents = go.places(path, &call.scope, group.parent);
                let mounted = go.mounts.entry(Place::Made((path, n))).or_default();
                mounted.extend(parents.iter().map(|&p| (p, group.prefix.clone())));
            }
            if !(routes || mounting.is_some()) {
                continue;
            }
            let external = package_call(go.code, path, &call.scope, &call.callee);
            if external
                .as_deref()
                .and_then(split)
                .is_some_and(|(package, name)| clients::is_package_send(&package, name))
            {
                continue;
            }
            let package_call =
                external
                    .as_deref()
                    .and_then(split)
                    .is_some_and(|(package, name)| {
                        let own = ROUTERS.iter().filter(|r| r.default && r.package == package);
                        own.flat_map(|r| r.routes).any(|(m, _)| *m == name)
                    });
            let parents = match (package_call, &call.receiver) {
                (true, _) => vec![Place::Default],
                (false, Some(receiver)) => go.places(path, &call.scope, receiver),
                (false, None) => continue,
            };
            let at = (path, call.scope.as_str());
            if let Some(row) = mounting
                && let [prefix, handler] = &call.args[..]
            {
                let prefix = path_literal(code.value(path, at.1, prefix));
                let prefix = prefix.and_then(|p| go_path(&p, row.params));
                go.mount_router(at, handler, &parents, prefix, row);
                continue;
            }
            if !routes {
                continue;
            }
            let mut mounts = false;
            for (i, arg) in call.args.iter().enumerate() {
                if member == "Handle" && call.args.len() == 2 && i == 1 {
                    let outer = path_literal(code.value(path, at.1, &call.args[0]));
                    mounts = go.mount(at, arg, &parents, outer.and_then(|p| pattern(&p)));
                } else {
                    go.unread(at, arg);
                }
            }
            if !mounts {
                let at = parents.iter().copied();
                registrations.extend(at.map(|at| Registration {
                    path,
                    n,
                    at,
                    member,
                }));
            }
        }
    }
    let mut found = Vec::new();
    for registration in registrations {
        go.provide(registration, &mut found);
    }
    found
}

fn chained(name: &str) -> bool {
    let mut routes = ROUTERS.iter().flat_map(|r| r.routes);
    routes.any(|(m, serves)| *m == name && matches!(serves, Serves::Chained))
}

fn upper(method: &str) -> bool {
    !method.is_empty() && method.bytes().all(|b| b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_has_each_parameter_spelling_as_one_segment() {
        let cases = [
            ("/users/{id}", "/users/{}"),
            ("/users/{id:[0-9]+}", "/users/{}"),
            ("/users/{id:[a-z/]+}/x", "/users/{}/x"),
            ("/files/{path...}", "/files/{}"),
            ("/users/:id", "/users/{}"),
            ("/files/*rest", "/files/{}"),
            ("/static/*", "/static/{}"),
            ("/static/", "/static"),
            ("/{$}", "/"),
            ("/posts/{$}", "/posts"),
            ("", "/"),
        ];
        for (written, normalized) in cases {
            let path = go_path(written, Params::Colon).map(|p| normalize_path(&p));
            assert_eq!(path.as_deref(), Some(normalized), "{written}");
        }
        for unreadable in ["/{id", "/id}", "/{a{b}", "/a/{$}/b", "{$}"] {
            assert_eq!(go_path(unreadable, Params::Colon), None, "{unreadable}");
        }
    }

    #[test]
    fn net_http_reads_only_braces_as_parameters() {
        let read = |written: &str| go_path(written, Params::Braces).map(|p| normalize_path(&p));
        assert_eq!(read("/users/{id}").as_deref(), Some("/users/{}"));
        assert_eq!(read("/files/{path...}").as_deref(), Some("/files/{}"));
        assert_eq!(read("/static/*").as_deref(), Some("/static/*"));
        assert_eq!(read("/users/:id"), None);
    }

    #[test]
    fn chi_reads_braces_and_star_but_not_a_colon() {
        let read = |written: &str| go_path(written, Params::Star).map(|p| normalize_path(&p));
        assert_eq!(read("/users/{id:[0-9]+}").as_deref(), Some("/users/{}"));
        assert_eq!(read("/static/*").as_deref(), Some("/static/{}"));
        assert_eq!(read("/users/:id"), None);
    }

    #[test]
    fn a_pattern_has_a_method_and_loses_its_host() {
        let cases = [
            ("/users", None, "/users", false),
            ("GET /users/{id}", Some("GET"), "/users/{}", false),
            ("POST   /users", Some("POST"), "/users", false),
            ("example.com/users", None, "/users", false),
            ("DELETE example.com/users/", Some("DELETE"), "/users", true),
            ("GET /{$}", Some("GET"), "/", false),
            ("/static/", None, "/static", true),
            ("/files/{path...}", None, "/files/{}", true),
        ];
        for (written, method, path, subtree) in cases {
            let read = pattern(written).map(|p| (p.method, p.path, p.subtree));
            let expected = (method.map(str::to_string), path.to_string(), subtree);
            assert_eq!(read, Some(expected), "{written}");
        }
        for unreadable in ["get /users", "GET users", "GET /{id", "example.com", ""] {
            assert!(pattern(unreadable).is_none(), "{unreadable}");
        }
    }

    #[test]
    fn a_mux_mounted_by_a_pattern_serves_the_routes_under_it_for_its_method() {
        let outer = |written: &str| pattern(written).unwrap();
        let tree = outer("/sub/");
        assert!(tree.holds("/sub") && tree.holds("/sub/a") && tree.holds("/sub/a/b"));
        assert!(!tree.holds("/a2") && !tree.holds("/subtle"));
        let exact = outer("/exact");
        assert!(exact.holds("/exact") && !exact.holds("/exact/deeper"));
        assert!(outer("/u/{id}/").holds("/u/5/x"));
        assert!(!outer("/u/5/").holds("/u/{}/x"));
        assert!(outer("/").holds("/anything/at/all"));
        let get = outer("GET /static/");
        assert_eq!(get.serves(None), Some(Some("GET".to_string())));
        assert_eq!(get.serves(Some("GET")), Some(Some("GET".to_string())));
        assert_eq!(get.serves(Some("POST")), None);
        assert_eq!(tree.serves(None), Some(None));
        assert_eq!(tree.serves(Some("POST")), Some(Some("POST".to_string())));
    }
}
