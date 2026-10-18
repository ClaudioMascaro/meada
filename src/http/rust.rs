//! actix-web and Rocket providers (spec #30). A handler is a function a route attribute sits on
//! (`#[get("/x")]`, the pack emits it as a call that decorates the function) or a function a
//! route builder names (`.route(path, web::get().to(h))`). Where it is served follows the value
//! of the app, scope, resource or Rocket instance it is registered on, through the same
//! mounts and prefixes as axum (`prefixes`): a `web::scope(prefix)` is mounted under the prefix
//! of what takes it, a `.configure(f)` mounts the service configuration `f` receives, and
//! `mount(prefix, routes![a, b])` puts the listed handlers under the prefix.
//!
//! A handler is a node of its own: one a call names (as an argument, `routes![..]` included)
//! is served only where a mount Meada reads puts it, and nowhere when it reads none. One no call
//! names is served at its own path, with no prefix.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    ANY, CallAt, METHOD_CHAIN, Mounts, PARAM, handler_site, join, normalize_path, path_literal,
    prefixes, provider,
};
use crate::contract::{Found, Site};
use crate::lang::{Call, Class, Value};
use crate::resolve::{Code, scope_of};

/// The methods of a `ServiceConfig` that hand it back.
const CONFIG_PASSES: &[&str] = &["service", "route", "configure", "app_data", "data"];

/// The methods `actix_web::web` has a route builder for: `web::get()`, `web::post()`.
const METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "trace"];

/// The HTTP verbs an `actix_web::web::method(Method::X)` may be given.
const VERBS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Framework {
    Actix,
    Rocket,
}

/// What a place of routes is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    App,
    Scope,
    Resource,
    Rocket,
    /// The service configuration a function receives.
    Config,
}

/// Where routes are registered or served, a node of the mounts.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place<'a> {
    /// The call that makes an app, a scope, a resource or a Rocket instance.
    Made(CallAt<'a>),
    /// The service configuration the function (file, qualified name) receives.
    Config(&'a str, &'a str),
    /// The function (file, qualified name) a route attribute sits on.
    Handler(&'a str, &'a str),
}

/// A function with route attributes: the routes they give it, as method and path.
struct Handler {
    framework: Framework,
    routes: Vec<(String, String)>,
}

/// A route a builder registers: `.route(path, web::get().to(h))`.
struct Route<'a> {
    path: &'a str,
    scope: &'a str,
    parents: Vec<Place<'a>>,
    route: String,
    method: String,
    handler: Option<&'a Value>,
}

struct Web<'c, 'a> {
    code: &'c Code<'a>,
    handlers: BTreeMap<(&'a str, &'a str), Handler>,
    mounts: Mounts<'a, Place<'a>>,
    routes: Vec<Route<'a>>,
}

/// `path` with each `{name}` or `{name:regex}` as one parameter, whatever the regex holds;
/// `None` when a brace is not closed.
fn actix_path(path: &str) -> Option<String> {
    let (mut out, mut open) = (String::new(), 0usize);
    for c in path.chars() {
        match c {
            '{' => {
                if open == 0 {
                    out.push_str(PARAM);
                }
                open += 1;
            }
            '}' => open = open.checked_sub(1)?,
            c if open == 0 => out.push(c),
            _ => {}
        }
    }
    (open == 0).then_some(out)
}

/// The values `value` holds, the items of a tuple or a list (`.service((a, b))`) included.
fn leaves(value: &Value) -> Vec<&Value> {
    let (mut out, mut work) = (Vec::new(), vec![value]);
    while let Some(value) = work.pop() {
        match value {
            Value::List(items) => work.extend(items.iter().rev()),
            value => out.push(value),
        }
    }
    out
}

impl<'c, 'a> Web<'c, 'a> {
    fn new(code: &'c Code<'a>) -> Self {
        Web {
            code,
            handlers: BTreeMap::new(),
            mounts: Mounts::new(),
            routes: Vec::new(),
        }
    }

