//! The management console's process (ADR 0014, feature `ui`): `meada ui --listen <addr>
//! --workspace <bootstrap manifest>` serves the front end's files from `[ui] dir` and the
//! JSON API under `/api/v1/`, on the listener `meada mcp --listen` uses (`mcp::serve_http`). It is
//! never a node's route and never an MCP server. Every request goes through, in order, the `Host`
//! check, the `Origin` check, admission and routing. No answer sets a cookie or carries an
//! `Access-Control-*` header.
//!
//! Admission: behind the operator's proxy every request is a viewer's that reads everything.
//! With `[accounts]` (feature `accounts`, ADR 0024) the console signs in local accounts itself and
//! admits its own session tokens (`accounts`); beside `[oidc]`, with `break_glass = true`, only the
//! default account's, told apart from the identity provider's tokens by issuer. With `[oidc]`
//! (feature `oidc`) the console is a resource server as a node is: a bearer token for
//! `[ui] audience`, checked by the nodes' own module (`Oidc::admit_console`), except for the
//! static files and `config`, which the browser needs to log in. Every admitted subject is a
//! viewer unless its verified groups or boolean claim admit it as an operator, or one of its
//! groups owns an entry of the current workspace version its grant reads (a team operator, ADR
//! 0015). A viewer's or team operator's token grant bounds every view (`Session::reads`, #290);
//! operators see the whole fleet and may write when operations are enabled; a team operator
//! saves and applies changes to its teams' entries. Each write is authorized before its body is consumed
//! and leaves one escaped audit line (`write`).
//!
//! The playground (`POST query`) and the exports (`GET repositories/<identity>/export`) answer
//! in process from what the console holds (`Holds`), through the engine and the tools' own code,
//! as a node and `meada export` answer; the console never calls a node.

use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use http_body_util::{BodyExt, Full, Limited, combinators::BoxBody};
use hyper::body::{Bytes, Incoming};
use hyper::{HeaderMap, Method, Request, Response, StatusCode, header};
use percent_encoding::percent_decode_str;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Result;
use crate::engine::Engine;
use crate::modules::{self, Problem, Shape};
use crate::msg::Msg;
use crate::workspace::Manifest;

#[cfg(feature = "accounts")]
mod accounts;
#[cfg(feature = "accounts")]
pub use accounts::reset_admin;
#[cfg(feature = "workspaces")]
mod catalog;
#[cfg(feature = "directory")]
mod directory;
mod read;
#[cfg(feature = "workspaces")]
mod workspaces;
mod write;

type Answer = Response<BoxBody<Bytes, Infallible>>;

type Body = std::result::Result<Bytes, StatusCode>;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const VERSION_FILE: &str = "meada-ui.json";

// ponytail: the most bytes of `meada-ui.json`; past it the file counts as another version.
const UI_VERSION_MAX: u64 = 4096;

const CHECK: &str = "/api/v1/admission/check";

// ponytail: the most bytes of a token check's body: a JWT in `{"token": ...}` with room to spare.
const CHECK_MAX: usize = 16 << 10;

const QUERY: &str = "/api/v1/query";

// ponytail: the most bytes of a playground query's body: an identity, a generation id and a
// tool's few short arguments, with room to spare.
const QUERY_MAX: usize = 16 << 10;

// ponytail: the most records `GET audit` answers at once, and how many without `limit` (#328).
const AUDIT_LIMIT_MAX: i64 = 100;
const AUDIT_LIMIT: i64 = 50;

const HOST_REFUSED: &str = "Forbidden: Host header is not allowed\n";
const ORIGIN_REFUSED: &str = "Forbidden: Origin header is not allowed\n";
const WRITE_REFUSED: &str = "Forbidden: a write needs an Origin header\n";

/// `[ui]` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    dir: Option<String>,
    public_url: Option<String>,
    audience: Option<String>,
    client_id: Option<String>,
    #[serde(default)]
    operations: bool,
    operator_groups: Option<Vec<String>>,
    operator_claim: Option<String>,
}

/// The validated section; `client_id` is also checked at start beside the OIDC module.
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
    fn validate(
        self,
        base: &Path,
        oidc: Option<&toml::Table>,
        accounts: bool,
    ) -> std::result::Result<Config, Vec<Problem>> {
        let mut problems = Vec::new();
        let local = accounts && oidc.is_none();
        if self.operations && oidc.is_none() && !local {
            #[cfg(feature = "accounts")]
            let needs = Msg::UiNeedsAdmission;
            #[cfg(not(feature = "accounts"))]
            let needs = Msg::UiNeedsOidc;
            problems.push(("ui.operations", needs.text()));
        }
        match (&self.operator_groups, &self.operator_claim) {
            (None, None) if self.operations && !local => {
                problems.push(("ui.operations", Msg::UiOperatorNeeded.text()));
            }
            (Some(_), Some(_)) => {
                problems.push(("ui.operator_groups", Msg::UiOperatorBoth.text()));
            }
            _ => {}
        }
        for (key, present) in [
            ("ui.operator_groups", self.operator_groups.is_some()),
            ("ui.operator_claim", self.operator_claim.is_some()),
        ] {
            if present && oidc.is_none() {
                problems.push((key, Msg::UiNeedsOidc.text()));
            }
        }
        if self.operator_groups.is_some()
            && oidc.is_some_and(|oidc| !oidc.contains_key("groups_claim"))
        {
            problems.push((
                "ui.operator_groups",
                Msg::KeyMissing("oidc.groups_claim").text(),
            ));
        }
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

pub(crate) fn validated(
    file: &Path,
    src: &str,
    base: &Path,
    section: Option<File>,
) -> Result<Option<Config>> {
    let Some(section) = section else {
        return Ok(None);
    };
    let table: toml::Table = toml::from_str(src)?;
    let oidc = table.get("oidc").and_then(toml::Value::as_table);
    let accounts = table.contains_key("accounts");
    section
        .validate(base, oidc, accounts)
        .map(Some)
        .map_err(|problems| {
            let lines: Vec<String> = (problems.iter())
                .map(|(key, problem)| crate::config::error_at(file, src, key, problem).to_string())
                .collect();
            lines.join("\n").into()
        })
}

struct Console {
    file: PathBuf,
    bootstrap: Manifest,
    readers: Arc<tokio::sync::Semaphore>,
    #[cfg(feature = "github-app")]
    apps: Vec<crate::github_app::Config>,
    ui: Config,
    root: PathBuf,
    loaded: Value,
    csp: header::HeaderValue,
    #[cfg(feature = "oidc")]
    oidc: Option<Arc<crate::oidc::Oidc>>,
    #[cfg(feature = "oidc")]
    providers: Arc<crate::grant::Providers>,
    holds: Holds,
    #[cfg(all(feature = "oidc", feature = "generation-pointer"))]
    entries: std::sync::Mutex<Option<(String, Arc<Vec<Value>>)>>,
    audit: crate::audit::Audit,
    #[cfg(feature = "accounts")]
    accounts: Option<accounts::Local>,
    #[cfg(feature = "directory")]
    scim: Option<crate::directory::scim::Scim>,
    #[cfg(feature = "directory")]
    directory: Option<crate::directory::read::Directory>,
}

enum Holds {
    Listed(Box<Manifest>),
    #[cfg(feature = "snapshot")]
    Generations(std::result::Result<crate::snapshot::serve::Current, String>),
}

impl Holds {
    fn of(file: &Path, read: &Manifest) -> Self {
        #[cfg(feature = "snapshot")]
        if let Some(snapshot) = &read.snapshot {
            return Holds::Generations(match &snapshot.local_dir {
                Some(local) => Ok(crate::snapshot::serve::Current::new(local.clone())),
                None => Err(Msg::SnapshotNoLocalDir(&file.display().to_string()).text()),
            });
        }
        let _ = file;
        Holds::Listed(Box::new(read.clone()))
    }
}

#[derive(Clone)]
struct Session {
    subject: Option<String>,
    operator: bool,
    #[cfg_attr(
        not(any(feature = "oidc", feature = "refresh-lock")),
        expect(dead_code)
    )]
    groups: Vec<String>,
    teams: Vec<String>,
    #[cfg(feature = "oidc")]
    grant: Option<crate::grant::Grant>,
    login: Option<String>,
    #[cfg(feature = "oidc")]
    github: Option<(String, crate::grant::GrantState)>,
    #[cfg(feature = "accounts")]
    account: bool,
    #[cfg(feature = "accounts")]
    suspended: bool,
    #[cfg(feature = "directory")]
    directory_down: bool,
}

