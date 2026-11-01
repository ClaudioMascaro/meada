//! A team operator's additions under the GitHub grant provider (#333, ADR 0016): the team fixture
//! of `team` (real Valkey and store, the `oidc` module's loopback issuer) with `[oidc.github]` and
//! `[postgres]`, so every write is audited, and the role check `meada github-app roles` run over
//! the store's version against `github_app`'s fake GitHub API (`oidc::github::Github`). What is
//! stored and marked is seen through the console's routes, never the module's tables.

use std::fs;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::github::{HOST, console, get, post};
use super::team::{BILLING, ENTRIES, INVOICES, JOINT, LEDGER, SHARED, Team, VAULT, entries, team};
use crate::oidc::github::{Github, LOGIN_CLAIM, POLL_MS};
use crate::server::Server;
use crate::snapshot::KEY_ID;
use crate::ui::Reply;

/// A repository of the GitHub grant provider's host no team's claims grant reads.
const FRESH: &str = "e.test/team/fresh";

/// The team fixture with `[oidc.github]` on `HOST` (role `write`) over a schema of its own, a
/// console reading it, and `ops/roles.toml`: the console's manifest with an `index_dir` of its
/// own and the App block, the role check's.
struct Case {
    team: Team,
    github: Github,
    console: Server,
}

fn case(name: &'static str) -> Option<Case> {
    let team = team(name, true)?;
    let github = Github::new(name)?;
    let dir = &team.fleet.dir;
    let file = dir.join("ops/console.toml");
    let source = fs::read_to_string(&file).unwrap() + &github.sections(HOST, "[\"write\"]", 3600);
    let job = source.replacen("../console-indexes", "../roles-indexes", 1) + &github.app(HOST);
    fs::write(dir.join("ops/roles.toml"), job).unwrap();
    github.migrate(dir, "ops/roles.toml", &github.url);
    fs::write(&file, source).unwrap();
    let console = console(dir);
    Some(Case {
        team,
        github,
        console,
    })
}

impl Case {
    /// A token of `subject` in `groups`, with `login` in `[oidc.github] login_claim`, the global
    /// operator with `operator`.
    fn token(&self, subject: &str, groups: &[&str], login: Option<&str>, operator: bool) -> String {
        let mut claims = self.team.claims(subject);
        claims["groups"] = json!(groups);
        if let Some(login) = login {
            claims[LOGIN_CLAIM] = json!(login);
        }
        if operator {
            claims["operator"] = json!(true);
        }
        self.team.sign(&claims)
    }

    fn save(&self, token: &str, text: &str) -> Reply {
        let body = json!({"entries": text, "message": "m"});
        post(&self.console, "workspace/versions", token, body)
    }

    /// A save that is accepted: the id it stored.
    fn saved(&self, token: &str, text: &str) -> String {
        let reply = self.save(token, text);
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.json()["result"].as_str().unwrap().to_string()
    }

    fn apply(&self, token: &str, version: &str) -> Reply {
        post(
            &self.console,
            "workspace/apply",
            token,
            json!({"version": version}),
        )
    }

    /// The entries of the version `id`, as `token` reads them, by identity.
    fn entry(&self, token: &str, id: &str, repo: &str) -> Value {
        let version = get(&self.console, &format!("workspace/versions/{id}"), token);
        let found = (version["repositories"].as_array().unwrap().iter())
            .find(|e| e["repo"] == repo)
            .cloned();
        found.unwrap_or_else(|| panic!("{repo} in {version}"))
    }

    /// The current version's id and the number of versions stored.
    fn state(&self, operator: &str) -> (Value, usize) {
        let versions = get(&self.console, "workspace/versions", operator);
        let count = versions["versions"].as_array().unwrap().len();
        (versions["current"]["id"].clone(), count)
    }

    /// `meada <args>` in the fleet's directory, as the job runs it.
    fn job(&self, args: &[&str]) -> (String, String) {
        let env = [
            ("MEADA_TEST_S3_KEY", KEY_ID),
            ("MEADA_REFRESH_FENCE", "1"),
            ("MEADA_PERMISSIONS_POLL_MS", POLL_MS),
        ];
        let dir = &self.team.fleet.dir;
        let (stdout, stderr, code) = crate::postgres::meada(dir, "en_US.UTF-8", &env, args);
        assert_eq!(code, 0, "{stderr}");
        (stdout, stderr)
    }
}

/// The text of the entries `team-billing` reads, as it would submit them unchanged.
fn seen() -> String {
    let seen: Vec<_> = (ENTRIES.iter())
        .filter(|(repo, _)| [BILLING, LEDGER, SHARED, JOINT].contains(repo))
        .copied()
        .collect();
    entries(&seen)
}

