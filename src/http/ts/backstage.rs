//! Backstage backends (#615): the framework mounts every backend plugin's router at
//! `/api/<pluginId>`, and plugins reach each other through `discovery.getBaseUrl('<pluginId>')`.
//! A `createBackendPlugin({ pluginId: '<literal>', register })` imported from a `@backstage/`
//! package is a mount node, and so is a `createBackendModule` (#813), whose `pluginId` is the
//! plugin it extends and whose plugin-scoped `httpRouter` is that plugin's own: what its
//! `register` gives `<dep>.use(...)`, `<dep>` a key of a `registerInit` `deps` object bound to
//! `coreServices.httpRouter`, is mounted under `/api/<pluginId>`, the `pluginId` a literal or a
//! `const` of the file bound to one. A plugin whose `pluginId` cannot be read mounts nothing, so
//! its routers stay at the root, and `meada status` says so (`Role::Unmounted`). On the consumer
//! side a URL that starts with `discovery.getBaseUrl('<literal>')`, held by a constant or written
//! in place, starts at `/api/<literal>`, and a hostless consumer of a route a plugin's mount
//! serves names that mount in its reason (`name`). A URL through a proxy endpoint
//! (`/api/proxy/<ep>`, #616) names the endpoint in its `via` (`proxied`).

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

/// Each backend plugin or module: its `createBackendPlugin` or `createBackendModule` call, its
/// `pluginId` when it is a literal or a `const` of the file bound to one, and every argument of
/// each `httpRouter.use(...)` its `register` makes, with the file and the scope it is written in.
// ponytail: `register` and `init` are read where they are written (a helper they call and a
// renamed destructured `httpRouter` are not followed); follow them through `Code::origins` if
// plugins in the wild are written that way.
pub(super) fn plugins<'a>(code: &Code<'a>) -> Vec<Plugin<'a>> {
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            // A module's `pluginId` is the plugin it extends, and it gets that plugin's router.
            if !["createBackendPlugin", "createBackendModule"]
                .iter()
                .any(|c| call.callee.ends_with(c))
            {
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

/// The Backstage discovery service's `getBaseUrl(id)`. The fold supplies the argument
/// under the current call frames, including inside a helper that returns it.
pub(super) fn discovered(
    call: &crate::lang::Call,
    arg: &mut dyn FnMut(usize) -> Value,
) -> Option<Value> {
    let (receiver, "getBaseUrl") = call.callee.rsplit_once('.')? else {
        return None;
    };
    let receiver = receiver.rsplit('.').next().unwrap_or(receiver);
    if !receiver.to_lowercase().contains("discovery") || call.args.len() != 1 {
        return None;
    }
    let value = arg(0);
    if matches!(value, Value::Ref(_)) {
        return Some(value);
    }
    let key = id_of(&value)?;
    Some(Value::Str(vec![Part::Read {
        scope: std::sync::Arc::from(""),
        callee: call.callee.clone(),
        key,
        parameter: false,
        then: Vec::new(),
    }]))
}

/// The plugin and method already identify the request here. A caller may refine its path.
pub(super) fn has_route(args: &[Value]) -> bool {
    let Some(Value::Str(parts)) = args.first() else {
        return false;
    };
    let base = parts.iter().find(|p| !matches!(p, Part::StripStart));
    matches!(base, Some(Part::Read { callee, parameter: false, .. }) if callee.ends_with(".getBaseUrl"))
        && super::super::method(args.get(1)).ok().flatten().is_some()
}

/// Replace a leading discovery result with the plugin's mount and retain its identity.
pub(super) fn based(args: &mut [Value]) -> Option<String> {
    let Some(Value::Str(parts)) = args.first_mut() else {
        return None;
    };
    let first = parts
        .iter_mut()
        .find(|part| !matches!(part, Part::StripStart))?;
    let Part::Read { callee, key, .. } = first else {
        return None;
    };
    if !callee.ends_with(".getBaseUrl") {
        return None;
    }
    let id = key.clone();
    *first = Part::Text(mount(&id));
    Some(id)
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
/// URL, or the path the target gets is not read (`Config::strips`).
const UNREAD: &str = " (its target or the path it gets is not read: a target neither ${VAR} nor an http(s) URL, a pathRewrite that does more than strip the endpoint, ignorePath, prependPath, or app-config files that disagree)";

/// The proxy endpoint whose target is not read, when `via` says it (`proxied`).
pub(in crate::http) fn unread(via: &str) -> Option<&str> {
    let endpoint = via.strip_prefix(CLIENT_VIA)?;
    endpoint
        .strip_prefix("Backstage proxy endpoint ")?
        .strip_suffix(UNREAD)
}
