//! TypeScript and JavaScript providers (spec #23). Express: `app.METHOD(path, ..., handler)` and
//! `router.METHOD(...)` on an app or a router, under every prefix of `app.use(prefix, router)`
//! mounts, the app or router followed through `Code::origins`; the Express rules do not run
//! outside a pack with `js` files. Next.js app-router route files: `app/users/[id]/route.ts`
//! exporting `GET`, `POST`... serves `/users/{}`, route groups `(name)` dropped. `provider_shape`
//! is the one place that picks the source of a route's code shape, in the order of ADR 0009: a
//! document, then the zod schema, then the returned literals. Seams of the root reused:
//! `prefixes`, `join`, `normalize_path`, `provider`, `METHODS`, `HTTP_METHODS`.

mod clients;

pub(super) use clients::consumers;

use super::{
    CallAt, HTTP_METHODS, METHODS, Mounts, contract, join, normalize_path, prefixes, provider,
};
use crate::contract::{Found, Site};
use crate::lang::{Export, Value, literal};
use crate::literals::{Handler, Literals};
use crate::resolve::Code;
use crate::shape::Shape;

/// Express and Next.js route-file providers, with the shapes their handlers return (spec #23).
pub(super) fn providers(code: &Code, literals: &mut Literals) -> Vec<Found> {
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
    found.extend(express(code, literals));
    found
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
