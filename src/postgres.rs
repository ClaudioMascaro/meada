//! The data layer (ADR 0016, feature `postgres`): the durable mutable state of server mode, in a
//! Postgres (15 and later) that the bootstrap manifest's `[postgres]` section names without a
//! secret. The URL comes only from the variable `url_env` or the file `url_file` names, is read
//! when a process connects and kept in memory only; a URL that fails a check, an unset variable
//! or an unreadable file is an error naming the key, never the URL or a part of it. Once
//! connected, an error never carries the password, but may name the host (a certificate for
//! another name) or the user (a refused login). A host outside loopback always connects over TLS with its name verified, against
//! `ca_file` alone when set, else the system roots; a loopback host connects in plain TCP unless
//! `ca_file` is set (`over_tls`). The client is tokio-postgres on the binary's tokio runtime,
//! with rustls and ring: no ORM and no second TLS stack.
//!
//! Every table lives in `[postgres] schema`, and every statement names it: Meada writes nothing
//! outside it (ADR 0002, invariant 3). `meada postgres migrate` (`migrate`) is the only command
//! that creates or alters a table: under an advisory lock keyed by the schema, it applies each
//! migration from the stored version to the binary's, each in its own transaction, and never
//! lowers a version. Every other connection goes through `connect`, which reads the version and
//! refuses one the binary does not know (an absent schema counts as lower). A CLI command or the
//! console refuses on that error; a node writes it once on stderr and treats Postgres as
//! unreachable, keeping its readiness.
//!
//! Who connects: the console (audit, directory, permissions, overage), nodes with
//! `[oidc.github]` or overage through the directory (reads only), the job's `meada github-app
//! roles` and the CLI twins that write. `meada index` never does. Three Postgres roles are
//! recommended: one for `migrate`, which owns the schema; one that writes (the console, the role
//! check, the CLI twins); one that reads (nodes).

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use rustls::pki_types::ServerName;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_postgres::config::SslMode;
use tokio_postgres::tls::{ChannelBinding, NoTls, TlsConnect};

use crate::modules::{Problem, Secret, at_least};
use crate::msg::Msg;

/// The variable `url_env` names by default.
const URL_ENV: &str = "MEADA_POSTGRES_URL";
// ponytail: the most `url_file` may hold; past it the connection fails, naming the file.
const URL_MAX: u64 = 4096;
// ponytail: the most `ca_file` may hold; past it the connection fails, naming the file.
const CA_MAX: u64 = 1 << 20;
/// Postgres's longest identifier (`NAMEDATALEN - 1`).
const SCHEMA_MAX: usize = 63;
/// The longest `statement_timeout` Postgres takes, in seconds (`INT_MAX` milliseconds).
const STATEMENT_MAX: u64 = i32::MAX as u64 / 1000;

