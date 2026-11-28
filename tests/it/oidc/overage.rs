use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use jsonwebtoken::{Algorithm, EncodingKey};
use serde_json::{Value, json};

use super::{Issuer, PUBLIC, SCOPE, SERVED, WEB, ask, key, set, sign, trace};
use crate::manifest::{MANIFEST, chain_manifest, run, run_args};
use crate::postgres::{Admin, Audited, Relay, audited};
use crate::server::Server;
use crate::ui::{console_dir, send_body, ui_section};

const GATEWAY: &str = "example.com/chain/gateway";
const WORKER: &str = "example.com/chain/worker";
const ANA: &str = "5d0a8c1e-0000-4000-8000-00000000a001";
const BIA: &str = "5d0a8c1e-0000-4000-8000-00000000a002";
const CAIO: &str = "5d0a8c1e-0000-4000-8000-00000000a003";
const NOBODY: &str = "5d0a8c1e-0000-4000-8000-00000000a0ff";
const UNSEEN: &str = "5d0a8c1e-0000-4000-8000-00000000a0fe";
const MAPPED: &str = "7e2b9d3f-0000-4000-8000-0000000000f1";
const UNMAPPED: &str = "7e2b9d3f-0000-4000-8000-0000000000f2";
const SCIM_TOKEN: &str = "scim-336-token-0123456789abcdefghijklmno";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const TTL_MS: &str = "3000";

struct Case {
    audit: Audited,
    console: Server,
    issuer: Issuer,
    signing: EncodingKey,
    node: PathBuf,
    relay: Relay,
}

