//! The team operator (ADR 0015, #324): the role admission computes from the token's groups, its
//! grant and the current workspace version, `session`'s fields, the save and apply of its teams'
//! entries, `entries_refused` and the owners in the stderr line. Real Valkey and store, like
//! `writes`, and the `oidc` module's loopback issuer minting tokens with chosen `groups`.

use super::{PUBLIC, VERSION, ask, bearer, claims, oidc};
use crate::generations::{Fleet, admin, prefix, send, workspace};
use crate::manifest::MANIFEST;
use crate::oidc::{Issuer, key, set, sign};
use crate::server::Server;
use crate::snapshot::{KEY_ID, append_section};
use crate::ui::{Reply, front_end, send_body};
use jsonwebtoken::{Algorithm, EncodingKey};
use serde_json::{Value, json};
use std::fs;

pub(super) const BILLING: &str = "e.test/team/billing";
pub(super) const LEDGER: &str = "e.test/team/ledger";
pub(super) const SEARCH: &str = "e.test/team/search";
pub(super) const SHARED: &str = "e.test/team/shared";
pub(super) const JOINT: &str = "e.test/team/joint";
pub(super) const VAULT: &str = "e.test/team/vault";
pub(super) const DOCS: &str = "e.test/team/docs";
pub(super) const INVOICES: &str = "e.test/team/invoices";

/// Version 1: who owns what. `team-billing` reads billing, ledger, shared, joint and invoices;
/// `team-search` reads search and joint; `team-docs` reads nothing; the vault is read by nobody.
pub(super) const ENTRIES: [(&str, Option<&str>); 7] = [
    (BILLING, Some("team-billing")),
    (LEDGER, Some("team-billing")),
    (SEARCH, Some("team-search")),
    (SHARED, None),
    (JOINT, Some("team-search")),
    (VAULT, Some("team-billing")),
    (DOCS, Some("team-docs")),
];

pub(super) fn entries(list: &[(&str, Option<&str>)]) -> String {
    let entry = |(repo, owner): &(&str, Option<&str>)| {
        let owner = owner.map_or(String::new(), |o| format!("owner = \"{o}\"\n"));
        format!("[[repository]]\nrepo = \"{repo}\"\npath = \"../{repo}\"\n{owner}")
    };
    list.iter().map(entry).collect()
}

/// The `[oidc]` of the team console: its groups table is the grant of each team.
fn team_oidc(issuer: &str) -> String {
    let mapped = oidc(issuer)
        .split_once("[oidc.groups]")
        .unwrap()
        .0
        .to_string();
    format!(
        "{mapped}[oidc.groups]\n\"team-billing\" = [\"{BILLING}\", \"{LEDGER}\", \"{SHARED}\", \
         \"{JOINT}\", \"{INVOICES}\"]\n\"team-search\" = [\"{SEARCH}\", \"{JOINT}\"]\n"
    )
}

pub(super) struct Team {
    pub(super) fleet: Fleet,
    encoding: EncodingKey,
    issuer: Issuer,
    pub(super) console: Server,
    /// The version applied before the console started.
    pub(super) v1: String,
}

/// A fleet whose current workspace version is `ENTRIES` (imported and applied through the CLI)
/// and a console over its store with `operations`.
pub(super) fn team(case: &'static str, operations: bool) -> Option<Team> {
    let fleet = Fleet::new(case, 2)?;
    let (encoding, jwk) = key("team");
    let issuer = Issuer::new(set(&[&jwk]));
    append_section(&fleet.dir, "\n[refresh_lock]\nttl_seconds = 10\n");
    fs::write(fleet.dir.join("ops/entries.toml"), entries(&ENTRIES)).unwrap();
    let (v1, _) = crate::generations::import(&fleet.dir, "ops/entries.toml", "version 1", "cli");
    let (stdout, stderr, code) = workspace(&fleet.dir, &["apply", &v1]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.starts_with(&v1), "{stdout}");
    let source = fs::read_to_string(fleet.dir.join(MANIFEST)).unwrap();
    let sections = source.split_once("[snapshot]").unwrap().1;
    let bootstrap = format!(
        "index_dir = \"../console-indexes\"\n[workspace]\nsource = \"store\"\n[snapshot]{sections}\n\
         {}\n[ui]\ndir = \"../ui\"\npublic_url = \"{PUBLIC}\"\nclient_id = \"meada-console\"\n\
         operations = {operations}\noperator_claim = \"operator\"\n",
        team_oidc(&issuer.base),
    );
    fs::write(fleet.dir.join("ops/console.toml"), bootstrap).unwrap();
    front_end(&fleet.dir, Some(VERSION));
    let console = start(&fleet);
    Some(Team {
        fleet,
        encoding,
        issuer,
        console,
        v1,
    })
}

