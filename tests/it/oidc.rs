//! The OIDC module (ADR 0012, spec #259, feature `oidc`): `meada mcp --listen` over the chain
//! with `[oidc]`, against an issuer and a JWKS the test serves on loopback, signed with keys it
//! generates. Pinned paths need the generation pointer, so their test runs with
//! `generation-pointer` against the local services too.
#![cfg(feature = "oidc")]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};

use crate::manifest::{MANIFEST, chain_manifest, run, run_args};
use crate::server::Server;

const WEB: &str = "example.com/chain/web";
const PUBLIC: &str = "https://meada.example";
const METADATA: &str = "https://meada.example/.well-known/oauth-protected-resource";
const SERVED: &str = "example.com/chain/web\nexample.com/chain/gateway\nexample.com/chain/catalog\n\
                      example.com/chain/worker\n";

/// An issuer on loopback: OpenID discovery at its root and the JWKS it holds, which a test may
/// rotate, or set to `None` to answer 500.
struct Issuer {
    base: String,
    jwks: Arc<Mutex<Option<String>>>,
}

impl Issuer {
    fn new(jwks: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let jwks = Arc::new(Mutex::new(Some(jwks)));
        let (at, held) = (base.clone(), jwks.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    header.clear();
                }
                let (status, body) = match line.split(' ').nth(1).unwrap_or("") {
                    "/.well-known/openid-configuration" => (
                        200,
                        json!({"issuer": at, "jwks_uri": format!("{at}/jwks")}).to_string(),
                    ),
                    "/jwks" => match held.lock().unwrap().clone() {
                        Some(jwks) => (200, jwks),
                        None => (500, String::new()),
                    },
                    _ => (404, String::new()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self { base, jwks }
    }

    fn serve(&self, jwks: Option<String>) {
        *self.jwks.lock().unwrap() = jwks;
    }
}

/// A P-256 key generated now: the signing key and its public JWK, with `kid`.
fn key(kid: &str) -> (EncodingKey, Jwk) {
    let rng = ring::rand::SystemRandom::new();
    let alg = &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING;
    let pkcs8 = ring::signature::EcdsaKeyPair::generate_pkcs8(alg, &rng).unwrap();
    let signing = EncodingKey::from_ec_der(pkcs8.as_ref());
    let mut jwk = Jwk::from_encoding_key(&signing, Algorithm::ES256).unwrap();
    jwk.common.key_id = Some(kid.into());
    (signing, jwk)
}

fn set(keys: &[&Jwk]) -> String {
    json!({ "keys": keys }).to_string()
}

fn sign(key: &EncodingKey, kid: &str, alg: Algorithm, claims: &Value) -> String {
    let mut header = Header::new(alg);
    header.kid = Some(kid.into());
    jsonwebtoken::encode(&header, claims, key).unwrap()
}

/// The scope every server here requires.
const SCOPE: &str = "meada:read";

/// The claims of a token of `issuer` for the resource at `path`, granting `identities`, with
/// the required scope.
fn claims(issuer: &str, path: &str, identities: &[&str]) -> Value {
    json!({
        "iss": issuer, "aud": format!("{PUBLIC}/{path}"),
        "exp": jsonwebtoken::get_current_timestamp() + 300,
        "meada_repositories": identities, "scope": format!("openid {SCOPE}"),
    })
}

/// The chain's manifest, indexed, with an `[oidc]` section for `issuer`.
fn oidc_manifest(case: &str, issuer: &str) -> std::path::PathBuf {
    let dir = chain_manifest(case);
    assert!(run(&dir, "index").2);
    append(&dir, MANIFEST, issuer);
    dir
}

/// `[oidc]` for `issuer`, requiring `SCOPE`, appended to the manifest `manifest` of `dir`.
fn append(dir: &Path, manifest: &str, issuer: &str) {
    let file = dir.join(manifest);
    let section = format!(
        "\n[oidc]\nissuer = \"{issuer}\"\npublic_url = \"{PUBLIC}/\"\n\
         identities_claim = \"meada_repositories\"\nscopes = [\"{SCOPE}\"]\n"
    );
    fs::write(&file, fs::read_to_string(&file).unwrap() + &section).unwrap();
}

fn start(dir: &Path) -> Server {
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    Server::start_args(dir, &args, &[])
}

/// A tool and its arguments, `tools/call`ed alone.
type Call = Option<(&'static str, Value)>;

/// `trace` from `dashboard`, which crosses every link of the chain.
fn trace() -> Call {
    Some(("trace", json!({"from": "dashboard"})))
}

/// What `url` answers with `token` and `headers` (values as bytes, so not always UTF-8): its
/// status, `WWW-Authenticate` and body. With `call`, that tool call POSTed alone, else a GET.
fn ask(url: &str, token: Option<&str>, headers: &[(&str, &[u8])], call: Call) -> Answer {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let value = |bytes: &[u8]| ureq::http::HeaderValue::from_bytes(bytes).unwrap();
    let mut sent = Vec::new();
    if let Some(token) = token {
        sent.push(("Authorization", value(format!("Bearer {token}").as_bytes())));
    }
    sent.extend(headers.iter().map(|(k, v)| (*k, value(v))));
    let response = match call {
        Some((name, arguments)) => {
            let mut request = agent.post(url);
            for (name, value) in &sent {
                request = request.header(*name, value);
            }
            let message = json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": name, "arguments": arguments}
            });
            request
                .header("Accept", "application/json, text/event-stream")
                .header("Content-Type", "application/json")
                .send(message.to_string())
        }
        None => {
            let mut request = agent.get(url);
            for (name, value) in &sent {
                request = request.header(*name, value);
            }
            request.call()
        }
    };
    let mut response = response.unwrap();
    let challenge =
        (response.headers().get("www-authenticate")).map(|v| v.to_str().unwrap().to_string());
    let kind = (response.headers().get("content-type"))
        .map_or(String::new(), |v| v.to_str().unwrap().to_string());
    let body = response.body_mut().read_to_string().unwrap();
    Answer {
        status: response.status().as_u16(),
        challenge,
        kind,
        body,
    }
}

/// The status and `WWW-Authenticate` of a GET of `url` sending `headers` as raw lines, for the
/// shapes an HTTP client refuses to send (two `Authorization` headers, a value not in UTF-8).
fn raw(url: &str, headers: &[&[u8]]) -> (u16, Option<String>) {
    let (host, path) = url
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    let mut request =
        format!("GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n").into_bytes();
    for header in headers {
        request.extend_from_slice(header);
        request.extend_from_slice(b"\r\n");
    }
    request.extend_from_slice(b"\r\n");
    let mut stream = std::net::TcpStream::connect(host).unwrap();
    stream.write_all(&request).unwrap();
    let mut answer = Vec::new();
    std::io::Read::read_to_end(&mut stream, &mut answer).unwrap();
    let answer = String::from_utf8_lossy(&answer);
    let head = answer.split("\r\n\r\n").next().unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    let challenge = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.eq_ignore_ascii_case("www-authenticate")).then(|| value.trim().to_string())
    });
    (status, challenge)
}