fn case(name: &str, user: (&str, &str), group_attribute: &str) -> Option<Case> {
    let (signing, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let oidc = format!(
        "\n[oidc]\nissuer = \"{}\"\npublic_url = \"{PUBLIC}/\"\naudience = \"{PUBLIC}\"\n\
         identities_claim = \"meada_repositories\"\nscopes = [\"{SCOPE}\"]\n",
        issuer.base
    );
    let ui = format!("{}client_id = \"meada-console\"\n", ui_section());
    let dir = console_dir(&format!("{name}_console"), Some(VERSION), &oidc, &ui);
    let audit = audited(&dir, &format!("{name}_console"))?;
    let (claim, attribute) = user;
    let directory = format!(
        "\n[directory]\nuser_claim = \"{claim}\"\nuser_attribute = \"{attribute}\"\n\
         group_attribute = \"{group_attribute}\"\n"
    );
    let file = dir.join(MANIFEST);
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{text}{}{directory}", audit.section)).unwrap();
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    let env = [("MEADA_SCIM_TOKEN", SCIM_TOKEN)];
    let console = Server::start_command(&dir, "ui", &args, &env);

    let node = chain_manifest(&format!("{name}_node"));
    assert!(run(&node, "index").2);
    let relay = Relay::new(&audit.url);
    fs::write(node.join("ops/pg-url"), &relay.url).unwrap();
    let node_oidc = format!(
        "{oidc}type = \"entra\"\ngroups_claim = \"groups\"\n\n[oidc.groups]\n\
         \"{MAPPED}\" = [\"{WEB}\"]\n\"display\" = [\"{GATEWAY}\"]\n\n\
         [postgres]\nurl_file = \"pg-url\"\nschema = \"{}\"\n{directory}",
        audit.schema
    );
    let file = node.join(MANIFEST);
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{text}{node_oidc}")).unwrap();
    Some(Case {
        audit,
        console,
        issuer,
        signing,
        node,
        relay,
    })
}

fn entra(name: &str) -> Option<Case> {
    case(name, ("oid", "externalId"), "externalId")
}

impl Case {
    fn manifest(&self, overage: &str) -> String {
        let file = self.node.join(MANIFEST);
        let text = fs::read_to_string(file).unwrap();
        let name = format!("ops/{overage}.toml");
        let table = format!("{text}\n[oidc.entra]\noverage = \"{overage}\"\n");
        fs::write(self.node.join(&name), table).unwrap();
        name
    }

    fn start(&self, overage: &str, env: &[(&str, &str)]) -> Server {
        let manifest = self.manifest(overage);
        let args = ["--listen", "127.0.0.1:0", "--workspace", manifest.as_str()];
        let mut env = env.to_vec();
        env.push(("MEADA_OVERAGE_TTL_MS", TTL_MS));
        Server::start_args(&self.node, &args, &env)
    }

    fn token(&self, oid: &str, edit: impl FnOnce(&mut Value)) -> String {
        let mut claims = json!({
            "iss": self.issuer.base, "aud": PUBLIC, "oid": oid, "preferred_username": oid,
            "exp": jsonwebtoken::get_current_timestamp() + 600,
            "scope": format!("openid {SCOPE}"), "_claim_names": {"groups": "src1"},
        });
        edit(&mut claims);
        sign(&self.signing, "k1", Algorithm::ES256, &claims)
    }

    fn machine(&self) -> String {
        self.token(NOBODY, |c| {
            c["meada_repositories"] = json!(["*"]);
            c.as_object_mut().unwrap().remove("_claim_names");
        })
    }

    fn scim(&self, method: &str, path: &str, body: Option<Value>, status: u16) -> Value {
        let authorization = format!("Bearer {SCIM_TOKEN}");
        let mut headers = vec![("Authorization", authorization.as_str())];
        if body.is_some() {
            headers.push(("Content-Type", "application/scim+json"));
        }
        let body = body.map_or(String::new(), |b| b.to_string());
        let reply = send_body(
            &self.console,
            method,
            &format!("/scim/v2{path}"),
            &headers,
            &body,
        );
        assert_eq!(reply.status, status, "{method} {path}: {}", reply.body);
        match reply.body.is_empty() {
            true => Value::Null,
            false => reply.json(),
        }
    }

    fn user(&self, name: &str, oid: &str, active: bool) -> String {
        let schema = "urn:ietf:params:scim:schemas:core:2.0:User";
        let body = json!({"schemas": [schema], "userName": name, "externalId": oid,
                          "active": active});
        let created = self.scim("POST", "/Users", Some(body), 201);
        created["id"].as_str().unwrap().to_string()
    }

    fn group(&self, display: &str, external: &str, members: &[&str]) -> String {
        let schema = "urn:ietf:params:scim:schemas:core:2.0:Group";
        let members: Vec<Value> = members.iter().map(|m| json!({"value": m})).collect();
        let body = json!({"schemas": [schema], "displayName": display, "externalId": external,
                          "members": members});
        let created = self.scim("POST", "/Groups", Some(body), 201);
        created["id"].as_str().unwrap().to_string()
    }

    fn patch(&self, resource: &str, id: &str, operations: Value) {
        let schema = "urn:ietf:params:scim:api:messages:2.0:PatchOp";
        let body = json!({"schemas": [schema], "Operations": operations});
        let status = if resource == "Users" { 200 } else { 204 };
        self.scim("PATCH", &format!("/{resource}/{id}"), Some(body), status);
    }

    fn deactivate(&self, user: &str) {
        let off = json!([{"op": "Replace", "path": "active", "value": false}]);
        self.patch("Users", user, off);
    }

    fn join(&self, group: &str, user: &str) {
        let add = json!([{"op": "add", "path": "members", "value": [{"value": user}]}]);
        self.patch("Groups", group, add);
    }

    fn leave(&self, group: &str, user: &str) {
        let path = format!("members[value eq \"{user}\"]");
        self.patch("Groups", group, json!([{"op": "remove", "path": path}]));
    }

    fn clean(&self, stderr: &str, tokens: &[String]) {
        for token in tokens {
            for part in token.split('.') {
                assert!(!stderr.contains(part), "a token part on stderr: {stderr}");
            }
        }
        for value in [ANA, BIA, CAIO, NOBODY, UNSEEN, MAPPED, UNMAPPED, SCIM_TOKEN] {
            assert!(!stderr.contains(value), "{value} on stderr: {stderr}");
        }
        self.audit.assert_no_url(stderr);
    }
}

fn listed(server: &Server, token: &str) -> String {
    let answer = ask(&format!("{}/", server.url), Some(token), &[], None);
    assert_eq!(answer.status, 404, "{answer:?}");
    answer.body
}

fn lines(identities: &[&str]) -> String {
    (SERVED.lines())
        .filter(|i| identities.contains(i))
        .map(|i| format!("{i}\n"))
        .collect()
}

fn until(server: &Server, token: &str, expected: &str, within: Duration) -> Duration {
    let start = Instant::now();
    loop {
        let found = listed(server, token);
        if found == expected {
            return start.elapsed();
        }
        assert!(
            start.elapsed() < within,
            "still {found:?}, not {expected:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn overage_lines(stderr: &str) -> Vec<&str> {
    let unreadable = "meada: the directory cannot be read for a token in overage (";
    stderr
        .lines()
        .filter(|l| l.starts_with(unreadable))
        .collect()
}

#[test]
fn an_overage_token_reads_what_its_directory_groups_map_to() {
    let Some(case) = entra("overage_reads") else {
        return;
    };
    let ana = case.user("ana", ANA, true);
    case.group("display", MAPPED, &[&ana]);
    case.group("other", UNMAPPED, &[&ana]);
    let (directory, ignore) = (case.start("directory", &[]), case.start("ignore", &[]));
    let token = case.token(ANA, |_| {});
    assert_eq!(listed(&directory, &token), lines(&[WEB]));
    assert_eq!(listed(&ignore, &token), "");
    let traced = ask(
        &format!("{}/{WEB}", directory.url),
        Some(&token),
        &[],
        trace(),
    );
    assert_eq!(traced.status, 200, "{}", traced.body);
    let refused = ask(
        &format!("{}/{WORKER}", directory.url),
        Some(&token),
        &[],
        trace(),
    );
    assert_eq!(refused.status, 403);
    let refused = ask(&format!("{}/{WEB}", ignore.url), Some(&token), &[], trace());
    assert_eq!(refused.status, 403);
    let both = case.token(ANA, |c| c["meada_repositories"] = json!([WORKER]));
    assert_eq!(listed(&directory, &both), lines(&[WEB, WORKER]));
    assert_eq!(listed(&ignore, &both), lines(&[WORKER]));
    let upper = case.token(&ANA.to_uppercase(), |_| {});
    assert_eq!(listed(&directory, &upper), "");
    let machine = case.machine();
    assert_eq!(listed(&directory, &machine), SERVED);
    let sent = [token, both, upper, machine];
    case.clean(&directory.stderr(), &sent);
    case.clean(&ignore.stderr(), &sent);
}

#[test]
fn a_user_without_groups_and_a_token_that_is_no_overage_read_nothing_through_groups() {
    let Some(case) = entra("overage_none") else {
        return;
    };
    let ana = case.user("ana", ANA, true);
    let bia = case.user("bia", BIA, true);
    case.user("caio", CAIO, true);
    let group = case.group("display", MAPPED, &[&ana, &bia]);
    case.deactivate(&bia);
    let node = case.start("directory", &[]);
    for (who, oid) in [("inactive", BIA), ("no group", CAIO), ("not found", NOBODY)] {
        assert_eq!(listed(&node, &case.token(oid, |_| {})), "", "{who}");
    }
    assert_eq!(listed(&node, &case.token(ANA, |_| {})), lines(&[WEB]));
    let _ = group;

    case.relay.close();
    let mut sent = Vec::new();
    for edit in [
        json!({"groups": ["other"]}),
        json!({"groups": []}),
        json!({"_claim_names": "groups"}),
        json!({"_claim_names": ["groups"]}),
        json!({"_claim_names": {"groups": 1}}),
        json!({"_claim_names": {"groups": ["src1"]}}),
        json!({"_claim_names": {"roles": "src1"}}),
        json!({"_claim_names": null}),
    ] {
        let token = case.token(CAIO, |c| {
            for (key, value) in edit.as_object().unwrap() {
                c[key] = value.clone();
            }
        });
        assert_eq!(listed(&node, &token), "", "{edit}");
        sent.push(token);
    }
    let fresh = case.token(UNSEEN, |_| {});
    assert_eq!(listed(&node, &fresh), "");
    sent.push(fresh);
    let stderr = node.stderr();
    assert_eq!(overage_lines(&stderr).len(), 1, "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    case.clean(&stderr, &sent);
}

#[test]
fn an_answer_is_cached_for_the_ttl_and_no_longer() {
    let Some(case) = entra("overage_ttl") else {
        return;
    };
    let ana = case.user("ana", ANA, true);
    let caio = case.user("caio", CAIO, true);
    let group = case.group("display", MAPPED, &[&ana]);
    let node = case.start("directory", &[]);
    let (ana_token, caio_token) = (case.token(ANA, |_| {}), case.token(CAIO, |_| {}));
    let asked = Instant::now();
    assert_eq!(listed(&node, &ana_token), lines(&[WEB]));
    assert_eq!(listed(&node, &caio_token), "");
    case.leave(&group, &ana);
    case.join(&group, &caio);
    assert!(
        asked.elapsed() < Duration::from_millis(2500),
        "too slow to tell"
    );
    assert_eq!(listed(&node, &ana_token), lines(&[WEB]));
    assert_eq!(listed(&node, &caio_token), "");
    until(&node, &ana_token, "", Duration::from_secs(10));
    until(&node, &caio_token, &lines(&[WEB]), Duration::from_secs(10));
    assert!(
        asked.elapsed() >= Duration::from_millis(2900),
        "{:?}",
        asked.elapsed()
    );
    case.clean(&node.stderr(), &[ana_token, caio_token]);
}

#[test]
fn with_postgres_stopped_a_cached_subject_keeps_its_groups_until_expiry() {
    let Some(case) = entra("overage_down") else {
        return;
    };
    let ana = case.user("ana", ANA, true);
    case.user("bia", BIA, true);
    case.group("display", MAPPED, &[&ana]);
    let status = ["status", "--workspace", MANIFEST];
    let ready = run_args(&case.node, &status).2;
    let node = case.start("directory", &[]);
    let (ana_token, bia_token) = (case.token(ANA, |_| {}), case.token(BIA, |_| {}));
    assert_eq!(listed(&node, &ana_token), lines(&[WEB]));
    case.relay.close();
    let down = Instant::now();
    assert_eq!(listed(&node, &ana_token), lines(&[WEB]));
    assert_eq!(listed(&node, &bia_token), "");
    let machine = case.machine();
    assert_eq!(listed(&node, &machine), SERVED);
    until(&node, &ana_token, "", Duration::from_secs(10));
    assert!(
        down.elapsed() < Duration::from_secs(4),
        "{:?}",
        down.elapsed()
    );
    assert_eq!(run_args(&case.node, &status).2, ready, "readiness changed");
    assert_eq!(listed(&node, &machine), SERVED);
    let stderr = node.stderr();
    assert_eq!(overage_lines(&stderr).len(), 2, "{stderr}");
    assert_eq!(stderr.lines().count(), 2, "{stderr}");
    case.clean(&stderr, &[ana_token.clone(), bia_token.clone(), machine]);

    let admin = Admin::new(&case.audit.url);
    let schema = &case.audit.schema;
    admin.run(&format!(
        "UPDATE \"{schema}\".schema_version SET version = version + 1"
    ));
    let relay = Relay::new(&case.audit.url);
    fs::write(case.node.join("ops/pg-url"), &relay.url).unwrap();
    let node = case.start("directory", &[]);
    assert_eq!(listed(&node, &ana_token), "");
    let stderr = node.stderr();
    let lines: Vec<&str> = overage_lines(&stderr);
    assert_eq!(lines.len(), 1, "{stderr}");
    assert!(
        lines[0].contains(&format!(
            "is at version {}, above this Meada's {}",
            crate::common::SCHEMA_VERSION + 1,
            crate::common::SCHEMA_VERSION
        )),
        "{stderr}"
    );
    case.clean(&stderr, &[ana_token, bia_token]);
}

#[test]
fn a_group_value_that_fails_the_owner_name_rule_is_skipped() {
    let Some(case) = entra("overage_hostile") else {
        return;
    };
    let ana = case.user("ana", ANA, true);
    let padded = case.group("padded", "padded", &[&ana]);
    let long = case.group("long", "long", &[&ana]);
    case.group("display", MAPPED, &[&ana]);
    let admin = Admin::new(&case.audit.url);
    let schema = &case.audit.schema;
    let update = |id: &str, value: &str| {
        let set = format!("UPDATE \"{schema}\".directory_groups SET external_id = '{value}'");
        admin.run(&format!("{set} WHERE id = '{id}'"));
    };
    update(&padded, " padded ");
    update(&long, &"x".repeat(257));
    let node = case.start("directory", &[]);
    let token = case.token(ANA, |_| {});
    assert_eq!(listed(&node, &token), lines(&[WEB]));
    let stderr = node.stderr();
    let skipped = format!("\"{schema}\".directory_groups: a row fails its checks and is skipped");
    assert_eq!(
        stderr.lines().filter(|l| *l == skipped).count(),
        2,
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 2, "{stderr}");
    case.clean(&stderr, &[token]);
}

#[test]
fn a_user_name_is_matched_ignoring_case_and_a_group_by_its_display_name() {
    let Some(case) = case(
        "overage_names",
        ("preferred_username", "userName"),
        "displayName",
    ) else {
        return;
    };
    let ana = case.user("Ana", ANA, true);
    case.group("display", "ignored-external-id", &[&ana]);
    case.group(MAPPED, MAPPED, &[]);
    let node = case.start("directory", &[]);
    for claim in ["ana", "ANA", "Ana"] {
        let token = case.token(ANA, |c| c["preferred_username"] = json!(claim));
        assert_eq!(listed(&node, &token), lines(&[GATEWAY]), "{claim}");
    }
    let other = case.token(ANA, |c| c["preferred_username"] = json!("anaa"));
    assert_eq!(listed(&node, &other), "");
    case.clean(&node.stderr(), &[other]);
}
