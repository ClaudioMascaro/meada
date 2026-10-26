//! ASP.NET Core minimal API providers (spec #28, #110): `MapGet`, `MapPost`, `MapPut`,
//! `MapPatch`, `MapDelete` and `MapMethods(pattern, methods, handler)` called on the
//! application or on a `MapGroup(prefix)` group serve the pattern under every prefix of the
//! groups around it, at any depth.
//!
//! A builder is followed through the shared value flow (`Code::origins` with `Follow::Values`):
//! locals, method arguments, return values and the receiver of an extension method
//! (`group.MapOrders()` binds `this RouteGroupBuilder group`). Each origin is a node of the
//! mounts: a `MapGroup` call is mounted on the origins of its receiver under its prefix, and
//! the application (`WebApplication.Create(..)`, or `Build()` on a builder made by
//! `WebApplication.CreateBuilder(..)` and its siblings) is the root; any other origin is a
//! mount Meada does not read, so what it holds is served nowhere. A builder one function
//! returns keeps the calls the walk climbed to reach it (`Route::through`), and what is
//! written in that function is read with that call's arguments alone (`Code::origins_within`),
//! so two groups one factory makes from two builders keep their own prefixes. A builder used
//! under two prefixes (an extension method called on two groups) serves each route under both.
//! A factory that calls itself to make the group it returns, and a repository past
//! `MINIMAL_NODES` nodes, serve nothing.
//!
//! A lambda handler is an inline handler named for its route (`handler_site`, the pack's
//! `Kind::Inline` scope of a lambda argument); a method group (`Handlers.List`) is that method
//! when it names one, and anything else leaves the route at its file. A method of the
//! repository named like one of the framework's (`MapGet`, `MapGroup`) hides it.

use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    CallAt, METHODS, Mounts, balanced, contract, handler_site, join, normalize_path, path_literal,
    prefixes, provider,
};
use super::optional;
use crate::contract::Found;
use crate::lang::{Call, Value};
use crate::names::{one, spellings};
use crate::resolve::{Code, Of, passes_nothing};

/// The route methods of one HTTP method, with the method each serves.
const VERBS: &[(&str, &str)] = &[
    ("MapGet", "GET"),
    ("MapPost", "POST"),
    ("MapPut", "PUT"),
    ("MapPatch", "PATCH"),
    ("MapDelete", "DELETE"),
];
const MAP_METHODS: &str = "MapMethods";
const MAP_GROUP: &str = "MapGroup";
const APP: &str = "Microsoft.AspNetCore.Builder.WebApplication.";
/// What makes an application: `WebApplication.Create(..)`.
const CREATE: &str = "Create";
/// What makes a builder whose `Build()` is an application.
const BUILDERS: &[&str] = &["CreateBuilder", "CreateSlimBuilder", "CreateEmptyBuilder"];
const HTTP_METHODS: &str = "Microsoft.AspNetCore.Http.HttpMethods.";
/// The conventions a group is configured with, each returning the group it is called on
/// (`app.MapGroup("/api").RequireAuthorization()`).
// ponytail: the common ones; a group configured through another call is served nowhere.
const CONVENTIONS: &[&str] = &[
    "AddEndpointFilter",
    "AllowAnonymous",
    "DisableAntiforgery",
    "RequireAuthorization",
    "RequireCors",
    "RequireRateLimiting",
    "WithGroupName",
    "WithMetadata",
    "WithOpenApi",
    "WithTags",
];

/// An origin of a builder: the call that made it and the calls the walk climbed through to
/// reach it, outermost first (`Route::through`).
type Node<'a> = (CallAt<'a>, Vec<CallAt<'a>>);

/// The method a callee calls: `MapGet` of `app.MapGet`, `AddEndpointFilter` of
/// `g.AddEndpointFilter<F>`.
fn method(callee: &str) -> &str {
    let name = callee.rsplit('.').next().unwrap_or(callee);
    name.split('<').next().unwrap_or(name)
}

/// How many nodes the mounts of one repository may hold.
// ponytail: real apps make tens of groups; past this (a crafted web of factories) nothing is
// served, rather than routes under part of their prefixes.
const MINIMAL_NODES: usize = 4096;

