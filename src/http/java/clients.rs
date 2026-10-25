//! Java HTTP consumers (spec #26): RestTemplate, WebClient, `java.net.http` and OkHttp. A
//! consumer is a call on a client the source shows is the library's: a RestTemplate method on a
//! name written as a `RestTemplate` (`Code::origins` with `Follow::Declarations`); the `.uri(..)`
//! of a WebClient request (`get()`, `method(HttpMethod.X)`) made on a name written as a
//! `WebClient` or made by `WebClient.create` or `WebClient.builder()`; and the `build()` of a
//! builder chain that starts at `HttpRequest.newBuilder()` or `new Request.Builder()`, each name
//! read through `Code::outside`. A WebClient's base is that of the `create(base)` or
//! `builder().baseUrl(base)` the name traces back to (`Follow::Values`), unknown for a client
//! only its type tells.
//!
//! The call is read whole (`Expand::Chain`), so its builder chain and arguments fold where a
//! wrapper's callers pass them, and an enum member (`HttpMethod.GET`) arrives as a
//! `Value::External` spelled by the imports of the file that writes it: a type of the repository
//! or another package's by that name is no Spring method. The URL folds `+`, constants,
//! `URI.create(..)` and `String.format` (each `%s` or `%d` a hole); the root's `fetch` reads it.
// ponytail: a client or a builder held in a local or an instance field is read by its written
// type alone (a WebClient's base unknown, a builder not at all); `static final` ones and chains
// written in place are read through.

use super::super::{based, consumer, fetch};
use super::{REQUEST_METHODS, one};
use crate::contract::Found;
use crate::lang::{Invocation, Part, Value};
use crate::resolve::{Code, Expand, Expanded, Follow, Form, Of, method as member, passes_nothing};

const REST_TEMPLATE: &str = "org.springframework.web.client.RestTemplate";
/// The methods of a RestTemplate that send one method; `exchange` names it.
const REST_METHODS: &[(&str, &str)] = &[
    ("getForObject", "GET"),
    ("getForEntity", "GET"),
    ("postForObject", "POST"),
    ("postForEntity", "POST"),
    ("put", "PUT"),
    ("patchForObject", "PATCH"),
    ("delete", "DELETE"),
];
const WEB_CLIENT: &str = "org.springframework.web.reactive.function.client.WebClient";
/// The methods of a WebClient that start a request of one method; `method` names it.
const WEB_METHODS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("head", "HEAD"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("options", "OPTIONS"),
];
const HTTP_METHOD: &str = "org.springframework.http.HttpMethod.";
const NEW_BUILDER: &str = "java.net.http.HttpRequest.newBuilder";
/// The methods of an `HttpRequest.Builder` that set one method; `method` names it.
const JDK_METHODS: &[(&str, &str)] = &[
    ("GET", "GET"),
    ("POST", "POST"),
    ("PUT", "PUT"),
    ("DELETE", "DELETE"),
];
const OK_BUILDER: &str = "okhttp3.Request.Builder";
/// The methods of an OkHttp `Request.Builder` that set one method; `method` names it.
const OK_METHODS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("head", "HEAD"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
];

/// The client a call sends on, as far as the call site tells.
enum Client {
    /// A RestTemplate method sending this method; `None` for `exchange`.
    Rest(Option<&'static str>),
    /// A WebClient request, with the base URL of the client (`None` without one).
    Web(Option<Vec<Part>>),
    /// The builder chain of an `HttpRequest` or an OkHttp `Request`: where it starts (the URL
    /// `newBuilder(uri)` may give) and the methods that set the method.
    Builder {
        start: &'static str,
        setter: &'static str,
        methods: &'static [(&'static str, &'static str)],
    },
}

/// Java HTTP consumers: see the module.
pub(in crate::http) fn consumers<'a>(code: &Code<'a>) -> Vec<Found> {
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        if !path.ends_with(".java") {
            continue;
        }
        for (n, stored) in facts.calls.iter().enumerate() {
            let Some(client) = client(code, path, n, member(&stored.callee)) else {
                continue;
            };
            let read = |values: &[Value], _: &[(String, Value)]| {
                let [Value::Call(call)] = values else {
                    return None;
                };
                let (url, method) = sends(&client, call);
                fetch(&[url, method])
            };
            for Expanded {
                site: (site, at),
                found: read,
                ..
            } in code.expand(Expand::Chain(path, n), &read)
            {
                found.push(consumer(site, at, read, None));
            }
        }
    }
    found
}

