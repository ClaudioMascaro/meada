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
    /// 403 or 429, with the seconds the answer says to wait, if it says.
    Throttled(Option<u64>),
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

/// `GET url` (or `POST`, with no body) with the App's `jwt`: at most `ANSWER_MAX` bytes of a
/// success answer. No error carries the JWT.
fn call(agent: &ureq::Agent, post: bool, url: &str, jwt: &str) -> Result<Vec<u8>, Failure> {
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
        return Err(Failure::Throttled(wait(response.headers(), now())));
    }
    if !response.status().is_success() {
        return Err(Failure::Other(Msg::GithubAppStatus { url, status }.text()));
    }
    // ureq refuses a body that fills its limit, so `max` bytes take a limit one past it.
    let body = response.body_mut().with_config().limit(ANSWER_MAX + 1);
    body.read_to_vec().map_err(other)
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
        let body = call(agent, false, &url, jwt)?;
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
    let body = call(agent, true, &url, jwt)?;
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
            Err(Failure::Throttled(wait)) => {
                let secs = self.mark(throttled, wait).await?;
                let what = format!("GitHub App {} of {}", app.app_id, app.host);
                self.notes
                    .push(Msg::GithubAppMarked { what: &what, secs }.text());
                return Ok(());
            }
            Err(Failure::Other(why)) => return Err(why),
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
            Err(Failure::Throttled(wait)) => {
                let secs = self.mark(throttled, wait).await?;
                let what = format!("installation {}", at.id);
                self.notes
                    .push(Msg::GithubAppMarked { what: &what, secs }.text());
                return Ok(None);
            }
            Err(Failure::Other(why)) => return Err(why),
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
    let all = run.candidates(apps, owner, true).await?;
    if all.is_empty() {
        return Ok(None);
    }
    let mut open = Vec::new();
    for at in &all {
        if !run
            .exists(&run.installation_key(host, at.id, "throttled"))
            .await?
        {
            open.push(*at);
        }
    }
    let covered = format!("{host}/{owner}");
    if open.is_empty() {
        run.notes.push(Msg::GithubAppAllThrottled(&covered).text());
        open = all;
    }
    let next = run.key(host, &format!("owner:{owner}:next"));
    let n: i64 = run.valkey.query(cmd("INCR").arg(&next)).await?;
    let len = open.len() as i64;
    for k in 0..len {
        let at = open[(n - 1 + k).rem_euclid(len) as usize];
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

    #[test]
    fn a_token_must_fit_on_one_protocol_line() {
        assert!(printable("ghs_abc123"));
        for bad in ["", "a b", "a\nusername=x", "a\u{0}"] {
            assert!(!printable(bad), "{bad:?}");
        }
    }
}
