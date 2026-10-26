//! ASP.NET Core controller providers (spec #28, #109): a method of a controller class carrying
//! a route attribute serves the class `[Route]` prefix joined to each of its templates. The
//! pack reads an attribute as a call decorating its target (`Call::decorates`), so the names
//! and the templates are read here: an attribute or a base class counts only when
//! `Code::outside` says a `using` from `Microsoft.AspNetCore.Mvc` binds its name (a class of
//! the repository with the same name hides it), `HttpGet` and `HttpGetAttribute` are one name,
//! and a template is read through `Code::value` (the pack folds no constants yet, so one that
//! is no string literal serves nothing).
//! A controller is a class with `[ApiController]` or deriving from `ControllerBase` or
//! `Controller`. A class `[Route]` is a prefix, `[controller]` (the class name without its
//! `Controller` suffix) and `[action]` (the method name) are replaced as ASP.NET Core does, and
//! a template starting with `/` or `~/` replaces the prefix. A method `[Route]` without a method
//! attribute serves every method (`ANY`).
//! Conventional routing (`MapControllerRoute`) is not read. A class deriving from a class of the
//! repository is not read either, since it inherits that class's attributes.

mod clients;
mod reads;
mod refit;

use std::collections::{BTreeMap, BTreeSet};

mod minimal;
pub(super) use minimal::providers as minimal_providers;

use super::{ANY, METHODS, balanced, contract, join, path_literal, provider};
use crate::contract::{Found, Site};
use crate::index::Indexed;
use crate::lang::{Call, Facts, Kind};
use crate::names::{is_method, known};
use crate::resolve::{Code, Target};

const MVC: &str = "Microsoft.AspNetCore.Mvc.";
const BASES: &[&str] = &["ControllerBase", "Controller"];

/// What an MVC attribute `spelled` (through its `using`) is: `ApiController`, `Route` or
/// `Http<Method>`, whether or not it is written with the `Attribute` suffix.
fn attribute(spelled: &str) -> Option<&str> {
    let name = spelled.strip_prefix(MVC)?;
    let name = name.strip_suffix("Attribute").unwrap_or(name);
    let verb = name.strip_prefix("Http").map(str::to_uppercase);
    (matches!(name, "ApiController" | "Route" | "ActionName" | "NonAction")
        || verb.is_some_and(|v| METHODS.contains(&v.as_str())))
    .then_some(name)
}

/// Whether `spelled` is one of MVC's controller base classes.
fn base(spelled: &str) -> bool {
    spelled
        .strip_prefix(MVC)
        .is_some_and(|n| BASES.contains(&n))
}

/// The name `name`, written in `scope` of `file`, as a name of the framework (the namespace
/// `ns`) `is_known` accepts: bound by a `using`, or written in full
/// (`Microsoft.AspNetCore.Mvc.Route`), which no `using` has to bind unless the repository
/// declares a namespace of the framework's root (`Microsoft`, `hidden`).
fn spelled<'a>(
    code: &Code<'a>,
    (file, scope): (&'a str, &'a str),
    name: &'a str,
    (ns, hidden): (&str, bool),
    is_known: impl Fn(&str) -> bool,
) -> Option<String> {
    let full = || (name.starts_with(ns) && !hidden && is_known(name)).then(|| name.to_string());
    known(code, file, scope, name, &is_known).or_else(full)
}

/// Whether `target` is a class (a type, which an attribute names).
fn class(code: &Code, target: Target) -> bool {
    let defs = &code.files()[target.path].defs;
    (defs.iter()).any(|d| d.name == target.name && d.kind == Kind::Class)
}

/// What may hide a framework's attribute in the repository: the attribute classes it declares,
/// by name, and whether it declares a namespace of the framework's root (`spelled`).
struct Hiding<'a> {
    declared: BTreeSet<&'a str>,
    hidden: bool,
}

impl<'a> Hiding<'a> {
    /// What hides the attributes of the namespace `ns` in `files`.
    fn of(files: &[(&'a str, &'a Facts)], ns: &str) -> Self {
        let defs = || files.iter().flat_map(|(_, f)| f.defs.iter());
        let declared = (defs().filter(|d| d.kind == Kind::Class))
            .filter_map(|d| d.name.rsplit('.').next())
            .filter(|n| n.ends_with("Attribute"))
            .collect();
        let root = format!("{}.", ns.split('.').next().unwrap_or(ns));
        let hidden = defs().any(|d| d.name.starts_with(&root));
        Hiding { declared, hidden }
    }

