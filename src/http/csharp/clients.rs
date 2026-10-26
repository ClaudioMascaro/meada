//! C# HTTP consumers (spec #28, #111): the `HttpClient` methods that send a request. A call
//! counts only when its receiver is an `HttpClient` by its declared type or by the construction
//! it traces back to, the type read through the usings (`names::typed`, `Code::outside`), never
//! by text; a client `IHttpClientFactory.CreateClient(name)` made is a named client, which is
//! not read.
//!
//! The call is read whole (`Expand::Chain`), so a wrapper (a method that takes the path, the
//! method or the client as a parameter) turns each of its call sites into a consumer with its
//! arguments substituted, and a `HttpMethod.X` arrives as a `Value::External` spelled by the
//! imports of the file that writes it. The URL folds string literals, interpolated and verbatim
//! strings, `+`, `const` and `static readonly` fields (the pack's `Facts::consts`) and
//! `new Uri(..)`. A client's base is the `BaseAddress` its construction sets in an object
//! initializer (`new HttpClient { BaseAddress = new Uri(base) }`, in a local, a field or a
//! `static readonly` field), or the one `BaseAddress = new Uri(base)` assignment made on the
//! name the call is made on (a local, or a field assigned once in its type): a base not read is
//! unknown, so the path alone is the contract.
// ponytail: a client held in a field some method assigns more than once, or passed to a wrapper
// that was built elsewhere, has an unknown base; `AddHttpClient<T>` is #113's.

use super::super::{based, consumer, fetches};
use crate::contract::Found;
use crate::lang::{Invocation, Part, Value};
use crate::names::{one, spellings};
use crate::resolve::{Code, Expand, Expanded, method as member};

const HTTP_CLIENT: &str = "System.Net.Http.HttpClient";
const REQUEST: &str = "System.Net.Http.HttpRequestMessage";
const HTTP_METHOD: &str = "System.Net.Http.HttpMethod.";
/// The methods of an `HttpClient` that send one method.
const CLIENT_METHODS: &[(&str, &str)] = &[
    ("GetAsync", "GET"),
    ("PostAsync", "POST"),
    ("PutAsync", "PUT"),
    ("PatchAsync", "PATCH"),
    ("DeleteAsync", "DELETE"),
    ("GetStringAsync", "GET"),
    ("GetStreamAsync", "GET"),
    ("GetByteArrayAsync", "GET"),
    ("GetFromJsonAsync", "GET"),
    ("PostAsJsonAsync", "POST"),
    ("PutAsJsonAsync", "PUT"),
];
/// The members of `HttpMethod` that name one method.
const MEMBERS: &[(&str, &str)] = &[
    ("Get", "GET"),
    ("Post", "POST"),
    ("Put", "PUT"),
    ("Patch", "PATCH"),
    ("Delete", "DELETE"),
    ("Head", "HEAD"),
    ("Options", "OPTIONS"),
];
/// The member `BaseAddress` of an `HttpClient`.
const BASE: &str = "BaseAddress";

/// C# HTTP consumers: see the module.
pub(in crate::http) fn consumers<'a>(code: &Code<'a>) -> Vec<Found> {
    let mut found = Vec::new();
    for (&path, facts) in code.files().iter().filter(|(p, _)| p.ends_with(".cs")) {
        for (n, stored) in facts.calls.iter().enumerate() {
            let Some(sent) = sent(stored.callee.as_str()) else {
                continue;
            };
            let Some((receiver, _)) = stored.callee.rsplit_once('.') else {
                continue;
            };
            if stored.decorates.is_some() || !is_client(code, path, n, receiver) {
                continue;
            }
            let read = |values: &[Value], _: &[(String, Value)]| {
                let [Value::Call(call)] = values else {
                    return None;
                };
                let (url, method) = sends(code, (path, &stored.scope), receiver, sent, call);
                fetches(&[url, method])
            };
            for Expanded {
                site: (site, at),
                found: reads,
                ..
            } in code.expand(Expand::Chain(path, n), &read)
            {
                (reads.into_iter()).for_each(|read| found.push(consumer(site, at, read, None)));
            }
        }
    }
    found
}

