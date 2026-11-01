//! Agent tokens (#560, ADR 0024): a signed-in person issues, through `POST account/tokens`, an
//! agent token for the nodes, shown once; a node holding the console's public key admits it and
//! answers under the grant its groups map to; the console refuses it. Each test runs against the
//! local Postgres of `MEADA_TEST_POSTGRES_URL` in a schema of its own, and skips without it.

use std::fs;

use serde_json::{Value, json};

use super::administration::act;
use super::{
    ADMIN_ENV, CHOSEN, Local, PUBLIC, VERSION, ask, assert_leaks_nothing, console_dir, keys, login,
    post, refusal, section, start, token,
};
use crate::manifest::{MANIFEST, chain_manifest, run};
use crate::oidc::{ask as ask_node, trace, traced, web_only};
use crate::postgres::audited;
use crate::server::Server;
use crate::ui::ui_section;

const PASSWORD: &str = "a long agent passphrase";
const WEB: &str = "example.com/chain/web";
const CATALOG: &str = "example.com/chain/catalog";
/// `[accounts.groups]` of the console and of the node: `web` reads the web alone.
const TABLE: &str = "\n[accounts.groups]\nweb = [\"example.com/chain/web\"]\n";
const REFUSED: &str = "this account may not issue agent tokens: it is suspended or unknown";

/// The console of `case` with `operations`, `[accounts]` and `TABLE`, `admin` set up with
/// `CHOSEN`, and `admin`'s session token.
fn console(case: &'static str) -> Option<(Local, String)> {
    let ui = ui_section() + "operations = true\n";
    let dir = console_dir(case, Some(VERSION), "", &ui);
    let audit = audited(&dir, case)?;
    let keys = keys(&dir);
    let manifest = dir.join(MANIFEST);
    let text = fs::read_to_string(&manifest).unwrap();
    let text = format!("{text}{}{}{TABLE}", audit.section, section());
    fs::write(&manifest, text).unwrap();
    let console = start(&dir, &[(ADMIN_ENV, CHOSEN)]);
    let admin = token(&login(&console, "admin", CHOSEN));
    let local = Local {
        dir,
        audit,
        console,
        keys,
    };
    Some((local, admin))
}

/// `POST account/tokens` with `body`.
fn issue(console: &Server, token: &str, body: Value) -> crate::ui::Reply {
    post(console, "account/tokens", Some(token), &body.to_string())
}

/// The claims of `token`, read without a check.
fn claims(token: &str) -> Value {
    jsonwebtoken::dangerous::insecure_decode_claims::<Value>(token).unwrap()
}

/// The records of `account.token` the audit page lists, oldest first: subject, role, arguments,
/// outcome and message.
fn issued(console: &Server, token: &str) -> Vec<Value> {
    let page = ask(console, "/api/v1/audit?limit=100", token);
    assert_eq!(page.status, 200, "{}", page.body);
    let mut found: Vec<Value> = (page.json()["records"].as_array().unwrap().iter())
        .filter(|r| r["action"] == "account.token")
        .map(|r| {
            json!([
                r["subject"],
                r["role"],
                r["arguments"],
                r["outcome"],
                r["message"]
            ])
        })
        .collect();
    found.reverse();
    found
}

