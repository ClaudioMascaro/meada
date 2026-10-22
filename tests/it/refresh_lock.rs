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

use crate::generations::{Fleet, admin, pointer, prefix, send, sorted};
use crate::manifest::{MANIFEST, chain_manifest, run};
use crate::serving::probe;
use crate::snapshot::{KEY_ID, append_section, bucket, next_commit, objects, pull};

const MEADA: &str = env!("CARGO_BIN_EXE_meada");
const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// A fleet whose manifest has `[refresh_lock]` with the shortest TTL.
pub(crate) fn fleet(case: &'static str) -> Option<Fleet> {
    let fleet = Fleet::new(case, 3)?;
    append_section(&fleet.dir, "\n[refresh_lock]\nttl_seconds = 10\n");
    Some(fleet)
}

/// `meada <args>` in `dir` as the tests run it, the store's key id set; `MEADA` names the
/// binary for a child's script.
pub(crate) fn meada(dir: &Path, args: &[&str]) -> Command {
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

pub(crate) fn text(out: &Output) -> (String, String, Option<i32>) {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

/// A running `meada snapshot lock`, killed and reaped if a failed assertion drops it (its child
/// gets SIGTERM on Linux when it dies).
pub(crate) struct Running(Option<Child>);

impl Running {
    pub(crate) fn id(&self) -> u32 {
        self.0.as_ref().expect("running").id()
    }

    /// Stdout, stderr and the exit code, once it exits.
    pub(crate) fn wait(mut self) -> (String, String, Option<i32>) {
        let child = self.0.take().expect("running");
        text(&child.wait_with_output().unwrap())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `meada snapshot lock --workspace <manifest> -- sh -c <script>`, started.
pub(crate) fn lock(dir: &Path, manifest: &str, script: &str) -> Running {
    let child = meada(dir, &["snapshot", "lock", "--workspace", manifest, "--"])
        .args(["sh", "-c", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meada runs");
    Running(Some(child))
}

/// A script fragment: a `sleep 60` in the background, its pid in `ops/sleeper` once whole.
const SLEEPER: &str = "sleep 60 & echo $! > ops/sleeper.new && mv ops/sleeper.new ops/sleeper";

/// `meada snapshot publish` as a job holding `token` at `fence` would run it.
fn publish_as(dir: &Path, token: &str, fence: u64) -> (String, String, Option<i32>) {
    text(
        &publish_in(dir, "en_US.UTF-8", token, fence)
            .output()
            .expect("meada runs"),
    )
}

/// `publish_as`'s command under `lang`.
fn publish_in(dir: &Path, lang: &str, token: &str, fence: u64) -> Command {
    let mut publish = meada(dir, &["snapshot", "publish", "--workspace", MANIFEST]);
    publish.env("LANG", lang).env("MEADA_REFRESH_LOCK", token);
    publish.env("MEADA_REFRESH_FENCE", fence.to_string());
    publish
}

/// Waits up to 30 s for `path` to exist.
pub(crate) fn wait_for(path: &Path) {
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
pub(crate) fn current(fleet: &Fleet) -> (String, String) {
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
    let (stdout, stderr, code) = second.wait();
    let key = format!("{{{}}}:lock", prefix(fleet.case));
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert_eq!(stderr, held_by(&fleet, first.id()));
    assert!(!dir.join("ops/ran").exists());

    fs::write(dir.join("ops/go"), "").unwrap();
    let (stdout, stderr, code) = first.wait();
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
    assert_eq!(again.wait().2, Some(3));
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
    let script = format!(
        "echo \"$MEADA_REFRESH_LOCK $MEADA_REFRESH_FENCE\" > ops/env; {SLEEPER}; wait; \
         exec \"$MEADA\" snapshot publish --workspace ops/meada.workspace.toml"
    );
    let start = Instant::now();
    let child = lock(dir, "ops/lock.toml", &script);
    wait_for(&dir.join("ops/sleeper"));
    blocked.store(true, Ordering::SeqCst);
    let (stdout, stderr, code) = child.wait();
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
    // The publish the stalled job would have run, with its token and fence: the lock expired,
    // so it uploads nothing and the pointer stays.
    let env = fs::read_to_string(dir.join("ops/env")).unwrap();
    let (token, fence) = env.trim().split_once(' ').unwrap();
    let (stdout, stderr, code) = publish_as(dir, token, fence.parse().unwrap());
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    let not_held = format!("meada: {lock_key}: the refresh lock no longer holds this job's token");
    assert!(stderr.starts_with(&not_held), "{stderr}");
    assert_eq!(current(&fleet), (g1.trim().to_string(), "1".to_string()));
}

/// A child that ignores SIGTERM, when the lock is lost, gets SIGKILL `KILL_GRACE` (10 s) later
/// with its whole group, and `lock` exits 1.
#[test]
fn a_child_that_ignores_sigterm_is_killed_after_the_grace() {
    let Some(fleet) = fleet("lock_grace") else {
        return;
    };
    let dir = &fleet.dir;
    let child = lock(dir, MANIFEST, &format!("trap '' TERM; {SLEEPER}; wait"));
    wait_for(&dir.join("ops/sleeper"));
    let lock_key = format!("{{{}}}:lock", prefix(fleet.case));
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["SET", &lock_key, TOKEN]);
    let taken = Instant::now();
    let (stdout, stderr, code) = child.wait();
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(stderr.contains("the refresh lock was lost"), "{stderr}");
    assert!(
        taken.elapsed() >= Duration::from_secs(10),
        "{:?}",
        taken.elapsed()
    );
    let sleeper = fs::read_to_string(dir.join("ops/sleeper")).unwrap();
    assert!(gone(sleeper.trim()), "the child's group outlived lock");
    send(&mut valkey, &["DEL", &lock_key]);
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
    let (stdout, stderr, code) = child.wait();
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(stderr.contains("the refresh lock was lost"), "{stderr}");
    let held: Option<String> = redis::cmd("GET").arg(&lock_key).query(&mut valkey).unwrap();
    assert_eq!(held.as_deref(), Some(TOKEN));
    send(&mut valkey, &["DEL", &lock_key]);
}

/// A SIGTERM to `lock` reaches the child's whole group; `lock` waits for it, releases the lock
/// and exits with the child's status (128 plus the signal).
#[test]
fn a_sigterm_to_lock_reaches_the_child_and_releases_the_lock() {
    let Some(fleet) = fleet("lock_term") else {
        return;
    };
    let dir = &fleet.dir;
    let child = lock(dir, MANIFEST, &format!("{SLEEPER}; wait"));
    wait_for(&dir.join("ops/sleeper"));
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
    let (_, stderr, code) = child.wait();
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
    // Neither variable, only the token, only the fence.
    let only: [&[(&str, &str)]; 3] = [
        &[],
        &[("MEADA_REFRESH_LOCK", TOKEN)],
        &[("MEADA_REFRESH_FENCE", "7")],
    ];
    for vars in only {
        let mut publish = meada(dir, &["snapshot", "publish", "--workspace", MANIFEST]);
        let (stdout, stderr, code) = text(&publish.envs(vars.iter().copied()).output().unwrap());
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{vars:?}: {stderr}");
        assert!(
            stderr.contains("needs MEADA_REFRESH_LOCK and MEADA_REFRESH_FENCE"),
            "{vars:?}: {stderr}"
        );
    }
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

    // A stale fence: refused, naming the fence key and the recovery, in either language.
    let (_, stderr, code) = publish_as(dir, TOKEN, 5);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("refused fence 5"), "{stderr}");
    let recover = format!(
        "{fence_key} was lowered or lost; raise it with meada snapshot fence recover \
         (--at-least for a fence read in a node's <local_dir>/current), or delete"
    );
    assert!(stderr.contains(&recover), "{stderr}");
    assert!(!stderr.contains("set {"), "{stderr}");
    let (_, stderr, code) = text(&publish_in(dir, "pt_BR.UTF-8", TOKEN, 5).output().unwrap());
    assert_eq!(code, Some(1), "{stderr}");
    let recover = format!(
        "{fence_key} foi rebaixado ou perdido; eleve-o com meada snapshot fence recover \
         (--at-least para um fence lido no <local_dir>/current de um nó), ou apague"
    );
    assert!(stderr.contains(&recover), "{stderr}");
    assert_eq!(current(&fleet), (g1.clone(), "5".to_string()));

    let (g2, stderr, code) = publish_as(dir, TOKEN, 6);
    assert_eq!(code, Some(0), "{stderr}");
    assert_ne!(g2.trim(), g1);
    assert_eq!(current(&fleet), (g2.trim().to_string(), "6".to_string()));
    send(&mut valkey, &["DEL", &lock_key]);
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
    let (g2, stderr, code) = lock(dir, MANIFEST, script).wait();
    assert_eq!(code, Some(0), "{stderr}");
    let g2 = g2.trim().to_string();
    assert_eq!(current(&fleet), (g2.clone(), "1".to_string()));

    let (stdout, stderr, code) = pull(dir, &node, &[]);
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(
        stderr.contains(&format!("generation {g2} has fence 1, below fence 5")),
        "{stderr}"
    );
    // As the node's manifest names it: relative to the directory `status` runs in.
    let shown = "node/generations/current";
    let below = format!(
        "not swapped; raise {fence_key} with meada snapshot fence recover --at-least 5 and \
         publish again, or delete {shown}"
    );
    assert!(stderr.contains(&below), "{stderr}");
    let pt = |args: &[&str]| {
        text(
            &meada(dir, args)
                .env("LANG", "pt_BR.UTF-8")
                .output()
                .unwrap(),
        )
    };
    let (_, stderr, code) = pt(&["snapshot", "pull", "--workspace", &node]);
    assert_eq!(code, Some(1), "{stderr}");
    let below = format!(
        "não trocada; eleve {fence_key} com meada snapshot fence recover --at-least 5 e \
         publique de novo, ou apague {shown}"
    );
    assert!(stderr.contains(&below), "{stderr}");
    assert_eq!(
        fs::read_to_string(&pointer_file).unwrap(),
        format!("{g1} 5\n")
    );

    let (stdout, stderr, code) = probe(dir, &node);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
    let stuck = format!(
        "fence: stuck: 5 in {shown} is above 1 in {current_key}; raise {fence_key} with meada \
         snapshot fence recover --at-least 5 and publish again, or delete {shown}\n"
    );
    assert!(stdout.ends_with(&stuck), "{stdout}");
    let (stdout, stderr, code) = pt(&["status", "--workspace", &node, "--generations"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
    let stuck = format!(
        "fence: travado: 5 em {shown} está acima de 1 em {current_key}; eleve {fence_key} com \
         meada snapshot fence recover --at-least 5 e publique de novo, ou apague {shown}\n"
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

/// `meada <args>` in `dir` under `lang`: stdout, stderr and the exit code.
pub(crate) fn run_in(dir: &Path, lang: &str, args: &[&str]) -> (String, String, Option<i32>) {
    text(
        &meada(dir, args)
            .env("LANG", lang)
            .output()
            .expect("meada runs"),
    )
}

/// `meada snapshot lock -- meada snapshot publish`, run to its end: the generation id.
pub(crate) fn locked_publish(dir: &Path) -> String {
    let script = "exec \"$MEADA\" snapshot publish --workspace ops/meada.workspace.toml";
    let (stdout, stderr, code) = lock(dir, MANIFEST, script).wait();
    assert_eq!(code, Some(0), "{stderr}");
    stdout.trim().to_string()
}

/// `{<prefix>}:<name>` of `fleet`.
pub(crate) fn key(fleet: &Fleet, name: &str) -> String {
    format!("{{{}}}:{name}", prefix(fleet.case))
}

/// `GET {<prefix>}:<name>`.
pub(crate) fn get(fleet: &Fleet, name: &str) -> Option<String> {
    let get = redis::cmd("GET").arg(key(fleet, name)).clone();
    get.query(&mut admin(&fleet.valkey)).unwrap()
}

/// The holder's fields: host, pid, started and holds.
fn holder(fleet: &Fleet) -> Vec<Option<String>> {
    let fields = ["host", "pid", "started", "holds"];
    let hmget = redis::cmd("HMGET")
        .arg(key(fleet, "holder"))
        .arg(&fields)
        .clone();
    hmget.query(&mut admin(&fleet.valkey)).unwrap()
}

/// The refusal of a lock that `meada snapshot lock` (process `pid`, running `sh`) holds, after
/// checking its holder: this host in lowercase, the pid, a start time in RFC 3339 and `lock sh`.
pub(crate) fn held_by(fleet: &Fleet, pid: u32) -> String {
    let fields: Vec<String> = holder(fleet).into_iter().map(Option::unwrap).collect();
    let [host, shown_pid, started, holds] = &fields[..] else {
        panic!("{fields:?}");
    };
    let this = nix::unistd::gethostname().unwrap();
    assert_eq!(*host, this.to_str().unwrap().to_lowercase());
    let pid = pid.to_string();
    assert_eq!((shown_pid, holds.as_str()), (&pid, "lock sh"));
    let b = started.as_bytes();
    let shape = (b.len(), b[4], b[7], b[10], b[13], b[16], b[19]);
    assert_eq!(shape, (20, b'-', b'-', b'T', b':', b':', b'Z'), "{started}");
    format!(
        "meada: {}: held by host {host}, process {pid}, since {started}, for lock sh: nothing \
         run\n",
        key(fleet, "lock")
    )
}

/// The members of `{<prefix>}:kept`, sorted.
pub(crate) fn kept(fleet: &Fleet) -> Vec<String> {
    let members = redis::cmd("SMEMBERS").arg(key(fleet, "kept")).clone();
    let mut kept: Vec<String> = members.query(&mut admin(&fleet.valkey)).unwrap();
    kept.sort();
    kept
}

/// Whether neither the lock nor its holder is left.
pub(crate) fn released(fleet: &Fleet) -> bool {
    let exists = redis::cmd("EXISTS")
        .arg(key(fleet, "lock"))
        .arg(key(fleet, "holder"))
        .clone();
    exists.query::<i64>(&mut admin(&fleet.valkey)).unwrap() == 0
}

/// While `meada snapshot lock` runs a sleeping child, `pointer set`, `retain` and `drop` and
/// `fence recover` each exit 1 naming the holder (host, process, start time, `lock sh`), as a
/// second `lock` does, and leave the pointer, the retained list, the kept set and the fence as
/// they were.
#[test]
fn every_operation_is_refused_while_a_lock_holds_naming_the_holder() {
    let Some(fleet) = fleet("ops_held") else {
        return;
    };
    let dir = &fleet.dir;
    let g1 = locked_publish(dir);
    let child = lock(dir, MANIFEST, "touch ops/started; exec sleep 60");
    wait_for(&dir.join("ops/started"));
    let state = |fleet: &Fleet| {
        let pointer = pointer(&mut admin(&fleet.valkey), fleet.case);
        (pointer, kept(fleet), get(fleet, "fence"))
    };
    let before = state(&fleet);
    let held = held_by(&fleet, child.id());
    let set: &[&str] = &["snapshot", "pointer", "set", &g1, "--workspace", MANIFEST];
    let retain: &[&str] = &[
        "snapshot",
        "pointer",
        "retain",
        &g1,
        "--workspace",
        MANIFEST,
    ];
    let drop: &[&str] = &["snapshot", "pointer", "drop", &g1, "--workspace", MANIFEST];
    let recover: &[&str] = &["snapshot", "fence", "recover", "--workspace", MANIFEST];
    for args in [set, retain, drop, recover] {
        let (stdout, stderr, code) = run_in(dir, "en_US.UTF-8", args);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{args:?}: {stderr}");
        assert_eq!(stderr, held, "{args:?}");
    }
    let (_, stderr, code) = lock(dir, MANIFEST, "touch ops/ran").wait();
    assert_eq!((code, stderr), (Some(1), held));
    assert!(!dir.join("ops/ran").exists());
    assert_eq!(state(&fleet), before);

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
    child.wait();
    assert!(released(&fleet));
}

/// The holder lives with the lock: its four fields and a TTL no longer than the lock's while
/// `lock` runs, renewed past one TTL, gone once the child exits. An operation leaves neither
/// the lock nor the holder, after success and after failure.
#[test]
fn the_holder_lives_with_the_lock_and_an_operation_leaves_none() {
    let Some(fleet) = fleet("ops_holder") else {
        return;
    };
    let dir = &fleet.dir;
    let child = lock(dir, MANIFEST, "touch ops/started; sleep 13");
    wait_for(&dir.join("ops/started"));
    let pttl = |name: &str| -> i64 {
        let pttl = redis::cmd("PTTL").arg(key(&fleet, name)).clone();
        pttl.query(&mut admin(&fleet.valkey)).unwrap()
    };
    let (lock_ttl, holder_ttl) = (pttl("lock"), pttl("holder"));
    assert!(
        0 < holder_ttl && holder_ttl <= lock_ttl && lock_ttl <= 10_000,
        "{holder_ttl} {lock_ttl}"
    );
    held_by(&fleet, child.id());
    std::thread::sleep(Duration::from_secs(11));
    held_by(&fleet, child.id());
    assert!(pttl("holder") > 0);
    let (_, stderr, code) = child.wait();
    assert_eq!(code, Some(0), "{stderr}");
    assert!(released(&fleet));

    let g1 = locked_publish(dir);
    let set = ["snapshot", "pointer", "set", &g1, "--workspace", MANIFEST];
    let not_retained = [
        "snapshot",
        "pointer",
        "set",
        "0000000000000000",
        "--workspace",
        MANIFEST,
    ];
    let recover = ["snapshot", "fence", "recover", "--workspace", MANIFEST];
    let retain = [
        "snapshot",
        "pointer",
        "retain",
        &g1,
        "--workspace",
        MANIFEST,
    ];
    let drop = ["snapshot", "pointer", "drop", &g1, "--workspace", MANIFEST];
    let cases: [(&[&str], Option<i32>); 5] = [
        (&set, Some(0)),
        (&not_retained, Some(1)),
        (&recover, Some(0)),
        (&retain, Some(0)),
        (&drop, Some(1)),
    ];
    for (args, expected) in cases {
        let (_, stderr, code) = run_in(dir, "en_US.UTF-8", args);
        assert_eq!(code, expected, "{args:?}: {stderr}");
        assert!(released(&fleet), "{args:?}");
    }
}

/// A node took fence 5; with `{<prefix>}:fence` deleted, a publish under the lock is refused by
/// the fence; `fence recover` raises the fence to 5, and the next publish succeeds and the node
/// swaps.
#[test]
fn a_stuck_fleet_moves_again_after_fence_recover() {
    let Some(fleet) = fleet("ops_stuck") else {
        return;
    };
    let dir = &fleet.dir;
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["SET", &key(&fleet, "lock"), TOKEN]);
    let (g1, stderr, code) = publish_as(dir, TOKEN, 5);
    assert_eq!(code, Some(0), "{stderr}");
    let g1 = g1.trim().to_string();
    send(&mut valkey, &["DEL", &key(&fleet, "lock")]);
    let (node, _) = fleet.node("node");
    assert_eq!(fleet.pull(&node), g1);

    send(&mut valkey, &["DEL", &key(&fleet, "fence")]);
    next_commit(dir, "chain-web", "after_flush");
    let script = "exec \"$MEADA\" snapshot publish --workspace ops/meada.workspace.toml";
    let (stdout, stderr, code) = lock(dir, MANIFEST, script).wait();
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(stderr.contains("refused fence 1"), "{stderr}");
    assert!(stderr.contains("meada snapshot fence recover"), "{stderr}");
    assert_eq!(current(&fleet), (g1.clone(), "5".to_string()));

    let recover = ["snapshot", "fence", "recover", "--workspace", MANIFEST];
    let (stdout, stderr, code) = run_in(dir, "en_US.UTF-8", &recover);
    assert_eq!(
        (code, stdout.as_str(), stderr.as_str()),
        (Some(0), "5\n", "")
    );
    assert_eq!(get(&fleet, "fence").as_deref(), Some("5"));

    let g2 = locked_publish(dir);
    assert_eq!(current(&fleet), (g2.clone(), "6".to_string()));
    assert_eq!(fleet.pull(&node), g2);
}

/// `fence recover` takes the highest fence recorded in the pointer and the retained list, or
/// `--at-least` when higher, and never lowers the fence; a malformed `--at-least` is refused
/// before the lock is taken.
#[test]
fn fence_recover_takes_the_highest_recorded_fence_or_the_floor_never_lowering() {
    let Some(fleet) = fleet("ops_floor") else {
        return;
    };
    let dir = &fleet.dir;
    let mut valkey = admin(&fleet.valkey);
    let fence_key = key(&fleet, "fence");
    // G1 at 8 then G2 at 9 under a lock set by hand; the pointer then lowered to 5 by hand, so
    // the highest fence recorded is the list's 9 and not the pointer's.
    send(&mut valkey, &["SET", &key(&fleet, "lock"), TOKEN]);
    assert_eq!(publish_as(dir, TOKEN, 8).2, Some(0));
    next_commit(dir, "chain-web", "second");
    let (g2, stderr, code) = publish_as(dir, TOKEN, 9);
    assert_eq!(code, Some(0), "{stderr}");
    send(&mut valkey, &["DEL", &key(&fleet, "lock")]);
    send(
        &mut valkey,
        &["HSET", &key(&fleet, "current"), "fence", "5"],
    );
    assert_eq!(current(&fleet), (g2.trim().to_string(), "5".to_string()));

    let recover = |floor: Option<&str>| {
        let floor = floor.map(|f| format!("--at-least={f}"));
        let mut args = vec!["snapshot", "fence", "recover", "--workspace", MANIFEST];
        args.extend(floor.as_deref());
        run_in(dir, "en_US.UTF-8", &args)
    };
    let ok = |fence: &str| (format!("{fence}\n"), String::new(), Some(0));
    send(&mut valkey, &["SET", &fence_key, "1"]);
    assert_eq!(recover(Some("3")), ok("9"));
    assert_eq!(recover(Some("20")), ok("20"));
    // Above everything recorded: kept, past the operation's own acquisition (`INCR`).
    assert_eq!(recover(None), ok("21"));
    send(&mut valkey, &["SET", &fence_key, "100"]);
    assert_eq!(recover(Some("50")), ok("101"));
    assert_eq!(get(&fleet, "fence").as_deref(), Some("101"));

    for bad in ["x", "-1", "+5", "1e3", "", " 5", "18446744073709551616"] {
        let (stdout, stderr, code) = recover(Some(bad));
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{bad}: {stderr}");
        let refused = format!("meada: --at-least {bad}: not a decimal fence\n");
        assert_eq!(stderr, refused);
        assert_eq!(get(&fleet, "fence").as_deref(), Some("101"), "{bad}");
        assert!(released(&fleet), "{bad}");
    }
}

/// `fence recover` counts the fence of a live heartbeat written by hand (the spec's format),
/// ignores a name whose score or key expired, and names a malformed heartbeat, node name or
/// retained entry on stderr without counting it.
#[test]
fn fence_recover_counts_live_heartbeats_and_names_malformed_ones() {
    let Some(fleet) = fleet("ops_beats") else {
        return;
    };
    let dir = &fleet.dir;
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["SET", &key(&fleet, "lock"), TOKEN]);
    let (g1, stderr, code) = publish_as(dir, TOKEN, 2);
    assert_eq!(code, Some(0), "{stderr}");
    let g1 = g1.trim();
    send(&mut valkey, &["DEL", &key(&fleet, "lock")]);
    let retained = key(&fleet, "retained");
    send(&mut valkey, &["RPUSH", &retained, "../../..:99"]);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (live, past) = ((now + 60).to_string(), (now - 10).to_string());
    let beat = |fence: &str| {
        format!(
            "{{\"name\":\"n\",\"generation\":\"{g1}\",\"fence\":{fence},\"held\":[\"{g1}\"],\
             \"ready\":true,\"version\":\"0.1.0\",\"time\":\"2026-10-22T00:00:00Z\"}}"
        )
    };
    let nodes = key(&fleet, "nodes");
    // The name, its score in the node set, and its heartbeat (none: its key expired).
    let cases = [
        ("n1", &live, Some(beat("9"))),
        ("n2", &past, Some(beat("50"))),
        ("n3", &live, None),
        ("n4", &live, Some(beat("\"40\""))),
        ("n5", &live, Some(beat("null"))),
        ("Bad_Name", &live, Some(beat("60"))),
    ];
    for (name, score, heartbeat) in &cases {
        send(&mut valkey, &["ZADD", &nodes, score, name]);
        if let Some(heartbeat) = heartbeat {
            let node = key(&fleet, &format!("node:{name}"));
            send(&mut valkey, &["SET", &node, heartbeat, "EX", "60"]);
        }
    }
    let recover = ["snapshot", "fence", "recover", "--workspace", MANIFEST];
    let (stdout, stderr, code) = run_in(dir, "en_US.UTF-8", &recover);
    assert_eq!((code, stdout.as_str()), (Some(0), "9\n"), "{stderr}");
    let n4 = key(&fleet, "node:n4");
    assert_eq!(
        stderr,
        format!(
            "meada: {retained}: an entry is not `<generation id>:<fence>`, an id of 16 lowercase \
             hex characters and a decimal fence: not counted\n\
             meada: {nodes}: \"Bad_Name\" is not a node name (lowercase letters, digits, . and -, \
             1 to 63 characters): not counted\n\
             meada: {n4}: not a heartbeat, a JSON object whose fence is a decimal or null: not \
             counted\n"
        )
    );
    assert_eq!(get(&fleet, "fence").as_deref(), Some("9"));
}

/// `pointer retain` of a generation already retained only keeps it; of one outside the list it
/// appends `<id>:<fence>` at the oldest end with the operation's fence, which `fence recover`
/// counts; of one the store does not hold, or past the kept maximum, it is refused with nothing
/// written. `pointer drop` refuses the current generation and one outside the list, naming the
/// key, and drops a kept one from both keys.
#[test]
fn retain_and_drop_change_only_the_retained_list_and_the_kept_set() {
    let Some(fleet) = fleet("ops_kept") else {
        return;
    };
    let dir = &fleet.dir;
    let mut valkey = admin(&fleet.valkey);
    let (retained, kept_key) = (key(&fleet, "retained"), key(&fleet, "kept"));
    let op = |op: &str, id: &str| {
        let args = ["snapshot", "pointer", op, id, "--workspace", MANIFEST];
        run_in(dir, "en_US.UTF-8", &args)
    };
    let printed = |id: &str| (format!("{id}\n"), String::new(), Some(0));
    let refused = |stderr: String| (String::new(), stderr, Some(1));
    let state = |fleet: &Fleet| {
        let pointer = pointer(&mut admin(&fleet.valkey), fleet.case);
        (pointer, kept(fleet), get(fleet, "fence"))
    };
    let g1 = locked_publish(dir);
    next_commit(dir, "chain-web", "second");
    let g2 = locked_publish(dir);

    // A generation the store does not hold: refused naming its document, before the lock.
    let zero = "0000000000000000";
    let before = state(&fleet);
    let (stdout, stderr, code) = op("retain", zero);
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    let document = format!("generations/{zero}.json: ");
    assert!(
        stderr.starts_with("meada: ") && stderr.contains(&document),
        "{stderr}"
    );
    assert_eq!(state(&fleet), before);

    // Already retained: only kept, and again changes nothing.
    assert_eq!(op("retain", &g2), printed(&g2));
    assert_eq!(op("retain", &g2), printed(&g2));
    assert_eq!(kept(&fleet), std::slice::from_ref(&g2));
    let list = vec![format!("{g2}:2"), format!("{g1}:1")];
    assert_eq!(pointer(&mut valkey, fleet.case).1, list);

    // Outside the list: appended at its oldest end with the operation's fence (5: the two
    // publishes, the two retains, then this one).
    send(&mut valkey, &["LREM", &retained, "0", &format!("{g1}:1")]);
    assert_eq!(op("retain", &g1), printed(&g1));
    let list = vec![format!("{g2}:2"), format!("{g1}:5")];
    assert_eq!(pointer(&mut valkey, fleet.case).1, list);
    assert_eq!(kept(&fleet), sorted(&[&g1, &g2]));
    // `fence recover` counts that entry's fence: above the pointer's and the fence key's.
    send(&mut valkey, &["SET", &key(&fleet, "fence"), "1"]);
    let recover = ["snapshot", "fence", "recover", "--workspace", MANIFEST];
    let recovered = run_in(dir, "en_US.UTF-8", &recover);
    assert_eq!(recovered, ("5\n".to_string(), String::new(), Some(0)));
    assert!(released(&fleet));

    // Past the kept maximum (8, here G2 and seven others): refused naming it, nothing written.
    let filler: Vec<String> = (0..7).map(|n| format!("{n:016x}")).collect();
    let filler = || filler.iter().map(String::as_str);
    send(&mut valkey, &["SREM", &kept_key, &g1]);
    let sadd: Vec<&str> = ["SADD", kept_key.as_str()]
        .into_iter()
        .chain(filler())
        .collect();
    send(&mut valkey, &sadd);
    let before = state(&fleet);
    let full = format!(
        "meada: {kept_key}: already keeps 8 generations, the most it may: drop one first \
         (meada snapshot pointer drop)\n"
    );
    assert_eq!(op("retain", &g1), refused(full));
    assert_eq!(state(&fleet).0, before.0);
    assert_eq!(state(&fleet).1, before.1);
    assert!(released(&fleet));
    let srem: Vec<&str> = ["SREM", kept_key.as_str()]
        .into_iter()
        .chain(filler())
        .collect();
    send(&mut valkey, &srem);
    send(&mut valkey, &["SADD", &kept_key, &g1]);

    // `drop`: the current generation and one outside the list are refused naming the key.
    let current = key(&fleet, "current");
    let before = state(&fleet);
    let is_current =
        format!("meada: {current}: generation {g2} is the current one: nothing dropped\n");
    assert_eq!(op("drop", &g2), refused(is_current));
    let outside =
        format!("meada: {retained}: generation {zero} is not retained: nothing dropped\n");
    assert_eq!(op("drop", zero), refused(outside));
    assert_eq!(state(&fleet).0, before.0);
    assert_eq!(state(&fleet).1, before.1);
    // A kept one leaves both keys.
    assert_eq!(op("drop", &g1), printed(&g1));
    assert_eq!(pointer(&mut valkey, fleet.case).1, [format!("{g2}:2")]);
    assert_eq!(kept(&fleet), std::slice::from_ref(&g2));
    assert!(released(&fleet));
}

// `meada workspace apply` (#302): the workspace pointer moved under the refresh lock.

/// `meada workspace apply <id>` on the fleet's manifest: stdout, stderr and the exit code.
fn apply(dir: &Path, id: &str) -> (String, String, Option<i32>) {
    let args = ["workspace", "apply", id, "--workspace", MANIFEST];
    run_in(dir, "en_US.UTF-8", &args)
}

/// `{<prefix>}:workspace`'s id and fence.
fn workspace_hash(fleet: &Fleet) -> Vec<Option<String>> {
    let hmget = redis::cmd("HMGET")
        .arg(crate::generations::workspace_pointer(fleet.case))
        .arg("id")
        .arg("fence")
        .clone();
    hmget.query(&mut admin(&fleet.valkey)).unwrap()
}

/// `apply` after `import` moves `{<prefix>}:workspace` to the version with a fence above the
/// previous one and prints `<id> <fence>`; applying the older version again rolls it back with
/// a new fence. Each leaves neither the lock nor the holder.
#[test]
fn apply_moves_the_workspace_pointer_and_rolls_back_with_a_new_fence() {
    let Some(fleet) = fleet("ws_apply") else {
        return;
    };
    let dir = &fleet.dir;
    let (v1, _) = crate::generations::import(dir, MANIFEST, "one", "alice");
    let fewer = "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n";
    fs::write(dir.join("ops/fewer.toml"), fewer).unwrap();
    let (v2, _) = crate::generations::import(dir, "ops/fewer.toml", "two", "bob");
    let mut last = 0u64;
    for id in [&v1, &v2, &v1] {
        let (stdout, stderr, code) = apply(dir, id);
        assert_eq!((code, stderr.as_str()), (Some(0), ""), "{id}");
        let (shown, fence) = stdout.trim().split_once(' ').unwrap();
        let fence: u64 = fence.parse().unwrap();
        assert_eq!(shown, id.as_str());
        assert!(fence > last, "{fence} {last}");
        let expected = vec![Some(id.clone()), Some(fence.to_string())];
        assert_eq!(workspace_hash(&fleet), expected);
        assert!(released(&fleet));
        last = fence;
    }
}

/// `apply` is refused, leaving the pointer as it was: for a version the store does not hold,
/// naming its key; while a job holds the lock, naming the holder; and with
/// `{<prefix>}:fence` lowered, by the fence. `fence recover` counts the workspace pointer's
/// fence, so the next `apply` succeeds.
#[test]
fn apply_is_refused_for_an_unknown_version_a_held_lock_and_a_lowered_fence() {
    let Some(fleet) = fleet("ws_apply_refused") else {
        return;
    };
    let dir = &fleet.dir;
    let (v1, _) = crate::generations::import(dir, MANIFEST, "one", "alice");
    for _ in 0..3 {
        assert_eq!(apply(dir, &v1).2, Some(0));
    }
    let applied = vec![Some(v1.clone()), Some("3".to_string())];
    assert_eq!(workspace_hash(&fleet), applied);

    let zero = "0000000000000000";
    let (stdout, stderr, code) = apply(dir, zero);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    let version = crate::generations::version_key(zero);
    let missing = format!("meada: {version}: no such workspace version in the store\n");
    assert_eq!(stderr, missing);
    assert_eq!(workspace_hash(&fleet), applied);

    let child = lock(dir, MANIFEST, "touch ops/started; exec sleep 60");
    wait_for(&dir.join("ops/started"));
    let held = held_by(&fleet, child.id());
    let (stdout, stderr, code) = apply(dir, &v1);
    assert_eq!((stdout.as_str(), code, stderr), ("", Some(1), held));
    assert_eq!(workspace_hash(&fleet), applied);
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
    child.wait();
    assert!(released(&fleet));

    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["DEL", &key(&fleet, "fence")]);
    let (stdout, stderr, code) = apply(dir, &v1);
    assert_eq!((stdout.as_str(), code), ("", Some(1)), "{stderr}");
    let pointer = crate::generations::workspace_pointer(fleet.case);
    assert!(
        stderr.starts_with(&format!("meada: {pointer}: refused fence 1, not above")),
        "{stderr}"
    );
    assert!(stderr.contains("meada snapshot fence recover"), "{stderr}");
    assert_eq!(workspace_hash(&fleet), applied);
    assert!(released(&fleet));

    let recover = ["snapshot", "fence", "recover", "--workspace", MANIFEST];
    let (stdout, stderr, code) = run_in(dir, "en_US.UTF-8", &recover);
    assert_eq!(
        (code, stdout.as_str(), stderr.as_str()),
        (Some(0), "3\n", "")
    );
    let (stdout, stderr, code) = apply(dir, &v1);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout, format!("{v1} 4\n"));
}

/// A malformed version id is refused before any connection: the store and Valkey the manifest
/// names are a port this test holds and never sees a connection on.
#[test]
fn apply_refuses_a_malformed_id_before_any_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let dir = chain_manifest("ws_apply_malformed");
    fs::write(dir.join("ops/s3-secret"), "x\n").unwrap();
    let sections = format!(
        "{}\n[valkey]\nurl = \"redis://127.0.0.1:{port}\"\n\n[generations]\n\n[refresh_lock]\n",
        crate::snapshot::section(&format!("http://127.0.0.1:{port}"), "offline")
    );
    append_section(&dir, &sections);
    let (stdout, stderr, code) = apply(&dir, "../x");
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        "meada: ../x: not a workspace version id of 16 lowercase hex characters\n"
    );
    listener.set_nonblocking(true).unwrap();
    let accepted = listener.accept().map(drop).map_err(|e| e.kind());
    assert_eq!(accepted, Err(std::io::ErrorKind::WouldBlock));
}
