use std::collections::{BTreeMap, HashMap, HashSet};

use crate::config::gateway_path;
use crate::contract::{ContractKind, Found, Lead, Level, Link, Role, Search, Site, View, shown};
use crate::entry::EntryKind;
use crate::index::{self, Consumer, Export as IndexExport, Gateway, Indexed, Provider};
use crate::lang::{Depends, Limit, Part, Value, literal, property};
use crate::literals::Literals;
use crate::msg::Msg;
use crate::resolve::Code;
use crate::shape::{self, Shape};
use crate::workspace::Repository;

mod csharp;
mod go;
mod iac;
mod infrastructure;
mod java;
mod join;
mod kotlin;
mod python;
mod rust;
mod swift;
mod ts;
pub(crate) use infrastructure::Prepared;
pub(crate) use ts::Config as AppConfig;
#[cfg(test)]
pub(crate) use ts::app_config_probe;
pub(crate) use ts::{INCLUDE_DEPTH, INCLUDES, IncludeReason, config_inputs};

pub use meada_core::http::{PARAM, normalize_path};

const ANY: &str = "ANY";

pub struct Http;

pub fn contract(method: &str, path: &str) -> String {
    format!("http::{}::{}", method.to_uppercase(), normalize_path(path))
}

const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"];

#[derive(Default)]
struct Read {
    contract: Option<String>,
    via: Option<String>,
    method: Option<String>,
    fragments: Vec<String>,
    lead: Option<Lead>,
    bare_env: bool,
}

fn fetch(args: &[Value]) -> Option<Read> {
    let mut read = locate(args)?;
    if read.contract.is_none() {
        read.method = self::method(args.get(1))
            .ok()
            .flatten()
            .map(|m| m.to_uppercase());
        if let Some(Value::Str(parts)) = args.first() {
            read.fragments = fragments(parts);
        }
    }
    Some(read)
}

fn fetches(args: &[Value]) -> Option<Vec<Read>> {
    let Some(Value::Str(parts)) = args.first() else {
        return Some(vec![fetch(args)?]);
    };
    let Some((Part::EnvOr(var, default), tail)) = parts.split_first() else {
        return Some(vec![fetch(args)?]);
    };
    let with = |mut head: Vec<Part>| {
        head.extend(tail.iter().cloned());
        let mut args = args.to_vec();
        args[0] = Value::Str(head);
        args
    };
    let mut through = fetch(&with(vec![Part::Env(var.clone())]))?;
    let mut default = default.clone();
    if let (Some(Part::Text(end)), Some(Part::Text(next))) = (default.last_mut(), tail.first())
        && next.starts_with('/')
    {
        *end = end.trim_end_matches('/').to_string();
    }
    let by_default =
        fetch(&with(default)).filter(|d| d.via.as_deref().is_some_and(|v| v.starts_with("host:")));
    Some(match by_default {
        Some(host) => {
            let via = (through.via.as_deref()).zip(host.via.as_deref());
            through.via = via.map(|(env, host)| format!("{env}|{host}"));
            let mut host = host;
            host.via = host.via.map(|v| format!("{v}{DEFAULT}"));
            vec![through, host]
        }
        None => vec![through],
    })
}

const CLIENT_VIA: &str = "client:";

const PROXY_VIA: &str = " through proxy endpoint ";

const DEFAULT: &str = "|default";

fn keep_applicable(exports: &mut [IndexExport]) {
    let served: HashSet<String> = (exports.iter().flat_map(|e| &e.providers))
        .flat_map(|p| &p.hosts)
        .map(|h| h.to_lowercase())
        .collect();
    let served = |host: &str| {
        host.strip_prefix("host:")
            .is_some_and(|h| served.contains(h))
    };
    for export in exports {
        export.consumers.retain_mut(|c| {
            let Some(via) = c.via.as_deref() else {
                return true;
            };
            let (keep, via) = match (via.strip_suffix(DEFAULT), via.split_once('|')) {
                (Some(host), _) => (served(host), host.to_string()),
                (None, Some((env, host))) => (!served(host), env.to_string()),
                (None, None) => return true,
            };
            c.via = Some(via);
            keep
        });
        (export.consumers)
            .sort_by(|a, b| (&a.site, &a.contract, &a.via).cmp(&(&b.site, &b.contract, &b.via)));
    }
}

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
    let path = match path.ends_with(GAP) {
        true => path,
        false => path.trim_end_matches('/'),
    };
    (path.split(GAP).filter(|f| !f.is_empty()))
        .map(str::to_string)
        .collect()
}