struct Minimal<'c, 'a> {
    code: &'c Code<'a>,
    /// Whether a node was dropped past `MINIMAL_NODES`.
    overflow: bool,
    /// The nodes whose origins went round a factory that calls itself (`origins`): what is
    /// mounted above them is not known, so a route reaching one is served nowhere.
    cycles: BTreeSet<usize>,
    /// Each node by its place in `nodes`.
    ids: BTreeMap<Node<'a>, usize>,
    nodes: Vec<Node<'a>>,
    /// The nodes not yet read.
    pending: Vec<usize>,
    roots: BTreeSet<usize>,
    mounts: Mounts<'a, usize>,
}

impl<'c, 'a> Minimal<'c, 'a> {
    /// Whether the call at `index` of `path` is the framework's: no function of the repository
    /// answers it (a method of the repository named `MapGet` hides it only there).
    fn framework(&self, path: &'a str, index: usize) -> bool {
        self.code.reaches(path, index).is_none()
    }

    /// The place of `node` in `nodes`, read later when it is new; `None` past `MINIMAL_NODES`.
    fn intern(&mut self, node: Node<'a>) -> Option<usize> {
        if let Some(&id) = self.ids.get(&node) {
            return Some(id);
        }
        if self.nodes.len() >= MINIMAL_NODES {
            self.overflow = true;
            return None;
        }
        let id = self.nodes.len();
        self.ids.insert(node.clone(), id);
        self.nodes.push(node);
        self.pending.push(id);
        Some(id)
    }

    /// The origins of `value`, written in `scope` of `path` inside the functions the calls of
    /// `context` reach, which bind their parameters (`Code::origins_within`); `None` when one
    /// climbs a call twice, round a factory that calls itself.
    fn origins(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        context: &[CallAt<'a>],
    ) -> Option<Vec<Node<'a>>> {
        let found =
            (self.code).origins_within(path, scope, Of::Value(value), passes_nothing, context);
        let mut out = BTreeSet::new();
        for origin in found {
            let Some(call) = origin.call else {
                continue;
            };
            for route in origin.routes {
                let distinct: BTreeSet<&CallAt<'a>> = route.through.iter().collect();
                if distinct.len() < route.through.len() {
                    return None;
                }
                out.insert((call, route.through));
            }
        }
        Some(out.into_iter().collect())
    }

    /// Whether the call at `index` of `path` is the framework's `WebApplication.<name>` for
    /// one of `names`, as the file's usings spell it.
    fn app_call(&self, path: &'a str, index: usize, names: &[&str]) -> bool {
        let call = &self.code.files()[path].calls[index];
        let spelled = spellings(self.code, path, &call.scope, &call.callee);
        one(&spelled, |s| {
            s.strip_prefix(APP).is_some_and(|n| names.contains(&n))
        })
        .is_some()
    }

    /// Reads the node `id`: a `MapGroup` call is mounted on the origins of its receiver under
    /// its prefix, a convention (`CONVENTIONS`) with none, the application is a root, and
    /// anything else is left unread.
    fn read(&mut self, id: usize) {
        let code = self.code;
        let ((path, index), context) = self.nodes[id].clone();
        let Some(call) = code.call(path, index) else {
            return;
        };
        let scope = call.scope.as_str();
        let name = method(&call.callee);
        if self.app_call(path, index, &[CREATE]) {
            self.roots.insert(id);
            return;
        }
        let Some(receiver) = call.receiver.as_ref() else {
            return;
        };
        // `Build()` on what the framework's builder made is the framework's.
        if name == "Build" {
            let made = self
                .origins(path, scope, receiver, &context)
                .unwrap_or_default();
            if !made.is_empty() && (made.iter()).all(|((p, n), _)| self.app_call(p, *n, BUILDERS)) {
                self.roots.insert(id);
            }
            return;
        }
        let prefix = match name {
            MAP_GROUP => (call.args.first())
                .or_else(|| call.keyword("prefix"))
                .and_then(|v| path_literal(code.value(path, scope, v))),
            // A convention returns the group it configures.
            _ if CONVENTIONS.contains(&name) => Some(String::new()),
            _ => return,
        };
        if !self.framework(path, index) {
            return;
        }
        let Some(parents) = self.origins(path, scope, receiver, &context) else {
            self.cycles.insert(id);
            return;
        };
        let parents: Vec<(usize, Option<String>)> = (parents.into_iter())
            .filter_map(|node| Some((self.intern(node)?, prefix.clone())))
            .collect();
        self.mounts.insert(id, parents);
    }

