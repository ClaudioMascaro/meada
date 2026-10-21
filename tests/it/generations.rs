//! The generation pointer and pins (ADR 0012, spec #259, feature `generation-pointer`):
//! `meada snapshot publish` records the current and retained generations in the local Valkey
//! `MEADA_TEST_VALKEY_URL` names, `meada snapshot pull` mirrors them from the local
//! S3-compatible server `MEADA_TEST_S3_ENDPOINT` names, and two servers serve
//! `/<identity>@<generation>` from a held generation while `/<identity>` serves the current one.
//! Without either variable these tests print why and skip; with `MEADA_TEST_SERVICES_REQUIRED=1`
//! they fail.
#![cfg(feature = "generation-pointer")]

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::assert_golden;
use crate::manifest::{MANIFEST, chain_manifest, contents, run};
use crate::server::{Server, call, text, tokens};
use crate::serving::{probe, serve};
use crate::snapshot::{
    SECRET, append_section, block_on, bucket, bucket_name, named_node, next_commit, objects,
    publish, pull, service, store_manifest,
};

const WEB: &str = "example.com/chain/web";

/// The local Valkey, or `None` with the reason printed.
fn valkey_service() -> Option<String> {
    let url = std::env::var("MEADA_TEST_VALKEY_URL").ok();
    if url.is_none() {
        let required = std::env::var("MEADA_TEST_SERVICES_REQUIRED").is_ok_and(|v| v == "1");
        assert!(!required, "MEADA_TEST_VALKEY_URL is not set");
        eprintln!("skipped: MEADA_TEST_VALKEY_URL is not set");
    }
    url
}

/// The `[valkey]` and `[generations]` sections of a case: its own key prefix.
fn sections(valkey: &str, case: &str, keep: u64) -> String {
    format!(
        "\n[valkey]\nurl = \"{valkey}\"\nkey_prefix = \"{}\"\n\n[generations]\nkeep = {keep}\n",
        prefix(case)
    )
}

fn prefix(case: &str) -> String {
    format!("meada:test-{}-{case}", std::process::id())
}

/// A connection to the local Valkey, as the tests see it.
fn admin(url: &str) -> redis::Connection {
    redis::Client::open(url).unwrap().get_connection().unwrap()
}

/// What Valkey holds for `case`: the pointer hash and the retained list.
fn pointer(valkey: &mut redis::Connection, case: &str) -> (Vec<String>, Vec<String>) {
    let prefix = prefix(case);
    let hash: Vec<String> = redis::cmd("HMGET")
        .arg(format!("{{{prefix}}}:current"))
        .arg("id")
        .arg("fence")
        .query(valkey)
        .unwrap();
    let list: Vec<String> = redis::cmd("LRANGE")
        .arg(format!("{{{prefix}}}:retained"))
        .arg(0)
        .arg(-1)
        .query(valkey)
        .unwrap();
    (hash, list)
}

/// A store, a Valkey and the chain's manifest with `[generations]`, indexed.
struct Fleet {
    endpoint: String,
    bucket: String,
    valkey: String,
    dir: PathBuf,
    case: &'static str,
    keep: u64,
}

impl Fleet {
    fn new(case: &'static str, keep: u64) -> Option<Self> {
        let endpoint = service()?;
        let valkey = valkey_service()?;
        let name = bucket_name(case);
        bucket(&endpoint, &name);
        let dir = store_manifest(case, &endpoint, &name, SECRET);
        append_section(&dir, &sections(&valkey, case, keep));
        Some(Self {
            endpoint,
            bucket: name,
            valkey,
            dir,
            case,
            keep,
        })
    }

    /// A node of this fleet, with its own `[valkey]` and `[generations]`.
    fn node(&self, name: &str) -> (String, PathBuf) {
        let (manifest, local) = named_node(&self.dir, name, &self.endpoint, &self.bucket);
        let file = self.dir.join(&manifest);
        let toml =
            fs::read_to_string(&file).unwrap() + &sections(&self.valkey, self.case, self.keep);
        fs::write(&file, toml).unwrap();
        (manifest, local)
    }

    /// `meada snapshot publish`: the generation id.
    fn publish(&self) -> String {
        let (stdout, stderr, code) = publish(&self.dir, MANIFEST);
        assert_eq!(code, Some(0), "{stderr}");
        stdout.trim().to_string()
    }

