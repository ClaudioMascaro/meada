//! The MCP server: a thin adapter over the query engine (ADR 0008). No tool writes; the only
//! write is the index refresh every query does first. Stdout is the protocol channel, so
//! anything else goes to stderr.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use percent_encoding::percent_decode_str;
#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577 and still what Claude Code sends (see `root_dir`)"
)]
use rmcp::model::ListRootsRequest;
use rmcp::{
    RoleServer, ServerHandler, ServiceError, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ClientResult, ServerRequest},
    service::{PeerRequestOptions, RequestContext},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::engine::{Direction, Engine, Start};

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

/// How long a call waits for the client's `roots/list` answer: generous for a busy client, far
/// below the point where a client gives up on the call itself (#282).
const ROOTS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct Meada {
    tool_router: ToolRouter<Self>,
    /// The last root this connection's client sent. Clones share it.
    last_root: Arc<Mutex<Option<PathBuf>>>,
}

/// Opens the client's first root, else the current directory, and runs `ask`, which refreshes
/// the index. A client that does not answer `roots/list` in time gets its last root, or a tool
/// error when it sent none.
async fn answer(
    ctx: RequestContext<RoleServer>,
    last_root: &Mutex<Option<PathBuf>>,
    ask: impl FnOnce(&Engine) -> crate::Result<String>,
) -> Result<String, String> {
    let dir = root_dir(&ctx, last_root).await?;
    Engine::open(&dir)
        .and_then(|engine| ask(&engine))
        .map_err(|e| e.to_string())
}

/// The directory to answer about: the client's first `file://` root, or the server's current
/// directory. Asked on every call, so `roots/list_changed` needs no cache to invalidate. When
/// the client declares roots but does not answer within [`ROOTS_TIMEOUT`], the last root it
/// sent is used, with one line on stderr; without one, the call fails: the server's directory
/// may be another checkout than the client's session.
#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577 and still what Claude Code sends; the cwd fallback keeps the server working once clients drop them"
)]
async fn root_dir(
    ctx: &RequestContext<RoleServer>,
    last_root: &Mutex<Option<PathBuf>>,
) -> Result<PathBuf, String> {
    let cwd = || PathBuf::from(".");
    if ctx.client_capabilities().is_none_or(|c| c.roots.is_none()) {
        return Ok(cwd());
    }
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
        Ok(ClientResult::ListRootsResult(result)) => {
            let root = result.roots.iter().find_map(|r| file_path(&r.uri));
            *last_root.lock().unwrap() = root.clone();
            Ok(root.unwrap_or_else(cwd))
        }
        Err(ServiceError::Timeout { .. }) => {
            let secs = ROOTS_TIMEOUT.as_secs();
            // Copied out, so a blocked stderr cannot hold the lock other calls wait on.
            let last = last_root.lock().unwrap().clone();
            let Some(root) = last else {
                eprintln!("meada: the client did not answer roots/list within {secs} s");
                return Err(format!(
                    "the client did not answer roots/list within {secs} s and sent no root before"
                ));
            };
            eprintln!(
                "meada: the client did not answer roots/list within {secs} s, using the last root it sent: {}",
                root.display()
            );
            Ok(root)
        }
        _ => Ok(cwd()),
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
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            last_root: Arc::default(),
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
        answer(ctx, &self.last_root, |engine| {
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
        answer(ctx, &self.last_root, |engine| {
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
        let direction = match args.direction.as_deref() {
            None | Some("down") => Direction::Down,
            Some("up") => Direction::Up,
            Some(other) => return Err(format!("`direction` takes `down` or `up`, not `{other}`")),
        };
        answer(ctx, &self.last_root, |engine| {
            engine.trace(start, direction, args.budget.map(|b| b as usize))
        })
        .await
    }
}

#[tool_handler(router = self.tool_router, name = "meada", instructions = "Code graph of the current repository.")]
impl ServerHandler for Meada {}

/// Serves the tools over stdio until the client disconnects.
pub fn serve() -> crate::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        Meada::new()
            .serve(rmcp::transport::stdio())
            .await?
            .waiting()
            .await?;
        Ok(())
    })
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
    fn file_uri_decodes_percent_encoding_and_rejects_other_schemes() {
        assert_eq!(
            file_path("file:///tmp/my%20repo/caf%C3%A9"),
            Some(PathBuf::from("/tmp/my repo/café"))
        );
        assert_eq!(file_path("https://example.com/x"), None);
        assert_eq!(file_path("file://localhost/x"), None);
    }
}
