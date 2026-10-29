//! The audit (#328, ADR 0016): with `[postgres]`, every console write leaves one record, written
//! before the write and refusing it when it cannot be written, and the global operator reads
//! them through `GET audit`. The team fixture of `team` (real Valkey and store, the `oidc`
//! module's loopback issuer), a local Postgres in a schema of each test's own behind a loopback
//! relay the test may close.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::team::{
    BILLING, DOCS, ENTRIES, JOINT, LEDGER, SHARED, Team, audit, entries, start, team,
};
use super::{PUBLIC, ask, bearer};
use crate::manifest::MANIFEST;
use crate::postgres::{Audited, audited};
use crate::refresh_lock::{held_by, lock, locked_publish, wait_for};
use crate::server::Server;
use crate::snapshot::next_commit;
use crate::ui::{Reply, send_body};

/// `team` with `[postgres]` in the console's manifest, and the console restarted over it.
fn audited_team(case: &'static str) -> Option<(Team, Audited, Server)> {
    let team = team(case, true)?;
    let audit = audited(&team.fleet.dir, case)?;
    let file = team.fleet.dir.join("ops/console.toml");
    let source = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{source}{}", audit.section)).unwrap();
    let console = start(&team.fleet);
    Some((team, audit, console))
}

fn post(console: &Server, route: &str, token: &str, body: Value) -> Reply {
    send_body(
        console,
        "POST",
        &format!("/api/v1/{route}"),
        &[("Authorization", &bearer(token)), ("Origin", PUBLIC)],
        &body.to_string(),
    )
}

/// A write whose body announces a megabyte and never comes: the status, read within 10 s.
fn unfinished(console: &Server, route: &str, token: &str) -> u16 {
    let address = console.url.trim_start_matches("http://");
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let head = format!(
        "POST /api/v1/{route} HTTP/1.1\r\nHost: {address}\r\nAuthorization: {}\r\nOrigin: \
         {PUBLIC}\r\nContent-Length: 1048576\r\n\r\n{{\"version\": \"",
        bearer(token)
    );
    stream.write_all(head.as_bytes()).unwrap();
    let start = Instant::now();
    let mut answer = [0; 64];
    let n = stream.read(&mut answer).expect("an answer before the body");
    assert!(start.elapsed() < Duration::from_secs(10));
    let answer = String::from_utf8_lossy(&answer[..n]);
    answer.split(' ').nth(1).unwrap().parse().unwrap()
}

/// `record` without its `id` and `time`, for comparison.
fn fields(record: &Value) -> Value {
    let mut record = record.clone();
    let object = record.as_object_mut().unwrap();
    object.remove("id");
    object.remove("time");
    record
}

/// No part of a token of `tokens` and no Postgres URL in `text`.
fn assert_leaks_nothing(audit: &Audited, text: &str, tokens: &[&str]) {
    for token in tokens {
        for part in token.split('.') {
            assert!(!text.contains(part), "a token part in {text}");
        }
    }
    audit.assert_no_url(text);
}