/// The migrations in order: migration `n` is `MIGRATIONS[n - 1]`, `{schema}` standing for the
/// quoted schema. `migrate` creates the schema itself, in migration 1's transaction, when it is
/// absent. A later ticket appends the next one when it merges, never editing one before it.
const MIGRATIONS: &[&str] = &[
    // 1: the version the schema holds, one row that `migrate` alone writes.
    "CREATE TABLE {schema}.schema_version (version integer NOT NULL);
     INSERT INTO {schema}.schema_version VALUES (0);",
    // 2: the audit (`crate::audit`), one row per write, never deleted by Meada.
    "CREATE TABLE {schema}.audit (
       id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
       \"time\" timestamptz NOT NULL DEFAULT now(),
       via text NOT NULL,
       subject text,
       login text,
       role text NOT NULL,
       owners jsonb,
       action text NOT NULL,
       arguments jsonb NOT NULL,
       outcome text NOT NULL,
       message text
     );",
    // 3: the directory (`crate::directory`, #334): users and groups as the identity provider
    // sent them, Meada's ids 32 lowercase hex characters, the memberships, and the revocation
    // markers a deleted user leaves until it is provisioned again, active.
    "CREATE TABLE {schema}.directory_users (
       id text PRIMARY KEY DEFAULT replace(gen_random_uuid()::text, '-', ''),
       user_name text NOT NULL,
       external_id text,
       active boolean NOT NULL,
       resource jsonb NOT NULL,
       created timestamptz NOT NULL DEFAULT now(),
       last_modified timestamptz NOT NULL DEFAULT now()
     );
     CREATE UNIQUE INDEX ON {schema}.directory_users (lower(user_name));
     CREATE UNIQUE INDEX ON {schema}.directory_users (external_id);
     CREATE TABLE {schema}.directory_groups (
       id text PRIMARY KEY DEFAULT replace(gen_random_uuid()::text, '-', ''),
       display_name text NOT NULL,
       external_id text,
       resource jsonb NOT NULL,
       created timestamptz NOT NULL DEFAULT now(),
       last_modified timestamptz NOT NULL DEFAULT now()
     );
     CREATE INDEX ON {schema}.directory_groups (display_name);
     CREATE INDEX ON {schema}.directory_groups (external_id);
     CREATE TABLE {schema}.directory_members (
       group_id text NOT NULL REFERENCES {schema}.directory_groups (id) ON DELETE CASCADE,
       user_id text NOT NULL REFERENCES {schema}.directory_users (id) ON DELETE CASCADE,
       PRIMARY KEY (group_id, user_id)
     );
     CREATE INDEX ON {schema}.directory_members (user_id);
     CREATE TABLE {schema}.directory_revoked (
       user_name text NOT NULL,
       external_id text,
       revoked timestamptz NOT NULL DEFAULT now()
     );",
    // 4: the directory's state (#335), one row the SCIM endpoint updates in each write's
    // transaction: whether a group was ever provisioned (a directory whose final group was
    // deleted differs from one never fed, for `owner_missing`) and the time of the last write.
    // An existing directory starts from what it holds.
    "CREATE TABLE {schema}.directory_state (
       singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
       groups_provisioned boolean NOT NULL,
       last_write timestamptz
     );
     INSERT INTO {schema}.directory_state (groups_provisioned, last_write) VALUES (
       EXISTS (SELECT 1 FROM {schema}.directory_groups),
       greatest(
         (SELECT max(last_modified) FROM {schema}.directory_users),
         (SELECT max(last_modified) FROM {schema}.directory_groups),
         (SELECT max(revoked) FROM {schema}.directory_revoked)
       )
     );",
    // 5: the permissions set (#331), one kept: `meada github-app roles` replaces it in one
    // transaction, its repositories what GitHub showed of each identity on the host (`listed`,
    // `not_found`, `uncovered`, `not_a_repository`), its rows every collaborator GitHub listed,
    // and its exclusions the additions left out (`no_role`, `role_not_accepted`); times from
    // Postgres's clock.
    "CREATE TABLE {schema}.permission_sets (
       id text PRIMARY KEY DEFAULT replace(gen_random_uuid()::text, '-', ''),
       fence bigint NOT NULL,
       version text,
       started timestamptz NOT NULL,
       finished timestamptz NOT NULL DEFAULT clock_timestamp(),
       repositories integer NOT NULL
     );
     CREATE TABLE {schema}.permission_rows (
       set_id text NOT NULL REFERENCES {schema}.permission_sets (id) ON DELETE CASCADE,
       identity text NOT NULL,
       login text NOT NULL,
       role text NOT NULL,
       PRIMARY KEY (set_id, identity, login)
     );
     CREATE INDEX ON {schema}.permission_rows (set_id, login);
     CREATE TABLE {schema}.permission_repositories (
       set_id text NOT NULL REFERENCES {schema}.permission_sets (id) ON DELETE CASCADE,
       identity text NOT NULL,
       coverage text NOT NULL,
       PRIMARY KEY (set_id, identity)
     );
     CREATE TABLE {schema}.permission_exclusions (
       set_id text NOT NULL REFERENCES {schema}.permission_sets (id) ON DELETE CASCADE,
       identity text NOT NULL,
       added_by text NOT NULL,
       reason text NOT NULL,
       PRIMARY KEY (set_id, identity)
     );",
    // 6: where a repository of the permissions set came from (#341, ADR 0021): the workspace
    // selects it (`selected`, every row so far), only `[catalog]` allows it (`catalog`), or both.
    // The catalog is the `catalog` and `both` rows whose coverage is `listed`.
    "ALTER TABLE {schema}.permission_repositories
       ADD COLUMN source text NOT NULL DEFAULT 'selected';",
    // 7: named workspaces (#342, ADR 0021). A workspace is a stable id with a name, a current
    // version and a `seq`, the base every write must still find; `entries` is the identities of
    // the current version in manifest order, a copy that moves with `current` so that a list
    // reads no store document. Operators are (workspace, subject) pairs; a version row says who
    // stored a document of the shared store and when; the history is append only. An exclusion
    // belongs to the deployment workspace (`context` empty) or to one named workspace, and a
    // named entry has no `added_by`.
    "CREATE TABLE {schema}.workspaces (
       id text PRIMARY KEY,
       name text NOT NULL,
       description text NOT NULL DEFAULT '',
       creator text NOT NULL,
       creator_login text,
       created timestamptz NOT NULL DEFAULT now(),
       deleted timestamptz,
       seq bigint NOT NULL,
       current text NOT NULL,
       entries text[] NOT NULL DEFAULT '{}'
     );
     CREATE TABLE {schema}.workspace_operators (
       workspace_id text NOT NULL REFERENCES {schema}.workspaces (id),
       subject text NOT NULL,
       login text,
       added_by text NOT NULL,
       added timestamptz NOT NULL DEFAULT now(),
       PRIMARY KEY (workspace_id, subject)
     );
     CREATE TABLE {schema}.workspace_versions (
       workspace_id text NOT NULL REFERENCES {schema}.workspaces (id),
       version text NOT NULL,
       author text NOT NULL,
       saved timestamptz NOT NULL DEFAULT now(),
       message text NOT NULL,
       PRIMARY KEY (workspace_id, version)
     );
     CREATE TABLE {schema}.workspace_history (
       workspace_id text NOT NULL REFERENCES {schema}.workspaces (id),
       seq bigint NOT NULL,
       action text NOT NULL,
       version text,
       actor text NOT NULL,
       authority text NOT NULL,
       \"time\" timestamptz NOT NULL DEFAULT now(),
       message text,
       PRIMARY KEY (workspace_id, seq)
     );
     ALTER TABLE {schema}.permission_exclusions DROP CONSTRAINT permission_exclusions_pkey;
     ALTER TABLE {schema}.permission_exclusions ADD COLUMN context text NOT NULL DEFAULT '';
     ALTER TABLE {schema}.permission_exclusions ALTER COLUMN added_by DROP NOT NULL;
     ALTER TABLE {schema}.permission_exclusions ADD PRIMARY KEY (set_id, context, identity);",
    // 8: the version lifecycle of named workspaces (#343, ADR 0021 contract 4). A version row
    // keeps the identities of its document, in manifest order, so that a list of versions
    // filters by the reader's grant without reading a store document per row, as `workspaces
    // .entries` does for the current one. Every row so far is a creation's version, which is
    // still the workspace's current one. The column is NOT NULL once filled: a row the backfill
    // missed fails the migration instead of reading as an empty set, which every grant contains.
    "ALTER TABLE {schema}.workspace_versions ADD COLUMN entries text[];
     UPDATE {schema}.workspace_versions v SET entries = w.entries
       FROM {schema}.workspaces w WHERE w.id = v.workspace_id AND w.current = v.version;
     ALTER TABLE {schema}.workspace_versions ALTER COLUMN entries SET NOT NULL;",
];

