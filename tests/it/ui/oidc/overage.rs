//! Entra ID's groups overage through the directory in the console (#336, ADR 0016): a token with
//! `_claim_names` and no `groups` gets its groups from the directory the identity provider
//! pushed, and is granted, made an operator or a team operator and shown its groups like any
//! other; the token check resolves a pasted one the same way. The team fixture of `team` (real
//! Valkey and store, the `oidc` module's loopback issuer) with a local Postgres in a schema of
//! each test's own, fed through the SCIM endpoint as `directory`'s tests feed it. Without
//! `MEADA_TEST_POSTGRES_URL` they print why and skip.

use std::fs;

use serde_json::{Value, json};

use super::directory::{GROUP, TOKEN, USER, expect, patch};
use super::team::{BILLING, ENTRIES, INVOICES, JOINT, LEDGER, SEARCH, SHARED, Team, team};
use super::{PUBLIC, ask, bearer};
use crate::postgres::{Audited, audited};
use crate::server::Server;
use crate::snapshot::KEY_ID;
use crate::ui::{Reply, send_body};

/// The `[directory]` of every console here: Entra's recipe, the object id as `externalId`.
const DIRECTORY: &str = "\n[directory]\nuser_claim = \"oid\"\nuser_attribute = \"externalId\"\ngroup_attribute = \"externalId\"\n";
const NODES: &str = "https://meada.example";
const ANA: &str = "5d0a8c1e-0000-4000-8000-00000000a001";
const OPA: &str = "5d0a8c1e-0000-4000-8000-00000000a002";
const NOB: &str = "5d0a8c1e-0000-4000-8000-00000000a003";

/// The consoles of one case: the first reads the directory for overage, the second has
/// `overage = "ignore"`; both share the schema, the store and the issuer.
struct Overage {
    team: Team,
    audit: Audited,
    directory: Server,
    ignore: Server,
}

/// The team fixture whose console manifest is `type = "entra"`, has `operator_groups` naming
/// `ops-group`, `[postgres]` and `[directory]`, started twice: with `overage = "directory"` and
/// with `"ignore"`, `ttl_ms` shortening the answers' lifetime when given.
fn overaged(case: &'static str, ttl_ms: Option<&str>) -> Option<Overage> {
    let team = team(case, true)?;
    let audit = audited(&team.fleet.dir, case)?;
    let file = team.fleet.dir.join("ops/console.toml");
    let text = fs::read_to_string(&file).unwrap();
    let text = text.replacen("\n[oidc]\n", "\n[oidc]\ntype = \"entra\"\n", 1);
    // `operator_groups` and `operator_claim` exclude each other.
    let key = "operator_claim = \"operator\"\n";
    let text = text.replacen(key, "operator_groups = [\"ops-group\"]\n", 1);
    assert!(
        text.contains("type = \"entra\"") && text.contains("ops-group"),
        "{text}"
    );
    let base = format!("{text}{}{DIRECTORY}", audit.section);
    let start = |overage: &str, name: &str| {
        let manifest = format!("ops/{name}.toml");
        let sections = format!("{base}\n[oidc.entra]\noverage = \"{overage}\"\n");
        fs::write(team.fleet.dir.join(&manifest), sections).unwrap();
        let args = ["--listen", "127.0.0.1:0", "--workspace", manifest.as_str()];
        let mut env = vec![("MEADA_TEST_S3_KEY", KEY_ID), ("MEADA_SCIM_TOKEN", TOKEN)];
        env.extend(ttl_ms.map(|ms| ("MEADA_OVERAGE_TTL_MS", ms)));
        Server::start_command(&team.fleet.dir, "ui", &args, &env)
    };
    let directory = start("directory", "overage-directory");
    let ignore = start("ignore", "overage-ignore");
    Some(Overage {
        team,
        audit,
        directory,
        ignore,
    })
}

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"].as_str().unwrap_or("").into()
}

/// A user `name` whose `externalId` is `oid`; its id.
fn user(console: &Server, name: &str, oid: &str, active: bool) -> String {
    let body = json!({"schemas": [USER], "userName": name, "externalId": oid, "active": active});
    let created = expect(console, "POST", "/Users", Some(&body), 201);
    created["id"].as_str().unwrap().to_string()
}

/// A group `external` (what a token's `groups` would carry) with `members`; its id.
fn group(console: &Server, external: &str, members: &[&str]) -> String {
    let members: Vec<Value> = members.iter().map(|id| json!({"value": id})).collect();
    let body = json!({"schemas": [GROUP], "displayName": format!("display of {external}"),
                      "externalId": external, "members": members});
    let created = expect(console, "POST", "/Groups", Some(&body), 201);
    created["id"].as_str().unwrap().to_string()
}

fn activate(console: &Server, id: &str, active: bool) {
    let set = patch(json!([{"op": "Replace", "path": "active", "value": active}]));
    expect(console, "PATCH", &format!("/Users/{id}"), Some(&set), 200);
}