// ponytail: only the conventions the ticket names (#620), not `*Test.java` or `*Tests.cs`; add
// one when a repository needs it, or let it set `test_routes`.
pub(crate) fn is_test_file(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let Some((name, dirs)) = parts.split_last() else {
        return false;
    };
    let named = name.ends_with("_test.go")
        || name.contains(".test.")
        || name.contains(".spec.")
        || (name.starts_with("test_") && name.ends_with(".py"))
        || name.ends_with("_test.py");
    named
        || dirs.iter().any(|d| matches!(*d, "tests" | "__tests__"))
        || dirs.windows(2).any(|w| w == ["src", "test"])
}

fn drop_test_providers(repo: &Repository, export: &mut IndexExport) {
    let in_test = |site: &str| {
        let own = site
            .strip_prefix(repo.identity.as_str())
            .and_then(|s| s.strip_prefix("::"));
        own.and_then(|s| s.split("::").next())
            .is_some_and(is_test_file)
    };
    let mut dropped = HashSet::new();
    export.providers.retain(|p| {
        let keep = route(&p.contract).is_none() || !in_test(&p.site);
        if !keep {
            dropped.insert(format!("{}::{}", p.site, p.contract));
        }
        keep
    });
    export
        .entry_points
        .retain(|e| !dropped.contains(&e.identity));
}

fn route(contract: &str) -> Option<(&str, &str)> {
    contract.strip_prefix("http::")?.split_once("::")
}

fn gateway_contracts(provider: &Provider) -> impl Iterator<Item = (String, &str)> {
    let method = route(&provider.contract).map(|(method, _)| method);
    (provider.gateways.iter())
        .filter_map(move |g| Some((contract(method?, &g.path), g.reason.as_str())))
}

fn fits(provided: &str, wanted: &str) -> bool {
    provided == wanted
        || matches!((route(provided), route(wanted)), (Some((ANY, p)), Some((_, w))) if p == w)
}

fn serves(provider: &Provider, contract: &str) -> bool {
    fits(&provider.contract, contract)
        || gateway_contracts(provider).any(|(g, _)| fits(&g, contract))
}

fn drift(links: &mut [Link], exports: &[IndexExport]) {
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

fn route_path<'a>(contract: &'a str, method: Option<&str>) -> Option<&'a str> {
    let (route_method, path) = route(contract)?;
    method
        .is_none_or(|m| m == route_method || route_method == ANY)
        .then_some(path)
}

fn route_fits(contract: &str, method: Option<&str>, fragments: &[String]) -> bool {
    let Some(mut path) = route_path(contract, method) else {
        return false;
    };
    for fragment in fragments {
        match path.find(fragment.as_str()) {
            Some(at) => path = &path[at + fragment.len()..],
            None => return false,
        }
    }
    true
}

fn tail_fits(contract: &str, method: Option<&str>, known: &str) -> bool {
    let Some(path) = route_path(contract, method) else {
        return false;
    };
    known == "/" || path.match_indices('/').any(|(at, _)| path[at..] == *known)
}

