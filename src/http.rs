//! The HTTP contract kind: routes served by providers and the `fetch` calls that consume them.
//! A contract is `http::<METHOD>::<normalized path>`.

use std::collections::BTreeMap;

use crate::config::Serve;
use crate::contract::{ContractKind, Found, Level, Link, Site};
use crate::index::{Consumer, Provider};
use crate::lang::{Export, Part, Value};
use crate::resolve::Code;
use crate::workspace::Repository;

/// What every parameter (`[id]`, `[...slug]`, `:id`, a template hole) becomes in a path.
pub const PARAM: &str = "{}";

pub struct Http;

/// The contract identifier of `method` on `path`.
pub fn contract(method: &str, path: &str) -> String {
    format!("http::{}::{}", method.to_uppercase(), normalize_path(path))
}

/// Drops the query string, the fragment and a trailing `/`, and turns every segment holding
/// a parameter into `PARAM`. Case and API versions are kept. Template holes must already be
/// written as `PARAM`.
pub fn normalize_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segments: Vec<&str> = path
        .split('/')
        .map(|s| {
            let param =
                s.contains(PARAM) || s.starts_with(':') || (s.starts_with('[') && s.ends_with(']'));
            if param { PARAM } else { s }
        })
        .collect();
    let path = segments.join("/");
    let path = path.trim_end_matches('/');
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"];

/// The route a Next.js app-router file serves: `app/users/[id]/route.ts` (or under
/// `src/app/`) serves `/users/{}`. Route groups `(name)` are dropped.
fn next_route(path: &str) -> Option<String> {
    let rest = path
        .strip_prefix("src/")
        .unwrap_or(path)
        .strip_prefix("app/")?;
    let (dir, file) = rest.rsplit_once('/').unwrap_or(("", rest));
    if !matches!(file, "route.ts" | "route.js") {
        return None;
    }
    let segments: Vec<&str> = dir
        .split('/')
        .filter(|s| !s.is_empty() && !(s.starts_with('(') && s.ends_with(')')))
        .collect();
    Some(format!("/{}", segments.join("/")))
}

/// What a reader needs depends on a parameter of the enclosing function: a client wrapper.
struct Depends;

/// What one `fetch` call gives: its contract (`None` when the URL or the method cannot be read),
/// how it names the provider's repository, and, for an unresolved one, what could still be
/// read: its method (`None` when unknown) and the literal fragments of its path.
struct Read {
    contract: Option<String>,
    via: Option<String>,
    method: Option<String>,
    fragments: Vec<String>,
}

/// What `fetch(url, init)` reads, or `None` when the URL or the method depends on a parameter.
fn fetch(args: &[Value]) -> Option<Read> {
    let (contract, via) = locate(args)?;
    let (mut method, mut texts) = (None, Vec::new());
    if contract.is_none() {
        method = self::method(args.get(1))
            .ok()
            .flatten()
            .map(|m| m.to_uppercase());
        if let Some(Value::Str(parts)) = args.first() {
            texts = fragments(parts);
        }
    }
    Some(Read {
        contract,
        via,
        method,
        fragments: texts,
    })
}

/// The literal parts of the path of a URL, in order: what is left of `parts` once its scheme
/// and host and its query are dropped, split at every part that is not literal.
fn fragments(parts: &[Part]) -> Vec<String> {
    const GAP: &str = "\0";
    let text: String = parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => t.as_str(),
            _ => GAP,
        })
        .collect();
    let path = match text.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("", |at| &rest[at..]),
        None => &text,
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    // Route paths lose their trailing `/` (`normalize_path`), so a path ending in a literal does.
    let path = match path.ends_with(GAP) {
        true => path,
        false => path.trim_end_matches('/'),
    };
    (path.split(GAP).filter(|f| !f.is_empty()))
        .map(str::to_string)
        .collect()
}

/// The method and path of an HTTP contract identifier: `("GET", "/users/{}")`.
pub fn route(contract: &str) -> Option<(&str, &str)> {
    contract.strip_prefix("http::")?.split_once("::")
}

/// The gateway path of the HTTP route `contract` under `serve`, with the reason (see
/// `Serve::gateway_path`).
pub fn gateway_path(serve: &Serve, contract: &str) -> Option<(String, String)> {
    serve.gateway_path(route(contract)?.1)
}

