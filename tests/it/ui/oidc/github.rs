use std::collections::BTreeSet;
use std::fs;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::team::{DOCS, ENTRIES, JOINT, SEARCH, Team, entries, team};
use super::{NODES, PUBLIC, ask, bearer};
use crate::oidc::github::{Github, LOGIN_CLAIM, POLL_MS};
use crate::server::Server;
use crate::snapshot::KEY_ID;
use crate::ui::{Reply, send_body};

pub(super) const HOST: &str = "e.test";
const MAX_AGE: u64 = 8;

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

fn grant_until(console: &Server, token: &str, expected: Value) -> Value {
    let start = Instant::now();
    loop {
        let session = get(console, "session", token);
        if session["grant"] == expected {
            return session;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{session}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

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

fn with_github(team: &Team, github: &Github) -> Server {
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
    console(dir)
}

#[test]
fn the_console_reads_the_union_and_shows_the_github_grant() {
    let Some(team) = team("ui_github", true) else {
        return;
    };
    let Some(github) = Github::new("ui_github") else {
        return;
    };
    let dir = &team.fleet.dir;
    let console = with_github(&team, &github);

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

#[test]
fn a_set_committed_before_a_failed_refresh_still_expires_and_a_later_set_revokes() {
    use crate::manifest::MANIFEST;
    use crate::refresh_lock::{current, lock, locked_publish};
    use crate::serving::probe;
    let Some(team) = team("ui_github_failed", true) else {
        return;
    };
    let Some(github) = Github::new("ui_github_failed") else {
        return;
    };
    let dir = &team.fleet.dir;
    let console = with_github(&team, &github);
    let mut claims = team.claims("ana");
    claims["aud"] = json!(PUBLIC);
    claims["groups"] = json!(["team-docs"]);
    claims[LOGIN_CLAIM] = json!("Ana-Souza");
    let ana = team.sign(&claims);

    let g1 = locked_publish(dir);
    let (node, _) = team.fleet.node("node");
    assert_eq!(team.fleet.pull(&node), g1);
    let held = format!("current: {g1}\nheld: {g1}\nstore: reachable (current {g1})\n");
    let unchanged = || {
        assert_eq!(current(&team.fleet).0, g1);
        assert_eq!(team.fleet.pull(&node), g1);
        assert_eq!(probe(dir, &node), (held.clone(), String::new(), Some(0)));
    };
    let indexes = dir.join("indexes/example.com/chain/web");
    assert!(indexes.is_dir());
    fs::remove_dir_all(&indexes).unwrap();
    fs::write(&indexes, "not a directory").unwrap();
    let refresh = || {
        let script = format!(
            "set -e\nMEADA_PERMISSIONS_POLL_MS={POLL_MS} \"$MEADA\" github-app roles --workspace \
             ops/roles.toml\n\"$MEADA\" index --workspace {MANIFEST}\n\"$MEADA\" snapshot publish \
             --workspace {MANIFEST}\n"
        );
        let (stdout, stderr, code) = lock(dir, MANIFEST, &script).wait();
        github.clean(&stderr, std::slice::from_ref(&ana));
        assert_eq!((code, stdout.lines().count()), (Some(1), 1), "{stderr}");
        assert!(stderr.contains("index failed"), "{stderr}");
        assert!(stdout.starts_with("permissions set "), "{stdout}");
    };

    github.list("team/docs", &[("ana-souza", "write")]);
    let committed = Instant::now();
    refresh();
    let session = session_until(&console, &ana, "current");
    assert_eq!(session["grant"], json!([DOCS]), "{session}");
    unchanged();

    let session = session_until(&console, &ana, "expired");
    assert!(committed.elapsed() >= Duration::from_secs(MAX_AGE) - Duration::from_millis(100));
    assert_eq!(session["grant"], json!([]), "{session}");
    assert_eq!(session["role"], "viewer", "{session}");
    let workspace = ask(&console, "/api/v1/workspace", &ana).body;
    assert!(named(&workspace).is_empty(), "{workspace}");
    unchanged();

    github.list("team/docs", &[("ana-souza", "write")]);
    refresh();
    let session = session_until(&console, &ana, "current");
    assert_eq!(session["grant"], json!([DOCS]), "{session}");
    unchanged();

    github.list("team/docs", &[("bia_acme", "write")]);
    refresh();
    let session = grant_until(&console, &ana, json!([]));
    assert_eq!(session["github_grant"], "current", "{session}");
    let workspace = ask(&console, "/api/v1/workspace", &ana).body;
    assert!(named(&workspace).is_empty(), "{workspace}");
    unchanged();
    let stderr = console.stderr();
    github.clean(&stderr, &[ana]);
}