#[derive(Debug)]
struct Answer {
    status: u16,
    challenge: Option<String>,
    kind: String,
    body: String,
}

/// The text of a tool result POSTed alone.
fn traced(answer: &Answer) -> String {
    assert_eq!(answer.status, 200, "{answer:?}");
    let json: Value = serde_json::from_str(&answer.body).unwrap();
    json["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

fn golden() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/chain.trace-down.txt"),
    )
    .unwrap()
}

/// The tool calls on `/WEB` that, with every repository granted, each reach a peer.
fn reaching() -> [Call; 3] {
    [
        trace(),
        Some(("search", json!({"query": "listOrders"}))),
        Some(("context", json!({"symbol": "src/orders.ts::listOrders"}))),
    ]
}

/// What `token` reads at `url` through each call of `reaching`.
fn answers(url: &str, token: &str) -> Vec<String> {
    (reaching().into_iter())
        .map(|call| traced(&ask(url, Some(token), &[], call)))
        .collect()
}

/// Fails when `answer` names a chain repository other than `WEB`, a path only a peer holds, or
/// a missing peer, or does not name `WEB` (an error would pass otherwise).
fn web_only(answer: &str) {
    assert!(answer.contains(WEB), "{answer}");
    for leak in [
        "example.com/chain/gateway",
        "example.com/chain/catalog",
        "example.com/chain/worker",
        "src/server.ts",
        "src/catalog.ts",
        "app/api/",
        "lib/products.ts",
        "src/jobs.ts",
        "missing peer",
    ] {
        assert!(!answer.contains(leak), "{leak} in: {answer}");
    }
}