/// What a call of the method `callee` ends in sends.
#[derive(Clone, Copy)]
enum Sent {
    /// The method of this name sends this HTTP method.
    Verb(&'static str),
    /// `SendAsync(new HttpRequestMessage(method, url))`.
    Request,
}

fn sent(callee: &str) -> Option<Sent> {
    let name = member(callee);
    match CLIENT_METHODS.iter().find(|(m, _)| *m == name) {
        Some((_, verb)) => Some(Sent::Verb(verb)),
        None if name == "SendAsync" => Some(Sent::Request),
        None => None,
    }
}

/// Whether the call at `index` of `path`, made on `receiver`, is made on an `HttpClient`: the
/// receiver is written as one (a field, a property, a parameter, a typed local), or was made by
/// `new HttpClient(..)`; one `CreateClient` made is a named client.
fn is_client<'a>(code: &Code<'a>, path: &'a str, index: usize, receiver: &'a str) -> bool {
    let scope = code.files()[path].calls[index].scope.as_str();
    let named = code
        .local(path, scope, receiver)
        .is_some_and(|b| matches!(&b.value, Value::Call(c) if member(&c.callee) == "CreateClient"));
    if named {
        return false;
    }
    if let Some((at, within, ty)) = code.written(path, scope, receiver) {
        return one(&spellings(code, at, within, ty), |t| t == HTTP_CLIENT).is_some();
    }
    // No type written: the construction in place, or a local made by `new HttpClient()`.
    let made = |ty: &'a str| one(&spellings(code, path, scope, ty), |t| t == HTTP_CLIENT).is_some();
    match code.receiver(path, index) {
        Some(Value::New(ty, _)) => made(ty),
        _ => false,
    }
}

/// The URL and the method of `call`, read whole by `Expand::Chain`, made on `receiver` (as the
/// stored call writes it) at `at`.
fn sends<'a>(
    code: &Code<'a>,
    (path, scope): (&'a str, &'a str),
    receiver: &str,
    sent: Sent,
    call: &Invocation,
) -> (Value, Value) {
    let text = |m: &str| Value::Str(vec![Part::Text(m.to_string())]);
    let (url, method) = match sent {
        Sent::Verb(verb) => (call.args.first(), text(verb)),
        Sent::Request => match call.args.first() {
            Some(Value::New(ty, args)) if is_request(code, (path, scope), ty) => {
                (args.get(1), http_method(args.first()))
            }
            // A request a wrapper takes: read at each caller.
            Some(Value::Ref(name)) => return (Value::Ref(name.clone()), Value::Ref(name.clone())),
            _ => (None, Value::Unknown),
        },
    };
    let url = self::url(code, (path, scope), url);
    match call.receiver.as_deref() {
        // A client a wrapper takes: its base is read at each caller.
        Some(Value::Ref(name)) => (Value::Ref(name.clone()), method),
        client => {
            // The assignment on the name the call is made on wins over the initializer.
            let name = receiver.strip_prefix("this.").unwrap_or(receiver);
            let assigned = (code.local(path, scope, &format!("{name}.{BASE}")))
                .map(|b| code.value(path, &b.within, &b.value));
            (based_on(base(client, assigned.as_ref()), url), method)
        }
    }
}

/// The method a folded `HttpMethod.X` gives: the member as text, a parameter as it is (the call
/// is a wrapper's), anything else unknown.
fn http_method(value: Option<&Value>) -> Value {
    let member = |s: &str| {
        let name = s.strip_prefix(HTTP_METHOD)?;
        MEMBERS.iter().find(|(m, _)| *m == name).map(|(_, v)| *v)
    };
    match value {
        Some(Value::External(spellings)) => match one(spellings, |s| member(s).is_some()) {
            Some(s) => Value::Str(vec![Part::Text(member(s).unwrap_or_default().into())]),
            None => Value::Unknown,
        },
        Some(Value::Ref(name)) => Value::Ref(name.clone()),
        _ => Value::Unknown,
    }
}

