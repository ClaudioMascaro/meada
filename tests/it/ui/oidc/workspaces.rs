use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use jsonwebtoken::{Algorithm, EncodingKey};
use serde_json::{Value, json};

use super::{PUBLIC, VERSION, claims, console_dir, oidc, ui_oidc};

mod acceptance;
mod configuration;
mod lifecycle;
#[cfg(feature = "proposals")]
mod proposals;
use crate::manifest::MANIFEST;
use crate::oidc::github::{Github, LOGIN_CLAIM, POLL_MS};
use crate::oidc::{Issuer, key, set, sign};
use crate::server::Server;
use crate::snapshot::{KEY_ID, SECRET, bucket, bucket_name, pull, service};
use crate::ui::{Reply, send, send_body};

const HOST: &str = "github.com";
const EN: &str = "en_US.UTF-8";
const NODE: &str = "https://meada.example";
const INSTALLATION: u64 = 15;
const NODE_MANIFEST: &str = "node/meada.workspace.toml";
const SCIM_TOKEN: &str = "scim-334-token-0123456789abcdefghijklmno";
const ADMIN_ENV: &str = "MEADA_TEST_ADMIN_PASSWORD_562";
const ADMIN_PASSWORD: &str = "the deployer chose this one";
const CALLER_PASSWORD: &str = "a named workspaces passphrase";
const CONSOLE_SURFACE: &str = "console";
const NODES_SURFACE: &str = "nodes";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Oidc,
    Local,
}

fn pat(name: &str) -> String {
    format!("ghp_{name}_562_0123456789abcdef")
}

fn table() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/named-workspaces.json")).unwrap()
}

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
    mode: Mode,
    tokens: Mutex<HashMap<(String, &'static str), String>>,
}

impl Case {
    fn new(name: &str) -> Option<Self> {
        Self::start(name, false)
    }

    #[cfg_attr(not(feature = "accounts"), expect(dead_code))]
    fn local(name: &str) -> Option<Self> {
        Self::local_with(name, "")
    }

    #[cfg_attr(not(feature = "accounts"), expect(dead_code))]
    fn local_with(name: &str, tail: &str) -> Option<Self> {
        let case = Self::with(name, Mode::Local, false, 3600, tail)?;
        let callers: Vec<String> = (case.table["callers"].as_object().unwrap().keys())
            .cloned()
            .collect();
        for caller in callers {
            case.token(&caller);
        }
        Some(case)
    }

    fn start(name: &str, directory: bool) -> Option<Self> {
        Self::aged(name, directory, 3600)
    }

    fn aged(name: &str, directory: bool, max_age: u64) -> Option<Self> {
        Self::with(name, Mode::Oidc, directory, max_age, "")
    }

