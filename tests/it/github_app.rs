//! The GitHub App module (ADR 0013, spec #289, feature `github-app`): `meada github-app
//! credential` driven through git's credential-helper protocol on stdin and stdout, against a
//! GitHub API the test serves on loopback (installations and token mints, each App's JWT
//! verified against the test key under `tests/fixtures/github-app/`) and the local Valkey
//! `MEADA_TEST_VALKEY_URL` names. Without the variable the tests that need Valkey print why and
//! skip; with `MEADA_TEST_SERVICES_REQUIRED=1` they fail.
#![cfg(feature = "github-app")]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde_json::{Value, json};

use crate::common;
use crate::manifest::{MANIFEST, chain_manifest, run};
use crate::server::{Server, get as http_get};

#[cfg(all(feature = "postgres", feature = "oidc", feature = "generation-pointer"))]
mod roles;

const HOST: &str = "github.test";
/// What the fake answers as every token's expiry, and its Unix time.
const EXPIRES_AT: &str = "2099-01-01T00:00:00Z";
const EXPIRES: u64 = 4_070_908_800;

/// The local Valkey, or `None` with the reason printed.
pub(crate) fn service() -> Option<String> {
    let url = std::env::var("MEADA_TEST_VALKEY_URL").ok();
    if url.is_none() {
        let required = std::env::var("MEADA_TEST_SERVICES_REQUIRED").is_ok_and(|v| v == "1");
        assert!(!required, "MEADA_TEST_VALKEY_URL is not set");
        eprintln!("skipped: MEADA_TEST_VALKEY_URL is not set");
    }
    url
}

pub(crate) fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/github-app")
        .join(name)
}

/// What the fake API holds and saw.
#[derive(Default)]
pub(crate) struct State {
    /// Per App id: its installations, as id and account login.
    apps: HashMap<u64, Vec<(u64, String)>>,
    /// Installations whose mint answers 429 with this `retry-after`.
    throttled: HashMap<u64, u64>,
    /// Installations whose mint answers 500.
    failing: HashSet<u64>,
    /// Every token minted, with its installation, in order.
    minted: Vec<(u64, String)>,
    /// Every JWT received.
    jwts: Vec<String>,
    pub(crate) requests: usize,
    /// Per `<owner>/<repo>`: its collaborators, login and `role_name`, in the order listed; a
    /// repository not here answers 404.
    pub(crate) collaborators: HashMap<String, Vec<(String, String)>>,
    /// Per installation id: the `<owner>/<repo>` names `GET /installation/repositories` lists
    /// for it, in order; an installation not here lists none.
    pub(crate) covered: HashMap<u64, Vec<String>>,
    /// The body every `/installation/repositories` page answers instead of the fake's own, when
    /// set (a hostile or malformed one).
    pub(crate) covered_body: Option<String>,
    /// Installations whose listings answer this status (403 or 429), with this `retry-after`.
    listing_throttled: HashMap<u64, (u16, Option<u64>)>,
    /// The status every listing answers, when set (a 5xx).
    pub(crate) listing_status: Option<u16>,
    /// The status `GET /app/installations` answers, when set (a 5xx).
    pub(crate) installations_status: Option<u16>,
    /// Every listing answers a body of this many bytes, when set.
    listing_bytes: Option<usize>,
    /// The `Link` header every listing page answers instead of the fake's own `next`.
    link: Option<String>,
    /// Every listing request: the installation whose token it carried, and its path and query.
    listings: Vec<(u64, String)>,
    /// The next listing request waits while `holding` is set, which it sets itself.
    hold_next: bool,
    holding: bool,
}

/// A GitHub API on loopback: `GET /app/installations` and `POST
/// /app/installations/{id}/access_tokens`, each refused with 401 unless its JWT is RS256, signed
/// by the key of the App its `iss` names, issued at least 50 s ago and expiring within 10
/// minutes; and `GET /repos/{owner}/{repo}/collaborators` with a token it minted, 100 a page with
/// a `Link` to the next page on `/repositories/<n>/` as GitHub's are. Each connection is answered
/// on a thread of its own, so a held listing holds only itself.
pub(crate) struct Api {
    pub(crate) base: String,
    state: Arc<Mutex<State>>,
}

