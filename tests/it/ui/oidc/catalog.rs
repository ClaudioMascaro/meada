//! The catalog in the console (#341, ADR 0021 contract 1): `meada github-app roles` with
//! `[catalog]` against `github_app`'s fake GitHub API writes the permissions set, and the console
//! (`oidc`'s loopback issuer, the local Valkey and Postgres) answers `GET /api/v1/catalog` and
//! `POST /api/v1/catalog/check` from it, per caller, with the rows of the shared Y/A/B table
//! (`tests/fixtures/named-workspaces.json`, the same rows `grant::permissions`'s unit test reads).
//! What a caller may select is seen through the console's routes, never the module's tables.
//! Without a service the tests print why and skip; with `MEADA_TEST_SERVICES_REQUIRED=1` they
//! fail.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use jsonwebtoken::{Algorithm, EncodingKey};
use serde_json::{Value, json};

use super::{PUBLIC, VERSION, bearer, claims, console_dir, oidc, ui_oidc};
use crate::oidc::github::{Github, LOGIN_CLAIM, POLL_MS};
use crate::oidc::{Issuer, key, set, sign};
use crate::server::Server;
use crate::ui::{Reply, send, send_body};

/// The table's git host, `[oidc.github] host` here.
const HOST: &str = "github.com";
/// The fake's installation on the owner `acme`.
const INSTALLATION: u64 = 15;

fn table() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/named-workspaces.json")).unwrap()
}

/// One case: the fake GitHub with the table's repositories covered, the job's manifest, the
/// console running over the set the job wrote and the key that signs its tokens.
struct Case {
    github: Github,
    dir: PathBuf,
    console: Server,
    signing: EncodingKey,
    /// Serves the keys the console verifies tokens with, for the case's life.
    issuer: Issuer,
    table: Value,
}

/// The console's manifest: `[oidc]` as `oidc` has it, `[oidc.github]` with `roles` and
/// `max_age`, the job's sections, `[catalog]` and `[ui]` with the group `admins` as operators.
fn console_manifest(case: &str, issuer: &str, github: &Github, max_age: u64) -> PathBuf {
    let roles = "[\"write\", \"maintain\", \"admin\"]";
    let head = format!(
        "{}{}{}\n[catalog]\nallow = [\"{HOST}/acme/*\"]\nmax_repositories = 10\n",
        oidc(issuer),
        github.sections(HOST, roles, max_age),
        github.job(HOST),
    );
    let ui = format!("{}operator_groups = [\"admins\"]\n", ui_oidc());
    let dir = console_dir(case, Some(VERSION), &head, &ui);
    fs::rename(
        dir.join("ops/meada.workspace.toml"),
        dir.join("ops/console.toml"),
    )
    .unwrap();
    // The job reads the same sections: no `[ui]`, no operators.
    let job = format!("index_dir = \"../roles-indexes\"\n{head}");
    fs::write(dir.join("ops/roles.toml"), job).unwrap();
    dir
}

impl Case {
    /// The case `name`: the table's repositories and collaborators on the fake, a repository
    /// `acme/c` nobody collaborates on and one `other/z` no pattern admits, the set seeded once.
    fn new(name: &str, max_age: u64) -> Option<Self> {
        let github = Github::new(name)?;
        let (signing, jwk) = key("catalog");
        let issuer = Issuer::new(set(&[&jwk]));
        let dir = console_manifest(name, &issuer.base, &github, max_age);
        let table = table();
        let identities = table["identities"].as_object().unwrap();
        let mut covered = vec!["acme/c".to_string(), "other/z".to_string()];
        for (short, identity) in identities {
            let repo = identity
                .as_str()
                .unwrap()
                .strip_prefix("github.com/")
                .unwrap();
            covered.push(repo.to_string());
            let rows: Vec<(&str, &str)> = table["callers"]
                .as_object()
                .unwrap()
                .values()
                .flat_map(|caller| {
                    let login = caller["login"].as_str();
                    let rows = caller["rows"].as_array().unwrap();
                    let same = rows
                        .iter()
                        .filter(move |row| row["identity"] == short.as_str());
                    same.map(move |row| (login.unwrap(), row["role"].as_str().unwrap()))
                })
                .collect();
            github.list(repo, &rows);
        }
        github.list("acme/c", &[]);
        github.list("other/z", &[("zed", "admin")]);
        github.api.install(1, INSTALLATION, "acme");
        github.api.install(1, 16, "other");
        github.api.with(|s| {
            s.covered.insert(INSTALLATION, covered.clone());
            s.covered.insert(16, vec!["other/z".into()]);
        });
        github.migrate(&dir, "ops/roles.toml", &github.url);
        let console = Server::start_command(
            &dir,
            "ui",
            &["--listen", "127.0.0.1:0", "--workspace", "ops/console.toml"],
            &[("MEADA_PERMISSIONS_POLL_MS", POLL_MS)],
        );
        Some(Self {
            github,
            dir,
            console,
            signing,
            issuer,
            table,
        })
    }