impl Session {
    #[cfg(feature = "oidc")]
    fn admitted(admitted: crate::grant::Admitted, teams: Vec<String>) -> Self {
        Session {
            subject: admitted.subject,
            grant: Some(admitted.grant),
            operator: admitted.operator,
            groups: admitted.groups,
            teams,
            login: admitted.login,
            github: admitted.github,
            #[cfg(feature = "accounts")]
            account: false,
            #[cfg(feature = "accounts")]
            suspended: false,
            #[cfg(feature = "directory")]
            directory_down: false,
        }
    }

    #[cfg(feature = "accounts")]
    fn local(admitted: crate::grant::Admitted, teams: Vec<String>) -> Self {
        Session {
            account: true,
            ..Session::admitted(admitted, teams)
        }
    }

    fn anonymous() -> Self {
        Session {
            subject: None,
            operator: false,
            groups: Vec::new(),
            teams: Vec::new(),
            #[cfg(feature = "oidc")]
            grant: None,
            login: None,
            #[cfg(feature = "oidc")]
            github: None,
            #[cfg(feature = "accounts")]
            account: false,
            #[cfg(feature = "accounts")]
            suspended: false,
            #[cfg(feature = "directory")]
            directory_down: false,
        }
    }

    fn reads(&self, identity: &str) -> bool {
        if self.operator {
            return true;
        }
        #[cfg(feature = "oidc")]
        if let Some(grant) = &self.grant {
            return grant.reads(identity);
        }
        let _ = identity;
        true
    }

    fn is_team_operator(&self) -> bool {
        !self.operator && !self.teams.is_empty()
    }

    fn owners(&self) -> Option<&[String]> {
        self.is_team_operator().then_some(&self.teams[..])
    }

    #[cfg(feature = "refresh-lock")]
    fn core_grant(&self) -> meada_core::authorize::Grant {
        #[cfg(feature = "oidc")]
        if let Some(grant) = &self.grant {
            return grant.into();
        }
        meada_core::authorize::Grant::All
    }

    #[cfg(feature = "refresh-lock")]
    fn github_host(&self) -> Option<&str> {
        #[cfg(feature = "oidc")]
        if let Some((host, _)) = &self.github {
            return Some(host);
        }
        None
    }

    fn role(&self) -> &'static str {
        match (self.operator, self.is_team_operator()) {
            (true, _) => "operator",
            (_, true) => "team_operator",
            _ => "viewer",
        }
    }

    fn document(&self) -> Value {
        #[cfg_attr(not(feature = "oidc"), expect(unused_mut))]
        let mut document = json!({"subject": self.subject, "role": self.role()});
        #[cfg(feature = "oidc")]
        if let Some(grant) = &self.grant {
            document["groups"] = json!(self.groups);
            document["grant"] = match grant {
                crate::grant::Grant::All => json!("*"),
                crate::grant::Grant::Only(identities) => {
                    let mut identities: Vec<_> = identities.iter().collect();
                    identities.sort();
                    json!(identities)
                }
            };
            if self.is_team_operator() {
                document["teams"] = json!(self.teams);
            }
            if let Some((host, state)) = &self.github {
                if let Some(login) = &self.login {
                    document["login"] = json!(login);
                }
                document["github_host"] = json!(host);
                document["github_grant"] = json!(state);
            }
        }
        #[cfg(feature = "accounts")]
        if self.account {
            document["account"] = true.into();
        }
        document
    }

    fn whole(&self) -> bool {
        if self.operator {
            return true;
        }
        #[cfg(feature = "oidc")]
        if let Some(grant) = &self.grant {
            return matches!(grant, crate::grant::Grant::All);
        }
        true
    }
}

