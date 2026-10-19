//! Go providers (spec #25): `net/http`, gin, echo, chi and gorilla/mux. A router is the call
//! that makes it (`gin.Default()`, `echo.New()`, `chi.NewRouter()`, `http.NewServeMux()`) or a
//! group a method of it makes (`r.Group("/v1")`, `r.Route("/v1", fn)`,
//! `r.PathPrefix("/v1").Subrouter()`), followed through assignments, arguments and return values
//! (`Code::origins`); a route is a call of one of the router's methods on it. Where a route is
//! served follows the mounts: a group is mounted on the router it is made on under its prefix
//! (the router the function of a `Route` or `Group` receives is that group), a chi router on
//! the one it is given to (`Mount(prefix, router)`), a `ServeMux` on the mux it is given to
//! (`Handle(pattern, mux)`, where it keeps its own paths, served for the pattern's subtree and
//! method alone), and `Code::origins` reads every router that reaches the receiver, so a
//! function registering on the router it receives serves it under every prefix its callers give
//! it. A mount Meada does not read serves nothing: a router inside any other call
//! (`http.StripPrefix`, `echo.WrapHandler`, a middleware) is one.
//!
//! The routers are the rows of `ROUTERS`: a package, what makes its routers, the methods that
//! make a group or mount a router, and the methods that register a route. Another router package
//! is another row.

use super::rust::actix_path;
use super::{
    ANY, CallAt, Mounts, PARAM, PREFIX_DEPTH, PREFIX_WORK, handler_site, join, normalize_path,
    path_literal, prefixes, provider,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::contract::Found;
use crate::lang::{Call, Value, literal};
use crate::resolve::{Code, Target};

/// How a route method says which methods the route serves.
#[derive(Clone, Copy)]
enum Serves {
    /// The one method it is named for (`GET`).
    Method(&'static str),
    /// Every method (`Any`).
    Any,
    /// The method is the first argument, the path the second (`Handle(method, path, ..)`).
    Given,
    /// A Go 1.22 pattern, `[METHOD ][HOST]/[PATH]`: its method, every method without one.
    Pattern,
    /// Every method, or the ones a `.Methods(..)` chained on the call names (gorilla/mux).
    Chained,
}

/// How a method makes a group of routes under a prefix.
#[derive(Clone, Copy)]
enum Form {
    /// `Group(prefix, middleware..)`: gin and echo.
    Prefix,
    /// `Group(fn)`: chi, no prefix; `fn` receives the group.
    Callback,
    /// `Route(prefix, fn)`: chi; `fn` receives the group.
    Route,
    /// `PathPrefix(prefix).Subrouter()`: gorilla/mux; the method is the `Subrouter` call.
    Subrouter,
}

/// The spellings of a path parameter a router reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Params {
    /// `{name}`, `{name:regex}`, `{name...}` and `{$}` only: `net/http` and gorilla/mux, where
    /// `:name` and `*` are literal.
    Braces,
    /// Braces, and `*` for the rest of the path: chi.
    Star,
    /// Braces, `:name` and `*rest`: gin and echo.
    Colon,
}

/// Where a route method takes its handler.
#[derive(Clone, Copy)]
enum Handler {
    /// The last argument: gin's `GET(path, middleware.., handler)`.
    Last,
    /// The argument after the path: echo's `GET(path, handler, middleware..)`.
    Next,
}

/// A package whose routers serve routes.
struct Router {
    /// The import path of the package, without a major version.
    package: &'static str,
    /// The functions of the package that make a router.
    made: &'static [&'static str],
    /// The methods of a router that make a group under a prefix.
    groups: &'static [(&'static str, Form)],
    /// The method that mounts the router given as its second argument under its first (chi's
    /// `Mount`).
    mount: Option<&'static str>,
    /// The methods that register a route, with the methods each serves.
    routes: &'static [(&'static str, Serves)],
    handler: Handler,
    params: Params,
    /// Whether the package has a router of its own that its functions register on
    /// (`http.HandleFunc`), and the mux it is served under no prefix.
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

const ROUTERS: &[Router] = &[
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
        // echo has no `Handle`: `Add(method, path, handler)` is not read.
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

/// gorilla/mux restricts the methods of a route with `.Methods(..)` chained on the call that
/// registers it, after the calls that keep the route (`Name`, `Schemes`, a second `Methods`).
const RESTRICT: &str = "Methods";
const KEEPS_ROUTE: &[&str] = &["Name", "Schemes", "Headers", "Queries", "Host", RESTRICT];

/// Where routes are registered or served, a node of the mounts.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place<'a> {
    /// The call that makes a router or a group.
    Made(CallAt<'a>),
    /// The router of `net/http` that its package functions register on.
    Default,
}

/// A call registering a route, on the place it is registered on.
struct Registration<'a> {
    path: &'a str,
    n: usize,
    at: Place<'a>,
    member: &'a str,
}

/// `package` without a major version suffix (`github.com/labstack/echo/v4`).
fn unversioned(package: &str) -> &str {
    match package.rsplit_once('/') {
        Some((rest, major))
            if major
                .strip_prefix('v')
                .is_some_and(|n| n.parse::<u32>().is_ok_and(|n| n >= 2)) =>
        {
            rest
        }
        _ => package,
    }
}

