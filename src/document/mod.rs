//! Provider documents (spec #21): OpenAPI and AsyncAPI files a repository with a `[serve]`
//! table keeps. `refresh` reads each one into a `Summary`, derived from its bytes alone and
//! stored in the index; `Http::declare` turns the routes into provider contracts at query time,
//! `Channel::declare` the channel sides, and `report` gives the status lines. Readers see the JSON value model whatever the syntax (`yaml.rs`);
//! `schema.rs` reads a response schema into a shape.

mod asyncapi;
mod openapi;
mod schema;
mod yaml;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::index::Export;
use crate::msg::Msg;
use crate::shape::Shape;
use crate::workspace::Repository;
use crate::{channel, http};

/// The largest document read; a larger one is unreadable. The largest public single-file
/// descriptions are 10 to 15 MB (GitHub's REST API, Stripe's SDK spec), far past the manifests'
/// `package::MAX_BYTES`.
// ponytail: a flat 16 MiB, parsed whole in memory; a streaming reader that keeps only the
// paths, servers and channels lifts it.
pub(crate) const MAX_BYTES: usize = 16 << 20;

/// Whether `path` (repository-relative) is a document by its file name: `openapi.yaml`,
/// `openapi.yml`, `openapi.json`, or a name ending in `.openapi.` and one of those extensions;
/// the same with `asyncapi`.
pub fn is_document(path: &str) -> bool {
    named(path, "openapi") || named(path, "asyncapi")
}

/// Whether the file name of `path` is `<format>.<ext>` or `<name>.<format>.<ext>`, with `ext`
/// `yaml`, `yml` or `json`.
fn named(path: &str, format: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(ext, "yaml" | "yml" | "json")
        && (stem == format || stem.strip_suffix(format).is_some_and(|s| s.ends_with('.')))
}

/// The kind of a document read, named by its top-level field: `openapi` or `asyncapi`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Openapi,
    Asyncapi,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Openapi => "openapi",
            Format::Asyncapi => "asyncapi",
        }
    }
}

/// What Meada read from one document. Nothing else of the file is kept.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Summary {
    /// For a document read; `None` when it was ignored (see `problems`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `info.title`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Sorted by path, then method, each once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
    /// The root `servers` URLs that name a host or a server path, sorted, each once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Server>,
    /// The channel sides an AsyncAPI document lists, sorted by broker kind, name and role, each
    /// once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<Side>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Problem>,
}

impl Summary {
    /// What `search` shows of a document read: `openapi 3.1.0, routes: 3`, or `asyncapi 3.0.0,
    /// channels: 2`. Nothing for one ignored.
    pub fn shown(&self) -> Option<String> {
        let (format, version) = (self.format?, self.version.as_ref()?);
        let counted = match format {
            Format::Asyncapi => "channels",
            Format::Openapi => "routes",
        };
        let count = self.count(format);
        Some(format!("{} {version}, {counted}: {count}", format.name()))
    }

    /// How many routes an OpenAPI document lists, or channels an AsyncAPI one.
    pub fn count(&self, format: Format) -> usize {
        match format {
            Format::Openapi => self.routes.len(),
            Format::Asyncapi => self.channel_count(),
        }
    }

    /// How many channels the sides are of: a channel both sent to and received from counts once.
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

/// What a root `servers` URL gives: the host it is served on (lower case, as a consumer's URL
/// names it) and its server path (normalized, no trailing `/`, empty for none). A relative URL
/// has no host.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Server {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
}

/// One side of a channel an AsyncAPI document lists: a producer or a consumer of the channel
/// `name` of the broker kind `broker`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Side {
    pub broker: String,
    pub name: String,
    pub role: Role,
    /// What its messages carry (`asyncapi.rs`), when the document says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

/// Which way the messages of a channel side go.
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

    /// `kafka send orders.created`: the broker kind, the verb of the document's operation and
    /// the channel. It ends the site of a side only a document lists.
    pub fn label(&self) -> String {
        let verb = match self.role {
            Role::Producer => "send",
            Role::Consumer => "receive",
        };
        format!("{} {verb} {}", self.broker, self.name)
    }
}

/// One path and method of a document; `path` is normalized like a code path.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Route {
    pub path: String,
    /// Upper case: `GET`.
    pub method: String,
    /// What its 2xx JSON response carries (`openapi.rs`), when the document says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
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
    /// It parses, but is not AsyncAPI 2.x or 3.0; `found` is what it says it is, such as
    /// `asyncapi 1.2.0`.
    AsyncFormat { found: Option<String> },
    /// A channel on a server whose protocol Meada has no broker kind for: that channel is
    /// ignored. Each protocol once, sorted.
    Protocol { protocol: String },
    /// A channel name with parameters, ignored. Each once, sorted.
    Parameters { channel: String },
    /// An AsyncAPI document with channels and no root `servers`: no channel has a broker kind,
    /// so every one is ignored.
    NoServers,
    /// A server a channel uses has no `protocol`: the channel is ignored on it. Each once,
    /// sorted.
    NoProtocol { server: String },
    /// A channel lists a server the document does not have: ignored on it. Each pair once,
    /// sorted.
    UnknownServer { channel: String, server: String },
    /// It parses, but is not OpenAPI 3.0 or 3.1; `found` is what it says it is, such as
    /// `swagger 2.0`.
    Format { found: Option<String> },
    /// A `$ref` to another file or a URL in a response schema: not followed, so the part of
    /// the shape behind it is open. Each reference once, sorted.
    ExternalRef { reference: String },
    /// A path item that is a `$ref` to another file or a URL: not followed, so the routes
    /// behind it are not read. Each reference once, sorted.
    ExternalPathItem { reference: String },
}

/// The summary of the document at `path` (repository-relative) holding `src`. JSON by the
/// extension, YAML otherwise. It never fails: what cannot be read is a problem, and a document
/// larger than `MAX_BYTES` is unreadable at its first line whatever the rest holds.
///
/// The content decides the format, since a listed document can have any name: a top-level
/// `openapi` field is OpenAPI, an `asyncapi` one AsyncAPI. With neither, the name only picks
/// which format the document is reported as not being.
pub fn read(path: &str, src: &[u8]) -> Summary {
    if src.len() > MAX_BYTES {
        let error = format!("larger than {} MiB", MAX_BYTES >> 20);
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

/// The status lines of `repo` (its index content `export`) about its documents, in path
/// order: each one read, each one ignored with why, and each route a later document lists
/// again; then each path `[serve] documents` lists that has no document.
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
    use super::{Format, MAX_BYTES, Problem, is_document, read};

    #[test]
    fn a_document_past_the_size_limit_is_unreadable() {
        let valid = "openapi: 3.1.0\npaths:\n  /a:\n    get: {}\n";
        let padded = |len: usize| format!("{valid}{}", "#".repeat(len - valid.len()));

        let at_limit = read("openapi.yaml", padded(MAX_BYTES).as_bytes());
        let past = read("openapi.yaml", padded(MAX_BYTES + 1).as_bytes());

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
        // Neither field: the name picks the message, OpenAPI's for any other name.
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
