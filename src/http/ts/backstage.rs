//! Backstage backends (#615): the framework mounts every backend plugin's router at
//! `/api/<pluginId>`, and plugins reach each other through `discovery.getBaseUrl('<pluginId>')`.
//! A `createBackendPlugin({ pluginId: '<literal>', register })` imported from a `@backstage/`
//! package is a mount node: what its `register` gives `<dep>.use(...)`, `<dep>` a key of a
//! `registerInit` `deps` object bound to `coreServices.httpRouter`, is mounted under
//! `/api/<pluginId>`, the `pluginId` a literal or a `const` of the file bound to one. A plugin
//! whose `pluginId` cannot be read mounts nothing, so its routers stay at the root, and
//! `meada status` says so (`Role::Unmounted`). On the consumer side a URL that starts with `discovery.getBaseUrl('<literal>')`,
//! held by a constant or written in place, starts at `/api/<literal>`, and a hostless consumer of
//! a route a plugin's mount serves names that mount in its reason (`name`). A URL through a
//! proxy endpoint (`/api/proxy/<ep>`, #616) names the endpoint in its `via` (`proxied`).

use super::super::{CLIENT_VIA, CallAt, PROXY_VIA, fits};
use crate::contract::Found;
use crate::lang::{Part, Value, literal, property};
use crate::resolve::Code;

/// Where Backstage mounts the router of the plugin `id`.
pub(super) fn mount(id: &str) -> String {
    format!("/api/{id}")
}

/// The id a string literal names.
fn id_of(value: &Value) -> Option<String> {
    let Value::Str(parts) = value else {
        return None;
    };
    literal(parts)
}

/// Whether `scope` is `def` or lies inside it.
fn within(scope: &str, def: &str) -> bool {
    scope
        .strip_prefix(def)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// A backend plugin: its `createBackendPlugin` call, its `pluginId` (`None` when it cannot be
/// read: its routers stay at the root), and each argument its
/// `httpRouter.use(...)` calls give, with the file and the scope it is written in.
type Plugin<'a> = (
    CallAt<'a>,
    Option<String>,
    Vec<(&'a str, &'a str, &'a Value)>,
);

/// Each backend plugin: its `createBackendPlugin` call, its `pluginId` when it is a literal or a
/// `const` of the file bound to one, and every argument of each `httpRouter.use(...)` its
/// `register` makes, with the file and the scope it is written in.
// ponytail: `register` and `init` are read where they are written (a helper they call and a
// renamed destructured `httpRouter` are not followed); follow them through `Code::origins` if
// plugins in the wild are written that way.
pub(super) fn plugins<'a>(code: &Code<'a>) -> Vec<Plugin<'a>> {
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            if !call.callee.ends_with("createBackendPlugin") {
                continue;
            }
            let from_backstage = code.outside(path, &call.scope, &call.callee, false);
            if !from_backstage.is_some_and(|o| o.spelled.starts_with("@backstage/")) {
                continue;
            }
            let Some([Value::Object(props), ..]) = code.args(path, n) else {
                continue;
            };
            let (Ok(Some(id)), Ok(Some(Value::Function(register)))) =
                (property(props, "pluginId"), property(props, "register"))
            else {
                continue;
            };
            // A literal, or a `const` of the file bound to one.
            let id = match id {
                Value::Ref(name) => code
                    .constant(path, &call.scope, name)
                    .filter(|((file, _), _)| *file == path)
                    .and_then(|(_, value)| id_of(value)),
                id => id_of(id),
            };
            let inside = |c: &(usize, &crate::lang::Call)| within(&c.1.scope, register);
            let calls: Vec<_> = facts.calls.iter().enumerate().filter(inside).collect();
            // The names `init` takes the `httpRouter` service by.
            let mut routers = Vec::new();
            for &(i, c) in &calls {
                if !c.callee.ends_with(".registerInit") {
                    continue;
                }
                let Some([Value::Object(init), ..]) = code.args(path, i) else {
                    continue;
                };
                if let Ok(Some(Value::Object(deps))) = property(init, "deps") {
                    let service = |v: &Value| matches!(v, Value::Member(o, m) if o == "coreServices" && m == "httpRouter");
                    routers.extend(deps.iter().filter(|(_, v)| service(v)).map(|(k, _)| k));
                }
            }
            let mut uses = Vec::new();
            for &(i, c) in &calls {
                let Some((receiver, "use")) = c.callee.rsplit_once('.') else {
                    continue;
                };
                if !routers.iter().any(|r| *r == receiver) {
                    continue;
                }
                let args = code.args(path, i).unwrap_or_default();
                uses.extend(args.iter().map(|a| (path, c.scope.as_str(), a)));
            }
            out.push(((path, n), id, uses));
        }
    }
    out
}

