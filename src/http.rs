//! The HTTP contract kind: routes served by providers and the `fetch` calls that consume them.
//! A contract is `http::<METHOD>::<normalized path>`.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::config::gateway_path;
use crate::contract::{ContractKind, Found, Level, Link, Role, Site, shown};
use crate::entry::EntryKind;
use crate::index::{self, Consumer, Export as IndexExport, Gateway, Provider};
use crate::lang::{Depends, Part, Value, literal, property};
use crate::literals::Literals;
use crate::resolve::Code;
use crate::shape::{self, Shape};
use crate::workspace::Repository;

mod go;
mod python;
mod rust;
mod ts;

/// What every parameter (`[id]`, `[...slug]`, `:id`, a template hole) becomes in a path.
pub const PARAM: &str = "{}";

/// The method of a provider that serves every method: `http::ANY::<path>`.
pub const ANY: &str = "ANY";

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
            let param = s.contains(PARAM)
                || s.starts_with(':')
                || (s.starts_with('[') && s.ends_with(']'))
                || (s.contains('{') && s.contains('}'))
                || (s.starts_with('<') && s.ends_with('>'));
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

/// The contracts `provider` serves at its gateway paths, each with the reason for the rewrite.
pub(crate) fn gateway_contracts(provider: &Provider) -> impl Iterator<Item = (String, &str)> {
    let method = route(&provider.contract).map(|(method, _)| method);
    (provider.gateways.iter())
        .filter_map(move |g| Some((contract(method?, &g.path), g.reason.as_str())))
}

/// Whether a provider of the route `provided` serves the route `wanted`: the same route, or
/// the same path when the provider serves every method (`ANY`).
pub(crate) fn fits(provided: &str, wanted: &str) -> bool {
    provided == wanted
        || matches!((route(provided), route(wanted)), (Some((ANY, p)), Some((_, w))) if p == w)
}

/// Whether `provider` serves `contract`, by its own path or by a gateway path.
pub(crate) fn serves(provider: &Provider, contract: &str) -> bool {
    fits(&provider.contract, contract)
        || gateway_contracts(provider).any(|(g, _)| fits(&g, contract))
}

/// Sets the drift (spec #23) of every HTTP link of `links` with a single provider (any level
/// but `ambiguous`) and a shape on both sides: the consumer's at its call site, and that of the
/// provider at the link's provider site serving the link's contract. Run on the links of the
/// whole workspace once the decisions are replayed, so everything reading links sees it.
pub(crate) fn drift(links: &mut [Link], exports: &[IndexExport]) {
    // A call site holds one consumer per contract it reaches (a wrapper fetching two routes);
    // the same contract from several hosts (`via`) counts only when they read alike.
    let mut consumers: HashMap<(&str, Option<&str>), Vec<Option<&Shape>>> = HashMap::new();
    for c in exports.iter().flat_map(|e| &e.consumers) {
        let key = (c.site.as_str(), c.contract.as_deref());
        consumers.entry(key).or_default().push(c.shape.as_ref());
    }
    let read = |site, contract| match consumers.get(&(site, contract))?.as_slice() {
        [first, rest @ ..] if rest.iter().all(|s| s == first) => *first,
        _ => None,
    };
    let mut providers: HashMap<&str, Vec<&Provider>> = HashMap::new();
    for p in exports.iter().flat_map(|e| &e.providers) {
        if p.shape.is_some() && route(&p.contract).is_some() {
            providers.entry(p.site.as_str()).or_default().push(p);
        }
    }
    for link in links {
        if link.level == Level::Ambiguous || route(&link.contract).is_none() {
            continue;
        }
        // A decided link of an unresolved consumer carries the provider's contract.
        let site = link.consumer.as_str();
        let Some(consumer) = read(site, Some(&link.contract)).or_else(|| read(site, None)) else {
            continue;
        };
        let served = (providers.get(link.provider.as_str()).into_iter().flatten())
            .find(|p| serves(p, &link.contract));
        if let Some(provider) = served.and_then(|p| p.shape.as_ref()) {
            link.drift = shape::drift(provider, consumer);
        }
    }
}