/// The package and the name an outside function is called by (`net/http`, `HandleFunc`).
fn split(external: &str) -> Option<(&str, &str)> {
    external
        .rsplit_once('.')
        .map(|(package, name)| (unversioned(package), name))
}

/// `path` as the segments of a path: each `{name}`, `{name:regex}` and `{rest...}` a parameter,
/// the `{$}` that ends a pattern gone, and, as the router's `params` say, each `*rest` segment a
/// parameter (chi, gin, echo) and each `:name` one (gin, echo); `None` when a brace is not
/// closed, `{$}` is not the end, or a `:name` segment is a literal that no contract can spell.
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

/// A `net/http` pattern, `[METHOD ][HOST]/[PATH]`: the host is dropped.
#[derive(Clone)]
struct Pattern {
    /// `None` for a pattern without a method: every method.
    method: Option<String>,
    /// The path, normalized.
    path: String,
    /// Whether the path ends in `/` or `{name...}`: it holds everything below it.
    subtree: bool,
}

impl Pattern {
    /// Whether a route of a mux the pattern mounts, at `route` (normalized), is reached: a mux
    /// sees the whole path, so the route must fall under the pattern.
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

    /// The method a route of `method` is served for through the pattern: the pattern's own, when
    /// the route has none or the same.
    fn serves(&self, method: Option<&str>) -> Option<Option<String>> {
        match (&self.method, method) {
            (Some(m), Some(r)) if m != r => None,
            (Some(m), _) => Some(Some(m.clone())),
            (None, r) => Some(r.map(str::to_string)),
        }
    }
}

/// The pattern `pattern` of `net/http`, `None` when its method or its path cannot be read.
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

/// `net/http`, the router whose mux a mount is read for.
const STDLIB: &Router = &ROUTERS[0];

struct Go<'c, 'a> {
    code: &'c Code<'a>,
    /// The groups and the mounted routers of gin, echo, chi and gorilla/mux under their
    /// prefixes. A router with an empty list is mounted where Meada does not read.
    mounts: Mounts<'a, Place<'a>>,
    /// The `ServeMux`es given as a handler, with the router and the pattern each is given
    /// under. A mux with an empty list, or only unreadable patterns, is served nowhere.
    patterns: BTreeMap<Place<'a>, Vec<(Place<'a>, Option<Pattern>)>>,
    /// The groups whose function receives the group as its first parameter: chi's `Route(prefix,
    /// fn)` and `Group(fn)`, by the function (a literal or a named one).
    callbacks: BTreeMap<Target<'a>, Vec<Place<'a>>>,
    /// The methods `.Methods(..)` restricts the route registered by a call to; `None` when they
    /// cannot be read or a second `Methods` ANDs the first: the route serves nothing.
    restricted: BTreeMap<CallAt<'a>, Option<Vec<String>>>,
}

/// A call making a group of routes under a prefix.
struct Group<'a> {
    /// The value the group is made on.
    parent: &'a Value,
    /// The segment it adds, `None` when it cannot be read.
    prefix: Option<String>,
    /// The function that receives the group.
    callback: Option<Target<'a>>,
}

