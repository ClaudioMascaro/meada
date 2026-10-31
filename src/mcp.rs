//! The MCP server: a thin adapter over the query engine (ADR 0008), over two transports that
//! share one tool definition. No tool writes. Over stdio (`serve`) it answers about the
//! client's first root (asked through a multi round-trip request, SEP-2322, on protocol
//! 2026-07-28 and later, else through `roots/list`), and the only write is the index refresh
//! every query does first; stdout is the protocol channel, so anything else goes to stderr.
//! Over streamable HTTP (`listen`, server mode, ADR 0011) it serves one endpoint per
//! repository of a workspace manifest and only reads.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode, header, server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use percent_encoding::percent_decode_str;
#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; still how Claude Code gives the session's directory (see `root_dir`)"
)]
use rmcp::model::{ListRootsRequest, ListRootsResult};
use rmcp::{
    RoleServer, ServerHandler, ServiceError, ServiceExt,
    handler::server::{router::tool::ToolRouter, tool::InputResponses, wrapper::Parameters},
    model::{
        CallToolResponse, CallToolResult, ClientResult, ContentBlock, Implementation, InputRequest,
        InputRequiredResult, ProtocolVersion, ServerCapabilities, ServerConfig, ServerRequest,
    },
    service::{PeerRequestOptions, RequestContext},
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::engine::{self, Direction, Engine, Start};
use crate::msg::Msg;
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
    /// `process::` identity `trace` starts from), `duplicate` (symbols with identical text in
    /// two or more places, by name or path), or `ambiguous` or `unresolved` (open items,
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

impl SearchArgs {
    fn ask(&self, engine: &Engine) -> crate::Result<String> {
        let budget = self.budget.map(|b| b as usize);
        engine.search(&self.query, self.kind.as_deref(), budget)
    }
}

impl ContextArgs {
    fn ask(&self, engine: &Engine) -> crate::Result<String> {
        engine.context(&self.symbol, self.budget.map(|b| b as usize))
    }
}

impl TraceArgs {
    /// Where the trace starts and which way it goes, or the tool's refusal; `served`: server
    /// mode, which has no working tree to diff.
    fn checked(&self, served: bool) -> Result<(Start<'_>, Direction), String> {
        let start = match (self.from.as_deref(), self.changes.as_deref()) {
            (Some(from), None) => Start::From(from),
            (None, Some("uncommitted")) => Start::Uncommitted,
            (None, Some(other)) => {
                return Err(format!("`changes` takes `uncommitted`, not `{other}`"));
            }
            _ => return Err("pass either `from` or `changes: uncommitted`".into()),
        };
        if matches!(start, Start::Uncommitted) && served {
            return Err(
                "server mode has no working tree to diff: run Meada locally for \
                        `changes: uncommitted`"
                    .into(),
            );
        }
        let direction = match self.direction.as_deref() {
            None | Some("down") => Direction::Down,
            Some("up") => Direction::Up,
            Some(other) => {
                return Err(format!("`direction` takes `down` or `up`, not `{other}`"));
            }
        };
        Ok((start, direction))
    }

    /// The trace `checked` let through.
    fn ask(&self, engine: &Engine, served: bool) -> crate::Result<String> {
        let (start, direction) = self.checked(served)?;
        engine.trace(start, direction, self.budget.map(|b| b as usize))
    }
}

/// A tool's call, its arguments checked, waiting for the engine it answers from.
#[cfg(feature = "ui")]
pub(crate) type Ask = Box<dyn FnOnce(&Engine) -> crate::Result<String>>;

/// A call of the tool `tool` with `arguments`, as a node's MCP surface takes and answers it, in
/// server mode (the console's playground, #305): `Err(None)` when no tool has that name or the
/// arguments are not the tool's, `Err(Some(text))` when the tool refuses them with the text of a
/// node's tool error. Nothing opens before the arguments pass.
#[cfg(feature = "ui")]
pub(crate) fn served_call(tool: &str, arguments: serde_json::Value) -> Result<Ask, Option<String>> {
    fn parsed<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T, Option<String>> {
        serde_json::from_value(v).map_err(|_| None)
    }
    Ok(match tool {
        "search" => {
            let args: SearchArgs = parsed(arguments)?;
            Box::new(move |engine| args.ask(engine))
        }
        "context" => {
            let args: ContextArgs = parsed(arguments)?;
            Box::new(move |engine| args.ask(engine))
        }
        "trace" => {
            let args: TraceArgs = parsed(arguments)?;
            args.checked(true).map_err(Some)?;
            Box::new(move |engine| args.ask(engine, true))
        }
        _ => return Err(None),
    })
}

/// How long a call waits for the client's `roots/list` answer: generous for a busy client, far
/// below the point where a client gives up on the call itself (#282).
const ROOTS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct Meada {
    tool_router: ToolRouter<Self>,
    source: Source,
}

/// Where a call's engine opens.
#[derive(Clone)]
enum Source {
    /// The client's first root, else the current directory (stdio). `last_root` is the last
    /// root this connection's client sent; clones share it.
    Roots {
        last_root: Arc<Mutex<Option<PathBuf>>>,
    },
    /// The workspace of the manifest at `manifest`, as `listed` read it at start, `identity`
    /// first (server mode): read-only, one engine per call, and no request to the client. A
    /// call reads only the repositories its request's `Pass` may read.
    Served {
        listed: Arc<Manifest>,
        manifest: Arc<Path>,
        identity: Arc<str>,
    },
    /// The generation `<local_dir>/current` named when the request was routed, or the one its
    /// path pinned, `identity` first (server mode with `--generations`, feature `snapshot`): as
    /// `Served`, with the repositories the generation recorded that the `Pass` may read. With
    /// `named` (the node has `[generations]`), each result names the generation in its `_meta`.
    #[cfg(feature = "snapshot")]
    Generation {
        held: Arc<crate::snapshot::serve::Held>,
        identity: Arc<str>,
        named: bool,
        /// A named workspace of the generation (#342): its workspace and its id, which each
        /// result names in `_meta` (`meada/workspace`).
        context: Option<(Arc<Manifest>, Arc<str>)>,
    },
}

impl Meada {
    /// Opens the engine where `source` says and runs `ask`; over stdio it refreshes the index
    /// first, after `root_dir` found the client's root: a 2026-07-28 client is first answered
    /// with `InputRequiredResult` asking for it (`input` carries the retry's answer), and one
    /// whose answer fails gets its last root, or a tool error when it sent none. In server mode
    /// the workspace holds what the request's `Pass` may read, and a call without one (never
    /// routed) is a tool error. The engine blocks (SQLite, and a read that meets a commit waits
    /// up to the busy timeout), so it runs where the runtime expects blocking work.
    async fn answer(
        &self,
        ctx: RequestContext<RoleServer>,
        input: InputResponses,
        ask: impl FnOnce(&Engine) -> crate::Result<String>,
    ) -> CallToolResponse {
        let pass = (ctx.extensions.get::<hyper::http::request::Parts>())
            .and_then(|parts| parts.extensions.get::<Pass>());
        let open = match (&self.source, pass) {
            (Source::Roots { last_root }, _) => match root_dir(&ctx, input.0, last_root).await {
                Ok(Root::Dir(dir)) => tokio::task::block_in_place(|| Engine::open(&dir)),
                Ok(Root::Ask(ask)) => return ask.into(),
                Err(why) => return self.result(Err(why)).into(),
            },
            (_, None) => return self.result(Err(Msg::NotAdmitted.text())).into(),
            (
                Source::Served {
                    listed,
                    manifest,
                    identity,
                },
                Some(pass),
            ) => tokio::task::block_in_place(|| {
                Engine::open_listed(listed, identity, manifest, &|i| pass.reads(i))
            }),
            #[cfg(feature = "snapshot")]
            (
                Source::Generation {
                    held,
                    identity,
                    context,
                    ..
                },
                Some(pass),
            ) => tokio::task::block_in_place(|| match context {
                Some((listed, _)) => {
                    Engine::open_context(held, listed, identity, &|i| pass.reads(i))
                }
                None => Engine::open_held(held, identity, &|i| pass.reads(i)),
            }),
        };
        let answer = tokio::task::block_in_place(|| open.and_then(|engine| ask(&engine)));
        self.result(answer.map_err(|e| e.to_string())).into()
    }

    /// `answer` as the tool's result, an error as a tool error. A `named` generation's result
    /// carries it in `_meta` (`meada/generation`); the text stays what the tool answers.
    fn result(&self, answer: Result<String, String>) -> CallToolResult {
        #[cfg_attr(not(feature = "snapshot"), allow(unused_mut))]
        let mut result = match answer {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
        };
        #[cfg(feature = "snapshot")]
        if let Source::Generation {
            held,
            named: true,
            context,
            ..
        } = &self.source
        {
            let mut meta = rmcp::model::MetaObject::new();
            meta.insert(GENERATION_META.into(), held.id.clone().into());
            if let Some((_, id)) = context {
                meta.insert(WORKSPACE_META.into(), id.to_string().into());
            }
            result.meta = Some(meta);
        }
        result
    }
}

/// The `_meta` key of a result that names its generation.
#[cfg(feature = "snapshot")]
const GENERATION_META: &str = "meada/generation";

/// The `_meta` key naming the named workspace an answer was read through (#342).
#[cfg(feature = "snapshot")]
const WORKSPACE_META: &str = "meada/workspace";

/// What `root_dir` found: the directory, or the round trip to ask the client for it.
enum Root {
    Dir(PathBuf),
    Ask(InputRequiredResult),
}

/// The directory to answer about: the client's first `file://` root, or the server's current
/// directory. Asked on every call, so `roots/list_changed` needs no cache to invalidate. On a
/// connection of protocol 2026-07-28 or later the client drops server requests, so the call
/// answers `InputRequiredResult` asking for `roots` and the retry carries them (SEP-2322,
/// [`Root::Ask`]); earlier protocols get a `roots/list` request, waited for up to
/// [`ROOTS_TIMEOUT`]. A client without the `roots` capability gets the server's directory. When
/// the answer fails (an error, a closed transport, an unexpected response, no answer in time,
/// or a retry whose `roots` is missing or malformed), the last root the client sent is used,
/// with one line on stderr; without one, the call fails: the server's directory may be another
/// checkout than the client's session. An answer with no local root means the server's
/// directory, as without the capability (#285).
#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; still how Claude Code gives the session's directory, now through the multi round-trip request of the 2026-07-28 era"
)]
async fn root_dir(
    ctx: &RequestContext<RoleServer>,
    input: Option<rmcp::model::InputResponses>,
    last_root: &Mutex<Option<PathBuf>>,
) -> Result<Root, String> {
    let cwd = || PathBuf::from(".");
    if ctx.client_capabilities().is_none_or(|c| c.roots.is_none()) {
        return Ok(Root::Dir(cwd()));
    }
    let round_trip = ctx
        .protocol_version()
        .is_some_and(|v| v >= ProtocolVersion::V_2026_07_28);
    let roots = if round_trip {
        // That era drops server requests: ask in the result, read the retry's `roots`.
        let Some(input) = input else {
            let request = InputRequest::ListRoots(ListRootsRequest {
                method: Default::default(),
                extensions: Default::default(),
            });
            return Ok(Root::Ask(InputRequiredResult::from_input_requests(
                [("roots".to_string(), request)].into(),
            )));
        };
        match input.get("roots") {
            None => Err("sent no roots/list answer".to_string()),
            Some(answer) => serde_json::from_value::<ListRootsResult>(answer.clone())
                .map_err(|e| format!("sent a malformed roots/list answer: {e}")),
        }
    } else {
        let request = ServerRequest::ListRootsRequest(ListRootsRequest {
            method: Default::default(),
            extensions: Default::default(),
        });
        let sent = ctx
            .peer
            .send_request_with_option(request, PeerRequestOptions::with_timeout(ROOTS_TIMEOUT))
            .await;
        let response = match sent {
            Ok(handle) => handle.await_response().await,
            Err(e) => Err(e),
        };
        match response {
            Ok(ClientResult::ListRootsResult(result)) => Ok(result),
            Err(ServiceError::Timeout { .. }) => Err(format!(
                "did not answer roots/list within {} s",
                ROOTS_TIMEOUT.as_secs()
            )),
            Err(e) => Err(format!("failed roots/list: {e}")),
            Ok(_) => Err(format!(
                "failed roots/list: {}",
                ServiceError::UnexpectedResponse
            )),
        }
    };
    let why = match roots {
        Ok(result) => {
            let root = result.roots.iter().find_map(|r| file_path(&r.uri));
            *last_root.lock().unwrap() = root.clone();
            return Ok(Root::Dir(root.unwrap_or_else(cwd)));
        }
        Err(why) => why,
    };
    // Copied out, so a blocked stderr cannot hold the lock other calls wait on.
    let last = last_root.lock().unwrap().clone();
    let Some(root) = last else {
        eprintln!("meada: the client {why}");
        return Err(format!("the client {why} and sent no root before"));
    };
    eprintln!(
        "meada: the client {why}, using the last root it sent: {}",
        root.display()
    );
    Ok(Root::Dir(root))
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
        input: InputResponses,
    ) -> CallToolResponse {
        self.answer(ctx, input, |engine| args.ask(engine)).await
    }

    #[tool(
        description = "Show one symbol: its community, the processes it belongs to, its source, callers and callees."
    )]
    async fn context(
        &self,
        Parameters(args): Parameters<ContextArgs>,
        ctx: RequestContext<RoleServer>,
        input: InputResponses,
    ) -> CallToolResponse {
        self.answer(ctx, input, |engine| args.ask(engine)).await
    }

    #[tool(
        description = "Follow a chain through calls and cross-repository links, down or up, until it ends. Links show →[level: reason]; a footer sums it up."
    )]
    async fn trace(
        &self,
        Parameters(args): Parameters<TraceArgs>,
        ctx: RequestContext<RoleServer>,
        input: InputResponses,
    ) -> CallToolResponse {
        let served = !matches!(self.source, Source::Roots { .. });
        if let Err(why) = args.checked(served) {
            return self.result(Err(why)).into();
        }
        self.answer(ctx, input, |engine| args.ask(engine, served))
            .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Meada {
    fn get_info(&self) -> ServerConfig {
        let instructions = match &self.source {
            Source::Roots { .. } => "Code graph of the current repository.".to_string(),
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
        Meada::new(Source::Roots {
            last_root: Arc::default(),
        })
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
    /// pull that adds a repository needs no restart. With `pins` (the node has `[generations]`,
    /// feature `generation-pointer`), `/<identity>@<generation>` serves a held generation and
    /// every answer names its generation.
    #[cfg(feature = "snapshot")]
    Generation {
        current: crate::snapshot::serve::Current,
        config: StreamableHttpServerConfig,
        pins: bool,
    },
}

/// What admits a request before routing: the OIDC module when the manifest has `[oidc]`
/// (feature `oidc`), else nothing, the operator's proxy admitting clients (ADR 0011).
#[derive(Clone)]
struct Gate {
    #[cfg(feature = "oidc")]
    oidc: Option<Arc<crate::oidc::Oidc>>,
}

/// What a request `Gate` let through may read: every identity, or those its token grants.
/// Routing hands it to the endpoint in the request's extensions, so a call's workspace holds
/// only what it may read.
#[derive(Clone)]
struct Pass {
    #[cfg(feature = "oidc")]
    token: Option<(Arc<crate::oidc::Oidc>, crate::oidc::Grant)>,
}

impl Gate {
    /// The gate of `manifest`: with `[oidc]`, the issuer's metadata and JWKS read now, a start
    /// check before the listener binds.
    fn new(manifest: &Manifest) -> crate::Result<Self> {
        #[cfg(feature = "oidc")]
        let oidc = (manifest.oidc.clone().map(crate::oidc::Oidc::start)).transpose()?;
        #[cfg(not(feature = "oidc"))]
        let _ = manifest;
        Ok(Self {
            #[cfg(feature = "oidc")]
            oidc,
        })
    }

    /// The request's pass to routing, or the answer it gets instead (`Oidc::admit`); `path` is
    /// the decoded path without its slashes, `identity` the repository it names.
    fn admit(
        &self,
        request: &Request<Incoming>,
        path: &str,
        identity: &str,
    ) -> Result<Pass, Box<Response<BoxBody<Bytes, Infallible>>>> {
        #[cfg(feature = "oidc")]
        if let Some(oidc) = &self.oidc {
            let admit = || oidc.admit(request.headers(), path, identity);
            let grant = tokio::task::block_in_place(admit)?;
            let token = Some((oidc.clone(), grant));
            return Ok(Pass { token });
        }
        let _ = (request, path, identity);
        Ok(Pass {
            #[cfg(feature = "oidc")]
            token: None,
        })
    }
}

impl Pass {
    /// Whether the request may read `identity`.
    fn reads(&self, identity: &str) -> bool {
        #[cfg(feature = "oidc")]
        if let Some((_, grant)) = &self.token {
            return grant.reads(identity);
        }
        let _ = identity;
        true
    }

    /// What the request may read, as `meada-core` takes it: every identity without a token.
    #[cfg(feature = "snapshot")]
    fn grant(&self) -> meada_core::authorize::Grant {
        #[cfg(feature = "oidc")]
        if let Some((_, grant)) = &self.token {
            return grant.core();
        }
        meada_core::authorize::Grant::All
    }

    /// The 403 for `path`, an endpoint of `identity`, when the request may not read it.
    fn refused(&self, path: &str, identity: &str) -> Option<Response<BoxBody<Bytes, Infallible>>> {
        #[cfg(feature = "oidc")]
        if let Some((oidc, grant)) = &self.token
            && !grant.reads(identity)
        {
            return Some(oidc.forbidden(path));
        }
        let _ = (path, identity);
        None
    }
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
/// nobody unless the manifest has `[oidc]` (feature `oidc`, `Gate`): by default the operator's
/// proxy admits clients. On SIGINT or SIGTERM it stops accepting, lets the
/// answers in flight finish for at most `DRAIN` (a second signal ends the wait), drops what
/// is left and returns.
pub fn listen(listen: &str, manifest: &Path) -> crate::Result<()> {
    let address = address(listen)?;
    // A managed workspace's nodes serve generations (ADR 0014).
    #[cfg(feature = "generation-pointer")]
    if Manifest::read(manifest)?.store {
        let shown = manifest.display().to_string();
        return Err(Msg::WorkspaceStoreRefused(&shown).text().into());
    }
    let listed = Arc::new(engine::served(manifest)?);
    let gate = Gate::new(&listed)?;
    let served: Vec<String> = listed.entries.iter().map(|l| l.repo.clone()).collect();
    let manifest: Arc<Path> = manifest.into();
    run(address, gate, move |config| {
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
    let read = Manifest::read(manifest)?;
    #[cfg(feature = "generation-pointer")]
    let pins = read.generations.is_some();
    #[cfg(not(feature = "generation-pointer"))]
    let pins = false;
    let gate = Gate::new(&read)?;
    run(address, gate, move |config| Routes::Generation {
        current: crate::snapshot::serve::Current::new(local_dir),
        config: config.clone(),
        pins,
    })
}

/// The server over `address` with the endpoints `routes` makes from the transport's
/// configuration, each request through `gate` first.
fn run(
    address: SocketAddr,
    gate: Gate,
    routes: impl FnOnce(&StreamableHttpServerConfig) -> Routes,
) -> crate::Result<()> {
    serve_http(address, move |_, config| {
        #[cfg(feature = "oidc")]
        if let Some(oidc) = &gate.oidc {
            oidc.listening();
        }
        let routes = Arc::new(routes(config));
        move |request| {
            let (routes, gate) = (routes.clone(), gate.clone());
            async move { route(&routes, &gate, request).await }
        }
    })
}

/// A plain HTTP/1 server over `address`, the node's and the console's (`ui`, feature `ui`):
/// `make` gets the bound address and the MCP transport's configuration and returns what answers
/// each request. The first line on stderr is the bound address. On SIGINT or SIGTERM it stops
/// accepting, lets the answers in flight finish for at most the drain bound (a second signal
/// ends the wait), drops what is left and returns.
pub(crate) fn serve_http<F, A>(
    address: SocketAddr,
    make: impl FnOnce(SocketAddr, &StreamableHttpServerConfig) -> F,
) -> crate::Result<()>
where
    F: Fn(Request<Incoming>) -> A + Clone + Send + Sync + 'static,
    A: Future<Output = Response<BoxBody<Bytes, Infallible>>> + Send + 'static,
{
    let drain = DRAIN_BOUND.get();
    let runtime = tokio::runtime::Runtime::new()?;
    let stopped: crate::Result<()> = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind(address).await?;
        // Before the address is printed: a signal sent on reading it is never lost.
        let mut stop = Stop::install()?;
        let bound = listener.local_addr()?;
        eprintln!("listening on http://{bound}");
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .disable_allowed_hosts()
            .enforce_origin_validation();
        let answer = make(bound, &config);
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
            let answer = answer.clone();
            let mut stopping = stopping.subscribe();
            connections.spawn(async move {
                let service = service_fn(move |request| {
                    let answer = answer(request);
                    async move { Ok::<_, Infallible>(answer.await) }
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
/// served, one per line. With pins, `/<identity>@<generation>` (the generation what follows the
/// last `@`, when it is an id) answers from that generation if the node holds it, and is a 404
/// naming it, the current one and the held ones if not; every answer then carries its
/// generation in `Meada-Generation`. A request carrying `Origin` gets 403 before anything
/// else (`browser`). `gate` answers next: with `[oidc]`, a request it lets through is refused
/// with 403 at an endpoint its token may not read, a 404 lists only the identities the token may
/// read, and the endpoint gets its `Pass`.
async fn route(
    routes: &Routes,
    gate: &Gate,
    mut request: Request<Incoming>,
) -> Response<BoxBody<Bytes, Infallible>> {
    if let Some(refused) = browser(request.headers()) {
        return refused;
    }
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
    // A pin, `<identity>@<generation>`, on a node with pins.
    #[cfg(feature = "snapshot")]
    let pin = match routes {
        Routes::Generation { pins: true, .. } => {
            (identity.rsplit_once('@')).filter(|(_, id)| crate::snapshot::is_id(id))
        }
        _ => None,
    };
    // The repository the path names.
    #[cfg(feature = "snapshot")]
    let target = pin.map_or(identity, |(identity, _)| identity);
    #[cfg(not(feature = "snapshot"))]
    let target = identity;
    // `/-/workspaces/<id>/<identity>` (#342): a named workspace's route. The first segment `-` is
    // reserved, so it is never a repository's.
    #[cfg(feature = "snapshot")]
    let (workspace, target) =
        match (target.strip_prefix("-/workspaces/")).and_then(|r| r.split_once('/')) {
            Some((id, repo)) if crate::snapshot::is_id(id) => (Some(id), repo),
            _ => (None, target),
        };
    let pass = match gate.admit(&request, identity, target) {
        Ok(pass) => pass,
        Err(answer) => return *answer,
    };
    let listing = |identities: &mut dyn Iterator<Item = &str>| -> String {
        (identities.filter(|i| pass.reads(i)))
            .map(|i| format!("{i}\n"))
            .collect()
    };
    match routes {
        Routes::Listed {
            endpoints,
            listing: all,
        } => match endpoints.get(identity) {
            Some(endpoint) => match pass.refused(identity, identity) {
                Some(refused) => refused,
                None => {
                    request.extensions_mut().insert(pass.clone());
                    endpoint.handle(request).await
                }
            },
            None => plain(StatusCode::NOT_FOUND, listing(&mut all.lines())),
        },
        #[cfg(feature = "snapshot")]
        Routes::Generation {
            current,
            config,
            pins,
        } => {
            let held = tokio::task::block_in_place(|| match pin {
                Some((_, id)) => current.pinned(id),
                None => current.get().map(Some),
            });
            let named = |mut response: Response<_>, id: &str| {
                if *pins && let Ok(value) = header::HeaderValue::from_str(id) {
                    response.headers_mut().insert(GENERATION_HEADER, value);
                }
                response
            };
            let held = held.map_err(|e| e.to_string());
            // A named workspace's route is admitted only when the generation records the context
            // and the identity, and the token reads every entry of the context's version, excluded
            // ones included (`Held::admits`, #510); otherwise the answer is the 404 an unknown id
            // gets, listing the workspaces the token may view (ADR 0021).
            if let (Some(id), Ok(Some(held))) = (workspace, held.as_ref()) {
                let grant = pass.grant();
                let context = match held.admits(id, target, &grant) {
                    true => {
                        tokio::task::block_in_place(|| held.context(id)).map_err(|e| e.to_string())
                    }
                    false => Ok(None),
                };
                return match context {
                    Ok(Some(listed)) => {
                        let source = Source::Generation {
                            held: held.clone(),
                            identity: target.into(),
                            named: *pins,
                            context: Some((listed, id.into())),
                        };
                        request.extensions_mut().insert(pass.clone());
                        named(endpoint(source, config).handle(request).await, &held.id)
                    }
                    // A pin of a generation with no such context for the caller names the requested
                    // and the current generation, never another workspace's id.
                    Ok(None) if pin.is_some() => {
                        let now = crate::snapshot::pull::current(current.local())
                            .ok()
                            .flatten();
                        let now = now.map(|(id, _)| id);
                        let body = format!(
                            "{}\n{}\n",
                            Msg::GenerationNoContext(&held.id).text(),
                            Msg::SnapshotStatusCurrent(now.as_deref()).text(),
                        );
                        named(plain(StatusCode::NOT_FOUND, body), &held.id)
                    }
                    Ok(None) => {
                        let viewable: String = (held.viewable(&grant).into_iter())
                            .map(|c| format!("{c}\n"))
                            .collect();
                        named(plain(StatusCode::NOT_FOUND, viewable), &held.id)
                    }
                    Err(problem) => {
                        eprintln!("meada: {problem}");
                        plain(StatusCode::SERVICE_UNAVAILABLE, UNSERVABLE.into())
                    }
                };
            }
            match held {
                Ok(Some(held))
                    if workspace.is_none()
                        && held.listed.entries.iter().any(|l| l.repo == target) =>
                {
                    if let Some(refused) = pass.refused(identity, target) {
                        return refused;
                    }
                    let id = held.id.clone();
                    let source = Source::Generation {
                        held,
                        identity: target.into(),
                        named: *pins,
                        context: None,
                    };
                    request.extensions_mut().insert(pass.clone());
                    named(endpoint(source, config).handle(request).await, &id)
                }
                Ok(Some(held)) => {
                    let mut repos = held.listed.entries.iter().map(|l| l.repo.as_str());
                    named(plain(StatusCode::NOT_FOUND, listing(&mut repos)), &held.id)
                }
                // A pin of a generation the node does not hold: only to a request that may read
                // the identity it names.
                Ok(None) => {
                    // A workspace route answers 404 where the plain route would say 403: a 403
                    // would confirm that the id exists.
                    if workspace.is_none()
                        && let Some(refused) = pass.refused(identity, target)
                    {
                        return refused;
                    }
                    if workspace.is_some() && !pass.reads(target) {
                        return plain(StatusCode::NOT_FOUND, String::new());
                    }
                    let local = current.local();
                    let now = crate::snapshot::pull::current(local).ok().flatten();
                    let now = now.map(|(id, _)| id);
                    let id = pin.map_or("", |(_, id)| id);
                    let body = format!(
                        "{}\n{}\n{}\n",
                        Msg::GenerationNotHeld(id).text(),
                        Msg::SnapshotStatusCurrent(now.as_deref()).text(),
                        Msg::SnapshotStatusHeld(&crate::snapshot::serve::ids(local)).text(),
                    );
                    let response = plain(StatusCode::NOT_FOUND, body);
                    match &now {
                        Some(now) => named(response, now),
                        None => response,
                    }
                }
                // The body is fixed: the request may come from anyone, and the problem names
                // local paths.
                Err(problem) => {
                    eprintln!("meada: {problem}");
                    plain(StatusCode::SERVICE_UNAVAILABLE, UNSERVABLE.into())
                }
            }
        }
    }
}

/// The 403 for a request carrying `Origin` (a browser, as on a DNS rebinding page), for every
/// path and with or without `[oidc]`: no listing, pin or 503 reaches a browser.
fn browser(headers: &header::HeaderMap) -> Option<Response<BoxBody<Bytes, Infallible>>> {
    headers.contains_key(header::ORIGIN).then(|| {
        Response::builder()
            .status(StatusCode::FORBIDDEN)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Full::from("Forbidden: Origin header is not allowed\n").boxed())
            .expect("a valid response")
    })
}

/// The body of the 503 a node answers while it holds no servable generation.
#[cfg(feature = "snapshot")]
const UNSERVABLE: &str = "no servable generation\n";

/// The response header naming the generation of an answer (with pins).
#[cfg(feature = "snapshot")]
const GENERATION_HEADER: &str = "meada-generation";

/// The address `--listen` names: `host:port`, `:port` or `port`, the host an IP address or
/// `localhost`; without one, the loopback interface. No name is resolved.
pub(crate) fn address(listen: &str) -> crate::Result<SocketAddr> {
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