/// The demo: a viewer of the group `web` issues an agent token for the nodes, valid 24 hours,
/// answered once as `{token, expires_at, surface}`; the token carries the nodes' audience, the
/// subject, the role, the groups and a random id; the console answers it with 401, and a node
/// holding the console's public key reads `WEB` with it and answers 403 on the catalog. Eight
/// hours is the default; 0 or 169 hours, another surface, an unknown field, a body that does not
/// parse and one past the bound are refused. A change-required account gets the 403
/// `password_change_required`; a suspended one the role refusal with its current session token.
/// Each issue and the refusal are recorded `account.token`, with the surface, the hours and the
/// token id, never the token; no output holds a token, a password or a key.
#[test]
fn an_agent_token_is_issued_once_and_a_node_answers_under_its_grant() {
    let Some((local, admin)) = console("accounts_agent_tokens") else {
        return;
    };
    let console = &local.console;
    let create = |username: &str, change_required: bool| {
        let body = json!({"username": username, "role": "viewer", "groups": ["web"],
                          "password": PASSWORD, "change_required": change_required});
        let reply = post(console, "accounts", Some(&admin), &body.to_string());
        assert_eq!(reply.status, 200, "{}", reply.body);
    };
    create("ana", false);
    let ana = token(&login(console, "ana", PASSWORD));

    let reply = issue(console, &ana, json!({"surface": "nodes", "hours": 24}));
    assert_eq!(reply.status, 200, "{}", reply.body);
    let answer = reply.json();
    let mut keys: Vec<&String> = answer.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["expires_at", "surface", "token"]);
    assert_eq!(answer["surface"], "nodes");
    let agent = answer["token"].as_str().unwrap().to_string();
    let signed = claims(&agent);
    let id = signed["jti"].as_str().unwrap().to_string();
    assert!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "{id}"
    );
    assert_eq!(
        (
            signed["iss"].as_str(),
            signed["aud"].as_str(),
            signed["sub"].as_str()
        ),
        (Some(PUBLIC), Some("meada:nodes"), Some("local:ana"))
    );
    assert_eq!(
        (&signed["role"], &signed["groups"]),
        (&json!("viewer"), &json!(["web"]))
    );
    assert!(signed.get("change_required").is_none(), "{signed}");
    let lifetime = signed["exp"].as_u64().unwrap() - signed["iat"].as_u64().unwrap();
    assert_eq!(lifetime, 24 * 3600);
    let expires = answer["expires_at"].as_str().unwrap();
    assert!(expires.ends_with('Z') && expires.contains('T'), "{expires}");

    // The default validity, and a second token with its own id.
    let default = token(&issue(console, &ana, json!({"surface": "nodes"})));
    let signed_default = claims(&default);
    let lifetime =
        signed_default["exp"].as_u64().unwrap() - signed_default["iat"].as_u64().unwrap();
    assert_eq!(lifetime, 8 * 3600);
    let default_id = signed_default["jti"].as_str().unwrap().to_string();
    assert_ne!(default_id, id);

    // The console refuses an agent token, whatever the route.
    for route in ["/api/v1/session", "/api/v1/manifest"] {
        let reply = ask(console, route, &agent);
        assert_eq!(refusal(&reply), (401, "unauthorized".into()), "{route}");
    }
    let reply = issue(console, &agent, json!({"surface": "nodes"}));
    assert_eq!(refusal(&reply), (401, "unauthorized".into()));

    // A node with the console's public key and issuer reads `WEB` alone.
    let dir = chain_manifest("accounts_agent_tokens_node");
    assert!(run(&dir, "index").2);
    crate::oidc::accounts::node_section(&dir, MANIFEST, &local.keys.1, PUBLIC, TABLE);
    let node = Server::start_args(
        &dir,
        &["--listen", "127.0.0.1:0", "--workspace", MANIFEST],
        &[],
    );
    let at = |identity: &str| format!("{}/{identity}", node.url);
    web_only(&traced(&ask_node(&at(WEB), Some(&agent), &[], trace())));
    let other = ask_node(&at(CATALOG), Some(&agent), &[], trace());
    assert_eq!((other.status, other.body.as_str()), (403, "forbidden\n"));
    let listing = ask_node(&format!("{}/", node.url), Some(&agent), &[], None);
    assert_eq!((listing.status, listing.body), (404, format!("{WEB}\n")));
    // A session token is refused by the node.
    assert_eq!(ask_node(&at(WEB), Some(&ana), &[], trace()).status, 401);

    // Each body refusal, not recorded.
    for (body, expected) in [
        (
            json!({"surface": "nodes", "hours": 0}).to_string(),
            (422, "invalid_hours"),
        ),
        (
            json!({"surface": "nodes", "hours": 169}).to_string(),
            (422, "invalid_hours"),
        ),
        (
            json!({"surface": "proposals"}).to_string(),
            (422, "invalid_surface"),
        ),
        (
            json!({"surface": "nodes", "token": "x"}).to_string(),
            (400, "bad_request"),
        ),
        (json!({"hours": 8}).to_string(), (400, "bad_request")),
        ("not json".to_string(), (400, "bad_request")),
        (
            format!("{{\"surface\": \"{}\"}}", "n".repeat(2048)),
            (413, "too_large"),
        ),
    ] {
        let reply = post(console, "account/tokens", Some(&ana), &body);
        assert_eq!(
            refusal(&reply),
            (expected.0, expected.1.to_string()),
            "{body}"
        );
    }

    // A change-required account: the 403 of admission.
    create("bea", true);
    let bea = token(&login(console, "bea", PASSWORD));
    let reply = issue(console, &bea, json!({"surface": "nodes"}));
    assert_eq!(refusal(&reply), (403, "password_change_required".into()));

    // A suspended account, its session token still valid: the role refusal, recorded.
    let suspended = act(console, &admin, "ana", json!({"action": "suspend"}));
    assert_eq!(suspended.status, 200, "{}", suspended.body);
    let reply = issue(console, &ana, json!({"surface": "nodes"}));
    assert_eq!(refusal(&reply), (403, "forbidden".into()));
    assert_eq!(reply.json()["error"]["message"], REFUSED);

    // The operator's own token carries its role.
    let operator = token(&issue(
        console,
        &admin,
        json!({"surface": "nodes", "hours": 1}),
    ));
    assert_eq!(claims(&operator)["role"], "operator");
    let operator_id = claims(&operator)["jti"].as_str().unwrap().to_string();

    let viewer = |hours: &str, id: &str| {
        json!(["local:ana", "viewer",
               {"account": "ana", "surface": "nodes", "hours": hours, "token_id": id},
               "accepted", "agent token issued"])
    };
    assert_eq!(
        issued(console, &admin),
        [
            viewer("24", &id),
            viewer("8", &default_id),
            json!(["local:ana", "viewer", {"account": "ana", "surface": "nodes", "hours": "8"},
                   "refused", REFUSED]),
            json!(["local:admin", "operator",
                   {"account": "admin", "surface": "nodes", "hours": "1", "token_id": operator_id},
                   "accepted", "agent token issued"]),
        ]
    );

    let tokens = [
        agent.as_str(),
        default.as_str(),
        operator.as_str(),
        ana.as_str(),
        bea.as_str(),
    ];
    let passwords = [PASSWORD, CHOSEN];
    let page = ask(console, "/api/v1/audit?limit=100", &admin);
    assert_leaks_nothing(&local.audit, &local.keys, &page.body, &passwords, &tokens);
    let node_stderr = node.stderr();
    assert_leaks_nothing(&local.audit, &local.keys, &node_stderr, &passwords, &tokens);
    let Local {
        console,
        audit,
        keys,
        ..
    } = local;
    let stderr = console.stderr();
    assert!(stderr.contains("account.token"), "{stderr}");
    assert_leaks_nothing(&audit, &keys, &stderr, &passwords, &tokens);
}
