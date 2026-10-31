//! The GitHub App module (ADR 0013, feature `github-app`, blocks `[[github_app]]`): git's
//! credential helper for the operator's indexing job, answering with GitHub App installation
//! tokens. Meada never fetches: git does, and asks `meada github-app credential` for a password.
//!
//! Each `[[github_app]]` names a git `host`, the REST base `api`, the `app_id` and the App's PEM
//! key (`private_key_file` or `private_key_env`). Installations are discovered, never declared:
//! per App, `GET /app/installations` with the App's JWT, at most every `REFRESH_EVERY`, kept in
//! Valkey as owner to installation. A `get` for `<owner>/<repo>` takes the owner's installations
//! across the host's Apps in a stable order (App id, then installation id), skips the ones
//! marked throttled, and picks one round robin through one counter per host and owner. Its
//! token comes from Valkey, or is minted (`POST /app/installations/{id}/access_tokens`) under a
//! per-installation lock, and cached until `TOKEN_MARGIN` before it expires.
//!
//! A 403 or 429 to the module's own REST calls marks the installation (a mint) or the App (a
//! listing) throttled; `meada github-app throttled` marks the installation that last answered
//! for a repository after the job's fetch failed so. Every Valkey key lives under the
//! `[valkey]` `key_prefix`. No private key, JWT or token reaches stderr or an error.
//!
//! The role check (`roles`, #331, with `postgres` and `oidc`) is the module's second REST use:
//! each repository's collaborators on `[oidc.github] host`, through the same installations and
//! throttle marks, into one fenced permissions set in Postgres, and with `source = "store"` the
//! exclusions file the readers of the pulled version honor (`snapshot::versions::eligible`).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use redis::cmd;
use serde::{Deserialize, Serialize};
use toml::de::{DeTable, DeValue};
use ureq::http::{HeaderMap, Uri};

use crate::modules::{Problem, Secret, Shape, reachable};
use crate::msg::Msg;
use crate::valkey::{self, Valkey, scrub};

// ponytail: a cached token is dropped this long before it expires, and a token with less left
// is minted again (GitHub's last an hour).
const TOKEN_MARGIN: u64 = 600;
// ponytail: an App's installations are listed again at most this often (10 minutes).
const REFRESH_EVERY: u64 = 600;
// ponytail: the App's JWT expires this far ahead (GitHub refuses past 10 minutes) and is issued
// `JWT_BACKDATE` in the past, for clock drift.
const JWT_LIFE: u64 = 540;
const JWT_BACKDATE: u64 = 60;
// ponytail: an App's installations stay usable this long while listing them again fails.
const INSTALLATIONS_TTL: u64 = 3600;
// ponytail: which installation answered for a repository is kept this long, for `throttled`.
const LAST_TTL: u64 = 3600;
// ponytail: a listing or mint lock is held at most this long, and a `get` waits as long for
// another run's.
const LOCK_TTL: u64 = 30;
/// How often a `get` waiting for another run's listing or mint looks again.
const POLL: Duration = Duration::from_millis(100);
// ponytail: the longest throttle mark; a `retry-after` or a doubling past it is cut to it.
const THROTTLE_MAX: u64 = 3600;
/// A mark with no time given: one minute, doubled on each repeat while the mark holds.
const THROTTLE_FIRST: u64 = 60;
// ponytail: one REST request, connection to last byte.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
// ponytail: the most bytes of one REST answer read; past it the request fails.
const ANSWER_MAX: u64 = 4 << 20;
// ponytail: the most pages of installations listed per App, `PER_PAGE` each.
const PAGES_MAX: u32 = 100;
const PER_PAGE: usize = 100;
// ponytail: the most bytes of a `private_key_file`; past it the key is refused, naming the file.
const KEY_MAX: u64 = 16 << 10;
// ponytail: the most bytes of git's request on stdin; past it the request is refused.
const INPUT_MAX: u64 = 64 << 10;
/// The username git sends with an installation token.
const USERNAME: &str = "x-access-token";
/// Replaces an App's installations in one step: `KEYS[1]` the hash, `ARGV[1]` its TTL, then
/// owner and installation id pairs.
const REPLACE: &str = "redis.call('DEL', KEYS[1]) \
    for i = 2, #ARGV, 2 do redis.call('HSET', KEYS[1], ARGV[i], ARGV[i + 1]) end \
    if #ARGV > 1 then redis.call('EXPIRE', KEYS[1], ARGV[1]) end return 1";

/// One `[[github_app]]` block as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    host: String,
    api: String,
    app_id: i64,
    private_key_file: Option<String>,
    private_key_env: Option<String>,
}

/// A validated block. The key is only named here, so nothing here is a secret.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The git host it serves, lowercase, as git's `host` names it (with a port when not 443).
    host: String,
    /// The REST base URL, with no trailing slash.
    api: String,
    app_id: u64,
    key: Secret,
}

impl File {
    /// The block checked; a relative `private_key_file` is relative to `base`.
    fn validate(self, base: &Path) -> Result<Config, Problem> {
        let host = self.host.to_ascii_lowercase();
        let named = !host.is_empty()
            && (host.chars()).all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':'));
        if !named {
            return Err(("github_app.host", Msg::GithubAppHost.text()));
        }
        if !reachable(&self.api, Shape::Path) {
            return Err(("github_app.api", Msg::GithubAppApi.text()));
        }
        let app_id = crate::modules::at_least("github_app.app_id", Some(self.app_id), 1, 1)?;
        let both = || ("github_app.private_key_env", Msg::GithubAppKeyBoth.text());
        let key = Secret::named(self.private_key_env, self.private_key_file, base, both)?;
        let key = key.ok_or_else(|| ("github_app", Msg::GithubAppKeyNone.text()))?;
        Ok(Config {
            host,
            api: self.api.trim_end_matches('/').to_string(),
            app_id,
            key,
        })
    }
}

/// The `[[github_app]]` blocks of `src`, the content of `file`, checked: a problem names the
/// file, the line of the key in its own block and the key. Blocks need `[valkey]`
/// (`has_valkey`), and one App is listed once per host.
pub(crate) fn validated(
    file: &Path,
    src: &str,
    base: &Path,
    blocks: Vec<File>,
    has_valkey: bool,
) -> crate::Result<Vec<Config>> {
    if !blocks.is_empty() && !has_valkey {
        crate::modules::needs(file, src, "github_app", &["valkey"])?;
    }
    let mut apps: Vec<Config> = Vec::new();
    for (n, block) in blocks.into_iter().enumerate() {
        let app = (block.validate(base)).map_err(|(key, why)| error_in(file, src, n, key, &why))?;
        if apps
            .iter()
            .any(|a| a.host == app.host && a.app_id == app.app_id)
        {
            let why = Msg::GithubAppTwice.text();
            return Err(error_in(file, src, n, "github_app.app_id", &why));
        }
        apps.push(app);
    }
    Ok(apps)
}

/// `<file>:<line>: `<key>`: <message>` for `key` of the `n`th block (its first line when the
/// block lacks the key), as `config::error_at` words one.
fn error_in(
    file: &Path,
    src: &str,
    n: usize,
    key: &str,
    message: &str,
) -> Box<dyn std::error::Error> {
    let at = || {
        let table = DeTable::parse(src).ok()?;
        let (_, blocks) = (table.get_ref().iter()).find(|(k, _)| k.get_ref() == "github_app")?;
        let DeValue::Array(blocks) = blocks.get_ref() else {
            return None;
        };
        let DeValue::Table(block) = blocks.get(n)?.get_ref() else {
            return None;
        };
        let name = key.strip_prefix("github_app.");
        let named = (block.iter()).find(|(k, _)| name == Some(&**k.get_ref()));
        let named = named.map(|(k, _)| k.span().start);
        named.or_else(|| block.iter().map(|(k, _)| k.span().start).min())
    };
    let line = crate::config::line_of(src, at().unwrap_or(0));
    format!("{}:{line}: `{key}`: {message}", file.display()).into()
}

/// What the helper answers: stdout for git, and lines for stderr.
#[derive(Default)]
pub struct Reply {
    pub stdout: String,
    pub notes: Vec<String>,
}

/// The attributes of git's request the helper reads; the others are ignored.
#[derive(Default)]
struct Request {
    protocol: Option<String>,
    host: Option<String>,
    path: Option<String>,
    password: Option<String>,
}

/// git's request on `input`: `key=value` lines up to a blank line or the end, at most
/// `INPUT_MAX` bytes of UTF-8. Anything else is refused with a fixed message: no byte of the
/// input reaches an error.
fn request(input: &mut dyn Read) -> Result<Request, String> {
    let refused = || Msg::GithubAppInput.text();
    let mut bytes = Vec::new();
    (input.take(INPUT_MAX + 1).read_to_end(&mut bytes)).map_err(|_| refused())?;
    if bytes.len() as u64 > INPUT_MAX {
        return Err(refused());
    }
    let text = String::from_utf8(bytes).map_err(|_| refused())?;
    let mut request = Request::default();
    for line in text.split('\n').take_while(|line| !line.is_empty()) {
        let (key, value) = line.split_once('=').ok_or_else(refused)?;
        let slot = match key {
            "protocol" => &mut request.protocol,
            "host" => &mut request.host,
            "path" => &mut request.path,
            "password" => &mut request.password,
            _ => continue,
        };
        *slot = Some(value.to_string());
    }
    Ok(request)
}

/// The owner and the repository `path` names (`<owner>/<repo>`, `.git` optional, one leading
/// `/` allowed), lowercase as GitHub compares them; `None` for any other path. An owner holds
/// ASCII letters, digits, `-` and `_` (an Enterprise Managed User's `_shortcode`); a repository
/// those and `.`, and is never `.` or `..`.
fn repository(path: &str) -> Option<(String, String)> {
    let path = path.strip_prefix('/').unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let owner_ok = !owner.is_empty()
        && (owner.chars()).all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    let repo_ok = !matches!(repo, "" | "." | "..")
        && (repo.chars()).all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (owner_ok && repo_ok).then(|| (owner.to_ascii_lowercase(), repo.to_ascii_lowercase()))
}

/// The seconds since the Unix epoch.
fn now() -> u64 {
    (SystemTime::now().duration_since(UNIX_EPOCH)).map_or(0, |d| d.as_secs())
}

