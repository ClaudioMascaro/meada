//! A node with `[accounts]` and no `[oidc]` (#560, ADR 0024): `meada mcp --listen` admits the
//! agent tokens for the nodes the console signs, checked with the signing key's public half
//! alone against the console's `[ui] public_url`, and answers under the grant their groups map
//! to, as an identity provider's token is answered. Tokens are signed here with a key pair the
//! test generates; no service is needed.

use std::fs;
use std::path::{Path, PathBuf};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};

use super::{Answer, SERVED, WEB, answers, ask, golden, raw, trace, traced, web_only};
use crate::manifest::{MANIFEST, chain_manifest, run, run_args};
use crate::server::Server;

/// The console's `public_url`: the issuer of every agent token.
const CONSOLE: &str = "https://console.example";
const NODES: &str = "meada:nodes";

/// The chain, indexed, its manifest with `[postgres]`, the console's `[ui]` and `[accounts]` over
/// `ops/public.pem` (written now, the private half never), and `tail`; the dir and the private
/// half's signing key.
fn node_manifest(case: &str, tail: &str) -> (PathBuf, EncodingKey) {
    let dir = chain_manifest(case);
    assert!(run(&dir, "index").2);
    let signing = with_accounts(&dir, MANIFEST, tail);
    (dir, signing)
}

/// `[postgres]`, `[ui]` and `[accounts]` (a key pair generated now, its public half written beside
/// `manifest`, the private file named but never written) and `tail`, appended to `manifest` of
/// `dir`; the private half's signing key.
pub(crate) fn with_accounts(dir: &Path, manifest: &str, tail: &str) -> EncodingKey {
    let (private, public) = crate::ui::accounts::pair();
    node_section(dir, manifest, &public, CONSOLE, tail);
    EncodingKey::from_ed_pem(private.as_bytes()).unwrap()
}

/// `[postgres]`, `[ui]` with the console's `issuer` and `[accounts]` over `public` (a PEM written
/// beside `manifest`, the private file named but never written) and `tail`, appended to
/// `manifest` of `dir`.
pub(crate) fn node_section(dir: &Path, manifest: &str, public: &str, issuer: &str, tail: &str) {
    let file = dir.join(manifest);
    let base = file.parent().unwrap();
    fs::write(base.join("public.pem"), public).unwrap();
    let section = format!(
        "\n[postgres]\nurl_env = \"MEADA_TEST_NO_POSTGRES_560\"\n\n[ui]\ndir = \"../ui\"\n\
         public_url = \"{issuer}/\"\n\n[accounts]\npublic_key_file = \"public.pem\"\n\
         private_key_file = \"no-such-private.pem\"\n{tail}"
    );
    fs::write(&file, fs::read_to_string(&file).unwrap() + &section).unwrap();
}

/// `[accounts.groups]` granting the group `web` the repository `WEB`.
const GROUPS: &str = "\n[accounts.groups]\nweb = [\"example.com/chain/web\"]\n\
                      everything = [\"*\"]\n";

fn start(dir: &Path) -> Server {
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    Server::start_args(dir, &args, &[])
}

/// The claims of an agent token for the nodes of `CONSOLE`, of `groups`, valid five minutes.
fn claims(groups: &[&str]) -> Value {
    let now = jsonwebtoken::get_current_timestamp();
    json!({
        "iss": CONSOLE, "aud": NODES, "sub": "local:ana", "role": "viewer", "groups": groups,
        "jti": "0123456789abcdef0123456789abcdef", "iat": now, "exp": now + 300,
    })
}

fn sign(key: &EncodingKey, claims: &Value) -> String {
    jsonwebtoken::encode(&Header::new(Algorithm::EdDSA), claims, key).unwrap()
}

