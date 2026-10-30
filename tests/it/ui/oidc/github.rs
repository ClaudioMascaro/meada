//! The GitHub grant in the console (#332, ADR 0016): the team fixture of `team` (real Valkey and
//! store, the `oidc` module's loopback issuer) with `[oidc.github]` and `[postgres]`, its
//! permissions set seeded by `meada github-app roles` against `github_app`'s fake GitHub API
//! (`oidc::github::Github`), and `MEADA_PERMISSIONS_POLL_MS` crossing `max_age_seconds` in
//! seconds. What a token reads is seen through the console's routes, never the module's tables.

use std::collections::BTreeSet;
use std::fs;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::team::{DOCS, ENTRIES, JOINT, SEARCH, entries, team};
use super::{NODES, PUBLIC, ask, bearer};
use crate::oidc::github::{Github, LOGIN_CLAIM, POLL_MS};
use crate::server::Server;
use crate::snapshot::KEY_ID;
use crate::ui::{Reply, send_body};

/// The team identities' git host, `[oidc.github] host` here.
pub(super) const HOST: &str = "e.test";
const MAX_AGE: u64 = 8;

/// The console of `ops/console.toml`, polling every `POLL_MS`.
pub(super) fn console(dir: &std::path::Path) -> Server {
    Server::start_command(
        dir,
        "ui",
        &["--listen", "127.0.0.1:0", "--workspace", "ops/console.toml"],
        &[
            ("MEADA_TEST_S3_KEY", KEY_ID),
            ("MEADA_PERMISSIONS_POLL_MS", POLL_MS),
        ],
    )
}

pub(super) fn get(console: &Server, route: &str, token: &str) -> Value {
    let reply = ask(console, &format!("/api/v1/{route}"), token);
    assert_eq!(reply.status, 200, "{route}: {}", reply.body);
    reply.json()
}

