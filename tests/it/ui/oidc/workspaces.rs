//! Named workspaces from creation to query (#342, ADR 0020 and 0021), as public processes: the
//! console (`meada ui`) creates and shows workspaces for callers of the shared Y/A/B table
//! (`tests/fixtures/named-workspaces.json`) over the loopback issuer, the fake GitHub API, the
//! local Valkey, Postgres and S3-compatible store; the job (`workspace pull`, `github-app roles`,
//! `index`, `snapshot publish`) makes the selected repositories queryable once; and a node serves
//! `/-/workspaces/<id>/<identity>` to the same callers. What a caller may do is seen through
//! statuses and answers, never the module's tables. Without a service the tests print why and
//! skip; with `MEADA_TEST_SERVICES_REQUIRED=1` they fail.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use jsonwebtoken::{Algorithm, EncodingKey};
use serde_json::{Value, json};

use super::{PUBLIC, VERSION, claims, console_dir, oidc, ui_oidc};
use crate::manifest::MANIFEST;
use crate::oidc::github::{Github, LOGIN_CLAIM, POLL_MS};
use crate::oidc::{Issuer, key, set, sign};
use crate::server::Server;
use crate::snapshot::{KEY_ID, SECRET, bucket, bucket_name, pull, service};
use crate::ui::{Reply, send, send_body};

const HOST: &str = "github.com";
const EN: &str = "en_US.UTF-8";
/// The audience of the node's tokens.
const NODE: &str = "https://meada.example";
const INSTALLATION: u64 = 15;
const NODE_MANIFEST: &str = "node/meada.workspace.toml";

fn table() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/named-workspaces.json")).unwrap()
}

/// The fixture each table repository's mirror is a copy of.
const FIXTURES: [(&str, &str); 3] = [
    ("Y", "chain-web"),
    ("A", "chain-gateway"),
    ("B", "chain-catalog"),
];

struct Case {
    github: Github,
    dir: PathBuf,
    endpoint: String,
    console: Server,
    signing: EncodingKey,
    issuer: Issuer,
    table: Value,
    name: String,
}

impl Case {
    fn new(name: &str) -> Option<Self> {
        let github = Github::new(name)?;
        let endpoint = service()?;
        let bucket_id = bucket_name(name);
        bucket(&endpoint, &bucket_id);
        let (signing, jwk) = key("ws");
        let issuer = Issuer::new(set(&[&jwk]));
        let table = table();
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = fs::remove_dir_all(&dir);
        // The fake GitHub: the table's repositories, their collaborators, and `acme/c` nobody
        // collaborates on.
        let mut covered = vec!["acme/c".to_string()];
        for (short, identity) in table["identities"].as_object().unwrap() {
            let repo = identity
                .as_str()
                .unwrap()
                .strip_prefix("github.com/")
                .unwrap();
            covered.push(repo.to_string());
            let rows: Vec<(&str, &str)> = table["callers"]
                .as_object()
                .unwrap()
                .values()
                .flat_map(|caller| {
                    let login = caller["login"].as_str();
                    let same = caller["rows"].as_array().unwrap().iter();
                    same.filter(move |row| row["identity"] == short.as_str())
                        .map(move |row| (login.unwrap(), row["role"].as_str().unwrap()))
                })
                .collect();
            github.list(repo, &rows);
        }
        github.list("acme/c", &[]);
        github.api.install(1, INSTALLATION, "acme");
        github.api.with(|s| s.covered.insert(INSTALLATION, covered));
        // The console's and the job's manifest: one file, as an operator's bootstrap is.
        let roles = "[\"write\", \"maintain\", \"admin\"]";
        let head = format!(
            "{}{}{}",
            oidc(&issuer.base),
            github.sections(HOST, roles, 3600),
            github.job(HOST),
        );
        let head = format!(
            "{head}{}local_dir = \"../console-generations\"\n\n[generations]\nkeep = 5\n\n\
             [catalog]\nallow = [\"{HOST}/acme/*\"]\nmax_repositories = 10\n\n[workspaces]\n\
             mirrors = \"{}\"\n\n[workspace]\nsource = \"store\"\n",
            crate::snapshot::section(&endpoint, &bucket_id),
            dir.join("mirrors").display(),
        );
        let ui = format!(
            "{}operations = true\noperator_groups = [\"admins\"]\n",
            ui_oidc()
        );
        let made = console_dir(name, Some(VERSION), &head, &ui);
        assert_eq!(made, dir);
        fs::write(dir.join("ops/s3-secret"), format!("{SECRET}\n")).unwrap();
        // The mirrors the table's repositories are cloned at, provisioned by the operator.
        for (short, fixture) in FIXTURES {
            let identity = table["identities"][short].as_str().unwrap();
            let origin = format!("https://{identity}.git");
            let copy = crate::common::fixture_repo(&format!("{name}-src"), fixture, Some(&origin));
            let to = dir.join("mirrors").join(identity);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            fs::rename(copy, to).unwrap();
        }
        github.migrate(&dir, MANIFEST, &github.url);
        let console = Server::start_command(
            &dir,
            "ui",
            &["--listen", "127.0.0.1:0", "--workspace", MANIFEST],
            &[
                ("MEADA_PERMISSIONS_POLL_MS", POLL_MS),
                ("MEADA_TEST_S3_KEY", KEY_ID),
            ],
        );
        Some(Self {
            github,
            dir,
            endpoint,
            console,
            signing,
            issuer,
            table,
            name: name.into(),
        })
    }