/// Whether the route `contract` can be what a dynamic URL reaches: it is an HTTP route of
/// `method` when that is known, and its path holds every literal fragment, in order.
pub fn route_fits(contract: &str, method: Option<&str>, fragments: &[String]) -> bool {
    let Some((route_method, mut path)) = route(contract) else {
        return false;
    };
    if method.is_some_and(|m| m != route_method) {
        return false;
    }
    for fragment in fragments {
        match path.find(fragment.as_str()) {
            Some(at) => path = &path[at + fragment.len()..],
            None => return false,
        }
    }
    true
}

/// The contract and `via` of `fetch(url, init)`, or `None` when the URL or the method
/// depends on a parameter; the contract is `None` when the URL or the method cannot be read.
fn locate(args: &[Value]) -> Option<(Option<String>, Option<String>)> {
    let method = method(args.get(1)).ok()?;
    let parts = match args.first() {
        Some(Value::Str(parts)) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => return None,
        Some(Value::Str(parts)) => parts,
        Some(Value::Ref(_)) => return None,
        _ => return Some((None, None)),
    };
    let (via, path) = match parts.split_first() {
        Some((Part::Env(var), rest)) => (Some(format!("env:{var}")), Some(render(rest))),
        _ => {
            let url = render(parts);
            match url.split_once("://") {
                Some((_, rest)) => {
                    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                    let host = &rest[..end];
                    if host.contains(PARAM) || host.is_empty() {
                        (None, None)
                    } else {
                        let via = format!("host:{}", host.to_lowercase());
                        (Some(via), Some(rest[end..].to_string()))
                    }
                }
                None => {
                    // `${base}/users`: an unknown base, the host unresolved.
                    let base = parts.iter().take_while(|p| matches!(p, Part::Hole)).count();
                    let rest = render(&parts[base..]);
                    (None, (base == 0 || rest.starts_with('/')).then_some(rest))
                }
            }
        }
    };
    // A path that does not start at the root, such as `${base}${path}`, is not read.
    let path = path.filter(|p| p.is_empty() || p.starts_with(['/', '?', '#']));
    Some(match (method, path) {
        (Some(method), Some(path)) => (Some(contract(&method, &path)), via),
        _ => (None, via),
    })
}

/// The method `init` gives: `GET` when it gives none (the `fetch` default), `None` when it
/// cannot be read.
fn method(init: Option<&Value>) -> Result<Option<String>, Depends> {
    let Some(init) = init else {
        return Ok(Some("GET".into()));
    };
    let value = match init {
        Value::Object(properties) => match method_property(properties)? {
            Some(value) => value,
            None => return Ok(Some("GET".into())),
        },
        value => value,
    };
    match value {
        Value::Str(parts) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => Err(Depends),
        Value::Str(parts) => Ok(literal(parts)),
        Value::Ref(_) => Err(Depends),
        Value::Undefined => Ok(Some("GET".into())),
        _ => Ok(None),
    }
}

/// The value of the last `method` of an object, as in JavaScript, through spreads.
fn method_property(properties: &[(String, Value)]) -> Result<Option<&Value>, Depends> {
    static UNKNOWN: Value = Value::Unknown;
    for (key, value) in properties.iter().rev() {
        match (key.as_str(), value) {
            ("method", value) => return Ok(Some(value)),
            ("...", Value::Object(inner)) => {
                if let Some(found) = method_property(inner)? {
                    return Ok(Some(found));
                }
            }
            ("...", Value::Undefined) => {}
            ("...", Value::Ref(_)) => return Err(Depends),
            // A spread of something unknown may hide a method.
            ("...", _) => return Ok(Some(&UNKNOWN)),
            _ => {}
        }
    }
    Ok(None)
}

/// The text of `parts` when every part is literal.
fn literal(parts: &[Part]) -> Option<String> {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// `parts` as a path, every non-literal part a parameter.
fn render(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => t.as_str(),
            Part::Env(_) | Part::Ref(_) | Part::Hole => PARAM,
        })
        .collect()
}

/// What `express()`, `Router()` and `express.Router()` return: an app or a router.
const ROUTERS: &[&str] = &["express", "Router", "express.Router"];