/// `text` with an entry for `repo` owned by `team-billing`, `extra` lines after its owner.
fn with(text: &str, repo: &str, extra: &str) -> String {
    format!(
        "{text}[[repository]]\nrepo = \"{repo}\"\npath = \"../v\"\nowner = \"team-billing\"\n{extra}"
    )
}

/// A 403 `entries_refused`: its refused entries and hidden count.
fn refusals(reply: &Reply) -> (Value, Value) {
    assert_eq!(reply.status, 403, "{}", reply.body);
    let error = &reply.json()["error"];
    assert_eq!(error["code"], "entries_refused", "{}", reply.body);
    (error["refused"].clone(), error["hidden"].clone())
}

/// A team operator's addition on the GitHub grant provider's host outside its grant is stored
/// with `added_by` set from its login over a forged value; without a login it is
/// `unattributed`; an edit of `added_by` is `added_by_change`; an identity the workspace already
/// holds outside its grant is `outside_grant`, named as submitted, with or without a login. A refusal writes nothing and
/// the audit records it; stderr carries no token and no login.
#[test]
fn a_team_operator_s_addition_on_the_host_is_attributed_from_its_login() {
    let Some(case) = case("ui_additions") else {
        return;
    };
    let ana = case.token("ana", &["team-billing"], Some("Ana-Souza"), false);
    let anon = case.token("ana", &["team-billing"], None, false);
    let root = case.token("root", &[], None, true);
    let base = seen();
    let before = case.state(&root);
    let forged = "added_by = \"mallory\"\n";
    let cases = [
        (
            &anon,
            with(&base, FRESH, forged),
            FRESH,
            "add",
            "unattributed",
        ),
        (
            &ana,
            base.replacen(
                "owner = \"team-billing\"\n",
                "owner = \"team-billing\"\nadded_by = \"ana-souza\"\n",
                1,
            ),
            BILLING,
            "edit",
            "added_by_change",
        ),
        (&ana, with(&base, VAULT, ""), VAULT, "add", "outside_grant"),
        (&anon, with(&base, VAULT, ""), VAULT, "add", "outside_grant"),
    ];
    for (token, text, identity, change, reason) in &cases {
        let (refused, hidden) = refusals(&case.save(token, text));
        let expected = json!([{"identity": identity, "change": change, "reason": reason}]);
        assert_eq!(
            (refused, hidden),
            (expected, json!(0)),
            "{identity} {reason}"
        );
        assert_eq!(case.state(&root), before, "{identity} {reason}");
    }

    let saved = case.saved(&ana, &with(&base, FRESH, forged));
    let fresh = case.entry(&root, &saved, FRESH);
    assert_eq!(
        fresh,
        json!({"repo": FRESH, "path": "../v", "owner": "team-billing", "added_by": "ana-souza"})
    );
    // The entries outside its grant are kept as they are.
    assert_eq!(case.entry(&root, &saved, VAULT)["owner"], "team-billing");

    let records = get(&case.console, "audit", &root)["records"].clone();
    let found: Vec<_> = (records.as_array().unwrap().iter())
        .map(|r| {
            (
                r["action"].clone(),
                r["outcome"].clone(),
                r["login"].clone(),
            )
        })
        .collect();
    let save = |outcome: &str, login: Value| (json!("workspace.save"), json!(outcome), login);
    let ana_login = json!("ana-souza");
    assert_eq!(
        found,
        [
            save("accepted", ana_login.clone()),
            save("refused", Value::Null),
            save("refused", ana_login.clone()),
            save("refused", ana_login),
            save("refused", Value::Null),
        ]
    );
    let message = records[2]["message"].as_str().unwrap();
    assert!(
        message.ends_with(&format!("({VAULT}; hidden 0)")),
        "{message}"
    );
    let stderr = case.console.stderr();
    case.github.clean(&stderr, &[ana, anon, root]);
    assert!(!stderr.contains("mallory"), "{stderr}");
}