fn locate(args: &[Value]) -> Option<Read> {
    let method = method(args.get(1)).ok()?;
    let parts = match args.first() {
        Some(Value::Str(parts)) if parts.iter().any(|p| p.parameter().is_some()) => return None,
        Some(Value::Str(parts)) => parts,
        Some(Value::Ref(_)) => return None,
        _ => return Some(Read::default()),
    };
    if parts.iter().any(|p| {
        matches!(
            p,
            Part::StripStart | Part::StripEnd(_) | Part::RecordValues { .. }
        )
    }) {
        return Some(Read::default());
    }
    let end = parts.iter().rposition(|p| {
        !matches!(p, Part::QuerySuffix) && !matches!(p, Part::Text(t) if t.is_empty())
    });
    let path_parts = end.map_or(&[][..], |end| &parts[..=end]);
    if path_parts.is_empty() || path_parts.iter().any(|p| matches!(p, Part::QuerySuffix)) {
        return Some(Read::default());
    }
    let (via, path, lead) = match path_parts.split_first() {
        Some((Part::Env(var) | Part::EnvOr(var, _), rest)) => {
            let via = Some(format!("env:{var}"));
            let path = render(rest);
            if path.is_empty() || path.starts_with(['?', '#']) {
                return Some(Read {
                    via,
                    bare_env: true,
                    ..Read::default()
                });
            }
            (via, Some(path), None)
        }
        _ => {
            let url = render(path_parts);
            match url.split_once("://") {
                Some((_, rest)) => {
                    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                    let host = &rest[..end];
                    if host.contains(PARAM) || host.is_empty() {
                        (None, None, None)
                    } else {
                        let via = format!("host:{}", host.to_lowercase());
                        (Some(via), Some(rest[end..].to_string()), None)
                    }
                }
                None if !matches!(parts.first(), Some(p) if unknown(p)) => (None, Some(url), None),
                None => {
                    let rest =
                        render(&path_parts[path_parts.iter().take_while(|p| unknown(p)).count()..]);
                    let lead = rest.is_empty() || rest.starts_with(['/', '?', '#']);
                    let lead = lead.then(|| Lead {
                        limit: match &path_parts[0] {
                            Part::Limit(limit) => limit.clone(),
                            _ => Limit::Other,
                        },
                        path: normalize_path(&rest),
                    });
                    (None, None, lead)
                }
            }
        }
    };
    let path = path.filter(|p| p.is_empty() || p.starts_with(['/', '?', '#']));
    Some(Read {
        contract: method
            .zip(path)
            .map(|(method, path)| contract(&method, &path)),
        via,
        lead,
        ..Read::default()
    })
}

fn unknown(part: &Part) -> bool {
    matches!(
        part,
        Part::Hole | Part::Read { .. } | Part::Invoke { .. } | Part::Limit(_)
    )
}

pub(in crate::http) fn client_base() -> Vec<Part> {
    vec![Part::Limit(Limit::ClientBase)]
}

pub(in crate::http) fn rooted_over_unread(base: &[Part], url: &Value) -> bool {
    let rooted = |t: &str| t.starts_with('/') && !t.starts_with("//");
    matches!(url, Value::Str(parts) if matches!(parts.first(), Some(Part::Text(t)) if rooted(t)))
        && base.first().is_some_and(unknown)
        && !(base.iter()).any(|p| matches!(p, Part::Text(t) if t.contains("://")))
}

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
        Value::Str(parts) if parts.iter().any(|p| p.parameter().is_some()) => Err(Depends),
        Value::Str(parts) => Ok(literal(parts)),
        Value::Ref(_) => Err(Depends),
        Value::Undefined => Ok(Some("GET".into())),
        _ => Ok(None),
    }
}

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

fn provider(site: Site, contract: String) -> Found {
    Found {
        identity: None,
        role: Role::Provider,
        site,
        contract: Some(contract),
        via: None,
        method: None,
        fragments: Vec::new(),
        shape: None,
        lead: None,
        cap: None,
        client: None,
    }
}

fn consumer(path: &str, index: usize, read: Read, shape: Option<Shape>) -> Found {
    let site = Site::Call {
        path: path.into(),
        index,
    };
    consumer_at(site, read, shape)
}

fn consumer_at(site: Site, read: Read, shape: Option<Shape>) -> Found {
    Found {
        identity: read
            .bare_env
            .then(|| crate::contract::ReceiverIdentity::HttpBase {
                strips: Vec::new(),
                suffix: String::new(),
            }),
        role: Role::Consumer,
        site,
        contract: read.contract,
        via: read.via,
        method: read.method,
        fragments: read.fragments,
        lead: read.lead,
        shape,
        cap: None,
        client: None,
    }
}