    /// `meada <args>` in the case's directory as the job runs it.
    fn job(&self, args: &[&str]) -> (String, String, i32) {
        let env = [
            ("MEADA_PERMISSIONS_POLL_MS", POLL_MS),
            ("MEADA_TEST_S3_KEY", KEY_ID),
        ];
        crate::postgres::meada(&self.dir, EN, &env, args)
    }

    fn job_ok(&self, args: &[&str]) -> String {
        let (stdout, stderr, code) = self.job(args);
        assert_eq!(code, 0, "{args:?}: {stderr}");
        stdout
    }

    fn roles(&self) -> String {
        self.job_ok(&["github-app", "roles", "--workspace", MANIFEST])
    }

    /// The pull that reads the contexts and then the role check, as a refresh runs them.
    fn refresh(&self) -> String {
        self.job_ok(&["workspace", "pull", "--workspace", MANIFEST]);
        self.roles()
    }

    fn identity(&self, short: &str) -> String {
        self.table["identities"][short]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// A console token of the table's caller `name`.
    fn token(&self, name: &str) -> String {
        let caller = &self.table["callers"][name];
        let mut claims = claims(&self.issuer.base, &[]);
        claims["sub"] = json!(name);
        if let Some(login) = caller["login"].as_str() {
            claims[LOGIN_CLAIM] = json!(login);
        }
        if let Some(granted) = caller["claims_grant"].as_array() {
            let granted: Vec<String> = (granted.iter())
                .map(|short| self.identity(short.as_str().unwrap()))
                .collect();
            claims["meada_repositories"] = json!(granted);
        }
        if caller["platform_administrator"] == true {
            claims["groups"] = json!(["admins"]);
        }
        sign(&self.signing, "ws", Algorithm::ES256, &claims)
    }

    fn get(&self, path: &str, token: &str) -> Reply {
        let bearer = format!("Bearer {token}");
        send(&self.console, "GET", path, &[("Authorization", &bearer)])
    }

    fn post(&self, path: &str, token: &str, body: &Value) -> Reply {
        let bearer = format!("Bearer {token}");
        let headers = [("Authorization", bearer.as_str()), ("Origin", PUBLIC)];
        send_body(&self.console, "POST", path, &headers, &body.to_string())
    }

    /// Waits until the console has read a permissions set (the catalog stops being `no_set`).
    fn ready(&self) {
        let token = self.token("root");
        let start = Instant::now();
        loop {
            let reply = self.get("/api/v1/catalog", &token);
            if reply.json()["reason"] != json!("no_set") {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "{}", reply.body);
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The manifest text selecting the table's `shorts`.
    fn entries(&self, shorts: &[&str]) -> String {
        let one = |s: &&str| format!("[[repository]]\nrepo = \"{}\"\n", self.identity(s));
        shorts.iter().map(one).collect::<Vec<_>>().join("\n")
    }

    /// `POST /api/v1/workspaces` by `caller` selecting `shorts`.
    fn create(&self, caller: &str, name: &str, shorts: &[&str]) -> Reply {
        let body = json!({
            "name": name, "description": "", "entries": self.entries(shorts), "message": "first",
        });
        self.post("/api/v1/workspaces", &self.token(caller), &body)
    }

    /// The ids of the workspaces `caller` lists, sorted.
    fn listed(&self, caller: &str) -> Vec<String> {
        let reply = self.get("/api/v1/workspaces", &self.token(caller));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let mut ids: Vec<String> = (reply.json()["workspaces"].as_array().unwrap().iter())
            .map(|w| w["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    }

    /// The node's manifest and its serving process, over this case's store and Valkey.
    fn node(&self) -> (Server, String) {
        let node = self.dir.join("node");
        fs::create_dir_all(&node).unwrap();
        fs::write(node.join("s3-secret"), format!("{SECRET}\n")).unwrap();
        fs::write(node.join("pg-url"), &self.github.url).unwrap();
        let toml = format!(
            "index_dir = \"indexes\"\n{}local_dir = \"generations\"\n\n[generations]\nkeep = 5\n\
             \n[oidc]\nissuer = \"{}\"\npublic_url = \"{NODE}/\"\naudience = \"{NODE}\"\n\
             identities_claim = \"meada_repositories\"\nscopes = [\"meada:read\"]\n{}{}",
            crate::snapshot::section(&self.endpoint, &bucket_name(&self.name)),
            self.issuer.base,
            self.github
                .sections(HOST, "[\"write\", \"maintain\", \"admin\"]", 3600),
            self.github
                .job(HOST)
                .split("[[github_app]]")
                .next()
                .unwrap(),
        );
        fs::write(self.dir.join(NODE_MANIFEST), toml).unwrap();
        let (stdout, stderr, code) = pull(&self.dir, NODE_MANIFEST, &[]);
        assert_eq!(code, Some(0), "{stdout} {stderr}");
        let args = [
            "--listen",
            "127.0.0.1:0",
            "--workspace",
            NODE_MANIFEST,
            "--generations",
        ];
        let env = [
            ("MEADA_PERMISSIONS_POLL_MS", POLL_MS),
            ("MEADA_TEST_S3_KEY", KEY_ID),
        ];
        (
            Server::start_args(&self.dir, &args, &env),
            stdout.trim().to_string(),
        )
    }

    /// A node token of the table's caller `name`: its login claim, no claims grant.
    fn node_token(&self, name: &str) -> String {
        let mut claims = json!({
            "iss": self.issuer.base, "aud": NODE,
            "exp": jsonwebtoken::get_current_timestamp() + 300,
            "meada_repositories": [], "scope": "openid meada:read",
        });
        if let Some(login) = self.table["callers"][name]["login"].as_str() {
            claims[LOGIN_CLAIM] = json!(login);
        }
        sign(&self.signing, "ws", Algorithm::ES256, &claims)
    }
}

/// What a node answers a `search` call at `path` for `token`: the status and the body.
fn node_call(node: &Server, path: &str, token: &str, query: &str) -> (u16, String) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let message = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "search", "arguments": {"query": query}}
    });
    let mut answer = agent
        .post(format!("{}{path}", node.url))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .send(message.to_string())
        .unwrap();
    let status = answer.status().as_u16();
    (status, answer.body_mut().read_to_string().unwrap())
}

/// Waits until the node's permissions set is the one the job wrote: `token` reads the workspace
/// route of `id` (200) or not.
fn until_status(node: &Server, path: &str, token: &str, status: u16) -> String {
    let start = Instant::now();
    loop {
        let (found, body) = node_call(node, path, token, "handle");
        if found == status {
            return body;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{found}: {body}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The creation rows of the table, as the console answers them: an A-only caller creates Y+A and
/// is refused Y+B (the refusal counts what its grant does not read and names nothing of B), a
/// B-only caller the reverse, a caller in both groups the union; a direct role is the same as a
/// group's; a claims grant or a token without a login creates nothing; the platform
/// administrator selects the whole catalog. A caller-supplied actor or role is an unknown key.
#[test]
fn creation_follows_the_shared_table_and_trusts_no_asserted_identity() {
    let Some(case) = Case::new("ws_create_table") else {
        return;
    };
    // Before any set: pending, for everyone, nothing stored.
    let early = case.create("alice", "early", &["Y"]);
    assert_eq!(
        (early.status, early.json()["error"]["reason"].clone()),
        (409, json!("no_set"))
    );
    case.refresh();
    case.ready();
    let b = case.identity("B");
    for row in case.table["creation"].as_array().unwrap() {
        let caller = row["caller"].as_str().unwrap();
        if case.table["callers"][caller]["platform_administrator"] == true {
            continue;
        }
        let shorts: Vec<&str> = (row["select"].as_array().unwrap().iter())
            .map(|s| s.as_str().unwrap())
            .collect();
        let name = format!("{caller} {}", shorts.join("+"));
        let reply = case.create(caller, &name, &shorts);
        let label = format!("{name}: {}", reply.body);
        if let Some(reason) = row["pending"].as_str() {
            assert_eq!(reply.status, 409, "{label}");
            assert_eq!(reply.json()["error"]["reason"], reason, "{label}");
            continue;
        }
        match row["allowed"].as_bool().unwrap() {
            true => assert!(matches!(reply.status, 200 | 201), "{label}"),
            false => {
                assert_eq!(reply.status, 403, "{label}");
                let error = &reply.json()["error"];
                assert_eq!(error["code"], "entries_refused", "{label}");
                let named = row["named"].as_array().map_or(0, Vec::len);
                assert_eq!(error["refused"].as_array().unwrap().len(), named, "{label}");
                assert_eq!(error["hidden"].as_u64(), row["hidden"].as_u64(), "{label}");
            }
        }
    }
    // A refusal that hides B says nothing of B.
    let refused = case.create("alice", "ya yb", &["Y", "B"]);
    assert_eq!(refused.status, 403);
    assert!(!refused.body.contains(&b), "{}", refused.body);
    // A body past the bound is refused unread.
    let huge =
        json!({"name": "big", "entries": case.entries(&["Y"]), "message": "m".repeat(90 << 10)});
    assert_eq!(
        case.post("/api/v1/workspaces", &case.token("alice"), &huge)
            .status,
        413
    );
    // A claims grant creates nothing; neither does a token without a login.
    for caller in ["claims", "nologin"] {
        let reply = case.create(caller, "none", &["Y"]);
        assert!(
            matches!(reply.status, 403 | 409),
            "{caller}: {}",
            reply.body
        );
    }
    assert_eq!(
        case.create("nologin", "none", &["Y"]).json()["error"]["reason"],
        "missing_login"
    );
    // The administrator selects what the catalog holds, and `acme/c` nobody collaborates on.
    let all = case.post(
        "/api/v1/workspaces",
        &case.token("root"),
        &json!({"name": "everything", "entries": case.entries(&["Y", "A", "B"]), "message": "m"}),
    );
    assert_eq!(all.status, 201, "{}", all.body);
    // Identity, role and grant come from the token: asserted ones are unknown keys.
    for forged in [
        json!({"actor": "root"}),
        json!({"role": "operator"}),
        json!({"operator": true, "login": "carol"}),
    ] {
        let mut body =
            json!({"name": "forged", "entries": case.entries(&["Y", "B"]), "message": "m"});
        body.as_object_mut()
            .unwrap()
            .extend(forged.as_object().unwrap().clone());
        let reply = case.post("/api/v1/workspaces", &case.token("alice"), &body);
        assert_eq!(reply.status, 400, "{}", reply.body);
    }
    // A manifest naming what a named entry does not hold is an invalid entry, not a refusal.
    let invalid = case.post(
        "/api/v1/workspaces",
        &case.token("alice"),
        &json!({"name": "p", "entries": format!("[[repository]]\nrepo = \"{}\"\npath = \"x\"\n", case.identity("Y")), "message": "m"}),
    );
    assert_eq!(
        (invalid.status, invalid.json()["error"]["code"].clone()),
        (422, json!("invalid_entries"))
    );
    // The same creator, name and entries are the same workspace.
    let again = case.create("alice", "alice Y+A", &["Y", "A"]);
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.json()["created"], false);
    case.github.clean(&case.console.stderr(), &[]);
}

/// Group peers inherit viewer access through their own grants, and only the creator and the
/// administrator operate: a workspace of Y+A is listed and shown to everyone whose grant reads
/// both, never to a caller who reads only Y or only Y and B, which get the one 404 of an id that
/// does not exist; the answers carry no operator list or editing control for a peer.
#[test]
fn peers_view_a_workspace_through_their_own_grants_and_operate_nothing() {
    let Some(case) = Case::new("ws_peers") else {
        return;
    };
    case.refresh();
    case.ready();
    let id_of = |reply: &Reply| reply.json()["id"].as_str().unwrap().to_string();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let yab = id_of(&case.create("carol", "all", &["Y", "A", "B"]));
    // Everyone sees what their grants read whole: dave (direct roles) and erin (maintain) as alice.
    assert_eq!(case.listed("alice"), vec![ya.clone()]);
    assert_eq!(case.listed("dave"), vec![ya.clone()]);
    assert_eq!(case.listed("erin"), vec![ya.clone()]);
    assert_eq!(case.listed("bob"), vec![yb.clone()]);
    let mut every = vec![ya.clone(), yb.clone(), yab.clone()];
    every.sort();
    assert_eq!(case.listed("carol"), every);
    assert_eq!(case.listed("root"), every);
    // Reading only Y (or reading by claims alone, which lists but never creates) lists nothing
    // that holds more.
    assert!(case.listed("yoda").is_empty());
    // The creator operates; a peer does not.
    let shown = case
        .get(&format!("/api/v1/workspaces/{ya}"), &case.token("alice"))
        .json();
    assert_eq!(
        (shown["operate"].clone(), shown["state"].clone()),
        (json!(true), json!("applied"))
    );
    assert_eq!(shown["entries"].as_array().unwrap().len(), 2);
    assert!(
        shown["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["ingestion"] == "pending")
    );
    let peer = case
        .get(&format!("/api/v1/workspaces/{ya}"), &case.token("dave"))
        .json();
    assert_eq!(peer["operate"], false);
    assert!(
        peer.get("operators").is_none() && peer.get("hidden").is_none(),
        "{peer}"
    );
    // A caller who cannot read the whole workspace gets the answer of an unknown id.
    let unknown = case.get("/api/v1/workspaces/0123456789abcdef", &case.token("bob"));
    for hidden in [ya.as_str(), yab.as_str()] {
        let denied = case.get(&format!("/api/v1/workspaces/{hidden}"), &case.token("bob"));
        assert_eq!((denied.status, &denied.body), (404, &unknown.body));
    }
    let denied = case.get(&format!("/api/v1/workspaces/{yb}"), &case.token("yoda"));
    assert_eq!((denied.status, &denied.body), (404, &unknown.body));
    // A cursor and a limit page the list; anything else is a fixed 400.
    let page = case
        .get("/api/v1/workspaces?limit=1", &case.token("root"))
        .json();
    assert_eq!(page["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(page["next"], json!(page["workspaces"][0]["id"]));
    for bad in [
        "limit=0",
        "limit=101",
        "after=zz",
        "extra=1",
        "limit=1&limit=2",
    ] {
        let reply = case.get(&format!("/api/v1/workspaces?{bad}"), &case.token("root"));
        assert_eq!(reply.status, 400, "{bad}");
    }
    // The creator's workspace gives a peer no write.
    let write = case.post(
        &format!("/api/v1/workspaces/{ya}/versions"),
        &case.token("dave"),
        &json!({}),
    );
    assert_eq!(write.status, 404, "{}", write.body);
    case.github.clean(&case.console.stderr(), &[]);
}

/// The whole path: the creators select, the job pulls the contexts, checks sponsors, indexes each
/// repository once and publishes one generation recording members and contexts, and a node
/// serves each context to the callers whose grants read all of it. Y is indexed once for the
/// three workspaces; a caller never reads another group's repository through Y or a link; the
/// plain route serves the deployment workspace alone (empty here); pending ingestion shows as
/// pending until the generation records it.
#[test]
fn a_created_workspace_becomes_queryable_for_group_peers_only() {
    let Some(case) = Case::new("ws_query") else {
        return;
    };
    case.refresh();
    case.ready();
    let id_of = |reply: &Reply| reply.json()["id"].as_str().unwrap().to_string();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let yab = id_of(&case.create("carol", "all", &["Y", "A", "B"]));
    let (y, a, b) = (case.identity("Y"), case.identity("A"), case.identity("B"));

    // The job: pull, role check, index, publish. The mirrors are the operator's.
    let pulled = case.job_ok(&["workspace", "pull", "--workspace", MANIFEST]);
    assert_eq!(pulled, "", "no deployment version, no id on stdout");
    let line = case.roles();
    assert!(line.ends_with(", 0 exclusions\n"), "{line}");
    let shown = case.job_ok(&["workspace", "show", "--pulled", "--workspace", MANIFEST]);
    for identity in [&y, &a, &b] {
        let path = case.dir.join("mirrors").join(identity);
        assert_eq!(
            shown.matches(&format!("repo = \"{identity}\"\n")).count(),
            1,
            "{shown}"
        );
        assert!(
            shown.contains(&format!("path = {:?}", path.display().to_string())),
            "{shown}"
        );
    }
    case.job_ok(&["index", "--workspace", MANIFEST]);
    let generation = case.job_ok(&["snapshot", "publish", "--workspace", MANIFEST]);
    let generation = generation.trim();

    // The generation records Y once, as a member, and three contexts.
    let (node, pulled) = case.node();
    assert_eq!(pulled, generation);
    let document: Value = serde_json::from_slice(
        &fs::read(
            case.dir
                .join("node/generations")
                .join(generation)
                .join("generation.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(document["repositories"].as_array().map_or(0, Vec::len), 0);
    let members: Vec<&str> = (document["members"].as_array().unwrap().iter())
        .map(|m| m["identity"].as_str().unwrap())
        .collect();
    assert_eq!(members, [a.as_str(), b.as_str(), y.as_str()]);
    let contexts = document["contexts"].as_array().unwrap();
    assert_eq!(contexts.len(), 3);
    assert!(
        contexts
            .iter()
            .all(|c| c.get("name").is_none() && c.get("operators").is_none())
    );

    // Queries: group peers through their own grants, never another group's repository.
    let route = |id: &str, identity: &str| format!("/-/workspaces/{id}/{identity}");
    let matrix: [(&str, &str, &str, u16); 12] = [
        ("alice", &ya, &y, 200),
        ("alice", &ya, &a, 200),
        ("alice", &yb, &y, 404),
        ("alice", &yab, &y, 404),
        ("bob", &yb, &y, 200),
        ("bob", &yb, &b, 200),
        ("bob", &ya, &a, 404),
        ("bob", &yab, &y, 404),
        ("carol", &ya, &a, 200),
        ("carol", &yb, &b, 200),
        ("carol", &yab, &b, 200),
        ("yoda", &ya, &y, 404),
    ];
    for (caller, id, identity, status) in matrix {
        let token = case.node_token(caller);
        let body = until_status(&node, &route(id, identity), &token, status);
        if status == 200 {
            let json: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(
                json["result"]["_meta"]["meada/workspace"],
                json!(id),
                "{caller}: {body}"
            );
            assert_eq!(
                json["result"]["_meta"]["meada/generation"],
                json!(generation),
                "{body}"
            );
        }
    }
    // A peer of the creator has the same answers (dave holds Y and A directly).
    let dave = case.node_token("dave");
    until_status(&node, &route(&ya, &a), &dave, 200);
    until_status(&node, &route(&yb, &b), &dave, 404);
    // A 404 lists only the workspaces the token may view; an unknown id is the same answer.
    let alice = case.node_token("alice");
    let (status, listing) = node_call(&node, &route(&yb, &y), &alice, "handle");
    assert_eq!((status, listing.trim()), (404, ya.as_str()));
    let (status, unknown) = node_call(&node, &route("0123456789abcdef", &y), &alice, "handle");
    assert_eq!((status, unknown), (404, listing));
    // The plain route serves the deployment workspace, which has none of these.
    let (status, _) = node_call(&node, &format!("/{y}"), &alice, "handle");
    assert_eq!(status, 404);
    // Nothing of B in an answer through Y and A.
    let (_, through) = node_call(&node, &route(&ya, &y), &alice, "handle");
    assert!(!through.contains(&b), "{through}");
    // A pin reproduces the context as that generation recorded it, for the callers that read it
    // today; a pin of a generation the node does not hold is a 404.
    let pinned = format!("{}@{generation}", route(&ya, &y));
    let (status, body) = node_call(&node, &pinned, &alice, "handle");
    assert_eq!(status, 200, "{body}");
    let json: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        json["result"]["_meta"]["meada/generation"],
        json!(generation),
        "{body}"
    );
    assert_eq!(
        json["result"]["_meta"]["meada/workspace"],
        json!(ya),
        "{body}"
    );
    let bob = case.node_token("bob");
    assert_eq!(node_call(&node, &pinned, &bob, "handle").0, 404);
    // A held generation with no such workspace for the caller names itself and the current one,
    // and no workspace id.
    let (status, body) = node_call(
        &node,
        &format!("{}@{generation}", route(&yb, &y)),
        &alice,
        "handle",
    );
    assert_eq!(status, 404);
    assert!(
        body.contains("records no such workspace") && body.contains(generation),
        "{body}"
    );
    assert!(!body.contains(&ya) && !body.contains(&yb), "{body}");
    let unheld = format!("{}@0123456789abcdef", route(&ya, &y));
    assert_eq!(node_call(&node, &unheld, &alice, "handle").0, 404);
    assert_eq!(node_call(&node, &unheld, &bob, "handle").0, 404);
    // Reserved first segment: a hostile id that is no workspace is a plain 404.
    let (status, _) = node_call(
        &node,
        &format!("/-/workspaces/not-an-id/{y}"),
        &alice,
        "handle",
    );
    assert_eq!(status, 404);

    // Pending ingestion turns ready only when a generation records the context: the console
    // pulls the generation it serves its own answers from.
    let pending = case
        .get(&format!("/api/v1/workspaces/{ya}"), &case.token("alice"))
        .json();
    assert_eq!(pending["state"], "applied");
    let (stdout, stderr, code) = pull(&case.dir, MANIFEST, &[]);
    assert_eq!(code, Some(0), "{stdout} {stderr}");
    let live = case
        .get(&format!("/api/v1/workspaces/{ya}"), &case.token("alice"))
        .json();
    assert_eq!(live["state"], "live", "{live}");
    assert!(
        live["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["ingestion"] == "ready")
    );
    // The console answers through a context under the same rule.
    let export = case.get(
        &format!(
            "/api/v1/workspaces/{ya}/repositories/{}/export",
            y.replace('/', "%2F")
        ),
        &case.token("alice"),
    );
    assert_eq!(export.status, 200, "{}", export.body);
    assert!(!export.body.contains(&b), "{}", export.body);
    let hidden = case.get(
        &format!(
            "/api/v1/workspaces/{yb}/repositories/{}/export",
            y.replace('/', "%2F")
        ),
        &case.token("alice"),
    );
    assert_eq!(hidden.status, 404);
    let query = case.post(
        "/api/v1/query",
        &case.token("alice"),
        &json!({"repository": y, "workspace": ya, "tool": "search", "arguments": {"query": "handle"}}),
    );
    assert_eq!(query.status, 200, "{}", query.body);
    let query = case.post(
        "/api/v1/query",
        &case.token("alice"),
        &json!({"repository": y, "workspace": yb, "tool": "search", "arguments": {"query": "handle"}}),
    );
    assert_eq!(query.status, 404);
    // A fetch or index failure of any member aborts the whole publication (#316): the pointer
    // stays, and every workspace keeps answering from the last complete generation.
    let mirror = case.dir.join("mirrors").join(&b);
    fs::rename(&mirror, case.dir.join("b-away")).unwrap();
    let (_, stderr, code) = case.job(&["index", "--workspace", MANIFEST]);
    assert_ne!(code, 0, "{stderr}");
    let (_, _, code) = case.job(&["snapshot", "publish", "--workspace", MANIFEST]);
    assert_ne!(code, 0);
    let (stdout, stderr, code) = pull(&case.dir, NODE_MANIFEST, &[]);
    assert_eq!((code, stdout.trim()), (Some(0), generation), "{stderr}");
    assert_eq!(node_call(&node, &route(&ya, &y), &alice, "handle").0, 200);
    case.github.clean(&node.stderr(), &[]);
    case.github.clean(&case.console.stderr(), &[]);
}

/// An entry no operator holds an accepted role on is a successful decision before the fetch, not
/// a failed refresh: the administrator selects `acme/c` (in the catalog, nobody collaborates on
/// it) and the role check excludes it from that workspace, so nothing is fetched or indexed for
/// it, no mirror is needed, and the publication proceeds. Eligibility is read again from the
/// newest set: losing the role excludes the entry at the next check.
#[test]
fn an_entry_without_a_sponsor_is_excluded_before_the_fetch_and_not_a_failure() {
    let Some(case) = Case::new("ws_sponsor") else {
        return;
    };
    case.refresh();
    case.ready();
    let c = format!("{HOST}/acme/c");
    let body = json!({
        "name": "orphan", "entries": format!("[[repository]]\nrepo = \"{c}\"\n"), "message": "m",
    });
    let created = case.post("/api/v1/workspaces", &case.token("root"), &body);
    assert_eq!(created.status, 201, "{}", created.body);
    let id = created.json()["id"].as_str().unwrap().to_string();
    // The sponsor-ful workspace of alice shares nothing with it.
    let ya = case.create("alice", "ya", &["Y", "A"]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    case.job_ok(&["workspace", "pull", "--workspace", MANIFEST]);
    let line = case.roles();
    assert!(line.ends_with(", 1 exclusions\n"), "{line}");
    let shown = case.job_ok(&["workspace", "show", "--pulled", "--workspace", MANIFEST]);
    assert!(!shown.contains(&c), "{shown}");
    case.job_ok(&["index", "--workspace", MANIFEST]);
    let generation = case.job_ok(&["snapshot", "publish", "--workspace", MANIFEST]);
    let (stdout, stderr, code) = pull(&case.dir, MANIFEST, &[]);
    assert_eq!(code, Some(0), "{stdout} {stderr}");
    let document: Value = serde_json::from_slice(
        &fs::read(
            case.dir
                .join("console-generations")
                .join(generation.trim())
                .join("generation.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let recorded: Vec<&str> = (document["contexts"].as_array().unwrap().iter())
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        recorded,
        [ya.as_str()],
        "the orphan context records nothing"
    );
    // The administrator sees the entry as excluded, in text, and not as ready.
    let shown = case
        .get(&format!("/api/v1/workspaces/{id}"), &case.token("root"))
        .json();
    assert_eq!(shown["entries"][0]["ingestion"], "excluded", "{shown}");
    assert_eq!(shown["state"], "applied");
    case.github.clean(&case.console.stderr(), &[]);
}
