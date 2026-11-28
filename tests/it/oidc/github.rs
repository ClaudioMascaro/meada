use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use jsonwebtoken::{Algorithm, EncodingKey};
use serde_json::{Value, json};

use super::{Issuer, PUBLIC, SCOPE, SERVED, WEB, ask, key, set, sign, trace, traced};
use crate::github_app::{Api, fixture, prefix};
use crate::manifest::{MANIFEST, chain_manifest, run};
use crate::postgres::Relay;
use crate::server::Server;

pub(crate) const HOST: &str = "example.com";
const GATEWAY: &str = "example.com/chain/gateway";
const CATALOG: &str = "example.com/chain/catalog";
const WORKER: &str = "example.com/chain/worker";
pub(crate) const LOGIN_CLAIM: &str = "github_login";
pub(crate) const POLL_MS: &str = "200";
const EN: &str = "en_US.UTF-8";

pub(crate) struct Github {
    pub(crate) api: Api,
    case: String,
    valkey: String,
    pub(crate) url: String,
    pub(crate) schema: String,
}

impl Github {
    pub(crate) fn new(case: &str) -> Option<Self> {
        let valkey = crate::github_app::service()?;
        let url = crate::postgres::service()?;
        let schema = crate::postgres::schema(case);
        crate::postgres::Admin::new(&url).run(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"));
        let api = Api::new();
        api.install(1, 11, "chain");
        api.install(1, 11, "team");
        Some(Self {
            api,
            case: case.into(),
            valkey,
            url,
            schema,
        })
    }

    pub(crate) fn sections(&self, host: &str, roles: &str, max_age: u64) -> String {
        format!(
            "\n[oidc.github]\nhost = \"{host}\"\nlogin_claim = \"{LOGIN_CLAIM}\"\nroles = {roles}\n\
             max_age_seconds = {max_age}\n\n[postgres]\nurl_file = \"pg-url\"\nschema = \"{}\"\n",
            self.schema,
        )
    }

    pub(crate) fn job(&self, host: &str) -> String {
        format!(
            "\n[valkey]\nurl = \"{}\"\nkey_prefix = \"{}\"\n{}",
            self.valkey,
            prefix(&self.case),
            self.app(host)
        )
    }

    pub(crate) fn app(&self, host: &str) -> String {
        format!(
            "\n[[github_app]]\nhost = \"{host}\"\napi = \"{}\"\napp_id = 1\n\
             private_key_file = \"{}\"\n",
            self.api.base,
            fixture("app1.pem").display()
        )
    }

    pub(crate) fn list(&self, repo: &str, collaborators: &[(&str, &str)]) {
        let rows = (collaborators.iter())
            .map(|(l, r)| (l.to_string(), r.to_string()))
            .collect();
        (self.api).with(|s| s.collaborators.insert(repo.to_string(), rows));
    }

    pub(crate) fn migrate(&self, dir: &Path, manifest: &str, url: &str) {
        let ops = dir.join(manifest).parent().unwrap().to_path_buf();
        fs::write(ops.join("pg-url"), url).unwrap();
        let env = [("MEADA_PERMISSIONS_POLL_MS", POLL_MS)];
        let migrate = ["postgres", "migrate", "--workspace", manifest];
        let (_, stderr, code) = crate::postgres::meada(dir, EN, &env, &migrate);
        assert_eq!(code, 0, "{stderr}");
    }

    pub(crate) fn seed(&self, dir: &Path, manifest: &str, url: &str) -> String {
        self.migrate(dir, manifest, url);
        let env = [("MEADA_PERMISSIONS_POLL_MS", POLL_MS)];
        let roles = ["github-app", "roles", "--workspace", manifest];
        let (stdout, stderr, code) = crate::postgres::meada(dir, EN, &env, &roles);
        assert_eq!(code, 0, "{stderr}");
        stdout
    }

    pub(crate) fn clean(&self, stderr: &str, tokens: &[String]) {
        self.api.clean(stderr);
        for token in tokens {
            for part in token.split('.') {
                assert!(!stderr.contains(part), "a token part on stderr: {stderr}");
            }
        }
        let lower = stderr.to_lowercase();
        for login in ["ana-souza", "bia_acme", "caio"] {
            assert!(!lower.contains(login), "{login} on stderr: {stderr}");
        }
        assert!(!stderr.contains(&self.url), "{stderr}");
        if let Some((userinfo, _)) =
            (self.url.split_once("://")).and_then(|(_, r)| r.split_once('@'))
        {
            assert!(!stderr.contains(userinfo), "{stderr}");
        }
    }
}

fn node_manifest(case: &str, issuer: &str, github: &Github, roles: &str, max_age: u64) -> PathBuf {
    let dir = chain_manifest(case);
    assert!(run(&dir, "index").2);
    let file = dir.join(MANIFEST);
    let section = format!(
        "\n[oidc]\nissuer = \"{issuer}\"\npublic_url = \"{PUBLIC}/\"\naudience = \"{PUBLIC}\"\n\
         identities_claim = \"meada_repositories\"\nscopes = [\"{SCOPE}\"]\n{}{}",
        github.sections(HOST, roles, max_age),
        github.job(HOST)
    );
    fs::write(&file, fs::read_to_string(&file).unwrap() + &section).unwrap();
    dir
}

fn node(dir: &Path, env: &[(&str, &str)]) -> Server {
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    let mut env = env.to_vec();
    env.push(("MEADA_PERMISSIONS_POLL_MS", POLL_MS));
    Server::start_args(dir, &args, &env)
}

fn token(signing: &EncodingKey, issuer: &str, identities: &[&str], login: Option<Value>) -> String {
    let mut claims = json!({
        "iss": issuer, "aud": PUBLIC, "exp": jsonwebtoken::get_current_timestamp() + 300,
        "meada_repositories": identities, "scope": format!("openid {SCOPE}"),
    });
    if let Some(login) = login {
        claims[LOGIN_CLAIM] = login;
    }
    sign(signing, "k1", Algorithm::ES256, &claims)
}

fn listed(server: &Server, token: &str) -> String {
    let answer = ask(&format!("{}/", server.url), Some(token), &[], None);
    assert_eq!(answer.status, 404, "{answer:?}");
    answer.body
}

fn until(server: &Server, token: &str, expected: &str, within: Duration) -> Duration {
    let start = Instant::now();
    loop {
        let found = listed(server, token);
        if found == expected {
            return start.elapsed();
        }
        assert!(
            start.elapsed() < within,
            "still {found:?}, not {expected:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn poll_ms() -> u64 {
    POLL_MS.parse().unwrap()
}

fn nap_too_slow(polls: u64) -> bool {
    let poll = poll_ms();
    let began = Instant::now();
    std::thread::sleep(Duration::from_millis(polls * poll));
    let took = began.elapsed();
    let slow = took > Duration::from_millis((polls + 2) * poll);
    if slow {
        eprintln!("too slow to tell: a {polls} poll sleep took {took:?}");
    }
    slow
}

fn lines(identities: &[&str]) -> String {
    (SERVED.lines())
        .filter(|i| identities.contains(i))
        .map(|i| format!("{i}\n"))
        .collect()
}

#[test]
fn a_token_reads_the_union_of_its_claims_and_its_login_s_roles() {
    let Some(github) = Github::new("gh_union") else {
        return;
    };
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let roles = "[\"write\", \"Custom Reviewer\"]";
    let dir = node_manifest("gh_union", &issuer.base, &github, roles, 5400);
    github.list(
        "chain/web",
        &[("Ana-Souza", "write"), ("bia_acme", "triage")],
    );
    github.list("chain/gateway", &[("ana-souza", "Custom Reviewer")]);
    github.list("chain/catalog", &[("ana-souza", "triage")]);
    github.list("chain/worker", &[]);
    let line = github.seed(&dir, MANIFEST, &github.url);
    assert!(
        line.ends_with(": 4 repositories, 4 rows, 0 exclusions\n"),
        "{line}"
    );
    let server = node(&dir, &[]);
    let ana = token(&signing, &issuer.base, &[WORKER], Some(json!("ANA-souza")));
    until(
        &server,
        &ana,
        &lines(&[WEB, GATEWAY, WORKER]),
        Duration::from_secs(20),
    );
    let partial = traced(&ask(
        &format!("{}/{WEB}", server.url),
        Some(&ana),
        &[],
        trace(),
    ));
    assert!(
        partial.contains(&format!("{GATEWAY} src/orders.ts:5 listOrders")),
        "{partial}"
    );
    assert!(!partial.contains(CATALOG), "{partial}");
    let refused = ask(
        &format!("{}/{CATALOG}", server.url),
        Some(&ana),
        &[],
        trace(),
    );
    assert_eq!(refused.status, 403);

    let mut sent = vec![ana];
    let forty = "a".repeat(40);
    for login in [
        None,
        Some(json!("")),
        Some(json!(forty)),
        Some(json!(42)),
        Some(json!("ana.souza")),
        Some(json!(["ana-souza"])),
    ] {
        let claims_only = token(&signing, &issuer.base, &[WORKER], login.clone());
        assert_eq!(listed(&server, &claims_only), lines(&[WORKER]), "{login:?}");
        let answer = ask(
            &format!("{}/{WEB}", server.url),
            Some(&claims_only),
            &[],
            trace(),
        );
        assert_eq!(answer.status, 403, "{login:?}");
        sent.push(claims_only);
    }
    let bia = token(&signing, &issuer.base, &[], Some(json!("bia_acme")));
    assert_eq!(listed(&server, &bia), "");
    let machine = token(&signing, &issuer.base, &["*"], None);
    assert_eq!(listed(&server, &machine), SERVED);
    github.list("chain/gateway", &[]);
    github.seed(&dir, MANIFEST, &github.url);
    until(
        &server,
        &sent[0],
        &lines(&[WEB, WORKER]),
        Duration::from_secs(5),
    );
    sent.extend([bia, machine]);
    github.clean(&server.stderr(), &sent);
}

#[test]
fn the_github_grant_ages_out_while_postgres_is_unreachable() {
    let Some(github) = Github::new("gh_age") else {
        return;
    };
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let relay = Relay::new(&github.url);
    let dir = node_manifest("gh_age", &issuer.base, &github, "[\"write\"]", 5);
    github.list("chain/web", &[("ana-souza", "write")]);
    let seeded = Instant::now();
    github.seed(&dir, MANIFEST, &relay.url);
    let server = node(&dir, &[]);
    let ana = token(&signing, &issuer.base, &[WORKER], Some(json!("ana-souza")));
    let both = lines(&[WEB, WORKER]);
    until(&server, &ana, &both, Duration::from_secs(2));
    relay.close();
    let down = Instant::now();
    assert_eq!(listed(&server, &ana), both);
    until(&server, &ana, &lines(&[WORKER]), Duration::from_secs(10));
    let machine = token(&signing, &issuer.base, &["*"], None);
    assert_eq!(listed(&server, &machine), SERVED);
    assert!(
        seeded.elapsed() >= Duration::from_millis(5000 - 100),
        "{:?}",
        seeded.elapsed()
    );
    if nap_too_slow(3) {
        return;
    }
    let failing = down.elapsed();
    relay.open();
    github.seed(&dir, MANIFEST, &relay.url);
    until(&server, &ana, &both, Duration::from_secs(5));
    let stderr = server.stderr();
    let failed = (stderr.lines())
        .filter(|l| l.starts_with("meada: the permissions set cannot be read ("))
        .count();
    let polls = (failing.as_millis() / u128::from(poll_ms())) as usize;
    assert!(
        failed >= 2 && failed <= polls + 2,
        "{failed} for {polls}: {stderr}"
    );
    assert!(
        (stderr.lines()).all(|l| l.starts_with("meada: the permissions set cannot be read (")),
        "{stderr}"
    );
    github.clean(&stderr, &[ana, machine]);
}

#[test]
fn a_node_serves_without_postgres_and_names_an_unknown_version_once() {
    let Some(github) = Github::new("gh_down") else {
        return;
    };
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let relay = Relay::new(&github.url);
    let dir = node_manifest("gh_down", &issuer.base, &github, "[\"write\"]", 5400);
    github.list("chain/web", &[("ana-souza", "write")]);
    github.seed(&dir, MANIFEST, &relay.url);
    let ana = token(&signing, &issuer.base, &[WORKER], Some(json!("ana-souza")));
    let machine = token(&signing, &issuer.base, &["*"], None);

    relay.close();
    let server = node(&dir, &[]);
    assert_eq!(listed(&server, &machine), SERVED);
    if nap_too_slow(4) {
        return;
    }
    assert_eq!(listed(&server, &ana), lines(&[WORKER]));
    assert_eq!(listed(&server, &machine), SERVED);
    let stderr = server.stderr();
    assert!(
        stderr.contains("meada: the permissions set cannot be read ("),
        "{stderr}"
    );
    assert!(!stderr.contains("version"), "{stderr}");
    github.clean(&stderr, &[ana.clone(), machine.clone()]);

    relay.open();
    let server = node(&dir, &[("MEADA_POSTGRES_VERSION_MAX", "4")]);
    if nap_too_slow(5) {
        return;
    }
    assert_eq!(listed(&server, &ana), lines(&[WORKER]));
    assert_eq!(listed(&server, &machine), SERVED);
    let stderr = server.stderr();
    let named = format!(
        "is at version {}, above this Meada's 4",
        crate::common::SCHEMA_VERSION
    );
    assert_eq!(stderr.matches(&named).count(), 1, "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    github.clean(&stderr, &[ana, machine]);
}

#[test]
fn a_future_set_and_hostile_rows_grant_nothing() {
    let Some(github) = Github::new("gh_hostile") else {
        return;
    };
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = node_manifest("gh_hostile", &issuer.base, &github, "[\"write\"]", 5400);
    github.list("chain/web", &[("ana-souza", "write")]);
    github.seed(&dir, MANIFEST, &github.url);
    let admin = crate::postgres::Admin::new(&github.url);
    let schema = &github.schema;
    admin.run(&format!(
        "UPDATE {schema}.permission_sets SET finished = now() + interval '1 hour';
         INSERT INTO {schema}.permission_rows (set_id, identity, login, role)
           SELECT id, '*', 'ana-souza', 'write' FROM {schema}.permission_sets;
         INSERT INTO {schema}.permission_rows (set_id, identity, login, role)
           SELECT id, 'gitlab.com/chain/worker', 'ana-souza', 'write' FROM {schema}.permission_sets;"
    ));
    let server = node(&dir, &[]);
    let ana = token(&signing, &issuer.base, &[], Some(json!("ana-souza")));
    if nap_too_slow(3) {
        return;
    }
    assert_eq!(listed(&server, &ana), "");
    admin.run(&format!(
        "UPDATE {schema}.permission_sets SET finished = now()"
    ));
    until(&server, &ana, &lines(&[WEB]), Duration::from_secs(5));
    let stderr = server.stderr();
    let sets = format!("\"{schema}\".permission_sets: a row fails its checks and is skipped");
    let rows = format!("\"{schema}\".permission_rows: a row fails its checks and is skipped");
    assert!(stderr.lines().any(|l| l == sets), "{stderr}");
    assert_eq!(stderr.lines().filter(|l| *l == rows).count(), 2, "{stderr}");
    assert!((stderr.lines()).all(|l| l == sets || l == rows), "{stderr}");
    github.clean(&stderr, &[ana]);
}