/// The URL a folded argument gives: through `new Uri(..)` of one argument, which the
/// repository does not declare.
fn url<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), value: Option<&Value>) -> Value {
    match value {
        Some(Value::New(ty, args)) if is_uri(code, (path, scope), ty) && args.len() == 1 => {
            args[0].clone()
        }
        Some(value) => value.clone(),
        None => Value::Unknown,
    }
}

/// Whether `ty`, written in `scope`, is `System.Uri`: no type of the repository hides it.
fn is_uri<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), ty: &str) -> bool {
    ty == "System.Uri" || (ty == "Uri" && code.definition(path, scope, "Uri").is_none())
}

/// Whether `ty`, written in `scope`, is `HttpRequestMessage`, through the usings of the call.
fn is_request<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), ty: &str) -> bool {
    ty == REQUEST
        || (ty == "HttpRequestMessage"
            && one(&spellings(code, path, scope, "HttpRequestMessage"), |s| {
                s == REQUEST
            })
            .is_some())
}

/// The base address of a client folded as `client` (a construction with an initializer) or
/// assigned (`BaseAddress = new Uri(base)`); a hole when neither is read.
fn base(client: Option<&Value>, assigned: Option<&Value>) -> Vec<Part> {
    let initial = match client {
        Some(Value::New(_, args)) => match args.last() {
            Some(Value::Object(set)) => set.iter().rev().find(|(k, _)| k == BASE).map(|(_, v)| v),
            _ => None,
        },
        _ => None,
    };
    match assigned.or(initial) {
        Some(Value::New(_, args)) if args.len() == 1 => match &args[0] {
            Value::Str(parts) => parts.clone(),
            Value::Ref(name) => vec![Part::Ref(name.clone())],
            _ => vec![Part::Hole],
        },
        _ => vec![Part::Hole],
    }
}

/// `url` with `base` in front when `url` is relative to it. A path from the root
/// (`/orders`) replaces the base's own path, as `Uri` does.
fn based_on(mut base: Vec<Part>, url: Value) -> Value {
    let rooted = matches!(&url, Value::Str(p) if matches!(p.first(), Some(Part::Text(t)) if t.starts_with('/')));
    if rooted {
        origin(&mut base);
    }
    based(Some(&base), &url)
}