    /// `meada snapshot pull` on `node`: the id it printed.
    fn pull(&self, node: &str) -> String {
        let (stdout, stderr, code) = pull(&self.dir, node, &[]);
        assert_eq!(code, Some(0), "{stderr}");
        stdout.trim().to_string()
    }
}

/// The generation ids `local` holds, sorted.
fn held(local: &Path) -> Vec<String> {
    let mut ids: Vec<String> = (fs::read_dir(local).unwrap())
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.len() == 16 && !name.contains('.'))
        .collect();
    ids.sort();
    ids
}

fn sorted(ids: &[&str]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    ids.sort();
    ids
}

/// An HTTP agent that reads 4xx and 5xx answers instead of failing on them.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
}

/// The `Meada-Generation` header of a response.
fn generation(response: &ureq::http::Response<ureq::Body>) -> Option<String> {
    let value = response.headers().get("meada-generation")?;
    Some(value.to_str().unwrap().to_string())
}

/// `GET url`: status, `Content-Type`, `Meada-Generation` and body.
fn get(url: &str) -> (u16, String, Option<String>, String) {
    let mut response = agent().get(url).call().unwrap();
    let kind = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_string();
    let named = generation(&response);
    let body = response.body_mut().read_to_string().unwrap();
    (response.status().as_u16(), kind, named, body)
}

/// A `trace` from `dashboard` POSTed alone to `url`: status, `Meada-Generation` and the body.
fn post_trace(url: &str) -> (u16, Option<String>, serde_json::Value) {
    let message = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "trace", "arguments": {"from": "dashboard"}}
    });
    let mut response = agent()
        .post(url)
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .send(message.to_string())
        .unwrap();
    let named = generation(&response);
    let body = response.body_mut().read_to_string().unwrap();
    (
        response.status().as_u16(),
        named,
        serde_json::from_str(&body).unwrap(),
    )
}

/// The `_meta` generation of a tool result.
fn meta(result: &rmcp::model::CallToolResult) -> Option<String> {
    let meta = result.meta.as_ref()?;
    Some(meta.get("meada/generation")?.as_str()?.to_string())
}

/// What `server` answers at `/WEB@<pin>` and at `/WEB`: a pinned `trace` equals the chain's
/// golden for `pin`, and `/WEB` answers from `current`, each naming its generation in the
/// header and in `_meta`; the tool list on the pinned URL stays within its budget.
fn answers(server: &Server, pin: &str, current: &str) {
    block_on(async {
        let client = server.client(&format!("/{WEB}@{pin}")).await;
        let tools = client.list_all_tools().await.unwrap();
        let mut names: Vec<&str> = tools.iter().map(|t| &*t.name).collect();
        names.sort();
        assert_eq!(names, ["context", "search", "trace"]);
        assert!(tokens(&tools) <= 1_000, "{} tokens", tokens(&tools));
        let from = serde_json::json!({"from": "dashboard"});
        let trace = client.call_tool(call("trace", from)).await.unwrap();
        assert_golden("chain.trace-down.txt", text(&trace));
        assert_eq!(meta(&trace).as_deref(), Some(pin));
        let second = serde_json::json!({"query": "second"});
        let old = client
            .call_tool(call("search", second.clone()))
            .await
            .unwrap();
        assert!(!text(&old).contains("src/status.ts"), "{}", text(&old));
        client.cancel().await.unwrap();

        let client = server.client(&format!("/{WEB}")).await;
        let new = client.call_tool(call("search", second)).await.unwrap();
        assert!(text(&new).contains("src/status.ts"), "{}", text(&new));
        assert_eq!(meta(&new).as_deref(), Some(current));
        // A tool error names its generation too.
        let uncommitted = serde_json::json!({"changes": "uncommitted"});
        let refused = client.call_tool(call("trace", uncommitted)).await.unwrap();
        assert_eq!(refused.is_error, Some(true));
        assert_eq!(meta(&refused).as_deref(), Some(current));
        client.cancel().await.unwrap();
    });
    let golden = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/chain.trace-down.txt"),
    )
    .unwrap();
    for (path, id) in [
        (format!("/{WEB}@{pin}"), pin),
        (format!("/{WEB}/"), current),
    ] {
        let (status, named, json) = post_trace(&format!("{}{path}", server.url));
        assert_eq!(
            (status, named.as_deref()),
            (200, Some(id)),
            "{path}: {json}"
        );
        assert_eq!(json["result"]["_meta"]["meada/generation"], id, "{json}");
        if id == pin {
            assert_eq!(json["result"]["content"][0]["text"], golden.as_str());
        }
    }
    // `/` still lists the identities, naming the current generation; an identity a held pin
    // does not record lists that generation's.
    let (status, _, named, body) = get(&server.url);
    assert_eq!((status, named.as_deref()), (404, Some(current)));
    assert!(body.starts_with(&format!("{WEB}\n")), "{body}");
    let (status, _, named, body) = get(&format!("{}/example.com/chain/none@{pin}", server.url));
    assert_eq!((status, named.as_deref()), (404, Some(pin)));
    assert!(body.starts_with(&format!("{WEB}\n")), "{body}");
}