/// The `[postgres]` section as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    url_env: Option<String>,
    url_file: Option<String>,
    ca_file: Option<String>,
    schema: Option<String>,
    pool_size: Option<i64>,
    connect_timeout_seconds: Option<i64>,
    statement_timeout_seconds: Option<i64>,
}

/// The validated section. It names where the URL is, never the URL, so nothing here is a secret.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    url: Secret,
    ca_file: Option<PathBuf>,
    schema: String,
    /// The most connections a process holds: the callers that pool them (S4 tickets 2, 5, 6
    /// and 8) read it; `migrate` and `status` open one.
    #[allow(dead_code)]
    pool_size: u64,
    connect_timeout: Duration,
    statement_timeout: Duration,
}

impl File {
    /// The section checked; relative paths are relative to `base`, the manifest's directory.
    pub fn validate(self, base: &Path) -> Result<Config, Problem> {
        let both = || ("postgres.url_file", Msg::PostgresBoth.text());
        let url = Secret::named(self.url_env, self.url_file, base, both)?;
        let schema = self.schema.unwrap_or_else(|| "meada".into());
        if !schema_ok(&schema) {
            return Err(("postgres.schema", Msg::PostgresSchema.text()));
        }
        let seconds =
            |key, value, default| at_least(key, value, default, 1).map(Duration::from_secs);
        let statement = "postgres.statement_timeout_seconds";
        Ok(Config {
            url: url.unwrap_or_else(|| Secret::Env(URL_ENV.into())),
            ca_file: self.ca_file.map(|f| base.join(f)),
            schema,
            pool_size: at_least("postgres.pool_size", self.pool_size, 4, 1)?,
            connect_timeout: seconds(
                "postgres.connect_timeout_seconds",
                self.connect_timeout_seconds,
                5,
            )?,
            statement_timeout: match seconds(statement, self.statement_timeout_seconds, 10)? {
                bound if bound.as_secs() > STATEMENT_MAX => {
                    return Err((statement, Msg::AtMost(STATEMENT_MAX).text()));
                }
                bound => bound,
            },
        })
    }
}

