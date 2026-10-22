//! The playground (`POST /api/v1/query`) and the exports (`GET
//! /api/v1/repositories/<identity>/export`) of the console (#305), over the chain fixture: from
//! the indexes under `index_dir` without `[snapshot]`, and from the generations a `meada snapshot
//! pull` keeps in the console's own `local_dir` with it. Every answer equals the chain goldens a
//! node and `meada export` are held to.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{PUBLIC, Reply, VERSION, front_end, get, send_body, ui_section};
use crate::common::assert_golden;
use crate::manifest::{CHAIN, MANIFEST, chain_manifest, contents, run};
use crate::server::Server;

pub(super) const WEB: &str = "example.com/chain/web";
pub(super) const GATEWAY: &str = "example.com/chain/gateway";
pub(super) const CATALOG: &str = "example.com/chain/catalog";

/// The text of a node's tool error for `trace` with `changes: uncommitted`.
const NO_WORKING_TREE: &str =
    "server mode has no working tree to diff: run Meada locally for `changes: uncommitted`";

/// The chain's mirrors in `<case>/`, listed by `ops/meada.workspace.toml` with `head` and `[ui]`
/// after them, and the front end in `<case>/ui`; not indexed. Returns the case directory.
pub(super) fn chain_console(case: &str, head: &str, ui: &str) -> PathBuf {
    let dir = chain_manifest(case);
    let _ = fs::remove_dir_all(dir.join("ui"));
    front_end(&dir, Some(VERSION));
    append(&dir.join(MANIFEST), &format!("{head}\n[ui]\n{ui}"));
    dir
}

fn append(file: &Path, text: &str) {
    let toml = fs::read_to_string(file).unwrap() + text;
    fs::write(file, toml).unwrap();
}

/// `POST /api/v1/query` with `body`, from the console's own origin and with `headers`.
pub(super) fn query(console: &Server, body: &Value, headers: &[(&str, &str)]) -> Reply {
    let mut headers = headers.to_vec();
    headers.push(("Origin", PUBLIC));
    send_body(
        console,
        "POST",
        "/api/v1/query",
        &headers,
        &body.to_string(),
    )
}

/// The three tools' calls compared with the chain goldens: the repository, the tool, its
/// arguments and the golden.
pub(super) const CALLS: [(&str, &str, &str, &str); 3] = [
    (
        WEB,
        "trace",
        r#"{"from": "dashboard"}"#,
        "chain.trace-down.txt",
    ),
    (
        CATALOG,
        "context",
        r#"{"symbol": "lib/products.ts::find"}"#,
        "chain.context-processes.txt",
    ),
    (
        GATEWAY,
        "search",
        r#"{"query": "lib", "kind": "community"}"#,
        "chain.search-community.txt",
    ),
];

/// The body of a query of `tool` with `arguments` (JSON text) about `repository`.
pub(super) fn body(repository: &str, tool: &str, arguments: &str) -> Value {
    let arguments: Value = serde_json::from_str(arguments).unwrap();
    json!({"repository": repository, "tool": tool, "arguments": arguments})
}

/// `search`, `context` and `trace` through the playground, and each repository's export, equal
/// the chain goldens, asked at `generation` (the current one when `None`); each answer names
/// `answered`. `headers` go with every request.
pub(super) fn answer_the_goldens(
    console: &Server,
    generation: Option<&str>,
    answered: Option<&str>,
    headers: &[(&str, &str)],
) {
    for (repository, tool, arguments, golden) in CALLS {
        let mut asked = body(repository, tool, arguments);
        if let Some(id) = generation {
            asked["generation"] = json!(id);
        }
        let reply = query(console, &asked, headers);
        assert_eq!(reply.status, 200, "{tool}: {}", reply.body);
        assert_eq!(reply.header("content-type"), Some("application/json"));
        reply.assert_no_cookie_or_cors();
        let answer = reply.json();
        assert_eq!(answer["generation"], json!(answered), "{tool}");
        assert_golden(golden, answer["text"].as_str().unwrap());
    }
    for (repository, path) in CHAIN {
        let at = generation.map_or(String::new(), |id| format!("?generation={id}"));
        let route = format!("/api/v1/repositories/{repository}/export{at}");
        let reply = super::send(console, "GET", &route, headers);
        assert_eq!(reply.status, 200, "{repository}: {}", reply.body);
        assert_eq!(reply.header("content-type"), Some("application/json"));
        reply.assert_no_cookie_or_cors();
        assert_golden(&format!("{path}.export.json"), &reply.body);
    }
}