impl Api {
    pub(crate) fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State::default()));
        let held = state.clone();
        let keys: HashMap<u64, DecodingKey> = (1..=2)
            .map(|n| {
                let pem = fs::read(fixture(&format!("app{n}.pub.pem"))).unwrap();
                (n, DecodingKey::from_rsa_pem(&pem).unwrap())
            })
            .collect();
        let at = base.clone();
        let keys = Arc::new(keys);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (held, keys, at) = (held.clone(), keys.clone(), at.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    let _ = reader.read_line(&mut line);
                    let mut jwt = None;
                    let mut header = String::new();
                    while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                        if let Some((name, value)) = header.trim_end().split_once(": ")
                            && name.eq_ignore_ascii_case("authorization")
                        {
                            jwt = value.strip_prefix("Bearer ").map(str::to_string);
                        }
                        header.clear();
                    }
                    let mut parts = line.split(' ');
                    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                    if path.contains("/collaborators") {
                        let hold = std::mem::take(&mut held.lock().unwrap().hold_next);
                        if hold {
                            held.lock().unwrap().holding = true;
                            while held.lock().unwrap().holding {
                                std::thread::sleep(std::time::Duration::from_millis(20));
                            }
                        }
                    }
                    let (status, extra, body) = answer(&held, &keys, &at, method, path, jwt);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} X\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                });
            }
        });
        Self { base, state }
    }

    pub(crate) fn install(&self, app: u64, id: u64, login: &str) {
        let mut state = self.state.lock().unwrap();
        state.apps.entry(app).or_default().push((id, login.into()));
    }

    pub(crate) fn with<T>(&self, read: impl FnOnce(&mut State) -> T) -> T {
        read(&mut self.state.lock().unwrap())
    }

    /// The installation the fake minted `token` for.
    fn minted_for(&self, token: &str) -> u64 {
        let state = self.state.lock().unwrap();
        let found = state.minted.iter().find(|(_, t)| t == token);
        found.unwrap_or_else(|| panic!("not a minted token")).0
    }

    /// Asserts `stderr` holds no part of a private key, a JWT or a token.
    pub(crate) fn clean(&self, stderr: &str) {
        let state = self.state.lock().unwrap();
        for jwt in &state.jwts {
            for part in jwt.split('.').skip(1) {
                assert!(!stderr.contains(part), "a JWT part on stderr: {stderr}");
            }
        }
        for (_, token) in &state.minted {
            assert!(
                !stderr.contains(token.as_str()),
                "a token on stderr: {stderr}"
            );
        }
        for n in 1..=2 {
            let pem = fs::read_to_string(fixture(&format!("app{n}.pem"))).unwrap();
            for line in pem.lines().filter(|l| !l.starts_with("-----")) {
                assert!(!stderr.contains(line), "a key line on stderr: {stderr}");
            }
        }
    }
}

/// A token made now: another on each call, no literal credential.
fn made_token() -> String {
    use std::hash::{BuildHasher, RandomState};
    format!(
        "ghs_{:016x}",
        RandomState::new().hash_one(std::process::id())
    )
}

