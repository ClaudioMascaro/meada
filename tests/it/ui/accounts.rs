//! The console with `[accounts]` (#554, ADR 0024): its own sign-in, the default account, session
//! tokens it signs with Ed25519, the forced password change and the audit of each. The section
//! and key refusals need no service; the rest run against the local Postgres of
//! `MEADA_TEST_POSTGRES_URL`, each test in a schema of its own, and skip without it; the save as
//! the global operator also needs the fleet's Valkey and store.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{PUBLIC, Reply, VERSION, console_dir, get, send, send_body, ui_section};
use crate::manifest::{MANIFEST, chain_manifest, run, run_args};
use crate::postgres::{Audited, audited, meada};
use crate::server::Server;

const EN: &str = "en_US.UTF-8";
/// The variable every test here names for the initial password, so the runner's own
/// `MEADA_ADMIN_PASSWORD` never reaches a console.
const ADMIN_ENV: &str = "MEADA_TEST_ADMIN_PASSWORD_554";
const CHANGED: &str = "correct horse battery staple";

/// A fresh Ed25519 key pair in PEM, generated with ring: the private half (PKCS #8) and the
/// public half (SPKI).
fn pair() -> (String, String) {
    use ring::signature::KeyPair;
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let mut spki = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    spki.extend_from_slice(pair.public_key().as_ref());
    (pem("PRIVATE KEY", pkcs8.as_ref()), pem("PUBLIC KEY", &spki))
}

fn pem(label: &str, der: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in der.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            text.push(match i <= chunk.len() {
                true => ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char,
                false => '=',
            });
        }
    }
    format!("-----BEGIN {label}-----\n{text}\n-----END {label}-----\n")
}

/// `ops/public.pem` and `ops/private.pem` under `dir`, a matching pair; the PEM texts.
fn keys(dir: &Path) -> (String, String) {
    let (private, public) = pair();
    fs::write(dir.join("ops/public.pem"), &public).unwrap();
    fs::write(dir.join("ops/private.pem"), &private).unwrap();
    (private, public)
}

/// The `[accounts]` over `keys`' files, the initial password read from `ADMIN_ENV`.
fn section() -> String {
    format!(
        "\n[accounts]\npublic_key_file = \"public.pem\"\nprivate_key_file = \"private.pem\"\n\
         admin_password_env = \"{ADMIN_ENV}\"\n"
    )
}