/// The error a reply carries: its status, code and message.
pub(super) fn refusal(reply: &Reply) -> (u16, String, String) {
    let error = &reply.json()["error"];
    let text = |key: &str| error[key].as_str().unwrap_or_default().to_string();
    (reply.status, text("code"), text("message"))
}

/// Without `[snapshot]`: started before `meada index --workspace`, the two routes name it and
/// every other view answers; after it, with no restart, the playground and the exports equal the
/// chain goldens with `generation` null, and `index_dir` is byte-identical after them.
#[test]
fn answers_from_index_dir_as_a_node_and_meada_export_do() {
    let dir = chain_console("ui_playground_listed", "", &ui_section());
    let console = super::start(&dir, "127.0.0.1:0");

    let (repository, tool, arguments, _) = CALLS[0];
    let early = query(&console, &body(repository, tool, arguments), &[]);
    let (status, code, message) = refusal(&early);
    assert_eq!((status, code.as_str()), (503, "unavailable"), "{message}");
    assert!(message.contains("meada index --workspace"), "{message}");
    let export = get(&console, &format!("/api/v1/repositories/{WEB}/export"));
    let (status, _, message) = refusal(&export);
    assert_eq!(status, 503);
    assert!(message.contains("meada index --workspace"), "{message}");
    assert_eq!(get(&console, "/api/v1/config").status, 200);
    assert_eq!(get(&console, "/api/v1/manifest").status, 200);

    assert!(run(&dir, "index").2, "index");
    let before = contents(&dir.join("indexes"));
    answer_the_goldens(&console, None, None, &[]);
    refuses_what_it_does_not_answer(&console, &[]);
    assert_eq!(contents(&dir.join("indexes")), before, "index_dir changed");
}