pub(super) fn post(console: &Server, route: &str, token: &str, body: Value) -> Reply {
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

/// Waits until `token`'s session has `github_grant` = `state`; that session.
fn session_until(console: &Server, token: &str, state: &str) -> Value {
    let start = Instant::now();
    loop {
        let session = get(console, "session", token);
        if session["github_grant"] == state {
            return session;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{session}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Every team identity `text` names.
fn named(text: &str) -> BTreeSet<String> {
    let team = format!("{HOST}/team/");
    (text.match_indices(&team))
        .map(|(at, _)| {
            let rest = &text[at..];
            let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || "./-_".contains(c)));
            rest[..end.unwrap_or(rest.len())].to_string()
        })
        .collect()
}

/// The session reads the union and says whose login it is and whether the GitHub part is
/// current: `unread` before a set, then `current` with the GitHub identities in `grant` and every
/// bounded view, a group owning an entry only GitHub grants making a team operator; then
/// `expired`, a viewer reading its claims grant alone. The token check splits a pasted token's
/// identities by side, sorted, with no request to the issuer or GitHub; the admission view shows
/// the set's id, `finished`, age, `max_age_seconds` and the counts the job wrote; an audited write
/// records the subject's login. stderr names no token, login or URL.
#[test]
fn the_console_reads_the_union_and_shows_the_github_grant() {
    let Some(team) = team("ui_github", true) else {
        return;
    };
    let Some(github) = Github::new("ui_github") else {
        return;
    };
    let dir = &team.fleet.dir;
    let roles = "[\"write\"]";
    let job = format!(
        "index_dir = \"../roles-indexes\"\n{}\n[oidc]\nissuer = \"https://id.example\"\n\
         public_url = \"{NODES}\"\n{}{}",
        entries(&ENTRIES.map(|(repo, _)| (repo, None))),
        github.sections(HOST, roles, MAX_AGE),
        github.job(HOST)
    );
    fs::write(dir.join("ops/roles.toml"), job).unwrap();
    github.migrate(dir, "ops/roles.toml", &github.url);
    let file = dir.join("ops/console.toml");
    let source = fs::read_to_string(&file).unwrap();
    fs::write(&file, source + &github.sections(HOST, roles, MAX_AGE)).unwrap();
    let console = console(dir);

    let ana_claims = |aud: &str, groups: &[&str]| {
        let mut claims = team.claims("ana");
        claims["aud"] = json!(aud);
        claims["groups"] = json!(groups);
        claims[LOGIN_CLAIM] = json!("Ana-Souza");
        team.sign(&claims)
    };
    let ana = ana_claims(PUBLIC, &["team-docs"]);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        get(&console, "session", &ana),
        json!({"subject": "ana", "role": "viewer", "groups": ["team-docs"], "grant": [],
               "login": "ana-souza", "github_host": HOST, "github_grant": "unread"})
    );

    github.list("team/docs", &[("ana-souza", "write")]);
    github.list(
        "team/search",
        &[("ana-souza", "triage"), ("bia_acme", "write")],
    );
    let line = github.seed(dir, "ops/roles.toml", &github.url);
    let id = (line.strip_prefix("permissions set "))
        .and_then(|rest| rest.split_once(": 2 repositories, 3 rows, 0 exclusions\n"))
        .map(|(id, _)| id.to_string())
        .unwrap_or_else(|| panic!("{line}"));
    let session = session_until(&console, &ana, "current");
    assert_eq!(
        session,
        json!({"subject": "ana", "role": "team_operator", "groups": ["team-docs"],
               "grant": [DOCS], "teams": ["team-docs"], "login": "ana-souza",
               "github_host": HOST, "github_grant": "current"})
    );
    let only_docs = BTreeSet::from([DOCS.to_string()]);
    let workspace = ask(&console, "/api/v1/workspace", &ana).body;
    assert_eq!(named(&workspace), only_docs, "{workspace}");
    for route in [
        format!("workspace/versions/{}", team.v1),
        "workspace/versions".into(),
        "repositories".into(),
        "manifest".into(),
        "store".into(),
        "credentials".into(),
    ] {
        let body = ask(&console, &format!("/api/v1/{route}"), &ana).body;
        assert!(named(&body).is_subset(&only_docs), "{route}: {body}");
    }

    // The token check: from the console's cache, with no request to the issuer or GitHub.
    let root = team.token("root", &[], true);
    let (logged, requests) = (
        team.issuer.log.lock().unwrap().len(),
        github.api.with(|s| s.requests),
    );
    let pasted = ana_claims(NODES, &["team-search"]);
    let checked = post(&console, "admission/check", &root, json!({"token": pasted})).json();
    assert_eq!(
        checked["identities"],
        json!([SEARCH, JOINT, DOCS]),
        "{checked}"
    );
    assert_eq!(
        checked["sources"],
        json!({"claims": [JOINT, SEARCH], "github": [DOCS]})
    );
    let mut machine = team.claims("agent");
    machine["aud"] = json!(NODES);
    machine["meada_repositories"] = json!(["*"]);
    let machine = team.sign(&machine);
    let checked = post(
        &console,
        "admission/check",
        &root,
        json!({"token": machine}),
    )
    .json();
    assert_eq!(checked["sources"], json!({"claims": "*", "github": []}));
    assert_eq!(team.issuer.log.lock().unwrap().len(), logged);
    assert_eq!(github.api.with(|s| s.requests), requests);

    let admission = get(&console, "admission", &root);
    let set = &admission["github"]["set"];
    assert_eq!(set["id"], json!(id));
    assert!(
        set["age_seconds"]
            .as_u64()
            .is_some_and(|age| age <= MAX_AGE),
        "{set}"
    );
    assert!(
        set["finished"].as_str().is_some_and(|t| t.ends_with('Z')),
        "{set}"
    );
    let mut github_view = admission["github"].clone();
    for key in ["id", "finished", "age_seconds"] {
        github_view["set"].as_object_mut().unwrap().remove(key);
    }
    assert_eq!(
        github_view,
        json!({"host": HOST, "login_claim": LOGIN_CLAIM, "roles": ["write"],
               "max_age_seconds": MAX_AGE, "grant": "current",
               "set": {"repositories": 2, "rows": 3, "exclusions": 0}})
    );

    // An audited write records the subject's login.
    let mut operator = team.claims("root");
    operator["operator"] = json!(true);
    operator[LOGIN_CLAIM] = json!("Root-Op");
    let operator = team.sign(&operator);
    let reply = post(
        &console,
        "fence/recover",
        &operator,
        json!({"at_least": 50}),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    let audit = get(&console, "audit", &operator);
    assert_eq!(audit["records"][0]["login"], "root-op", "{audit}");
    assert_eq!(audit["records"][0]["action"], "fence.recover", "{audit}");

    let session = session_until(&console, &ana, "expired");
    assert_eq!(
        session,
        json!({"subject": "ana", "role": "viewer", "groups": ["team-docs"], "grant": [],
               "login": "ana-souza", "github_host": HOST, "github_grant": "expired"})
    );
    let workspace = ask(&console, "/api/v1/workspace", &ana).body;
    assert!(named(&workspace).is_empty(), "{workspace}");
    let stderr = console.stderr();
    github.clean(&stderr, &[ana, pasted, machine, root, operator]);
    assert!(!stderr.to_lowercase().contains("root-op"), "{stderr}");
}