/// The challenges at `path`: no token, a refused one, a token without the identity, and one
/// without the scope.
fn missing(path: &str) -> Option<String> {
    Some(format!(
        "Bearer resource_metadata=\"{METADATA}/{path}\", scope=\"{SCOPE}\""
    ))
}

fn invalid(path: &str) -> Option<String> {
    Some(format!(
        "Bearer error=\"invalid_token\", resource_metadata=\"{METADATA}/{path}\", \
         scope=\"{SCOPE}\""
    ))
}

fn forbidden(path: &str) -> Option<String> {
    Some(format!(
        "Bearer error=\"insufficient_scope\", resource_metadata=\"{METADATA}/{path}\""
    ))
}

fn unscoped(path: &str) -> Option<String> {
    Some(format!(
        "Bearer error=\"insufficient_scope\", resource_metadata=\"{METADATA}/{path}\", \
         scope=\"{SCOPE}\""
    ))
}

/// A token of `issuer` for `path`, granting `WEB`, without the required scope.
fn scopeless(issuer: &str, signing: &EncodingKey, path: &str) -> String {
    let mut claims = claims(issuer, path, &[WEB]);
    claims["scope"] = "openid".into();
    sign(signing, "k1", Algorithm::ES256, &claims)
}

/// The tokens a refusal test sends: each is refused with 401 `invalid_token` at `/<path>`.
fn refused_tokens(issuer: &str, signing: &EncodingKey, path: &str) -> Vec<(&'static str, String)> {
    let good = claims(issuer, path, &[WEB]);
    let edit = |change: &dyn Fn(&mut Value)| {
        let mut claims = good.clone();
        change(&mut claims);
        sign(signing, "k1", Algorithm::ES256, &claims)
    };
    let (stranger, _) = key("k1");
    let valid = edit(&|_| {});
    let payload = valid.split('.').nth(1).unwrap().to_string();
    let secret = EncodingKey::from_secret(b"k1");
    vec![
        ("malformed", "not-a-jwt".to_string()),
        (
            "bad signature",
            sign(&stranger, "k1", Algorithm::ES256, &good),
        ),
        ("expired", edit(&|c| c["exp"] = 1_000.into())),
        (
            "wrong issuer",
            edit(&|c| c["iss"] = "https://other.example".into()),
        ),
        (
            "wrong audience",
            edit(&|c| c["aud"] = format!("{PUBLIC}/other").into()),
        ),
        // {"alg":"none","typ":"JWT","kid":"k1"}
        (
            "alg none",
            format!("eyJhbGciOiJub25lIiwidHlwIjoiSldUIiwia2lkIjoiazEifQ.{payload}."),
        ),
        ("hmac", sign(&secret, "k1", Algorithm::HS256, &good)),
    ]
}

