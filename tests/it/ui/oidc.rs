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
         \"{GATEWAY}\"\npath = \"../gateway\"\n\n[oidc]\nissuer = \"{issuer}\"\npublic_url = \
         \"{NODES}\"\nidentities_claim = \"meada_repositories\"\ngroups_claim = \"groups\"\n\
         scopes = [\"{SCOPE}\"]\n\n[oidc.groups]\n\"team-web\" = [\"{WEB}\"]\n\"team-gw\" = \
         [\"{GATEWAY}\"]\n\"team-all\" = [\"*\"]\n"
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
        json!({"subject": SUBJECT, "role": "viewer"})
    );

    let config = get(&console, "/api/v1/config");
    assert_eq!(config.status, 200);
    assert_eq!(
        config.json(),
        json!({
            "version": VERSION, "operations": false, "issuer": base,
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
    let (dir, console) = console("ui_oidc_grant", &issuer);
    let es256 = |claims: &Value| sign(&signing, "k1", Algorithm::ES256, claims);
    let web = es256(&claims(&base, &[WEB]));
    let gateway = es256(&edited(&base, |c| {
        c["meada_repositories"] = json!([]);
        c["groups"] = json!(["team-gw"]);
    }));
    let all = es256(&edited(&base, |c| c["meada_repositories"] = json!(["*"])));

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
    assert_eq!(manifest["modules"].as_array().unwrap().len(), 7);
    assert!(!manifest.to_string().contains("../gateway"));
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

    assert_leaks_nothing(&console.stderr(), &[&web, &gateway, &all]);
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
