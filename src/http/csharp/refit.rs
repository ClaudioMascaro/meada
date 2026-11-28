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
const VERBS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Post", "POST"),
    ("Put", "PUT"),
    ("Patch", "PATCH"),
    ("Delete", "DELETE"),
    ("Head", "HEAD"),
];

fn verb(spelled: &str) -> Option<&'static str> {
    let name = spelled.strip_prefix(REFIT)?;
    let name = name.strip_suffix("Attribute").unwrap_or(name);
    VERBS.iter().find(|(a, _)| *a == name).map(|(_, v)| *v)
}

struct Endpoint<'a> {
    target: &'a str,
    verb: &'static str,
    route: String,
}

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
    let unread = crate::http::client_base();
    let mut found = Vec::new();
    for (key, endpoints) in &interfaces {
        let bases: Vec<&[Part]> = match bases.get(key).filter(|b| !b.is_empty()) {
            Some(bases) => bases.iter().map(Vec::as_slice).collect(),
            None => vec![&unread],
        };
        for endpoint in endpoints {
            for base in &bases {
                let method = Value::Str(vec![Part::Text(endpoint.verb.into())]);
                let Some(reads) = fetches(&[url(base, &endpoint.route), method]) else {
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
            let read = |values: &[Value], _: &[(String, Value)]| match values.first() {
                Some(Value::Str(p)) if p.iter().any(|p| matches!(p, Part::Ref(_))) => None,
                Some(Value::Ref(_)) => None,
                Some(Value::Str(p)) => Some(p.clone()),
                _ => Some(crate::http::client_base()),
            };
            for base in code.expand(Expand::Call(path, n), &read) {
                let parts = match reads.value(code, path, Value::Str(base.found)) {
                    Value::Str(parts) => parts,
                    _ => crate::http::client_base(),
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

// ponytail: real interfaces extend a few others; this bounds a crafted chain.
const BASE_DEPTH: usize = 32;

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

fn url(base: &[Part], route: &str) -> Value {
    let trim = |parts: &mut Vec<Part>| {
        if let Some(Part::Text(t)) = parts.last_mut() {
            *t = t.trim_end_matches('/').to_string();
        }
    };
    let mut out = base.to_vec();
    match out.as_mut_slice() {
        [Part::EnvOr(_, default)] => trim(default),
        _ => trim(&mut out),
    }
    out.push(Part::Text(route.into()));
    Value::Str(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Lead;
    use crate::lang::csharp::CSharp;
    use crate::lang::{LanguagePack, Limit};
    use crate::resolve::Resolver;

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

    fn sent(sources: &[(&str, &str)], configs: &[(&str, &[u8])]) -> Vec<Sent> {
        let mut found: Vec<Sent> = (found(sources, configs).into_iter())
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

    fn found(sources: &[(&str, &str)], configs: &[(&str, &[u8])]) -> Vec<Found> {
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
            symlinks: &[],
            channel_clients: &[],
        };
        consumers(&code, &indexed)
    }

    fn at(contract: &str, via: Option<&str>, target: &str) -> Sent {
        (
            Some(contract.into()),
            via.map(str::to_string),
            format!("A.IApi.{target}"),
        )
    }

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
        let registered = "services.AddRefitClient<IApi>()\
                          .ConfigureHttpClient(c => c.BaseAddress = new Uri(\"https://x.example.com\"));\n\
                          return null;";
        let unknown = [(None, None)];
        assert_eq!(item(made(registered)), unknown);
        assert_eq!(
            item(made("return Other.For<IApi>(\"https://x.example.com\");")),
            unknown
        );
    }

    #[test]
    fn an_endpoint_on_a_base_meada_did_not_read_names_the_client_base() {
        let maker = "using Refit;\nnamespace A;\npublic static class Make {\n\
                     public static IApi Api() { return RestService.For<IApi>(Settings.Url()); }\n}\n";
        let lead = Some(Lead {
            limit: Limit::ClientBase,
            path: "/items/{}".into(),
        });
        for sources in [
            vec![("A/IApi.cs", API), ("A/Make.cs", maker)],
            vec![("A/IApi.cs", API)],
        ] {
            let items: Vec<_> = (found(&sources, &[]).into_iter())
                .filter(|f| matches!(&f.site, Site::Def { name, .. } if name == "A.IApi.Item(int)"))
                .map(|f| (f.contract, f.lead))
                .collect();
            assert_eq!(items, [(None, lead.clone())]);
        }
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
        assert_eq!(
            sent(&[("A/IApi.cs", &api("", "Refit.Get(\"/a\")"))], &[]),
            [(None, None, "A.IApi.Item(int)".into())]
        );
    }

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
        let src = files(
            &ifaces,
            "RestService.For<IEmpty>(\"https://e.example.com\"); RestService.For<IOut>(\"https://x.example.com\");",
        );
        assert_eq!(
            pings(&[("A/A.cs", &src)]),
            [(host("e.example.com"), "A.IBase.Ping()".into())]
        );
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
        let cyclic = "public interface IBase : I0 { [Get(\"/ping\")] Task Ping(); }\n\
             public interface I0 : IBase { }\n";
        let src = files(cyclic, make);
        assert_eq!(
            pings(&[("A/A.cs", &src)]),
            [(host.clone(), "A.IBase.Ping()".into())]
        );
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
            sent(&[("A/A.cs", &past)], &[]),
            [(None, None, format!("A.I{}.Ping()", BASE_DEPTH + 1))]
        );
    }
}
