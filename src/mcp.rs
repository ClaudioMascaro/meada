//! The MCP server: a thin adapter over the query engine (ADR 0008), over two transports that
//! share one tool definition. No tool writes. Over stdio (`serve`) it answers about the
//! client's first root, and the only write is the index refresh every query does first;
//! stdout is the protocol channel, so anything else goes to stderr. Over streamable HTTP
//! (`listen`, server mode, ADR 0011) it serves one endpoint per repository of a workspace
//! manifest and only reads.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode, header, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use percent_encoding::percent_decode_str;
use rmcp::{
    RoleServer, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerConfig},
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::engine::{self, Direction, Engine, Start};

#[derive(Deserialize, JsonSchema)]
struct SearchArgs {
    /// Name to look for, case-insensitive.
    query: String,
    /// Only this kind: `symbol`, `contract`, `route` (an HTTP contract), `resource` (a
    /// declared one, matched by kind and id), `channel` (a queue or topic, by broker kind and
    /// name), `document` (an OpenAPI or AsyncAPI file, by path or title), `package` (by name,
    /// with its consumers), `drift` (fields consumers read that a route lacks, by route or field
    /// path), `community` (by name), `process` (by its entry point's label, with the
    /// `process::` identity `trace` starts from), or `ambiguous` or `unresolved` (open items,
    /// with the `meada decide` command for each).
    kind: Option<String>,
    /// Token budget for the answer (default 2000).
    budget: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct ContextArgs {
    /// Symbol name, `<path>::<name>` or full identity; a resource, `resource::<kind>::<id>`; or a channel, `channel::<broker>::<name>`.
    symbol: String,
    /// Token budget for the answer (default 2000).
    budget: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct TraceArgs {
    /// Start: a symbol, a route (`GET /users/{}` or its contract), a call site, or a process
    /// (`process::<entry point identity>`, down only).
    from: Option<String>,
    /// `uncommitted`: start from the symbols the working-tree diff touches instead.
    changes: Option<String>,
    /// `down` (default: what it reaches) or `up` (what reaches it).
    direction: Option<String>,
    /// Token budget for the answer (default 2000).
    budget: Option<u32>,
}

#[derive(Clone)]
struct Meada {
    tool_router: ToolRouter<Self>,
    source: Source,
}

/// Where a call's engine opens.
#[derive(Clone)]
enum Source {
    /// The client's first root, else the current directory (stdio).
    Roots,
    /// The workspace of a manifest, `identity` first (server mode): read-only, one engine per
    /// call, and no request to the client.
    Served {
        manifest: Arc<Path>,
        identity: Arc<str>,
    },
}

impl Meada {
    /// Opens the engine where `source` says and runs `ask`; over stdio it refreshes the index
    /// first. The engine blocks (SQLite, and a read that meets a commit waits up to the busy
    /// timeout), so it runs where the runtime expects blocking work.
    async fn answer(
        &self,
        ctx: RequestContext<RoleServer>,
        ask: impl FnOnce(&Engine) -> crate::Result<String>,
    ) -> Result<String, String> {
        let open = match &self.source {
            Source::Roots => {
                let dir = root_dir(&ctx).await;
                tokio::task::block_in_place(|| Engine::open(&dir))
            }
            Source::Served { manifest, identity } => {
                tokio::task::block_in_place(|| Engine::open_manifest(manifest, identity))
            }
        };
        tokio::task::block_in_place(|| open.and_then(|engine| ask(&engine)))
            .map_err(|e| e.to_string())
    }
}

/// The directory to answer about: the client's first `file://` root, or the server's current
/// directory. Asked on every call, so `roots/list_changed` needs no cache to invalidate.
#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577 and still what Claude Code sends; the cwd fallback keeps the server working once clients drop them"
)]
async fn root_dir(ctx: &RequestContext<RoleServer>) -> PathBuf {
    let cwd = || PathBuf::from(".");
    if ctx.client_capabilities().is_none_or(|c| c.roots.is_none()) {
        return cwd();
    }
    match ctx.peer.list_roots().await {
        Ok(result) => result
            .roots
            .iter()
            .find_map(|r| file_path(&r.uri))
            .unwrap_or_else(cwd),
        Err(_) => cwd(),
    }
}

