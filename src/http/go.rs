//! Go providers (spec #25): `net/http`, gin and echo. A router is the call that makes it
//! (`gin.Default()`, `echo.New()`, `http.NewServeMux()`) or a group a method of it makes
//! (`r.Group("/v1")`), followed through assignments, arguments and return values
//! (`Code::origins`); a route is a call of one of the router's methods on it. Where a route is
//! served follows the mounts: a group is mounted on the router it is made on under its prefix, a
//! `ServeMux` on the mux it is given to (`Handle(pattern, mux)`, where it keeps its own paths,
//! served for the pattern's subtree and method alone), and `Code::origins` reads every router
//! that reaches the receiver, so a function registering on the router it receives serves it
//! under every prefix its callers give it. A mount Meada does not read serves nothing: a mux
//! inside any other call (`http.StripPrefix`, `echo.WrapHandler`, a middleware) is one.
//!
//! The routers are the rows of `ROUTERS`: a package, what makes its routers, the method that
//! makes a group, and the methods that register a route. Another router package is another row.

mod clients;

pub(super) use clients::consumers;

use super::rust::actix_path;
use super::{
    ANY, CallAt, Mounts, PARAM, PREFIX_DEPTH, PREFIX_WORK, handler_site, join, normalize_path,
    path_literal, prefixes, provider,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::contract::Found;
use crate::lang::{Call, Value, literal};
use crate::resolve::Code;

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
    /// The method of a router that makes a group under a prefix.
    group: Option<&'static str>,
    /// The methods that register a route, with the methods each serves.
    routes: &'static [(&'static str, Serves)],
    handler: Handler,
    /// Whether `:name` and `*rest` segments are parameters (gin, echo). In `net/http` they are
    /// literal, and only `{name}`, `{name...}` and `{$}` are not.
    colon: bool,
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
        group: None,
        routes: &[("Handle", Serves::Pattern), ("HandleFunc", Serves::Pattern)],
        handler: Handler::Next,
        colon: false,
        default: true,
    },
    Router {
        package: "github.com/gin-gonic/gin",
        made: &["Default", "New"],
        group: Some("Group"),
        routes: GIN_ROUTES,
        handler: Handler::Last,
        colon: true,
        default: false,
    },
    Router {
        package: "github.com/labstack/echo",
        made: &["New"],
        group: Some("Group"),
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
        colon: true,
        default: false,
    },
];

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

/// What `callee` (`pkg.Name`) names in a package outside the repository: only when its head is
/// the import itself, not a name typed as the package's class (`e := other()` with
/// `e.Default()`), which the resolver would also read as that package's.
fn external<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, callee: &'a str) -> Option<String> {
    let (head, _) = callee.split_once('.').unwrap_or((callee, ""));
    code.external(path, scope, head)?;
    code.external(path, scope, callee)
}

/// The package and the name an outside function is called by (`net/http`, `HandleFunc`).
fn split(external: &str) -> Option<(&str, &str)> {
    external
        .rsplit_once('.')
        .map(|(package, name)| (unversioned(package), name))
}

/// `path` as the segments of a path: each `{name}`, `{name:regex}` and `{rest...}` a parameter,
/// the `{$}` that ends a pattern gone, and, when `colon` (gin and echo), each `*rest` segment a
/// parameter and each `:name` one; `None` when a brace is not closed, `{$}` is not the end, or a
/// `:name` segment of `net/http` is a literal that no contract can spell.
fn go_path(path: &str, colon: bool) -> Option<String> {
    let path = path
        .strip_suffix("{$}")
        .filter(|p| p.ends_with('/'))
        .unwrap_or(path);
    if path.contains("{$}") {
        return None;
    }
    let path = actix_path(path)?;
    // ponytail: a literal `:name` segment of `net/http` would read as a parameter in a contract
    // (`normalize_path`), so the route yields no provider.
    if !colon && path.split('/').any(|s| s.starts_with(':')) {
        return None;
    }
    let segments = path.split('/').map(|s| {
        if colon && s.starts_with('*') {
            PARAM
        } else {
            s
        }
    });
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
        path: normalize_path(&go_path(path, false)?),
        subtree: path.ends_with('/') && !path.ends_with("{$}") || path.ends_with("...}"),
    })
}

/// `net/http`, the router whose mux a mount is read for.
const STDLIB: &Router = &ROUTERS[0];

struct Go<'c, 'a> {
    code: &'c Code<'a>,
    /// The groups of gin and echo under their prefixes.
    mounts: Mounts<'a, Place<'a>>,
    /// The `ServeMux`es given as a handler, with the router and the pattern each is given
    /// under. A mux with an empty list, or only unreadable patterns, is served nowhere.
    patterns: BTreeMap<Place<'a>, Vec<(Place<'a>, Option<Pattern>)>>,
}