/// A token of `oid`, with `_claim_names` and no `groups`, as Entra issues one in overage.
fn overage_token(team: &Team, subject: &str, oid: &str) -> String {
    team.sign(&overage_claims(team, subject, oid))
}

fn overage_claims(team: &Team, subject: &str, oid: &str) -> Value {
    let mut claims = team.claims(subject);
    claims["oid"] = json!(oid);
    claims["_claim_names"] = json!({"groups": "src1"});
    claims
}

fn session(console: &Server, token: &str) -> Value {
    let reply = ask(console, "/api/v1/session", token);
    assert_eq!(reply.status, 200, "{}", reply.body);
    reply.json()
}

fn post(console: &Server, route: &str, token: &str, body: Value) -> Reply {
    let headers = [("Authorization", bearer(token)), ("Origin", PUBLIC.into())];
    let headers: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
    let route = format!("/api/v1/{route}");
    send_body(console, "POST", &route, &headers, &body.to_string())
}

/// Neither `text` nor any of `values` there: tokens' parts, the SCIM token, the Postgres URL.
fn assert_leaks_nothing(audit: &Audited, text: &str, tokens: &[&str], values: &[&str]) {
    for token in tokens {
        for part in token.split('.') {
            assert!(!text.contains(part), "a token part in {text}");
        }
    }
    assert!(!text.contains(TOKEN), "the SCIM token in {text}");
    for value in values {
        assert!(!text.contains(value), "{value} in {text}");
    }
    audit.assert_no_url(text);
}

/// A token in overage is granted through the directory's groups, made `operator` through
/// `operator_groups` and a team operator through a group owning an entry it reads, and its
/// `session` lists the groups; a user in no group is a viewer with none; under `overage =
/// "ignore"` the same tokens have no groups. No line or body carries a token, an object id, a
/// group or the URL.
#[test]
fn an_overage_token_is_granted_and_made_an_operator_through_directory_groups() {
    let Some(case) = overaged("ui_overage_roles", None) else {
        return;
    };
    let Overage {
        team,
        audit,
        directory: console,
        ignore: console_ignore,
    } = case;
    let (directory, ignore) = (&console, &console_ignore);
    let ana = user(directory, "ana", ANA, true);
    let opa = user(directory, "opa", OPA, true);
    user(directory, "nob", NOB, true);
    group(directory, "team-billing", &[&ana]);
    group(directory, "ops-group", &[&opa]);
    group(directory, "team-empty", &[]);
    let tokens = [
        overage_token(&team, "ana", ANA),
        overage_token(&team, "opa", OPA),
        overage_token(&team, "nob", NOB),
    ];
    let [ana_t, opa_t, nob_t] = &tokens;
    let sessions = |token: &str| session(directory, token);
    assert_eq!(
        sessions(ana_t),
        json!({"subject": "ana", "role": "team_operator", "groups": ["team-billing"],
               "grant": [BILLING, INVOICES, JOINT, LEDGER, SHARED],
               "teams": ["team-billing"]})
    );
    let operator = sessions(opa_t);
    assert_eq!(operator["role"], "operator", "{operator}");
    assert_eq!(operator["groups"], json!(["ops-group"]));
    let viewer = sessions(nob_t);
    assert_eq!(
        (&viewer["role"], &viewer["groups"], &viewer["grant"]),
        (&json!("viewer"), &json!([]), &json!([]))
    );
    // Under `ignore` they have no directory-derived groups: viewers, whatever the directory holds.
    for token in &tokens {
        let seen = session(ignore, token);
        assert_eq!(seen["role"], "viewer", "{seen}");
        assert_eq!((&seen["groups"], &seen["grant"]), (&json!([]), &json!([])));
        assert!(seen.get("teams").is_none(), "{seen}");
    }
    let values = [ANA, OPA, NOB, "team-billing", "ops-group", "team-empty"];
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    for console in [console, console_ignore] {
        assert_leaks_nothing(&audit, &console.stderr(), &refs, &values);
    }
}

/// A token that is no overage, or whose `_claim_names` is no object or maps to a non-string, or
/// which has `groups`, is never looked up: it keeps what its claims say, whatever the
/// directory holds for its `oid`.
#[test]
fn a_token_with_groups_or_an_odd_claim_names_is_no_overage() {
    let Some(case) = overaged("ui_overage_shapes", None) else {
        return;
    };
    let Overage {
        team, directory, ..
    } = &case;
    let ana = user(directory, "ana", ANA, true);
    group(directory, "team-billing", &[&ana]);
    let shapes: Vec<(&str, Value)> = vec![
        ("groups present", json!({"groups": ["eng"]})),
        ("empty groups", json!({"groups": []})),
        ("names a string", json!({"_claim_names": "groups"})),
        ("names an array", json!({"_claim_names": ["groups"]})),
        ("names a number", json!({"_claim_names": {"groups": 1}})),
        (
            "names an array source",
            json!({"_claim_names": {"groups": ["s"]}}),
        ),
        (
            "names another claim",
            json!({"_claim_names": {"roles": "s"}}),
        ),
    ];
    for (name, edit) in shapes {
        let mut claims = team.claims("ana");
        claims["oid"] = json!(ANA);
        for (key, value) in edit.as_object().unwrap() {
            claims[key] = value.clone();
        }
        let seen = session(directory, &team.sign(&claims));
        let groups = claims["groups"].as_array().cloned().unwrap_or_default();
        assert_eq!(seen["groups"], json!(groups), "{name}: {seen}");
        assert_eq!(seen["role"], "viewer", "{name}: {seen}");
    }
    // The control: the same user, in overage, is a team operator.
    let seen = session(directory, &overage_token(team, "ana", ANA));
    assert_eq!(seen["role"], "team_operator", "{seen}");
}