/// The absolute path of a `file://` URI, percent-decoded; `None` for any other scheme and for
/// a URI with a host (`file://host/path`), which names no local directory.
fn file_path(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file://").filter(|p| p.starts_with('/'))?;
    Some(PathBuf::from(
        percent_decode_str(path).decode_utf8().ok()?.into_owned(),
    ))
}

#[tool_router]
impl Meada {
    fn new(source: Source) -> Self {
        Self {
            tool_router: Self::tool_router(),
            source,
        }
    }

    #[tool(
        description = "Find symbols by name and contracts by identifier (`http::GET::/users/{}`). One line each: repository, path:line, name."
    )]
    async fn search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<String, String> {
        self.answer(ctx, |engine| {
            let budget = args.budget.map(|b| b as usize);
            engine.search(&args.query, args.kind.as_deref(), budget)
        })
        .await
    }

    #[tool(
        description = "Show one symbol: its community, the processes it belongs to, its source, callers and callees."
    )]
    async fn context(
        &self,
        Parameters(args): Parameters<ContextArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<String, String> {
        self.answer(ctx, |engine| {
            engine.context(&args.symbol, args.budget.map(|b| b as usize))
        })
        .await
    }

    #[tool(
        description = "Follow a chain through calls and cross-repository links, down or up, until it ends. Links show →[level: reason]; a footer sums it up."
    )]
    async fn trace(
        &self,
        Parameters(args): Parameters<TraceArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<String, String> {
        let start = match (args.from.as_deref(), args.changes.as_deref()) {
            (Some(from), None) => Start::From(from),
            (None, Some("uncommitted")) => Start::Uncommitted,
            (None, Some(other)) => {
                return Err(format!("`changes` takes `uncommitted`, not `{other}`"));
            }
            _ => return Err("pass either `from` or `changes: uncommitted`".into()),
        };
        if matches!(start, Start::Uncommitted) && matches!(self.source, Source::Served { .. }) {
            return Err(
                "server mode has no working tree to diff: run Meada locally for \
                        `changes: uncommitted`"
                    .into(),
            );
        }
        let direction = match args.direction.as_deref() {
            None | Some("down") => Direction::Down,
            Some("up") => Direction::Up,
            Some(other) => return Err(format!("`direction` takes `down` or `up`, not `{other}`")),
        };
        self.answer(ctx, |engine| {
            engine.trace(start, direction, args.budget.map(|b| b as usize))
        })
        .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Meada {
    fn get_info(&self) -> ServerConfig {
        let instructions = match &self.source {
            Source::Roots => "Code graph of the current repository.".to_string(),
            Source::Served { identity, .. } => format!("Code graph of the repository {identity}."),
        };
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("meada", env!("CARGO_PKG_VERSION")))
            .with_instructions(instructions)
    }
}

/// Serves the tools over stdio until the client disconnects.
pub fn serve() -> crate::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        Meada::new(Source::Roots)
            .serve(rmcp::transport::stdio())
            .await?
            .waiting()
            .await?;
        Ok(())
    })
}

/// One repository's MCP endpoint.
type Endpoint = StreamableHttpService<Meada, NeverSessionManager>;