    /// The attribute `c` of `path`, read in `scope`, as the framework (the namespace `ns`)
    /// spells it, when `is_attribute` accepts it and no class of the repository the file reaches
    /// hides it: `[Get]` may be a class named `GetAttribute`. The class is looked up from the
    /// scope around the attribute's target (a class's attribute is written in its namespace, a
    /// method's in its class) and the one around that.
    fn spelled(
        &self,
        code: &Code<'a>,
        (path, scope): (&'a str, &'a str),
        c: &'a Call,
        ns: &str,
        is_attribute: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let simple = c.callee.rsplit('.').next().unwrap_or(&c.callee);
        let own = [
            Some(c.callee.as_str()),
            self.declared
                .get(format!("{simple}Attribute").as_str())
                .copied(),
        ]
        .into_iter()
        .flatten()
        .any(|own| {
            [c.scope.as_str(), crate::parent(&c.scope)]
                .into_iter()
                .any(|scope| {
                    code.definition(path, scope, own)
                        .is_some_and(|t| class(code, t))
                })
        });
        let spelled = spelled(
            code,
            (path, scope),
            &c.callee,
            (ns, self.hidden),
            is_attribute,
        );
        spelled.filter(|_| !own)
    }
}

/// The consumers of the `HttpClient` (`clients.rs`) and of Refit's interfaces (`refit.rs`).
pub(super) fn consumers<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Found> {
    let mut found = clients::consumers(code, indexed);
    found.extend(refit::consumers(code, indexed));
    found
}

