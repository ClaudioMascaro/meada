//! Serving a pulled generation (ADR 0012, spec #259, feature `snapshot`): `meada mcp --listen
//! --workspace <node manifest> --generations`, `meada status --generations` and `meada export
//! --generations` over `local_dir`s two nodes pulled from the local S3-compatible server
//! `MEADA_TEST_S3_ENDPOINT` names. Without it these tests print why and skip; with
//! `MEADA_TEST_SERVICES_REQUIRED=1` they fail.
#![cfg(feature = "snapshot")]

use std::path::{Path, PathBuf};

use crate::common::assert_golden;
use crate::manifest::{CHAIN, MANIFEST, contents, run_args};
use crate::server::{Server, call, text};
use crate::snapshot::{
    SECRET, block_on, bucket, bucket_name, named_node, publish, pull, service, store_manifest,
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

/// `meada status --workspace <manifest> --generations` in `dir`.
fn probe(dir: &Path, manifest: &str) -> (String, String, Option<i32>) {
    run_args(dir, &["status", "--workspace", manifest, "--generations"])
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
