//! The console with `[oidc]` (#306, ADR 0014): admission by the nodes' module against the
//! `oidc` module's loopback issuer and JWKS, the `session`, `config` and admission routes, and the
//! token's grant bounding every view. No service is needed.

use std::path::PathBuf;

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};

use super::{
    PUBLIC, Reply, VERSION, console_dir, get, send, send_body, start, ui_section, write_manifest,
};
use crate::manifest::{MANIFEST, run_args};
use crate::oidc::{Issuer, key, set, sign};
use crate::server::Server;

const WEB: &str = "example.com/chain/web";
const GATEWAY: &str = "example.com/chain/gateway";
/// The nodes' public URL, which `[oidc] public_url` names.
const NODES: &str = "https://meada.example";
const SCOPE: &str = "meada:read";
const SUBJECT: &str = "alice-7c1e";

/// The `[[repository]]` entries and `[oidc]` of every console here: the identities claim, groups
/// mapping `team-web` to `WEB`, `team-gw` to `GATEWAY` and `team-all` to `*`, and `SCOPE`.
fn head(issuer: &str) -> String {
    format!(
        "\n[[repository]]\nrepo = \"{WEB}\"\npath = \"../web\"\n\n[[repository]]\nrepo = \
         \"{GATEWAY}\"\npath = \"../gateway\"\n{}",
        oidc(issuer)
    )
}

/// The `[oidc]` of `head`.
fn oidc(issuer: &str) -> String {
    format!(
        "\n[oidc]\nissuer = \"{issuer}\"\npublic_url = \"{NODES}\"\nidentities_claim = \
         \"meada_repositories\"\ngroups_claim = \"groups\"\nscopes = [\"{SCOPE}\"]\n\n\
         [oidc.groups]\n\"team-web\" = [\"{WEB}\"]\n\"team-gw\" = [\"{GATEWAY}\"]\n\"team-all\" = \
         [\"*\"]\n"
    )
}

fn ui_oidc() -> String {
    ui_section() + "client_id = \"meada-console\"\n"
}

/// A console over `issuer`'s keys, running.
fn console(case: &str, issuer: &Issuer) -> (PathBuf, Server) {
    let dir = console_dir(case, Some(VERSION), &head(&issuer.base), &ui_oidc());
    let console = start(&dir, "127.0.0.1:0");
    (dir, console)
}

/// The claims of a console token of `issuer` for `subject`, granting `identities`, with `SCOPE`.
fn claims(issuer: &str, identities: &[&str]) -> Value {
    json!({
        "iss": issuer, "aud": PUBLIC, "sub": SUBJECT,
        "exp": jsonwebtoken::get_current_timestamp() + 300,
        "meada_repositories": identities, "scope": format!("openid {SCOPE}"),
    })
}

/// `claims` changed by `edit`.
fn edited(issuer: &str, edit: impl FnOnce(&mut Value)) -> Value {
    let mut claims = claims(issuer, &[WEB]);
    edit(&mut claims);
    claims
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn ask(console: &Server, path: &str, token: &str) -> Reply {
    send(console, "GET", path, &[("Authorization", &bearer(token))])
}

/// The token check of `pasted`, by the bearer of `token`.
fn check(console: &Server, token: &str, pasted: &str) -> Reply {
    let body = json!({ "token": pasted }).to_string();
    let headers = [("Authorization", bearer(token)), ("Origin", PUBLIC.into())];
    let headers: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
    send_body(console, "POST", "/api/v1/admission/check", &headers, &body)
}

/// Neither `stderr` nor `answers` carries a part of a token of `tokens` or a claim value.
fn assert_leaks_nothing(stderr: &str, tokens: &[&str]) {
    for token in tokens {
        for part in token.split('.') {
            assert!(!stderr.contains(part), "a token part on stderr: {stderr}");
        }
    }
    for claim in [SUBJECT, "team-web", "team-gw", WEB, GATEWAY] {
        assert!(!stderr.contains(claim), "{claim} on stderr: {stderr}");
    }
}

/// No token, an expired one, the nodes' audience, another issuer and `HS256` get 401 with
/// `WWW-Authenticate`; a missing scope is a 403; a valid token is a viewer's session. The static
/// files and `config` need no token, and every answer's policy names the issuer's origin.
#[test]
fn admits_a_bearer_token_for_the_console_s_audience() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let base = issuer.base.clone();
    let (_dir, console) = console("ui_oidc_admit", &issuer);
    let csp = format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self' {base}; \
         frame-ancestors 'none'"
    );

    let es256 = |claims: &Value| sign(&signing, "k1", Algorithm::ES256, claims);
    let valid = es256(&claims(&base, &[WEB]));
    let hs256 = jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims(&base, &[WEB]),
        &EncodingKey::from_secret(b"a shared secret"),
    )
    .unwrap();
    let refused = [
        es256(&edited(&base, |c| {
            c["exp"] = json!(jsonwebtoken::get_current_timestamp() - 3600)
        })),
        es256(&edited(&base, |c| c["aud"] = json!(NODES))),
        es256(&edited(&base, |c| {
            c["iss"] = json!("https://other.example")
        })),
        hs256,
    ];

    let missing = get(&console, "/api/v1/session");
    assert_eq!(missing.status, 401);
    assert_eq!(
        missing.header("www-authenticate"),
        Some("Bearer scope=\"meada:read\"")
    );
    assert_eq!(missing.json()["error"]["code"], json!("unauthorized"));
    for token in &refused {
        let reply = ask(&console, "/api/v1/session", token);
        assert_eq!(reply.status, 401, "{token}");
        assert_eq!(
            reply.header("www-authenticate"),
            Some("Bearer error=\"invalid_token\", scope=\"meada:read\"")
        );
        assert_eq!(reply.header("content-security-policy"), Some(csp.as_str()));
        assert!(!reply.body.contains(token.as_str()));
    }

    let unscoped = es256(&edited(&base, |c| c["scope"] = json!("openid")));
    let reply = ask(&console, "/api/v1/manifest", &unscoped);
    assert_eq!(reply.status, 403);
    assert_eq!(
        reply.header("www-authenticate"),
        Some("Bearer error=\"insufficient_scope\", scope=\"meada:read\"")
    );

    let session = ask(&console, "/api/v1/session", &valid);
    assert_eq!(session.status, 200);
    assert_eq!(
        session.json(),
        json!({"subject": SUBJECT, "role": "viewer", "groups": [], "grant": [WEB]})
    );

    let config = get(&console, "/api/v1/config");
    assert_eq!(config.status, 200);
    assert_eq!(
        config.json(),
        json!({
            "version": VERSION, "operations": false, "public_url": PUBLIC, "issuer": base,
            "client_id": "meada-console", "scopes": [SCOPE], "audience": PUBLIC,
        })
    );
    let index = get(&console, "/");
    assert_eq!(index.status, 200);
    for reply in [&missing, &session, &config, &index] {
        assert_eq!(reply.header("content-security-policy"), Some(csp.as_str()));
        reply.assert_no_cookie_or_cors();
    }

    let mut tokens: Vec<&str> = refused.iter().map(String::as_str).collect();
    tokens.extend([valid.as_str(), unscoped.as_str()]);
    assert_leaks_nothing(&console.stderr(), &tokens);
}

