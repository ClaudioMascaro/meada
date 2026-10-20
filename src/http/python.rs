//! FastAPI and Flask providers (spec #24): a decorator registers the function it decorates on an
//! app or a router, under the router's own prefix and every prefix it is mounted under
//! (`include_router`, `mount`, `register_blueprint`, through the root's `prefixes`), the app or
//! router followed through `Code::origins` and counted only when its framework's constructor
//! creates it (`Code::external`). Django providers: the `path()` and `re_path()` entries of every
//! root URLconf's `urlpatterns`, each `include()` followed at any depth (`URLCONF_BUDGET`,
//! `INCLUDE_DEPTH`); a function view serves what its decorators allow or every method, a
//! class-based view the handler methods it defines. Seams of the root reused: `prefixes`,
//! `path_literal`, `methods`, `normalize_path`, `provider`.

mod clients;

pub(super) use clients::consumers;

use super::{
    ANY, CallAt, HTTP_METHODS, Mounts, PARAM, contract, join, methods, normalize_path,
    path_literal, prefixes, provider,
};
use crate::contract::{Found, Site};
use crate::lang::{Call, Value, literal};
use crate::resolve::{Code, Target};

/// FastAPI, Flask and Django providers: see the module.
pub(super) fn providers(code: &Code) -> Vec<Found> {
    let mut found = decorated(code);
    found.extend(django(code));
    found
}

/// A method mounting an app or a router (the child) on the one it is called on.
struct Mount {
    method: &'static str,
    /// The child's position, and its keyword.
    child: (usize, &'static str),
    /// The prefix given: its position when it may be positional, and its keyword.
    prefix: (Option<usize>, &'static str),
    /// Whether the child may be an app (FastAPI's `include_router(app.router)` and `mount`), so
    /// a child that cannot be read may be any app.
    takes_apps: bool,
}

/// The argument of `call` at `position`, or else its keyword argument `keyword`.
fn argument<'c>(call: &'c Call, (position, keyword): (Option<usize>, &str)) -> Option<&'c Value> {
    (position.and_then(|n| call.args.get(n))).or_else(|| call.keyword(keyword))
}

/// A Python web framework whose routes are decorators on an app or a router.
struct Framework {
    /// What creates its apps, as named outside the repository: an app nothing mounts is served
    /// at its root.
    apps: &'static [&'static str],
    /// What creates its routers: a router is served only where a mount Meada reads puts it.
    routers: &'static [&'static str],
    /// The constructor's keyword giving a router's own prefix.
    prefix: &'static str,
    mounts: &'static [Mount],
    /// Whether a prefix given when mounting replaces the router's own (Flask) or goes before
    /// it (FastAPI).
    replaces: bool,
    /// The decorator taking a path and `methods=[...]` (`GET` without them).
    route: &'static str,
    /// Whether `@app.get(path)` and the other method decorators register a route.
    shortcuts: bool,
}

const FRAMEWORKS: &[Framework] = &[
    Framework {
        apps: &["fastapi.FastAPI"],
        routers: &["fastapi.APIRouter"],
        prefix: "prefix",
        mounts: &[
            Mount {
                method: "include_router",
                child: (0, "router"),
                prefix: (None, "prefix"),
                takes_apps: true,
            },
            Mount {
                method: "mount",
                child: (1, "app"),
                prefix: (Some(0), "path"),
                takes_apps: true,
            },
        ],
        replaces: false,
        route: "api_route",
        shortcuts: true,
    },
    Framework {
        apps: &["flask.Flask"],
        routers: &["flask.Blueprint"],
        prefix: "url_prefix",
        mounts: &[Mount {
            method: "register_blueprint",
            child: (0, "blueprint"),
            prefix: (None, "url_prefix"),
            takes_apps: false,
        }],
        replaces: true,
        route: "route",
        shortcuts: false,
    },
];

