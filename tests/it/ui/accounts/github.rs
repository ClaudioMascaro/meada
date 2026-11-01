//! The verified login (#561, ADR 0024): with `[accounts.github]`, a person pastes a GitHub
//! personal access token, the console reads their login from `GET /user` on `github_app`'s fake
//! GitHub API once, keeps the login and drops the token; session and agent tokens then carry it,
//! and the GitHub grant joins the groups grant on the console and on a node. The permissions set
//! is seeded by `meada github-app roles` reading `[accounts.github]`, through the local Valkey and
//! Postgres (`oidc::github::Github`); each test skips without them. What a token reads is seen
//! through the console's routes and the node's answers, never the module's tables.

use std::fs;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::administration::{act, listed};
use super::{
    ADMIN_ENV, CHOSEN, ask, assert_leaks_nothing, keys, login, post, refusal, section, start, token,
};
use crate::manifest::{MANIFEST, run};
use crate::oidc::github::{Github, POLL_MS};
use crate::oidc::{ask as ask_node, trace, traced, web_only};
use crate::postgres::audited;
use crate::server::Server;
use crate::ui::playground::{CATALOG, GATEWAY, WEB, chain_console};
use crate::ui::ui_section;

/// The chain's git host, `[accounts.github] host` here.
const HOST: &str = "example.com";
const PASSWORD: &str = "a long github passphrase";
/// The personal access tokens the fake GitHub knows, and the login each belongs to.
const ANA_PAT: &str = "ghp_ana0123456789abcdef561";
const BIA_PAT: &str = "ghp_bia0123456789abcdef561";
/// Another token of ana's login, which bia pastes.
const DUPLICATE_PAT: &str = "ghp_dup0123456789abcdef561";
/// A token the fake GitHub refuses.
const UNKNOWN_PAT: &str = "ghp_nobody0123456789abc561";

fn get(console: &Server, route: &str, token: &str) -> Value {
    let reply = ask(console, &format!("/api/v1/{route}"), token);
    assert_eq!(reply.status, 200, "{route}: {}", reply.body);
    reply.json()
}