/// No token, a refused one, a token in the query string, a token for another repository and an
/// `Origin` get their answers at `/WEB`; the 404s need a token and list only what it may read;
/// a `*` token's `trace` equals the golden; both metadata documents answer without a token;
/// and no part of any token sent reaches stderr, the JWKS refresh failure included.
#[test]
fn a_bearer_token_admits_what_its_claim_lists_and_leaks_nowhere() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = oidc_manifest("oidc_current", &issuer.base);
    let server = start(&dir);
    let at = |path: &str| format!("{}/{path}", server.url);
    let mut sent = Vec::new();

    // The metadata of `/WEB` and of a pin of it, with no token.
    for path in [WEB.to_string(), format!("{WEB}@0123456789abcdef")] {
        let answer = ask(
            &at(&format!(".well-known/oauth-protected-resource/{path}")),
            None,
            &[],
            None,
        );
        assert_eq!(
            (answer.status, answer.kind.as_str()),
            (200, "application/json")
        );
        let document: Value = serde_json::from_str(&answer.body).unwrap();
        assert_eq!(
            document,
            json!({
                "resource": format!("{PUBLIC}/{path}"),
                "authorization_servers": [issuer.base],
                "bearer_methods_supported": ["header"],
                "scopes_supported": [SCOPE],
            })
        );
    }

    // No token, at the endpoint, the root and an unknown path: 401, no listing.
    let answer = ask(&at(WEB), None, &[], trace());
    assert_eq!((answer.status, answer.challenge), (401, missing(WEB)));
    let answer = ask(&format!("{}/", server.url), None, &[], None);
    assert_eq!(
        (answer.status, answer.challenge.as_deref()),
        (
            401,
            Some(format!("Bearer resource_metadata=\"{METADATA}\", scope=\"{SCOPE}\"").as_str())
        )
    );
    assert!(!answer.body.contains(WEB), "{}", answer.body);
    let answer = ask(&at("example.com/chain/nope"), None, &[], None);
    assert_eq!(answer.challenge, missing("example.com/chain/nope"));
    // A CR LF in the path stays encoded in the one challenge header.
    let answer = ask(&at("x%0D%0AX-Evil:%201"), None, &[], None);
    assert_eq!(answer.challenge, missing("x%0D%0AX-Evil:%201"));

    // A valid token in the query string is not read.
    let valid = sign(
        &signing,
        "k1",
        Algorithm::ES256,
        &claims(&issuer.base, WEB, &["*"]),
    );
    sent.push(valid.clone());
    let answer = ask(
        &format!("{}?access_token={valid}", at(WEB)),
        None,
        &[],
        trace(),
    );
    assert_eq!((answer.status, answer.challenge), (401, missing(WEB)));

    for (case, token) in refused_tokens(&issuer.base, &signing, WEB) {
        let answer = ask(&at(WEB), Some(&token), &[], trace());
        assert_eq!(
            (answer.status, answer.challenge),
            (401, invalid(WEB)),
            "{case}"
        );
        assert_eq!(answer.body, "unauthorized\n", "{case}");
        sent.push(token);
    }
    // Two `Authorization` headers, or one that is not UTF-8, carrying a valid token.
    let header = format!("Authorization: Bearer {valid}").into_bytes();
    let twice = raw(&at(WEB), &[&header, &header]);
    assert_eq!(twice, (401, invalid(WEB)));
    let mut bytes = header.clone();
    bytes.push(0xff);
    assert_eq!(raw(&at(WEB), &[&bytes]), (401, invalid(WEB)));
    // A valid token without the required scope.
    let token = scopeless(&issuer.base, &signing, WEB);
    let answer = ask(&at(WEB), Some(&token), &[], trace());
    assert_eq!((answer.status, answer.challenge), (403, unscoped(WEB)));
    sent.push(token);

    // A `*` token reads the whole chain, as the golden has it.
    assert_eq!(traced(&ask(&at(WEB), Some(&valid), &[], trace())), golden());
    let other = sign(
        &signing,
        "k1",
        Algorithm::ES256,
        &claims(&issuer.base, WEB, &["example.com/chain/catalog"]),
    );
    sent.push(other.clone());
    let answer = ask(&at(WEB), Some(&other), &[], trace());
    assert_eq!((answer.status, answer.challenge), (403, forbidden(WEB)));
    // A browser's request is refused whatever its token.
    let answer = ask(
        &at(WEB),
        Some(&valid),
        &[("Origin", &b"https://evil.example"[..])],
        trace(),
    );
    assert_eq!(answer.status, 403);

    // The 404s list only what the token may read: `/`'s token addresses the root.
    let root = |identities: &[&str]| {
        let mut claims = claims(&issuer.base, "", identities);
        claims["aud"] = PUBLIC.into();
        sign(&signing, "k1", Algorithm::ES256, &claims)
    };
    for (token, listed) in [
        (root(&[WEB, "example.com/chain/nope"]), format!("{WEB}\n")),
        (root(&["*"]), SERVED.to_string()),
        (root(&[]), String::new()),
    ] {
        let answer = ask(&format!("{}/", server.url), Some(&token), &[], None);
        assert_eq!(
            (answer.status, answer.body.as_str()),
            (404, listed.as_str())
        );
        sent.push(token);
    }
    let unknown = "example.com/chain/nope";
    let token = sign(
        &signing,
        "k1",
        Algorithm::ES256,
        &claims(&issuer.base, unknown, &[WEB]),
    );
    let answer = ask(&at(unknown), Some(&token), &[], None);
    assert_eq!((answer.status, answer.body), (404, format!("{WEB}\n")));
    sent.push(token);

    // An unknown `kid` while the JWKS fails: the cached keys stay, and stderr says so.
    issuer.serve(None);
    let (rotated, _) = key("k2");
    let token = sign(
        &rotated,
        "k2",
        Algorithm::ES256,
        &claims(&issuer.base, WEB, &[WEB]),
    );
    assert_eq!(
        ask(&at(WEB), Some(&token), &[], trace()).challenge,
        invalid(WEB)
    );
    sent.push(token);
    assert_eq!(traced(&ask(&at(WEB), Some(&valid), &[], trace())), golden());

    let stderr = server.stderr();
    assert!(stderr.contains("keeping the cached keys"), "{stderr}");
    for token in &sent {
        for part in token.split('.').filter(|p| p.len() >= 8) {
            assert!(!stderr.contains(part), "a token part on stderr: {stderr}");
        }
    }
}

