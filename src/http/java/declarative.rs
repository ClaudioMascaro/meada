// ponytail: `name` is the literal one (`value`, or the first argument); a placeholder in it is
// kept as written.

use super::{
    MAPPING, MAPPING_PATHS, balanced, folded_local, jax_rs_annotation, made, mapping, paths,
    request_methods, shortcut,
};
use crate::contract::{Found, Site};
use crate::index::Indexed;
use crate::jvm::names::{java_files, jvm_files, kotlin};
use crate::jvm::spring::{self, Configs};
use crate::lang::kotlin::TYPE_ARGUMENT;
use crate::lang::{Call, Facts, Invocation, Part, Value, literal};
use crate::names::{annotation, is_method, known, one, spellings};
use crate::resolve::Code;
use crate::resolve::{Expand, Inject, Injected, method as member};
use std::cell::OnceCell;
use std::collections::BTreeMap;

use super::super::{ANY, CLIENT_VIA, METHODS, based, consumer_at, fetches, join};

fn text(t: &str) -> Value {
    Value::Str(vec![Part::Text(t.to_string())])
}

const FEIGN: &str = "org.springframework.cloud.openfeign.FeignClient";

pub(in crate::http) fn consumers<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Found> {
    let configs = OnceCell::new();
    let mut found = Vec::new();
    for (path, facts) in java_files(code) {
        let annotations: Vec<(usize, &'a Call, String)> = (facts.calls.iter().enumerate())
            .filter(|(_, c)| c.decorates.is_some())
            .filter_map(|(n, c)| Some((n, c, annotation(code, path, c, is_feign)?)))
            .collect();
        feign_clients(
            code,
            path,
            facts,
            &annotations,
            (&configs, indexed),
            &mut found,
        );
    }
    retrofit_clients(code, (&configs, indexed), &mut found);
    found
}

fn is_feign(spelled: &str) -> bool {
    spelled == FEIGN || mapping(spelled)
}

fn feign_clients<'a>(
    code: &Code<'a>,
    path: &'a str,
    facts: &'a Facts,
    annotations: &[(usize, &'a Call, String)],
    configs: (&OnceCell<Configs>, &Indexed),
    found: &mut Vec<Found>,
) {
    let on = |target: &'a str| {
        annotations
            .iter()
            .filter(move |(_, c, _)| c.decorates.as_deref() == Some(target))
    };
    for interface in &facts.interfaces {
        let Some((n, ..)) = on(interface).find(|(.., name)| name == FEIGN) else {
            continue;
        };
        if on(interface).any(|(.., name)| name == MAPPING) {
            continue;
        }
        let Some(client) = code.call(path, *n) else {
            continue;
        };
        let Some(base) = Base::of(code, path, client, configs) else {
            continue;
        };
        for (n, call, name) in annotations {
            let Some(target) = call
                .decorates
                .as_deref()
                .filter(|t| is_method(facts, t))
                .filter(|_| call.scope == *interface)
            else {
                continue;
            };
            let shortcut = shortcut(name);
            if shortcut.is_none() && name != MAPPING {
                continue;
            }
            let Some(given) = code.call(path, *n) else {
                continue;
            };
            let methods = match shortcut {
                Some(verb) => vec![verb.to_string()],
                None => match request_methods(code, path, given) {
                    Some(methods) => methods
                        .into_iter()
                        .map(|m| if m == ANY { "GET".into() } else { m })
                        .collect(),
                    None => continue,
                },
            };
            let Some(routes) = paths(code, path, given, MAPPING_PATHS) else {
                continue;
            };
            for route in &routes {
                let full = join(&base.prefix, route);
                if !balanced(&full) {
                    continue;
                }
                for method in &methods {
                    let url = based(base.url.as_deref(), &text(&full));
                    let Some(reads) = fetches(&[url, text(method)]) else {
                        continue;
                    };
                    for mut read in reads {
                        read.via = read.via.or_else(|| base.named());
                        let site = Site::Def {
                            path: path.into(),
                            name: target.into(),
                        };
                        found.push(consumer_at(site, read, None));
                    }
                }
            }
        }
    }
}

struct Base {
    url: Option<Vec<Part>>,
    prefix: String,
    name: Option<String>,
}

impl Base {
    fn of<'a>(
        code: &Code<'a>,
        file: &'a str,
        client: &'a Call,
        (cell, indexed): (&OnceCell<Configs>, &Indexed),
    ) -> Option<Self> {
        let configs = cell.get_or_init(|| Configs::read(indexed.configs, indexed.manifests));
        let folded = |value: &'a Value| code.value(file, &client.scope, value);
        let resolved = |value: &'a Value| match spring::resolved(&folded(value), configs.of(file)) {
            Value::Str(parts) => Some(parts),
            _ => None,
        };
        let url = match client.keyword("url").map(resolved) {
            None => None,
            Some(Some(parts)) if literal(&parts).is_some_and(|u| u.is_empty()) => None,
            Some(Some(parts)) => Some(schemed(parts)),
            Some(None) => Some(crate::http::client_base()),
        };
        let prefix = match client.keyword("path").map(resolved) {
            None => String::new(),
            Some(parts) => literal(&parts?)?,
        };
        let name = (client.keyword("name").or(client.keyword("value")))
            .or(client.args.first())
            .map(folded)
            .and_then(|value| match value {
                Value::Str(parts) => literal(&parts),
                _ => None,
            })
            .filter(|n| !n.is_empty() && !n.contains('|'));
        Some(Self { url, prefix, name })
    }

    fn named(&self) -> Option<String> {
        self.name
            .as_ref()
            .map(|n| format!("{CLIENT_VIA}Feign client {n}"))
    }
}