pub(super) fn start(fleet: &Fleet) -> Server {
    Server::start_command(
        &fleet.dir,
        "ui",
        &["--listen", "127.0.0.1:0", "--workspace", "ops/console.toml"],
        &[("MEADA_TEST_S3_KEY", KEY_ID)],
    )
}

impl Team {
    pub(super) fn token(&self, subject: &str, groups: &[&str], operator: bool) -> String {
        let mut claims = claims(&self.issuer.base, &[]);
        claims["sub"] = json!(subject);
        claims["groups"] = json!(groups);
        if operator {
            claims["operator"] = json!(true);
        }
        sign(&self.encoding, "team", Algorithm::ES256, &claims)
    }

    fn post(&self, route: &str, token: &str, body: Value) -> Reply {
        send_body(
            &self.console,
            "POST",
            &format!("/api/v1/{route}"),
            &[("Authorization", &bearer(token)), ("Origin", PUBLIC)],
            &body.to_string(),
        )
    }

    fn save(&self, token: &str, text: &str) -> Reply {
        self.post(
            "workspace/versions",
            token,
            json!({"entries": text, "message": "team save"}),
        )
    }

    fn apply(&self, token: &str, version: &str) -> Reply {
        self.post("workspace/apply", token, json!({"version": version}))
    }

    fn get(&self, route: &str, token: &str) -> Value {
        ask(&self.console, &format!("/api/v1/{route}"), token).json()
    }