/// `meada ui`: refuses to start, listing every problem, when the bootstrap manifest at
/// `manifest` has errors or no `[ui]`, when `[ui] dir` is missing or built for another Meada
/// version, or with `[oidc]` when `[ui] client_id` is missing or the issuer's metadata or JWKS
/// cannot be read. Then serves at `listen` (`host:port`, `:port` or `port`, the loopback
/// interface without a host) until SIGINT or SIGTERM, as `meada mcp --listen` does.
pub fn listen(listen: &str, manifest: &Path) -> Result<()> {
    let address = crate::mcp::address(listen)?;
    let console = Arc::new(Console::start(manifest)?);
    crate::mcp::serve_http(address, move |bound, _| {
        #[cfg(feature = "accounts")]
        if let Some(local) = &console.accounts {
            local.announce();
        }
        #[cfg(feature = "oidc")]
        console.providers.listening();
        move |request: Request<Incoming>| {
            let console = console.clone();
            async move {
                let (parts, body) = request.into_parts();
                #[cfg(feature = "directory")]
                if let Some(scim) = &console.scim
                    && crate::directory::scim::routed(parts.uri.path())
                {
                    let answer = console.scim(scim, bound, &parts, body).await;
                    return console.secure(answer);
                }
                let admitted = tokio::task::block_in_place(|| console.checked(bound, &parts));
                let answer = match admitted {
                    Err(refused) => *refused,
                    Ok(session) => {
                        let body = read(&parts, body).await;
                        let request = Request::from_parts(parts, body);
                        tokio::task::block_in_place(|| console.answer(&request, &session))
                    }
                };
                console.secure(answer)
            }
        }
    })
}

/// `meada workspaces mcp` (ADR 0022, feature `proposals`): the proposal server, the console's
/// named-workspace functions answered over MCP to agents with persistence off.
#[cfg(feature = "proposals")]
pub fn proposals(listen: &str, manifest: &Path) -> Result<()> {
    workspaces::proposals::listen(listen, manifest)
}

