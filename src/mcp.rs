//! The MCP server: a thin adapter over the query engine (ADR 0008). No tool writes; the only
//! write is the index refresh every query does first. Stdout is the protocol channel, so
//! anything else goes to stderr.

use std::path::Path;

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
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
    /// name), `document` (an OpenAPI file, by path or title), or `ambiguous` or `unresolved` (open
    /// items, with the `meada decide` command for each).
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
    /// Start: a symbol, a route (`GET /users/{}` or its contract) or a call site.
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
}

/// Opens the repository of the current directory and runs `ask`, which refreshes the index.
fn answer(ask: impl FnOnce(&Engine) -> crate::Result<String>) -> Result<String, String> {
    Engine::open(Path::new("."))
        .and_then(|engine| ask(&engine))
        .map_err(|e| e.to_string())
}

#[tool_router]
impl Meada {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Find symbols by name and contracts by identifier (`http::GET::/users/{}`). One line each: repository, path:line, name."
    )]
    async fn search(&self, Parameters(args): Parameters<SearchArgs>) -> Result<String, String> {
        answer(|engine| {
            let budget = args.budget.map(|b| b as usize);
            engine.search(&args.query, args.kind.as_deref(), budget)
        })
    }

    #[tool(description = "Show one symbol: its source, callers and callees.")]
    async fn context(&self, Parameters(args): Parameters<ContextArgs>) -> Result<String, String> {
        answer(|engine| engine.context(&args.symbol, args.budget.map(|b| b as usize)))
    }

    #[tool(
        description = "Follow a chain through calls and cross-repository links, down or up, until it ends. Links show →[level: reason]; a footer sums it up."
    )]
    async fn trace(&self, Parameters(args): Parameters<TraceArgs>) -> Result<String, String> {
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
        answer(|engine| engine.trace(start, direction, args.budget.map(|b| b as usize)))
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