/// The global operator's `pointer` `set`, `retain` and `drop`, `fence/recover`,
/// `workspace/versions` and `workspace/apply` each leave one record (`via = "console"`, its
/// action, its arguments, `accepted`), and `audit` returns them newest first with Postgres's
/// time, paged by `next`; a write while `meada snapshot lock` holds the lock goes from pending to
/// `refused`. Only the global operator reads the audit, and a query other than `before` and
/// `limit` is a 400. The stderr lines are the ones S2 prints.
#[test]
fn every_operator_write_leaves_one_record_read_newest_first() {
    let Some((team, audit, console)) = audited_team("ui_audit_writes") else {
        return;
    };
    let dir = &team.fleet.dir;
    let g1 = locked_publish(dir);
    next_commit(dir, "chain-web", "audit-second");
    let g2 = locked_publish(dir);
    let root = team.token("root", &[], true);
    let moved: Vec<_> = ENTRIES
        .iter()
        .copied()
        .filter(|(r, _)| *r != DOCS)
        .collect();
    let text = entries(&moved);
    let accepted = |route: &str, body: Value| {
        let reply = post(&console, route, &root, body);
        assert_eq!(reply.status, 200, "{route}: {}", reply.body);
        reply.json()["result"].as_str().unwrap().to_string()
    };
    let retained = accepted("pointer", json!({"action": "retain", "generation": g1}));
    let set = accepted("pointer", json!({"action": "set", "generation": g1}));
    let dropped = accepted("pointer", json!({"action": "drop", "generation": g2}));
    let recovered = accepted("fence/recover", json!({"at_least": 50}));
    let saved = accepted(
        "workspace/versions",
        json!({"entries": text, "message": "no docs"}),
    );
    let applied = accepted("workspace/apply", json!({"version": saved}));
    assert_eq!(
        (retained.as_str(), dropped.as_str(), recovered.as_str()),
        (g1.as_str(), g2.as_str(), "50")
    );
    // A held lock: refused, after its pending record.
    let child = lock(dir, MANIFEST, "touch ops/started; exec sleep 60");
    wait_for(&dir.join("ops/started"));
    let held = post(
        &console,
        "pointer",
        &root,
        json!({"action": "set", "generation": g2}),
    );
    assert_eq!(held.status, 409, "{}", held.body);
    let holder = held_by(&team.fleet, child.id());
    let holder = holder.trim_end().trim_start_matches("meada: ");
    assert_eq!(held.json()["error"]["message"], holder);
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
    child.wait();

    // `config` tells the front end the page has its route.
    assert_eq!(ask(&console, "/api/v1/config", &root).json()["audit"], true);
    let page = ask(&console, "/api/v1/audit", &root);
    assert_eq!(page.status, 200, "{}", page.body);
    let page = page.json();
    assert_eq!(page["next"], Value::Null);
    let records = page["records"].as_array().unwrap().clone();
    let record = |action: &str, arguments: Value, outcome: &str, message: &str| {
        json!({"via": "console", "subject": "root", "login": null, "role": "operator",
               "owners": null, "action": action, "arguments": arguments, "outcome": outcome,
               "message": message})
    };
    let expected = [
        record("pointer.set", json!({"generation": g2}), "refused", holder),
        record(
            "workspace.apply",
            json!({"version": saved}),
            "accepted",
            &applied,
        ),
        record(
            "workspace.save",
            json!({"version": saved, "message": "no docs"}),
            "accepted",
            &saved,
        ),
        record("fence.recover", json!({"at_least": "50"}), "accepted", "50"),
        record("pointer.drop", json!({"generation": g2}), "accepted", &g2),
        record("pointer.set", json!({"generation": g1}), "accepted", &set),
        record("pointer.retain", json!({"generation": g1}), "accepted", &g1),
    ];
    assert_eq!(records.iter().map(fields).collect::<Vec<_>>(), expected);
    // Newest first, each `time` Postgres's own.
    let times = audit.admin.texts(&format!(
        "SELECT to_char(\"time\" AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') FROM \
         \"{}\".audit ORDER BY id DESC",
        audit.schema
    ));
    let found: Vec<_> = records
        .iter()
        .map(|r| r["time"].as_str().unwrap())
        .collect();
    assert_eq!(found, times);
    let ids: Vec<i64> = records.iter().map(|r| r["id"].as_i64().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] > w[1]), "{ids:?}");

    // Paging with `next` lists every record once.
    let mut paged = Vec::new();
    let mut path = "/api/v1/audit?limit=3".to_string();
    loop {
        let page = ask(&console, &path, &root).json();
        paged.extend(
            page["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].as_i64().unwrap()),
        );
        match page["next"].as_i64() {
            Some(next) => path = format!("/api/v1/audit?before={next}&limit=3"),
            None => break,
        }
    }
    assert_eq!(paged, ids);
    for query in [
        "limit=0",
        "limit=101",
        "before=x",
        "before=0",
        "limit=5&limit=5",
        "page=2",
    ] {
        let reply = ask(&console, &format!("/api/v1/audit?{query}"), &root);
        assert_eq!(reply.status, 400, "{query}: {}", reply.body);
        assert_eq!(reply.json()["error"]["code"], "bad_request");
    }
    let ana = team.token("ana", &["team-billing"], false);
    let bob = team.token("bob", &["team-docs"], false);
    for token in [&ana, &bob] {
        let reply = ask(&console, "/api/v1/audit", token);
        assert_eq!(reply.status, 403, "{}", reply.body);
        assert_eq!(
            reply.json()["error"],
            json!({"code": "forbidden", "message": "console writes require an authenticated operator with a subject"})
        );
    }

    // A row that fails its checks (a role, then a login outside its pattern) is left out, one
    // stderr line naming the table and no value.
    audit.admin.run(&format!(
        "INSERT INTO \"{0}\".audit (via, subject, role, action, arguments, outcome, message) \
         VALUES ('console', 'mallory-328', 'admin', 'pointer.set', '{{}}', 'accepted', 'x');
         INSERT INTO \"{0}\".audit (via, subject, login, role, action, arguments, outcome, \
         message) VALUES ('console', 'mallory-332', 'Mallory.332', 'operator', 'pointer.set', \
         '{{}}', 'accepted', 'x')",
        audit.schema
    ));
    let page = ask(&console, "/api/v1/audit", &root).json();
    assert_eq!(page["records"].as_array().unwrap(), &records);

    let line = |action: &str, arguments: &str, outcome: &str| {
        format!("ui: root {action} {arguments}: {outcome}")
    };
    let skipped = format!(
        "ui: \"{}\".audit: a record fails its checks and is left out",
        audit.schema
    );
    let lines = audit_lines(console);
    assert_eq!(
        lines,
        [
            line("pointer retain", &g1, &g1),
            line("pointer set", &g1, &set),
            line("pointer drop", &g2, &g2),
            line("fence recover", "50", "50"),
            line("workspace save", &saved, &saved),
            line("workspace apply", &saved, &applied),
            line("pointer set", &g2, holder),
            skipped.clone(),
            skipped,
        ]
    );
    assert!(!lines.concat().to_lowercase().contains("mallory"));
    let all = format!(
        "{}{}",
        lines.join("\n"),
        serde_json::to_string(&records).unwrap()
    );
    assert_leaks_nothing(&audit, &all, &[&root, &ana, &bob]);
}