async fn read(parts: &hyper::http::request::Parts, body: Incoming) -> Body {
    let max = match parts.uri.path() {
        CHECK => CHECK_MAX,
        #[cfg(feature = "workspaces")]
        "/api/v1/catalog/check" => catalog::CHECK_BODY_MAX,
        #[cfg(feature = "workspaces")]
        "/api/v1/workspaces" | "/api/v1/workspaces/effective" => workspaces::CREATE_MAX,
        QUERY => QUERY_MAX,
        #[cfg(feature = "accounts")]
        "/api/v1/login" => accounts::LOGIN_MAX,
        #[cfg(feature = "accounts")]
        "/api/v1/account/password" => accounts::PASSWORD_BODY_MAX,
        #[cfg(feature = "accounts")]
        "/api/v1/account/tokens" => accounts::TOKENS_BODY_MAX,
        #[cfg(feature = "accounts")]
        "/api/v1/account/github" => accounts::GITHUB_BODY_MAX,
        #[cfg(feature = "accounts")]
        path if (path.strip_prefix("/api/v1/")).is_some_and(accounts::is_administration) => {
            accounts::ADMIN_BODY_MAX
        }
        "/api/v1/pointer" => write::POINTER_MAX,
        "/api/v1/fence/recover" => write::FENCE_MAX,
        "/api/v1/workspace/versions" => write::SAVE_MAX,
        "/api/v1/workspace/apply" => write::APPLY_MAX,
        #[cfg(feature = "workspaces")]
        path => match workspaces::body_max(path) {
            Some(max) => max,
            None => return Ok(Bytes::new()),
        },
        #[cfg(not(feature = "workspaces"))]
        _ => return Ok(Bytes::new()),
    };
    if parts.method != Method::POST {
        return Ok(Bytes::new());
    }
    match Limited::new(body, max).collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(e) if e.is::<http_body_util::LengthLimitError>() => Err(StatusCode::PAYLOAD_TOO_LARGE),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

impl Console {
    fn start(file: &Path) -> Result<Self> {
        let read = Manifest::read(file)?;
        let Some(ui) = read.ui.clone() else {
            return Err(format!("{}: {}", file.display(), Msg::UiNoSection.text()).into());
        };
        let src = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let at = |key: &str, problem: String| crate::config::error_at(file, &src, key, &problem);
        let mut problems = Vec::new();
        #[cfg(feature = "oidc")]
        if read.oidc.is_some() && ui.client_id.is_none() {
            problems.push(at("ui", Msg::KeyMissing("ui.client_id").text()));
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
        #[cfg(feature = "oidc")]
        let providers = Arc::new(crate::grant::Providers::of(&read));
        #[cfg(feature = "oidc")]
        let start = |config| crate::oidc::Oidc::start(config, providers.clone());
        #[cfg(feature = "oidc")]
        let oidc = match read.oidc.clone().map(start).transpose() {
            Ok(oidc) => oidc,
            Err(e) => {
                problems.push(e);
                None
            }
        };
        #[cfg(feature = "accounts")]
        if let (Some(oidc), Some(accounts)) = (&oidc, &read.accounts)
            && accounts.break_glass
            && oidc.login().0.trim_end_matches('/') == ui.public_url
        {
            let problem = Msg::AccountsBreakGlassIssuer.text();
            problems.push(at("accounts.break_glass", problem));
        }
        #[cfg(feature = "postgres")]
        if let Some(postgres) = &read.postgres
            && let Err(version) = crate::postgres::probe(postgres)
        {
            problems.push(at("postgres", version));
        }
        #[cfg(feature = "directory")]
        let scim = match (&read.directory, &read.postgres) {
            (Some(directory), Some(postgres)) => match directory.token() {
                Ok(token) => Some(crate::directory::scim::Scim::new(
                    directory,
                    &token,
                    postgres.clone(),
                    &ui.public_url,
                )),
                Err((key, problem)) => {
                    problems.push(at(key, problem));
                    None
                }
            },
            _ => None,
        };
        #[cfg(feature = "directory")]
        let directory = Self::directory_of(&read);
        #[cfg(feature = "accounts")]
        let accounts = match (&read.accounts, problems.is_empty()) {
            (Some(section), true) => match accounts::Local::start(section) {
                Ok(local) => Some(local),
                Err((key, problem)) => {
                    problems.push(at(key, problem));
                    None
                }
            },
            _ => None,
        };
        #[cfg(feature = "postgres")]
        let audit = crate::audit::Audit::new(read.postgres.clone());
        #[cfg(not(feature = "postgres"))]
        let audit = crate::audit::Audit::default();
        #[cfg(feature = "oidc")]
        let issuer = oidc.as_ref().map(|oidc| origin(oidc.login().0));
        #[cfg(not(feature = "oidc"))]
        let issuer: Option<&str> = None;
        let (Some(root), true) = (root, problems.is_empty()) else {
            let lines: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
            return Err(lines.join("\n").into());
        };
        let table: toml::Table = toml::from_str(&src)?;
        #[cfg_attr(not(feature = "directory"), expect(unused_mut))]
        let mut loaded = serde_json::to_value(table)?;
        #[cfg(feature = "directory")]
        if let Some(directory) = loaded.get_mut("directory").and_then(Value::as_object_mut)
            && !directory.contains_key("token_file")
        {
            (directory.entry("token_env")).or_insert(crate::directory::TOKEN_ENV.into());
        }
        let holds = Holds::of(file, &read);
        Ok(Self {
            file: file.to_path_buf(),
            bootstrap: read.clone(),
            readers: Arc::new(tokio::sync::Semaphore::new(read::READERS_MAX)),
            #[cfg(feature = "github-app")]
            apps: Manifest::github_apps(file)?.1,
            ui,
            root,
            loaded,
            csp: header::HeaderValue::from_str(&csp(issuer))?,
            #[cfg(feature = "oidc")]
            oidc,
            #[cfg(feature = "oidc")]
            providers,
            holds,
            #[cfg(all(feature = "oidc", feature = "generation-pointer"))]
            entries: std::sync::Mutex::new(None),
            audit,
            #[cfg(feature = "accounts")]
            accounts,
            #[cfg(feature = "directory")]
            scim,
            #[cfg(feature = "directory")]
            directory,
        })
    }

    #[cfg(feature = "directory")]
    fn directory_of(read: &Manifest) -> Option<crate::directory::read::Directory> {
        let (directory, postgres) = (read.directory.as_ref()?, read.postgres.as_ref()?);
        Some(crate::directory::read::Directory::new(
            directory,
            postgres.clone(),
        ))
    }

    #[cfg(feature = "directory")]
    async fn scim(
        &self,
        scim: &crate::directory::scim::Scim,
        bound: SocketAddr,
        parts: &hyper::http::request::Parts,
        body: Incoming,
    ) -> Answer {
        use crate::directory::scim::{self, Reply};
        let method = &parts.method;
        let write = matches!(
            *method,
            Method::POST | Method::PUT | Method::PATCH | Method::DELETE
        );
        let headers = &parts.headers;
        let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
        let reply = if !host.is_some_and(|host| self.hosts(host, bound)) {
            None
        } else if headers.contains_key(header::ORIGIN) {
            Some(Reply::error(
                StatusCode::FORBIDDEN,
                None,
                scim::ORIGIN_REFUSED,
            ))
        } else if !scim.admits(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok()),
        ) {
            Some(Reply::error(
                StatusCode::UNAUTHORIZED,
                None,
                scim::UNAUTHORIZED,
            ))
        } else {
            let json = (headers
                .get(header::CONTENT_TYPE)
                .and_then(|c| c.to_str().ok()))
            .and_then(|c| c.split(';').next())
            .is_some_and(|c| {
                let c = c.trim();
                c.eq_ignore_ascii_case("application/scim+json")
                    || c.eq_ignore_ascii_case("application/json")
            });
            let body = match matches!(*method, Method::POST | Method::PUT | Method::PATCH) {
                false => Ok(Bytes::new()),
                true => match Limited::new(body, scim::SCIM_BODY_MAX).collect().await {
                    Ok(collected) => Ok(collected.to_bytes()),
                    Err(e) if e.is::<http_body_util::LengthLimitError>() => Err(Reply::error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        None,
                        scim::TOO_LARGE,
                    )),
                    Err(_) => Err(Reply::error(
                        StatusCode::BAD_REQUEST,
                        Some("invalidSyntax"),
                        scim::UNREADABLE,
                    )),
                },
            };
            Some(match body {
                Err(refused) => refused,
                Ok(body) if !body.is_empty() && !json => {
                    Reply::error(StatusCode::UNSUPPORTED_MEDIA_TYPE, None, scim::MEDIA_TYPE)
                }
                Ok(body) => {
                    let (path, query) = (parts.uri.path(), parts.uri.query());
                    scim.answer(method, path, query, &body).await
                }
            })
        };
        let path = parts.uri.path();
        let status = reply.as_ref().map_or(StatusCode::FORBIDDEN, |r| r.status);
        if write {
            let named = reply.as_ref().and_then(|r| r.named.as_deref());
            let (resource, id) = (scim::resource(path), named.unwrap_or("-"));
            eprintln!("scim: {method} {resource} {id}: {}", status.as_u16());
        }
        let Some(reply) = reply else {
            return plain(StatusCode::FORBIDDEN, HOST_REFUSED);
        };
        let mut answer = Response::builder()
            .status(reply.status)
            .header(header::CACHE_CONTROL, "no-store");
        if let Some(location) = &reply.location {
            answer = answer.header(header::LOCATION, location);
        }
        if reply.status == StatusCode::UNAUTHORIZED {
            answer = answer.header(header::WWW_AUTHENTICATE, "Bearer");
        }
        let body = match &reply.body {
            Some(document) => {
                answer = answer.header(header::CONTENT_TYPE, "application/scim+json");
                Full::from(document.to_string())
            }
            None => Full::default(),
        };
        answer.body(body.boxed()).expect("a valid response")
    }

    fn secure(&self, mut answer: Answer) -> Answer {
        let headers = answer.headers_mut();
        headers.insert("x-content-type-options", "nosniff".parse().unwrap());
        headers.insert("referrer-policy", "no-referrer".parse().unwrap());
        headers.insert("content-security-policy", self.csp.clone());
        answer
    }

    fn checked(
        self: &Arc<Self>,
        bound: SocketAddr,
        request: &hyper::http::request::Parts,
    ) -> std::result::Result<Session, Box<Answer>> {
        let headers = &request.headers;
        let write = !matches!(request.method, Method::GET | Method::HEAD);
        let route = request.uri.path().strip_prefix("/api/v1/");
        let operation = route.and_then(write::operation).filter(|_| write);
        let audit = |subject: Option<&str>, outcome: &str| {
            if let Some(operation) = operation {
                write::audit(subject, None, operation, "-", outcome);
            }
        };
        let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
        if !host.is_some_and(|host| self.hosts(host, bound)) {
            audit(None, HOST_REFUSED);
            return Err(Box::new(plain(StatusCode::FORBIDDEN, HOST_REFUSED)));
        }
        match headers
            .get(header::ORIGIN)
            .map(|o| o.to_str().unwrap_or(""))
        {
            Some(origin) if !origin.eq_ignore_ascii_case(&self.ui.public_url) => {
                audit(None, ORIGIN_REFUSED);
                return Err(Box::new(plain(StatusCode::FORBIDDEN, ORIGIN_REFUSED)));
            }
            None if write => {
                audit(None, WRITE_REFUSED);
                return Err(Box::new(plain(StatusCode::FORBIDDEN, WRITE_REFUSED)));
            }
            _ => {}
        }
        let disabled = |subject: Option<&str>| {
            let message = Msg::UiWritesDisabled.text();
            audit(subject, &message);
            Err(Box::new(error(
                StatusCode::FORBIDDEN,
                "forbidden",
                &message,
            )))
        };
        #[cfg(feature = "accounts")]
        let admitted_first = (self.accounts.as_ref()).is_some_and(|local| !local.break_glass);
        #[cfg(not(feature = "accounts"))]
        let admitted_first = false;
        let writes_off = operation.is_some() && !self.ui.operations;
        if writes_off && !admitted_first {
            return disabled(None);
        }
        #[cfg(feature = "workspaces")]
        let writes =
            operation.is_some() || (write && route.is_some_and(|r| r.starts_with("workspaces")));
        #[cfg(not(feature = "workspaces"))]
        let writes = operation.is_some();
        let session = match self.admit(headers, route, writes) {
            Ok(session) => session,
            Err((refused, message)) => {
                if !message.is_empty() {
                    audit(None, &message);
                }
                return Err(refused);
            }
        };
        if writes_off {
            return disabled(session.subject.as_deref());
        }
        #[cfg(feature = "accounts")]
        let subject = session.subject.clone();
        #[cfg(feature = "accounts")]
        let administering = write && route.is_some_and(|r| self.has_administration(r));
        #[cfg(feature = "accounts")]
        let session = match self.standing(session, route, writes || administering) {
            Ok(session) => session,
            Err((refused, message)) => {
                audit(subject.as_deref(), &message);
                return Err(refused);
            }
        };
        #[cfg(feature = "accounts")]
        if let Some(refused) = self.administration_refused(&session, route, &request.method) {
            return Err(refused);
        }
        if let Some(operation) = operation {
            let team =
                session.is_team_operator() && route.is_some_and(|r| r.starts_with("workspace/"));
            let anonymous = session.subject.as_deref().is_none_or(str::is_empty);
            if !(session.operator || team) || anonymous {
                let message = match session.is_team_operator() && !anonymous {
                    true => Msg::UiGlobalOperatorRequired.text(),
                    false => Msg::UiOperatorRequired.text(),
                };
                write::audit(
                    session.subject.as_deref(),
                    session.owners(),
                    operation,
                    "-",
                    &message,
                );
                if let Some(action) = route.and_then(write::recorded) {
                    let actor = write::actor(&session, session.owners());
                    (self.audit).refused(&actor, action, &json!({}), &message);
                }
                return Err(Box::new(error(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    &message,
                )));
            }
        }
        Ok(session)
    }

    fn answer(self: &Arc<Self>, request: &Request<Body>, session: &Session) -> Answer {
        let path = request.uri().path();
        let route = path.strip_prefix("/api/v1/");
        let write = !matches!(*request.method(), Method::GET | Method::HEAD);
        match route {
            Some(route) => self.api(request, route, session),
            None if write => plain(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed\n"),
            None => self.file(path),
        }
    }

    fn admit(
        self: &Arc<Self>,
        headers: &HeaderMap,
        route: Option<&str>,
        write: bool,
    ) -> std::result::Result<Session, (Box<Answer>, String)> {
        #[cfg(feature = "accounts")]
        if let Some(local) = &self.accounts {
            if !local.break_glass {
                return self.admit_local(local, headers, route);
            }
            if let Some(admitted) = self.admit_break_glass(local, headers, route) {
                return admitted;
            }
        }
        let open = matches!(route, None | Some("config"));
        #[cfg(feature = "oidc")]
        if let Some(oidc) = &self.oidc
            && !open
        {
            return match oidc.admit_console(
                headers,
                &self.ui.audience,
                self.ui.operator_groups.as_deref(),
                self.ui.operator_claim.as_deref(),
                self.user_claim(),
            ) {
                Ok((admitted, user)) => {
                    #[cfg(feature = "directory")]
                    let down = self.directory_refusal(user.as_deref(), write)?;
                    #[cfg(not(feature = "directory"))]
                    let _ = user;
                    let teams =
                        self.teams_on(route, admitted.operator, &admitted.groups, &admitted.grant);
                    #[cfg_attr(not(feature = "directory"), expect(unused_mut))]
                    let mut session = Session::admitted(admitted, teams);
                    #[cfg(feature = "directory")]
                    {
                        session.directory_down = down;
                    }
                    Ok(session)
                }
                Err((status, challenge)) => {
                    let (code, message) = match status {
                        StatusCode::UNAUTHORIZED => ("unauthorized", Msg::UiUnauthorized.text()),
                        _ => ("forbidden", Msg::UiScope.text()),
                    };
                    let mut answer = error(status, code, &message);
                    if let Ok(challenge) = header::HeaderValue::from_str(&challenge) {
                        answer
                            .headers_mut()
                            .insert(header::WWW_AUTHENTICATE, challenge);
                    }
                    Err((Box::new(answer), message))
                }
            };
        }
        let _ = (headers, open, write);
        Ok(Session::anonymous())
    }

    #[cfg(feature = "oidc")]
    fn teams_on(
        self: &Arc<Self>,
        route: Option<&str>,
        operator: bool,
        groups: &[String],
        grant: &crate::grant::Grant,
    ) -> Vec<String> {
        let role_route = route == Some("session") || route.and_then(write::operation).is_some();
        match operator || !role_route {
            true => Vec::new(),
            false => self.teams(groups, grant),
        }
    }

    #[cfg(feature = "oidc")]
    fn user_claim(&self) -> Option<&str> {
        #[cfg(feature = "directory")]
        return self.directory.as_ref().map(|d| d.user_claim());
        #[cfg(not(feature = "directory"))]
        None
    }

    fn hosts(&self, host: &str, bound: SocketAddr) -> bool {
        let public = (self.ui.public_url.split_once("://")).map_or("", |(_, authority)| authority);
        host.eq_ignore_ascii_case(public) || (bound.ip().is_loopback() && host == bound.to_string())
    }

    fn api(self: &Arc<Self>, request: &Request<Body>, route: &str, session: &Session) -> Answer {
        let method = request.method();
        let read = matches!(*method, Method::GET | Method::HEAD);
        if !read && write::operation(route).is_some() {
            return self.write(route, method, request.body(), session);
        }
        #[cfg(feature = "workspaces")]
        if let Some(answer) = self.workspaces(request, route, session) {
            return answer;
        }
        if read
            && let Some(identity) =
                (route.strip_prefix("repositories/")).and_then(|rest| rest.strip_suffix("/export"))
        {
            return self.export(identity, request.uri().query(), session);
        }
        if route == "query" && *method == Method::POST {
            return self.query(request.body(), session);
        }
        #[cfg(feature = "accounts")]
        if let Some(local) = &self.accounts {
            match (route, *method == Method::POST) {
                (accounts::LOGIN, true) => return self.login(local, request.body()),
                (accounts::PASSWORD, true) if session.account => {
                    return self.password(local, request.body(), session);
                }
                (accounts::TOKENS, true) if session.account && !local.break_glass => {
                    return self.agent_token(local, request.body(), session);
                }
                (accounts::GITHUB, true) if session.account && !local.break_glass => {
                    return self.verify_github(local, request.body(), session);
                }
                (accounts::GITHUB_REMOVE, true) if session.account && !local.break_glass => {
                    return self.remove_own_github(local, session);
                }
                _ => {}
            }
            let body = request.body();
            if let Some(answer) = self.administer(local, method, route, body, session) {
                return answer;
            }
        }
        #[cfg(feature = "workspaces")]
        if route == "catalog" && read {
            return self.catalog(request.uri().query(), session);
        }
        #[cfg(feature = "workspaces")]
        if route == "catalog/check" && *method == Method::POST {
            return self.catalog_check(request.body(), session);
        }
        if read && route == "audit" && self.audit.on() {
            return self.audit_page(request.uri().query(), session);
        }
        #[cfg(feature = "directory")]
        if read && route == "directory/groups" && self.directory.is_some() {
            return self.directory_groups(request.uri().query(), session);
        }
        if read && let Some(answer) = self.operational(route, session) {
            return answer;
        }
        let document = match route {
            "config" if read => Some(self.config()),
            "session" if read => Some(session.document()),
            "manifest" if read => Some(self.manifest(session)),
            #[cfg(feature = "oidc")]
            "admission" if read => {
                let document = (self.oidc.as_ref()).map(|o| {
                    #[cfg_attr(not(feature = "directory"), expect(unused_mut))]
                    let mut document = o.admission(&|i| session.reads(i));
                    #[cfg(feature = "directory")]
                    self.with_directory_counts(&mut document, session);
                    document
                });
                #[cfg(feature = "accounts")]
                let document = match &self.accounts {
                    Some(local) => Some(self.accounts_admission(local, document, session)),
                    None => document,
                };
                document
            }
            #[cfg(feature = "oidc")]
            "admission/check" if *method == Method::POST => match &self.oidc {
                Some(oidc) => return self.check(oidc, request.body(), session),
                None => None,
            },
            _ => None,
        };
        match document {
            Some(document) => json_answer(StatusCode::OK, &document),
            None => error(StatusCode::NOT_FOUND, "not_found", &Msg::UiNoRoute.text()),
        }
    }

    fn config(&self) -> Value {
        let mut config = json!({
            "version": VERSION,
            "operations": self.ui.operations,
            "public_url": self.ui.public_url,
        });
        if self.audit.on() {
            config["audit"] = true.into();
        }
        #[cfg(feature = "workspaces")]
        if self.bootstrap.catalog.is_some() && self.providers.github().is_some() {
            config["catalog"] = true.into();
        }
        #[cfg(feature = "workspaces")]
        if let Some(section) = &self.bootstrap.workspaces {
            config["workspaces"] = json!({"max_entries": section.max_entries});
        }
        #[cfg(feature = "proposals")]
        if self.bootstrap.proposals.is_some() {
            config["proposals"] = true.into();
        }
        #[cfg(feature = "accounts")]
        match &self.accounts {
            Some(local) if local.break_glass => config["break_glass"] = true.into(),
            Some(_) => config["sign_in"] = "accounts".into(),
            None => {}
        }
        #[cfg(feature = "oidc")]
        if let Some(oidc) = &self.oidc {
            let (issuer, scopes) = oidc.login();
            config["sign_in"] = "oidc".into();
            config["issuer"] = issuer.into();
            config["client_id"] = self.ui.client_id.clone().into();
            config["scopes"] = scopes.into();
            config["audience"] = self.ui.audience.clone().into();
        }
        config
    }

    fn manifest(&self, session: &Session) -> Value {
        let mut errors: Vec<String> = match Manifest::read(&self.file) {
            Ok(_) => Vec::new(),
            Err(e) => e.to_string().lines().map(String::from).collect(),
        };
        let mut loaded = self.loaded.clone();
        #[cfg(feature = "workspaces")]
        if !session.operator {
            if let Some(catalog) = loaded.get_mut("catalog").and_then(Value::as_object_mut) {
                catalog.remove("allow");
            }
            if let Some(section) = loaded.get_mut("workspaces").and_then(Value::as_object_mut) {
                section.remove("mirrors");
            }
        }
        if !session.whole() {
            if let Some(Value::Array(entries)) = loaded.get_mut("repository") {
                read::filter_entries(entries, "repo", session);
            }
            for table in GROUP_TABLES {
                let Some(groups) = loaded.pointer_mut(table).and_then(Value::as_object_mut) else {
                    continue;
                };
                for identities in groups.values_mut().filter_map(Value::as_array_mut) {
                    identities.retain(|identity| {
                        identity
                            .as_str()
                            .is_some_and(|i| i == "*" || session.reads(i))
                    });
                }
            }
            let on_disk = fs::read_to_string(&self.file).ok();
            let on_disk = on_disk.and_then(|src| toml::from_str::<toml::Table>(&src).ok());
            let hidden: Option<Vec<String>> = on_disk.map(|table| {
                let table = serde_json::to_value(table).unwrap_or_default();
                (identities(&table).chain(identities(&self.loaded)))
                    .filter(|i| !session.reads(i))
                    .map(str::to_string)
                    .collect()
            });
            let file = self.file.display().to_string();
            cut_named(&mut errors, hidden.as_deref(), &file);
        }
        let modules: Vec<Value> = (modules::MODULES.iter())
            .map(|m| json!({"section": m.section, "feature": m.feature, "built": m.built}))
            .collect();
        json!({
            "file": self.file.display().to_string(),
            "manifest": loaded,
            "modules": modules,
            "errors": errors,
        })
    }

    #[cfg(feature = "oidc")]
    fn check(self: &Arc<Self>, oidc: &crate::oidc::Oidc, body: &Body, session: &Session) -> Answer {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Pasted {
            token: String,
        }
        let body = match body {
            Ok(body) => body,
            Err(StatusCode::PAYLOAD_TOO_LARGE) => {
                let message = Msg::UiTooLarge(CHECK_MAX).text();
                return error(StatusCode::PAYLOAD_TOO_LARGE, "too_large", &message);
            }
            Err(status) => return error(*status, "bad_request", &Msg::UiCheckBody.text()),
        };
        let Ok(pasted) = serde_json::from_slice::<Pasted>(body) else {
            return error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                &Msg::UiCheckBody.text(),
            );
        };
        let identities = match self.served_identities() {
            Ok(identities) => identities,
            Err(message) => return error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message),
        };
        let served: Vec<&str> = identities.iter().map(String::as_str).collect();
        let checked = oidc.check(&pasted.token, &served);
        let admitted = |reads: &dyn Fn(&str) -> bool| {
            (served.iter())
                .filter(|i| reads(i) && session.reads(i))
                .collect::<Vec<_>>()
        };
        let identities = (checked.grant).map(|grant| admitted(&|i| grant.reads(i)));
        let checks = json!({
            "signature": checked.signature,
            "issuer": checked.issuer,
            "audience": checked.audience,
            "lifetime": checked.lifetime,
            "scopes": checked.scopes,
        });
        let mut answer = json!({"checks": checks, "identities": identities});
        if self.providers.github().is_some() {
            let sorted = |reads: &dyn Fn(&str) -> bool| {
                let mut identities = admitted(reads);
                identities.sort();
                identities
            };
            answer["sources"] = match checked.sources {
                None => Value::Null,
                Some((claims, github)) => json!({
                    "claims": match claims {
                        crate::grant::Grant::All => json!("*"),
                        claims => json!(sorted(&|i| claims.reads(i))),
                    },
                    "github": sorted(&|i| github.iter().any(|g| g == i)),
                }),
            };
        }
        json_answer(StatusCode::OK, &answer)
    }

    fn query(&self, body: &Body, session: &Session) -> Answer {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Query {
            repository: String,
            #[serde(default)]
            generation: Option<String>,
            #[serde(default)]
            workspace: Option<String>,
            tool: String,
            arguments: Value,
        }
        let bad = |message: &str| error(StatusCode::BAD_REQUEST, "bad_request", message);
        let body = match body {
            Ok(body) => body,
            Err(StatusCode::PAYLOAD_TOO_LARGE) => {
                let message = Msg::UiTooLarge(QUERY_MAX).text();
                return error(StatusCode::PAYLOAD_TOO_LARGE, "too_large", &message);
            }
            Err(_) => return bad(&Msg::UiQueryBody.text()),
        };
        let Ok(query) = serde_json::from_slice::<Query>(body) else {
            return bad(&Msg::UiQueryBody.text());
        };
        let ask = match crate::mcp::served_call(&query.tool, query.arguments) {
            Ok(ask) => ask,
            Err(None) => return bad(&Msg::UiQueryBody.text()),
            Err(Some(refused)) => return bad(&refused),
        };
        let (engine, generation) = match self.open(
            &query.repository,
            query.generation.as_deref(),
            query.workspace.as_deref(),
            session,
        ) {
            Ok(opened) => opened,
            Err(refused) => return *refused,
        };
        match ask(&engine) {
            Ok(text) => json_answer(
                StatusCode::OK,
                &json!({"text": text, "generation": generation}),
            ),
            Err(e) => error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "tool_error",
                &e.to_string(),
            ),
        }
    }

    fn audit_page(&self, query: Option<&str>, session: &Session) -> Answer {
        if !session.operator {
            let message = Msg::UiOperatorRequired.text();
            return error(StatusCode::FORBIDDEN, "forbidden", &message);
        }
        let (mut before, mut limit) = (None, None);
        for pair in query.into_iter().flat_map(|q| q.split('&')) {
            let number = |text: &str| {
                let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
                digits.then(|| text.parse::<i64>().ok()).flatten()
            };
            let slot = match pair.split_once('=') {
                Some(("before", id)) if before.is_none() => {
                    before = number(id).filter(|id| *id > 0).map(Some);
                    before.is_some()
                }
                Some(("limit", n)) if limit.is_none() => {
                    limit = number(n)
                        .filter(|n| (1..=AUDIT_LIMIT_MAX).contains(n))
                        .map(Some);
                    limit.is_some()
                }
                _ => false,
            };
            if !slot {
                return error(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    &Msg::UiAuditQuery.text(),
                );
            }
        }
        let limit = limit.flatten().unwrap_or(AUDIT_LIMIT);
        match self.audit.page(before.flatten(), limit) {
            Ok(page) => {
                for _ in 0..page.skipped {
                    let table = self.audit.table();
                    eprintln!("ui: {}", Msg::AuditBadRecord(&table).text());
                }
                json_answer(
                    StatusCode::OK,
                    &json!({"records": page.records, "next": page.next}),
                )
            }
            Err(reason) => {
                eprintln!(
                    "ui: {}",
                    Msg::PostgresUnreachable(&reason).text().escape_debug()
                );
                let message = Msg::AuditUnavailable.text();
                error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "audit_unavailable",
                    &message,
                )
            }
        }
    }

    fn export(&self, identity: &str, query: Option<&str>, session: &Session) -> Answer {
        let identity = percent_decode_str(identity).decode_utf8_lossy();
        let generation = (query.into_iter().flat_map(|q| q.split('&')))
            .find_map(|pair| pair.strip_prefix("generation="))
            .map(|id| percent_decode_str(id).decode_utf8_lossy());
        let engine = match self.open(&identity, generation.as_deref(), None, session) {
            Ok((engine, _)) => engine,
            Err(refused) => return *refused,
        };
        match engine.export() {
            Ok(export) => Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Full::from(export).boxed())
                .expect("a valid response"),
            Err(e) => error(
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                &e.to_string(),
            ),
        }
    }

    fn open(
        &self,
        repository: &str,
        generation: Option<&str>,
        workspace: Option<&str>,
        session: &Session,
    ) -> std::result::Result<(Engine, Option<String>), Box<Answer>> {
        let unknown = || {
            let message = Msg::UiNoRepository(repository).text();
            Box::new(error(StatusCode::NOT_FOUND, "not_found", &message))
        };
        let unavailable = |message: String| {
            Box::new(error(
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                &message,
            ))
        };
        let not_held = |id: &str, held: &[String]| {
            let message = Msg::UiNotHeld { id, held }.text();
            Box::new(error(StatusCode::NOT_FOUND, "not_found", &message))
        };
        let reads = |identity: &str| session.reads(identity);
        if !session.reads(repository) {
            return Err(unknown());
        }
        #[cfg(feature = "workspaces")]
        let through = workspace.filter(|_| self.bootstrap.workspaces.is_some());
        #[cfg(not(feature = "workspaces"))]
        let through: Option<&str> = None;
        if workspace.is_some() && through.is_none() {
            return Err(unknown());
        }
        match &self.holds {
            Holds::Listed(listed) => {
                if through.is_some() {
                    return Err(unknown());
                }
                if let Some(id) = generation {
                    return Err(not_held(id, &[]));
                }
                if !listed.entries.iter().any(|l| l.repo == repository) {
                    return Err(unknown());
                }
                Engine::open_listed(listed, repository, &self.file, &reads)
                    .map(|engine| (engine, None))
                    .map_err(|e| unavailable(e.to_string()))
            }
            #[cfg(feature = "snapshot")]
            Holds::Generations(current) => {
                let current = current.as_ref().map_err(|e| unavailable(e.clone()))?;
                let local = current.local();
                crate::repo::isolate(local);
                let held = match generation {
                    None => current.get(),
                    Some(id) => match current.pinned(id) {
                        Ok(Some(held)) => Ok(held),
                        Ok(None) => return Err(not_held(id, &crate::snapshot::serve::ids(local))),
                        Err(e) => Err(e),
                    },
                };
                let held = held.map_err(|e| unavailable(e.to_string()))?;
                #[cfg(feature = "workspaces")]
                if let Some(id) = through {
                    if !self.through_workspace(id, repository, &held, session) {
                        let message = Msg::WorkspacesNotFound.text();
                        return Err(Box::new(error(
                            StatusCode::NOT_FOUND,
                            "not_found",
                            &message,
                        )));
                    }
                    let listed = held.context(id).map_err(|e| unavailable(e.to_string()))?;
                    let listed = listed.ok_or_else(unknown)?;
                    return Engine::open_context(&held, &listed, repository, &reads)
                        .map(|engine| (engine, Some(held.id.clone())))
                        .map_err(|e| unavailable(e.to_string()));
                }
                if !held.listed.entries.iter().any(|l| l.repo == repository) {
                    return Err(unknown());
                }
                Engine::open_held(&held, repository, &reads)
                    .map(|engine| (engine, Some(held.id.clone())))
                    .map_err(|e| unavailable(e.to_string()))
            }
        }
    }

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

