//! Provider documents (spec #21): OpenAPI files a repository with a `[serve]` table keeps.
//! `refresh` reads each one into a `Summary`, derived from its bytes alone and stored in the
//! index; `Http::declare` turns the routes into provider contracts at query time, and `report`
//! gives the status lines. Readers see the JSON value model whatever the syntax (`yaml.rs`).

mod openapi;
mod yaml;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http;
use crate::index::Export;
use crate::msg::Msg;

/// Whether `path` (repository-relative) is a document by its file name: `openapi.yaml`,
/// `openapi.yml`, `openapi.json`, or a name ending in `.openapi.` and one of those extensions.
pub fn is_document(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(ext, "yaml" | "yml" | "json") && (stem == "openapi" || stem.ends_with(".openapi"))
}

/// What Meada read from one document. Nothing else of the file is kept.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Summary {
    /// `openapi` for a document read; `None` when it was ignored (see `problems`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `info.title`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Sorted by path, then method, each once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
    /// The root `servers` URLs that name a host or a base path, sorted, each once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Problem>,
}

impl Summary {
    fn problem(problem: Problem) -> Self {
        Summary {
            problems: vec![problem],
            ..Summary::default()
        }
    }
}

/// What a root `servers` URL gives: the host it is served on (lower case, as a consumer's URL
/// names it) and its base path (normalized, no trailing `/`, empty for none). A relative URL has
/// no host.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Server {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
}

/// One path and method of a document; `path` is normalized like a code path.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Route {
    pub path: String,
    /// Upper case: `GET`.
    pub method: String,
}

impl Route {
    pub fn contract(&self) -> String {
        http::contract(&self.method, &self.path)
    }

