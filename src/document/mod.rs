mod asyncapi;
pub(crate) mod events;
mod openapi;
mod schema;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::index::Export;
use crate::msg::Msg;
use crate::shape::Shape;
use crate::workspace::Repository;
use crate::{channel, http};

// ponytail: a flat 16 MiB, parsed whole in memory; a streaming reader that keeps only the
// paths, servers and channels lifts it.
pub(crate) const DOCUMENT_BYTES_MAX: usize = 16 << 20;

pub fn is_document(path: &str) -> bool {
    named(path, "openapi") || named(path, "asyncapi")
}

fn named(path: &str, format: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(ext, "yaml" | "yml" | "json")
        && (stem == format || stem.strip_suffix(format).is_some_and(|s| s.ends_with('.')))
}

pub use meada_core::export::Format;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Summary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<Side>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Problem>,
}

impl Summary {
    pub fn shown(&self) -> Option<String> {
        let (format, version) = (self.format?, self.version.as_ref()?);
        let counted = match format {
            Format::Asyncapi => "channels",
            Format::Openapi => "routes",
        };
        let count = self.count(format);
        Some(format!("{} {version}, {counted}: {count}", format.name()))
    }

    pub fn count(&self, format: Format) -> usize {
        match format {
            Format::Openapi => self.routes.len(),
            Format::Asyncapi => self.channel_count(),
        }
    }

    pub fn channel_count(&self) -> usize {
        let channels: BTreeSet<String> = self.channels.iter().map(Side::contract).collect();
        channels.len()
    }