    /// Whether a node mounted under `nodes`, at any depth, went round a factory that calls
    /// itself (`cycles`).
    fn round(&self, nodes: &[usize]) -> bool {
        let (mut seen, mut work) = (BTreeSet::new(), nodes.to_vec());
        while let Some(id) = work.pop() {
            if !seen.insert(id) {
                continue;
            }
            if self.cycles.contains(&id) {
                return true;
            }
            let parents = self.mounts.get(&id).into_iter().flatten();
            work.extend(parents.map(|(parent, _)| *parent));
        }
        false
    }

    /// The methods `value` names, upper-case: a list of literals or `HttpMethods.X`; `None`
    /// when one cannot be read.
    fn methods(&self, path: &'a str, scope: &'a str, value: &'a Value) -> Option<Vec<String>> {
        let Value::List(items) = value else {
            return None;
        };
        let mut out = Vec::new();
        for item in items {
            let method = match item {
                Value::Ref(name) => {
                    let spelled = spellings(self.code, path, scope, name);
                    one(&spelled, |s| s.starts_with(HTTP_METHODS))?[HTTP_METHODS.len()..]
                        .to_string()
                }
                other => path_literal(self.code.value(path, scope, other))?,
            };
            let method = method.to_uppercase();
            if !METHODS.contains(&method.as_str()) {
                return None;
            }
            out.push(method);
        }
        out.sort();
        out.dedup();
        Some(out)
    }
}

/// A route a call registers: its call, its pattern, its methods and its handler.
struct Route<'a> {
    path: &'a str,
    call: &'a Call,
    pattern: String,
    methods: Vec<String>,
    handler: Option<&'a Value>,
    nodes: Vec<usize>,
}