/// A key the JWKS gains after the start signs a token Meada accepts: the unknown `kid`
/// fetches the JWKS again.
#[test]
fn a_rotated_key_is_picked_up_on_its_first_token() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = oidc_manifest("oidc_rotation", &issuer.base);
    let server = start(&dir);
    let (rotated, rotated_jwk) = key("k2");
    issuer.serve(Some(set(&[&jwk, &rotated_jwk])));
    let at = format!("{}/{WEB}", server.url);
    for (key, kid) in [(&rotated, "k2"), (&signing, "k1")] {
        let token = sign(
            key,
            kid,
            Algorithm::ES256,
            &claims(&issuer.base, WEB, &["*"]),
        );
        assert_eq!(
            traced(&ask(&at, Some(&token), &[], trace())),
            golden(),
            "{kid}"
        );
    }
}

/// Behind `/WEB`, a token reads only the repositories its claim lists (#290): listing all four,
/// each answer reaches a peer, the trace as the golden has it; granting `WEB` alone, no answer
/// names a peer, a path only a peer holds or a missing peer.
#[test]
fn a_grant_scopes_the_workspace_of_every_answer() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = oidc_manifest("oidc_grant", &issuer.base);
    let server = start(&dir);
    let at = format!("{}/{WEB}", server.url);
    let grant = |identities: &[&str]| {
        let claims = claims(&issuer.base, WEB, identities);
        sign(&signing, "k1", Algorithm::ES256, &claims)
    };
    let every: Vec<&str> = SERVED.lines().collect();
    let full = answers(&at, &grant(&every));
    assert_eq!(full[0], golden());
    for answer in &full[1..] {
        assert!(answer.contains("example.com/chain/gateway"), "{answer}");
    }
    for answer in answers(&at, &grant(&[WEB])) {
        web_only(&answer);
    }
}

/// The same grant over a generation made by hand (`--generations`, no store needed).
#[cfg(feature = "snapshot")]
#[test]
fn a_grant_scopes_the_workspace_of_a_generation() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let (dir, manifest, _) = crate::serving::hand_node("oidc_grant_generation");
    append(&dir, &manifest, &issuer.base);
    let server = crate::serving::serve(&dir, &manifest);
    let token = sign(
        &signing,
        "k1",
        Algorithm::ES256,
        &claims(&issuer.base, WEB, &[WEB]),
    );
    for answer in answers(&format!("{}/{WEB}", server.url), &token) {
        web_only(&answer);
    }
}

/// The group `append_groups` maps to `WEB`, and one it leaves unmapped.
const MAPPED: &str = "team-web-7c1d";
const UNMAPPED: &str = "team-none-5e2a";

