//! Swift HTTP consumers (spec #29, #121): URLSession's `data(from:)` and `data(for:)`, Alamofire's
//! `request` and AsyncHTTPClient's `execute`. A call counts when its receiver is the library's by
//! what the imports say: a name the library binds (`URLSession.shared`, `AF`), a name written as
//! its type (a parameter, a property, a local), or the construction or the name a local or a
//! constant holds (`URLSession(configuration:)`, `let session = URLSession.shared`); a type of
//! the repository with the same name hides it. UIKit, AppKit, SwiftUI and Cocoa re-export
//! Foundation, so an import of one of them spells its names. `dataTask(with:)` is not read.
//!
//! The call is read whole (`Expand::Chain`), so a client wrapper, a function whose URL or method
//! depends on its parameters, makes each of its call sites a consumer with its arguments
//! substituted, through any number of levels. The URL folds like any value (`lang/swift/values.rs`:
//! literals, interpolation, `+`, `let` and `static let` constants), through `URL(string:)`,
//! `URL(string:relativeTo:)` (resolved as RFC 3986 does) and `appendingPathComponent`. A request
//! is `URLRequest(url:)` or `HTTPClientRequest(url:)`, its method the one the pack records for
//! the statement that assigns it (`httpMethod = ..`, `method = ..`, `Facts::bound`), `GET` when
//! none does; an assignment that may not run, or a second one, leaves the method unknown, and
//! an assigned `url` leaves the URL unknown. What does not fold is an unresolved consumer.

use super::super::{consumer, fetches};
use crate::contract::Found;
use crate::lang::{Call, Invocation, Part, Value};
use crate::names::{one, spellings, typed};
use crate::resolve::{Code, Expand, Expanded, method as member, scope_of};

use super::super::join::{depends, find, merged, resolve, slice, text_parts};

/// The libraries a call can send through.
#[derive(Clone, Copy)]
enum Lib {
    /// Foundation's `URLSession.data(from:)` and `data(for:)`.
    Session,
    /// Alamofire's `request`.
    Alamofire,
    /// AsyncHTTPClient's `execute`.
    Http,
}

impl Lib {
    /// The library a call of this member and labels may send through.
    fn of(call: &Call) -> Option<Self> {
        let labels = call.labels.as_deref()?;
        let first = labels.first().map(String::as_str);
        match (member(&call.callee), first) {
            ("data", Some("from" | "for")) => Some(Self::Session),
            ("request", Some("_")) => Some(Self::Alamofire),
            ("execute", Some("_")) => Some(Self::Http),
            _ => None,
        }
    }

    /// The types the receiver may be, and the values the library binds, as the imports spell them.
    fn receivers(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::Session => (
                &["Foundation.URLSession"],
                &["Foundation.URLSession.shared"],
            ),
            Self::Alamofire => (
                &["Alamofire.Session"],
                &["Alamofire.AF", "Alamofire.Session.default"],
            ),
            Self::Http => (
                &["AsyncHTTPClient.HTTPClient"],
                &["AsyncHTTPClient.HTTPClient.shared"],
            ),
        }
    }

    /// The request type the library sends, and the member that assigns its method.
    fn request(self) -> (&'static str, &'static str) {
        match self {
            Self::Http => ("HTTPClientRequest", "method="),
            _ => ("URLRequest", "httpMethod="),
        }
    }
}

/// The modules that re-export Foundation's names: `import UIKit` spells `URLSession` as
/// `Foundation.URLSession` does.
const FOUNDATION: &[&str] = &[
    "UIKit",
    "AppKit",
    "SwiftUI",
    "Cocoa",
    "FoundationNetworking",
];

/// Whether exactly one of the spellings of a name is in `table`, Foundation's re-exports being
/// one spelling with it.
fn named(spelled: &[String], table: &[&str]) -> bool {
    let mut spellings: Vec<String> = Vec::new();
    for s in spelled {
        let s = match s.split_once('.') {
            Some((module, rest)) if FOUNDATION.contains(&module) => format!("Foundation.{rest}"),
            _ => s.clone(),
        };
        if !spellings.contains(&s) {
            spellings.push(s);
        }
    }
    one(&spellings, |s| table.contains(&s)).is_some()
}

struct Clients<'c, 'a> {
    code: &'c Code<'a>,
}

/// How many locals and constants a client is followed through.
// ponytail: real clients are held through a name or two; the limit keeps a crafted chain cheap.
const CLIENT_DEPTH: usize = 8;

