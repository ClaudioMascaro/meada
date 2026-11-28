use std::fs;
use std::io::Write;

use jsonwebtoken::Algorithm;
use serde_json::{Value, json};

use super::directory::{GROUP, TOKEN, USER, append, expect, patch, scim};
use super::team::{BILLING, ENTRIES, JOINT, SEARCH, SHARED, Team, entries, team};
use super::{PUBLIC, VERSION, ask, bearer, claims, console_dir, head, ui_oidc};
use crate::manifest::MANIFEST;
use crate::oidc::{Issuer, key, set, sign};
use crate::postgres::{Audited, audited};
use crate::server::Server;
use crate::snapshot::KEY_ID;
use crate::ui::{Reply, send_body};

const DIRECTORY: &str = "\n[directory]\nuser_claim = \"sub\"\nuser_attribute = \"userName\"\n";

fn start(team: &Team) -> Server {
    let args = ["--listen", "127.0.0.1:0", "--workspace", "ops/console.toml"];
    let env = [("MEADA_TEST_S3_KEY", KEY_ID), ("MEADA_SCIM_TOKEN", TOKEN)];
    Server::start_command(&team.fleet.dir, "ui", &args, &env)
}

fn directed(case: &'static str) -> Option<(Team, Audited, Server)> {
    let team = team(case, true)?;
    let audit = audited(&team.fleet.dir, case)?;
    let manifest = team.fleet.dir.join("ops/console.toml");
    let mut manifest = fs::OpenOptions::new().append(true).open(manifest).unwrap();
    write!(manifest, "{}{DIRECTORY}", audit.section).unwrap();
    let console = start(&team);
    Some((team, audit, console))
}

fn post(console: &Server, route: &str, token: &str, body: Value) -> Reply {
    let headers = [("Authorization", bearer(token)), ("Origin", PUBLIC.into())];
    let headers: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
    send_body(
        console,
        "POST",
        &format!("/api/v1/{route}"),
        &headers,
        &body.to_string(),
    )
}

fn code(reply: &Reply) -> String {
    reply.json()["error"]["code"].as_str().unwrap_or("").into()
}

fn user(console: &Server, name: &str, extra: Value) -> String {
    let mut body = json!({"schemas": [USER], "userName": name});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let created = expect(console, "POST", "/Users", Some(&body), 201);
    created["id"].as_str().unwrap().to_string()
}

fn group(console: &Server, name: &str) -> String {
    let body = json!({"schemas": [GROUP], "displayName": name});
    let created = expect(console, "POST", "/Groups", Some(&body), 201);
    created["id"].as_str().unwrap().to_string()
}

fn active(console: &Server, id: &str, active: Value) {
    let on = patch(json!([{"op": "Replace", "path": "active", "value": active}]));
    expect(console, "PATCH", &format!("/Users/{id}"), Some(&on), 200);
}

fn nameless(team: &Team) -> String {
    let mut claims = team.claims("");
    claims.as_object_mut().unwrap().remove("sub");
    claims["operator"] = json!(true);
    team.sign(&claims)
}

fn deprovisioned(reply: &Reply) -> bool {
    reply.status == 403 && code(reply) == "deprovisioned"
}

const READS: [&str; 10] = [
    "session",
    "fleet",
    "store",
    "repositories",
    "credentials",
    "workspace",
    "workspace/versions",
    "manifest",
    "admission",
    "audit",
];

fn writes(team: &Team, g: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("pointer", json!({"action": "retain", "generation": g})),
        ("fence/recover", json!({})),
        (
            "workspace/versions",
            json!({"entries": entries(&ENTRIES[..2]), "message": "m"}),
        ),
        ("workspace/apply", json!({"version": team.v1})),
    ]
}

fn state(console: &Server, token: &str) -> [Value; 4] {
    ["fleet", "store", "workspace", "workspace/versions"]
        .map(|route| ask(console, &format!("/api/v1/{route}"), token).json())
}

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

