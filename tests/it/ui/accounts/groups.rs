use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::administration::{act, administered, listed};
use super::{
    ADMIN_ENV, CHOSEN, ask, assert_leaks_nothing, keys, login, post, refusal, section, start, token,
};
use crate::manifest::MANIFEST;
use crate::postgres::{Audited, audited};
use crate::server::Server;
use crate::ui::playground::{CATALOG, GATEWAY, WEB, body, chain_console, query};
use crate::ui::{Reply, send, ui_section};

const PASSWORD: &str = "a long team passphrase";

fn table() -> String {
    format!(
        "\n[accounts.groups]\n\"team-web\" = [\"{WEB}\", \"{CATALOG}\"]\n\"team-gw\" = [\"{GATEWAY}\"]\n\
         \"team-all\" = [\"*\"]\n"
    )
}

fn accounts(
    dir: &Path,
    manifest: &str,
    case: &str,
    table: &str,
) -> Option<(Audited, (String, String))> {
    let audit = audited(dir, case)?;
    let keys = keys(dir);
    let file = dir.join(manifest);
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        format!("{text}{}{}{table}", audit.section, section()),
    )
    .unwrap();
    Some((audit, keys))
}

fn create(console: &Server, admin: &str, username: &str, role: &str, groups: &[&str]) -> Reply {
    let body = json!({"username": username, "role": role, "groups": groups, "password": PASSWORD,
                      "change_required": false});
    post(console, "accounts", Some(admin), &body.to_string())
}

fn signed_in(console: &Server, username: &str) -> String {
    token(&login(console, username, PASSWORD))
}

fn get(console: &Server, route: &str, token: &str) -> Value {
    let reply = ask(console, &format!("/api/v1/{route}"), token);
    assert_eq!(reply.status, 200, "{route}: {}", reply.body);
    reply.json()
}

fn entries(console: &Server, token: &str) -> Vec<String> {
    let manifest = get(console, "manifest", token);
    (manifest["manifest"]["repository"]
        .as_array()
        .unwrap()
        .iter())
    .map(|e| e["repo"].as_str().unwrap().to_string())
    .collect()
}