/// Minimal API providers: see the module.
pub(in crate::http) fn providers(code: &Code) -> Vec<Found> {
    let files: Vec<_> = (code.files().iter())
        .filter(|(p, _)| p.ends_with(".cs"))
        .map(|(p, f)| (*p, *f))
        .collect();
    let mut minimal = Minimal {
        code,
        overflow: false,
        cycles: BTreeSet::new(),
        ids: BTreeMap::new(),
        nodes: Vec::new(),
        pending: Vec::new(),
        roots: BTreeSet::new(),
        mounts: Mounts::new(),
    };
    let mut routes = Vec::new();
    for &(path, facts) in &files {
        for (n, call) in facts.calls.iter().enumerate() {
            let name = method(&call.callee);
            let verb = VERBS.iter().find(|(m, _)| *m == name).map(|(_, v)| *v);
            if call.decorates.is_some()
                || (verb.is_none() && name != MAP_METHODS)
                || !minimal.framework(path, n)
            {
                continue;
            }
            // The index keeps no argument: they come from the full facts of the file.
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let Some(receiver) = call.receiver.as_ref() else {
                continue;
            };
            let scope = call.scope.as_str();
            let arg = |at: usize, key: &str| call.args.get(at).or_else(|| call.keyword(key));
            let (methods, handler) = match verb {
                Some(verb) => (Some(vec![verb.to_string()]), arg(1, "handler")),
                None => (
                    arg(1, "httpMethods").and_then(|v| minimal.methods(path, scope, v)),
                    arg(2, "handler"),
                ),
            };
            let pattern = arg(0, "pattern")
                .and_then(|v| path_literal(code.value(path, scope, v)))
                .map(optional);
            let (Some(methods), Some(pattern)) = (methods, pattern) else {
                continue;
            };
            let Some(nodes) = minimal.origins(path, scope, receiver, &[]) else {
                continue;
            };
            let nodes = nodes
                .into_iter()
                .filter_map(|n| minimal.intern(n))
                .collect();
            routes.push(Route {
                path,
                call,
                pattern,
                methods,
                handler,
                nodes,
            });
        }
    }
    while let Some(id) = minimal.pending.pop() {
        minimal.read(id);
    }
    if minimal.overflow {
        return Vec::new();
    }
    let root = |id: usize| minimal.roots.contains(&id).then(String::new);
    let mut found = Vec::new();
    for route in &routes {
        if minimal.round(&route.nodes) {
            continue;
        }
        let mut served = BTreeSet::new();
        for &node in &route.nodes {
            served.extend(prefixes(node, &minimal.mounts, &root, &mut Vec::new()));
        }
        for prefix in served {
            let full = join(&prefix, &route.pattern);
            if !balanced(&full) {
                continue;
            }
            let full = normalize_path(&full);
            for method in &route.methods {
                let site = handler_site(
                    code,
                    route.path,
                    &route.call.scope,
                    route.handler,
                    method,
                    &full,
                );
                found.push(provider(site, contract(method, &full)));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Site;
    use crate::lang::LanguagePack;
    use crate::lang::csharp::CSharp;
    use crate::resolve::Resolver;

    const USINGS: &str = "using Microsoft.AspNetCore.Builder;\nusing Microsoft.AspNetCore.Http;\n\
                          using Microsoft.AspNetCore.Routing;\n";
    const APP_START: &str = "var builder = WebApplication.CreateBuilder(args);\n\
                             var app = builder.Build();\n";

    /// Each provider the files give (`(path, source)`), as `contract site`, sorted.
    fn served_in(sources: &[(&str, &str)]) -> Vec<String> {
        let parsed: Vec<_> = (sources.iter())
            .map(|(p, s)| (*p, CSharp.parse(p, s.as_bytes()).unwrap()))
            .collect();
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &CSharp);
        let code = Code::new(&resolver, &files, &CSharp);
        let mut found: Vec<String> = (providers(&code).into_iter())
            .map(|f| {
                let site = match f.site {
                    Site::Def { path, name } => format!("{path}::{name}"),
                    Site::Inline { path, def, name } => format!("{path}::{def} ({name})"),
                    Site::File(path) => path,
                    Site::Call { path, .. } => format!("{path} (call)"),
                };
                format!("{} {site}", f.contract.unwrap_or_default())
            })
            .collect();
        found.sort();
        found
    }

    /// The providers of top-level statements `program` after the usings and the application.
    fn served(program: &str) -> Vec<String> {
        let src = format!("{USINGS}{APP_START}{program}\n");
        served_in(&[("Program.cs", &src)])
    }

    #[test]
    fn a_route_on_the_application_is_served_at_its_root_with_its_lambda_as_handler() {
        let found = served("app.MapGet(\"/health\", () => \"ok\");");
        let at = USINGS.len() + APP_START.len() + "app.MapGet(\"/health\", ".len();
        assert_eq!(
            found,
            [format!(
                "http::GET::/health Program.cs::Main.@{at} (GET /health)"
            )]
        );
    }

    /// The contracts alone.
    fn contracts(found: Vec<String>) -> Vec<String> {
        let mut out: Vec<String> = (found.into_iter())
            .map(|f| f.split(' ').next().unwrap_or_default().to_string())
            .collect();
        out.dedup();
        out
    }

    #[test]
    fn groups_nest_through_locals_and_chains() {
        let found = served(
            "var api = app.MapGroup(\"/api\");\n\
             var v1 = api.MapGroup(\"v1\");\n\
             v1.MapGet(\"/orders/{id:int}\", () => 1);\n\
             app.MapGroup(\"/a\").MapGroup(\"/b\").MapPost(\"c\", () => 2);\n\
             api.MapPut(\"/{id?}\", () => 3);\n\
             v1.MapPatch(\"/p\", () => 4);\n\
             v1.MapDelete(\"/\", () => 5);",
        );
        assert_eq!(
            contracts(found),
            [
                "http::DELETE::/api/v1",
                "http::GET::/api/v1/orders/{}",
                "http::PATCH::/api/v1/p",
                "http::POST::/a/b/c",
                "http::PUT::/api/{}",
            ]
        );
    }

    #[test]
    fn map_methods_serves_each_method_it_lists() {
        let found = served(
            "app.MapMethods(\"/m\", new[] { \"put\", HttpMethods.Patch }, () => 1);\n\
             app.MapMethods(\"/c\", [\"GET\"], () => 1);\n\
             app.MapMethods(\"/n\", Methods(), () => 1);\n\
             app.MapMethods(\"/x\", new[] { \"BREW\" }, () => 1);",
        );
        assert_eq!(
            contracts(found),
            ["http::GET::/c", "http::PATCH::/m", "http::PUT::/m"]
        );
    }

    #[test]
    fn a_builder_meada_does_not_read_serves_nothing() {
        // A parameter no call reaches, a group whose prefix is unknown, an application that is
        // not the framework's, and a builder reassigned (the pack drops its bound).
        let found = served_in(&[(
            "Program.cs",
            &format!(
                "{USINGS}{APP_START}\
                 app.MapGroup(Prefix()).MapGet(\"/a\", () => 1);\n\
                 var g = app.MapGroup(\"/g\");\n\
                 g = app.MapGroup(\"/h\");\n\
                 g.MapGet(\"/b\", () => 1);\n\
                 Other.Make().MapGet(\"/c\", () => 1);\n\
                 static class Routes {{ public static void Map(IEndpointRouteBuilder e) \
                 {{ e.MapGet(\"/d\", () => 1); }} }}\n"
            ),
        )]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_method_of_the_repository_named_like_the_frameworks_hides_it() {
        let found = served(
            "app.MapGet(\"/a\", () => 1);\n\
             static class Mine { public static void MapGet(this WebApplication a, string p, \
             System.Delegate d) {} }",
        );
        assert!(found.is_empty(), "{found:?}");
        let found = served(
            "app.MapGroup(\"/g\").MapPost(\"/a\", () => 1);\n\
             static class Mine { public static RouteGroupBuilder MapGroup(this WebApplication a, \
             string p) => null; }",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_method_group_handler_is_that_method() {
        let found = served(
            "app.MapGet(\"/a\", Handlers.List);\n\
             app.MapGet(handler: Handlers.List, pattern: \"/c\");\n\
             app.MapGet(\"/d\", handler: Handlers.List);\n\
             static class Handlers { public static int List() => 1; }",
        );
        assert_eq!(
            found,
            [
                "http::GET::/a Program.cs::Handlers.List()",
                "http::GET::/c Program.cs::Handlers.List()",
                "http::GET::/d Program.cs::Handlers.List()",
            ]
        );
    }

    /// `Routes.cs`: an extension method mapping routes on the group it is called on, a cycle,
    /// a method given a group, and factories returning groups.
    const ROUTES: &str = "using Microsoft.AspNetCore.Builder;\nusing Microsoft.AspNetCore.Routing;\n\
        namespace Acme.Routes;\n\
        public static class OrderRoutes {\n\
          public static RouteGroupBuilder MapOrders(this RouteGroupBuilder group) {\n\
            group.MapGet(\"/\", () => 1);\n\
            group.MapGet(\"/{id}\", (int id) => id);\n\
            return group;\n\
          }\n\
          public static void Nest(this RouteGroupBuilder g) {\n\
            g.MapGet(\"/leaf\", () => 1);\n\
            g.MapGroup(\"/again\").Nest();\n\
          }\n\
          public static void Admin(RouteGroupBuilder admin) => admin.MapDelete(\"/users\", () => 1);\n\
          public static RouteGroupBuilder Sub(RouteGroupBuilder parent) => parent.MapGroup(\"/sub\");\n\
          public static RouteGroupBuilder Api(this WebApplication app) => app.MapGroup(\"/api\");\n\
          public static RouteGroupBuilder Typed(RouteGroupBuilder parent) {\n\
            RouteGroupBuilder p = parent;\n\
            return p.MapGroup(\"/t\");\n\
          }\n\
        }\n";

    fn routed(program: &str) -> Vec<String> {
        let main = format!("{USINGS}using Acme.Routes;\n{APP_START}{program}\n");
        contracts(served_in(&[("Program.cs", &main), ("Routes.cs", ROUTES)]))
    }

    #[test]
    fn an_extension_method_maps_its_routes_under_each_group_it_is_called_on() {
        let found = routed(
            "var orders = app.MapGroup(\"/v1\").MapGroup(\"/orders\");\n\
             orders.MapOrders();\n\
             app.MapGroup(\"/v2\").MapOrders().MapPost(\"/new\", () => 1);",
        );
        assert_eq!(
            found,
            [
                "http::GET::/v1/orders",
                "http::GET::/v1/orders/{}",
                "http::GET::/v2",
                "http::GET::/v2/{}",
                "http::POST::/v2/new",
            ]
        );
    }

    #[test]
    fn an_extension_method_the_file_does_not_import_maps_nothing() {
        let main = format!("{USINGS}{APP_START}app.MapGroup(\"/v1\").MapOrders();\n");
        assert!(served_in(&[("Program.cs", &main), ("Routes.cs", ROUTES)]).is_empty());
    }

    #[test]
    fn a_group_mounted_inside_itself_adds_nothing_more() {
        let found = routed("app.MapGroup(\"/loop\").Nest();");
        assert_eq!(
            found,
            ["http::GET::/loop/again/leaf", "http::GET::/loop/leaf"]
        );
    }

    #[test]
    fn a_group_follows_method_arguments_and_return_values() {
        let found = routed(
            "OrderRoutes.Admin(app.MapGroup(\"/admin\"));\n\
             var a = OrderRoutes.Sub(app.MapGroup(\"/a\"));\n\
             var b = OrderRoutes.Sub(app.MapGroup(\"/b\"));\n\
             a.MapGet(\"/x\", () => 1);\n\
             b.MapGet(\"/y\", () => 1);\n\
             app.Api().MapGet(\"/z\", () => 1);\n\
             var c = OrderRoutes.Typed(app.MapGroup(\"/c\"));\n\
             var d = OrderRoutes.Typed(app.MapGroup(\"/d\"));\n\
             c.MapGet(\"/u\", () => 1);\n\
             d.MapGet(\"/v\", () => 1);",
        );
        assert_eq!(
            found,
            [
                "http::DELETE::/admin/users",
                "http::GET::/a/sub/x",
                "http::GET::/api/z",
                "http::GET::/b/sub/y",
                "http::GET::/c/t/u",
                "http::GET::/d/t/v",
            ]
        );
    }

    #[test]
    fn a_convention_on_a_group_keeps_its_prefix() {
        let found = served(
            "var api = app.MapGroup(\"/api\").RequireAuthorization().WithTags(\"t\");\n\
             api.AddEndpointFilter<F>().MapGet(\"/a\", () => 1);\n\
             app.MapGroup(\"/b\").Configure().MapGet(\"/c\", () => 1);",
        );
        assert_eq!(contracts(found), ["http::GET::/api/a"]);
    }

    /// Groups nested in place are read once each: a deep nest costs its depth, and one past
    /// the pack's `VALUE_DEPTH` (and so `PREFIX_DEPTH`) is served nowhere.
    #[test]
    fn a_deep_nest_of_groups_is_read_in_linear_time_and_served_nowhere_past_the_prefix_depth() {
        let nest = |n: usize| {
            let groups = ".MapGroup(\"/a\")".repeat(n);
            format!("app{groups}.MapGet(\"/x\", () => 1);")
        };
        assert_eq!(contracts(served(&nest(3))), ["http::GET::/a/a/a/x"]);
        assert_eq!(served(&nest(60)).len(), 1);
        let time = |n: usize| {
            let start = std::time::Instant::now();
            assert!(served(&nest(n)).is_empty());
            start.elapsed()
        };
        let (small, large) = (time(100), time(400));
        assert!(large < small * 12, "{small:?} then {large:?}");
    }

    #[test]
    fn a_method_of_the_repository_hides_the_frameworks_only_where_the_call_reaches_it() {
        let found = served(
            "app.MapGroup(\"/g\").WithTags(\"t\").MapGet(\"/a\", () => 1);\n\
             class OrderBuilder { public int Build() => 1; public void MapGet() {} \
             public void MapGroup() {} public void WithTags() {} }",
        );
        assert_eq!(contracts(found), ["http::GET::/g/a"]);
    }

    /// A factory that calls itself to make the group it returns is read to a bound and serves
    /// nothing: a route above a node whose origins climb one call twice is served nowhere.
    #[test]
    fn a_recursive_group_factory_is_read_in_bounded_time() {
        let factories = "static class F {\n\
            public static RouteGroupBuilder Deep(RouteGroupBuilder g, int n) {\n\
              if (n == 0) return g;\n\
              return Deep(g, n - 1).MapGroup(\"/d\");\n\
            }\n\
            public static RouteGroupBuilder A(RouteGroupBuilder g) => A(g).MapGroup(\"/a\");\n\
          }";
        let start = std::time::Instant::now();
        let found = contracts(served(&format!(
            "F.Deep(app.MapGroup(\"/r\"), 2).MapGet(\"/x\", () => 1);\n\
             F.A(app.MapGroup(\"/s\")).MapGet(\"/y\", () => 1);\n\
             app.MapGet(\"/ok\", () => 1);\n{factories}"
        )));
        let took = start.elapsed();
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
        // What the recursive factories make is served nowhere, not under part of its prefixes.
        assert_eq!(found, ["http::GET::/ok"]);
    }

    /// Past `MINIMAL_NODES` nodes nothing is served.
    #[test]
    fn past_the_node_ceiling_nothing_is_served() {
        let groups: String = (0..MINIMAL_NODES)
            .map(|i| format!("app.MapGroup(\"/g{i}\").MapGet(\"/x\", () => 1);\n"))
            .collect();
        assert!(served(&groups).is_empty());
        let groups: String = (0..10)
            .map(|i| format!("app.MapGroup(\"/g{i}\").MapGet(\"/x\", () => 1);\n"))
            .collect();
        assert_eq!(served(&groups).len(), 10);
    }
}
