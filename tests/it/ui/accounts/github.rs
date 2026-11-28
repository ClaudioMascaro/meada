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

const HOST: &str = "example.com";
const PASSWORD: &str = "a long github passphrase";
const ANA_PAT: &str = "ghp_ana0123456789abcdef561";
const BIA_PAT: &str = "ghp_bia0123456789abcdef561";
const DUPLICATE_PAT: &str = "ghp_dup0123456789abcdef561";
const UNKNOWN_PAT: &str = "ghp_nobody0123456789abc561";

fn get(console: &Server, route: &str, token: &str) -> Value {
    let reply = ask(console, &format!("/api/v1/{route}"), token);
    assert_eq!(reply.status, 200, "{route}: {}", reply.body);
    reply.json()
}

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

#[test]
fn a_verified_login_grants_its_github_roles_on_the_console_and_a_node() {
    let case = "accounts_github_login";
    let Some(github) = Github::new(case) else {
        return;
    };
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
    assert!(get(&console, "session", &unverified).get("login").is_none());
    let admission = get(&console, "admission", &ana);
    assert_eq!(admission["accounts"]["mode"], "accounts");
    assert_eq!(admission["github"]["host"], HOST, "{admission}");
    assert_eq!(admission["github"]["grant"], "current", "{admission}");
    assert_eq!(admission["github"]["set"]["rows"], 3, "{admission}");

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

    let removed = post(&console, "account/github/remove", Some(&ana), "");
    assert_eq!(removed.status, 200, "{}", removed.body);
    assert_eq!(removed.json()["login"], Value::Null);
    let ana_after = removed.json()["token"].as_str().unwrap().to_string();
    let session = current(&console, &ana_after);
    assert!(session.get("login").is_none(), "{session}");
    assert_eq!(session["grant"], json!([]));
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
    let again = verify(&console, &bia, DUPLICATE_PAT);
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.json()["login"], "ana-souza");

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