/// The tokens a node refuses with 401 `invalid_token`, each with its case.
fn refused_tokens(signing: &EncodingKey) -> Vec<(&'static str, String)> {
    let good = claims(&["web"]);
    let edit = |change: &dyn Fn(&mut Value)| {
        let mut claims = good.clone();
        change(&mut claims);
        sign(signing, &claims)
    };
    let (stranger, _) = crate::ui::accounts::pair();
    let stranger = EncodingKey::from_ed_pem(stranger.as_bytes()).unwrap();
    let now = jsonwebtoken::get_current_timestamp();
    let mut critical = Header::new(Algorithm::EdDSA);
    critical.crit = Some(vec!["exp".into()]);
    let (p256, _) = super::key("k1");
    let payload = sign(signing, &good).split('.').nth(1).unwrap().to_string();
    vec![
        ("malformed", "not-a-jwt".to_string()),
        ("wrong key", sign(&stranger, &good)),
        // A session token: the console's audience.
        (
            "session token",
            edit(&|c| c["aud"] = "meada:console".into()),
        ),
        (
            "proposal server's",
            edit(&|c| c["aud"] = "meada:proposals".into()),
        ),
        ("expired", edit(&|c| c["exp"] = (now - 600).into())),
        ("nbf ahead", edit(&|c| c["nbf"] = (now + 600).into())),
        (
            "wrong issuer",
            edit(&|c| c["iss"] = "https://other.example".into()),
        ),
        (
            "no exp",
            edit(&|c| _ = c.as_object_mut().unwrap().remove("exp")),
        ),
        ("not local", edit(&|c| c["sub"] = "ana".into())),
        (
            "change required",
            edit(&|c| c["change_required"] = true.into()),
        ),
        (
            "crit",
            jsonwebtoken::encode(&critical, &good, signing).unwrap(),
        ),
        ("ES256", super::sign(&p256, "k1", Algorithm::ES256, &good)),
        (
            "HS256",
            jsonwebtoken::encode(
                &Header::new(Algorithm::HS256),
                &good,
                &EncodingKey::from_secret(b"public"),
            )
            .unwrap(),
        ),
        // {"alg":"none","typ":"JWT"}
        (
            "alg none",
            format!("eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.{payload}."),
        ),
    ]
}

const MISSING: Option<&str> = Some("Bearer");
const INVALID: Option<&str> = Some("Bearer error=\"invalid_token\"");
const FORBIDDEN: Option<&str> = Some("Bearer error=\"insufficient_scope\"");

fn challenge(answer: &Answer) -> (u16, Option<&str>) {
    (answer.status, answer.challenge.as_deref())
}