#[test]
fn a_deactivated_user_is_refused_on_every_route_but_config() {
    let Some((team, audit, console)) = directed("ui_deprovision") else {
        return;
    };
    let root = team.token("root", &[], true);
    let ana = team.token("ana", &["team-billing"], false);
    let boss = team.token("boss", &[], true);
    let ghost = team.token("ghost", &[], false);
    let mut headless = team.claims("");
    headless.as_object_mut().unwrap().remove("sub");
    let headless = team.sign(&headless);
    let mut numeric = team.claims("");
    numeric["sub"] = json!(7);
    let numeric = team.sign(&numeric);
    let ana_id = user(&console, "Ana", json!({"active": true}));
    let boss_id = user(&console, "BOSS", json!({}));
    assert_eq!(
        ask(&console, "/api/v1/session", &ana).json()["role"],
        "team_operator"
    );
    let g1 = crate::refresh_lock::locked_publish(&team.fleet.dir);
    let before = state(&console, &root);
    active(&console, &ana_id, json!(false));
    active(&console, &boss_id, json!("False"));
    for token in [&ana, &boss] {
        for route in READS {
            let reply = ask(&console, &format!("/api/v1/{route}"), token);
            assert!(deprovisioned(&reply), "{route}: {}", reply.body);
        }
        for (route, body) in writes(&team, &g1) {
            let reply = post(&console, route, token, body);
            assert!(deprovisioned(&reply), "{route}: {}", reply.body);
        }
        let query = json!({"repository": BILLING, "tool": "search", "arguments": {"query": "x"}});
        assert!(deprovisioned(&post(&console, "query", token, query)));
        let check = json!({"token": "x"});
        assert!(deprovisioned(&post(
            &console,
            "admission/check",
            token,
            check
        )));
    }
    let config = ask(&console, "/api/v1/config", &ana);
    assert_eq!(config.status, 200, "{}", config.body);
    assert_eq!(ask(&console, "/", &ana).status, 200);
    assert_eq!(state(&console, &root), before);
    assert!(audit.records().is_empty(), "{:?}", audit.records());
    for token in [&root, &ghost, &headless, &numeric] {
        let reply = ask(&console, "/api/v1/session", token);
        assert_eq!(reply.status, 200, "{}", reply.body);
    }
    assert_eq!(
        ask(&console, "/api/v1/session", &headless).json()["subject"],
        Value::Null
    );
    active(&console, &ana_id, json!(true));
    active(&console, &boss_id, json!(true));
    let session = ask(&console, "/api/v1/session", &ana).json();
    assert_eq!(session["role"], "team_operator", "{session}");
    assert_eq!(
        ask(&console, "/api/v1/session", &boss).json()["role"],
        "operator"
    );
    let stderr = console.stderr();
    assert_leaks_nothing(
        &audit,
        &stderr,
        &[&root, &ana, &boss, &ghost],
        &["ana", "Ana", "boss", "BOSS", "ghost"],
    );
}

#[test]
fn a_deleted_user_stays_refused_until_it_is_provisioned_active() {
    let Some((team, audit, console)) = directed("ui_deprovision_deleted") else {
        return;
    };
    let dora = team.token("dora", &[], false);
    let id = user(&console, "dora", json!({"active": true}));
    assert_eq!(ask(&console, "/api/v1/session", &dora).status, 200);
    expect(&console, "DELETE", &format!("/Users/{id}"), None, 204);
    let reply = ask(&console, "/api/v1/session", &dora);
    assert!(deprovisioned(&reply), "{}", reply.body);
    let again = user(&console, "Dora", json!({}));
    assert!(deprovisioned(&ask(&console, "/api/v1/session", &dora)));
    let put = json!({"schemas": [USER], "userName": "dora", "active": true});
    expect(&console, "PUT", &format!("/Users/{again}"), Some(&put), 200);
    assert_eq!(ask(&console, "/api/v1/session", &dora).status, 200);
    audit.assert_no_url(&console.stderr());
}