/// The fake's answer to `method path` with `jwt` (a JWT, or a token for a listing), the fake
/// served at `base`: status, extra header lines and body.
fn answer(
    state: &Mutex<State>,
    keys: &HashMap<u64, DecodingKey>,
    base: &str,
    method: &str,
    path: &str,
    jwt: Option<String>,
) -> (u16, String, String) {
    let mut state = state.lock().unwrap();
    state.requests += 1;
    let Some(jwt) = jwt else {
        return (401, String::new(), String::new());
    };
    if let Some(listing) =
        (path.strip_prefix("/repos/")).and_then(|rest| rest.split_once("/collaborators?"))
    {
        return collaborators(&mut state, base, method, path, listing, &jwt);
    }
    if let Some(query) = path.strip_prefix("/installation/repositories?") {
        return covered(&mut state, method, path, query, &jwt);
    }
    state.jwts.push(jwt.clone());
    let now = jsonwebtoken::get_current_timestamp();
    let app = keys.iter().find_map(|(app, key)| {
        let claims = jsonwebtoken::decode::<Value>(&jwt, key, &Validation::new(Algorithm::RS256));
        let claims = claims.ok()?.claims;
        let (iat, exp) = (claims["iat"].as_u64()?, claims["exp"].as_u64()?);
        let fits = claims["iss"].as_u64() == Some(*app) && iat <= now - 50 && exp <= now + 600;
        fits.then_some(*app)
    });
    let Some(app) = app else {
        return (401, String::new(), String::new());
    };
    let installations = state.apps.get(&app).cloned().unwrap_or_default();
    if method == "GET" && path.starts_with("/app/installations?per_page=100&page=") {
        if let Some(status) = state.installations_status {
            return (status, String::new(), String::new());
        }
        let page: Vec<Value> = match path.ends_with("page=1") {
            true => (installations.iter())
                .map(|(id, login)| json!({"id": id, "account": {"login": login}}))
                .collect(),
            false => Vec::new(),
        };
        return (200, String::new(), json!(page).to_string());
    }
    let id = (path.strip_prefix("/app/installations/"))
        .and_then(|rest| rest.strip_suffix("/access_tokens"))
        .and_then(|id| id.parse::<u64>().ok());
    match id {
        Some(id) if method == "POST" && installations.iter().any(|(i, _)| *i == id) => {
            if let Some(after) = state.throttled.get(&id) {
                return (429, format!("Retry-After: {after}\r\n"), String::new());
            }
            if state.failing.contains(&id) {
                return (500, String::new(), String::new());
            }
            let token = made_token();
            state.minted.push((id, token.clone()));
            let body = json!({"token": token, "expires_at": EXPIRES_AT});
            (201, String::new(), body.to_string())
        }
        _ => (404, String::new(), String::new()),
    }
}

/// The fake's answer to a collaborators listing: `listing` the `<owner>/<repo>` and the query.
fn collaborators(
    state: &mut State,
    base: &str,
    method: &str,
    path: &str,
    (repo, query): (&str, &str),
    token: &str,
) -> (u16, String, String) {
    let Some(&(id, _)) = state.minted.iter().find(|(_, t)| t == token) else {
        return (401, String::new(), String::new());
    };
    state.listings.push((id, path.to_string()));
    if method != "GET" {
        return (404, String::new(), String::new());
    }
    if let Some(&(status, after)) = state.listing_throttled.get(&id) {
        let after = after.map_or(String::new(), |a| format!("Retry-After: {a}\r\n"));
        return (status, after, String::new());
    }
    if let Some(status) = state.listing_status {
        return (status, String::new(), String::new());
    }
    if let Some(bytes) = state.listing_bytes {
        return (200, String::new(), format!("[{}]", " ".repeat(bytes - 2)));
    }
    let mut names: Vec<&String> = state.collaborators.keys().collect();
    names.sort();
    let Some(n) = names.iter().position(|name| name.as_str() == repo) else {
        return (404, String::new(), String::new());
    };
    let page: usize = (query.split('&'))
        .find_map(|pair| pair.strip_prefix("page="))
        .and_then(|page| page.parse().ok())
        .unwrap_or(1);
    let all = &state.collaborators[repo];
    let rows: Vec<Value> = (all.iter().skip((page - 1) * 100).take(100))
        .map(|(login, role)| json!({"login": login, "type": "User", "role_name": role}))
        .collect();
    let link = match &state.link {
        Some(link) => format!("Link: {link}\r\n"),
        None if page * 100 < all.len() => format!(
            "Link: <{base}/repositories/{}/collaborators?affiliation=all&per_page=100&page={}>; \
             rel=\"next\"\r\n",
            n + 1,
            page + 1
        ),
        None => String::new(),
    };
    (200, link, json!(rows).to_string())
}

