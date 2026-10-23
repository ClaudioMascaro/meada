//! Operational console reads through the binary and HTTP, against the configured services.

use serde_json::json;

use super::playground::{WEB, chain_console};
use super::{get, start, ui_section};
use crate::manifest::run;

#[test]
fn local_repositories_report_freshness_and_missing_modules_are_explicit() {
    let dir = chain_console("ui_read_local", "", &ui_section());
    assert!(run(&dir, "index").2);
    let console = start(&dir, "127.0.0.1:0");
    let reply = get(&console, "/api/v1/repositories");
    assert_eq!(reply.status, 200, "{}", reply.body);
    let repositories = reply.json();
    let web = repositories["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["identity"] == WEB)
        .unwrap();
    assert_eq!(web["behind"], false);
    assert_eq!(web["indexed_commit"], web["head"]);
    let original_provenance =
        std::fs::read(dir.join("indexes").join(WEB).join("index.commit.json")).unwrap();
    assert!(web["head"].as_str().is_some_and(|h| h.len() == 40));
    std::fs::write(
        dir.join("chain-web/src/read-state.ts"),
        "export const changed = 1;\n",
    )
    .unwrap();
    let reply = get(&console, "/api/v1/repositories").json();
    let web = reply["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["identity"] == WEB)
        .unwrap();
    assert_eq!(web["behind"], true);
    crate::common::git(&dir.join("chain-web"), &["add", "src/read-state.ts"]);
    crate::common::git(&dir.join("chain-web"), &["commit", "-qm", "advance mirror"]);
    let reply = get(&console, "/api/v1/repositories").json();
    let web = reply["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["identity"] == WEB)
        .unwrap();
    assert_ne!(web["indexed_commit"], web["head"]);
    assert!(run(&dir, "index").2);
    // A stale write after a rebuild/crash must never claim its older commit.
    let provenance = dir.join("indexes").join(WEB).join("index.commit.json");
    let mut oversized = std::fs::read(&provenance).unwrap();
    oversized.resize(4097, b' ');
    for bytes in [original_provenance, oversized] {
        std::fs::write(&provenance, bytes).unwrap();
        let reply = get(&console, "/api/v1/repositories").json();
        let web = reply["repositories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["identity"] == WEB)
            .unwrap();
        assert_eq!(web["indexed_commit"], serde_json::Value::Null);
    }
    std::fs::remove_file(provenance).unwrap();
    let reply = get(&console, "/api/v1/repositories").json();
    let web = reply["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["identity"] == WEB)
        .unwrap();
    assert_eq!(web["indexed_commit"], serde_json::Value::Null);
    for (route, needed) in [
        ("fleet", "generations"),
        ("store", "snapshot"),
        ("workspace", "generations"),
        ("credentials", "github_app"),
    ] {
        let reply = get(&console, &format!("/api/v1/{route}"));
        assert_eq!(reply.status, 200, "{route}: {}", reply.body);
        assert_eq!(reply.json()["requires"], json!([needed]));
    }
}

#[cfg(feature = "generation-pointer")]
mod fleet {
    use super::*;
    use crate::generations::{Companion, Fleet, admin, import, prefix, send};
    use crate::manifest::MANIFEST;
    use crate::server::Server;
    use crate::snapshot::{KEY_ID, SECRET, bucket, next_commit, put_stored};
    use serde_json::Value;
    use std::fs;
    use std::time::{Duration, Instant};

    fn wait(console: &Server, predicate: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let reply = get(console, "/api/v1/fleet");
            assert_eq!(reply.status, 200, "{}", reply.body);
            let body = reply.json();
            if predicate(&body) {
                return body;
            }
            assert!(Instant::now() < end, "{body}");
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    pub(super) fn console(fleet: &Fleet) -> (String, Server) {
        let (manifest, _) = fleet.node("console");
        super::super::front_end(&fleet.dir, Some(super::super::VERSION));
        let file = fleet.dir.join(&manifest);
        let text = fs::read_to_string(&file).unwrap().replace(
            "local_dir = \"generations\"",
            "local_dir = \"generations\"\nnode_name = \"console\"",
        );
        fs::write(file, format!("{text}\n[ui]\n{}", ui_section())).unwrap();
        let server = Server::start_command(
            &fleet.dir,
            "ui",
            &["--listen", "127.0.0.1:0", "--workspace", &manifest],
            &[("MEADA_TEST_S3_KEY", KEY_ID)],
        );
        (manifest, server)
    }

    #[test]
    fn reads_live_fleet_retention_store_workspace_and_hostile_heartbeats() {
        let Some(fleet) = Fleet::new("ui_operational", 2) else {
            return;
        };
        let (version, _) = import(&fleet.dir, MANIFEST, "first version", "alice");
        let first = fleet.publish();
        next_commit(&fleet.dir, "chain-web", "second_read");
        let second = fleet.publish();
        next_commit(&fleet.dir, "chain-web", "third_read");
        let current = fleet.publish();
        let s3 = bucket(&fleet.endpoint, &fleet.bucket);
        let key = format!("prod/meada/generations/{current}.json");
        let mut document: Value = serde_json::from_slice(&crate::snapshot::get(&s3, &key)).unwrap();
        document["workspace"] = json!(version);
        put_stored(&s3, &key, &serde_json::to_vec(&document).unwrap());
        let (console_manifest, console) = console(&fleet);
        let (node0, _) = fleet.node("node0");
        let (node1, _) = fleet.node("node1");
        for (manifest, name) in [(&node0, "node0"), (&node1, "node1")] {
            let path = fleet.dir.join(manifest);
            let text = fs::read_to_string(&path).unwrap().replace(
                "local_dir = \"generations\"",
                &format!("local_dir = \"generations\"\nnode_name = \"{name}\""),
            );
            fs::write(path, text).unwrap();
        }
        let companions: Vec<_> = [&node0, &node1, &console_manifest]
            .into_iter()
            .map(|manifest| Companion::start(&fleet.dir, manifest, &[]))
            .collect();
        let state = wait(&console, |body| {
            body["nodes"].as_array().is_some_and(|nodes| {
                nodes.len() == 3 && nodes.iter().all(|node| node["heartbeat"]["ready"] == true)
            })
        });
        assert_eq!(state["pointer"], json!({"id": current, "fence": 0}));
        assert_eq!(state["nodes"][0]["workspace"], version);
        assert_eq!(state["last_publish"]["generation"], current);
        assert!(
            state["last_publish"]["modified"]
                .as_str()
                .unwrap()
                .contains('T')
        );
        assert_eq!(state["stuck"], Value::Null);
        let mut valkey = admin(&fleet.valkey);
        let prefix = format!("{{{}}}", prefix(fleet.case));
        send(&mut valkey, &["SADD", &format!("{prefix}:kept"), &second]);
        let store = get(&console, "/api/v1/store").json();
        let rows = store["generations"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert!(
            rows.iter()
                .find(|row| row["id"] == first)
                .unwrap()
                .get("document")
                .is_none()
        );
        assert_eq!(
            rows.iter().find(|row| row["id"] == second).unwrap()["kept"],
            true
        );
        let detail = get(&console, &format!("/api/v1/store/generations/{first}"));
        assert_eq!(detail.status, 200, "{}", detail.body);
        let entry = &detail.json()["document"]["repositories"][0];
        let (size, hash) = crate::snapshot::metadata(&s3, entry["bundle"].as_str().unwrap());
        assert_eq!(entry["size"], size);
        assert_eq!(entry["blake3"], hash);
        for bad in ["../../etc/passwd", "0123456789ABCDEF", "0000000000000000"] {
            assert_eq!(
                get(&console, &format!("/api/v1/store/generations/{bad}")).status,
                404
            );
        }
        let repositories = get(&console, "/api/v1/repositories").json();
        assert_eq!(repositories["repositories"].as_array().unwrap().len(), 4);
        assert!(
            repositories["repositories"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["generation"] == current && row["schema"].is_i64())
        );
        assert_eq!(
            get(&console, "/api/v1/workspace").json()["current"],
            Value::Null
        );
        send(
            &mut valkey,
            &[
                "HSET",
                &format!("{prefix}:workspace"),
                "id",
                &version,
                "fence",
                "3",
            ],
        );
        let workspace = get(&console, "/api/v1/workspace").json();
        assert_eq!(workspace["current"], json!({"id": version, "fence": 3}));
        assert_eq!(workspace["version"]["author"], "alice");
        assert_eq!(
            get(&console, &format!("/api/v1/workspace/versions/{version}")).json(),
            workspace["version"]
        );
        put_stored(&s3, "prod/meada/workspaces/0000000000000000.json", b"{}");
        let listing = get(&console, "/api/v1/workspace/versions").json();
        assert_eq!(listing["versions"][0]["id"], version);
        assert_eq!(listing["malformed"].as_array().unwrap().len(), 1);
        for companion in companions {
            companion.stop();
        }
        let mut beat = state["nodes"][0]["heartbeat"].clone();
        beat["name"] = json!("stuck");
        beat["generation"] = json!(second);
        beat["fence"] = json!(9);
        let nodes = format!("{prefix}:nodes");
        for (name, altered) in [
            ("stuck", beat.clone()),
            ("bad-id", {
                let mut b = beat.clone();
                b["generation"] = json!("../../etc/passwd");
                b
            }),
            ("bad-fence", {
                let mut b = beat.clone();
                b["fence"] = json!(-1);
                b
            }),
            ("../bad-name", beat.clone()),
            ("missing-pointer", {
                let mut b = beat.clone();
                b.as_object_mut().unwrap().remove("generation");
                b.as_object_mut().unwrap().remove("fence");
                b
            }),
        ] {
            let mut altered = altered;
            altered["name"] = json!(name);
            send(&mut valkey, &["ZADD", &nodes, "9999999999", name]);
            send(
                &mut valkey,
                &[
                    "SET",
                    &format!("{prefix}:node:{name}"),
                    &altered.to_string(),
                ],
            );
        }
        let state = get(&console, "/api/v1/fleet").json();
        assert_eq!(state["malformed"].as_array().unwrap().len(), 4);
        let invalid_name = state["malformed"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "../bad-name")
            .unwrap();
        let reason_safe = !invalid_name["reason"]
            .as_str()
            .unwrap()
            .contains("../bad-name");
        assert_eq!(state["highest_fence"], 9);
        let mut unheld = beat.clone();
        unheld["name"] = json!("unheld");
        unheld["generation"] = json!(first);
        unheld["fence"] = json!(0);
        send(&mut valkey, &["ZADD", &nodes, "9999999999", "unheld"]);
        send(
            &mut valkey,
            &["SET", &format!("{prefix}:node:unheld"), &unheld.to_string()],
        );
        let repositories = get(&console, "/api/v1/repositories").json();
        let rows: Vec<_> = repositories["repositories"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["generation"] == first)
            .collect();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row["schema"].is_null()));
        assert_eq!(state["stuck"]["kind"], "fleet");
        let stuck = state["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == "stuck")
            .unwrap();
        assert_eq!(stuck["stuck"]["recoveries"].as_array().unwrap().len(), 2);
        #[cfg(feature = "refresh-lock")]
        {
            let path = fleet.dir.join(MANIFEST);
            let text = fs::read_to_string(&path).unwrap() + "\n[refresh_lock]\nttl_seconds = 10\n";
            fs::write(&path, text).unwrap();
            let (_, stderr, status) = crate::generations::meada_in(
                &fleet.dir,
                "en_US.UTF-8",
                &["snapshot", "fence", "recover", "--workspace", MANIFEST],
            );
            assert_eq!(status, Some(0), "{stderr}");
            assert_eq!(get(&console, "/api/v1/fleet").json()["stuck"], Value::Null);
        }
        // A live CLI lock exposes its real holder and remaining TTL without a console write.
        #[cfg(feature = "refresh-lock")]
        {
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_meada"));
            let mut lock = command
                .current_dir(&fleet.dir)
                .args([
                    "snapshot",
                    "lock",
                    "--workspace",
                    MANIFEST,
                    "--",
                    "sleep",
                    "4",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let state = wait(&console, |body| {
                body["holder"]["ttl_ms"].as_i64().is_some_and(|ttl| ttl > 0)
            });
            assert!(state["holder"]["holds"].as_str().unwrap().contains("sleep"));
            assert!(
                state["holder"]["pid"]
                    .as_str()
                    .unwrap()
                    .parse::<u32>()
                    .is_ok()
            );
            assert!(lock.wait().unwrap().success());
        }
        send(
            &mut valkey,
            &["SET", &format!("{prefix}:lock"), "opaque", "PX", "10000"],
        );
        send(
            &mut valkey,
            &[
                "HSET",
                &format!("{prefix}:holder"),
                "host",
                "operator",
                "pid",
                "+1",
                "started",
                "2026-10-22T00:00:00Z",
                "holds",
                "hostile\nargument",
            ],
        );
        let holder = get(&console, "/api/v1/fleet").json()["holder"].clone();
        assert_eq!(
            json!({"reason_safe": reason_safe, "holder": holder["malformed"], "safe_arguments": holder.get("holds").is_none()}),
            json!({"reason_safe": true, "holder": "malformed lock holder", "safe_arguments": true})
        );
        assert!(!get(&console, "/api/v1/fleet").body.contains(SECRET));
    }
}

/// A hanging Valkey consumes one deadline for the whole read; basic routes and invalid ids
/// remain available, including when the store endpoint cannot answer either.
#[cfg(feature = "generation-pointer")]
#[test]
fn service_reads_share_a_two_second_deadline_and_identify_the_service() {
    use std::net::TcpListener;
    use std::time::{Duration, Instant};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_secs(4));
    });
    let head = format!(
        "[valkey]\nurl = \"redis://{address}\"\nkey_prefix = \"ui-deadline\"\n[snapshot]\nendpoint = \"http://127.0.0.1:9\"\nregion = \"us-east-1\"\nbucket = \"unreachable\"\npath_style = true\n[generations]\nkeep = 2\n"
    );
    let dir = super::console_dir(
        "ui_read_deadline",
        Some(super::VERSION),
        &head,
        &ui_section(),
    );
    let console = start(&dir, "127.0.0.1:0");
    let began = Instant::now();
    let refused = get(&console, "/api/v1/fleet");
    assert_eq!(refused.status, 503, "{}", refused.body);
    assert!(
        refused.json()["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("valkey:")
    );
    assert!(
        began.elapsed() < Duration::from_millis(2800),
        "{:?}",
        began.elapsed()
    );
    for route in ["config", "session", "manifest"] {
        assert_eq!(get(&console, &format!("/api/v1/{route}")).status, 200);
    }
    let began = Instant::now();
    assert_eq!(
        get(&console, "/api/v1/store/generations/../../secret").status,
        404
    );
    assert!(began.elapsed() < Duration::from_millis(500));
}

/// A credential must be a regular file; the HTTP read must refuse a FIFO before opening it.
#[cfg(all(unix, feature = "snapshot"))]
#[test]
fn a_nonregular_store_credential_is_refused_without_opening_it() {
    use std::time::{Duration, Instant};
    let head = "[snapshot]\nendpoint = \"http://127.0.0.1:9\"\nregion = \"us-east-1\"\nbucket = \"unreachable\"\npath_style = true\naccess_key_id_env = \"MEADA_READ_KEY\"\nsecret_access_key_file = \"slow-secret\"\n";
    let dir = super::console_dir(
        "ui_store_credential_deadline",
        Some(super::VERSION),
        head,
        &ui_section(),
    );
    let secret = dir.join("ops/slow-secret");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&secret)
            .status()
            .unwrap()
            .success()
    );
    let console = crate::server::Server::start_command(
        &dir,
        "ui",
        &[
            "--listen",
            "127.0.0.1:0",
            "--workspace",
            crate::manifest::MANIFEST,
        ],
        &[("MEADA_READ_KEY", "test-key")],
    );
    let began = Instant::now();
    let reply = get(&console, "/api/v1/store");
    assert_eq!(reply.status, 503, "{}", reply.body);
    assert!(
        reply.json()["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("store:")
    );
    assert!(reply.body.contains("not a regular file"));
    assert!(began.elapsed() < Duration::from_millis(500));
    assert_eq!(get(&console, "/api/v1/config").status, 200);
}

/// Four slow reads retain their workers after timing out. A later burst waits within the
/// same deadline and must not start more work; the basic routes keep answering throughout.
#[cfg(unix)]
#[test]
fn read_deadlines_bound_workers_and_overload_without_blocking_basic_routes() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let dir = chain_console("ui_read_workers", "", &ui_section());
    assert!(run(&dir, "index").2);
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
case " $* " in
  *" rev-parse --verify HEAD "*)
    if [ -f "$MEADA_READ_BLOCK" ]; then
      printf '%s\n' "$$" >> "$MEADA_READ_SEEN"
      while [ -f "$MEADA_READ_BLOCK" ]; do sleep 0.05; done
    fi;;
esac
exec /usr/bin/git "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let flag = dir.join("hold-git");
    let seen = dir.join("git-readers");
    fs::write(&seen, "").unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let console = crate::server::Server::start_command(
        &dir,
        "ui",
        &[
            "--listen",
            "127.0.0.1:0",
            "--workspace",
            crate::manifest::MANIFEST,
        ],
        &[
            ("PATH", &path),
            ("MEADA_READ_BLOCK", flag.to_str().unwrap()),
            ("MEADA_READ_SEEN", seen.to_str().unwrap()),
        ],
    );
    fs::write(&flag, "hold").unwrap();
    let (release, watchdog) = std::sync::mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let unblocks = scope.spawn(move || {
            let _ = watchdog.recv_timeout(Duration::from_secs(8));
            let _ = fs::remove_file(&flag);
        });
        let first: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let began = Instant::now();
                    let reply = get(&console, "/api/v1/repositories");
                    (reply, began.elapsed())
                })
            })
            .collect();
        for request in first {
            let (reply, elapsed) = request.join().unwrap();
            assert_eq!(reply.status, 503, "{}", reply.body);
            assert!(elapsed < Duration::from_millis(2800), "{elapsed:?}");
        }
        assert_eq!(fs::read_to_string(&seen).unwrap().lines().count(), 4);
        let excess: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| get(&console, "/api/v1/repositories")))
            .collect();
        for route in ["config", "session", "manifest"] {
            let began = Instant::now();
            assert_eq!(get(&console, &format!("/api/v1/{route}")).status, 200);
            assert!(began.elapsed() < Duration::from_millis(500));
        }
        for request in excess {
            assert_eq!(request.join().unwrap().status, 503);
        }
        assert_eq!(
            fs::read_to_string(&seen).unwrap().lines().count(),
            4,
            "timed-out workers released their permits too early"
        );
        release.send(()).unwrap();
        unblocks.join().unwrap();
    });
    let reply = get(&console, "/api/v1/repositories");
    assert_eq!(reply.status, 200, "{}", reply.body);
}
