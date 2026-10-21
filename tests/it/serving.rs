//! Serving a pulled generation (ADR 0012, spec #259, feature `snapshot`): `meada mcp --listen
//! --workspace <node manifest> --generations`, `meada status --generations` and `meada export
//! --generations` over `local_dir`s two nodes pulled from the local S3-compatible server
//! `MEADA_TEST_S3_ENDPOINT` names. Without it these tests print why and skip; with
//! `MEADA_TEST_SERVICES_REQUIRED=1` they fail.
#![cfg(feature = "snapshot")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::common::assert_golden;
use crate::manifest::{CHAIN, MANIFEST, contents, run_args};
use crate::server::{Server, call, get, text};
use crate::snapshot::{
    HOSTILE_SCHEMAS, KEY_ID, SECRET, block_on, bucket, bucket_name, crafted_index, named_node,
    next_commit, publish, pull, service, store_manifest,
};

const WEB: &str = "example.com/chain/web";
const CATALOG: &str = "example.com/chain/catalog";

/// `meada mcp --listen 127.0.0.1:0 --workspace <manifest> --generations`, in `dir`.
fn serve(dir: &Path, manifest: &str) -> Server {
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
fn probe(dir: &Path, manifest: &str) -> (String, String, Option<i32>) {
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
    for (manifest, local) in &nodes {
        let before = contents(local);
        let server = serve(&dir, manifest);
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
            let (export, stderr, code) = run_args(&dir, &args);
            assert_eq!((code, stderr.as_str()), (Some(0), ""), "{manifest} {repo}");
            assert_golden(&format!("{path}.export.json"), &export);
        }
        let (status, stderr, code) = probe(&dir, manifest);
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