/// ASP.NET Core controller providers: see the module.
pub(super) fn providers<'a>(code: &Code<'a>) -> Vec<Found> {
    let mut found = Vec::new();
    let files: Vec<(&'a str, &'a Facts)> = (code.files().iter())
        .filter(|(p, _)| p.ends_with(".cs"))
        .map(|(p, f)| (*p, *f))
        .collect();
    // The attribute classes the repository declares, and a namespace `Microsoft` of its own,
    // hide MVC's.
    let hiding = Hiding::of(&files, MVC);
    // A class with a class of the repository as a base inherits its attributes, and that base
    // passes them down: neither is read.
    let (mut inheriting, mut based) = (BTreeSet::new(), BTreeSet::new());
    let mut parts: BTreeMap<&str, usize> = BTreeMap::new();
    for &(path, facts) in &files {
        for name in &facts.partial {
            *parts.entry(name).or_default() += 1;
        }
        for class in facts.defs.iter().filter(|d| d.kind == Kind::Class) {
            for b in &class.bases {
                let target = code.definition(path, crate::parent(&class.name), b);
                if let Some(t) = target.filter(|t| {
                    !code.files()[t.path]
                        .interfaces
                        .contains(&t.name.to_string())
                }) {
                    inheriting.insert(Target {
                        path,
                        name: &class.name,
                    });
                    based.insert(t);
                }
            }
        }
    }
    for &(path, facts) in &files {
        // The MVC attributes (call index, call, name), and what an attribute not confirmed as
        // MVC's `[Route]` leaves unread: the definitions it decorates.
        let (mut attributes, mut unread): (Vec<(usize, &'a Call, String)>, BTreeSet<&str>) =
            (Vec::new(), BTreeSet::new());
        for (n, c) in (facts.calls.iter().enumerate()).filter(|(_, c)| c.decorates.is_some()) {
            let simple = c.callee.rsplit('.').next().unwrap_or(&c.callee);
            let is_attribute = |s: &str| attribute(s).is_some();
            let spelled = hiding.spelled(code, (path, &c.scope), c, MVC, is_attribute);
            match spelled.as_deref().and_then(attribute) {
                Some(name) => attributes.push((n, c, name.to_string())),
                None if simple.strip_suffix("Attribute").unwrap_or(simple) == "Route" => {
                    unread.extend(c.decorates.as_deref());
                }
                None => {}
            }
        }
        for class in facts.defs.iter().filter(|d| d.kind == Kind::Class) {
            let on = |target: &'a str| {
                (attributes.iter()).filter(move |(_, c, _)| c.decorates.as_deref() == Some(target))
            };
            let on_class = |n: &'static str| on(&class.name).filter(move |(.., a)| a == n);
            let scope = crate::parent(&class.name);
            let controller = on_class("ApiController").next().is_some()
                || (class.bases.iter())
                    .any(|b| spelled(code, (path, scope), b, (MVC, hiding.hidden), base).is_some());
            let here = Target {
                path,
                name: &class.name,
            };
            if !controller
                || inheriting.contains(&here)
                || based.contains(&here)
                || unread.contains(class.name.as_str())
                || parts.get(class.name.as_str()) > Some(&1)
            {
                continue;
            }
            let Some(prefixes) = on_class("Route")
                .map(|(n, ..)| template(code, path, *n))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let prefixes: Vec<String> = prefixes.into_iter().flatten().collect();
            // Without a class `[Route]` the template of an action stands alone, and an action
            // with none is routed by convention, which is not read.
            let (prefixed, prefixes) = match prefixes.is_empty() {
                true => (false, vec![String::new()]),
                false => (true, prefixes),
            };
            let targets: BTreeSet<&str> = (attributes.iter())
                .filter(|(_, c, _)| c.scope == class.name)
                .filter_map(|(_, c, _)| c.decorates.as_deref())
                .filter(|t| is_method(facts, t))
                .collect();
            for target in targets {
                let verbs: Vec<(usize, String)> = on(target)
                    .filter_map(|(n, _, a)| Some((*n, a.strip_prefix("Http")?.to_uppercase())))
                    .collect();
                let has = |n: &str| on(target).any(|(.., a)| a == n);
                // Only a public method is an action, and `[NonAction]` says it is none.
                if has("NonAction")
                    || unread.contains(target)
                    || facts.hidden.iter().any(|h| h == target)
                {
                    continue;
                }
                // `[ActionName]` renames the action, to a literal only.
                let action = target.split('(').next().unwrap_or(target);
                let action = action.rsplit('.').next().unwrap_or(action);
                let action = match on(target).find(|(.., a)| a == "ActionName") {
                    None => action.strip_suffix("Async").unwrap_or(action).to_string(),
                    Some((n, ..)) => match template(code, path, *n) {
                        Some(Some(name)) => name,
                        _ => continue,
                    },
                };
                let Some(routes) = routes(
                    code,
                    path,
                    &verbs,
                    on(target).filter(|(.., a)| a == "Route").map(|(n, ..)| *n),
                    prefixed,
                ) else {
                    continue;
                };
                for (prefix, (methods, route)) in prefixes
                    .iter()
                    .flat_map(|p| routes.iter().map(move |r| (p, r)))
                {
                    let full = match route.starts_with("/") || route.starts_with("~/") {
                        true => route.trim_start_matches('~').to_string(),
                        false => join(prefix.trim_start_matches('~'), route),
                    };
                    let name = class.name.rsplit('.').next().unwrap_or(&class.name);
                    let name = name.strip_suffix("Controller").unwrap_or(name);
                    let Some(full) = tokens(&full, name, &action)
                        .map(optional)
                        .filter(|f| balanced(f))
                    else {
                        continue;
                    };
                    for method in methods {
                        let site = Site::Def {
                            path: path.into(),
                            name: target.into(),
                        };
                        found.push(provider(site, contract(method, &full)));
                    }
                }
            }
        }
    }
    found
}

/// The `(methods, template)` routes of one action, from its method attributes (`verbs`, as
/// `(call index, METHOD)`) and its `[Route]` attributes (call indexes): a method attribute with
/// a template is a route of its method, a `[Route]` one of every method the action's method
/// attributes name, or `ANY` without any; method attributes with no template give the prefix
/// alone, when the class has a `[Route]` and no attribute has a template. `None` when a template cannot be read.
fn routes<'a>(
    code: &Code<'a>,
    file: &'a str,
    verbs: &[(usize, String)],
    route: impl Iterator<Item = usize>,
    prefixed: bool,
) -> Option<Vec<(Vec<String>, String)>> {
    let mut routes = Vec::new();
    for (n, verb) in verbs {
        if let Some(t) = template(code, file, *n)? {
            routes.push((vec![verb.clone()], t));
        }
    }
    let every: BTreeSet<&String> = verbs.iter().map(|(_, v)| v).collect();
    let every = match every.is_empty() {
        true => vec![ANY.to_string()],
        false => every.into_iter().cloned().collect(),
    };
    for n in route {
        if let Some(t) = template(code, file, n)? {
            routes.push((every.clone(), t));
        }
    }
    if routes.is_empty() && prefixed {
        routes = verbs
            .iter()
            .map(|(_, v)| (vec![v.clone()], String::new()))
            .collect();
    }
    Some(routes)
}