/// The start is refused without `[ui] client_id`, and when the issuer's metadata or JWKS cannot
/// be read, as a node's is.
#[test]
fn refuses_to_start_without_client_id_metadata_or_jwks() {
    let (_, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let refused = |dir: &std::path::Path| {
        let args = ["ui", "--listen", "127.0.0.1:0", "--workspace", MANIFEST];
        let (stdout, stderr, code) = run_args(dir, &args);
        assert_eq!(code, Some(1), "{stderr}");
        assert!(
            stdout.is_empty() && !stderr.contains("listening"),
            "{stderr}"
        );
        stderr
    };

    let dir = console_dir(
        "ui_oidc_client",
        Some(VERSION),
        &head(&issuer.base),
        &ui_section(),
    );
    assert_eq!(
        refused(&dir),
        format!("meada: {MANIFEST}:23: `ui`: `ui.client_id` is missing\n")
    );

    // A port nothing listens on: no metadata.
    let dir = console_dir(
        "ui_oidc_metadata",
        Some(VERSION),
        &head("http://127.0.0.1:9"),
        &ui_oidc(),
    );
    let stderr = refused(&dir);
    assert!(stderr.contains("metadata could not be read"), "{stderr}");

    issuer.serve(None);
    let dir = console_dir(
        "ui_oidc_jwks",
        Some(VERSION),
        &head(&issuer.base),
        &ui_oidc(),
    );
    let stderr = refused(&dir);
    assert!(
        stderr.contains("JWKS") && stderr.contains("http status 500"),
        "{stderr}"
    );
}

/// `admission`: the issuer's metadata, the cached key ids, the last read and the groups table
/// bounded by the viewer's grant; once the JWKS fails, the failed refresh with why.
#[test]
fn shows_the_admission_state_and_a_failed_refresh() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let base = issuer.base.clone();
    let (_dir, console) = console("ui_oidc_admission", &issuer);
    let web = sign(&signing, "k1", Algorithm::ES256, &claims(&base, &[WEB]));

    let admission = ask(&console, "/api/v1/admission", &web).json();
    assert_eq!(
        admission["metadata"],
        json!({"issuer": base, "jwks_uri": format!("{base}/jwks")})
    );
    assert_eq!(admission["issuer"], json!(base));
    assert_eq!(admission["keys"], json!(["k1"]));
    assert!(admission["refreshed"].as_str().unwrap().ends_with('Z'));
    assert_eq!(admission["failed"], Value::Null);
    assert_eq!(admission["groups_claim"], json!("groups"));
    // `team-gw` maps to an identity outside the grant: listed without it.
    assert_eq!(
        admission["groups"],
        json!({"team-web": [WEB], "team-gw": [], "team-all": ["*"]})
    );
    let all = sign(
        &signing,
        "k1",
        Algorithm::ES256,
        &edited(&base, |c| c["groups"] = json!(["team-all"])),
    );
    let whole = ask(&console, "/api/v1/admission", &all).json();
    assert_eq!(whole["groups"]["team-gw"], json!([GATEWAY]));

    // A token naming a `kid` the cache lacks fetches the JWKS, which now fails.
    issuer.serve(None);
    let (other, _) = key("k9");
    let unknown = sign(&other, "k9", Algorithm::ES256, &claims(&base, &[WEB]));
    assert_eq!(ask(&console, "/api/v1/session", &unknown).status, 401);
    let admission = ask(&console, "/api/v1/admission", &web).json();
    assert_eq!(admission["keys"], json!(["k1"]));
    let failed = &admission["failed"];
    assert!(failed["time"].as_str().unwrap().ends_with('Z'), "{failed}");
    let reason = failed["reason"].as_str().unwrap();
    assert!(reason.contains("http status 500"), "{reason}");

    let stderr = console.stderr();
    assert!(stderr.contains("keeping the cached keys"), "{stderr}");
    assert_leaks_nothing(&stderr, &[&web, &all, &unknown]);
}

