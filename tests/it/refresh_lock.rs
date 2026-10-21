//! The refresh lock (ADR 0012, spec #259, feature `refresh-lock`): `meada snapshot lock` runs a
//! refresh as its child under a lock in the local Valkey `MEADA_TEST_VALKEY_URL` names, and the
//! fence of each acquisition keeps an overlapping or stalled job from replacing a newer
//! generation in the local S3-compatible server `MEADA_TEST_S3_ENDPOINT` names. Without either
//! variable these tests print why and skip; with `MEADA_TEST_SERVICES_REQUIRED=1` they fail.
#![cfg(feature = "refresh-lock")]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::generations::{Fleet, admin, pointer, prefix, send};
use crate::manifest::{MANIFEST, chain_manifest, run};
use crate::serving::probe;
use crate::snapshot::{KEY_ID, append_section, bucket, next_commit, objects, pull};

const MEADA: &str = env!("CARGO_BIN_EXE_meada");
const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// A fleet whose manifest has `[refresh_lock]` with the shortest TTL.
fn fleet(case: &'static str) -> Option<Fleet> {
    let fleet = Fleet::new(case, 3)?;
    append_section(&fleet.dir, "\n[refresh_lock]\nttl_seconds = 10\n");
    Some(fleet)
}

/// `meada <args>` in `dir` as the tests run it, the store's key id set; `MEADA` names the
/// binary for a child's script.
fn meada(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(MEADA);
    command
        .current_dir(dir)
        .args(args)
        .env("LANG", "en_US.UTF-8")
        .env("MEADA", MEADA)
        .env("MEADA_TEST_S3_KEY", KEY_ID)
        .env_remove("MEADA_TEST_S3_TOKEN")
        .env_remove("MEADA_REFRESH_LOCK")
        .env_remove("MEADA_REFRESH_FENCE");
    command
}

