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

use std::collections::BTreeSet;

use super::{ANY, METHODS, balanced, contract, join, path_literal, provider};
use crate::contract::{Found, Site};
use crate::jvm::names::{is_method, known};
use crate::lang::{Call, Kind};
use crate::resolve::Code;

const MVC: &str = "Microsoft.AspNetCore.Mvc.";
const BASES: &[&str] = &["ControllerBase", "Controller"];

/// What an MVC attribute `spelled` (through its `using`) is: `ApiController`, `Route` or
/// `Http<Method>`, whether or not it is written with the `Attribute` suffix.
fn attribute(spelled: &str) -> Option<&str> {
    let name = spelled.strip_prefix(MVC)?;
    let name = name.strip_suffix("Attribute").unwrap_or(name);
    let verb = name.strip_prefix("Http").map(str::to_uppercase);
    (matches!(name, "ApiController" | "Route")
        || verb.is_some_and(|v| METHODS.contains(&v.as_str())))
    .then_some(name)
}

/// Whether `spelled` is one of MVC's controller base classes.
fn base(spelled: &str) -> bool {
    spelled
        .strip_prefix(MVC)
        .is_some_and(|n| BASES.contains(&n))
}

/// The name `name`, written in `scope` of `file`, as a name of the framework `is_known`
/// accepts: bound by a `using`, or written in full (`Microsoft.AspNetCore.Mvc.Route`), which no
/// `using` has to bind unless the repository declares a namespace `Microsoft` (`hidden`).
fn spelled<'a>(
    code: &Code<'a>,
    file: &'a str,
    scope: &'a str,
    name: &'a str,
    hidden: bool,
    is_known: impl Fn(&str) -> bool,
) -> Option<String> {
    let full = || (name.starts_with(MVC) && !hidden && is_known(name)).then(|| name.to_string());
    known(code, file, scope, name, &is_known).or_else(full)
}

/// ASP.NET Core controller providers: see the module.
pub(super) fn providers<'a>(code: &Code<'a>) -> Vec<Found> {
    let mut found = Vec::new();
    // The attribute classes the repository declares, by name: one hides MVC's of the same name.
    let declared: BTreeSet<&str> = (code.files().values())
        .flat_map(|f| f.defs.iter())
        .filter(|d| d.kind == Kind::Class)
        .filter_map(|d| d.name.rsplit('.').next())
        .filter(|n| n.ends_with("Attribute"))
        .collect();
    // A namespace `Microsoft` of the repository could hide the framework's in a qualified name.
    let hidden =
        (code.files().values()).any(|f| f.defs.iter().any(|d| d.name.starts_with("Microsoft.")));
    for (path, facts) in code.files().iter().filter(|(p, _)| p.ends_with(".cs")) {
        let (path, facts) = (*path, *facts);
        let attributes: Vec<(usize, &'a Call, String)> = (facts.calls.iter().enumerate())
            .filter(|(_, c)| c.decorates.is_some())
            .filter_map(|(n, c)| {
                let is_attribute = |s: &str| attribute(s).is_some();
                let spelled = spelled(code, path, &c.scope, &c.callee, hidden, is_attribute)?;
                // `[HttpGet]` may be a class of the repository named `HttpGetAttribute`.
                let simple = c.callee.rsplit('.').next().unwrap_or(&c.callee);
                if declared.contains(format!("{simple}Attribute").as_str()) {
                    return None;
                }
                let name = attribute(&spelled)?.to_string();
                Some((n, c, name))
            })
            .collect();
        for class in facts.defs.iter().filter(|d| d.kind == Kind::Class) {
            let on_class = |n: &'static str| {
                (attributes.iter()).filter(move |(_, c, a)| {
                    c.decorates.as_deref() == Some(class.name.as_str()) && a == n
                })
            };
            let scope = crate::parent(&class.name);
            // A base of the repository (not an interface) passes its attributes down: not read.
            let inherits = class.bases.iter().any(|b| {
                code.definition(path, scope, b)
                    .is_some_and(|t| !code.files()[t.path].interfaces.iter().any(|i| i == t.name))
            });
            let controller = on_class("ApiController").next().is_some()
                || class
                    .bases
                    .iter()
                    .any(|b| spelled(code, path, scope, b, hidden, base).is_some());
            if inherits || !controller {
                continue;
            }
            let Some(prefixes) = on_class("Route")
                .map(|(n, ..)| template(code, path, *n))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let prefixes: Vec<String> = match prefixes.into_iter().flatten().collect::<Vec<_>>() {
                p if p.is_empty() => vec![String::new()],
                p => p,
            };
            let targets: BTreeSet<&str> = (attributes.iter())
                .filter(|(_, c, _)| c.scope == class.name)
                .filter_map(|(_, c, _)| c.decorates.as_deref())
                .filter(|t| is_method(facts, t))
                .collect();
            for target in targets {
                let on = || {
                    (attributes.iter()).filter(|(_, c, _)| c.decorates.as_deref() == Some(target))
                };
                let verbs: Vec<(usize, String)> = on()
                    .filter_map(|(n, _, a)| Some((*n, a.strip_prefix("Http")?.to_uppercase())))
                    .collect();
                let Some(routes) = routes(
                    code,
                    path,
                    &verbs,
                    on().filter(|(.., a)| a.as_str() == "Route")
                        .map(|(n, ..)| *n),
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
                    let action = target.split('(').next().unwrap_or(target);
                    let action = action.rsplit('.').next().unwrap_or(action);
                    let name = class.name.rsplit('.').next().unwrap_or(&class.name);
                    let name = name.strip_suffix("Controller").unwrap_or(name);
                    let Some(full) = tokens(&full, name, action)
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
/// alone, when no attribute has a template. `None` when a template cannot be read.
fn routes<'a>(
    code: &Code<'a>,
    file: &'a str,
    verbs: &[(usize, String)],
    route: impl Iterator<Item = usize>,
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
    if routes.is_empty() {
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
fn optional(path: String) -> String {
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
}