/// The Unix time of `text`, GitHub's `YYYY-MM-DDTHH:MM:SSZ`; `None` for any other text.
fn unix(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    let shaped = b.len() == 20
        && (b[4], b[7], b[10], b[13], b[16], b[19]) == (b'-', b'-', b'T', b':', b':', b'Z');
    if !shaped {
        return None;
    }
    let part = |at: usize, len: usize| text.get(at..at + len)?.parse::<i64>().ok();
    let (y, m, d) = (part(0, 4)?, part(5, 2)?, part(8, 2)?);
    let (hh, mm, ss) = (part(11, 2)?, part(14, 2)?, part(17, 2)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm), years starting in March.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// Whether `token` can stand on a credential-protocol line: visible ASCII, no space.
fn printable(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| b.is_ascii_graphic())
}

/// How a REST call failed.
enum Failure {
    /// 403 or 429, with the status and the seconds the answer says to wait, if it says.
    Throttled(
        #[cfg_attr(not(all(feature = "postgres", feature = "oidc")), allow(dead_code))] u16,
        Option<u64>,
    ),
    /// 404.
    Missing(String),
    Other(String),
}

/// The seconds a 403 or 429 says to wait: `retry-after`, else until `x-ratelimit-reset` when
/// `x-ratelimit-remaining` is 0, else nothing.
fn wait(headers: &HeaderMap, now: u64) -> Option<u64> {
    let number = |name: &str| headers.get(name)?.to_str().ok()?.trim().parse::<u64>().ok();
    if let Some(after) = number("retry-after") {
        return Some(after);
    }
    match number("x-ratelimit-remaining") {
        Some(0) => number("x-ratelimit-reset").map(|reset| reset.saturating_sub(now)),
        _ => None,
    }
}

/// `GET url` (or `POST`, with no body) with the bearer `jwt` (the App's JWT, or an installation
/// token): at most `ANSWER_MAX` bytes of a success answer, and its `Link` header. No error
/// carries the bearer.
fn call(
    agent: &ureq::Agent,
    post: bool,
    url: &str,
    jwt: &str,
) -> Result<(Vec<u8>, Option<String>), Failure> {
    fn headers<B>(request: ureq::RequestBuilder<B>, jwt: &str) -> ureq::RequestBuilder<B> {
        (request.header("Authorization", format!("Bearer {jwt}")))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "meada")
    }
    let sent = match post {
        true => headers(agent.post(url), jwt).send_empty(),
        false => headers(agent.get(url), jwt).call(),
    };
    let other = |e: ureq::Error| Failure::Other(scrub(&format!("{url}: {e}"), Some(jwt)));
    let mut response = sent.map_err(other)?;
    let status = response.status().as_u16();
    if matches!(status, 403 | 429) {
        return Err(Failure::Throttled(status, wait(response.headers(), now())));
    }
    if !response.status().is_success() {
        let why = Msg::GithubAppStatus { url, status }.text();
        return Err(match status {
            404 => Failure::Missing(why),
            _ => Failure::Other(why),
        });
    }
    let link = (response.headers().get("link"))
        .and_then(|link| link.to_str().ok())
        .map(str::to_string);
    // ureq refuses a body that fills its limit, so `max` bytes take a limit one past it.
    let body = response.body_mut().with_config().limit(ANSWER_MAX + 1);
    Ok((body.read_to_vec().map_err(other)?, link))
}

/// The JWT of `app`, signed now with its key: RS256, `iat` `JWT_BACKDATE` ago, `exp`
/// `JWT_LIFE` ahead, `iss` the App id. An error names where the key comes from, never the key.
fn jwt(app: &Config) -> Result<String, String> {
    #[derive(Serialize)]
    struct Claims {
        iat: u64,
        exp: u64,
        iss: u64,
    }
    let source = match &app.key {
        Secret::Env(var) => format!("${var}"),
        Secret::File(file) => file.display().to_string(),
    };
    let pem = match (app.key.read(KEY_MAX)?, &app.key) {
        (Some(pem), _) => pem,
        (None, Secret::Env(var)) => return Err(Msg::ValkeyEnvUnset(var).text()),
        (None, Secret::File(_)) => return Err(Msg::GithubAppKey(&source).text()),
    };
    let refused = |_| Msg::GithubAppKey(&source).text();
    let key = EncodingKey::from_rsa_pem(pem.as_bytes()).map_err(refused)?;
    let now = now();
    let claims = Claims {
        iat: now.saturating_sub(JWT_BACKDATE),
        exp: now + JWT_LIFE,
        iss: app.app_id,
    };
    jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key).map_err(refused)
}

/// The installations of `app` that can mint, as owner (lowercase) and id: every page of
/// `GET /app/installations`, up to `PAGES_MAX`, without suspended ones or accounts with no
/// login.
fn list(agent: &ureq::Agent, app: &Config, jwt: &str) -> Result<Vec<(String, u64)>, Failure> {
    #[derive(Deserialize)]
    struct Installation {
        id: u64,
        account: Option<Account>,
        suspended_at: Option<String>,
    }
    #[derive(Deserialize)]
    struct Account {
        login: Option<String>,
    }
    let mut all = Vec::new();
    for page in 1..=PAGES_MAX {
        let url = format!(
            "{}/app/installations?per_page={PER_PAGE}&page={page}",
            app.api
        );
        let (body, _) = call(agent, false, &url, jwt)?;
        let found: Vec<Installation> = (serde_json::from_slice(&body))
            .map_err(|_| Failure::Other(Msg::GithubAppAnswer(&url).text()))?;
        let full = found.len() == PER_PAGE;
        let usable = (found.into_iter()).filter(|i| i.suspended_at.is_none());
        all.extend(usable.filter_map(|i| Some((i.account?.login?.to_ascii_lowercase(), i.id))));
        if !full {
            break;
        }
    }
    Ok(all)
}

/// A token minted for installation `id` of `app`, with its Unix expiry.
fn mint(agent: &ureq::Agent, app: &Config, id: u64, jwt: &str) -> Result<(String, u64), Failure> {
    #[derive(Deserialize)]
    struct Minted {
        token: String,
        expires_at: String,
    }
    let url = format!("{}/app/installations/{id}/access_tokens", app.api);
    let not = || Failure::Other(Msg::GithubAppAnswer(&url).text());
    let (body, _) = call(agent, true, &url, jwt)?;
    let minted: Minted = serde_json::from_slice(&body).map_err(|_| not())?;
    let expires = unix(&minted.expires_at).ok_or_else(not)?;
    match printable(&minted.token) {
        true => Ok((minted.token, expires)),
        false => Err(not()),
    }
}

/// One run of the helper: the Valkey connection, the HTTP client, each App's JWT once made,
/// and the stderr lines so far.
struct Run<'a> {
    valkey: Valkey,
    config: &'a valkey::Config,
    agent: ureq::Agent,
    jwts: HashMap<(String, u64), String>,
    notes: Vec<String>,
}

/// An installation one of the host's Apps has on an owner.
#[derive(Clone, Copy)]
struct Candidate<'a> {
    app: &'a Config,
    id: u64,
}

impl<'a> Run<'a> {
    async fn open(config: &'a valkey::Config) -> Result<Self, String> {
        Ok(Self {
            valkey: Valkey::connect(config).await?,
            config,
            agent: crate::modules::agent(REQUEST_TIMEOUT),
            jwts: HashMap::new(),
            notes: Vec::new(),
        })
    }

    /// `<key_prefix>:github-app:<host>:<name>`: every key of the module.
    fn key(&self, host: &str, name: &str) -> String {
        self.config.key(&format!("github-app:{host}:{name}"), false)
    }

    fn app_key(&self, app: &Config, name: &str) -> String {
        self.key(&app.host, &format!("app:{}:{name}", app.app_id))
    }

    fn installation_key(&self, host: &str, id: u64, name: &str) -> String {
        self.key(host, &format!("installation:{id}:{name}"))
    }

    /// Which installation last answered `get` for `owner/repo`, for `throttled`.
    fn last_key(&self, host: &str, owner: &str, repo: &str) -> String {
        self.key(host, &format!("repo:{owner}/{repo}:last"))
    }

    async fn exists(&self, key: &str) -> Result<bool, String> {
        self.valkey.query(cmd("EXISTS").arg(key)).await
    }

    /// Takes the lock `key` for `LOCK_TTL`, if nobody holds it.
    async fn take(&self, key: &str) -> Result<bool, String> {
        let set = cmd("SET")
            .arg(key)
            .arg(1)
            .arg("NX")
            .arg("EX")
            .arg(LOCK_TTL)
            .clone();
        Ok(self.valkey.query::<Option<String>>(&set).await?.is_some())
    }

    /// Drops `key`. A lock that expired under its holder and was taken by another run is
    /// dropped all the same: at worst a third run lists or mints too.
    async fn delete(&self, key: &str) -> Result<(), String> {
        self.valkey.query(cmd("DEL").arg(key)).await
    }

    /// Marks `key` throttled for `wait` seconds, or, with none, for one minute doubled on each
    /// repeat while the mark holds; at most `THROTTLE_MAX`. The seconds marked.
    async fn mark(&self, key: &str, wait: Option<u64>) -> Result<u64, String> {
        let secs = match wait {
            Some(secs) => secs,
            None => {
                let held: Option<String> = self.valkey.query(cmd("GET").arg(key)).await?;
                let held = held.and_then(|h| h.parse::<u64>().ok());
                held.map_or(THROTTLE_FIRST, |h| h.saturating_mul(2))
            }
        };
        let secs = secs.clamp(1, THROTTLE_MAX);
        let set = cmd("SET").arg(key).arg(secs).arg("EX").arg(secs).clone();
        self.valkey.query::<()>(&set).await?;
        Ok(secs)
    }

    /// The JWT of `app`, made on first use in this run.
    fn jwt(&mut self, app: &Config) -> Result<String, String> {
        let at = (app.host.clone(), app.app_id);
        if let Some(jwt) = self.jwts.get(&at) {
            return Ok(jwt.clone());
        }
        let jwt = jwt(app)?;
        self.jwts.insert(at, jwt.clone());
        Ok(jwt)
    }

