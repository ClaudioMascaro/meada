//! The proposal server (ADR 0022, feature `proposals`): `meada workspaces mcp --listen <addr>
//! --workspace <bootstrap manifest>` answers agents over MCP (Streamable HTTP, stateless, JSON
//! responses) with the console's own named-workspace functions, persistence off. Four tools, each
//! read-only: `catalog` (what the caller may select), `workspaces` and `show` (what it may see),
//! and `propose` (manifest text validated, composed, diffed and authorized as the console would
//! save it, stored nowhere).
//!
//! It is neither a node nor the console. A request goes through, in order: `Origin` refused with
//! 403 (a browser); the protected resource metadata (RFC 9728) served without a token; admission
//! (a bearer token for `[proposals] audience`, the nodes' OIDC module, 401 before any routing or
//! tool name); the directory's block of a deprovisioned subject; the path (`/` only); the limit
//! of calls in flight (503 `busy`); the body bound (413). Actor, login, groups, grant, role and
//! the workspaces the subject operates come from the token and the permissions set alone: no
//! tool takes an argument naming any of them, every input schema is closed, and an undeclared
//! property is `invalid_params`.
//!
//! Nothing here writes: every Postgres session is read-only (`default_transaction_read_only`),
//! the audit is off, no store or Valkey write is called, and the only trace of a call is one
//! stderr line, `proposals: <subject> <tool> <workspace or -> : <outcome>`, with no token,
//! manifest, hint or identity in it.

use std::path::Path;
// The tool macros name a bare `Result`: the standard one, not the crate's alias.
use std::result::Result;
use std::sync::Arc;