/// Whether the route `contract` can be what a dynamic URL reaches: it is an HTTP route of
/// `method` when that is known (or of every method), and its path holds every literal
/// fragment, in order.
pub fn route_fits(contract: &str, method: Option<&str>, fragments: &[String]) -> bool {
    let Some((route_method, mut path)) = route(contract) else {
        return false;
    };
    if method.is_some_and(|m| m != route_method && route_method != ANY) {
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
        Value::Object(properties) => match property(properties, "method")? {
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

/// `url` with the client's `base` URL joined in front, when `url` is a path relative to it: it
/// starts with literal text that is not a scheme. An absolute URL, and one whose start is not
/// known, are left as they are.
fn based(base: Option<&[Part]>, url: &Value) -> Value {
    let (Some(base), Value::Str(parts)) = (base, url) else {
        return url.clone();
    };
    let lead: String = (parts.iter())
        .map_while(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    if !parts.is_empty() && (lead.is_empty() || lead.contains("://")) {
        return url.clone();
    }
    let mut out = base.to_vec();
    if let Some(Part::Text(t)) = out.last_mut() {
        *t = t.trim_end_matches('/').to_string();
    }
    out.push(Part::Text("/".into()));
    let mut path = parts.clone();
    if let Some(Part::Text(t)) = path.first_mut() {
        *t = t.trim_start_matches('/').to_string();
    }
    out.extend(path);
    Value::Str(out)
}

/// The methods a list of literal strings names (`methods=["GET", "post"]`), upper-cased; none
/// for anything but a list.
fn methods(list: Value) -> Vec<String> {
    let Value::List(items) = list else {
        return Vec::new();
    };
    (items.into_iter())
        .filter_map(|m| match m {
            Value::Str(parts) => literal(&parts).map(|m| m.to_uppercase()),
            _ => None,
        })
        .collect()
}

/// A provider of `contract` at `site`.
fn provider(site: Site, contract: String) -> Found {
    Found {
        role: Role::Provider,
        site,
        contract: Some(contract),
        via: None,
        method: None,
        fragments: Vec::new(),
        shape: None,
    }
}

/// The consumer at the call `index` of `path`, as `read` reads it, with the shape it reads of
/// the response (spec #23), if any.
fn consumer(path: &str, index: usize, read: Read, shape: Option<Shape>) -> Found {
    Found {
        role: Role::Consumer,
        site: Site::Call {
            path: path.into(),
            index,
        },
        contract: read.contract,
        via: read.via,
        method: read.method,
        fragments: read.fragments,
        shape,
    }
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

/// The HTTP methods as lower-case names: Express and FastAPI register routes with them, and
/// Python clients send requests with them.
const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch", "options", "head"];

/// A call by its file and index: what creates an app or a router, or a URLconf entry.
type CallAt<'a> = (&'a str, usize);

/// For each router, the routers it is mounted on and the segment the mount puts before its
/// routes, `None` when it cannot be read.
type Mounts<'a, N = CallAt<'a>> = BTreeMap<N, Vec<(N, Option<String>)>>;

/// `prefix` and `path` joined with one `/`.
fn join(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    match path.starts_with('/') {
        true => format!("{prefix}{path}"),
        false => format!("{prefix}/{path}"),
    }
}

/// How many mounts deep a router's prefixes are read.
// ponytail: a router nested deeper than this is served nowhere; the stack is not Meada's to spend.
const PREFIX_DEPTH: usize = 64;

/// How many mounts are walked to give the prefixes of one router.
// ponytail: a router whose mounts fan out past this (one router nested twice, 40 levels up) is
// served nowhere rather than under a part of its prefixes; memoizing the walk would lift it.
const PREFIX_WORK: usize = 4096;

/// Every prefix the routes of `router` take, sorted: the segment of each mount under every
/// prefix of the router it is mounted on, or its `root` prefix when nothing mounts it (`None`
/// when such a router is not served). A prefix that cannot be read adds none, and a router
/// mounted too deep or through too many mounts (`PREFIX_DEPTH`, `PREFIX_WORK`) has none.
fn prefixes<'a, N: Copy + Ord>(
    router: N,
    mounts: &Mounts<'a, N>,
    root: &dyn Fn(N) -> Option<String>,
    seen: &mut Vec<N>,
) -> Vec<String> {
    let mut work = PREFIX_WORK;
    prefixes_within(router, mounts, root, seen, &mut work).unwrap_or_default()
}

/// `prefixes`, `None` when `work` mounts do not cover it.
fn prefixes_within<'a, N: Copy + Ord>(
    router: N,
    mounts: &Mounts<'a, N>,
    root: &dyn Fn(N) -> Option<String>,
    seen: &mut Vec<N>,
    work: &mut usize,
) -> Option<Vec<String>> {
    let Some(parents) = mounts.get(&router) else {
        return Some(root(router).into_iter().collect());
    };
    // A router mounted inside itself, through any depth, adds nothing.
    if seen.contains(&router) {
        return Some(Vec::new());
    }
    if seen.len() >= PREFIX_DEPTH {
        return None;
    }
    seen.push(router);
    let mut out = Vec::new();
    for (parent, segment) in parents {
        *work = work.checked_sub(1)?;
        let Some(segment) = segment else {
            continue;
        };
        for above in prefixes_within(*parent, mounts, root, seen, work)? {
            out.push(join(&above, segment));
        }
    }
    seen.pop();
    out.sort();
    out.dedup();
    Some(out)
}

/// A provider's prefix or path as written: `None` when it is not a string, or when a part of it
/// cannot be read. A path parameter written in the literal (`{id}`, `<int:id>`) stays one.
// ponytail: a prefix or path that depends on a parameter is not read through the callers, as
// `Code::expand` reads a client wrapper's URL; it yields no route.
fn path_literal(value: Value) -> Option<String> {
    match value {
        Value::Str(parts) => literal(&parts),
        _ => None,
    }
}

/// Where the route `method` `full` of a handler written in `scope` of `path` starts a trace: a
/// closure is an inline handler named for its route, a function of the repository the function,
/// and anything else the file.
fn handler_site<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    handler: Option<&'a Value>,
    method: &str,
    full: &str,
) -> Site {
    match handler {
        Some(Value::Function(def)) => Site::Inline {
            path: path.into(),
            def: def.clone(),
            name: format!("{method} {full}"),
        },
        Some(Value::Ref(name)) => match code.definition(path, scope, name) {
            Some(t) => Site::Def {
                path: t.path.into(),
                name: t.name.into(),
            },
            None => Site::File(path.into()),
        },
        _ => Site::File(path.into()),
    }
}

impl ContractKind for Http {
    fn prefix(&self) -> &'static str {
        "http"
    }

    fn extract(&self, code: &Code) -> Vec<Found> {
        let mut found = Vec::new();
        // One reader of each shape source for the whole extraction: each budget per file once.
        let mut literals = Literals::default();
        found.extend(ts::consumers(code, &mut literals));
        found.extend(ts::providers(code, &mut literals));
        found.extend(python::consumers(code));
        found.extend(rust::providers(code));
        found.extend(go::providers(code));
        found.extend(go::consumers(code));
        found.extend(python::providers(code));
        found
    }

    /// Adds the routes of the repository's documents (spec #21), then the gateway paths and
    /// hosts. A document route with the contract of a code route merges into it: the code
    /// keeps the handler. Any other is a provider of its own, without a handler, at
    /// `<repository>::<document>::<METHOD> <path>`; a route an earlier document (by path)
    /// already listed adds nothing of its own.
    ///
    /// Each document listing a route gives it its own gateway forms: one per `servers` path of
    /// the document (the server path as the prefix, with the `[serve]` strip), or the `[serve]`
    /// form when the document has no server path. A code route keeps its `[serve]` form and
    /// adds those of the documents listing it. The hosts of every HTTP provider are the
    /// `[serve]` hosts and the hosts of every document's servers.
    fn declare(&self, repo: &Repository, export: &mut IndexExport) {
        let serve = &repo.config.serve;
        let code: HashSet<String> = (export.providers.iter())
            .map(|p| p.contract.clone())
            .collect();
        let mut listed = code.clone();
        let mut documented = Vec::new();
        let mut forms: HashMap<String, Vec<Gateway>> = HashMap::new();
        // The first document by path listing a route gives its shape, merged or not.
        let mut shapes = HashMap::new();
        for doc in &export.documents {
            let prefixes = (doc.summary.servers.iter())
                .map(|s| s.path.as_str())
                .filter(|p| !p.is_empty());
            let prefixes: Vec<&str> = prefixes.collect();
            for route in &doc.summary.routes {
                let contract = route.contract();
                shapes.entry(contract.clone()).or_insert(&route.shape);
                let found: Vec<(String, String)> = match prefixes.is_empty() {
                    true => serve.gateway_path(&route.path).into_iter().collect(),
                    false => (prefixes.iter())
                        .filter_map(|p| gateway_path(Some(p), serve.strip.as_deref(), &route.path))
                        .collect(),
                };
                let own = forms.entry(contract.clone()).or_default();
                for (path, reason) in found {
                    let form = Gateway { path, reason };
                    if !own.contains(&form) {
                        own.push(form);
                    }
                }
                if listed.insert(contract.clone()) {
                    let site = format!("{}::{}::{}", repo.identity, doc.path, route.label());
                    documented.push(Provider {
                        contract,
                        site,
                        ..Provider::default()
                    });
                }
            }
        }
        export.providers.extend(documented);
        index::sort_providers(&mut export.providers);
        let mut hosts: Vec<String> = serve.hosts.iter().map(|h| h.to_lowercase()).collect();
        let servers = export.documents.iter().flat_map(|d| &d.summary.servers);
        hosts.extend(servers.filter_map(|s| s.host.clone()));
        hosts.sort();
        hosts.dedup();
        for p in &mut export.providers {
            let Some((_, path)) = route(&p.contract) else {
                continue;
            };
            // A document's shape wins over the one the code gives.
            if let Some(Some(shape)) = shapes.get(&p.contract) {
                p.shape = Some(shape.clone());
            }
            let served = serve
                .gateway_path(path)
                .map(|(path, reason)| Gateway { path, reason });
            let from_documents = forms.get(&p.contract).cloned().unwrap_or_default();
            // The `[serve]` form first: a code route has it, a documented one when a document
            // without server paths lists it.
            let served =
                served.filter(|s| code.contains(&p.contract) || from_documents.contains(s));
            p.gateway = served.as_ref().map(|s| s.path.clone());
            p.gateways = served.into_iter().collect();
            for form in from_documents {
                if !p.gateways.contains(&form) {
                    p.gateways.push(form);
                }
            }
            p.hosts = hosts.clone();
        }
    }

    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
        _exports: &[IndexExport],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        let via = consumer.via.as_deref();
        // A route serves a consumer by its code path, or by its gateway path with the reason
        // for the rewrite. Within one repository a code path match wins over gateway path
        // matches; across repositories both are candidates.
        let mut serving: Vec<_> = (providers.iter())
            .filter_map(|&(m, p)| {
                if fits(&p.contract, contract) {
                    return Some((m, p, None));
                }
                let (_, reason) = gateway_contracts(p).find(|(g, _)| fits(g, contract))?;
                Some((m, p, Some(reason.to_string())))
            })
            .collect();
        let direct: Vec<&str> = (serving.iter())
            .filter(|(.., gateway)| gateway.is_none())
            .map(|&(m, ..)| m.identity.as_str())
            .collect();
        serving
            .retain(|(m, _, gateway)| gateway.is_none() || !direct.contains(&m.identity.as_str()));
        let serving = serving.into_iter();
        let var = via.and_then(|v| v.strip_prefix("env:"));
        let peers: Vec<&str> = var
            .map(|var| from.config.peers_reading(var).collect())
            .unwrap_or_default();
        let (fits, level, reason): (Vec<_>, _, _) =
            if let Some(host) = via.and_then(|v| v.strip_prefix("host:")) {
                let fits = serving
                    .filter(|(_, p, _)| p.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)))
                    .collect();
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
                let n = fits.len();
                // Through a gateway the link is inferred, whatever the consumer side gave, and
                // its reason is the rewrite.
                let mut reason = match (gateway, ambiguous) {
                    (Some(rewrite), true) => format!("{n} providers fit; {rewrite}"),
                    (Some(rewrite), false) => rewrite.clone(),
                    (None, true) => format!("{n} providers fit; {reason} {}", repo.identity),
                    (None, false) => format!("{reason} {}", repo.identity),
                };
                if route(&provider.contract).is_some_and(|(method, _)| method == ANY) {
                    reason.push_str("; any method");
                }
                Link {
                    consumer: consumer.site.clone(),
                    provider: provider.site.clone(),
                    contract: contract.clone(),
                    level: match (ambiguous, gateway) {
                        (true, _) => Level::Ambiguous,
                        (_, Some(_)) => Level::Inferred,
                        _ => level,
                    },
                    reason,
                    by: None,
                    p: None,
                    drift: None,
                }
            })
            .collect()
    }

    /// A route is labelled by its method and path.
    fn label(&self, contract: &str) -> String {
        match route(contract) {
            Some((method, path)) => format!("{method} {path}"),
            None => shown(contract),
        }
    }

    fn serves(&self, provider: &Provider, contract: &str) -> bool {
        serves(provider, contract)
    }

    /// A route fits a dynamic URL by its code path or by a gateway path.
    fn candidate(&self, consumer: &Consumer, provider: &Provider) -> bool {
        let fits = |c: &str| route_fits(c, consumer.method.as_deref(), &consumer.fragments);
        fits(&provider.contract) || gateway_contracts(provider).any(|(g, _)| fits(&g))
    }

    /// A route is a whole contract (`GET /users` names one too), or a path on any method.
    fn starts<'a>(&self, from: &str, providers: &[(&'a str, &'a str)]) -> Vec<&'a str> {
        let (contract, path) = if route(from).is_some() {
            (Some(from.to_string()), None)
        } else if let Some((method, path)) = from.split_once(' ')
            && path.starts_with('/')
            && method.chars().all(|c| c.is_ascii_alphabetic())
        {
            (Some(self::contract(method, path)), None)
        } else if from.starts_with('/') {
            (None, Some(format!("::{}", normalize_path(from))))
        } else {
            (None, None)
        };
        (providers.iter())
            .filter(|(c, _)| {
                contract.as_deref().is_some_and(|w| fits(c, w))
                    || path.as_deref().is_some_and(|p| c.ends_with(p))
            })
            .map(|&(_, site)| site)
            .collect()
    }

    /// A provider of a route is a door.
    fn entry(&self, role: Role, contract: &str) -> Option<EntryKind> {
        (role == Role::Provider && route(contract).is_some()).then_some(EntryKind::Route)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CallAt, Http, Mounts, PREFIX_DEPTH, fragments, normalize_path, prefixes, route_fits,
    };
    use crate::contract::{ContractKind, Role};
    use crate::entry::EntryKind;
    use crate::index::{Consumer, Gateway, Provider};

    fn provider(contract: &str) -> Provider {
        Provider {
            contract: contract.into(),
            ..Provider::default()
        }
    }

    #[test]
    fn a_provider_serves_its_route_any_method_and_its_gateway_path_and_nothing_else() {
        let get = provider("http::GET::/users");
        assert!(Http.serves(&get, "http::GET::/users"));
        assert!(!Http.serves(&get, "http::POST::/users"));
        let any = provider("http::ANY::/users");
        assert!(Http.serves(&any, "http::DELETE::/users"));
        assert!(!Http.serves(&any, "http::DELETE::/other"));
        let gateway = Provider {
            gateways: vec![Gateway {
                path: "/shop/users".into(),
                reason: "gateway prefix /shop".into(),
            }],
            ..get
        };
        assert!(Http.serves(&gateway, "http::GET::/shop/users"));
    }

    #[test]
    fn a_route_is_a_candidate_for_a_dynamic_url_through_its_gateway_path() {
        let consumer = Consumer {
            contract: None,
            site: "r::a.ts::f::g#0".into(),
            via: None,
            shape: None,
            method: Some("GET".into()),
            fragments: vec!["/shop/".into()],
            hops: Vec::new(),
        };
        let direct = provider("http::GET::/shop/users");
        let gateway = Provider {
            gateways: vec![Gateway {
                path: "/shop/users".into(),
                reason: "gateway prefix /shop".into(),
            }],
            ..provider("http::GET::/users")
        };
        assert!(Http.candidate(&consumer, &direct));
        assert!(Http.candidate(&consumer, &gateway));
        assert!(!Http.candidate(&consumer, &provider("http::GET::/users")));
        assert!(!Http.candidate(&consumer, &provider("resource::db::orders")));
    }

    #[test]
    fn a_route_is_labelled_by_its_method_and_path_and_anything_else_as_it_is_shown() {
        assert_eq!(Http.label("http::GET::/users/{}"), "GET /users/{}");
        assert_eq!(Http.label("resource::db::orders"), "resource::db::orders");
    }

    /// `n` routers, each mounted on the next under `/{i}`, as many times as `ways`; the last
    /// is the app.
    fn nested(n: usize, ways: usize) -> (Mounts<'static>, CallAt<'static>) {
        let mut mounts = Mounts::new();
        for i in 0..n {
            let parents = (0..ways).map(|w| (("f.rs", i + 1), Some(format!("/{i}{w}"))));
            mounts.insert(("f.rs", i), parents.collect());
        }
        (mounts, ("f.rs", 0))
    }

    #[test]
    fn a_router_nested_within_the_limits_has_every_prefix() {
        let (mounts, router) = nested(PREFIX_DEPTH - 1, 1);
        let found = prefixes(router, &mounts, &|_| Some(String::new()), &mut Vec::new());
        assert_eq!(found.len(), 1);
        let (mounts, router) = nested(10, 2);
        let found = prefixes(router, &mounts, &|_| Some(String::new()), &mut Vec::new());
        assert_eq!(found.len(), 1 << 10);
    }

    #[test]
    fn a_router_nested_past_the_depth_or_the_work_limit_is_served_nowhere() {
        let root = |_: CallAt| Some(String::new());
        let (mounts, router) = nested(PREFIX_DEPTH + 1, 1);
        assert!(prefixes(router, &mounts, &root, &mut Vec::new()).is_empty());
        let (mounts, router) = nested(40, 2);
        assert!(prefixes(router, &mounts, &root, &mut Vec::new()).is_empty());
    }

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
            ("/api/users/{user_id}", "/api/users/{}"),
            ("/api/users/<id>", "/api/users/{}"),
            ("/api/users/<int:id>/posts", "/api/users/{}/posts"),
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
            // A route of every method fits any method.
            ("http::ANY::/api/users", Some("DELETE"), &[][..], true),
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

    #[test]
    fn a_trace_start_names_the_providers_of_a_route_in_three_forms_and_of_nothing_else() {
        let providers = [
            ("http::GET::/users/{}", "r::a.ts::get"),
            ("http::POST::/users/{}", "r::a.ts::post"),
            ("http::GET::/other", "r::a.ts::other"),
            ("channel::kafka::users", "r::a.ts::handle"),
        ];
        let starts = |from| Http.starts(from, &providers);
        assert_eq!(starts("http::GET::/users/{}"), ["r::a.ts::get"]);
        assert_eq!(starts("GET /users/{}"), ["r::a.ts::get"]);
        assert_eq!(starts("/users/{}"), ["r::a.ts::get", "r::a.ts::post"]);
        assert!(starts("get").is_empty());
        assert!(starts("GET users").is_empty());
    }

    #[test]
    fn a_provider_of_a_route_is_an_entry_point_and_a_consumer_is_not() {
        let route = "http::GET::/users";
        assert_eq!(Http.entry(Role::Provider, route), Some(EntryKind::Route));
        assert_eq!(Http.entry(Role::Consumer, route), None);
        assert_eq!(Http.entry(Role::Provider, "http::nonsense"), None);
    }
}