/// The route-registering methods of an Express app or router.
const EXPRESS_METHODS: &[&str] = &["get", "post", "put", "delete", "patch", "options", "head"];

/// An app or router: the file and qualified name of the constant holding it.
type Router<'a> = (&'a str, &'a str);

/// `prefix` and `path` joined with one `/`.
fn join(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    match path.starts_with('/') {
        true => format!("{prefix}{path}"),
        false => format!("{prefix}/{path}"),
    }
}

/// Every prefix `router` is mounted under, sorted: the empty one when nothing mounts it.
fn prefixes<'a>(
    router: Router<'a>,
    mounts: &BTreeMap<Router<'a>, Vec<(Router<'a>, String)>>,
    seen: &mut Vec<Router<'a>>,
) -> Vec<String> {
    let Some(parents) = mounts.get(&router) else {
        return vec![String::new()];
    };
    // A router mounted inside itself, through any depth, adds nothing.
    if seen.contains(&router) {
        return Vec::new();
    }
    seen.push(router);
    let mut out: Vec<String> = parents
        .iter()
        .flat_map(|(parent, prefix)| {
            prefixes(*parent, mounts, seen)
                .into_iter()
                .map(|above| join(&above, prefix))
                .collect::<Vec<_>>()
        })
        .collect();
    seen.pop();
    out.sort();
    out.dedup();
    out
}

/// Express providers: `app.METHOD(path, ..., handler)` and `router.METHOD(...)` on a constant
/// holding an app or a router, under every prefix of `app.use(prefix, router)` mounts,
/// through imports and at any depth.
// ponytail: an app or router passed as a parameter, `router.route(path)` chains and
// `require` are not followed.
fn express(code: &Code) -> Vec<Found> {
    let router = |path, scope, name| -> Option<Router> {
        let (at, value) = code.constant(path, scope, name)?;
        matches!(value, Value::Call(c) if ROUTERS.contains(&c.as_str())).then_some(at)
    };
    let mut mounts: BTreeMap<Router, Vec<(Router, String)>> = BTreeMap::new();
    let mut routes = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let Some((receiver, member)) = call.callee.split_once('.') else {
                continue;
            };
            let registers = EXPRESS_METHODS.contains(&member);
            if !(registers || member == "use") || receiver.starts_with('(') {
                continue;
            }
            let Some(at) = router(path, &call.scope, receiver) else {
                continue;
            };
            if registers {
                routes.push((path, n, at, member.to_uppercase()));
                continue;
            }
            let Some(args) = code.args(path, n) else {
                continue;
            };
            let (prefix, rest) = match args.first().map(|a| code.value(path, &call.scope, a)) {
                Some(Value::Str(parts)) => (render(&parts), &args[1..]),
                _ => (String::new(), args),
            };
            for arg in rest {
                if let Value::Ref(name) = arg
                    && let Some(child) = router(path, &call.scope, name)
                {
                    mounts.entry(child).or_default().push((at, prefix.clone()));
                }
            }
        }
    }
    let mut found = Vec::new();
    for (path, n, at, method) in routes {
        let scope = &code.files()[path].calls[n].scope;
        // `app.get("env")` reads a setting.
        let Some([first, .., handler]) = code.args(path, n) else {
            continue;
        };
        let Value::Str(parts) = code.value(path, scope, first) else {
            continue;
        };
        let route = render(&parts);
        for prefix in prefixes(at, &mounts, &mut Vec::new()) {
            let full = normalize_path(&join(&prefix, &route));
            let site = match handler {
                Value::Function(def) => Site::Inline {
                    path: path.into(),
                    def: def.clone(),
                    name: format!("{method} {full}"),
                },
                Value::Ref(name) => match code.definition(path, scope, name) {
                    Some(t) => Site::Def {
                        path: t.path.into(),
                        name: t.name.into(),
                    },
                    None => Site::File(path.into()),
                },
                _ => Site::File(path.into()),
            };
            found.push(Found {
                site,
                contract: Some(contract(&method, &full)),
                via: None,
                method: None,
                fragments: Vec::new(),
            });
        }
    }
    found
}

