//! Account administration (#557, ADR 0024): the global operator lists, creates and changes local
//! accounts through the console's API, whatever `[ui] operations` says; the last active operator
//! is never suspended nor demoted, even by two requests at once; a suspended or demoted account
//! loses its writes at once, its reads lasting until its token expires. Each test runs against
//! the local Postgres of `MEADA_TEST_POSTGRES_URL` in a schema of its own, and skips without it.

use serde_json::{Value, json};

use super::{
    ADMIN_ENV, CHOSEN, Local, ask, assert_leaks_nothing, local_with, login, post, refusal, token,
};
use crate::server::Server;
use crate::ui::{Reply, send};

/// A temporary password the policy takes.
const TEMPORARY: &str = "temporary passphrase one";
/// Another, for a reset.
const RESET: &str = "temporary passphrase two";
/// What an account sets once its change is required.
const OWN: &str = "my own long passphrase";
/// The 403 of a console write by anyone but an operator.
const OPERATOR_REQUIRED: &str = "console writes require an authenticated operator with a subject";
/// The 403 of account administration by anyone but the global operator.
const ADMINISTRATION: &str = "account administration needs the global operator";

/// The console of `case` with `operations` as given, `admin` set up with `CHOSEN` (no change
/// required), and `admin`'s session token.
fn console(case: &'static str, operations: bool) -> Option<(Local, String)> {
    let local = local_with(case, operations, &[(ADMIN_ENV, CHOSEN)])?;
    let admin = token(&login(&local.console, "admin", CHOSEN));
    Some((local, admin))
}

/// `POST accounts` creating `username` of `role` with `password`, `change_required` as given
/// (absent: the default).
fn create(
    console: &Server,
    token: &str,
    username: &str,
    role: &str,
    password: &str,
    change_required: Option<bool>,
) -> Reply {
    let mut body = json!({"username": username, "role": role, "password": password});
    if let Some(change_required) = change_required {
        body["change_required"] = change_required.into();
    }
    post(console, "accounts", Some(token), &body.to_string())
}

/// `POST accounts/<username>` with `body`.
fn act(console: &Server, token: &str, username: &str, body: Value) -> Reply {
    post(
        console,
        &format!("accounts/{username}"),
        Some(token),
        &body.to_string(),
    )
}

/// The accounts `GET accounts` lists.
fn listed(console: &Server, token: &str) -> Vec<Value> {
    let reply = ask(console, "/api/v1/accounts", token);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert!(!reply.body.contains("$argon2"), "{}", reply.body);
    reply.json()["accounts"].as_array().unwrap().clone()
}

/// The listed account `username`.
fn account(console: &Server, token: &str, username: &str) -> Value {
    let all = listed(console, token);
    (all.into_iter())
        .find(|a| a["username"] == username)
        .unwrap_or_else(|| panic!("{username} is not listed"))
}

/// A sign-in of `username` with `password` whose change is required, then the change to `OWN`:
/// the fresh token.
fn changed(console: &Server, username: &str, password: &str) -> String {
    let first = login(console, username, password);
    assert_eq!(first.json()["change_required"], true, "{}", first.body);
    let body = json!({"current": password, "new": OWN}).to_string();
    token(&post(
        console,
        "account/password",
        Some(&token(&first)),
        &body,
    ))
}