    /// The current version's id, as the global operator reads it.
    fn current(&self, operator: &str) -> String {
        self.get("workspace", operator)["current"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn versions(&self, operator: &str) -> usize {
        self.get("workspace/versions", operator)["versions"]
            .as_array()
            .unwrap()
            .len()
    }

    /// The pointer key in Valkey.
    fn pointer(&self) -> String {
        format!("{{{}}}:workspace", prefix(self.fleet.case))
    }
}

/// The audit lines a console wrote, once it is killed.
pub(super) fn audit(console: Server) -> Vec<String> {
    let stderr = console.stderr();
    stderr
        .lines()
        .filter(|line| line.starts_with("ui: "))
        .map(String::from)
        .collect()
}

fn refusals(reply: &Reply) -> (Value, Value) {
    assert_eq!(reply.status, 403, "{}", reply.body);
    let error = &reply.json()["error"];
    assert_eq!(error["code"], "entries_refused", "{}", reply.body);
    (error["refused"].clone(), error["hidden"].clone())
}

/// The role is the global operator's, a team operator's or a viewer's, per request: an owning
/// group whose grant reads the entry makes a team operator; `operations = false`, a missing
/// or unreadable pointer, or a file workspace make a viewer; the fields follow #294's formats.
#[test]
fn the_session_role_follows_the_groups_the_grant_and_the_current_version() {
    let Some(team) = team("ui_team_session", true) else {
        return;
    };
    let ana = team.token("ana", &["team-billing", "eng"], false);
    let session = team.get("session", &ana);
    assert_eq!(
        session,
        json!({"subject": "ana", "role": "team_operator", "groups": ["eng", "team-billing"],
               "grant": [BILLING, INVOICES, JOINT, LEDGER, SHARED], "teams": ["team-billing"]})
    );
    let both = team.token("cy", &["team-search", "team-billing"], false);
    let session = team.get("session", &both);
    assert_eq!(session["role"], "team_operator");
    assert_eq!(session["teams"], json!(["team-billing", "team-search"]));
    // Owns only an entry outside its grant, or nothing at all.
    for (subject, groups) in [("bob", &["team-docs"][..]), ("dee", &["other"][..])] {
        let session = team.get("session", &team.token(subject, groups, false));
        assert_eq!(session["role"], "viewer");
        assert_eq!(session["groups"], json!(groups));
        assert_eq!(session["grant"], json!([]));
        assert!(session.get("teams").is_none());
    }
    // The global operator stays one whatever it owns.
    let operator = team.token("root", &["team-billing"], true);
    let session = team.get("session", &operator);
    assert_eq!(session["role"], "operator");
    assert_eq!(
        session["grant"],
        json!([BILLING, INVOICES, JOINT, LEDGER, SHARED])
    );
    assert!(session.get("teams").is_none());
    // No pointer, a pointer to a missing document, a malformed pointer: a viewer, never a write.
    let pointer = team.pointer();
    send(&mut admin(&team.fleet.valkey), &["DEL", &pointer]);
    assert_eq!(team.get("session", &ana)["role"], "viewer");
    let hset = |id: &str, fence: &str| {
        let mut valkey = admin(&team.fleet.valkey);
        send(&mut valkey, &["HSET", &pointer, "id", id, "fence", fence]);
    };
    hset("ffffffffffffffff", "9");
    assert_eq!(team.get("session", &ana)["role"], "viewer");
    hset("not-an-id", "9");
    assert_eq!(team.get("session", &ana)["role"], "viewer");
    hset(&team.v1, "2");
    assert_eq!(team.get("session", &ana)["role"], "team_operator");
    // Operations off: everyone who is not the operator stays a viewer.
    let source = fs::read_to_string(team.fleet.dir.join("ops/console.toml")).unwrap();
    let off = source.replace("operations = true", "operations = false");
    fs::write(team.fleet.dir.join("ops/console.toml"), off).unwrap();
    let console = start(&team.fleet);
    let session = ask(&console, "/api/v1/session", &ana).json();
    assert_eq!(session["role"], "viewer");
    assert_eq!(session["groups"], json!(["eng", "team-billing"]));
}

/// With a file workspace nobody is a team operator, though the owners are recorded.
#[test]
fn a_file_workspace_has_no_team_operator() {
    let Some(team) = team("ui_team_file", true) else {
        return;
    };
    let source = fs::read_to_string(team.fleet.dir.join(MANIFEST)).unwrap();
    let file = format!(
        "{source}\n{}\n[ui]\ndir = \"../ui\"\npublic_url = \"{PUBLIC}\"\nclient_id = \
         \"meada-console\"\noperations = true\noperator_claim = \"operator\"\n",
        team_oidc(&team.issuer.base)
    );
    fs::write(team.fleet.dir.join("ops/console.toml"), file).unwrap();
    let console = start(&team.fleet);
    let ana = team.token("ana", &["team-billing"], false);
    assert_eq!(
        ask(&console, "/api/v1/session", &ana).json()["role"],
        "viewer"
    );
    let post = send_body(
        &console,
        "POST",
        "/api/v1/workspace/versions",
        &[("Authorization", &bearer(&ana)), ("Origin", PUBLIC)],
        "{}",
    );
    assert_eq!(post.status, 403);
}

/// A team operator sees a viewer's view: its grant bounds the versions, the team's entries
/// outside the grant included; every role sees each entry's owner.
#[test]
fn a_team_operator_sees_its_grant_and_every_role_sees_the_owners() {
    let Some(team) = team("ui_team_views", true) else {
        return;
    };
    let operator = team.token("root", &[], true);
    let ana = team.token("ana", &["team-billing"], false);
    let viewer = team.token("dee", &["team-search"], false);
    let owners = |entries: &Value| -> Vec<(String, Option<String>)> {
        (entries.as_array().unwrap().iter())
            .map(|e| {
                let owner = e["owner"].as_str().map(String::from);
                (e["repo"].as_str().unwrap().to_string(), owner)
            })
            .collect()
    };
    let of = |role: &str| {
        team.get(&format!("workspace/versions/{}", team.v1), role)["repositories"].clone()
    };
    let whole = owners(&of(&operator));
    assert_eq!(whole.len(), 7);
    assert_eq!(whole[0], (BILLING.into(), Some("team-billing".into())));
    assert_eq!(whole[3], (SHARED.into(), None));
    let seen = owners(&of(&ana));
    let expected: Vec<_> = (whole.iter())
        .filter(|(repo, _)| [BILLING, LEDGER, SHARED, JOINT].contains(&repo.as_str()))
        .cloned()
        .collect();
    assert_eq!(seen, expected);
    // Its own team's vault entry is absent from every bounded answer.
    for route in ["workspace", "workspace/versions"] {
        let body = ask(&team.console, &format!("/api/v1/{route}"), &ana).body;
        assert!(
            !body.contains(VAULT) && !body.contains(SEARCH),
            "{route}: {body}"
        );
    }
    let viewer_sees = owners(&of(&viewer));
    assert_eq!(
        viewer_sees,
        [
            (SEARCH.into(), Some("team-search".into())),
            (JOINT.into(), Some("team-search".into()))
        ]
    );
}

/// A save that edits, removes and adds entries of its team stores the composed version with the
/// subject as author; its apply moves the pointer; an older version applied rolls back.
#[test]
fn a_team_operator_saves_and_applies_its_entries() {
    let Some(team) = team("ui_team_save", true) else {
        return;
    };
    let operator = team.token("root", &[], true);
    let ana = team.token("ana", &["team-billing"], false);
    let text = format!(
        "[[repository]]\nrepo = \"{BILLING}\"\npath = \"../changed\"\nowner = \"team-billing\"\n\n\
         [[repository]]\nrepo = \"{SHARED}\"\npath = \"../e.test/team/shared\"\n\n\
         [[repository]]\nrepo = \"{JOINT}\"\npath = \"../e.test/team/joint\"\nowner = \"team-search\"\n\n\
         [[repository]]\nrepo = \"{INVOICES}\"\npath = \"../invoices\"\nowner = \"team-billing\"\n"
    );
    let reply = team.save(&ana, &text);
    assert_eq!(reply.status, 200, "{}", reply.body);
    let saved = reply.json()["result"].as_str().unwrap().to_string();
    let version = team.get(&format!("workspace/versions/{saved}"), &operator);
    assert_eq!(version["author"], "ana");
    let repos: Vec<_> = (version["repositories"].as_array().unwrap().iter())
        .map(|e| e["repo"].as_str().unwrap())
        .collect();
    // Ledger removed, the entries outside the grant kept in place, the addition appended.
    assert_eq!(
        repos,
        [BILLING, SEARCH, SHARED, JOINT, VAULT, DOCS, INVOICES]
    );
    assert_eq!(version["repositories"][0]["path"], "../changed");
    assert_eq!(team.current(&operator), team.v1);
    let reply = team.apply(&ana, &saved);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(team.current(&operator), saved);
    // An older version applied is checked against the current one: its own entries roll back.
    let reply = team.apply(&ana, &team.v1);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(team.current(&operator), team.v1);
    let lines = audit(team.console);
    let team_billing = "ui: ana as [\"team-billing\"]";
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(
        lines[0],
        format!("{team_billing} workspace save {saved}: {saved}")
    );
    assert!(lines[1].starts_with(&format!("{team_billing} workspace apply {saved}: {saved} ")));
    assert!(lines[2].starts_with(&format!(
        "{team_billing} workspace apply {}: {} ",
        team.v1, team.v1
    )));
}

/// A team operator in two teams changes an entry of each in one save; the owners of the line are
/// both, sorted.
#[test]
fn a_team_operator_in_two_teams_changes_an_entry_of_each() {
    let Some(team) = team("ui_team_two", true) else {
        return;
    };
    let cy = team.token("cy", &["team-search", "team-billing"], false);
    let text = format!(
        "[[repository]]\nrepo = \"{BILLING}\"\npath = \"../one\"\nowner = \"team-billing\"\n\n\
         [[repository]]\nrepo = \"{LEDGER}\"\npath = \"../e.test/team/ledger\"\nowner = \"team-billing\"\n\n\
         [[repository]]\nrepo = \"{SEARCH}\"\npath = \"../two\"\nowner = \"team-search\"\n\n\
         [[repository]]\nrepo = \"{SHARED}\"\npath = \"../e.test/team/shared\"\n\n\
         [[repository]]\nrepo = \"{JOINT}\"\npath = \"../e.test/team/joint\"\nowner = \"team-search\"\n"
    );
    let reply = team.save(&cy, &text);
    assert_eq!(reply.status, 200, "{}", reply.body);
    let id = reply.json()["result"].as_str().unwrap().to_string();
    assert_eq!(
        audit(team.console),
        [format!(
            "ui: cy as [\"team-billing\",\"team-search\"] workspace save {id}: {id}"
        )]
    );
}

/// Each way a save can exceed the team's rights is a 403 `entries_refused` naming the entry, its
/// change and its reason, and writes nothing: no version, no pointer move, no lock left held.
#[test]
fn a_save_beyond_the_team_s_rights_is_refused_and_writes_nothing() {
    let Some(team) = team("ui_team_refused", true) else {
        return;
    };
    let operator = team.token("root", &[], true);
    let ana = team.token("ana", &["team-billing"], false);
    // What ana reads, as she would submit it unchanged.
    let seen: Vec<_> = (ENTRIES.iter())
        .filter(|(repo, _)| [BILLING, LEDGER, SHARED, JOINT].contains(repo))
        .copied()
        .collect();
    let base = entries(&seen);
    let path = |repo: &str| format!("path = \"../{repo}\"");
    let add = |repo: &str, owner: &str| {
        format!("{base}[[repository]]\nrepo = \"{repo}\"\npath = \"../v\"\nowner = \"{owner}\"\n")
    };
    let cases = [
        // Another owner's entry.
        (
            base.replace(&path(JOINT), "path = \"../j\""),
            JOINT,
            "edit",
            "not_your_team",
        ),
        // An owner changed, cleared included.
        (
            base.replacen("owner = \"team-billing\"", "owner = \"team-search\"", 1),
            BILLING,
            "edit",
            "owner_change",
        ),
        // An ownerless entry.
        (
            base.replace(&path(SHARED), "path = \"../s\""),
            SHARED,
            "edit",
            "ownerless",
        ),
        // An identity outside the grant, which the team's own vault is.
        (add(VAULT, "team-billing"), VAULT, "add", "outside_grant"),
        (add(SEARCH, "team-search"), SEARCH, "add", "outside_grant"),
    ];
    let before = (team.current(&operator), team.versions(&operator));
    for (text, identity, change, reason) in cases {
        let reply = team.save(&ana, &text);
        let (refused, hidden) = refusals(&reply);
        assert_eq!(hidden, 0, "{}", reply.body);
        assert!(
            refused
                .as_array()
                .unwrap()
                .contains(&json!({"identity": identity, "change": change, "reason": reason})),
            "{identity}: {}",
            reply.body
        );
        assert_eq!((team.current(&operator), team.versions(&operator)), before);
    }
    // The lock was released each time: a save that is allowed still goes through.
    let ok = team.save(&ana, &base.replace(&path(BILLING), "path = \"../b\""));
    assert_eq!(ok.status, 200, "{}", ok.body);
    let lines = audit(team.console);
    assert_eq!(lines.len(), 6);
    assert!(lines[0].starts_with("ui: ana as [\"team-billing\"] workspace save -: this change touches 1 entry you may not change: nothing applied"), "{}", lines[0]);
    assert!(
        lines[0].ends_with(&format!("({JOINT}; hidden 0)")),
        "{}",
        lines[0]
    );
}

/// An apply names the refused entries its grant reads and counts the others; nothing moves.
#[test]
fn an_apply_beyond_the_team_s_rights_names_only_what_it_reads() {
    let Some(team) = team("ui_team_apply_refused", true) else {
        return;
    };
    let operator = team.token("root", &[], true);
    let ana = team.token("ana", &["team-billing"], false);
    // The global operator may change anything: the joint entry, the search team's and the vault.
    let changed: Vec<_> = ENTRIES.iter().map(|&(repo, owner)| (repo, owner)).collect();
    let mut text = entries(&changed);
    for repo in [JOINT, SEARCH, VAULT] {
        text = text.replace(&format!("path = \"../{repo}\""), "path = \"../moved\"");
    }
    let saved = team.save(&operator, &text);
    assert_eq!(saved.status, 200, "{}", saved.body);
    let id = saved.json()["result"].as_str().unwrap().to_string();
    let reply = team.apply(&ana, &id);
    let (refused, hidden) = refusals(&reply);
    assert_eq!(
        refused,
        json!([{"identity": JOINT, "change": "edit", "reason": "not_your_team"}])
    );
    assert_eq!(hidden, 2);
    assert_eq!(team.current(&operator), team.v1);
    assert!(
        !reply.body.contains(SEARCH) && !reply.body.contains(VAULT),
        "{}",
        reply.body
    );
    // The lock is free: the global operator applies the same version.
    assert_eq!(team.apply(&operator, &id).status, 200);
    let lines = audit(team.console);
    assert_eq!(lines.len(), 3);
    assert!(
        lines[1].starts_with(&format!(
            "ui: ana as [\"team-billing\"] workspace apply {id}: "
        )),
        "{}",
        lines[1]
    );
    assert!(
        lines[1].ends_with(&format!("({JOINT}; hidden 2)")),
        "{}",
        lines[1]
    );
    assert!(!lines[1].contains(SEARCH) && !lines[1].contains(VAULT));
}

/// A version saved against an older current one is checked against the current one.
#[test]
fn an_apply_is_checked_against_the_version_current_then() {
    let Some(team) = team("ui_team_current", true) else {
        return;
    };
    let operator = team.token("root", &[], true);
    let ana = team.token("ana", &["team-billing"], false);
    let own = |path: &str| {
        let seen: Vec<_> = (ENTRIES.iter())
            .filter(|(repo, _)| [BILLING, LEDGER, SHARED, JOINT].contains(repo))
            .copied()
            .collect();
        entries(&seen).replace(
            &format!("path = \"../{BILLING}\""),
            &format!("path = \"../{path}\""),
        )
    };
    let saved = team.save(&ana, &own("mine"));
    assert_eq!(saved.status, 200, "{}", saved.body);
    let mine = saved.json()["result"].as_str().unwrap().to_string();
    // Meanwhile the global operator changes the ownerless entry and applies it.
    let other = entries(&ENTRIES).replace(
        &format!("path = \"../{SHARED}\""),
        "path = \"../elsewhere\"",
    );
    let other = team.save(&operator, &other).json()["result"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(team.apply(&operator, &other).status, 200);
    // Applying ana's version would revert the ownerless entry: refused against the current one.
    let reply = team.apply(&ana, &mine);
    let (refused, _) = refusals(&reply);
    assert_eq!(
        refused,
        json!([{"identity": SHARED, "change": "edit", "reason": "ownerless"}])
    );
    assert_eq!(team.current(&operator), other);
}

/// A team that removes its last entry is a viewer on its next request.
#[test]
fn a_team_that_removes_its_last_entry_is_a_viewer_next() {
    let Some(team) = team("ui_team_last", true) else {
        return;
    };
    let sam = team.token("sam", &["team-search"], false);
    // team-search owns the search and joint entries; sam reads both and removes both.
    let saved = team.save(&sam, "");
    assert_eq!(saved.status, 200, "{}", saved.body);
    let id = saved.json()["result"].as_str().unwrap().to_string();
    assert_eq!(team.apply(&sam, &id).status, 200);
    assert_eq!(team.get("session", &sam)["role"], "viewer");
    let reply = team.save(&sam, "");
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(
        reply.json()["error"],
        json!({"code": "forbidden", "message": "console writes require an authenticated operator with a subject"})
    );
    let lines = audit(team.console);
    assert_eq!(lines.len(), 3);
    assert_eq!(
        lines[2],
        "ui: sam workspace save -: console writes require an authenticated operator with a subject"
    );
}

/// The generation operations stay the global operator's: a team operator gets the role refusal
/// naming the global operator before its body is read, and a line with its teams.
#[test]
fn a_team_operator_cannot_move_generations() {
    let Some(team) = team("ui_team_generations", true) else {
        return;
    };
    let ana = team.token("ana", &["team-billing"], false);
    for route in ["pointer", "fence/recover"] {
        // A body that would be a 400 or a 413 if it were read.
        let reply = team.post(route, &ana, json!("not even the right shape"));
        assert_eq!(reply.status, 403, "{route}: {}", reply.body);
        let message = reply.json()["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(message.contains("global operator"), "{message}");
    }
    let lines = audit(team.console);
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert!(line.starts_with("ui: ana as [\"team-billing\"] "), "{line}");
        assert!(line.contains("global operator"), "{line}");
        assert!(!line.contains("Bearer") && !line.contains("eyJ"), "{line}");
    }
}
