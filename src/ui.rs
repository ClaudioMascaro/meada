//! The management console's process (ADR 0014, feature `ui`): `meada ui --listen <addr>
//! --workspace <bootstrap manifest>` serves the front end's files from `[ui] dir` and a read-only
//! JSON API under `/api/v1/`, on the listener `meada mcp --listen` uses (`mcp::serve_http`). It is
//! never a node's route and never an MCP server. Every request goes through, in order, the `Host`
//! check, the `Origin` check, admission (the operator's proxy: every request is a viewer's) and
//! routing. No answer sets a cookie or carries an `Access-Control-*` header.

use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper::{Method, Request, Response, StatusCode, header};
use percent_encoding::percent_decode_str;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Result;
use crate::modules::{self, Problem, Shape};
use crate::msg::Msg;
use crate::workspace::Manifest;

type Answer = Response<BoxBody<Bytes, Infallible>>;

/// The version the front end's files must be built for.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The file at the root of `[ui] dir` naming the version the front end was built for:
/// `{"version": "<Meada version>"}`.
const VERSION_FILE: &str = "meada-ui.json";

// ponytail: the most bytes of `meada-ui.json`; past it the file counts as another version.
const VERSION_MAX: u64 = 4096;

/// The content security policy of every answer.
const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; \
                   frame-ancestors 'none'";

/// The fixed bodies of the 403s.
const HOST_REFUSED: &str = "Forbidden: Host header is not allowed\n";
const ORIGIN_REFUSED: &str = "Forbidden: Origin header is not allowed\n";
const WRITE_REFUSED: &str = "Forbidden: a write needs an Origin header\n";

/// `[ui]` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    // Required, but checked by `validate`, so that both missing keys are named at once.
    dir: Option<String>,
    public_url: Option<String>,
    audience: Option<String>,
    client_id: Option<String>,
    #[serde(default)]
    operations: bool,
    operator_groups: Option<Vec<String>>,
    operator_claim: Option<String>,
}

/// The validated section. `client_id` is read here and checked with the console's admission
/// (#306), `operations` and the operator keys with the operations (#307).
#[derive(Clone, Debug)]
pub struct Config {
    /// The front end's files.
    pub dir: PathBuf,
    /// The console's own origin, with no trailing slash.
    pub public_url: String,
    /// The `aud` the console's tokens carry: `public_url` by default.
    pub audience: String,
    pub client_id: Option<String>,
    pub operations: bool,
    pub operator_groups: Option<Vec<String>>,
    pub operator_claim: Option<String>,
}

impl File {
    /// The section checked, every problem at once; `dir` is relative to `base`.
    fn validate(self, base: &Path) -> std::result::Result<Config, Vec<Problem>> {
        let mut problems = Vec::new();
        if self.dir.is_none() {
            problems.push(("ui", Msg::KeyMissing("ui.dir").text()));
        }
        match &self.public_url {
            None => problems.push(("ui", Msg::KeyMissing("ui.public_url").text())),
            Some(url) if !modules::reachable(url, Shape::Bare) => {
                problems.push(("ui.public_url", Msg::OidcUrl { bare: true }.text()))
            }
            Some(_) => {}
        }
        let (Some(dir), Some(public_url)) = (self.dir, self.public_url) else {
            return Err(problems);
        };
        if !problems.is_empty() {
            return Err(problems);
        }
        let public_url = public_url.trim_end_matches('/').to_string();
        Ok(Config {
            dir: base.join(dir),
            audience: self.audience.unwrap_or_else(|| public_url.clone()),
            public_url,
            client_id: self.client_id,
            operations: self.operations,
            operator_groups: self.operator_groups,
            operator_claim: self.operator_claim,
        })
    }
}

/// `[ui]` of `src`, the content of `file`, checked when the manifest has it: each problem on a
/// line naming the file, the line and the key.
pub(crate) fn validated(
    file: &Path,
    src: &str,
    base: &Path,
    section: Option<File>,
) -> Result<Option<Config>> {
    let Some(section) = section else {
        return Ok(None);
    };
    section.validate(base).map(Some).map_err(|problems| {
        let lines: Vec<String> = (problems.iter())
            .map(|(key, problem)| crate::config::error_at(file, src, key, problem).to_string())
            .collect();
        lines.join("\n").into()
    })
}

/// The console: what it started with.
struct Console {
    /// The bootstrap manifest's path.
    file: PathBuf,
    ui: Config,
    /// `[ui] dir`, resolved: no file outside it is served.
    root: PathBuf,
    /// The bootstrap manifest as loaded at start, a JSON table. It names secrets' variables and
    /// files only: a section holds no secret (ADR 0012).
    loaded: Value,
}