/// `admission/check`: each check's outcome as a node makes it, and the identities a valid token
/// admits, bounded by the viewer's own grant; the pasted token reaches no stderr line and the
/// check sends the issuer nothing, not even for an unknown `kid`. A body over the cap is a 413.
#[test]
fn checks_a_pasted_token_and_forwards_it_nowhere() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let base = issuer.base.clone();
    let (_dir, console) = console("ui_oidc_check", &issuer);
    let es256 = |claims: &Value| sign(&signing, "k1", Algorithm::ES256, claims);
    // The viewer reads `WEB` only.
    let viewer = es256(&claims(&base, &[WEB]));
    let all = es256(&edited(&base, |c| c["groups"] = json!(["team-all"])));

    // A token for a node: its audience is a resource of the nodes'.
    let node = |edit: &dyn Fn(&mut Value)| {
        let mut claims = claims(&base, &[WEB, GATEWAY]);
        claims["aud"] = json!(format!("{NODES}/{WEB}"));
        edit(&mut claims);
        claims
    };
    let valid = es256(&node(&|_| {}));
    let expired = es256(&node(&|c| {
        c["exp"] = json!(jsonwebtoken::get_current_timestamp() - 3600)
    }));
    let console_audience = es256(&node(&|c| c["aud"] = json!(PUBLIC)));
    let (forger, _) = key("k1");
    let forged = sign(&forger, "k1", Algorithm::ES256, &node(&|_| {}));
    let (other, _) = key("k9");
    let unknown = sign(&other, "k9", Algorithm::ES256, &node(&|_| {}));
    let pasted = [&valid, &expired, &console_audience, &forged, &unknown];
    let requests = issuer.log.lock().unwrap().len();

    let checks = |pasted: &str, by: &str| {
        let reply = check(&console, by, pasted);
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.assert_no_cookie_or_cors();
        assert!(!reply.body.contains(pasted));
        reply.json()
    };
    let passed = |audience: bool, lifetime: bool| {
        json!({"signature": true, "issuer": true, "audience": audience,
               "lifetime": lifetime, "scopes": true})
    };
    let failed = json!({"signature": false, "issuer": null, "audience": null,
                        "lifetime": null, "scopes": null});
    assert_eq!(
        checks(&valid, &viewer),
        json!({"checks": passed(true, true), "identities": [WEB]})
    );
    assert_eq!(
        checks(&valid, &all),
        json!({"checks": passed(true, true), "identities": [WEB, GATEWAY]})
    );
    assert_eq!(
        checks(&expired, &all),
        json!({"checks": passed(true, false), "identities": null})
    );
    assert_eq!(
        checks(&console_audience, &all),
        json!({"checks": passed(false, true), "identities": null})
    );
    for token in [&forged, &unknown] {
        assert_eq!(
            checks(token, &all),
            json!({"checks": failed, "identities": null})
        );
    }
    let log = issuer.log.lock().unwrap().clone();
    assert_eq!(log.len(), requests, "the check reached the issuer: {log:?}");
    for request in &log {
        for token in pasted {
            for part in token.split('.') {
                assert!(!request.contains(part), "a pasted token part: {request}");
            }
        }
    }

    // Over the cap: 413, and the body is never quoted back.
    let big = "x".repeat(17 << 10);
    let reply = check(&console, &all, &big);
    assert_eq!(reply.status, 413);
    assert_eq!(reply.json()["error"]["code"], json!("too_large"));
    let headers = [("Authorization", bearer(&all)), ("Origin", PUBLIC.into())];
    let headers: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let quoted = json!(valid).to_string();
    let reply = send_body(
        &console,
        "POST",
        "/api/v1/admission/check",
        &headers,
        &quoted,
    );
    assert_eq!(reply.status, 400);
    assert_eq!(reply.json()["error"]["code"], json!("bad_request"));
    assert!(!reply.body.contains(valid.as_str()));
    // A check needs the console's own origin and a token, as every API route does.
    let body = json!({ "token": valid }).to_string();
    let reply = send_body(
        &console,
        "POST",
        "/api/v1/admission/check",
        &headers[..1],
        &body,
    );
    assert_eq!(reply.status, 403);
    let reply = send_body(
        &console,
        "POST",
        "/api/v1/admission/check",
        &headers[1..],
        &body,
    );
    assert_eq!(reply.status, 401);

    let mut tokens: Vec<&str> = pasted.iter().map(|t| t.as_str()).collect();
    tokens.extend([viewer.as_str(), all.as_str()]);
    assert_leaks_nothing(&console.stderr(), &tokens);
}