/// `meada ui` on `dir`'s manifest, refused: its stderr.
fn refused(dir: &Path, env: &[(&str, &str)]) -> String {
    let args = ["ui", "--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    let (stdout, stderr, code) = meada(dir, EN, env, &args);
    assert_eq!((stdout.as_str(), code), ("", 1), "{stderr}");
    stderr
}

/// Each section rule names its sections or its key: `[accounts]` needs `[postgres]`, never sits
/// beside `[oidc]`, takes one key of each pair; `[ui] operations = true` needs no `[oidc]` beside
/// it, and no operator key.
#[test]
fn the_section_s_rules_name_their_sections_and_keys() {
    let ui = ui_section() + "operations = true\n";
    let postgres = "\n[postgres]\nurl_env = \"MEADA_TEST_NO_POSTGRES_554\"\n";
    let oidc = "\n[oidc]\nissuer = \"https://id.example\"\npublic_url = \"https://meada.example\"\n\
                identities_claim = \"repositories\"\n";
    let both = "\n[accounts]\npublic_key_env = \"A\"\npublic_key_file = \"public.pem\"\n";
    for (case, head, ui, expected) in [
        (
            "accounts_no_postgres",
            section(),
            ui.clone(),
            "[accounts] needs [postgres]",
        ),
        (
            "accounts_beside_oidc",
            format!("{postgres}{oidc}{}", section()),
            ui_section() + "client_id = \"meada-console\"\n",
            "[accounts] cannot sit beside [oidc]",
        ),
        (
            "accounts_both_keys",
            format!("{postgres}{both}"),
            ui.clone(),
            "`accounts.public_key_file`: is set together with public_key_env",
        ),
    ] {
        let dir = console_dir(case, Some(VERSION), &head, &ui);
        let (_, stderr, code) = run_args(&dir, &["status", "--workspace", MANIFEST]);
        assert_eq!(code, Some(1), "{case}: {stderr}");
        assert!(stderr.contains(expected), "{case}: {stderr}");
        // `operations = true` passed: no other problem is named.
        assert_eq!(stderr.lines().count(), 1, "{case}: {stderr}");
    }
}

/// The console refuses to start on a missing, unreadable, non-Ed25519 or mismatched key pair,
/// naming the key and never what it holds; with a good pair and no Postgres, on the default
/// account it cannot create, naming Postgres. `operations = true` passes with `[accounts]` alone.
#[test]
fn the_console_refuses_a_missing_unreadable_foreign_or_mismatched_key() {
    let head = format!(
        "\n[postgres]\nurl_env = \"MEADA_TEST_POSTGRES_URL_554\"\nconnect_timeout_seconds = 1\n{}",
        section()
    );
    let ui = ui_section() + "operations = true\n";
    let dir = console_dir("accounts_keys", Some(VERSION), &head, &ui);
    let (private, public) = keys(&dir);
    let (other_private, other_public) = pair();
    let write = |public: &str, private: &str| {
        fs::write(dir.join("ops/public.pem"), public).unwrap();
        fs::write(dir.join("ops/private.pem"), private).unwrap();
    };
    let closed = "postgres://meada:pw-554@127.0.0.1:9/meada";
    let env = [("MEADA_TEST_POSTGRES_URL_554", closed)];
    for (public_text, private_text, key, expected) in [
        (
            other_public.as_str(),
            private.as_str(),
            "accounts.private_key_file",
            "does not match the public key",
        ),
        (
            private.as_str(),
            private.as_str(),
            "accounts.public_key_file",
            "is not an Ed25519 public key in PEM",
        ),
        (
            public.as_str(),
            "-----BEGIN PRIVATE KEY-----\nbm90IGEga2V5\n-----END PRIVATE KEY-----\n",
            "accounts.private_key_file",
            "is not an Ed25519 private key in PEM",
        ),
    ] {
        write(public_text, private_text);
        let stderr = refused(&dir, &env);
        assert!(stderr.contains(&format!("`{key}`: {expected}")), "{stderr}");
        for text in [&private, &public, &other_private, &other_public] {
            for line in text.lines().filter(|l| !l.starts_with("-----")) {
                assert!(!stderr.contains(line), "a key in {stderr}");
            }
        }
    }
    // Unreadable: the file named, not a value.
    fs::remove_file(dir.join("ops/public.pem")).unwrap();
    let stderr = refused(&dir, &env);
    assert!(stderr.contains("`accounts.public_key_file`: "), "{stderr}");
    // Missing: no key named at all.
    let missing = "\n[postgres]\nurl_env = \"MEADA_TEST_POSTGRES_URL_554\"\n\n[accounts]\n\
                   public_key_file = \"public.pem\"\n";
    let bare = console_dir("accounts_keys_missing", Some(VERSION), missing, &ui);
    keys(&bare);
    let stderr = refused(&bare, &env);
    assert!(
        stderr.contains(
            "names no private key of the signing key pair: set \
                         `accounts.private_key_env` or `accounts.private_key_file`"
        ),
        "{stderr}"
    );
    // A good pair: the default account needs Postgres at start, named and never its URL.
    write(&public, &private);
    let stderr = refused(&dir, &env);
    assert!(
        stderr.contains("`postgres`: the default account cannot be created: "),
        "{stderr}"
    );
    assert!(
        !stderr.contains("pw-554") && !stderr.contains(closed),
        "{stderr}"
    );
}

/// A node parses `[accounts]` and reads no key: one naming a private key file that does not exist
/// starts and answers.
#[test]
fn a_node_never_reads_the_private_key() {
    let dir = chain_manifest("accounts_node");
    let (_, stderr, ok) = run(&dir, "index");
    assert!(ok, "{stderr}");
    let manifest = dir.join(MANIFEST);
    let mut text = fs::read_to_string(&manifest).unwrap();
    text += "\n[postgres]\nurl_env = \"MEADA_TEST_NO_POSTGRES_554\"\n\n[accounts]\n\
             public_key_file = \"no-such-public.pem\"\nprivate_key_file = \"no-such-private.pem\"\n";
    fs::write(&manifest, text).unwrap();
    let node = Server::start_args(
        &dir,
        &["--listen", "127.0.0.1:0", "--workspace", MANIFEST],
        &[],
    );
    let (status, _, _) = crate::server::get(&node.url);
    assert_ne!(status, 0);
    let stderr = node.stderr();
    assert!(!stderr.contains("private"), "{stderr}");
}

/// A console with `[accounts]` over a Postgres schema of `case`'s own, running.
struct Local {
    dir: PathBuf,
    audit: Audited,
    console: Server,
    /// The key pair's PEM texts, which no output may carry.
    keys: (String, String),
}

/// The console of `case` with `operations` and `[accounts]`, started with `env`.
fn local(case: &'static str, env: &[(&str, &str)]) -> Option<Local> {
    let ui = ui_section() + "operations = true\n";
    let dir = console_dir(case, Some(VERSION), "", &ui);
    let audit = audited(&dir, case)?;
    let keys = keys(&dir);
    let manifest = dir.join(MANIFEST);
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, format!("{text}{}{}", audit.section, section())).unwrap();
    let console = start(&dir, env);
    Some(Local {
        dir,
        audit,
        console,
        keys,
    })
}