/// The fake's answer to an installation's repositories listing (#341): 100 a page as
/// `{"total_count", "repositories": [{"full_name"}]}`, for the installation `token` was minted
/// for, with the throttles and failures the collaborators listing has.
fn covered(
    state: &mut State,
    method: &str,
    path: &str,
    query: &str,
    token: &str,
) -> (u16, String, String) {
    let Some(&(id, _)) = state.minted.iter().find(|(_, t)| t == token) else {
        return (401, String::new(), String::new());
    };
    state.listings.push((id, path.to_string()));
    if method != "GET" {
        return (404, String::new(), String::new());
    }
    if let Some(&(status, after)) = state.listing_throttled.get(&id) {
        let after = after.map_or(String::new(), |a| format!("Retry-After: {a}\r\n"));
        return (status, after, String::new());
    }
    if let Some(status) = state.listing_status {
        return (status, String::new(), String::new());
    }
    if let Some(body) = &state.covered_body {
        return (200, String::new(), body.clone());
    }
    let page: usize = (query.split('&'))
        .find_map(|pair| pair.strip_prefix("page="))
        .and_then(|page| page.parse().ok())
        .unwrap_or(1);
    let all = state.covered.get(&id).cloned().unwrap_or_default();
    let names: Vec<Value> = (all.iter().skip((page - 1) * 100).take(100))
        .map(|name| json!({"full_name": name, "id": 1}))
        .collect();
    let body = json!({"total_count": all.len(), "repositories": names});
    (200, String::new(), body.to_string())
}

/// `<case>/meada.workspace.toml` with `[valkey]` at `valkey` (the case's own key prefix) and
/// one `[[github_app]]` block per App id of `apps`, each signing with its fixture key.
fn manifest(case: &str, valkey: &str, api: &str, apps: &[u64]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(case);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut toml = format!(
        "index_dir = \"indexes\"\n\n[valkey]\nurl = \"{valkey}\"\nkey_prefix = \"{}\"\n",
        prefix(case)
    );
    for app in apps {
        toml += &block(api, *app);
    }
    fs::write(dir.join("meada.workspace.toml"), toml).unwrap();
    dir
}

fn block(api: &str, app: u64) -> String {
    let key = fixture(&format!("app{app}.pem"));
    format!(
        "\n[[github_app]]\nhost = \"{HOST}\"\napi = \"{api}\"\napp_id = {app}\n\
         private_key_file = \"{}\"\n",
        key.display()
    )
}

pub(crate) fn prefix(case: &str) -> String {
    format!("meada:test-{}-{case}", std::process::id())
}

/// `meada github-app credential --workspace meada.workspace.toml <action>` in `dir` with
/// `input`: stdout, stderr and whether it exited 0.
fn helper(dir: &Path, action: &str, input: &str) -> (String, String, bool) {
    let args = [
        "github-app",
        "credential",
        "--workspace",
        "meada.workspace.toml",
        action,
    ];
    common::meada(dir, &args, input, |c| {
        c.env("LANG", "en_US.UTF-8");
    })
}

/// `get` for `https://<host>/<path>`.
fn ask(dir: &Path, host: &str, path: &str) -> (String, String, bool) {
    helper(
        dir,
        "get",
        &format!("protocol=https\nhost={host}\npath={path}\n\n"),
    )
}

/// The password a `get` for `HOST/<path>` answers, checking the rest of the answer.
fn token(api: &Api, dir: &Path, path: &str) -> String {
    let (stdout, stderr, ok) = ask(dir, HOST, path);
    api.clean(&stderr);
    assert!(ok, "{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{stdout:?} {stderr}");
    assert_eq!(lines[0], "username=x-access-token");
    assert_eq!(lines[2], format!("password_expiry_utc={EXPIRES}"));
    lines[1].strip_prefix("password=").unwrap().to_string()
}