/// FastAPI and Flask providers: a decorator registering the function it decorates on an app
/// or a router, under the router's own prefix and every prefix it is mounted under, at any
/// depth. The app or router is followed through constants, imports, arguments and return
/// values; a decorator counts only on one its framework's constructor creates. A router is
/// served only under the mounts Meada reads; an app nothing mounts is served at its root,
/// unless a mount whose child cannot be read, called on another app or router of its
/// framework, may mount it.
// ponytail: such a mount hides every other app of its framework that nothing else mounts, even
// one it cannot hold.
fn decorated<'a>(code: &Code<'a>) -> Vec<Found> {
    // The framework that creates an app or router, and whether it is an app.
    let framework = |(path, n): CallAt<'a>| -> Option<(&Framework, bool)> {
        let call = &code.files()[path].calls[n];
        let name = code.external(path, &call.scope, &call.callee)?;
        FRAMEWORKS.iter().find_map(|f| {
            let app = f.apps.contains(&name.as_str());
            (app || f.routers.contains(&name.as_str())).then_some((f, app))
        })
    };
    let routers = |path, scope, name| -> Vec<(CallAt, &Framework)> {
        let origins = code.name_origins(path, scope, name).into_iter();
        origins.filter_map(|r| Some((r, framework(r)?.0))).collect()
    };
    // The prefix a router's constructor gives it: none when absent.
    let own = |router @ (path, n): CallAt<'a>| -> Option<String> {
        let call = code.call(path, n)?;
        match call.keyword(framework(router)?.0.prefix) {
            None => Some(String::new()),
            Some(prefix) => path_literal(code.value(path, &call.scope, prefix)),
        }
    };
    let mut mounts = Mounts::new();
    // The apps and routers holding a mount that takes apps and whose child cannot be read.
    let mut unread: Vec<(CallAt, &Framework)> = Vec::new();
    let mut routes = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let Some((receiver, member)) = call.callee.rsplit_once('.') else {
                continue;
            };
            let mounting = (FRAMEWORKS.iter()).any(|f| f.mounts.iter().any(|m| m.method == member));
            if !(call.decorates.is_some() || mounting) || receiver.starts_with('(') {
                continue;
            }
            for (at, kind) in routers(path, &call.scope, receiver) {
                if let Some(def) = &call.decorates {
                    routes.push((path, n, at, kind, member, def));
                    continue;
                }
                let Some(how) = kind.mounts.iter().find(|m| m.method == member) else {
                    continue;
                };
                let Some(mount) = code.call(path, n) else {
                    continue;
                };
                let (position, keyword) = how.child;
                let Some(child) = argument(mount, (Some(position), keyword)) else {
                    continue;
                };
                let given = argument(mount, how.prefix)
                    .map(|p| code.value(path, &call.scope, p))
                    .filter(|p| !matches!(p, Value::Undefined));
                let children: Vec<CallAt> = (code.value_origins(path, &call.scope, child))
                    .into_iter()
                    .filter(|&c| framework(c).is_some_and(|(f, _)| std::ptr::eq(f, kind)))
                    .collect();
                if children.is_empty() && how.takes_apps {
                    unread.push((at, kind));
                }
                for child in children {
                    let segment = match given.clone() {
                        None => own(child),
                        Some(given) if kind.replaces => path_literal(given),
                        Some(given) => path_literal(given)
                            .zip(own(child))
                            .map(|(g, o)| join(&g, &o)),
                    };
                    mounts.entry(child).or_default().push((at, segment));
                }
            }
        }
    }
    // Where a router nothing mounts is served: an app at its root, unless a mount that cannot
    // be read may hold it.
    let root = |router: CallAt<'a>| -> Option<String> {
        let (kind, app) = framework(router)?;
        let elsewhere = (unread.iter()).any(|&(at, f)| at != router && std::ptr::eq(f, kind));
        (app && !elsewhere).then(|| own(router)).flatten()
    };
    let mut found = Vec::new();
    for (path, n, at, kind, member, def) in routes {
        let Some(call) = code.call(path, n) else {
            continue;
        };
        let Some(route) =
            (call.args.first()).and_then(|p| path_literal(code.value(path, &call.scope, p)))
        else {
            continue;
        };
        let methods: Vec<String> = if kind.shortcuts && HTTP_METHODS.contains(&member) {
            vec![member.to_uppercase()]
        } else if member == kind.route {
            match call
                .keyword("methods")
                .map(|m| code.value(path, &call.scope, m))
            {
                None | Some(Value::Undefined) => vec!["GET".into()],
                Some(listed) => methods(listed),
            }
        } else {
            Vec::new()
        };
        for prefix in prefixes(at, &mounts, &root, &mut Vec::new()) {
            let full = normalize_path(&join(&prefix, &route));
            for method in &methods {
                let site = Site::Def {
                    path: path.into(),
                    name: def.clone(),
                };
                found.push(provider(site, contract(method, &full)));
            }
        }
    }
    found
}