/// Publish G1, pull, publish G2, pull: a pinned `trace` on G1 still answers G1's golden on both
/// servers while `/<identity>` answers G2; after `keep` more publishes G1 is gone and its pin is
/// the 404 naming it, the current generation and the held ones.
#[test]
fn a_pin_answers_its_generation_on_every_node_until_it_is_no_longer_retained() {
    let Some(fleet) = Fleet::new("generations_pins", 2) else {
        return;
    };
    let mut valkey = admin(&fleet.valkey);
    let g1 = fleet.publish();
    // The pointer is in Valkey, and the store has no `current`.
    assert_eq!(
        pointer(&mut valkey, fleet.case),
        (vec![g1.clone(), "0".into()], vec![format!("{g1}:0")])
    );
    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let keys: Vec<String> = (objects(&s3, "prod/meada").iter())
        .map(|o| o.location.to_string())
        .collect();
    assert!(!keys.iter().any(|k| k.ends_with("/current")), "{keys:?}");

    let nodes = [fleet.node("one"), fleet.node("two")];
    for (node, local) in &nodes {
        assert_eq!(fleet.pull(node), g1);
        assert_eq!(
            fs::read_to_string(local.join("current")).unwrap(),
            format!("{g1} 0\n")
        );
    }
    next_commit(&fleet.dir, "chain-web", "second");
    let g2 = fleet.publish();
    let list = vec![format!("{g2}:0"), format!("{g1}:0")];
    assert_eq!(
        pointer(&mut valkey, fleet.case),
        (vec![g2.clone(), "0".into()], list.clone())
    );
    // A republish of the current generation leaves the list as it was: each id once.
    assert_eq!(fleet.publish(), g2);
    assert_eq!(pointer(&mut valkey, fleet.case).1, list);
    for (node, local) in &nodes {
        assert_eq!(fleet.pull(node), g2);
        assert_eq!(held(local), sorted(&[&g1, &g2]));
    }
    // A node that joins now holds every retained generation.
    let (late, late_local) = fleet.node("late");
    assert_eq!(fleet.pull(&late), g2);
    assert_eq!(held(&late_local), sorted(&[&g1, &g2]));

    let servers: Vec<Server> = nodes
        .iter()
        .map(|(node, _)| serve(&fleet.dir, node))
        .collect();
    for server in &servers {
        answers(server, &g1, &g2);
    }

    let mut last = g2.clone();
    for line in ["third", "fourth"] {
        next_commit(&fleet.dir, "chain-web", line);
        last = fleet.publish();
        for (node, _) in &nodes {
            assert_eq!(fleet.pull(node), last);
        }
    }
    for ((_, local), server) in nodes.iter().zip(&servers) {
        let holds = held(local);
        assert_eq!(holds.len(), 2, "{holds:?}");
        assert!(!holds.contains(&g1) && holds.contains(&last), "{holds:?}");
        let (status, kind, named, body) = get(&format!("{}/{WEB}@{g1}", server.url));
        assert_eq!((status, named.as_deref()), (404, Some(last.as_str())));
        assert!(kind.starts_with("text/plain"), "{kind}");
        assert_eq!(
            body,
            format!(
                "generation {g1} is not held here\ncurrent: {last}\nheld: {}\n",
                holds.join(" ")
            )
        );
    }
}

/// `args` sent to Valkey as one command.
fn send(valkey: &mut redis::Connection, args: &[&str]) {
    let mut command = redis::cmd(args[0]);
    for arg in &args[1..] {
        command.arg(*arg);
    }
    command.exec(valkey).unwrap();
}

