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
use tokio::sync::Semaphore;

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
    /// two or more places, by name or path), `value` (a scalar of a YAML or JSON file, by value
    /// or key path; with no kind, the answer when nothing else matches), `iac` (a Terraform
    /// resource, data block or module call, or a Kubernetes object, by address, type or physical
    /// name), `ci` (a pipeline, an action, a CI job or a CI step, by address, id, name or
    /// `uses`), `image` (an image builder or runner), `alert` (an alert emitter or matcher), or `ambiguous`
    /// or `unresolved` (open items, with the `meada decide` command for each).
    kind: Option<String>,
    /// Token budget for the answer (default 2000).
    budget: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct ContextArgs {
    /// Symbol, `<path>::<name>` or full identity. Also accepts `http::GET::/orders`, `resource::<kind>::<id>`, `channel::<broker>::<name>`, or an IaC address.
    symbol: String,
    /// Token budget for the answer (default 2000).
    budget: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct TraceArgs {
    /// Start: a symbol, a route (`GET /users/{}` or its contract), a call site, an IaC node's
    /// address, or a process (`process::<entry point identity>`, down only).
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

    fn ask(&self, engine: &Engine, served: bool) -> crate::Result<String> {
        let (start, direction) = self.checked(served)?;
        engine.trace(start, direction, self.budget.map(|b| b as usize))
    }
}

#[cfg(feature = "ui")]
pub(crate) type Ask = Box<dyn FnOnce(&Engine) -> crate::Result<String>>;

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

const ROOTS_TIMEOUT: Duration = Duration::from_secs(5);

// ponytail: a fixed count, chosen by the memory one answer holds (up to about 90 MB on the
// largest repositories measured); sharing one engine among the calls of a batch would lift it.
const CALLS_IN_FLIGHT: usize = 4;

#[derive(Clone)]
struct Meada {
    tool_router: ToolRouter<Self>,
    source: Source,
}

#[derive(Clone)]
enum Source {
    Roots {
        last_root: Arc<Mutex<Option<PathBuf>>>,
        turns: Arc<Semaphore>,
    },
    Served {
        listed: Arc<Manifest>,
        manifest: Arc<Path>,
        identity: Arc<str>,
    },
    #[cfg(feature = "snapshot")]
    Generation {
        held: Arc<crate::snapshot::serve::Held>,
        identity: Arc<str>,
        named: bool,
        context: Option<(Arc<Manifest>, Arc<str>)>,
    },
}

impl Meada {
    async fn answer(
        &self,
        ctx: RequestContext<RoleServer>,
        input: InputResponses,
        ask: impl FnOnce(&Engine) -> crate::Result<String>,
    ) -> CallToolResponse {
        let pass = (ctx.extensions.get::<hyper::http::request::Parts>())
            .and_then(|parts| parts.extensions.get::<Pass>());
        let mut turn = None;
        let open = match (&self.source, pass) {
            (Source::Roots { last_root, turns }, _) => {
                match root_dir(&ctx, input.0, last_root).await {
                    Ok(Root::Dir(dir)) => {
                        turn = turns.acquire().await.ok();
                        tokio::task::block_in_place(|| Engine::open(&dir))
                    }
                    Ok(Root::Ask(ask)) => return ask.into(),
                    Err(why) => return self.result(Err(why)).into(),
                }
            }
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
        drop(turn);
        self.result(answer.map_err(|e| e.to_string())).into()
    }

    fn result(&self, answer: Result<String, String>) -> CallToolResult {
        #[cfg_attr(not(feature = "snapshot"), expect(unused_mut))]
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

#[cfg(feature = "snapshot")]
const GENERATION_META: &str = "meada/generation";

#[cfg(feature = "snapshot")]
const WORKSPACE_META: &str = "meada/workspace";

enum Root {
    Dir(PathBuf),
    Ask(InputRequiredResult),
}

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
            turns: Arc::new(Semaphore::new(CALLS_IN_FLIGHT)),
        })
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
        Ok(())
    })
}

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