impl<'c, 'a> Go<'c, 'a> {
    /// What `callee` (`pkg.Name`) names in a package outside the repository: only when its head
    /// is the import itself, not a name typed as the package's class (`e := other()` with
    /// `e.Default()`), which the resolver would also read as that package's.
    fn external(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Option<String> {
        external(self.code, path, scope, callee)
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

    /// The places the routers that `value`, written in `scope` of `path`, may be are served at.
    fn places(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Vec<Place<'a>> {
        let origins = self.code.value_origins(path, scope, value);
        origins.into_iter().map(Place::Made).collect()
    }

    /// The `ServeMux`es `value`, written in `scope` of `path`, may be.
    fn muxes(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Vec<Place<'a>> {
        let places = self.places(path, scope, value).into_iter();
        let muxes = places.filter(|p| matches!(p, Place::Made(at) if self.makes(STDLIB, *at)));
        muxes.collect()
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

    /// Marks the `ServeMux`es `value` holds as mounted where Meada cannot read: they are served
    /// nowhere. Whether it found a mux.
    fn unread(&mut self, (path, scope): (&'a str, &'a str), value: &'a Value) -> bool {
        let muxes = self.muxes(path, scope, value);
        for mux in &muxes {
            self.patterns.entry(*mux).or_default();
        }
        let mut found = !muxes.is_empty();
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

    /// The method a `Handle(method, ..)` call names: a string, or a constant of `net/http`
    /// (`http.MethodGet`), in upper case.
    fn method(&self, path: &'a str, call: &'a Call) -> Option<String> {
        let method = match call.args.first()? {
            Value::Ref(name) => {
                let external = self.external(path, &call.scope, name)?;
                let (package, name) = split(&external)?;
                let name = name
                    .strip_prefix("Method")
                    .filter(|_| package == "net/http")?;
                name.to_uppercase()
            }
            arg => match self.code.value(path, &call.scope, arg) {
                Value::Str(parts) => literal(&parts)?,
                _ => return None,
            },
        };
        upper(&method).then_some(method)
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
            // The method (`None`: every method), the path, and the position of the path.
            let (method, route, at) = match serves {
                Serves::Method(m) => (
                    Some(m.to_string()),
                    read(0).and_then(|p| go_path(&p, router.colon)),
                    0,
                ),
                Serves::Any => (None, read(0).and_then(|p| go_path(&p, router.colon)), 0),
                Serves::Given => match self.method(path, call) {
                    Some(m) => (Some(m), read(1).and_then(|p| go_path(&p, router.colon)), 1),
                    None => continue,
                },
                Serves::Pattern => match read(0).and_then(|p| pattern(&p)) {
                    Some(p) => (p.method, Some(p.path), 0),
                    None => continue,
                },
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
            for chain in &chains {
                let method = (chain.iter()).try_fold(method.clone(), |m, outer| {
                    outer
                        .holds(&route)
                        .then(|| outer.serves(m.as_deref()))
                        .flatten()
                });
                served.extend(method.map(|m| (m, route.clone())));
            }
            for prefix in prefixes {
                served.insert((method.clone(), normalize_path(&join(&prefix, &route))));
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
    };
    let mut registrations = Vec::new();
    for (&path, facts) in code.files() {
        for (n, stored) in facts.calls.iter().enumerate() {
            let member = stored.callee.rsplit('.').next().unwrap_or_default();
            let groups = ROUTERS.iter().any(|r| r.group == Some(member));
            let routes = ROUTERS
                .iter()
                .any(|r| r.routes.iter().any(|(m, _)| *m == member));
            if !(groups || routes) {
                continue;
            }
            // The values of the call: `files` holds none, only the facts the index keeps.
            let Some(call) = code.call(path, n) else {
                continue;
            };
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
            if groups && !package_call {
                let prefix = (call.args.first())
                    .and_then(|p| path_literal(code.value(path, &call.scope, p)))
                    .and_then(|p| go_path(&p, true));
                let mounted = go.mounts.entry(Place::Made((path, n))).or_default();
                mounted.extend(parents.iter().map(|&p| (p, prefix.clone())));
            }
            if !routes {
                continue;
            }
            // A mux given as the handler is mounted under the pattern, whatever router it is
            // given to; a mux held by any other argument is an unread mount. What holds no mux
            // is a route.
            let at = (path, call.scope.as_str());
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
            let path = go_path(written, true).map(|p| normalize_path(&p));
            assert_eq!(path.as_deref(), Some(normalized), "{written}");
        }
        for unreadable in ["/{id", "/id}", "/{a{b}", "/a/{$}/b", "{$}"] {
            assert_eq!(go_path(unreadable, true), None, "{unreadable}");
        }
    }

    #[test]
    fn net_http_reads_only_braces_as_parameters() {
        let read = |written: &str| go_path(written, false).map(|p| normalize_path(&p));
        assert_eq!(read("/users/{id}").as_deref(), Some("/users/{}"));
        assert_eq!(read("/files/{path...}").as_deref(), Some("/files/{}"));
        // `*` is a literal there, and a `:name` segment one a contract cannot spell.
        assert_eq!(read("/static/*").as_deref(), Some("/static/*"));
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
