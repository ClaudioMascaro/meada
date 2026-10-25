//! Java HTTP consumers (spec #26): RestTemplate, WebClient, `java.net.http` and OkHttp. A
//! consumer is a call on a client the source shows is the library's: a RestTemplate method on a
//! name written as a `RestTemplate` (`Code::origins` with `Follow::Declarations`); the `.uri(..)`
//! of a WebClient request (`get()`, `method(HttpMethod.X)`) made on a name written as a
//! `WebClient` or made by `WebClient.create` or `WebClient.builder()`; and the `build()` of a
//! builder chain that starts at `HttpRequest.newBuilder()` or `new Request.Builder()`, each name
//! read through `Code::outside` (an on-demand import's one candidate included).
//!
//! The call is read whole (`Expand::Chain`), so its builder chain and arguments fold where a
//! wrapper's callers pass them, and an enum member (`HttpMethod.GET`) arrives as a
//! `Value::External` spelled by the imports of the file that writes it: a type of the repository
//! or another package's by that name is no Spring method. A WebClient's base is read off the
//! client the folded chain is made on (`WebClient.create(base)`, `builder().baseUrl(base)`,
//! through constants and `final` fields), so a client a wrapper takes as a parameter has the base
//! each caller passes; one not read has an unknown base. The URL folds `+`, constants,
//! `URI.create(..)` and `String.format`; the root's `fetches` reads it. A base a Spring
//! `@Value("${key}")` field or constructor parameter holds comes from the module's
//! configuration (`jvm::spring::inject`, through `Code::expand_with`), and
//! `System.getenv("X")` is the variable `X`.
// ponytail: a client or a builder held in a local is read by its written type alone (a
// WebClient's base unknown, a builder not at all); constants, `final` fields and chains written
// in place are read through.

use super::super::{based, consumer, fetches};
use super::{made, member_of};
use crate::contract::Found;
use crate::index::Indexed;
use crate::jvm::names::{java_files, one, spellings, typed};
use crate::jvm::spring::{self, Configs};
use crate::lang::{Annotated, Call, Invocation, Part, Value};
use crate::resolve::{Code, Expand, Expanded, FLUENT_CHAIN, method as member};

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
    ("HEAD", "HEAD"),
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
    /// A WebClient request; the base is read off the client its folded chain is made on.
    Web,
    /// The builder chain of an `HttpRequest` or an OkHttp `Request`: where it starts (the URL
    /// `newBuilder(uri)` may give) and the methods that set the method.
    Builder {
        start: &'static str,
        setter: &'static str,
        methods: &'static [(&'static str, &'static str)],
    },
}

/// Java HTTP consumers: see the module.
pub(in crate::http) fn consumers<'a>(code: &Code<'a>, indexed: &Indexed) -> Vec<Found> {
    // Read the first time an annotation is injected: a repository without one parses none.
    let configs = std::cell::OnceCell::new();
    let inject = |code: &Code<'a>, path: &'a str, annotated: &'a Annotated| {
        let configs = configs.get_or_init(|| Configs::read(indexed.configs, indexed.manifests));
        spring::inject(configs, code, path, annotated)
    };
    let mut found = Vec::new();
    for (path, facts) in java_files(code) {
        for n in 0..facts.calls.len() {
            let Some(client) = client(code, path, &facts.calls, n) else {
                continue;
            };
            let read = |values: &[Value], _: &[(String, Value)]| {
                let [Value::Call(call)] = values else {
                    return None;
                };
                let (url, method) = sends(&client, call);
                fetches(&[url, method])
            };
            for Expanded {
                site: (site, at),
                found: reads,
                ..
            } in code.expand_with(Expand::Chain(path, n), Some(&inject), &read)
            {
                (reads.into_iter()).for_each(|read| found.push(consumer(site, at, read, None)));
            }
        }
    }
    found
}