/// A malformed pointer or list entry in Valkey stops the pull with an error naming the key, and
/// the node stays as it was; so does a missing pointer or an empty list. The node's pointer file
/// carries the fence read with the id.
#[test]
fn every_id_and_fence_read_from_valkey_is_checked_before_the_node_changes() {
    let Some(fleet) = Fleet::new("generations_hostile", 2) else {
        return;
    };
    let g1 = fleet.publish();
    let (node, local) = fleet.node("node");
    assert_eq!(fleet.pull(&node), g1);
    let mut valkey = admin(&fleet.valkey);
    let prefix = prefix(fleet.case);
    let current = format!("{{{prefix}}}:current");
    let retained = format!("{{{prefix}}}:retained");
    let (c, r) = (current.as_str(), retained.as_str());
    let upper = g1.to_uppercase();
    let (bare, bad_fence) = (g1.clone(), format!("{g1}:x"));
    let pointer = "decimal fence";
    let entry = "<generation id>:<fence>";
    let cases: [(&str, Vec<&str>, &str, &str); 11] = [
        (
            "a path in the id",
            vec!["HSET", c, "id", "../../../../etc/"],
            c,
            pointer,
        ),
        ("an uppercase id", vec!["HSET", c, "id", &upper], c, pointer),
        (
            "a negative fence",
            vec!["HSET", c, "fence", "-1"],
            c,
            pointer,
        ),
        (
            "a fence in exponent form",
            vec!["HSET", c, "fence", "1e3"],
            c,
            pointer,
        ),
        ("a missing fence", vec!["HDEL", c, "fence"], c, pointer),
        (
            "a pointer that is no hash",
            vec!["SET", c, &g1],
            c,
            "WRONGTYPE",
        ),
        ("a missing pointer", vec!["UNLINK", c], c, "not found"),
        ("an empty list", vec!["UNLINK", r], r, "empty"),
        ("an entry with no fence", vec!["RPUSH", r, &bare], r, entry),
        (
            "an entry with a path for its id",
            vec!["RPUSH", r, "../../..:0"],
            r,
            entry,
        ),
        (
            "an entry whose fence is no number",
            vec!["RPUSH", r, &bad_fence],
            r,
            entry,
        ),
    ];
    let reset = |valkey: &mut redis::Connection| {
        send(valkey, &["DEL", c, r]);
        send(valkey, &["HSET", c, "id", &g1, "fence", "0"]);
        send(valkey, &["RPUSH", r, &format!("{g1}:0")]);
    };
    for (case, edit, key, why) in cases {
        reset(&mut valkey);
        if edit[0] == "SET" {
            send(&mut valkey, &["DEL", c]);
        }
        send(&mut valkey, &edit);
        let before = contents(&local);
        let (stdout, stderr, code) = pull(&fleet.dir, &node, &[]);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{case}: {stderr}");
        assert!(
            stderr.contains(&format!("meada: {key}: ")),
            "{case}: {stderr}"
        );
        assert!(stderr.contains(why), "{case}: {stderr}");
        assert!(contents(&local) == before, "{case}: the node changed");
    }

    // The fence read with the id reaches the pointer file.
    reset(&mut valkey);
    send(&mut valkey, &["HSET", c, "fence", "7"]);
    assert_eq!(fleet.pull(&node), g1);
    let pointer = fs::read_to_string(local.join("current")).unwrap();
    assert_eq!(pointer, format!("{g1} 7\n"));
}

/// `[generations]` needs `[snapshot]` and `[valkey]`, and keeps at least two generations.
#[test]
fn generations_section_errors_name_the_file_the_line_and_the_sections() {
    let dir = chain_manifest("generations_section");
    let at = "meada: ops/meada.workspace.toml";
    let snapshot = "[snapshot]\nendpoint = \"http://127.0.0.1:9\"\nregion = \"us-east-1\"\n\
                    bucket = \"meada-x\"\npath_style = true\n";
    let valkey = "[valkey]\nurl = \"redis://127.0.0.1\"\n";
    for (sections, expected) in [
        (
            "[generations]\n".to_string(),
            format!("{at}:3: [generations] needs [snapshot] and [valkey]\n"),
        ),
        (
            format!("{snapshot}[generations]\nkeep = 2\n"),
            format!("{at}:8: [generations] needs [valkey]\n"),
        ),
        (
            format!("{valkey}[generations]\n"),
            format!("{at}:5: [generations] needs [snapshot]\n"),
        ),
        (
            format!("{snapshot}{valkey}[generations]\nkeep = 1\n"),
            format!("{at}:11: `generations.keep`: must be at least 2\n"),
        ),
    ] {
        let toml = format!("index_dir = \"../indexes\"\n\n{sections}");
        fs::write(dir.join(MANIFEST), toml).unwrap();
        let (stdout, stderr, ok) = run(&dir, "status");
        assert!(!ok && stdout.is_empty(), "{sections}");
        assert_eq!(stderr, expected, "{sections}");
    }
}