/// The client the call at `index` of `path`, whose callee names `member`, sends on.
fn client<'a>(code: &Code<'a>, path: &'a str, index: usize, member: &str) -> Option<Client> {
    let rest = REST_METHODS.iter().find(|(m, _)| *m == member);
    if rest.is_some() || member == "exchange" {
        let call = code.call(path, index)?;
        let Some(Value::Ref(name)) = code.receiver(path, index) else {
            return None;
        };
        let typed = typed(code, path, &call.scope, name)?;
        return (typed == REST_TEMPLATE).then(|| Client::Rest(rest.map(|(_, m)| *m)));
    }
    match member {
        "uri" => {
            let call = code.call(path, index)?;
            let Some(Value::Call(request)) = code.receiver(path, index) else {
                return None;
            };
            let starts = self::member(&request.callee);
            if starts != "method" && !WEB_METHODS.iter().any(|(m, _)| *m == starts) {
                return None;
            }
            web_base(code, path, &call.scope, request.receiver.as_deref()?).map(Client::Web)
        }
        "build" => {
            let chain = code.chain(path, index)?;
            let start = *chain.last()?;
            let call = code.call(path, start)?;
            let started = |name: &'a str, known: &str| {
                one(&spellings(code, path, &call.scope, name), |s| s == known).is_some()
            };
            if started(&call.callee, NEW_BUILDER) {
                return Some(Client::Builder {
                    start: "newBuilder",
                    setter: "uri",
                    methods: JDK_METHODS,
                });
            }
            match code.receiver(path, start) {
                Some(Value::New(ty, _)) if started(ty, OK_BUILDER) => Some(Client::Builder {
                    start: "",
                    setter: "url",
                    methods: OK_METHODS,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The URL and the method of `call`, read whole by `Expand::Chain`, on `client`.
fn sends(client: &Client, call: &Invocation) -> (Value, Value) {
    let text = |m: &str| Value::Str(vec![Part::Text(m.to_string())]);
    match client {
        Client::Rest(method) => {
            let method = match method {
                Some(m) => text(m),
                None => spring_method(call.args.get(1)),
            };
            (url(call.args.first()), method)
        }
        Client::Web(base) => {
            let method = match call.receiver.as_deref() {
                Some(Value::Call(request)) => match member(&request.callee) {
                    "method" => spring_method(request.args.first()),
                    m => (WEB_METHODS.iter().find(|(w, _)| *w == m))
                        .map_or(Value::Unknown, |(_, m)| text(m)),
                },
                _ => Value::Unknown,
            };
            (based(base.as_deref(), &url(call.args.first())), method)
        }
        Client::Builder {
            start,
            setter,
            methods,
        } => {
            // Outermost first: the last call that sets a part is the one that holds.
            let (mut url, mut method) = (None, None);
            let mut at = Some(call);
            while let Some(step) = at {
                let name = member(&step.callee);
                if url.is_none() && (name == *setter || name == *start) {
                    url = step.args.first().map(|u| self::url(Some(u)));
                }
                if method.is_none() {
                    if name == "method" {
                        method = Some(step.args.first().cloned().unwrap_or(Value::Unknown));
                    } else if let Some((_, m)) = methods.iter().find(|(s, _)| *s == name) {
                        method = Some(text(m));
                    }
                }
                at = match step.receiver.as_deref() {
                    Some(Value::Call(next)) if name != *start => Some(next),
                    _ => None,
                };
            }
            (url.unwrap_or(Value::Unknown), method.unwrap_or(text("GET")))
        }
    }
}

/// The URL a folded argument gives: through `URI.create(..)` and `String.format(..)`.
fn url(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return Value::Unknown;
    };
    let Value::Call(call) = value else {
        return value.clone();
    };
    // Whether the call is made on the class `class`.
    let of = |class: &str| matches!(call.receiver.as_deref(), Some(Value::External(s)) if one(s, |s| s == class).is_some());
    if of("java.net.URI") && member(&call.callee) == "create" {
        return url(call.args.first());
    }
    // `java.lang` needs no import: a bare `String` is its own unless something hides it.
    if (of("String") || of("java.lang.String")) && member(&call.callee) == "format" {
        return format(call.args.first());
    }
    Value::Unknown
}

/// `String.format` with the format `value`: each `%s` or `%d` a hole, `%%` a `%`. Unknown for
/// any other conversion, and for a format that is no string.
fn format(value: Option<&Value>) -> Value {
    let parts = match value {
        Some(Value::Str(parts)) => parts,
        Some(Value::Ref(name)) => return Value::Ref(name.clone()),
        _ => return Value::Unknown,
    };
    let mut out = Vec::new();
    for part in parts {
        let Part::Text(text) = part else {
            out.push(part.clone());
            continue;
        };
        let mut literal = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                literal.push(c);
                continue;
            }
            match chars.next() {
                Some('%') => literal.push('%'),
                Some('s' | 'd') => {
                    out.push(Part::Text(std::mem::take(&mut literal)));
                    out.push(Part::Hole);
                }
                _ => return Value::Unknown,
            }
        }
        out.push(Part::Text(literal));
    }
    out.retain(|p| !matches!(p, Part::Text(t) if t.is_empty()));
    Value::Str(out)
}

/// The method a folded `HttpMethod.X` gives: Spring's member as text, a parameter as it is
/// (the call is a wrapper's), anything else unknown.
fn spring_method(value: Option<&Value>) -> Value {
    fn member(s: &str) -> Option<&str> {
        let m = s.strip_prefix(HTTP_METHOD)?;
        REQUEST_METHODS.contains(&m).then_some(m)
    }
    match value {
        Some(Value::External(spellings)) => match one(spellings, |s| member(s).is_some()) {
            Some(s) => Value::Str(vec![Part::Text(member(s).unwrap_or_default().into())]),
            None => Value::Unknown,
        },
        Some(Value::Ref(name)) => Value::Ref(name.clone()),
        _ => Value::Unknown,
    }
}

/// How the imports of `path` may spell `name`, written in `scope`: the import that binds it,
/// or the on-demand imports that may.
fn spellings<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Vec<String> {
    match code.outside(path, scope, name, false) {
        Some(o) if !o.candidates.is_empty() => o.candidates,
        Some(o) if o.direct => vec![o.spelled],
        _ => Vec::new(),
    }
}

/// The outside type the source writes for `name` (a field, a parameter, a local).
fn typed<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, name: &'a str) -> Option<String> {
    let found = code.origins(path, scope, Of::Name(name), Follow::Declarations);
    let origin = found.into_iter().next()?;
    let typed = origin.form == Form::Type && origin.routes.iter().all(|r| r.steps.is_empty());
    typed.then_some(origin.outside?.spelled)
}

