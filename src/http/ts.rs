//! TypeScript and JavaScript providers (spec #23). Express: `app.METHOD(path, ..., handler)` and
//! `router.METHOD(...)` on an app or a router, under every prefix of `app.use(prefix, router)`
//! mounts, the app or router followed through `Code::origins` with `Follow::Values` (constants,
//! imports, arguments and returns), and what `express()` or `Router()` names outside the
//! repository read with `Code::outside`; the Express rules do not run outside a pack with `js`
//! files. A Backstage backend plugin is a mount of its own (`backstage`). Next.js app-router
//! route files: `app/users/[id]/route.ts`
//! exporting `GET`, `POST`... serves `/users/{}`, route groups `(name)` dropped. `provider_shape`
//! is the one place that picks the source of a route's code shape, in the order of ADR 0009: a
//! document, then the zod schema, then the returned literals. Seams of the root reused:
//! `prefixes`, `join`, `normalize_path`, `provider`, `METHODS`, `HTTP_METHODS`.

mod backstage;
mod clients;
mod config;

use std::collections::BTreeSet;

pub(super) use backstage::{name, unread};
pub(super) use clients::consumers;

use super::{
    CallAt, HTTP_METHODS, METHODS, Mounts, contract, join, normalize_path, prefixes, provider,
};
use crate::contract::{Found, Role, Site};
use crate::lang::{Export, Value, literal};
use crate::literals::{Handler, Literals};
use crate::resolve::{Code, Follow, Of};
use crate::shape::Shape;

/// Express and Next.js route-file providers, with the shapes their handlers return (spec #23),
/// and each route a Backstage plugin's mount serves, with the plugin's id (`backstage::name`).
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
            // `export const GET = withAuth(...)` defines no function: the file serves it.
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
                shape,
                ..provider(site, contract(exported, &route))
            });
        }
    }
    let (express, mounted) = express(code, literals);
    found.extend(express);
    (found, mounted)
}

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

/// What `express()`, `Router()` and `express.Router()` return: an app or a router.
const ROUTERS: &[&str] = &["express", "Router", "express.Router"];

/// The package whose default export is a router factory, under any local name.
const PROMISE_ROUTER: &str = "express-promise-router";

/// Whether the call `router` creates an Express app or router: `express()`, `Router()` or
/// `express.Router()`, unless an import from another module than `express` binds the name; or
/// a call of the default import of `express-promise-router`, whatever it is named.
fn is_express<'a>(code: &Code<'a>, (path, n): CallAt<'a>) -> bool {
    let call = &code.files()[path].calls[n];
    let outside = code.outside(path, &call.scope, &call.callee, false);
    if let Some(o) = &outside
        && o.direct
        && o.imported
            .as_ref()
            .is_some_and(|(p, _)| *p == PROMISE_ROUTER)
    {
        return true;
    }
    ROUTERS.contains(&call.callee.as_str())
        && outside.is_none_or(|o| o.spelled.starts_with("express."))
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
fn express(code: &Code, literals: &mut Literals) -> (Vec<Found>, Vec<(String, String)>) {
    // The names Express is known by are spellings (`Router()`), which any other language may
    // give its own functions and types.
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
    // The apps and routers `arg` may be, as a mount gives it.
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
                // Anything else may be a path Meada cannot read (a parameter, a member of an
                // imported object): what it mounts is served under no prefix.
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
    // A Backstage plugin is a mount node nothing mounts: its routers take `/api/<pluginId>`.
    let mut plugins = BTreeSet::new();
    let mut unmounted = Vec::new();
    for (plugin, id, uses) in backstage::plugins(code) {
        // A plugin that mounts nothing has no routes to leave at the root.
        if uses.is_empty() {
            continue;
        }
        let Some(id) = id else {
            let (path, index) = plugin;
            unmounted.push(Found {
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
    // The routes each plugin's mount serves, by contract and plugin id (`backstage::name`).
    let mut served = Vec::new();
    let plugin = |n: CallAt| plugins.contains(&n).then(String::new);
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
        // The prefixes whose mounts start at a Backstage plugin: the routes it serves.
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