type Endpoint = StreamableHttpService<Meada, NeverSessionManager>;

enum Routes {
    Listed {
        endpoints: HashMap<String, Endpoint>,
        listing: String,
    },
    #[cfg(feature = "snapshot")]
    Generation {
        current: crate::snapshot::serve::Current,
        config: StreamableHttpServerConfig,
        pins: bool,
    },
}

#[derive(Clone)]
struct Gate {
    #[cfg(feature = "oidc")]
    oidc: Option<Arc<crate::oidc::Oidc>>,
    #[cfg(feature = "accounts")]
    local: Option<Arc<crate::accounts::Agents>>,
    #[cfg(feature = "oidc")]
    providers: Arc<crate::grant::Providers>,
}

#[derive(Clone)]
struct Pass {
    #[cfg(feature = "oidc")]
    grant: Option<crate::grant::Grant>,
}

impl Gate {
    fn new(manifest: &Manifest, file: &Path) -> crate::Result<Self> {
        #[cfg(feature = "oidc")]
        let providers = Arc::new(crate::grant::Providers::of(manifest));
        #[cfg(feature = "oidc")]
        let start = |config| crate::oidc::Oidc::start(config, providers.clone());
        #[cfg(feature = "oidc")]
        let oidc = (manifest.oidc.clone().map(start)).transpose()?;
        #[cfg(feature = "accounts")]
        let local = match (&manifest.accounts, &oidc) {
            (Some(accounts), None) => Some(Arc::new(local_admission(manifest, accounts, file)?)),
            _ => None,
        };
        let _ = (manifest, file);
        Ok(Self {
            #[cfg(feature = "oidc")]
            oidc,
            #[cfg(feature = "accounts")]
            local,
            #[cfg(feature = "oidc")]
            providers,
        })
    }

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
            return Ok(Pass { grant: Some(grant) });
        }
        #[cfg(feature = "accounts")]
        if let Some(local) = &self.local {
            let claims = local.admit(request.headers()).map_err(|sent| {
                let challenge = crate::accounts::challenge(sent);
                Box::new(challenged(StatusCode::UNAUTHORIZED, challenge))
            })?;
            let login = claims.login.as_deref();
            let grant = self.providers.local(&claims.groups, login).union();
            return Ok(Pass { grant: Some(grant) });
        }
        let _ = (request, path, identity);
        Ok(Pass {
            #[cfg(feature = "oidc")]
            grant: None,
        })
    }

    fn refused(
        &self,
        pass: &Pass,
        path: &str,
        identity: &str,
    ) -> Option<Response<BoxBody<Bytes, Infallible>>> {
        #[cfg(feature = "oidc")]
        if let (Some(oidc), Some(grant)) = (&self.oidc, &pass.grant)
            && !grant.reads(identity)
        {
            return Some(oidc.forbidden(path));
        }
        #[cfg(feature = "accounts")]
        if let (Some(_), Some(grant)) = (&self.local, &pass.grant)
            && !grant.reads(identity)
        {
            let challenge = "Bearer error=\"insufficient_scope\"";
            return Some(challenged(StatusCode::FORBIDDEN, challenge));
        }
        let _ = (pass, path, identity);
        None
    }
}

#[cfg(feature = "accounts")]
fn local_admission(
    manifest: &Manifest,
    accounts: &crate::accounts::Config,
    file: &Path,
) -> crate::Result<crate::accounts::Agents> {
    let src = std::fs::read_to_string(file)?;
    let at = |(key, problem): crate::modules::Problem| {
        crate::config::error_at(file, &src, key, &problem)
    };
    let Some(ui) = &manifest.ui else {
        return Err(at(("accounts", Msg::AccountsNodeIssuer.text())));
    };
    (accounts.agents(&ui.public_url, crate::accounts::NODES)).map_err(at)
}