fn start(dir: &Path, env: &[(&str, &str)]) -> Server {
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    Server::start_command(dir, "ui", &args, env)
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn post(console: &Server, route: &str, token: Option<&str>, body: &str) -> Reply {
    let auth = token.map(bearer);
    let mut headers = vec![("Origin", PUBLIC)];
    headers.extend(auth.as_deref().map(|a| ("Authorization", a)));
    send_body(console, "POST", &format!("/api/v1/{route}"), &headers, body)
}

fn login(console: &Server, username: &str, password: &str) -> Reply {
    let body = json!({"username": username, "password": password}).to_string();
    post(console, "login", None, &body)
}

fn change(console: &Server, token: &str, current: &str, new: &str) -> Reply {
    let body = json!({"current": current, "new": new}).to_string();
    post(console, "account/password", Some(token), &body)
}

fn ask(console: &Server, path: &str, token: &str) -> Reply {
    send(console, "GET", path, &[("Authorization", &bearer(token))])
}

/// The token of a sign-in that passed.
fn token(reply: &Reply) -> String {
    assert_eq!(reply.status, 200, "{}", reply.body);
    reply.json()["token"].as_str().unwrap().to_string()
}

/// The fixed error of `reply`: its status and code.
fn refusal(reply: &Reply) -> (u16, String) {
    let code = reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    (reply.status, code)
}

/// The audit's records as `GET audit` answers them, oldest first, without their `id` and `time`.
fn records(console: &Server, token: &str) -> Vec<Value> {
    let page = ask(console, "/api/v1/audit", token);
    assert_eq!(page.status, 200, "{}", page.body);
    let mut records: Vec<Value> = page.json()["records"].as_array().unwrap().clone();
    records.reverse();
    for record in &mut records {
        let object = record.as_object_mut().unwrap();
        object.remove("id");
        object.remove("time");
    }
    records
}

fn record(
    subject: Option<&str>,
    role: &str,
    action: &str,
    arguments: Value,
    outcome: &str,
) -> Value {
    let message = match (action, outcome) {
        ("account.login", "accepted") => "signed in",
        ("account.password", "accepted") => "password changed",
        ("pointer", _) => "console writes require an authenticated operator with a subject",
        _ => "the username or the password is wrong",
    };
    json!({"via": "console", "subject": subject, "login": null, "role": role, "owners": null,
           "action": action, "arguments": arguments, "outcome": outcome, "message": message})
}

/// Nothing secret in `text`: no password of `passwords`, no part of a token of `tokens`, no
/// argon2 hash, no key and no Postgres URL.
fn assert_leaks_nothing(
    audit: &Audited,
    keys: &(String, String),
    text: &str,
    passwords: &[&str],
    tokens: &[&str],
) {
    for password in passwords {
        assert!(!text.contains(password), "a password in {text}");
    }
    for token in tokens {
        for part in token.split('.') {
            assert!(!text.contains(part), "a token part in {text}");
        }
    }
    assert!(!text.contains("$argon2"), "a hash in {text}");
    for key in [&keys.0, &keys.1] {
        for line in key.lines().filter(|l| !l.starts_with("-----")) {
            assert!(!text.contains(line), "a key in {text}");
        }
    }
    audit.assert_no_url(text);
}

/// The tracer bullet: the default account signs in with `admin123`, its token opens only the
/// password change; after the change, a fresh token is the global operator's session, and a
/// viewer seeded beside it reads and cannot write. Every sign-in, refusal and change leaves a
/// record the audit page lists, and no answer, record or stderr line holds a password, a hash or
/// a token; no answer sets a cookie or a CORS header.
#[test]
fn the_default_account_signs_in_changes_its_password_and_is_the_global_operator() {
    let Some(local) = local("accounts_flow", &[]) else {
        return;
    };
    let console = &local.console;
    let mut replies = Vec::new();

    let config = get(console, "/api/v1/config");
    assert_eq!(config.status, 200);
    assert_eq!(config.json()["sign_in"], "accounts");
    let missing = get(console, "/api/v1/session");
    assert_eq!(refusal(&missing), (401, "unauthorized".into()));
    assert_eq!(missing.header("www-authenticate"), Some("Bearer"));
    assert_eq!(get(console, "/").status, 200);

    // The `Origin` checks come first: a sign-in is a write.
    let body = json!({"username": "admin", "password": "admin123"}).to_string();
    let bare = send_body(console, "POST", "/api/v1/login", &[], &body);
    assert_eq!(bare.status, 403);
    let foreign = [("Origin", "https://evil.example")];
    let cross = send_body(console, "POST", "/api/v1/login", &foreign, &body);
    assert_eq!(cross.status, 403);
    // A body that does not parse, and one past the cap.
    for bad in [
        "",
        "{",
        "{\"username\": \"admin\"}",
        "{\"username\": 1, \"password\": \"x\"}",
    ] {
        let reply = post(console, "login", None, bad);
        assert_eq!(refusal(&reply), (400, "bad_request".into()), "{bad}");
    }
    let huge = json!({"username": "admin", "password": "x".repeat(8 << 10)}).to_string();
    assert_eq!(post(console, "login", None, &huge).status, 413);

    // The default account: its token opens the password change alone.
    let first = login(console, "admin", "admin123");
    let changing = token(&first);
    let answer = first.json();
    assert_eq!(answer["change_required"], true);
    assert!(answer["expires_at"].as_str().unwrap().ends_with('Z'));
    for route in ["session", "manifest", "audit"] {
        let reply = ask(console, &format!("/api/v1/{route}"), &changing);
        assert_eq!(
            refusal(&reply),
            (403, "password_change_required".into()),
            "{route}"
        );
        replies.push(reply);
    }
    // A write needs the token before anything else: none is a 401, one awaiting its change a 403.
    let pointer = json!({"action": "set", "generation": "0".repeat(32)}).to_string();
    let tokenless = post(console, "pointer", None, &pointer);
    assert_eq!(refusal(&tokenless), (401, "unauthorized".into()));
    let early = post(console, "pointer", Some(&changing), &pointer);
    assert_eq!(refusal(&early), (403, "password_change_required".into()));
    let huge = json!({"current": "admin123", "new": "x".repeat(8 << 10)}).to_string();
    let reply = post(console, "account/password", Some(&changing), &huge);
    assert_eq!(reply.status, 413);
    // Unknown username and wrong password alike; a tampered token is a 401 before routing.
    let unknown = login(console, "nobody", "admin123");
    let wrong = login(console, "admin", "admin124");
    assert_eq!(refusal(&unknown), (401, "invalid_credentials".into()));
    assert_eq!(unknown.body, wrong.body);
    let tampered = format!("{changing}x");
    let reply = ask(console, "/api/v1/session", &tampered);
    assert_eq!(refusal(&reply), (401, "unauthorized".into()));
    assert_eq!(
        reply.header("www-authenticate"),
        Some("Bearer error=\"invalid_token\"")
    );
    // Session claims signed by the test: with the console's key they pass, so another key's and
    // an expired one's 401s come from the key and the lifetime alone.
    let signed = |private: &str, exp: u64| {
        let claims = json!({"iss": PUBLIC, "aud": "meada:console", "sub": "local:admin",
                            "role": "viewer", "groups": [], "iat": exp - 3600, "exp": exp});
        let key = jsonwebtoken::EncodingKey::from_ed_pem(private.as_bytes()).unwrap();
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
        jsonwebtoken::encode(&header, &claims, &key).unwrap()
    };
    let now = jsonwebtoken::get_current_timestamp();
    let own = signed(&local.keys.0, now + 3600);
    let reply = ask(console, "/api/v1/session", &own);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.json()["subject"], "local:admin");
    let minted = [
        ("another key", signed(&pair().0, now + 3600)),
        ("expired", signed(&local.keys.0, now - 3600)),
    ];
    for (why, token) in &minted {
        let reply = ask(console, "/api/v1/session", token);
        assert_eq!(refusal(&reply), (401, "unauthorized".into()), "{why}");
        assert_eq!(
            reply.header("www-authenticate"),
            Some("Bearer error=\"invalid_token\""),
            "{why}"
        );
        replies.push(reply);
    }

    // The change: the policy first, then the current password.
    let short = change(console, &changing, "admin123", "too short");
    assert_eq!(refusal(&short), (422, "password_policy".into()));
    assert_eq!(short.json()["error"]["rule"], "too_short");
    let bad_current = change(console, &changing, "admin124", CHANGED);
    assert_eq!(refusal(&bad_current), (401, "invalid_credentials".into()));
    let changed = change(console, &changing, "admin123", CHANGED);
    let operator = token(&changed);
    assert_eq!(changed.json()["change_required"], false);
    let session = ask(console, "/api/v1/session", &operator);
    assert_eq!(session.status, 200, "{}", session.body);
    assert_eq!(
        session.json(),
        json!({"subject": "local:admin", "role": "operator", "groups": [], "grant": "*",
               "account": true})
    );
    // The token issued before the change keeps its mark until it expires.
    let reply = ask(console, "/api/v1/session", &changing);
    assert_eq!(refusal(&reply), (403, "password_change_required".into()));
    assert_eq!(refusal(&login(console, "admin", "admin123")).0, 401);
    let again = login(console, "admin", CHANGED);
    assert_eq!(again.json()["change_required"], false);
    let operator_again = token(&again);

    // A viewer, seeded through the same store with the operator's hash: it reads and cannot
    // write, and the policy refuses its current password as the new one.
    local.audit.admin.run(&format!(
        "INSERT INTO \"{0}\".accounts (username, password_hash, role, change_required) SELECT \
         'viewer-1', password_hash, 'viewer', false FROM \"{0}\".accounts WHERE username = 'admin'",
        local.audit.schema
    ));
    let viewer = token(&login(console, "viewer-1", CHANGED));
    let session = ask(console, "/api/v1/session", &viewer);
    assert_eq!(
        session.json(),
        json!({"subject": "local:viewer-1", "role": "viewer", "groups": [], "grant": "*",
               "account": true})
    );
    assert_eq!(ask(console, "/api/v1/manifest", &viewer).status, 200);
    assert_eq!(
        refusal(&ask(console, "/api/v1/audit", &viewer)),
        (403, "forbidden".into())
    );
    let write = post(console, "pointer", Some(&viewer), &pointer);
    assert_eq!(refusal(&write), (403, "forbidden".into()));
    let reused = change(console, &viewer, CHANGED, CHANGED);
    assert_eq!(reused.json()["error"]["rule"], "current_password");

    let all = records(console, &operator);
    let admin = Some("local:admin");
    let viewer_subject = Some("local:viewer-1");
    assert_eq!(
        all,
        [
            record(
                admin,
                "operator",
                "account.login",
                json!({"account": "admin"}),
                "accepted"
            ),
            record(
                None,
                "none",
                "account.login",
                json!({"reason": "unknown"}),
                "refused"
            ),
            record(
                admin,
                "operator",
                "account.login",
                json!({"account": "admin", "reason": "password"}),
                "refused"
            ),
            record(
                admin,
                "operator",
                "account.password",
                json!({"account": "admin", "reason": "password"}),
                "refused"
            ),
            record(
                admin,
                "operator",
                "account.password",
                json!({"account": "admin"}),
                "accepted"
            ),
            record(
                admin,
                "operator",
                "account.login",
                json!({"account": "admin", "reason": "password"}),
                "refused"
            ),
            record(
                admin,
                "operator",
                "account.login",
                json!({"account": "admin"}),
                "accepted"
            ),
            record(
                viewer_subject,
                "viewer",
                "account.login",
                json!({"account": "viewer-1"}),
                "accepted"
            ),
            record(viewer_subject, "viewer", "pointer", json!({}), "refused"),
        ]
    );

    // A hostile row admits nothing: one line naming the table, the one 401, no value anywhere.
    local.audit.admin.run(&format!(
        "INSERT INTO \"{0}\".accounts (username, password_hash, role, change_required) SELECT \
         'root-1', password_hash, 'root', false FROM \"{0}\".accounts WHERE username = 'admin'",
        local.audit.schema
    ));
    let hostile = login(console, "root-1", CHANGED);
    assert_eq!(hostile.body, unknown.body);

    replies.extend([
        config, missing, first, unknown, wrong, short, changed, session, write,
    ]);
    replies.extend([tokenless, early, hostile]);
    for reply in &replies {
        reply.assert_no_cookie_or_cors();
    }
    let passwords = ["admin123", "admin124", CHANGED, "too short"];
    let mut tokens = vec![
        changing.as_str(),
        operator.as_str(),
        operator_again.as_str(),
        viewer.as_str(),
        own.as_str(),
    ];
    tokens.extend(minted.iter().map(|(_, token)| token.as_str()));
    for reply in &replies {
        if reply.status != 200 {
            assert_leaks_nothing(&local.audit, &local.keys, &reply.body, &passwords, &tokens);
        }
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
    assert!(
        stderr.contains("ui: local:admin account.login admin: accepted"),
        "{stderr}"
    );
    assert!(
        stderr.contains("ui: - account.login -: refused (unknown)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("ui: local:admin account.password admin: accepted"),
        "{stderr}"
    );
    let table = format!("\"{}\".accounts", audit.schema);
    let bad = format!("ui: accounts: a row of {table} fails its checks and admits nothing\n");
    assert_eq!(stderr.matches(&bad).count(), 1, "{stderr}");
    assert!(!stderr.contains("'root'"), "{stderr}");
    assert_leaks_nothing(&audit, &keys, &stderr, &passwords, &tokens);
}

/// The default account is created once, by consoles started together, from the deployer's
/// password when one is set (no change required then) and only when it passes the policy, which
/// otherwise refuses the start naming the key and never the value. A table that holds an account
/// is never touched again.
#[test]
fn the_default_account_is_created_once_from_the_setting_and_never_again() {
    let Some(local) = local(
        "accounts_bootstrap",
        &[(ADMIN_ENV, "deployer chose this one")],
    ) else {
        return;
    };
    let console = &local.console;
    let first = login(console, "admin", "deployer chose this one");
    assert_eq!(first.json()["change_required"], false, "{}", first.body);
    assert_eq!(refusal(&login(console, "admin", "admin123")).0, 401);
    let Local {
        dir,
        audit,
        console,
        ..
    } = local;
    drop(console);

    // A table that holds an account is never touched: a new setting changes nothing.
    let restarted = start(&dir, &[(ADMIN_ENV, "another one entirely")]);
    assert_eq!(
        login(&restarted, "admin", "deployer chose this one").status,
        200
    );
    assert_eq!(
        login(&restarted, "admin", "another one entirely").status,
        401
    );
    drop(restarted);

    // On an empty table, a setting that fails the policy refuses the start, naming its key.
    audit
        .admin
        .run(&format!("DELETE FROM \"{}\".accounts", audit.schema));
    let stderr = refused(&dir, &[(ADMIN_ENV, "admin")]);
    assert!(
        stderr.contains("`accounts.admin_password_env`: the password has fewer than 12 characters"),
        "{stderr}"
    );
    assert!(!stderr.contains("\"admin\""), "{stderr}");
    let long = "x".repeat(129);
    let stderr = refused(&dir, &[(ADMIN_ENV, &long)]);
    assert!(
        stderr.contains("more than 128 characters") && !stderr.contains(&long),
        "{stderr}"
    );

    // Two consoles started together on an empty table: both start, one account, `admin123`
    // with a change required.
    let together: Vec<Server> = std::thread::scope(|scope| {
        let starts: Vec<_> = (0..2).map(|_| scope.spawn(|| start(&dir, &[]))).collect();
        starts.into_iter().map(|s| s.join().unwrap()).collect()
    });
    let count = audit.admin.texts(&format!(
        "SELECT count(*) FROM \"{}\".accounts",
        audit.schema
    ));
    assert_eq!(count, ["1"]);
    for console in &together {
        let reply = login(console, "admin", "admin123");
        assert_eq!(reply.json()["change_required"], true, "{}", reply.body);
    }
}

/// A sign-in whose audit record cannot be written gives no token: a 503 `audit_unavailable`;
/// with Postgres gone, so does every sign-in, and no hash is computed past `HASHES_MAX` at once.
#[test]
fn no_token_without_its_record_and_no_hash_past_the_bound() {
    let Some(local) = local("accounts_audit", &[]) else {
        return;
    };
    let console = &local.console;
    // Past the bound: a burst of sign-ins, each a hash, gets some 503 `busy`, the rest the 401.
    let burst: Vec<Reply> = std::thread::scope(|scope| {
        let sent: Vec<_> = (0..24)
            .map(|_| scope.spawn(|| login(console, "nobody", "admin123")))
            .collect();
        sent.into_iter().map(|s| s.join().unwrap()).collect()
    });
    let codes: Vec<(u16, String)> = burst.iter().map(refusal).collect();
    assert!(codes.contains(&(503, "busy".into())), "{codes:?}");
    assert!(
        (codes.iter())
            .all(|c| *c == (503, "busy".into()) || *c == (401, "invalid_credentials".into())),
        "{codes:?}"
    );
    // The records go; the sign-in that passes its password gives no token.
    local.audit.admin.run(&format!(
        "ALTER TABLE \"{}\".audit RENAME TO audit_gone",
        local.audit.schema
    ));
    let reply = login(console, "admin", "admin123");
    assert_eq!(refusal(&reply), (503, "audit_unavailable".into()));
    assert!(
        reply.json().get("token").is_none() && !reply.body.contains("ey"),
        "{}",
        reply.body
    );
    local.audit.admin.run(&format!(
        "ALTER TABLE \"{}\".audit_gone RENAME TO audit",
        local.audit.schema
    ));
    let changing = token(&login(console, "admin", "admin123"));
    // Postgres gone: no sign-in, no change.
    local.audit.relay.close();
    let reply = login(console, "admin", "admin123");
    assert_eq!(refusal(&reply), (503, "audit_unavailable".into()));
    let reply = change(console, &changing, "admin123", CHANGED);
    assert_eq!(refusal(&reply), (503, "audit_unavailable".into()));
    let Local {
        console,
        audit,
        keys,
        ..
    } = local;
    let stderr = console.stderr();
    assert_leaks_nothing(&audit, &keys, &stderr, &["admin123", CHANGED], &[&changing]);
}

/// The demo's end: the default account, its password changed, saves a workspace version as the
/// global operator, and the save is audited under `local:admin`. Real Valkey, store and Postgres.
#[cfg(feature = "refresh-lock")]
#[test]
fn the_global_operator_saves_a_workspace_version_audited_under_its_local_subject() {
    use crate::generations::{Fleet, import, workspace};
    use crate::snapshot::{KEY_ID, append_section};
    const ONE: &str = "e.test/accounts/one";
    const TWO: &str = "e.test/accounts/two";
    let case = "accounts_save";
    let Some(fleet) = Fleet::new(case, 2) else {
        return;
    };
    let entries = |repos: &[&str]| -> String {
        (repos.iter())
            .map(|r| format!("[[repository]]\nrepo = \"{r}\"\npath = \"../{r}\"\n"))
            .collect()
    };
    append_section(&fleet.dir, "\n[refresh_lock]\nttl_seconds = 10\n");
    fs::write(fleet.dir.join("ops/entries.toml"), entries(&[ONE, TWO])).unwrap();
    let (v1, _) = import(&fleet.dir, "ops/entries.toml", "version 1", "cli");
    let (_, stderr, code) = workspace(&fleet.dir, &["apply", &v1]);
    assert_eq!(code, Some(0), "{stderr}");
    let Some(audit) = audited(&fleet.dir, case) else {
        return;
    };
    keys(&fleet.dir);
    let source = fs::read_to_string(fleet.dir.join(MANIFEST)).unwrap();
    let sections = source.split_once("[snapshot]").unwrap().1;
    let bootstrap = format!(
        "index_dir = \"../console-indexes\"\n[workspace]\nsource = \"store\"\n[snapshot]{sections}\n\
         [ui]\n{}operations = true\n{}{}",
        ui_section(),
        audit.section,
        section()
    );
    fs::write(fleet.dir.join("ops/console.toml"), bootstrap).unwrap();
    super::front_end(&fleet.dir, Some(VERSION));
    let args = ["--listen", "127.0.0.1:0", "--workspace", "ops/console.toml"];
    let console = Server::start_command(&fleet.dir, "ui", &args, &[("MEADA_TEST_S3_KEY", KEY_ID)]);

    let changing = token(&login(&console, "admin", "admin123"));
    let operator = token(&change(&console, &changing, "admin123", CHANGED));
    let body = json!({"entries": entries(&[ONE]), "message": "only one"}).to_string();
    let refused = post(&console, "workspace/versions", Some(&changing), &body);
    assert_eq!(refusal(&refused), (403, "password_change_required".into()));
    let saved = post(&console, "workspace/versions", Some(&operator), &body);
    assert_eq!(saved.status, 200, "{}", saved.body);
    let version = saved.json()["result"].as_str().unwrap().to_string();
    let all = records(&console, &operator);
    let last = all.last().unwrap();
    assert_eq!(
        (
            last["action"].as_str(),
            last["subject"].as_str(),
            last["role"].as_str()
        ),
        (
            Some("workspace.save"),
            Some("local:admin"),
            Some("operator")
        )
    );
    assert_eq!(last["outcome"], "accepted");
    assert_eq!(
        last["arguments"],
        json!({"version": version, "message": "only one"})
    );
    let listed = ask(&console, "/api/v1/workspace/versions", &operator).json();
    let authors: Vec<&str> = (listed["versions"].as_array().unwrap().iter())
        .filter_map(|v| v["author"].as_str())
        .collect();
    assert!(authors.contains(&"local:admin"), "{listed}");
}