/// The base URL of the WebClient `client` (a name, or a call made in place), `None` when it
/// is no WebClient: `Some(None)` without a base, an unknown one when only its type tells or
/// the clients it may be differ.
fn web_base<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    client: &'a Value,
) -> Option<Option<Vec<Part>>> {
    let hole = || Some(vec![Part::Hole]);
    let (made, typed) = match client {
        Value::Ref(name) => {
            let follow = Follow::Values {
                passes: passes_nothing,
            };
            let origins = code.origins(path, scope, Of::Name(name), follow);
            let made: Vec<_> = (origins.iter().filter_map(|o| o.call))
                .map(|(at, i)| made(code, at, i))
                .collect();
            let typed = typed(code, path, scope, name).is_some_and(|t| t == WEB_CLIENT);
            (made, typed)
        }
        Value::Call(inner) => (
            vec![made(code, path, code.call_index(path, &inner.range)?)],
            false,
        ),
        _ => return None,
    };
    if !typed && made.iter().all(Option::is_none) {
        return None;
    }
    // A call that is no constructor the source shows gives a client whose base is not known.
    let mut bases: Vec<Option<Vec<Part>>> = Vec::new();
    for base in made.into_iter().map(|m| m.unwrap_or_else(hole)) {
        if !bases.contains(&base) {
            bases.push(base);
        }
    }
    match bases.len() {
        1 => bases.pop(),
        _ => Some(hole()),
    }
}