impl<'c, 'a> Clients<'c, 'a> {
    /// Whether `receiver`, written in `scope` of `path`, is the library's client: exactly one
    /// spelling of its name is a value the library binds or a type it declares, or the name is
    /// written as that type, or it holds one (a local, a constant, a construction).
    fn is(
        &self,
        path: &'a str,
        scope: &'a str,
        receiver: &'a Value,
        lib: Lib,
        depth: usize,
    ) -> bool {
        let (types, values) = lib.receivers();
        let code = self.code;
        match receiver {
            Value::Ref(name) => {
                if named(&spellings(code, path, scope, name), values)
                    || named(&typed(code, path, scope, name), types)
                {
                    return true;
                }
                if depth >= CLIENT_DEPTH {
                    return false;
                }
                if let Some(bound) = code.local(path, scope, name) {
                    return self.is(path, &bound.within, &bound.value, lib, depth + 1);
                }
                match code.constant(path, scope, name) {
                    Some(((at, constant), value)) => {
                        self.is(at, scope_of(constant), value, lib, depth + 1)
                    }
                    None => false,
                }
            }
            Value::Call(made) => named(&spellings(code, path, scope, &made.callee), types),
            _ => false,
        }
    }
}

/// Swift HTTP consumers: see the module.
pub(super) fn consumers(code: &Code) -> Vec<Found> {
    let clients = Clients { code };
    let mut found = Vec::new();
    for (&path, facts) in code.files() {
        if !path.ends_with(".swift") {
            continue;
        }
        for (n, stored) in facts.calls.iter().enumerate() {
            let Some(lib) = Lib::of(stored) else {
                continue;
            };
            // The values of the call: `files` holds none, only the facts the index keeps.
            let (Some(call), Some(receiver)) = (code.call(path, n), code.receiver(path, n)) else {
                continue;
            };
            if !clients.is(path, &call.scope, receiver, lib, 0) {
                continue;
            }
            let reads = Reads {
                code,
                path,
                scope: &call.scope,
            };
            let read = |values: &[Value], kwargs: &[(String, Value)]| {
                let [Value::Call(sent)] = values else {
                    return None;
                };
                let (url, method) = reads.sent(lib, sent, kwargs);
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

/// What a folded call sends, read where the call is written (`path`, `scope`).
struct Reads<'c, 'a> {
    code: &'c Code<'a>,
    path: &'a str,
    scope: &'a str,
}

fn text(method: &str) -> Value {
    Value::Str(vec![Part::Text(method.into())])
}

fn keyword<'v>(call: &'v Invocation, key: &str) -> Option<&'v Value> {
    (call.kwargs.iter().find(|(k, _)| k == key)).map(|(_, v)| v)
}

impl Reads<'_, '_> {
    /// The URL and the method the folded call `sent` of `lib` sends; a name that depends on a
    /// parameter stays a `Value::Ref`, read again at the wrapper's call sites.
    fn sent(&self, lib: Lib, sent: &Invocation, kwargs: &[(String, Value)]) -> (Value, Value) {
        let key = |k: &str| kwargs.iter().find(|(name, _)| name == k).map(|(_, v)| v);
        match lib {
            Lib::Session => match (key("from"), key("for")) {
                (Some(url), None) => (self.url(url), text("GET")),
                (None, Some(request)) => self.request(lib, request),
                _ => (Value::Unknown, Value::Unknown),
            },
            Lib::Http => match sent.args.first() {
                Some(request) => self.request(lib, request),
                None => (Value::Unknown, Value::Unknown),
            },
            Lib::Alamofire => {
                let Some(first) = sent.args.first() else {
                    return (Value::Unknown, Value::Unknown);
                };
                if let Value::Call(request) = first
                    && request.callee == lib.request().0
                {
                    return self.request(lib, first);
                }
                let method = match key("method") {
                    None => text("GET"),
                    Some(m) => alamofire_method(m),
                };
                (self.url(first), method)
            }
        }
    }

    /// The URL and the method of a request value.
    fn request(&self, lib: Lib, value: &Value) -> (Value, Value) {
        let (ty, method) = lib.request();
        let unknown = (Value::Unknown, Value::Unknown);
        let Value::Call(request) = value else {
            // A request a wrapper takes is read at its callers.
            return match value {
                Value::Ref(_) => (value.clone(), value.clone()),
                _ => unknown,
            };
        };
        if request.callee != ty || self.code.definition(self.path, self.scope, ty).is_some() {
            return unknown;
        }
        // An assigned `url` is a URL Meada does not read.
        let url = match keyword(request, "url=") {
            Some(_) => Value::Unknown,
            None => keyword(request, "url").map_or(Value::Unknown, |u| self.url(u)),
        };
        let method = match (lib, keyword(request, method)) {
            (_, None) => text("GET"),
            (Lib::Http, Some(m)) => http_client_method(m),
            (_, Some(Value::Str(parts))) => Value::Str(parts.clone()),
            (_, Some(Value::Ref(name))) => Value::Ref(name.clone()),
            _ => Value::Unknown,
        };
        (url, method)
    }

    /// The text a URL or a string value is.
    fn url(&self, value: &Value) -> Value {
        match value {
            Value::Str(_) | Value::Ref(_) => value.clone(),
            Value::Call(call) => self.made(call).map_or(Value::Unknown, Value::Str),
            _ => Value::Unknown,
        }
    }

    /// The parts of a URL a call makes: `URL(string:)`, `URL(string:relativeTo:)` or
    /// `appendingPathComponent` on one. A type of the repository named `URL` is no Foundation's.
    fn made(&self, call: &Invocation) -> Option<Vec<Part>> {
        if call.callee == "URL" && self.code.definition(self.path, self.scope, "URL").is_none() {
            let parts = text_parts(keyword(call, "string")?)?;
            return match keyword(call, "relativeTo") {
                None => Some(parts),
                Some(base) => {
                    let base = text_parts(&self.url(base))?;
                    // A parameter on either side is resolved at the wrapper's call sites.
                    resolve(&base, &parts).or_else(|| depends(&base, &parts))
                }
            };
        }
        if member(&call.callee) == "appendingPathComponent" && call.callee.contains('.') {
            let base = text_parts(&self.url(call.receiver.as_deref()?))?;
            return Some(append(base, text_parts(call.args.first()?)?));
        }
        None
    }
}

/// `base` with the path component `component` added after a `/`, in its path (before a query or
/// a fragment).
fn append(base: Vec<Part>, mut component: Vec<Part>) -> Vec<Part> {
    let end = (base.len(), 0);
    let at = find(&base, (0, 0), &['?', '#']).unwrap_or(end);
    let mut out = slice(&base, (0, 0), at);
    if let Some(Part::Text(t)) = out.last_mut() {
        *t = t.trim_end_matches('/').to_string();
    }
    if let Some(Part::Text(t)) = component.first_mut() {
        *t = t.trim_start_matches('/').to_string();
    }
    out.push(Part::Text("/".into()));
    out.extend(component);
    out.extend(slice(&base, at, end));
    merged(out)
}

/// The methods of Alamofire's `HTTPMethod`, by their lower-case members.
const ALAMOFIRE_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

/// The methods of NIOHTTP1's `HTTPMethod`, by their upper-case members.
const NIO_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// The method an implicit member (`.post`) or one of `HTTPMethod` as the imports spell it
/// (`Alamofire.HTTPMethod.post`) names among `table`; a parameter of a wrapper stays as it is,
/// anything else is unknown.
fn member_method(value: &Value, prefixes: &[&str], table: &[&str], upper: bool) -> Value {
    let named = match value {
        Value::Member(ty, name) if ty.is_empty() => Some(name.as_str()),
        Value::External(spelled) => {
            fn member<'s>(s: &'s str, prefixes: &[&str], table: &[&str]) -> Option<&'s str> {
                let rest = prefixes.iter().find_map(|p| s.strip_prefix(p))?;
                table.contains(&rest).then_some(rest)
            }
            one(spelled, |s| member(s, prefixes, table).is_some())
                .and_then(|s| member(s, prefixes, table))
        }
        Value::Ref(_) => return value.clone(),
        _ => None,
    };
    match named.filter(|m| table.contains(m)) {
        Some(m) if upper => text(m),
        Some(m) => text(&m.to_uppercase()),
        None => Value::Unknown,
    }
}