fn text(out: &Output) -> (String, String, Option<i32>) {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

/// `meada snapshot lock --workspace <manifest> -- sh -c <script>`, started.
fn lock(dir: &Path, manifest: &str, script: &str) -> Child {
    meada(dir, &["snapshot", "lock", "--workspace", manifest, "--"])
        .args(["sh", "-c", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meada runs")
}

/// `meada snapshot publish` as a job holding `token` at `fence` would run it.
fn publish_as(dir: &Path, token: &str, fence: u64) -> (String, String, Option<i32>) {
    let out = meada(dir, &["snapshot", "publish", "--workspace", MANIFEST])
        .env("MEADA_REFRESH_LOCK", token)
        .env("MEADA_REFRESH_FENCE", fence.to_string())
        .output()
        .expect("meada runs");
    text(&out)
}

/// Waits up to 30 s for `path` to exist.
fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "{}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The fence and generation in `{<prefix>}:current`.
fn current(fleet: &Fleet) -> (String, String) {
    let (hash, _) = pointer(&mut admin(&fleet.valkey), fleet.case);
    (hash[0].clone(), hash[1].clone())
}

/// Two `lock` runs at once: the second exits 1 naming the lock and runs nothing, the first
/// runs its refresh (`meada index`, then `meada snapshot publish` with the variables `lock`
/// gave it) and releases the lock, whose next acquisition gets the next fence.
#[test]
fn two_lock_runs_at_once_one_runs_and_one_exits_1() {
    let Some(fleet) = fleet("lock_twice") else {
        return;
    };
    let dir = &fleet.dir;
    let script = "echo \"$MEADA_REFRESH_LOCK $MEADA_REFRESH_FENCE\" > ops/env; \
                  while [ ! -e ops/go ]; do sleep 0.05; done; \
                  \"$MEADA\" index --workspace ops/meada.workspace.toml && \
                  exec \"$MEADA\" snapshot publish --workspace ops/meada.workspace.toml";
    let first = lock(dir, MANIFEST, script);
    wait_for(&dir.join("ops/env"));
    let second = lock(dir, MANIFEST, "touch ops/ran");
    let (stdout, stderr, code) = text(&second.wait_with_output().unwrap());
    let key = format!("{{{}}}:lock", prefix(fleet.case));
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert_eq!(
        stderr,
        format!("meada: {key}: held by another refresh: nothing run\n")
    );
    assert!(!dir.join("ops/ran").exists());

    fs::write(dir.join("ops/go"), "").unwrap();
    let (stdout, stderr, code) = text(&first.wait_with_output().unwrap());
    assert_eq!(code, Some(0), "{stderr}");
    let env = fs::read_to_string(dir.join("ops/env")).unwrap();
    let (token, fence) = env.trim().split_once(' ').unwrap();
    assert!(token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(fence, "1");
    assert_eq!(
        current(&fleet),
        (stdout.trim().to_string(), "1".to_string())
    );
    // Released: the lock is gone, and the next run takes it with the next fence.
    let held: Option<String> = redis::cmd("GET")
        .arg(&key)
        .query(&mut admin(&fleet.valkey))
        .unwrap();
    assert_eq!(held, None);
    let again = lock(
        dir,
        MANIFEST,
        "echo $MEADA_REFRESH_FENCE > ops/fence; exit 3",
    );
    assert_eq!(again.wait_with_output().unwrap().status.code(), Some(3));
    assert_eq!(fs::read_to_string(dir.join("ops/fence")).unwrap(), "2\n");
}

/// A TCP forwarder to `to` that stops forwarding, both ways and for new connections, once
/// `blocked` is set: a Valkey that stalls.
fn proxy(to: &str) -> (u16, Arc<AtomicBool>) {
    let to = to.to_socket_addrs().unwrap().next().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let blocked = Arc::new(AtomicBool::new(false));
    let flag = blocked.clone();
    std::thread::spawn(move || {
        let mut stalled = Vec::new();
        for client in listener.incoming().flatten() {
            if flag.load(Ordering::SeqCst) {
                stalled.push(client);
                continue;
            }
            let Ok(server) = TcpStream::connect(to) else {
                continue;
            };
            let pairs = [
                (client.try_clone().unwrap(), server.try_clone().unwrap()),
                (server, client),
            ];
            for (mut from, mut to) in pairs {
                let flag = flag.clone();
                std::thread::spawn(move || {
                    let mut buf = [0; 4096];
                    while let Ok(n) = from.read(&mut buf) {
                        let forward = n > 0 && !flag.load(Ordering::SeqCst);
                        if n == 0 || (forward && to.write_all(&buf[..n]).is_err()) {
                            break;
                        }
                    }
                });
            }
        }
    });
    (port, blocked)
}

/// Whether process `pid` is gone (or a zombie no one waits for yet), within 5 s.
fn gone(pid: &str) -> bool {
    let stat = PathBuf::from(format!("/proc/{pid}/stat"));
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        match fs::read_to_string(&stat) {
            Err(_) => return true,
            Ok(s)
                if s.rsplit(')')
                    .next()
                    .is_some_and(|r| r.trim_start().starts_with('Z')) =>
            {
                return true;
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    false
}

/// A child outliving the TTL while no renewal gets through (Valkey stalls behind a proxy) is
/// terminated with its whole process group, `lock` exits 1 saying the lock was lost, and the
/// publish the child would have run leaves the pointer unchanged.
#[test]
fn a_child_outliving_the_ttl_with_renewal_blocked_is_terminated() {
    let Some(fleet) = fleet("lock_lost") else {
        return;
    };
    let dir = &fleet.dir;
    let lock_key = format!("{{{}}}:lock", prefix(fleet.case));
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["SET", &lock_key, TOKEN]);
    let (g1, stderr, code) = publish_as(dir, TOKEN, 1);
    assert_eq!(code, Some(0), "{stderr}");
    send(&mut valkey, &["DEL", &lock_key]);
    next_commit(dir, "chain-web", "stalled");

    let host = fleet
        .valkey
        .trim_start_matches("redis://")
        .trim_end_matches('/');
    let (port, blocked) = proxy(host);
    let toml = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let proxied = toml.replace(&fleet.valkey, &format!("redis://127.0.0.1:{port}"));
    assert_ne!(proxied, toml);
    fs::write(dir.join("ops/lock.toml"), proxied).unwrap();
    let script = "sleep 60 & echo $! > ops/sleeper; touch ops/started; wait; \
                  exec \"$MEADA\" snapshot publish --workspace ops/meada.workspace.toml";
    let start = Instant::now();
    let child = lock(dir, "ops/lock.toml", script);
    wait_for(&dir.join("ops/started"));
    blocked.store(true, Ordering::SeqCst);
    let (stdout, stderr, code) = text(&child.wait_with_output().unwrap());
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    let lost =
        format!("meada: {lock_key}: the refresh lock was lost: the command was terminated\n");
    assert_eq!(stderr, lost);
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "{:?}",
        start.elapsed()
    );
    let sleeper = fs::read_to_string(dir.join("ops/sleeper")).unwrap();
    assert!(gone(sleeper.trim()), "the child's group outlived lock");
    assert_eq!(current(&fleet), (g1.trim().to_string(), "1".to_string()));
}

/// A renewal that finds another token in the lock (another job took it after it expired) loses
/// it: the child is terminated and `lock` exits 1, leaving the other job's lock alone.
#[test]
fn a_renewal_that_finds_another_token_terminates_the_child() {
    let Some(fleet) = fleet("lock_taken") else {
        return;
    };
    let dir = &fleet.dir;
    let child = lock(dir, MANIFEST, "touch ops/started; sleep 60");
    wait_for(&dir.join("ops/started"));
    let lock_key = format!("{{{}}}:lock", prefix(fleet.case));
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["SET", &lock_key, TOKEN]);
    let (stdout, stderr, code) = text(&child.wait_with_output().unwrap());
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(stderr.contains("the refresh lock was lost"), "{stderr}");
    let held: Option<String> = redis::cmd("GET").arg(&lock_key).query(&mut valkey).unwrap();
    assert_eq!(held.as_deref(), Some(TOKEN));
}

/// A SIGTERM to `lock` reaches the child's whole group; `lock` waits for it, releases the lock
/// and exits with the child's status (128 plus the signal).
#[test]
fn a_sigterm_to_lock_reaches_the_child_and_releases_the_lock() {
    let Some(fleet) = fleet("lock_term") else {
        return;
    };
    let dir = &fleet.dir;
    let child = lock(dir, MANIFEST, "sleep 60 & echo $! > ops/sleeper; wait");
    wait_for(&dir.join("ops/sleeper"));
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
    let (_, stderr, code) = text(&child.wait_with_output().unwrap());
    assert_eq!(code, Some(128 + 15), "{stderr}");
    let sleeper = fs::read_to_string(dir.join("ops/sleeper")).unwrap();
    assert!(gone(sleeper.trim()), "the child's group outlived lock");
    let lock_key = format!("{{{}}}:lock", prefix(fleet.case));
    let held: Option<String> = (redis::cmd("GET").arg(&lock_key))
        .query(&mut admin(&fleet.valkey))
        .unwrap();
    assert_eq!(held, None);
}

/// With `[refresh_lock]` a publish needs both variables. A publish whose token the lock no
/// longer holds uploads nothing and writes no pointer; one whose fence is not above the current
/// one is refused naming the fence key and the recovery; a newer fence succeeds.
#[test]
fn a_publish_with_a_stale_fence_is_refused_while_a_newer_one_succeeds() {
    let Some(fleet) = fleet("lock_fence") else {
        return;
    };
    let dir = &fleet.dir;
    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let out = meada(dir, &["snapshot", "publish", "--workspace", MANIFEST]).output();
    let (stdout, stderr, code) = text(&out.unwrap());
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(
        stderr.contains("needs MEADA_REFRESH_LOCK and MEADA_REFRESH_FENCE"),
        "{stderr}"
    );
    assert!(objects(&s3, "prod/meada").is_empty());

    let p = prefix(fleet.case);
    let (lock_key, fence_key) = (format!("{{{p}}}:lock"), format!("{{{p}}}:fence"));
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["SET", &lock_key, TOKEN]);
    let (g1, stderr, code) = publish_as(dir, TOKEN, 5);
    assert_eq!(code, Some(0), "{stderr}");
    let g1 = g1.trim().to_string();

    // The lock holds another token: nothing more is uploaded and the pointer stays.
    next_commit(dir, "chain-web", "second");
    let before = objects(&s3, "prod/meada").len();
    let (_, stderr, code) = publish_as(dir, "ffffffffffffffffffffffffffffffff", 9);
    assert_eq!(code, Some(1), "{stderr}");
    let not_held = format!("meada: {lock_key}: the refresh lock no longer holds this job's token");
    assert!(stderr.starts_with(&not_held), "{stderr}");
    assert_eq!(objects(&s3, "prod/meada").len(), before);
    assert_eq!(current(&fleet), (g1.clone(), "5".to_string()));

    // A stale fence: refused, naming the fence key and the recovery.
    let (_, stderr, code) = publish_as(dir, TOKEN, 5);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("refused fence 5"), "{stderr}");
    assert!(
        stderr.contains(&format!("set {fence_key} at or above")),
        "{stderr}"
    );
    assert!(stderr.contains("<local_dir>/current"), "{stderr}");
    assert_eq!(current(&fleet), (g1.clone(), "5".to_string()));

    let (g2, stderr, code) = publish_as(dir, TOKEN, 6);
    assert_eq!(code, Some(0), "{stderr}");
    assert_ne!(g2.trim(), g1);
    assert_eq!(current(&fleet), (g2.trim().to_string(), "6".to_string()));
}

/// After `DEL {<prefix>}:fence {<prefix>}:current` the next `lock` publishes at fence 1, but a
/// node holding fence 5 does not swap and its `status` names the stuck case (exit code
/// unchanged); once the node's pointer file is deleted, its next pull swaps.
#[test]
fn a_lost_fence_stops_a_node_until_its_pointer_file_goes() {
    let Some(fleet) = fleet("lock_stuck") else {
        return;
    };
    let dir = &fleet.dir;
    let p = prefix(fleet.case);
    let (lock_key, fence_key) = (format!("{{{p}}}:lock"), format!("{{{p}}}:fence"));
    let current_key = format!("{{{p}}}:current");
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["SET", &lock_key, TOKEN]);
    let (g1, stderr, code) = publish_as(dir, TOKEN, 5);
    assert_eq!(code, Some(0), "{stderr}");
    let g1 = g1.trim().to_string();
    send(&mut valkey, &["DEL", &lock_key]);
    let (node, local) = fleet.node("node");
    assert_eq!(fleet.pull(&node), g1);
    let pointer_file = local.join("current");
    assert_eq!(
        fs::read_to_string(&pointer_file).unwrap(),
        format!("{g1} 5\n")
    );

    send(&mut valkey, &["DEL", &fence_key, &current_key]);
    next_commit(dir, "chain-web", "after_flush");
    let script = "exec \"$MEADA\" snapshot publish --workspace ops/meada.workspace.toml";
    let (g2, stderr, code) = text(&lock(dir, MANIFEST, script).wait_with_output().unwrap());
    assert_eq!(code, Some(0), "{stderr}");
    let g2 = g2.trim().to_string();
    assert_eq!(current(&fleet), (g2.clone(), "1".to_string()));

    let (stdout, stderr, code) = pull(dir, &node, &[]);
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(
        stderr.contains(&format!("generation {g2} has fence 1, below fence 5")),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("set {fence_key} at or above 5")),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&pointer_file).unwrap(),
        format!("{g1} 5\n")
    );

    let (stdout, stderr, code) = probe(dir, &node);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
    // As the node's manifest names it: relative to the directory `status` runs in.
    let shown = "node/generations/current";
    let stuck = format!(
        "fence: stuck: 5 in {shown} is above 1 in {current_key}; set {fence_key} at or above 5 \
         and publish again, or delete {shown}\n"
    );
    assert!(stdout.ends_with(&stuck), "{stdout}");

    fs::remove_file(&pointer_file).unwrap();
    assert_eq!(fleet.pull(&node), g2);
    assert_eq!(
        fs::read_to_string(&pointer_file).unwrap(),
        format!("{g2} 1\n")
    );
    let (stdout, _, code) = probe(dir, &node);
    assert_eq!(code, Some(0));
    assert!(!stdout.contains("fence:"), "{stdout}");
}