    /// Lists the installations of `app` again when they are older than `REFRESH_EVERY` and the
    /// App is not throttled. One run lists under the lock; the others use what Valkey holds,
    /// waiting for the listing only while it holds nothing yet.
    async fn refresh(&mut self, app: &Config) -> Result<(), String> {
        let (listed, lock) = (self.app_key(app, "listed"), self.app_key(app, "listing"));
        let held = self.app_key(app, "installations");
        let throttled = self.app_key(app, "throttled");
        let deadline = Instant::now() + Duration::from_secs(LOCK_TTL);
        loop {
            if self.exists(&listed).await? || self.exists(&throttled).await? {
                return Ok(());
            }
            if self.take(&lock).await? {
                let listing = self.list(app, &held, &listed, &throttled).await;
                self.delete(&lock).await?;
                return listing;
            }
            if self.exists(&held).await? || Instant::now() >= deadline {
                return Ok(());
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Lists `app`'s installations into `held`, marking `listed` for `REFRESH_EVERY`, or marks
    /// the App `throttled` on a 403 or 429.
    async fn list(
        &mut self,
        app: &Config,
        held: &str,
        listed: &str,
        throttled: &str,
    ) -> Result<(), String> {
        let jwt = self.jwt(app)?;
        let found = match list(&self.agent, app, &jwt) {
            Ok(found) => found,
            Err(Failure::Throttled(_, wait)) => {
                let secs = self.mark(throttled, wait).await?;
                let what = format!("GitHub App {} of {}", app.app_id, app.host);
                self.notes
                    .push(Msg::GithubAppMarked { what: &what, secs }.text());
                return Ok(());
            }
            Err(Failure::Other(why) | Failure::Missing(why)) => return Err(why),
        };
        let mut replace = cmd("EVAL");
        replace.arg(REPLACE).arg(1).arg(held).arg(INSTALLATIONS_TTL);
        for (owner, id) in &found {
            replace.arg(owner).arg(id);
        }
        self.valkey.query::<()>(&replace).await?;
        let set = cmd("SET")
            .arg(listed)
            .arg(1)
            .arg("EX")
            .arg(REFRESH_EVERY)
            .clone();
        self.valkey.query(&set).await
    }

    /// The installations covering `owner` on the host of `apps`, in a stable order: App id,
    /// then installation id. With `refresh`, each App's are listed again when due; a listing
    /// that fails is a stderr line, and what Valkey holds is used.
    async fn candidates<'c>(
        &mut self,
        apps: &[&'c Config],
        owner: &str,
        refresh: bool,
    ) -> Result<Vec<Candidate<'c>>, String> {
        let mut found = Vec::new();
        for app in apps {
            if refresh && let Err(why) = self.refresh(app).await {
                self.notes.push(why);
            }
            let held = self.app_key(app, "installations");
            let id: Option<String> = self.valkey.query(cmd("HGET").arg(&held).arg(owner)).await?;
            if let Some(id) = id.and_then(|id| id.parse().ok()) {
                found.push(Candidate { app, id });
            }
        }
        found.sort_by_key(|c| (c.app.app_id, c.id));
        Ok(found)
    }

    /// The installations covering `owner` in the order to try them: those not throttled (all
    /// of them when every one is, with `true`), from the one the owner's round-robin counter
    /// names. Empty when none covers the owner.
    async fn rotation<'c>(
        &mut self,
        apps: &[&'c Config],
        host: &str,
        owner: &str,
    ) -> Result<(Vec<Candidate<'c>>, bool), String> {
        let all = self.candidates(apps, owner, true).await?;
        let mut open = Vec::new();
        for at in &all {
            if !self
                .exists(&self.installation_key(host, at.id, "throttled"))
                .await?
            {
                open.push(*at);
            }
        }
        let throttled = !all.is_empty() && open.is_empty();
        if throttled {
            open = all;
        }
        if open.is_empty() {
            return Ok((open, false));
        }
        let next = self.key(host, &format!("owner:{owner}:next"));
        let n: i64 = self.valkey.query(cmd("INCR").arg(&next)).await?;
        let first = (n - 1).rem_euclid(open.len() as i64) as usize;
        open.rotate_left(first);
        Ok((open, throttled))
    }

    /// The token of `at` with at least `TOKEN_MARGIN` left and its expiry: from Valkey, or
    /// minted under the installation's lock, another run's mint awaited up to `LOCK_TTL`.
    /// `Ok(None)` when the installation is throttled, the mint's 403 or 429 marking it.
    async fn token(&mut self, at: Candidate<'_>) -> Result<Option<(String, u64)>, String> {
        let (host, id) = (&at.app.host, at.id);
        let cached = self.installation_key(host, id, "token");
        let lock = self.installation_key(host, id, "mint");
        let throttled = self.installation_key(host, id, "throttled");
        let deadline = Instant::now() + Duration::from_secs(LOCK_TTL);
        loop {
            let held: Option<String> = self.valkey.query(cmd("GET").arg(&cached)).await?;
            if let Some((expires, token)) = held.as_deref().and_then(|h| h.split_once(' '))
                && let Ok(expires) = expires.parse()
            {
                return Ok(Some((token.to_string(), expires)));
            }
            if self.exists(&throttled).await? {
                return Ok(None);
            }
            if self.take(&lock).await? {
                let minted = self.mint(at, &cached, &throttled).await;
                self.delete(&lock).await?;
                return minted;
            }
            if Instant::now() >= deadline {
                let what = format!("installation {}", at.id);
                return Err(Msg::GithubAppWaited {
                    what: &what,
                    secs: LOCK_TTL,
                }
                .text());
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Mints a token for `at`, caching it in `cached` until `TOKEN_MARGIN` before it expires,
    /// or marks the installation `throttled` on a 403 or 429.
    async fn mint(
        &mut self,
        at: Candidate<'_>,
        cached: &str,
        throttled: &str,
    ) -> Result<Option<(String, u64)>, String> {
        let jwt = self.jwt(at.app)?;
        let (token, expires) = match mint(&self.agent, at.app, at.id, &jwt) {
            Ok(minted) => minted,
            Err(Failure::Throttled(_, wait)) => {
                let secs = self.mark(throttled, wait).await?;
                let what = format!("installation {}", at.id);
                self.notes
                    .push(Msg::GithubAppMarked { what: &what, secs }.text());
                return Ok(None);
            }
            Err(Failure::Other(why) | Failure::Missing(why)) => return Err(why),
        };
        let ttl = expires.saturating_sub(now()).saturating_sub(TOKEN_MARGIN);
        if ttl > 0 {
            let value = format!("{expires} {token}");
            let set = cmd("SET")
                .arg(cached)
                .arg(&value)
                .arg("EX")
                .arg(ttl)
                .clone();
            self.valkey.query::<()>(&set).await?;
        }
        Ok(Some((token, expires)))
    }
}

/// `meada github-app credential --workspace <manifest> <action>`: git's credential helper, its
/// request on `input`. `get` answers a covered repository; `store` does nothing; `erase` drops
/// the cached token git sends back, so the next `get` mints another.
pub fn credential(manifest: &Path, action: &str, input: &mut dyn Read) -> crate::Result<Reply> {
    let request = request(input)?;
    let mut reply = Reply::default();
    if action == "store" || request.protocol.as_deref() != Some("https") {
        return Ok(reply);
    }
    let Some(host) = request.host.map(|h| h.to_ascii_lowercase()) else {
        return Ok(reply);
    };
    let (config, all) = crate::workspace::Manifest::github_apps(manifest)?;
    let apps: Vec<&Config> = all.iter().filter(|a| a.host == host).collect();
    let path = request.path.as_deref();
    let Some(config) = config.filter(|_| !apps.is_empty()) else {
        // Not this helper's host: a stderr line only for a repository it could have served.
        if action == "get"
            && let Some((owner, _)) = path.and_then(repository)
        {
            let uncovered = Msg::GithubAppUncovered {
                host: &host,
                owner: &owner,
            };
            reply.notes.push(uncovered.text());
        }
        return Ok(reply);
    };
    let Some(path) = path else {
        reply.notes.push(Msg::GithubAppNoPath(&host).text());
        return Ok(reply);
    };
    let (owner, repo) = repository(path).ok_or_else(|| Msg::GithubAppPath.text())?;
    let run = async {
        let mut run = Run::open(&config).await?;
        let answer = match action {
            "get" => get(&mut run, &apps, &host, &owner, &repo).await,
            _ => (erase(&mut run, &apps, &owner, request.password.as_deref()).await)
                .map(|()| Some(String::new())),
        };
        Ok::<_, String>((answer, run.notes))
    };
    let (answer, notes) = crate::modules::block_on(run)??;
    reply.notes = notes;
    match answer {
        Ok(Some(stdout)) => reply.stdout = stdout,
        Ok(None) => {
            let uncovered = Msg::GithubAppUncovered {
                host: &host,
                owner: &owner,
            };
            reply.notes.push(uncovered.text());
        }
        Err(why) => {
            reply.notes.push(why);
            return Err(reply.notes.join("\n").into());
        }
    }
    Ok(reply)
}

/// The answer to `get` for `owner/repo`: an installation round robin among those not
/// throttled (all of them when every one is), the next one when its mint is throttled, and an
/// error when every mint is. `None` when no installation covers the owner.
async fn get(
    run: &mut Run<'_>,
    apps: &[&Config],
    host: &str,
    owner: &str,
    repo: &str,
) -> Result<Option<String>, String> {
    let (open, throttled) = run.rotation(apps, host, owner).await?;
    if open.is_empty() {
        return Ok(None);
    }
    let covered = format!("{host}/{owner}");
    if throttled {
        run.notes.push(Msg::GithubAppAllThrottled(&covered).text());
    }
    for at in open {
        let Some((token, expires)) = run.token(at).await? else {
            continue;
        };
        let last = run.last_key(host, owner, repo);
        let set = cmd("SET")
            .arg(&last)
            .arg(at.id)
            .arg("EX")
            .arg(LAST_TTL)
            .clone();
        run.valkey.query::<()>(&set).await?;
        return Ok(Some(format!(
            "username={USERNAME}\npassword={token}\npassword_expiry_utc={expires}\n"
        )));
    }
    Err(Msg::GithubAppNoToken(&covered).text())
}

/// Drops the cached token of `owner`'s installation that holds `password`, the token git was
/// refused with.
async fn erase(
    run: &mut Run<'_>,
    apps: &[&Config],
    owner: &str,
    password: Option<&str>,
) -> Result<(), String> {
    let Some(password) = password else {
        return Ok(());
    };
    for at in run.candidates(apps, owner, false).await? {
        let cached = run.installation_key(&at.app.host, at.id, "token");
        let held: Option<String> = run.valkey.query(cmd("GET").arg(&cached)).await?;
        let token = held.as_deref().and_then(|h| h.split_once(' '));
        if token.is_some_and(|(_, token)| token == password) {
            run.delete(&cached).await?;
        }
    }
    Ok(())
}

/// `meada github-app throttled --workspace <manifest> <url> [--retry-after <s>]`: marks the
/// installation that last answered `get` for the repository at `url` throttled. A refused `url`
/// is never echoed: it may carry a token.
pub fn throttled(manifest: &Path, url: &str, retry_after: Option<u64>) -> crate::Result<Reply> {
    let bad = || Msg::GithubAppRepositoryUrl.text();
    let uri: Uri = url.parse().map_err(|_| bad())?;
    let (Some("https"), Some(host)) = (uri.scheme_str(), uri.host()) else {
        return Err(bad().into());
    };
    let host = match uri.port_u16() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
    .to_ascii_lowercase();
    let (owner, repo) = repository(uri.path()).ok_or_else(bad)?;
    let (config, all) = crate::workspace::Manifest::github_apps(manifest)?;
    let mut reply = Reply::default();
    let Some(config) = config.filter(|_| all.iter().any(|a| a.host == host)) else {
        let uncovered = Msg::GithubAppUncovered {
            host: &host,
            owner: &owner,
        };
        reply.notes.push(uncovered.text());
        return Ok(reply);
    };
    let run = async {
        let run = Run::open(&config).await?;
        let last = run.last_key(&host, &owner, &repo);
        let id: Option<String> = run.valkey.query(cmd("GET").arg(&last)).await?;
        let Some(id) = id.and_then(|id| id.parse::<u64>().ok()) else {
            let repo = format!("{host}/{owner}/{repo}");
            return Ok(Msg::GithubAppNotAnswered(&repo).text());
        };
        let key = run.installation_key(&host, id, "throttled");
        let secs = run.mark(&key, retry_after).await?;
        let what = format!("installation {id}");
        Ok::<_, String>(Msg::GithubAppMarked { what: &what, secs }.text())
    };
    reply.notes.push(crate::modules::block_on(run)??);
    Ok(reply)
}

// The role check (#331, ADR 0016): `meada github-app roles`, the job's second REST use.

#[cfg(feature = "workspaces")]
mod catalog;

// ponytail: the most pages of one repository's collaborators the role check lists, `PER_PAGE`
// each (100,000 collaborators); past it the run fails, writing nothing.
#[cfg(all(feature = "postgres", feature = "oidc"))]
const COLLABORATOR_PAGES_MAX: u32 = 1000;

/// One repository's collaborators listing: the REST base, the owner and the repository, and
/// what its pages have named so far.
#[cfg(all(feature = "postgres", feature = "oidc"))]
struct Listing<'a> {
    api: &'a str,
    owner: &'a str,
    repo: &'a str,
    /// The pages requested so far.
    seen: std::collections::HashSet<u32>,
    /// The repository id a `/repositories/<id>/collaborators` link named first.
    pinned: Option<String>,
}

#[cfg(all(feature = "postgres", feature = "oidc"))]
impl<'a> Listing<'a> {
    fn new(api: &'a str, owner: &'a str, repo: &'a str) -> Self {
        Self {
            api,
            owner,
            repo,
            seen: std::collections::HashSet::new(),
            pinned: None,
        }
    }

    /// The URL of page `page`, always built here: `affiliation=all`, `PER_PAGE` a page.
    fn url(&self, page: u32) -> String {
        let (api, owner, repo) = (self.api, self.owner, self.repo);
        format!(
            "{api}/repos/{owner}/{repo}/collaborators?affiliation=all&per_page={PER_PAGE}&page={page}"
        )
    }

    /// The page `link`'s `rel="next"` names, `None` on the last page. The link is hostile and
    /// never echoed: it must be on the REST base's origin with no user, at this repository's
    /// collaborators (`/repos/<owner>/<repo>` or the `/repositories/<id>` GitHub links with,
    /// one id for every page), and name a `page` not requested yet, at most
    /// `COLLABORATOR_PAGES_MAX` of them. Only the page number is followed.
    fn next(&mut self, link: Option<&str>) -> Result<Option<u32>, String> {
        let next = link
            .into_iter()
            .flat_map(|l| l.split(','))
            .find_map(|part| {
                let (url, params) = part.trim().strip_prefix('<')?.split_once('>')?;
                (params.split(';'))
                    .any(|p| p.trim() == "rel=\"next\"")
                    .then_some(url)
            });
        let Some(next) = next else {
            return Ok(None);
        };
        let foreign = || Msg::RolesLinkForeign.text();
        let (next, base): (Uri, Uri) = (
            next.parse().map_err(|_| foreign())?,
            self.api.parse().map_err(|_| foreign())?,
        );
        let origin = |u: &Uri| {
            (
                u.scheme_str().map(str::to_ascii_lowercase),
                u.authority().map(|a| a.as_str().to_ascii_lowercase()),
            )
        };
        if origin(&next) != origin(&base)
            || next.authority().is_some_and(|a| a.as_str().contains('@'))
        {
            return Err(foreign());
        }
        let path = (next.path().strip_prefix(base.path().trim_end_matches('/')))
            .and_then(|p| p.strip_suffix("/collaborators"))
            .ok_or_else(foreign)?;
        let ours = format!("/repos/{}/{}", self.owner, self.repo);
        match path.strip_prefix("/repositories/") {
            Some(id) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) => {
                if self.pinned.get_or_insert_with(|| id.to_string()) != id {
                    return Err(foreign());
                }
            }
            _ if path.eq_ignore_ascii_case(&ours) => {}
            _ => return Err(foreign()),
        }
        let page = (next.query().unwrap_or("").split('&'))
            .find_map(|pair| pair.strip_prefix("page="))
            .and_then(|page| page.parse::<u32>().ok())
            .filter(|page| *page > 0)
            .ok_or_else(foreign)?;
        if self.seen.contains(&page) {
            return Err(Msg::RolesLinkCycle.text());
        }
        Ok(Some(page))
    }

    /// Marks `page` requested, refused past `COLLABORATOR_PAGES_MAX`.
    fn take(&mut self, page: u32) -> Result<(), String> {
        if self.seen.len() as u32 >= COLLABORATOR_PAGES_MAX {
            return Err(Msg::RolesPages(COLLABORATOR_PAGES_MAX).text());
        }
        self.seen.insert(page);
        Ok(())
    }
}

/// A collaborator's login and role, as the permissions set holds them.
#[cfg(all(feature = "postgres", feature = "oidc"))]
type Row = (String, String);

/// An entry of the workspace as the role check reads it: its identity and its `added_by`.
#[cfg(all(feature = "postgres", feature = "oidc"))]
type Entry = (String, Option<String>);

/// A collaborator as `role_name`'s row: the login, lowercase, and the role as GitHub spells it;
/// `None` for one whose login is not a GitHub login (1 to 39 ASCII letters, digits, `-` and `_`,
/// compared ignoring case) or whose role is not a name `[oidc.github] roles` could hold.
#[cfg(all(feature = "postgres", feature = "oidc"))]
fn collaborator(value: &serde_json::Value) -> Option<Row> {
    let login = value.get("login")?.as_str()?.to_ascii_lowercase();
    let role = value.get("role_name")?.as_str()?;
    let role_ok = crate::oidc::role_ok(role);
    let login_ok = meada_core::manifest::added_by_problem(&login).is_none();
    (login_ok && role_ok).then(|| (login, role.to_string()))
}

/// Every collaborator of `listing` GitHub lists for `token`, every page, with how many it listed
/// that `collaborator` refused; `None` on a 404. A page that is not a JSON array, or a `Link`
/// that `Listing::next` refuses, fails the listing.
#[cfg(all(feature = "postgres", feature = "oidc"))]
fn collaborators(
    agent: &ureq::Agent,
    mut listing: Listing<'_>,
    token: &str,
) -> Result<Option<(Vec<Row>, usize)>, Failure> {
    let (mut rows, mut skipped, mut page) = (Vec::new(), 0, 1);
    loop {
        listing.take(page).map_err(Failure::Other)?;
        let url = listing.url(page);
        let (body, link) = match call(agent, false, &url, token) {
            Err(Failure::Missing(_)) => return Ok(None),
            answered => answered?,
        };
        let found: Vec<serde_json::Value> = (serde_json::from_slice(&body))
            .map_err(|_| Failure::Other(Msg::GithubAppAnswer(&url).text()))?;
        for value in &found {
            match collaborator(value) {
                Some(row) => rows.push(row),
                None => skipped += 1,
            }
        }
        match listing.next(link.as_deref()).map_err(Failure::Other)? {
            Some(next) => page = next,
            None => return Ok(Some((rows, skipped))),
        }
    }
}

/// The role rows under every selected spelling of a repository (#508): GitHub lists a repository
/// once, under the spelling its target keeps, and each other selected spelling that folds to it
/// on `host` gets the same rows, since a grant matches an identity exactly. Its `source` row
/// stays the target's alone.
#[cfg(all(feature = "postgres", feature = "oidc"))]
fn spelled<'a>(
    host: &str,
    targets: &[(&str, &str)],
    selected: impl Iterator<Item = &'a str>,
    rows: &mut std::collections::BTreeMap<(String, String), String>,
) {
    let fold = |identity: &str| crate::modules::fold(host, identity);
    let written: HashMap<String, &str> = (targets.iter()).map(|&(i, _)| (fold(i), i)).collect();
    let mut copies = Vec::new();
    for spelling in selected {
        let Some(&key) = written.get(&fold(spelling)).filter(|key| **key != spelling) else {
            continue;
        };
        let listed = rows.range((key.to_string(), String::new())..);
        for ((_, login), role) in listed.take_while(|((identity, _), _)| identity == key) {
            copies.push(((spelling.to_string(), login.clone()), role.clone()));
        }
    }
    for (row, role) in copies {
        rows.entry(row).or_insert(role);
    }
}

/// Where each identity comes from (`source`): the workspace's own in its order, then the
/// repositories named workspaces select, then the catalog's alone, sorted. Compared as GitHub
/// compares them (#508): an identity on `host` folds to lowercase, the row keeps the identity as
/// written, and two spellings of one repository are one `source` row (the first kept); `spelled`
/// gives the others their role rows.
#[cfg(all(feature = "postgres", feature = "oidc"))]
fn targets_of<'a>(
    host: &str,
    entries: &[&'a str],
    named: &'a std::collections::BTreeSet<String>,
    candidates: &'a std::collections::BTreeSet<String>,
) -> Vec<(&'a str, &'static str)> {
    let fold = |identity: &str| crate::modules::fold(host, identity);
    let mut targets = Vec::new();
    let mut own = std::collections::HashSet::new();
    for identity in entries
        .iter()
        .copied()
        .chain(named.iter().map(String::as_str))
    {
        let folded = fold(identity);
        let both = candidates.contains(&folded);
        if own.insert(folded) {
            targets.push((identity, if both { "both" } else { "selected" }));
        }
    }
    let alone = candidates.iter().filter(|c| !own.contains(&fold(c)));
    targets.extend(alone.map(|c| (c.as_str(), "catalog")));
    targets
}

/// What `roles` reads of the workspace: the pulled version's id with `source = "store"` and each
/// entry's identity and `added_by`, whole: the exclusions are this command's to write. With
/// `[workspaces]` also the named workspaces the pull read (#342) and the digest of their file.
#[cfg(all(feature = "postgres", feature = "oidc"))]
fn checked_entries(manifest: &Path, read: &crate::workspace::Manifest) -> crate::Result<Checked> {
    #[cfg(feature = "generation-pointer")]
    if read.store {
        let dir = read.index_dir.join(crate::snapshot::versions::DIR);
        let not_pulled = || crate::snapshot::versions::not_pulled(manifest, &dir);
        let pulled = crate::snapshot::versions::pulled(&dir)?;
        #[cfg(feature = "workspaces")]
        let (pulled, named) = match &read.workspaces {
            // An empty deployment workspace has no version: its entries are none.
            Some(_) => {
                let named = crate::workspaces::read_contexts(&dir)?;
                let missing = || Msg::ContextsMissing(&dir.display().to_string()).text();
                (pulled, Some(named.ok_or_else(missing)?))
            }
            None => (pulled, None),
        };
        #[cfg(feature = "workspaces")]
        let pulled = match (pulled, named.is_some()) {
            (None, true) => Some((String::new(), Vec::new(), String::new())),
            (pulled, _) => pulled,
        };
        let (id, entries, _) = pulled.ok_or_else(not_pulled)?;
        let entries = entries.into_iter().map(|e| (e.repo, e.added_by));
        #[cfg(feature = "workspaces")]
        let (contexts, digest) = named.unzip();
        return Ok(Checked {
            version: Some(id).filter(|id| !id.is_empty()),
            entries: entries.collect(),
            #[cfg(feature = "workspaces")]
            contexts: contexts.unwrap_or_default(),
            #[cfg(feature = "workspaces")]
            digest,
        });
    }
    let _ = manifest;
    Ok(Checked {
        version: None,
        entries: (read.entries.iter())
            .map(|l| (l.repo.clone(), None))
            .collect(),
        #[cfg(feature = "workspaces")]
        contexts: Vec::new(),
        #[cfg(feature = "workspaces")]
        digest: None,
    })
}

/// What `checked_entries` read.
#[cfg(all(feature = "postgres", feature = "oidc"))]
struct Checked {
    version: Option<String>,
    entries: Vec<Entry>,
    /// The named workspaces and the digest of the contexts file they came from (#342).
    #[cfg(feature = "workspaces")]
    contexts: Vec<crate::workspaces::Context>,
    #[cfg(feature = "workspaces")]
    digest: Option<String>,
}

/// The fence of this run: `MEADA_REFRESH_FENCE` under `[refresh_lock]` (a positive decimal that
/// Postgres's `bigint` holds), else 0.
#[cfg(all(feature = "postgres", feature = "oidc"))]
fn run_fence(read: &crate::workspace::Manifest) -> Result<u64, String> {
    #[cfg(feature = "refresh-lock")]
    if read.refresh_lock.is_some() {
        let fence = std::env::var(crate::snapshot::lock::FENCE_ENV).ok();
        let fence = fence.and_then(|f| meada_core::ids::fence(&f));
        let fence = fence.filter(|f| *f > 0 && i64::try_from(*f).is_ok());
        return fence.ok_or_else(|| Msg::RolesFenceEnv.text());
    }
    let _ = read;
    Ok(0)
}

/// `meada github-app roles --workspace <manifest>` (#331): every identity of the workspace on
/// `[oidc.github] host`, its collaborators listed through the installation ADR 0013's selection
/// picks, into one permissions set in Postgres, replacing the one before, and with
/// `source = "store"` the exclusions file of the pulled version. A listing that fails fails the
/// run and nothing is written; see `check` for the order of the writes.
#[cfg(all(feature = "postgres", feature = "oidc"))]
pub fn roles(manifest: &Path) -> crate::Result<Reply> {
    let shown = manifest.display().to_string();
    let read = crate::workspace::Manifest::read(manifest)?;
    let github = (read.oidc.as_ref())
        .and_then(|o| o.github.clone())
        .ok_or_else(|| Msg::RolesNoGithub(&shown).text())?;
    let (valkey, all) = crate::workspace::Manifest::github_apps(manifest)?;
    let apps: Vec<&Config> = all.iter().filter(|a| a.host == github.host).collect();
    let (Some(valkey), false) = (valkey, apps.is_empty()) else {
        let src = std::fs::read_to_string(manifest).map_err(|e| format!("{shown}: {e}"))?;
        let why = Msg::RolesNoApp(&github.host).text();
        return Err(crate::config::error_at(manifest, &src, "oidc.github", &why));
    };
    // `Manifest::read` refuses `[oidc.github]` without `[postgres]`.
    let postgres = read
        .postgres
        .clone()
        .ok_or_else(|| Msg::RolesNoGithub(&shown).text())?;
    let fence = run_fence(&read)?;
    let Checked {
        version,
        entries,
        #[cfg(feature = "workspaces")]
        contexts,
        #[cfg(feature = "workspaces")]
        digest,
    } = checked_entries(manifest, &read)?;
    #[cfg(feature = "generation-pointer")]
    let dir = (read.store).then(|| read.index_dir.join(crate::snapshot::versions::DIR));
    #[cfg(not(feature = "generation-pointer"))]
    let dir: Option<std::path::PathBuf> = None;
    let mut notes = Vec::new();
    let checked = check(Check {
        postgres: &postgres,
        valkey: &valkey,
        apps: &apps,
        github: &github,
        #[cfg(feature = "workspaces")]
        catalog: read.catalog.as_ref(),
        fence,
        version,
        entries,
        #[cfg(feature = "workspaces")]
        contexts,
        #[cfg(feature = "workspaces")]
        digest,
        dir,
        notes: &mut notes,
    });
    match crate::modules::block_on(checked)? {
        Ok(line) => Ok(Reply {
            stdout: format!("{line}\n"),
            notes,
        }),
        Err(why) => {
            notes.push(why);
            Err(notes.join("\n").into())
        }
    }
}

/// What one role check reads and where it writes its stderr lines.
#[cfg(all(feature = "postgres", feature = "oidc"))]
struct Check<'a> {
    postgres: &'a crate::postgres::Config,
    valkey: &'a valkey::Config,
    apps: &'a [&'a Config],
    github: &'a crate::oidc::Github,
    /// `[catalog]` (#341): the repositories listed besides the workspace's own.
    #[cfg(feature = "workspaces")]
    catalog: Option<&'a crate::catalog::Config>,
    fence: u64,
    version: Option<String>,
    entries: Vec<Entry>,
    /// The named workspaces the last pull read and the digest of their file (#342).
    #[cfg(feature = "workspaces")]
    contexts: Vec<crate::workspaces::Context>,
    #[cfg(feature = "workspaces")]
    digest: Option<String>,
    /// `<index_dir>/workspace/` with `source = "store"`: where the exclusions file lives.
    dir: Option<std::path::PathBuf>,
    notes: &'a mut Vec<String>,
}

