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
//! returns keeps the calls the walk climbed to reach it (`Route::through`), and the parameter
//! it was made from is read from that call alone (`Code::passed`), so two groups one factory
//! makes from two builders keep their own prefixes. A builder used under two prefixes (an
//! extension method called on two groups) serves each route under both.
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
use crate::lang::{Call, Kind, Value};
use crate::names::{one, spellings};
use crate::resolve::{Code, Follow, Of, passes_nothing};

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

/// An origin of a builder: the call that made it and the calls the walk climbed through to
/// reach it, outermost first (`Route::through`).
type Node<'a> = (CallAt<'a>, Vec<CallAt<'a>>);

/// The method a callee calls: `MapGet` of `app.MapGet`.
fn method(callee: &str) -> &str {
    callee.rsplit('.').next().unwrap_or(callee)
}

struct Minimal<'c, 'a> {
    code: &'c Code<'a>,
    /// The method names the repository declares: one hides the framework's.
    declared: BTreeSet<&'a str>,
    /// Each node by its place in `nodes`.
    ids: BTreeMap<Node<'a>, usize>,
    nodes: Vec<Node<'a>>,
    /// The nodes not yet read.
    pending: Vec<usize>,
    roots: BTreeSet<usize>,
    mounts: Mounts<'a, usize>,
}

impl<'c, 'a> Minimal<'c, 'a> {
    /// Whether the repository leaves the framework's method `name` alone.
    fn framework(&self, name: &str) -> bool {
        !self.declared.contains(name)
    }

    /// The place of `node` in `nodes`, read later when it is new.
    fn intern(&mut self, node: Node<'a>) -> usize {
        if let Some(&id) = self.ids.get(&node) {
            return id;
        }
        let id = self.nodes.len();
        self.ids.insert(node.clone(), id);
        self.nodes.push(node);
        self.pending.push(id);
        id
    }

    /// The origins of `value`, written in `scope` of `path`, reached through the calls of
    /// `context`. A parameter of the function the innermost of them calls is what that call
    /// passes for it, read where the call is written.
    fn origins(
        &self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        context: &[CallAt<'a>],
    ) -> Vec<Node<'a>> {
        let code = self.code;
        if let (Value::Ref(name), Some((&(at, index), rest))) = (value, context.split_last())
            && let Some((function, n)) = code.param_of(path, scope, name)
        {
            let call = &code.files()[at].calls[index];
            return match code.passed(at, index, function, n) {
                Some(passed) => self.origins(at, &call.scope, passed, rest),
                None => Vec::new(),
            };
        }
        let follow = Follow::Values {
            passes: passes_nothing,
        };
        let mut out = BTreeSet::new();
        for origin in code.origins(path, scope, Of::Value(value), follow) {
            let Some(call) = origin.call else {
                continue;
            };
            for route in origin.routes {
                let mut through = context.to_vec();
                through.extend(route.through);
                out.insert((call, through));
            }
        }
        out.into_iter().collect()
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

    /// Reads the node `id`: a `MapGroup` call is mounted on the origins of its receiver, the
    /// application is a root, and anything else is left unread.
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
        if name == "Build" && self.framework("Build") {
            let made = self.origins(path, scope, receiver, &context);
            if !made.is_empty() && (made.iter()).all(|((p, n), _)| self.app_call(p, *n, BUILDERS)) {
                self.roots.insert(id);
            }
            return;
        }
        if name != MAP_GROUP || !self.framework(MAP_GROUP) {
            return;
        }
        let prefix = (call.args.first())
            .or_else(|| call.keyword("prefix"))
            .and_then(|v| path_literal(code.value(path, scope, v)));
        let parents = self.origins(path, scope, receiver, &context);
        let parents: Vec<(usize, Option<String>)> = (parents.into_iter())
            .map(|node| (self.intern(node), prefix.clone()))
            .collect();
        self.mounts.insert(id, parents);
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
    let declared = (files.iter())
        .flat_map(|(_, f)| f.defs.iter())
        .filter(|d| matches!(d.kind, Kind::Method | Kind::Function))
        .map(|d| d.name.split('(').next().unwrap_or(&d.name))
        .map(method)
        .collect();
    let mut minimal = Minimal {
        code,
        declared,
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
                || !minimal.framework(name)
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
            let nodes = minimal.origins(path, scope, receiver, &[]);
            let nodes = nodes.into_iter().map(|n| minimal.intern(n)).collect();
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
    let root = |id: usize| minimal.roots.contains(&id).then(String::new);
    let mut found = Vec::new();
    for route in &routes {
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
        // not the framework's, and a builder reassigned (#111 makes it hold no one value).
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
             app.Api().MapGet(\"/z\", () => 1);",
        );
        assert_eq!(
            found,
            [
                "http::DELETE::/admin/users",
                "http::GET::/a/sub/x",
                "http::GET::/api/z",
                "http::GET::/b/sub/y",
            ]
        );
    }
}