/// `[refresh_lock]` needs `[generations]`, and a TTL of at least ten seconds.
#[test]
fn refresh_lock_section_errors_name_the_file_the_line_and_the_section() {
    let dir = chain_manifest("lock_section");
    let at = "meada: ops/meada.workspace.toml";
    let snapshot = "[snapshot]\nendpoint = \"http://127.0.0.1:9\"\nregion = \"us-east-1\"\n\
                    bucket = \"meada-x\"\npath_style = true\n";
    let pointer = format!("{snapshot}[valkey]\nurl = \"redis://127.0.0.1\"\n[generations]\n");
    for (sections, expected) in [
        (
            format!("{snapshot}[valkey]\nurl = \"redis://127.0.0.1\"\n[refresh_lock]\n"),
            format!("{at}:10: [refresh_lock] needs [generations]\n"),
        ),
        (
            format!("{pointer}[refresh_lock]\nttl_seconds = 9\n"),
            format!("{at}:12: `refresh_lock.ttl_seconds`: must be at least 10\n"),
        ),
    ] {
        let toml = format!("index_dir = \"../indexes\"\n\n{sections}");
        fs::write(dir.join(MANIFEST), toml).unwrap();
        let (stdout, stderr, ok) = run(&dir, "status");
        assert!(!ok && stdout.is_empty(), "{sections}");
        assert_eq!(stderr, expected, "{sections}");
    }
}