use futures::FutureExt;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ErrorData, Implementation, ServerCapabilities, ServerConfig};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::never::NeverSessionManager,
};
use rmcp::{RoleServer, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;

use super::super::catalog::{
    CURSOR_MAX, LIMIT_MAX as CATALOG_LIMIT_MAX, PREFIX_MAX, checked, identity_ok,
};
use super::*;

// ponytail: ADR 0022's bounds: a request body (a manifest of at most 64 KiB and its few other
// fields), a tool's answer, the calls in flight, and the hints and a hint's note. The catalog's
// prefix and cursor are the console's own.
const BODY_MAX: usize = 80 << 10;
const OUTPUT_MAX: usize = 192 << 10;
const CALLS_MAX: usize = 32;
const HINTS_MAX: usize = 50;
const NOTE_MAX: usize = 256;

/// The calls in flight this process allows: `CALLS_MAX`, or in a debug build what
/// `MEADA_PROPOSALS_CALLS` says, so a test reaches the bound (no option or setting does).
fn calls_max() -> usize {
    #[cfg(debug_assertions)]
    if let Some(n) = std::env::var("MEADA_PROPOSALS_CALLS")
        .ok()
        .and_then(|n| n.parse().ok())
    {
        return n;
    }
    CALLS_MAX
}

/// Where the protected resource metadata of the server's root lives (RFC 9728).
const METADATA: &str = "/.well-known/oauth-protected-resource";

/// The body of the 403 for a request carrying `Origin`, the nodes' own.
const ORIGIN_REFUSED: &str = "Forbidden: Origin header is not allowed\n";

/// The MCP service of the tools, stateless with JSON responses.
type Endpoint = StreamableHttpService<Tools, NeverSessionManager>;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CatalogArgs {
    /// Only identities starting with this text (at most 128 characters).
    query: Option<String>,
    /// How many identities to return: 1 to 100, 50 by default.
    limit: Option<u32>,
    /// The `next` cursor of the previous page.
    after: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkspacesArgs {
    /// How many workspaces to return: 1 to 100, 25 by default.
    limit: Option<u32>,
    /// The `next` cursor of the previous page (a workspace id).
    after: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ShowArgs {
    /// The workspace id (16 hexadecimal characters).
    workspace: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProposeArgs {
    /// The manifest text: `[[repository]]` entries with `repo` and optional `serve`, `resource`
    /// and `peer` blocks, at most 64 KiB.
    entries: String,
    /// The workspace the proposal changes (its id); none for a new workspace.
    workspace: Option<String>,
    /// A new workspace's name (1 to 80 characters).
    name: Option<String>,
    /// A new workspace's description (at most 500 characters).
    description: Option<String>,
    /// Repositories an agent read elsewhere, at most 50: their selectability is reported, and
    /// they never add an entry or change a result.
    hints: Option<Vec<Hint>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Hint {
    /// The repository identity (`<host>/<owner>/<repo>`).
    identity: String,
    /// Free text of at most 256 characters, read by nobody: discarded unread.
    note: Option<String>,
}

/// Why a tool gives no answer: an argument past its bound (the tool error `invalid_params`), or
/// the error the console would answer, `{"code", "message", ...}`.
enum Refusal {
    Arguments(String),
    Error(Value),
}

type Outcome = Result<Value, Refusal>;

/// The tools, over the console started for them.
#[derive(Clone)]
struct Tools {
    tool_router: ToolRouter<Self>,
    console: Arc<Console>,
}

/// The listener's state: the console started for the tools, the section, the tools' endpoint and
/// the permits of the calls in flight.
struct Server {
    console: Arc<Console>,
    section: crate::proposals::Config,
    endpoint: Endpoint,
    calls: Arc<tokio::sync::Semaphore>,
}

/// `meada workspaces mcp`: refuses to start, listing every problem, without `[proposals]` (and
/// so `[workspaces]` and `[oidc.github]`), when the issuer's metadata or JWKS cannot be read, or
/// when the Postgres schema is another version; then serves at `listen` until SIGINT or SIGTERM,
/// as `meada mcp --listen` does.
pub(in crate::ui) fn listen(listen: &str, manifest: &Path) -> crate::Result<()> {
    let address = crate::mcp::address(listen)?;
    let console = Arc::new(Console::for_proposals(manifest)?);
    let section = (console.bootstrap.proposals.clone()).expect("checked at start");
    crate::mcp::serve_http(address, move |_, config| {
        if let Some(oidc) = &console.oidc {
            oidc.listening();
        }
        let tools = Tools::new(console.clone());
        let server = Arc::new(Server {
            console: console.clone(),
            section,
            endpoint: StreamableHttpService::new(
                move || Ok(tools.clone()),
                Default::default(),
                config.clone(),
            ),
            calls: Arc::new(tokio::sync::Semaphore::new(calls_max())),
        });
        move |request| {
            let server = server.clone();
            async move { server.route(request).await }
        }
    })
}

impl Console {
    /// The console the tools answer from: the bootstrap's sections as the console reads them,
    /// with every Postgres session read-only, the audit off, no SCIM endpoint, no front end and
    /// no GitHub App.
    fn for_proposals(file: &Path) -> crate::Result<Self> {
        let read = Manifest::read(file)?;
        let shown = file.display().to_string();
        let no_section = || format!("{shown}: {}", Msg::ProposalsNoSection.text());
        if read.proposals.is_none() {
            return Err(no_section().into());
        }
        // `[proposals]` needs `[workspaces]`, which needs `[ui]`, `[oidc.github]` and `[postgres]`.
        let ui = read.ui.clone().ok_or_else(no_section)?;
        let read = read.read_only();
        let src = fs::read_to_string(file).map_err(|e| format!("{shown}: {e}"))?;
        let at = |key: &str, problem: String| crate::config::error_at(file, &src, key, &problem);
        let mut problems: Vec<String> = Vec::new();
        let oidc = match read.oidc.clone().map(crate::oidc::Oidc::start).transpose() {
            Ok(oidc) => oidc,
            Err(e) => {
                problems.push(e.to_string());
                None
            }
        };
        if let Some(postgres) = &read.postgres
            && let Err(version) = crate::postgres::probe(postgres)
        {
            problems.push(at("postgres", version).to_string());
        }
        if !problems.is_empty() {
            return Err(problems.join("\n").into());
        }
        Ok(Self {
            file: file.to_path_buf(),
            bootstrap: read.clone(),
            readers: Arc::new(tokio::sync::Semaphore::new(super::super::read::READERS_MAX)),
            apps: Vec::new(),
            ui,
            root: PathBuf::new(),
            loaded: Value::Null,
            csp: header::HeaderValue::from_str(&csp(None))?,
            oidc,
            holds: Holds::of(file, &read),
            entries: std::sync::Mutex::new(None),
            audit: crate::audit::Audit::default(),
            #[cfg(feature = "directory")]
            scim: None,
            #[cfg(feature = "directory")]
            directory: Self::directory_of(&read),
        })
    }
}

impl Server {
    /// A request, in the order the module's documentation gives.
    async fn route(&self, request: Request<Incoming>) -> Answer {
        if request.headers().contains_key(header::ORIGIN) {
            return plain(StatusCode::FORBIDDEN, ORIGIN_REFUSED);
        }
        if request.uri().path() == METADATA {
            return self.metadata();
        }
        let admitted = tokio::task::block_in_place(|| self.admit(request.headers()));
        let session = match admitted {
            Ok(session) => session,
            Err(refused) => return *refused,
        };
        if request.uri().path() != "/" {
            return not_found();
        }
        let Ok(permit) = self.calls.clone().try_acquire_owned() else {
            let message = Msg::ProposalsBusy.text();
            return error(StatusCode::SERVICE_UNAVAILABLE, "busy", &message);
        };
        let (mut parts, body) = request.into_parts();
        let body = match Limited::new(body, BODY_MAX).collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) if e.is::<http_body_util::LengthLimitError>() => {
                let message = Msg::UiTooLarge(BODY_MAX).text();
                return error(StatusCode::PAYLOAD_TOO_LARGE, "too_large", &message);
            }
            Err(_) => return plain(StatusCode::BAD_REQUEST, "Bad Request\n"),
        };
        parts.extensions.insert(session);
        let answer = (self.endpoint)
            .handle(Request::from_parts(parts, Full::new(body)))
            .await;
        drop(permit);
        answer
    }

    /// The protected resource metadata of the server (RFC 9728): its URL, the issuer and the
    /// scopes its tokens carry.
    fn metadata(&self) -> Answer {
        let Some(oidc) = &self.console.oidc else {
            return not_found();
        };
        let (issuer, scopes) = oidc.login();
        let mut document = json!({
            "resource": self.section.public_url,
            "authorization_servers": [issuer],
            "bearer_methods_supported": ["header"],
        });
        if !scopes.is_empty() {
            document["scopes_supported"] = scopes.into();
        }
        json_answer(StatusCode::OK, &document)
    }

    /// The session of a request's bearer token for `[proposals] audience`, checked by the nodes'
    /// module as the console's is; a subject the directory deprovisioned is refused. Otherwise
    /// the 401 or 403 with a challenge naming the metadata.
    fn admit(&self, headers: &HeaderMap) -> Result<Session, Box<Answer>> {
        let console = &self.console;
        let Some(oidc) = &console.oidc else {
            return Err(Box::new(plain(StatusCode::UNAUTHORIZED, "unauthorized\n")));
        };
        let admitted = oidc.admit_console(
            headers,
            &self.section.audience,
            console.ui.operator_groups.as_deref(),
            console.ui.operator_claim.as_deref(),
            console.user_claim(),
        );
        match admitted {
            Ok(admitted) => {
                #[cfg(feature = "directory")]
                if let Some((refused, _)) =
                    console.directory_refusal(admitted.user.as_deref(), false)
                {
                    return Err(refused);
                }
                Ok(Session::admitted(admitted, Vec::new(), oidc))
            }
            Err((status, challenge)) => {
                let metadata = format!(
                    "resource_metadata=\"{}{METADATA}\"",
                    self.section.public_url
                );
                let challenge = match challenge.strip_prefix("Bearer ") {
                    Some(rest) => format!("Bearer {metadata}, {rest}"),
                    None => format!("Bearer {metadata}"),
                };
                let body = match status {
                    StatusCode::UNAUTHORIZED => "unauthorized\n",
                    _ => "forbidden\n",
                };
                let mut answer = plain(status, body);
                if let Ok(value) = header::HeaderValue::from_str(&challenge) {
                    answer.headers_mut().insert(header::WWW_AUTHENTICATE, value);
                }
                Err(Box::new(answer))
            }
        }
    }
}

#[tool_router]
impl Tools {
    fn new(console: Arc<Console>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            console,
        }
    }

    #[tool(
        description = "The repositories you may select for a named workspace, from Meada's verified permissions: `state` (`ready`, or `pending` with a `reason` for everyone), the set's age, and a page of identities with the `next` cursor.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn catalog(
        &self,
        Parameters(args): Parameters<CatalogArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(&ctx, "catalog", None, |console, session| {
            console.tool_catalog(&args, session)
        })
    }

    #[tool(
        description = "The named workspaces you may see: id, name, description, version, `seq` (the base a change must still find), state, and whether you operate each.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn workspaces(
        &self,
        Parameters(args): Parameters<WorkspacesArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(&ctx, "workspaces", None, |console, session| {
            console.tool_workspaces(&args, session)
        })
    }

    #[tool(
        description = "One named workspace you may see: its record, `seq`, state, the entries you read and their canonical manifest text.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn show(
        &self,
        Parameters(args): Parameters<ShowArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let workspace = args.workspace.clone();
        self.call(&ctx, "show", Some(&workspace), |console, session| {
            console.tool_show(&args, session)
        })
    }

    #[tool(
        description = "Validate manifest text as a new named workspace or a change to one, as the console would save it, and save nothing: the canonical `manifest` text, the `diff` against the base, `diagnostics`, each identity's `eligibility`, the console's `refusals`, the `effective` configuration, `applicable`, and the `base` to hand to a person who saves it in the console.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn propose(
        &self,
        Parameters(args): Parameters<ProposeArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let workspace = args.workspace.clone();
        self.call(&ctx, "propose", workspace.as_deref(), |console, session| {
            console.tool_propose(&args, session)
        })
    }
}