/// A user the directory deactivated or deleted is refused by console admission with 403
/// `deprovisioned` before the overage answer cached for it is used, on `session` and a write
/// alike; reprovisioned active and in no group, it is a viewer once its answer expired: an
/// expired answer revives nothing.
#[test]
fn a_deprovisioned_overage_user_is_refused_before_its_cached_groups_are_used() {
    let Some(case) = overaged("ui_overage_deprovision", Some("2000")) else {
        return;
    };
    let Overage {
        team,
        audit,
        directory: console,
        ..
    } = case;
    let directory = &console;
    let ana = user(directory, "ana", ANA, true);
    group(directory, "team-billing", &[&ana]);
    let token = overage_token(&team, "ana", ANA);
    assert_eq!(session(directory, &token)["role"], "team_operator");

    let refused = |reply: Reply| {
        assert_eq!((reply.status, code(&reply)), (403, "deprovisioned".into()));
        assert!(!reply.body.contains(ANA) && !reply.body.contains("team-billing"));
    };
    let write = || {
        post(
            directory,
            "workspace/apply",
            &token,
            json!({"version": team.v1}),
        )
    };
    activate(directory, &ana, false);
    refused(ask(directory, "/api/v1/session", &token));
    refused(write());
    activate(directory, &ana, true);
    assert_eq!(session(directory, &token)["role"], "team_operator");
    // Deleted: refused, now and after the cached answer expired.
    expect(directory, "DELETE", &format!("/Users/{ana}"), None, 204);
    refused(ask(directory, "/api/v1/session", &token));
    std::thread::sleep(std::time::Duration::from_millis(2500));
    refused(ask(directory, "/api/v1/session", &token));
    // Provisioned again, active, in no group: admitted, and the expired answer is gone.
    user(directory, "ana", ANA, true);
    let seen = session(directory, &token);
    assert_eq!(
        (&seen["role"], &seen["groups"]),
        (&json!("viewer"), &json!([]))
    );
    let stderr = console.stderr();
    assert_leaks_nothing(&audit, &stderr, &[&token], &[ANA, "team-billing"]);
}

/// The token check of a pasted overage token lists the identities its directory groups grant (the
/// nodes' admission, so none under `ignore`) and records no request on the loopback issuer.
#[test]
fn the_token_check_resolves_a_pasted_overage_token_through_the_directory() {
    let Some(case) = overaged("ui_overage_check", None) else {
        return;
    };
    let Overage {
        team,
        audit,
        directory: console,
        ignore: console_ignore,
    } = case;
    let (directory, ignore) = (&console, &console_ignore);
    let ana = user(directory, "ana", ANA, true);
    group(directory, "team-billing", &[&ana]);
    let mut claims = overage_claims(&team, "ana", ANA);
    claims["aud"] = json!(NODES);
    let pasted = team.sign(&claims);
    let root = team.token("root", &["ops-group"], false);
    let requests = team.issuer.log.lock().unwrap().len();
    let checked = |console: &Server| {
        let reply = post(console, "admission/check", &root, json!({"token": pasted}));
        assert_eq!(reply.status, 200, "{}", reply.body);
        assert!(!reply.body.contains(&pasted));
        reply.json()
    };
    let found = checked(directory);
    let mut identities: Vec<String> = serde_json::from_value(found["identities"].clone()).unwrap();
    identities.sort();
    let mut expected = vec![BILLING, JOINT, LEDGER, SHARED];
    expected.sort();
    assert_eq!(identities, expected, "{found}");
    for check in ["signature", "issuer", "audience", "lifetime", "scopes"] {
        assert_eq!(found["checks"][check], json!(true), "{check}: {found}");
    }
    assert_eq!(checked(ignore)["identities"], json!([]));
    let log = team.issuer.log.lock().unwrap().clone();
    assert_eq!(log.len(), requests, "the check reached the issuer: {log:?}");
    let stderr = console.stderr();
    assert_leaks_nothing(&audit, &stderr, &[&pasted, &root], &[ANA, "team-billing"]);
    // The entries the fixture's grant never names stay out of it.
    assert!(ENTRIES.iter().any(|(repo, _)| *repo == SEARCH));
}