#[test]
fn an_external_id_lookup_does_not_ignore_case() {
    let (encoding, jwk) = key("k1");
    let issuer = Issuer::new(set(&[&jwk]));
    let dir = console_dir(
        "ui_deprovision_external",
        Some(VERSION),
        &head(&issuer.base),
        &ui_oidc(),
    );
    let Some(audit) = audited(&dir, "ui_deprovision_external") else {
        return;
    };
    let text = "\n[directory]\nuser_claim = \"sub\"\nuser_attribute = \"externalId\"\n";
    append(&dir, &format!("{}{text}", audit.section));
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    let console = Server::start_command(&dir, "ui", &args, &[("MEADA_SCIM_TOKEN", TOKEN)]);
    let token = |sub: &str| {
        let mut claims = claims(&issuer.base, &[]);
        claims["sub"] = json!(sub);
        sign(&encoding, "k1", Algorithm::ES256, &claims)
    };
    let id = user(&console, "someone", json!({"externalId": "Ext-1"}));
    active(&console, &id, json!(false));
    let exact = ask(&console, "/api/v1/session", &token("Ext-1"));
    assert!(deprovisioned(&exact), "{}", exact.body);
    let other = ask(&console, "/api/v1/session", &token("ext-1"));
    assert_eq!(other.status, 200, "{}", other.body);
    assert_eq!(
        ask(&console, "/api/v1/session", &token("someone")).status,
        200
    );
}

#[test]
fn an_unreadable_directory_admits_reads_and_refuses_writes() {
    let Some((team, audit, console)) = directed("ui_deprovision_down") else {
        return;
    };
    let root = team.token("root", &[], true);
    let g1 = crate::refresh_lock::locked_publish(&team.fleet.dir);
    let before = state(&console, &root);
    audit.relay.close();
    let reads = ["session", "fleet", "workspace/versions", "workspace"];
    for route in reads {
        let reply = ask(&console, &format!("/api/v1/{route}"), &root);
        assert_eq!(reply.status, 200, "{route}: {}", reply.body);
    }
    let picker = ask(&console, "/api/v1/directory/groups", &root);
    assert_eq!(picker.status, 503, "{}", picker.body);
    assert_eq!(picker.json()["error"]["code"], "unavailable");
    let unavailable = "the audit in Postgres is unavailable: no write is made without its record";
    let sent = writes(&team, &g1);
    for (route, body) in &sent {
        let reply = post(&console, route, &root, body.clone());
        assert_eq!(reply.status, 503, "{route}: {}", reply.body);
        assert_eq!(
            reply.json()["error"],
            json!({"code": "audit_unavailable", "message": unavailable})
        );
    }
    assert_eq!(ask(&console, "/api/v1/config", &root).status, 200);
    assert!(audit.records().is_empty());
    let stderr = console.stderr();
    let lines: Vec<&str> = stderr.lines().filter(|l| l.starts_with("ui: ")).collect();
    assert_eq!(lines.len(), reads.len() + 1 + sent.len(), "{lines:?}");
    assert!(
        lines
            .iter()
            .all(|l| l.starts_with("ui: directory: unreadable (")),
        "{lines:?}"
    );
    assert_leaks_nothing(&audit, &stderr, &[&root], &["root"]);
    assert_eq!(state(&team.console, &root), before);
}

