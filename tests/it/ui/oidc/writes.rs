//! Console operations through HTTP, using the same real stores as their CLI counterparts.

use super::{PUBLIC, SUBJECT, VERSION, WEB, ask, bearer, claims, oidc, ui_oidc};
use crate::generations::Fleet;
use crate::oidc::{Issuer, key, set, sign};
use crate::server::Server;
use crate::snapshot::{KEY_ID, next_commit};
use crate::ui::{Reply, front_end, send_body};
use jsonwebtoken::{Algorithm, EncodingKey};
use serde_json::{Value, json};
use std::fs;

fn start_operator(fleet: &Fleet, issuer: &Issuer) -> (String, Server) {
    let (manifest, _) = fleet.node("console-write");
    front_end(&fleet.dir, Some(VERSION));
    let file = fleet.dir.join(&manifest);
    let source = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{source}\n[refresh_lock]\nttl_seconds = 10\n{}\n[ui]\n{}operations = true\noperator_claim = \"operator\"\n", oidc(&issuer.base), ui_oidc())).unwrap();
    let server = Server::start_command(
        &fleet.dir,
        "ui",
        &["--listen", "127.0.0.1:0", "--workspace", &manifest],
        &[("MEADA_TEST_S3_KEY", KEY_ID)],
    );
    (manifest, server)
}