/// `meada ui`: refuses to start, listing every problem, when the bootstrap manifest at
/// `manifest` has errors or no `[ui]`, when `[ui] dir` is missing or built for another Meada
/// version, or with `[oidc]` (until the console admits tokens, #306). Then serves at `listen`
/// (`host:port`, `:port` or `port`, the loopback interface without a host) until SIGINT or
/// SIGTERM, as `meada mcp --listen` does.
pub fn listen(listen: &str, manifest: &Path) -> Result<()> {
    let address = crate::mcp::address(listen)?;
    let console = Arc::new(Console::start(manifest)?);
    crate::mcp::serve_http(address, move |bound, _| {
        move |request: Request<hyper::body::Incoming>| {
            let console = console.clone();
            async move { tokio::task::block_in_place(|| console.answer(bound, &request)) }
        }
    })
}

impl Console {
    fn start(file: &Path) -> Result<Self> {
        let read = Manifest::read(file)?;
        let Some(ui) = read.ui else {
            return Err(format!("{}: {}", file.display(), Msg::UiNoSection.text()).into());
        };
        let src = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let at = |key: &str, problem: String| crate::config::error_at(file, &src, key, &problem);
        let mut problems = Vec::new();
        #[cfg(feature = "oidc")]
        if read.oidc.is_some() {
            problems.push(at("oidc", Msg::UiOidc.text()));
        }
        let dir = ui.dir.display().to_string();
        let root = (ui.dir.canonicalize().ok()).filter(|root| root.is_dir());
        match &root {
            None => problems.push(at("ui.dir", Msg::UiDirMissing(&dir).text())),
            Some(root) => {
                let found = built_for(root);
                if found.as_deref() != Some(VERSION) {
                    let found = found.as_deref();
                    let expected = VERSION;
                    problems.push(at(
                        "ui.dir",
                        Msg::UiVersion {
                            dir: &dir,
                            found,
                            expected,
                        }
                        .text(),
                    ));
                }
            }
        }
        let (Some(root), true) = (root, problems.is_empty()) else {
            let lines: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
            return Err(lines.join("\n").into());
        };
        let table: toml::Table = toml::from_str(&src)?;
        Ok(Self {
            file: file.to_path_buf(),
            ui,
            root,
            loaded: serde_json::to_value(table)?,
        })
    }

    /// The answer to `request`, the listener bound at `bound`.
    fn answer<B>(&self, bound: SocketAddr, request: &Request<B>) -> Answer {
        let mut answer = self.checked(bound, request);
        let headers = answer.headers_mut();
        headers.insert("x-content-type-options", "nosniff".parse().unwrap());
        headers.insert("referrer-policy", "no-referrer".parse().unwrap());
        headers.insert("content-security-policy", CSP.parse().unwrap());
        answer
    }