    fn problem(problem: Problem) -> Self {
        Summary {
            problems: vec![problem],
            ..Summary::default()
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Server {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Side {
    pub broker: String,
    pub name: String,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Producer,
    Consumer,
}

impl Side {
    pub fn contract(&self) -> String {
        channel::contract(&self.broker, Some(&self.name))
    }

    pub fn label(&self) -> String {
        let verb = match self.role {
            Role::Producer => "send",
            Role::Consumer => "receive",
        };
        format!("{} {verb} {}", self.broker, self.name)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Route {
    pub path: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

impl Route {
    pub fn contract(&self) -> String {
        http::contract(&self.method, &self.path)
    }

    pub fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "problem", rename_all = "kebab-case")]
pub enum Problem {
    Unreadable { line: usize, error: String },
    AsyncFormat { found: Option<String> },
    Protocol { protocol: String },
    Parameters { channel: String },
    NoServers,
    NoProtocol { server: String },
    UnknownServer { channel: String, server: String },
    Format { found: Option<String> },
    ExternalRef { reference: String },
    ExternalPathItem { reference: String },
}

pub fn read(path: &str, src: &[u8]) -> Summary {
    if src.len() > DOCUMENT_BYTES_MAX {
        let error = format!("larger than {} MiB", DOCUMENT_BYTES_MAX >> 20);
        return Summary::problem(Problem::Unreadable { line: 1, error });
    }
    match parse(path, src) {
        Ok(doc) if doc.get("openapi").is_some() => openapi::read(path, &doc),
        Ok(doc) if doc.get("asyncapi").is_some() || named(path, "asyncapi") => {
            asyncapi::read(path, &doc)
        }
        Ok(doc) => openapi::read(path, &doc),
        Err((line, error)) => Summary::problem(Problem::Unreadable { line, error }),
    }
}

fn parse(path: &str, src: &[u8]) -> Result<Value, (usize, String)> {
    let breaks = src.iter().filter(|&&b| b == b'\n').count();
    let last = breaks + usize::from(!src.ends_with(b"\n"));
    events::first(events::Syntax::of(path), src)
        .map_err(|(line, error)| (line.min(last.max(1)), error))
}

pub fn report(repo: &Repository, export: &Export) -> Vec<String> {
    let mut lines = Vec::new();
    let mut first: BTreeMap<String, &str> = BTreeMap::new();
    for doc in &export.documents {
        let path = doc.path.as_str();
        if let (Some(format), Some(version)) = (doc.summary.format, &doc.summary.version) {
            lines.push(
                Msg::DocumentRead {
                    path,
                    format,
                    version,
                    count: doc.summary.count(format),
                }
                .text(),
            );
        }
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
                    Problem::AsyncFormat { found } => Msg::DocumentAsyncFormat {
                        path,
                        found: found.as_deref(),
                    },
                    Problem::Protocol { protocol } => Msg::DocumentProtocol { path, protocol },
                    Problem::Parameters { channel } => Msg::DocumentParameters { path, channel },
                    Problem::NoServers => Msg::DocumentNoServers { path },
                    Problem::NoProtocol { server } => Msg::DocumentNoProtocol { path, server },
                    Problem::UnknownServer { channel, server } => Msg::DocumentUnknownServer {
                        path,
                        channel,
                        server,
                    },
                    Problem::ExternalRef { reference } => Msg::ExternalRef { path, reference },
                    Problem::ExternalPathItem { reference } => {
                        Msg::ExternalPathItem { path, reference }
                    }
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
    for path in &repo.config.serve.documents {
        if !export.documents.iter().any(|d| d.path == *path) {
            lines.push(Msg::DocumentMissing { path }.text());
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{DOCUMENT_BYTES_MAX, Format, Problem, is_document, read};

    #[test]
    fn a_document_past_the_size_limit_is_unreadable() {
        let valid = "openapi: 3.1.0\npaths:\n  /a:\n    get: {}\n";
        let padded = |len: usize| format!("{valid}{}", "#".repeat(len - valid.len()));

        let at_limit = read("openapi.yaml", padded(DOCUMENT_BYTES_MAX).as_bytes());
        let past = read("openapi.yaml", padded(DOCUMENT_BYTES_MAX + 1).as_bytes());

        assert_eq!(at_limit.format, Some(Format::Openapi));
        assert_eq!(past.format, None);
        assert_eq!(
            past.problems,
            [Problem::Unreadable {
                line: 1,
                error: "larger than 16 MiB".into()
            }]
        );
    }

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
            ("asyncapi.yaml", true),
            ("asyncapi.yml", true),
            ("docs/asyncapi.json", true),
            ("orders.asyncapi.yaml", true),
            ("orders.asyncapi.yml", true),
            ("orders.asyncapi.json", true),
            ("myasyncapi.yaml", false),
            ("asyncapi.txt", false),
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
        let ok = |v: &str| Ok((Format::Openapi, v.to_string()));
        let not = |f: Option<&str>| Err(f.map(String::from));
        for (path, src, expected) in [
            ("openapi.yaml", "openapi: 3.1.0\n", ok("3.1.0")),
            ("openapi.yaml", "openapi: 3.0.3\n", ok("3.0.3")),
            ("openapi.yaml", "openapi: '3.0'\n", ok("3.0")),
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
            ("openapi.yaml", "info: {}\n", not(None)),
            ("openapi.yaml", "- a list\n", not(None)),
        ] {
            assert_eq!(found(path, src), expected, "{src}");
        }
    }

    #[test]
    fn the_format_field_decides_whatever_the_name() {
        let format = |path: &str, src: &str| {
            let summary = read(path, src.as_bytes());
            (summary.format, summary.problems)
        };
        let openapi = "openapi: 3.1.0\npaths: {/x: {get: {}}}\n";
        let asyncapi = "asyncapi: 2.6.0\n";
        for path in [
            "api.asyncapi.yaml",
            "api.openapi.yaml",
            "docs/contract.yaml",
        ] {
            assert_eq!(
                format(path, openapi),
                (Some(Format::Openapi), vec![]),
                "{path}"
            );
            assert_eq!(
                format(path, asyncapi),
                (Some(Format::Asyncapi), vec![]),
                "{path}"
            );
        }
        assert_eq!(
            read("api.asyncapi.yaml", openapi.as_bytes()).routes.len(),
            1
        );
        let swagger = "swagger: '2.0'\n";
        let found = Some("swagger 2.0".to_string());
        for (path, problem) in [
            ("asyncapi.yaml", Problem::AsyncFormat { found: None }),
            ("openapi.yaml", Problem::Format { found: None }),
            ("docs/contract.yaml", Problem::Format { found: None }),
        ] {
            assert_eq!(format(path, "info: {}\n"), (None, vec![problem]), "{path}");
        }
        assert_eq!(
            format("api.asyncapi.yaml", swagger),
            (None, vec![Problem::AsyncFormat { found }])
        );
    }

    #[test]
    fn a_path_item_reference_is_followed_when_local() {
        let src = r##"
openapi: 3.1.0
paths:
  /items:
    $ref: '#/components/pathItems/Items'
  /chained:
    $ref: '#/components/pathItems/Alias'
  /elsewhere:
    $ref: 'shared.yaml#/paths/~1elsewhere'
  /gone:
    $ref: '#/components/pathItems/Gone'
components:
  pathItems:
    Items:
      get: {}
      post: {}
    Alias:
      $ref: '#/components/pathItems/Items'
"##;
        let summary = read("openapi.yaml", src.as_bytes());
        let routes: Vec<String> = summary.routes.iter().map(|r| r.label()).collect();
        assert_eq!(
            routes,
            ["GET /chained", "POST /chained", "GET /items", "POST /items"]
        );
        assert_eq!(
            summary.problems,
            [Problem::ExternalPathItem {
                reference: "shared.yaml#/paths/~1elsewhere".into()
            }]
        );
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
    fn a_route_shape_is_its_lowest_2xx_json_response() {
        let json = |field: &str| {
            format!(
                r#"{{"content": {{"application/json": {{"schema": {{"properties": {{"{field}": {{"type": "string"}}}}}}}}}}}}"#
            )
        };
        let plain = r#"{"description": "no body"}"#;
        let fields = |responses: String| {
            let src = format!(
                r#"{{"openapi": "3.0.3", "paths": {{"/x": {{"get": {{"responses": {{{responses}}}}}}}}},
                "components": {{"responses": {{"Ok": {}}}}}}}"#,
                json("shared")
            );
            let summary = read("openapi.json", src.as_bytes());
            assert_eq!(summary.problems, [], "{responses}");
            let shape = summary.routes[0].shape.as_ref();
            shape.map(|s| s.fields.iter().cloned().collect::<Vec<_>>())
        };
        let some = |field: &str| Some(vec![field.to_string()]);
        for (responses, expected) in [
            (format!(r#""206": {}, "200": {}"#, json("partial"), json("ok")), some("ok")),
            (format!(r#""202": {plain}, "2XX": {}"#, json("range")), some("range")),
            (format!(r#""201": {}, "2XX": {}"#, json("created"), json("range")), some("created")),
            (
                r#""200": {"content": {"application/vnd.api+json": {"schema": {"type": "object"}}}}"#
                    .to_string(),
                None,
            ),
            (format!(r#""200": {plain}"#), None),
            (format!(r#""default": {}, "404": {}"#, json("d"), json("e")), None),
            (
                r##""200": {"$ref": "#/components/responses/Ok"}"##.to_string(),
                some("shared"),
            ),
        ] {
            assert_eq!(fields(responses.clone()), expected, "{responses}");
        }
        assert_eq!(
            read(
                "openapi.yaml",
                b"openapi: 3.1.0\npaths:\n  /x:\n    get: {}\n"
            )
            .routes[0]
                .shape,
            None
        );
    }

    #[test]
    fn external_references_are_problems_of_the_document() {
        let src = r#"
openapi: 3.1.0
paths:
  /a:
    get:
      responses:
        "200":
          content:
            application/json:
              schema:
                properties:
                  b: {$ref: "b.yaml#/B"}
                  a: {$ref: "https://example.com/a.json"}
  /b:
    get:
      responses:
        "200":
          content:
            application/json:
              schema: {$ref: "b.yaml#/B"}
"#;
        let summary = read("openapi.yaml", src.as_bytes());
        assert_eq!(
            summary.problems,
            [
                Problem::ExternalRef {
                    reference: "b.yaml#/B".into()
                },
                Problem::ExternalRef {
                    reference: "https://example.com/a.json".into()
                }
            ]
        );
        assert_eq!(summary.format, Some(Format::Openapi));
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