fn alamofire_method(value: &Value) -> Value {
    member_method(value, &["Alamofire.HTTPMethod."], ALAMOFIRE_METHODS, false)
}

fn http_client_method(value: &Value) -> Value {
    let prefixes = ["NIOHTTP1.HTTPMethod.", "AsyncHTTPClient.HTTPMethod."];
    member_method(value, &prefixes, NIO_METHODS, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(parts: &[Part]) -> String {
        let one = |p: &Part| match p {
            Part::Text(t) => t.clone(),
            _ => "{}".to_string(),
        };
        parts.iter().map(one).collect()
    }

    #[test]
    fn a_path_component_is_added_before_the_query() {
        let base = vec![Part::Text("https://h.example.com/api/?v=1#top".into())];
        let joined = append(base, vec![Part::Text("users".into())]);
        assert_eq!(written(&joined), "https://h.example.com/api/users?v=1#top");
    }

    #[test]
    fn the_modules_that_re_export_foundation_spell_its_names() {
        let both = [
            "UIKit.URLSession".to_string(),
            "Foundation.URLSession".to_string(),
        ];
        assert!(named(&both, &["Foundation.URLSession"]));
        assert!(named(
            &["Cocoa.URLSession".to_string()],
            &["Foundation.URLSession"]
        ));
        assert!(!named(
            &["Other.URLSession".to_string()],
            &["Foundation.URLSession"]
        ));
    }

    #[test]
    fn a_path_component_joins_with_one_slash() {
        let base = vec![Part::Text("https://h.example.com/".into())];
        let joined = append(base, vec![Part::Text("/users".into())]);
        assert_eq!(written(&joined), "https://h.example.com/users");
    }
}