#[cfg(feature = "accounts")]
fn challenged(status: StatusCode, challenge: &'static str) -> Response<BoxBody<Bytes, Infallible>> {
    let body = match status {
        StatusCode::UNAUTHORIZED => "unauthorized\n",
        _ => "forbidden\n",
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::WWW_AUTHENTICATE, challenge)
        .body(Full::from(body).boxed())
        .expect("a valid response")
}

impl Pass {
    fn reads(&self, identity: &str) -> bool {
        #[cfg(feature = "oidc")]
        if let Some(grant) = &self.grant {
            return grant.reads(identity);
        }
        let _ = identity;
        true
    }

    #[cfg(feature = "snapshot")]
    fn grant(&self) -> meada_core::authorize::Grant {
        #[cfg(feature = "oidc")]
        if let Some(grant) = &self.grant {
            return grant.into();
        }
        meada_core::authorize::Grant::All
    }
}

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
    #[cfg(feature = "generation-pointer")]
    if Manifest::read(manifest)?.store {
        let shown = manifest.display().to_string();
        return Err(Msg::WorkspaceStoreRefused(&shown).text().into());
    }
    let listed = Arc::new(engine::served(manifest)?);
    let gate = Gate::new(&listed, manifest)?;
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
    let gate = Gate::new(&read, manifest)?;
    run(address, gate, move |config| Routes::Generation {
        current: crate::snapshot::serve::Current::new(local_dir),
        config: config.clone(),
        pins,
    })
}

fn run(
    address: SocketAddr,
    gate: Gate,
    routes: impl FnOnce(&StreamableHttpServerConfig) -> Routes,
) -> crate::Result<()> {
    serve_http(address, move |_, config| {
        #[cfg(feature = "oidc")]
        gate.providers.listening();
        let routes = Arc::new(routes(config));
        move |request| {
            let (routes, gate) = (routes.clone(), gate.clone());
            async move { route(&routes, &gate, request).await }
        }
    })
}

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
                let connection = (http1::Builder::new())
                    .timer(TokioTimer::new())
                    .serve_connection(TokioIo::new(stream), service);
                let mut connection = std::pin::pin!(connection);
                tokio::select! {
                    _ = connection.as_mut() => return,
                    _ = stopping.changed() => connection.as_mut().graceful_shutdown(),
                }
                let _ = connection.await;
            });
        }
        drop(listener);
        drop(stopping);
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
    runtime.shutdown_background();
    stopped
}

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
    #[cfg(feature = "snapshot")]
    let pin = match routes {
        Routes::Generation { pins: true, .. } => {
            (identity.rsplit_once('@')).filter(|(_, id)| crate::snapshot::is_id(id))
        }
        _ => None,
    };
    #[cfg(feature = "snapshot")]
    let target = pin.map_or(identity, |(identity, _)| identity);
    #[cfg(not(feature = "snapshot"))]
    let target = identity;
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
            Some(endpoint) => match gate.refused(&pass, identity, identity) {
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
                    if let Some(refused) = gate.refused(&pass, identity, target) {
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
                Ok(None) => {
                    if workspace.is_none()
                        && let Some(refused) = gate.refused(&pass, identity, target)
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
                Err(problem) => {
                    eprintln!("meada: {problem}");
                    plain(StatusCode::SERVICE_UNAVAILABLE, UNSERVABLE.into())
                }
            }
        }
    }
}

fn browser(headers: &header::HeaderMap) -> Option<Response<BoxBody<Bytes, Infallible>>> {
    headers.contains_key(header::ORIGIN).then(|| {
        Response::builder()
            .status(StatusCode::FORBIDDEN)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Full::from("Forbidden: Origin header is not allowed\n").boxed())
            .expect("a valid response")
    })
}

#[cfg(feature = "snapshot")]
const UNSERVABLE: &str = "no servable generation\n";

#[cfg(feature = "snapshot")]
const GENERATION_HEADER: &str = "meada-generation";

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