    /// `meada github-app roles` over the job's manifest: the line on stdout.
    fn seed(&self) -> String {
        let env = [("MEADA_PERMISSIONS_POLL_MS", POLL_MS)];
        let roles = ["github-app", "roles", "--workspace", "ops/roles.toml"];
        let (stdout, stderr, code) = crate::postgres::meada(&self.dir, "en_US.UTF-8", &env, &roles);
        assert_eq!(code, 0, "{stderr}");
        stdout
    }

    /// A token of the table's caller `name`: its login claim when it has one, its claims grant
    /// and the group `admins` for the platform administrator.
    fn token(&self, name: &str) -> String {
        let caller = &self.table["callers"][name];
        let mut claims = claims(&self.issuer.base, &[]);
        claims["sub"] = json!(name);
        if let Some(login) = caller["login"].as_str() {
            claims[LOGIN_CLAIM] = json!(login);
        }
        if let Some(granted) = caller["claims_grant"].as_array() {
            let granted: Vec<&str> = (granted.iter())
                .map(|short| {
                    self.table["identities"][short.as_str().unwrap()]
                        .as_str()
                        .unwrap()
                })
                .collect();
            claims["meada_repositories"] = json!(granted);
        }
        if caller["platform_administrator"] == true {
            claims["groups"] = json!(["admins"]);
        }
        sign(&self.signing, "catalog", Algorithm::ES256, &claims)
    }