/// The global operator's saves keep `added_by` as written, setting, replacing and clearing it,
/// its own additions carrying none; a team operator's apply of a stored version whose addition
/// on the host, inside its grant, lacks `added_by` is `unattributed` under the lock, and goes
/// through once the addition has one.
#[test]
fn the_global_operator_keeps_added_by_as_written_and_an_apply_needs_it() {
    let Some(case) = case("ui_additions_operator") else {
        return;
    };
    let root = case.token("root", &[], Some("Root-Op"), true);
    let ana = case.token("ana", &["team-billing"], Some("Ana-Souza"), false);
    let whole = entries(&ENTRIES);
    let attributed = |login: &str| {
        whole.replacen(
            "owner = \"team-billing\"\n",
            &format!("owner = \"team-billing\"\nadded_by = \"{login}\"\n"),
            1,
        )
    };
    for (text, expected) in [
        (attributed("zed"), json!("zed")),
        (attributed("yan"), json!("yan")),
        (whole.clone(), Value::Null),
    ] {
        let id = case.saved(&root, &text);
        assert_eq!(
            case.entry(&root, &id, BILLING)["added_by"],
            expected,
            "{text}"
        );
    }
    let id = case.saved(&root, &with(&whole, FRESH, "added_by = \"mallory\"\n"));
    assert_eq!(case.entry(&root, &id, FRESH)["added_by"], "mallory");
    let id = case.saved(&root, &with(&whole, FRESH, ""));
    assert!(case.entry(&root, &id, FRESH).get("added_by").is_none());

    let before = case.state(&root);
    let unattributed = case.saved(&root, &with(&whole, INVOICES, ""));
    let (refused, hidden) = refusals(&case.apply(&ana, &unattributed));
    assert_eq!(
        (refused, hidden),
        (
            json!([{"identity": INVOICES, "change": "add", "reason": "unattributed"}]),
            json!(0)
        )
    );
    assert_eq!(case.state(&root).0, before.0);
    let signed = case.saved(&root, &with(&whole, INVOICES, "added_by = \"ana-souza\"\n"));
    let reply = case.apply(&ana, &signed);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(case.state(&root).0, json!(signed));
}