/// `base` cut at the path after its host.
fn origin(base: &mut Vec<Part>) {
    let (mut scheme, mut cut) = (false, None);
    for (n, part) in base.iter_mut().enumerate() {
        let Part::Text(t) = part else { continue };
        let from = match scheme {
            true => 0,
            false => match t.find("://") {
                Some(at) => at + 3,
                None => continue,
            },
        };
        scheme = true;
        if let Some(at) = t[from..].find('/') {
            t.truncate(from + at);
            cut = Some(n + 1);
            break;
        }
    }
    if let Some(n) = cut {
        base.truncate(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;
    use crate::lang::csharp::CSharp;
    use crate::resolve::Resolver;
    use std::collections::BTreeMap;

    /// A consumer as `(contract, via, method)`, the method only for an unresolved one.
    type Sent = (Option<String>, Option<String>, Option<String>);

    const USINGS: &str = "using System;\nusing System.Net.Http;\nusing System.Net.Http.Json;\n";

    /// The consumers of the class body `body` in `A/C.cs`, with other files of the repository.
    fn sent_with(body: &str, others: &[(&str, &str)]) -> Vec<Sent> {
        let src = format!("{USINGS}\nnamespace A;\npublic class C {{\n{body}\n}}\n");
        let mut parsed = vec![("A/C.cs", CSharp.parse("A/C.cs", src.as_bytes()).unwrap())];
        for (path, source) in others {
            parsed.push((path, CSharp.parse(path, source.as_bytes()).unwrap()));
        }
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &CSharp);
        let code = Code::new(&resolver, &files, &CSharp);
        let mut found: Vec<Sent> = (consumers(&code).into_iter())
            .map(|f| (f.contract, f.via, f.method))
            .collect();
        found.sort();
        found
    }

    fn sent(body: &str) -> Vec<Sent> {
        sent_with(body, &[])
    }

    /// The contracts of the consumers of `body`.
    fn contracts(body: &str) -> Vec<String> {
        sent(body).into_iter().filter_map(|(c, ..)| c).collect()
    }

    fn to(contract: &str) -> Sent {
        (Some(contract.into()), None, None)
    }

    fn host(contract: &str, host: &str) -> Sent {
        (Some(contract.into()), Some(format!("host:{host}")), None)
    }

    /// An unresolved consumer of `method`.
    fn open(method: &str) -> Sent {
        (None, None, Some(method.into()))
    }

    const CLIENT: &str = "private readonly HttpClient _http;\n";

    #[test]
    fn each_http_client_method_sends_its_method() {
        let body = format!(
            "{CLIENT}async void F() {{\n\
             await _http.GetAsync(\"/a\");\n\
             await _http.PostAsync(\"/b\", null);\n\
             await _http.PutAsync(\"/c\", null);\n\
             await _http.PatchAsync(\"/d\", null);\n\
             await _http.DeleteAsync(\"/e\");\n\
             await _http.GetStringAsync(\"/f\");\n\
             await _http.GetStreamAsync(\"/g\");\n\
             await _http.GetByteArrayAsync(\"/h\");\n\
             await _http.GetFromJsonAsync<int>(\"/i\");\n\
             await _http.PostAsJsonAsync(\"/j\", 1);\n\
             await _http.PutAsJsonAsync(\"/k\", 1);\n\
             await this._http.GetAsync(\"/l\");\n\
             }}"
        );
        assert_eq!(
            contracts(&body),
            [
                "http::DELETE::/e",
                "http::GET::/a",
                "http::GET::/f",
                "http::GET::/g",
                "http::GET::/h",
                "http::GET::/i",
                "http::GET::/l",
                "http::PATCH::/d",
                "http::POST::/b",
                "http::POST::/j",
                "http::PUT::/c",
                "http::PUT::/k",
            ]
        );
    }

    #[test]
    fn send_async_names_its_method_and_url_in_the_request_message() {
        let body = format!(
            "{CLIENT}async void F() {{\n\
             await _http.SendAsync(new HttpRequestMessage(HttpMethod.Post, \"/a\"));\n\
             await _http.SendAsync(new HttpRequestMessage(HttpMethod.Delete, new Uri(\"https://h.example.com/b\")));\n\
             var req = new HttpRequestMessage(HttpMethod.Put, \"/c\");\n\
             await _http.SendAsync(req);\n\
             await _http.SendAsync(Build());\n\
             }}"
        );
        assert_eq!(
            sent(&body),
            [
                (None, None, None),
                host("http::DELETE::/b", "h.example.com"),
                to("http::POST::/a"),
                to("http::PUT::/c"),
            ]
        );
    }

    #[test]
    fn a_base_address_joins_in_front_of_a_relative_url() {
        let body = "static readonly HttpClient Shared = new HttpClient { BaseAddress = new Uri(\"https://s.example.com/api/\") };\n\
             private readonly HttpClient _field = new HttpClient { BaseAddress = new Uri(\"https://f.example.com\") };\n\
             private readonly HttpClient _set;\n\
             C() { _set = new HttpClient(); _set.BaseAddress = new Uri(\"https://c.example.com/v1\"); }\n\
             async void F() {\n\
             await Shared.GetAsync(\"orders\");\n\
             await Shared.GetAsync(\"/root\");\n\
             await _field.GetAsync(\"/orders\");\n\
             await _set.GetAsync(\"orders/1\");\n\
             var local = new HttpClient { BaseAddress = new Uri(\"https://l.example.com\") };\n\
             await local.GetAsync(\"/l\");\n\
             HttpClient typed = new() { BaseAddress = new Uri(\"https://t.example.com\") };\n\
             await typed.GetAsync(\"/t\");\n\
             var assigned = new HttpClient();\n\
             assigned.BaseAddress = new Uri(\"https://a.example.com\");\n\
             await assigned.GetAsync(\"/a\");\n\
             await new HttpClient().GetAsync(\"https://x.example.com/n\");\n\
             }";
        assert_eq!(
            sent(body),
            [
                host("http::GET::/a", "a.example.com"),
                host("http::GET::/api/orders", "s.example.com"),
                host("http::GET::/l", "l.example.com"),
                host("http::GET::/n", "x.example.com"),
                host("http::GET::/orders", "f.example.com"),
                host("http::GET::/root", "s.example.com"),
                host("http::GET::/t", "t.example.com"),
                host("http::GET::/v1/orders/1", "c.example.com"),
            ]
        );
    }

    #[test]
    fn urls_fold_interpolation_verbatim_raw_strings_plus_and_constants() {
        let other = "namespace A.Config;\npublic static class Hosts { public const string Orders = \"https://o.example.com\"; public static readonly string Api = Orders + \"/api\"; }\n";
        let body = format!(
            "{CLIENT}const string Base = \"https://b.example.com\";\n\
             static readonly string Path = Base + \"/p/{{x}}\";\n\
             async void F(int id) {{\n\
             await _http.GetAsync($\"{{Base}}/orders/{{id}}\");\n\
             await _http.GetAsync(Path);\n\
             await _http.GetAsync(A.Config.Hosts.Api + \"/users\");\n\
             await _http.GetAsync(@\"https://v.example.com/a\\b\");\n\
             await _http.GetAsync($@\"https://v.example.com/{{id}}/\"\"q\"\"\");\n\
             await _http.GetAsync(\"\"\"https://r.example.com/raw\"\"\");\n\
             await _http.GetAsync($\"https://i.example.com/{{{{lit}}}}/{{id:D2}}\");\n\
             await _http.GetAsync($$\"\"\"https://r.example.com/{{id}}/{{y}}\"\"\");\n\
             }}"
        );
        assert_eq!(
            sent_with(&body, &[("A/Config/Hosts.cs", other)]),
            [
                host("http::GET::/a\\b", "v.example.com"),
                host("http::GET::/api/users", "o.example.com"),
                host("http::GET::/orders/{}", "b.example.com"),
                host("http::GET::/p/{}", "b.example.com"),
                host("http::GET::/raw", "r.example.com"),
                host("http::GET::/{}/\"q\"", "v.example.com"),
                host("http::GET::/{}/{}", "i.example.com"),
                host("http::GET::/{}/{}", "r.example.com"),
            ]
        );
    }

    #[test]
    fn a_call_whose_url_cannot_be_evaluated_is_an_unresolved_consumer_of_its_method() {
        let body = format!(
            "{CLIENT}string Build() => \"\";\n\
             async void F() {{\n\
             await _http.GetAsync(Build());\n\
             await _http.PostAsJsonAsync(Build(), 1);\n\
             }}"
        );
        assert_eq!(sent(&body), [open("GET"), open("POST")]);
    }

    #[test]
    fn a_wrapper_makes_each_call_site_a_consumer_through_any_level() {
        let body = format!(
            "{CLIENT}Task<string> Inner(string p) => _http.GetStringAsync($\"https://w.example.com/{{p}}\");\n\
             Task<string> Outer(string id) => Inner(\"orders/\" + id);\n\
             Task<string> Twice(string id) => Outer(id + \"/lines\");\n\
             void Use(string unknown) {{ Outer(\"1\"); Twice(unknown); Twice(\"2\"); }}\n"
        );
        assert_eq!(
            contracts(&body),
            [
                "http::GET::/orders/1",
                "http::GET::/orders/2/lines",
                "http::GET::/orders/{}/lines",
            ]
        );
    }

    #[test]
    fn a_field_set_in_the_constructor_is_still_the_client_of_a_wrapper() {
        let body = "private readonly HttpClient _http;\n\
            public C(HttpClient http) { this._http = http; }\n\
            private Task<string> Fetch(string path) => _http.GetStringAsync($\"https://w.example.com{path}\");\n\
            public async Task Run(string id) { await Fetch(\"/direct\"); }\n";
        assert_eq!(contracts(body), ["http::GET::/direct"]);
    }

    #[test]
    fn a_wrapper_that_calls_itself_is_read_once() {
        let body = format!(
            "{CLIENT}Task<string> Walk(string p, int n) {{ if (n > 0) Walk(p, n - 1); return _http.GetStringAsync(p); }}\n\
             void Use() {{ Walk(\"https://c.example.com/x\", 2); }}\n"
        );
        assert_eq!(contracts(&body), ["http::GET::/x"]);
    }

    #[test]
    fn a_wrapper_takes_the_method_the_request_and_the_client_from_its_callers() {
        let body = format!(
            "{CLIENT}Task<HttpResponseMessage> Send(HttpMethod m, string u) => _http.SendAsync(new HttpRequestMessage(m, u));\n\
             Task<string> Get(HttpClient c, string p) => c.GetStringAsync(p);\n\
             Task<HttpResponseMessage> Raw(HttpRequestMessage r) => _http.SendAsync(r);\n\
             void Use() {{\n\
             Send(HttpMethod.Patch, \"https://s.example.com/p\");\n\
             Get(new HttpClient {{ BaseAddress = new Uri(\"https://g.example.com/api\") }}, \"/x\");\n\
             Raw(new HttpRequestMessage(HttpMethod.Head, \"https://r.example.com/h\"));\n\
             }}\n"
        );
        assert_eq!(
            sent(&body),
            [
                host("http::GET::/x", "g.example.com"),
                host("http::HEAD::/h", "r.example.com"),
                host("http::PATCH::/p", "s.example.com"),
            ]
        );
    }

    #[test]
    fn only_an_http_client_by_its_type_sends() {
        let other =
            "namespace A.Other;\npublic class Fake { public Task GetAsync(string u) => null; }\n";
        let hidden = "namespace A.Mine;\npublic class HttpClient { public Task GetAsync(string u) => null; }\n\
                      public class User { HttpClient c; void F() { c.GetAsync(\"/mine\"); } }\n";
        let body = format!(
            "{CLIENT}A.Other.Fake _fake;\n\
             IHttpClientFactory _factory;\n\
             async void F() {{\n\
             await _fake.GetAsync(\"/fake\");\n\
             await _factory.CreateClient(\"orders\").GetAsync(\"/chained\");\n\
             HttpClient named = _factory.CreateClient(\"orders\");\n\
             await named.GetAsync(\"/named\");\n\
             var made = _factory.CreateClient();\n\
             await made.GetAsync(\"/made\");\n\
             await _http.GetAsync(\"/kept\");\n\
             }}"
        );
        assert_eq!(
            sent_with(
                &body,
                &[("A/Other/Fake.cs", other), ("A/Mine/User.cs", hidden)]
            ),
            [to("http::GET::/kept")]
        );
    }

    #[test]
    fn the_type_is_read_through_an_alias_using() {
        let body = "Net.HttpClient _alias;\n\
             void F() { _alias.GetAsync(\"/alias\"); }";
        let src =
            format!("using Net = System.Net.Http;\nnamespace A;\npublic class C {{\n{body}\n}}\n");
        let parsed = [("A/C.cs", CSharp.parse("A/C.cs", src.as_bytes()).unwrap())];
        let files: BTreeMap<&str, &_> = parsed.iter().map(|(p, f)| (*p, f)).collect();
        let layout = CSharp.layout(&[], &files).unwrap();
        let resolver = Resolver::with_shared(&files, layout.modules, layout.shared, &CSharp);
        let code = Code::new(&resolver, &files, &CSharp);
        let mut found: Vec<_> = consumers(&code)
            .into_iter()
            .filter_map(|f| f.contract)
            .collect();
        found.sort();
        assert_eq!(found, ["http::GET::/alias"]);
    }
}
