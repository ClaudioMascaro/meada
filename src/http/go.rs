//! Go providers (spec #25): `net/http`, gin and echo. A router is the call that makes it
//! (`gin.Default()`, `echo.New()`, `http.NewServeMux()`) or a group a method of it makes
//! (`r.Group("/v1")`), followed through assignments, arguments and return values
//! (`Code::origins`); a route is a call of one of the router's methods on it. Where a route is
//! served follows the mounts: a group is mounted on the router it is made on under its prefix, a
//! `ServeMux` on the mux it is given to (`Handle(pattern, mux)`, or `StripPrefix(prefix, mux)`
//! for a prefix), and `Code::origins` reads every router that reaches the receiver, so a
//! function registering on the router it receives serves it under every prefix its callers
//! give it. A mount Meada does not read serves nothing.
//!
//! The routers are the rows of `ROUTERS`: a package, what makes its routers, the method that
//! makes a group, and the methods that register a route. Another router package is another row.

use super::rust::actix_path;
use super::{
    ANY, CallAt, Mounts, PARAM, handler_site, join, normalize_path, path_literal, prefixes,
    provider,
};
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
        default: true,
    },
    Router {
        package: "github.com/gin-gonic/gin",
        made: &["Default", "New"],
        group: Some("Group"),
        routes: GIN_ROUTES,
        handler: Handler::Last,
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

/// The package and the name an outside function is called by (`net/http`, `HandleFunc`).
fn split(external: &str) -> Option<(&str, &str)> {
    external
        .rsplit_once('.')
        .map(|(package, name)| (unversioned(package), name))
}

/// `path` as the segments of a path: each `{name}`, `{name:regex}` and `{rest...}` a parameter,
/// the `{$}` that ends a pattern gone, and each `*rest` segment of gin and echo a parameter;
/// `None` when a brace is not closed or `{$}` is not the end.
fn go_path(path: &str) -> Option<String> {
    let path = path
        .strip_suffix("{$}")
        .filter(|p| p.ends_with('/'))
        .unwrap_or(path);
    if path.contains("{$}") {
        return None;
    }
    let path = actix_path(path)?;
    let segments = path
        .split('/')
        .map(|s| if s.starts_with('*') { PARAM } else { s });
    Some(segments.collect::<Vec<_>>().join("/"))
}

/// The method and the path of a `net/http` pattern, `[METHOD ][HOST]/[PATH]`: the host is
/// dropped, and the method is `None` for a pattern without one.
fn pattern(pattern: &str) -> Option<(Option<String>, String)> {
    let (method, rest) = match pattern.split_once([' ', '\t']) {
        Some((method, rest)) => (Some(method), rest.trim_start_matches([' ', '\t'])),
        None => (None, pattern),
    };
    if method.is_some_and(|m| !upper(m)) {
        return None;
    }
    let path = &rest[rest.find('/')?..];
    Some((method.map(str::to_string), go_path(path)?))
}

/// `net/http`, the router whose mux a mount is read for.
const STDLIB: &Router = &ROUTERS[0];

struct Go<'c, 'a> {
    code: &'c Code<'a>,
    mounts: Mounts<'a, Place<'a>>,
}

impl<'c, 'a> Go<'c, 'a> {
    /// Whether the call `at` is a function of `router` that makes a router.
    fn makes(&self, router: &Router, (path, n): CallAt<'a>) -> bool {
        let Some(call) = self.code.call(path, n) else {
            return false;
        };
        let external = self.code.external_of(path, call);
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

    /// Mounts the `ServeMux`es that `value`, the handler of a call written in `scope` of
    /// `path`, holds on `parents`: a mux itself is served under `prefix`, one inside
    /// `StripPrefix(p, ..)` under `p` more. A mux inside any other call is mounted where Meada
    /// cannot read: it has no parent, so it is served nowhere. Whether it found a mux.
    fn mount(
        &mut self,
        (path, scope): (&'a str, &'a str),
        value: &'a Value,
        parents: &[Place<'a>],
        prefix: Option<String>,
    ) -> bool {
        if let Value::Call(call) = value {
            let stripped = self.code.external(path, scope, &call.callee);
            let stripped = stripped.as_deref().and_then(split) == Some(("net/http", "StripPrefix"));
            if let (true, [given, inner]) = (stripped, &call.args[..]) {
                let given = path_literal(self.code.value(path, scope, given));
                let prefix = match (prefix, given.and_then(|p| go_path(&p))) {
                    (Some(above), Some(given)) => Some(join(&above, &given)),
                    _ => None,
                };
                return self.mount((path, scope), inner, parents, prefix);
            }
            let mut found = false;
            for arg in &call.args {
                found |= self.mount((path, scope), arg, &[], None);
            }
            return found;
        }
        let places = self.places(path, scope, value).into_iter();
        let muxes: Vec<Place> = places
            .filter(|p| matches!(p, Place::Made(at) if self.makes(STDLIB, *at)))
            .collect();
        for mux in &muxes {
            let mounted = self.mounts.entry(*mux).or_default();
            mounted.extend(parents.iter().map(|&p| (p, prefix.clone())));
        }
        !muxes.is_empty()
    }

    /// The method a `Handle(method, ..)` call names: a string, or a constant of `net/http`
    /// (`http.MethodGet`), in upper case.
    fn method(&self, path: &'a str, call: &'a Call) -> Option<String> {
        let method = match call.args.first()? {
            Value::Ref(name) => {
                let external = self.code.external(path, &call.scope, name)?;
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
            let external = self.code.external(path, scope, &call.callee);
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
            let prefixes = prefixes(at, &self.mounts, &root, &mut Vec::new());
            if prefixes.is_empty() {
                continue;
            }
            // The method (`None`: every method), the path, and the position of the path.
            let (method, route, at) = match serves {
                Serves::Method(m) => (Some(m.to_string()), read(0).and_then(|p| go_path(&p)), 0),
                Serves::Any => (None, read(0).and_then(|p| go_path(&p)), 0),
                Serves::Given => match self.method(path, call) {
                    Some(m) => (Some(m), read(1).and_then(|p| go_path(&p)), 1),
                    None => continue,
                },
                Serves::Pattern => match read(0).and_then(|p| pattern(&p)) {
                    Some((method, route)) => (method, Some(route), 0),
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
            let method = method.unwrap_or_else(|| ANY.to_string());
            for prefix in prefixes {
                let full = normalize_path(&join(&prefix, &route));
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
            let external = code.external_of(path, call);
            let parents = match external.as_deref().and_then(split) {
                // `http.HandleFunc(..)`: registers on the router of the package itself.
                Some((package, name)) => {
                    let own = ROUTERS.iter().filter(|r| r.default && r.package == package);
                    if !own.flat_map(|r| r.routes).any(|(m, _)| *m == name) {
                        continue;
                    }
                    vec![Place::Default]
                }
                None => match &call.receiver {
                    Some(receiver) => go.places(path, &call.scope, receiver),
                    None => continue,
                },
            };
            if groups && external.is_none() {
                let prefix = (call.args.first())
                    .and_then(|p| path_literal(code.value(path, &call.scope, p)))
                    .and_then(|p| go_path(&p));
                let mounted = go.mounts.entry(Place::Made((path, n))).or_default();
                mounted.extend(parents.iter().map(|&p| (p, prefix.clone())));
            }
            if !routes {
                continue;
            }
            // A mux given as the handler is mounted; what is no mux is a route.
            let mounts = member == "Handle" && !parents.is_empty();
            let mounts = match (mounts, &call.args[..]) {
                (true, [_, handler]) => {
                    go.mount((path, &call.scope), handler, &parents, Some(String::new()))
                }
                _ => false,
            };
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
            let path = go_path(written).map(|p| normalize_path(&p));
            assert_eq!(path.as_deref(), Some(normalized), "{written}");
        }
        for unreadable in ["/{id", "/id}", "/{a{b}", "/a/{$}/b", "{$}"] {
            assert_eq!(go_path(unreadable), None, "{unreadable}");
        }
    }

    #[test]
    fn a_pattern_has_a_method_and_loses_its_host() {
        let cases = [
            ("/users", None, "/users"),
            ("GET /users/{id}", Some("GET"), "/users/{}"),
            ("POST   /users", Some("POST"), "/users"),
            ("example.com/users", None, "/users"),
            ("DELETE example.com/users/", Some("DELETE"), "/users"),
            ("GET /{$}", Some("GET"), "/"),
            ("/static/", None, "/static"),
        ];
        for (written, method, path) in cases {
            let read = pattern(written).map(|(m, p)| (m, normalize_path(&p)));
            assert_eq!(
                read,
                Some((method.map(str::to_string), path.to_string())),
                "{written}"
            );
        }
        for unreadable in ["get /users", "GET users", "GET /{id", "example.com", ""] {
            assert_eq!(pattern(unreadable), None, "{unreadable}");
        }
    }
}