/// `append`'s `[oidc]` with `groups_claim = "groups"` and `[oidc.groups]` mapping `MAPPED` to
/// `WEB`.
fn append_groups(dir: &Path, manifest: &str, issuer: &str) {
    append(dir, manifest, issuer);
    let file = dir.join(manifest);
    let groups = format!("groups_claim = \"groups\"\n[oidc.groups]\n\"{MAPPED}\" = [\"{WEB}\"]\n");
    fs::write(&file, fs::read_to_string(&file).unwrap() + &groups).unwrap();
}

/// A token carrying only groups (`MAPPED` and `UNMAPPED`, no `meada_repositories`) reads `/WEB`,
/// gets 403 on another repository and sees only `WEB` in the root's 404; stderr names neither
/// group.
fn grants_web_through_its_groups(server: Server, issuer: &str, signing: &EncodingKey) {
    let token = |path: &str| {
        let mut claims = claims(issuer, path, &[]);
        claims.as_object_mut().unwrap().remove("meada_repositories");
        claims["groups"] = json!([MAPPED, UNMAPPED]);
        if path.is_empty() {
            claims["aud"] = PUBLIC.into();
        }
        sign(signing, "k1", Algorithm::ES256, &claims)
    };
    let at = |path: &str| format!("{}/{path}", server.url);
    traced(&ask(&at(WEB), Some(&token(WEB)), &[], trace()));
    let catalog = "example.com/chain/catalog";
    let answer = ask(&at(catalog), Some(&token(catalog)), &[], trace());
    assert_eq!((answer.status, answer.challenge), (403, forbidden(catalog)));
    let answer = ask(&at(""), Some(&token("")), &[], None);
    assert_eq!((answer.status, answer.body), (404, format!("{WEB}\n")));
    let stderr = server.stderr();
    for group in [MAPPED, UNMAPPED] {
        assert!(!stderr.contains(group), "{group} on stderr: {stderr}");
    }
}

/// `[oidc.groups]` grants what a token's groups map to (#291).
#[test]
fn a_token_reads_what_its_groups_map_to() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = chain_manifest("oidc_groups");
    assert!(run(&dir, "index").2);
    append_groups(&dir, MANIFEST, &issuer.base);
    grants_web_through_its_groups(start(&dir), &issuer.base, &signing);
}

/// The same token gets the same grant on a generation made by hand (`--generations`).
#[cfg(feature = "snapshot")]
#[test]
fn a_token_reads_what_its_groups_map_to_on_a_generation() {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let (dir, manifest, _) = crate::serving::hand_node("oidc_groups_generation");
    append_groups(&dir, &manifest, &issuer.base);
    let server = crate::serving::serve(&dir, &manifest);
    grants_web_through_its_groups(server, &issuer.base, &signing);
}