/// The role check, in this order. Postgres is reached and the fence checked: a run the fence
/// refuses changes nothing, and one that cannot reach Postgres removes the exclusions file and
/// stops. The exclusions file is removed, so no reader admits the previous check's exclusions
/// once this one has started, whatever it ends in. Every identity is listed. Then, holding
/// `excluded.lock` from here to the end, one transaction, under a lock on the sets that serializes
/// writers, checks the fence again (a refused run leaves the file as it is), removes the file
/// again (another run on this machine may have written it meanwhile), inserts the set, its
/// repositories' coverage, its rows and its exclusions and deletes every older set; the file is written by rename only while the set is
/// still the newest. Postgres and the file are never written atomically together: a run that
/// fails after its commit leaves the set and no file, so the readers refuse until a run succeeds
/// (the local recovery is to run `meada github-app roles` again), and a file on disk is always
/// that of the last run on this machine to commit. The line it prints on success.
#[cfg(all(feature = "postgres", feature = "oidc"))]
async fn check(check: Check<'_>) -> Result<String, String> {
    use std::collections::BTreeMap;
    let Check {
        postgres,
        valkey,
        apps,
        github,
        #[cfg(feature = "workspaces")]
        catalog,
        fence,
        version,
        entries,
        #[cfg(feature = "workspaces")]
        contexts,
        #[cfg(feature = "workspaces")]
        digest,
        dir,
        notes,
    } = check;
    let remove_excluded = || -> Result<(), String> {
        #[cfg(not(feature = "generation-pointer"))]
        let _ = &dir;
        #[cfg(feature = "generation-pointer")]
        if let Some(dir) = &dir {
            crate::snapshot::versions::remove_excluded(dir)?;
        }
        Ok(())
    };
    let q = postgres.quoted();
    let first = format!("SELECT (SELECT max(fence) FROM {q}.permission_sets), now()::text");
    let reached = async {
        let opened = crate::postgres::connected(postgres).await?;
        let scrub = opened.scrubber();
        let row = (opened.client)
            .query_one(&first, &[])
            .await
            .map_err(&scrub)?;
        Ok::<_, String>((opened.client, scrub, row))
    };
    // A check that cannot reach Postgres has failed all the same: the file goes, so a pulled
    // version that did not change admits no previous check's exclusions.
    let (mut client, scrub, row) = match reached.await {
        Ok(reached) => reached,
        Err(why) => {
            remove_excluded()?;
            return Err(why);
        }
    };
    let held_fence = |held: Option<i64>| match held {
        Some(held) if held > fence as i64 => Err(Msg::RolesFenceBelow { fence, held }.text()),
        _ => Ok(()),
    };
    held_fence(row.get(0))?;
    let started: String = row.get(1);
    remove_excluded()?;

    // Every identity on the host, listed; rows keyed by identity and login, the first kept.
    let mut run = Run::open(valkey).await?;
    let mut rows: BTreeMap<(String, String), String> = BTreeMap::new();
    // The catalog's candidates (#341): what the installations cover and `[catalog] allow`
    // admits, listed before any collaborator so a failure writes nothing.
    #[cfg_attr(not(feature = "workspaces"), allow(unused_mut))]
    let mut candidates: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    #[cfg(feature = "workspaces")]
    if let Some(catalog) = catalog {
        let named: Vec<String> = contexts.iter().flat_map(|c| c.identities()).collect();
        let selected = (entries.iter().map(|(i, _)| i.as_str()))
            .chain(named.iter().map(String::as_str))
            .collect();
        let listed = catalog::candidates(&mut run, apps, &github.host, catalog, &selected).await;
        notes.append(&mut run.notes);
        candidates = listed?;
    }
    #[cfg(feature = "workspaces")]
    let named: std::collections::BTreeSet<String> =
        contexts.iter().flat_map(|c| c.identities()).collect();
    #[cfg(not(feature = "workspaces"))]
    let named: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let own: Vec<&str> = entries.iter().map(|(i, _)| i.as_str()).collect();
    let targets = targets_of(&github.host, &own, &named, &candidates);
    // Per identity on the host, what GitHub showed of it: the App and repository coverage.
    let mut coverage: Vec<(&str, &str, &str)> = Vec::new();
    let prefix = format!("{}/", github.host);
    let mut list_all = async || {
        for &(identity, source) in &targets {
            let Some(rest) = identity.strip_prefix(&prefix) else {
                continue;
            };
            let Some((owner, repo)) = repository(rest).filter(|_| !rest.ends_with(".git")) else {
                run.notes.push(Msg::RolesNotRepository(identity).text());
                coverage.push((identity, "not_a_repository", source));
                continue;
            };
            let shown = listed(&mut run, apps, &github.host, identity, &owner, &repo).await?;
            let Shown::Listed(found) = shown else {
                coverage.push((identity, shown.coverage(), source));
                continue;
            };
            coverage.push((identity, "listed", source));
            for (login, role) in found {
                rows.entry((identity.to_string(), login)).or_insert(role);
            }
        }
        Ok::<_, String>(())
    };
    let listing = list_all().await;
    notes.append(&mut run.notes);
    listing?;
    // Grants match an identity exactly: every selected spelling of a repository gets its roles.
    let selected = own.iter().copied().chain(named.iter().map(String::as_str));
    spelled(&github.host, &targets, selected, &mut rows);
    let role_of = |identity: &str, login: &String| rows.get(&(identity.to_string(), login.clone()));

    // The additions whose `added_by` holds no accepted role on their repository, with
    // `source = "store"` only (a file's entries have no `added_by`).
    let repositories = coverage.iter().filter(|(_, c, _)| *c == "listed").count();
    let mut excluded: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for (identity, added_by) in &entries {
        let Some(login) = added_by.as_ref().filter(|_| identity.starts_with(&prefix)) else {
            continue;
        };
        let role = role_of(identity, login);
        if role.is_some_and(|role| github.roles.contains(role)) {
            continue;
        }
        let listed = role.is_some();
        notes.push(Msg::RolesExcluded { identity, listed }.text());
        let reason = if listed {
            "role_not_accepted"
        } else {
            "no_role"
        };
        excluded.insert(identity, (login, reason));
    }

    // The sponsor rule (#342, ADR 0021): an entry of a named workspace that no operator of the
    // workspace holds an accepted role on, in the rows just listed, is left out of that
    // workspace before the fetch: a policy decision, not a failed fetch.
    #[cfg_attr(not(feature = "workspaces"), allow(unused_mut))]
    let mut named_out: std::collections::BTreeSet<(String, String)> = Default::default();
    #[cfg(feature = "workspaces")]
    for context in &contexts {
        for identity in context.identities() {
            let sponsored = (context.operators.iter()).any(|login| {
                let role = role_of(&identity, login);
                identity.starts_with(&prefix) && role.is_some_and(|r| github.roles.contains(r))
            });
            if !sponsored {
                let (context, identity) = (&context.id, &identity);
                notes.push(Msg::RolesNoSponsor { context, identity }.text());
                named_out.insert((context.clone(), identity.clone()));
            }
        }
    }

    // Held to the end: this machine's runs commit and publish their file one at a time.
    #[cfg(feature = "generation-pointer")]
    let _local = match &dir {
        Some(dir) => Some(crate::snapshot::versions::lock_excluded(dir)?),
        None => None,
    };
    let tx = client.transaction().await.map_err(&scrub)?;
    let lock = format!("LOCK TABLE {q}.permission_sets IN EXCLUSIVE MODE");
    tx.batch_execute(&lock).await.map_err(&scrub)?;
    let max = format!("SELECT max(fence) FROM {q}.permission_sets");
    held_fence(tx.query_one(&max, &[]).await.map_err(&scrub)?.get(0))?;
    // Past the fence, under the local lock: a file another run here wrote meanwhile goes.
    #[cfg(feature = "generation-pointer")]
    if let Some(dir) = &dir {
        crate::snapshot::versions::remove_excluded(dir)?;
    }
    let insert = format!(
        "INSERT INTO {q}.permission_sets (fence, version, started, repositories) \
         VALUES ($1, $2, $3::text::timestamptz, $4) RETURNING id"
    );
    let (fence, count) = (fence as i64, repositories as i32);
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 4] =
        [&fence, &version, &started, &count];
    let id: String = tx.query_one(&insert, &params).await.map_err(&scrub)?.get(0);
    let (mut identities, mut logins, mut roles) = (Vec::new(), Vec::new(), Vec::new());
    for ((identity, login), role) in &rows {
        identities.push(identity.as_str());
        logins.push(login.as_str());
        roles.push(role.as_str());
    }
    let insert = format!(
        "INSERT INTO {q}.permission_rows (set_id, identity, login, role) \
         SELECT $1, i, l, r FROM unnest($2::text[], $3::text[], $4::text[]) AS t (i, l, r)"
    );
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 4] =
        [&id, &identities, &logins, &roles];
    tx.execute(&insert, &params).await.map_err(&scrub)?;
    let identities: Vec<&str> = coverage.iter().map(|(i, _, _)| *i).collect();
    let shown: Vec<&str> = coverage.iter().map(|(_, c, _)| *c).collect();
    let sources: Vec<&str> = coverage.iter().map(|(_, _, s)| *s).collect();
    let insert = format!(
        "INSERT INTO {q}.permission_repositories (set_id, identity, coverage, source) \
         SELECT $1, i, c, s FROM unnest($2::text[], $3::text[], $4::text[]) AS t (i, c, s)"
    );
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 4] =
        [&id, &identities, &shown, &sources];
    tx.execute(&insert, &params).await.map_err(&scrub)?;
    // The deployment workspace's (context ''), then each named workspace's, which have no
    // `added_by`.
    let mut contexts_col: Vec<&str> = vec![""; excluded.len()];
    let mut identities: Vec<&str> = excluded.keys().copied().collect();
    let mut added: Vec<Option<&str>> = excluded.values().map(|(login, _)| Some(*login)).collect();
    let mut reasons: Vec<&str> = excluded.values().map(|(_, reason)| *reason).collect();
    for (context, identity) in &named_out {
        contexts_col.push(context);
        identities.push(identity);
        added.push(None);
        reasons.push("no_sponsor");
    }
    let insert = format!(
        "INSERT INTO {q}.permission_exclusions (set_id, context, identity, added_by, reason) \
         SELECT $1, c, i, a, r FROM \
         unnest($2::text[], $3::text[], $4::text[], $5::text[]) AS t (c, i, a, r)"
    );
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] =
        [&id, &contexts_col, &identities, &added, &reasons];
    tx.execute(&insert, &params).await.map_err(&scrub)?;
    let older = format!("DELETE FROM {q}.permission_sets WHERE id <> $1");
    tx.execute(&older, &[&id]).await.map_err(&scrub)?;
    tx.commit().await.map_err(&scrub)?;

    #[cfg(feature = "generation-pointer")]
    // An empty deployment workspace under `[workspaces]` has no version: the file names the
    // sentinel the readers expect.
    #[cfg(feature = "workspaces")]
    let file_version = version.clone().or_else(|| {
        digest
            .is_some()
            .then(|| crate::snapshot::versions::NO_VERSION.to_string())
    });
    #[cfg(not(feature = "workspaces"))]
    let file_version = version.clone();
    if let (Some(dir), Some(version)) = (&dir, &file_version) {
        let newest = format!("SELECT EXISTS (SELECT 1 FROM {q}.permission_sets WHERE id = $1)");
        let newest: bool = client
            .query_one(&newest, &[&id])
            .await
            .map_err(&scrub)?
            .get(0);
        if !newest {
            return Err(Msg::RolesReplaced.text());
        }
        let identities = excluded.keys().map(|i| i.to_string()).collect();
        #[cfg(feature = "workspaces")]
        if let Some(digest) = &digest {
            let named = Some((digest.as_str(), &named_out));
            crate::snapshot::versions::write_excluded_with(dir, version, &identities, named)?;
        } else {
            crate::snapshot::versions::write_excluded(dir, version, &identities)?;
        }
        #[cfg(not(feature = "workspaces"))]
        crate::snapshot::versions::write_excluded(dir, version, &identities)?;
    }
    #[cfg(feature = "workspaces")]
    if catalog.is_some() {
        let catalog = (coverage.iter())
            .filter(|(_, c, s)| *c == "listed" && *s != "selected")
            .count();
        return Ok(Msg::RolesSetCatalog {
            id: &id,
            repositories,
            catalog,
            rows: rows.len(),
            exclusions: excluded.len() + named_out.len(),
        }
        .text());
    }
    Ok(Msg::RolesSet {
        id: &id,
        repositories,
        rows: rows.len(),
        exclusions: excluded.len() + named_out.len(),
    }
    .text())
}