fn schemed(mut url: Vec<Part>) -> Vec<Part> {
    if let Some(Part::Text(first)) = url.first_mut()
        && !first.is_empty()
        && !first.contains("://")
    {
        first.insert_str(0, "http://");
    }
    url
}

const RETROFIT: &str = "retrofit2.Retrofit";
const RETROFIT_BUILDER: &str = "retrofit2.Retrofit.Builder";
const RETROFIT_HTTP: &str = "retrofit2.http.";
const RETROFIT_CREATE: &str = "retrofit2.create";

fn retrofit_verb(spelled: &str) -> Option<&str> {
    let verb = spelled.strip_prefix(RETROFIT_HTTP)?;
    METHODS.contains(&verb).then_some(verb)
}

struct Endpoint<'a> {
    target: &'a str,
    verb: &'static str,
    routes: Vec<String>,
}

type Interface<'a> = (&'a str, &'a str);

fn retrofit_clients<'a>(
    code: &Code<'a>,
    (configs, indexed): (&OnceCell<Configs>, &Indexed),
    found: &mut Vec<Found>,
) {
    let mut interfaces: BTreeMap<Interface, Vec<Endpoint>> = BTreeMap::new();
    for (path, facts) in jvm_files(code) {
        if facts.interfaces.is_empty() {
            continue;
        }
        let feign: Vec<&str> = (facts.calls.iter())
            .filter(|c| annotation(code, path, c, is_feign).as_deref() == Some(FEIGN))
            .filter_map(|c| c.decorates.as_deref())
            .collect();
        for (n, call) in facts.calls.iter().enumerate() {
            let Some(target) = call.decorates.as_deref() else {
                continue;
            };
            let interface = call.scope.as_str();
            if !facts.interfaces.contains(&call.scope) || feign.contains(&interface) {
                continue;
            }
            if !is_method(facts, target) {
                continue;
            }
            let verb = |s: &str| retrofit_verb(s).is_some() || jax_rs_annotation(s);
            let spelled = annotation(code, path, call, verb);
            let named = spelled.as_deref().and_then(retrofit_verb);
            let Some(verb) = METHODS.iter().copied().find(|v| Some(*v) == named) else {
                continue;
            };
            let routes = code
                .call(path, n)
                .and_then(|c| paths(code, path, c, &["value"]));
            let Some(mut routes) = routes else {
                continue;
            };
            routes.retain(|r| balanced(r));
            let endpoint = Endpoint {
                target,
                verb,
                routes,
            };
            interfaces
                .entry((path, interface))
                .or_default()
                .push(endpoint);
        }
    }
    let mut bases: BTreeMap<Interface, Vec<Vec<Part>>> = BTreeMap::new();
    for (path, facts) in jvm_files(code) {
        if interfaces.is_empty() {
            break;
        }
        for (n, stored) in facts.calls.iter().enumerate() {
            if member(&stored.callee) != "create" || stored.decorates.is_some() {
                continue;
            }
            let Some(call) = code.call(path, n) else {
                continue;
            };
            let (ty, class) = match (call.args.as_slice(), call.keyword(TYPE_ARGUMENT)) {
                ([Value::Member(ty, class)], _) => (ty, class),
                ([], Some(Value::Member(ty, class)))
                    if known(code, path, &call.scope, "create", |s| s == RETROFIT_CREATE)
                        .is_some() =>
                {
                    (ty, class)
                }
                _ => continue,
            };
            let made = code.definition(path, &call.scope, ty);
            let Some(key) = made
                .map(|t| (t.path, t.name))
                .filter(|k| interfaces.contains_key(k))
            else {
                continue;
            };
            let inject = |code: &Code<'a>, path: &'a str, on: Injected<'a>| {
                let configs =
                    configs.get_or_init(|| Configs::read(indexed.configs, indexed.manifests));
                spring::inject(configs, code, path, on.annotated()?)
            };
            let inject: Option<&Inject> = kotlin(path).then_some(&inject);
            let client = code.receiver(path, n);
            if class != "class" || !client.is_some_and(|c| is_retrofit(code, path, &call.scope, c))
            {
                continue;
            }
            // A Retrofit held by a local with no written type: its value, where it is made.
            let held = client.and_then(|c| folded_local(code, (path, &call.scope), c, inject));
            let read = |values: &[Value], _: &[(String, Value)]| match values {
                [Value::Call(create)] => Some(base(create, held.as_ref())).filter(|b| !has_ref(b)),
                _ => None,
            };
            let entry = bases.entry(key).or_default();
            for base in code.expand_with(Expand::Chain(path, n), inject, &read) {
                if !entry.contains(&base.found) {
                    entry.push(base.found);
                }
            }
        }
    }
    let unread = crate::http::client_base();
    for (&(path, interface), endpoints) in &interfaces {
        let created = bases.get(&(path, interface)).filter(|b| !b.is_empty());
        let bases: Vec<&[Part]> = match created {
            Some(bases) => bases.iter().map(Vec::as_slice).collect(),
            None => vec![&unread],
        };
        for endpoint in endpoints {
            for (route, base) in
                (endpoint.routes.iter()).flat_map(|r| bases.iter().map(move |b| (r, b)))
            {
                let url = match route.is_empty() {
                    true => Value::Unknown,
                    false => retrofit_url(base, route),
                };
                let Some(reads) = fetches(&[url, text(endpoint.verb)]) else {
                    continue;
                };
                for read in reads {
                    let site = Site::Def {
                        path: path.into(),
                        name: endpoint.target.into(),
                    };
                    found.push(consumer_at(site, read, None));
                }
            }
        }
    }
}