    /// What the call `at` makes, when it makes an app, a scope, a resource or a Rocket instance.
    fn made(&self, (path, n): CallAt<'a>) -> Option<Kind> {
        let call = &self.code.files()[path].calls[n];
        let name = self.code.external_of(path, call)?;
        match name.as_str() {
            "actix_web.App.new" | "actix_web.App.default" => Some(Kind::App),
            "actix_web.web.scope" => Some(Kind::Scope),
            "actix_web.web.resource" => Some(Kind::Resource),
            "rocket.build" | "rocket.custom" => Some(Kind::Rocket),
            _ => None,
        }
    }

    fn kind(&self, place: Place<'a>) -> Option<Kind> {
        match place {
            Place::Made(at) => self.made(at),
            Place::Config(..) => Some(Kind::Config),
            Place::Handler(..) => None,
        }
    }

    /// The function `name`, written in `scope`, is the service configuration of: a parameter
    /// whose type is actix-web's `ServiceConfig`.
    fn config(&self, path: &'a str, scope: &'a str, name: &str) -> bool {
        let receivers = &self.code.files()[path].receivers;
        receivers.iter().any(|r| {
            r.scope == scope
                && r.name == name
                && matches!(&r.class, Class::Type(t)
                if self.code.external(path, &r.scope, t).is_some_and(|t| {
                    t.starts_with("actix_web.") && t.ends_with(".ServiceConfig")
                }))
        })
    }

    /// The places the value a method is called on may be: the apps, scopes, resources and
    /// Rocket instances the value flow reaches, and the service configuration it is.
    fn places(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Vec<Place<'a>> {
        let origins = self.code.value_origins(path, scope, value).into_iter();
        let mut out: Vec<Place> = origins
            .filter(|&at| self.made(at).is_some())
            .map(Place::Made)
            .collect();
        if self.configuration(path, scope, value) {
            out.push(Place::Config(path, scope));
        }
        out
    }

    /// Whether `value` is the service configuration the function `scope` receives, or the
    /// result of its methods called on it (`cfg.service(a).route(..)`).
    fn configuration(&self, path: &'a str, scope: &'a str, value: &'a Value) -> bool {
        let mut value = value;
        for _ in 0..METHOD_CHAIN {
            match value {
                Value::Ref(name) => return self.config(path, scope, name),
                Value::Call(invocation) => {
                    let call = (self.code.call_index(path, &invocation.range))
                        .and_then(|at| self.code.call(path, at));
                    let Some(call) = call else {
                        return false;
                    };
                    let member = call.callee.rsplit('.').next().unwrap_or_default();
                    match &call.receiver {
                        Some(next) if CONFIG_PASSES.contains(&member) => value = next,
                        _ => return false,
                    }
                }
                _ => return false,
            }
        }
        false
    }

    /// The items `value`, written in `scope` of `path`, lists or names: a tuple's or a list's
    /// (`routes![a, b]`), those of a constant it names, and those of what a function of the
    /// repository that the call answers exactly returns. Each with the file and scope it is
    /// written in.
    fn items(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
    ) -> Vec<(&'a str, &'a str, &'a Value)> {
        let code = self.code;
        let mut out = Vec::new();
        for leaf in leaves(value) {
            match leaf {
                Value::Call(invocation) => {
                    // A function of the repository the call answers exactly: what it returns,
                    // else the call itself.
                    let function = code.definition(path, scope, &invocation.callee);
                    let at = code.call_index(path, &invocation.range);
                    let returned = at.map_or(Vec::new(), |at| code.returned(path, at));
                    let before = out.len();
                    for (at_path, name, value) in returned {
                        if function.is_some_and(|t| (t.path, t.name) == (at_path, name)) {
                            out.extend(leaves(value).into_iter().map(|v| (at_path, name, v)));
                        }
                    }
                    if out.len() == before {
                        out.push((path, scope, leaf));
                    }
                }
                Value::Ref(name) => match code.constant(path, scope, name) {
                    Some(((at, name), held)) => {
                        let held = leaves(held).into_iter();
                        out.extend(held.map(|v| (at, scope_of(name), v)));
                    }
                    None => out.push((path, scope, leaf)),
                },
                leaf => out.push((path, scope, leaf)),
            }
        }
        out
    }