    /// The identity the table calls `short`.
    fn identity(&self, short: &str) -> String {
        self.table["identities"][short]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn get(&self, path: &str, token: &str) -> Reply {
        let headers = [("Authorization", bearer(token))];
        let headers: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
        send(&self.console, "GET", path, &headers)
    }

    fn post(&self, path: &str, token: &str, body: Value) -> Reply {
        let headers = [("Authorization", bearer(token)), ("Origin", PUBLIC.into())];
        let headers: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
        send_body(&self.console, "POST", path, &headers, &body.to_string())
    }

    /// `GET /api/v1/catalog` with `query`, once the console has read a set (the catalog is
    /// pending `no_set` until its first poll finds one).
    fn catalog(&self, token: &str, query: &str) -> Value {
        let reply = self.get(&format!("/api/v1/catalog{query}"), token);
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.json()
    }

    /// Waits until `token`'s catalog is pending `reason` (`None`: no longer pending `no_set`,
    /// whatever else it says); that answer.
    fn until(&self, token: &str, reason: Option<&str>) -> Value {
        let start = Instant::now();
        loop {
            let answer = self.catalog(token, "");
            let found = answer["reason"].as_str();
            if found == reason || (reason.is_none() && found != Some("no_set")) {
                return answer;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "{answer}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Case {
    /// Waits until `token`'s catalog is ready; that answer.
    fn ready(&self, token: &str) -> Value {
        let start = Instant::now();
        loop {
            let answer = self.catalog(token, "");
            if answer["state"] == "ready" {
                return answer;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "{answer}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The identities of a catalog answer, in order.
fn listed(answer: &Value) -> Vec<String> {
    let rows = answer["repositories"].as_array().unwrap();
    rows.iter()
        .map(|r| r["identity"].as_str().unwrap().to_string())
        .collect()
}

/// For every caller of the table: the catalog lists exactly the repositories its login holds an
/// accepted role on, direct or through a group alike (`dave` and `alice`), none through a
/// rejected role (`zed`), a claims grant (`claims`) or a token without a login (`nologin`,
/// pending `missing_login`); the platform administrator lists the whole catalog, including the
/// repository nobody collaborates on. A repository no pattern admits (`other/z`) and one that is
/// allowed but not covered are in nobody's catalog. The check agrees with the creation table
/// row by row, and a caller never reads another repository's name.
#[test]
fn the_catalog_lists_what_each_caller_may_select() {
    let Some(case) = Case::new("ui_catalog_table", 3600) else {
        return;
    };
    let line = case.seed();
    assert!(
        line.contains(": 4 repositories (4 in the catalog), 14 rows, 0 exclusions\n"),
        "{line}"
    );
    let accepted: Vec<&str> = (case.table["accepted_roles"].as_array().unwrap().iter())
        .map(|r| r.as_str().unwrap())
        .collect();
    let all: BTreeSet<String> = ["Y", "A", "B"].iter().map(|s| case.identity(s)).collect();
    let mut sent = Vec::new();
    for (name, caller) in case.table["callers"].as_object().unwrap() {
        let token = case.token(name);
        let answer = case.until(&token, None);
        let expected: BTreeSet<String> = match name.as_str() {
            "root" => all
                .iter()
                .cloned()
                .chain([format!("{HOST}/acme/c")])
                .collect(),
            "nologin" => BTreeSet::new(),
            _ => caller["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| accepted.contains(&row["role"].as_str().unwrap()))
                .map(|row| case.identity(row["identity"].as_str().unwrap()))
                .collect(),
        };
        let reason = (name == "nologin").then_some("missing_login");
        let state = if reason.is_some() { "pending" } else { "ready" };
        assert_eq!(
            (answer["state"].as_str(), answer["reason"].as_str()),
            (Some(state), reason),
            "{name}"
        );
        assert_eq!(
            listed(&answer),
            expected.iter().cloned().collect::<Vec<_>>(),
            "{name}"
        );
        assert_eq!(answer["next"], Value::Null, "{name}");
        assert!(answer["set"]["max_age_seconds"] == 3600 && answer["set"]["finished"].is_string());
        // No repository outside what the caller may select is named anywhere in the answer.
        for hidden in all
            .iter()
            .chain([&format!("{HOST}/acme/c"), &format!("{HOST}/other/z")])
        {
            if !expected.contains(hidden) {
                assert!(
                    !answer.to_string().contains(hidden.as_str()),
                    "{name}: {hidden}"
                );
            }
        }
        sent.push(token);
    }

    for row in case.table["creation"].as_array().unwrap() {
        let name = row["caller"].as_str().unwrap();
        let token = case.token(name);
        let select: Vec<String> = (row["select"].as_array().unwrap().iter())
            .map(|s| case.identity(s.as_str().unwrap()))
            .collect();
        let reply = case.post(
            "/api/v1/catalog/check",
            &token,
            json!({"identities": select}),
        );
        assert_eq!(reply.status, 200, "{name}: {}", reply.body);
        let answer = reply.json();
        let states: Vec<&str> = (answer["results"].as_array().unwrap().iter())
            .map(|r| r["state"].as_str().unwrap())
            .collect();
        let name = format!("{name} selects {select:?}");
        match row["pending"].as_str() {
            Some(reason) => {
                assert_eq!(answer["reason"], reason, "{name}");
                assert!(states.iter().all(|s| *s == "pending"), "{name}: {states:?}");
            }
            None => {
                let allowed = states.iter().all(|s| *s == "selectable");
                assert_eq!(allowed, row["allowed"] == true, "{name}: {states:?}");
                assert!(states.iter().all(|s| *s != "pending"), "{name}");
                assert_eq!(answer["reason"], Value::Null, "{name}");
            }
        }
        // An identity the caller may not select reads `unavailable` and nothing more.
        let results = answer["results"].as_array().unwrap();
        assert!(
            results.iter().all(|r| r.as_object().unwrap().len() == 2),
            "{name}"
        );
    }

    // Not an identity of the catalog at all reads like one the caller may not select.
    let bob = case.token("bob");
    let asked = [
        case.identity("A"),
        format!("{HOST}/acme/ghost"),
        case.identity("B"),
    ];
    let reply = case.post("/api/v1/catalog/check", &bob, json!({"identities": asked}));
    assert_eq!(
        reply.json(),
        json!({"state": "ready", "reason": null, "results": [
            {"identity": asked[0], "state": "unavailable"},
            {"identity": asked[1], "state": "unavailable"},
            {"identity": asked[2], "state": "selectable"},
        ]})
    );

    let stderr = case.console.stderr();
    case.github.clean(&stderr, &sent);
}

/// Pagination is bounded and opaque: `limit` 1 to 100, a cursor that decodes to an identity, `q`
/// a prefix; anything else is a 400 with a fixed message. A catalog read and a check need a
/// token, a write needs `Origin`, and the console never asks GitHub: the fake sees no request
/// while the console answers.
#[test]
fn the_catalog_pages_with_bounded_opaque_cursors_and_calls_nobody() {
    let Some(case) = Case::new("ui_catalog_pages", 3600) else {
        return;
    };
    case.seed();
    let asked = case.github.api.with(|s| s.requests);
    let root = case.token("root");
    case.until(&root, None);
    let whole: Vec<String> = listed(&case.catalog(&root, ""));
    assert_eq!(whole.len(), 4);
    assert!(whole.windows(2).all(|w| w[0] < w[1]), "{whole:?}");

    let mut seen = Vec::new();
    let mut after = String::new();
    let mut pages = 0;
    loop {
        let answer = case.catalog(&root, &format!("?limit=3{after}"));
        pages += 1;
        seen.extend(listed(&answer));
        match answer["next"].as_str() {
            Some(next) => after = format!("&after={next}"),
            None => break,
        }
    }
    assert_eq!((pages, &seen), (2, &whole));

    let prefixed = case.catalog(&root, &format!("?q={HOST}/acme/y"));
    assert_eq!(listed(&prefixed), [case.identity("Y")]);
    assert_eq!(
        listed(&case.catalog(&root, "?q=nothing")),
        Vec::<String>::new()
    );

    let fixed = case.get("/api/v1/catalog?limit=0", &root);
    assert_eq!(fixed.status, 400);
    let message = fixed.json()["message"].clone();
    let too_long = format!("?q={}", "a".repeat(129));
    for bad in [
        "?limit=0",
        "?limit=101",
        "?limit=x",
        "?limit=1&limit=2",
        "?after=bogus",
        "?after=",
        "?other=1",
        "?limit",
        too_long.as_str(),
    ] {
        let reply = case.get(&format!("/api/v1/catalog{bad}"), &root);
        assert_eq!(reply.status, 400, "{bad}");
        assert_eq!(reply.json()["message"], message, "{bad}");
        assert!(!reply.body.contains("bogus"), "{}", reply.body);
    }
    let many: Vec<String> = (0..51).map(|i| format!("{HOST}/acme/r{i}")).collect();
    for body in [
        json!({"identities": []}),
        json!({"identities": many}),
        json!({"identities": ["not an identity"]}),
        json!({"identities": ["github.com/acme/y"], "extra": 1}),
        json!({"identities": "github.com/acme/y"}),
    ] {
        let reply = case.post("/api/v1/catalog/check", &root, body.clone());
        assert_eq!(reply.status, 400, "{body}");
    }
    let huge = json!({"identities": ["x".repeat(20 << 10)]});
    assert_eq!(case.post("/api/v1/catalog/check", &root, huge).status, 413);

    // No token: 401 before anything; no `Origin` on the POST: 403.
    let bare = send(&case.console, "GET", "/api/v1/catalog", &[]);
    assert_eq!(bare.status, 401);
    let unsigned = [("Authorization", bearer(&root))];
    let unsigned: Vec<(&str, &str)> = unsigned.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let body = json!({"identities": [case.identity("Y")]}).to_string();
    let reply = send_body(
        &case.console,
        "POST",
        "/api/v1/catalog/check",
        &unsigned,
        &body,
    );
    assert_eq!(reply.status, 403);

    // The console's `config` names the picker and its `manifest` keeps the allow list from
    // anyone but the platform administrator.
    let config = send(&case.console, "GET", "/api/v1/config", &[]);
    assert_eq!(config.json()["catalog"], true);
    let alice = case.token("alice");
    let shown =
        |token: &str| case.get("/api/v1/manifest", token).json()["manifest"]["catalog"].clone();
    assert_eq!(shown(&alice), json!({"max_repositories": 10}));
    assert_eq!(
        shown(&root),
        json!({"allow": [format!("{HOST}/acme/*")], "max_repositories": 10})
    );
    let admission = case.get("/api/v1/admission", &root).json();
    assert_eq!(admission["github"]["set"]["catalog"], 4);

    assert_eq!(
        case.github.api.with(|s| s.requests),
        asked,
        "the console called GitHub"
    );
}

/// A set past `max_age_seconds` makes the catalog pending `stale_set` for everyone, the
/// administrator included, with nothing listed and every check `pending`, never a revived older
/// grant; a new set restores it. A run that fails leaves the old set standing. The set's age is
/// shown.
#[test]
fn a_stale_set_makes_the_catalog_pending_and_a_new_set_restores_it() {
    let Some(case) = Case::new("ui_catalog_stale", 4) else {
        return;
    };
    case.seed();
    let alice = case.token("alice");
    let root = case.token("root");
    let fresh = case.until(&alice, None);
    assert_eq!(listed(&fresh), [case.identity("A"), case.identity("Y")]);
    assert_eq!(fresh["set"]["max_age_seconds"], 4);

    // A failed run (every listing 503) writes nothing: the set it leaves ages out all the same.
    case.github.api.with(|s| s.listing_status = Some(503));
    let env = [("MEADA_PERMISSIONS_POLL_MS", POLL_MS)];
    let roles = ["github-app", "roles", "--workspace", "ops/roles.toml"];
    let (_, stderr, code) = crate::postgres::meada(&case.dir, "en_US.UTF-8", &env, &roles);
    assert_eq!(code, 1, "{stderr}");
    case.github.api.with(|s| s.listing_status = None);

    for token in [&alice, &root] {
        let stale = case.until(token, Some("stale_set"));
        assert_eq!(
            (stale["state"].as_str(), listed(&stale)),
            (Some("pending"), vec![])
        );
        assert!(stale["set"]["age_seconds"].as_u64().unwrap() >= 4);
        let reply = case.post(
            "/api/v1/catalog/check",
            token,
            json!({"identities": [case.identity("Y")]}),
        );
        assert_eq!(
            reply.json(),
            json!({"state": "pending", "reason": "stale_set", "results": [
                {"identity": case.identity("Y"), "state": "pending"}
            ]})
        );
    }

    case.seed();
    let restored = case.ready(&alice);
    assert_eq!(listed(&restored), [case.identity("A"), case.identity("Y")]);
    let asked = case.get("/api/v1/session", &alice).json();
    assert_eq!(asked["github_grant"], "current");
}

/// A group, a role claim and a claims grant name no repository the catalog lets a caller select:
/// a token granting all three repositories by claims and carrying groups named as the
/// repositories' teams, with the login of someone holding a role on `Y` alone, selects `Y` only.
#[test]
fn agent_metadata_and_raw_group_names_establish_no_entitlement() {
    let Some(case) = Case::new("ui_catalog_claims", 3600) else {
        return;
    };
    case.seed();
    let mut forged = claims(&case.issuer.base, &[]);
    forged["sub"] = json!("agent");
    forged[LOGIN_CLAIM] = json!("yoda");
    forged["meada_repositories"] = json!(["*"]);
    forged["groups"] = json!(["a", "b", "group:a", "group:b", "team-web", "meada-admins"]);
    forged["operator"] = json!(true);
    forged["roles"] = json!(["admin"]);
    let token = sign(&case.signing, "catalog", Algorithm::ES256, &forged);
    let answer = case.until(&token, None);
    assert_eq!(listed(&answer), [case.identity("Y")]);
    let asked = [case.identity("A"), case.identity("B"), case.identity("Y")];
    let reply = case.post(
        "/api/v1/catalog/check",
        &token,
        json!({"identities": asked}),
    );
    let states: Vec<String> = (reply.json()["results"].as_array().unwrap().iter())
        .map(|r| r["state"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(states, ["unavailable", "unavailable", "selectable"]);
}
