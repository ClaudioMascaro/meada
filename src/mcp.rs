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
use std::time::Duration;

use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode, header, server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
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
use crate::workspace::Manifest;

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
    /// The workspace of the manifest at `manifest`, as `listed` read it at start, `identity`
    /// first (server mode): read-only, one engine per call, and no request to the client.
    Served {
        listed: Arc<Manifest>,
        manifest: Arc<Path>,
        identity: Arc<str>,
    },
    /// The generation `<local_dir>/current` names when the call arrives, `identity` first (server
    /// mode with `--generations`, feature `snapshot`): as `Served`, with the repositories the
    /// generation recorded.
    #[cfg(feature = "snapshot")]
    Generation {
        local_dir: Arc<Path>,
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
            Source::Served {
                listed,
                manifest,
                identity,
            } => tokio::task::block_in_place(|| Engine::open_listed(listed, identity, manifest)),
            #[cfg(feature = "snapshot")]
            Source::Generation {
                local_dir,
                identity,
            } => tokio::task::block_in_place(|| Engine::open_generation(local_dir, identity)),
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
        if matches!(start, Start::Uncommitted) && !matches!(self.source, Source::Roots) {
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
            #[cfg(feature = "snapshot")]
            Source::Generation { identity, .. } => {
                format!("Code graph of the repository {identity}.")
            }
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

/// How long the server waits, after a stop signal, for the answers in flight to finish before
/// it drops what is left. Under Kubernetes' default grace period of 30 seconds.
const DRAIN: Duration = Duration::from_secs(25);

thread_local! {
    static DRAIN_BOUND: std::cell::Cell<Duration> = const { std::cell::Cell::new(DRAIN) };
}

/// Runs `f` with `listen` on this thread draining for at most `bound` instead of 25 seconds.
/// No option or setting reaches it: tests, through `MEADA_DRAIN_MS` in `main`, debug builds only.
#[doc(hidden)]
pub fn with_drain_bound<T>(bound: Duration, f: impl FnOnce() -> T) -> T {
    struct Restore(Duration);
    impl Drop for Restore {
        fn drop(&mut self) {
            DRAIN_BOUND.set(self.0);
        }
    }
    let _restore = Restore(DRAIN_BOUND.replace(bound));
    f()
}

/// One repository's MCP endpoint.
type Endpoint = StreamableHttpService<Meada, NeverSessionManager>;

/// The endpoints a server answers at.
enum Routes {
    /// One per repository of the manifest, fixed at start, and the 404's body.
    Listed {
        endpoints: HashMap<String, Endpoint>,
        listing: String,
    },
    /// One per repository of the generation current when the request arrives, made then: a
    /// pull that adds a repository needs no restart.
    #[cfg(feature = "snapshot")]
    Generation {
        local_dir: Arc<Path>,
        config: StreamableHttpServerConfig,
    },
}

/// The MCP service of `source`, stateless with JSON responses.
fn endpoint(source: Source, config: &StreamableHttpServerConfig) -> Endpoint {
    let meada = Meada::new(source);
    StreamableHttpService::new(
        move || Ok(meada.clone()),
        Default::default(),
        config.clone(),
    )
}

/// Server mode (ADR 0011): serves the tools over MCP streamable HTTP at `listen` (`host:port`,
/// `:port` or `port`, the loopback interface without a host), one endpoint per repository the
/// manifest at `manifest` lists, at `/<identity>`. Refuses to start unless every repository is
/// servable (`engine::served`). The first line on stderr is the bound address.
///
/// Every POST stands alone (stateless, JSON responses), so any of several servers over the
/// same indexes answers any call. The `Host` header is not checked, since a reverse proxy
/// forwards its own hostname; a request with an `Origin` header (a browser, as on a DNS
/// rebinding page) is refused instead, since MCP clients send none. Meada authenticates
/// nobody: the operator's proxy admits clients. On SIGINT or SIGTERM it stops accepting, lets the
/// answers in flight finish for at most `DRAIN` (a second signal ends the wait), drops what
/// is left and returns.
pub fn listen(listen: &str, manifest: &Path) -> crate::Result<()> {
    let address = address(listen)?;
    let listed = Arc::new(engine::served(manifest)?);
    let served: Vec<String> = listed.entries.iter().map(|l| l.repo.clone()).collect();
    let manifest: Arc<Path> = manifest.into();
    run(address, move |config| {
        let endpoints: HashMap<String, Endpoint> = (served.iter())
            .map(|identity| {
                let source = Source::Served {
                    listed: listed.clone(),
                    manifest: manifest.clone(),
                    identity: identity.as_str().into(),
                };
                (identity.clone(), endpoint(source, config))
            })
            .collect();
        let listing = served.iter().map(|i| format!("{i}\n")).collect();
        Routes::Listed { endpoints, listing }
    })
}

/// `listen` over the generations a node pulls (`--generations`, feature `snapshot`): refuses to
/// start unless the node's current generation is servable (`engine::served_generation`), and
/// answers every call from the generation current when it arrives.
#[cfg(feature = "snapshot")]
pub fn listen_generations(listen: &str, manifest: &Path) -> crate::Result<()> {
    let address = address(listen)?;
    let (local_dir, _) = engine::served_generation(manifest)?;
    let local_dir: Arc<Path> = local_dir.into();
    run(address, move |config| Routes::Generation {
        local_dir,
        config: config.clone(),
    })
}

/// The server over `address` with the endpoints `routes` makes from the transport's
/// configuration.
fn run(
    address: SocketAddr,
    routes: impl FnOnce(&StreamableHttpServerConfig) -> Routes,
) -> crate::Result<()> {
    let drain = DRAIN_BOUND.get();
    let runtime = tokio::runtime::Runtime::new()?;
    let stopped: crate::Result<()> = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(address).await?;
        // Before the address is printed: a signal sent on reading it is never lost.
        let mut stop = Stop::install()?;
        eprintln!("listening on http://{}", listener.local_addr()?);
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .disable_allowed_hosts()
            .enforce_origin_validation();
        let routes = Arc::new(routes(&config));
        let (stopping, _) = tokio::sync::watch::channel(());
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let accepted = tokio::select! {
                accepted = listener.accept() => accepted,
                () = stop.wait() => break,
            };
            let stream = match accepted {
                Ok((stream, _)) => stream,
                // Out of descriptors, say: the server goes on, once a few connections closed.
                Err(e) => {
                    eprintln!("meada: accepting a connection: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            };
            let routes = routes.clone();
            let mut stopping = stopping.subscribe();
            connections.spawn(async move {
                let service = service_fn(move |request| {
                    let routes = routes.clone();
                    async move { Ok::<_, Infallible>(route(&routes, request).await) }
                });
                // A connection that breaks ends only itself; the timer is what makes hyper's
                // header read timeout close one that sends nothing.
                let connection = (http1::Builder::new())
                    .timer(TokioTimer::new())
                    .serve_connection(TokioIo::new(stream), service);
                let mut connection = std::pin::pin!(connection);
                // On the stop, the answer in flight finishes and the connection then closes.
                tokio::select! {
                    _ = connection.as_mut() => return,
                    _ = stopping.changed() => connection.as_mut().graceful_shutdown(),
                }
                let _ = connection.await;
            });
        }
        // Stop accepting; hyper closes each connection once its answer is sent. rmcp's token
        // is cancelled only after the drain: cancelling it first ends the calls in flight with
        // an error instead of letting them finish.
        drop(listener);
        drop(stopping);
        // A second signal ends the drain at once.
        let drained = tokio::time::timeout(drain, async {
            while connections.join_next().await.is_some() {}
        });
        tokio::select! {
            drained = drained => if drained.is_err() {
                eprintln!("meada: dropping connections still open after {drain:?}");
            },
            () = stop.wait() => eprintln!("meada: second signal, dropping connections still open"),
        }
        config.cancellation_token.cancel();
        Ok(())
    });
    // An answer still waiting on a lock holds its thread past the drain, so the runtime is not
    // waited for: what is left dies with the process.
    runtime.shutdown_background();
    stopped
}

/// The stop signals: SIGINT and SIGTERM (Ctrl-C alone off Unix). Installed on `install`, so a
/// signal sent once the address is printed is never lost; `wait` resolves once per signal
/// (signals that arrive together count as one).
#[cfg(unix)]
struct Stop {
    term: tokio::signal::unix::Signal,
    int: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Stop {
    fn install() -> crate::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            term: signal(SignalKind::terminate())?,
            int: signal(SignalKind::interrupt())?,
        })
    }

    async fn wait(&mut self) {
        tokio::select! { _ = self.int.recv() => {}, _ = self.term.recv() => {} }
    }
}

