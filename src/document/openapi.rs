//! OpenAPI 3.0 and 3.1 (spec #21): every path and method of a document is a route, with the
//! shape of its 2xx JSON response.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{Problem, Route, Server, Summary, schema};
use crate::http::{PARAM, normalize_path};
use crate::shape::Source;

/// The operations a path item may hold.
const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// What `doc`, the document at `path`, gives: its routes when its `openapi` field is a 3.0 or
/// 3.1 version, else the format it has instead.
pub fn read(path: &str, doc: &Value) -> Summary {
    let field = |name: &str| text(doc.get(name)?);
    let version = field("openapi");
    let Some(version) = version.filter(|v| supported(v)) else {
        let found = match (field("openapi"), field("swagger")) {
            (Some(v), _) => Some(format!("openapi {v}")),
            (None, Some(v)) => Some(format!("swagger {v}")),
            (None, None) => None,
        };
        return Summary::problem(Problem::Format { found });
    };
    let mut operations = Vec::new();
    let paths = doc.get("paths").and_then(Value::as_object);
    for (route, item) in paths.into_iter().flatten() {
        if !route.starts_with('/') {
            continue;
        }
        for &method in METHODS {
            if let Some(operation) = item.get(method).filter(|o| o.is_object()) {
                let route = normalize_path(&template(route));
                operations.push((route, method.to_uppercase(), operation));
            }
        }
    }
    // Stable: of `/users/{id}` and `/users/{name}`, one route, the first key gives the shape.
    operations.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    operations.dedup_by(|a, b| (&a.0, &a.1) == (&b.0, &b.1));
    let mut external = BTreeSet::new();
    let routes = (operations.into_iter())
        .map(|(route, method, operation)| {
            let shape = response(doc, operation).map(|response| {
                let source = Source::Document(path.to_string());
                let (shape, refs) = schema::read(doc, response, source);
                external.extend(refs);
                shape
            });
            Route {
                path: route,
                method,
                shape,
            }
        })
        .collect();
    Summary {
        format: Some("openapi".into()),
        version: Some(version),
        title: doc.pointer("/info/title").and_then(text),
        routes,
        servers: servers(doc),
        problems: (external.into_iter())
            .map(|reference| Problem::ExternalRef { reference })
            .collect(),
    }
}

/// The `application/json` schema of `operation`'s lowest explicit 2xx response that has one,
/// else of its `2XX` response. `application/*+json` is not read.
fn response<'a>(doc: &'a Value, operation: &'a Value) -> Option<&'a Value> {
    let responses = operation.get("responses")?.as_object()?;
    let schema = |response: &'a Value| {
        let response = local(doc, response)?;
        response
            .get("content")?
            .get("application/json")?
            .get("schema")
    };
    let mut explicit: Vec<_> = (responses.iter())
        .filter(|(code, _)| {
            code.len() == 3 && code.starts_with('2') && code.bytes().all(|b| b.is_ascii_digit())
        })
        .collect();
    explicit.sort_by_key(|&(code, _)| code);
    (explicit.into_iter())
        .find_map(|(_, response)| schema(response))
        .or_else(|| schema(responses.get("2XX")?))
}

/// `value`, or the object its local `$ref` points to, through a few references; `None` for an
/// external or missing one.
fn local<'a>(doc: &'a Value, mut value: &'a Value) -> Option<&'a Value> {
    for _ in 0..8 {
        let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
            return Some(value);
        };
        value = doc.pointer(reference.strip_prefix('#')?)?;
    }
    None
}

/// The root `servers` URLs of `doc`. Server variables take their default; a URL that still holds
/// one has nothing to say. A relative URL gives its path only, a URL with neither host nor
/// path nothing.
fn servers(doc: &Value) -> Vec<Server> {
    let entries = doc.get("servers").and_then(Value::as_array);
    let mut found: Vec<Server> = (entries.into_iter().flatten())
        .filter_map(|entry| {
            let mut url = entry.get("url")?.as_str()?.to_string();
            let variables = entry.get("variables").and_then(Value::as_object);
            for (name, variable) in variables.into_iter().flatten() {
                if let Some(default) = variable.get("default").and_then(text) {
                    url = url.replace(&format!("{{{name}}}"), &default);
                }
            }
            if url.contains(['{', '}']) {
                return None;
            }
            let url = url.trim();
            let (host, path) = match url.split_once("://") {
                Some((_, rest)) => {
                    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                    let authority = &rest[..end];
                    let host = authority.rsplit('@').next().unwrap_or(authority);
                    let host = Some(host.to_lowercase()).filter(|h| !h.is_empty());
                    (host, &rest[end..])
                }
                None => (None, url),
            };
            let path = path.split(['?', '#']).next().unwrap_or("");
            let path = if path.is_empty() || path.starts_with('/') {
                normalize_path(path)
            } else {
                normalize_path(&format!("/{path}"))
            };
            let path = path.trim_end_matches('/').to_string();
            (host.is_some() || !path.is_empty()).then_some(Server { host, path })
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// A string, or a number as written in JSON (`openapi: 3.0` in YAML is a number).
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `3.0`, `3.1` or any patch of them.
fn supported(version: &str) -> bool {
    let mut parts = version.split('.');
    parts.next() == Some("3") && matches!(parts.next(), Some("0" | "1"))
}

/// `path` with every template (`{id}`) written as the parameter marker.
fn template(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(PARAM);
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::{read, template};
    use crate::document::Server;

    #[test]
    fn root_servers_give_hosts_and_base_paths() {
        let doc = serde_json::json!({
            "openapi": "3.1.0",
            "servers": [
                {"url": "https://API.example.com/v2/"},
                {"url": "https://{region}.example.com:8443/{ver}", "variables": {
                    "region": {"default": "eu"}, "ver": {"default": "v3"}}},
                {"url": "https://{env}.example.com/v4", "variables": {"env": {"enum": ["a"]}}},
                {"url": "/relative/base"},
                {"url": "https://bare.example.com"},
                {"url": "/"},
                {"url": "https://api.example.com/v2"},
            ],
            "paths": {"/y": {"servers": [{"url": "https://no.example.com"}]}},
        });
        let server = |host: Option<&str>, path: &str| Server {
            host: host.map(String::from),
            path: path.into(),
        };
        assert_eq!(
            read("openapi.json", &doc).servers,
            vec![
                server(None, "/relative/base"),
                server(Some("api.example.com"), "/v2"),
                server(Some("bare.example.com"), ""),
                server(Some("eu.example.com:8443"), "/v3"),
            ]
        );
    }

    #[test]
    fn path_templates_become_the_parameter_marker() {
        for (path, expected) in [
            ("/users/{id}", "/users/{}"),
            ("/users/{id}/posts/{post}", "/users/{}/posts/{}"),
            ("/files/{name}.json", "/files/{}.json"),
            ("/plain", "/plain"),
            ("/open/{x", "/open/{x"),
        ] {
            assert_eq!(template(path), expected);
        }
    }
}