/// What GitHub showed of one repository.
#[cfg(all(feature = "postgres", feature = "oidc"))]
enum Shown {
    Listed(Vec<Row>),
    /// A 404.
    NotFound,
    /// No installation covers the owner.
    Uncovered,
}

#[cfg(all(feature = "postgres", feature = "oidc"))]
impl Shown {
    /// The coverage the set records for it.
    fn coverage(&self) -> &'static str {
        match self {
            Shown::Listed(_) => "listed",
            Shown::NotFound => "not_found",
            Shown::Uncovered => "uncovered",
        }
    }
}

/// The collaborators of `identity` (`owner/repo` on `host`) and their roles, through the
/// installations covering the owner in `Run::rotation`'s order: a 403 or 429 marks the
/// installation throttled and passes to the next. `Shown::Uncovered` or `Shown::NotFound`, with a
/// stderr line, when no installation covers the owner or GitHub answers 404. Every other failure, every installation
/// throttled included, is the line that fails the run.
#[cfg(all(feature = "postgres", feature = "oidc"))]
async fn listed(
    run: &mut Run<'_>,
    apps: &[&Config],
    host: &str,
    identity: &str,
    owner: &str,
    repo: &str,
) -> Result<Shown, String> {
    let failed = |why: &str| Msg::RolesFailed { identity, why }.text();
    let (open, throttled) = run
        .rotation(apps, host, owner)
        .await
        .map_err(|e| failed(&e))?;
    if open.is_empty() {
        run.notes.push(Msg::RolesUncovered(identity).text());
        return Ok(Shown::Uncovered);
    }
    if throttled {
        return Err(failed(&Msg::RolesThrottled(None).text()));
    }
    let mut last = None;
    for at in open {
        let Some((token, _)) = run.token(at).await.map_err(|e| failed(&e))? else {
            continue;
        };
        let listing = Listing::new(&at.app.api, owner, repo);
        match collaborators(&run.agent, listing, &token) {
            Ok(Some((rows, skipped))) => {
                for _ in 0..skipped {
                    run.notes.push(Msg::RolesSkipped(identity).text());
                }
                return Ok(Shown::Listed(rows));
            }
            Ok(None) => {
                run.notes.push(Msg::RolesNotFound(identity).text());
                return Ok(Shown::NotFound);
            }
            Err(Failure::Throttled(status, wait)) => {
                let key = run.installation_key(host, at.id, "throttled");
                let secs = run.mark(&key, wait).await.map_err(|e| failed(&e))?;
                let what = format!("installation {}", at.id);
                run.notes
                    .push(Msg::GithubAppMarked { what: &what, secs }.text());
                last = Some(status);
            }
            Err(Failure::Other(why) | Failure::Missing(why)) => return Err(failed(&why)),
        }
    }
    Err(failed(&Msg::RolesThrottled(last).text()))
}

