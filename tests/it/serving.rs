//! Serving a pulled generation (ADR 0012, spec #259, feature `snapshot`): `meada mcp --listen
//! --workspace <node manifest> --generations`, `meada status --generations` and `meada export
//! --generations` over `local_dir`s two nodes pulled from the local S3-compatible server
//! `MEADA_TEST_S3_ENDPOINT` names. Without it these tests print why and skip; with
//! `MEADA_TEST_SERVICES_REQUIRED=1` they fail. The ones over a generation made by hand
//! (`hand_node`) need no store.
#![cfg(feature = "snapshot")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::common::{assert_golden, git};
use crate::manifest::{CHAIN, MANIFEST, chain_manifest, contents, run, run_args};
use crate::server::{Server, call, get, get_with, text};
use crate::snapshot::{
    HOSTILE_SCHEMAS, KEY_ID, SECRET, block_on, bucket, bucket_name, crafted_index, named_node,
    next_commit, publish, pull, service, store_manifest,
};

const WEB: &str = "example.com/chain/web";
const CATALOG: &str = "example.com/chain/catalog";

/// `meada mcp --listen 127.0.0.1:0 --workspace <manifest> --generations`, in `dir`.
pub(crate) fn serve(dir: &Path, manifest: &str) -> Server {
    let args = [
        "--listen",
        "127.0.0.1:0",
        "--workspace",
        manifest,
        "--generations",
    ];
    Server::start_args(dir, &args, &[])
}

/// `meada status --workspace <manifest> --generations` in `dir`, with the store's key id: stdout,
/// stderr and the exit code.
pub(crate) fn probe(dir: &Path, manifest: &str) -> (String, String, Option<i32>) {
    let out = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(dir)
        .args(["status", "--workspace", manifest, "--generations"])
        .env("LANG", "en_US.UTF-8")
        .env("MEADA_TEST_S3_KEY", KEY_ID)
        .env_remove("MEADA_TEST_S3_TOKEN")
        .output()
        .expect("meada runs");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

/// The index of `WEB` in generation `generation` of `local`: the file a call opens first.
fn web_index(local: &Path, generation: &str) -> PathBuf {
    let document = fs::read(local.join(generation).join("generation.json")).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&document).unwrap();
    let key = document["repositories"][0]["bundle"].as_str().unwrap();
    let bundle = key.rsplit('/').next().unwrap().trim_end_matches(".tar");
    local.join(generation).join(bundle).join("index.db")
}

/// The answer to `search second` at `WEB`'s endpoint of `server`.
async fn search_second(server: &Server) -> String {
    let client = server.client(&format!("/{WEB}")).await;
    let query = serde_json::json!({"query": "second"});
    let answer = client.call_tool(call("search", query)).await.unwrap();
    client.cancel().await.unwrap();
    text(&answer).to_string()
}

/// A published chain and two nodes, each with the generation pulled.
struct Nodes {
    endpoint: String,
    bucket: String,
    /// The case directory, where the chain's manifest and every node's directory are.
    dir: PathBuf,
    first: String,
    /// Each node's manifest, relative to `dir`, and its `local_dir`.
    nodes: Vec<(String, PathBuf)>,
}