/// The client the call at `index` of `path` (among the file's `calls` as the index keeps
/// them) sends on. What the kept facts rule out is ruled out before the call's values are read:
/// `Code::call` parses the file again on a refresh.
fn client<'a>(code: &Code<'a>, path: &'a str, calls: &'a [Call], index: usize) -> Option<Client> {
    let stored = &calls[index];
    let member = member(&stored.callee);
    let rest = REST_METHODS.iter().find(|(m, _)| *m == member);
    if rest.is_some() || member == "exchange" {
        let (receiver, _) = stored.callee.rsplit_once('.')?;
        let typed = typed(code, path, &stored.scope, receiver);
        one(&typed, |t| t == REST_TEMPLATE)?;
        return Some(Client::Rest(rest.map(|(_, m)| *m)));
    }
    match member {
        "uri" => {
            let starts = self::member(&calls.get(stored.on?)?.callee);
            if starts != "method" && !WEB_METHODS.iter().any(|(m, _)| *m == starts) {
                return None;
            }
            let Some(Value::Call(request)) = code.receiver(path, index) else {
                return None;
            };
            let client = request.receiver.as_deref()?;
            web_client(code, path, &stored.scope, client).then_some(Client::Web)
        }
        "build" => {
            // The chain's start as the kept facts tell: `newBuilder`, or a method of a `new`.
            let mut start = stored;
            for _ in 0..FLUENT_CHAIN {
                match start.on.and_then(|on| calls.get(on)) {
                    Some(next) => start = next,
                    None => break,
                }
            }
            if self::member(&start.callee) != "newBuilder"
                && !start.callee.starts_with("(expression).")
            {
                return None;
            }
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
        Client::Web => {
            let Some(Value::Call(request)) = call.receiver.as_deref() else {
                return (Value::Unknown, Value::Unknown);
            };
            let method = match member(&request.callee) {
                "method" => spring_method(request.args.first()),
                m => (WEB_METHODS.iter().find(|(w, _)| *w == m))
                    .map_or(Value::Unknown, |(_, m)| text(m)),
            };
            match request.receiver.as_deref() {
                // A client a wrapper's caller passes: its base is read at each caller.
                Some(Value::Ref(name)) => (Value::Ref(name.clone()), method),
                client => (
                    based(base(client).as_deref(), &url(call.args.first())),
                    method,
                ),
            }
        }
        Client::Builder {
            start,
            setter,
            methods,
        } => {
            // Outermost first: the last call that sets a part is the one that holds.
            let (mut url, mut method) = (None, None);
            let mut reached = false;
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
                    Some(Value::New(..)) => {
                        reached = true;
                        None
                    }
                    _ => {
                        reached = name == *start;
                        None
                    }
                };
            }
            // `GET` when the whole chain was read and names none; a chain folded short of its
            // start (past a depth or a budget) may name one further down.
            let absent = match reached {
                true => text("GET"),
                false => Value::Unknown,
            };
            (url.unwrap_or(Value::Unknown), method.unwrap_or(absent))
        }
    }
}