#[test]
fn a_local_viewer_s_groups_bound_every_view() {
    let case = "accounts_groups_views";
    let dir = chain_console(case, "", &ui_section());
    assert!(crate::manifest::run(&dir, "index").2, "index");
    let Some((audit, pems)) = accounts(&dir, MANIFEST, case, "") else {
        return;
    };
    let env = [(ADMIN_ENV, CHOSEN)];
    let console = start(&dir, &env);
    let admin = token(&login(&console, "admin", CHOSEN));
    let created = create(
        &console,
        &admin,
        "vera",
        "viewer",
        &["team-web", "team-web"],
    );
    assert_eq!(created.status, 200, "{}", created.body);
    assert_eq!(created.json()["account"]["groups"], json!(["team-web"]));
    let vera = signed_in(&console, "vera");
    let session = get(&console, "session", &vera);
    assert_eq!(
        session,
        json!({"subject": "local:vera", "role": "viewer", "groups": ["team-web"], "grant": "*",
               "account": true})
    );
    assert_eq!(entries(&console, &vera).len(), 4);
    drop(console);

    let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    fs::write(dir.join(MANIFEST), text + &table()).unwrap();
    let console = start(&dir, &env);
    let admin = token(&login(&console, "admin", CHOSEN));
    let vera = signed_in(&console, "vera");
    let session = get(&console, "session", &vera);
    assert_eq!(
        session,
        json!({"subject": "local:vera", "role": "viewer", "groups": ["team-web"],
               "grant": [CATALOG, WEB], "account": true})
    );
    assert_eq!(get(&console, "session", &admin)["grant"], json!([]));
    assert_eq!(get(&console, "session", &admin)["role"], "operator");

    assert_eq!(entries(&console, &vera), [WEB, CATALOG]);
    let manifest = get(&console, "manifest", &vera);
    assert_eq!(
        manifest["manifest"]["accounts"]["groups"],
        json!({"team-web": [WEB, CATALOG], "team-gw": [], "team-all": ["*"]})
    );
    assert!(!manifest.to_string().contains(GATEWAY), "{manifest}");
    assert_eq!(entries(&console, &admin).len(), 4);
    let whole = get(&console, "manifest", &admin);
    assert_eq!(
        whole["manifest"]["accounts"]["groups"]["team-gw"],
        json!([GATEWAY])
    );

    let admission = get(&console, "admission", &vera);
    assert_eq!(admission["accounts"]["mode"], "accounts");
    let fingerprint = admission["accounts"]["public_key"].as_str().unwrap();
    assert!(
        fingerprint.starts_with("blake3:") && fingerprint.len() == 7 + 64,
        "{fingerprint}"
    );
    assert_eq!(
        admission["groups"],
        json!({"team-web": [WEB, CATALOG], "team-gw": [], "team-all": ["*"]})
    );
    let admission = get(&console, "admission", &admin);
    assert_eq!(admission["groups"]["team-gw"], json!([GATEWAY]));
    assert_eq!(admission["accounts"]["public_key"], fingerprint);
    let check = post(
        &console,
        "admission/check",
        Some(&vera),
        r#"{"token": "x"}"#,
    );
    assert_eq!(check.status, 404, "{}", check.body);

    let repositories = ask(&console, "/api/v1/repositories", &vera);
    assert_eq!(repositories.status, 200, "{}", repositories.body);
    assert!(repositories.body.contains(WEB), "{}", repositories.body);
    assert!(
        !repositories.body.contains(GATEWAY),
        "{}",
        repositories.body
    );

    let bearer = format!("Bearer {vera}");
    let viewer = [("Authorization", bearer.as_str())];
    let trace = query(
        &console,
        &body(WEB, "trace", r#"{"from": "dashboard"}"#),
        &viewer,
    );
    assert_eq!(trace.status, 200, "{}", trace.body);
    assert!(!trace.body.contains(GATEWAY), "{}", trace.body);
    let export = send(
        &console,
        "GET",
        &format!("/api/v1/repositories/{WEB}/export"),
        &viewer,
    );
    assert_eq!(export.status, 200, "{}", export.body);
    assert!(!export.body.contains(GATEWAY), "{}", export.body);
    let asked = body(GATEWAY, "search", r#"{"query": "lib"}"#);
    assert_eq!(query(&console, &asked, &viewer).status, 404);
    let route = format!("/api/v1/repositories/{GATEWAY}/export");
    assert_eq!(send(&console, "GET", &route, &viewer).status, 404);

    for bad in [json!([" padded"]), json!([""]), json!(["line\nbreak"])] {
        let reply = act(
            &console,
            &admin,
            "vera",
            json!({"action": "groups", "groups": bad}),
        );
        assert_eq!(refusal(&reply), (422, "invalid_group".into()), "{bad}");
    }
    let refused = create(&console, &admin, "wes", "viewer", &[" padded"]);
    assert_eq!(refusal(&refused), (422, "invalid_group".into()));
    let changed = act(
        &console,
        &admin,
        "vera",
        json!({"action": "groups", "groups": ["team-gw", "team-gw"]}),
    );
    assert_eq!(changed.status, 200, "{}", changed.body);
    assert_eq!(changed.json()["account"]["groups"], json!(["team-gw"]));
    let vera_listed = (listed(&console, &admin).into_iter())
        .find(|a| a["username"] == "vera")
        .unwrap();
    assert_eq!(vera_listed["groups"], json!(["team-gw"]));
    let own = act(
        &console,
        &vera,
        "vera",
        json!({"action": "groups", "groups": ["team-all"]}),
    );
    assert_eq!(refusal(&own), (403, "forbidden".into()));
    let records = administered(&console, &admin);
    assert!(
        records.contains(&json!([
            "account.groups",
            "local:admin",
            "operator",
            {"account": "vera", "groups": "[\"team-gw\"]"},
            "accepted",
            "groups changed"
        ])),
        "{records:?}"
    );
    assert!(
        records.contains(&json!([
            "account.create",
            "local:admin",
            "operator",
            {"account": "vera", "role": "viewer", "groups": "[\"team-web\"]",
             "change_required": "false"},
            "accepted",
            "account created"
        ])),
        "{records:?}"
    );
    assert_eq!(
        get(&console, "session", &vera)["grant"],
        json!([CATALOG, WEB])
    );
    let again = signed_in(&console, "vera");
    assert_eq!(get(&console, "session", &again)["grant"], json!([GATEWAY]));
    assert_eq!(entries(&console, &again), [GATEWAY]);

    let records = records_text(&console, &admin);
    let stderr = console.stderr();
    let tokens = [admin.as_str(), vera.as_str(), again.as_str()];
    for text in [&stderr, &records] {
        assert_leaks_nothing(&audit, &pems, text, &[PASSWORD, CHOSEN], &tokens);
    }
}

fn records_text(console: &Server, admin: &str) -> String {
    get(console, "audit?limit=100", admin).to_string()
}

#[cfg(feature = "refresh-lock")]
#[test]
fn a_local_viewer_whose_group_owns_an_entry_is_a_team_operator() {
    use crate::generations::{Fleet, import, workspace};
    #[cfg(feature = "github-app")]
    use crate::generations::{admin as valkey, prefix, send as seed};
    use crate::manifest::CHAIN;
    use crate::snapshot::{KEY_ID, append_section};
    const WORKER: &str = "example.com/chain/worker";
    let case = "accounts_groups_team";
    let Some(fleet) = Fleet::new(case, 2) else {
        return;
    };
    let generation = fleet.publish();
    append_section(&fleet.dir, "\n[refresh_lock]\nttl_seconds = 10\n");
    let owners = [Some("team-web"), Some("team-gw"), None, Some("team-gw")];
    let entry = |(repo, path): (&str, &str), owner: Option<&str>, at: &str| {
        let owner = owner.map_or(String::new(), |o| format!("owner = \"{o}\"\n"));
        format!("[[repository]]\nrepo = \"{repo}\"\npath = \"../{at}{path}\"\n{owner}")
    };
    let all: String = (CHAIN.iter().zip(owners))
        .map(|(chain, owner)| entry(*chain, owner, ""))
        .collect();
    fs::write(fleet.dir.join("ops/entries.toml"), &all).unwrap();
    let (v1, _) = import(&fleet.dir, "ops/entries.toml", "version 1", "cli");
    let (_, stderr, code) = workspace(&fleet.dir, &["apply", &v1]);
    assert_eq!(code, Some(0), "{stderr}");
    let source = fs::read_to_string(fleet.dir.join(MANIFEST)).unwrap();
    let sections = source.split_once("[snapshot]").unwrap().1;
    let bootstrap = format!(
        "index_dir = \"../console-indexes\"\n[workspace]\nsource = \"store\"\n[snapshot]{sections}\n\
         [ui]\n{}operations = true\n",
        ui_section()
    );
    #[cfg(feature = "github-app")]
    let bootstrap = format!(
        "{bootstrap}\n[[github_app]]\nhost = \"example.com\"\napi = \"http://127.0.0.1:9\"\n\
         app_id = 1\nprivate_key_file = \"absent-private-key.pem\"\n"
    );
    fs::write(fleet.dir.join("ops/console.toml"), bootstrap).unwrap();
    let team = format!(
        "\n[accounts.groups]\n\"team-web\" = [\"{WEB}\", \"{CATALOG}\", \"{WORKER}\"]\n\
         \"team-gw\" = [\"{GATEWAY}\"]\n"
    );
    let Some((audit, pems)) = accounts(&fleet.dir, "ops/console.toml", case, &team) else {
        return;
    };
    crate::ui::front_end(&fleet.dir, Some(crate::ui::VERSION));
    #[cfg(feature = "github-app")]
    seed(
        &mut valkey(&fleet.valkey),
        &[
            "HSET",
            &format!(
                "{}:github-app:example.com:app:1:installations",
                prefix(case)
            ),
            "chain",
            "11",
            "private-owner",
            "22",
        ],
    );
    let args = ["--listen", "127.0.0.1:0", "--workspace", "ops/console.toml"];
    let env = [("MEADA_TEST_S3_KEY", KEY_ID), (ADMIN_ENV, CHOSEN)];
    let console = Server::start_command(&fleet.dir, "ui", &args, &env);
    let admin = token(&login(&console, "admin", CHOSEN));
    for (username, groups) in [("ana", "team-web"), ("bo", "team-docs")] {
        let created = create(&console, &admin, username, "viewer", &[groups]);
        assert_eq!(created.status, 200, "{}", created.body);
    }
    let ana = signed_in(&console, "ana");
    assert_eq!(
        get(&console, "session", &ana),
        json!({"subject": "local:ana", "role": "team_operator", "groups": ["team-web"],
               "grant": [CATALOG, WEB, WORKER], "teams": ["team-web"], "account": true})
    );
    let bo = get(&console, "session", &signed_in(&console, "bo"));
    assert_eq!((&bo["role"], &bo["grant"]), (&json!("viewer"), &json!([])));
    assert!(bo.get("teams").is_none(), "{bo}");
    let given = act(
        &console,
        &admin,
        "admin",
        json!({"action": "groups", "groups": ["team-web"]}),
    );
    assert_eq!(given.status, 200, "{}", given.body);
    let admin = token(&login(&console, "admin", CHOSEN));
    let session = get(&console, "session", &admin);
    assert_eq!(session["role"], "operator", "{session}");
    assert!(session.get("teams").is_none(), "{session}");

    for route in [
        "workspace".to_string(),
        "workspace/versions".to_string(),
        format!("workspace/versions/{v1}"),
        "store".to_string(),
        format!("store/generations/{generation}"),
        "repositories".to_string(),
        "fleet".to_string(),
    ] {
        let reply = ask(&console, &format!("/api/v1/{route}"), &ana);
        assert_eq!(reply.status, 200, "{route}: {}", reply.body);
        assert!(!reply.body.contains(GATEWAY), "{route}: {}", reply.body);
    }
    let version = get(&console, &format!("workspace/versions/{v1}"), &ana);
    assert!(version.to_string().contains(WEB), "{version}");
    #[cfg(feature = "github-app")]
    {
        let credentials = get(&console, "credentials", &ana);
        let installations = credentials["apps"][0]["installations"].as_array().unwrap();
        assert_eq!(installations.len(), 1, "{credentials}");
        assert_eq!(installations[0]["owner"], "chain");
        assert!(!credentials.to_string().contains("private-owner"));
    }

    let read = |path: &str| {
        let others = [2, 3].map(|n| entry(CHAIN[n], owners[n], "")).concat();
        entry(CHAIN[0], owners[0], path) + &others
    };
    let save = |token: &str, text: String| {
        let body = json!({"entries": text, "message": "team save"}).to_string();
        post(&console, "workspace/versions", Some(token), &body)
    };
    let saved = save(&ana, read("changed-"));
    assert_eq!(saved.status, 200, "{}", saved.body);
    let id = saved.json()["result"].as_str().unwrap().to_string();
    let stored = get(&console, &format!("workspace/versions/{id}"), &admin);
    assert_eq!(stored["author"], "local:ana", "{stored}");
    assert_eq!(
        stored["repositories"].as_array().unwrap().len(),
        4,
        "{stored}"
    );
    let ownerless = read("").replace("path = \"../chain-catalog\"", "path = \"../moved\"");
    let refused = save(&ana, ownerless);
    assert_eq!(refused.status, 403, "{}", refused.body);
    let error = &refused.json()["error"];
    assert_eq!(error["code"], "entries_refused", "{}", refused.body);
    assert!(
        (error["refused"].as_array().unwrap())
            .contains(&json!({"identity": CATALOG, "change": "edit", "reason": "ownerless"})),
        "{}",
        refused.body
    );
    let theirs = read("").replace("path = \"../chain-worker\"", "path = \"../moved\"");
    let refused = save(&ana, theirs);
    assert_eq!(refused.status, 403, "{}", refused.body);
    let error = &refused.json()["error"];
    assert!(
        (error["refused"].as_array().unwrap())
            .contains(&json!({"identity": WORKER, "change": "edit", "reason": "not_your_team"})),
        "{}",
        refused.body
    );

    let taken = act(
        &console,
        &admin,
        "ana",
        json!({"action": "groups", "groups": []}),
    );
    assert_eq!(taken.status, 200, "{}", taken.body);
    assert_eq!(taken.json()["account"]["groups"], json!([]));
    let after = save(&ana, read("again-"));
    assert_eq!(refusal(&after), (403, "forbidden".into()));
    assert_eq!(get(&console, "session", &ana)["role"], "team_operator");
    let next = signed_in(&console, "ana");
    let session = get(&console, "session", &next);
    assert_eq!(
        (&session["role"], &session["grant"]),
        (&json!("viewer"), &json!([]))
    );

    let stderr = console.stderr();
    assert_leaks_nothing(
        &audit,
        &pems,
        &stderr,
        &[PASSWORD, CHOSEN],
        &[&admin, &ana, &next],
    );
}

#[test]
fn the_groups_table_takes_arrays_of_identities() {
    use crate::manifest::run_args;
    use crate::ui::{VERSION, console_dir};
    let postgres = "\n[postgres]\nurl_env = \"MEADA_TEST_NO_POSTGRES_558\"\n";
    for (case, table, problem) in [
        ("accounts_groups_ok", table(), None),
        (
            "accounts_groups_string",
            "\n[accounts.groups]\n\"team-web\" = \"example.com/chain/web\"\n".to_string(),
            Some("accounts.groups.team-web"),
        ),
    ] {
        let head = format!("{postgres}{}{table}", section());
        let dir = console_dir(case, Some(VERSION), &head, &ui_section());
        let (_, stderr, code) = run_args(&dir, &["status", "--workspace", MANIFEST]);
        match problem {
            None => assert!(!stderr.contains("accounts"), "{case}: {stderr}"),
            Some(key) => {
                assert_eq!(code, Some(1), "{case}: {stderr}");
                assert!(stderr.contains(key), "{case}: {stderr}");
            }
        }
    }
}