/// A retained generation that cannot be pulled (here its document is gone from the store) never
/// blocks the current one: the pull swaps, warns naming it and exits 0, the node serves the
/// current generation, the pin of the missing one is the 404, and the next pull tries again.
#[test]
fn a_retained_generation_that_fails_to_pull_does_not_block_the_current_one() {
    let Some(fleet) = Fleet::new("generations_broken", 2) else {
        return;
    };
    let g1 = fleet.publish();
    next_commit(&fleet.dir, "chain-web", "second");
    let g2 = fleet.publish();
    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let document = format!("prod/meada/generations/{g1}.json");
    let key = object_store::path::Path::from(document.as_str());
    block_on(object_store::ObjectStoreExt::delete(&s3, &key)).unwrap();

    let (node, local) = fleet.node("fresh");
    let warning = format!("meada: retained generation {g1} not pulled, the next pull retries: ");
    for _ in 0..2 {
        let (stdout, stderr, code) = pull(&fleet.dir, &node, &[]);
        assert_eq!((code, stdout.trim()), (Some(0), g2.as_str()), "{stderr}");
        assert!(stderr.starts_with(&warning), "{stderr}");
        assert!(stderr.contains(&document), "{stderr}");
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
        assert_eq!(held(&local), std::slice::from_ref(&g2));
        let pointer = fs::read_to_string(local.join("current")).unwrap();
        assert_eq!(pointer, format!("{g2} 0\n"));
    }
    let server = serve(&fleet.dir, &node);
    block_on(async {
        let client = server.client(&format!("/{WEB}")).await;
        let query = serde_json::json!({"query": "second"});
        let found = client.call_tool(call("search", query)).await.unwrap();
        assert!(text(&found).contains("src/status.ts"), "{}", text(&found));
        assert_eq!(meta(&found).as_deref(), Some(g2.as_str()));
        client.cancel().await.unwrap();
    });
    let (status, _, named, body) = get(&format!("{}/{WEB}@{g1}", server.url));
    assert_eq!((status, named.as_deref()), (404, Some(g2.as_str())));
    let missing = format!("generation {g1} is not held here\n");
    assert!(body.starts_with(&missing), "{body}");
}

/// `meada status --generations` with `[generations]` reads the current id from Valkey and checks
/// that the store holds its document; a dead Valkey or a dead store is named, and the exit code
/// depends only on what the node holds.
#[test]
fn the_status_probe_reads_the_current_generation_from_valkey() {
    let Some(fleet) = Fleet::new("generations_status", 2) else {
        return;
    };
    let g1 = fleet.publish();
    let (node, _) = fleet.node("node");
    assert_eq!(fleet.pull(&node), g1);
    let (stdout, stderr, code) = probe(&fleet.dir, &node);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
    let reachable = format!("current: {g1}\nheld: {g1}\nstore: reachable (current {g1})\n");
    assert_eq!(stdout, reachable);
    let file = fleet.dir.join(&node);
    let toml = fs::read_to_string(&file).unwrap();
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap().port();
    drop(dead);
    let cases = [
        (
            &fleet.valkey,
            format!("redis://127.0.0.1:{port}"),
            "valkey: ",
        ),
        (
            &fleet.endpoint,
            format!("http://127.0.0.1:{port}"),
            "store: ",
        ),
    ];
    for (from, to, which) in cases {
        let broken = toml.replace(from.as_str(), &to);
        assert_ne!(broken, toml);
        fs::write(&file, broken).unwrap();
        let (stdout, stderr, code) = probe(&fleet.dir, &node);
        assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
        assert!(stdout.starts_with(&format!("current: {g1}\n")), "{stdout}");
        let line = format!("store: unreachable ({which}");
        assert!(stdout.contains(&line), "{stdout}");
    }
}