/// `meada github-app throttled` for `HOST/<path>`, with `retry_after`: its stderr.
fn throttled(api: &Api, dir: &Path, path: &str, retry_after: Option<&str>) -> String {
    let url = format!("https://{HOST}/{path}");
    let mut args = vec![
        "github-app",
        "throttled",
        "--workspace",
        "meada.workspace.toml",
        &url,
    ];
    if let Some(after) = retry_after {
        args.extend(["--retry-after", after]);
    }
    let (_, stderr, ok) = common::meada(dir, &args, "", |c| {
        c.env("LANG", "en_US.UTF-8");
    });
    api.clean(&stderr);
    assert!(ok, "{stderr}");
    stderr
}

fn admin(url: &str) -> redis::Connection {
    redis::Client::open(url).unwrap().get_connection().unwrap()
}

/// The TTL Valkey reports for `name` under the case's module keys.
fn ttl(valkey: &mut redis::Connection, case: &str, name: &str) -> i64 {
    let key = format!("{}:github-app:{HOST}:{name}", prefix(case));
    redis::cmd("TTL").arg(key).query(valkey).unwrap()
}

#[test]
fn a_covered_repository_gets_a_minted_token_and_a_second_get_mints_nothing() {
    let Some(valkey) = service() else { return };
    let api = Api::new();
    api.install(1, 11, "Acme");
    api.install(1, 12, "beta");
    let dir = manifest("github_app_mint", &valkey, &api.base, &[1]);
    let first = token(&api, &dir, "acme/web.git");
    assert_eq!(api.with(|s| s.minted.clone()), vec![(11, first.clone())]);
    assert_eq!(token(&api, &dir, "ACME/other"), first);
    assert_eq!(api.with(|s| s.minted.len()), 1);
    // The cached token lives until 10 minutes before it expires.
    let mut admin = admin(&valkey);
    let left = ttl(&mut admin, "github_app_mint", "installation:11:token");
    let now = jsonwebtoken::get_current_timestamp() as i64;
    assert!((left - (EXPIRES as i64 - now - 600)).abs() <= 5, "{left}");
    // `store` does nothing.
    let input =
        format!("protocol=https\nhost={HOST}\npath=acme/web\nusername=x\npassword={first}\n");
    assert_eq!(
        helper(&dir, "store", &input),
        (String::new(), String::new(), true)
    );
    // A mint that fails is an error, and says nothing secret.
    api.with(|s| s.failing.insert(12));
    let (stdout, stderr, ok) = ask(&dir, HOST, "beta/web");
    assert!(!ok && stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("http status 500"), "{stderr}");
    api.clean(&stderr);
}

#[test]
fn two_apps_alternate_and_a_throttled_installation_is_skipped_until_its_retry_time() {
    let Some(valkey) = service() else { return };
    let api = Api::new();
    api.install(1, 11, "acme");
    api.install(2, 21, "acme");
    let case = "github_app_round_robin";
    let dir = manifest(case, &valkey, &api.base, &[2, 1]);
    let served: Vec<u64> = (0..4)
        .map(|_| api.minted_for(&token(&api, &dir, "acme/web")))
        .collect();
    assert_eq!(served, vec![11, 21, 11, 21]);
    // The last `get` for acme/web was served by 21: a fetch of it failed with 429.
    let stderr = throttled(&api, &dir, "acme/web", Some("1"));
    assert_eq!(stderr, "meada: installation 21: throttled for 1 s\n");
    for _ in 0..2 {
        assert_eq!(api.minted_for(&token(&api, &dir, "acme/api")), 11);
    }
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let served: HashSet<u64> = (0..2)
        .map(|_| api.minted_for(&token(&api, &dir, "acme/web")))
        .collect();
    assert_eq!(served, HashSet::from([11, 21]));
    // Without a retry time: one minute, doubled on each repeat while it holds.
    let last = api.minted_for(&token(&api, &dir, "acme/web"));
    let mut admin = admin(&valkey);
    let mark = format!("installation:{last}:throttled");
    throttled(&api, &dir, "acme/web", None);
    assert!((55..=60).contains(&ttl(&mut admin, case, &mark)));
    throttled(&api, &dir, "acme/web", None);
    assert!((115..=120).contains(&ttl(&mut admin, case, &mark)));
    // A repository no `get` answered marks nothing.
    let stderr = throttled(&api, &dir, "acme/never", None);
    assert!(stderr.contains("nothing marked"), "{stderr}");
}