/// The base URL of a folded WebClient: what `WebClient.create(base)` gives, or the outermost
/// `baseUrl(base)` of a `WebClient.builder()` chain ending in `build()`; `None` without one,
/// unknown for a client made any other way or not read.
fn base(client: Option<&Value>) -> Option<Vec<Part>> {
    let hole = Some(vec![Part::Hole]);
    let Some(Value::Call(top)) = client else {
        return hole;
    };
    let mut steps = vec![top];
    while let Some(Value::Call(next)) = steps[steps.len() - 1].receiver.as_deref() {
        steps.push(next);
    }
    let start = steps[steps.len() - 1];
    let made = matches!(start.receiver.as_deref(),
        Some(Value::External(s)) if one(s, |s| s == WEB_CLIENT).is_some());
    let given = |call: &Invocation| match call.args.first() {
        None | Some(Value::Undefined) => None,
        Some(Value::Str(parts)) => Some(parts.clone()),
        // A wrapper's parameter: read at each caller.
        Some(Value::Ref(name)) => Some(vec![Part::Ref(name.clone())]),
        Some(_) => Some(vec![Part::Hole]),
    };
    match member(&start.callee) {
        "create" if made && steps.len() == 1 => given(start),
        "builder" if made && member(&top.callee) == "build" => {
            let set = steps.iter().find(|c| member(&c.callee) == "baseUrl");
            set.and_then(|c| given(c))
        }
        _ => hole,
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
    if of("java.lang.String") && member(&call.callee) == "format" {
        return format(&call.args);
    }
    Value::Unknown
}

/// `String.format` with these arguments, the format first: each `%s` or `%d` the next
/// argument (a string's parts, a parameter as it is, anything else a hole), `%%` a `%`.
/// Unknown for any other conversion, and for a format that is no string.
fn format(args: &[Value]) -> Value {
    let parts = match args.first() {
        Some(Value::Str(parts)) => parts,
        Some(Value::Ref(name)) => return Value::Ref(name.clone()),
        _ => return Value::Unknown,
    };
    let mut given = args.iter().skip(1);
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
                    match given.next() {
                        Some(Value::Str(parts)) => out.extend(parts.iter().cloned()),
                        Some(Value::Ref(name)) => out.push(Part::Ref(name.clone())),
                        _ => out.push(Part::Hole),
                    }
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
        member_of(s, HTTP_METHOD)
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

/// Whether `client` (a name, or a call made in place) is a WebClient: written as one, or made
/// by a `WebClient.create` or `WebClient.builder()` it traces back to.
fn web_client<'a>(code: &Code<'a>, path: &'a str, scope: &'a str, client: &'a Value) -> bool {
    made(code, (path, scope), client, WEB_CLIENT, makes)
}

/// Whether the call at `index` of `path` makes a WebClient: `WebClient.create(..)`, or a
/// `WebClient.builder()` chain ending in `build()`.
fn makes<'a>(code: &Code<'a>, path: &'a str, index: usize) -> bool {
    let Some(chain) = code.chain(path, index) else {
        return false;
    };
    let calls: Option<Vec<_>> = chain.iter().map(|&i| code.call(path, i)).collect();
    let Some((top, start)) = calls.as_ref().and_then(|c| Some((c.first()?, c.last()?))) else {
        return false;
    };
    let names = spellings(code, path, &start.scope, &start.callee);
    let is = |what: &str| one(&names, |s| s == format!("{WEB_CLIENT}.{what}")).is_some();
    (is("create") && chain.len() == 1) || (is("builder") && member(&top.callee) == "build")
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
        sent_in("a/C.java", imports, body, others, &[])
    }

    /// `sent_with`, the class at `path`, with the config files `configs` (path and content).
    fn sent_in(
        path: &str,
        imports: &str,
        body: &str,
        others: &[(&str, &str)],
        configs: &[(&str, &str)],
    ) -> Vec<Sent> {
        let src = format!("package a;\n{imports}\nclass C {{\n{body}\n}}\n");
        let mut parsed = vec![(path, Java.parse(path, src.as_bytes()).unwrap())];
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
        };
        let mut found: Vec<Sent> = (consumers(&code, &indexed).into_iter())
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
        // Another on-demand import whose package is not Spring's leaves Spring's the one candidate.
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
             rest.getForObject(String.format(\"%s/f/%d/x%%\", USERS, id), String.class);\n\
             rest.getForObject(String.format(\"/g/%s\", 7), String.class);\n\
             rest.getForObject(URI.create(USERS + \"/me\"), String.class);\n\
             rest.getForObject(\"https://Users.example.com/u\", String.class);\n}}"
        );
        assert_eq!(
            sent_with(&imports, &body, &others),
            [
                to("http::GET::/api/users/f/{}/x%"),
                to("http::GET::/api/users/me"),
                to("http::GET::/api/users/{}"),
                to("http::GET::/g/{}"),
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
    fn a_client_an_on_demand_import_brings_is_the_librarys_unless_another_may_be() {
        let wild = "import org.springframework.web.reactive.function.client.*;\n\
             import org.springframework.web.client.*;\nimport java.util.*;\n";
        let body = "static final WebClient W = WebClient.create(\"http://w.example.com\");\n\
             RestTemplate rest;\n\
             void f() { W.get().uri(\"/w\").retrieve(); rest.delete(\"/r\"); }";
        let host = Some("host:w.example.com".to_string());
        assert_eq!(
            sent_with(wild, body, &[]),
            [
                to("http::DELETE::/r"),
                (Some("http::GET::/w".into()), host.clone(), None)
            ]
        );
        // A wildcard of the repository that brings its own `RestTemplate` may be the one.
        let others = [(
            "b/RestTemplate.java",
            "package b; public class RestTemplate {}",
        )];
        let both = format!("{wild}import b.*;\n");
        assert_eq!(
            sent_with(&both, body, &others),
            [(Some("http::GET::/w".into()), host, None)]
        );
    }

    #[test]
    fn a_web_client_parameter_takes_the_base_each_caller_passes() {
        let body = "static final WebClient ORDERS = WebClient.create(\"http://orders.example.com\");\n\
             WebClient other;\n\
             void call(WebClient c, String p) { c.get().uri(p).retrieve(); }\n\
             void a() { call(ORDERS, \"/x\"); call(other, \"/y\"); }";
        assert_eq!(
            sent(body),
            [
                (
                    Some("http::GET::/x".into()),
                    Some("host:orders.example.com".into()),
                    None
                ),
                to("http::GET::/y"),
            ]
        );
    }

    #[test]
    fn a_final_web_client_field_takes_the_base_of_its_one_value() {
        // An initializer, one constructor assignment, two of them, and a constructor parameter.
        let body = "private final WebClient f = WebClient.create(\"http://f.example.com\");\n\
             private final WebClient g;\n\
             private final WebClient h;\n\
             private final WebClient p;\n\
             C(String url) {\n\
               this.g = WebClient.builder().baseUrl(\"http://g.example.com\").build();\n\
               h = WebClient.create(\"http://h1.example.com\");\n\
               this.p = WebClient.create(url);\n\
             }\n\
             C(int n) { this.h = WebClient.create(\"http://h2.example.com\"); }\n\
             void run() {\n\
               f.get().uri(\"/f\").retrieve();\n\
               this.g.get().uri(\"/g\").retrieve();\n\
               h.get().uri(\"/h\").retrieve();\n\
               p.get().uri(\"/p\").retrieve();\n\
             }";
        let via = |c: &str, v: &str| (Some(c.to_string()), Some(format!("host:{v}")), None);
        assert_eq!(
            sent(body),
            [
                via("http::GET::/f", "f.example.com"),
                via("http::GET::/g", "g.example.com"),
                to("http::GET::/h"),
                to("http::GET::/p"),
            ]
        );
    }

    #[test]
    fn a_parameter_that_hides_a_rest_template_field_sends_nothing() {
        let body = format!("{REST}void f(Object rest) {{ rest.delete(\"/x\"); }}");
        assert!(sent(&body).is_empty());
    }

    #[test]
    fn a_jdk_request_reads_its_builder_chain_get_by_default() {
        let body = "void f() {\n\
             HttpRequest.newBuilder().uri(URI.create(\"/a\")).build();\n\
             HttpRequest.newBuilder().uri(URI.create(\"/b\")).POST(null).build();\n\
             HttpRequest.newBuilder().PUT(null).uri(URI.create(\"/c\")).header(\"k\", \"v\").build();\n\
             HttpRequest.newBuilder(URI.create(\"/d\")).DELETE().build();\n\
             HttpRequest.newBuilder().method(\"PATCH\", null).uri(URI.create(\"/e\")).build();\n\
             HttpRequest.newBuilder().GET().uri(URI.create(\"/x\")).uri(URI.create(\"/f\")).build();\n\
             HttpRequest.newBuilder(URI.create(\"/g\")).HEAD().build();\n}";
        assert_eq!(
            contracts(body),
            [
                "http::DELETE::/d",
                "http::GET::/a",
                "http::GET::/f",
                "http::HEAD::/g",
                "http::PATCH::/e",
                "http::POST::/b",
                "http::PUT::/c",
            ]
        );
    }

    #[test]
    fn a_chain_folded_short_of_its_start_names_no_method() {
        // Past the pack's value depth the chain's start is unknown: no `GET` by default.
        let headers = ".header(\"k\", \"v\")".repeat(80);
        let body = format!(
            "void f() {{ HttpRequest.newBuilder(){headers}.uri(URI.create(\"/a\")).build(); }}"
        );
        assert_eq!(sent(&body), [open(None)]);
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

    const SPRING: &str = "import org.springframework.beans.factory.annotation.Value;\n";
    const MODULE: &str = "api/src/main/java/a/C.java";
    const CONFIG: &str = "api/src/main/resources/application.properties";

    /// The consumers of `body` in the module `api/`, whose `application.properties` is `config`.
    fn injected(body: &str, config: &str, others: &[(&str, &str)]) -> Vec<Sent> {
        let imports = format!("{IMPORTS}{SPRING}");
        sent_in(MODULE, &imports, body, others, &[(CONFIG, config)])
    }

    fn via(contract: &str, via: &str) -> Sent {
        (Some(contract.into()), Some(via.into()), None)
    }

    const CONFIGURED: &str = "lit.url=http://lit.example.com\n\
        env.url=${API_URL}/v1\n\
        served.url=${AUDIT_URL:https://web.example.com/api}\n";

    #[test]
    fn a_value_field_reads_its_key_from_the_configuration_of_its_module() {
        let body = format!(
            "{REST}@Value(\"${{lit.url}}\") private String lit;\n\
             @Value(\"${{env.url}}\") private String env;\n\
             @Value(\"${{served.url}}\") private String served;\n\
             @Value(\"${{RAW_URL}}\") private String raw;\n\
             @Value(\"http://inline.example.com\") private String inline;\n\
             void f() {{\n\
             rest.getForObject(lit + \"/a\", String.class);\n\
             rest.getForObject(this.env + \"/b\", String.class);\n\
             rest.getForObject(served + \"/c\", String.class);\n\
             rest.getForObject(raw + \"/d\", String.class);\n\
             rest.getForObject(inline + \"/e\", String.class);\n}}"
        );
        assert_eq!(
            injected(&body, CONFIGURED, &[]),
            [
                via("http::GET::/a", "host:lit.example.com"),
                via("http::GET::/api/c", "host:web.example.com|default"),
                via("http::GET::/c", "env:AUDIT_URL|host:web.example.com"),
                via("http::GET::/d", "env:RAW_URL"),
                via("http::GET::/e", "host:inline.example.com"),
                via("http::GET::/v1/b", "env:API_URL"),
            ]
        );
    }

    #[test]
    fn a_value_field_another_binding_hides_or_a_file_assigns_reads_nothing() {
        let body = format!(
            "{REST}@Value(\"${{lit.url}}\") private String lit;\n\
             @Value(\"${{lit.url}}\") private String moved;\n\
             @Value(\"${{lit.url}}\") private String named;\n\
             @Value(\"${{lit.url}}\") private static String shared;\n\
             String named() {{ return null; }}\n\
             void reset() {{ moved = \"http://other.example.com\"; }}\n\
             void param(String lit) {{ rest.getForObject(lit + \"/p\", String.class); }}\n\
             void local() {{ String lit = x(); rest.getForObject(lit + \"/l\", String.class); }}\n\
             void f() {{\n\
             rest.getForObject(moved + \"/m\", String.class);\n\
             rest.getForObject(named + \"/n\", String.class);\n\
             rest.getForObject(shared + \"/s\", String.class);\n}}"
        );
        // Spring injects no `static` field.
        assert_eq!(
            injected(&body, CONFIGURED, &[]),
            [
                to("http::GET::/l"),
                to("http::GET::/m"),
                via("http::GET::/n", "host:lit.example.com"),
                to("http::GET::/p"),
                to("http::GET::/s"),
            ]
        );
        // Another `Value` annotation, and a module without the key's configuration.
        let other = format!(
            "{REST}@Value(\"${{lit.url}}\") String lit;\nvoid f() {{ rest.getForObject(lit + \"/o\", String.class); }}"
        );
        let imports = format!("{IMPORTS}import org.acme.Value;\n");
        let configs = [(CONFIG, CONFIGURED)];
        assert_eq!(
            sent_in(MODULE, &imports, &other, &[], &configs),
            [to("http::GET::/o")]
        );
        let elsewhere = "web/src/main/java/a/C.java";
        let spring = format!("{IMPORTS}{SPRING}");
        assert_eq!(
            sent_in(elsewhere, &spring, &other, &[], &configs),
            [via("http::GET::/o", "env:lit.url")]
        );
    }

    #[test]
    fn a_value_constructor_parameter_stands_in_only_where_no_caller_passes_one() {
        let client = |name: &str| {
            format!(
                "package a;\n{IMPORTS}{SPRING}\nclass {name} {{\n\
                 private final WebClient web;\n\
                 private final String base;\n\
                 private final RestTemplate rest = new RestTemplate();\n\
                 {name}(@Value(\"${{lit.url}}\") String url) {{\n\
                   this.web = WebClient.create(url);\n\
                   this.base = url;\n\
                 }}\n\
                 void f() {{ web.get().uri(\"/w\").retrieve(); }}\n\
                 void g() {{ rest.getForObject(base + \"/r\", String.class); }}\n}}\n"
            )
        };
        let (injected_by_spring, called) = (client("Injected"), client("Called"));
        let others = [
            (
                "api/src/main/java/a/Injected.java",
                injected_by_spring.as_str(),
            ),
            ("api/src/main/java/a/Called.java", called.as_str()),
        ];
        // `C` calls `Called`, with a URL and with one it cannot read: Spring's value stands in
        // for neither, and the fields do not follow the callers. Nothing calls `Injected`.
        let body = "void make(String any) {\n\
             new Called(\"http://given.example.com\");\n\
             new Called(any);\n}";
        let found = injected(body, CONFIGURED, &others);
        // A consumer in a wrapper that callers climb still reads the injected base.
        let wrapped = format!(
            "{REST}private final WebClient web;\n\
             C(@Value(\"${{lit.url}}\") String url) {{ this.web = WebClient.create(url); }}\n\
             void send(String path) {{ web.get().uri(path).retrieve(); }}\n\
             void go() {{ send(\"/x\"); }}"
        );
        assert_eq!(
            injected(&wrapped, CONFIGURED, &[]),
            [via("http::GET::/x", "host:lit.example.com")]
        );
        assert_eq!(
            found,
            [
                to("http::GET::/r"),
                via("http::GET::/r", "host:lit.example.com"),
                to("http::GET::/w"),
                via("http::GET::/w", "host:lit.example.com"),
            ]
        );
    }

    #[test]
    fn an_environment_read_names_the_variable_unless_system_is_another_type() {
        let body = format!(
            "{REST}void f() {{\n\
             rest.getForObject(System.getenv(\"WEB_URL\") + \"/a\", String.class);\n\
             rest.getForObject(java.lang.System.getenv(\"WEB_URL\") + \"/b\", String.class);\n\
             rest.getForObject(System.getenv(name()) + \"/c\", String.class);\n}}"
        );
        // A read Meada cannot name is an unknown base.
        assert_eq!(
            sent(&body),
            [
                via("http::GET::/a", "env:WEB_URL"),
                via("http::GET::/b", "env:WEB_URL"),
                to("http::GET::/c"),
            ]
        );
        let hidden = [
            to("http::GET::/a"),
            via("http::GET::/b", "env:WEB_URL"),
            to("http::GET::/c"),
        ];
        let imports = format!("{IMPORTS}import org.acme.System;\n");
        assert_eq!(sent_with(&imports, &body, &[]), hidden);
        assert_eq!(sent(&format!("{body}\nstatic class System {{}}")), hidden);
        // Importing `java.lang.System` itself hides nothing.
        let imports = format!("{IMPORTS}import java.lang.System;\n");
        assert_eq!(sent_with(&imports, &body, &[]), sent(&body));
    }

    #[test]
    fn a_constructor_any_call_may_reach_takes_no_injected_value() {
        let client = |name: &str, more: &str| {
            format!(
                "package a;\n{IMPORTS}{SPRING}\nclass {name} {{\n\
                 private final WebClient web;\n\
                 {name}(@Value(\"${{lit.url}}\") String url) {{ this.web = WebClient.create(url); }}\n\
                 {more}\n\
                 void f() {{ web.get().uri(\"/{name}\").retrieve(); }}\n}}\n"
            )
        };
        // `this(..)`, `super(..)` and a `new` inside an anonymous class pass their own URL.
        let delegated = client(
            "Delegated",
            "Delegated() { this(\"http://localhost:8080\"); }",
        );
        let base = client("Base", "");
        let anonymous = client("Anonymous", "");
        let others = [
            ("api/src/main/java/a/Delegated.java", delegated.as_str()),
            ("api/src/main/java/a/Base.java", base.as_str()),
            (
                "api/src/main/java/a/Sub.java",
                "package a;\nclass Sub extends Base { Sub() { super(\"http://other.example.com\"); } }\n",
            ),
            ("api/src/main/java/a/Anonymous.java", anonymous.as_str()),
            (
                "api/src/main/java/a/Maker.java",
                "package a;\nclass Maker { Object m() { return new Object() {\n\
                 Object o = new Anonymous(\"http://x.example.com\"); }; } }\n",
            ),
        ];
        assert_eq!(
            injected("", CONFIGURED, &others),
            [
                to("http::GET::/Anonymous"),
                to("http::GET::/Base"),
                to("http::GET::/Delegated"),
            ]
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
