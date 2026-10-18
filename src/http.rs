//! The HTTP contract kind: routes served by providers and the `fetch` calls that consume them.
//! A contract is `http::<METHOD>::<normalized path>`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::channel::Routes;
use crate::config::gateway_path;
use crate::contract::{ContractKind, Found, Level, Link, Role, Site};
use crate::declared::{self, Declared, Types};
use crate::index::{self, Consumer, Export as IndexExport, Gateway, Provider};
use crate::lang::{Call, Depends, Export, Part, Step, Ty, Value, literal, property};
use crate::literals::{Handler, Literals};
use crate::resolve::{Code, Expanded, Frame, Target, scope_of};
use crate::shape::{self, Import, Shape, Source, under};
use crate::workspace::Repository;

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

/// The consumer shape of the `fetch` at `call` (spec #23): the field paths read from the
/// parsed body of its response, by the function making the call and by each client wrapper
/// `frames` climbs from it to the consumer's site, which reads what the one below returns.
/// The body is the result of `.json()` on the response; a wrapper returns the response, the
/// body or a part of the body. `None` when nothing is read.
fn response_reads<'a>(
    code: &Code<'a>,
    types: &mut Types,
    call: (&'a str, usize),
    frames: &[Frame<'a>],
) -> Option<Shape> {
    /// What a value is: the response, or the body below a path (`Body(vec![])`: all of it).
    #[derive(Clone, PartialEq)]
    enum Got<'s> {
        Response,
        Body(Vec<&'s Step>),
    }
    let site = frames.last().map_or(call.0, |f| f.path);
    let mut out = Shape::new(Source::Reads(site.to_string()));
    let (mut got, mut at) = (Got::Response, call);
    // The types the wrapper below declares for the body with its own type parameters, by path:
    // its call gives the arguments that stand for them. And that wrapper.
    let mut generic: Vec<(String, &'a Ty)> = Vec::new();
    let mut wrapper: Option<Target<'a>> = None;
    for level in 0..=frames.len() {
        let mut returned = Vec::new();
        let mut deferred: Vec<(String, &'a Ty)> = Vec::new();
        // Where the call `at` is written; a call that is not there reads nothing.
        let scope = code.files().get(at.0).and_then(|f| f.calls.get(at.1));
        let scope = scope.map(|c| c.scope.as_str());
        for steps in code.uses(at.0, at.1)? {
            let (mut path, rest) = match (&got, steps.first()) {
                (Got::Response, Some(Step::Return)) if steps.len() == 1 => {
                    returned.push(Got::Response);
                    continue;
                }
                (Got::Response, Some(Step::Call(m))) if m == "json" => (Vec::new(), &steps[1..]),
                (Got::Response, _) => continue,
                (Got::Body(path), _) => (path.clone(), &steps[..]),
            };
            // A read stops at a method call: `name.toUpperCase()` reads `name`.
            let read = (rest.iter())
                .take_while(|s| matches!(s, Step::Field(_) | Step::Element))
                .count();
            path.extend(&rest[..read]);
            let base = written(&path);
            for prefix in shape::prefixes(&base) {
                out.list(prefix.to_string(), false, false);
            }
            match rest.get(read) {
                Some(Step::Return) => returned.push(Got::Body(path)),
                // The type declares what the value holds: the scope is the call's.
                Some(Step::Typed(ty)) if ty.has_param() => deferred.push((base, ty)),
                Some(Step::Typed(ty)) => {
                    let Some(scope) = scope else { continue };
                    let listed = types.read(code, at.0, scope, ty).unwrap_or_default();
                    typed(&mut out, &base, listed);
                }
                // The wrapper's type parameters, given here, stand in the types it declared.
                Some(Step::Argument(tys)) => {
                    let (Some(scope), Some(wrapper)) = (scope, wrapper) else {
                        continue;
                    };
                    let names = code.type_params(wrapper.path, wrapper.name);
                    let args = declared::Args {
                        names,
                        tys,
                        at: (at.0, scope),
                    };
                    for (base, ty) in &generic {
                        let listed = types
                            .read_with(code, at.0, scope, ty, Some(args))
                            .unwrap_or_default();
                        typed(&mut out, base, listed);
                    }
                }
                _ => {}
            }
        }
        let Some(frame) = frames.get(level) else {
            break;
        };
        // The frame calls the function making the call `at`, which returns one thing.
        returned.dedup();
        match <[Got; 1]>::try_from(returned) {
            Ok([one]) if frame.def.path == at.0 && Some(frame.def.name) == scope => {
                // A return type that is a type parameter of the wrapper declares what it returns.
                if let (Got::Body(path), Some(ty)) = (&one, code.returns(at.0, frame.def.name))
                    && ty.has_param()
                {
                    deferred.push((written(path), ty));
                }
                got = one;
            }
            _ => break,
        }
        (generic, wrapper) = (deferred, Some(frame.def));
        at = (frame.path, frame.index);
    }
    out.fields.remove("");
    (!out.fields.is_empty() || !out.imports.is_empty()).then_some(out)
}

/// Adds the fields a declared type lists, below `base`, to a consumer's `shape`, with the types
/// and schemas of a package it names for the query to fill.
fn typed(shape: &mut Shape, base: &str, listed: Declared) {
    for (field, optional) in listed.fields {
        shape.list(under(base, &field), optional, true);
    }
    for import in listed.imports {
        let at = under(base, &import.at);
        shape.imports.insert(Import { at, ..import });
    }
}

/// The field path `steps` (fields and elements) read: `items[].id`.
fn written(steps: &[&Step]) -> String {
    let mut out = String::new();
    for step in steps {
        match step {
            Step::Field(name) if out.is_empty() => out += name,
            Step::Field(name) => out += &format!(".{name}"),
            _ => out += "[]",
        }
    }
    out
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

/// What `express()`, `Router()` and `express.Router()` return: an app or a router.
const ROUTERS: &[&str] = &["express", "Router", "express.Router"];

/// The HTTP methods as lower-case names: Express and FastAPI register routes with them, and
/// Python clients send requests with them.
const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch", "options", "head"];

/// A call by its file and index: what creates an app or a router, or a URLconf entry.
type CallAt<'a> = (&'a str, usize);

/// For each router, the routers it is mounted on and the segment the mount puts before its
/// routes, `None` when it cannot be read.
type Mounts<'a> = BTreeMap<CallAt<'a>, Vec<(CallAt<'a>, Option<String>)>>;

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
fn prefixes<'a>(
    router: CallAt<'a>,
    mounts: &Mounts<'a>,
    root: &dyn Fn(CallAt<'a>) -> Option<String>,
    seen: &mut Vec<CallAt<'a>>,
) -> Vec<String> {
    let mut work = PREFIX_WORK;
    prefixes_within(router, mounts, root, seen, &mut work).unwrap_or_default()
}

/// `prefixes`, `None` when `work` mounts do not cover it.
fn prefixes_within<'a>(
    router: CallAt<'a>,
    mounts: &Mounts<'a>,
    root: &dyn Fn(CallAt<'a>) -> Option<String>,
    seen: &mut Vec<CallAt<'a>>,
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

/// Whether the call `router` creates an Express app or router: `express()`, `Router()` or
/// `express.Router()`, unless an import from another module than `express` binds the name.
fn is_express<'a>(code: &Code<'a>, (path, n): CallAt<'a>) -> bool {
    let call = &code.files()[path].calls[n];
    ROUTERS.contains(&call.callee.as_str())
        && (code.external(path, &call.scope, &call.callee))
            .is_none_or(|name| name.starts_with("express."))
}

/// The shape the handler `name` of `path` gives its route from what the code returns (spec
/// #23): the one place that picks the source of a code route's shape. The sources come in the
/// order of ADR 0009, a document merged later in `declare`, then the zod schema the response
/// is parsed with or typed with, then the returned literals.
fn provider_shape(
    code: &Code,
    literals: &mut Literals,
    path: &str,
    name: &str,
    kind: Handler,
) -> Option<Shape> {
    let responses = literals.responses(code, path, name, kind)?;
    (literals.types().zod().returned(code, &responses, name))
        .or_else(|| literals.shape(code, &responses))
}

/// Express providers: `app.METHOD(path, ..., handler)` and `router.METHOD(...)` on an app or a
/// router, under every prefix of `app.use(prefix, router)` mounts, at any depth. The app or
/// router is followed through constants, imports, arguments and return values.
// ponytail: `router.route(path)` chains and `require` are not followed.
fn express(code: &Code, literals: &mut Literals) -> Vec<Found> {
    // The names Express is known by are spellings (`Router()`), which any other language may
    // give its own functions and types.
    if !code.pack().extensions().contains(&"js") {
        return Vec::new();
    }
    let routers = |path, scope, name| -> Vec<CallAt> {
        let origins = code.origins(path, scope, name).into_iter();
        origins.filter(|&r| is_express(code, r)).collect()
    };
    let mut mounts = Mounts::new();
    let mut routes = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let Some((receiver, member)) = call.callee.rsplit_once('.') else {
                continue;
            };
            let registers = HTTP_METHODS.contains(&member);
            if !(registers || member == "use") || receiver.starts_with('(') {
                continue;
            }
            let ats = routers(path, &call.scope, receiver);
            if registers {
                routes.extend(
                    ats.into_iter()
                        .map(|at| (path, n, at, member.to_uppercase())),
                );
                continue;
            }
            if ats.is_empty() {
                continue;
            }
            let Some(args) = code.args(path, n) else {
                continue;
            };
            let first = args.first().map(|a| (a, code.value(path, &call.scope, a)));
            let (prefix, rest) = match first {
                Some((_, Value::Str(parts))) => (literal(&parts), &args[1..]),
                // A router or a handler first: the mount has no path.
                None | Some((_, Value::Call(_) | Value::Function(_) | Value::New(..))) => {
                    (Some(String::new()), args)
                }
                Some((Value::Ref(name), _))
                    if code.definition(path, &call.scope, name).is_some() =>
                {
                    (Some(String::new()), args)
                }
                // A router reached through arguments and return values has no path either.
                Some((first, _))
                    if (code.value_origins(path, &call.scope, first).into_iter())
                        .any(|c| is_express(code, c)) =>
                {
                    (Some(String::new()), args)
                }
                // Anything else may be a path Meada cannot read (a parameter, a member of an
                // imported object): what it mounts is served under no prefix.
                // ponytail: so is a router mounted after a package's middleware passed uncalled
                // (`app.use(requireAuth, router)`); tell package handlers from package strings
                // through `Code::origin` if that loses routes that matter.
                _ => (None, &args[1..]),
            };
            for arg in rest {
                let children = code.value_origins(path, &call.scope, arg).into_iter();
                for child in children.filter(|&c| is_express(code, c)) {
                    let parents = mounts.entry(child).or_default();
                    parents.extend(ats.iter().map(|&at| (at, prefix.clone())));
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
        let Some(route) = literal(&parts) else {
            continue;
        };
        // The handler's literals (spec #23), read once for every prefix it is mounted under.
        let target = match handler {
            Value::Function(def) => Some((path, def.as_str())),
            Value::Ref(name) => code.definition(path, scope, name).map(|t| (t.path, t.name)),
            _ => None,
        };
        let shape = target
            .and_then(|(at, name)| provider_shape(code, literals, at, name, Handler::Express));
        let root = |_: CallAt| Some(String::new());
        for prefix in prefixes(at, &mounts, &root, &mut Vec::new()) {
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
                shape: shape.clone(),
                ..provider(site, contract(&method, &full))
            });
        }
    }
    found
}

/// The method routers of axum, as functions of `axum::routing` and as methods of a method
/// router: `get(a).post(b)`.
const AXUM_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "trace",
];

/// The methods of a method router that leave the methods it serves as they are.
const AXUM_PASSES: &[&str] = &["layer", "route_layer", "with_state", "handle_error"];

/// How many calls a method router chain is read through.
// ponytail: a route serves a handful of methods; a crafted chain is not read to its end.
const METHOD_CHAIN: usize = 64;

/// Whether the call `router` creates an axum router.
fn is_axum_router<'a>(code: &Code<'a>, (path, n): CallAt<'a>) -> bool {
    let call = &code.files()[path].calls[n];
    let name = code.external(path, &call.scope, &call.callee);
    matches!(
        name.as_deref(),
        Some("axum.Router.new" | "axum.Router.default")
    )
}

/// The calls of the method router `value` that give a method a handler, and the method each
/// serves (`ANY` for `any`), first call first: its `get(a)` and the `.post(b)` after it. The
/// method of a function is the name axum gives it, whatever the file renames it to. `None` when
/// the chain is not made of `axum::routing` functions and methods that serve methods.
fn method_router<'a>(
    code: &Code<'a>,
    path: &'a str,
    value: &'a Value,
) -> Option<Vec<(&'a Call, String)>> {
    let mut out = Vec::new();
    let mut value = value;
    for _ in 0..METHOD_CHAIN {
        let Value::Call(invocation) = value else {
            return None;
        };
        let call = code.call(path, code.call_index(path, &invocation.range)?)?;
        let name = call.callee.rsplit('.').next()?;
        match &call.receiver {
            // `get(h)`, `routing::get(h)`: the function of axum that serves the method.
            None => {
                let external = code.external(path, &call.scope, &call.callee)?;
                match external.strip_prefix("axum.routing.")? {
                    "any" => out.push((call, ANY.to_string())),
                    name if AXUM_METHODS.contains(&name) => {
                        out.push((call, name.to_uppercase()));
                    }
                    "MethodRouter.new" => {}
                    _ => return None,
                }
                out.reverse();
                return Some(out);
            }
            Some(receiver) => {
                match name {
                    name if AXUM_METHODS.contains(&name) => {
                        out.push((call, name.to_uppercase()));
                    }
                    name if AXUM_PASSES.contains(&name) => {}
                    _ => return None,
                }
                value = receiver;
            }
        }
    }
    None
}