/// The plugin id `getBaseUrl('<literal>')` names, on a receiver named for Backstage's
/// discovery service (`discovery`, `this.discovery`, `discoveryApi`).
fn discovered(callee: &str, args: &[Value]) -> Option<String> {
    let (receiver, "getBaseUrl") = callee.rsplit_once('.')? else {
        return None;
    };
    let receiver = receiver.rsplit('.').next().unwrap_or(receiver);
    if !receiver.to_lowercase().contains("discovery") {
        return None;
    }
    match args {
        [id] => id_of(id),
        _ => None,
    }
}

/// The plugin id the URL of the `fetch` at `n` of `path` starts with: its first part is a
/// constant holding `discovery.getBaseUrl('<literal>')`, or that call written in place, the
/// first call inside the `fetch`.
pub(super) fn base<'a>(code: &Code<'a>, path: &'a str, n: usize) -> Option<String> {
    let fetch = code.call(path, n)?;
    let Some(Value::Str(parts)) = fetch.args.first() else {
        return None;
    };
    match parts.first()? {
        Part::Ref(name) => match code.constant(path, &fetch.scope, name)? {
            (_, Value::Call(call)) => discovered(&call.callee, &call.args),
            _ => None,
        },
        Part::Hole => {
            // A call written in another argument (`{ headers: ... }`) is not the URL's.
            let mut elsewhere = Vec::new();
            fetch.args[1..]
                .iter()
                .for_each(|a| calls_in(a, &mut elsewhere));
            let calls = code.files().get(path)?.calls.iter().enumerate();
            let inside = calls.filter(|&(i, c)| {
                i != n
                    && fetch.range.start <= c.range.start
                    && c.range.end <= fetch.range.end
                    && !elsewhere.contains(&c.range.start)
            });
            let (first, _) = inside.min_by_key(|(_, c)| c.range.start)?;
            let call = code.call(path, first)?;
            discovered(&call.callee, &call.args)
        }
        _ => None,
    }
}

/// Where every call `value` holds starts, at any depth.
fn calls_in(value: &Value, out: &mut Vec<usize>) {
    match value {
        Value::Call(call) => {
            out.push(call.range.start);
            call.receiver.iter().for_each(|r| calls_in(r, out));
            call.args.iter().for_each(|a| calls_in(a, out));
        }
        Value::Object(properties) => properties.iter().for_each(|(_, v)| calls_in(v, out)),
        Value::List(items) | Value::New(_, items) => items.iter().for_each(|v| calls_in(v, out)),
        _ => {}
    }
}

/// `args` of a `fetch` with the unknown first part of its URL (what `getBaseUrl('<id>')` folded
/// to) replaced by the mount of the plugin `id`; `None` when the URL does not start with an
/// unknown part.
pub(super) fn based(args: &[Value], id: &str) -> Option<Vec<Value>> {
    let mut args = args.to_vec();
    let Some(Value::Str(parts)) = args.first_mut() else {
        return None;
    };
    let first @ Part::Hole = parts.first_mut()? else {
        return None;
    };
    *first = Part::Text(mount(id));
    Some(args)
}

/// Names, in the reason of each hostless consumer of `consumers` with a route a Backstage
/// plugin's mount serves (`mounted`, by contract and plugin id), that plugin's mount.
pub(in crate::http) fn name(consumers: &mut [Found], mounted: &[(String, String)]) {
    for consumer in consumers.iter_mut().filter(|c| c.via.is_none()) {
        let Some(contract) = &consumer.contract else {
            continue;
        };
        let mut ids = (mounted.iter())
            .filter(|(served, _)| fits(served, contract))
            .map(|(_, id)| id);
        // Two plugins serving one route leave the reason to the link.
        if let (Some(id), None) = (ids.next(), ids.next()) {
            consumer.via = Some(client(id));
        }
    }
}

/// How a consumer of the plugin `id`'s routes names its client, for the link's reason.
pub(super) fn client(id: &str) -> String {
    format!("{CLIENT_VIA}Backstage plugin {id} at {}", mount(id))
}

/// How a consumer whose URL went through the proxy endpoint `endpoint` (#616) names where it
/// goes: `via` (its target, `env:` or `host:`) with the endpoint, or, when the endpoint's target
/// is not read (`usable` false), a client that says so.
pub(super) fn proxied(via: Option<String>, endpoint: &str, usable: bool) -> Option<String> {
    if !usable {
        return Some(format!(
            "{CLIENT_VIA}Backstage proxy endpoint {endpoint}{UNREAD}"
        ));
    }
    via.map(|v| format!("{v}{PROXY_VIA}{endpoint}"))
}

/// What a proxy endpoint's client says when its target is neither `${VAR}` nor an `http(s)://`
/// URL.
const UNREAD: &str = " (its target is neither ${VAR} nor an http(s) URL)";

/// The proxy endpoint whose target is not read, when `via` says it (`proxied`).
pub(in crate::http) fn unread(via: &str) -> Option<&str> {
    let endpoint = via.strip_prefix(CLIENT_VIA)?;
    endpoint
        .strip_prefix("Backstage proxy endpoint ")?
        .strip_suffix(UNREAD)
}