impl Tools {
    /// Runs a tool for the session the listener admitted, bounds its answer and leaves the one
    /// stderr line of the call.
    fn call(
        &self,
        ctx: &RequestContext<RoleServer>,
        tool: &str,
        workspace: Option<&str>,
        run: impl FnOnce(&Console, &Session) -> Outcome,
    ) -> Result<CallToolResult, ErrorData> {
        let session = (ctx.extensions.get::<hyper::http::request::Parts>())
            .and_then(|parts| parts.extensions.get::<Session>())
            .cloned();
        let Some(session) = session else {
            let message = Msg::ProposalsNotAdmitted.text();
            return Err(ErrorData::invalid_request(message, None));
        };
        let outcome = tokio::task::block_in_place(|| run(&self.console, &session));
        let (result, line) = match outcome {
            Ok(answer) => {
                // A proposal the console would not accept names why: its first refusal, else
                // its first diagnostic.
                let why = match answer["applicable"] {
                    Value::Bool(false) => (answer["refusals"][0]["code"].as_str())
                        .or(answer["diagnostics"][0]["code"].as_str())
                        .map(str::to_string),
                    _ => None,
                };
                let (result, line) = bounded(answer);
                (result, why.filter(|_| line == "ok").unwrap_or(line))
            }
            // A tool error, as rmcp answers an argument its schema refuses: the agent corrects it.
            Err(Refusal::Arguments(message)) => (
                Ok(CallToolResult::structured_error(json!({"error": {
                    "code": "invalid_params", "message": message,
                }}))),
                "invalid_params".to_string(),
            ),
            Err(Refusal::Error(error)) => {
                let code = error["code"].as_str().unwrap_or("error").to_string();
                (
                    Ok(CallToolResult::structured_error(json!({"error": error}))),
                    code,
                )
            }
        };
        // An id that fails its pattern is never printed: it is the caller's text.
        let workspace = workspace.filter(|w| meada_core::ids::is_id(w));
        eprintln!(
            "proposals: {} {tool} {}: {line}",
            session.subject.as_deref().unwrap_or("-").escape_debug(),
            workspace.unwrap_or("-"),
        );
        result
    }
}