/// Whether `schema` is a name Meada accepts: lowercase letters, digits and `_`, starting with a
/// letter, at most `SCHEMA_MAX` characters. Such a name needs no escaping inside double quotes.
fn schema_ok(schema: &str) -> bool {
    schema.len() <= SCHEMA_MAX
        && schema.starts_with(|c: char| c.is_ascii_lowercase())
        && (schema.bytes()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The connection URL's parts, decoded.
#[derive(Debug, PartialEq)]
struct Url {
    user: Option<String>,
    password: Option<Vec<u8>>,
    /// A DNS name or an IP address, an IPv6 one without its brackets.
    host: String,
    port: u16,
    dbname: Option<String>,
}

/// `text` as a connection URL: `postgres://` or `postgresql://`, optionally a user and a
/// password, a TCP host (a name or an IP address, IPv6 in brackets), optionally a port and a
/// database, and nothing after: no query (so no `sslmode`, TLS being Meada's decision) and no
/// fragment. A Unix socket path or a list of hosts is no TCP host.
fn parse(text: &str) -> Result<Url, Msg<'static>> {
    let invalid = || Msg::PostgresUrlInvalid;
    let rest = (text.strip_prefix("postgres://"))
        .or_else(|| text.strip_prefix("postgresql://"))
        .ok_or_else(invalid)?;
    if rest.contains('?') {
        return Err(Msg::PostgresUrlQuery);
    }
    if rest.contains('#') {
        return Err(invalid());
    }
    let decoded = |part: &str| {
        let bytes: Vec<u8> = percent_encoding::percent_decode_str(part).collect();
        String::from_utf8(bytes).map_err(|_| invalid())
    };
    let (authority, dbname) = rest.split_once('/').unwrap_or((rest, ""));
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo), hostport),
        None => (None, authority),
    };
    let (host, port) = match hostport.strip_prefix('[') {
        Some(v6) => {
            let (host, after) = v6.split_once(']').ok_or_else(invalid)?;
            match after {
                "" => (host, None),
                _ => (host, Some(after.strip_prefix(':').ok_or_else(invalid)?)),
            }
        }
        None => match hostport.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (hostport, None),
        },
    };
    // A TCP host is a name or an address rustls can verify: no `/`, `%`, `,` or space.
    if ServerName::try_from(host).is_err() {
        return Err(invalid());
    }
    let port = match port {
        None => 5432,
        Some(port) => port.parse().ok().filter(|&p| p != 0).ok_or_else(invalid)?,
    };
    let (user, password) = match userinfo.map(|u| u.split_once(':').unwrap_or((u, ""))) {
        None => (None, None),
        Some((user, password)) => (
            Some(decoded(user)?).filter(|u| !u.is_empty()),
            (!password.is_empty())
                .then(|| percent_encoding::percent_decode_str(password).collect()),
        ),
    };
    Ok(Url {
        user,
        password,
        host: host.to_string(),
        port,
        dbname: Some(decoded(dbname)?).filter(|d| !d.is_empty()),
    })
}

/// Whether a connection to `host` goes over TLS: always outside loopback (`localhost`,
/// `127.0.0.0/8`, `::1`), and on loopback when the section names a `ca_file`, so a local TLS
/// proxy keeps working and a configured CA is never silently ignored.
fn over_tls(host: &str, ca_file: bool) -> bool {
    ca_file || !crate::modules::loopback(host)
}

impl Config {
    /// The key that names where the URL comes from.
    fn url_key(&self) -> &'static str {
        match self.url {
            Secret::Env(_) => "postgres.url_env",
            Secret::File(_) => "postgres.url_file",
        }
    }

    /// The URL, read now from the variable or the file (on a blocking thread, so a slow mount
    /// stalls no timeout) and checked. An error names the key and never the URL.
    async fn url(&self) -> Result<Url, String> {
        let named = |what: String| format!("`{}`: {what}", self.url_key());
        let source = self.url.clone();
        let read = tokio::task::spawn_blocking(move || source.read(URL_MAX)).await;
        let text = match (read.map_err(|e| e.to_string())?, &self.url) {
            (Ok(Some(text)), _) => text,
            (Ok(None), Secret::Env(var)) => return Err(named(Msg::PostgresUrlUnset(var).text())),
            (Ok(None), Secret::File(file)) => return Err(named(file.display().to_string())),
            (Err(problem), _) => return Err(named(problem)),
        };
        parse(&text).map_err(|reason| named(reason.text()))
    }

    /// The schema, quoted for a statement (`schema_ok` leaves nothing to escape).
    pub(crate) fn quoted(&self) -> String {
        format!("\"{}\"", self.schema)
    }
}

/// The TLS of a connection to Postgres: rustls with ring (no process default is read or
/// installed), verifying against `ca_file` alone when set, else the system's roots.
fn tls(ca_file: Option<&Path>) -> Result<rustls::ClientConfig, String> {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    use rustls_platform_verifier::BuilderVerifierExt;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?;
    let builder = match ca_file {
        Some(file) => {
            let invalid = || {
                let bad = rustls::CertificateError::BadEncoding;
                format!(
                    "{}: {}",
                    file.display(),
                    rustls::Error::InvalidCertificate(bad)
                )
            };
            let pem = crate::modules::read_utf8(file, CA_MAX)?;
            let mut roots = rustls::RootCertStore::empty();
            for cert in CertificateDer::pem_slice_iter(pem.as_bytes()) {
                roots
                    .add(cert.map_err(|_| invalid())?)
                    .map_err(|_| invalid())?;
            }
            if roots.is_empty() {
                return Err(invalid());
            }
            builder.with_root_certificates(roots)
        }
        None => builder
            .with_platform_verifier()
            .map_err(|e| e.to_string())?,
    };
    Ok(builder.with_no_client_auth())
}