/// The lines of `console` that start with `ui: `, once it is killed.
fn audit_lines(console: Server) -> Vec<String> {
    audit(console)
}

/// A team operator's accepted save and apply record `team_operator` and the owners of the touched
/// entries; its refused save records `refused`, its teams and the message with the named
/// identities and the hidden count. A viewer's write and a team operator's `pointer` or
/// `fence/recover` leave one `refused` record with the route's action and no argument, answered
/// before their bodies arrive.
#[test]
fn team_writes_and_refusals_before_the_body_are_recorded() {
    let Some((team, audit, console)) = audited_team("ui_audit_team") else {
        return;
    };
    let ana = team.token("ana", &["team-billing"], false);
    let bob = team.token("bob", &["team-docs"], false);
    let text = format!(
        "[[repository]]\nrepo = \"{BILLING}\"\npath = \"../changed\"\nowner = \"team-billing\"\n\n\
         [[repository]]\nrepo = \"{LEDGER}\"\npath = \"../e.test/team/ledger\"\nowner = \"team-billing\"\n\n\
         [[repository]]\nrepo = \"{SHARED}\"\npath = \"../e.test/team/shared\"\n\n\
         [[repository]]\nrepo = \"{JOINT}\"\npath = \"../e.test/team/joint\"\nowner = \"team-search\"\n"
    );
    let reply = post(
        &console,
        "workspace/versions",
        &ana,
        json!({"entries": text, "message": "m"}),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    let saved = reply.json()["result"].as_str().unwrap().to_string();
    let reply = post(&console, "workspace/apply", &ana, json!({"version": saved}));
    assert_eq!(reply.status, 200, "{}", reply.body);
    let applied = reply.json()["result"].as_str().unwrap().to_string();
    // The joint entry is team-search's: refused, naming it.
    let other = text.replace("../e.test/team/joint", "../moved");
    let reply = post(
        &console,
        "workspace/versions",
        &ana,
        json!({"entries": other, "message": "m"}),
    );
    assert_eq!(reply.status, 403, "{}", reply.body);
    let error = reply.json()["error"].clone();
    let named: Vec<&str> = (error["refused"].as_array().unwrap().iter())
        .map(|r| r["identity"].as_str().unwrap())
        .collect();
    let refused = format!(
        "{} ({}; hidden {})",
        error["message"].as_str().unwrap(),
        named.join(", "),
        error["hidden"]
    );
    assert_eq!(named, [JOINT]);
    for (route, token) in [
        ("workspace/apply", &bob),
        ("pointer", &ana),
        ("fence/recover", &ana),
    ] {
        assert_eq!(unfinished(&console, route, token), 403, "{route}");
    }

    let records = audit.records();
    let teams = json!(["team-billing"]);
    let found: Vec<_> = (records.iter())
        .map(|r| {
            (
                r["subject"].clone(),
                r["role"].clone(),
                r["owners"].clone(),
                r["action"].clone(),
                r["outcome"].clone(),
                r["message"].clone(),
            )
        })
        .collect();
    let operator = "this action needs the global operator: a team operator may only save and apply workspace versions";
    let viewer = "console writes require an authenticated operator with a subject";
    assert_eq!(
        found,
        [
            (
                json!("ana"),
                json!("team_operator"),
                teams.clone(),
                json!("workspace.save"),
                json!("accepted"),
                json!(saved)
            ),
            (
                json!("ana"),
                json!("team_operator"),
                teams.clone(),
                json!("workspace.apply"),
                json!("accepted"),
                json!(applied)
            ),
            (
                json!("ana"),
                json!("team_operator"),
                teams.clone(),
                json!("workspace.save"),
                json!("refused"),
                json!(refused)
            ),
            (
                json!("bob"),
                json!("viewer"),
                json!(null),
                json!("workspace.apply"),
                json!("refused"),
                json!(viewer)
            ),
            (
                json!("ana"),
                json!("team_operator"),
                teams.clone(),
                json!("pointer"),
                json!("refused"),
                json!(operator)
            ),
            (
                json!("ana"),
                json!("team_operator"),
                teams.clone(),
                json!("fence.recover"),
                json!("refused"),
                json!(operator)
            ),
        ]
    );
    // The save's arguments name the version as submitted, before the composition.
    assert_eq!(records[0]["arguments"]["message"], "m");
    assert!(meada_core::ids::is_id(
        records[0]["arguments"]["version"].as_str().unwrap()
    ));
    assert_eq!(records[1]["arguments"], json!({"version": saved}));
    for record in &records[3..] {
        assert_eq!(record["arguments"], json!({}));
    }
    let lines = audit_lines(console);
    let ana_as = "ui: ana as [\"team-billing\"]";
    assert_eq!(
        lines[0],
        format!("{ana_as} workspace save {saved}: {saved}")
    );
    assert_eq!(
        lines[1],
        format!("{ana_as} workspace apply {saved}: {applied}")
    );
    assert_eq!(lines[2], format!("{ana_as} workspace save -: {refused}"));
    assert_eq!(lines[3], format!("ui: bob workspace apply -: {viewer}"));
    assert_eq!(lines[4], format!("{ana_as} pointer -: {operator}"));
    assert_eq!(lines[5], format!("{ana_as} fence recover -: {operator}"));
    assert_eq!(lines.len(), 6);
    let all = format!(
        "{}{}",
        lines.join("\n"),
        serde_json::to_string(&records).unwrap()
    );
    assert_leaks_nothing(&audit, &all, &[&ana, &bob]);
}

/// With Postgres stopped (the relay closed), every write answers 503 `audit_unavailable` and
/// Valkey and the store are unchanged; reads and `session` still answer, and so does a console
/// started then; a viewer's write is still the role refusal with its stderr line, and no record.
/// On a schema version this binary does not know, the console refuses to start, naming both.
#[test]
fn without_postgres_no_write_runs_and_reads_still_answer() {
    let Some((team, audit, console)) = audited_team("ui_audit_down") else {
        return;
    };
    let dir = &team.fleet.dir;
    let g1 = locked_publish(dir);
    let root = team.token("root", &[], true);
    let bob = team.token("bob", &["team-docs"], false);
    let state = |console: &Server| {
        ["fleet", "store", "workspace", "workspace/versions"]
            .map(|route| ask(console, &format!("/api/v1/{route}"), &root).json())
    };
    let before = state(&console);
    audit.relay.close();
    let writes = [
        ("pointer", json!({"action": "retain", "generation": g1})),
        ("pointer", json!({"action": "set", "generation": g1})),
        ("pointer", json!({"action": "drop", "generation": g1})),
        ("fence/recover", json!({})),
        (
            "workspace/versions",
            json!({"entries": entries(&ENTRIES[..2]), "message": "m"}),
        ),
        ("workspace/apply", json!({"version": team.v1})),
    ];
    let unavailable = "the audit in Postgres is unavailable: no write is made without its record";
    for (route, body) in &writes {
        let reply = post(&console, route, &root, body.clone());
        assert_eq!(reply.status, 503, "{route}: {}", reply.body);
        assert_eq!(
            reply.json()["error"],
            json!({"code": "audit_unavailable", "message": unavailable})
        );
    }
    let viewer = post(
        &console,
        "workspace/apply",
        &bob,
        json!({"version": team.v1}),
    );
    assert_eq!(viewer.status, 403, "{}", viewer.body);
    assert_eq!(state(&console), before);
    assert_eq!(
        ask(&console, "/api/v1/session", &root).json()["role"],
        "operator"
    );
    let page = ask(&console, "/api/v1/audit", &root);
    assert_eq!(
        (page.status, page.json()["error"]["code"].clone()),
        (503, json!("audit_unavailable"))
    );
    // A console started now starts, and serves its reads.
    let again = start(&team.fleet);
    assert_eq!(ask(&again, "/api/v1/fleet", &root).status, 200);
    assert!(audit.records().is_empty());

    let lines = console.stderr();
    let lines: Vec<&str> = lines.lines().filter(|l| l.starts_with("ui: ")).collect();
    let unreachable = lines
        .iter()
        .filter(|l| l.starts_with("ui: postgres: unreachable ("))
        .count();
    assert_eq!(unreachable, writes.len() + 1, "{lines:?}");
    let written: Vec<_> = lines
        .iter()
        .filter(|l| !l.starts_with("ui: postgres: "))
        .copied()
        .collect();
    assert_eq!(written.len(), writes.len() + 1, "{written:?}");
    assert_eq!(
        written[0],
        format!("ui: root pointer retain {g1}: {unavailable}")
    );
    assert_eq!(
        written[3],
        format!("ui: root fence recover -: {unavailable}")
    );
    assert_eq!(
        written[6],
        "ui: bob workspace apply -: console writes require an authenticated operator with a subject"
    );
    assert_leaks_nothing(&audit, &lines.join("\n"), &[&root, &bob]);

    // A schema this binary does not know refuses the start, naming both versions.
    fs::write(dir.join("ops/pg-url"), &audit.url).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(dir)
        .args([
            "ui",
            "--listen",
            "127.0.0.1:0",
            "--workspace",
            "ops/console.toml",
        ])
        .env("MEADA_POSTGRES_VERSION_MAX", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    let higher = format!(
        "the schema {} is at version 5, above this Meada's 1: upgrade Meada",
        audit.schema
    );
    assert!(stderr.contains(&higher), "{stderr}");
    audit.assert_no_url(&stderr);
}
