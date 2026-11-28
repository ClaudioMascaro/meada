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

pub(crate) fn fleet(case: &'static str) -> Option<Fleet> {
    let fleet = Fleet::new(case, 3)?;
    append_section(&fleet.dir, "\n[refresh_lock]\nttl_seconds = 10\n");
    Some(fleet)
}

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

pub(crate) struct Running(Option<Child>);

impl Running {
    pub(crate) fn id(&self) -> u32 {
        self.0.as_ref().expect("running").id()
    }

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

pub(crate) fn lock(dir: &Path, manifest: &str, script: &str) -> Running {
    let child = meada(dir, &["snapshot", "lock", "--workspace", manifest, "--"])
        .args(["sh", "-c", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meada runs");
    Running(Some(child))
}

const SLEEPER: &str = "sleep 60 & echo $! > ops/sleeper.new && mv ops/sleeper.new ops/sleeper";

fn publish_as(dir: &Path, token: &str, fence: u64) -> (String, String, Option<i32>) {
    text(
        &publish_in(dir, "en_US.UTF-8", token, fence)
            .output()
            .expect("meada runs"),
    )
}

fn publish_in(dir: &Path, lang: &str, token: &str, fence: u64) -> Command {
    let mut publish = meada(dir, &["snapshot", "publish", "--workspace", MANIFEST]);
    publish.env("LANG", lang).env("MEADA_REFRESH_LOCK", token);
    publish.env("MEADA_REFRESH_FENCE", fence.to_string());
    publish
}

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

pub(crate) fn current(fleet: &Fleet) -> (String, String) {
    let (hash, _) = pointer(&mut admin(&fleet.valkey), fleet.case);
    (hash[0].clone(), hash[1].clone())
}

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
    let env = fs::read_to_string(dir.join("ops/env")).unwrap();
    let (token, fence) = env.trim().split_once(' ').unwrap();
    let (stdout, stderr, code) = publish_as(dir, token, fence.parse().unwrap());
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    let not_held = format!("meada: {lock_key}: the refresh lock no longer holds this job's token");
    assert!(stderr.starts_with(&not_held), "{stderr}");
    assert_eq!(current(&fleet), (g1.trim().to_string(), "1".to_string()));
}

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

#[test]
fn a_publish_with_a_stale_fence_is_refused_while_a_newer_one_succeeds() {
    let Some(fleet) = fleet("lock_fence") else {
        return;
    };
    let dir = &fleet.dir;
    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
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

    next_commit(dir, "chain-web", "second");
    let before = objects(&s3, "prod/meada").len();
    let (_, stderr, code) = publish_as(dir, "ffffffffffffffffffffffffffffffff", 9);
    assert_eq!(code, Some(1), "{stderr}");
    let not_held = format!("meada: {lock_key}: the refresh lock no longer holds this job's token");
    assert!(stderr.starts_with(&not_held), "{stderr}");
    assert_eq!(objects(&s3, "prod/meada").len(), before);
    assert_eq!(current(&fleet), (g1.clone(), "5".to_string()));

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

pub(crate) fn run_in(dir: &Path, lang: &str, args: &[&str]) -> (String, String, Option<i32>) {
    text(
        &meada(dir, args)
            .env("LANG", lang)
            .output()
            .expect("meada runs"),
    )
}

pub(crate) fn locked_publish(dir: &Path) -> String {
    let script = "exec \"$MEADA\" snapshot publish --workspace ops/meada.workspace.toml";
    let (stdout, stderr, code) = lock(dir, MANIFEST, script).wait();
    assert_eq!(code, Some(0), "{stderr}");
    stdout.trim().to_string()
}

pub(crate) fn key(fleet: &Fleet, name: &str) -> String {
    format!("{{{}}}:{name}", prefix(fleet.case))
}

pub(crate) fn get(fleet: &Fleet, name: &str) -> Option<String> {
    let get = redis::cmd("GET").arg(key(fleet, name)).clone();
    get.query(&mut admin(&fleet.valkey)).unwrap()
}

fn holder(fleet: &Fleet) -> Vec<Option<String>> {
    let fields = ["host", "pid", "started", "holds"];
    let hmget = redis::cmd("HMGET")
        .arg(key(fleet, "holder"))
        .arg(&fields)
        .clone();
    hmget.query(&mut admin(&fleet.valkey)).unwrap()
}

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

pub(crate) fn kept(fleet: &Fleet) -> Vec<String> {
    let members = redis::cmd("SMEMBERS").arg(key(fleet, "kept")).clone();
    let mut kept: Vec<String> = members.query(&mut admin(&fleet.valkey)).unwrap();
    kept.sort();
    kept
}

pub(crate) fn released(fleet: &Fleet) -> bool {
    let exists = redis::cmd("EXISTS")
        .arg(key(fleet, "lock"))
        .arg(key(fleet, "holder"))
        .clone();
    exists.query::<i64>(&mut admin(&fleet.valkey)).unwrap() == 0
}

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

const HOLDER_SKEW_MS: i64 = 5;

#[test]
fn the_holder_lives_with_the_lock_and_an_operation_leaves_none() {
    let Some(fleet) = fleet("ops_holder") else {
        return;
    };
    let dir = &fleet.dir;
    let child = lock(dir, MANIFEST, "touch ops/started; sleep 13");
    wait_for(&dir.join("ops/started"));
    let ttls = || -> (i64, i64) {
        redis::pipe()
            .atomic()
            .cmd("PTTL")
            .arg(key(&fleet, "lock"))
            .cmd("PTTL")
            .arg(key(&fleet, "holder"))
            .query(&mut admin(&fleet.valkey))
            .unwrap()
    };
    let (lock_ttl, holder_ttl) = ttls();
    assert!(
        0 < holder_ttl && holder_ttl <= lock_ttl + HOLDER_SKEW_MS && lock_ttl <= 10_000,
        "{holder_ttl} {lock_ttl}"
    );
    held_by(&fleet, child.id());
    std::thread::sleep(Duration::from_secs(11));
    held_by(&fleet, child.id());
    assert!(ttls().1 > 0);
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

#[test]
fn fence_recover_takes_the_highest_recorded_fence_or_the_floor_never_lowering() {
    let Some(fleet) = fleet("ops_floor") else {
        return;
    };
    let dir = &fleet.dir;
    let mut valkey = admin(&fleet.valkey);
    let fence_key = key(&fleet, "fence");
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
    let beat = |name: &str, fence: &str| {
        let generation = if fence == "null" {
            "null".to_string()
        } else {
            format!("\"{g1}\"")
        };
        format!(
            "{{\"name\":\"{name}\",\"generation\":{generation},\"fence\":{fence},\"held\":[\"{g1}\"],\
             \"ready\":true,\"version\":\"0.1.0\",\"time\":\"2026-10-22T00:00:00Z\"}}"
        )
    };
    let nodes = key(&fleet, "nodes");
    let cases = [
        ("n1", &live, Some(beat("n1", "9"))),
        ("n2", &past, Some(beat("n2", "50"))),
        ("n3", &live, None),
        ("n4", &live, Some(beat("n4", "\"40\""))),
        ("n5", &live, Some(beat("n5", "null"))),
        ("Bad_Name", &live, Some(beat("Bad_Name", "60"))),
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

    assert_eq!(op("retain", &g2), printed(&g2));
    assert_eq!(op("retain", &g2), printed(&g2));
    assert_eq!(kept(&fleet), std::slice::from_ref(&g2));
    let list = vec![format!("{g2}:2"), format!("{g1}:1")];
    assert_eq!(pointer(&mut valkey, fleet.case).1, list);

    send(&mut valkey, &["LREM", &retained, "0", &format!("{g1}:1")]);
    assert_eq!(op("retain", &g1), printed(&g1));
    let list = vec![format!("{g2}:2"), format!("{g1}:5")];
    assert_eq!(pointer(&mut valkey, fleet.case).1, list);
    assert_eq!(kept(&fleet), sorted(&[&g1, &g2]));
    send(&mut valkey, &["SET", &key(&fleet, "fence"), "1"]);
    let recover = ["snapshot", "fence", "recover", "--workspace", MANIFEST];
    let recovered = run_in(dir, "en_US.UTF-8", &recover);
    assert_eq!(recovered, ("5\n".to_string(), String::new(), Some(0)));
    assert!(released(&fleet));

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
    assert_eq!(op("drop", &g1), printed(&g1));
    assert_eq!(pointer(&mut valkey, fleet.case).1, [format!("{g2}:2")]);
    assert_eq!(kept(&fleet), std::slice::from_ref(&g2));
    assert!(released(&fleet));
}

fn apply(dir: &Path, id: &str) -> (String, String, Option<i32>) {
    let args = ["workspace", "apply", id, "--workspace", MANIFEST];
    run_in(dir, "en_US.UTF-8", &args)
}

fn workspace_hash(fleet: &Fleet) -> Vec<Option<String>> {
    let hmget = redis::cmd("HMGET")
        .arg(crate::generations::workspace_pointer(fleet.case))
        .arg("id")
        .arg("fence")
        .clone();
    hmget.query(&mut admin(&fleet.valkey)).unwrap()
}

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

#[test]
fn apply_moves_the_workspace_pointer_for_versions_that_set_change_and_clear_owners() {
    let Some(fleet) = fleet("ws_apply_owner") else {
        return;
    };
    let dir = &fleet.dir;
    let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let set = crate::generations::owned(&text, &[Some("team-a"), Some("team-b"), None, None]);
    let changed = crate::generations::owned(&text, &[Some("Team-A"), None, None, None]);
    let changed = changed.replacen("../chain-worker\"", "../chain-worker-2\"", 1);
    let mut ids = Vec::new();
    for (name, version) in [("set", set), ("changed", changed)] {
        let file = format!("ops/{name}.toml");
        fs::write(dir.join(&file), version).unwrap();
        ids.push(crate::generations::import(dir, &file, name, "alice").0);
    }
    let (plain, _) = crate::generations::import(dir, MANIFEST, "plain", "alice");
    let mut last = 0u64;
    for id in [&ids[0], &ids[1], &plain, &ids[0]] {
        let (stdout, stderr, code) = apply(dir, id);
        assert_eq!((code, stderr.as_str()), (Some(0), ""), "{id}");
        let (shown, fence) = stdout.trim().split_once(' ').unwrap();
        let fence: u64 = fence.parse().unwrap();
        assert_eq!(shown, id.as_str());
        assert!(fence > last, "{fence} {last}");
        assert_eq!(
            workspace_hash(&fleet),
            [Some(id.clone()), Some(fence.to_string())]
        );
        assert!(released(&fleet));
        last = fence;
    }
}

#[test]
fn apply_moves_the_workspace_pointer_for_versions_that_set_change_and_clear_added_by() {
    let Some(fleet) = fleet("ws_apply_added_by") else {
        return;
    };
    let dir = &fleet.dir;
    let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let owners = [Some("team-a"), None, None, None];
    let set = [Some("ana"), Some("bia"), None, None];
    let changed = [Some("ana-souza"), None, Some("caio_acme"), None];
    let mut ids = Vec::new();
    for (name, logins) in [("set", set), ("changed", changed)] {
        let file = format!("ops/{name}.toml");
        let version =
            crate::generations::added(&crate::generations::owned(&text, &owners), &logins);
        fs::write(dir.join(&file), version).unwrap();
        ids.push(crate::generations::import(dir, &file, name, "alice").0);
    }
    let (plain, _) = crate::generations::import(dir, MANIFEST, "plain", "alice");
    let mut last = 0u64;
    for id in [&ids[0], &ids[1], &plain, &ids[0]] {
        let (stdout, stderr, code) = apply(dir, id);
        assert_eq!((code, stderr.as_str()), (Some(0), ""), "{id}");
        let (shown, fence) = stdout.trim().split_once(' ').unwrap();
        let fence: u64 = fence.parse().unwrap();
        assert_eq!(shown, id.as_str());
        assert!(fence > last, "{fence} {last}");
        assert_eq!(
            workspace_hash(&fleet),
            [Some(id.clone()), Some(fence.to_string())]
        );
        assert!(released(&fleet));
        last = fence;
    }
}

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

#[test]
fn fence_recovery_refuses_unusable_counters_without_poisoning_the_next_publish() {
    let Some(fleet) = fleet("ops_ceiling") else {
        return;
    };
    let dir = &fleet.dir;
    let g1 = locked_publish(dir);
    let mut valkey = admin(&fleet.valkey);
    let fence_key = key(&fleet, "fence");
    let workspace = key(&fleet, "workspace");
    for value in [
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
    ] {
        for lang in ["en_US.UTF-8", "pt_BR.UTF-8"] {
            for recorded in [false, true] {
                let before = get(&fleet, "fence");
                let mut args = vec!["snapshot", "fence", "recover", "--workspace", MANIFEST];
                if recorded {
                    send(
                        &mut valkey,
                        &["HSET", &workspace, "id", "0123456789abcdef", "fence", value],
                    );
                } else {
                    args.extend(["--at-least", value]);
                }
                let (stdout, stderr, code) = run_in(dir, lang, &args);
                assert_eq!((code, stdout.as_str()), (Some(1), ""), "{value}: {stderr}");
                let reason = if lang.starts_with("pt") {
                    "a operação esgotaria o contador de fences; o maior fence utilizável é 9223372036854775806 (limite de INCR do Valkey)"
                } else {
                    "operation would exhaust the fence counter; the maximum usable fence is 9223372036854775806 (Valkey INCR limit)"
                };
                assert_eq!(stderr, format!("meada: {fence_key}: {reason}\n"));
                assert_eq!(get(&fleet, "fence"), before);
                assert!(released(&fleet));
                send(&mut valkey, &["DEL", &workspace]);
            }
        }
    }
    assert_eq!(locked_publish(dir), g1);
    let recover = [
        "snapshot",
        "fence",
        "recover",
        "--workspace",
        MANIFEST,
        "--at-least",
        "9223372036854775806",
    ];
    assert_eq!(
        run_in(dir, "en_US.UTF-8", &recover),
        ("9223372036854775806\n".into(), String::new(), Some(0))
    );
    let again = run_in(dir, "en_US.UTF-8", &recover);
    assert_eq!(again.2, Some(1), "{again:?}");
    assert_eq!(get(&fleet, "fence").as_deref(), Some("9223372036854775806"));
    assert!(released(&fleet));
    assert_eq!(locked_publish(dir), g1);
    assert_eq!(current(&fleet).1, "9223372036854775807");
}

struct ServerTime {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl ServerTime {
    fn new(upstream: String, seconds: u64) -> Self {
        fn reply(to: &mut TcpStream, value: redis::Value) {
            use redis::Value;
            match value {
                Value::Nil => to.write_all(b"$-1\r\n").unwrap(),
                Value::Int(n) => write!(to, ":{n}\r\n").unwrap(),
                Value::Okay => to.write_all(b"+OK\r\n").unwrap(),
                Value::SimpleString(s) => write!(to, "+{s}\r\n").unwrap(),
                Value::BulkString(bytes) => {
                    write!(to, "${}\r\n", bytes.len()).unwrap();
                    to.write_all(&bytes).unwrap();
                    to.write_all(b"\r\n").unwrap();
                }
                Value::Array(values) => {
                    write!(to, "*{}\r\n", values.len()).unwrap();
                    for value in values {
                        reply(to, value);
                    }
                }
                other => panic!("unexpected RESP2 value: {other:?}"),
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = std::thread::spawn(move || {
            for client in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                let mut client = client.unwrap();
                client
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut parser = redis::Parser::new();
                let mut server = admin(&upstream);
                while let Ok(value) = parser.parse_value(&mut client) {
                    let args: Vec<Vec<u8>> = redis::from_redis_value(value).unwrap();
                    let command = std::str::from_utf8(&args[0]).unwrap();
                    if command.eq_ignore_ascii_case("TIME") {
                        reply(
                            &mut client,
                            redis::Value::Array(vec![
                                redis::Value::BulkString(seconds.to_string().into_bytes()),
                                redis::Value::BulkString(b"0".to_vec()),
                            ]),
                        );
                    } else {
                        match redis::cmd(command)
                            .arg(&args[1..])
                            .query::<redis::Value>(&mut server)
                        {
                            Ok(value) => reply(&mut client, value),
                            Err(_) => client
                                .write_all(b"-ERR upstream command failed\r\n")
                                .unwrap(),
                        }
                    }
                }
            }
        });
        Self {
            address,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for ServerTime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn fence_recovery_uses_the_heartbeat_servers_clock() {
    let Some(fleet) = fleet("ops_server_clock") else {
        return;
    };
    let mut valkey = admin(&fleet.valkey);
    let id = "0123456789abcdef";
    send(
        &mut valkey,
        &["HSET", &key(&fleet, "workspace"), "id", id, "fence", "500"],
    );
    for (name, score, fence) in [("live", "1001", 700), ("expired", "999", 900)] {
        let beat = serde_json::json!({"name":name,"generation":id,"fence":fence,"held":[id],"ready":true,"version":"0.1.0","time":"2026-10-22T00:00:00Z"});
        send(&mut valkey, &["ZADD", &key(&fleet, "nodes"), score, name]);
        send(
            &mut valkey,
            &[
                "SET",
                &key(&fleet, &format!("node:{name}")),
                &beat.to_string(),
                "EX",
                "60",
            ],
        );
    }
    let time = ServerTime::new(fleet.valkey.clone(), 1000);
    let file = fleet.dir.join(MANIFEST);
    let source = fs::read_to_string(&file)
        .unwrap()
        .replace(&fleet.valkey, &format!("redis://{}", time.address));
    fs::write(&file, source).unwrap();
    let recovered = run_in(
        &fleet.dir,
        "en_US.UTF-8",
        &["snapshot", "fence", "recover", "--workspace", MANIFEST],
    );
    drop(time);
    assert_eq!(recovered, ("700\n".into(), String::new(), Some(0)));
}

fn compose_job(dir: &Path) {
    let deploy = Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/compose");
    for script in ["refresh.sh", "fetch.sh"] {
        let src = fs::read_to_string(deploy.join(script)).unwrap();
        let moved = src.replace("/etc/meada/", "ops/");
        fs::write(
            dir.join("ops").join(script),
            moved.replace("/srv/mirrors", "mirrors"),
        )
        .unwrap();
    }
    let _ = fs::remove_dir_all(dir.join("mirrors"));
    for (identity, mirror) in crate::manifest::CHAIN {
        let link = dir.join("mirrors").join(identity);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(dir.join(mirror), link).unwrap();
    }
}

#[cfg_attr(
    not(all(feature = "github-app", feature = "postgres", feature = "oidc")),
    expect(dead_code)
)]
pub(crate) fn compose_managed(dir: &Path) {
    let deploy = Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/compose");
    let src = fs::read_to_string(deploy.join("managed-refresh.sh")).unwrap();
    let moved = (src.replace("/etc/meada/", "ops/")).replace("/tmp/pulled.toml", "ops/pulled.toml");
    fs::write(dir.join("ops/managed-refresh.sh"), moved).unwrap();
}

fn commit_later(repo: &Path) {
    fs::write(repo.join("src/later.ts"), "export const later = 1;\n").unwrap();
    crate::common::git(repo, &["add", "-A"]);
    crate::common::git(repo, &["commit", "-q", "-m", "later"]);
}

fn compose_refresh(dir: &Path, repositories: &str) -> (String, String, Option<i32>) {
    compose_run(dir, "refresh.sh", repositories)
}

pub(crate) fn compose_run(
    dir: &Path,
    script: &str,
    repositories: &str,
) -> (String, String, Option<i32>) {
    let bin = Path::new(MEADA).parent().unwrap().display().to_string();
    let path = format!("{bin}:{}", std::env::var("PATH").unwrap_or_default());
    let args = ["snapshot", "lock", "--workspace", MANIFEST, "--"];
    let mut job = meada(dir, &args);
    job.args(["sh", &format!("ops/{script}")])
        .env("PATH", path)
        .env("PUBLISH", "1")
        .env("REPOSITORIES", repositories)
        .env("GIT_ALLOW_PROTOCOL", "file")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    text(&job.output().expect("meada runs"))
}

#[test]
fn a_failed_fetch_or_index_publishes_nothing_and_keeps_the_current_generation() {
    use crate::common::assert_golden;
    use crate::generations::meta;
    use crate::manifest::ReadOnly;
    use crate::server::{call, text as answer};
    use crate::serving::serve;
    use crate::snapshot::block_on;
    let Some(fleet) = fleet("lock_abort") else {
        return;
    };
    let dir = &fleet.dir;
    let g1 = locked_publish(dir);
    let (node, _) = fleet.node("node");
    assert_eq!(fleet.pull(&node), g1);
    compose_job(dir);
    let gateway = dir.join("chain-gateway");
    commit_later(&gateway);

    let (stdout, stderr, code) = compose_refresh(dir, "https://example.com/chain/web");
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    let fetch = "refresh: fetch failed for example.com/chain/web; the refresh stops before index \
                 and publish\n";
    assert!(stderr.ends_with(fetch), "{stderr}");
    assert_eq!(current(&fleet), (g1.clone(), "1".to_string()));
    let secret = "https://x-access-token:s3cr3t@example.com/chain/web";
    let (_, stderr, code) = compose_refresh(dir, secret);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("without credentials"), "{stderr}");
    assert!(!stderr.contains("s3cr3t"), "{stderr}");

    let web = dir.join("chain-web");
    let origin = "https://x-access-token:s3cr3t@example.com/chain/web.git";
    crate::common::git(&web, &["remote", "set-url", "origin", origin]);
    let (stdout, stderr, code) = compose_refresh(dir, "https://example.com/chain/web");
    let clean = "https://example.com/chain/web.git";
    crate::common::git(&web, &["remote", "set-url", "origin", clean]);
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(
        stderr.contains("origin of example.com/chain/web takes a URL without credentials"),
        "{stderr}"
    );
    assert!(!stderr.contains("s3cr3t"), "{stderr}");
    assert_eq!(current(&fleet), (g1.clone(), "1".to_string()));

    fs::write(dir.join("ops/fetch.sh"), ":\n").unwrap();
    commit_later(&web);
    let Some(locked) = ReadOnly::lock(&dir.join("indexes/example.com/chain/web")) else {
        return;
    };
    let (stdout, stderr, code) = compose_refresh(dir, "unused");
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(
        stderr.starts_with("meada: example.com/chain/web: ")
            && stderr.ends_with("\nmeada: index failed for 1 repository\n"),
        "{stderr}"
    );
    assert_eq!(current(&fleet), (g1.clone(), "1".to_string()));

    assert_eq!(fleet.pull(&node), g1);
    let held = format!("current: {g1}\nheld: {g1}\nstore: reachable (current {g1})\n");
    assert_eq!(probe(dir, &node), (held, String::new(), Some(0)));
    let server = serve(dir, &node);
    block_on(async {
        let client = server.client("/example.com/chain/web").await;
        let from = serde_json::json!({"from": "dashboard"});
        let trace = client.call_tool(call("trace", from)).await.unwrap();
        assert_golden("chain.trace-down.txt", answer(&trace));
        assert_eq!(meta(&trace).as_deref(), Some(g1.as_str()));
        client.cancel().await.unwrap();
    });

    drop(locked);
    let (g2, stderr, code) = compose_refresh(dir, "unused");
    assert_eq!(code, Some(0), "{stderr}");
    let g2 = g2.trim().to_string();
    assert_ne!(g2, g1);
    assert_eq!(current(&fleet).0, g2);
}

#[test]
fn a_pulled_fetch_refuses_a_mirror_whose_origin_carries_credentials() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("pulled_origin");
    let _ = fs::remove_dir_all(&dir);
    let mirrors = dir.join("mirrors").display().to_string();
    let deploy = Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/compose");
    let src = fs::read_to_string(deploy.join("fetch-pulled.sh")).unwrap();
    let mirror = dir.join("mirrors/example.com/chain/web");
    fs::create_dir_all(&mirror).unwrap();
    fs::write(
        dir.join("fetch-pulled.sh"),
        src.replace("/srv/mirrors", &mirrors),
    )
    .unwrap();
    let list = format!(
        "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"{}\"\n",
        mirror.display()
    );
    fs::write(dir.join("pulled.toml"), list).unwrap();
    crate::common::git(&mirror, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let fetch = |origin: &str| {
        crate::common::git(&mirror, &["remote", "add", "origin", origin]);
        let out = Command::new("sh")
            .current_dir(&dir)
            .args(["fetch-pulled.sh", "pulled.toml"])
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("sh runs");
        crate::common::git(&mirror, &["remote", "remove", "origin"]);
        text(&out)
    };

    let (stdout, stderr, code) = fetch("https://x-access-token:s3cr3t@example.com/chain/web.git");
    let refused = "refresh: the origin of example.com/chain/web takes a URL without credentials; \
                   give git a credential helper\n";
    assert_eq!(
        (stdout.as_str(), stderr.as_str(), code),
        ("", refused, Some(1))
    );

    let (_, stderr, code) = fetch("https://example.com/chain/web.git");
    let failed = "refresh: fetch failed for example.com/chain/web; the refresh stops before index \
                  and publish\n";
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.ends_with(failed), "{stderr}");
    assert!(!stderr.contains("without credentials"), "{stderr}");
}

#[cfg(feature = "postgres")]
#[test]
fn every_cli_twin_is_audited_and_none_writes_without_its_record() {
    use serde_json::json;
    let Some(fleet) = fleet("ops_audit") else {
        return;
    };
    let dir = &fleet.dir;
    let Some(audit) = crate::postgres::audited(dir, "ops_audit") else {
        return;
    };
    let g1 = locked_publish(dir);
    next_commit(dir, "chain-web", "audit-second");
    let g2 = locked_publish(dir);
    append_section(dir, &audit.section);
    let at = |args: &[&str]| -> Vec<String> {
        let tail = ["--workspace", MANIFEST];
        args.iter().chain(&tail).map(|a| a.to_string()).collect()
    };
    let run = |args: &[String], version_max: Option<&str>| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut command = meada(dir, &args);
        if let Some(max) = version_max {
            command.env("MEADA_POSTGRES_VERSION_MAX", max);
        }
        text(&command.output().unwrap())
    };
    let entries = "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n";
    fs::write(dir.join("ops/entries.toml"), entries).unwrap();
    let import = at(&[
        "workspace",
        "import",
        "ops/entries.toml",
        "--message",
        "one",
        "--author",
        "alice",
    ]);
    let (stdout, stderr, code) = run(&import, None);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stderr}");
    let version = stdout.trim().to_string();
    let writes = [
        at(&["workspace", "apply", &version]),
        at(&["snapshot", "pointer", "retain", &g1]),
        at(&["snapshot", "pointer", "set", &g1]),
        at(&["snapshot", "pointer", "drop", &g2]),
        at(&["snapshot", "fence", "recover", "--at-least", "0040"]),
    ];
    let mut printed = vec![version.clone()];
    for args in &writes {
        let (stdout, stderr, code) = run(args, None);
        assert_eq!((code, stderr.as_str()), (Some(0), ""), "{args:?}");
        printed.push(stdout.trim().to_string());
    }
    assert_eq!(printed[5], "40");
    let (_, refusal, code) = run(&at(&["snapshot", "pointer", "drop", &g1]), None);
    assert_eq!(code, Some(1), "{refusal}");
    let child = lock(dir, MANIFEST, "touch ops/started; exec sleep 60");
    wait_for(&dir.join("ops/started"));
    let (_, held, code) = run(&at(&["snapshot", "pointer", "set", &g2]), None);
    assert_eq!((code, held.clone()), (Some(1), held_by(&fleet, child.id())));
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
    child.wait();

    let records = audit.records();
    assert_eq!(records.len(), 8, "{records:#?}");
    let author = records[1]["subject"].as_str().unwrap().to_string();
    assert!(
        author.starts_with("cli:") && author.contains('@'),
        "{author}"
    );
    let expected = [
        (
            "workspace.import",
            json!({"version": version, "message": "one"}),
        ),
        ("workspace.apply", json!({"version": version})),
        ("pointer.retain", json!({"generation": g1})),
        ("pointer.set", json!({"generation": g1})),
        ("pointer.drop", json!({"generation": g2})),
        ("fence.recover", json!({"at_least": "40"})),
        ("pointer.drop", json!({"generation": g1})),
        ("pointer.set", json!({"generation": g2})),
    ];
    let line = |stderr: &str| stderr.trim_end().trim_start_matches("meada: ").to_string();
    for (n, record) in records.iter().enumerate() {
        let subject = if n == 0 { "alice" } else { author.as_str() };
        let (outcome, message) = match n {
            0..=5 => ("accepted", printed[n].clone()),
            6 => ("refused", line(&refusal)),
            _ => ("refused", line(&held)),
        };
        let (action, arguments) = &expected[n];
        let fields = [
            "via",
            "subject",
            "login",
            "role",
            "owners",
            "action",
            "arguments",
        ];
        let found: Vec<_> = fields.iter().map(|f| record[f].clone()).collect();
        assert_eq!(
            found,
            [
                json!("cli"),
                json!(subject),
                json!(null),
                json!("operator"),
                json!(null),
                json!(action),
                arguments.clone()
            ],
            "{record}"
        );
        assert_eq!(
            (&record["outcome"], &record["message"]),
            (&json!(outcome), &json!(message))
        );
    }

    let state = |fleet: &Fleet| {
        let pointer = pointer(&mut admin(&fleet.valkey), fleet.case);
        (
            pointer,
            kept(fleet),
            get(fleet, "fence"),
            workspace_hash(fleet),
        )
    };
    let before = state(&fleet);
    let other = "[[repository]]\nrepo = \"e.test/x\"\npath = \"../x\"\n";
    fs::write(dir.join("ops/other.toml"), other).unwrap();
    let twins = [
        vec![at(&[
            "workspace",
            "import",
            "ops/other.toml",
            "--message",
            "two",
        ])],
        writes.to_vec(),
    ]
    .concat();
    let refused = "meada: the audit record cannot be written to Postgres, so nothing was changed: ";
    audit.relay.close();
    for args in &twins {
        let (stdout, stderr, code) = run(args, None);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{args:?}: {stderr}");
        assert!(
            stderr.starts_with(refused) && stderr.lines().count() == 1,
            "{stderr}"
        );
        audit.assert_no_url(&stderr);
    }
    fs::write(dir.join("ops/pg-url"), &audit.url).unwrap();
    let higher = &format!(
        "is at version {}, above this Meada's 1: upgrade Meada",
        crate::common::SCHEMA_VERSION
    );
    for args in &twins {
        let (stdout, stderr, code) = run(args, Some("1"));
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{args:?}: {stderr}");
        assert!(
            stderr.starts_with(refused) && stderr.trim_end().ends_with(higher),
            "{stderr}"
        );
        audit.assert_no_url(&stderr);
    }
    assert_eq!(state(&fleet), before);
    assert_eq!(audit.records().len(), 8);
    let (listed, _, _) = run(&at(&["workspace", "list"]), None);
    assert!(
        listed.contains(": one") && !listed.contains(": two"),
        "{listed}"
    );
}

#[cfg(feature = "postgres")]
#[test]
fn an_import_past_the_version_bound_leaves_no_audit_record() {
    let Some(fleet) = fleet("ops_audit_big") else {
        return;
    };
    let dir = &fleet.dir;
    let Some(audit) = crate::postgres::audited(dir, "ops_audit_big") else {
        return;
    };
    append_section(dir, &audit.section);
    let args = |file: &str| -> Vec<String> {
        [
            "workspace",
            "import",
            file,
            "--message",
            "m",
            "--author",
            "a",
        ]
        .iter()
        .chain(&["--workspace", MANIFEST])
        .map(|a| a.to_string())
        .collect()
    };
    let run = |file: &str| {
        let args = args(file);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        text(&meada(dir, &args).output().unwrap())
    };
    let entries = "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n";
    fs::write(dir.join("ops/entries.toml"), entries).unwrap();
    let (_, stderr, code) = run("ops/entries.toml");
    assert_eq!(code, Some(0), "{stderr}");
    let before = audit.records().len();
    assert_eq!(before, 1);
    fs::write(dir.join("ops/big.toml"), vec![b'#'; 4_194_304 + 1]).unwrap();
    let (stdout, stderr, code) = run("ops/big.toml");
    assert_eq!((stdout.as_str(), code), ("", Some(1)), "{stderr}");
    assert!(stderr.contains("larger than 4194304 bytes"), "{stderr}");
    assert_eq!(audit.records().len(), before);
}