impl ContractKind for Http {
    fn prefix(&self) -> &'static str {
        "http"
    }

    fn extract(&self, code: &Code) -> Vec<Found> {
        let mut found = Vec::new();
        for (&path, facts) in code.files() {
            for (n, call) in facts.calls.iter().enumerate() {
                let is_fetch = call.callee == "fetch"
                    || code.param_defaults_to(path, &call.scope, &call.callee, "fetch");
                if !is_fetch || code.args(path, n).is_none_or(<[_]>::is_empty) {
                    continue;
                }
                for (site, at, read) in code.expand(path, n, &fetch) {
                    found.push(Found {
                        site: Site::Call {
                            path: site.into(),
                            index: at,
                        },
                        contract: read.contract,
                        via: read.via,
                        method: read.method,
                        fragments: read.fragments,
                    });
                }
            }
            let Some(route) = next_route(path) else {
                continue;
            };
            for export in &facts.exports {
                let Export::Local { exported, local } = export else {
                    continue;
                };
                if !METHODS.contains(&exported.as_str()) {
                    continue;
                }
                // `export const GET = withAuth(...)` defines no function: the file serves it.
                let site = if facts.defs.iter().any(|d| d.name == *local) {
                    Site::Def {
                        path: path.into(),
                        name: local.clone(),
                    }
                } else {
                    Site::File(path.into())
                };
                found.push(Found {
                    site,
                    contract: Some(contract(exported, &route)),
                    via: None,
                    method: None,
                    fragments: Vec::new(),
                });
            }
        }
        found.extend(express(code));
        found
    }

    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        let via = consumer.via.as_deref();
        // A route serves a consumer by its code path, or by its gateway path with the reason
        // for the rewrite. The two never meet on one route: a gateway path differs from it.
        let wanted = route(contract);
        let serving = providers.iter().filter_map(|&(m, p)| {
            if p.contract == *contract {
                return Some((m, p, None));
            }
            let (method, _) = route(&p.contract)?;
            let (path, reason) = gateway_path(&m.config.serve, &p.contract)?;
            (wanted == Some((method, path.as_str()))).then_some((m, p, Some(reason)))
        });
        let var = via.and_then(|v| v.strip_prefix("env:"));
        let peers: Vec<&str> = var
            .map(|var| from.config.peers_reading(var).collect())
            .unwrap_or_default();
        let (fits, level, reason): (Vec<_>, _, _) =
            if let Some(host) = via.and_then(|v| v.strip_prefix("host:")) {
                let fits = serving.filter(|(m, ..)| m.config.serves(host)).collect();
                (fits, Level::Exact, format!("host {host} is served by"))
            } else if let (Some(var), false) = (var, peers.is_empty()) {
                let fits = serving
                    .filter(|(m, ..)| peers.contains(&m.identity.as_str()))
                    .collect();
                (
                    fits,
                    Level::Inferred,
                    format!("base URL from {var}, a base_url_env of peer"),
                )
            } else {
                // The host is unresolved: a relative URL, an unknown base, or a variable no
                // peer names. Exactly one repository serving the route is enough.
                let fits: Vec<_> = serving.collect();
                let mut repos: Vec<&str> = fits.iter().map(|(m, ..)| m.identity.as_str()).collect();
                repos.sort();
                repos.dedup();
                let why = match var {
                    Some(var) => format!("base URL from {var}, no peer's base_url_env"),
                    None => "host unresolved".to_string(),
                };
                let reason = match repos.len() {
                    1 => format!("{why}; the only repository serving it is"),
                    _ => format!("{why}; served by"),
                };
                (fits, Level::Inferred, reason)
            };
        let ambiguous = fits.len() > 1;
        fits.iter()
            .map(|(repo, provider, gateway)| {
                // Through a gateway the link is inferred, whatever the consumer side gave.
                let rewrite = gateway.as_ref().map(|g| format!(" ({g})"));
                let rewrite = rewrite.as_deref().unwrap_or("");
                Link {
                    consumer: consumer.site.clone(),
                    provider: provider.site.clone(),
                    contract: contract.clone(),
                    level: match (ambiguous, gateway) {
                        (true, _) => Level::Ambiguous,
                        (_, Some(_)) => Level::Inferred,
                        _ => level,
                    },
                    reason: if ambiguous {
                        let n = fits.len();
                        format!("{n} providers fit; {reason} {}{rewrite}", repo.identity)
                    } else {
                        format!("{reason} {}{rewrite}", repo.identity)
                    },
                    by: None,
                    p: None,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{fragments, normalize_path, route_fits};

    #[test]
    fn paths_normalize_alike_on_both_sides() {
        for (path, normalized) in [
            ("/api/users", "/api/users"),
            ("/api/users/", "/api/users"),
            ("/api/users?page=2", "/api/users"),
            ("/api/users#top", "/api/users"),
            ("/api/users/?page=2#top", "/api/users"),
            ("/api/users/[id]", "/api/users/{}"),
            ("/api/users/:id/posts", "/api/users/{}/posts"),
            ("/api/users/{}", "/api/users/{}"),
            ("/api/users/u-{}", "/api/users/{}"),
            ("/docs/[...slug]", "/docs/{}"),
            ("/docs/[[...slug]]", "/docs/{}"),
            ("/api/v1/Users", "/api/v1/Users"),
            ("/api/v2/users", "/api/v2/users"),
            ("/", "/"),
            ("", "/"),
            ("?q=1", "/"),
        ] {
            assert_eq!(normalize_path(path), normalized, "{path}");
        }
    }

    #[test]
    fn a_route_fits_a_dynamic_url_by_method_and_ordered_literal_fragments() {
        let fits = |contract, method, fragments: &[&str]| {
            let fragments: Vec<String> = fragments.iter().map(|f| f.to_string()).collect();
            route_fits(contract, method, &fragments)
        };
        for (contract, method, parts, expected) in [
            // No method, no fragment: every route.
            ("http::GET::/api/users", None, &[][..], true),
            // The method filters when known.
            ("http::GET::/api/users", Some("GET"), &[][..], true),
            ("http::POST::/api/users", Some("GET"), &[][..], false),
            // Every fragment must be in the path, in order, without overlapping.
            ("http::GET::/api/users/{}", None, &["/api/"][..], true),
            (
                "http::GET::/api/users/{}",
                None,
                &["/api", "/users/"][..],
                true,
            ),
            (
                "http::GET::/api/users/{}",
                None,
                &["/users/", "/api/"][..],
                false,
            ),
            (
                "http::GET::/api/users",
                None,
                &["/api/users", "/users"][..],
                false,
            ),
            (
                "http::GET::/api/users/{}/posts",
                None,
                &["/api/", "/posts"][..],
                true,
            ),
            ("http::GET::/api/orders", None, &["/users"][..], false),
            ("http::GET::/api/users", None, &["/api/users"][..], true),
            // Both filters apply together.
            (
                "http::DELETE::/api/users/{}",
                Some("GET"),
                &["/api/"][..],
                false,
            ),
            // Not an HTTP route.
            ("channel::orders", None, &[][..], false),
        ] {
            assert_eq!(
                fits(contract, method, parts),
                expected,
                "{contract} {method:?} {parts:?}"
            );
        }
    }

    #[test]
    fn the_fragments_of_a_url_are_its_literal_path_parts_without_scheme_host_and_query() {
        use crate::lang::Part::{Hole, Text};
        let text = |t: &str| Text(t.to_string());
        for (parts, expected) in [
            (vec![text("/api/users")], vec!["/api/users"]),
            (
                vec![text("/api/"), Hole, text("/posts")],
                vec!["/api/", "/posts"],
            ),
            (vec![Hole, text("api/users")], vec!["api/users"]),
            (
                vec![text("https://"), Hole, text(".example.com/api/v2/"), Hole],
                vec!["/api/v2/"],
            ),
            (vec![text("https://example.com")], vec![]),
            (
                vec![text("/api/users?page="), Hole, text("&x=1")],
                vec!["/api/users"],
            ),
            (vec![Hole, Hole], vec![]),
            // The trailing `/` of the end of the path is dropped, as route paths drop it.
            (vec![text("/api/users/?page="), Hole], vec!["/api/users"]),
            (
                vec![text("/api/users/"), Hole, text("/")],
                vec!["/api/users/"],
            ),
            (vec![text("/api/users/"), Hole], vec!["/api/users/"]),
        ] {
            assert_eq!(fragments(&parts), expected);
        }
    }
}