    /// The attributed handlers: a function with route attributes of actix-web or Rocket, and
    /// the method and path of each.
    fn attributed(&mut self) {
        let code = self.code;
        for (&path, facts) in code.files() {
            for (n, call) in facts.calls.iter().enumerate() {
                let Some(def) = &call.decorates else {
                    continue;
                };
                let Some(name) = code.external_of(path, call) else {
                    continue;
                };
                let (framework, attribute) = match name.split_once('.') {
                    Some(("actix_web", attribute)) => (Framework::Actix, attribute),
                    Some(("rocket", attribute)) => (Framework::Rocket, attribute),
                    _ => continue,
                };
                let Some(call) = code.call(path, n) else {
                    continue;
                };
                let Some(first) = call.args.first() else {
                    continue;
                };
                let Some(route) = path_literal(code.value(path, &call.scope, first)) else {
                    continue;
                };
                let route = match framework {
                    Framework::Actix => actix_path(&route),
                    Framework::Rocket => Some(route),
                };
                let Some(route) = route else {
                    continue;
                };
                let methods = match (attribute, framework) {
                    ("route", Framework::Actix) => {
                        // One method per `method = ".."`, none readable if any is not.
                        let given = call.kwargs.iter().filter(|(k, _)| k == "method");
                        let given: Option<Vec<String>> = given
                            .map(|(_, v)| path_literal(code.value(path, &call.scope, v)))
                            .map(|m| m.map(|m| m.to_uppercase()))
                            .collect();
                        given.filter(|m| !m.is_empty())
                    }
                    ("route", Framework::Rocket) => None,
                    (method, _) => Some(vec![method.to_uppercase()]),
                };
                let Some(methods) = methods else {
                    continue;
                };
                let handler = self
                    .handlers
                    .entry((path, def.as_str()))
                    .or_insert(Handler {
                        framework,
                        routes: Vec::new(),
                    });
                // An attribute of the other framework on the same function is not read.
                if handler.framework != framework {
                    continue;
                }
                handler
                    .routes
                    .extend(methods.into_iter().map(|m| (m, route.clone())));
            }
        }
    }