/// The base URL of the WebClient the call at `index` of `path` makes, `None` when it makes
/// none: `WebClient.create(base)`, or a `WebClient.builder()` chain built with its last
/// `baseUrl(base)`; `Some(None)` without a base.
fn made<'a>(code: &Code<'a>, path: &'a str, index: usize) -> Option<Option<Vec<Part>>> {
    let chain = code.chain(path, index)?;
    let calls: Vec<_> = chain
        .iter()
        .map(|&i| code.call(path, i))
        .collect::<Option<_>>()?;
    let start = calls.last()?;
    let names = spellings(code, path, &start.scope, &start.callee);
    let base = |call: &'a crate::lang::Call| match call.args.first() {
        None => None,
        Some(url) => match code.value(path, &call.scope, url) {
            Value::Str(parts) => Some(parts),
            Value::Undefined => None,
            _ => Some(vec![Part::Hole]),
        },
    };
    let created = format!("{WEB_CLIENT}.create");
    let builder = format!("{WEB_CLIENT}.builder");
    if one(&names, |s| s == created).is_some() && calls.len() == 1 {
        return Some(base(start));
    }
    if one(&names, |s| s == builder).is_some() && member(&calls[0].callee) == "build" {
        let set = calls.iter().find(|c| member(&c.callee) == "baseUrl");
        return Some(set.and_then(|c| base(c)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::java::Java;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    /// A consumer as `(contract, via, method)`, the method only for an unresolved one.
    type Sent = (Option<String>, Option<String>, Option<String>);

    const IMPORTS: &str = "import org.springframework.web.client.RestTemplate;\n\
        import org.springframework.web.reactive.function.client.WebClient;\n\
        import org.springframework.http.HttpMethod;\n\
        import java.net.URI;\n\
        import java.net.http.HttpRequest;\n\
        import okhttp3.Request;\n";

    /// The consumers of the class body `body` in `a/C.java`, with other files of the repository.
    fn sent_with(imports: &str, body: &str, others: &[(&str, &str)]) -> Vec<Sent> {
        let src = format!("package a;\n{imports}\nclass C {{\n{body}\n}}\n");
        let mut parsed = vec![("a/C.java", Java.parse("a/C.java", src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, Java.parse(path, source.as_bytes()).unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = Java.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &Java);
        let code = Code::new(&resolver, &files, &Java);
        let mut found: Vec<Sent> = (consumers(&code).into_iter())
            .map(|f| (f.contract, f.via, f.method))
            .collect();
        found.sort();
        found
    }

    fn sent(body: &str) -> Vec<Sent> {
        sent_with(IMPORTS, body, &[])
    }

    /// The contracts of the consumers of `body`.
    fn contracts(body: &str) -> Vec<String> {
        sent(body).into_iter().filter_map(|(c, ..)| c).collect()
    }

    fn to(contract: &str) -> Sent {
        (Some(contract.into()), None, None)
    }

    /// An unresolved consumer of `method`.
    fn open(method: Option<&str>) -> Sent {
        (None, None, method.map(String::from))
    }

    const REST: &str = "RestTemplate rest;\n";

    #[test]
    fn each_rest_template_method_sends_its_method_and_exchange_names_it() {
        let body = format!(
            "{REST}void f() {{\n\
             rest.getForObject(\"/a\", String.class);\n\
             this.rest.getForEntity(\"/b\", String.class);\n\
             rest.postForObject(\"/c\", null, String.class);\n\
             rest.postForEntity(\"/d\", null, String.class);\n\
             rest.put(\"/e\", null);\n\
             rest.patchForObject(\"/f\", null, String.class);\n\
             rest.delete(\"/g\");\n\
             rest.exchange(\"/h\", HttpMethod.PUT, null, String.class);\n\
             rest.exchange(\"/i\", HttpMethod.TRACE, null, String.class);\n}}"
        );
        assert_eq!(
            contracts(&body),
            [
                "http::DELETE::/g",
                "http::GET::/a",
                "http::GET::/b",
                "http::PATCH::/f",
                "http::POST::/c",
                "http::POST::/d",
                "http::PUT::/e",
                "http::PUT::/h",
                "http::TRACE::/i",
            ]
        );
    }

    #[test]
    fn a_call_on_anything_but_a_rest_template_sends_nothing() {
        let others = [(
            "a/Local.java",
            "package a; class Local { void delete(String p) {} }",
        )];
        let body = "Object o; Local local; void f(java.util.Map<String, String> m) {\n\
             local.delete(\"/x\");\n m.get(\"/y\");\n}";
        assert!(sent_with(IMPORTS, body, &others).is_empty());
        let other = IMPORTS.replace(
            "org.springframework.web.client.RestTemplate",
            "org.acme.RestTemplate",
        );
        let body = format!("{REST}void f() {{ rest.getForObject(\"/a\", String.class); }}");
        assert!(sent_with(&other, &body, &[]).is_empty());
    }

    #[test]
    fn an_http_method_that_is_not_springs_member_is_unknown() {
        let exchange = |imports: &str, local: &str, method: &str| {
            let body = format!(
                "{REST}{local}\nvoid f() {{ rest.exchange(\"/x\", {method}, null, String.class); }}"
            );
            sent_with(imports, &body, &[])
        };
        let other = IMPORTS.replace("org.springframework.http.HttpMethod", "org.acme.HttpMethod");
        let wild = IMPORTS.replace(
            "import org.springframework.http.HttpMethod;",
            "import org.springframework.http.*;\nimport java.util.*;",
        );
        assert_eq!(
            exchange(&wild, "", "HttpMethod.POST"),
            [to("http::POST::/x")]
        );
        // Hidden by a type of the file, another package's, not a member, or not a name at all.
        let unknown = [open(None)];
        assert_eq!(
            exchange(IMPORTS, "enum HttpMethod { GET }", "HttpMethod.GET"),
            unknown
        );
        assert_eq!(exchange(&other, "", "HttpMethod.GET"), unknown);
        assert_eq!(exchange(IMPORTS, "", "HttpMethod.SPELL"), unknown);
        assert_eq!(exchange(IMPORTS, "", "null"), unknown);
        // Two on-demand imports that may both bring it: none is certain.
        let both = format!("{wild}import org.acme.*;\n");
        assert_eq!(
            exchange(&both, "", "HttpMethod.POST"),
            [to("http::POST::/x")]
        );
    }

    #[test]
    fn a_url_folds_constants_formats_templates_and_uri_create() {
        let others = [(
            "b/Api.java",
            "package b; public final class Api { public static final String BASE = \"/api\"; }",
        )];
        let imports = format!("{IMPORTS}import b.Api;\n");
        let body = format!(
            "{REST}static final String USERS = Api.BASE + \"/users\";\n\
             void f(int id) {{\n\
             rest.getForObject(USERS + \"/{{id}}\", String.class, id);\n\
             rest.getForObject(String.format(\"/f/%s/%d/x%%\", USERS, id), String.class);\n\
             rest.getForObject(URI.create(USERS + \"/me\"), String.class);\n\
             rest.getForObject(\"https://Users.example.com/u\", String.class);\n}}"
        );
        assert_eq!(
            sent_with(&imports, &body, &others),
            [
                to("http::GET::/api/users/me"),
                to("http::GET::/api/users/{}"),
                to("http::GET::/f/{}/{}/x%"),
                (
                    Some("http::GET::/u".into()),
                    Some("host:users.example.com".into()),
                    None
                ),
            ]
        );
    }

    #[test]
    fn a_url_that_cannot_be_read_is_an_unresolved_consumer() {
        let others = [("a/String.java", "package a; class String {}")];
        let body = format!(
            "{REST}String moving = \"/m\";\n\
             void f() {{\n\
             rest.getForObject(moving, String.class);\n\
             rest.getForObject(String.format(\"/%x\", 1), String.class);\n}}"
        );
        assert_eq!(sent(&body), [open(Some("GET")), open(Some("GET"))]);
        // `String` of the package hides `java.lang.String`.
        let body = format!("{REST}void f() {{ rest.delete(String.format(\"/%s\", 1)); }}");
        assert_eq!(sent_with(IMPORTS, &body, &others), [open(Some("DELETE"))]);
    }

    #[test]
    fn a_web_client_request_joins_the_base_of_the_client_it_is_made_on() {
        let body = "static final WebClient MADE = WebClient.create(\"http://made.example.com/v1\");\n\
             static final WebClient BUILT = WebClient.builder().baseUrl(\"http://built.example.com\").build();\n\
             WebClient injected;\n\
             void f() {\n\
             MADE.get().uri(\"/a/{id}\", 1).retrieve();\n\
             BUILT.post().uri(\"b\").retrieve();\n\
             BUILT.method(HttpMethod.PATCH).uri(\"/c\").retrieve();\n\
             injected.delete().uri(\"/d\").retrieve();\n\
             WebClient.create(\"http://inline.example.com\").put().uri(\"/e\").retrieve();\n\
             MADE.get().uri(\"http://other.example.com/f\").retrieve();\n\
             MADE.get().uri(b -> b.build()).retrieve();\n}";
        let via = |c: &str, v: &str| (Some(c.to_string()), Some(format!("host:{v}")), None);
        assert_eq!(
            sent(body),
            [
                open(Some("GET")),
                to("http::DELETE::/d"),
                via("http::GET::/f", "other.example.com"),
                via("http::GET::/v1/a/{}", "made.example.com"),
                via("http::PATCH::/c", "built.example.com"),
                via("http::POST::/b", "built.example.com"),
                via("http::PUT::/e", "inline.example.com"),
            ]
        );
    }

    #[test]
    fn a_web_client_an_on_demand_import_brings_is_no_client() {
        // ponytail: the pack reads a class an on-demand import brings as an outside name of its
        // own, so neither its type nor its static calls are the library's.
        let wild = "import org.springframework.web.reactive.function.client.*;\n";
        let body = "static final WebClient W = WebClient.create(\"http://w.example.com\");\n\
             void f() { W.get().uri(\"/w\").retrieve(); }";
        assert!(sent_with(wild, body, &[]).is_empty());
    }

    #[test]
    fn a_jdk_request_reads_its_builder_chain_get_by_default() {
        let body = "void f() {\n\
             HttpRequest.newBuilder().uri(URI.create(\"/a\")).build();\n\
             HttpRequest.newBuilder().uri(URI.create(\"/b\")).POST(null).build();\n\
             HttpRequest.newBuilder().PUT(null).uri(URI.create(\"/c\")).header(\"k\", \"v\").build();\n\
             HttpRequest.newBuilder(URI.create(\"/d\")).DELETE().build();\n\
             HttpRequest.newBuilder().method(\"PATCH\", null).uri(URI.create(\"/e\")).build();\n\
             HttpRequest.newBuilder().GET().uri(URI.create(\"/x\")).uri(URI.create(\"/f\")).build();\n}";
        assert_eq!(
            contracts(body),
            [
                "http::DELETE::/d",
                "http::GET::/a",
                "http::GET::/f",
                "http::PATCH::/e",
                "http::POST::/b",
                "http::PUT::/c",
            ]
        );
    }

    #[test]
    fn an_okhttp_request_reads_its_builder_chain_get_by_default() {
        let body = "void f() {\n\
             new Request.Builder().url(\"/a\").build();\n\
             new Request.Builder().url(\"/b\").post(null).build();\n\
             new Request.Builder().put(null).url(\"/c\").build();\n\
             new Request.Builder().url(\"/d\").delete().build();\n\
             new Request.Builder().url(\"/e\").method(\"PATCH\", null).build();\n}";
        assert_eq!(
            contracts(body),
            [
                "http::DELETE::/d",
                "http::GET::/a",
                "http::PATCH::/e",
                "http::POST::/b",
                "http::PUT::/c",
            ]
        );
        // A builder of the repository by the same name builds no request.
        let others = [(
            "a/Request.java",
            "package a; class Request { static class Builder {} }",
        )];
        let imports = IMPORTS.replace("import okhttp3.Request;\n", "");
        assert!(sent_with(&imports, body, &others).is_empty());
    }

    #[test]
    fn a_wrapper_reads_the_method_and_url_its_callers_pass_through_two_levels() {
        let body = "static final String BASE = \"http://api.example.com\";\n\
             static final WebClient WEB = WebClient.create(BASE);\n\
             void send(HttpMethod method, String path) {\n\
               HttpRequest.newBuilder().uri(URI.create(BASE + path)).build();\n\
               WEB.method(method).uri(path).retrieve();\n\
             }\n\
             void outer(HttpMethod m, String p) { send(m, \"/v1\" + p); }\n\
             void calls() { outer(HttpMethod.DELETE, \"/items\"); }";
        let host = Some("host:api.example.com".to_string());
        assert_eq!(
            sent(body),
            [
                (Some("http::DELETE::/v1/items".into()), host.clone(), None),
                (Some("http::GET::/v1/items".into()), host, None),
            ]
        );
        let exchange = format!(
            "{REST}void send(HttpMethod method, String path) {{ rest.exchange(path, method, null, String.class); }}\n\
             void outer(HttpMethod m, String p) {{ send(m, \"/v1\" + p); }}\n\
             void calls() {{ outer(HttpMethod.DELETE, \"/items\"); outer(HttpMethod.GET, \"/all\"); }}"
        );
        assert_eq!(
            contracts(&exchange),
            ["http::DELETE::/v1/items", "http::GET::/v1/all"]
        );
    }

    #[test]
    fn a_wrapper_caller_whose_enum_another_import_binds_sends_no_method() {
        let others = [(
            "a/Caller.java",
            "package a;\nimport org.acme.HttpMethod;\n\
             class Caller { void go(C c) { c.send(HttpMethod.DELETE, \"/x\"); } }",
        )];
        let body = format!(
            "{REST}public void send(HttpMethod method, String path) {{ rest.exchange(path, method, null, String.class); }}\n\
             void here() {{ send(HttpMethod.PUT, \"/y\"); }}"
        );
        assert_eq!(
            sent_with(IMPORTS, &body, &others),
            [open(None), to("http::PUT::/y")]
        );
    }

    #[test]
    fn a_wrapper_in_a_cycle_or_without_callers_is_read_where_it_stands() {
        let body = format!(
            "{REST}void a(String p) {{ rest.getForObject(p, String.class); b(p); }}\n\
             void b(String p) {{ a(p); }}\n\
             void lone(String p) {{ rest.delete(\"/lone/\" + p); }}"
        );
        assert_eq!(sent(&body), [to("http::DELETE::/lone/{}")]);
    }
}