/// `answer` as the tool's result, or `too_large` with the size and the length of each of its
/// lists when it is past `OUTPUT_MAX`: never a truncated answer.
fn bounded(answer: Value) -> (Result<CallToolResult, ErrorData>, String) {
    let bytes = answer.to_string().len();
    if bytes <= OUTPUT_MAX {
        return (Ok(CallToolResult::structured(answer)), "ok".into());
    }
    let counts: serde_json::Map<String, Value> = (answer.as_object().into_iter().flatten())
        .filter_map(|(key, value)| Some((key.clone(), value.as_array()?.len().into())))
        .collect();
    let error = json!({
        "code": "too_large",
        "message": Msg::ProposalsTooLarge(OUTPUT_MAX).text(),
        "bytes": bytes,
        "counts": counts,
    });
    let result = CallToolResult::structured_error(json!({"error": error}));
    (Ok(result), "too_large".into())
}

/// A console answer as a tool's outcome: its document, a 400 or 413 as an argument refused with
/// the console's message, any other status as the console's error.
fn from_console(answer: Answer) -> Outcome {
    let status = answer.status();
    let bytes = (answer.into_body().collect().now_or_never())
        .and_then(|collected| collected.ok())
        .map(|collected| collected.to_bytes())
        .unwrap_or_default();
    let document: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    match status {
        status if status.is_success() => Ok(document),
        StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE => {
            let message = document["error"]["message"].as_str().unwrap_or_default();
            Err(Refusal::Arguments(message.to_string()))
        }
        _ => Err(Refusal::Error(document["error"].clone())),
    }
}