fn is_retrofit<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, client: &'a Value) -> bool {
    made(code, (path, scope), client, RETROFIT, builds)
}

fn builds<'a>(code: &Code<'a>, path: &'a str, index: usize) -> bool {
    let Some(chain) = code.chain(path, index) else {
        return false;
    };
    let (Some(top), Some(&start)) = (code.call(path, index), chain.last()) else {
        return false;
    };
    let Some(first) = code.call(path, start) else {
        return false;
    };
    let written = match code.receiver(path, start) {
        Some(Value::New(ty, _)) => ty,
        _ => &first.callee,
    };
    member(&top.callee) == "build"
        && one(&spellings(code, path, &first.scope, written), |s| {
            s == RETROFIT_BUILDER
        })
        .is_some()
}

fn has_ref(parts: &[Part]) -> bool {
    parts.iter().any(|p| matches!(p, Part::Ref(_)))
}

fn base(create: &Invocation, held: Option<&Value>) -> Vec<Part> {
    let receiver = match (create.receiver.as_deref(), held) {
        (Some(Value::Unknown), Some(held)) => Some(held),
        (receiver, _) => receiver,
    };
    let (mut set, mut at) = (None, receiver);
    while let Some(Value::Call(step)) = at {
        if set.is_none() && member(&step.callee) == "baseUrl" {
            set = step.args.first();
        }
        at = match member(&step.callee) {
            "Builder" => None,
            _ => step.receiver.as_deref(),
        };
    }
    match (set, at) {
        (Some(Value::Str(parts)), _) => parts.clone(),
        (Some(Value::Ref(name)), _) | (None, Some(Value::Ref(name))) => {
            vec![Part::Ref(name.clone())]
        }
        _ => crate::http::client_base(),
    }
}