/// tokio-postgres's TLS hook over tokio-rustls, for the one host the URL names.
struct Tls(tokio_rustls::TlsConnector, ServerName<'static>);

impl TlsConnect<TcpStream> for Tls {
    type Stream = Stream;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = std::io::Result<Stream>> + Send>>;

    fn connect(self, stream: TcpStream) -> Self::Future {
        Box::pin(async move { self.0.connect(self.1, stream).await.map(Stream) })
    }
}

/// A TLS stream to Postgres.
struct Stream(tokio_rustls::client::TlsStream<TcpStream>);

impl tokio_postgres::tls::TlsStream for Stream {
    // ponytail: no channel binding, so SCRAM runs without `-PLUS`; the certificate check already
    // binds the server. Add `tls-server-end-point` when an operator requires it.
    fn channel_binding(&self) -> ChannelBinding {
        ChannelBinding::none()
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// A connection to the configured Postgres, its version not checked (`migrate` alone uses it as
/// is): the URL read, TCP to its host, TLS by `over_tls`, all within `connect_timeout_seconds`,
/// and every statement bounded by `statement_timeout_seconds`. An error never carries the
/// password.
async fn open(config: &Config) -> Result<Opened, String> {
    let bound = config.connect_timeout;
    match tokio::time::timeout(bound, handshake(config)).await {
        Ok(opened) => opened,
        Err(_) => Err(Msg::PostgresTimeout(bound.as_millis() as u64).text()),
    }
}

/// `open` without its bound.
async fn handshake(config: &Config) -> Result<Opened, String> {
    let url = config.url().await?;
    let secret = (url.password.as_deref()).map(|p| String::from_utf8_lossy(p).into_owned());
    let mut pg = tokio_postgres::Config::new();
    if let Some(user) = &url.user {
        pg.user(user);
    }
    if let Some(password) = &url.password {
        pg.password(password);
    }
    if let Some(dbname) = &url.dbname {
        pg.dbname(dbname);
    }
    let statement = config.statement_timeout.as_millis();
    pg.application_name("meada")
        .options(format!("-c statement_timeout={statement}"));
    let tls = over_tls(&url.host, config.ca_file.is_some());
    let ca_file = config.ca_file.clone();
    let connected: Result<_, String> = async {
        let tcp = TcpStream::connect((url.host.as_str(), url.port)).await;
        let tcp = tcp.map_err(|e| e.to_string())?;
        let _ = tcp.set_nodelay(true);
        let client = if tls {
            let made = tokio::task::spawn_blocking(move || self::tls(ca_file.as_deref())).await;
            let config = made.map_err(|e| e.to_string())??;
            let name = ServerName::try_from(url.host.clone()).map_err(|e| e.to_string())?;
            let tls = Tls(Arc::new(config).into(), name);
            let (client, connection) = (pg.ssl_mode(SslMode::Require).connect_raw(tcp, tls))
                .await
                .map_err(|e| reason(&e))?;
            tokio::spawn(connection);
            client
        } else {
            let (client, connection) = (pg.ssl_mode(SslMode::Disable).connect_raw(tcp, NoTls))
                .await
                .map_err(|e| reason(&e))?;
            tokio::spawn(connection);
            client
        };
        Ok(client)
    }
    .await;
    let client = connected.map_err(|e| scrub(&e, secret.as_deref()))?;
    Ok(Opened { client, secret })
}

/// An open connection and the password it was opened with, which no error may carry.
pub(crate) struct Opened {
    pub(crate) client: tokio_postgres::Client,
    secret: Option<String>,
}

impl Opened {
    pub(crate) fn scrub(&self, e: tokio_postgres::Error) -> String {
        scrub(&reason(&e), self.secret.as_deref())
    }

    /// `scrub` holding its own copy of the password, for the errors of a transaction, which
    /// borrows the client.
    #[cfg(feature = "oidc")]
    pub(crate) fn scrubber(&self) -> impl Fn(tokio_postgres::Error) -> String + use<> {
        let secret = self.secret.clone();
        move |e| scrub(&reason(&e), secret.as_deref())
    }
}

/// `e` with its causes (`db error: FATAL: password authentication failed for user "meada"`):
/// tokio-postgres's own text stops at `db error`.
fn reason(e: &tokio_postgres::Error) -> String {
    let mut text = e.to_string();
    let mut cause = std::error::Error::source(e);
    while let Some(next) = cause {
        text += &format!(": {next}");
        cause = next.source();
    }
    text
}

/// `text` without the secret, if the text carries it.
fn scrub(text: &str, secret: Option<&str>) -> String {
    match secret {
        Some(secret) if !secret.is_empty() => text.replace(secret, "***"),
        _ => text.to_string(),
    }
}

/// The schema version this binary knows: its number of migrations. In debug builds
/// `MEADA_POSTGRES_VERSION_MAX` caps it (a knob for tests and not documented, as
/// `MEADA_DRAIN_MS` is; a release binary ignores it).
fn known() -> i32 {
    let all = MIGRATIONS.len() as i32;
    #[cfg(debug_assertions)]
    if let Some(cap) = std::env::var("MEADA_POSTGRES_VERSION_MAX")
        .ok()
        .and_then(|cap| cap.parse::<i32>().ok())
    {
        return all.min(cap.max(0));
    }
    all
}

/// The version the schema holds: 0 when the schema or its version table is absent.
async fn stored(opened: &Opened, config: &Config) -> Result<i32, String> {
    let client = &opened.client;
    let table = format!("{}.schema_version", config.quoted());
    let found = (client
        .query_one("SELECT to_regclass($1)::text", &[&table])
        .await)
        .map_err(|e| opened.scrub(e))?;
    if found.get::<_, Option<String>>(0).is_none() {
        return Ok(0);
    }
    let version = format!("SELECT version FROM {table}");
    let row = client.query_one(&version, &[]).await;
    Ok(row.map_err(|e| opened.scrub(e))?.get(0))
}

/// The version check every connection but `migrate`'s makes: `stored` must be `known`. Lower
/// (an absent schema included) names `meada postgres migrate`; higher names both versions.
fn check(schema: &str, stored: i32, known: i32) -> Result<(), String> {
    match stored.cmp(&known) {
        Ordering::Equal => Ok(()),
        Ordering::Less => Err(Msg::PostgresVersionLower {
            schema,
            stored,
            known,
        }
        .text()),
        Ordering::Greater => Err(Msg::PostgresVersionHigher {
            schema,
            stored,
            known,
        }
        .text()),
    }
}

/// A connection whose schema holds the version this binary knows, with that version: the one
/// function every connection but `migrate`'s goes through. An error is either Postgres
/// unreachable or the version refusal; a CLI command or the console refuses on it, a node
/// writes it once on stderr and treats Postgres as unreachable.
pub async fn connect(config: &Config) -> Result<(tokio_postgres::Client, i32), String> {
    let opened = connected(config).await?;
    Ok((opened.client, known()))
}

impl Config {
    /// How long a connection and `statements` statements on it may take, all told: what a
    /// caller with no one waiting on it bounds itself by, since a peer that stops answering
    /// without closing the connection would hold it forever.
    pub(crate) fn bound(&self, statements: u32) -> Duration {
        self.connect_timeout + self.statement_timeout * statements
    }
}

/// `connect`, keeping the password so a later statement's error can be scrubbed of it.
pub(crate) async fn connected(config: &Config) -> Result<Opened, String> {
    versioned(config).await.map_err(|(why, _)| why)
}

/// `connected` telling the version refusal apart: its error is why, and whether it is a schema
/// version this binary does not know (a node writes that one once, #332).
pub(crate) async fn versioned(config: &Config) -> Result<Opened, (String, bool)> {
    let opened = open(config).await.map_err(|why| (why, false))?;
    let version = stored(&opened, config).await.map_err(|why| (why, false))?;
    check(&config.schema, version, known()).map_err(|why| (why, true))?;
    Ok(opened)
}

/// The console's start check: the version refusal when Postgres answers with a version this
/// binary does not know; Postgres unreachable passes (the console starts anyway).
pub fn probe(config: &Config) -> Result<(), String> {
    let run = async {
        let Ok(opened) = open(config).await else {
            return Ok(());
        };
        match stored(&opened, config).await {
            Ok(version) => check(&config.schema, version, known()),
            Err(_) => Ok(()),
        }
    };
    crate::modules::block_on(run).unwrap_or(Ok(()))
}

/// Whether `text` is a time as Meada's statements answer one (`to_char` of a `timestamptz` at
/// UTC): `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
#[cfg_attr(
    not(any(feature = "ui", feature = "oidc", feature = "generation-pointer")),
    allow(dead_code)
)]
pub(crate) fn is_time(text: &str) -> bool {
    let shape = "dddd-dd-ddTdd:dd:dd.ddddddZ";
    text.len() == shape.len()
        && (text.bytes().zip(shape.bytes())).all(|(b, s)| {
            if s == b'd' {
                b.is_ascii_digit()
            } else {
                b == s
            }
        })
}

/// The advisory lock key of `schema`'s migrations: the first eight bytes of a BLAKE3 hash, so
/// every binary computes the same key for the same schema and two schemas rarely share one.
fn lock_key(schema: &str) -> i64 {
    let hash = blake3::hash(format!("meada postgres migrate {schema}").as_bytes());
    i64::from_le_bytes(hash.as_bytes()[..8].try_into().expect("eight bytes"))
}

/// `meada postgres migrate --workspace <manifest>`: brings the schema `[postgres]` names to the
/// version this binary knows, under the advisory lock of `lock_key`, so two runs serialize (the
/// lock's wait has no bound: a run waits for the one before it to finish). Each migration runs in
/// its own transaction, migration 1 creating the schema when it is absent. A version above the
/// binary's is refused, never lowered. Returns the version reached.
pub fn migrate(manifest: &Path) -> crate::Result<i32> {
    let config = crate::workspace::Manifest::postgres(manifest)?
        .ok_or_else(|| Msg::PostgresMissing(&manifest.display().to_string()).text())?;
    let run = async {
        let mut opened = open(&config).await?;
        let key = lock_key(&config.schema);
        // The wait is a statement: `statement_timeout_seconds` would cancel it.
        let unbounded = (opened.client)
            .batch_execute("SET statement_timeout = 0")
            .await;
        unbounded.map_err(|e| opened.scrub(e))?;
        let lock = (opened.client)
            .execute("SELECT pg_advisory_lock($1)", &[&key])
            .await;
        lock.map_err(|e| opened.scrub(e))?;
        let bounded = (opened.client)
            .batch_execute("RESET statement_timeout")
            .await;
        bounded.map_err(|e| opened.scrub(e))?;
        let reached = steps(&mut opened, &config).await;
        // Closing the connection releases the lock too.
        let _ = (opened.client)
            .execute("SELECT pg_advisory_unlock($1)", &[&key])
            .await;
        reached
    };
    Ok(crate::modules::block_on(run)??)
}

/// The migrations from the stored version to the binary's, under the lock.
async fn steps(opened: &mut Opened, config: &Config) -> Result<i32, String> {
    let (stored, known) = (stored(opened, config).await?, known());
    let schema = config.schema.as_str();
    if stored > known {
        check(schema, stored, known)?;
    }
    let quoted = config.quoted();
    let secret = opened.secret.clone();
    let scrub = |e: tokio_postgres::Error| scrub(&reason(&e), secret.as_deref());
    for n in stored + 1..=known {
        let tx = opened.client.transaction().await.map_err(scrub)?;
        if n == 1 {
            let exists = "SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)";
            let exists: bool = tx
                .query_one(exists, &[&schema])
                .await
                .map_err(scrub)?
                .get(0);
            if !exists {
                let create = format!("CREATE SCHEMA {quoted}");
                tx.batch_execute(&create).await.map_err(scrub)?;
            }
        }
        let migration = MIGRATIONS[n as usize - 1].replace("{schema}", &quoted);
        tx.batch_execute(&migration).await.map_err(scrub)?;
        let set = format!("UPDATE {quoted}.schema_version SET version = $1");
        tx.execute(&set, &[&n]).await.map_err(scrub)?;
        tx.commit().await.map_err(scrub)?;
    }
    Ok(stored.max(known))
}

/// The `postgres:` line `meada status --workspace` prints: reachable with the schema and its
/// version, or unreachable with the reason, a version this binary does not know included, since
/// every process treats it as unreachable. Bounded by `connect_timeout_seconds`: a readiness
/// probe that runs `status --workspace` waits that long for a Postgres that does not answer.
pub fn status_line(config: &Config) -> String {
    let bound = config.connect_timeout;
    let probe = async { connect(config).await.map(|(_, version)| version) };
    match crate::modules::within(bound, probe, Msg::PostgresTimeout) {
        Ok(version) => Msg::PostgresReachable {
            schema: &config.schema,
            version,
        }
        .text(),
        Err(reason) => Msg::PostgresUnreachable(&reason).text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(toml: &str) -> Result<Config, Problem> {
        let file: File = toml::from_str(toml).unwrap();
        file.validate(Path::new("/etc/meada"))
    }

    #[test]
    fn the_section_takes_its_defaults_and_resolves_paths_from_the_manifest() {
        let config = section("").unwrap();
        assert_eq!(config.url, Secret::Env("MEADA_POSTGRES_URL".into()));
        assert_eq!(config.schema, "meada");
        assert_eq!(config.pool_size, 4);
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert_eq!(config.statement_timeout, Duration::from_secs(10));
        assert!(config.ca_file.is_none());
        let config = section(
            "url_file = \"pg-url\"\nca_file = \"ca.pem\"\nschema = \"meada_prod\"\npool_size = 1\n\
             connect_timeout_seconds = 1\nstatement_timeout_seconds = 30",
        )
        .unwrap();
        assert_eq!(config.url, Secret::File("/etc/meada/pg-url".into()));
        assert_eq!(config.ca_file, Some("/etc/meada/ca.pem".into()));
        assert_eq!(config.schema, "meada_prod");
        assert_eq!(config.statement_timeout, Duration::from_secs(30));
    }

    #[test]
    fn a_bad_section_names_its_key() {
        let long = format!("schema = \"{}\"", "a".repeat(64));
        for (toml, key) in [
            ("url_env = \"A\"\nurl_file = \"b\"", "postgres.url_file"),
            ("schema = \"Meada\"", "postgres.schema"),
            ("schema = \"1meada\"", "postgres.schema"),
            ("schema = \"_meada\"", "postgres.schema"),
            ("schema = \"\"", "postgres.schema"),
            ("schema = \"me-ada\"", "postgres.schema"),
            (&long, "postgres.schema"),
            ("pool_size = 0", "postgres.pool_size"),
            (
                "connect_timeout_seconds = 0",
                "postgres.connect_timeout_seconds",
            ),
            (
                "statement_timeout_seconds = 0",
                "postgres.statement_timeout_seconds",
            ),
            (
                "statement_timeout_seconds = -1",
                "postgres.statement_timeout_seconds",
            ),
            (
                "statement_timeout_seconds = 2147484",
                "postgres.statement_timeout_seconds",
            ),
        ] {
            assert_eq!(section(toml).err().map(|e| e.0), Some(key), "{toml}");
        }
        assert!(section("statement_timeout_seconds = 2147483").is_ok());
        assert!(section(&format!("schema = \"{}\"", "a".repeat(63))).is_ok());
        assert!(section("schema = \"m_9\"").is_ok());
    }

    #[test]
    fn a_url_is_a_tcp_host_with_optional_parts_and_nothing_after_them() {
        let url = parse("postgresql://meada:p%40ss@db.example.internal:6432/meada%5Fdb");
        assert_eq!(
            url.ok(),
            Some(Url {
                user: Some("meada".into()),
                password: Some(b"p@ss".to_vec()),
                host: "db.example.internal".into(),
                port: 6432,
                dbname: Some("meada_db".into()),
            })
        );
        let url = parse("postgres://[::1]").ok().unwrap();
        assert_eq!((url.host.as_str(), url.port), ("::1", 5432));
        assert_eq!((url.user, url.password, url.dbname), (None, None, None));
        assert_eq!(
            parse("postgres://u@127.0.0.1:5/").ok().map(|u| u.port),
            Some(5)
        );
        for text in [
            "mysql://db.example.internal/x",
            "postgres:/db.example.internal",
            "postgres://",
            "postgres:///meada",
            "postgres://%2Fvar%2Frun%2Fpostgresql/meada",
            "postgres://a.example,b.example/meada",
            "postgres://db.example.internal:0/x",
            "postgres://db.example.internal:99999/x",
            "postgres://db.example.internal:x/x",
            "postgres://[::1/x",
            "postgres://[::1]x/x",
            "postgres://::1/x",
            "postgres://db.example.internal/x#f",
        ] {
            assert!(
                matches!(parse(text), Err(Msg::PostgresUrlInvalid)),
                "{text}"
            );
        }
        for text in [
            "postgres://db.example.internal/x?sslmode=disable",
            "postgres:///meada?host=/var/run/postgresql",
            "postgresql://db.example.internal?",
        ] {
            assert!(matches!(parse(text), Err(Msg::PostgresUrlQuery)), "{text}");
        }
    }

    #[test]
    fn only_a_loopback_host_without_a_ca_file_connects_in_plain_tcp() {
        for host in ["localhost", "127.0.0.1", "127.4.5.6", "::1"] {
            assert!(!over_tls(host, false), "{host}");
            // A configured CA is never ignored: a local TLS proxy keeps working.
            assert!(over_tls(host, true), "{host}");
        }
        for host in [
            "10.0.0.1",
            "::2",
            "db.example.internal",
            "localhost.example",
            "::ffff:127.0.0.1",
        ] {
            assert!(over_tls(host, false), "{host}");
            assert!(over_tls(host, true), "{host}");
        }
    }

    #[test]
    fn the_version_check_refuses_lower_naming_migrate_and_higher_naming_both() {
        assert!(check("meada", 1, 1).is_ok());
        let lower = check("meada", 0, 1).unwrap_err();
        assert!(lower.contains("meada postgres migrate"), "{lower}");
        let higher = check("meada", 3, 2).unwrap_err();
        assert!(higher.contains('3') && higher.contains('2'), "{higher}");
        assert!(!higher.contains("migrate"), "{higher}");
    }

    #[test]
    fn migrations_name_only_their_schema_and_the_lock_key_is_stable() {
        // Every name after a keyword that takes a table is the schema's.
        for migration in MIGRATIONS {
            let sql = migration.replace("{schema}", "\"s\"");
            let words: Vec<&str> = sql.split_whitespace().collect();
            for pair in words.windows(2) {
                let keyword = ["TABLE", "INTO", "UPDATE", "FROM", "ON", "REFERENCES"];
                // `ON DELETE` is a foreign key's action, not a table.
                if keyword.contains(&pair[0]) && pair[1] != "DELETE" {
                    assert!(pair[1].starts_with("\"s\"."), "{sql}");
                }
            }
        }
        assert_eq!(lock_key("meada"), lock_key("meada"));
        assert_ne!(lock_key("meada"), lock_key("meada_prod"));
    }

    #[test]
    fn an_unset_variable_and_an_unreadable_file_name_the_key_and_no_url() {
        let pid = std::process::id();
        let var = format!("MEADA_TEST_UNSET_{pid}");
        let unset = section(&format!("url_env = \"{var}\"")).unwrap();
        let missing = section(&format!("url_file = \"/nonexistent/meada-{pid}\"")).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = runtime.block_on(unset.url()).unwrap_err();
        assert!(
            err.starts_with("`postgres.url_env`: ") && err.contains(&var),
            "{err}"
        );
        let err = runtime.block_on(missing.url()).unwrap_err();
        assert!(err.starts_with("`postgres.url_file`: "), "{err}");
        // Past `URL_MAX`, the file is refused, naming it.
        let big = std::env::temp_dir().join(format!("meada-postgres-url-{pid}"));
        std::fs::write(&big, vec![b'x'; URL_MAX as usize + 1]).unwrap();
        let config = section(&format!("url_file = \"{}\"", big.display())).unwrap();
        let err = runtime.block_on(config.url()).unwrap_err();
        assert!(err.ends_with("larger than 4096 bytes"), "{err}");
        // Past `CA_MAX`, so is the CA.
        let ca = std::env::temp_dir().join(format!("meada-postgres-ca-{pid}"));
        std::fs::write(&ca, vec![b'x'; CA_MAX as usize + 1]).unwrap();
        let err = tls(Some(&ca)).err().unwrap();
        assert!(err.ends_with("larger than 1048576 bytes"), "{err}");
    }
}