impl Console {
    /// The console's error a creation of workspace `id` would meet in its transaction, past
    /// eligibility: `workspace_deleted` or `limit`, as `refusal` words them, read without a
    /// write.
    fn creation_conflict(&self, id: &str, max: usize) -> Result<Option<Value>, Refusal> {
        let Some(policy) = self.workspace_policy() else {
            return Ok(None);
        };
        let read = crate::modules::block_on(store::creation_conflict(policy.postgres, id, max));
        let unavailable = || {
            let message = Msg::WorkspacesUnavailable.text();
            Refusal::Error(json!({"code": "unavailable", "message": message}))
        };
        Ok(match read.map_err(|_| unavailable())? {
            Err(_) => return Err(unavailable()),
            Ok(Some(store::CreateError::Deleted)) => Some(json!({
                "code": "workspace_deleted", "message": Msg::WorkspaceDeleted.text(),
            })),
            Ok(Some(_)) => Some(json!({
                "code": "limit", "message": Msg::WorkspacesLimit(max).text(),
            })),
            Ok(None) => None,
        })
    }
}

/// The refusal of an argument past its bound, naming what the argument takes.
fn argument(what: &str) -> Refusal {
    Refusal::Arguments(Msg::ProposalsArgument(what).text())
}

/// A problem of manifest text `src` as a diagnostic: the CLI's text, and the line, the dotted key
/// and the entry (counted from 1) when the problem names them (`entries:<line>: `<key>`: ...`).
fn diagnostic(code: &str, problem: &str, src: &str) -> Value {
    let mut found = json!({"code": code, "message": problem});
    let Some(rest) = problem.strip_prefix("entries:") else {
        return found;
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let Ok(line) = digits.parse::<usize>() else {
        return found;
    };
    found["line"] = line.into();
    let entry = (src.lines().take(line))
        .filter(|l| l.trim() == "[[repository]]")
        .count();
    if entry > 0 {
        found["entry"] = entry.into();
    }
    let key = (rest[digits.len()..].strip_prefix(": `")).and_then(|r| r.split_once('`'));
    if let Some((key, _)) = key {
        found["key"] = key.into();
    }
    found
}

/// A row of the console's `show` (`{"repo", "ingestion", "blocks"}`) as the entry it shows.
fn shown_entry(row: &Value) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert("repo".into(), row["repo"].clone());
    if let Some(blocks) = row["blocks"].as_object() {
        entry.extend(blocks.clone());
    }
    Value::Object(entry)
}

