//! Refit interfaces (spec #28, #113): an interface whose methods carry Refit's `[Get("/x")]`,
//! `[Post]`, `[Put]`, `[Patch]`, `[Delete]` or `[Head]` (bound through the usings, as the
//! controllers' attributes are, `Get` and `GetAttribute` one name) is a declarative client. A
//! method of one has no call of its own, so it is the consumer (`Site::Def`), and a caller of it
//! reaches it like any call, as Retrofit's do (`http/java/declarative.rs`).
//!
//! The base comes from each `RestService.For<Api>(base)` that makes the interface, read whole
//! (`Expand::Call`, so a wrapper's callers pass it), with its environment and configuration
//! reads decided where `For` is written; one consumer per base, and an interface no `For` names
//! has an unknown base. Refit appends the method's path to the base's own path. A path that does
//! not start with `/` is none Refit accepts. `AddRefitClient<Api>()` is not read.
// ponytail: `For` given an `HttpClient` has an unknown base; read its `BaseAddress` as
// `clients.rs` does if such clients matter.

use super::super::{balanced, consumer_at, fetches};
use super::reads::Reads;
use super::{Hiding, optional, template};
use crate::contract::{Found, Site};
use crate::index::Indexed;
use crate::lang::{Facts, Part, Step, Ty, Value};
use crate::names::{in_full, is_method, known};
use crate::resolve::{Code, Expand};
use std::collections::BTreeMap;

const REFIT: &str = "Refit.";
const REST_SERVICE: &str = "Refit.RestService";
/// Refit's method attributes and the method each sends.
const VERBS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Post", "POST"),
    ("Put", "PUT"),
    ("Patch", "PATCH"),
    ("Delete", "DELETE"),
    ("Head", "HEAD"),
];

/// The method a Refit attribute `spelled` (through its `using`) sends.
fn verb(spelled: &str) -> Option<&'static str> {
    let name = spelled.strip_prefix(REFIT)?;
    let name = name.strip_suffix("Attribute").unwrap_or(name);
    VERBS.iter().find(|(a, _)| *a == name).map(|(_, v)| *v)
}

/// A method of a Refit interface: its definition, the method it sends and its path.
struct Endpoint<'a> {
    target: &'a str,
    verb: &'static str,
    route: String,
}

/// Refit consumers: see the module.
pub(super) fn consumers<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Found> {
    let files: Vec<(&'a str, &'a Facts)> = (code.files().iter())
        .filter(|(p, _)| p.ends_with(".cs"))
        .map(|(p, f)| (*p, *f))
        .collect();
    let hiding = Hiding::of(&files);
    let mut interfaces: BTreeMap<(&'a str, &'a str), Vec<Endpoint<'a>>> = BTreeMap::new();
    for &(path, facts) in files.iter().filter(|(_, f)| !f.interfaces.is_empty()) {
        for (n, c) in facts.calls.iter().enumerate() {
            let Some(target) = c.decorates.as_deref() else {
                continue;
            };
            if !facts.interfaces.contains(&c.scope) || !is_method(facts, target) {
                continue;
            }
            // Read around the interface: a method of it named like the attribute is no type.
            let around = (path, crate::parent(&c.scope));
            let spelled = hiding.spelled(code, around, c, REFIT, |s| verb(s).is_some());
            let Some(verb) = spelled.as_deref().and_then(verb) else {
                continue;
            };
            let Some(Some(route)) = template(code, path, n) else {
                continue;
            };
            if !route.starts_with('/') || !balanced(&route) {
                continue;
            }
            let route = optional(route);
            let endpoint = Endpoint {
                target,
                verb,
                route,
            };
            (interfaces.entry((path, c.scope.as_str())).or_default()).push(endpoint);
        }
    }
    if interfaces.is_empty() {
        return Vec::new();
    }
    let bases = bases(code, &files, indexed, &interfaces);
    let mut found = Vec::new();
    for (key, endpoints) in &interfaces {
        let bases: Vec<Option<&[Part]>> = match bases.get(key).filter(|b| !b.is_empty()) {
            Some(bases) => bases.iter().map(|b| Some(b.as_slice())).collect(),
            None => vec![None],
        };
        for endpoint in endpoints {
            for base in &bases {
                let method = Value::Str(vec![Part::Text(endpoint.verb.into())]);
                let Some(reads) = fetches(&[url(*base, &endpoint.route), method]) else {
                    continue;
                };
                for read in reads {
                    let site = Site::Def {
                        path: key.0.into(),
                        name: endpoint.target.into(),
                    };
                    found.push(consumer_at(site, read, None));
                }
            }
        }
    }
    found
}