/// The demo's node side: no token is a 401 `Bearer` at the endpoint, the root and an unknown
/// path, with no listing; every refused token (another key, a session token, another surface's,
/// expired, `nbf` ahead, another issuer, a non-EdDSA algorithm, `crit`) a 401 `invalid_token`
/// before routing, as are two `Authorization` headers; a token of the group `web` reads `WEB`
/// alone, no answer naming a peer, and gets a 403 on another repository; a token of a group
/// mapping to `*` reads the chain as the golden has it; the 404s list only what the token reads; a
/// browser's request is refused first, whatever its token; and no part of a token reaches stderr.
#[test]
fn an_agent_token_reads_what_its_groups_map_to_and_nothing_else_passes() {
    let (dir, signing) = node_manifest("accounts_node_grant", GROUPS);
    let server = start(&dir);
    let at = |path: &str| format!("{}/{path}", server.url);
    let mut sent = Vec::new();

    for path in [WEB, "", "example.com/chain/nope"] {
        let answer = ask(&at(path), None, &[], None);
        assert_eq!(challenge(&answer), (401, MISSING), "{path}");
        assert_eq!(answer.body, "unauthorized\n");
    }
    for (case, token) in refused_tokens(&signing) {
        let answer = ask(&at(WEB), Some(&token), &[], trace());
        assert_eq!(challenge(&answer), (401, INVALID), "{case}");
        assert_eq!(answer.body, "unauthorized\n", "{case}");
        sent.push(token);
    }
    let web = sign(&signing, &claims(&["web"]));
    let header = format!("Authorization: Bearer {web}").into_bytes();
    assert_eq!(
        raw(&at(WEB), &[&header, &header]),
        (401, INVALID.map(str::to_string))
    );
    sent.push(web.clone());

    for answer in answers(&at(WEB), &web) {
        web_only(&answer);
    }
    let other = ask(&at("example.com/chain/catalog"), Some(&web), &[], trace());
    assert_eq!(challenge(&other), (403, FORBIDDEN));
    assert_eq!(other.body, "forbidden\n");
    let all = sign(&signing, &claims(&["web", "everything"]));
    assert_eq!(traced(&ask(&at(WEB), Some(&all), &[], trace())), golden());
    sent.push(all.clone());
    let none = sign(&signing, &claims(&["nobody"]));
    sent.push(none.clone());
    for (token, listed) in [
        (&web, format!("{WEB}\n")),
        (&all, SERVED.into()),
        (&none, String::new()),
    ] {
        let answer = ask(&format!("{}/", server.url), Some(token), &[], None);
        assert_eq!((answer.status, answer.body), (404, listed));
    }
    assert_eq!(ask(&at(WEB), Some(&none), &[], trace()).status, 403);

    // The `Origin` 403 comes first, with or without a token.
    for token in [None, Some(all.as_str())] {
        let origin = [("Origin", &b"https://evil.example"[..])];
        let answer = ask(&at(WEB), token, &origin, trace());
        assert_eq!((answer.status, answer.challenge), (403, None));
    }

    let stderr = server.stderr();
    for token in &sent {
        for part in token.split('.').filter(|p| p.len() >= 8) {
            assert!(!stderr.contains(part), "a token part on stderr: {stderr}");
        }
    }
}

/// Without `[accounts.groups]`, an agent token reads every repository (ADR 0024, the single-team
/// proof of concept), the trace as the golden has it; a token is still needed.
#[test]
fn without_a_groups_table_an_agent_token_reads_everything() {
    let (dir, signing) = node_manifest("accounts_node_all", "");
    let server = start(&dir);
    let at = format!("{}/{WEB}", server.url);
    let token = sign(&signing, &claims(&[]));
    assert_eq!(traced(&ask(&at, Some(&token), &[], trace())), golden());
    let listing = ask(&format!("{}/", server.url), Some(&token), &[], None);
    assert_eq!((listing.status, listing.body.as_str()), (404, SERVED));
    assert_eq!(challenge(&ask(&at, None, &[], trace())), (401, MISSING));
}

/// A node with `[accounts]` and no `[oidc]` refuses to start, nothing listening, when its public
/// key is missing, unreadable or no Ed25519 key, naming the key and never a value, and when its
/// manifest has no `[ui]`, whose `public_url` is every token's issuer. It never reads the private
/// half, which is never written here.
#[test]
fn a_node_refuses_to_start_without_a_readable_public_key_or_the_issuer() {
    let (dir, _) = node_manifest("accounts_node_refused", "");
    let args = ["mcp", "--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    let refused = |expected: &str| {
        let (_, stderr, code) = run_args(&dir, &args);
        assert_eq!(code, Some(1), "{stderr}");
        assert!(!stderr.contains("listening on"), "{stderr}");
        assert!(stderr.contains(expected), "{stderr}");
        assert!(
            !stderr.contains("BEGIN") && !stderr.contains("private"),
            "{stderr}"
        );
    };
    let public = dir.join("ops/public.pem");
    let text = fs::read_to_string(&public).unwrap();
    fs::remove_file(&public).unwrap();
    refused("`accounts.public_key_file`: ");
    fs::write(
        &public,
        "-----BEGIN PUBLIC KEY-----\nbm90IGEga2V5\n-----END PUBLIC KEY-----\n",
    )
    .unwrap();
    refused("`accounts.public_key_file`: is not an Ed25519 public key in PEM");
    fs::write(&public, &text).unwrap();
    let file = dir.join(MANIFEST);
    let manifest = fs::read_to_string(&file).unwrap();
    let without_ui = manifest.replace(
        &format!("[ui]\ndir = \"../ui\"\npublic_url = \"{CONSOLE}/\"\n"),
        "",
    );
    assert_ne!(without_ui, manifest);
    fs::write(&file, &without_ui).unwrap();
    refused("`accounts`: a node admits the agent tokens the console issues");
    let missing = manifest.replace("public_key_file = \"public.pem\"\n", "");
    fs::write(&file, missing).unwrap();
    refused("names no public key of the signing key pair");
}

