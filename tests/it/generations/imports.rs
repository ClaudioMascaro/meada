//! Immutable workspace imports through the binary, racing against the real configured S3.

use super::{Fleet, MANIFEST, meada_in};
use crate::snapshot::KEY_ID;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn headers(reader: &mut impl BufRead) -> Vec<u8> {
    let mut headers = Vec::new();
    loop {
        let mut line = Vec::new();
        assert!(reader.read_until(b'\n', &mut line).unwrap() > 0);
        let done = line == b"\r\n";
        headers.extend(line);
        if done {
            return headers;
        }
    }
}

/// Hold both genuine missing-object replies before allowing either importer to upload. All
/// signed bytes and buffered response bodies still pass between the CLI and real RustFS.
fn missing_together(to: &str) -> (String, std::thread::JoinHandle<()>) {
    let target = reqwest::Url::parse(to).unwrap();
    assert_eq!(target.scheme(), "http", "the local race fixture uses HTTP");
    let target = format!(
        "{}:{}",
        target.host_str().unwrap(),
        target.port_or_known_default().unwrap()
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (ready, incoming) = mpsc::channel();
        let mut workers = Vec::new();
        while workers.len() < 2 {
            match listener.accept() {
                Ok((client, _)) => {
                    let target = target.clone();
                    let ready = ready.clone();
                    workers.push(std::thread::spawn(move || {
                        let upstream = TcpStream::connect(target).unwrap();
                        for stream in [&client, &upstream] {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(5)))
                                .unwrap();
                            stream
                                .set_write_timeout(Some(Duration::from_secs(5)))
                                .unwrap();
                        }
                        let mut down = client.try_clone().unwrap();
                        let mut up = upstream.try_clone().unwrap();
                        let mut client = BufReader::new(client);
                        let mut upstream = BufReader::new(upstream);
                        let request = headers(&mut client);
                        assert!(request.starts_with(b"GET "));
                        up.write_all(&request).unwrap();
                        let response = headers(&mut upstream);
                        assert!(
                            String::from_utf8_lossy(&response)
                                .lines()
                                .next()
                                .unwrap()
                                .contains("404")
                        );
                        let (release, wait) = mpsc::channel();
                        ready.send(release).unwrap();
                        wait.recv_timeout(Duration::from_secs(10)).unwrap();
                        down.write_all(&response).unwrap();
                        let requests = std::thread::spawn(move || {
                            let _ = std::io::copy(&mut client, &mut up);
                            let _ = up.shutdown(Shutdown::Write);
                        });
                        let _ = std::io::copy(&mut upstream, &mut down);
                        let _ = down.shutdown(Shutdown::Both);
                        requests.join().unwrap();
                    }));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "both import connections arrive");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        }
        let releases: Vec<_> = (0..2)
            .map(|_| incoming.recv_timeout(Duration::from_secs(10)).unwrap())
            .collect();
        for release in releases {
            release.send(()).unwrap();
        }
        for worker in workers {
            worker.join().unwrap();
        }
    });
    (endpoint, server)
}