/// The audit's records of account administration (`account.*` but the sign-in and the password
/// change, and the route-level `account`), oldest first: action, subject, role, arguments,
/// outcome and message.
fn administered(console: &Server, token: &str) -> Vec<Value> {
    let page = ask(console, "/api/v1/audit?limit=100", token);
    assert_eq!(page.status, 200, "{}", page.body);
    let mut found: Vec<Value> = (page.json()["records"].as_array().unwrap().iter())
        .filter(|r| {
            let action = r["action"].as_str().unwrap();
            action == "account" || (action.starts_with("account.") && !action.ends_with("login"))
        })
        .filter(|r| r["action"] != "account.password")
        .map(|r| {
            json!([
                r["action"],
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

/// The global operator lists, creates and changes accounts: each refusal of a body (400, 413,
/// 422), a taken username (409), an unknown account (404), no route that deletes or renames,
/// `reset_password` clearing a lock and requiring a change unless told not to, and the record of
/// each change naming the account and the fields changed, never a password. Anyone else gets a
/// 403, recorded for a write.
#[test]
fn the_global_operator_lists_creates_and_changes_accounts() {
    let Some((local, admin)) = console("accounts_administration", true) else {
        return;
    };
    let console = &local.console;
    let mut replies = Vec::new();

    // The default account, as the list shows it: never a hash, no creator.
    let all = listed(console, &admin);
    assert_eq!(all.len(), 1, "{all:?}");
    let first = &all[0];
    let mut keys: Vec<&str> = first
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "change_required",
            "created",
            "created_by",
            "locked_until",
            "password_changed",
            "role",
            "state",
            "username"
        ]
    );
    assert_eq!(
        (
            &first["username"],
            &first["role"],
            &first["state"],
            &first["locked_until"],
            &first["change_required"],
            &first["created_by"]
        ),
        (
            &json!("admin"),
            &json!("operator"),
            &json!("active"),
            &Value::Null,
            &json!(false),
            &Value::Null
        )
    );
    for time in ["password_changed", "created"] {
        let text = first[time].as_str().unwrap();
        assert!(text.len() == 27 && text.ends_with('Z'), "{time}: {text}");
    }

    // Bodies that do not parse, one past the cap, and the 422s.
    for bad in [
        "",
        "{",
        r#"{"username": "carol", "role": "viewer"}"#,
        r#"{"username": "carol", "role": "root", "password": "temporary passphrase one"}"#,
        r#"{"username": "carol", "role": "viewer", "password": "temporary passphrase one", "groups": []}"#,
    ] {
        let reply = post(console, "accounts", Some(&admin), bad);
        assert_eq!(refusal(&reply), (400, "bad_request".into()), "{bad}");
        replies.push(reply);
    }
    let huge = create(
        console,
        &admin,
        "carol",
        "viewer",
        &"x".repeat(8 << 10),
        None,
    );
    assert_eq!(huge.status, 413);
    for username in ["Carol", "", "carol smith", "carol!", &"c".repeat(65)] {
        let reply = create(console, &admin, username, "viewer", TEMPORARY, None);
        assert_eq!(
            refusal(&reply),
            (422, "invalid_username".into()),
            "{username}"
        );
    }
    let short = create(console, &admin, "carol", "viewer", "too short", None);
    assert_eq!(refusal(&short), (422, "password_policy".into()));
    assert_eq!(short.json()["error"]["rule"], "too_short");
    let named = "carol.example.org";
    let same = create(console, &admin, named, "viewer", "Carol.Example.Org", None);
    assert_eq!(same.json()["error"]["rule"], "username", "{}", same.body);
    replies.extend([huge, short, same]);

    // Created, a change required by default; a taken username is a 409.
    let created = create(console, &admin, "carol", "viewer", TEMPORARY, None);
    assert_eq!(created.status, 200, "{}", created.body);
    let carol = &created.json()["account"];
    assert_eq!(
        (
            &carol["username"],
            &carol["role"],
            &carol["state"],
            &carol["change_required"],
            &carol["created_by"]
        ),
        (
            &json!("carol"),
            &json!("viewer"),
            &json!("active"),
            &json!(true),
            &json!("local:admin")
        )
    );
    let taken = create(console, &admin, "carol", "operator", RESET, None);
    assert_eq!(refusal(&taken), (409, "username_taken".into()));
    let dave = create(console, &admin, "dave", "operator", TEMPORARY, Some(false));
    assert_eq!(
        dave.json()["account"]["change_required"],
        false,
        "{}",
        dave.body
    );
    let names: Vec<Value> = listed(console, &admin)
        .iter()
        .map(|a| a["username"].clone())
        .collect();
    assert_eq!(names, [json!("admin"), json!("carol"), json!("dave")]);
    replies.extend([created, taken, dave]);

    // An unknown account is a 404; no action, method or route deletes or renames one.
    let unknown = act(console, &admin, "nobody", json!({"action": "suspend"}));
    assert_eq!(refusal(&unknown), (404, "not_found".into()));
    let malformed = act(console, &admin, "No%20body", json!({"action": "suspend"}));
    assert_eq!(refusal(&malformed), (404, "not_found".into()));
    for body in [
        json!({"action": "delete"}),
        json!({"action": "rename", "username": "caroline"}),
        json!({"action": "suspend", "username": "caroline"}),
        json!({"action": "groups", "groups": ["team-billing"]}),
        json!({"role": "operator"}),
    ] {
        let reply = act(console, &admin, "carol", body.clone());
        assert_eq!(refusal(&reply), (400, "bad_request".into()), "{body}");
    }
    let auth = format!("Bearer {admin}");
    let headers = [("Origin", super::super::PUBLIC), ("Authorization", &auth)];
    for (method, path) in [
        ("DELETE", "/api/v1/accounts/carol"),
        ("PUT", "/api/v1/accounts/carol"),
        ("PATCH", "/api/v1/accounts/carol"),
        ("POST", "/api/v1/accounts/carol/rename"),
    ] {
        let reply = send(console, method, path, &headers);
        assert_eq!(reply.status, 404, "{method} {path}: {}", reply.body);
    }
    assert_eq!(account(console, &admin, "carol")["username"], "carol");

    // The role, both ways.
    let promoted = act(
        console,
        &admin,
        "carol",
        json!({"action": "role", "role": "operator"}),
    );
    assert_eq!(
        promoted.json()["account"]["role"],
        "operator",
        "{}",
        promoted.body
    );
    let demoted = act(
        console,
        &admin,
        "carol",
        json!({"action": "role", "role": "viewer"}),
    );
    assert_eq!(
        demoted.json()["account"]["role"],
        "viewer",
        "{}",
        demoted.body
    );

    // A lock, listed with its end, cleared by a reset that requires a change by default.
    for i in 0..10 {
        let reply = login(console, "carol", &format!("wrong guess {i}"));
        assert_eq!(refusal(&reply).0, 401);
    }
    let locked = account(console, &admin, "carol");
    assert_eq!(locked["state"], "locked", "{locked}");
    assert!(
        locked["locked_until"].as_str().unwrap().ends_with('Z'),
        "{locked}"
    );
    assert_eq!(refusal(&login(console, "carol", TEMPORARY)).0, 401);
    let weak = act(
        console,
        &admin,
        "carol",
        json!({"action": "reset_password", "password": "too short"}),
    );
    assert_eq!(weak.json()["error"]["rule"], "too_short", "{}", weak.body);
    let reset = act(
        console,
        &admin,
        "carol",
        json!({"action": "reset_password", "password": RESET}),
    );
    let after = &reset.json()["account"];
    assert_eq!(
        (
            &after["state"],
            &after["locked_until"],
            &after["change_required"]
        ),
        (&json!("active"), &Value::Null, &json!(true)),
        "{}",
        reset.body
    );
    let carol_token = changed(console, "carol", RESET);
    let kept = act(
        console,
        &admin,
        "dave",
        json!({"action": "reset_password", "password": RESET, "change_required": false}),
    );
    assert_eq!(
        kept.json()["account"]["change_required"],
        false,
        "{}",
        kept.body
    );
    assert_eq!(
        login(console, "dave", RESET).json()["change_required"],
        false
    );
    replies.extend([weak, reset, kept]);

    // Suspended and reactivated.
    let suspended = act(console, &admin, "dave", json!({"action": "suspend"}));
    assert_eq!(
        suspended.json()["account"]["state"],
        "suspended",
        "{}",
        suspended.body
    );
    let back = act(console, &admin, "dave", json!({"action": "reactivate"}));
    assert_eq!(back.json()["account"]["state"], "active", "{}", back.body);

    // A viewer administers nothing: its reads a 403, its writes a 403 recorded.
    let list = ask(console, "/api/v1/accounts", &carol_token);
    assert_eq!(refusal(&list), (403, "forbidden".into()));
    assert_eq!(list.json()["error"]["message"], ADMINISTRATION);
    let refused_create = create(console, &carol_token, "eve", "operator", TEMPORARY, None);
    assert_eq!(refusal(&refused_create), (403, "forbidden".into()));
    let refused_change = act(console, &carol_token, "dave", json!({"action": "suspend"}));
    assert_eq!(refusal(&refused_change), (403, "forbidden".into()));
    replies.extend([list, refused_create, refused_change]);
    assert_eq!(account(console, &admin, "dave")["state"], "active");
    assert!(
        !listed(console, &admin)
            .iter()
            .any(|a| a["username"] == "eve"),
        "a refused creation created"
    );

    let admin_subject = json!("local:admin");
    let carol_subject = json!("local:carol");
    let row =
        |action: &str,
         subject: &Value,
         role: &str,
         arguments: Value,
         outcome: &str,
         message: &str| { json!([action, subject, role, arguments, outcome, message]) };
    assert_eq!(
        administered(console, &admin),
        [
            row(
                "account.create",
                &admin_subject,
                "operator",
                json!({"account": "carol", "role": "viewer", "change_required": "true"}),
                "accepted",
                "account created"
            ),
            row(
                "account.create",
                &admin_subject,
                "operator",
                json!({"account": "carol", "role": "operator", "change_required": "true"}),
                "refused",
                "another account has that username"
            ),
            row(
                "account.create",
                &admin_subject,
                "operator",
                json!({"account": "dave", "role": "operator", "change_required": "false"}),
                "accepted",
                "account created"
            ),
            row(
                "account.suspend",
                &admin_subject,
                "operator",
                json!({"account": "nobody"}),
                "refused",
                "no account has that username"
            ),
            row(
                "account.role",
                &admin_subject,
                "operator",
                json!({"account": "carol", "role": "operator"}),
                "accepted",
                "role changed"
            ),
            row(
                "account.role",
                &admin_subject,
                "operator",
                json!({"account": "carol", "role": "viewer"}),
                "accepted",
                "role changed"
            ),
            row(
                "account.reset_password",
                &admin_subject,
                "operator",
                json!({"account": "carol", "change_required": "true"}),
                "accepted",
                "password reset"
            ),
            row(
                "account.reset_password",
                &admin_subject,
                "operator",
                json!({"account": "dave", "change_required": "false"}),
                "accepted",
                "password reset"
            ),
            row(
                "account.suspend",
                &admin_subject,
                "operator",
                json!({"account": "dave"}),
                "accepted",
                "account suspended"
            ),
            row(
                "account.reactivate",
                &admin_subject,
                "operator",
                json!({"account": "dave"}),
                "accepted",
                "account reactivated"
            ),
            row(
                "account.create",
                &carol_subject,
                "viewer",
                json!({}),
                "refused",
                ADMINISTRATION
            ),
            row(
                "account",
                &carol_subject,
                "viewer",
                json!({"account": "dave"}),
                "refused",
                ADMINISTRATION
            ),
        ]
    );

    for reply in &replies {
        reply.assert_no_cookie_or_cors();
    }
    let passwords = [
        CHOSEN,
        TEMPORARY,
        RESET,
        OWN,
        "too short",
        "Carol.Example.Org",
    ];
    let tokens = [admin.as_str(), carol_token.as_str()];
    for reply in &replies {
        assert_leaks_nothing(&local.audit, &local.keys, &reply.body, &passwords, &tokens);
    }
    let rows = local.audit.admin.texts(&format!(
        "SELECT row_to_json(a)::text FROM \"{}\".audit a",
        local.audit.schema
    ));
    assert_leaks_nothing(
        &local.audit,
        &local.keys,
        &rows.join("\n"),
        &passwords,
        &tokens,
    );
    let Local {
        console,
        audit,
        keys,
        ..
    } = local;
    let stderr = console.stderr();
    for line in [
        "ui: local:admin account.create carol: accepted",
        "ui: local:admin account.create carol: refused (username_taken)",
        "ui: local:admin account.suspend nobody: refused (not_found)",
        "ui: local:admin account.reset_password carol: accepted",
        "ui: local:carol account dave: account administration needs the global operator",
    ] {
        assert!(stderr.contains(line), "{line} not in {stderr}");
    }
    assert_leaks_nothing(&audit, &keys, &stderr, &passwords, &tokens);
}

/// The demo: a viewer and an operator created by the global operator sign in; suspended, the
/// viewer's next write gets the role refusal and its next sign-in the one 401 (recorded
/// `suspended`), while its token still reads; suspended, then demoted, the operator's writes get
/// the viewer's refusal at once, though its token still says operator. Reactivated, it writes
/// again; its row made hostile, it writes as a viewer, with one line naming the table.
#[test]
fn a_suspended_or_demoted_account_loses_its_writes_at_once_and_keeps_its_reads() {
    let Some((local, admin)) = console("accounts_standing", true) else {
        return;
    };
    let console = &local.console;
    let pointer = json!({"action": "set", "generation": "0".repeat(32)}).to_string();
    // The generation is no id (16 hex digits): an operator's pointer write passes its role check
    // and is the 400 of its body, which is read only then; a refused role is the 403 before it.
    let written = |token: &str| refusal(&post(console, "pointer", Some(token), &pointer));
    let refused = (403, "forbidden".to_string());
    let admitted = (400, "bad_request".to_string());

    let created = create(console, &admin, "vera", "viewer", TEMPORARY, None);
    assert_eq!(created.status, 200, "{}", created.body);
    let vera = changed(console, "vera", TEMPORARY);
    assert_eq!(written(&vera), refused);
    let created = create(console, &admin, "otto", "operator", TEMPORARY, Some(false));
    assert_eq!(created.status, 200, "{}", created.body);
    let otto = token(&login(console, "otto", TEMPORARY));
    assert_eq!(written(&otto), admitted);

    // The viewer suspended: its write refused, its reads kept, its sign-in the one 401.
    let suspended = act(console, &admin, "vera", json!({"action": "suspend"}));
    assert_eq!(suspended.status, 200, "{}", suspended.body);
    assert_eq!(written(&vera), refused);
    assert_eq!(ask(console, "/api/v1/session", &vera).status, 200);
    let wrong = login(console, "vera", "not her passphrase");
    let signin = login(console, "vera", OWN);
    assert_eq!(refusal(&signin), (401, "invalid_credentials".into()));
    assert_eq!(signin.body, wrong.body);

    // The operator suspended: every write refused at once, reads kept.
    let suspended = act(console, &admin, "otto", json!({"action": "suspend"}));
    assert_eq!(suspended.status, 200, "{}", suspended.body);
    let write = post(console, "pointer", Some(&otto), &pointer);
    assert_eq!(refusal(&write), refused);
    assert_eq!(write.json()["error"]["message"], OPERATOR_REQUIRED);
    let administer = create(console, &otto, "mallory", "operator", TEMPORARY, None);
    assert_eq!(refusal(&administer), refused);
    let session = ask(console, "/api/v1/session", &otto);
    assert_eq!(session.json()["role"], "operator", "{}", session.body);
    // Reactivated, it writes again; demoted, its writes are a viewer's.
    let back = act(console, &admin, "otto", json!({"action": "reactivate"}));
    assert_eq!(back.status, 200, "{}", back.body);
    assert_eq!(written(&otto), admitted);
    let demoted = act(
        console,
        &admin,
        "otto",
        json!({"action": "role", "role": "viewer"}),
    );
    assert_eq!(demoted.status, 200, "{}", demoted.body);
    assert_eq!(written(&otto), refused);
    let administer = act(console, &otto, "vera", json!({"action": "reactivate"}));
    assert_eq!(refusal(&administer), refused);
    assert_eq!(
        ask(console, "/api/v1/session", &otto).json()["role"],
        "operator"
    );
    let fresh = token(&login(console, "otto", TEMPORARY));
    assert_eq!(
        ask(console, "/api/v1/session", &fresh).json()["role"],
        "viewer"
    );

    // The records: the suspended sign-in's reason, and the refused writes under the role the
    // row gave, not the token's.
    let page = ask(console, "/api/v1/audit?limit=100", &admin).json();
    let records = page["records"].as_array().unwrap();
    let reasons: Vec<Value> = (records.iter())
        .filter(|r| r["action"] == "account.login" && r["arguments"]["account"] == "vera")
        .map(|r| r["arguments"]["reason"].clone())
        .collect();
    assert_eq!(reasons[..2], [json!("suspended"), json!("suspended")]);
    let role_refused =
        |r: &&Value| r["message"] == OPERATOR_REQUIRED || r["message"] == ADMINISTRATION;
    let otto_refused: Vec<(&Value, &Value)> = (records.iter())
        .filter(|r| r["subject"] == "local:otto" && r["outcome"] == "refused")
        .filter(role_refused)
        .map(|r| (&r["action"], &r["role"]))
        .collect();
    assert_eq!(otto_refused.len(), 4, "{otto_refused:?}");
    assert!(
        otto_refused.iter().all(|(_, role)| *role == "viewer"),
        "{otto_refused:?}"
    );

    // A row that fails its checks admits nothing: promoted again, then its row made hostile, the
    // operator's token writes as a viewer, with one line naming the table and no value.
    let promoted = act(
        console,
        &admin,
        "otto",
        json!({"action": "role", "role": "operator"}),
    );
    assert_eq!(promoted.status, 200, "{}", promoted.body);
    assert_eq!(written(&otto), admitted);
    local.audit.admin.run(&format!(
        "UPDATE \"{}\".accounts SET role = 'root' WHERE username = 'otto'",
        local.audit.schema
    ));
    assert_eq!(written(&otto), refused);
    let Local { console, audit, .. } = local;
    let stderr = console.stderr();
    let table = format!("\"{}\".accounts", audit.schema);
    let bad = format!("ui: accounts: a row of {table} fails its checks and admits nothing\n");
    assert_eq!(stderr.matches(&bad).count(), 1, "{stderr}");
    assert!(!stderr.contains("'root'"), "{stderr}");
}

/// The last active operator is neither suspended nor demoted: alone (beside a hostile operator
/// row, which admits nothing), by itself, a 409; two
/// operators each removing themselves at once, each request admitted as an active operator,
/// one passes and the other is the 409 `last_operator`, round after round, so exactly one
/// active operator is ever left.
#[test]
fn the_last_active_operator_stays_even_against_two_requests_at_once() {
    let Some((local, admin)) = console("accounts_last_operator", true) else {
        return;
    };
    let console = &local.console;
    let last = (409, "last_operator".to_string());
    // A hostile operator row (no argon2id hash) admits nothing, so it is no other operator.
    local.audit.admin.run(&format!(
        "INSERT INTO \"{}\".accounts (username, password_hash, role, change_required) \
         VALUES ('ghost', 'not a hash', 'operator', false)",
        local.audit.schema
    ));
    let alone = act(console, &admin, "admin", json!({"action": "suspend"}));
    assert_eq!(refusal(&alone), last);
    let alone = act(
        console,
        &admin,
        "admin",
        json!({"action": "role", "role": "viewer"}),
    );
    assert_eq!(refusal(&alone), last);
    let created = create(console, &admin, "olga", "operator", TEMPORARY, Some(false));
    assert_eq!(created.status, 200, "{}", created.body);
    let olga = token(&login(console, "olga", TEMPORARY));
    let active = |token: &str| -> Vec<String> {
        (listed(console, token).iter())
            .filter(|a| a["role"] == "operator" && a["state"] != "suspended")
            .map(|a| a["username"].as_str().unwrap().to_string())
            .collect()
    };
    for round in 0..5 {
        let (demote, suspend) = std::thread::scope(|scope| {
            let demote = scope.spawn(|| {
                act(
                    console,
                    &admin,
                    "admin",
                    json!({"action": "role", "role": "viewer"}),
                )
            });
            let suspend = scope.spawn(|| act(console, &olga, "olga", json!({"action": "suspend"})));
            (demote.join().unwrap(), suspend.join().unwrap())
        });
        let codes = [refusal(&demote), refusal(&suspend)];
        let passed = [demote.status, suspend.status];
        assert!(
            passed.contains(&200) && codes.contains(&last),
            "round {round}: {codes:?} {passed:?}"
        );
        // One active operator is left, and puts the other back.
        let left = match demote.status {
            200 => &olga,
            _ => &admin,
        };
        let remaining = active(left);
        assert_eq!(remaining.len(), 1, "round {round}: {remaining:?}");
        let repaired = match demote.status {
            200 => act(
                console,
                &olga,
                "admin",
                json!({"action": "role", "role": "operator"}),
            ),
            _ => act(console, &admin, "olga", json!({"action": "reactivate"})),
        };
        assert_eq!(repaired.status, 200, "round {round}: {}", repaired.body);
        assert_eq!(active(left).len(), 2, "round {round}");
    }
    let page = ask(console, "/api/v1/audit?limit=100", &admin).json();
    let refusals = (page["records"].as_array().unwrap().iter())
        .filter(|r| r["message"] == "this would leave no active operator account")
        .count();
    assert_eq!(refusals, 7);
}

/// Account administration answers with `operations = false`; the workspace and generation
/// writes keep requiring it.
#[test]
fn account_administration_works_with_operations_off() {
    let Some((local, admin)) = console("accounts_operations_off", false) else {
        return;
    };
    let console = &local.console;
    let created = create(console, &admin, "vera", "viewer", TEMPORARY, None);
    assert_eq!(created.status, 200, "{}", created.body);
    let suspended = act(console, &admin, "vera", json!({"action": "suspend"}));
    assert_eq!(suspended.status, 200, "{}", suspended.body);
    assert_eq!(listed(console, &admin).len(), 2);
    let disabled = "console writes are disabled by `ui.operations`";
    for (route, body) in [
        (
            "pointer",
            json!({"action": "set", "generation": "0".repeat(32)}),
        ),
        ("workspace/versions", json!({"entries": "", "message": "m"})),
        ("workspace/apply", json!({"version": "0".repeat(32)})),
        ("fence/recover", json!({})),
    ] {
        let reply = post(console, route, Some(&admin), &body.to_string());
        assert_eq!(refusal(&reply), (403, "forbidden".into()), "{route}");
        assert_eq!(reply.json()["error"]["message"], disabled, "{route}");
    }
}

/// A change whose record cannot be written changes nothing: a 503 `audit_unavailable`; with
/// Postgres gone, a write cannot read its account and is a 503 too.
#[test]
fn an_account_change_without_its_record_changes_nothing() {
    let Some((local, admin)) = console("accounts_administration_audit", true) else {
        return;
    };
    let console = &local.console;
    let created = create(console, &admin, "vera", "viewer", TEMPORARY, None);
    assert_eq!(created.status, 200, "{}", created.body);
    let schema = &local.audit.schema;
    local.audit.admin.run(&format!(
        "ALTER TABLE \"{schema}\".audit RENAME TO audit_gone"
    ));
    let refused = create(console, &admin, "otto", "operator", TEMPORARY, None);
    assert_eq!(refusal(&refused), (503, "audit_unavailable".into()));
    let refused = act(console, &admin, "vera", json!({"action": "suspend"}));
    assert_eq!(refusal(&refused), (503, "audit_unavailable".into()));
    local.audit.admin.run(&format!(
        "ALTER TABLE \"{schema}\".audit_gone RENAME TO audit"
    ));
    let all = listed(console, &admin);
    let names: Vec<&Value> = all.iter().map(|a| &a["username"]).collect();
    assert_eq!(names, [&json!("admin"), &json!("vera")]);
    assert_eq!(account(console, &admin, "vera")["state"], "active");
    local.audit.relay.close();
    let refused = act(console, &admin, "vera", json!({"action": "suspend"}));
    assert_eq!(refusal(&refused), (503, "unavailable".into()));
    let listing = ask(console, "/api/v1/accounts", &admin);
    assert_eq!(refusal(&listing), (503, "unavailable".into()));
    let Local {
        console,
        audit,
        keys,
        ..
    } = local;
    let stderr = console.stderr();
    assert_leaks_nothing(&audit, &keys, &stderr, &[CHOSEN, TEMPORARY], &[&admin]);
}