/// A viewer granted one repository sees only its `[[repository]]` entry, and no error text that
/// names another; a `*` grant sees everything.
#[test]
fn a_viewer_s_grant_bounds_every_view() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let base = issuer.base.clone();
    const UNLISTED: &str = "private.example/team/hidden";
    let nested = head(&base).replace(
        "path = \"../web\"\n",
        &format!("path = \"../web\"\n[[repository.peer]]\nrepo = \"{UNLISTED}\"\n"),
    );
    let ui = ui_oidc() + "operator_claim = \"operator\"\n";
    let dir = console_dir("ui_oidc_grant", Some(VERSION), &nested, &ui);
    let console = start(&dir, "127.0.0.1:0");
    let es256 = |claims: &Value| sign(&signing, "k1", Algorithm::ES256, claims);
    let web = es256(&claims(&base, &[WEB]));
    let gateway = es256(&edited(&base, |c| {
        c["meada_repositories"] = json!([]);
        c["groups"] = json!(["team-gw"]);
    }));
    let all = es256(&edited(&base, |c| c["meada_repositories"] = json!(["*"])));

    let operator = es256(&edited(&base, |c| c["operator"] = json!(true)));

    let entries = |token: &str| {
        let manifest = ask(&console, "/api/v1/manifest", token).json();
        let entries = manifest["manifest"]["repository"]
            .as_array()
            .unwrap()
            .clone();
        let repos: Vec<String> = entries
            .iter()
            .map(|e| e["repo"].as_str().unwrap().to_string())
            .collect();
        (repos, manifest)
    };
    let (repos, manifest) = entries(&web);
    assert_eq!(repos, [WEB]);
    // State that belongs to no repository is whole.
    assert_eq!(manifest["manifest"]["oidc"]["issuer"], json!(base));
    assert_eq!(
        manifest["manifest"]["ui"]["client_id"],
        json!("meada-console")
    );
    assert_eq!(manifest["modules"].as_array().unwrap().len(), 9);
    assert!(!manifest.to_string().contains("../gateway"));
    assert!(!manifest.to_string().contains(GATEWAY), "{manifest}");
    assert_eq!(
        manifest["manifest"]["oidc"]["groups"]["team-web"],
        json!([WEB])
    );
    for token in [&all, &operator] {
        assert_eq!(
            entries(token).1["manifest"]["oidc"]["groups"]["team-gw"],
            json!([GATEWAY])
        );
    }
    assert_eq!(manifest["manifest"]["repository"][0]["peer"], json!([]));
    assert_eq!(
        entries(&all).1["manifest"]["repository"][0]["peer"][0]["repo"],
        UNLISTED
    );
    assert_eq!(entries(&gateway).0, [GATEWAY]);
    assert_eq!(entries(&all).0, [WEB, GATEWAY]);

    // On disk, an error quoting a value of `GATEWAY`'s entry: only its position for `web`.
    let broken = head(&base).replace(
        "path = \"../gateway\"\n",
        &format!("path = \"../gateway\"\nserve = \"{GATEWAY}\"\n"),
    );
    write_manifest(&dir, &broken, &ui_oidc());
    let errors = |token: &str| ask(&console, "/api/v1/manifest", token).json()["errors"].clone();
    let whole = errors(&all);
    let line = whole[0].as_str().unwrap();
    assert!(line.contains(GATEWAY), "{line}");
    let position = line.split(": ").next().unwrap();
    assert_eq!(errors(&web), json!([position]));
    assert_eq!(errors(&gateway), whole);
    let broken_peer = nested.replace(
        &format!("repo = \"{UNLISTED}\"\n"),
        &format!("repo = \"{UNLISTED}\"\nbase_url_env = \"{UNLISTED}\"\n"),
    );
    write_manifest(&dir, &broken_peer, &ui_oidc());
    let whole = errors(&all);
    assert!(whole.to_string().contains(UNLISTED), "{whole}");
    let positions: Vec<_> = whole
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap().split(": ").next().unwrap())
        .collect();
    assert_eq!(errors(&web), json!(positions));

    assert_leaks_nothing(&console.stderr(), &[&web, &gateway, &all]);
}

/// An identity known only through a groups mapping is still hidden in on-disk errors.
#[test]
fn manifest_errors_hide_identities_known_only_through_groups() {
    const HIDDEN: &str = "private.example/group/hidden";
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let source = head(&issuer.base) + &format!("\"team-hidden\" = [\"{HIDDEN}\"]\n");
    let dir = console_dir("ui_oidc_group_error", Some(VERSION), &source, &ui_oidc());
    let console = start(&dir, "127.0.0.1:0");
    let token = |ids: &[&str]| sign(&signing, "k1", Algorithm::ES256, &claims(&issuer.base, ids));
    let broken = source.replace(
        "path = \"../web\"\n",
        &format!("path = \"../web\"\nserve = \"{HIDDEN}\"\n"),
    );
    write_manifest(&dir, &broken, &ui_oidc());
    let whole = ask(&console, "/api/v1/manifest", &token(&["*"])).json();
    assert!(whole["errors"].to_string().contains(HIDDEN), "{whole}");
    let restricted = ask(&console, "/api/v1/manifest", &token(&[WEB])).json();
    assert!(!restricted.to_string().contains(HIDDEN), "{restricted}");
    assert!(!restricted["errors"].as_array().unwrap().is_empty());

    // A new on-disk group may have the wrong scalar shape, so it never reached loaded state.
    const NEW: &str = "private.example/group/new";
    let broken = source.replace(&format!("[\"{HIDDEN}\"]"), &format!("\"{NEW}\""));
    write_manifest(&dir, &broken, &ui_oidc());
    let whole = ask(&console, "/api/v1/manifest", &token(&["*"])).json();
    assert!(whole["errors"].to_string().contains(NEW), "{whole}");
    let restricted = ask(&console, "/api/v1/manifest", &token(&[WEB])).json();
    assert!(!restricted.to_string().contains(NEW), "{restricted}");
}