#[test]
fn concurrent_imports_keep_the_first_author_time_and_message() {
    let Some(fleet) = Fleet::new("workspace_import_race", 2) else {
        return;
    };
    let (endpoint, proxy) = missing_together(&fleet.endpoint);
    let manifest = std::fs::read_to_string(fleet.dir.join(MANIFEST)).unwrap();
    assert!(manifest.contains(&fleet.endpoint));
    std::fs::write(
        fleet.dir.join("ops/race.toml"),
        manifest.replace(&fleet.endpoint, &endpoint),
    )
    .unwrap();
    std::fs::write(
        fleet.dir.join("entries"),
        "[[repository]]\nrepo = \"example.com/team/race\"\npath = \"mirror\"\n",
    )
    .unwrap();
    let spawn = |author: &str, message: &str| {
        Command::new(env!("CARGO_BIN_EXE_meada"))
            .current_dir(&fleet.dir)
            .args([
                "workspace",
                "import",
                "entries",
                "--message",
                message,
                "--author",
                author,
                "--workspace",
                "ops/race.toml",
            ])
            .env("LANG", "en_US.UTF-8")
            .env("MEADA_TEST_S3_KEY", KEY_ID)
            .env_remove("MEADA_TEST_S3_TOKEN")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = spawn("writer-one", "message-one");
    let second = spawn("writer-two", "message-two");
    let outputs = [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ];
    proxy.join().unwrap();
    for output in &outputs {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(outputs[0].stdout, outputs[1].stdout);
    let existing: Vec<_> = outputs
        .iter()
        .filter(|output| !output.stderr.is_empty())
        .collect();
    assert_eq!(
        existing.len(),
        1,
        "one importer reports the immutable first version"
    );
    let winner = if outputs[0].stderr.is_empty() {
        ("writer-one", "message-one")
    } else {
        ("writer-two", "message-two")
    };
    let loser = if winner.0 == "writer-one" {
        ("writer-two", "message-two")
    } else {
        ("writer-one", "message-one")
    };
    let stored = String::from_utf8_lossy(&existing[0].stderr);
    assert!(
        stored.contains("is stored already")
            && stored.contains(winner.0)
            && stored.contains(winner.1),
        "{stored}"
    );
    let (listing, _, code) = meada_in(
        &fleet.dir,
        "en_US.UTF-8",
        &["workspace", "list", "--workspace", MANIFEST],
    );
    assert_eq!(code, Some(0));
    assert!(
        listing.contains(winner.0) && listing.contains(winner.1),
        "{listing}"
    );
    assert!(
        !listing.contains(loser.0) && !listing.contains(loser.1),
        "{listing}"
    );
    let time = listing.split_whitespace().nth(1).unwrap();
    assert!(
        stored.contains(time),
        "first time is immutable: {stored}; {listing}"
    );
}

/// A local current-version swap during the informational Valkey probe cannot mix the reported
/// workspace with the readiness exit code. The same managed version is read once per command.
#[test]
fn status_reports_and_probes_the_same_pulled_workspace_after_a_concurrent_swap() {
    use super::{BOOTSTRAP, import, on_bootstrap, send, store_bootstrap, workspace_pointer};
    let Some(fleet) = Fleet::new("workspace_status_once", 2) else {
        return;
    };
    let dir = &fleet.dir;
    store_bootstrap(dir, "../store-indexes");
    let (good, _) = import(dir, MANIFEST, "ready", "alice");
    std::fs::write(
        dir.join("ops/missing.toml"),
        "[[repository]]\nrepo = \"e.test/team/missing\"\npath = \"../missing-mirror\"\n",
    )
    .unwrap();
    let (bad, _) = import(dir, "ops/missing.toml", "unready", "bob");
    let mut valkey = super::admin(&fleet.valkey);
    let pointer = workspace_pointer(fleet.case);
    for (id, fence) in [(&bad, "2"), (&good, "3")] {
        send(&mut valkey, &["HSET", &pointer, "id", id, "fence", fence]);
        let (stdout, stderr, code) = on_bootstrap(dir, &["workspace", "pull"]);
        assert_eq!((code, stdout), (Some(0), format!("{id}\n")), "{stderr}");
    }
    let (_, stderr, code) = on_bootstrap(dir, &["index"]);
    assert_eq!(code, Some(0), "{stderr}");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let hanging = format!("redis://{}", listener.local_addr().unwrap());
    let bootstrap = dir.join(BOOTSTRAP);
    let source = std::fs::read_to_string(&bootstrap).unwrap();
    assert!(source.contains(&fleet.valkey));
    std::fs::write(&bootstrap, source.replace(&fleet.valkey, &hanging)).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(dir)
        .args(["status", "--workspace", BOOTSTRAP])
        .env("LANG", "en_US.UTF-8")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let _connection = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "status reaches the informational Valkey probe"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{error}"),
        }
    };
    let local = dir.join("store-indexes/workspace");
    std::fs::write(local.join("current.new"), format!("{bad} 4\n")).unwrap();
    std::fs::rename(local.join("current.new"), local.join("current")).unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stdout.contains("example.com/chain/web") && !stdout.contains("e.test/team/missing"),
        "{stdout}"
    );
    assert_eq!(output.status.code(), Some(0), "{stdout}; {stderr}");
    // The swapped version really is unready for a later command.
    let (_, _, code) = on_bootstrap(dir, &["status"]);
    assert_eq!(code, Some(1));
}
