use jsonwebtoken::{Algorithm, EncodingKey, Header};

use super::*;
use crate::oidc::{Issuer, key, set, sign};

const NODES: &str = "https://meada.example";
const SUBJECT: &str = "alice-563";
const OPERATOR_CLAIM: &str = "meada_operator";

fn oidc(issuer: &str) -> String {
    format!(
        "\n[oidc]\nissuer = \"{issuer}\"\npublic_url = \"{NODES}\"\n\
         identities_claim = \"meada_repositories\"\n"
    )
}

fn break_glass_section() -> String {
    section() + "break_glass = true\n"
}

fn ui_oidc() -> String {
    ui_section()
        + &format!(
            "client_id = \"meada-console\"\noperations = true\noperator_claim = \"{OPERATOR_CLAIM}\"\n"
        )
}

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
    assert!(!stderr.contains("public_key_file"), "{stderr}");
}

struct BreakGlass {
    audit: Audited,
    console: Server,
    keys: (String, String),
    issuer: Issuer,
    signing: EncodingKey,
    first: String,
}

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
    fn identity_provider(&self, operator: bool, edit: impl FnOnce(&mut Value)) -> String {
        let mut claims = json!({
            "iss": self.issuer.base, "aud": PUBLIC, "sub": SUBJECT,
            "exp": jsonwebtoken::get_current_timestamp() + 300,
            "meada_repositories": ["example.com/chain/web"], OPERATOR_CLAIM: operator,
        });
        edit(&mut claims);
        sign(&self.signing, "k1", Algorithm::ES256, &claims)
    }

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
    let admission = ask(console, "/api/v1/admission", &operator).json();
    assert_eq!(admission["issuer"], json!(glass.issuer.base), "{admission}");
    assert_eq!(admission["accounts"]["mode"], "break_glass", "{admission}");
    let fingerprint = admission["accounts"]["public_key"].as_str().unwrap();
    assert!(fingerprint.starts_with("blake3:"), "{fingerprint}");
    let again = login(console, "admin", CHANGED);
    assert_eq!(again.json()["change_required"], false);

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

    let body = json!({"current": CHANGED, "new": "another long password"}).to_string();
    let foreign = post(console, "account/password", Some(&viewer), &body);
    assert_eq!(refusal(&foreign), (404, "not_found".into()));
    replies.push(foreign);

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