fn operator(encoding: &EncodingKey, issuer: &Issuer, subject: &str) -> String {
    let mut claims = claims(&issuer.base, &[WEB]);
    claims["operator"] = json!(true);
    claims["sub"] = json!(subject);
    sign(encoding, "write", Algorithm::ES256, &claims)
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

/// Retain, set and drop use the CLI operations, expose their resulting state and print one
/// audit line each. An operator's restricted grant does not hide any repository or generation.
#[test]
fn pointer_operations_match_the_cli_results_and_audit_each_accepted_write() {
    let Some(fleet) = Fleet::new("ui_write_pointer", 2) else {
        return;
    };
    let first = fleet.publish();
    next_commit(&fleet.dir, "chain-web", "write-second");
    let second = fleet.publish();
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (_, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    for (action, generation, result) in [
        ("retain", &first, first.clone()),
        ("set", &first, format!("{first} 2")),
        ("drop", &second, second.clone()),
    ] {
        let reply = post(
            &console,
            "pointer",
            &token,
            json!({"action": action, "generation": generation}),
        );
        assert_eq!(reply.status, 200, "{action}: {}", reply.body);
        assert_eq!(reply.json(), json!({"result": result}));
    }
    let state = ask(&console, "/api/v1/fleet", &token).json();
    assert_eq!(state["pointer"], json!({"id": first, "fence": 2}));
    assert_eq!(
        state["retained"],
        json!([{"id": first, "fence": 2, "kept": true}])
    );
    let store = ask(&console, "/api/v1/store", &token).json();
    let current = store["generations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == first)
        .unwrap();
    assert_eq!(
        current["document"]["repositories"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let stderr = console.stderr();
    let audit: Vec<_> = stderr
        .lines()
        .filter(|line| line.starts_with("ui: "))
        .collect();
    assert_eq!(
        audit,
        [
            format!("ui: {SUBJECT} pointer retain {first}: {first}"),
            format!("ui: {SUBJECT} pointer set {first}: {first} 2"),
            format!("ui: {SUBJECT} pointer drop {second}: {second}"),
        ]
    );
    for part in token.split('.') {
        assert!(!stderr.contains(part));
    }
}

/// Recovery preserves the CLI's floor semantics and decimal precision beyond JavaScript's
/// safe integer range; every accepted operation is audited once.
#[test]
fn fence_recover_returns_the_cli_counter_and_preserves_an_exact_decimal_floor() {
    let Some(fleet) = Fleet::new("ui_write_recover", 2) else {
        return;
    };
    fleet.publish();
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (_, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    for (floor, expected) in [
        (json!(12), "12"),
        (json!(3), "13"),
        (json!("9007199254740993"), "9007199254740993"),
    ] {
        let reply = post(
            &console,
            "fence/recover",
            &token,
            json!({"at_least": floor}),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        assert_eq!(reply.json(), json!({"result": expected}));
    }
    assert_eq!(
        ask(&console, "/api/v1/fleet", &token).json()["fence"],
        9007199254740993u64
    );
    let stderr = console.stderr();
    let audit: Vec<_> = stderr
        .lines()
        .filter(|line| line.starts_with("ui: "))
        .collect();
    assert_eq!(
        audit,
        [
            format!("ui: {SUBJECT} fence recover 12: 12"),
            format!("ui: {SUBJECT} fence recover 3: 13"),
            format!("ui: {SUBJECT} fence recover 9007199254740993: 9007199254740993"),
        ]
    );
}

/// Shape errors are safe body errors, not CLI argument echoes: a token pasted into an id or
/// floor can appear in neither the response nor the audit, and opens no operation lock.
#[test]
fn tokens_in_operation_arguments_are_not_reflected_or_recorded() {
    let Some(fleet) = Fleet::new("ui_write_argument_secret", 2) else {
        return;
    };
    fleet.publish();
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (_, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    let before = ask(&console, "/api/v1/fleet", &token).json();
    for (route, body) in [
        ("pointer", json!({"action": "set", "generation": token})),
        ("fence/recover", json!({"at_least": token})),
        ("workspace/apply", json!({"version": token})),
    ] {
        let reply = post(&console, route, &token, body);
        assert_eq!(reply.status, 400, "{}", reply.body);
        assert_eq!(
            reply.json()["error"]["message"],
            "not a valid console operation body"
        );
        for part in token.split('.') {
            assert!(!reply.body.contains(part));
        }
    }
    let after = ask(&console, "/api/v1/fleet", &token).json();
    assert_eq!(after["fence"], before["fence"]);
    assert_eq!(after["pointer"], before["pointer"]);
    let stderr = console.stderr();
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("ui: "))
            .count(),
        3
    );
    for part in token.split('.') {
        assert!(!stderr.contains(part), "{stderr}");
    }
}

/// A workspace apply uses the same fence counter as a publish. Recovery and the fleet
/// diagnostic count its pointer, while a malformed heartbeat cannot poison either with MAX.
#[test]
fn recovery_and_fleet_count_workspace_fences_and_ignore_malformed_heartbeats() {
    use crate::generations::{admin, meada_in, prefix, send};
    let Some(fleet) = Fleet::new("ui_write_recover_hostile", 2) else {
        return;
    };
    let generation = fleet.publish();
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (manifest, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    let prefix = format!("{{{}}}", prefix(fleet.case));
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["SET", &format!("{prefix}:fence"), "50"]);
    send(
        &mut valkey,
        &[
            "HSET",
            &format!("{prefix}:workspace"),
            "id",
            "0000000000000000",
            "fence",
            "100",
        ],
    );
    send(
        &mut valkey,
        &["ZADD", &format!("{prefix}:nodes"), "9999999999", "poison"],
    );
    let poisoned = json!({"name": "poison", "generation": generation, "fence": u64::MAX, "held": "not an array", "ready": true, "version": "0.1.0", "time": "2026-10-23T00:00:00Z"});
    send(
        &mut valkey,
        &[
            "SET",
            &format!("{prefix}:node:poison"),
            &poisoned.to_string(),
        ],
    );
    let before = ask(&console, "/api/v1/fleet", &token).json();
    let (stdout, stderr, code) = meada_in(
        &fleet.dir,
        "en_US.UTF-8",
        &["snapshot", "fence", "recover", "--workspace", &manifest],
    );
    assert_eq!((code, stdout.as_str()), (Some(0), "100\n"), "{stderr}");
    assert!(
        stderr.contains("not a heartbeat") && stderr.contains("not counted"),
        "{stderr}"
    );
    assert_eq!(before["highest_fence"], 100);
    assert_eq!(before["stuck"]["kind"], "fleet");
    assert_eq!(before["malformed"].as_array().unwrap().len(), 1);
    send(&mut valkey, &["SET", &format!("{prefix}:fence"), "50"]);
    let reply = post(&console, "fence/recover", &token, json!({}));
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.json(), json!({"result": "100"}));
    let after = ask(&console, "/api/v1/fleet", &token).json();
    assert_eq!(after["stuck"], Value::Null);
    assert_eq!(after["highest_fence"], 100);
    let stderr = console.stderr();
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("ui: "))
            .collect::<Vec<_>>(),
        [format!("ui: {SUBJECT} fence recover -: 100")]
    );
}

/// Save shares entry validation with CLI import, keeps the admitted author verbatim and
/// imports entries despite other top-level keys. Audit output keeps its single-line shape.
#[test]
fn workspace_save_uses_cli_validation_and_stores_the_authenticated_author() {
    use crate::generations::meada_in;
    let Some(fleet) = Fleet::new("ui_write_save", 2) else {
        return;
    };
    fleet.publish();
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (manifest, console) = start_operator(&fleet, &issuer);
    let author = format!("{SUBJECT}\nsecond subject");
    let token = operator(&encoding, &issuer, &author);
    let entries = format!(
        "index_dir = \"ignored\"\n[[repository]]\nrepo = \"{WEB}\"\npath = \"../chain-web\"\n"
    );
    let message = "operator draft\nprivate body message";
    let reply = post(
        &console,
        "workspace/versions",
        &token,
        json!({"entries": entries, "message": message}),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    let id = reply.json()["result"].as_str().unwrap().to_string();
    let version = ask(
        &console,
        &format!("/api/v1/workspace/versions/{id}"),
        &token,
    )
    .json();
    assert_eq!(version["author"], author);
    assert_eq!(version["message"], message);
    assert_eq!(version["repositories"][0]["repo"], WEB);
    let invalid = "[[repository]]\nrepo = \"../not-an-identity\"\npath = \"\"\n[[repository]]\nrepo = \"local/also-invalid\"\npath = \"\"\n";
    fs::write(fleet.dir.join("entries"), invalid).unwrap();
    let (_, cli, code) = meada_in(
        &fleet.dir,
        "en_US.UTF-8",
        &[
            "workspace",
            "import",
            "entries",
            "--message",
            "invalid",
            "--workspace",
            &manifest,
        ],
    );
    assert_eq!(code, Some(1), "{cli}");
    let expected = cli
        .lines()
        .map(|line| line.strip_prefix("meada: ").unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let reply = post(
        &console,
        "workspace/versions",
        &token,
        json!({"entries": invalid, "message": "invalid"}),
    );
    assert_eq!(reply.status, 422, "{}", reply.body);
    assert_eq!(reply.json()["error"]["message"], expected);
    assert_eq!(ask(&console, "/api/v1/fleet", &token).json()["fence"], 1);
    let stderr = console.stderr();
    let audit: Vec<_> = stderr
        .lines()
        .filter(|line| line.starts_with("ui: "))
        .collect();
    assert_eq!(audit.len(), 2, "{stderr}");
    assert_eq!(
        audit[0],
        format!("ui: {SUBJECT}\\nsecond subject workspace save {id}: {id}")
    );
    assert!(!stderr.contains("private body message"));
    for part in token.split('.') {
        assert!(!stderr.contains(part));
    }
}

/// The editor upload fits its own cap, but the pretty stored document must also fit the
/// existing workspace version ceiling. Neither CLI nor HTTP may leave an unreadable version.
#[test]
fn imports_and_saves_refuse_a_serialized_version_larger_than_the_reader_limit() {
    use crate::generations::meada_in;
    let Some(fleet) = Fleet::new("ui_write_version_limit", 2) else {
        return;
    };
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (manifest, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    let entries: String = (0..70000)
        .map(|i| format!("[[repository]]\nrepo=\"e.test/o/r{i}\"\npath=\"\"\n"))
        .collect();
    let body = json!({"entries": entries, "message": "sized"});
    assert!(
        body.to_string().len() < 4 << 20,
        "the HTTP upload itself is allowed"
    );
    fs::write(fleet.dir.join("entries"), &entries).unwrap();
    let (stdout, stderr, code) = meada_in(
        &fleet.dir,
        "en_US.UTF-8",
        &[
            "workspace",
            "import",
            "entries",
            "--message",
            "sized",
            "--workspace",
            &manifest,
        ],
    );
    assert_eq!(code, Some(1), "{stdout}; {stderr}");
    assert!(stdout.is_empty());
    assert!(
        stderr.contains("workspace version exceeds 4194304 bytes"),
        "{stderr}"
    );
    let reply = post(&console, "workspace/versions", &token, body);
    assert_eq!(reply.status, 409, "{}", reply.body);
    assert!(
        reply.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("workspace version exceeds 4194304 bytes")
    );
    let listing = ask(&console, "/api/v1/workspace/versions", &token).json();
    assert_eq!(listing["versions"], json!([]));
    assert_eq!(listing["malformed"], json!([]));
    assert_eq!(
        console
            .stderr()
            .lines()
            .filter(|line| line.starts_with("ui: "))
            .count(),
        1
    );
}

/// Applying saved entries moves only the workspace pointer, with the same fenced result as
/// the CLI. Unknown but well-formed versions are operational refusals, never body errors.
#[test]
fn workspace_apply_moves_the_pointer_and_preserves_the_cli_refusal() {
    use crate::generations::meada_in;
    let Some(fleet) = Fleet::new("ui_write_apply", 2) else {
        return;
    };
    let generation = fleet.publish();
    let mut valkey = crate::generations::admin(&fleet.valkey);
    let prefix = format!("{{{}}}", crate::generations::prefix(fleet.case));
    crate::generations::send(
        &mut valkey,
        &["ZADD", &format!("{prefix}:nodes"), "9999999999", "serving"],
    );
    let beat = json!({"name": "serving", "generation": generation, "fence": 0, "held": [generation], "ready": true, "version": "0.1.0", "time": "2026-10-23T00:00:00Z"});
    crate::generations::send(
        &mut valkey,
        &["SET", &format!("{prefix}:node:serving"), &beat.to_string()],
    );
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (manifest, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    let entries = fs::read_to_string(fleet.dir.join(crate::manifest::MANIFEST)).unwrap();
    let saved = post(
        &console,
        "workspace/versions",
        &token,
        json!({"entries": entries, "message": "apply"}),
    );
    assert_eq!(saved.status, 200, "{}", saved.body);
    let id = saved.json()["result"].as_str().unwrap().to_string();
    let reply = post(&console, "workspace/apply", &token, json!({"version": id}));
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.json(), json!({"result": format!("{id} 2")}));
    let current = ask(&console, "/api/v1/workspace", &token).json();
    assert_eq!(current["current"], json!({"id": id, "fence": 2}));
    assert_eq!(
        current["version"]["repositories"].as_array().unwrap().len(),
        4
    );
    let repositories = ask(&console, "/api/v1/repositories", &token).json();
    assert_eq!(repositories["repositories"].as_array().unwrap().len(), 4);
    let zero = "0000000000000000";
    let (_, cli, code) = meada_in(
        &fleet.dir,
        "en_US.UTF-8",
        &["workspace", "apply", zero, "--workspace", &manifest],
    );
    assert_eq!(code, Some(1));
    let reply = post(
        &console,
        "workspace/apply",
        &token,
        json!({"version": zero}),
    );
    assert_eq!(reply.status, 409, "{}", reply.body);
    let expected = cli.strip_prefix("meada: ").unwrap().trim_end();
    assert_eq!(reply.json()["error"]["message"], expected);
    assert_eq!(
        ask(&console, "/api/v1/workspace", &token).json()["current"],
        current["current"]
    );
    let stderr = console.stderr();
    let audit: Vec<_> = stderr
        .lines()
        .filter(|line| line.starts_with("ui: "))
        .collect();
    assert_eq!(
        audit,
        [
            format!("ui: {SUBJECT} workspace save {id}: {id}"),
            format!("ui: {SUBJECT} workspace apply {id}: {id} 2"),
            format!("ui: {SUBJECT} workspace apply {zero}: {expected}"),
        ]
    );
}

/// Body caps and shape errors are rejected before an operation acquires its fence. No refused
/// upload is reflected, and every refusal is one audit line with a safe outcome.
#[test]
fn operation_caps_and_malformed_bodies_are_safe_and_leave_state_unchanged() {
    let Some(fleet) = Fleet::new("ui_write_bodies", 2) else {
        return;
    };
    fleet.publish();
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (_, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    let before = ask(&console, "/api/v1/fleet", &token).json();
    let headers = [
        ("Authorization", bearer(&token)),
        ("Origin", PUBLIC.to_string()),
    ];
    let headers: Vec<_> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut attempts = 0;
    for (route, cap) in [
        ("pointer", 1024),
        ("fence/recover", 1024),
        ("workspace/versions", 4 << 20),
        ("workspace/apply", 1024),
    ] {
        let path = format!("/api/v1/{route}");
        let reply = send_body(&console, "POST", &path, &headers, &" ".repeat(cap + 1));
        assert_eq!(reply.status, 413, "{route}: {}", reply.body);
        assert!(
            reply.json()["error"]["message"]
                .as_str()
                .unwrap()
                .contains(&cap.to_string())
        );
        let reply = send_body(&console, "POST", &path, &headers, "{\"unparsed-secret\":");
        assert_eq!(reply.status, 400, "{route}: {}", reply.body);
        assert!(!reply.body.contains("unparsed-secret"));
        attempts += 2;
    }
    for (route, body) in [
        (
            "pointer",
            json!({"action":"refresh", "generation":"0000000000000000"}),
        ),
        (
            "pointer",
            json!({"action":"set", "generation":"0000000000000000", "extra":"private-extra"}),
        ),
        ("fence/recover", json!({"at_least":-1})),
        ("fence/recover", json!({"at_least":"18446744073709551616"})),
        (
            "workspace/versions",
            json!({"entries":"", "message":"ok", "author":"private-extra"}),
        ),
        ("workspace/apply", json!({"version":"../private-extra"})),
    ] {
        let reply = post(&console, route, &token, body);
        assert_eq!(reply.status, 400, "{route}: {}", reply.body);
        assert_eq!(
            reply.json()["error"]["message"],
            "not a valid console operation body"
        );
        attempts += 1;
    }
    let after = ask(&console, "/api/v1/fleet", &token).json();
    assert_eq!(before["pointer"], after["pointer"]);
    assert_eq!(before["fence"], after["fence"]);
    assert_eq!(before["retained"], after["retained"]);
    let stderr = console.stderr();
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("ui: "))
            .count(),
        attempts
    );
    for secret in ["unparsed-secret", "private-extra"]
        .into_iter()
        .chain(token.split('.'))
    {
        assert!(!stderr.contains(secret), "{stderr}");
    }
}

/// Every HTTP write keeps the CLI refresh lock and holder refusal, while CLI import remains
/// usable without that lock. Viewer refusals and held-lock refusals change neither pointer nor
/// stored versions; each is audited exactly once.
#[test]
fn all_writes_refuse_a_held_lock_and_viewers_without_changing_the_workspace() {
    use crate::generations::import;
    use crate::refresh_lock::{held_by, lock, wait_for};
    let Some(fleet) = Fleet::new("ui_write_lock", 2) else {
        return;
    };
    let generation = fleet.publish();
    let (version, _) = import(&fleet.dir, crate::manifest::MANIFEST, "initial", "cli");
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (manifest, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    let viewer = sign(
        &encoding,
        "write",
        Algorithm::ES256,
        &claims(&issuer.base, &[WEB]),
    );
    let entries = "[[repository]]\nrepo=\"e.test/team/new\"\npath=\"../new\"\n";
    let child = lock(
        &fleet.dir,
        &manifest,
        "touch ops/write-lock-ready; exec sleep 60",
    );
    wait_for(&fleet.dir.join("ops/write-lock-ready"));
    let expected = held_by(&fleet, child.id());
    let expected = expected.strip_prefix("meada: ").unwrap().trim_end();
    let before = ask(&console, "/api/v1/fleet", &token).json();
    let workspace = ask(&console, "/api/v1/workspace", &token).json();
    for (route, body) in [
        ("pointer", json!({"action":"set", "generation":generation})),
        ("fence/recover", json!({"at_least":"999"})),
        (
            "workspace/versions",
            json!({"entries":entries, "message":"refused private body"}),
        ),
        ("workspace/apply", json!({"version":version})),
    ] {
        let reply = post(&console, route, &token, body.clone());
        assert_eq!(reply.status, 409, "{route}: {}", reply.body);
        assert_eq!(reply.json()["error"]["message"], expected);
        assert_eq!(post(&console, route, &viewer, body).status, 403);
    }
    let after = ask(&console, "/api/v1/fleet", &token).json();
    for field in ["pointer", "retained", "holder", "fence"] {
        if field == "holder" {
            assert_eq!(after[field]["holds"], before[field]["holds"]);
        } else {
            assert_eq!(after[field], before[field]);
        }
    }
    assert_eq!(ask(&console, "/api/v1/workspace", &token).json(), workspace);
    assert_eq!(
        ask(&console, "/api/v1/workspace/versions", &token).json()["versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::write(fleet.dir.join("entries"), entries).unwrap();
    let (_, cli) = import(&fleet.dir, "entries", "CLI import while locked", "cli");
    assert!(cli.is_empty(), "{cli}");
    assert_eq!(
        ask(&console, "/api/v1/workspace/versions", &token).json()["versions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    child.wait();
    let stderr = console.stderr();
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("ui: "))
            .count(),
        8
    );
    assert!(!stderr.contains("refused private body"));
    for part in token.split('.') {
        assert!(!stderr.contains(part), "{stderr}");
    }
}

/// Validation still returns the CLI's complete diagnostics, but its audit outcome must never
/// record editor contents, even when a credential was pasted into an invalid identity.
#[test]
fn save_validation_audits_a_fixed_refusal_without_editor_token_text() {
    use crate::generations::meada_in;
    let Some(fleet) = Fleet::new("ui_write_save_audit", 2) else {
        return;
    };
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (manifest, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    let entries = format!("[[repository]]\nrepo=\"../{token}\"\npath=\"\"\n");
    fs::write(fleet.dir.join("entries"), &entries).unwrap();
    let (_, cli, code) = meada_in(
        &fleet.dir,
        "en_US.UTF-8",
        &[
            "workspace",
            "import",
            "entries",
            "--message",
            "private validation message",
            "--workspace",
            &manifest,
        ],
    );
    assert_eq!(code, Some(1));
    let expected = cli.strip_prefix("meada: ").unwrap().trim_end();
    let reply = post(
        &console,
        "workspace/versions",
        &token,
        json!({"entries": entries, "message": "private validation message"}),
    );
    assert_eq!(reply.status, 422, "{}", reply.body);
    assert_eq!(reply.json()["error"]["message"], expected);
    let stderr = console.stderr();
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("ui: "))
            .collect::<Vec<_>>(),
        [format!(
            "ui: {SUBJECT} workspace save -: workspace entries failed validation"
        )]
    );
    for secret in ["private validation message"]
        .into_iter()
        .chain(token.split('.'))
    {
        assert!(!stderr.contains(secret), "{stderr}");
    }
}

/// All u64 floors retain their transport shape; unusable counters are operational conflicts,
/// with the CLI catalog message, and leave a subsequent valid recovery usable.
#[test]
fn fence_recover_refuses_unusable_u64_floors_as_operational_conflicts() {
    let Some(fleet) = Fleet::new("ui_write_ceiling", 2) else {
        return;
    };
    fleet.publish();
    let (encoding, jwk) = key("write");
    let issuer = Issuer::new(set(&[&jwk]));
    let (_, console) = start_operator(&fleet, &issuer);
    let token = operator(&encoding, &issuer, SUBJECT);
    let before = ask(&console, "/api/v1/fleet", &token).json()["fence"].clone();
    for floor in [
        json!(9223372036854775807u64),
        json!(9223372036854775808u64),
        json!(u64::MAX),
        json!("18446744073709551615"),
    ] {
        let reply = post(&console, "fence/recover", &token, json!({"at_least":floor}));
        assert_eq!(reply.status, 409, "{}", reply.body);
        assert_eq!(reply.json()["error"]["code"], "conflict");
        assert!(
            reply.json()["error"]["message"]
                .as_str()
                .unwrap()
                .contains("maximum usable fence is 9223372036854775806")
        );
        assert_eq!(
            ask(&console, "/api/v1/fleet", &token).json()["fence"],
            before
        );
    }
    let recovered = post(
        &console,
        "fence/recover",
        &token,
        json!({"at_least":"9007199254740993"}),
    );
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(recovered.json(), json!({"result":"9007199254740993"}));
}