/// On a node serving generations with pins (a generation made by hand, the pins' `[valkey]` never
/// reached), a pin and a named workspace's route are admitted as their identity is: a 401 without
/// a token, a 403 for a pin outside the grant, the pin served to a token reading `WEB`, the 404 of
/// a generation not held only to a token reading the identity, a workspace id unknown the 404
/// listing nothing to it. A generation that stops being servable is the fixed 503 to an admitted
/// token, and the 401 without one.
#[cfg(feature = "generation-pointer")]
#[test]
fn pins_named_workspaces_and_the_503_are_admitted_as_their_identity_is() {
    let (dir, manifest, local) = crate::serving::hand_node("accounts_node_pins");
    let node = dir.join(&manifest);
    let pins = "\n[valkey]\nurl = \"redis://127.0.0.1:1\"\n\n[generations]\nkeep = 2\n";
    fs::write(&node, fs::read_to_string(&node).unwrap() + pins).unwrap();
    let signing = with_accounts(&dir, &manifest, GROUPS);
    let server = crate::serving::serve(&dir, &manifest);
    let pointer = fs::read_to_string(local.join("current")).unwrap();
    let generation = pointer.split(' ').next().unwrap().to_string();
    let at = |path: &str| format!("{}/{path}", server.url);
    let (web, catalog) = (
        sign(&signing, &claims(&["web"])),
        sign(&signing, &claims(&["nobody"])),
    );
    let pinned = format!("{WEB}@{generation}");
    assert_eq!(
        challenge(&ask(&at(&pinned), None, &[], trace())),
        (401, MISSING)
    );
    assert_eq!(
        challenge(&ask(&at(&pinned), Some(&catalog), &[], trace())),
        (403, FORBIDDEN)
    );
    for answer in answers(&at(&pinned), &web) {
        web_only(&answer);
    }
    let gone = format!("{WEB}@ffffffffffffffff");
    assert_eq!(challenge(&ask(&at(&gone), None, &[], None)), (401, MISSING));
    let refused = ask(&at(&gone), Some(&catalog), &[], None);
    assert_eq!(challenge(&refused), (403, FORBIDDEN));
    assert!(!refused.body.contains(&generation), "{}", refused.body);
    let answer = ask(&at(&gone), Some(&web), &[], None);
    assert_eq!(answer.status, 404);
    assert!(
        answer.body.contains("ffffffffffffffff is not held here"),
        "{}",
        answer.body
    );
    let workspace = format!("-/workspaces/0123456789abcdef/{WEB}");
    assert_eq!(
        challenge(&ask(&at(&workspace), None, &[], trace())),
        (401, MISSING)
    );
    let unknown = ask(&at(&workspace), Some(&web), &[], trace());
    assert_eq!((unknown.status, unknown.body.as_str()), (404, ""));

    // No servable generation: the 503 to an admitted token, the 401 before it without one.
    fs::write(local.join("current"), "00000000000000bb 0\n").unwrap();
    let answer = ask(&at(WEB), Some(&web), &[], None);
    assert_eq!(
        (answer.status, answer.body.as_str()),
        (503, "no servable generation\n")
    );
    assert_eq!(challenge(&ask(&at(WEB), None, &[], None)), (401, MISSING));
}
