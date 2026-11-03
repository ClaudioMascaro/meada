//! Break glass (#563, ADR 0024): `[accounts] break_glass = true` beside `[oidc]`. Only the default
//! account signs in to the console, as the global operator, and its session token is admitted
//! beside the identity provider's, each bearer routed to one verifier by its issuer. The section
//! rules and the start check need no service; the sign-in runs against the local Postgres of
//! `MEADA_TEST_POSTGRES_URL` and skips without it.

use jsonwebtoken::{Algorithm, EncodingKey, Header};

use super::*;
use crate::oidc::{Issuer, key, set, sign};

/// The nodes' public URL, which `[oidc] public_url` names.
const NODES: &str = "https://meada.example";
/// The identity provider's subject of every token here.
const SUBJECT: &str = "alice-563";
/// The boolean claim that makes an identity provider's token the global operator's.
const OPERATOR_CLAIM: &str = "meada_operator";

/// `[oidc]` for `issuer`.
fn oidc(issuer: &str) -> String {
    format!(
        "\n[oidc]\nissuer = \"{issuer}\"\npublic_url = \"{NODES}\"\n\
         identities_claim = \"meada_repositories\"\n"
    )
}

/// `[accounts]` under break glass, over `keys`' files, the initial password read from
/// `ADMIN_ENV`.
fn break_glass_section() -> String {
    section() + "break_glass = true\n"
}

/// `[ui]` of an OIDC console with operations and its operator claim.
fn ui_oidc() -> String {
    ui_section()
        + &format!(
            "client_id = \"meada-console\"\noperations = true\noperator_claim = \"{OPERATOR_CLAIM}\"\n"
        )
}

/// Each break-glass rule names its sections or its key: `break_glass` needs `[oidc]`, and
/// `[accounts]` beside `[oidc]` without it is still the error naming both; the grant tables cannot
/// sit under it. With `[oidc]` the section is accepted: `status` reads the manifest and stops at
/// the Postgres it cannot reach.
#[test]
fn break_glass_needs_oidc_and_refuses_the_grant_tables() {
    let postgres = "\n[postgres]\nurl_env = \"MEADA_TEST_NO_POSTGRES_563\"\n";
    let oidc = oidc("https://id.example");
    for (case, head, expected) in [
        (
            "glass_no_oidc",
            format!("{postgres}{}", break_glass_section()),
            "`accounts.break_glass`: needs [oidc]: break glass keeps the default account beside \
             an identity provider",
        ),
        (
            "glass_off_beside_oidc",
            format!("{postgres}{oidc}{}break_glass = false\n", section()),
            "[accounts] cannot sit beside [oidc]",
        ),
        (
            "glass_groups",
            format!(
                "{postgres}{oidc}{}\n[accounts.groups]\nops = [\"*\"]\n",
                break_glass_section()
            ),
            "`accounts.groups`: [accounts.groups] cannot sit under `break_glass = true`",
        ),
        (
            "glass_github",
            format!(
                "{postgres}{oidc}{}\n[accounts.github]\nhost = \"github.com\"\n",
                break_glass_section()
            ),
            "`accounts.github`: [accounts.github] cannot sit under `break_glass = true`",
        ),
    ] {
        // Without `[oidc]`, `[ui]` names no operator claim, which needs it.
        let ui = match head.contains("[oidc]") {
            true => ui_oidc(),
            false => ui_section(),
        };
        let dir = console_dir(case, Some(VERSION), &head, &ui);
        let (_, stderr, code) = run_args(&dir, &["status", "--workspace", MANIFEST]);
        assert_eq!(code, Some(1), "{case}: {stderr}");
        assert!(stderr.contains(expected), "{case}: {stderr}");
        assert_eq!(stderr.lines().count(), 1, "{case}: {stderr}");
    }
    let head = format!("{postgres}{oidc}{}", break_glass_section());
    let dir = console_dir("glass_accepted", Some(VERSION), &head, &ui_oidc());
    let (stdout, stderr, _) = run_args(&dir, &["status", "--workspace", MANIFEST]);
    let both = format!("{stdout}{stderr}");
    assert!(both.contains("postgres: unreachable"), "{both}");
    assert!(!both.contains("accounts"), "{both}");
}