/// The methods a Django class-based view serves by defining them.
const VIEW_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "trace",
];

/// What a Django URLconf entry or a view decorator is, by its name outside the repository.
const DJANGO_PATH: &str = "django.urls.path";
const DJANGO_RE_PATH: &str = "django.urls.re_path";
const DJANGO_INCLUDE: &str = "django.urls.include";

/// How many URLconf entries `django` reads, over every root and every `include()` it follows.
/// A module included under several prefixes is read once for each, so a crafted ladder of
/// modules each including the next twice has as many paths as two to its height; past the
/// budget the remaining entries yield no provider.
// ponytail: real projects have a few thousand routes at most; 2^15 entries is far past them.
const URLCONF_BUDGET: usize = 1 << 15;
/// How many `include()` calls deep `django` follows a URLconf: the routes of a module included
/// deeper yield no provider.
// ponytail: real projects nest a few levels of `include()`; the limit keeps a crafted chain of
// thousands of modules from overflowing the stack, which the budget alone would not.
const INCLUDE_DEPTH: usize = 64;
const DJANGO_HTTP: &str = "django.views.decorators.http.";

/// A `re_path()` pattern as a route: literal text and simple named groups (`(?P<name>...)`),
/// each group a parameter; `None` for any other pattern. A leading `^` and a trailing `$`
/// anchor it.
fn regex_route(pattern: &str) -> Option<String> {
    let pattern = pattern.strip_prefix('^').unwrap_or(pattern);
    let pattern = pattern.strip_suffix('$').unwrap_or(pattern);
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        match c {
            // An escaped punctuation character is itself; `\d` and the like are classes.
            '\\' => {
                let escaped = rest.chars().next().filter(|e| !e.is_ascii_alphanumeric())?;
                rest = &rest[escaped.len_utf8()..];
                out.push(escaped);
            }
            '(' => {
                let group = rest.strip_prefix("?P<")?;
                let (name, body) = group.split_once('>')?;
                if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return None;
                }
                // A group holding no other group: it ends at the first `)` not escaped.
                let mut chars = body.char_indices();
                let end = loop {
                    match chars.next()? {
                        (_, '\\') => _ = chars.next()?,
                        (_, '(') => return None,
                        (at, ')') => break at,
                        _ => {}
                    }
                };
                rest = &body[end + 1..];
                out.push_str(PARAM);
            }
            '.' | '*' | '+' | '?' | '[' | ']' | '{' | '}' | '|' | ')' | '^' | '$' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

/// The methods the Django function view `view` serves, from its decorators: those of
/// `require_http_methods([...])`, `require_GET`, `require_POST` and `require_safe` (`GET`
/// and `HEAD`), all of them when several restrict it; `None` when nothing restricts it.
fn view_methods<'a>(code: &Code<'a>, view: Target<'a>) -> Option<Vec<String>> {
    let facts = code.files()[view.path];
    let scope = crate::parent(view.name);
    let def = facts.defs.iter().find(|d| d.name == view.name)?;
    let mut allowed: Option<Vec<String>> = None;
    let mut restrict = |methods: Vec<String>| {
        allowed = Some(match allowed.take() {
            None => methods,
            Some(before) => before.into_iter().filter(|m| methods.contains(m)).collect(),
        });
    };
    let named = |name: &str| -> Option<&[&str]> {
        match name.strip_prefix(DJANGO_HTTP)? {
            "require_GET" => Some(&["GET"][..]),
            "require_POST" => Some(&["POST"]),
            "require_safe" => Some(&["GET", "HEAD"]),
            _ => None,
        }
    };
    for name in &def.decorators {
        if let Some(methods) = code
            .external(view.path, scope, name)
            .as_deref()
            .and_then(named)
        {
            restrict(methods.iter().map(|m| m.to_string()).collect());
        }
    }
    for (n, call) in facts.calls.iter().enumerate() {
        if call.decorates.as_deref() != Some(view.name)
            || (code.external(view.path, &call.scope, &call.callee))
                .is_none_or(|e| e != format!("{DJANGO_HTTP}require_http_methods"))
        {
            continue;
        }
        let list = (code.args(view.path, n).and_then(<[_]>::first))
            .map(|m| code.value(view.path, &call.scope, m));
        restrict(list.map(methods).unwrap_or_default());
    }
    allowed
}