    fn checked<B>(&self, bound: SocketAddr, request: &Request<B>) -> Answer {
        let headers = request.headers();
        let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
        if !host.is_some_and(|host| self.hosts(host, bound)) {
            return plain(StatusCode::FORBIDDEN, HOST_REFUSED);
        }
        let write = !matches!(*request.method(), Method::GET | Method::HEAD);
        match headers
            .get(header::ORIGIN)
            .map(|o| o.to_str().unwrap_or(""))
        {
            Some(origin) if !origin.eq_ignore_ascii_case(&self.ui.public_url) => {
                return plain(StatusCode::FORBIDDEN, ORIGIN_REFUSED);
            }
            None if write => return plain(StatusCode::FORBIDDEN, WRITE_REFUSED),
            _ => {}
        }
        // Admission: the operator's proxy admits; every request is a viewer's.
        let path = request.uri().path();
        match path.strip_prefix("/api/v1/") {
            Some(route) => self.api(write, route),
            None if write => plain(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed\n"),
            None => self.file(path),
        }
    }

    /// Whether `host`, a request's `Host`, is `public_url`'s host and port, or the bound address
    /// on loopback.
    fn hosts(&self, host: &str, bound: SocketAddr) -> bool {
        let public = (self.ui.public_url.split_once("://")).map_or("", |(_, authority)| authority);
        host.eq_ignore_ascii_case(public) || (bound.ip().is_loopback() && host == bound.to_string())
    }

    /// The API's answer at `/api/v1/<route>`.
    fn api(&self, write: bool, route: &str) -> Answer {
        let document = match route {
            _ if write => None,
            "config" => Some(json!({"version": VERSION, "operations": self.ui.operations})),
            "session" => Some(json!({"subject": null, "role": "viewer"})),
            "manifest" => Some(self.manifest()),
            _ => None,
        };
        match document {
            Some(document) => json_answer(StatusCode::OK, &document),
            None => {
                let message = Msg::UiNoRoute.text();
                let error = json!({"error": {"code": "not_found", "message": message}});
                json_answer(StatusCode::NOT_FOUND, &error)
            }
        }
    }

    /// The bootstrap manifest as loaded at start, the module sections the build has, and the
    /// errors of the file as it now stands on disk, read again: a mounted file changed since the
    /// start shows its problems before a restart.
    fn manifest(&self) -> Value {
        let errors: Vec<String> = match Manifest::read(&self.file) {
            Ok(_) => Vec::new(),
            Err(e) => e.to_string().lines().map(String::from).collect(),
        };
        let modules: Vec<Value> = (modules::MODULES.iter())
            .map(|m| json!({"section": m.section, "feature": m.feature, "built": m.built}))
            .collect();
        json!({
            "file": self.file.display().to_string(),
            "manifest": self.loaded,
            "modules": modules,
            "errors": errors,
        })
    }

    /// The file of `[ui] dir` that `path` names: percent-decoded, a 404 for a `..`, `.` or empty
    /// segment, a path resolving outside the directory (a symbolic link included) or an
    /// extension outside the table. A path that names no file (a directory included: none is
    /// listed) answers `index.html`, unless it is under `/api/`.
    fn file(&self, path: &str) -> Answer {
        let Some(relative) = relative(path) else {
            return not_found();
        };
        match self.root.join(&relative).canonicalize() {
            Ok(real) if !real.starts_with(&self.root) => not_found(),
            Ok(real) if real.is_file() => served(&real, relative.starts_with("assets/")),
            _ if path.starts_with("/api/") => not_found(),
            _ => match self.root.join("index.html").canonicalize() {
                Ok(real) if real.starts_with(&self.root) && real.is_file() => served(&real, false),
                _ => not_found(),
            },
        }
    }
}

/// The version `meada-ui.json` at `root` names; `None` when it is missing, oversized or not
/// that document.
fn built_for(root: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Built {
        version: String,
    }
    let bytes = crate::config::read_bounded(&root.join(VERSION_FILE), VERSION_MAX).ok()?;
    Some(serde_json::from_slice::<Built>(&bytes).ok()?.version)
}

/// The path of `[ui] dir` a request path names, decoded: `index.html` for `/`, `None` for a
/// path with a `..`, `.` or empty segment, a NUL or a backslash.
fn relative(path: &str) -> Option<String> {
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    let rest = decoded.strip_prefix('/')?;
    if rest.is_empty() {
        return Some("index.html".into());
    }
    let safe = |s: &str| !matches!(s, "" | "." | "..") && !s.contains(['\0', '\\']);
    rest.split('/').all(safe).then(|| rest.to_string())
}

/// The content type of a file with extension `extension`: the fixed table, else none.
fn content_type(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => return None,
    })
}

/// The file at `real`, inside `[ui] dir`: cacheable for a year and immutable when `immutable`
/// (the front end's content-hashed files under `assets/`), else `no-cache`.
fn served(real: &Path, immutable: bool) -> Answer {
    let extension = real.extension().and_then(|e| e.to_str()).unwrap_or("");
    let (Some(kind), Ok(bytes)) = (content_type(extension), fs::read(real)) else {
        return not_found();
    };
    let cache = match immutable {
        true => "public, max-age=31536000, immutable",
        false => "no-cache",
    };
    Response::builder()
        .header(header::CONTENT_TYPE, kind)
        .header(header::CACHE_CONTROL, cache)
        .body(Full::from(bytes).boxed())
        .expect("a valid response")
}

fn plain(status: StatusCode, body: &'static str) -> Answer {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Full::from(body).boxed())
        .expect("a valid response")
}

fn not_found() -> Answer {
    plain(StatusCode::NOT_FOUND, "Not Found\n")
}

fn json_answer(status: StatusCode, document: &Value) -> Answer {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Full::from(document.to_string()).boxed())
        .expect("a valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_path_is_decoded_and_refused_with_a_dot_dot_dot_or_empty_segment() {
        assert_eq!(relative("/"), Some("index.html".into()));
        assert_eq!(relative("/assets/app.js"), Some("assets/app.js".into()));
        assert_eq!(relative("/caf%C3%A9.txt"), Some("café.txt".into()));
        for bad in [
            "/..",
            "/%2e%2e/x",
            "/a/../b",
            "/a//b",
            "/assets/",
            "/./x",
            "/a%2F..%2Fb",
            "/a%00b",
            "/a%5c..",
            "/%ff",
            "x",
        ] {
            assert_eq!(relative(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_the_table_s_extensions_have_a_content_type() {
        for ext in [
            "html", "js", "mjs", "css", "wasm", "json", "svg", "png", "ico", "woff2", "txt", "map",
        ] {
            assert!(content_type(ext).is_some(), "{ext}");
        }
        for ext in ["", "exe", "sh", "HTML", "toml"] {
            assert!(content_type(ext).is_none(), "{ext}");
        }
    }
}