fn nodes(case: &str) -> Option<Nodes> {
    let endpoint = service()?;
    let bucket_name = bucket_name(case);
    bucket(&endpoint, &bucket_name);
    let dir = store_manifest(case, &endpoint, &bucket_name, SECRET);
    let (first, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let nodes: Vec<_> = (["one", "two"].into_iter())
        .map(|name| {
            let node = named_node(&dir, name, &endpoint, &bucket_name);
            assert_eq!(pull(&dir, &node.0, &[]).2, Some(0));
            node
        })
        .collect();
    Some(Nodes {
        endpoint,
        bucket: bucket_name,
        dir,
        first: first.trim().to_string(),
        nodes,
    })
}

/// Two nodes pull one published generation; `trace`, `context` and `export` over it equal the
/// chain's goldens, and the status probe prints the node's generation and writes nothing.
#[test]
fn a_pulled_generation_answers_as_the_chain_goldens_have_it() {
    let Some(Nodes {
        dir, first, nodes, ..
    }) = nodes("serving_goldens")
    else {
        return;
    };
    answer_the_goldens(&dir, &first, &nodes);
}

/// A node whose companion writes heartbeats answers as the chain's goldens have it, with the
/// same `Meada-Generation` header and `_meta` (#298).
#[cfg(feature = "generation-pointer")]
#[test]
fn a_node_whose_companion_writes_heartbeats_answers_the_same() {
    use crate::generations::{Companion, Fleet, admin};
    let Some(fleet) = Fleet::new("serving_heartbeat", 3) else {
        return;
    };
    let first = fleet.publish();
    let (node, local) = fleet.node("one");
    assert_eq!(fleet.pull(&node), first);
    let companion = Companion::start(&fleet.dir, &node, &[]);
    let name = format!("{{{}}}:node:", crate::generations::prefix(fleet.case));
    let mut valkey = admin(&fleet.valkey);
    let key = loop {
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(format!("{name}*"))
            .query(&mut valkey)
            .unwrap();
        if let Some(key) = keys.into_iter().next() {
            break key;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    answer_the_goldens(&fleet.dir, &first, &[(node.clone(), local.clone())]);
    // Heartbeats were written all the while, and the server named the generation.
    assert!(
        redis::cmd("EXISTS")
            .arg(&key)
            .query::<bool>(&mut valkey)
            .unwrap()
    );
    let server = serve(&fleet.dir, &node);
    let (_, _, named, _) = crate::generations::get(&server.url);
    assert_eq!(named.as_deref(), Some(first.as_str()));
    block_on(async {
        let client = server.client(&format!("/{WEB}")).await;
        let from = serde_json::json!({"from": "dashboard"});
        let trace = client.call_tool(call("trace", from)).await.unwrap();
        let named = crate::generations::meta(&trace);
        assert_eq!(named.as_deref(), Some(first.as_str()));
        client.cancel().await.unwrap();
    });
    companion.stop();
}

fn answer_the_goldens(dir: &Path, first: &str, nodes: &[(String, PathBuf)]) {
    for (manifest, local) in nodes {
        let before = contents(local);
        let server = serve(dir, manifest);
        // No path names a repository but the ones the generation recorded.
        let listing: String = CHAIN.iter().map(|(repo, _)| format!("{repo}\n")).collect();
        let (status, _, body) = get(&server.url);
        assert_eq!((status, body.as_str()), (404, listing.as_str()));
        block_on(async {
            let client = server.client(&format!("/{WEB}")).await;
            let from = serde_json::json!({"from": "dashboard"});
            let trace = client.call_tool(call("trace", from)).await.unwrap();
            assert_golden("chain.trace-down.txt", text(&trace));
            client.cancel().await.unwrap();
            let client = server.client(&format!("/{CATALOG}")).await;
            let symbol = serde_json::json!({"symbol": "lib/products.ts::find"});
            let context = client.call_tool(call("context", symbol)).await.unwrap();
            assert_golden("chain.context-processes.txt", text(&context));
            client.cancel().await.unwrap();
        });

        for (repo, path) in CHAIN {
            let args = [
                "export",
                "--workspace",
                manifest,
                "--repo",
                repo,
                "--generations",
            ];
            let (export, stderr, code) = run_args(dir, &args);
            assert_eq!((code, stderr.as_str()), (Some(0), ""), "{manifest} {repo}");
            assert_golden(&format!("{path}.export.json"), &export);
        }
        let (status, stderr, code) = probe(dir, manifest);
        assert_eq!((code, stderr.as_str()), (Some(0), ""), "{status}");
        assert!(
            status.starts_with(&format!("current: {first}\n")),
            "{status}"
        );
        assert_eq!(contents(local), before, "{manifest} wrote something");
    }
}

/// A call opened before a swap finishes on the generation it read; the next one, and a call
/// on a node that has not pulled, are answered by the generation each node holds.
#[test]
fn a_call_before_a_swap_answers_from_the_old_generation_and_the_next_from_the_new() {
    let Some(Nodes {
        dir, first, nodes, ..
    }) = nodes("serving_swap")
    else {
        return;
    };
    next_commit(&dir, "chain-web", "second");
    let (second, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let second = second.trim().to_string();
    let ((one, local), (two, _)) = (nodes[0].clone(), nodes[1].clone());
    let (first_server, second_server) = (serve(&dir, &one), serve(&dir, &two));

    // The call waits on a commit in progress in the first generation's index, read before the
    // node swaps; the pull needs nothing of that index.
    let commit = rusqlite::Connection::open(web_index(&local, &first)).unwrap();
    commit.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let swap = async {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let (dir, one) = (dir.clone(), one.clone());
        let pulled = tokio::task::spawn_blocking(move || pull(&dir, &one, &[])).await;
        assert_eq!(pulled.unwrap().0.trim(), second);
        drop(commit);
    };
    let (before, ()) = block_on(async { tokio::join!(search_second(&first_server), swap) });
    assert!(!before.contains("src/status.ts"), "{before}");

    let after = block_on(search_second(&first_server));
    assert!(after.contains("src/status.ts"), "{after}");
    // The node that has not pulled still answers from the first generation, and then from the
    // second once it has.
    let unpulled = block_on(search_second(&second_server));
    assert!(!unpulled.contains("src/status.ts"), "{unpulled}");
    assert_eq!(pull(&dir, &two, &[]).0.trim(), second);
    let pulled = block_on(search_second(&second_server));
    assert!(pulled.contains("src/status.ts"), "{pulled}");
}

/// `meada mcp --listen 127.0.0.1:0 --workspace <manifest> --generations` where the start is
/// refused: the exit code and stderr, with no address printed.
fn refused(dir: &Path, manifest: &str) -> (Option<i32>, String) {
    let args = [
        "mcp",
        "--listen",
        "127.0.0.1:0",
        "--workspace",
        manifest,
        "--generations",
    ];
    let (stdout, stderr, code) = run_args(dir, &args);
    assert_eq!(stdout, "");
    (code, stderr)
}

/// The probe exits on local state only: 1 while the node holds no servable generation, naming
/// the pull and refusing the start, and 0 once it does, the store reachable or not.
#[test]
fn the_probe_and_the_start_depend_on_the_nodes_generation_and_not_on_the_store() {
    let Some(Nodes {
        endpoint,
        bucket,
        dir,
        first,
        nodes,
    }) = nodes("serving_probe")
    else {
        return;
    };
    // A node that has pulled nothing.
    let (empty, _) = named_node(&dir, "empty", &endpoint, &bucket);
    let (stdout, stderr, code) = probe(&dir, &empty);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(
        stdout.starts_with("current: none\nheld: none\n"),
        "{stdout}"
    );
    assert!(stdout.contains("store: reachable (current "), "{stdout}");
    assert!(stderr.contains("meada snapshot pull"), "{stderr}");
    let (code, stderr) = refused(&dir, &empty);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("meada snapshot pull"), "{stderr}");
    assert!(!stderr.contains("listening on"), "{stderr}");

    // A node that has: the generation, the ids held and the store's current one.
    let (one, local) = nodes[0].clone();
    let (stdout, stderr, code) = probe(&dir, &one);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
    assert_eq!(
        stdout,
        format!("current: {first}\nheld: {first}\nstore: reachable (current {first})\n")
    );

    // The store stopped: the node stays ready and a new server starts and serves.
    let manifest = dir.join(&one);
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap().port();
    drop(dead);
    let toml = fs::read_to_string(&manifest).unwrap();
    let gone = toml.replace(&endpoint, &format!("http://127.0.0.1:{port}"));
    assert_ne!(toml, gone);
    fs::write(&manifest, gone).unwrap();
    let (stdout, stderr, code) = probe(&dir, &one);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
    assert!(
        stdout.starts_with(&format!("current: {first}\n")),
        "{stdout}"
    );
    assert!(stdout.contains("store: unreachable ("), "{stdout}");
    let served = block_on(search_second(&serve(&dir, &one)));
    assert!(served.starts_with("no matches"), "{served}");

    // An index of this schema version holding a schema object this build does not create is
    // not servable either: the start names the repository and the object.
    let index = web_index(&local, &first);
    let kept = local.join("kept.db");
    fs::copy(&index, &kept).unwrap();
    for (sql, object) in HOSTILE_SCHEMAS {
        crafted_index(&kept, &index, sql);
        let (_, stderr, code) = probe(&dir, &one);
        assert_eq!(code, Some(1), "{stderr}");
        let why = format!("{WEB}: {object}");
        assert!(stderr.contains(&why), "{stderr}");
        let (code, stderr) = refused(&dir, &one);
        assert_eq!(code, Some(1));
        assert!(stderr.contains(&why), "{stderr}");
    }
    fs::rename(&kept, &index).unwrap();

    // An index of another schema version is not servable: the probe says which generation and
    // repository, and the start is refused.
    let db = rusqlite::Connection::open(web_index(&local, &first)).unwrap();
    db.execute_batch("PRAGMA user_version = 999").unwrap();
    drop(db);
    let (_, stderr, code) = probe(&dir, &one);
    assert_eq!(code, Some(1), "{stderr}");
    let why = format!("{WEB}: indexed by another version in generation {first}");
    assert!(stderr.contains(&why), "{stderr}");
    let (code, stderr) = refused(&dir, &one);
    assert_eq!(code, Some(1));
    assert!(stderr.contains(&why), "{stderr}");

    // `--generations` belongs to `--workspace`, and the section needs a `local_dir`.
    let (_, stderr, code) = run_args(&dir, &["status", "--generations"]);
    assert_ne!(code, Some(0), "{stderr}");
    let (_, stderr, code) = run_args(&dir, &["status", "--workspace", MANIFEST, "--generations"]);
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("`snapshot.local_dir` is not set"),
        "{stderr}"
    );
}

/// The generation id of `hand_node`.
const HAND: &str = "00000000000000aa";

/// A node holding a generation made by hand, as a pull leaves one, with no store: the chain's
/// mirrors (copied, `.git` included) and indexes under `<case>/hand/generations/HAND/`, its
/// document and the pointer naming it. Returns the case directory, the node's manifest relative
/// to it and its `local_dir`.
pub(crate) fn hand_node(case: &str) -> (PathBuf, String, PathBuf) {
    let dir = chain_manifest(case);
    let (manifest, local) = hand_node_of(&dir, &CHAIN);
    (dir, manifest, local)
}

/// `hand_node` over `repos` (identity, directory in `dir`), listed and indexed in `dir` already.
/// Returns the node's manifest relative to `dir` and its `local_dir`.
pub(crate) fn hand_node_of(dir: &Path, repos: &[(&str, &str)]) -> (String, PathBuf) {
    assert!(run(dir, "index").2, "index");
    let (manifest, local) = named_node(dir, "hand", "http://127.0.0.1:1", "meada-hand");
    let mut repositories = Vec::new();
    for (n, (repo, path)) in repos.iter().enumerate() {
        let bundle = format!("{n:016x}");
        let to = local.join(HAND).join(&bundle);
        fs::create_dir_all(&to).unwrap();
        let copied = Command::new("cp")
            .arg("-a")
            .arg(dir.join(path))
            .arg(to.join("mirror"))
            .status()
            .unwrap();
        assert!(copied.success());
        fs::copy(
            dir.join("indexes").join(repo).join("index.db"),
            to.join("index.db"),
        )
        .unwrap();
        let head = git(&to.join("mirror"), &["rev-parse", "HEAD"]);
        repositories.push(serde_json::json!({
            "identity": repo, "head": head.trim(), "blocks": {}, "size": 0,
            "bundle": format!("prod/meada/repos/{repo}/{bundle}.tar"), "blake3": "0".repeat(64)}));
    }
    let document = serde_json::json!({"generation": HAND, "repositories": repositories});
    let document = serde_json::to_vec(&document).unwrap();
    fs::write(local.join(HAND).join("generation.json"), document).unwrap();
    fs::write(local.join("current"), format!("{HAND} 0\n")).unwrap();
    (manifest, local)
}

/// A node whose current generation stops being servable answers every request 503 with a fixed
/// body: no local path and no parse error reaches a client. A browser's request gets the 403
/// before it.
#[test]
fn a_node_with_no_servable_generation_answers_503_with_a_fixed_body() {
    let (dir, manifest, local) = hand_node("serving_unservable");
    let server = serve(&dir, &manifest);
    let listing: String = CHAIN.iter().map(|(repo, _)| format!("{repo}\n")).collect();
    assert_eq!(get(&server.url).2, listing);
    // A document that no longer parses, then a pointer to no generation.
    fs::write(local.join(HAND).join("generation.json"), b"{").unwrap();
    let other = "00000000000000bb";
    fs::write(local.join("current"), format!("{other} 0\n")).unwrap();
    for path in ["", &format!("/{WEB}")] {
        let (status, _, body) = get(&format!("{}{path}", server.url));
        assert_eq!((status, body.as_str()), (503, "no servable generation\n"));
        let origin = [("Origin", "http://evil.example")];
        assert_eq!(get_with(&format!("{}{path}", server.url), &origin).0, 403);
    }
}

/// An index row naming a file outside the mirror (an absolute path, `..`) renders as a source
/// that cannot be read, and the answer still comes: `/dev/zero` would fill the memory, and a
/// path outside would show the line layout of another file.
#[test]
fn an_index_naming_files_outside_its_mirror_answers_without_reading_them() {
    let (dir, manifest, local) = hand_node("serving_outside");
    let index = local
        .join(HAND)
        .join(format!("{:016x}", 0))
        .join("index.db");
    let db = rusqlite::Connection::open(&index).unwrap();
    db.execute_batch(
        "UPDATE symbols SET path = '/dev/zero'; UPDATE calls SET path = '../../../../etc/hostname';",
    )
    .unwrap();
    drop(db);
    let server = serve(&dir, &manifest);
    block_on(async {
        let client = server.client(&format!("/{WEB}")).await;
        let found = client
            .call_tool(call("search", serde_json::json!({"query": "dashboard"})))
            .await
            .unwrap();
        let found = text(&found).to_string();
        assert!(found.contains(" /dev/zero "), "{found}");
        assert!(!found.contains("/dev/zero:"), "{found}");
        let symbol = serde_json::json!({"symbol": "dashboard"});
        let context = client.call_tool(call("context", symbol)).await.unwrap();
        let context = text(&context).to_string();
        assert!(!context.contains("/dev/zero:"), "{context}");
        assert!(!context.contains("hostname:"), "{context}");
        client.cancel().await.unwrap();
    });
}
