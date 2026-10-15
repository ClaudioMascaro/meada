//! OpenAPI 3.0 and 3.1 (spec #21): every path and method of a document is a route.

use serde_json::Value;

use super::{Problem, Route, Summary};
use crate::http::{PARAM, normalize_path};

/// The operations a path item may hold.
const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// What `doc` gives: its routes when its `openapi` field is a 3.0 or 3.1 version, else the
/// format it has instead.
pub fn read(doc: &Value) -> Summary {
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
    let mut routes = Vec::new();
    let paths = doc.get("paths").and_then(Value::as_object);
    for (path, item) in paths.into_iter().flatten() {
        if !path.starts_with('/') {
            continue;
        }
        for &method in METHODS {
            if item.get(method).is_some_and(Value::is_object) {
                routes.push(Route {
                    method: method.to_uppercase(),
                    path: normalize_path(&template(path)),
                });
            }
        }
    }
    routes.sort();
    // `/users/{id}` and `/users/{name}` are one route.
    routes.dedup();
    Summary {
        format: Some("openapi".into()),
        version: Some(version),
        title: doc.pointer("/info/title").and_then(text),
        routes,
        problems: Vec::new(),
    }
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
    use super::template;

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