/// What every console refuses: `trace` with `changes: uncommitted` with the node's text, an
/// unknown repository and an unheld generation with a 404, a body over the cap with a 413, a
/// malformed body or an unknown tool with a fixed 400, and a query without the console's own
/// `Origin` with a 403.
pub(super) fn refuses_what_it_does_not_answer(console: &Server, headers: &[(&str, &str)]) {
    let uncommitted = query(
        console,
        &body(WEB, "trace", r#"{"changes": "uncommitted"}"#),
        headers,
    );
    assert_eq!(
        refusal(&uncommitted),
        (400, "bad_request".into(), NO_WORKING_TREE.into())
    );

    let unknown = "example.com/chain/nowhere";
    let not_served = format!("`{unknown}` is not a repository this console serves");
    let reply = query(
        console,
        &body(unknown, "search", r#"{"query": "x"}"#),
        headers,
    );
    assert_eq!(
        refusal(&reply),
        (404, "not_found".into(), not_served.clone())
    );
    let route = format!("/api/v1/repositories/{unknown}/export");
    let reply = super::send(console, "GET", &route, headers);
    assert_eq!(refusal(&reply), (404, "not_found".into(), not_served));

    let mut unheld = body(WEB, "search", r#"{"query": "x"}"#);
    unheld["generation"] = json!("0000000000000000");
    let reply = query(console, &unheld, headers);
    let (status, code, message) = refusal(&reply);
    assert_eq!((status, code.as_str()), (404, "not_found"));
    assert!(
        message.starts_with("generation 0000000000000000 is not held here; held: "),
        "{message}"
    );

    let fixed = "the body must be {\"repository\", \"generation\", \"tool\", \"arguments\"}, the \
                 tool search, context or trace and the arguments its own";
    let big = json!({"repository": WEB, "tool": "search",
                     "arguments": {"query": "x".repeat(17 << 10)}});
    let reply = query(console, &big, headers);
    assert_eq!(
        refusal(&reply),
        (
            413,
            "too_large".into(),
            "the body is over 16384 bytes".into()
        )
    );
    for malformed in [
        "{".to_string(),
        body(WEB, "grep", r#"{"query": "x"}"#).to_string(),
        body(WEB, "search", r#"{"symbol": "x"}"#).to_string(),
        json!({"repository": WEB, "tool": "search", "arguments": {"query": "x"}, "extra": 1})
            .to_string(),
    ] {
        let mut sent = headers.to_vec();
        sent.push(("Origin", PUBLIC));
        let reply = send_body(console, "POST", "/api/v1/query", &sent, &malformed);
        assert_eq!(
            refusal(&reply),
            (400, "bad_request".into(), fixed.into()),
            "{malformed}"
        );
    }

    let asked = body(WEB, "search", r#"{"query": "x"}"#).to_string();
    let reply = send_body(console, "POST", "/api/v1/query", headers, &asked);
    assert_eq!(
        (reply.status, reply.body.as_str()),
        (403, "Forbidden: a write needs an Origin header\n")
    );
    let foreign = [headers, &[("Origin", "http://evil.example")]].concat();
    let reply = send_body(console, "POST", "/api/v1/query", &foreign, &asked);
    assert_eq!(reply.status, 403);
}

/// With `[snapshot]`: started before any pull, the two routes name `meada snapshot pull` and
/// every other view answers; after an init pull, with no restart, they equal the chain goldens
/// with `generation` the current id; after the next pull swaps, the next query answers the new
/// generation, a retained older one answers when named, and `local_dir` is byte-identical after
/// every query and export. Skips without the store, as every cloud module's tests.
#[cfg(feature = "snapshot")]
#[test]
fn answers_from_the_generations_its_own_pull_holds() {
    use crate::snapshot::{
        KEY_ID, SECRET, bucket, bucket_name, named_node, next_commit, publish, pull, service,
        store_manifest,
    };
    let Some(endpoint) = service() else { return };
    let name = bucket_name("ui-playground");
    bucket(&endpoint, &name);
    let dir = store_manifest("ui_playground_store", &endpoint, &name, SECRET);
    let (first, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let first = first.trim().to_string();
    // The console's bootstrap manifest is a node's, with `[ui]`; its front end beside it.
    let (manifest, local) = named_node(&dir, "console", &endpoint, &name);
    let ui = format!("dir = \"ui\"\npublic_url = \"{PUBLIC}\"\n");
    append(&dir.join(&manifest), &format!("\n[ui]\n{ui}"));
    front_end(&dir.join("console"), Some(VERSION));
    let args = ["--listen", "127.0.0.1:0", "--workspace", manifest.as_str()];
    let console = Server::start_command(&dir, "ui", &args, &[("MEADA_TEST_S3_KEY", KEY_ID)]);

    let (repository, tool, arguments, _) = CALLS[0];
    let early = query(&console, &body(repository, tool, arguments), &[]);
    let (status, code, message) = refusal(&early);
    assert_eq!((status, code.as_str()), (503, "unavailable"), "{message}");
    assert!(message.contains("meada snapshot pull"), "{message}");
    let export = get(&console, &format!("/api/v1/repositories/{WEB}/export"));
    let (status, _, message) = refusal(&export);
    assert_eq!(status, 503);
    assert!(message.contains("meada snapshot pull"), "{message}");
    assert_eq!(get(&console, "/api/v1/config").status, 200);
    assert_eq!(get(&console, "/api/v1/manifest").status, 200);

    assert_eq!(pull(&dir, &manifest, &[]).2, Some(0));
    let before = contents(&local);
    answer_the_goldens(&console, None, Some(&first), &[]);
    answer_the_goldens(&console, Some(&first), Some(&first), &[]);
    refuses_what_it_does_not_answer(&console, &[]);
    assert_eq!(contents(&local), before, "local_dir changed");

    // The next generation, swapped in by the next pull: the next query answers it.
    next_commit(&dir, "chain-web", "second");
    let (second, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let second = second.trim().to_string();
    assert_eq!(pull(&dir, &manifest, &[]).0.trim(), second);
    let before = contents(&local);
    let search = body(WEB, "search", r#"{"query": "second"}"#);
    let answer = query(&console, &search, &[]).json();
    assert_eq!(answer["generation"], json!(second));
    assert!(
        answer["text"].as_str().unwrap().contains("src/status.ts"),
        "{answer}"
    );
    let mut older = search.clone();
    older["generation"] = json!(first);
    let answer = query(&console, &older, &[]).json();
    assert_eq!(answer["generation"], json!(first));
    assert!(
        !answer["text"].as_str().unwrap().contains("src/status.ts"),
        "{answer}"
    );
    // The retained one answers the goldens when named.
    answer_the_goldens(&console, Some(&first), Some(&first), &[]);

    let mut held = [first.clone(), second.clone()];
    held.sort();
    let mut unheld = search;
    unheld["generation"] = json!("0000000000000000");
    let (status, _, message) = refusal(&query(&console, &unheld, &[]));
    assert_eq!(status, 404);
    assert_eq!(
        message,
        format!(
            "generation 0000000000000000 is not held here; held: {}",
            held.join(" ")
        )
    );
    assert_eq!(contents(&local), before, "local_dir changed");
}