#[cfg(not(unix))]
struct Stop;

#[cfg(not(unix))]
impl Stop {
    fn install() -> crate::Result<Self> {
        Ok(Self)
    }

    async fn wait(&mut self) {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// The endpoint `request`'s path names (`/<identity>`, percent-decoded, a trailing slash
/// tolerated) answers it; any other path, the root included, is a 404 listing the identities
/// served, one per line.
async fn route(
    routes: &Routes,
    request: Request<Incoming>,
) -> Response<BoxBody<Bytes, Infallible>> {
    let path = percent_decode_str(request.uri().path()).decode_utf8();
    let path = path.as_deref().unwrap_or("");
    let identity = path.strip_prefix('/').unwrap_or(path);
    let identity = identity.strip_suffix('/').unwrap_or(identity);
    let plain = |status, body: String| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Full::from(body).boxed())
            .expect("a valid response")
    };
    match routes {
        Routes::Listed { endpoints, listing } => match endpoints.get(identity) {
            Some(endpoint) => endpoint.handle(request).await,
            None => plain(StatusCode::NOT_FOUND, listing.clone()),
        },
        #[cfg(feature = "snapshot")]
        Routes::Generation { local_dir, config } => {
            let served: Result<Vec<String>, String> = tokio::task::block_in_place(|| {
                let held = crate::snapshot::serve::held(local_dir).map_err(|e| e.to_string())?;
                Ok(held.listed.entries.into_iter().map(|l| l.repo).collect())
            });
            match served {
                Ok(served) if served.iter().any(|repo| repo == identity) => {
                    let source = Source::Generation {
                        local_dir: local_dir.clone(),
                        identity: identity.into(),
                    };
                    endpoint(source, config).handle(request).await
                }
                Ok(served) => plain(
                    StatusCode::NOT_FOUND,
                    served.iter().map(|repo| format!("{repo}\n")).collect(),
                ),
                Err(problem) => plain(StatusCode::SERVICE_UNAVAILABLE, format!("{problem}\n")),
            }
        }
    }
}

/// The address `--listen` names: `host:port`, `:port` or `port`, the host an IP address or
/// `localhost`; without one, the loopback interface. No name is resolved.
fn address(listen: &str) -> crate::Result<SocketAddr> {
    let invalid = || {
        format!(
            "`--listen {listen}`: pass host:port, :port or port, the host an IP address or localhost"
        )
    };
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