fn csp(issuer: Option<&str>) -> String {
    let connect = issuer.map_or("'self'".to_string(), |issuer| format!("'self' {issuer}"));
    format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src {connect}; \
         frame-ancestors 'none'"
    )
}

#[cfg(feature = "oidc")]
fn origin(url: &str) -> &str {
    let after = url.find("://").map_or(0, |at| at + 3);
    let end = url[after..].find('/').map_or(url.len(), |at| after + at);
    &url[..end]
}

const GROUP_TABLES: [&str; 2] = ["/oidc/groups", "/accounts/groups"];

fn identities(manifest: &Value) -> impl Iterator<Item = &str> {
    let entries = manifest.get("repository").and_then(Value::as_array);
    let repositories = (entries.into_iter().flatten()).flat_map(|entry| {
        let own = entry.get("repo").and_then(Value::as_str);
        let peers = entry
            .get("peer")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|peer| peer.get("repo").and_then(Value::as_str));
        own.into_iter().chain(peers)
    });
    let groups = (GROUP_TABLES.iter())
        .filter_map(|table| manifest.pointer(table).and_then(Value::as_object))
        .flat_map(|groups| groups.values())
        .flat_map(|value| {
            value.as_str().into_iter().chain(
                value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            )
        });
    repositories.chain(groups)
}