/// What an `include()` argument names.
enum Include<'a> {
    /// A URLconf module of the repository.
    Module(&'a str),
    /// The entries of a list, as written.
    Entries(&'a Value),
    /// A dotted name no module of the repository answers to: a module outside it, or one
    /// Meada cannot resolve, so any URLconf whose path ends with it may be that module.
    Named(String),
    /// Anything Meada cannot read: it may name any URLconf.
    Unknown,
}

/// What the `include()` argument `arg`, written in `scope` of `path`, names: a module by dotted
/// name or as a module, a list of entries, or either of them first in a tuple with the
/// application namespace (`include(("shop.urls", "shop"))`).
fn include<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, arg: &'a Value) -> Include<'a> {
    let dotted = |value: Value| -> Include<'a> {
        let Value::Str(parts) = value else {
            return Include::Unknown;
        };
        let Some(name) = literal(&parts) else {
            return Include::Unknown;
        };
        match code.import_file(path, &name) {
            Some(module) => Include::Module(module),
            None => Include::Named(name),
        }
    };
    let entries = || {
        let origins = code.value_origins(path, scope, arg);
        let known = !origins.is_empty()
            && (origins.iter())
                .all(|&o| is_call(code, o, DJANGO_PATH) || is_call(code, o, DJANGO_RE_PATH));
        if known {
            Include::Entries(arg)
        } else {
            Include::Unknown
        }
    };
    match arg {
        Value::List(items) => match &items[..] {
            [first, Value::Str(_)] => include(code, path, scope, first),
            _ => Include::Entries(arg),
        },
        Value::Str(_) => dotted(code.value(path, scope, arg)),
        Value::Ref(name) => {
            if let Some(module) = code.module(path, scope, name) {
                return Include::Module(module);
            }
            if let Some(name) = code.external(path, scope, name) {
                return Include::Named(name);
            }
            match code.value(path, scope, arg) {
                value @ Value::Str(_) => dotted(value),
                _ => entries(),
            }
        }
        _ => entries(),
    }
}

/// The URLconf entries an `include()` call at `index` of `path` holds: the `urlpatterns` of
/// the module it names, or the entries of the list it is given.
fn included<'a>(code: &Code<'a>, path: &'a str, index: usize) -> Vec<CallAt<'a>> {
    let Some(call) = code.call(path, index) else {
        return Vec::new();
    };
    let Some(arg) = call.args.first() else {
        return Vec::new();
    };
    match include(code, path, &call.scope, arg) {
        Include::Module(module) => code.name_origins(module, "", "urlpatterns"),
        Include::Entries(list) => code.value_origins(path, &call.scope, list),
        Include::Named(_) | Include::Unknown => Vec::new(),
    }
}

/// Whether `module`, a file of the repository, may be what the dotted name `name` imports from
/// some source root: its dotted path is `name` or ends with it.
fn may_be(module: &str, name: &str) -> bool {
    let module = module.strip_suffix(".py").unwrap_or(module);
    let module = module.strip_suffix("/__init__").unwrap_or(module);
    let module = module.replace('/', ".");
    module == name || module.ends_with(&format!(".{name}"))
}

/// Whether the call at `at` is the function `name` outside the repository.
fn is_call<'a>(code: &Code<'a>, (path, n): CallAt<'a>, name: &str) -> bool {
    let call = &code.files()[path].calls[n];
    code.external(path, &call.scope, &call.callee).as_deref() == Some(name)
}

/// Django providers: the `path()` and `re_path()` entries of every root URLconf's
/// `urlpatterns`, each `include()` followed at any depth with the route before it. A root is a
/// module no `include()` may name: none names it, none holds a dotted name its path ends with,
/// and none holds an argument Meada cannot read. A function view serves the methods its
/// decorators allow, or every method (`ANY`); a class-based view (`View.as_view()`) the handler
/// methods it defines.
// ponytail: one `include()` Meada cannot read leaves no root at all; a single `urlpatterns =
// [...] + other` (no list literal as a whole) yields no entry.
fn django<'a>(code: &Code<'a>) -> Vec<Found> {
    let mut roots: Vec<&str> = (code.files().iter())
        .filter(|(_, f)| f.consts.iter().any(|c| c.name == "urlpatterns"))
        .map(|(&path, _)| path)
        .collect();
    if roots.is_empty() {
        return Vec::new();
    }
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            // The name check first: `is_call` resolves the callee.
            let named = call.callee.rsplit('.').next() == Some("include");
            if !named || !is_call(code, (path, n), DJANGO_INCLUDE) {
                continue;
            }
            let Some(arg) = code.args(path, n).and_then(<[_]>::first) else {
                continue;
            };
            match include(code, path, &call.scope, arg) {
                Include::Module(module) => roots.retain(|&c| c != module),
                Include::Named(name) => roots.retain(|c| !may_be(c, &name)),
                Include::Unknown => roots.clear(),
                Include::Entries(_) => {}
            }
        }
    }
    let (mut found, mut budget) = (Vec::new(), URLCONF_BUDGET);
    for root in roots {
        let entries = code.name_origins(root, "", "urlpatterns");
        urlconf(code, entries, "", &mut Vec::new(), &mut budget, &mut found);
    }
    found
}