impl Console {
    /// `catalog`: the console's `GET catalog` with the tool's arguments as its query.
    fn tool_catalog(&self, args: &CatalogArgs, session: &Session) -> Outcome {
        let encode = |text: &str| utf8_percent_encode(text, NON_ALPHANUMERIC).to_string();
        let mut query = Vec::new();
        if let Some(limit) = args.limit {
            if !(1..=CATALOG_LIMIT_MAX as u32).contains(&limit) {
                return Err(argument(&format!("limit takes 1 to {CATALOG_LIMIT_MAX}")));
            }
            query.push(format!("limit={limit}"));
        }
        if let Some(after) = &args.after {
            if after.is_empty() || after.len() > CURSOR_MAX {
                return Err(argument("after is the next cursor of a page"));
            }
            query.push(format!("after={}", encode(after)));
        }
        if let Some(prefix) = &args.query {
            if prefix.chars().count() > PREFIX_MAX || prefix.contains('\0') {
                return Err(argument(&format!(
                    "query takes at most {PREFIX_MAX} characters"
                )));
            }
            query.push(format!("q={}", encode(prefix)));
        }
        from_console(self.catalog(Some(&query.join("&")), session))
    }

    /// `workspaces`: the console's list, each workspace with its `seq`.
    fn tool_workspaces(&self, args: &WorkspacesArgs, session: &Session) -> Outcome {
        let limit = match args.limit {
            None => LIST_LIMIT,
            Some(limit) if (1..=LIST_LIMIT_MAX as u32).contains(&limit) => limit as usize,
            Some(_) => return Err(argument(&format!("limit takes 1 to {LIST_LIMIT_MAX}"))),
        };
        if args
            .after
            .as_deref()
            .is_some_and(|a| !meada_core::ids::is_id(a))
        {
            return Err(argument("after is the next cursor of a page"));
        }
        let (page, next) = match self.page(session, limit, args.after.as_deref()) {
            Ok(page) => page,
            Err(answer) => return from_console(*answer),
        };
        let held = self.held_generation();
        let workspaces: Vec<Value> = (page.iter())
            .map(|record| {
                let mut shown = self.summary(record, held.as_deref(), session);
                shown["seq"] = record.seq.into();
                shown
            })
            .collect();
        Ok(json!({"workspaces": workspaces, "next": next}))
    }

    /// `show`: the console's show, with the canonical text of the entries it lists and `seq`.
    fn tool_show(&self, args: &ShowArgs, session: &Session) -> Outcome {
        let mut shown = from_console(self.show(&args.workspace, session))?;
        let entries: Vec<Value> = (shown["entries"].as_array().into_iter().flatten())
            .map(shown_entry)
            .collect();
        shown["manifest"] = meada_core::workspace::entries_text(&entries).into();
        shown["seq"] = shown["base"].clone();
        Ok(shown)
    }