/// Server mode (ADR 0011): serves the tools over MCP streamable HTTP at `listen` (`host:port`,
/// `:port` or `port`, the loopback interface without a host), one endpoint per repository the
/// manifest at `manifest` lists, at `/<identity>`. Refuses to start unless every repository is
/// servable (`engine::served`). The first line on stderr is the bound address.
///
/// Every POST stands alone (stateless, JSON responses), so any of several servers over the
/// same indexes answers any call. The `Host` header is not checked, since a reverse proxy
/// forwards its own hostname; a request with an `Origin` header (a browser, as on a DNS
/// rebinding page) is refused instead, since MCP clients send none. Meada authenticates
/// nobody: the operator's proxy admits clients.
pub fn listen(listen: &str, manifest: &Path) -> crate::Result<()> {
    let address = address(listen)?;
    let served = engine::served(manifest)?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let listener = tokio::net::TcpListener::bind(address).await?;
        eprintln!("listening on http://{}", listener.local_addr()?);
        // Cancelling its `cancellation_token` ends the calls in flight: the stop on a signal.
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .disable_allowed_hosts()
            .enforce_origin_validation();
        let manifest: Arc<Path> = manifest.into();
        let endpoints: HashMap<String, Endpoint> = (served.iter())
            .map(|identity| {
                let meada = Meada::new(Source::Served {
                    manifest: manifest.clone(),
                    identity: identity.as_str().into(),
                });
                let endpoint = StreamableHttpService::new(
                    move || Ok(meada.clone()),
                    Default::default(),
                    config.clone(),
                );
                (identity.clone(), endpoint)
            })
            .collect();
        let listing: String = served
            .iter()
            .map(|identity| format!("{identity}\n"))
            .collect();
        let routes = Arc::new((endpoints, listing));
        loop {
            let (stream, _) = listener.accept().await?;
            let routes = routes.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request| {
                    let routes = routes.clone();
                    async move { Ok::<_, Infallible>(route(&routes.0, &routes.1, request).await) }
                });
                // A connection that breaks ends only itself.
                let _ = (http1::Builder::new())
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    })
}

/// The endpoint `request`'s path names (`/<identity>`, a trailing slash tolerated) answers it;
/// any other path, the root included, is a 404 listing the identities served, one per line.
async fn route(
    endpoints: &HashMap<String, Endpoint>,
    listing: &str,
    request: Request<Incoming>,
) -> Response<BoxBody<Bytes, Infallible>> {
    let path = request.uri().path();
    let identity = path.strip_prefix('/').unwrap_or(path);
    let identity = identity.strip_suffix('/').unwrap_or(identity);
    match endpoints.get(identity) {
        Some(endpoint) => endpoint.handle(request).await,
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Full::from(listing.to_string()).boxed())
            .expect("a valid response"),
    }
}

/// The address `--listen` names: `host:port`, `:port` or `port`, the host an IP address or
/// `localhost`; without one, the loopback interface. No name is resolved.
fn address(listen: &str) -> crate::Result<SocketAddr> {
    let invalid = || format!("`--listen {listen}`: pass host:port, :port or port");
    let (host, port) = listen.rsplit_once(':').unwrap_or(("", listen));
    let port: u16 = port.parse().map_err(|_| invalid())?;
    let host = (host.strip_prefix('['))
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    let ip = match host {
        "" | "localhost" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        host => host.parse().map_err(|_| invalid())?,
    };
    Ok(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `search` documentation is a literal: the tool list is a token-limited contract, and
    /// a derived text would change its lines. A new kind, or an engine word, fails here until
    /// its word is added.
    #[test]
    fn the_search_documentation_names_every_kind_word() {
        let schema = serde_json::to_string(&schemars::schema_for!(SearchArgs)).unwrap();
        for word in crate::engine::kind_words() {
            assert!(schema.contains(&format!("`{word}`")), "`{word}` is missing");
        }
    }

    #[test]
    fn listen_takes_host_port_colon_port_or_port_and_defaults_to_loopback() {
        let at = |s: &str| address(s).map(|a| a.to_string()).map_err(|e| e.to_string());
        assert_eq!(at("7333"), Ok("127.0.0.1:7333".into()));
        assert_eq!(at(":0"), Ok("127.0.0.1:0".into()));
        assert_eq!(at("localhost:7333"), Ok("127.0.0.1:7333".into()));
        assert_eq!(at("0.0.0.0:7333"), Ok("0.0.0.0:7333".into()));
        assert_eq!(at("[::1]:7333"), Ok("[::1]:7333".into()));
        for bad in ["", "host.example:7333", "127.0.0.1", "1:2:3", ":70000"] {
            assert!(at(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn file_uri_decodes_percent_encoding_and_rejects_other_schemes() {
        assert_eq!(
            file_path("file:///tmp/my%20repo/caf%C3%A9"),
            Some(PathBuf::from("/tmp/my repo/café"))
        );
        assert_eq!(file_path("https://example.com/x"), None);
        assert_eq!(file_path("file://localhost/x"), None);
    }
}