    /// `GET /users/{}`.
    pub fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

/// Why a document, or part of one, was not read; rendered by `report`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "problem", rename_all = "kebab-case")]
pub enum Problem {
    /// The file does not parse as YAML or JSON (or is not UTF-8).
    Unreadable { line: usize, error: String },
    /// It parses, but is not OpenAPI 3.0 or 3.1; `found` is what it says it is, such as
    /// `swagger 2.0`.
    Format { found: Option<String> },
}

/// The summary of the document at `path` (repository-relative) holding `src`. JSON by the
/// extension, YAML otherwise. It never fails: what cannot be read is a problem.
pub fn read(path: &str, src: &[u8]) -> Summary {
    match parse(path, src) {
        Ok(doc) => openapi::read(&doc),
        Err((line, error)) => Summary::problem(Problem::Unreadable { line, error }),
    }
}

/// The document's value, or the line and the reason it cannot be read. An error at the end of
/// the file is on its last line.
fn parse(path: &str, src: &[u8]) -> Result<Value, (usize, String)> {
    let breaks = src.iter().filter(|&&b| b == b'\n').count();
    let last = breaks + usize::from(!src.ends_with(b"\n"));
    parse_any(path, src).map_err(|(line, error)| (line.min(last.max(1)), error))
}

fn parse_any(path: &str, src: &[u8]) -> Result<Value, (usize, String)> {
    if path.ends_with(".json") {
        return serde_json::from_slice(src).map_err(|e| {
            let error = e.to_string();
            // The position is the line; the column is noise in a status line.
            let error = error.split(" at line ").next().unwrap_or(&error);
            (e.line(), error.to_string())
        });
    }
    let text = std::str::from_utf8(src).map_err(|e| {
        let line = src[..e.valid_up_to()]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        (line, "not UTF-8".to_string())
    })?;
    yaml::parse(text)
}

/// The status lines of a repository (its index content `export`) about its documents, in path
/// order: each document ignored, with why, and each route a later document lists again.
pub fn report(export: &Export) -> Vec<String> {
    let mut lines = Vec::new();
    let mut first: BTreeMap<String, &str> = BTreeMap::new();
    for doc in &export.documents {
        let path = doc.path.as_str();
        for problem in &doc.summary.problems {
            lines.push(
                match problem {
                    Problem::Unreadable { line, error } => Msg::DocumentUnreadable {
                        path,
                        line: *line,
                        error,
                    },
                    Problem::Format { found } => Msg::DocumentFormat {
                        path,
                        found: found.as_deref(),
                    },
                }
                .text(),
            );
        }
        for route in &doc.summary.routes {
            match first.get(&route.contract()) {
                Some(first) => lines.push(
                    Msg::DuplicateRoute {
                        route: &route.label(),
                        path,
                        first,
                    }
                    .text(),
                ),
                None => {
                    first.insert(route.contract(), path);
                }
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{Problem, is_document, read};

    #[test]
    fn documents_are_found_by_file_name() {
        for (path, expected) in [
            ("openapi.yaml", true),
            ("api/openapi.yml", true),
            ("openapi.json", true),
            ("docs/orders.openapi.yaml", true),
            ("orders.openapi.yml", true),
            ("orders.openapi.json", true),
            ("swagger.yaml", false),
            ("openapi.txt", false),
            ("myopenapi.yaml", false),
            ("openapi.yaml.bak", false),
            ("src/openapi.ts", false),
        ] {
            assert_eq!(is_document(path), expected, "{path}");
        }
    }

    #[test]
    fn the_format_comes_from_the_content() {
        let found = |path: &str, src: &str| {
            let summary = read(path, src.as_bytes());
            match &summary.problems[..] {
                [] => Ok((summary.format.unwrap(), summary.version.unwrap())),
                [Problem::Format { found }] => Err(found.clone()),
                other => panic!("{other:?}"),
            }
        };
        let ok = |v: &str| Ok(("openapi".to_string(), v.to_string()));
        let not = |f: Option<&str>| Err(f.map(String::from));
        for (path, src, expected) in [
            ("openapi.yaml", "openapi: 3.1.0\n", ok("3.1.0")),
            ("openapi.yaml", "openapi: 3.0.3\n", ok("3.0.3")),
            ("openapi.yaml", "openapi: '3.0'\n", ok("3.0")),
            // A number in YAML still names the version.
            ("openapi.yaml", "openapi: 3.1\n", ok("3.1")),
            ("openapi.json", r#"{"openapi": "3.1.0"}"#, ok("3.1.0")),
            ("openapi.yaml", "swagger: '2.0'\n", not(Some("swagger 2.0"))),
            (
                "openapi.yaml",
                "openapi: 2.5.0\n",
                not(Some("openapi 2.5.0")),
            ),
            (
                "openapi.yaml",
                "openapi: 3.2.0\n",
                not(Some("openapi 3.2.0")),
            ),
            ("openapi.yaml", "asyncapi: 3.0.0\n", not(None)),
            ("openapi.yaml", "- a list\n", not(None)),
        ] {
            assert_eq!(found(path, src), expected, "{src}");
        }
    }

    #[test]
    fn every_path_and_method_is_a_route() {
        let src = "openapi: 3.1.0\npaths:\n  /users/{id}/:\n    parameters: []\n    get: {}\n    \
                   delete: {}\n    trace: {}\n  /users/{name}:\n    get: {}\n  x-extension:\n    \
                   get: {}\n";
        let routes: Vec<String> = read("openapi.yaml", src.as_bytes())
            .routes
            .iter()
            .map(|r| r.contract())
            .collect();
        assert_eq!(
            routes,
            [
                "http::DELETE::/users/{}",
                "http::GET::/users/{}",
                "http::TRACE::/users/{}"
            ]
        );
    }

    #[test]
    fn a_document_that_does_not_parse_gives_its_line() {
        for (path, src, line) in [
            ("openapi.json", "{\n  \"openapi\": \"3.1.0\",\n}\n", 3),
            ("openapi.yaml", "openapi: 3.1.0\npaths: [\n", 2),
        ] {
            match &read(path, src.as_bytes()).problems[..] {
                [Problem::Unreadable { line: at, .. }] => assert_eq!(*at, line, "{src}"),
                other => panic!("{src}: {other:?}"),
            }
        }
        let invalid = read("openapi.yaml", b"openapi: 3.1.0\n\xff\n");
        assert_eq!(
            invalid.problems,
            [Problem::Unreadable {
                line: 2,
                error: "not UTF-8".into()
            }]
        );
    }
}