    /// Mounts `child` on each of `parents`, under `segment`.
    fn mount(&mut self, child: Place<'a>, parents: &[Place<'a>], segment: &Option<String>) {
        let mounted = self.mounts.entry(child).or_default();
        mounted.extend(parents.iter().map(|&at| (at, segment.clone())));
    }

    /// The handler a value names, when it is an attributed one of `framework`.
    fn handler_named(
        &self,
        (path, scope, value): (&'a str, &'a str, &'a Value),
    ) -> Option<(Place<'a>, Framework)> {
        let Value::Ref(name) = value else {
            return None;
        };
        let target = self.code.definition(path, scope, name)?;
        let handler = self.handlers.get(&(target.path, target.name))?;
        Some((Place::Handler(target.path, target.name), handler.framework))
    }

    /// Reads every call: a function an attribute sits on that any call names is registered
    /// (served where a mount reads, nowhere else), and the calls that register or mount.
    fn read(&mut self) {
        let code = self.code;
        let named: BTreeSet<&str> = (self.handlers.keys())
            .map(|(_, name)| name.rsplit('.').next().unwrap_or(name))
            .collect();
        for (&path, facts) in code.files() {
            for n in 0..facts.calls.len() {
                let Some(call) = code.call(path, n) else {
                    continue;
                };
                if call.decorates.is_some() {
                    continue;
                }
                for arg in &call.args {
                    for item in self.items(path, &call.scope, arg) {
                        let Value::Ref(name) = item.2 else {
                            continue;
                        };
                        if !named.contains(name.rsplit('.').next().unwrap_or(name)) {
                            continue;
                        }
                        if let Some((handler, _)) = self.handler_named(item) {
                            self.mounts.entry(handler).or_default();
                        }
                    }
                }
                let member = call.callee.rsplit('.').next().unwrap_or_default();
                if !matches!(member, "service" | "route" | "configure" | "mount") {
                    continue;
                }
                let Some(receiver) = &call.receiver else {
                    continue;
                };
                let parents = self.places(path, &call.scope, receiver);
                match member {
                    "service" => self.service(path, call, &parents),
                    "route" => self.route(path, call, &parents),
                    "configure" => self.configure(path, call, &parents),
                    _ => self.rocket(path, call, &parents),
                }
            }
        }
    }

    /// `parents` of the kinds `kinds`.
    fn only(&self, parents: &[Place<'a>], kinds: &[Kind]) -> Vec<Place<'a>> {
        let of = |p: &Place<'a>| self.kind(*p).is_some_and(|k| kinds.contains(&k));
        parents.iter().copied().filter(of).collect()
    }

    /// `x.service(h)`, `x.service(web::scope(prefix)...)`, `x.service(web::resource(path)...)`
    /// and a tuple of them.
    fn service(&mut self, path: &'a str, call: &'a Call, parents: &[Place<'a>]) {
        let parents = self.only(parents, &[Kind::App, Kind::Scope, Kind::Config]);
        if parents.is_empty() {
            return;
        }
        let code = self.code;
        let scope = call.scope.as_str();
        let items: Vec<_> = (call.args.iter())
            .flat_map(|a| self.items(path, scope, a))
            .collect();
        for item in items {
            if let Some((handler, Framework::Actix)) = self.handler_named(item) {
                self.mount(handler, &parents, &Some(String::new()));
                continue;
            }
            let (path, scope, arg) = item;
            for child in code.value_origins(path, scope, arg) {
                if !matches!(self.made(child), Some(Kind::Scope | Kind::Resource)) {
                    continue;
                }
                // The segment a scope or a resource puts before its routes is the one it is
                // made with; one Meada cannot read is mounted nowhere.
                let (made, at) = (child.0, child.1);
                let segment = code.call(made, at).and_then(|c| {
                    let first = c.args.first()?;
                    path_literal(code.value(made, &c.scope, first)).and_then(|p| actix_path(&p))
                });
                self.mount(Place::Made(child), &parents, &segment);
            }
        }
    }

    /// `x.route(path, web::get().to(h))` on an app, a scope or a service configuration, and
    /// `resource.route(web::get().to(h))`.
    fn route(&mut self, path: &'a str, call: &'a Call, parents: &[Place<'a>]) {
        let code = self.code;
        let scope = call.scope.as_str();
        let (route, value, kinds) = match call.args.as_slice() {
            [route, value] => (Some(route), value, [Kind::App, Kind::Scope, Kind::Config]),
            [value] => (None, value, [Kind::Resource; 3]),
            _ => return,
        };
        let parents = self.only(parents, &kinds);
        let route = match route {
            Some(route) => {
                path_literal(code.value(path, scope, route)).and_then(|r| actix_path(&r))
            }
            None => Some(String::new()),
        };
        let (Some(route), false) = (route, parents.is_empty()) else {
            return;
        };
        let Some((method, handler, to)) = self.builder(path, value) else {
            return;
        };
        self.routes.push(Route {
            path,
            scope: to.scope.as_str(),
            parents,
            route,
            method,
            handler: Some(handler),
        });
    }

    /// The method a route builder serves, the handler it names and the call naming it:
    /// `web::get().to(h)`, `web::route().to(h)` (every method) or `web::method(Method::X).to(h)`.
    fn builder(&self, path: &'a str, value: &'a Value) -> Option<(String, &'a Value, &'a Call)> {
        let code = self.code;
        let Value::Call(invocation) = value else {
            return None;
        };
        let to = code.call(path, code.call_index(path, &invocation.range)?)?;
        let ([handler], Some(Value::Call(made))) = (to.args.as_slice(), &to.receiver) else {
            return None;
        };
        if to.callee.rsplit('.').next() != Some("to") {
            return None;
        }
        let ctor = code.call(path, code.call_index(path, &made.range)?)?;
        if ctor.receiver.is_some() {
            return None;
        }
        let external = code.external_of(path, ctor)?;
        let method = match external.strip_prefix("actix_web.web.")? {
            "route" => ANY.to_string(),
            "method" => {
                let verb = path_literal(code.value(path, &ctor.scope, ctor.args.first()?))?;
                VERBS.contains(&verb.as_str()).then_some(verb)?
            }
            name if METHODS.contains(&name) => name.to_uppercase(),
            _ => return None,
        };
        Some((method, handler, to))
    }

    /// `x.configure(f)`: the service configuration `f` receives is mounted on `x`, with no
    /// segment.
    fn configure(&mut self, path: &'a str, call: &'a Call, parents: &[Place<'a>]) {
        let parents = self.only(parents, &[Kind::App, Kind::Scope, Kind::Config]);
        let [Value::Ref(name)] = call.args.as_slice() else {
            return;
        };
        if let Some(t) = self.code.definition(path, &call.scope, name) {
            self.mount(
                Place::Config(t.path, t.name),
                &parents,
                &Some(String::new()),
            );
        }
    }

    /// `rocket.mount(prefix, routes![a, b])`.
    fn rocket(&mut self, path: &'a str, call: &'a Call, parents: &[Place<'a>]) {
        let code = self.code;
        let scope = call.scope.as_str();
        let parents = self.only(parents, &[Kind::Rocket]);
        let [prefix, routes] = call.args.as_slice() else {
            return;
        };
        if parents.is_empty() {
            return;
        }
        let prefix = path_literal(code.value(path, scope, prefix));
        for item in self.items(path, scope, routes) {
            if let Some((handler, Framework::Rocket)) = self.handler_named(item) {
                self.mount(handler, &parents, &prefix);
            }
        }
    }

    fn found(&self) -> Vec<Found> {
        let code = self.code;
        // An app and a Rocket instance nothing mounts are served at their root; a scope, a
        // resource or a configuration, only where a mount puts it. An unregistered handler is
        // served at its own path.
        let root = |place: Place<'a>| match place {
            Place::Handler(..) => Some(String::new()),
            Place::Made(_) | Place::Config(..) => {
                (matches!(self.kind(place), Some(Kind::App | Kind::Rocket))).then(String::new)
            }
        };
        let served = |places: &[Place<'a>]| -> BTreeSet<String> {
            let each = places
                .iter()
                .flat_map(|&p| prefixes(p, &self.mounts, &root, &mut Vec::new()));
            each.collect()
        };
        let mut found = Vec::new();
        for route in &self.routes {
            for prefix in served(&route.parents) {
                let full = normalize_path(&join(&prefix, &route.route));
                let site = handler_site(
                    code,
                    route.path,
                    route.scope,
                    route.handler,
                    &route.method,
                    &full,
                );
                found.push(provider(site, format!("http::{}::{full}", route.method)));
            }
        }
        for (&(path, name), handler) in &self.handlers {
            for prefix in served(&[Place::Handler(path, name)]) {
                for (method, route) in &handler.routes {
                    let full = normalize_path(&join(&prefix, route));
                    let site = Site::Def {
                        path: path.into(),
                        name: name.into(),
                    };
                    found.push(provider(site, format!("http::{method}::{full}")));
                }
            }
        }
        found
    }
}

/// actix-web and Rocket providers: see the module.
pub(super) fn providers(code: &Code) -> Vec<Found> {
    if !code.pack().extensions().contains(&"rs") {
        return Vec::new();
    }
    let mut web = Web::new(code);
    web.attributed();
    web.read();
    web.found()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actix_parameters_fold_whatever_their_regex_holds() {
        assert_eq!(actix_path("/a/{id}").as_deref(), Some("/a/{}"));
        assert_eq!(actix_path("/{tail:.*}").as_deref(), Some("/{}"));
        assert_eq!(actix_path("/{n:\\d{2}}/x").as_deref(), Some("/{}/x"));
        assert_eq!(actix_path("/{open"), None);
        assert_eq!(actix_path("/close}"), None);
    }
}