    fn with(name: &str, mode: Mode, directory: bool, max_age: u64, tail: &str) -> Option<Self> {
        let github = Github::new(name)?;
        let endpoint = service()?;
        let bucket_id = bucket_name(name);
        bucket(&endpoint, &bucket_id);
        let (signing, jwk) = key("ws");
        let issuer = Issuer::new(set(&[&jwk]));
        let table = table();
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = fs::remove_dir_all(&dir);
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
        for (caller, row) in table["callers"].as_object().unwrap() {
            if let Some(login) = row["login"].as_str() {
                github
                    .api
                    .with(|s| s.users.insert(pat(caller), login.to_string()));
            }
        }
        let roles = "[\"write\", \"maintain\", \"admin\"]";
        let admission = match mode {
            Mode::Oidc => format!(
                "{}{}",
                oidc(&issuer.base),
                github.sections(HOST, roles, max_age)
            ),
            Mode::Local => {
                let keys = format!(
                    "private_key_file = \"private.pem\"\nadmin_password_env = \"{ADMIN_ENV}\"\n"
                );
                accounts_sections(&github, &table, &keys, max_age)
            }
        };
        let head = format!("{admission}{}", github.job(HOST));
        let head = format!(
            "{head}{}local_dir = \"../console-generations\"\n\n[generations]\nkeep = 5\n\n\
             [catalog]\nallow = [\"{HOST}/acme/*\"]\nmax_repositories = 10\n\n[workspaces]\n\
             mirrors = \"{}\"\n\n[workspace]\nsource = \"store\"\n",
            crate::snapshot::section(&endpoint, &bucket_id),
            dir.join("mirrors").display(),
        );
        let head = format!("{head}{tail}");
        let head = match directory {
            true => format!(
                "{head}\n[directory]\nuser_claim = \"sub\"\nuser_attribute = \"userName\"\n"
            ),
            false => head,
        };
        let ui = match mode {
            Mode::Oidc => format!(
                "{}operations = true\noperator_groups = [\"admins\"]\n",
                ui_oidc()
            ),
            Mode::Local => format!("{}operations = true\n", crate::ui::ui_section()),
        };
        let made = console_dir(name, Some(VERSION), &head, &ui);
        assert_eq!(made, dir);
        fs::write(dir.join("ops/s3-secret"), format!("{SECRET}\n")).unwrap();
        #[cfg(feature = "accounts")]
        if mode == Mode::Local {
            let (private, public) = crate::ui::accounts::pair();
            fs::write(dir.join("ops/private.pem"), private).unwrap();
            fs::write(dir.join("ops/public.pem"), &public).unwrap();
        }
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
                ("MEADA_SCIM_TOKEN", SCIM_TOKEN),
                (ADMIN_ENV, ADMIN_PASSWORD),
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
            mode,
            tokens: Mutex::new(HashMap::new()),
        })
    }

    fn subject(&self, name: &str) -> String {
        match self.mode {
            Mode::Oidc => name.to_string(),
            Mode::Local => format!("local:{name}"),
        }
    }

    fn local_token(&self, name: &str, surface: &'static str) -> String {
        let key = (name.to_string(), surface);
        if let Some(token) = self.tokens.lock().unwrap().get(&key) {
            return token.clone();
        }
        let token = match surface {
            CONSOLE_SURFACE => self.local_session(name),
            surface => {
                let session = self.local_token(name, CONSOLE_SURFACE);
                let asked = json!({"surface": surface});
                let reply = self.post("/api/v1/account/tokens", &session, &asked);
                assert_eq!(reply.status, 200, "{name} {surface}: {}", reply.body);
                assert_eq!(reply.json()["surface"], surface);
                reply.json()["token"].as_str().unwrap().to_string()
            }
        };
        self.tokens.lock().unwrap().insert(key, token.clone());
        token
    }

    fn local_session(&self, name: &str) -> String {
        let sign_in = |username: &str, password: &str| {
            let body = json!({"username": username, "password": password}).to_string();
            let reply = send_body(
                &self.console,
                "POST",
                "/api/v1/login",
                &[("Origin", PUBLIC)],
                &body,
            );
            assert_eq!(reply.status, 200, "{username}: {}", reply.body);
            reply.json()["token"].as_str().unwrap().to_string()
        };
        if name == "admin" {
            return sign_in(name, ADMIN_PASSWORD);
        }
        let caller = &self.table["callers"][name];
        let role = match caller["platform_administrator"] == true {
            true => "operator",
            false => "viewer",
        };
        let groups: Vec<&str> = match caller["claims_grant"].as_array() {
            Some(granted) if !granted.is_empty() => vec!["claims"],
            _ => Vec::new(),
        };
        let admin = self.local_token("admin", CONSOLE_SURFACE);
        let account = json!({"username": name, "role": role, "groups": groups,
                             "password": CALLER_PASSWORD, "change_required": false});
        let created = self.post("/api/v1/accounts", &admin, &account);
        assert_eq!(created.status, 200, "{name}: {}", created.body);
        let session = sign_in(name, CALLER_PASSWORD);
        let Some(login) = caller["login"].as_str() else {
            return session;
        };
        let verified = self.post(
            "/api/v1/account/github",
            &session,
            &json!({"token": pat(name)}),
        );
        assert_eq!(verified.status, 200, "{name}: {}", verified.body);
        assert_eq!(verified.json()["login"], login);
        verified.json()["token"].as_str().unwrap().to_string()
    }

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

    fn token(&self, name: &str) -> String {
        match self.mode {
            Mode::Oidc => self.token_for(name, PUBLIC),
            Mode::Local => self.local_token(name, CONSOLE_SURFACE),
        }
    }

    fn token_for(&self, name: &str, audience: &str) -> String {
        let caller = &self.table["callers"][name];
        let mut claims = claims(&self.issuer.base, &[]);
        claims["aud"] = json!(audience);
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

    fn entries(&self, shorts: &[&str]) -> String {
        let one = |s: &&str| format!("[[repository]]\nrepo = \"{}\"\n", self.identity(s));
        shorts.iter().map(one).collect::<Vec<_>>().join("\n")
    }

    fn create(&self, caller: &str, name: &str, shorts: &[&str]) -> Reply {
        let body = json!({
            "name": name, "description": "", "entries": self.entries(shorts), "message": "first",
        });
        self.post("/api/v1/workspaces", &self.token(caller), &body)
    }

    fn listed(&self, caller: &str) -> Vec<String> {
        let reply = self.get("/api/v1/workspaces", &self.token(caller));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let mut ids: Vec<String> = (reply.json()["workspaces"].as_array().unwrap().iter())
            .map(|w| w["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    }

    fn node(&self) -> (Server, String) {
        let node = self.dir.join("node");
        fs::create_dir_all(&node).unwrap();
        fs::write(node.join("s3-secret"), format!("{SECRET}\n")).unwrap();
        fs::write(node.join("pg-url"), &self.github.url).unwrap();
        let roles = "[\"write\", \"maintain\", \"admin\"]";
        let admission = match self.mode {
            Mode::Oidc => format!(
                "\n[oidc]\nissuer = \"{}\"\npublic_url = \"{NODE}/\"\naudience = \"{NODE}\"\n\
                 identities_claim = \"meada_repositories\"\nscopes = [\"meada:read\"]\n{}",
                self.issuer.base,
                self.github.sections(HOST, roles, 3600),
            ),
            Mode::Local => {
                fs::copy(self.dir.join("ops/public.pem"), node.join("public.pem")).unwrap();
                let private = "private_key_file = \"no-such-private.pem\"\n";
                format!(
                    "\n[ui]\n{}{}",
                    crate::ui::ui_section(),
                    accounts_sections(&self.github, &self.table, private, 3600)
                )
            }
        };
        let toml = format!(
            "index_dir = \"indexes\"\n{}local_dir = \"generations\"\n\n[generations]\nkeep = 5\n\
             {admission}{}",
            crate::snapshot::section(&self.endpoint, &bucket_name(&self.name)),
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

    fn node_token(&self, name: &str) -> String {
        if self.mode == Mode::Local {
            return self.local_token(name, NODES_SURFACE);
        }
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

fn accounts_sections(github: &Github, table: &Value, keys: &str, max_age: u64) -> String {
    let granted: Vec<String> = (table["callers"]["claims"]["claims_grant"]
        .as_array()
        .unwrap())
    .iter()
    .map(|short| {
        format!(
            "{:?}",
            table["identities"][short.as_str().unwrap()]
                .as_str()
                .unwrap()
        )
    })
    .collect();
    format!(
        "\n[accounts]\npublic_key_file = \"public.pem\"\n{keys}\n[accounts.groups]\n\
         claims = [{}]\n\n[accounts.github]\nhost = \"{HOST}\"\napi = \"{}\"\n\
         roles = [\"write\", \"maintain\", \"admin\"]\nmax_age_seconds = {max_age}\n\n\
         [postgres]\nurl_file = \"pg-url\"\nschema = \"{}\"\n",
        granted.join(", "),
        github.api.base,
        github.schema,
    )
}

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

#[test]
fn creation_follows_the_shared_table_and_trusts_no_asserted_identity() {
    if let Some(case) = Case::new("ws_create_table") {
        creation_follows_the_shared_table_and_trusts_no_asserted_identity_as(case);
    }
}

#[cfg(feature = "accounts")]
#[test]
fn local_accounts_creation_follows_the_shared_table_and_trusts_no_asserted_identity() {
    if let Some(case) = Case::local("ws_create_table_local") {
        creation_follows_the_shared_table_and_trusts_no_asserted_identity_as(case);
    }
}

fn creation_follows_the_shared_table_and_trusts_no_asserted_identity_as(case: Case) {
    let early = case.create("alice", "early", &["Y"]);
    assert_eq!(
        (early.status, early.json()["error"]["reason"].clone()),
        (409, json!("no_set"))
    );
    case.refresh();
    case.ready();
    let (y, a, b) = (case.identity("Y"), case.identity("A"), case.identity("B"));
    let identities = |reply: &Reply| -> Vec<String> {
        assert_eq!(reply.status, 200, "{}", reply.body);
        (reply.json()["repositories"].as_array().unwrap().iter())
            .map(|r| r["identity"].as_str().unwrap().to_string())
            .collect()
    };
    let alice = identities(&case.get("/api/v1/catalog", &case.token("alice")));
    assert_eq!(alice, [a.clone(), y.clone()]);
    let root = identities(&case.get("/api/v1/catalog", &case.token("root")));
    assert_eq!(
        root,
        [a.clone(), b.clone(), "github.com/acme/c".into(), y.clone()]
    );
    let checked = case.post(
        "/api/v1/catalog/check",
        &case.token("alice"),
        &json!({"identities": [y, b]}),
    );
    assert_eq!(checked.status, 200, "{}", checked.body);
    let states: Vec<(String, String)> = (checked.json()["results"].as_array().unwrap().iter())
        .map(|r| {
            (
                r["identity"].as_str().unwrap().into(),
                r["state"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            (y.clone(), "selectable".into()),
            (b.clone(), "unavailable".into())
        ]
    );
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
    let refused = case.create("alice", "ya yb", &["Y", "B"]);
    assert_eq!(refused.status, 403);
    assert!(!refused.body.contains(&b), "{}", refused.body);
    let huge =
        json!({"name": "big", "entries": case.entries(&["Y"]), "message": "m".repeat(90 << 10)});
    assert_eq!(
        case.post("/api/v1/workspaces", &case.token("alice"), &huge)
            .status,
        413
    );
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
    let all = case.post(
        "/api/v1/workspaces",
        &case.token("root"),
        &json!({"name": "everything", "entries": case.entries(&["Y", "A", "B"]), "message": "m"}),
    );
    assert_eq!(all.status, 201, "{}", all.body);
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
    let invalid = case.post(
        "/api/v1/workspaces",
        &case.token("alice"),
        &json!({"name": "p", "entries": format!("[[repository]]\nrepo = \"{}\"\npath = \"x\"\n", case.identity("Y")), "message": "m"}),
    );
    assert_eq!(
        (invalid.status, invalid.json()["error"]["code"].clone()),
        (422, json!("invalid_entries"))
    );
    let again = case.create("alice", "alice Y+A", &["Y", "A"]);
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.json()["created"], false);
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn peers_view_a_workspace_through_their_own_grants_and_operate_nothing() {
    if let Some(case) = Case::new("ws_peers") {
        peers_view_a_workspace_through_their_own_grants_and_operate_nothing_as(case);
    }
}

#[cfg(feature = "accounts")]
#[test]
fn local_accounts_peers_view_a_workspace_through_their_own_grants_and_operate_nothing() {
    if let Some(case) = Case::local("ws_peers_local") {
        peers_view_a_workspace_through_their_own_grants_and_operate_nothing_as(case);
    }
}

fn peers_view_a_workspace_through_their_own_grants_and_operate_nothing_as(case: Case) {
    case.refresh();
    case.ready();
    let id_of = |reply: &Reply| reply.json()["id"].as_str().unwrap().to_string();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let yab = id_of(&case.create("carol", "all", &["Y", "A", "B"]));
    assert_eq!(case.listed("alice"), vec![ya.clone()]);
    assert_eq!(case.listed("dave"), vec![ya.clone()]);
    assert_eq!(case.listed("erin"), vec![ya.clone()]);
    assert_eq!(case.listed("bob"), vec![yb.clone()]);
    let mut every = vec![ya.clone(), yb.clone(), yab.clone()];
    every.sort();
    assert_eq!(case.listed("carol"), every);
    assert_eq!(case.listed("root"), every);
    assert!(case.listed("yoda").is_empty());
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
    let unknown = case.get("/api/v1/workspaces/0123456789abcdef", &case.token("bob"));
    for hidden in [ya.as_str(), yab.as_str()] {
        let denied = case.get(&format!("/api/v1/workspaces/{hidden}"), &case.token("bob"));
        assert_eq!((denied.status, &denied.body), (404, &unknown.body));
    }
    let denied = case.get(&format!("/api/v1/workspaces/{yb}"), &case.token("yoda"));
    assert_eq!((denied.status, &denied.body), (404, &unknown.body));
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
    let save = json!({"entries": case.entries(&["Y", "A"]), "message": "x", "base": 1});
    let write = case.post(
        &format!("/api/v1/workspaces/{ya}/versions"),
        &case.token("dave"),
        &save,
    );
    assert_eq!(write.status, 403, "{}", write.body);
    assert_eq!(write.json()["error"]["code"], "not_operator");
    let write = case.post(
        &format!("/api/v1/workspaces/{ya}/versions"),
        &case.token("bob"),
        &save,
    );
    assert_eq!((write.status, &write.body), (404, &unknown.body));
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn a_created_workspace_becomes_queryable_for_group_peers_only() {
    if let Some(case) = Case::new("ws_query") {
        a_created_workspace_becomes_queryable_for_group_peers_only_as(case);
    }
}

#[cfg(feature = "accounts")]
#[test]
fn local_accounts_a_created_workspace_becomes_queryable_for_group_peers_only() {
    if let Some(case) = Case::local("ws_query_local") {
        a_created_workspace_becomes_queryable_for_group_peers_only_as(case);
    }
}

fn a_created_workspace_becomes_queryable_for_group_peers_only_as(case: Case) {
    case.refresh();
    case.ready();
    let id_of = |reply: &Reply| reply.json()["id"].as_str().unwrap().to_string();
    let ya = id_of(&case.create("alice", "Y and A", &["Y", "A"]));
    let yb = id_of(&case.create("bob", "Y and B", &["Y", "B"]));
    let yab = id_of(&case.create("carol", "all", &["Y", "A", "B"]));
    let (y, a, b) = (case.identity("Y"), case.identity("A"), case.identity("B"));

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
    let dave = case.node_token("dave");
    until_status(&node, &route(&ya, &a), &dave, 200);
    until_status(&node, &route(&yb, &b), &dave, 404);
    let alice = case.node_token("alice");
    let (status, listing) = node_call(&node, &route(&yb, &y), &alice, "handle");
    assert_eq!((status, listing.trim()), (404, ya.as_str()));
    let (status, unknown) = node_call(&node, &route("0123456789abcdef", &y), &alice, "handle");
    assert_eq!((status, unknown), (404, listing));
    let (status, _) = node_call(&node, &format!("/{y}"), &alice, "handle");
    assert_eq!(status, 404);
    let (_, through) = node_call(&node, &route(&ya, &y), &alice, "handle");
    assert!(!through.contains(&b), "{through}");
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
    let (status, _) = node_call(
        &node,
        &format!("/-/workspaces/not-an-id/{y}"),
        &alice,
        "handle",
    );
    assert_eq!(status, 404);

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
    let shown = case
        .get(&format!("/api/v1/workspaces/{id}"), &case.token("root"))
        .json();
    assert_eq!(shown["entries"][0]["ingestion"], "excluded", "{shown}");
    assert_eq!(shown["state"], "applied");
    case.github.clean(&case.console.stderr(), &[]);
}

#[test]
fn a_repository_selected_in_two_spellings_keeps_the_grant_of_each() {
    let Some(case) = Case::new("ws_spellings") else {
        return;
    };
    case.refresh();
    case.ready();
    let ya = case.create("alice", "ya", &["Y", "A"]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(case.listed("erin"), vec![ya.clone()]);
    let upper = case.identity("Y").replace("acme/y", "Acme/Y");
    let text = format!("[[repository]]\nrepo = \"{upper}\"\npath = \"../y\"\n");
    fs::write(case.dir.join("ops/upper.toml"), text).unwrap();
    let import = ["workspace", "import", "ops/upper.toml", "--message", "m"];
    let version = case.job_ok(&[&import[..], &["--workspace", MANIFEST]].concat());
    let locked = "ops/locked.workspace.toml";
    let manifest = fs::read_to_string(case.dir.join(MANIFEST)).unwrap();
    let with_lock = format!("{manifest}\n[refresh_lock]\nttl_seconds = 10\n");
    fs::write(case.dir.join(locked), with_lock).unwrap();
    case.job_ok(&["workspace", "apply", version.trim(), "--workspace", locked]);
    let root = case.token("root");
    let line = case.refresh();
    assert!(line.ends_with(", 0 exclusions\n"), "{line}");
    let start = Instant::now();
    loop {
        let listed = case.get("/api/v1/catalog", &root).json();
        let identities: Vec<&str> = (listed["repositories"].as_array().unwrap().iter())
            .map(|r| r["identity"].as_str().unwrap())
            .collect();
        if identities.contains(&upper.as_str()) {
            assert!(
                !identities.contains(&case.identity("Y").as_str()),
                "{listed}"
            );
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "{listed}");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(case.listed("erin"), vec![ya.clone()]);
    assert_eq!(case.listed("dave"), vec![ya.clone()]);
    assert!(case.listed("yoda").is_empty());
    let shown = case.get(&format!("/api/v1/workspaces/{ya}"), &case.token("erin"));
    assert_eq!(shown.status, 200, "{}", shown.body);
    case.github.clean(&case.console.stderr(), &[]);
}