fn retrofit_url(base: &[Part], route: &str) -> Value {
    if crate::http::rooted_over_unread(base, &text(route)) {
        return text(&join("", route));
    }
    let lead: String = (base.iter())
        .map_while(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    let host = (route.starts_with('/'))
        .then(|| lead.split_once("://"))
        .flatten();
    let rooted = match host {
        Some((scheme, rest)) if rest.contains('/') => {
            let host = rest.split('/').next().unwrap_or_default();
            vec![Part::Text(format!("{scheme}://{host}"))]
        }
        _ => base.to_vec(),
    };
    based(Some(&rooted), &text(route))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Lead, Site};
    use crate::lang::java::Java;
    use crate::lang::{LanguagePack, Limit};
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    type Sent = (String, Option<String>, Option<String>);

    const FEIGN: &str = "import org.springframework.cloud.openfeign.FeignClient;\n\
        import org.springframework.web.bind.annotation.GetMapping;\n\
        import org.springframework.web.bind.annotation.PostMapping;\n\
        import org.springframework.web.bind.annotation.RequestMapping;\n\
        import org.springframework.web.bind.annotation.RequestMethod;\n";

    fn sent_in(
        imports: &str,
        body: &str,
        others: &[(&str, &str)],
        configs: &[(&str, &str)],
    ) -> Vec<Sent> {
        let mut found: Vec<Sent> = (found_in(imports, body, others, configs).into_iter())
            .map(|f| (site(f.site), f.contract, f.via))
            .collect();
        found.sort();
        found
    }

    fn site(site: Site) -> String {
        match site {
            Site::Def { name, .. } => name,
            _ => "(not a definition)".into(),
        }
    }

    fn found_in(
        imports: &str,
        body: &str,
        others: &[(&str, &str)],
        configs: &[(&str, &str)],
    ) -> Vec<Found> {
        let src = format!("package a;\n{imports}\n{body}\n");
        let at = "api/src/main/java/a/C.java";
        let mut parsed = vec![(at, Java.parse(at, src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, Java.parse(path, source.as_bytes()).unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        let configs: Vec<(&str, &[u8])> =
            (configs.iter()).map(|(p, c)| (*p, c.as_bytes())).collect();
        let indexed = Indexed {
            manifests: &[],
            tsconfigs: &[],
            exported: &[],
            files: &[],
            configs: &configs,
            symlinks: &[],
            channel_clients: &[],
        };
        consumers(&code, &indexed)
    }

    fn sent(imports: &str, body: &str) -> Vec<Sent> {
        sent_in(imports, body, &[], &[])
    }

    fn to(site: &str, contract: &str, via: &str) -> Sent {
        (site.into(), Some(contract.into()), Some(via.into()))
    }

    #[test]
    fn a_feign_method_is_a_consumer_with_the_base_of_its_url() {
        let body = "@FeignClient(name = \"orders\", url = \"https://orders.example.com/\")\n\
            interface Orders { @GetMapping(\"/orders/{id}\") String get(String id); \
            @PostMapping(\"/orders\") void place(); }";
        assert_eq!(
            sent(FEIGN, body),
            [
                to(
                    "a.Orders.get(String)",
                    "http::GET::/orders/{}",
                    "host:orders.example.com"
                ),
                to(
                    "a.Orders.place()",
                    "http::POST::/orders",
                    "host:orders.example.com"
                ),
            ]
        );
    }

    fn client(args: &str, methods: &str) -> String {
        format!("@FeignClient({args}) interface Orders {{ {methods} }}")
    }

    const GET: &str = "@GetMapping(\"/orders\") String list();";
    const LIST: &str = "a.Orders.list()";

    #[test]
    fn a_url_placeholder_resolves_through_the_configuration_of_the_module() {
        let config = [(
            "api/src/main/resources/application.properties",
            "orders.url=https://orders.example.com\n",
        )];
        let body = client("name = \"orders\", url = \"${orders.url}\"", GET);
        let found = sent_in(FEIGN, &body, &[], &config);
        assert_eq!(
            found,
            [to(LIST, "http::GET::/orders", "host:orders.example.com")]
        );
        let found = sent(FEIGN, &body);
        assert_eq!(found, [to(LIST, "http::GET::/orders", "env:orders.url")]);
        let body = client(
            "name = \"orders\", url = \"${ORDERS:https://o.example.com}\"",
            GET,
        );
        let vias: Vec<_> = sent(FEIGN, &body).into_iter().filter_map(|s| s.2).collect();
        assert_eq!(
            vias,
            [
                "env:ORDERS|host:o.example.com",
                "host:o.example.com|default"
            ]
        );
    }

    #[test]
    fn a_url_without_a_scheme_gets_http_like_spring_cloud_does() {
        let at = |url: &str| {
            sent(
                FEIGN,
                &client(&format!("name = \"orders\", url = \"{url}\""), GET),
            )
        };
        let want = [to(LIST, "http::GET::/orders", "host:orders.example.com")];
        assert_eq!(at("orders.example.com"), want);
        assert_eq!(at("http://orders.example.com"), want);
        let port = [to(LIST, "http::GET::/orders", "host:localhost:8080")];
        assert_eq!(at("localhost:8080/"), port);
    }

    #[test]
    fn a_client_without_url_has_an_unresolved_host_and_names_itself() {
        let named = |args: &str| sent(FEIGN, &client(args, GET));
        let want = [to(LIST, "http::GET::/orders", "client:Feign client orders")];
        assert_eq!(named("name = \"orders\""), want);
        assert_eq!(named("value = \"orders\""), want);
        assert_eq!(named("\"orders\""), want);
        assert_eq!(named("name = \"orders\", url = \"\""), want);
        let prefixed = named("name = \"orders\", path = \"/v1/\"");
        assert_eq!(
            prefixed,
            [to(
                LIST,
                "http::GET::/v1/orders",
                "client:Feign client orders"
            )]
        );
        let both = named("name = \"orders\", url = \"https://h.example.com/api\", path = \"v1\"");
        assert_eq!(
            both,
            [to(LIST, "http::GET::/api/v1/orders", "host:h.example.com")]
        );
        let unnamed = named("name = Missing.NAME");
        assert_eq!(
            unnamed,
            [(LIST.into(), Some("http::GET::/orders".into()), None)]
        );
        assert_eq!(named("name = \"a|b\"")[0].2, None);
    }

    #[test]
    fn a_request_mapping_method_and_paths_give_one_consumer_each() {
        let body = client(
            "name = \"orders\"",
            "@RequestMapping(value = {\"/a\", \"/b\"}, method = RequestMethod.PUT) void f(); \
             @RequestMapping(\"/c\") void g();",
        );
        let found: Vec<_> = (sent(FEIGN, &body).into_iter())
            .map(|(site, c, _)| (site, c.unwrap()))
            .collect();
        assert_eq!(
            found,
            [
                ("a.Orders.f()".to_string(), "http::PUT::/a".to_string()),
                ("a.Orders.f()".to_string(), "http::PUT::/b".to_string()),
                ("a.Orders.g()".to_string(), "http::GET::/c".to_string()),
            ]
        );
    }

    #[test]
    fn what_is_not_a_spring_mapped_feign_interface_gives_no_consumer() {
        let marked = client("name = \"orders\"", GET);
        assert!(sent(FEIGN, &format!("interface Orders {{ {GET} }}")).is_empty());
        assert!(
            sent(
                FEIGN,
                &format!("@FeignClient(name = \"o\") class Orders {{ {GET} }}")
            )
            .is_empty()
        );
        let other = FEIGN.replace("org.springframework.cloud.openfeign", "org.acme");
        assert!(sent(&other, &marked).is_empty());
        let other = FEIGN.replace("web.bind.annotation.GetMapping", "acme.GetMapping");
        assert!(sent(&other, &marked).is_empty());
        let local = [(
            "api/src/main/java/a/FeignClient.java",
            "package a; class FeignClient {}",
        )];
        let on_demand = "import org.springframework.cloud.openfeign.*;\n\
            import org.springframework.web.bind.annotation.*;\n";
        assert!(sent_in(on_demand, &marked, &local, &[]).is_empty());
        assert_eq!(sent(on_demand, &marked).len(), 1);
        let local = [(
            "api/src/main/java/a/GetMapping.java",
            "package a; class GetMapping {}",
        )];
        let wild = "import org.springframework.cloud.openfeign.FeignClient;\n\
            import org.springframework.web.bind.annotation.*;\n";
        assert!(sent_in(wild, &marked, &local, &[]).is_empty());
        let prefixed = format!("@RequestMapping(\"/v1\") {marked}");
        assert!(sent(FEIGN, &prefixed).is_empty());
        let line = format!("import feign.RequestLine;\n{FEIGN}");
        let contract = client(
            "name = \"orders\"",
            "@RequestLine(\"GET /orders\") String list();",
        );
        assert!(sent(&line, &contract).is_empty());
        let served =
            format!("import org.springframework.web.bind.annotation.RestController;\n{FEIGN}");
        assert!(
            sent(
                &served,
                &format!("@RestController interface Orders {{ {GET} }}")
            )
            .is_empty()
        );
    }

    #[test]
    fn a_wildcard_import_binds_the_feign_names_through_one_package() {
        let wild = "import org.springframework.cloud.openfeign.*;\n\
            import org.springframework.web.bind.annotation.*;\n";
        let body = client("name = \"orders\"", GET);
        assert_eq!(
            sent(wild, &body),
            [to(LIST, "http::GET::/orders", "client:Feign client orders")]
        );
    }

    const RETROFIT: &str = "import retrofit2.Retrofit;\n\
        import retrofit2.http.GET;\n\
        import retrofit2.http.POST;\n";

    const API: &str = "interface Api { @GET(\"orders/{id}\") Object get(String id); \
        @POST(\"orders\") Object place(); }";

    fn factory(base: &str) -> String {
        format!(
            "{API} class Factory {{ static Api api() {{ \
             return new Retrofit.Builder().baseUrl({base}).build().create(Api.class); }} }}"
        )
    }

    fn base_of(sent: Vec<Sent>) -> Vec<(String, Option<String>)> {
        sent.into_iter()
            .map(|(s, c, v)| (format!("{s} {}", c.unwrap_or_default()), v))
            .collect()
    }

    #[test]
    fn a_retrofit_method_is_a_consumer_with_the_base_of_the_builder_that_creates_it() {
        let found = sent(RETROFIT, &factory("\"https://orders.example.com/v1/\""));
        assert_eq!(
            found,
            [
                to(
                    "a.Api.get(String)",
                    "http::GET::/v1/orders/{}",
                    "host:orders.example.com"
                ),
                to(
                    "a.Api.place()",
                    "http::POST::/v1/orders",
                    "host:orders.example.com"
                ),
            ]
        );
    }

    #[test]
    fn a_retrofit_base_folds_constants_final_fields_and_the_outermost_base_url() {
        let body = format!(
            "{API} class Factory {{ static final String HOST = \"https://o.example.com\"; \
             static final Retrofit R = new Retrofit.Builder().baseUrl(\"https://old.example.com\")\
             .baseUrl(HOST + \"/\").build(); \
             static Api api() {{ return R.create(Api.class); }} }}"
        );
        let vias: Vec<_> = sent(RETROFIT, &body)
            .into_iter()
            .filter_map(|s| s.2)
            .collect();
        assert_eq!(vias, ["host:o.example.com", "host:o.example.com"]);
    }

    #[test]
    fn an_interface_created_with_two_bases_has_a_consumer_for_each() {
        let body = format!(
            "{API} class Factory {{ \
             static Api one() {{ return new Retrofit.Builder().baseUrl(\"https://a.example.com/\").build().create(Api.class); }} \
             static Api two() {{ return new Retrofit.Builder().baseUrl(\"https://b.example.com/\").build().create(Api.class); }} }}"
        );
        let found = sent(RETROFIT, &body);
        let hosts: Vec<_> = found
            .iter()
            .filter(|s| s.0 == "a.Api.place()")
            .filter_map(|s| s.2.as_deref())
            .collect();
        assert_eq!(hosts, ["host:a.example.com", "host:b.example.com"]);
        assert_eq!(found.len(), 4);
    }

    #[test]
    fn a_path_from_the_root_replaces_the_base_path_and_a_relative_one_follows_it() {
        let body = "interface Api { @GET(\"/health\") Object a(); @GET(\"list\") Object b(); \
            @GET(\"\") Object c(); @GET Object d(); } \
            class Factory { static Api api() { return new Retrofit.Builder()\
            .baseUrl(\"https://h.example.com/api/\").build().create(Api.class); } }";
        let found = sent(RETROFIT, body);
        let contracts: Vec<_> = found.iter().map(|s| s.1.as_deref()).collect();
        assert_eq!(
            contracts,
            [
                Some("http::GET::/health"),
                Some("http::GET::/api/list"),
                None,
                None,
            ]
        );
    }

    #[test]
    fn a_path_from_the_root_replaces_the_path_of_a_concatenated_base() {
        let body = "interface Api { @GET(\"/health\") Object a(); @GET(\"list\") Object b(); } \
            class Factory { static final String HOST = \"https://h.example.com\"; \
            static Api api() { return new Retrofit.Builder()\
            .baseUrl(HOST + \"/v1/\").build().create(Api.class); } }";
        let found: Vec<_> = sent(RETROFIT, body)
            .into_iter()
            .map(|s| (s.1.unwrap(), s.2.unwrap()))
            .collect();
        let host = "host:h.example.com";
        assert_eq!(
            found,
            [
                ("http::GET::/health".to_string(), host.to_string()),
                ("http::GET::/v1/list".to_string(), host.to_string()),
            ]
        );
    }

    #[test]
    fn an_interface_no_builder_creates_has_an_unknown_base() {
        let found = sent(RETROFIT, API);
        assert_eq!(
            found,
            [
                ("a.Api.get(String)".into(), None, None),
                ("a.Api.place()".into(), None, None),
            ]
        );
        let other = format!(
            "{API} class F {{ static Api a() {{ return new Other.Builder().baseUrl(\"https://x.example.com\").build().create(Api.class); }} }}"
        );
        assert_eq!(
            base_of(sent(RETROFIT, &other))
                .iter()
                .filter(|(_, v)| v.is_some())
                .count(),
            0
        );
        let hole = factory("Config.url()");
        assert_eq!(
            base_of(sent(RETROFIT, &hole))
                .iter()
                .filter(|(_, v)| v.is_some())
                .count(),
            0
        );
    }

    #[test]
    fn an_unread_base_names_the_client_where_the_path_follows_it() {
        let leads = |imports: &str, body: &str| {
            let mut out: Vec<_> = (found_in(imports, body, &[], &[]).into_iter())
                .map(|f| (site(f.site), f.contract, f.lead))
                .collect();
            out.sort_by(|a, b| a.0.cmp(&b.0));
            out
        };
        let unread = |path: &str| {
            Some(Lead {
                limit: Limit::ClientBase,
                path: path.into(),
            })
        };
        let feign = client("name = \"orders\", url = Config.URL", GET);
        assert_eq!(
            leads(FEIGN, &feign),
            [(LIST.into(), None, unread("/orders"))]
        );
        let api = "interface Api { @GET(\"/health\") Object a(); @GET(\"list\") Object b(); }";
        let rooted = (
            "a.Api.a()".to_string(),
            Some("http::GET::/health".to_string()),
            None,
        );
        let open = ("a.Api.b()".to_string(), None, unread("/list"));
        assert_eq!(leads(RETROFIT, api), [rooted.clone(), open.clone()]);
        let unfolded = format!(
            "{api} class Factory {{ static Api api() {{ return new Retrofit.Builder()\
             .baseUrl(Config.url()).build().create(Api.class); }} }}"
        );
        assert_eq!(leads(RETROFIT, &unfolded), [rooted, open]);
    }

    #[test]
    fn a_retrofit_wrapper_takes_the_base_each_caller_builds() {
        let body = format!(
            "{API} class Factory {{ static Api of(Retrofit retrofit) {{ return retrofit.create(Api.class); }} \
             static Api a() {{ return of(new Retrofit.Builder().baseUrl(\"https://a.example.com/\").build()); }} }}"
        );
        let vias: Vec<_> = sent(RETROFIT, &body)
            .into_iter()
            .filter_map(|s| s.2)
            .collect();
        assert_eq!(vias, ["host:a.example.com", "host:a.example.com"]);
    }

    #[test]
    fn retrofits_annotations_count_only_through_its_package() {
        let jax = RETROFIT.replace("retrofit2.http", "javax.ws.rs");
        assert!(sent(&jax, API).is_empty());
        let shadow = [("api/src/main/java/a/GET.java", "package a; class GET {}")];
        let wild = "import retrofit2.http.*;\n";
        let hidden = sent_in(wild, API, &shadow, &[]);
        assert_eq!(
            hidden.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(),
            ["a.Api.place()"]
        );
        assert_eq!(sent(wild, API).len(), 2);
        let both = "import retrofit2.http.*;\nimport javax.ws.rs.*;\n";
        assert!(sent(both, API).is_empty());
        assert!(
            sent(
                RETROFIT,
                "class Api { @GET(\"x\") Object get() { return null; } }"
            )
            .is_empty()
        );
        let feign = format!("{FEIGN}{RETROFIT}");
        assert!(sent(&feign, &format!("@FeignClient(\"o\") {API}")).is_empty());
    }

    #[test]
    fn a_retrofit_route_with_a_brace_open_past_its_segment_is_not_read() {
        let body =
            "interface Api { @GET(\"/a/{id:x/y}\") Object a(); @GET(\"/b/{id}\") Object b(); }";
        let found: Vec<_> = sent(RETROFIT, body).into_iter().map(|s| s.0).collect();
        assert_eq!(found, ["a.Api.b()"]);
    }

    #[test]
    fn a_retrofit_annotation_reads_its_value_alone() {
        let body =
            "interface Api { @GET(path = \"/a\") Object a(); @GET(value = \"/b\") Object b(); }";
        let found: Vec<_> = sent(RETROFIT, body).into_iter().map(|s| s.1).collect();
        assert_eq!(found, [None, Some("http::GET::/b".to_string())]);
    }
}