/// The playground and the exports (#305) bounded by the viewer's grant, as a node bounds a call
/// (#290): a `*` grant answers the chain goldens; a grant of `WEB` alone answers about `WEB` with
/// every peer outside the grant dropped before the answer is built, and a repository outside it
/// is the 404 of one the console does not serve.
#[test]
fn a_viewer_s_grant_bounds_the_playground_and_the_exports() {
    use super::playground::{
        CATALOG, answer_the_goldens, body, chain_console, query, refusal,
        refuses_what_it_does_not_answer,
    };
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let base = issuer.base.clone();
    let dir = chain_console("ui_oidc_playground", &oidc(&base), &ui_oidc());
    assert!(crate::manifest::run(&dir, "index").2, "index");
    let console = start(&dir, "127.0.0.1:0");
    let es256 = |claims: &Value| sign(&signing, "k1", Algorithm::ES256, claims);
    let web = es256(&claims(&base, &[WEB]));
    let all = es256(&edited(&base, |c| c["meada_repositories"] = json!(["*"])));

    let whole = bearer(&all);
    let whole = [("Authorization", whole.as_str())];
    answer_the_goldens(&console, None, None, &whole);
    refuses_what_it_does_not_answer(&console, &whole);

    let viewer = bearer(&web);
    let viewer = [("Authorization", viewer.as_str())];
    let trace = query(
        &console,
        &body(WEB, "trace", r#"{"from": "dashboard"}"#),
        &viewer,
    );
    assert_eq!(trace.status, 200, "{}", trace.body);
    let text = trace.json()["text"].as_str().unwrap().to_string();
    assert!(
        text.starts_with(&format!("{WEB} src/dashboard.ts:4 dashboard\n")),
        "{text}"
    );
    assert!(!text.contains(GATEWAY) && !text.contains(CATALOG), "{text}");
    let route = format!("/api/v1/repositories/{WEB}/export");
    let export = send(&console, "GET", &route, &viewer);
    assert_eq!(export.status, 200);
    let links = export.json()["links"].as_array().unwrap().clone();
    for link in &links {
        for side in ["consumer", "provider"] {
            let side = link[side].as_str().unwrap();
            assert!(side.starts_with(&format!("{WEB}::")), "{link}");
        }
    }
    for outside in [GATEWAY, CATALOG] {
        let not_served = (
            404,
            "not_found".to_string(),
            format!("`{outside}` is not a repository this console serves"),
        );
        let asked = body(outside, "search", r#"{"query": "lib"}"#);
        assert_eq!(refusal(&query(&console, &asked, &viewer)), not_served);
        let route = format!("/api/v1/repositories/{outside}/export");
        assert_eq!(refusal(&send(&console, "GET", &route, &viewer)), not_served);
    }
    let asked = body(WEB, "search", r#"{"query": "lib"}"#);
    assert_eq!(query(&console, &asked, &[]).status, 401);
    assert_eq!(get(&console, &route).status, 401);

    assert_leaks_nothing(&console.stderr(), &[&web, &all]);
}

/// The identity provider's `type` (ADR 0016): `admission` names it, `generic` with the key absent
/// and `entra` when set, and nothing else changes for the same tokens: every read route (the
/// file `manifest` echoes aside), `session` and `admission/check`, a token in overage
/// (`_claim_names`, no `groups`) included.
#[test]
fn admission_names_the_type_and_nothing_else_changes() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let base = issuer.base.clone();
    let es256 = |claims: &Value| sign(&signing, "k1", Algorithm::ES256, claims);
    let overage = |c: &mut Value| {
        c["meada_repositories"] = json!([]);
        c["_claim_names"] = json!({"groups": "src1"});
        c["_claim_sources"] =
            json!({"src1": {"endpoint": "https://graph.example/getMemberObjects"}});
    };
    let tokens = [
        String::new(),
        "not-a-jwt".into(),
        es256(&claims(&base, &[WEB])),
        es256(&edited(&base, |c| {
            c["meada_repositories"] = json!([]);
            c["groups"] = json!(["team-gw"]);
        })),
        es256(&edited(&base, |c| c["groups"] = json!(["team-all"]))),
        es256(&edited(&base, overage)),
        es256(&edited(&base, |c| c["scope"] = json!("openid"))),
    ];
    let node = |edit: &dyn Fn(&mut Value)| {
        let mut claims = claims(&base, &[WEB, GATEWAY]);
        claims["aud"] = json!(format!("{NODES}/{WEB}"));
        edit(&mut claims);
        es256(&claims)
    };
    let pasted = [
        node(&|_| {}),
        node(&|c| overage(c)),
        node(&|c| c["groups"] = json!(["team-gw"])),
    ];
    let routes = [
        "session",
        "config",
        "manifest",
        "admission",
        "fleet",
        "store",
        "workspace",
        "repositories",
        "credentials",
    ];
    let typed = head(&base).replacen("\n[oidc]\n", "\n[oidc]\ntype = \"entra\"\n", 1)
        + "\n[oidc.entra]\noverage = \"ignore\"\n";
    let dir = console_dir("ui_oidc_type", Some(VERSION), &head(&base), &ui_oidc());
    let mut runs = Vec::new();
    for head in [head(&base), typed] {
        write_manifest(&dir, &head, &ui_oidc());
        let console = start(&dir, "127.0.0.1:0");
        let mut answers = Vec::new();
        let mut kinds = Vec::new();
        for token in &tokens {
            for route in routes {
                let path = format!("/api/v1/{route}");
                let reply = match token.is_empty() {
                    true => get(&console, &path),
                    false => ask(&console, &path, token),
                };
                let challenge = reply.header("www-authenticate").map(str::to_string);
                let mut body: Value = serde_json::from_str(&reply.body).unwrap();
                if route == "admission" && reply.status == 200 {
                    kinds.push(body["type"].take());
                    body["refreshed"].take();
                }
                if let Some(oidc) = body["manifest"]["oidc"].as_object_mut() {
                    oidc.remove("type");
                    oidc.remove("entra");
                }
                answers.push((route, reply.status, challenge, body));
            }
        }
        for by in &tokens[2..5] {
            for token in &pasted {
                let reply = check(&console, by, token);
                answers.push(("admission/check", reply.status, None, reply.json()));
            }
        }
        runs.push((answers, kinds, console.stderr()));
    }
    let [(answers, generic, stderr), (typed, entra, typed_stderr)] = &runs[..] else {
        unreachable!()
    };
    assert_eq!(generic, &vec![json!("generic"); 4]);
    assert_eq!(entra, &vec![json!("entra"); 4]);
    assert_eq!(answers, typed);
    assert_eq!(stderr, typed_stderr);
    // The overage token has no groups: no grant through `team-gw`.
    let session = |nth: usize| &answers[nth * routes.len()].3;
    assert_eq!(session(5)["groups"], json!([]));
    assert_eq!(session(5)["grant"], json!([]));
    assert_eq!(session(3)["grant"], json!([GATEWAY]));
    let tokens: Vec<&str> = tokens.iter().chain(&pasted).map(String::as_str).collect();
    assert_leaks_nothing(stderr, &tokens[1..]);
}

/// Without `[oidc]`, the admission routes are not there.
#[test]
fn without_oidc_the_admission_routes_are_absent() {
    let dir = console_dir("ui_oidc_absent", Some(VERSION), "", &ui_section());
    let console = start(&dir, "127.0.0.1:0");
    assert_eq!(get(&console, "/api/v1/admission").status, 404);
    let reply = send_body(
        &console,
        "POST",
        "/api/v1/admission/check",
        &[("Origin", PUBLIC)],
        "{\"token\": \"x\"}",
    );
    assert_eq!(reply.status, 404);
}

/// Managed workspaces have no bootstrap entries: every operational view and the token check
/// must use their checked current workspace, including nested peers and credential owners.
#[cfg(feature = "generation-pointer")]
#[test]
fn managed_workspace_reads_and_admission_check_obey_viewer_grants() {
    use crate::generations::{Fleet, admin, import, prefix, send as seed};
    use crate::snapshot::{KEY_ID, bucket, put_stored};
    use std::fs;
    let Some(fleet) = Fleet::new("ui_managed_grant", 3) else {
        return;
    };
    let (encoding, jwk) = key("managed");
    let issuer = Issuer::new(set(&[&jwk]));
    let source = fs::read_to_string(fleet.dir.join(MANIFEST)).unwrap();
    let with_peer = source.replace(
        "path = \"../chain-web\"",
        &format!("path = \"../chain-web\"\n[[repository.peer]]\nrepo = \"{GATEWAY}\""),
    );
    fs::write(fleet.dir.join("ops/entries.toml"), &with_peer).unwrap();
    let (version, _) = import(&fleet.dir, "ops/entries.toml", "managed grant", "alice");
    let generation = fleet.publish();
    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let key = format!("prod/meada/generations/{generation}.json");
    let mut document: Value = serde_json::from_slice(&crate::snapshot::get(&s3, &key)).unwrap();
    document["repositories"][0]["blocks"] = json!({"peer": [{"repo": GATEWAY}]});
    put_stored(&s3, &key, &serde_json::to_vec(&document).unwrap());
    let sections = source.split_once("[snapshot]").unwrap().1;
    let bootstrap = format!(
        "index_dir = \"../console-indexes\"\n[workspace]\nsource = \"store\"\n[snapshot]{sections}\n{}\n[ui]\n{}",
        oidc(&issuer.base),
        ui_oidc()
    );
    #[cfg(feature = "github-app")]
    let bootstrap = format!(
        "{bootstrap}\n[[github_app]]\nhost = \"example.com\"\napi = \"http://127.0.0.1:9\"\napp_id = 1\nprivate_key_file = \"absent-private-key.pem\"\n"
    );
    fs::write(fleet.dir.join("ops/console.toml"), bootstrap).unwrap();
    super::front_end(&fleet.dir, Some(VERSION));
    let mut valkey = admin(&fleet.valkey);
    let prefix = format!("{{{}}}", prefix(fleet.case));
    seed(
        &mut valkey,
        &[
            "HSET",
            &format!("{prefix}:workspace"),
            "id",
            &version,
            "fence",
            "7",
        ],
    );
    #[cfg(feature = "github-app")]
    seed(
        &mut valkey,
        &[
            "HSET",
            &format!(
                "{}:github-app:example.com:app:1:installations",
                crate::generations::prefix(fleet.case)
            ),
            "chain",
            "11",
            "private-owner",
            "22",
        ],
    );
    let console = Server::start_command(
        &fleet.dir,
        "ui",
        &["--listen", "127.0.0.1:0", "--workspace", "ops/console.toml"],
        &[("MEADA_TEST_S3_KEY", KEY_ID)],
    );
    let token = sign(
        &encoding,
        "managed",
        Algorithm::ES256,
        &claims(&issuer.base, &[WEB]),
    );
    for route in [
        "workspace".to_string(),
        "workspace/versions".to_string(),
        format!("workspace/versions/{version}"),
        "store".to_string(),
        format!("store/generations/{generation}"),
        "repositories".to_string(),
    ] {
        let reply = ask(&console, &format!("/api/v1/{route}"), &token);
        assert_eq!(reply.status, 200, "{route}: {}", reply.body);
        assert!(!reply.body.contains(GATEWAY), "{route}: {}", reply.body);
        assert!(
            !reply.body.contains("example.com/chain/catalog"),
            "{route}: {}",
            reply.body
        );
    }
    #[cfg(feature = "github-app")]
    {
        let credentials = ask(&console, "/api/v1/credentials", &token).json();
        let installations = credentials["apps"][0]["installations"].as_array().unwrap();
        assert_eq!(installations.len(), 1);
        assert_eq!(installations[0]["owner"], "chain");
        assert_eq!(installations[0]["id"], 11);
        assert!(!credentials.to_string().contains("private-owner"));
        let all = sign(
            &encoding,
            "managed",
            Algorithm::ES256,
            &claims(&issuer.base, &["*"]),
        );
        assert_eq!(
            ask(&console, "/api/v1/credentials", &all).json()["apps"][0]["installations"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    let pasted = sign(
        &encoding,
        "managed",
        Algorithm::ES256,
        &edited(&issuer.base, |c| c["aud"] = json!(format!("{NODES}/{WEB}"))),
    );
    let checked = check(&console, &token, &pasted);
    assert_eq!(checked.status, 200, "{}", checked.body);
    assert_eq!(checked.json()["checks"]["audience"], true);
    assert_eq!(checked.json()["identities"], json!([WEB]));
    assert_eq!(
        ask(
            &console,
            "/api/v1/workspace/versions/../../etc/passwd",
            &token
        )
        .status,
        404
    );
}

/// An operator is decided from the configured group or a boolean claim, and sees the whole
/// workspace even when the token grants a single repository. A string is never a boolean.
#[test]
fn operator_groups_and_boolean_claims_decide_the_role_and_whole_workspace_view() {
    let (encoding, jwk) = key("operator");
    let issuer = Issuer::new(set(&[&jwk]));
    for (case, setting, changes) in [
        (
            "ui_operator_group",
            "operator_groups = [\"operators\"]\n",
            json!({"groups": ["operators"]}),
        ),
        (
            "ui_operator_claim",
            "operator_claim = \"meada_operator\"\n",
            json!({"meada_operator": true}),
        ),
        (
            "ui_viewer_string",
            "operator_claim = \"meada_operator\"\n",
            json!({"meada_operator": "true"}),
        ),
    ] {
        let ui = ui_oidc() + "operations = true\n" + setting;
        let dir = console_dir(case, Some(VERSION), &head(&issuer.base), &ui);
        let console = start(&dir, "127.0.0.1:0");
        let mut token_claims = claims(&issuer.base, &[WEB]);
        token_claims
            .as_object_mut()
            .unwrap()
            .extend(changes.as_object().unwrap().clone());
        let token = sign(&encoding, "operator", Algorithm::ES256, &token_claims);
        let operator = case != "ui_viewer_string";
        let session = ask(&console, "/api/v1/session", &token).json();
        assert_eq!(
            session["role"],
            if operator { "operator" } else { "viewer" }
        );
        let manifest = ask(&console, "/api/v1/manifest", &token).json();
        assert_eq!(
            manifest["manifest"]["repository"].as_array().unwrap().len(),
            if operator { 2 } else { 1 }
        );
    }
}

/// Operator configuration errors name their file, line and the keys that must be corrected.
#[test]
fn operator_configuration_requires_oidc_one_role_key_and_the_group_claim() {
    let issuer = "http://127.0.0.1:9";
    let cases = [
        (
            "ui_operations_no_oidc",
            String::new(),
            "operations = true\noperator_claim = \"operator\"\n",
            "ui.operations",
            "[oidc]",
        ),
        (
            "ui_operator_no_key",
            oidc(issuer),
            "operations = true\n",
            "ui.operator_groups",
            "ui.operator_claim",
        ),
        (
            "ui_operator_both_keys",
            oidc(issuer),
            "operator_groups = [\"operators\"]\noperator_claim = \"operator\"\n",
            "ui.operator_groups",
            "ui.operator_claim",
        ),
        (
            "ui_operator_no_group_claim",
            format!(
                "[oidc]\nissuer = \"{issuer}\"\npublic_url = \"{NODES}\"\nidentities_claim = \"meada_repositories\"\n"
            ),
            "operator_groups = [\"operators\"]\n",
            "ui.operator_groups",
            "oidc.groups_claim",
        ),
        (
            "ui_operator_key_no_oidc",
            String::new(),
            "operator_claim = \"operator\"\n",
            "ui.operator_claim",
            "[oidc]",
        ),
    ];
    for (case, head, setting, first, second) in cases {
        let dir = console_dir(case, Some(VERSION), &head, &(ui_oidc() + setting));
        let (_, stderr, ok) = run_args(&dir, &["status", "--workspace", MANIFEST]);
        assert_eq!(ok, Some(1), "{case}: {stderr}");
        assert!(
            stderr.contains(first) && stderr.contains(second),
            "{case}: {stderr}"
        );
        assert!(
            stderr
                .lines()
                .any(|line| line.starts_with(&format!("meada: {MANIFEST}:"))
                    && line
                        .split(':')
                        .nth(2)
                        .is_some_and(|part| part.parse::<usize>().is_ok())),
            "{case}: {stderr}"
        );
    }
}

/// No refused operation waits for an upload: Host, Origin, authentication, disabled writes
/// and the role are decided first. Every refusal leaves one safe audit line.
#[test]
fn refused_writes_are_authorized_and_audited_before_any_body_is_consumed() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    let (encoding, jwk) = key("write-preflight");
    let issuer = Issuer::new(set(&[&jwk]));
    let token = |change: fn(&mut Value)| {
        sign(
            &encoding,
            "write-preflight",
            Algorithm::ES256,
            &edited(&issuer.base, change),
        )
    };
    let viewer = token(|_| {});
    let operator = token(|c| c["operator"] = json!(true));
    let nameless = token(|c| {
        c["operator"] = json!(true);
        c.as_object_mut().unwrap().remove("sub");
    });
    for enabled in [true, false] {
        let case = if enabled {
            "ui_write_preflight"
        } else {
            "ui_write_disabled"
        };
        let ui = format!(
            "{}operations = {enabled}\noperator_claim = \"operator\"\n",
            ui_oidc()
        );
        let dir = console_dir(case, Some(VERSION), &head(&issuer.base), &ui);
        let console = start(&dir, "127.0.0.1:0");
        let address = console.url.trim_start_matches("http://");
        let cases = if enabled {
            vec![
                (Some(&viewer), Some(PUBLIC), address, 403),
                (None, Some(PUBLIC), address, 401),
                (
                    Some(&operator),
                    Some("https://foreign.example"),
                    address,
                    403,
                ),
                (Some(&operator), None, address, 403),
                (Some(&operator), Some(PUBLIC), "foreign.example", 403),
                (Some(&nameless), Some(PUBLIC), address, 403),
            ]
        } else {
            vec![(Some(&operator), Some(PUBLIC), address, 403)]
        };
        for route in [
            "pointer",
            "fence/recover",
            "workspace/versions",
            "workspace/apply",
        ] {
            for (bearer_token, origin, host, expected) in &cases {
                for length in ["Content-Length: 99999999", "Transfer-Encoding: chunked"] {
                    let mut stream = TcpStream::connect(address).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_millis(700)))
                        .unwrap();
                    let mut request = format!(
                        "POST /api/v1/{route} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{length}\r\n"
                    );
                    if let Some(origin) = origin {
                        request += &format!("Origin: {origin}\r\n");
                    }
                    if let Some(token) = bearer_token {
                        request += &format!("Authorization: Bearer {token}\r\n");
                    }
                    request += "\r\n";
                    stream.write_all(request.as_bytes()).unwrap();
                    let mut status = String::new();
                    BufReader::new(stream)
                        .read_line(&mut status)
                        .expect("a refusal without reading the body");
                    assert_eq!(
                        status
                            .split_whitespace()
                            .nth(1)
                            .unwrap()
                            .parse::<u16>()
                            .unwrap(),
                        *expected,
                        "{route}: {status}"
                    );
                }
            }
        }
        let expected_audits = 4 * cases.len() * 2;
        let stderr = console.stderr();
        let lines: Vec<_> = stderr
            .lines()
            .filter(|line| line.starts_with("ui: "))
            .collect();
        assert_eq!(lines.len(), expected_audits, "{stderr}");
        assert!(
            lines.iter().any(|line| line.starts_with("ui: - ")),
            "{stderr}"
        );
        if enabled {
            assert!(
                lines
                    .iter()
                    .any(|line| line.starts_with(&format!("ui: {SUBJECT} "))),
                "{stderr}"
            );
        }
        for token in [&viewer, &operator, &nameless] {
            for part in token.split('.') {
                assert!(!stderr.contains(part), "token in audit: {stderr}");
            }
        }
    }
}

#[cfg(all(feature = "refresh-lock", feature = "postgres", feature = "github-app"))]
mod additions;
#[cfg(all(feature = "refresh-lock", feature = "postgres"))]
mod audit;
#[cfg(all(feature = "directory", feature = "refresh-lock"))]
mod deprovision;
#[cfg(feature = "directory")]
mod directory;
#[cfg(all(feature = "refresh-lock", feature = "postgres", feature = "github-app"))]
mod github;
#[cfg(all(feature = "directory", feature = "refresh-lock"))]
mod overage;
#[cfg(feature = "refresh-lock")]
mod team;
#[cfg(feature = "refresh-lock")]
mod writes;
