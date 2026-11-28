mod backstage;
mod clients;
mod config;

use std::collections::BTreeSet;

pub(super) use backstage::{name, unread};
pub(super) use clients::consumers;
pub(crate) use config::Config;
#[cfg(test)]
pub(crate) use config::probe as app_config_probe;
pub(crate) use config::{DEPTH as INCLUDE_DEPTH, INCLUDES, Reason as IncludeReason, config_inputs};
pub(in crate::http) use config::{unfollowed, unfollowed_of};

use super::{
    CallAt, HTTP_METHODS, METHODS, Mounts, contract, join, normalize_path, prefixes, provider,
};
use crate::contract::{Found, Role, Site};
use crate::lang::{Export, Value, literal};
use crate::literals::{Handler, Literals};
use crate::resolve::{Code, Follow, Of};
use crate::shape::Shape;

pub(super) fn providers(
    code: &Code,
    literals: &mut Literals,
) -> (Vec<Found>, Vec<(String, String)>) {
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
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
            let defined = facts.defs.iter().any(|d| d.name == *local);
            let (site, shape) = if defined {
                let site = Site::Def {
                    path: path.into(),
                    name: local.clone(),
                };
                (
                    site,
                    provider_shape(code, literals, path, local, Handler::Next),
                )
            } else {
                (Site::File(path.into()), None)
            };
            found.push(Found {
                identity: None,
                shape,
                ..provider(site, contract(exported, &route))
            });
        }
    }
    let (express, mounted) = express(code, literals);
    found.extend(express);
    (found, mounted)
}

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

const ROUTERS: &[&str] = &["express", "Router", "express.Router"];

const PROMISE_ROUTER: &str = "express-promise-router";

fn is_express<'a>(code: &Code<'a>, (path, n): CallAt<'a>) -> bool {
    let call = &code.files()[path].calls[n];
    let outside = code.outside(path, &call.scope, &call.callee, false);
    if let Some(o) = &outside
        && o.direct
        && o.imported
            .as_ref()
            .is_some_and(|(p, name)| *p == PROMISE_ROUTER && name == "default")
    {
        return true;
    }
    ROUTERS.contains(&call.callee.as_str())
        && outside.is_none_or(|o| o.spelled.starts_with("express."))
}

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

// ponytail: `router.route(path)` chains and `require` are not followed.
fn express(code: &Code, literals: &mut Literals) -> (Vec<Found>, Vec<(String, String)>) {
    if !code.pack().extensions().contains(&"js") {
        return Default::default();
    }
    let routers = |path, scope, name| -> Vec<CallAt> {
        let origins = code.origins(
            path,
            scope,
            Of::Name(name),
            Follow::Values {
                passes: crate::resolve::passes_nothing,
            },
        );
        (origins.into_iter().filter_map(|o| o.call))
            .filter(|&r| is_express(code, r))
            .collect()
    };
    let mounted = |path, scope, arg| -> Vec<CallAt> {
        let origins = code.origins(
            path,
            scope,
            Of::Value(arg),
            Follow::Values {
                passes: crate::resolve::passes_nothing,
            },
        );
        (origins.into_iter().filter_map(|o| o.call))
            .filter(|&c| is_express(code, c))
            .collect()
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
                None | Some((_, Value::Call(_) | Value::Function(_) | Value::New(..))) => {
                    (Some(String::new()), args)
                }
                Some((Value::Ref(name), _))
                    if code.definition(path, &call.scope, name).is_some() =>
                {
                    (Some(String::new()), args)
                }
                Some((first, _))
                    if (code.origins(
                        path,
                        &call.scope,
                        Of::Value(first),
                        Follow::Values {
                            passes: crate::resolve::passes_nothing,
                        },
                    ))
                    .into_iter()
                    .filter_map(|o| o.call)
                    .any(|c| is_express(code, c)) =>
                {
                    (Some(String::new()), args)
                }
                // ponytail: so is a router mounted after a package's middleware passed uncalled
                // (`app.use(requireAuth, router)`); tell package handlers from package strings
                // through `Code::origins` if that loses routes that matter.
                _ => (None, &args[1..]),
            };
            for arg in rest {
                for child in mounted(path, &call.scope, arg) {
                    let parents = mounts.entry(child).or_default();
                    parents.extend(ats.iter().map(|&at| (at, prefix.clone())));
                }
            }
        }
    }
    let mut plugins = BTreeSet::new();
    let mut unmounted = Vec::new();
    for (plugin, id, uses) in backstage::plugins(code) {
        if uses.is_empty() {
            continue;
        }
        let Some(id) = id else {
            let (path, index) = plugin;
            unmounted.push(Found {
                identity: None,
                role: Role::Unmounted,
                contract: None,
                ..provider(
                    Site::Call {
                        path: path.into(),
                        index,
                    },
                    String::new(),
                )
            });
            continue;
        };
        plugins.insert(plugin);
        for (path, scope, arg) in uses {
            for child in mounted(path, scope, arg) {
                let parents = mounts.entry(child).or_default();
                parents.push((plugin, Some(backstage::mount(&id))));
            }
        }
    }
    let mut found = unmounted;
    let mut served = Vec::new();
    let plugin = |n: CallAt| plugins.contains(&n).then(String::new);
    for (path, n, at, method) in routes {
        let scope = &code.files()[path].calls[n].scope;
        let Some([first, .., handler]) = code.args(path, n) else {
            continue;
        };
        let Value::Str(parts) = code.value(path, scope, first) else {
            continue;
        };
        let Some(route) = literal(&parts) else {
            continue;
        };
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
                identity: None,
                shape: shape.clone(),
                ..provider(site, contract(&method, &full))
            });
        }
        if plugins.is_empty() {
            continue;
        }
        for prefix in prefixes(at, &mounts, &plugin, &mut Vec::new()) {
            let id = prefix
                .strip_prefix("/api/")
                .and_then(|p| p.split('/').next());
            let full = normalize_path(&join(&prefix, &route));
            served.extend(id.map(|id| (contract(&method, &full), id.to_string())));
        }
    }
    (found, served)
}