#[test]
fn a_mint_answered_429_marks_its_installation_and_another_one_answers() {
    let Some(valkey) = service() else { return };
    let api = Api::new();
    api.install(1, 11, "acme");
    api.install(2, 21, "acme");
    api.with(|s| s.throttled.insert(11, 120));
    let case = "github_app_mint_429";
    let dir = manifest(case, &valkey, &api.base, &[1, 2]);
    // The first in order is 11: its mint is throttled, so 21 answers this `get` and the next.
    let (stdout, stderr, ok) = ask(&dir, HOST, "acme/web");
    assert!(ok, "{stderr}");
    assert_eq!(stderr, "meada: installation 11: throttled for 120 s\n");
    api.clean(&stderr);
    let password = stdout
        .lines()
        .nth(1)
        .unwrap()
        .strip_prefix("password=")
        .unwrap();
    assert_eq!(api.minted_for(password), 21);
    assert_eq!(api.minted_for(&token(&api, &dir, "acme/web")), 21);
    let mut admin = admin(&valkey);
    assert!((115..=120).contains(&ttl(&mut admin, case, "installation:11:throttled")));
    assert_eq!(api.with(|s| s.minted.len()), 1);
    // With 21's mint throttled too, the `get` fails saying so, and tries nothing "anyway".
    api.with(|s| s.throttled.insert(21, 120));
    let cached = format!("{}:github-app:{HOST}:installation:21:token", prefix(case));
    let _: () = redis::cmd("DEL").arg(cached).query(&mut admin).unwrap();
    let (_, stderr, ok) = ask(&dir, HOST, "acme/web");
    api.clean(&stderr);
    assert!(!ok, "{stderr}");
    let failed = format!("no installation covering {HOST}/acme gave a token: each mint was");
    assert!(
        stderr.contains(&failed) && !stderr.contains("anyway"),
        "{stderr}"
    );
}

#[test]
fn a_token_whose_cache_ran_out_or_that_git_erased_is_minted_again() {
    let Some(valkey) = service() else { return };
    let api = Api::new();
    api.install(1, 11, "acme");
    let case = "github_app_again";
    let dir = manifest(case, &valkey, &api.base, &[1]);
    let first = token(&api, &dir, "acme/web");
    let key = format!("{}:github-app:{HOST}:installation:11:token", prefix(case));
    let mut admin = admin(&valkey);
    let _: () = redis::cmd("PEXPIRE")
        .arg(&key)
        .arg(1)
        .query(&mut admin)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let second = token(&api, &dir, "acme/web");
    assert_ne!(second, first);
    // git erases a token it was refused with; another password erases nothing.
    let erase = |password: &str| {
        let input = format!(
            "protocol=https\nhost={HOST}\npath=acme/web\nusername=x-access-token\n\
             password={password}\n"
        );
        let (stdout, stderr, ok) = helper(&dir, "erase", &input);
        api.clean(&stderr);
        assert!(
            ok && stdout.is_empty() && stderr.is_empty(),
            "{stdout}{stderr}"
        );
    };
    erase(&first);
    assert_eq!(token(&api, &dir, "acme/web"), second);
    erase(&second);
    let third = token(&api, &dir, "acme/web");
    assert_ne!(third, second);
    assert_eq!(api.with(|s| s.minted.len()), 3);
}