#[test]
fn the_owner_picker_lists_group_names_for_the_global_operator_alone() {
    let Some((team, _audit, console)) = directed("ui_deprovision_picker") else {
        return;
    };
    let root = team.token("root", &[], true);
    let ana = team.token("ana", &["team-billing"], false);
    let viewer = team.token("vic", &["eng"], false);
    for name in ["alphabet", "Beta", "alpha", "caf\u{e9}"] {
        group(&console, name);
    }
    for n in 0..105 {
        group(&console, &format!("g{n:03}"));
    }
    let picker = |query: &str, token: &str| {
        ask(&console, &format!("/api/v1/directory/groups{query}"), token)
    };
    let names = |reply: Reply| {
        assert_eq!(reply.status, 200, "{}", reply.body);
        let groups = reply.json();
        assert_eq!(groups.as_object().unwrap().len(), 1, "{groups}");
        let groups = groups["groups"].as_array().unwrap().clone();
        groups
            .iter()
            .map(|g| g.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let all = names(picker("", &root));
    let mut head = vec![
        "Beta".to_string(),
        "alpha".into(),
        "alphabet".into(),
        "caf\u{e9}".into(),
    ];
    head.extend((0..96).map(|n| format!("g{n:03}")));
    assert_eq!(all, head);
    assert_eq!(names(picker("?prefix=", &root)), head);
    assert_eq!(names(picker("?prefix=alph", &root)), ["alpha", "alphabet"]);
    let letters = names(picker("?prefix=g", &root));
    assert_eq!(
        letters,
        (0..100).map(|n| format!("g{n:03}")).collect::<Vec<_>>()
    );
    assert_eq!(
        names(picker("?prefix=g10", &root)),
        ["g100", "g101", "g102", "g103", "g104"]
    );
    assert_eq!(names(picker("?prefix=zzz", &root)), Vec::<String>::new());
    assert_eq!(picker("?prefix=zzz", &root).body, r#"{"groups":[]}"#);
    assert_eq!(names(picker("?prefix=b", &root)), Vec::<String>::new());
    assert_eq!(names(picker("?prefix=Be", &root)), ["Beta"]);
    assert_eq!(names(picker("?prefix=caf%C3%A9", &root)), ["caf\u{e9}"]);
    assert_eq!(names(picker("?prefix=caf%00", &root)), Vec::<String>::new());
    let long = "a".repeat(256);
    assert_eq!(
        names(picker(&format!("?prefix={long}"), &root)),
        Vec::<String>::new()
    );
    for bad in [
        format!("?prefix={long}a"),
        "?q=a".into(),
        "?prefix=a&prefix=b".into(),
        "?prefix=%ff".into(),
        "?prefix=a&x=1".into(),
    ] {
        let reply = picker(&bad, &root);
        assert_eq!(
            (reply.status, code(&reply)),
            (400, "bad_request".into()),
            "{bad}"
        );
    }
    for token in [&ana, &viewer] {
        let reply = picker("?prefix=a", token);
        assert_eq!((reply.status, code(&reply)), (403, "forbidden".into()));
        assert!(!reply.body.contains("alpha"), "{}", reply.body);
    }
    let none = ask(&team.console, "/api/v1/directory/groups", &root);
    assert_eq!((none.status, code(&none)), (404, "not_found".into()));
}

#[test]
fn entries_whose_owner_names_no_group_are_flagged() {
    let Some((team, audit, console)) = directed("ui_deprovision_owner") else {
        return;
    };
    let root = nameless(&team);
    let viewer = team.token("vic", &["team-billing"], false);
    let plain = |route: &str| ask(&team.console, &format!("/api/v1/{route}"), &root).json();
    let directed = |route: &str, token: &str| ask(&console, &format!("/api/v1/{route}"), token);
    let flagged = |body: &Value| -> Vec<String> {
        let entries = body["version"]["repositories"].as_array();
        let entries = entries.or(body["repositories"].as_array()).unwrap();
        let mut found: Vec<String> = (entries.iter())
            .filter(|e| e.get("owner_missing").is_some())
            .map(|e| {
                assert_eq!(e["owner_missing"], json!(true), "present only when true");
                e["repo"].as_str().unwrap().to_string()
            })
            .collect();
        found.sort();
        found
    };
    let current = plain("workspace");
    let id = current["current"]["id"].as_str().unwrap().to_string();
    let at = |token: &str| directed("workspace", token);
    assert_eq!(
        at(&root).body,
        ask(&team.console, "/api/v1/workspace", &root).body
    );
    user(&console, "someone", json!({}));
    assert_eq!(
        at(&root).body,
        ask(&team.console, "/api/v1/workspace", &root).body
    );
    let other = group(&console, "unrelated");
    let owned = |names: &[&str]| -> Vec<String> {
        let mut found: Vec<String> = (ENTRIES.iter())
            .filter(|(_, owner)| owner.is_some_and(|o| names.contains(&o)))
            .map(|(repo, _)| repo.to_string())
            .collect();
        found.sort();
        found
    };
    let owners = ["team-billing", "team-search", "team-docs"];
    assert_eq!(flagged(&at(&root).json()), owned(&owners));
    let version = directed(&format!("workspace/versions/{id}"), &root).json();
    assert_eq!(flagged(&version), owned(&owners));
    let billing = group(&console, "team-billing");
    assert_eq!(
        flagged(&at(&root).json()),
        owned(&["team-search", "team-docs"])
    );
    assert!(!flagged(&at(&root).json()).contains(&SHARED.to_string()));
    let seen = at(&viewer).json();
    assert_eq!(flagged(&seen), [JOINT], "{seen}");
    assert!(seen["version"]["repositories"].as_array().unwrap().len() < ENTRIES.len());
    let rename = patch(json!([{"op": "Replace", "path": "displayName", "value": "team-bill"}]));
    expect(
        &console,
        "PATCH",
        &format!("/Groups/{billing}"),
        Some(&rename),
        204,
    );
    assert_eq!(flagged(&at(&root).json()), owned(&owners));
    let back = patch(json!([{"op": "Replace", "path": "displayName", "value": "team-billing"}]));
    expect(
        &console,
        "PATCH",
        &format!("/Groups/{billing}"),
        Some(&back),
        204,
    );
    assert_eq!(
        flagged(&at(&root).json()),
        owned(&["team-search", "team-docs"])
    );
    let writer = team.token("root", &[], true);
    let saved = post(
        &console,
        "workspace/versions",
        &writer,
        json!({"entries": entries(&ENTRIES), "message": "annotated"}),
    );
    assert_eq!(saved.status, 200, "{}", saved.body);
    let saved = saved.json()["result"].as_str().unwrap().to_string();
    let stored = ask(
        &team.console,
        &format!("/api/v1/workspace/versions/{saved}"),
        &root,
    );
    assert!(!stored.body.contains("owner_missing"), "{}", stored.body);
    assert_eq!(
        stored.json()["repositories"],
        current["version"]["repositories"],
        "the same entries"
    );
    expect(&console, "DELETE", &format!("/Groups/{billing}"), None, 204);
    expect(&console, "DELETE", &format!("/Groups/{other}"), None, 204);
    let none = scim(&console, "GET", "/Groups", None).json();
    assert_eq!(none["totalResults"], json!(0));
    assert_eq!(flagged(&at(&root).json()), owned(&owners));
    assert_eq!(
        flagged(&directed(&format!("workspace/versions/{id}"), &root).json()),
        owned(&owners)
    );
    audit.relay.close();
    let unreadable = at(&root);
    assert_eq!(unreadable.status, 200, "{}", unreadable.body);
    assert_eq!(
        unreadable.body,
        ask(&team.console, "/api/v1/workspace", &root).body
    );
    let stderr = console.stderr();
    let named: Vec<&str> = (stderr.lines())
        .filter(|l| l.starts_with("ui: directory: unreadable ("))
        .collect();
    assert_eq!(named.len(), 1, "{stderr}");
    assert_leaks_nothing(
        &audit,
        &stderr,
        &[&root, &viewer],
        &["team-billing", SEARCH],
    );
}

#[test]
fn admission_carries_the_directory_s_counts_and_last_write() {
    let Some((team, audit, console)) = directed("ui_deprovision_counts") else {
        return;
    };
    let root = nameless(&team);
    let counts = |console: &Server| ask(console, "/api/v1/admission", &root).json();
    assert_eq!(
        counts(&console)["directory"],
        json!({"users": 0, "groups": 0, "last_write": null})
    );
    assert!(counts(&team.console).get("directory").is_none());
    let one = user(&console, "one", json!({}));
    group(&console, "ops");
    let shown = counts(&console)["directory"].clone();
    let written = shown["last_write"].as_str().unwrap().to_string();
    assert_eq!(
        (shown["users"].clone(), shown["groups"].clone()),
        (json!(1), json!(1))
    );
    let shape = "dddd-dd-ddTdd:dd:dd.ddddddZ";
    assert!(
        written.len() == shape.len()
            && written.bytes().zip(shape.bytes()).all(|(b, s)| match s {
                b'd' => b.is_ascii_digit(),
                _ => b == s,
            }),
        "{written}"
    );
    let taken = json!({"schemas": [USER], "userName": "ONE"});
    let failed = scim(&console, "POST", "/Users", Some(&taken));
    assert_eq!(failed.status, 409, "{}", failed.body);
    assert_eq!(counts(&console)["directory"], shown);
    expect(&console, "DELETE", &format!("/Users/{one}"), None, 204);
    let after = counts(&console)["directory"].clone();
    assert_eq!(
        (after["users"].clone(), after["groups"].clone()),
        (json!(0), json!(1))
    );
    assert!(
        after["last_write"].as_str().unwrap() > written.as_str(),
        "{after}"
    );
    audit.relay.close();
    let down = counts(&console);
    assert_eq!(down["directory"], Value::Null);
    assert_eq!(down["type"], "generic");
    let stderr = console.stderr();
    assert_eq!(
        stderr.matches("ui: directory: unreadable (").count(),
        1,
        "{stderr}"
    );
    assert_leaks_nothing(&audit, &stderr, &[&root], &[]);
}
