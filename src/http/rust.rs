//! actix-web and Rocket providers (spec #30); the axum rules live in `rust/axum.rs`. A handler is
//! a function a route attribute sits on (`#[get("/x")]`, the pack emits it as a call that
//! decorates the function) or a function a route builder names
//! (`.route(path, web::get().to(h))`). Where it is served follows the value of the app, scope,
//! resource or Rocket instance it is registered on, through the same mounts and prefixes as axum
//! (`prefixes`): a `web::scope(prefix)` is mounted under the prefix of what takes it, a
//! `.configure(f)` mounts the service configuration `f` receives, and
//! `mount(prefix, routes![a, b])` puts the listed handlers under the prefix.
//!
//! What a value is made by is asked once, `Code::origins` with `Follow::Values`: the calls the
//! flow reaches, each with the name it has outside the repository. That name is read as a path
//! (`Code::outside` with the call's `by_path`), so a function of the file spelled like a crate
//! (`fn rocket()` calling `rocket::build()`) does not hide the crate. A route attribute and a
//! builder the reader meets in place are read the same way; a type written on a parameter is
//! read as a value (`Code::outside` without the flag).
//!
//! A handler is a node of its own: one a call names (as an argument, `routes![..]` included)
//! is served only where a mount Meada reads puts it, and nowhere when it reads none. One no call
//! names is served at its own path, with no prefix.

mod axum;

use std::cell::{Cell, OnceCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{
    ANY, CallAt, Mounts, PARAM, handler_site, join, normalize_path, path_literal, prefixes,
    provider,
};
use crate::contract::{Found, Site};
use crate::lang::rust::frameworks::{self, Made, VERBS};
use crate::lang::{Call, Class, Value};
use crate::resolve::{Code, Follow, Of, Outside, scope_of};

/// How many values the expansions of `items` (constants, the functions a call answers) may add,
/// over every call of the repository; past it a list is read no further.
// ponytail: a crafted repository must not make every call expand a long constant; what is
// left unread is taken as registered where Meada does not see it, so it is served nowhere.
const ITEM_WORK: usize = 1 << 14;

/// The methods `actix_web::web` has a route builder for: `web::get()`, `web::post()`.
const METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "trace"];