fn render(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => t.as_str(),
            Part::Env(_)
            | Part::EnvOr(..)
            | Part::Read { .. }
            | Part::Invoke { .. }
            | Part::EnvKey { .. }
            | Part::RecordStart { .. }
            | Part::RecordEnd
            | Part::RecordValues { .. }
            | Part::StripStart
            | Part::StripEnd(_)
            | Part::QuerySuffix
            | Part::Ref(_)
            | Part::Limit(_)
            | Part::Hole => PARAM,
        })
        .collect()
}

const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch", "options", "head"];

struct Library {
    modules: &'static [&'static str],
    clients: &'static [&'static str],
}

type CallAt<'a> = (&'a str, usize);

type Mounts<'a, N = CallAt<'a>> = BTreeMap<N, Vec<(N, Option<String>)>>;

fn join(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    match path.starts_with('/') {
        true => format!("{prefix}{path}"),
        false => format!("{prefix}/{path}"),
    }
}

fn balanced(path: &str) -> bool {
    path.split('/').all(|segment| {
        let mut depth = 0usize;
        for c in segment.chars() {
            match c {
                '{' => depth += 1,
                '}' => match depth.checked_sub(1) {
                    Some(d) => depth = d,
                    None => return false,
                },
                _ => {}
            }
        }
        depth == 0
    })
}

// ponytail: a router nested deeper than this is served nowhere; the stack is not Meada's to spend.
const PREFIX_DEPTH: usize = 64;

// ponytail: a router whose mounts fan out past this (one router nested twice, 40 levels up) is
// served nowhere rather than under a part of its prefixes; memoizing the walk would lift it.
const PREFIX_WORK: usize = 4096;

fn prefixes<'a, N: Copy + Ord>(
    router: N,
    mounts: &Mounts<'a, N>,
    root: &dyn Fn(N) -> Option<String>,
    seen: &mut Vec<N>,
) -> Vec<String> {
    let mut work = PREFIX_WORK;
    prefixes_within(router, mounts, root, seen, &mut work).unwrap_or_default()
}

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