#[test]
fn an_uncovered_host_or_owner_gets_no_answer_and_one_line_naming_them() {
    let api = Api::new();
    api.install(1, 11, "acme");
    // Valkey is not reached for a host no App serves.
    let valkey = service();
    let at = valkey.clone().unwrap_or("redis://127.0.0.1:1".into());
    let dir = manifest("github_app_uncovered", &at, &api.base, &[1]);
    let uncovered = |host: &str, owner: &str| {
        let (stdout, stderr, ok) = ask(&dir, host, &format!("{owner}/secret-repo.git"));
        api.clean(&stderr);
        assert!(ok && stdout.is_empty(), "{stdout}{stderr}");
        assert_eq!(
            stderr,
            format!("meada: no GitHub App installation covers {host}/{owner}\n")
        );
    };
    uncovered("gitlab.test", "acme");
    if valkey.is_none() {
        return;
    }
    uncovered(HOST, "nobody");
}

#[test]
fn a_new_github_app_block_and_new_installations_are_used_after_the_refresh() {
    let Some(valkey) = service() else { return };
    let api = Api::new();
    api.install(1, 11, "acme");
    api.install(2, 21, "acme");
    let case = "github_app_refresh";
    let dir = manifest(case, &valkey, &api.base, &[1]);
    assert_eq!(api.minted_for(&token(&api, &dir, "acme/web")), 11);
    // App 1 is installed on another owner: unseen until its installations are listed again.
    api.install(1, 12, "beta");
    let (stdout, _, _) = ask(&dir, HOST, "beta/web");
    assert!(stdout.is_empty());
    let mut admin = admin(&valkey);
    let listed = format!("{}:github-app:{HOST}:app:1:listed", prefix(case));
    let _: () = redis::cmd("DEL").arg(&listed).query(&mut admin).unwrap();
    assert_eq!(api.minted_for(&token(&api, &dir, "beta/web")), 12);
    // A block added to the manifest lists its installations on the next `get`.
    let file = dir.join("meada.workspace.toml");
    fs::write(
        &file,
        fs::read_to_string(&file).unwrap() + &block(&api.base, 2),
    )
    .unwrap();
    let served: HashSet<u64> = (0..2)
        .map(|_| api.minted_for(&token(&api, &dir, "acme/web")))
        .collect();
    assert_eq!(served, HashSet::from([11, 21]));
}

#[test]
fn bad_github_app_blocks_refuse_naming_the_key_or_the_section() {
    let api = Api::new();
    let dir = manifest("github_app_bad", "redis://127.0.0.1:1", &api.base, &[]);
    let file = dir.join("meada.workspace.toml");
    let valkey = "[valkey]\nurl = \"redis://127.0.0.1:1\"\n";
    let app = "\n[[github_app]]\nhost = \"github.test\"\napp_id = 1\n";
    let key = "private_key_env = \"MEADA_TEST_GITHUB_APP_KEY\"\n";
    let https = "api = \"https://api.github.test\"\n";
    for (toml, named) in [
        (format!("{app}{https}{key}"), "[github_app] needs [valkey]"),
        (
            format!("{valkey}{app}{https}{key}private_key_file = \"k.pem\"\n"),
            "`github_app.private_key_env`",
        ),
        (format!("{valkey}{app}{https}"), "`github_app`"),
        (
            format!("{valkey}{app}api = \"http://api.github.test\"\n{key}"),
            "`github_app.api`",
        ),
        (
            format!("{valkey}{app}{https}{key}owners = [\"acme\"]\n"),
            "owners",
        ),
    ] {
        fs::write(&file, format!("index_dir = \"indexes\"\n{toml}")).unwrap();
        let (stdout, stderr, ok) = ask(&dir, HOST, "acme/web");
        assert!(!ok && stdout.is_empty(), "{toml}");
        assert!(stderr.contains(named), "{named}: {stderr}");
        api.clean(&stderr);
    }
    assert_eq!(api.with(|s| s.requests), 0);
}

