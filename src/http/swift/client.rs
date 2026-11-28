use super::super::{consumer, fetches};
use crate::contract::Found;
use crate::lang::{Call, Invocation, Part, Value};
use crate::names::{one, spellings, typed};
use crate::resolve::{Code, Expand, Expanded, method as member, scope_of};

use super::super::join::{depends, find, merged, resolve, slice, text_parts};

#[derive(Clone, Copy)]
enum Lib {
    Session,
    Alamofire,
    Http,
}

impl Lib {
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

    fn request(self) -> (&'static str, &'static str) {
        match self {
            Self::Http => ("HTTPClientRequest", "method="),
            _ => ("URLRequest", "httpMethod="),
        }
    }
}

const FOUNDATION: &[&str] = &[
    "UIKit",
    "AppKit",
    "SwiftUI",
    "Cocoa",
    "FoundationNetworking",
];

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

// ponytail: real clients are held through a name or two; the limit keeps a crafted chain cheap.
const CLIENT_DEPTH: usize = 8;

impl<'c, 'a> Clients<'c, 'a> {
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

    fn request(&self, lib: Lib, value: &Value) -> (Value, Value) {
        let (ty, method) = lib.request();
        let unknown = (Value::Unknown, Value::Unknown);
        let Value::Call(request) = value else {
            return match value {
                Value::Ref(_) => (value.clone(), value.clone()),
                _ => unknown,
            };
        };
        if request.callee != ty || self.code.definition(self.path, self.scope, ty).is_some() {
            return unknown;
        }
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

    fn url(&self, value: &Value) -> Value {
        match value {
            Value::Str(_) | Value::Ref(_) => value.clone(),
            Value::Call(call) => self.made(call).map_or(Value::Unknown, Value::Str),
            _ => Value::Unknown,
        }
    }

    fn made(&self, call: &Invocation) -> Option<Vec<Part>> {
        if call.callee == "URL" && self.code.definition(self.path, self.scope, "URL").is_none() {
            let parts = text_parts(keyword(call, "string")?)?;
            return match keyword(call, "relativeTo") {
                None => Some(parts),
                Some(base) => {
                    let base = text_parts(&self.url(base));
                    let unread = crate::http::client_base();
                    let read = base.as_deref().unwrap_or(&unread);
                    if crate::http::rooted_over_unread(read, &Value::Str(parts.clone())) {
                        return Some(parts);
                    }
                    let base = base?;
                    resolve(&base, &parts).or_else(|| depends(&base, &parts))
                }
            };
        }
        if member(&call.callee) == "appendingPathComponent" && call.callee.contains('.') {
            let base = text_parts(&self.url(call.receiver.as_deref()?));
            let base = base.unwrap_or_else(crate::http::client_base);
            return Some(append(base, text_parts(call.args.first()?)?));
        }
        None
    }
}

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

const ALAMOFIRE_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

const NIO_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

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