// ponytail: a prefix or path that depends on a parameter is not read through the callers, as
// `Code::expand` reads a client wrapper's URL; it yields no route.
fn path_literal(value: Value) -> Option<String> {
    match value {
        Value::Str(parts) => literal(&parts),
        _ => None,
    }
}

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

    fn search_word(&self) -> Search {
        Search::Filter("route")
    }

    fn config_file(&self, path: &str) -> bool {
        iac::is_file(path)
    }

    fn config_summary(
        &self,
        path: &str,
        src: &[u8],
        root: &std::path::Path,
        facts: &crate::contract::ConfigFacts<'_>,
    ) -> serde_json::Value {
        iac::summary(path, src, root, facts)
    }

    fn symbols_read(&self, repo: &Repository, export: &IndexExport) -> Option<Vec<String>> {
        Some(iac::symbols(repo, export))
    }

    fn read_declarations(
        &self,
        _: &Repository,
        db: &rusqlite::Connection,
        export: &mut IndexExport,
    ) -> crate::Result<()> {
        iac::read(db, export)
    }

    fn exports_repository_links(&self) -> bool {
        true
    }

    fn view(&self, contract: &str) -> Option<View> {
        route(contract).map(|_| View {
            provider: "provider",
            consumer: None,
            carried: "response",
        })
    }

    fn explain_inferred(
        &self,
        consumer: &Consumer,
        from: &Repository,
        link: &Link,
        exports: &[IndexExport],
    ) -> bool {
        let via = consumer.via.as_deref().unwrap_or_default();
        let via = via.split_once(PROXY_VIA).map_or(via, |(via, _)| via);
        via.strip_prefix("env:")
            .is_some_and(|var| from.config.peers_reading(var).next().is_some())
            || exports
                .iter()
                .filter_map(|e| e.http.as_ref())
                .any(|p| p.explains(link))
    }

    fn extract(&self, code: &Code, indexed: &Indexed) -> Vec<Found> {
        code.with_returned_strings(|| {
            let mut found = Vec::new();
            let mut literals = Literals::default();
            let config = ts::Config::new(indexed);
            let mut consumers = ts::consumers(code, &config, &mut literals);
            let (providers, mounted) = ts::providers(code, &mut literals);
            ts::name(&mut consumers, &mounted);
            found.extend(consumers);
            found.extend(providers);
            found.extend(ts::unfollowed(code, &config));
            found.extend(python::consumers(code));
            found.extend(rust::providers(code));
            found.extend(go::providers(code));
            found.extend(go::consumers(code));
            found.extend(python::providers(code));
            found.extend(java::providers(code));
            found.extend(csharp::providers(code));
            found.extend(csharp::minimal_providers(code));
            found.extend(kotlin::providers(code));
            found.extend(swift::providers(code));
            found.extend(kotlin::consumers(code, indexed));
            found.extend(java::consumers(code, indexed));
            found.extend(csharp::consumers(code, indexed));
            found.extend(swift::consumers(code));
            found
        })
    }

    fn workspace_step(&self, exports: &mut [IndexExport], _: &[Repository]) {
        keep_applicable(exports);
    }

    fn workspace_ready(&self, exports: &mut [IndexExport], repositories: &[Repository]) {
        infrastructure::prepare(exports, repositories);
    }

    fn workspace_links(
        &self,
        exports: &[IndexExport],
        _: &[(&Repository, &Provider)],
    ) -> Option<Vec<Link>> {
        exports.iter().any(|e| e.http.is_some()).then(|| {
            exports
                .iter()
                .filter_map(|e| e.http.as_ref())
                .flat_map(|p| p.links.iter().cloned())
                .collect()
        })
    }

    fn declare(&self, repo: &Repository, export: &mut IndexExport) {
        if !repo.config.test_routes {
            drop_test_providers(repo, export);
        }
        iac::declare(repo, export);
        let serve = &repo.config.serve;
        let code: HashSet<String> = (export.providers.iter())
            .map(|p| p.contract.clone())
            .collect();
        let mut listed = code.clone();
        let mut documented = Vec::new();
        let mut forms: HashMap<String, Vec<Gateway>> = HashMap::new();
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
            if let Some(Some(shape)) = shapes.get(&p.contract) {
                p.shape = Some(shape.clone());
            }
            let served = serve
                .gateway_path(path)
                .map(|(path, reason)| Gateway { path, reason });
            let from_documents = forms.get(&p.contract).cloned().unwrap_or_default();
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
        exports: &[IndexExport],
    ) -> Vec<Link> {
        let Some(contract) = &consumer.contract else {
            return Vec::new();
        };
        let (via, endpoint) = match consumer.via.as_deref().map(|v| v.split_once(PROXY_VIA)) {
            Some(Some((via, endpoint))) => (Some(via), Some(endpoint)),
            _ => (consumer.via.as_deref(), None),
        };
        let through = endpoint.map(|e| format!(" through the proxy endpoint {e}"));
        let through = through.as_deref().unwrap_or_default();
        let var = via.and_then(|v| v.strip_prefix("env:"));
        let dir = var.and_then(|var| from.config.declared_dir(var));
        let mut serving: Vec<_> = (providers.iter())
            .filter(|(m, p)| {
                dir.is_none_or(|dir| {
                    m.identity == from.identity && under(&p.site, &m.identity, dir)
                })
            })
            .filter_map(|&(m, p)| {
                if fits(&p.contract, contract) {
                    return Some((m, p, None));
                }
                let (_, reason) = gateway_contracts(p).find(|(g, _)| fits(g, contract))?;
                Some((m, p, Some(reason.to_string())))
            })
            .collect();
        if via.is_none() {
            let manifests = (exports.iter())
                .find(|e| e.repository == from.identity)
                .map_or(&[][..], |e| &e.manifests[..]);
            let package = |site: &str, identity: &str| {
                let owner = crate::package::owner_of(manifests, path_of(site, identity)?)?;
                Some(owner.path.as_str())
            };
            if let Some(own) = package(&consumer.site, &from.identity) {
                let mine = |(m, p, _): &(&Repository, &Provider, Option<String>)| {
                    m.identity == from.identity && package(&p.site, &m.identity) == Some(own)
                };
                if serving.iter().any(mine) {
                    serving.retain(mine);
                }
            }
        }
        let direct: Vec<&str> = (serving.iter())
            .filter(|(.., gateway)| gateway.is_none())
            .map(|&(m, ..)| m.identity.as_str())
            .collect();
        serving
            .retain(|(m, _, gateway)| gateway.is_none() || !direct.contains(&m.identity.as_str()));
        let serving = serving.into_iter();
        let peers: Vec<&str> = var
            .map(|var| from.config.peers_reading(var).collect())
            .unwrap_or_default();
        let (fits, level, reason): (Vec<_>, _, _) = if let Some(host) =
            via.and_then(|v| v.strip_prefix("host:"))
        {
            let fits = serving
                .filter(|(_, p, _)| p.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)))
                .collect();
            (
                fits,
                Level::Exact,
                format!("host {host}{through} is served by"),
            )
        } else if let (Some(var), Some(dir)) = (var, dir) {
            (
                serving.collect(),
                Level::Declared,
                format!(
                    "base URL from {var}{through}, declared for {dir}/ in meada.toml; served by"
                ),
            )
        } else if let (Some(var), false) = (var, peers.is_empty()) {
            let fits = serving
                .filter(|(m, ..)| peers.contains(&m.identity.as_str()))
                .collect();
            (
                fits,
                Level::Inferred,
                format!("base URL from {var}{through}, a base_url_env of peer"),
            )
        } else {
            let fits: Vec<_> = serving.collect();
            let mut repos: Vec<&str> = fits.iter().map(|(m, ..)| m.identity.as_str()).collect();
            repos.sort();
            repos.dedup();
            let client = via.and_then(|v| v.strip_prefix(CLIENT_VIA));
            let why = match (var, client) {
                (Some(var), _) => format!("base URL from {var}{through}, no peer's base_url_env"),
                (None, Some(client)) => format!("host unresolved ({client})"),
                (None, None) => "host unresolved".to_string(),
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
                let mut reason = match (gateway, ambiguous) {
                    (Some(rewrite), true) => format!("{n} providers fit; {rewrite}"),
                    (Some(rewrite), false) => rewrite.clone(),
                    (None, true) => format!("{n} providers fit; {reason} {}", repo.identity),
                    (None, false) => format!("{reason} {}", repo.identity),
                };
                if route(&provider.contract).is_some_and(|(method, _)| method == ANY) {
                    reason.push_str("; any method");
                }
                if let Some(evidence) = &provider.evidence {
                    reason.push_str("; ");
                    reason.push_str(evidence);
                }
                let level = match (ambiguous, gateway) {
                    (true, _) => Level::Ambiguous,
                    (_, Some(_)) => Level::Inferred,
                    _ if provider.evidence.is_some() => Level::Inferred,
                    _ => level,
                };
                Link::new(
                    consumer.site.clone(),
                    provider.site.clone(),
                    contract.clone(),
                    consumer.cap.map_or(level, |cap| level.max(cap)),
                    reason,
                    None,
                    None,
                    None,
                )
            })
            .collect()
    }

    fn calls_from_definition(&self) -> bool {
        true
    }

    fn status(&self, repo: &Repository, export: &IndexExport, _links: &[Link]) -> Vec<String> {
        let prefix = format!("{}::", repo.identity);
        let mut lines: Vec<String> = (export.unmounted.iter())
            .map(|site| {
                let site = site.strip_prefix(&prefix).unwrap_or(site);
                Msg::BackstageUnmounted { site }.text()
            })
            .collect();
        let mut unread = std::collections::BTreeMap::new();
        for endpoint in export
            .consumers
            .iter()
            .filter_map(|c| ts::unread(c.via.as_deref()?))
        {
            *unread.entry(endpoint).or_insert(0) += 1;
        }
        lines.extend(
            (unread.into_iter())
                .map(|(endpoint, calls)| Msg::BackstageProxyTarget { endpoint, calls }.text()),
        );
        lines.extend(ts::unfollowed_of(&export.unfollowed).iter().map(|u| {
            Msg::BackstageInclude {
                file: &u.file,
                include: &u.include,
                reason: u.reason,
            }
            .text()
        }));
        lines
    }

    fn after_links(&self, links: &mut [Link], exports: &[IndexExport]) {
        drift(links, exports);
    }

    fn label(&self, contract: &str) -> String {
        match route(contract) {
            Some((method, path)) => format!("{method} {path}"),
            None => shown(contract),
        }
    }

    fn serves(&self, provider: &Provider, contract: &str) -> bool {
        serves(provider, contract)
    }

    fn no_candidates(
        &self,
        consumer: &Consumer,
        providers: &mut dyn Iterator<Item = &Provider>,
        missing: &str,
    ) -> Option<String> {
        let routes: Vec<_> = providers.filter_map(|p| route(&p.contract)).collect();
        if routes.is_empty() {
            return Some(crate::msg::Msg::UrlNoRoutes(missing).text());
        }
        let method = consumer.method.as_deref();
        let mut paths = (routes.iter())
            .filter(|(served, _)| method.is_none_or(|m| m == *served || *served == ANY))
            .map(|(_, path)| *path)
            .peekable();
        if paths.peek().is_none() {
            return Some(crate::msg::Msg::UrlNoMethod(method.unwrap_or_default()).text());
        }
        let written = consumer.lead.as_ref().map(|l| l.path.as_str()).or(
            match consumer.fragments.as_slice() {
                [one] => Some(one.as_str()),
                _ => None,
            },
        );
        let prefix = written.is_some_and(|written| {
            paths.any(|path| path != "/" && written.len() > path.len() && written.ends_with(path))
        });
        Some(crate::msg::Msg::UrlNoPath { prefix }.text())
    }

    fn candidate(&self, consumer: &Consumer, provider: &Provider) -> bool {
        let method = consumer.method.as_deref();
        let fits = |c: &str| match &consumer.lead {
            Some(lead) => tail_fits(c, method, &lead.path),
            None => route_fits(c, method, &consumer.fragments),
        };
        fits(&provider.contract) || gateway_contracts(provider).any(|(g, _)| fits(&g))
    }

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

    fn entry(&self, role: Role, contract: &str) -> Option<EntryKind> {
        (role == Role::Provider && route(contract).is_some()).then_some(EntryKind::Route)
    }
}