/// The providers of URLconf `entries` under `prefix`; `stack` holds the `include()` calls
/// being followed, so one met again (a cycle) adds nothing. Each entry read is charged to
/// `budget`; once it is spent, nothing more is read.
// ponytail: `django.conf.urls.url`, a view wrapped in a call (`require_POST(view)`), a handler
// method a class-based view inherits and `as_view()` on a class outside the repository yield
// nothing; a `re_path()` pattern is read as written, so a non-raw string with doubled
// backslashes yields nothing.
fn urlconf<'a>(
    code: &Code<'a>,
    entries: Vec<CallAt<'a>>,
    prefix: &str,
    stack: &mut Vec<CallAt<'a>>,
    budget: &mut usize,
    found: &mut Vec<Found>,
) {
    for entry @ (path, n) in entries {
        let Some(left) = budget.checked_sub(1) else {
            return;
        };
        *budget = left;
        let regex = match (
            is_call(code, entry, DJANGO_PATH),
            is_call(code, entry, DJANGO_RE_PATH),
        ) {
            (true, _) => false,
            (_, true) => true,
            _ => continue,
        };
        let Some(call) = code.call(path, n) else {
            continue;
        };
        let route =
            (call.args.first()).and_then(|r| path_literal(code.value(path, &call.scope, r)));
        let route = match regex {
            true => route.and_then(|p| regex_route(&p)),
            false => route,
        };
        let Some(route) = route.map(|r| format!("{prefix}{r}")) else {
            continue;
        };
        let Some(view) = call.args.get(1).or_else(|| call.keyword("view")) else {
            continue;
        };
        let full = normalize_path(&route);
        let serve = |path: &str, name: &str, method: &str| {
            let site = Site::Def {
                path: path.into(),
                name: name.into(),
            };
            provider(site, contract(method, &full))
        };
        if let Value::Ref(name) = view
            && let Some(def) = code.definition(path, &call.scope, name)
        {
            let methods = view_methods(code, def).unwrap_or_else(|| vec![ANY.into()]);
            for method in methods {
                found.push(serve(def.path, def.name, &method));
            }
            continue;
        }
        for origin @ (at, i) in code.value_origins(path, &call.scope, view) {
            let Some(origin_call) = code.call(at, i) else {
                continue;
            };
            if is_call(code, origin, DJANGO_INCLUDE) {
                if stack.contains(&origin) || stack.len() >= INCLUDE_DEPTH {
                    continue;
                }
                stack.push(origin);
                urlconf(code, included(code, at, i), &route, stack, budget, found);
                stack.pop();
            } else if let Some(class) = origin_call.callee.strip_suffix(".as_view")
                && let Some(class) = code.definition(at, &origin_call.scope, class)
            {
                for method in VIEW_METHODS {
                    let handler = format!("{}.{method}", class.name);
                    if code.files()[class.path]
                        .defs
                        .iter()
                        .any(|d| d.name == handler)
                    {
                        found.push(serve(class.path, &handler, method));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_regex_route_reads_literal_text_and_simple_named_groups_only() {
        for (pattern, route) in [
            ("^users/$", Some("users/")),
            ("^api/", Some("api/")),
            ("users/(?P<pk>[0-9]+)/$", Some("users/{}/")),
            (r"^v1\.0/(?P<slug>[-\w]+)/(?P<n>\d+)$", Some("v1.0/{}/{}")),
            (r"^(?P<code>[^\)]+)/$", Some("{}/")),
            (r"^archive/(\d{4})/$", None),
            ("^(?P<a>(x|y))/$", None),
            ("^users/.*$", None),
            ("^(?:users)/$", None),
        ] {
            assert_eq!(regex_route(pattern).as_deref(), route, "{pattern}");
        }
    }
}