fn cut_named(errors: &mut [String], hidden: Option<&[String]>, file: &str) {
    for line in errors {
        let named = hidden.is_none_or(|h| h.iter().any(|i| line.contains(i.as_str())));
        if named {
            *line = position(line, file);
        }
    }
}

fn position(line: &str, file: &str) -> String {
    let rest = line.strip_prefix(file).and_then(|r| r.strip_prefix(':'));
    let number: String = (rest.unwrap_or("").chars())
        .take_while(char::is_ascii_digit)
        .collect();
    match number.is_empty() {
        true => file.to_string(),
        false => format!("{file}:{number}"),
    }
}

fn built_for(root: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Built {
        version: String,
    }
    let bytes = crate::config::read_bounded(&root.join(VERSION_FILE), UI_VERSION_MAX).ok()?;
    Some(serde_json::from_slice::<Built>(&bytes).ok()?.version)
}

fn relative(path: &str) -> Option<String> {
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    let rest = decoded.strip_prefix('/')?;
    if rest.is_empty() {
        return Some("index.html".into());
    }
    let safe = |s: &str| !matches!(s, "" | "." | "..") && !s.contains(['\0', '\\']);
    rest.split('/').all(safe).then(|| rest.to_string())
}

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

fn error(status: StatusCode, code: &str, message: &str) -> Answer {
    json_answer(
        status,
        &json!({"error": {"code": code, "message": message}}),
    )
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

    #[test]
    fn an_error_naming_a_hidden_identity_is_cut_to_its_position() {
        let file = "ops/meada.toml";
        let hidden = ["private.example/team/api".to_string()];
        let line = |text: &str| format!("{file}:7: {text}");
        let mut errors = vec![
            line("`serve.strip`: private.example/team/api is no rule"),
            line("`serve.prefix`: must start with `/`"),
            "ops/meada.toml: not valid".to_string(),
        ];
        cut_named(&mut errors, Some(&hidden), file);
        assert_eq!(
            errors,
            [
                "ops/meada.toml:7".to_string(),
                line("`serve.prefix`: must start with `/`"),
                "ops/meada.toml: not valid".to_string(),
            ]
        );
        cut_named(&mut errors, None, file);
        assert_eq!(errors, ["ops/meada.toml:7", "ops/meada.toml:7", file]);
    }
}