fn under(site: &str, identity: &str, dir: &str) -> bool {
    (path_of(site, identity).and_then(|p| p.strip_prefix(dir)))
        .is_some_and(|rest| rest.starts_with('/'))
}

fn path_of<'s>(site: &'s str, identity: &str) -> Option<&'s str> {
    let rest = site.strip_prefix(identity)?.strip_prefix("::")?;
    Some(rest.split_once("::").map_or(rest, |(path, _)| path))
}

#[cfg(test)]
mod tests {
    use super::{
        CallAt, Http, Mounts, PREFIX_DEPTH, fragments, normalize_path, prefixes, route_fits,
        tail_fits, under,
    };
    use crate::contract::{ContractKind, Role, Search, View};
    use crate::entry::EntryKind;
    use crate::index::{Consumer, Gateway, Provider};
    use crate::lang::{Limit, Part, Value};

    #[test]
    fn a_whole_file_site_lies_under_its_directory() {
        assert!(under("acme::svc-b/main.go", "acme", "svc-b"));
        assert!(under("acme::svc-b/routes.go::Routes.run", "acme", "svc-b"));
        assert!(!under("acme::svc-bx/main.go", "acme", "svc-b"));
        assert!(!under("other::svc-b/main.go", "acme", "svc-b"));
    }