/// `token`'s session, once the GitHub grant's state is `current`.
fn current(console: &Server, token: &str) -> Value {
    let start = Instant::now();
    loop {
        let session = get(console, "session", token);
        if session["github_grant"] == "current" {
            return session;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{session}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The identities of the manifest view's `[[repository]]` entries.
fn entries(console: &Server, token: &str) -> Vec<String> {
    let manifest = get(console, "manifest", token);
    (manifest["manifest"]["repository"]
        .as_array()
        .unwrap()
        .iter())
    .map(|e| e["repo"].as_str().unwrap().to_string())
    .collect()
}

fn verify(console: &Server, token: &str, pat: &str) -> crate::ui::Reply {
    post(
        console,
        "account/github",
        Some(token),
        &json!({"token": pat}).to_string(),
    )
}

/// The demo: ana pastes a personal access token, the console reads her login from the fake
/// GitHub once and answers it with a fresh session token carrying it; her session then names the
/// login, the host and the GitHub grant's state, and her grant is what her login's roles read
/// (`write` on the web; `read` on the gateway is outside `roles`), bounding the manifest view;
/// the admission view shows the GitHub grant provider's state; a fresh agent token carries the
/// login and a node answers it on the web and refuses the catalog. A token GitHub refuses is a
/// 422, GitHub failing a 503, neither echoing its body; another account's login is a 409
/// `login_taken`; the body checks are 400, 413 and 422. ana removes her own login and the global
/// operator removes bia's; the list shows each account's login. Every verification and removal is
/// recorded, with the login and never the personal access token, which reaches the fake GitHub
/// in `GET /user` alone and appears in no answer, record or stderr line.
#[test]
fn a_verified_login_grants_its_github_roles_on_the_console_and_a_node() {
    let case = "accounts_github_login";
    let Some(github) = Github::new(case) else {
        return;
    };
    // No `operations`: account administration, agent tokens and the verified login need none,
    // and `operations = true` before `[accounts]` is appended would refuse the index.
    let dir = chain_console(case, "", &ui_section());
    assert!(run(&dir, "index").2, "index");
    let Some(audit) = audited(&dir, case) else {
        return;
    };
    let pems = keys(&dir);
    let provider = format!(
        "\n[accounts.github]\nhost = \"{HOST}\"\napi = \"{}\"\nroles = [\"write\"]\n\
         max_age_seconds = 5400\n",
        github.api.base
    );
    let file = dir.join(MANIFEST);
    let console_text = fs::read_to_string(&file).unwrap() + &audit.section + &section() + &provider;
    fs::write(&file, &console_text).unwrap();
    // The role check reads `[accounts.github]` as it reads `[oidc.github]`.
    let job = console_text.replacen("../indexes", "../roles-indexes", 1) + &github.job(HOST);
    fs::write(dir.join("ops/roles.toml"), job).unwrap();
    github.list("chain/web", &[("Ana-Souza", "write")]);
    github.list("chain/gateway", &[("ana-souza", "read")]);
    github.list("chain/catalog", &[("bia_acme", "write")]);
    github.list("chain/worker", &[]);
    let line = github.seed(&dir, "ops/roles.toml", &audit.relay.url);
    assert!(
        line.ends_with(": 4 repositories, 3 rows, 0 exclusions\n"),
        "{line}"
    );
    github.api.with(|state| {
        state.users.insert(ANA_PAT.into(), "Ana-Souza".into());
        state.users.insert(BIA_PAT.into(), "bia_acme".into());
        state.users.insert(DUPLICATE_PAT.into(), "ana-souza".into());
    });

    let env = [(ADMIN_ENV, CHOSEN), ("MEADA_PERMISSIONS_POLL_MS", POLL_MS)];
    let console = start(&dir, &env);
    let admin = token(&login(&console, "admin", CHOSEN));
    for username in ["ana", "bia"] {
        let body = json!({"username": username, "role": "viewer", "password": PASSWORD,
                          "change_required": false});
        let reply = post(&console, "accounts", Some(&admin), &body.to_string());
        assert_eq!(reply.status, 200, "{}", reply.body);
        assert_eq!(reply.json()["account"]["github_login"], Value::Null);
    }
    let unverified = token(&login(&console, "ana", PASSWORD));
    // Before the verification: the GitHub grant provider's state, no login, nothing granted.
    assert_eq!(
        current(&console, &unverified),
        json!({"subject": "local:ana", "role": "viewer", "groups": [], "grant": [],
               "github_host": HOST, "github_grant": "current", "account": true})
    );
    assert!(entries(&console, &unverified).is_empty());

    let verified = verify(&console, &unverified, ANA_PAT);
    assert_eq!(verified.status, 200, "{}", verified.body);
    let answer = verified.json();
    let mut fields: Vec<&String> = answer.as_object().unwrap().keys().collect();
    fields.sort();
    assert_eq!(fields, ["change_required", "expires_at", "login", "token"]);
    assert_eq!(
        (&answer["login"], &answer["change_required"]),
        (&json!("ana-souza"), &json!(false))
    );
    let ana = answer["token"].as_str().unwrap().to_string();
    let claims = jsonwebtoken::dangerous::insecure_decode_claims::<Value>(&ana).unwrap();
    assert_eq!(claims["login"], "ana-souza");
    assert_eq!(
        current(&console, &ana),
        json!({"subject": "local:ana", "role": "viewer", "groups": [], "grant": [WEB],
               "login": "ana-souza", "github_host": HOST, "github_grant": "current",
               "account": true})
    );
    assert_eq!(entries(&console, &ana), [WEB]);
    let repositories = ask(&console, "/api/v1/repositories", &ana);
    assert_eq!(repositories.status, 200, "{}", repositories.body);
    assert!(repositories.body.contains(WEB), "{}", repositories.body);
    for hidden in [GATEWAY, CATALOG] {
        assert!(!repositories.body.contains(hidden), "{}", repositories.body);
    }
    // The token issued before the verification reads as it did until it expires.
    assert!(get(&console, "session", &unverified).get("login").is_none());
    // The admission view: the GitHub grant provider's state, as an identity provider's shows it.
    let admission = get(&console, "admission", &ana);
    assert_eq!(admission["accounts"]["mode"], "accounts");
    assert_eq!(admission["github"]["host"], HOST, "{admission}");
    assert_eq!(admission["github"]["grant"], "current", "{admission}");
    assert_eq!(admission["github"]["set"]["rows"], 3, "{admission}");

    // An agent token carries the login, and a node answers it under the GitHub grant.
    let issued = post(
        &console,
        "account/tokens",
        Some(&ana),
        &json!({"surface": "nodes"}).to_string(),
    );
    let agent = token(&issued);
    let claims = jsonwebtoken::dangerous::insecure_decode_claims::<Value>(&agent).unwrap();
    assert_eq!(claims["login"], "ana-souza");
    let node = Server::start_args(
        &dir,
        &["--listen", "127.0.0.1:0", "--workspace", MANIFEST],
        &[("MEADA_PERMISSIONS_POLL_MS", POLL_MS)],
    );
    let at = |identity: &str| format!("{}/{identity}", node.url);
    let start_at = Instant::now();
    while ask_node(&format!("{}/", node.url), Some(&agent), &[], None).body != format!("{WEB}\n") {
        assert!(
            start_at.elapsed() < Duration::from_secs(20),
            "the node's set"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    web_only(&traced(&ask_node(&at(WEB), Some(&agent), &[], trace())));
    let other = ask_node(&at(CATALOG), Some(&agent), &[], trace());
    assert_eq!((other.status, other.body.as_str()), (403, "forbidden\n"));

    // Refusals: GitHub's refusal, GitHub failing, another account's login, the body checks.
    let bia_unverified = token(&login(&console, "bia", PASSWORD));
    let refused = verify(&console, &bia_unverified, UNKNOWN_PAT);
    assert_eq!(refusal(&refused), (422, "github_refused".into()));
    assert!(
        !refused.body.contains("Bad credentials"),
        "{}",
        refused.body
    );
    github.api.with(|state| state.user_status = Some(502));
    let failing = verify(&console, &bia_unverified, BIA_PAT);
    assert_eq!(refusal(&failing), (503, "github_unavailable".into()));
    assert!(!failing.body.contains("fake status"), "{}", failing.body);
    github.api.with(|state| state.user_status = None);
    let taken = verify(&console, &bia_unverified, DUPLICATE_PAT);
    assert_eq!(refusal(&taken), (409, "login_taken".into()));
    for (body, expected) in [
        ("{}".to_string(), (400, "bad_request")),
        (
            json!({"token": ANA_PAT, "login": "x"}).to_string(),
            (400, "bad_request"),
        ),
        ("not json".to_string(), (400, "bad_request")),
        (
            json!({"token": "ghp has spaces"}).to_string(),
            (422, "invalid_token"),
        ),
        (json!({"token": ""}).to_string(), (422, "invalid_token")),
        (
            json!({"token": "g".repeat(2048)}).to_string(),
            (413, "too_large"),
        ),
    ] {
        let reply = post(&console, "account/github", Some(&bia_unverified), &body);
        assert_eq!(
            refusal(&reply),
            (expected.0, expected.1.to_string()),
            "{body}"
        );
    }
    let bia = token(&verify(&console, &bia_unverified, BIA_PAT));
    assert_eq!(current(&console, &bia)["grant"], json!([CATALOG]));
    let logins: Vec<(Value, Value)> = (listed(&console, &admin).iter())
        .map(|a| (a["username"].clone(), a["github_login"].clone()))
        .collect();
    assert_eq!(
        logins,
        [
            (json!("admin"), Value::Null),
            (json!("ana"), json!("ana-souza")),
            (json!("bia"), json!("bia_acme")),
        ]
    );

    // ana removes her own login: a fresh token without it, reading nothing again.
    let removed = post(&console, "account/github/remove", Some(&ana), "");
    assert_eq!(removed.status, 200, "{}", removed.body);
    assert_eq!(removed.json()["login"], Value::Null);
    let ana_after = removed.json()["token"].as_str().unwrap().to_string();
    let session = current(&console, &ana_after);
    assert!(session.get("login").is_none(), "{session}");
    assert_eq!(session["grant"], json!([]));
    // The global operator removes bia's; a viewer may not remove another's.
    let own = act(
        &console,
        &ana_after,
        "bia",
        json!({"action": "remove_github"}),
    );
    assert_eq!(refusal(&own), (403, "forbidden".into()));
    let removed = act(&console, &admin, "bia", json!({"action": "remove_github"}));
    assert_eq!(removed.status, 200, "{}", removed.body);
    assert_eq!(removed.json()["account"]["github_login"], Value::Null);
    // The login freed, another account may verify it.
    let again = verify(&console, &bia, DUPLICATE_PAT);
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.json()["login"], "ana-souza");

    // The records: each verification and removal, the actor's login in the `login` column.
    let page = get(&console, "audit?limit=100", &admin);
    let mut found: Vec<Value> = (page["records"].as_array().unwrap().iter())
        .filter(|r| {
            let action = r["action"].as_str().unwrap();
            action == "account.github" || action == "account.remove_github"
        })
        .map(|r| {
            json!([
                r["action"],
                r["subject"],
                r["login"],
                r["arguments"],
                r["outcome"],
                r["message"]
            ])
        })
        .collect();
    found.reverse();
    let ana_login = json!({"account": "ana", "login": "ana-souza"});
    assert_eq!(
        found,
        [
            json!([
                "account.github",
                "local:ana",
                null,
                ana_login,
                "accepted",
                "GitHub login verified"
            ]),
            json!(["account.github", "local:bia", null,
                   {"account": "bia", "reason": "github_refused"}, "refused",
                   "GitHub refused this personal access token"]),
            json!(["account.github", "local:bia", null,
                   {"account": "bia", "reason": "github_unavailable"}, "failed",
                   "GitHub could not be reached; try again later"]),
            json!(["account.github", "local:bia", null,
                   {"account": "bia", "login": "ana-souza"}, "refused",
                   "another account already verified this GitHub login"]),
            json!(["account.github", "local:bia", null,
                   {"account": "bia", "login": "bia_acme"}, "accepted",
                   "GitHub login verified"]),
            json!(["account.remove_github", "local:ana", "ana-souza", {"account": "ana"},
                   "accepted", "GitHub login removed"]),
            json!(["account.remove_github", "local:admin", null, {"account": "bia"},
                   "accepted", "GitHub login removed"]),
            json!(["account.github", "local:bia", "bia_acme",
                   {"account": "bia", "login": "ana-souza"}, "accepted",
                   "GitHub login verified"]),
        ]
    );

    // The personal access tokens reached the fake GitHub in `GET /user` alone, once per attempt.
    let pats = [ANA_PAT, BIA_PAT, DUPLICATE_PAT, UNKNOWN_PAT];
    let sent: Vec<(String, String)> = github.api.with(|state| {
        (state.bearers.iter())
            .filter(|(_, bearer)| bearer.starts_with("ghp_"))
            .cloned()
            .collect()
    });
    let user = |pat: &str| ("/user".to_string(), pat.to_string());
    assert_eq!(
        sent,
        [
            user(ANA_PAT),
            user(UNKNOWN_PAT),
            user(BIA_PAT),
            user(DUPLICATE_PAT),
            user(BIA_PAT),
            user(DUPLICATE_PAT),
        ]
    );
    let records = page.to_string();
    let tokens = [
        admin.as_str(),
        unverified.as_str(),
        ana.as_str(),
        ana_after.as_str(),
        agent.as_str(),
        bia.as_str(),
        bia_unverified.as_str(),
    ];
    let mut secrets = vec![PASSWORD, CHOSEN];
    secrets.extend(pats);
    let node_stderr = node.stderr();
    let stderr = console.stderr();
    assert!(stderr.contains("account.github"), "{stderr}");
    for text in [&records, &stderr, &node_stderr] {
        assert_leaks_nothing(&audit, &pems, text, &secrets, &tokens);
    }
    for reply in [&refused, &failing, &taken] {
        for pat in pats {
            assert!(!reply.body.contains(pat), "{}", reply.body);
        }
    }
}