/// The axum routers `value`, written in `call`, may be.
fn axum_routers<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    value: &'a Value,
) -> Vec<CallAt<'a>> {
    let origins = code.value_origins(path, &call.scope, value);
    (origins.into_iter())
        .filter(|&r| is_axum_router(code, r))
        .collect()
}

/// The axum routers given to what `value`, written in `call`, is made of, when it is not a
/// router itself: the arguments and receiver of its calls, the values a function of the
/// repository returns (found by name too), and the values of constants.
fn axum_unread<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    value: &'a Value,
) -> BTreeSet<CallAt<'a>> {
    let mut found = BTreeSet::new();
    let mut seen = HashSet::new();
    let mut work = vec![(path, call.scope.as_str(), value)];
    while let Some((path, scope, value)) = work.pop() {
        if !seen.insert(std::ptr::from_ref(value)) {
            continue;
        }
        let routers: Vec<CallAt> = (code.value_origins(path, scope, value).into_iter())
            .filter(|&r| is_axum_router(code, r))
            .collect();
        if !routers.is_empty() {
            found.extend(routers);
            continue;
        }
        match value {
            Value::Call(invocation) => {
                let Some(index) = code.call_index(path, &invocation.range) else {
                    continue;
                };
                let Some(call) = code.call(path, index) else {
                    continue;
                };
                let given = call.args.iter().chain(&call.receiver);
                work.extend(given.map(|v| (path, call.scope.as_str(), v)));
                work.extend(code.returned(path, index));
            }
            Value::Ref(name) => {
                let held = code.constant(path, scope, name);
                work.extend(held.map(|((at, name), v)| (at, scope_of(name), v)));
            }
            Value::List(items) => work.extend(items.iter().map(|v| (path, scope, v))),
            _ => {}
        }
    }
    found
}