/// What the outside name of a call makes, when it makes an app, a scope, a resource or a Rocket
/// instance.
fn made_of(outside: &Outside) -> Option<Made> {
    let made = frameworks::made(&outside.spelled)?;
    matches!(
        made,
        Made::App | Made::Scope | Made::Resource | Made::Rocket
    )
    .then_some(made)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Framework {
    Actix,
    Rocket,
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
    /// What `items` may still visit.
    work: Cell<usize>,
    /// The names the files of each crate mention, by crate root; `None` for a crate one of
    /// whose files does not tell.
    mentioned: OnceCell<HashMap<&'a str, Option<BTreeSet<&'a str>>>>,
}

/// `path` with each `{name}` or `{name:regex}` as one parameter, whatever the regex holds;
/// `None` when a brace is not closed.
pub(super) fn actix_path(path: &str) -> Option<String> {
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
            work: Cell::new(ITEM_WORK),
            mentioned: OnceCell::new(),
        }
    }

    /// Takes `n` from the budget of `items`; false once it is spent.
    fn charge(&self, n: usize) -> bool {
        let left = self.work.get();
        self.work.set(left.saturating_sub(n));
        left >= n
    }

    /// The root of the crate `path` belongs to, as its module tree says (`crate`).
    fn root(&self, path: &'a str) -> &'a str {
        self.code.import_file(path, "crate").unwrap_or(path)
    }

    /// Whether code Meada does not read may register the function `name` of `path`: a file of
    /// its crate (of any crate, when the file is one another crate may import) mentions the
    /// name (`Facts::mentions`), or does not say what it mentions.
    fn mentioned(&self, path: &'a str, name: &str) -> bool {
        let crates = self.mentioned.get_or_init(|| {
            let mut crates: HashMap<&str, Option<BTreeSet<&str>>> = HashMap::new();
            for &file in self.code.files().keys() {
                let names = crates
                    .entry(self.root(file))
                    .or_insert(Some(BTreeSet::new()));
                match (names, self.code.mentions(file)) {
                    (Some(names), Some(found)) => names.extend(found.iter().map(String::as_str)),
                    (names, _) => *names = None,
                }
            }
            crates
        });
        let name = name.rsplit('.').next().unwrap_or(name);
        let mentions =
            |names: &Option<BTreeSet<&str>>| names.as_ref().is_none_or(|n| n.contains(name));
        match self.code.published(path) {
            true => crates.values().any(mentions),
            false => crates.get(self.root(path)).is_none_or(mentions),
        }
    }

    /// The crate a bare route attribute comes from, when the `#[macro_use] extern crate x;` of
    /// its crate's root brings it in.
    fn macro_use(&self, path: &'a str) -> Option<&'a str> {
        let root = self.code.files().get(self.root(path))?;
        let krates = root.macro_use.iter();
        let mut krates = krates.filter(|k| matches!(k.as_str(), "actix_web" | "rocket"));
        let first = krates.next()?;
        krates.next().is_none().then_some(first.as_str())
    }

    /// What the call `at` makes, when it makes an app, a scope, a resource or a Rocket instance.
    fn made(&self, (path, n): CallAt<'a>) -> Option<Made> {
        let call = &self.code.files()[path].calls[n];
        made_of(
            &self
                .code
                .outside(path, &call.scope, &call.callee, call.by_path)?,
        )
    }

    fn kind(&self, place: Place<'a>) -> Option<Made> {
        match place {
            Place::Made(at) => self.made(at),
            Place::Config(..) => Some(Made::Config),
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
                if self.code.outside(path, &r.scope, t, false).is_some_and(|o| {
                    o.spelled.starts_with("actix_web.") && o.spelled.ends_with(".ServiceConfig")
                }))
        })
    }

    /// The places the value a method is called on may be: the apps, scopes, resources and
    /// Rocket instances the value flow reaches, and the service configuration it is.
    fn places(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Vec<Place<'a>> {
        let origins = self
            .code
            .origins(path, scope, Of::Value(value), Follow::Values);
        let mut out: Vec<Place> = (origins.into_iter())
            .filter(|o| o.outside.as_ref().and_then(made_of).is_some())
            .filter_map(|o| o.call.map(Place::Made))
            .collect();
        if self.configuration(path, scope, value) {
            out.push(Place::Config(path, scope));
        }
        out
    }

    /// Whether `value` is the service configuration the function `scope` receives, or the
    /// result of its methods called on it (`cfg.service(a).route(..)`).
    fn configuration(&self, path: &'a str, scope: &'a str, value: &'a Value) -> bool {
        let code = self.code;
        let invocation = match value {
            Value::Ref(name) => return self.config(path, scope, name),
            Value::Call(invocation) => invocation,
            _ => return false,
        };
        let chain = code
            .call_index(path, &invocation.range)
            .and_then(|at| code.chain(path, at));
        let passes = frameworks::passes(Made::Config);
        let Some(calls) = chain else {
            return false;
        };
        let passed = calls.iter().all(|&at| {
            let member = code
                .call(path, at)
                .and_then(|c| c.callee.rsplit('.').next());
            member.is_some_and(|m| passes.contains(&m))
        });
        let start = calls.last().and_then(|&at| code.receiver(path, at));
        passed && matches!(start, Some(Value::Ref(name)) if self.config(path, scope, name))
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
                            let held = leaves(value);
                            if !self.charge(held.len()) {
                                break;
                            }
                            out.extend(held.into_iter().map(|v| (at_path, name, v)));
                        }
                    }
                    if out.len() == before {
                        out.push((path, scope, leaf));
                    }
                }
                Value::Ref(name) => match code.constant(path, scope, name) {
                    Some(((at, name), held)) => {
                        let held = leaves(held);
                        if !self.charge(held.len()) {
                            break;
                        }
                        out.extend(held.into_iter().map(|v| (at, scope_of(name), v)));
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
                let Some(call_at) = code.call(path, n) else {
                    continue;
                };
                // A bare attribute no `use` binds comes from the crate `#[macro_use]` names.
                let bare = (!call.by_path
                    && code.definition(path, &call.scope, &call.callee).is_none())
                .then(|| self.macro_use(path))
                .flatten()
                .map(|krate| format!("{krate}.{}", call.callee));
                let outside = code.outside(path, &call.scope, &call.callee, call.by_path);
                let Some(name) = outside.map(|o| o.spelled).or(bare) else {
                    continue;
                };
                let parts: Vec<&str> = name.split('.').collect();
                let (framework, attribute) = match parts.as_slice() {
                    ["actix_web", attribute] => (Framework::Actix, *attribute),
                    ["rocket", attribute] => (Framework::Rocket, *attribute),
                    _ => continue,
                };
                let call = call_at;
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
                    // Only a name, a call or a list holding one can name a handler.
                    let mut names = leaves(arg).into_iter();
                    if !names.any(|l| matches!(l, Value::Ref(_) | Value::Call(_))) {
                        continue;
                    }
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
                self.pass_config(path, call);
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

    /// `f(cfg)`: the service configuration a function passes on to a function of the repository
    /// (answered exactly) that takes one there is mounted where the first is.
    fn pass_config(&mut self, path: &'a str, call: &'a Call) {
        let code = self.code;
        let (scope, plain) = (call.scope.as_str(), call.receiver.is_none());
        let Some(target) = code.definition(path, scope, &call.callee).filter(|_| plain) else {
            return;
        };
        let defs = code.files()[target.path].defs.iter();
        let Some(params) = defs
            .into_iter()
            .find(|d| d.name == target.name)
            .map(|d| &d.params)
        else {
            return;
        };
        for (arg, param) in call.args.iter().zip(params) {
            if self.configuration(path, scope, arg)
                && self.config(target.path, target.name, &param.name)
            {
                let (child, parent) = (
                    Place::Config(target.path, target.name),
                    Place::Config(path, scope),
                );
                self.mount(child, &[parent], &Some(String::new()));
            }
        }
    }

    /// `parents` of the kinds `kinds`.
    fn only(&self, parents: &[Place<'a>], kinds: &[Made]) -> Vec<Place<'a>> {
        let of = |p: &Place<'a>| self.kind(*p).is_some_and(|k| kinds.contains(&k));
        parents.iter().copied().filter(of).collect()
    }

    /// `x.service(h)`, `x.service(web::scope(prefix)...)`, `x.service(web::resource(path)...)`
    /// and a tuple of them.
    fn service(&mut self, path: &'a str, call: &'a Call, parents: &[Place<'a>]) {
        let parents = self.only(parents, &[Made::App, Made::Scope, Made::Config]);
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
            for origin in code.origins(path, scope, Of::Value(arg), Follow::Values) {
                let made = origin.outside.as_ref().and_then(made_of);
                let (true, Some(child)) = (
                    matches!(made, Some(Made::Scope | Made::Resource)),
                    origin.call,
                ) else {
                    continue;
                };
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
            [route, value] => (Some(route), value, [Made::App, Made::Scope, Made::Config]),
            [value] => (None, value, [Made::Resource; 3]),
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
        let outside = code.outside(path, &ctor.scope, &ctor.callee, ctor.by_path)?;
        let method = match outside.spelled.strip_prefix("actix_web.web.")? {
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
        let parents = self.only(parents, &[Made::App, Made::Scope, Made::Config]);
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
        let parents = self.only(parents, &[Made::Rocket]);
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
        // resource or a configuration, only where a mount puts it. A handler no code mentions is
        // served at its own path; one that some code mentions without Meada reading a
        // registration may be registered there, so it is served nowhere.
        let root = |place: Place<'a>| match place {
            Place::Handler(path, name) => (!self.mentioned(path, name)).then(String::new),
            Place::Made(_) | Place::Config(..) => {
                (matches!(self.kind(place), Some(Made::App | Made::Rocket))).then(String::new)
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
    let mut found = axum::providers(code);
    let mut web = Web::new(code);
    web.attributed();
    web.read();
    found.extend(web.found());
    found
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