/// Under break glass a bearer's issuer picks its verifier, so the identity provider's issuer must
/// not be the console's own `public_url`: the start is refused naming the key, before any account
/// or key is read.
#[test]
fn the_console_refuses_an_identity_provider_issuing_as_itself() {
    let (_, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let head = format!(
        "\n[postgres]\nurl_env = \"MEADA_TEST_NO_POSTGRES_563\"\n{}{}",
        oidc(&issuer.base),
        break_glass_section()
    );
    let ui = format!(
        "dir = \"../ui\"\npublic_url = \"{}\"\nclient_id = \"meada-console\"\n",
        issuer.base
    );
    let dir = console_dir("glass_same_issuer", Some(VERSION), &head, &ui);
    let stderr = refused(&dir, &[]);
    assert!(
        stderr.contains(
            "`accounts.break_glass`: the identity provider's issuer is the console's own \
             public_url"
        ),
        "{stderr}"
    );
    // Refused before the key pair is read: no key file exists here.
    assert!(!stderr.contains("public_key_file"), "{stderr}");
}

/// An OIDC console with break glass over a Postgres schema of `case`'s own, running.
struct BreakGlass {
    audit: Audited,
    console: Server,
    keys: (String, String),
    issuer: Issuer,
    /// The identity provider's signing key, `k1`.
    signing: EncodingKey,
    /// The default account's generated first password.
    first: String,
}

/// The console of `case`: `[oidc]` over a loopback issuer, `[accounts]` under break glass,
/// `operations = true`.
fn break_glass_console(case: &'static str) -> Option<BreakGlass> {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = console_dir(case, Some(VERSION), &oidc(&issuer.base), &ui_oidc());
    let audit = audited(&dir, case)?;
    let keys = keys(&dir);
    let manifest = dir.join(MANIFEST);
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        format!("{text}{}{}", audit.section, break_glass_section()),
    )
    .unwrap();
    let mut console = start(&dir, &[]);
    let first = first_password(&mut console);
    Some(BreakGlass {
        audit,
        console,
        keys,
        issuer,
        signing,
        first,
    })
}

impl BreakGlass {
    /// A console token of the identity provider, `operator` by its boolean claim, with `edit`.
    fn identity_provider(&self, operator: bool, edit: impl FnOnce(&mut Value)) -> String {
        let mut claims = json!({
            "iss": self.issuer.base, "aud": PUBLIC, "sub": SUBJECT,
            "exp": jsonwebtoken::get_current_timestamp() + 300,
            "meada_repositories": ["example.com/chain/web"], OPERATOR_CLAIM: operator,
        });
        edit(&mut claims);
        sign(&self.signing, "k1", Algorithm::ES256, &claims)
    }

    /// A token signed with the console's own key: a session token of `subject` by default.
    fn meada(&self, subject: &str, edit: impl FnOnce(&mut Value)) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        let mut claims = json!({
            "iss": PUBLIC, "aud": "meada:console", "sub": subject, "role": "operator",
            "groups": [], "iat": now, "exp": now + 300,
        });
        edit(&mut claims);
        let key = EncodingKey::from_ed_pem(self.keys.0.as_bytes()).unwrap();
        jsonwebtoken::encode(&Header::new(Algorithm::EdDSA), &claims, &key).unwrap()
    }
}