/// What stands for the brace of axum 0.8's `{{` and `}}` while `normalize_path` runs, which
/// would take it for a parameter.
const OPEN: char = '\u{1}';
const CLOSE: char = '\u{2}';

/// `path` with each wildcard segment (`*rest`) as a parameter and each escaped brace as
/// `OPEN` or `CLOSE`.
fn axum_path(path: &str) -> String {
    let segments = path.split('/');
    let segments = segments.map(|s| if s.starts_with('*') { PARAM } else { s });
    let path = segments.collect::<Vec<_>>().join("/");
    path.replace("{{", &OPEN.to_string())
        .replace("}}", &CLOSE.to_string())
}

/// A path `axum_path` read, with its escaped braces the literal braces they serve.
fn axum_literal(path: &str) -> String {
    path.replace(OPEN, "{").replace(CLOSE, "}")
}

/// axum providers: `Router::new().route(path, get(h).post(g))` and `any(h)`, under every prefix
/// of the `nest` mounts the router is under, at any depth; `merge` adds no prefix. A router is
/// the `Router::new()` call that makes it, followed through the chain of builder calls on it,
/// constants, arguments and return values (`Code::origins`). A router nothing mounts is the
/// app and is served at its root; one a mount puts under a prefix Meada cannot read, or that
/// is mounted on a router it cannot read, is served nowhere. A handler is the function the
/// route names (a closure is an inline handler); any other handler leaves the route at its file.
// ponytail: a router held by `let mut` or reassigned, `route_service`, and
// `on(MethodFilter, h)` are not followed; the routes on them are not providers.
fn axum(code: &Code) -> Vec<Found> {
    if !code.pack().extensions().contains(&"rs") {
        return Vec::new();
    }
    let mut mounts = Mounts::new();
    let mut routes = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let member = call.callee.rsplit('.').next().unwrap_or_default();
            if !matches!(
                member,
                "route" | "nest" | "nest_service" | "merge" | "route_service" | "fallback_service"
            ) {
                continue;
            }
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let Some(receiver) = &call.receiver else {
                continue;
            };
            let parents = axum_routers(code, path, call, receiver);
            match (member, call.args.as_slice()) {
                ("route", [route, handlers]) => {
                    let Value::Str(parts) = code.value(path, &call.scope, route) else {
                        continue;
                    };
                    let (Some(route), Some(handlers)) =
                        (literal(&parts), method_router(code, path, handlers))
                    else {
                        continue;
                    };
                    for at in parents {
                        routes.push((path, at, axum_path(&route), handlers.clone()));
                    }
                }
                ("nest" | "nest_service" | "merge", args) => {
                    let (prefix, child) = match (member, args) {
                        ("nest" | "nest_service", [prefix, child]) => {
                            let prefix = code.value(path, &call.scope, prefix);
                            (path_literal(prefix).map(|p| axum_path(&p)), child)
                        }
                        ("merge", [child]) => (Some(String::new()), child),
                        _ => continue,
                    };
                    // A child mounted on a router Meada cannot read is mounted nowhere it
                    // could serve it: it still has a mount, with no parents.
                    let children = axum_routers(code, path, call, child);
                    for child in &children {
                        let mounted = mounts.entry(*child).or_default();
                        mounted.extend(parents.iter().map(|&at| (at, prefix.clone())));
                    }
                    // A child Meada cannot read is a call it does not follow: the routers
                    // given to it are mounted there, so served nowhere.
                    if children.is_empty() {
                        for unread in axum_unread(code, path, call, child) {
                            mounts.entry(unread).or_default();
                        }
                    }
                }
                // A service Meada does not read: the routers given to it are served there.
                ("route_service" | "fallback_service", args) => {
                    for unread in args.iter().flat_map(|a| axum_unread(code, path, call, a)) {
                        mounts.entry(unread).or_default();
                    }
                }
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    let root = |_: CallAt| Some(String::new());
    for (path, at, route, handlers) in routes {
        for prefix in prefixes(at, &mounts, &root, &mut Vec::new()) {
            let full = axum_literal(&normalize_path(&join(&prefix, &route)));
            for (call, method) in &handlers {
                let scope = &call.scope;
                let site = match call.args.first() {
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
                };
                found.push(provider(site, format!("http::{method}::{full}")));
            }
        }
    }
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
        let origins = code.origins(path, scope, name).into_iter();
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

/// A Python HTTP client library, by what it names outside the repository.
struct Library {
    /// The module whose functions send a request (`requests.get`).
    module: Option<&'static str>,
    /// What constructs a client, an object whose methods send requests.
    clients: &'static [&'static str],
}

const LIBRARIES: &[Library] = &[
    Library {
        module: Some("requests"),
        clients: &["requests.Session"],
    },
    Library {
        module: Some("httpx"),
        clients: &["httpx.Client", "httpx.AsyncClient"],
    },
    Library {
        module: None,
        clients: &["aiohttp.ClientSession"],
    },
];

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

/// Whether `member` sends a request: one of the methods, or `request(method, url)`.
fn sends(member: &str) -> bool {
    HTTP_METHODS.contains(&member) || member == "request"
}

/// The base URL the clients that may be the `receiver` of the call at `path` were built
/// with, `None` when it is not a client: `Some(None)` when it has no base URL, an unknown one
/// when the clients differ. A client is the call of a library's constructor that the receiver
/// traces back to, by assignment, `with`, a `self` attribute or an argument.
// ponytail: a receiver that is the result of a call (`Session().get(...)`, `make().get(...)`)
// is not read; a parameter given a client takes the base of every caller that passes one.
fn client_base<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    receiver: &'a str,
) -> Option<Option<Vec<Part>>> {
    let mut bases: Vec<Option<Vec<Part>>> = Vec::new();
    for (at, i) in code.origins(path, scope, receiver) {
        let Some(ctor) = code.call(at, i) else {
            continue;
        };
        let name = code.external(at, &ctor.scope, &ctor.callee);
        if !name.is_some_and(|n| LIBRARIES.iter().any(|l| l.clients.contains(&n.as_str()))) {
            continue;
        }
        let base = match ctor.keyword("base_url") {
            None => None,
            Some(url) => match code.value(at, &ctor.scope, url) {
                Value::Undefined => None,
                Value::Str(parts) => Some(parts),
                _ => Some(vec![Part::Hole]),
            },
        };
        if !bases.contains(&base) {
            bases.push(base);
        }
    }
    match bases.len() {
        0 => None,
        1 => bases.pop(),
        _ => Some(Some(vec![Part::Hole])),
    }
}