/// Cached installations and their expiry/throttle state. This reads Valkey only, never a key
/// or GitHub, and extracts the expiry beside the token before returning anything to its caller.
#[cfg(feature = "ui")]
pub(crate) async fn inspect(
    valkey: &Valkey,
    config: &valkey::Config,
    apps: &[Config],
    reads_owner: &impl Fn(&str, &str) -> bool,
) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let mut found = Vec::new();
    for app in apps {
        let key = |name: &str| config.key(&format!("github-app:{}:{name}", app.host), false);
        let installations: Vec<(String, String)> = valkey
            .query(cmd("HGETALL").arg(key(&format!("app:{}:installations", app.app_id))))
            .await?;
        let throttle: i64 = valkey
            .query(cmd("EXPIRETIME").arg(key(&format!("app:{}:throttled", app.app_id))))
            .await?;
        let now = now();
        let mut entries = Vec::new();
        let mut malformed = throttle == -1;
        for (owner, id) in installations {
            if !reads_owner(&app.host, &owner) {
                continue;
            }
            // An owner/id read from the cache must not add a key segment.
            let owner_ok = !owner.is_empty()
                && owner
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-');
            let Some(id) = meada_core::ids::fence(&id).filter(|id| *id > 0 && owner_ok) else {
                malformed = true;
                continue;
            };
            let cached = key(&format!("installation:{id}:token"));
            // The token never leaves Valkey: the read retains the decimal expiry alone.
            let expires: Option<String> = valkey.query(cmd("EVAL").arg("local v=redis.call('GET',KEYS[1]); if v then return string.match(v,'^(%d+) ') end; return false").arg(1).arg(&cached)).await?;
            let expires = expires.as_deref().and_then(meada_core::ids::fence);
            let throttle: i64 = valkey
                .query(cmd("EXPIRETIME").arg(key(&format!("installation:{id}:throttled"))))
                .await?;
            malformed |= throttle == -1;
            entries.push(json!({"owner": owner, "id": id, "cached": expires.is_some(), "expires": expires, "remaining_seconds": expires.map(|e| e.saturating_sub(now)), "throttled_until": u64::try_from(throttle).ok()}));
        }
        entries.sort_by(|a, b| {
            a["owner"]
                .as_str()
                .cmp(&b["owner"].as_str())
                .then(a["id"].as_u64().cmp(&b["id"].as_u64()))
        });
        found.push(json!({"host": app.host, "app_id": app.app_id, "installations": entries, "malformed": malformed, "throttled_until": u64::try_from(throttle).ok()}));
    }
    Ok(json!({"apps": found}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(feature = "postgres", feature = "oidc"))]
    fn a_selected_identity_and_its_lowercase_candidate_are_one_both_row() {
        let candidates = [
            "github.com/acme/api",
            "github.com/acme/web",
            "ghe.example/acme/x",
        ]
        .map(String::from)
        .into();
        let named = ["github.com/ACME/api"].map(String::from).into();
        let entries = [
            "github.com/Acme/API",
            "github.com/acme/api",
            "ghe.example/Acme/X",
        ];
        let targets = targets_of("github.com", &entries, &named, &candidates);
        // Another host compares exactly: its uppercase entry is `selected` and its lowercase
        // candidate a `catalog` row of its own.
        assert_eq!(
            targets,
            [
                ("github.com/Acme/API", "both"),
                ("ghe.example/Acme/X", "selected"),
                ("ghe.example/acme/x", "catalog"),
                ("github.com/acme/web", "catalog"),
            ]
        );
    }

    /// Option (a) of #508: the deployment writes `github.com/Acme/API` and a named workspace
    /// selected `github.com/acme/api`, so GitHub is listed once, under the first; both spellings
    /// get its role rows, since a grant matches exactly. A spelling of another host, or of a
    /// repository nobody listed, gets none.
    #[test]
    #[cfg(all(feature = "postgres", feature = "oidc"))]
    fn every_selected_spelling_of_a_repository_gets_its_role_rows() {
        let candidates = ["github.com/acme/api"].map(String::from).into();
        let named = ["github.com/acme/api", "github.com/ACME/web"]
            .map(String::from)
            .into();
        let entries = [
            "github.com/Acme/API",
            "github.com/acme/web",
            "ghe.example/A/x",
        ];
        let targets = targets_of("github.com", &entries, &named, &candidates);
        let row = |identity: &str, login: &str| (identity.to_string(), login.to_string());
        // What GitHub showed: two logins on the repository, under the spelling its target keeps.
        let mut rows = std::collections::BTreeMap::from([
            (row("github.com/Acme/API", "ana"), "write".to_string()),
            (row("github.com/Acme/API", "bo"), "admin".to_string()),
            (row("ghe.example/A/x", "ana"), "write".to_string()),
        ]);
        let selected = entries.into_iter().chain(named.iter().map(String::as_str));
        spelled("github.com", &targets, selected, &mut rows);
        let keys: Vec<(&str, &str, &str)> = (rows.iter())
            .map(|((i, l), r)| (i.as_str(), l.as_str(), r.as_str()))
            .collect();
        assert_eq!(
            keys,
            [
                ("ghe.example/A/x", "ana", "write"),
                ("github.com/Acme/API", "ana", "write"),
                ("github.com/Acme/API", "bo", "admin"),
                ("github.com/acme/api", "ana", "write"),
                ("github.com/acme/api", "bo", "admin"),
            ]
        );
        // One `source` row per repository all the same.
        assert_eq!(
            targets,
            [
                ("github.com/Acme/API", "both"),
                ("github.com/acme/web", "selected"),
                ("ghe.example/A/x", "selected"),
            ]
        );
    }

    fn block(toml: &str) -> Result<Config, Problem> {
        let file: File = toml::from_str(toml).unwrap();
        file.validate(Path::new("/etc/meada"))
    }

    const GOOD: &str = "host = \"GitHub.com\"\napi = \"https://api.github.com/\"\napp_id = 7\n";

    #[test]
    fn a_block_takes_its_key_from_one_place_and_names_each_bad_key() {
        let app = block(&format!("{GOOD}private_key_file = \"app.pem\"")).unwrap();
        assert_eq!(
            (app.host.as_str(), app.api.as_str()),
            ("github.com", "https://api.github.com")
        );
        assert_eq!(app.key, Secret::File("/etc/meada/app.pem".into()));
        let app = block(&format!("{GOOD}private_key_env = \"KEY\"")).unwrap();
        assert_eq!(app.key, Secret::Env("KEY".into()));
        let key = "\nprivate_key_env = \"K\"";
        let ghes = "host = \"ghe.example:8443\"\napi = \"https://ghe.example/api/v3\"\napp_id = 1";
        assert!(block(&format!("{ghes}{key}")).is_ok());
        let loopback = "host = \"h\"\napi = \"http://127.0.0.1:9\"\napp_id = 1";
        assert!(block(&format!("{loopback}{key}")).is_ok());
        let both = format!("{GOOD}private_key_env = \"K\"\nprivate_key_file = \"f\"");
        let at = |host: &str, api: &str, id: i64| {
            format!("host = \"{host}\"\napi = \"{api}\"\napp_id = {id}{key}")
        };
        for (toml, named) in [
            (both, "github_app.private_key_env"),
            (GOOD.to_string(), "github_app"),
            (at("a/b", "https://x", 1), "github_app.host"),
            (at("", "https://x", 1), "github_app.host"),
            (at("h", "http://api.example", 1), "github_app.api"),
            (at("h", "https://u@api.example", 1), "github_app.api"),
            (at("h", "https://api.example?a=1", 1), "github_app.api"),
            (at("h", "https://x", 0), "github_app.app_id"),
        ] {
            assert_eq!(block(&toml).unwrap_err().0, named, "{toml}");
        }
        assert!(toml::from_str::<File>(&format!("{GOOD}owners = []")).is_err());
    }

    #[test]
    fn a_problem_points_at_the_line_of_its_own_block() {
        let one = "host = \"a\"\napi = \"https://a\"\napp_id = 1\nprivate_key_env = \"K\"\n";
        let two = "host = \"a\"\napi = \"http://a\"\napp_id = 2\nprivate_key_env = \"K\"\n";
        let src = format!(
            "index_dir = \"i\"\n[valkey]\nurl = \"redis://127.0.0.1\"\n\n[[github_app]]\n{one}\n\
             [[github_app]]\n{two}"
        );
        let blocks = vec![toml::from_str(one).unwrap(), toml::from_str(two).unwrap()];
        let err = validated(Path::new("m.toml"), &src, Path::new(""), blocks, true).unwrap_err();
        let err = err.to_string();
        assert!(err.starts_with("m.toml:13: `github_app.api`: "), "{err}");
    }

    #[test]
    fn git_s_request_is_read_strictly_and_never_echoed() {
        let text =
            "capability[]=authtype\nprotocol=https\nhost=github.com\npath=a/b.git\n\nhost=x\n";
        let got = request(&mut text.as_bytes()).unwrap();
        assert_eq!(
            (got.protocol.as_deref(), got.host.as_deref()),
            (Some("https"), Some("github.com"))
        );
        assert_eq!(got.path.as_deref(), Some("a/b.git"));
        let long = "x=".repeat(INPUT_MAX as usize);
        let bad: [&[u8]; 3] = [
            b"protocol=https\nsecret-without-equals\n",
            long.as_bytes(),
            b"\xff=\n",
        ];
        for input in bad {
            let why = request(&mut &input[..]).err().unwrap();
            assert_eq!(why, Msg::GithubAppInput.text());
        }
    }

    #[test]
    fn a_path_is_owner_slash_repo_and_nothing_an_owner_cannot_be() {
        let pair = |owner: &str, repo: &str| Some((owner.to_string(), repo.to_string()));
        assert_eq!(repository("Acme/Web.git"), pair("acme", "web"));
        assert_eq!(repository("/acme/web"), pair("acme", "web"));
        assert_eq!(repository("user_emu/r.s"), pair("user_emu", "r.s"));
        for bad in [
            "", "acme", "/web", "./web", "../web", "a.b/web", "acme/..", "acme/.", "acme/w/x",
            "a{b}/w", "a b/w", "acme/",
        ] {
            assert_eq!(repository(bad), None, "{bad}");
        }
    }

    #[test]
    fn github_s_times_read_as_unix_seconds() {
        assert_eq!(unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix("2016-07-11T22:14:10Z"), Some(1_468_275_250));
        assert_eq!(unix("2099-01-01T00:00:00Z"), Some(4_070_908_800));
        for bad in [
            "2016-07-11 22:14:10Z",
            "2016-13-11T22:14:10Z",
            "2016-07-11T22:14:10",
            "",
        ] {
            assert_eq!(unix(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_throttled_answer_waits_as_its_headers_say() {
        let headers = |pairs: &[(&'static str, &str)]| {
            let mut map = HeaderMap::new();
            for (k, v) in pairs {
                map.insert(*k, v.parse().unwrap());
            }
            map
        };
        assert_eq!(wait(&headers(&[("retry-after", "30")]), 0), Some(30));
        let reset = [
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1100"),
        ];
        assert_eq!(wait(&headers(&reset), 1000), Some(100));
        let left = [
            ("x-ratelimit-remaining", "5"),
            ("x-ratelimit-reset", "1100"),
        ];
        assert_eq!(wait(&headers(&left), 1000), None);
        assert_eq!(wait(&headers(&[]), 1000), None);
    }

    #[test]
    fn a_refused_repository_url_is_never_echoed() {
        // Refused before the manifest is read: a trailing slash, `http`, no repository, garbage.
        for url in [
            "https://x-access-token:ghs_s3cret@github.com/acme/web/",
            "http://x-access-token:ghs_s3cret@github.com/acme/web",
            "https://x-access-token:ghs_s3cret@github.com/acme",
            "x-access-token:ghs_s3cret@github.com",
        ] {
            let why = throttled(Path::new("m.toml"), url, None).err().unwrap();
            let why = why.to_string();
            assert_eq!(why, Msg::GithubAppRepositoryUrl.text(), "{url}");
            assert!(
                !why.contains("s3cret") && !why.contains("x-access"),
                "{why}"
            );
        }
    }

    #[cfg(all(feature = "postgres", feature = "oidc"))]
    #[test]
    fn a_next_page_is_this_repository_s_listing_on_the_rest_base_and_never_seen() {
        let api = "https://ghe.example/api/v3";
        let link = |url: &str| format!("<{url}>; rel=\"next\", <{api}/x?page=9>; rel=\"last\"");
        let mut listing = Listing::new(api, "acme", "web");
        listing.take(1).unwrap();
        assert_eq!(listing.next(None), Ok(None));
        let last = format!("<{api}/x?page=9>; rel=\"last\"");
        assert_eq!(listing.next(Some(&last)), Ok(None));
        let ours =
            format!("{api}/repos/Acme/Web/collaborators?affiliation=all&per_page=100&page=2");
        assert_eq!(listing.next(Some(&link(&ours))), Ok(Some(2)));
        let pinned = format!("{api}/repositories/42/collaborators?page=3");
        assert_eq!(listing.next(Some(&link(&pinned))), Ok(Some(3)));
        let foreign = Msg::RolesLinkForeign.text();
        for bad in [
            "https://elsewhere.example/api/v3/repos/acme/web/collaborators?page=2".to_string(),
            "http://ghe.example/api/v3/repos/acme/web/collaborators?page=2".into(),
            "https://u:p@ghe.example/api/v3/repos/acme/web/collaborators?page=2".into(),
            "https://ghe.example:8443/api/v3/repos/acme/web/collaborators?page=2".into(),
            "https://ghe.example/repos/acme/web/collaborators?page=2".into(),
            format!("{api}/repos/acme/api/collaborators?page=2"),
            format!("{api}/repos/acme/web/collaborators/x?page=2"),
            format!("{api}/repos/acme/web/teams?page=2"),
            format!("{api}/repositories/43/collaborators?page=2"),
            format!("{api}/repositories/4x/collaborators?page=2"),
            format!("{api}/repos/acme/web/collaborators?per_page=100"),
            format!("{api}/repos/acme/web/collaborators?page=0"),
            format!("{api}/repos/acme/web/collaborators?page=-1"),
            "not a url".into(),
        ] {
            assert_eq!(
                listing.next(Some(&link(&bad))),
                Err(foreign.clone()),
                "{bad}"
            );
        }
        // A page requested already is a cycle.
        assert_eq!(
            listing.next(Some(&link(&format!(
                "{api}/repos/acme/web/collaborators?page=1"
            )))),
            Err(Msg::RolesLinkCycle.text())
        );
        // Past `COLLABORATOR_PAGES_MAX` pages the listing fails.
        for page in 2..=COLLABORATOR_PAGES_MAX {
            listing.take(page).unwrap();
        }
        let past = Msg::RolesPages(COLLABORATOR_PAGES_MAX).text();
        assert_eq!(listing.take(COLLABORATOR_PAGES_MAX + 1), Err(past));
    }

    #[cfg(all(feature = "postgres", feature = "oidc"))]
    #[test]
    fn a_collaborator_needs_a_github_login_and_a_role_name() {
        let row = |v: serde_json::Value| collaborator(&v);
        let pair = |l: &str, r: &str| Some((l.to_string(), r.to_string()));
        assert_eq!(
            row(serde_json::json!({"login": "Ana_Souza-1", "role_name": "write"})),
            pair("ana_souza-1", "write")
        );
        assert_eq!(
            row(serde_json::json!({"login": "a", "role_name": "Custom Role"})),
            pair("a", "Custom Role")
        );
        let long = "x".repeat(40);
        let role = "r".repeat(101);
        for bad in [
            serde_json::json!({"login": "a"}),
            serde_json::json!({"role_name": "read"}),
            serde_json::json!({"login": "", "role_name": "read"}),
            serde_json::json!({"login": long, "role_name": "read"}),
            serde_json::json!({"login": "a.b", "role_name": "read"}),
            serde_json::json!({"login": "ä", "role_name": "read"}),
            serde_json::json!({"login": 7, "role_name": "read"}),
            serde_json::json!({"login": "a", "role_name": ""}),
            serde_json::json!({"login": "a", "role_name": role}),
            serde_json::json!({"login": "a", "role_name": "re\nad"}),
            serde_json::json!("a"),
        ] {
            assert_eq!(row(bad.clone()), None, "{bad}");
        }
    }

    #[test]
    fn a_token_must_fit_on_one_protocol_line() {
        assert!(printable("ghs_abc123"));
        for bad in ["", "a b", "a\nusername=x", "a\u{0}"] {
            assert!(!printable(bad), "{bad:?}");
        }
    }
}