/// The template an attribute (the call `index` of `file`) gives, through its first positional
/// argument or `template:`: `Some(None)` when it gives none, `None` when it cannot be read. The
/// index keeps no argument: they come from the full facts of the file.
fn template<'a>(code: &Code<'a>, file: &'a str, index: usize) -> Option<Option<String>> {
    let call = code.call(file, index)?;
    match call.args.first().or_else(|| call.keyword("template")) {
        None => Some(None),
        Some(v) => path_literal(code.value(file, &call.scope, v)).map(Some),
    }
}

/// `path` without the `?` of an optional parameter (`{id?}`), which the trunk reads as a query.
pub(super) fn optional(path: String) -> String {
    let mut depth = 0usize;
    path.chars()
        .filter(|c| {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
            !(*c == '?' && depth > 0)
        })
        .collect()
}

/// `template` with `[controller]` and `[action]` replaced and `[[`, `]]` read as the brackets
/// they escape; `None` for any other token, which ASP.NET Core would not start with.
fn tokens(template: &str, controller: &str, action: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(at) = rest.find(['[', ']']) {
        out.push_str(&rest[..at]);
        let (bracket, after) = rest[at..].split_at(1);
        rest = after;
        if after.starts_with(bracket) {
            out.push_str(bracket);
            rest = &after[1..];
        } else if bracket == "[" {
            let (token, after) = rest.split_once(']')?;
            out.push_str(match token.to_ascii_lowercase().as_str() {
                "controller" => controller,
                "action" => action,
                _ => return None,
            });
            rest = after;
        } else {
            out.push(']');
        }
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::csharp::CSharp;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    const USINGS: &str = "using Microsoft.AspNetCore.Mvc;\n";

    /// The contracts of the providers the files give (`(path, source)`), sorted.
    fn served_in(sources: &[(&str, &str)]) -> Vec<String> {
        let parsed: Vec<_> = (sources.iter())
            .map(|(p, s)| (*p, CSharp.parse(p, s.as_bytes()).unwrap()))
            .collect();
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &CSharp);
        let code = Code::new(&resolver, &files, &CSharp);
        let mut found: Vec<String> = (providers(&code).into_iter())
            .filter_map(|f| f.contract)
            .collect();
        found.sort();
        found
    }

    fn served(usings: &str, body: &str) -> Vec<String> {
        let src = format!("{usings}\nnamespace A;\n{body}\n");
        served_in(&[("A/C.cs", &src)])
    }

    fn controller(attributes: &str, methods: &str) -> Vec<String> {
        let body =
            format!("{attributes} public class OrdersController : ControllerBase {{ {methods} }}");
        served(USINGS, &body)
    }

    #[test]
    fn the_class_route_prefixes_each_method_attribute_and_its_tokens_are_replaced() {
        let found = controller(
            "[Route(\"api/[controller]\")]",
            "[HttpGet] public void List() {} \
             [HttpGet(\"{id:int}\")] public void One(int id) {} \
             [HttpPost(\"[action]/{id?}\")] public void Create() {} \
             [HttpPut] public void Put() {} \
             [HttpPatch] public void Patch() {} \
             [HttpDelete] public void Delete() {} \
             [HttpHead] public void Head() {} \
             [HttpOptions] public void Options() {}",
        );
        assert_eq!(
            found,
            [
                "http::DELETE::/api/Orders",
                "http::GET::/api/Orders",
                "http::GET::/api/Orders/{}",
                "http::HEAD::/api/Orders",
                "http::OPTIONS::/api/Orders",
                "http::PATCH::/api/Orders",
                "http::POST::/api/Orders/Create/{}",
                "http::PUT::/api/Orders",
            ]
        );
    }

    #[test]
    fn a_method_route_without_a_method_attribute_serves_every_method() {
        let found = controller("", "[Route(\"any/[action]\")] public void Run() {}");
        assert_eq!(found, ["http::ANY::/any/Run"]);
    }

    #[test]
    fn a_rooted_or_tilde_template_replaces_the_class_prefix() {
        let found = controller(
            "[Route(\"api\")]",
            "[HttpGet(\"/root\")] public void A() {} [HttpGet(\"~/home\")] public void B() {}",
        );
        assert_eq!(found, ["http::GET::/home", "http::GET::/root"]);
    }

    #[test]
    fn a_route_on_a_method_with_a_method_attribute_takes_its_methods() {
        let found = controller("", "[HttpPost][Route(\"x\")] public void A() {}");
        assert_eq!(found, ["http::POST::/x"]);
    }

    #[test]
    fn a_controller_is_an_api_controller_or_derives_from_the_mvc_bases() {
        let m = "[HttpGet(\"a\")] public void A() {}";
        let plain = |attr: &str, base: &str, usings: &str| {
            served(
                usings,
                &format!("{attr} public class OrdersController {base} {{ {m} }}"),
            )
        };
        assert_eq!(plain("[ApiController]", "", USINGS), ["http::GET::/a"]);
        assert_eq!(
            plain("[ApiControllerAttribute]", "", USINGS),
            ["http::GET::/a"]
        );
        assert_eq!(plain("", ": Controller", USINGS), ["http::GET::/a"]);
        assert_eq!(
            plain("", ": ControllerBase, IDisposable", USINGS),
            ["http::GET::/a"]
        );
        let qualified = "[Microsoft.AspNetCore.Mvc.ApiController] public class C { \
            [Microsoft.AspNetCore.Mvc.HttpGetAttribute(\"q\")] public void A() {} }";
        assert_eq!(served("", qualified), ["http::GET::/q"]);
        assert!(plain("", "", USINGS).is_empty());
        assert!(plain("", ": Other", USINGS).is_empty());
    }

    #[test]
    fn names_count_only_through_the_mvc_namespace() {
        let m = "[HttpGet(\"a\")] public void A() {}";
        assert!(served("", &format!("[ApiController] public class C {{ {m} }}")).is_empty());
        let own = format!(
            "public class HttpGetAttribute : System.Attribute {{ public HttpGetAttribute(string s) {{}} }} \
             [ApiController] public class C {{ {m} }}"
        );
        assert!(served(USINGS, &own).is_empty());
    }

    #[test]
    fn an_unknown_token_or_an_unbalanced_template_serves_nothing_and_doubled_brackets_escape() {
        let found = controller(
            "",
            "[HttpGet(\"[area]/a\")] public void A() {} \
             [HttpGet(\"{id:regex(^[[0-9]]{3}/x$)}\")] public void B() {} \
             [HttpGet(\"a[[x]]\")] public void C() {}",
        );
        assert_eq!(found, ["http::GET::/a[x]"]);
    }

    #[test]
    fn a_controller_of_a_repository_base_class_is_not_read() {
        let a = format!(
            "{USINGS}\nnamespace A;\n[Route(\"api\")] public class Base : ControllerBase {{}}\n\
            public class UsersController : Base {{ [HttpGet] public void A() {{}} }}\n\
            public interface IApi {{}}\n\
            public class OrdersController : ControllerBase, IApi {{ [HttpGet(\"o\")] public void A() {{}} }}"
        );
        assert_eq!(served_in(&[("A/C.cs", &a)]), ["http::GET::/o"]);
    }

    const GET: &str = "[HttpGet(\"x\")] public void A() {}";

    #[test]
    fn a_route_that_is_not_mvcs_leaves_its_class_and_its_method_unread() {
        // A class `Route` that is no attribute, and a `RouteAttribute` no using reaches.
        let own = format!(
            "public class Route {{}} [ApiController] [Route(\"api\")] \
             public class OController : ControllerBase {{ {GET} }}"
        );
        assert!(served(USINGS, &own).is_empty());
        let legacy = "namespace Legacy.Web; public class RouteAttribute : System.Attribute { \
                      public RouteAttribute(string s) {} }";
        let api = format!(
            "{USINGS}\nnamespace A; [ApiController] [Route(\"api\")] \
             public class OController : ControllerBase {{ {GET} }}"
        );
        assert_eq!(
            served_in(&[("L/R.cs", legacy), ("A/C.cs", &api)]),
            ["http::GET::/api/x"]
        );
        // Written in full, it is not MVC's either.
        let full = format!(
            "[ApiController] [Legacy.Web.Route(\"api\")] public class OController : \
             ControllerBase {{ {GET} }}"
        );
        assert!(served(USINGS, &full).is_empty());
        // The same on a method.
        let method = "public class Route {} [ApiController] public class OController : \
                      ControllerBase { [HttpGet] [Route(\"x\")] public void A() {} }";
        assert!(served(USINGS, method).is_empty());
        // An attribute class of another namespace the file does not reach hides nothing.
        let other = format!(
            "{USINGS}\nnamespace A; [ApiController] public class OController : ControllerBase \
             {{ {GET} }}"
        );
        assert_eq!(
            served_in(&[("L/R.cs", legacy), ("A/C.cs", &other)]),
            ["http::GET::/x"]
        );
    }

    #[test]
    fn conventional_routing_serves_nothing_and_an_empty_route_serves_the_root() {
        let conventional = "public class HomeController : Controller { \
                            [HttpGet] public void Index() {} [HttpPost] public void Save() {} }";
        assert!(served(USINGS, conventional).is_empty());
        let rooted = "[Route(\"\")] public class HomeController : Controller { \
                      [HttpGet] public void Index() {} }";
        assert_eq!(served(USINGS, rooted), ["http::GET::/"]);
        let prefixed = "[Route(\"api\")] public class HomeController : Controller { \
                        [HttpGet] public void Index() {} }";
        assert_eq!(served(USINGS, prefixed), ["http::GET::/api"]);
    }

    #[test]
    fn action_drops_the_async_suffix_and_follows_a_literal_action_name() {
        let class = |methods: &str| {
            served(
                USINGS,
                &format!(
                    "[Route(\"api/[controller]/[action]\")] public class OrdersController : \
                     ControllerBase {{ {methods} }}"
                ),
            )
        };
        assert_eq!(
            class("[HttpGet] public void GetAllAsync() {}"),
            ["http::GET::/api/Orders/GetAll"]
        );
        assert_eq!(
            class("[HttpGet] [ActionName(\"list\")] public void GetAllAsync() {}"),
            ["http::GET::/api/Orders/list"]
        );
        assert!(class("[HttpGet] [ActionName(Other.Name)] public void A() {}").is_empty());
    }

    #[test]
    fn a_base_class_of_the_repository_is_not_a_controller() {
        let src = "[ApiController] [Route(\"api/[controller]\")] \
                   public abstract class CrudController : ControllerBase { \
                   [HttpGet] public void All() {} } \
                   public class OrdersController : CrudController {}";
        assert!(served(USINGS, src).is_empty());
    }

    #[test]
    fn a_class_declared_in_parts_is_not_read() {
        let a = format!(
            "{USINGS}\nnamespace A; [Route(\"api/[controller]\")] \
             public partial class OrdersController : ControllerBase {{}}"
        );
        let b = format!(
            "{USINGS}\nnamespace A; public partial class OrdersController : ControllerBase \
             {{ [HttpGet(\"x\")] public void A() {{}} }}"
        );
        assert!(served_in(&[("A/C.cs", &a), ("A/D.cs", &b)]).is_empty());
        let alone = "[ApiController] public partial class OrdersController : ControllerBase { \
                     [HttpGet(\"x\")] public void A() {} }";
        assert_eq!(served(USINGS, alone), ["http::GET::/x"]);
    }

    #[test]
    fn an_attribute_class_of_the_repository_hides_mvcs_by_either_spelling() {
        let own = "public class HttpGetAttribute : System.Attribute { \
                   public HttpGetAttribute(string s) {} } [ApiController] \
                   public class C : ControllerBase { [HttpGetAttribute(\"x\")] public void A() {} }";
        assert!(served(USINGS, own).is_empty());
    }

    #[test]
    fn a_non_action_or_a_non_public_method_serves_nothing() {
        let found = controller(
            "[ApiController]",
            "[NonAction] [HttpGet(\"n\")] public void N() {} \
             [HttpGet(\"p\")] private void P() {} [HttpGet(\"i\")] void I() {} \
             [HttpGet(\"ok\")] public void Ok() {}",
        );
        assert_eq!(found, ["http::GET::/ok"]);
    }
}