/// The bases each interface is made with by `RestService.For<Api>(base)`, by the interface's
/// file and name.
fn bases<'a>(
    code: &Code<'a>,
    files: &[(&'a str, &'a Facts)],
    indexed: &Indexed,
    interfaces: &BTreeMap<(&'a str, &'a str), Vec<Endpoint<'a>>>,
) -> BTreeMap<(&'a str, &'a str), Vec<Vec<Part>>> {
    let reads = Reads::new(indexed.configs);
    let mut out: BTreeMap<_, Vec<Vec<Part>>> = BTreeMap::new();
    for &(path, facts) in files {
        for (n, stored) in facts.calls.iter().enumerate() {
            let Some((ty, "For")) = stored.callee.rsplit_once('.') else {
                continue;
            };
            let rest = |s: &str| s == REST_SERVICE;
            if stored.decorates.is_some()
                || !(in_full(code, path, &stored.scope, ty, rest)
                    || known(code, path, &stored.scope, ty, rest).is_some())
            {
                continue;
            }
            let Some(Step::Argument(tys)) = code.uses(path, n).and_then(|u| u.first()?.first())
            else {
                continue;
            };
            let Some(Ty::Named(api)) = tys.first() else {
                continue;
            };
            let Some(made) = code.definition(path, &stored.scope, api) else {
                continue;
            };
            let served: Vec<_> = (inherited(code, made.path, made.name).into_iter())
                .filter(|k| interfaces.contains_key(k))
                .collect();
            if served.is_empty() {
                continue;
            }
            // A base a wrapper's caller passes is read at each caller (`None` climbs).
            let read = |values: &[Value], _: &[(String, Value)]| match values.first() {
                Some(Value::Str(p)) if p.iter().any(|p| matches!(p, Part::Ref(_))) => None,
                Some(Value::Ref(_)) => None,
                Some(Value::Str(p)) => Some(p.clone()),
                _ => Some(vec![Part::Hole]),
            };
            for base in code.expand(Expand::Call(path, n), &read) {
                let parts = match reads.value(code, path, Value::Str(base.found)) {
                    Value::Str(parts) => parts,
                    _ => vec![Part::Hole],
                };
                for &key in &served {
                    let entry = out.entry(key).or_default();
                    if !entry.contains(&parts) {
                        entry.push(parts.clone());
                    }
                }
            }
        }
    }
    out
}

/// How many levels of written bases `For<Api>` reaches.
const BASE_DEPTH: usize = 32;

/// The interface `name` of `path` and the repository interfaces it extends, directly or through
/// others, by file and name: bounded by `BASE_DEPTH`, each once (a cycle ends). A base outside
/// the repository resolves to no definition and adds nothing.
fn inherited<'a>(code: &Code<'a>, path: &'a str, name: &'a str) -> Vec<(&'a str, &'a str)> {
    let mut seen = vec![(path, name)];
    let mut level = vec![(path, name)];
    for _ in 0..BASE_DEPTH {
        let mut next = Vec::new();
        for (path, name) in level {
            let Some(def) = code.files()[path].defs.iter().find(|d| d.name == name) else {
                continue;
            };
            for base in &def.bases {
                let Some(t) = code.definition(path, crate::parent(name), base) else {
                    continue;
                };
                let key = (t.path, t.name);
                if !seen.contains(&key) {
                    seen.push(key);
                    next.push(key);
                }
            }
        }
        level = next;
    }
    seen
}