/// After a role check that excludes an addition (its `added_by` holds no accepted role),
/// `workspace` and `versions/<id>` of the version it checked mark the entry `excluded: true` for
/// the global operator and for a viewer whose GitHub grant reads it; another version holding the
/// same entry carries no mark, nor any other entry, and a subject that does not read the entry
/// never sees it.
#[test]
fn an_excluded_addition_is_marked_in_the_views_of_the_version_checked() {
    let Some(case) = case("ui_additions_excluded") else {
        return;
    };
    let root = case.token("root", &[], None, true);
    let bia = case.token("bia", &[], Some("bia_acme"), false);
    let ana = case.token("ana", &["team-billing"], Some("Ana-Souza"), false);
    let whole = with(&entries(&ENTRIES), FRESH, "added_by = \"ana-souza\"\n");
    let checked = case.saved(&root, &whole);
    assert_eq!(case.apply(&root, &checked).status, 200);
    let other = case.saved(&root, &whole.replace("../e.test/team/ledger", "../moved"));

    case.github.list("team/fresh", &[("bia_acme", "write")]);
    case.job(&["workspace", "pull", "--workspace", "ops/roles.toml"]);
    let (line, stderr) = case.job(&["github-app", "roles", "--workspace", "ops/roles.toml"]);
    assert!(line.ends_with(", 1 exclusions\n"), "{line}");
    assert!(stderr.contains(FRESH), "{stderr}");
    case.github.clean(&stderr, &[]);

    let start = Instant::now();
    let workspace = loop {
        let workspace = get(&case.console, "workspace", &root);
        if workspace.to_string().contains("excluded") {
            break workspace;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{workspace}");
        std::thread::sleep(Duration::from_millis(50));
    };
    let marked = |entries: &Value| -> Vec<String> {
        (entries.as_array().unwrap().iter())
            .filter(|e| e.get("excluded").is_some())
            .inspect(|e| assert_eq!(e["excluded"], true, "{e}"))
            .map(|e| e["repo"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(marked(&workspace["version"]["repositories"]), [FRESH]);
    let version = get(
        &case.console,
        &format!("workspace/versions/{checked}"),
        &root,
    );
    assert_eq!(marked(&version["repositories"]), [FRESH]);
    let version = get(&case.console, &format!("workspace/versions/{other}"), &root);
    assert!(marked(&version["repositories"]).is_empty(), "{version}");
    assert_eq!(case.entry(&root, &other, FRESH)["added_by"], "ana-souza");

    // A viewer reading the entry through GitHub sees the mark; one that does not, no entry.
    let start = Instant::now();
    while get(&case.console, "session", &bia)["github_grant"] != "current" {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(50));
    }
    let seen = get(&case.console, "workspace", &bia)["version"]["repositories"].clone();
    assert_eq!(
        seen,
        json!([{"repo": FRESH, "path": "../v", "owner": "team-billing",
                "added_by": "ana-souza", "excluded": true}])
    );
    let body = get(
        &case.console,
        &format!("workspace/versions/{checked}"),
        &ana,
    )
    .to_string();
    assert!(
        !body.contains(FRESH) && !body.contains("excluded"),
        "{body}"
    );
    case.github.clean(&case.console.stderr(), &[root, bia, ana]);
}

/// Local accounts (#561): a console with `[accounts]`, `[accounts.groups]` granting `team-billing`
/// what `[oidc.groups]` grants it here, and `[accounts.github]` on `HOST`, over the team fixture's
/// store. A local viewer of `team-billing` is a team operator; before it verifies a login its
/// addition on the host is `unattributed`, after it the addition is stored with `added_by` set
/// from the verified login over a forged value, and the audit's `login` column carries it. The
/// role check reads `[accounts.github]`: once it excludes the addition, the global operator and a
/// local viewer whose verified login reads the entry see the mark. No stderr line carries a token,
/// a login or a personal access token.
#[cfg(feature = "accounts")]
#[test]
fn a_local_team_operator_s_addition_is_attributed_from_its_verified_login() {
    use super::PUBLIC;
    use crate::manifest::MANIFEST;

    const ADMIN: &str = "MEADA_TEST_ADMIN_PASSWORD_561";
    const CHOSEN: &str = "deployer chose this one";
    const PASSWORD: &str = "a long team passphrase";
    const ANA_PAT: &str = "ghp_ana0123456789abcdef561t";
    const BIA_PAT: &str = "ghp_bia0123456789abcdef561t";
    let name = "ui_additions_local";
    let Some(team) = super::team::team(name, true) else {
        return;
    };
    let Some(github) = Github::new(name) else {
        return;
    };
    let dir = &team.fleet.dir;
    let (private, public) = crate::ui::accounts::pair();
    fs::write(dir.join("ops/public.pem"), &public).unwrap();
    fs::write(dir.join("ops/private.pem"), &private).unwrap();
    let source = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let sections = source.split_once("[snapshot]").unwrap().1;
    let local = format!(
        "index_dir = \"../local-indexes\"\n[workspace]\nsource = \"store\"\n[snapshot]{sections}\n\
         \n[postgres]\nurl_file = \"pg-url\"\nschema = \"{schema}\"\n\
         \n[accounts]\npublic_key_file = \"public.pem\"\nprivate_key_file = \"private.pem\"\n\
         admin_password_env = \"{ADMIN}\"\n\
         \n[accounts.groups]\n\"team-billing\" = [\"{BILLING}\", \"{LEDGER}\", \"{SHARED}\", \
         \"{JOINT}\", \"{INVOICES}\"]\n\
         \n[accounts.github]\nhost = \"{HOST}\"\napi = \"{api}\"\nroles = [\"write\"]\n\
         max_age_seconds = 3600\n\
         \n[ui]\ndir = \"../ui\"\npublic_url = \"{PUBLIC}\"\noperations = true\n",
        schema = github.schema,
        api = github.api.base,
    );
    let job = local.replacen("../local-indexes", "../roles-indexes", 1) + &github.app(HOST);
    fs::write(dir.join("ops/roles.toml"), job).unwrap();
    github.migrate(dir, "ops/roles.toml", &github.url);
    fs::write(dir.join("ops/local.toml"), local).unwrap();
    github.api.with(|state| {
        state.users.insert(ANA_PAT.into(), "Ana-Souza".into());
        state.users.insert(BIA_PAT.into(), "bia_acme".into());
    });
    let console = Server::start_command(
        dir,
        "ui",
        &["--listen", "127.0.0.1:0", "--workspace", "ops/local.toml"],
        &[
            ("MEADA_TEST_S3_KEY", KEY_ID),
            ("MEADA_PERMISSIONS_POLL_MS", POLL_MS),
            (ADMIN, CHOSEN),
        ],
    );
    let signed_in = |username: &str, password: &str| {
        let body = json!({"username": username, "password": password});
        let reply = post(&console, "login", "", body);
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.json()["token"].as_str().unwrap().to_string()
    };
    let root = signed_in("admin", CHOSEN);
    for (username, groups) in [("ana", json!(["team-billing"])), ("bia", json!([]))] {
        let body = json!({"username": username, "role": "viewer", "groups": groups,
                          "password": PASSWORD, "change_required": false});
        let reply = post(&console, "accounts", &root, body);
        assert_eq!(reply.status, 200, "{}", reply.body);
    }
    let verify = |token: &str, pat: &str| {
        let reply = post(&console, "account/github", token, json!({"token": pat}));
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.json()["token"].as_str().unwrap().to_string()
    };
    let save = |token: &str, text: &str| {
        let body = json!({"entries": text, "message": "m"});
        post(&console, "workspace/versions", token, body)
    };
    let versions = |token: &str| get(&console, "workspace/versions", token);

    // Before a verified login: the team operator's addition on the host is unattributed.
    let anon = signed_in("ana", PASSWORD);
    assert_eq!(
        get(&console, "session", &anon)["teams"],
        json!(["team-billing"])
    );
    let base = seen();
    let forged = "added_by = \"mallory\"\n";
    let before = versions(&root);
    let (refused, hidden) = refusals(&save(&anon, &with(&base, FRESH, forged)));
    assert_eq!(
        (refused, hidden),
        (
            json!([{"identity": FRESH, "change": "add", "reason": "unattributed"}]),
            json!(0)
        )
    );
    assert_eq!(versions(&root), before);

    // After it: attributed from the verified login, the forged value ignored.
    let ana = verify(&anon, ANA_PAT);
    let session = get(&console, "session", &ana);
    assert_eq!(
        (&session["login"], &session["teams"]),
        (&json!("ana-souza"), &json!(["team-billing"]))
    );
    let reply = save(&ana, &with(&base, FRESH, forged));
    assert_eq!(reply.status, 200, "{}", reply.body);
    let saved = reply.json()["result"].as_str().unwrap().to_string();
    let version = get(&console, &format!("workspace/versions/{saved}"), &root);
    let fresh = (version["repositories"].as_array().unwrap().iter())
        .find(|e| e["repo"] == FRESH)
        .cloned();
    assert_eq!(
        fresh,
        Some(
            json!({"repo": FRESH, "path": "../v", "owner": "team-billing",
                    "added_by": "ana-souza"})
        )
    );
    let records = get(&console, "audit?limit=100", &root)["records"].clone();
    let saves: Vec<_> = (records.as_array().unwrap().iter())
        .filter(|r| r["action"] == "workspace.save")
        .map(|r| {
            (
                r["subject"].clone(),
                r["outcome"].clone(),
                r["login"].clone(),
            )
        })
        .collect();
    assert_eq!(
        saves,
        [
            (json!("local:ana"), json!("accepted"), json!("ana-souza")),
            (json!("local:ana"), json!("refused"), Value::Null),
        ]
    );

    // The role check reads `[accounts.github]` and excludes the addition: the marks.
    let applied = post(
        &console,
        "workspace/apply",
        &root,
        json!({"version": saved}),
    );
    assert_eq!(applied.status, 200, "{}", applied.body);
    github.list("team/fresh", &[("bia_acme", "write")]);
    let env = [
        ("MEADA_TEST_S3_KEY", KEY_ID),
        ("MEADA_REFRESH_FENCE", "1"),
        ("MEADA_PERMISSIONS_POLL_MS", POLL_MS),
    ];
    for args in [
        ["workspace", "pull", "--workspace", "ops/roles.toml"],
        ["github-app", "roles", "--workspace", "ops/roles.toml"],
    ] {
        let (stdout, stderr, code) = crate::postgres::meada(dir, "en_US.UTF-8", &env, &args);
        assert_eq!(code, 0, "{stderr}");
        if args[0] == "github-app" {
            assert!(stdout.ends_with(", 1 exclusions\n"), "{stdout}");
            github.clean(&stderr, &[]);
        }
    }
    let start = Instant::now();
    let workspace = loop {
        let workspace = get(&console, "workspace", &root);
        if workspace.to_string().contains("excluded") {
            break workspace;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{workspace}");
        std::thread::sleep(Duration::from_millis(50));
    };
    let marked: Vec<_> = (workspace["version"]["repositories"]
        .as_array()
        .unwrap()
        .iter())
    .filter(|e| e.get("excluded").is_some())
    .map(|e| e["repo"].clone())
    .collect();
    assert_eq!(marked, [json!(FRESH)]);
    let bia = verify(&signed_in("bia", PASSWORD), BIA_PAT);
    let start = Instant::now();
    while get(&console, "session", &bia)["github_grant"] != "current" {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(50));
    }
    let seen_by_bia = get(&console, "workspace", &bia)["version"]["repositories"].clone();
    assert_eq!(
        seen_by_bia,
        json!([{"repo": FRESH, "path": "../v", "owner": "team-billing",
                "added_by": "ana-souza", "excluded": true}])
    );

    let stderr = console.stderr();
    github.clean(&stderr, &[root, anon, ana, bia]);
    for pat in [ANA_PAT, BIA_PAT] {
        assert!(!stderr.contains(pat), "{stderr}");
        assert!(!records.to_string().contains(pat), "{records}");
    }
    assert!(!stderr.contains("mallory"), "{stderr}");
}