impl<'c, 'a> Go<'c, 'a> {
    /// What `callee` (`pkg.Name`) names in a package outside the repository: only when its head
    /// is the import itself, not a name typed as the package's class (`e := other()` with
    /// `e.Default()`), which the resolver would also read as that package's.
    fn external(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Option<String> {
        let (head, _) = callee.split_once('.').unwrap_or((callee, ""));
        self.code.external(path, scope, head)?;
        self.code.external(path, scope, callee)
    }

    /// Whether the call `at` is a function of `router` that makes a router.
    fn makes(&self, router: &Router, (path, n): CallAt<'a>) -> bool {
        let Some(call) = self.code.call(path, n) else {
            return false;
        };
        let external = self.external(path, &call.scope, &call.callee);
        match external.as_deref().and_then(split) {
            Some((package, name)) => package == router.package && router.made.contains(&name),
            None => false,
        }
    }

    /// The places the routers that `value`, written in `scope` of `path`, may be are served at:
    /// the ones it flows from, and the groups whose function receives it as its first parameter.
    fn places(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Vec<Place<'a>> {
        let origins = self.code.value_origins(path, scope, value);
        let mut places: Vec<Place> = origins.into_iter().map(Place::Made).collect();
        if let Value::Ref(name) = value {
            // The function itself or one it is written in (a closure captures the parameter).
            let mut within = scope;
            loop {
                let function = Target { path, name: within };
                if let Some(groups) = self.callbacks.get(&function)
                    && self.code.is_param(path, scope, name, function, 0)
                {
                    places.extend(groups);
                }
                if within.is_empty() {
                    break;
                }
                within = crate::parent(within);
            }
        }
        places
    }

    /// The routers of `router` that `value`, written in `scope` of `path`, may be.
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

    /// The `ServeMux`es `value`, written in `scope` of `path`, may be.
    fn muxes(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Vec<Place<'a>> {
        self.routers(path, scope, value, STDLIB)
    }

    /// The group the call `call` of `path` makes, when it is one.
    fn group(&self, path: &'a str, call: &'a Call) -> Option<Group<'a>> {
        let scope = call.scope.as_str();
        let member = call.callee.rsplit('.').next().unwrap_or_default();
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

    /// Mounts the `ServeMux`es that `value`, the handler of a call written in `scope` of `path`,
    /// holds on `parents` under `outer`. A mux inside a call (`http.StripPrefix`,
    /// `echo.WrapHandler`, a middleware) is mounted where Meada does not read (`unread`). Whether
    /// it found a mux.
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

    /// Mounts the routers of `router` that `value`, the handler of a call written in `scope` of
    /// `path`, may be on `parents` under `prefix` (`None`: it cannot be read), and the others it
    /// holds nowhere Meada reads (`unread`): chi strips the prefix, so a `ServeMux` mounted this
    /// way does not see its own paths.
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

    /// Marks the routers `value` holds as mounted where Meada cannot read: they are served
    /// nowhere. Whether it found one.
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

    /// The patterns a route registered on the mux `at` is reached through, one list for each way
    /// the mux is served: none when it is mounted nowhere Meada reads, or too deep.
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
        // A mux mounted inside itself, through any depth, adds nothing.
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

    /// The method `arg`, an argument written in `scope` of `path`, names: a string, or a
    /// constant of `net/http` (`http.MethodGet`), in upper case.
    fn method(&self, path: &'a str, scope: &'a str, arg: &'a Value) -> Option<String> {
        let method = match arg {
            Value::Ref(name) => {
                let external = self.external(path, scope, name)?;
                let (package, name) = split(&external)?;
                let name = name
                    .strip_prefix("Method")
                    .filter(|_| package == "net/http")?;
                name.to_uppercase()
            }
            arg => match self.code.value(path, scope, arg) {
                Value::Str(parts) => literal(&parts)?,
                _ => return None,
            },
        };
        upper(&method).then_some(method)
    }

    /// Reads the `.Methods(..)` call `n` of `path`: the route it restricts is the call it is
    /// chained on, through the calls that keep the route.
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
            let name = inner.callee.rsplit('.').next().unwrap_or_default();
            if chained(name) {
                // A second `Methods` on the route ANDs the first.
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

    /// The handler `value` stands for: the function a `http.HandlerFunc(f)` conversion holds,
    /// else the value itself.
    fn unwrapped(&self, path: &'a str, scope: &'a str, value: &'a Value) -> &'a Value {
        if let Value::Call(call) = value
            && let [inner] = &call.args[..]
        {
            let external = self.external(path, scope, &call.callee);
            if external.as_deref().and_then(split) == Some(("net/http", "HandlerFunc")) {
                return inner;
            }
        }
        value
    }

    /// The providers of one call registering a route on the router `at`.
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
            // A mux sees the whole path of the muxes it is given to; the others add prefixes.
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
            // The methods (`None`: every method), the path, and the position of the path.
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
            // A route whose path Meada cannot read serves nothing.
            let Some(route) = route else { continue };
            let handler = match router.handler {
                Handler::Last => call.args.last().filter(|_| call.args.len() > at + 1),
                Handler::Next => call.args.get(at + 1),
            };
            let Some(handler) = handler else { continue };
            let handler = self.unwrapped(path, scope, handler);
            // The method (`None`: every method) and the path of each way the route is served.
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

/// Go providers: see the module.
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
    // The functions that receive a group and the routes a `.Methods(..)` restricts are read
    // first: a call registering a route may come before them, or in another file.
    for (&path, facts) in code.files() {
        for (n, stored) in facts.calls.iter().enumerate() {
            let member = stored.callee.rsplit('.').next().unwrap_or_default();
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
            let member = stored.callee.rsplit('.').next().unwrap_or_default();
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
            // The values of the call: `files` holds none, only the facts the index keeps.
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
            // `http.HandleFunc(..)` registers on the router of the package itself; any other
            // call is a method of a router, whatever its head's type reads as.
            let external = go.external(path, &call.scope, &call.callee);
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
            // A mux given as the handler is mounted under the pattern, whatever router it is
            // given to; a router held by any other argument is an unread mount. What holds none
            // is a route.
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

/// Whether a route method of a router is restricted by a chained `.Methods(..)`.
fn chained(name: &str) -> bool {
    let mut routes = ROUTERS.iter().flat_map(|r| r.routes);
    routes.any(|(m, serves)| *m == name && matches!(serves, Serves::Chained))
}

/// Whether `method` is an HTTP method's spelling: upper-case letters.
fn upper(method: &str) -> bool {
    !method.is_empty() && method.bytes().all(|b| b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_has_each_parameter_spelling_as_one_segment() {
        // gin and echo: `{..}` as the pattern of `net/http`, `:name` and `*rest` too.
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
        // `*` is a literal there, and a `:name` segment one a contract cannot spell.
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