/// The URL of the path `route` (from the root) of a Refit with `base`: the base, its own path
/// kept, then the route. `None` for a base not read.
fn url(base: Option<&[Part]>, route: &str) -> Value {
    let Some(base) = base else {
        return Value::Str(vec![Part::Text(route.into())]);
    };
    let trim = |parts: &mut Vec<Part>| {
        if let Some(Part::Text(t)) = parts.last_mut() {
            *t = t.trim_end_matches('/').to_string();
        }
    };
    let mut out = base.to_vec();
    match out.as_mut_slice() {
        // A base read from configuration joins on the literal it defaults to.
        [Part::EnvOr(_, default)] => trim(default),
        _ => trim(&mut out),
    }
    out.push(Part::Text(route.into()));
    Value::Str(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::csharp::CSharp;
    use crate::resolve::Resolver;

    /// A consumer as `(contract, via, the definition it is at)`.
    type Sent = (Option<String>, Option<String>, String);

    const API: &str = "using Refit;\nusing System.Threading.Tasks;\nnamespace A;\n\
                       public interface IApi {\n\
                       [Get(\"/items/{id}\")] Task<string> Item(int id);\n\
                       [Post(\"/items\")] Task Create();\n\
                       [Put(\"/items/{id}\")] Task Put(int id);\n\
                       [Patch(\"/items/{id}\")] Task Patch(int id);\n\
                       [Delete(\"/items/{id}\")] Task Delete(int id);\n\
                       [HeadAttribute(\"/items\")] Task Head();\n\
                       Task Plain();\n}\n";

    /// The consumers of the files (`(path, source)`), with `configs` beside them, sorted.
    fn sent(sources: &[(&str, &str)], configs: &[(&str, &[u8])]) -> Vec<Sent> {
        let parsed: Vec<_> = (sources.iter())
            .map(|(p, s)| (*p, CSharp.parse(p, s.as_bytes()).unwrap()))
            .collect();
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &CSharp);
        let code = Code::new(&resolver, &files, &CSharp);
        let indexed = Indexed {
            manifests: &[],
            tsconfigs: &[],
            exported: &[],
            files: &[],
            configs,
        };
        let mut found: Vec<Sent> = (consumers(&code, &indexed).into_iter())
            .map(|f| {
                let at = match f.site {
                    Site::Def { name, .. } => name,
                    _ => String::new(),
                };
                (f.contract, f.via, at)
            })
            .collect();
        found.sort();
        found
    }

    fn at(contract: &str, via: Option<&str>, target: &str) -> Sent {
        (
            Some(contract.into()),
            via.map(str::to_string),
            format!("A.IApi.{target}"),
        )
    }

    /// `body`, a static class's method that makes the client, beside `API`.
    fn made(body: &str) -> Vec<Sent> {
        let maker = format!(
            "using Refit;\nnamespace A;\npublic static class Make {{\n\
             public static IApi Api() {{ {body} }}\n}}\n"
        );
        sent(&[("A/IApi.cs", API), ("A/Make.cs", &maker)], &[])
    }

    #[test]
    fn each_method_of_a_refit_interface_is_a_consumer_with_the_base_it_is_made_with() {
        let host = Some("host:refit.example.com");
        assert_eq!(
            made("return RestService.For<IApi>(\"https://refit.example.com/v1/\");"),
            [
                at("http::DELETE::/v1/items/{}", host, "Delete(int)"),
                at("http::GET::/v1/items/{}", host, "Item(int)"),
                at("http::HEAD::/v1/items", host, "Head()"),
                at("http::PATCH::/v1/items/{}", host, "Patch(int)"),
                at("http::POST::/v1/items", host, "Create()"),
                at("http::PUT::/v1/items/{}", host, "Put(int)"),
            ]
        );
    }

    /// The consumers of `IApi.Item(int)` alone.
    fn item(found: Vec<Sent>) -> Vec<(Option<String>, Option<String>)> {
        (found.into_iter())
            .filter(|(.., at)| at == "A.IApi.Item(int)")
            .map(|(c, v, _)| (c, v))
            .collect()
    }

    #[test]
    fn a_base_a_wrapper_takes_and_one_from_configuration_are_read_at_their_callers() {
        let wrapped = "return With(\"https://wrapped.example.com\"); }\n\
                       static IApi With(string url) { return RestService.For<IApi>(url);";
        assert_eq!(
            item(made(wrapped)),
            [(
                Some("http::GET::/items/{}".into()),
                Some("host:wrapped.example.com".into())
            )]
        );
        let maker = "using Refit;\nusing Microsoft.Extensions.Configuration;\nnamespace A;\n\
                     public class Make {\n\
                     public IApi Api(IConfiguration config) => RestService.For<IApi>(config[\"Api:Url\"]);\n}\n";
        let settings: &[u8] = br#"{ "Api": { "Url": "https://config.example.com/v2" } }"#;
        let found = sent(
            &[("A/IApi.cs", API), ("A/Make.cs", maker)],
            &[("appsettings.json", settings)],
        );
        assert_eq!(
            item(found),
            [
                (
                    Some("http::GET::/items/{}".into()),
                    Some("env:Api__Url|host:config.example.com".into())
                ),
                (
                    Some("http::GET::/v2/items/{}".into()),
                    Some("host:config.example.com|default".into())
                ),
            ]
        );
    }

    #[test]
    fn an_interface_no_rest_service_makes_has_an_unknown_base() {
        // `AddRefitClient` is not read.
        let registered = "services.AddRefitClient<IApi>()\
                          .ConfigureHttpClient(c => c.BaseAddress = new Uri(\"https://x.example.com\"));\n\
                          return null;";
        let unknown = [(Some("http::GET::/items/{}".into()), None)];
        assert_eq!(item(made(registered)), unknown);
        // Another `For`, or one of another interface.
        assert_eq!(
            item(made("return Other.For<IApi>(\"https://x.example.com\");")),
            unknown
        );
    }

    #[test]
    fn an_attribute_refit_does_not_bind_or_a_path_refit_rejects_is_no_consumer() {
        let api = |usings: &str, attribute: &str| {
            format!(
                "{usings}\nnamespace A;\n\
                 public interface IApi {{ [{attribute}] System.Threading.Tasks.Task Item(int id); }}\n"
            )
        };
        let none: Vec<Sent> = Vec::new();
        // No `using Refit`, a class of the repository named like it, a relative path.
        assert_eq!(sent(&[("A/IApi.cs", &api("", "Get(\"/a\")"))], &[]), none);
        let own = format!(
            "{}\npublic class GetAttribute : System.Attribute {{ public GetAttribute(string p) {{}} }}\n",
            api("using Refit;", "Get(\"/a\")")
        );
        assert_eq!(sent(&[("A/IApi.cs", &own)], &[]), none);
        assert_eq!(
            sent(&[("A/IApi.cs", &api("using Refit;", "Get(\"a\")"))], &[]),
            none
        );
        // Written in full, it binds without a using.
        assert_eq!(
            sent(&[("A/IApi.cs", &api("", "Refit.Get(\"/a\")"))], &[]),
            [(
                Some("http::GET::/a".into()),
                None,
                "A.IApi.Item(int)".into()
            )]
        );
    }

    /// The consumers of `ping` by `(host, the interface that declares it)`.
    fn pings(sources: &[(&str, &str)]) -> Vec<(Option<String>, String)> {
        (sent(sources, &[]).into_iter())
            .filter(|(c, ..)| c.as_deref() == Some("http::GET::/ping"))
            .map(|(_, v, at)| (v, at))
            .collect()
    }

    fn files(interfaces: &str, makers: &str) -> String {
        format!(
            "using Refit;\nusing System.Threading.Tasks;\nnamespace A;\n{interfaces}\n\
             public class Make {{ void Run() {{ {makers} }} }}\n"
        )
    }

    const BASE: &str = "public interface IBase { [Get(\"/ping\")] Task Ping(); }\n";

    #[test]
    fn a_for_gives_its_base_to_the_endpoints_its_interface_inherits() {
        let ifaces = format!(
            "{BASE}public interface IOrders : IBase {{ [Get(\"/orders\")] Task List(); }}\n\
             public interface IEmpty : IBase {{ }}\n\
             public interface IOut : Elsewhere.IThing {{ }}\n"
        );
        let host = |h: &str| Some(format!("host:{h}"));
        let src = files(
            &ifaces,
            "RestService.For<IOrders>(\"https://o.example.com\");",
        );
        assert_eq!(
            pings(&[("A/A.cs", &src)]),
            [(host("o.example.com"), "A.IBase.Ping()".into())]
        );
        // No endpoint of its own, and a base from outside the repository adds nothing.
        let src = files(
            &ifaces,
            "RestService.For<IEmpty>(\"https://e.example.com\"); RestService.For<IOut>(\"https://x.example.com\");",
        );
        assert_eq!(
            pings(&[("A/A.cs", &src)]),
            [(host("e.example.com"), "A.IBase.Ping()".into())]
        );
        // Two interfaces sharing a base: one consumer per base.
        let src = files(
            &ifaces,
            "RestService.For<IOrders>(\"https://o.example.com\"); RestService.For<IEmpty>(\"https://e.example.com\");",
        );
        assert_eq!(
            pings(&[("A/A.cs", &src)]),
            [
                (host("e.example.com"), "A.IBase.Ping()".into()),
                (host("o.example.com"), "A.IBase.Ping()".into()),
            ]
        );
    }

    #[test]
    fn the_walk_over_bases_ends_on_a_cycle_and_at_the_bound() {
        let host = Some("host:o.example.com".to_string());
        let make = "RestService.For<I0>(\"https://o.example.com\");";
        // `IBase : I0` would close a cycle: it ends, with the base kept.
        let cyclic = "public interface IBase : I0 { [Get(\"/ping\")] Task Ping(); }\n\
             public interface I0 : IBase { }\n";
        let src = files(cyclic, make);
        assert_eq!(
            pings(&[("A/A.cs", &src)]),
            [(host.clone(), "A.IBase.Ping()".into())]
        );
        // A chain `I0 : I1 : ... : In` whose last link declares the endpoint.
        let chain = |n: usize| {
            let mut s = String::new();
            for i in 0..n {
                s += &format!("public interface I{i} : I{} {{ }}\n", i + 1);
            }
            s + &format!("public interface I{n} {{ [Get(\"/ping\")] Task Ping(); }}\n")
        };
        let reached = files(&chain(BASE_DEPTH), make);
        assert_eq!(
            pings(&[("A/A.cs", &reached)]),
            [(host, format!("A.I{BASE_DEPTH}.Ping()"))]
        );
        let past = files(&chain(BASE_DEPTH + 1), make);
        assert_eq!(
            pings(&[("A/A.cs", &past)]),
            [(None, format!("A.I{}.Ping()", BASE_DEPTH + 1))]
        );
    }
}