/// Python HTTP consumers: a function of a library's module (`requests.get(url)`) or a method
/// of a client it constructs (`session.get(url)`), whose HTTP method is its name or, for
/// `request(method, url)`, its first argument. A wrapper around one is read at each of its call
/// sites, as `fetch` wrappers are.
fn clients(code: &Code) -> Vec<Found> {
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        let imports = (facts.imports.iter()).any(|i| {
            LIBRARIES
                .iter()
                .any(|l| l.module == Some(i.source.as_str()))
        });
        for (n, call) in facts.calls.iter().enumerate() {
            let function = imports
                .then(|| code.external(path, &call.scope, &call.callee))
                .flatten()
                .and_then(|name| {
                    let (module, function) = name.rsplit_once('.')?;
                    let library = LIBRARIES.iter().any(|l| l.module == Some(module));
                    (library && sends(function)).then(|| (function.to_string(), None))
                });
            let method = call
                .callee
                .rsplit_once('.')
                .filter(|(r, m)| function.is_none() && sends(m) && !r.starts_with('('));
            let (member, base) = match (function, method) {
                (Some(function), _) => function,
                (None, Some((receiver, member))) => {
                    match client_base(code, path, &call.scope, receiver) {
                        Some(base) => (member.to_string(), base),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let read = |args: &[Value], kwargs: &[(String, Value)]| {
                let arg = |n: usize, key: &str| {
                    let named = kwargs.iter().find(|(k, _)| k == key).map(|(_, v)| v);
                    args.get(n).or(named)
                };
                let (method, url) = match member.as_str() {
                    "request" => (arg(0, "method").cloned(), arg(1, "url")?),
                    name => (
                        Some(Value::Str(vec![Part::Text(name.to_uppercase())])),
                        arg(0, "url")?,
                    ),
                };
                let url = based(base.as_deref(), url);
                fetch(&[url, method.unwrap_or(Value::Unknown)])
            };
            for (site, at, read) in code.expand(path, n, &read) {
                found.push(consumer(site, at, read, None));
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
        Include::Module(module) => code.origins(module, "", "urlpatterns"),
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
        let entries = code.origins(root, "", "urlpatterns");
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
                if stack.contains(&origin) {
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

impl ContractKind for Http {
    fn prefix(&self) -> &'static str {
        "http"
    }

    fn extract(&self, code: &Code) -> Vec<Found> {
        let mut found = Vec::new();
        // `fetch` is a consumer only where the language binds it everywhere.
        let global = code.binds_everywhere("fetch");
        // One reader of each shape source for the whole extraction: each budget per file once.
        let mut literals = Literals::default();
        for (&path, facts) in code.files() {
            for (n, call) in facts.calls.iter().enumerate() {
                let is_fetch = global
                    && (call.callee == "fetch"
                        || code.param_defaults_to(path, &call.scope, &call.callee, "fetch"));
                if !is_fetch || code.args(path, n).is_none_or(<[_]>::is_empty) {
                    continue;
                }
                let expanded = code.expand_frames(path, n, &|args, _| fetch(args));
                for Expanded {
                    site: (site, at),
                    frames,
                    found: read,
                } in expanded
                {
                    let shape = response_reads(code, literals.types(), (path, n), &frames);
                    found.push(consumer(site, at, read, shape));
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
                let defined = facts.defs.iter().any(|d| d.name == *local);
                let (site, shape) = if defined {
                    let site = Site::Def {
                        path: path.into(),
                        name: local.clone(),
                    };
                    (
                        site,
                        provider_shape(code, &mut literals, path, local, Handler::Next),
                    )
                } else {
                    (Site::File(path.into()), None)
                };
                found.push(Found {
                    shape,
                    ..provider(site, contract(exported, &route))
                });
            }
        }
        found.extend(clients(code));
        found.extend(express(code, &mut literals));
        found.extend(axum(code));
        found.extend(decorated(code));
        found.extend(django(code));
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
        _routes: &Routes,
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
}

#[cfg(test)]
mod tests {
    use super::{
        CallAt, Mounts, PREFIX_DEPTH, fragments, normalize_path, prefixes, regex_route, route_fits,
    };

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
    fn an_escaped_brace_is_literal_and_a_wildcard_is_a_parameter() {
        use super::{axum_literal, axum_path};
        let read = |p: &str| axum_literal(&normalize_path(&axum_path(p)));
        assert_eq!(read("/{{raw}}"), "/{raw}");
        assert_eq!(read("/a/{{x}}/{id}/*rest"), "/a/{x}/{}/{}");
        assert_eq!(read("/{*rest}"), "/{}");
    }

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
}