    #[test]
    fn a_base_meada_could_not_read_leaves_the_consumer_unresolved_with_its_lead() {
        let read = Part::Read {
            scope: "".into(),
            callee: "c[]".into(),
            key: "K".into(),
            parameter: false,
            then: Vec::new(),
        };
        let text = |t: &str| Value::Str(vec![Part::Text(t.into())]);
        let call = Limit::Returned("c.base".into());
        for (lead, limit) in [
            (Part::Hole, Limit::Other),
            (read, Limit::Other),
            (Part::Limit(call.clone()), call),
            (Part::Limit(Limit::ClientBase), Limit::ClientBase),
        ] {
            let parts = vec![lead, Part::Text("/a/".into()), Part::Hole];
            let found = super::fetches(&[Value::Str(parts), text("GET")]).unwrap();
            assert_eq!(found[0].contract, None);
            assert_eq!(found[0].via, None);
            let lead = found[0].lead.as_ref().unwrap();
            assert_eq!((&lead.limit, lead.path.as_str()), (&limit, "/a/{}"));
        }
        let found = super::fetches(&[text("/a"), text("GET")]).unwrap();
        assert_eq!(found[0].contract.as_deref(), Some("http::GET::/a"));
        assert!(found[0].lead.is_none());
    }

    #[test]
    fn a_route_fits_a_known_end_by_a_suffix_of_its_path_at_a_slash() {
        for (contract, method, known, expected) in [
            ("http::GET::/items", Some("GET"), "/items", true),
            ("http::GET::/api/orders/items", Some("GET"), "/items", true),
            ("http::GET::/api/items/export", Some("GET"), "/items", false),
            ("http::GET::/api/lineitems", Some("GET"), "/items", false),
            ("http::GET::/v2/orders/{}", None, "/orders/{}", true),
            ("http::GET::/v2/orders/{}", None, "/orders", false),
            ("http::POST::/v2/orders", Some("GET"), "/orders", false),
            ("http::ANY::/v2/orders", Some("GET"), "/orders", true),
            ("http::GET::/v2/orders", None, "/", true),
            ("channel::orders", None, "/", false),
        ] {
            assert_eq!(
                tail_fits(contract, method, known),
                expected,
                "{contract} {method:?} {known}"
            );
        }
    }