    /// `propose`: the manifest text as the console would store it for `session` (created, or
    /// saved over the workspace's current version), checked as the console checks a creation or
    /// a save before its transaction, and stored nowhere.
    fn tool_propose(&self, args: &ProposeArgs, session: &Session) -> Outcome {
        let Some(section) = &self.bootstrap.workspaces else {
            return from_console(Self::no_workspace());
        };
        // Every bound before any read.
        if args.entries.len() > named::TEXT_MAX {
            let max = named::TEXT_MAX >> 10;
            return Err(argument(&format!(
                "entries takes at most {max} KiB of manifest text"
            )));
        }
        // A rename is the console's own operation, never part of a proposal for a change.
        if args.workspace.is_some() && (args.name.is_some() || args.description.is_some()) {
            return Err(argument(
                "name and description are a new workspace's; leave them out with workspace",
            ));
        }
        let hints = args.hints.as_deref().unwrap_or_default();
        let hint_ok = |h: &Hint| {
            identity_ok(&h.identity)
                && (h.note.as_deref()).is_none_or(|n| n.chars().count() <= NOTE_MAX)
        };
        if hints.len() > HINTS_MAX || !hints.iter().all(hint_ok) {
            return Err(argument(&format!(
                "hints takes at most {HINTS_MAX} {{identity, note?}}, each an identity and a note \
                 of at most {NOTE_MAX} characters"
            )));
        }
        let mut diagnostics: Vec<Value> = Vec::new();
        let submitted = match named::version_entries("entries", &args.entries, section.max_entries)
        {
            Ok(entries) => Some(entries),
            Err(problems) => {
                for problem in &problems {
                    diagnostics.push(diagnostic("invalid_entries", problem, &args.entries));
                }
                None
            }
        };
        let invalid = |problem: String| json!({"code": "invalid_workspace", "message": problem});
        // The base: a workspace the caller sees, under the discovery rule.
        let base = match &args.workspace {
            None => None,
            Some(id) if !meada_core::ids::is_id(id) => return from_console(Self::no_workspace()),
            Some(id) => match self.visible_records(session, Some(id)) {
                Ok(mut records) => match records.pop() {
                    Some(record) => Some(record),
                    None => return from_console(Self::no_workspace()),
                },
                Err(answer) => return from_console(*answer),
            },
        };
        match (&base, &args.name) {
            (None, None) => diagnostics.push(invalid(
                meada_core::msg::Msg::NamedNameBad(named::NAME_MAX).text(),
            )),
            (None, Some(name)) => diagnostics.extend(named::name_problem(name).map(invalid)),
            _ => {}
        }
        if let Some(description) = &args.description {
            diagnostics.extend(named::description_problem(description).map(invalid));
        }
        let current = match &base {
            Some(record) => match self.version_entries(record) {
                Ok(current) => current,
                // The call's one stderr line says `unavailable`; the store's reason stays unsaid.
                Err(_) => {
                    let message = Msg::WorkspacesDocumentBad(&record.id).text();
                    return Err(Refusal::Error(
                        json!({"code": "unavailable", "message": message}),
                    ));
                }
            },
            None => Vec::new(),
        };
        let grant = Self::read_grant(session);
        // What a save's refusal may name: the platform administrator's management view reads
        // every entry, anyone else what its grant reads.
        let management = match session.operator {
            true => Grant::All,
            false => grant.clone(),
        };
        // What the console would store: the platform administrator's text whole; an operator's
        // text composed over the entries it cannot read, which stay in place.
        let stored = submitted
            .as_ref()
            .map(|submitted| match (&base, session.operator) {
                (Some(_), false) => named::compose(&grant, &current, submitted),
                _ => submitted.clone(),
            });
        if let Some(stored) = &stored {
            if stored.is_empty() {
                let message = match base {
                    Some(_) => Msg::WorkspacesEmptyVersion.text(),
                    None => Msg::UiEntriesInvalid.text(),
                };
                diagnostics.push(json!({"code": "invalid_entries", "message": message}));
            } else if stored.len() > section.max_entries {
                let message = meada_core::msg::Msg::NamedTooMany(section.max_entries).text();
                diagnostics.push(json!({"code": "invalid_entries", "message": message}));
            }
        }
        // The refusals the console would give, in its order: authority, then the entries it adds
        // or changes against the admission cache's eligibility.
        let mut refusals: Vec<Value> = Vec::new();
        let anonymous = session.subject.as_deref().is_none_or(str::is_empty);
        match &base {
            Some(record) if !Self::operates(session, record) => refusals.push(json!({
                "code": "not_operator", "message": Msg::WorkspacesNotOperator.text(),
            })),
            None if anonymous => refusals.push(json!({
                "code": "forbidden", "message": Msg::UiOperatorRequired.text(),
            })),
            _ => {}
        }
        if let Some(stored) = &stored {
            // As the console does: a save names a refusal by what the caller's management view
            // reads, a creation by its grant.
            let (changed, naming) = match &base {
                Some(_) => (named::changed(&current, stored), &management),
                None => (named::identities(stored), &grant),
            };
            if !changed.is_empty()
                && let Some((answer, _)) = self.precheck(session, &changed, naming)
                && let Err(Refusal::Error(error)) = from_console(answer)
            {
                refusals.push(error);
            }
            // Then what the creation's transaction finds: the same workspace created and deleted,
            // or the limit of workspaces.
            if let (None, true, Some(subject), Some(name)) = (
                &base,
                refusals.is_empty() && diagnostics.is_empty(),
                session.subject.as_deref(),
                &args.name,
            ) {
                let id = meada_core::ids::workspace_id(subject, name, &named::version_id(stored));
                refusals.extend(self.creation_conflict(&id, section.max_workspaces)?);
            }
        }
        // Each identity the text and the hints name, once, in that order; a hint's note is
        // discarded unread.
        let mut named_identities: Vec<String> = submitted
            .as_deref()
            .map(named::identities)
            .unwrap_or_default();
        named_identities.extend(hints.iter().map(|h| h.identity.clone()));
        let eligibility = match self.catalog_of(session) {
            Some(catalog) => checked(&catalog, &named_identities),
            None => Value::Null,
        };
        // The text, the diff and the view name only the entries the caller reads.
        let shown = stored.map(|stored| Self::readable_entries(session, stored));
        let manifest = (shown.as_deref()).and_then(meada_core::workspace::entries_text);
        let mut diff = Value::Null;
        let mut effective = Value::Null;
        if let (Some(record), Some(shown)) = (&base, &shown) {
            let before = Self::readable_entries(session, current.clone());
            let found = named::diff(&before, shown);
            diff =
                json!({"added": found.added, "removed": found.removed, "changed": found.changed});
            let operates = Self::operates(session, record);
            let held = self.held_generation();
            let excluded = self.excluded_for(&record.id, operates);
            let view = Self::view(shown, held.as_deref(), &excluded, session);
            effective = json!({
                "workspace": record.id,
                "generation": held.as_ref().map(|h| h.id.clone()),
                "entries": view,
            });
        }
        let base = base.map(
            |record| json!({"workspace": record.id, "seq": record.seq, "version": record.current}),
        );
        Ok(json!({
            "base": base,
            "manifest": manifest,
            "diff": diff,
            "diagnostics": diagnostics,
            "eligibility": eligibility,
            "refusals": refusals,
            "effective": effective,
            "applicable": diagnostics.is_empty() && refusals.is_empty() && manifest.is_some(),
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Tools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "meada-proposals",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Named workspaces of this Meada deployment: the repositories you may select, \
                 the workspaces you may see, and proposals validated as the console would save \
                 them. Nothing is saved or applied here: hand a proposal's manifest and base to \
                 a person, who reviews, saves and applies it in the console.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_problem_names_its_line_key_and_entry_when_it_has_them() {
        let src =
            "[[repository]]\nrepo = \"h/o/y\"\n\n[[repository]]\nrepo = \"h/o/a\"\npath = \"x\"\n";
        let found = diagnostic(
            "invalid_entries",
            "entries:6: `repository.path`: is reserved",
            src,
        );
        assert_eq!(found["line"], 6);
        assert_eq!(found["entry"], 2);
        assert_eq!(found["key"], "repository.path");
        let bare = diagnostic("invalid_entries", "entries: too long", src);
        assert!(bare.get("line").is_none() && bare.get("key").is_none());
    }

    #[test]
    fn an_answer_past_the_cap_is_too_large_with_its_counts_never_truncated() {
        let small = json!({"entries": [1, 2]});
        assert_eq!(bounded(small).1, "ok");
        let big = json!({"entries": vec!["x".repeat(1024); 200], "next": null});
        let (result, line) = bounded(big);
        assert_eq!(line, "too_large");
        let result = result.unwrap();
        let error = &result.structured_content.unwrap()["error"];
        assert_eq!(error["counts"], json!({"entries": 200}));
        assert_eq!(result.is_error, Some(true));
    }
}