#[test]
fn mcp_listen_over_a_manifest_with_github_app_makes_no_request() {
    let api = Api::new();
    api.install(1, 11, "acme");
    let dir = chain_manifest("github_app_listen");
    assert!(run(&dir, "index").2);
    let file = dir.join(MANIFEST);
    let sections = format!(
        "\n[valkey]\nurl = \"redis://127.0.0.1:1\"\n{}",
        block(&api.base, 1)
    );
    fs::write(&file, fs::read_to_string(&file).unwrap() + &sections).unwrap();
    let server = Server::start_args(
        &dir,
        &["--listen", "127.0.0.1:0", "--workspace", MANIFEST],
        &[],
    );
    let (status, _, _) = http_get(&format!("{}/nope", server.url));
    assert_eq!(status, 404);
    drop(server);
    assert_eq!(api.with(|s| s.requests), 0);
}

/// The console inspects only cached state: no private key read and no request to GitHub.
#[cfg(feature = "ui")]
#[test]
fn console_reads_installation_expiry_and_throttle_without_secrets_or_api_calls() {
    use crate::ui::{front_end, get, ui_section};
    let Some(valkey) = service() else { return };
    let case = "ui_credential_cache";
    let api = Api::new();
    api.install(1, 11, "Acme");
    let dir = manifest(case, &valkey, &api.base, &[1]);
    let secret = token(&api, &dir, "acme/web.git");
    throttled(&api, &dir, "acme/web.git", Some("60"));
    front_end(&dir, Some(env!("CARGO_PKG_VERSION")));
    let file = dir.join("meada.workspace.toml");
    let text = fs::read_to_string(&file).unwrap();
    // A missing private key is harmless to the read route: it must never open it.
    let text = text.replace(
        fixture("app1.pem").to_str().unwrap(),
        "/does/not/exist/private-key.pem",
    );
    fs::write(
        &file,
        format!("{text}\n[ui]\n{}", ui_section().replace("../ui", "ui")),
    )
    .unwrap();
    let before = api.with(|state| (state.requests, state.minted.clone(), state.jwts.clone()));
    let console = crate::server::Server::start_command(
        &dir,
        "ui",
        &[
            "--listen",
            "127.0.0.1:0",
            "--workspace",
            "meada.workspace.toml",
        ],
        &[],
    );
    let reply = get(&console, "/api/v1/credentials");
    assert_eq!(reply.status, 200, "{}", reply.body);
    let document = reply.json();
    assert_eq!(document["apps"][0]["host"], HOST);
    let installation = &document["apps"][0]["installations"][0];
    assert_eq!(installation["owner"], "acme");
    assert_eq!(installation["id"], 11);
    assert_eq!(installation["cached"], true);
    assert_eq!(installation["expires"], EXPIRES);
    assert!(installation["remaining_seconds"].as_u64().unwrap() > 0);
    assert!(
        installation["throttled_until"].as_u64().unwrap() > jsonwebtoken::get_current_timestamp()
    );
    assert!(!reply.body.contains(&secret));
    assert!(!reply.body.contains("PRIVATE KEY"));
    assert_eq!(
        before,
        api.with(|state| (state.requests, state.minted.clone(), state.jwts.clone()))
    );
    let key = format!(
        "{}:github-app:{HOST}:installation:11:throttled",
        prefix(case)
    );
    let mut connection = admin(&valkey);
    let _: () = redis::cmd("PEXPIRE")
        .arg(&key)
        .arg(400)
        .query(&mut connection)
        .unwrap();
    assert_eq!(ttl(&mut connection, case, "installation:11:throttled"), 0);
    let imminent = get(&console, "/api/v1/credentials").json();
    assert!(imminent["apps"][0]["installations"][0]["throttled_until"].is_u64());
    let stderr = console.stderr();
    assert!(!stderr.contains(&secret));
}