/// A JWKS past its cap, or an issuer that does not answer, refuses the start: nothing listens.
#[test]
fn an_oversized_jwks_refuses_the_start() {
    let issuer = Issuer::new(format!(
        "{{\"keys\": [], \"pad\": \"{}\"}}",
        " ".repeat(256 << 10)
    ));
    let dir = oidc_manifest("oidc_oversized", &issuer.base);
    let args = ["mcp", "--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    let (_, stderr, code) = run_args(&dir, &args);
    assert_ne!(code, Some(0));
    assert!(!stderr.contains("listening on"), "{stderr}");
    assert!(
        stderr.contains("JWKS") && stderr.contains("limit"),
        "{stderr}"
    );
    // No issuer at all.
    let gone = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", gone.local_addr().unwrap());
    drop(gone);
    let dir = chain_manifest("oidc_no_issuer");
    assert!(run(&dir, "index").2);
    append(&dir, MANIFEST, &base);
    let (_, stderr, code) = run_args(&dir, &args);
    assert_ne!(code, Some(0));
    assert!(stderr.contains("metadata could not be read"), "{stderr}");
}

/// On a node with pins, `/<identity>@<generation>` takes the token of `/<identity>` and its
/// own, refuses the others as `/<identity>` does, serves its metadata, and its 404 for a
/// generation the node does not hold needs a token.
#[cfg(feature = "generation-pointer")]
#[test]
fn a_pinned_path_is_admitted_as_its_identity_is() {
    use crate::generations::Fleet;
    use crate::serving::serve;

    let Some(fleet) = Fleet::new("oidc_pins", 2) else {
        return;
    };
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let g1 = fleet.publish();
    let (node, _) = fleet.node("one");
    assert_eq!(fleet.pull(&node), g1);
    append(&fleet.dir, &node, &issuer.base);
    let server = serve(&fleet.dir, &node);
    let pinned = format!("{WEB}@{g1}");
    let at = |path: &str| format!("{}/{path}", server.url);

    let answer = ask(
        &at(&format!(".well-known/oauth-protected-resource/{pinned}")),
        None,
        &[],
        None,
    );
    let document: Value = serde_json::from_str(&answer.body).unwrap();
    assert_eq!(document["resource"], format!("{PUBLIC}/{pinned}"));
    assert_eq!(document["authorization_servers"], json!([issuer.base]));

    let answer = ask(&at(&pinned), None, &[], trace());
    assert_eq!((answer.status, answer.challenge), (401, missing(&pinned)));
    for (case, token) in refused_tokens(&issuer.base, &signing, WEB) {
        let answer = ask(&at(&pinned), Some(&token), &[], trace());
        assert_eq!(
            (answer.status, answer.challenge),
            (401, invalid(&pinned)),
            "{case}"
        );
    }
    // `/WEB`'s audience and the pin's own both read the pin.
    for audience in [WEB, pinned.as_str()] {
        let token = sign(
            &signing,
            "k1",
            Algorithm::ES256,
            &claims(&issuer.base, audience, &["*"]),
        );
        assert_eq!(
            traced(&ask(&at(&pinned), Some(&token), &[], trace())),
            golden(),
            "{audience}"
        );
    }
    // A token granting `WEB` alone reads none of the peers at the pin.
    let web = sign(
        &signing,
        "k1",
        Algorithm::ES256,
        &claims(&issuer.base, WEB, &[WEB]),
    );
    for answer in answers(&at(&pinned), &web) {
        web_only(&answer);
    }
    let other = claims(&issuer.base, WEB, &["example.com/chain/catalog"]);
    let other = sign(&signing, "k1", Algorithm::ES256, &other);
    let answer = ask(&at(&pinned), Some(&other), &[], trace());
    assert_eq!((answer.status, answer.challenge), (403, forbidden(&pinned)));
    let answer = ask(
        &at(&pinned),
        Some(&scopeless(&issuer.base, &signing, WEB)),
        &[],
        trace(),
    );
    assert_eq!((answer.status, answer.challenge), (403, unscoped(&pinned)));

    // A generation the node does not hold: 401 without a token, 403 with one that does not
    // grant the identity, the 404 with one that does.
    let gone = format!("{WEB}@ffffffffffffffff");
    let answer = ask(&at(&gone), None, &[], None);
    assert_eq!((answer.status, answer.challenge), (401, missing(&gone)));
    assert!(!answer.body.contains(&g1), "{}", answer.body);
    let answer = ask(&at(&gone), Some(&other), &[], None);
    assert_eq!((answer.status, answer.challenge), (403, forbidden(&gone)));
    assert!(!answer.body.contains(&g1), "{}", answer.body);
    let token = sign(
        &signing,
        "k1",
        Algorithm::ES256,
        &claims(&issuer.base, WEB, &[WEB]),
    );
    let answer = ask(&at(&gone), Some(&token), &[], None);
    assert_eq!(answer.status, 404);
    assert_eq!(
        answer.body,
        format!("generation ffffffffffffffff is not held here\ncurrent: {g1}\nheld: {g1}\n")
    );
    // An identity the pinned generation lacks lists only what the token may read.
    let none = format!("example.com/chain/none@{g1}");
    let token = sign(
        &signing,
        "k1",
        Algorithm::ES256,
        &claims(&issuer.base, &none, &[WEB]),
    );
    let answer = ask(&at(&none), Some(&token), &[], None);
    assert_eq!((answer.status, answer.body), (404, format!("{WEB}\n")));
}