    #[test]
    fn test_files_follow_each_languages_conventions() {
        for path in [
            "client/client_test.go",
            "src/orders.test.ts",
            "src/orders.spec.js",
            "web/__tests__/orders.ts",
            "app/test_api.py",
            "app/api_test.py",
            "tests/api.rs",
            "svc/tests/it/main.rs",
            "svc/src/test/java/example/A.java",
        ] {
            assert!(super::is_test_file(path), "{path}");
        }
        for path in [
            "main.go",
            "src/orders.ts",
            "src/contest.ts",
            "app/testing.py",
            "app/latest_api.py",
            "src/main/java/example/A.java",
            "src/testdata/a.go",
            "mytests/a.go",
        ] {
            assert!(!super::is_test_file(path), "{path}");
        }
    }

    fn provider(contract: &str) -> Provider {
        Provider {
            contract: contract.into(),
            ..Provider::default()
        }
    }

    #[test]
    fn routes_are_selected_by_the_route_word() {
        assert_eq!(Http.search_word(), Search::Filter("route"));
    }

    #[test]
    fn a_route_is_viewed_by_its_providers_with_a_response_and_a_non_route_not_at_all() {
        let view = Http.view("http::GET::/users/{}").unwrap();
        let expected = View {
            provider: "provider",
            consumer: None,
            carried: "response",
        };
        assert_eq!(view, expected);
        assert_eq!(Http.view("http::orders"), None);
        assert_eq!(Http.view("resource::db::orders"), None);
    }

    #[test]
    fn a_route_ranks_against_its_path_alone() {
        assert_eq!(Http.parts("http::get::/users"), (None, Some("/users")));
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
            identity: None,
            contract: None,
            site: "r::a.ts::f::g#0".into(),
            via: None,
            shape: None,
            method: Some("GET".into()),
            fragments: vec!["/shop/".into()],
            hops: Vec::new(),
            root: None,
            lead: None,
            cap: None,
            client: None,
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
            ("http::GET::/api/users", None, &[][..], true),
            ("http::GET::/api/users", Some("GET"), &[][..], true),
            ("http::POST::/api/users", Some("GET"), &[][..], false),
            ("http::ANY::/api/users", Some("DELETE"), &[][..], true),
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
            (
                "http::DELETE::/api/users/{}",
                Some("GET"),
                &["/api/"][..],
                false,
            ),
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