/// The demo: with the identity provider's tokens still admitted, the default account signs in
/// with the first password the console generated, changes its password and is the global
/// operator, reaching a pointer recovery;
/// a colleague's local account, and every other username, gets the wrong password's 401, recorded
/// `not_break_glass`. Account administration, agent tokens and login verification answer 404. A
/// bearer reaches one verifier by its issuer: a token of the console's issuer signed by the
/// identity provider, one of the identity provider's issuer signed by the console's key, a
/// colleague's session token from before the switch and a token for another surface are all
/// 401s, and an identity provider's session gets the password change's 404. With the identity
/// provider's keys unreachable, the default account still signs in. No answer, record or stderr
/// line holds a password, a hash or a token.
#[test]
fn the_break_glass_account_signs_in_beside_the_identity_provider() {
    let Some(glass) = break_glass_console("accounts_break_glass") else {
        return;
    };
    let generated = glass.first.as_str();
    let console = &glass.console;
    let mut replies = Vec::new();

    let config = get(console, "/api/v1/config");
    assert_eq!(config.status, 200);
    let document = config.json();
    assert_eq!(
        (&document["sign_in"], &document["break_glass"]),
        (&json!("oidc"), &json!(true))
    );
    assert_eq!(document["issuer"], json!(glass.issuer.base));

    // The identity provider's tokens: a viewer's and an operator's sessions, unchanged.
    let viewer = glass.identity_provider(false, |_| {});
    let session = ask(console, "/api/v1/session", &viewer);
    assert_eq!(session.status, 200, "{}", session.body);
    assert_eq!(
        (&session.json()["subject"], &session.json()["role"]),
        (&json!(SUBJECT), &json!("viewer"))
    );
    assert!(session.json().get("account").is_none(), "{}", session.body);
    let operator_by_claim = glass.identity_provider(true, |_| {});
    let session = ask(console, "/api/v1/session", &operator_by_claim);
    assert_eq!(session.json()["role"], "operator", "{}", session.body);
    let missing = get(console, "/api/v1/session");
    assert_eq!(missing.status, 401);

    // The default account: its first password opens the password change alone, then the global
    // operator.
    let first = login(console, "admin", generated);
    let changing = token(&first);
    assert_eq!(first.json()["change_required"], true);
    let early = ask(console, "/api/v1/session", &changing);
    assert_eq!(refusal(&early), (403, "password_change_required".into()));
    let changed = change(console, &changing, generated, CHANGED);
    let operator = token(&changed);
    let session = ask(console, "/api/v1/session", &operator);
    assert_eq!(
        session.json(),
        json!({"subject": "local:admin", "role": "operator", "groups": [], "grant": "*",
               "account": true})
    );
    assert_eq!(ask(console, "/api/v1/manifest", &operator).status, 200);
    // The admission view is the identity provider's, with break glass's mode and key (#558).
    let admission = ask(console, "/api/v1/admission", &operator).json();
    assert_eq!(admission["issuer"], json!(glass.issuer.base), "{admission}");
    assert_eq!(admission["accounts"]["mode"], "break_glass", "{admission}");
    let fingerprint = admission["accounts"]["public_key"].as_str().unwrap();
    assert!(fingerprint.starts_with("blake3:"), "{fingerprint}");
    let again = login(console, "admin", CHANGED);
    assert_eq!(again.json()["change_required"], false);

    // A colleague's local account, an unknown username and a wrong password: the same 401.
    glass.audit.admin.run(&format!(
        "INSERT INTO \"{0}\".accounts (username, password_hash, role, change_required) SELECT \
         'colleague', password_hash, 'operator', false FROM \"{0}\".accounts WHERE username = \
         'admin'",
        glass.audit.schema
    ));
    let colleague = login(console, "colleague", CHANGED);
    let unknown = login(console, "nobody", CHANGED);
    let wrong = login(console, "admin", "not the password");
    assert_eq!(refusal(&colleague), (401, "invalid_credentials".into()));
    assert_eq!(colleague.body, wrong.body);
    assert_eq!(unknown.body, wrong.body);

    // Nothing else of local accounts exists.
    for (method, route) in [
        ("GET", "accounts"),
        ("POST", "accounts"),
        ("POST", "accounts/colleague"),
        ("POST", "account/tokens"),
        ("POST", "account/github"),
        ("POST", "account/github/remove"),
    ] {
        let reply = match method {
            "GET" => ask(console, &format!("/api/v1/{route}"), &operator),
            _ => post(console, route, Some(&operator), "{}"),
        };
        assert_eq!(
            refusal(&reply),
            (404, "not_found".into()),
            "{method} {route}"
        );
    }

    // A pointer recovery: authorized as the global operator and recorded as its subject, whatever
    // the fleet then says.
    #[cfg(feature = "refresh-lock")]
    {
        let recover = post(console, "fence/recover", Some(&operator), "{}");
        assert!(
            ![401, 403].contains(&recover.status),
            "{}: {}",
            recover.status,
            recover.body
        );
        replies.push(recover);
    }

    // One verifier per issuer.
    let crossed = [
        (
            "the console's issuer, the identity provider's key",
            glass.identity_provider(true, |c| c["iss"] = json!(PUBLIC)),
        ),
        (
            "the identity provider's issuer, the console's key",
            glass.meada("local:admin", |c| {
                c["iss"] = json!(glass.issuer.base);
                c["aud"] = json!(PUBLIC);
            }),
        ),
        (
            "a colleague's session token",
            glass.meada("local:colleague", |_| {}),
        ),
        (
            "an agent token for the nodes",
            glass.meada("local:admin", |c| c["aud"] = json!("meada:nodes")),
        ),
        (
            "an expired session token",
            glass.meada("local:admin", |c| {
                c["exp"] = json!(jsonwebtoken::get_current_timestamp() - 3600)
            }),
        ),
    ];
    for (label, token) in &crossed {
        let reply = ask(console, "/api/v1/session", token);
        assert_eq!(reply.status, 401, "{label}: {}", reply.body);
        replies.push(reply);
    }
    assert_eq!(
        ask(
            console,
            "/api/v1/session",
            &glass.meada("local:admin", |_| {})
        )
        .status,
        200
    );

    // An identity provider's session has no password here: the change is not its route.
    let body = json!({"current": CHANGED, "new": "another long password"}).to_string();
    let foreign = post(console, "account/password", Some(&viewer), &body);
    assert_eq!(refusal(&foreign), (404, "not_found".into()));
    replies.push(foreign);

    // The identity provider's keys unreachable: a token of a key the console has not cached is
    // refused, and the break-glass account still signs in and is served.
    glass.issuer.serve(None);
    let (rotated, _) = key("k2");
    let unverifiable = sign(
        &rotated,
        "k2",
        Algorithm::ES256,
        &json!({"iss": glass.issuer.base, "aud": PUBLIC, "sub": SUBJECT,
                "exp": jsonwebtoken::get_current_timestamp() + 300}),
    );
    assert_eq!(ask(console, "/api/v1/session", &unverifiable).status, 401);
    let down = token(&login(console, "admin", CHANGED));
    assert_eq!(ask(console, "/api/v1/session", &down).status, 200);

    let all = records(console, &operator);
    let refused_logins: Vec<&Value> = (all.iter())
        .filter(|r| r["action"] == "account.login" && r["outcome"] == "refused")
        .collect();
    assert_eq!(
        refused_logins,
        [
            &record(
                None,
                "none",
                "account.login",
                json!({"reason": "not_break_glass"}),
                "refused"
            ),
            &record(
                None,
                "none",
                "account.login",
                json!({"reason": "not_break_glass"}),
                "refused"
            ),
            &record(
                Some("local:admin"),
                "operator",
                "account.login",
                json!({"account": "admin", "reason": "password"}),
                "refused"
            ),
        ]
    );
    #[cfg(feature = "refresh-lock")]
    {
        let recovery = (all.iter()).find(|r| r["action"] == "fence.recover");
        let recovery = recovery.expect("the recovery's record");
        assert_eq!(
            (&recovery["subject"], &recovery["role"]),
            (&json!("local:admin"), &json!("operator"))
        );
    }

    replies.extend([config, first, early, changed, colleague, unknown, wrong]);
    for reply in &replies {
        reply.assert_no_cookie_or_cors();
    }
    let passwords = [generated, CHANGED, "not the password"];
    let mut tokens = vec![
        changing.as_str(),
        operator.as_str(),
        viewer.as_str(),
        down.as_str(),
    ];
    tokens.extend(crossed.iter().map(|(_, token)| token.as_str()));
    for reply in &replies {
        if reply.status != 200 {
            assert_leaks_nothing(&glass.audit, &glass.keys, &reply.body, &passwords, &tokens);
        }
    }
    let rows = glass.audit.admin.texts(&format!(
        "SELECT row_to_json(a)::text FROM \"{}\".audit a",
        glass.audit.schema
    ));
    assert_leaks_nothing(
        &glass.audit,
        &glass.keys,
        &rows.join("\n"),
        &passwords,
        &tokens,
    );
    let BreakGlass {
        console,
        audit,
        keys,
        ..
    } = glass;
    let stderr = console.stderr();
    assert_eq!(
        stderr
            .matches("ui: - account.login -: refused (not_break_glass)")
            .count(),
        2,
        "{stderr}"
    );
    assert!(
        stderr.contains("ui: local:admin account.password admin: accepted"),
        "{stderr}"
    );
    assert!(!stderr.contains("colleague"), "{stderr}");
    assert_leaks_nothing(&audit, &keys, &stderr, &passwords, &tokens);
}
