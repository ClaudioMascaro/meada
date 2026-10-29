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
pub(crate) fn valkey_service() -> Option<String> {
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

pub(crate) fn prefix(case: &str) -> String {
    format!("meada:test-{}-{case}", std::process::id())
}

/// A connection to the local Valkey, as the tests see it.
pub(crate) fn admin(url: &str) -> redis::Connection {
    redis::Client::open(url).unwrap().get_connection().unwrap()
}

/// What Valkey holds for `case`: the pointer hash and the retained list.
pub(crate) fn pointer(valkey: &mut redis::Connection, case: &str) -> (Vec<String>, Vec<String>) {
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
pub(crate) struct Fleet {
    pub(crate) endpoint: String,
    pub(crate) bucket: String,
    pub(crate) valkey: String,
    pub(crate) dir: PathBuf,
    pub(crate) case: &'static str,
    pub(crate) keep: u64,
}

impl Fleet {
    pub(crate) fn new(case: &'static str, keep: u64) -> Option<Self> {
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
    pub(crate) fn node(&self, name: &str) -> (String, PathBuf) {
        let (manifest, local) = named_node(&self.dir, name, &self.endpoint, &self.bucket);
        let file = self.dir.join(&manifest);
        let toml =
            fs::read_to_string(&file).unwrap() + &sections(&self.valkey, self.case, self.keep);
        fs::write(&file, toml).unwrap();
        (manifest, local)
    }

    /// `meada snapshot publish`: the generation id.
    pub(crate) fn publish(&self) -> String {
        let (stdout, stderr, code) = publish(&self.dir, MANIFEST);
        assert_eq!(code, Some(0), "{stderr}");
        stdout.trim().to_string()
    }

    /// `meada snapshot pull` on `node`: the id it printed.
    pub(crate) fn pull(&self, node: &str) -> String {
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

pub(crate) fn sorted(ids: &[&str]) -> Vec<String> {
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
pub(crate) fn get(url: &str) -> (u16, String, Option<String>, String) {
    get_with(url, &[])
}

/// `get` with more `headers`.
fn get_with(url: &str, headers: &[(&str, &str)]) -> (u16, String, Option<String>, String) {
    let mut request = agent().get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request.call().unwrap();
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
pub(crate) fn meta(result: &rmcp::model::CallToolResult) -> Option<String> {
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
        // Without `[oidc]`, a browser's request (DNS rebinding) gets 403 before routing: no
        // pin's ids, no listing.
        let origin = [("Origin", "http://evil.example")];
        for path in [format!("/{WEB}@{g1}"), format!("/{WEB}@{last}"), "/".into()] {
            let (status, _, _, body) = get_with(&format!("{}{path}", server.url), &origin);
            assert_eq!(status, 403, "{path}");
            assert!(
                !body.contains(&last) && !body.contains(WEB),
                "{path}: {body}"
            );
        }
    }
}

/// `args` sent to Valkey as one command.
pub(crate) fn send(valkey: &mut redis::Connection, args: &[&str]) {
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
    // `keep` plus the kept maximum is 10: with G1's entry, 11.
    let many: Vec<String> = (0..10).map(|n| format!("{n:016x}:0")).collect();
    let many = many.iter().map(String::as_str);
    let too_long: Vec<&str> = ["RPUSH", r].into_iter().chain(many).collect();
    let cases: [(&str, Vec<&str>, &str, &str); 12] = [
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
        ("a list too long", too_long, r, "holds more than 10 entries"),
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

    // The retained-list ceiling has the same refusal in the Brazilian Portuguese catalog.
    reset(&mut valkey);
    let many: Vec<String> = (0..10).map(|n| format!("{n:016x}:0")).collect();
    let mut append = vec!["RPUSH", r];
    append.extend(many.iter().map(String::as_str));
    send(&mut valkey, &append);
    let before = contents(&local);
    let (stdout, stderr, code) = meada_in(
        &fleet.dir,
        "pt_BR.UTF-8",
        &["snapshot", "pull", "--workspace", &node],
    );
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        format!(
            "meada: {r}: tem mais de 10 entradas, `keep` mais as 8 gerações mantidas permitidas\n"
        )
    );
    assert!(contents(&local) == before);

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

/// `meada snapshot pointer set` (ADR 0014) under the refresh lock: G1 and G2 published, two
/// nodes on G2; a generation outside the retained list and a malformed id are refused, then
/// `set G1` gives the pointer a fence above G2's, both nodes swap back on their next pull and
/// `/<identity>` answers G1's goldens, until the next publish moves the pointer past it.
#[cfg(feature = "refresh-lock")]
#[test]
fn pointer_set_rolls_every_node_back_until_the_next_publish() {
    use crate::refresh_lock::{fleet, get, key, locked_publish, released, run_in};
    let Some(fleet) = fleet("generations_rollback") else {
        return;
    };
    let dir = &fleet.dir;
    let mut valkey = admin(&fleet.valkey);
    let g1 = locked_publish(dir);
    next_commit(dir, "chain-web", "second");
    let g2 = locked_publish(dir);
    let nodes = [fleet.node("one"), fleet.node("two")];
    for (node, _) in &nodes {
        assert_eq!(fleet.pull(node), g2);
    }
    let set = |id: &str| {
        let args = ["snapshot", "pointer", "set", id, "--workspace", MANIFEST];
        run_in(dir, "en_US.UTF-8", &args)
    };
    let before = pointer(&mut valkey, fleet.case);
    assert_eq!(before.0, [g2.clone(), "2".into()]);

    // Outside the retained list: refused naming it, after taking (and releasing) the lock.
    let retained = key(&fleet, "retained");
    let refused = format!(
        "meada: {retained}: generation 0000000000000000 is not retained: the pointer did not \
         move\n"
    );
    let (stdout, stderr, code) = set("0000000000000000");
    assert_eq!((code, stdout.as_str(), stderr), (Some(1), "", refused));
    assert_eq!(get(&fleet, "fence").as_deref(), Some("3"));
    // Malformed: refused before the lock is taken, so the fence did not move either.
    for bad in [
        g1.to_uppercase(),
        "../../../../etc/".into(),
        g1[..15].into(),
    ] {
        let (stdout, stderr, code) = set(&bad);
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{bad}");
        let refused = format!("meada: {bad}: not a generation id of 16 lowercase hex characters\n");
        assert_eq!(stderr, refused);
    }
    assert_eq!(get(&fleet, "fence").as_deref(), Some("3"));
    assert_eq!(pointer(&mut valkey, fleet.case), before);
    assert!(released(&fleet));

    let (stdout, stderr, code) = set(&g1);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout, format!("{g1} 4\n"));
    let list = vec![format!("{g1}:4"), format!("{g2}:2")];
    assert_eq!(
        pointer(&mut valkey, fleet.case),
        (vec![g1.clone(), "4".into()], list)
    );
    assert!(released(&fleet));
    for (node, local) in &nodes {
        assert_eq!(fleet.pull(node), g1);
        let file = fs::read_to_string(local.join("current")).unwrap();
        assert_eq!(file, format!("{g1} 4\n"));
    }
    let server = serve(dir, &nodes[0].0);
    block_on(async {
        let client = server.client(&format!("/{WEB}")).await;
        let from = serde_json::json!({"from": "dashboard"});
        let trace = client.call_tool(call("trace", from)).await.unwrap();
        assert_golden("chain.trace-down.txt", text(&trace));
        assert_eq!(meta(&trace).as_deref(), Some(g1.as_str()));
        let second = serde_json::json!({"query": "second"});
        let found = client.call_tool(call("search", second)).await.unwrap();
        assert!(!text(&found).contains("src/status.ts"), "{}", text(&found));
        client.cancel().await.unwrap();
    });

    next_commit(dir, "chain-web", "third");
    let g3 = locked_publish(dir);
    assert_eq!(pointer(&mut valkey, fleet.case).0, [g3.clone(), "5".into()]);
    for (node, _) in &nodes {
        assert_eq!(fleet.pull(node), g3);
    }
}

/// `meada snapshot pointer retain` and `drop` (ADR 0014): two nodes, `keep = 2`; G1 retained,
/// then three more publishes: both nodes still hold G1 and `/<identity>@G1` answers its goldens,
/// while G2, beyond `keep` and not kept, is gone. After `drop G1` the next pull deletes it and
/// its pin is the 404 naming it, the current generation and the held ones.
#[cfg(feature = "refresh-lock")]
#[test]
fn a_retained_generation_stays_on_every_node_until_it_is_dropped() {
    use crate::refresh_lock::{key, locked_publish, released, run_in};
    let Some(fleet) = Fleet::new("generations_kept", 2) else {
        return;
    };
    append_section(&fleet.dir, "\n[refresh_lock]\nttl_seconds = 10\n");
    let dir = &fleet.dir;
    let mut valkey = admin(&fleet.valkey);
    let pointer_op = |op: &str, id: &str| {
        let args = ["snapshot", "pointer", op, id, "--workspace", MANIFEST];
        run_in(dir, "en_US.UTF-8", &args)
    };
    let kept = || -> Vec<String> {
        let members = redis::cmd("SMEMBERS").arg(key(&fleet, "kept")).clone();
        members.query(&mut admin(&fleet.valkey)).unwrap()
    };
    let printed = |id: &str| (format!("{id}\n"), String::new(), Some(0));
    let g1 = locked_publish(dir);
    let nodes = [fleet.node("one"), fleet.node("two")];
    for (node, _) in &nodes {
        assert_eq!(fleet.pull(node), g1);
    }
    assert_eq!(pointer_op("retain", &g1), printed(&g1));
    assert!(released(&fleet));
    assert_eq!(kept(), std::slice::from_ref(&g1));
    // Already retained: only kept, its entry as it was.
    assert_eq!(pointer(&mut valkey, fleet.case).1, [format!("{g1}:1")]);

    let mut ids = vec![g1.clone()];
    for line in ["second", "third", "fourth"] {
        next_commit(dir, "chain-web", line);
        ids.push(locked_publish(dir));
        for (node, _) in &nodes {
            assert_eq!(fleet.pull(node), *ids.last().unwrap());
        }
    }
    let [_, g2, g3, g4] = &ids[..] else {
        panic!("{ids:?}")
    };
    // The fences: G1's publish 1, the retain 2, then 3, 4 and 5.
    let list = vec![format!("{g4}:5"), format!("{g3}:4"), format!("{g1}:1")];
    assert_eq!(pointer(&mut valkey, fleet.case).1, list);
    let servers: Vec<Server> = (nodes.iter()).map(|(node, _)| serve(dir, node)).collect();
    for ((_, local), server) in nodes.iter().zip(&servers) {
        assert_eq!(held(local), sorted(&[&g1, g3, g4]));
        assert!(!held(local).contains(g2));
        answers(server, &g1, g4);
    }

    assert_eq!(pointer_op("drop", &g1), printed(&g1));
    assert!(released(&fleet));
    assert!(kept().is_empty());
    assert_eq!(pointer(&mut valkey, fleet.case).1, list[..2].to_vec());
    let holds = sorted(&[g3, g4]);
    for ((node, local), server) in nodes.iter().zip(&servers) {
        assert_eq!(fleet.pull(node), *g4);
        assert_eq!(held(local), holds);
        let (status, _, named, body) = get(&format!("{}/{WEB}@{g1}", server.url));
        assert_eq!((status, named.as_deref()), (404, Some(g4.as_str())));
        assert_eq!(
            body,
            format!(
                "generation {g1} is not held here\ncurrent: {g4}\nheld: {}\n",
                holds.join(" ")
            )
        );
    }
}

/// `meada <args>` in `dir` under `lang`, with the store's key id: stdout, stderr and the exit
/// code.
pub(crate) fn meada_in(dir: &Path, lang: &str, args: &[&str]) -> (String, String, Option<i32>) {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(dir)
        .args(args)
        .env("LANG", lang)
        .env("MEADA_TEST_S3_KEY", crate::snapshot::KEY_ID)
        .env_remove("MEADA_TEST_S3_TOKEN")
        .output()
        .expect("meada runs");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

/// `meada workspace <args> --workspace ops/meada.workspace.toml` in `dir`, in English.
pub(crate) fn workspace(dir: &Path, args: &[&str]) -> (String, String, Option<i32>) {
    let args: Vec<&str> = (["workspace"].into_iter())
        .chain(args.iter().copied())
        .chain(["--workspace", MANIFEST])
        .collect();
    meada_in(dir, "en_US.UTF-8", &args)
}

/// `meada workspace import <file> --message <message> --author <author>`: the id it printed and
/// stderr.
pub(crate) fn import(dir: &Path, file: &str, message: &str, author: &str) -> (String, String) {
    let args = ["import", file, "--message", message, "--author", author];
    let (stdout, stderr, code) = workspace(dir, &args);
    assert_eq!(code, Some(0), "{stderr}");
    (stdout.trim().to_string(), stderr)
}

/// The store key of the workspace version `id` in a fleet's store.
pub(crate) fn version_key(id: &str) -> String {
    format!("prod/meada/workspaces/{id}.json")
}

/// `{<prefix>}:workspace` of `case`.
pub(crate) fn workspace_pointer(case: &str) -> String {
    format!("{{{}}}:workspace", prefix(case))
}

/// Whether `text` is a time as a version records it.
fn is_time(text: &str) -> bool {
    let shape = "dddd-dd-ddTdd:dd:ddZ";
    text.len() == shape.len()
        && (text.bytes().zip(shape.bytes())).all(|(b, s)| match s {
            b'd' => b.is_ascii_digit(),
            _ => b == s,
        })
}

/// `import` of the chain's manifest stores `workspaces/<id>.json` with every field and the
/// metadata of every object, and prints the id; its other sections are named as not imported.
/// The same entries again upload nothing and answer the first author and message; two entries
/// swapped are another version.
#[test]
fn import_stores_a_version_once_and_answers_its_id() {
    let Some(fleet) = Fleet::new("workspace_import", 3) else {
        return;
    };
    let dir = &fleet.dir;
    let (id, stderr) = import(dir, MANIFEST, "first", "alice");
    let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let not_imported = |key: &str, found: &str| {
        let line = crate::manifest::line_of(&text, found);
        format!(
            "meada: {MANIFEST}:{line}: `{key}`: not imported: a workspace version holds only \
             [[repository]] entries\n"
        )
    };
    let expected: String = [
        not_imported("generations", "[generations]"),
        not_imported("index_dir", "index_dir"),
        not_imported("snapshot", "[snapshot]"),
        not_imported("valkey", "[valkey]"),
    ]
    .concat();
    assert_eq!(stderr, expected);
    let entries: Vec<(&str, String)> = (crate::manifest::CHAIN.iter())
        .map(|(repo, path)| (*repo, format!("../{path}")))
        .collect();
    let blocks = serde_json::json!({});
    let hashed: Vec<(&str, &str, &serde_json::Value, Option<&str>)> = (entries.iter())
        .map(|(repo, path)| (*repo, path.as_str(), &blocks, None))
        .collect();
    assert_eq!(id, meada_core::ids::workspace_version_id(&hashed));
    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let key = version_key(&id);
    let bytes = crate::snapshot::get(&s3, &key);
    let (size, hash) = crate::snapshot::metadata(&s3, &key);
    assert_eq!(size, bytes.len() as u64);
    assert_eq!(hash, blake3::hash(&bytes).to_hex().to_string());
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let time = document["time"].as_str().unwrap().to_string();
    assert!(is_time(&time), "{time}");
    let repositories: Vec<serde_json::Value> = (entries.iter())
        .map(|(repo, path)| serde_json::json!({"repo": repo, "path": path}))
        .collect();
    let expected = serde_json::json!({
        "id": id,
        "author": "alice",
        "time": time,
        "message": "first",
        "repositories": repositories,
    });
    assert_eq!(document, expected);
    let stored = objects(&s3, "prod/meada/workspaces");
    assert_eq!(stored.len(), 1);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let (again, stderr) = import(dir, MANIFEST, "second", "bob");
    assert_eq!(again, id);
    let first = format!("meada: {id} is stored already, by alice at {time}: first\n");
    assert!(stderr.ends_with(&first), "{stderr}");
    assert_eq!(objects(&s3, "prod/meada/workspaces"), stored);
    assert_eq!(crate::snapshot::get(&s3, &key), bytes);
    let swapped = (text.replacen("chain/web\"", "chain/SWAP\"", 1))
        .replacen("chain-web\"", "chain-SWAP\"", 1)
        .replacen("chain/gateway\"", "chain/web\"", 1)
        .replacen("chain-gateway\"", "chain-web\"", 1)
        .replacen("chain/SWAP\"", "chain/gateway\"", 1)
        .replacen("chain-SWAP\"", "chain-gateway\"", 1);
    assert_ne!(swapped, text);
    fs::write(dir.join("ops/swapped.toml"), &swapped).unwrap();
    let (other, _) = import(dir, "ops/swapped.toml", "swapped", "alice");
    assert_ne!(other, id);
    assert_eq!(objects(&s3, "prod/meada/workspaces").len(), 2);
}

/// A bootstrap manifest whose store and Valkey listen nowhere, in `<dir>/ops/`.
fn unreachable_bootstrap(dir: &Path) {
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap().port();
    drop(dead);
    fs::write(dir.join("ops/s3-secret"), "secret\n").unwrap();
    let bootstrap = format!(
        "index_dir = \"../indexes\"\n{}\n[valkey]\nurl = \"redis://127.0.0.1:{port}\"\n\n\
         [generations]\nkeep = 3\n",
        crate::snapshot::section(&format!("http://127.0.0.1:{port}"), "nowhere")
    );
    fs::write(dir.join(MANIFEST), bootstrap).unwrap();
}

/// A file with a validation error: the error names the file, the line and the dotted key, its
/// other sections are named as not imported, and nothing is uploaded: the store and Valkey the
/// bootstrap manifest names listen nowhere, and no connection error comes.
#[test]
fn import_validates_before_any_connection() {
    let dir = chain_manifest("workspace_import_invalid");
    unreachable_bootstrap(&dir);
    let invalid = "index_dir = \"/srv\"\n\n[snapshot]\nbucket = \"x\"\n\n[[repository]]\n\
                   repo = \"example.com/chain/web\"\npath = \"../chain-web\"\nbranch = \"main\"\n";
    fs::write(dir.join("ops/invalid.toml"), invalid).unwrap();
    let args = ["import", "ops/invalid.toml", "--message", "m"];
    let (stdout, stderr, code) = workspace(&dir, &args);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        "meada: ops/invalid.toml:1: `index_dir`: not imported: a workspace version holds only \
         [[repository]] entries\n\
         meada: ops/invalid.toml:3: `snapshot`: not imported: a workspace version holds only \
         [[repository]] entries\n\
         meada: ops/invalid.toml:9: `repository.branch` is not a meada.workspace.toml key\n"
    );
    // An `owner` that breaks a rule is refused alike, in both languages.
    let padded = "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"w\"\nowner = \" t\"\n";
    fs::write(dir.join("ops/invalid.toml"), padded).unwrap();
    for (lang, what) in [
        ("en_US.UTF-8", "has whitespace at either end"),
        ("pt_BR.UTF-8", "tem espaço em branco numa das pontas"),
    ] {
        let args = ["workspace", "import", "ops/invalid.toml", "--message", "m"]
            .into_iter()
            .chain(["--workspace", MANIFEST])
            .collect::<Vec<_>>();
        let (stdout, stderr, code) = meada_in(&dir, lang, &args);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{lang}");
        let expected = format!("meada: ops/invalid.toml:4: `repository.owner`: {what}\n");
        assert_eq!(stderr, expected, "{lang}");
    }
    let local = "[[repository]]\nrepo = \"local/web\"\npath = \"w\"\n";
    fs::write(dir.join("ops/invalid.toml"), local).unwrap();
    let (stdout, stderr, code) = workspace(&dir, &args);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        "meada: ops/invalid.toml: local/web: a local identity: server mode serves repositories \
         by their remote\n"
    );
}

/// `text`'s `[[repository]]` entries with `owners[n]`, when it is `Some`, as the `owner` right
/// after the `path` of its n-th entry.
pub(crate) fn owned(text: &str, owners: &[Option<&str>]) -> String {
    let mut owners = owners.iter();
    (text.lines())
        .map(
            |line| match line.starts_with("path = ").then(|| owners.next()) {
                Some(Some(Some(owner))) => format!("{line}\nowner = \"{owner}\"\n"),
                _ => format!("{line}\n"),
            },
        )
        .collect()
}

/// An entry's `owner` is imported, stored, shown and pulled like its other keys: the document
/// holds it, `show` prints it right after `path` and importing that output gives the same id,
/// and `pull` writes the document as stored. A stored document whose owner breaks a rule (empty,
/// padded, with a control character, over 256 characters) is listed as malformed, refused by
/// `show` and by `pull`, which leaves the index directory's workspace state as it was.
#[test]
fn an_owner_is_stored_shown_and_pulled_and_a_malformed_one_is_refused() {
    let Some(fleet) = Fleet::new("workspace_owner", 3) else {
        return;
    };
    let dir = &fleet.dir;
    let key = workspace_pointer(fleet.case);
    let mut valkey = admin(&fleet.valkey);
    let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let owners = [
        Some("team-web"),
        None,
        Some("Equipe de Cobrança, SP"),
        Some("a b,c"),
    ];
    fs::write(dir.join("ops/owned.toml"), owned(&text, &owners)).unwrap();
    let (id, _) = import(dir, "ops/owned.toml", "owned", "alice");
    let (plain, _) = import(dir, MANIFEST, "plain", "alice");
    assert_ne!(id, plain, "an owner changes the version id");

    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let stored = crate::snapshot::get(&s3, &version_key(&id));
    let document: serde_json::Value = serde_json::from_slice(&stored).unwrap();
    let entries = document["repositories"].as_array().unwrap();
    let stored_owners: Vec<Option<&str>> = entries.iter().map(|e| e["owner"].as_str()).collect();
    assert_eq!(stored_owners, owners);

    let (shown, stderr, code) = workspace(dir, &["show", &id]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    let after_path = "path = \"../chain-web\"\nowner = \"team-web\"\nrepo =";
    assert!(shown.contains(after_path), "{shown}");
    assert_eq!(shown.matches("owner = ").count(), 3, "{shown}");
    fs::write(dir.join("ops/shown.toml"), &shown).unwrap();
    let (again, _) = import(dir, "ops/shown.toml", "again", "bob");
    assert_eq!(again, id);

    send(&mut valkey, &["HSET", &key, "id", &id, "fence", "1"]);
    let (stdout, stderr, code) = workspace(dir, &["pull"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout, format!("{id}\n"));
    let pulled = fs::read(dir.join(format!("indexes/workspace/{id}.json"))).unwrap();
    assert_eq!(pulled, stored);
    let before = pulled_files(dir);

    // Each forged document carries the id its entries derive, so only the owner check refuses it.
    let long = "g".repeat(257);
    let bad = ["", " team", "team ", "te\u{7}am", long.as_str()];
    let blocks = serde_json::json!({});
    let mut forged = Vec::new();
    for owner in bad {
        let mut other = document.clone();
        other["repositories"][0]["owner"] = owner.into();
        let hashed: Vec<(&str, &str, &serde_json::Value, Option<&str>)> =
            (other["repositories"].as_array().unwrap().iter())
                .map(|e| {
                    let (repo, path) = (e["repo"].as_str().unwrap(), e["path"].as_str().unwrap());
                    (repo, path, &blocks, e["owner"].as_str())
                })
                .collect();
        let id = meada_core::ids::workspace_version_id(&hashed);
        other["id"] = id.clone().into();
        let bytes = serde_json::to_vec_pretty(&other).unwrap();
        crate::snapshot::put_stored(&s3, &version_key(&id), &bytes);
        forged.push(id);
    }
    let not_version = |id: &str| {
        let key = version_key(id);
        format!("{key}: malformed: not the workspace version document of its id")
    };
    let (listed, stderr, code) = workspace(dir, &["list"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    for id in &forged {
        assert!(
            listed.contains(&format!("! {}\n", not_version(id))),
            "{listed}"
        );
    }
    for id in &forged {
        let (stdout, stderr, code) = workspace(dir, &["show", id]);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{id}");
        assert_eq!(stderr, format!("meada: {}\n", not_version(id)));
        send(&mut valkey, &["HSET", &key, "id", id, "fence", "5"]);
        let (stdout, stderr, code) = workspace(dir, &["pull"]);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{id}");
        assert_eq!(stderr, format!("meada: {}\n", not_version(id)));
        assert_eq!(pulled_files(dir), before, "{id}");
    }
}

/// `list` shows the versions newest first, with no version current until
/// `{<key_prefix>}:workspace` names one; a malformed pointer refuses the listing naming its key,
/// and a corrupted or forged document is listed as malformed.
#[test]
fn list_marks_the_current_version_and_names_what_is_malformed() {
    let Some(fleet) = Fleet::new("workspace_list", 3) else {
        return;
    };
    let dir = &fleet.dir;
    let (v1, _) = import(dir, MANIFEST, "one", "alice");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let fewer = "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n";
    fs::write(dir.join("ops/fewer.toml"), fewer).unwrap();
    let (v2, _) = import(dir, "ops/fewer.toml", "two\nlines", "bob");
    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let time = |id: &str| {
        let document = crate::snapshot::get(&s3, &version_key(id));
        let document: serde_json::Value = serde_json::from_slice(&document).unwrap();
        document["time"].as_str().unwrap().to_string()
    };
    let (t1, t2) = (time(&v1), time(&v2));
    let key = workspace_pointer(fleet.case);
    let (stdout, stderr, code) = workspace(dir, &["list"]);
    assert_eq!(code, Some(0), "{stderr}");
    let both = |mark: char| format!("  {v2} {t2} bob: two\\nlines\n{mark} {v1} {t1} alice: one\n");
    assert_eq!(stdout, both(' '));
    let none = format!(
        "meada: {key}: no workspace version is current: apply one with `meada workspace apply`\n"
    );
    assert_eq!(stderr, none);
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["HSET", &key, "id", &v1, "fence", "1"]);
    let (stdout, stderr, code) = workspace(dir, &["list"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout, both('*'));
    send(&mut valkey, &["HSET", &key, "id", "../escape"]);
    let (stdout, stderr, code) = workspace(dir, &["list"]);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        format!(
            "meada: {key}: not a hash of `id`, a workspace version id of 16 lowercase hex \
             characters, and `fence`, a decimal fence\n"
        )
    );
    send(&mut valkey, &["HSET", &key, "id", &v1]);
    let corrupted = version_key("000000000000000a");
    crate::snapshot::put(&s3, &corrupted, b"{}");
    let forged = version_key("000000000000000b");
    crate::snapshot::put_stored(&s3, &forged, &crate::snapshot::get(&s3, &version_key(&v1)));
    let (stdout, stderr, code) = workspace(dir, &["list"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(
        stdout,
        both('*')
            + &format!(
                "! {corrupted}: not stored with the size and hash it records\n\
                 ! {forged}: malformed: not the workspace version document of its id\n"
            )
    );
}

/// `show <id>` prints the entries in the `[[repository]]` shape, blocks included, and importing
/// that output gives the same id; with no current version `show` names `meada workspace apply`,
/// and with one it prints that one. A malformed id is refused first.
#[test]
fn show_prints_the_entries_import_reads_back() {
    let Some(fleet) = Fleet::new("workspace_show", 3) else {
        return;
    };
    let dir = &fleet.dir;
    let key = workspace_pointer(fleet.case);
    let (stdout, stderr, code) = workspace(dir, &["show"]);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        format!(
            "meada: {key}: no workspace version is current: apply one with `meada workspace \
             apply`\n"
        )
    );
    let (stdout, stderr, code) = workspace(dir, &["show", "../0000000000000"]);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        "meada: ../0000000000000: not a workspace version id of 16 lowercase hex characters\n"
    );
    let blocks = "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n\
                  [repository.serve]\n\
                  [[repository.peer]]\nrepo = \"example.com/chain/gateway\"\n\
                  base_url_env = [\"GATEWAY_URL\"]\n\n\
                  [[repository]]\nrepo = \"example.com/chain/gateway\"\n\
                  path = \"/srv/chain-gateway\"\n\
                  [repository.serve]\nprefix = \"/api/\"\nhosts = [\"api.example.com\"]\n\
                  [[repository.resource]]\nkind = \"postgres\"\nid = \"orders\"\n\
                  role = \"read-write\"\nvia = [\"src/db.ts::query\"]\n";
    fs::write(dir.join("ops/blocks.toml"), blocks).unwrap();
    let (id, _) = import(dir, "ops/blocks.toml", "blocks", "alice");
    let (shown, stderr, code) = workspace(dir, &["show", &id]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(
        shown,
        "[[repository]]\npath = \"../chain-web\"\nrepo = \"example.com/chain/web\"\n\n\
         [[repository.peer]]\nbase_url_env = [\"GATEWAY_URL\"]\n\
         repo = \"example.com/chain/gateway\"\n\n\
         [repository.serve]\n\n\
         [[repository]]\npath = \"/srv/chain-gateway\"\nrepo = \"example.com/chain/gateway\"\n\n\
         [[repository.resource]]\nid = \"orders\"\nkind = \"postgres\"\nrole = \"read-write\"\n\
         via = [\"src/db.ts::query\"]\n\n\
         [repository.serve]\nhosts = [\"api.example.com\"]\nprefix = \"/api/\"\n"
    );
    fs::write(dir.join("ops/shown.toml"), &shown).unwrap();
    let (again, stderr) = import(dir, "ops/shown.toml", "again", "bob");
    assert_eq!(again, id, "{stderr}");
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["HSET", &key, "id", &id, "fence", "1"]);
    let (current, stderr, code) = workspace(dir, &["show"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(current, shown);
}

/// With `[workspace] source = "store"` and no version pulled, `index`, `status`, `export` and
/// `snapshot publish` refuse naming `meada workspace pull`, `mcp --listen` without
/// `--generations` refuses naming `--generations`, and a node's `--generations` commands serve
/// as before.
#[test]
fn a_store_source_without_a_pulled_version_is_refused_and_nodes_serve_as_before() {
    let Some(fleet) = Fleet::new("workspace_store_source", 3) else {
        return;
    };
    let dir = &fleet.dir;
    let g1 = fleet.publish();
    let (node, _) = fleet.node("node");
    assert_eq!(fleet.pull(&node), g1);
    let manifest = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let sections = &manifest[manifest.find("\n[snapshot]").unwrap()..];
    let bootstrap =
        format!("index_dir = \"../indexes\"\n{sections}\n[workspace]\nsource = \"store\"\n");
    fs::write(dir.join("ops/bootstrap.toml"), bootstrap).unwrap();
    let not_pulled = "meada: ops/bootstrap.toml: [workspace] source = \"store\": no workspace \
                      version pulled in ops/../indexes/workspace: run `meada workspace pull` \
                      first\n";
    let generations = "meada: ops/bootstrap.toml: [workspace] source = \"store\": a managed \
                       workspace's nodes serve generations: run meada mcp --listen with \
                       --generations\n";
    let bootstrap = ["--workspace", "ops/bootstrap.toml"];
    let commands: [(&[&str], &str); 5] = [
        (&["index"], not_pulled),
        (&["status"], not_pulled),
        (&["export", "--repo", WEB], not_pulled),
        (&["snapshot", "publish"], not_pulled),
        (&["mcp", "--listen", "127.0.0.1:0"], generations),
    ];
    for (command, refused) in commands {
        let args = [command, &bootstrap].concat();
        let (stdout, stderr, code) = meada_in(dir, "en_US.UTF-8", &args);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{args:?}: {stderr}");
        assert_eq!(stderr, refused, "{args:?}");
    }
    let file = dir.join(&node);
    let toml = fs::read_to_string(&file).unwrap() + "\n[workspace]\nsource = \"store\"\n";
    fs::write(&file, toml).unwrap();
    let (stdout, stderr, code) = probe(dir, &node);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
    let export = [
        "export",
        "--workspace",
        &node,
        "--repo",
        WEB,
        "--generations",
    ];
    let (stdout, stderr, code) = meada_in(dir, "en_US.UTF-8", &export);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains(WEB), "{stdout}");
    let server = serve(dir, &node);
    block_on(async {
        let client = server.client(&format!("/{WEB}")).await;
        let query = serde_json::json!({"query": "health"});
        let found = client.call_tool(call("search", query)).await.unwrap();
        assert!(text(&found).contains("src/status.ts"), "{}", text(&found));
        client.cancel().await.unwrap();
    });
}

// The heartbeat (#298): each round of `meada snapshot pull --every` writes the node's state.

/// `meada snapshot pull --every 1` on `manifest` in `dir`, a companion that is killed when
/// dropped.
pub(crate) struct Companion(std::process::Child);

impl Companion {
    pub(crate) fn start(dir: &Path, manifest: &str, env: &[(&str, &str)]) -> Self {
        let mut command = crate::snapshot::pull_command(dir, manifest, &["--every", "1"]);
        command.envs(env.iter().copied());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        Self(command.spawn().expect("meada runs"))
    }

    /// Kills it: what it wrote on stdout and on stderr.
    pub(crate) fn stop(mut self) -> (String, String) {
        use std::io::Read;
        let _ = self.0.kill();
        let _ = self.0.wait();
        let (mut stdout, mut stderr) = (String::new(), String::new());
        let _ = self.0.stdout.take().unwrap().read_to_string(&mut stdout);
        let _ = self.0.stderr.take().unwrap().read_to_string(&mut stderr);
        (stdout, stderr)
    }
}

impl Drop for Companion {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The key of node `name` in `case`'s prefix.
fn node_key(case: &str, name: &str) -> String {
    format!("{{{}}}:node:{name}", prefix(case))
}

/// The node set of `case`, `{<key_prefix>}:nodes`.
fn nodes_key(case: &str) -> String {
    format!("{{{}}}:nodes", prefix(case))
}

/// The heartbeat document of node `name`, `None` while there is none.
fn beat(valkey: &mut redis::Connection, case: &str, name: &str) -> Option<serde_json::Value> {
    let text: Option<String> = redis::cmd("GET")
        .arg(node_key(case, name))
        .query(valkey)
        .unwrap();
    text.map(|t| serde_json::from_str(&t).unwrap())
}

/// The first heartbeat of `name` that `wanted` accepts, polled every 20 ms for 30 s.
fn wait_beat(
    valkey: &mut redis::Connection,
    case: &str,
    name: &str,
    wanted: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    for _ in 0..1500 {
        if let Some(beat) = beat(valkey, case, name).filter(&wanted) {
            return beat;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("no heartbeat of {name} that was wanted");
}

/// Sets `[snapshot] node_name` of the node at `manifest` (relative to the fleet's directory).
fn name_node(fleet: &Fleet, manifest: &str, name: &str) {
    let file = fleet.dir.join(manifest);
    let toml = fs::read_to_string(&file).unwrap();
    let marker = "local_dir = \"generations\"\n";
    let toml = toml.replace(marker, &format!("{marker}node_name = \"{name}\"\n"));
    fs::write(&file, toml).unwrap();
}

/// Valkey's clock in Unix seconds.
fn server_time(valkey: &mut redis::Connection) -> u64 {
    let (seconds, _): (u64, u64) = redis::cmd("TIME").query(valkey).unwrap();
    seconds
}

/// Whether `time` is an RFC 3339 UTC instant to the second.
fn is_rfc3339_utc(time: &str) -> bool {
    let shape = b"dddd-dd-ddTdd:dd:ddZ";
    time.len() == shape.len()
        && time.bytes().zip(shape).all(|(b, s)| match s {
            b'd' => b.is_ascii_digit(),
            s => b == *s,
        })
}

/// A round writes `{<key_prefix>}:node:<name>` with the node pointer file, the held
/// directories, the readiness `status --generations` reports and the binary's version; the name
/// is in `{<key_prefix>}:nodes` scored by the key's expiry in Unix seconds; the TTL is three
/// periods. A round that swaps writes the new generation and its fence.
#[test]
fn every_round_of_pull_every_writes_the_nodes_heartbeat() {
    let Some(fleet) = Fleet::new("heartbeat_fields", 3) else {
        return;
    };
    let mut valkey = admin(&fleet.valkey);
    let g1 = fleet.publish();
    let (node, local) = fleet.node("one");
    let name = "meada-node-0";
    name_node(&fleet, &node, name);

    let companion = Companion::start(&fleet.dir, &node, &[]);
    let first = wait_beat(&mut valkey, fleet.case, name, |_| true);
    // Right after the write: the TTL is three periods, and the score is the expiry.
    let ttl: i64 = (redis::cmd("PTTL"))
        .arg(node_key(fleet.case, name))
        .query(&mut valkey)
        .unwrap();
    assert!((2_000..=3_000).contains(&ttl), "ttl {ttl} ms");
    let score: u64 = (redis::cmd("ZSCORE"))
        .arg(nodes_key(fleet.case))
        .arg(name)
        .query(&mut valkey)
        .unwrap();
    let now = server_time(&mut valkey);
    assert!(
        (now + 1..=now + 3).contains(&score),
        "score {score} at {now}"
    );
    let members: Vec<String> = (redis::cmd("ZRANGE"))
        .arg(nodes_key(fleet.case))
        .arg(0)
        .arg(-1)
        .query(&mut valkey)
        .unwrap();
    assert_eq!(members, [name]);

    assert_eq!(
        fs::read_to_string(local.join("current")).unwrap(),
        format!("{g1} 0\n")
    );
    let mut fields: Vec<&String> = first.as_object().unwrap().keys().collect();
    fields.sort();
    assert_eq!(
        fields,
        [
            "fence",
            "generation",
            "held",
            "name",
            "ready",
            "time",
            "version"
        ]
    );
    assert_eq!(first["name"], name);
    assert_eq!(first["generation"], g1.as_str());
    assert_eq!(first["fence"], 0);
    assert_eq!(first["held"], serde_json::json!([g1]));
    assert_eq!(first["version"], env!("CARGO_PKG_VERSION"));
    assert!(is_rfc3339_utc(first["time"].as_str().unwrap()), "{first}");
    // The readiness is the one `status --generations` exits on.
    let (_, stderr, code) = probe(&fleet.dir, &node);
    assert_eq!(
        (code, first["ready"].as_bool()),
        (Some(0), Some(true)),
        "{stderr}"
    );
    companion.stop();

    // A swap writes the new generation and fence: the pointer gets fence 5.
    next_commit(&fleet.dir, "chain-web", "second");
    let g2 = fleet.publish();
    let prefix = prefix(fleet.case);
    redis::cmd("HSET")
        .arg(format!("{{{prefix}}}:current"))
        .arg("fence")
        .arg(5)
        .exec(&mut valkey)
        .unwrap();
    redis::cmd("LSET")
        .arg(format!("{{{prefix}}}:retained"))
        .arg(0)
        .arg(format!("{g2}:5"))
        .exec(&mut valkey)
        .unwrap();
    let companion = Companion::start(&fleet.dir, &node, &[]);
    let second = wait_beat(&mut valkey, fleet.case, name, |b| {
        b["generation"] == g2.as_str()
    });
    companion.stop();
    assert_eq!(second["fence"], 5);
    assert_eq!(second["held"], serde_json::json!(sorted(&[&g1, &g2])));
    assert_eq!(
        fs::read_to_string(local.join("current")).unwrap(),
        format!("{g2} 5\n")
    );
}

/// Once the companion stops the key expires; the next heartbeat another node writes removes the
/// stale name from the node set.
#[test]
fn a_stopped_node_expires_and_the_next_heartbeat_removes_its_name() {
    let Some(fleet) = Fleet::new("heartbeat_expiry", 3) else {
        return;
    };
    let mut valkey = admin(&fleet.valkey);
    fleet.publish();
    let (one, _) = fleet.node("one");
    let (two, _) = fleet.node("two");
    name_node(&fleet, &one, "node-one");
    name_node(&fleet, &two, "node-two");

    let companion = Companion::start(&fleet.dir, &one, &[]);
    wait_beat(&mut valkey, fleet.case, "node-one", |_| true);
    companion.stop();
    for _ in 0..200 {
        if beat(&mut valkey, fleet.case, "node-one").is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        beat(&mut valkey, fleet.case, "node-one").is_none(),
        "expired"
    );
    // The name outlives the key until another heartbeat cleans the set.
    let members = |valkey: &mut redis::Connection| -> Vec<String> {
        let mut members: Vec<String> = redis::cmd("ZRANGE")
            .arg(nodes_key(fleet.case))
            .arg(0)
            .arg(-1)
            .query(valkey)
            .unwrap();
        members.sort();
        members
    };
    assert_eq!(members(&mut valkey), ["node-one"]);

    let companion = Companion::start(&fleet.dir, &two, &[]);
    wait_beat(&mut valkey, fleet.case, "node-two", |_| true);
    companion.stop();
    assert_eq!(members(&mut valkey), ["node-two"]);
}

/// A round that fails (a malformed pointer in Valkey) still writes the heartbeat of the node's
/// unchanged state.
#[test]
fn a_failed_round_still_writes_the_heartbeat() {
    let Some(fleet) = Fleet::new("heartbeat_failed", 3) else {
        return;
    };
    let mut valkey = admin(&fleet.valkey);
    let g1 = fleet.publish();
    let (node, local) = fleet.node("one");
    name_node(&fleet, &node, "node-one");
    assert_eq!(fleet.pull(&node), g1);

    let prefix = prefix(fleet.case);
    redis::cmd("HSET")
        .arg(format!("{{{prefix}}}:current"))
        .arg("fence")
        .arg("not-a-fence")
        .exec(&mut valkey)
        .unwrap();
    let companion = Companion::start(&fleet.dir, &node, &[]);
    // Rounds have failed for a while when the key is deleted and written again.
    std::thread::sleep(std::time::Duration::from_millis(2_500));
    redis::cmd("DEL")
        .arg(node_key(fleet.case, "node-one"))
        .exec(&mut valkey)
        .unwrap();
    let beat = wait_beat(&mut valkey, fleet.case, "node-one", |_| true);
    let (stdout, stderr) = companion.stop();
    assert_eq!(stdout, "");
    assert!(
        !stderr.is_empty() && !stderr.contains("heartbeat not written"),
        "{stderr}"
    );
    assert_eq!(beat["generation"], g1.as_str());
    assert_eq!(beat["fence"], 0);
    assert_eq!(beat["held"], serde_json::json!([g1]));
    assert_eq!(beat["ready"], true);
    assert_eq!(
        fs::read_to_string(local.join("current")).unwrap(),
        format!("{g1} 0\n")
    );
}

/// A single `pull` writes no heartbeat, and with `[valkey]` but no `[generations]` a `pull
/// --every` writes none either.
#[test]
fn only_pull_every_with_generations_writes_a_heartbeat() {
    let Some(fleet) = Fleet::new("heartbeat_none", 3) else {
        return;
    };
    let mut valkey = admin(&fleet.valkey);
    fleet.publish();
    let (node, _) = fleet.node("one");
    fleet.pull(&node);
    let written = |valkey: &mut redis::Connection| -> Vec<String> {
        redis::cmd("KEYS")
            .arg(format!("{{{}}}:node*", prefix(fleet.case)))
            .query(valkey)
            .unwrap()
    };
    assert_eq!(written(&mut valkey), Vec::<String>::new());

    // A store without Valkey's pointer, and a node with `[valkey]` only.
    let case = "heartbeat_plain";
    let name = bucket_name(case);
    bucket(&fleet.endpoint, &name);
    let dir = store_manifest(case, &fleet.endpoint, &name, SECRET);
    let (id, stderr, code) = publish(&dir, MANIFEST);
    assert_eq!(code, Some(0), "{stderr}");
    let (plain, local) = named_node(&dir, "plain", &fleet.endpoint, &name);
    let file = dir.join(&plain);
    let toml = fs::read_to_string(&file).unwrap()
        + &format!(
            "\n[valkey]\nurl = \"{}\"\nkey_prefix = \"{}\"\n",
            fleet.valkey,
            prefix(fleet.case)
        );
    fs::write(&file, toml).unwrap();
    let companion = Companion::start(&dir, &plain, &[]);
    for _ in 0..200 {
        if local.join("current").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // Two more rounds.
    std::thread::sleep(std::time::Duration::from_millis(2_500));
    let (stdout, stderr) = companion.stop();
    assert_eq!((stdout.trim(), stderr.as_str()), (id.trim(), ""));
    assert_eq!(written(&mut valkey), Vec::<String>::new());
}

/// With Valkey unreachable each round prints one warning naming the heartbeat and no password;
/// the pointer file, stdout and `status --generations`' exit code are what they were.
#[test]
fn an_unreachable_valkey_costs_one_warning_per_round_and_nothing_else() {
    let Some(fleet) = Fleet::new("heartbeat_outage", 3) else {
        return;
    };
    let g1 = fleet.publish();
    let (node, local) = fleet.node("one");
    name_node(&fleet, &node, "node-one");
    assert_eq!(fleet.pull(&node), g1);
    let before = (
        fs::read(local.join("current")).unwrap(),
        probe(&fleet.dir, &node).2,
    );

    let file = fleet.dir.join(&node);
    let toml = fs::read_to_string(&file).unwrap();
    let toml = toml.replace(
        &format!("url = \"{}\"", fleet.valkey),
        "url = \"redis://127.0.0.1:1\"\npassword_env = \"MEADA_TEST_HEARTBEAT_PASSWORD\"",
    );
    fs::write(&file, toml).unwrap();
    let password = "hunter2-heartbeat";
    let companion = Companion::start(
        &fleet.dir,
        &node,
        &[("MEADA_TEST_HEARTBEAT_PASSWORD", password)],
    );
    std::thread::sleep(std::time::Duration::from_millis(3_500));
    let (stdout, stderr) = companion.stop();
    assert_eq!(stdout, "");
    // Each round also fails to read the pointer from the same Valkey: its error is the
    // pull's, and the heartbeat's is one line of its own.
    let beats: Vec<&str> = (stderr.lines())
        .filter(|l| l.starts_with("meada: heartbeat not written to "))
        .collect();
    assert!(beats.len() >= 2, "{stderr}");
    for line in &beats {
        assert!(line.contains("node-one"), "{line}");
    }
    assert!(!stderr.contains(password), "{stderr}");
    let after = (
        fs::read(local.join("current")).unwrap(),
        probe(&fleet.dir, &node).2,
    );
    assert_eq!(after, before);
}

/// Without `[snapshot] node_name` the heartbeat carries the host name in lowercase.
#[test]
fn the_host_name_in_lowercase_names_a_node_by_default() {
    let Some(fleet) = Fleet::new("heartbeat_host", 3) else {
        return;
    };
    let host = nix::unistd::gethostname().unwrap();
    let host = host.into_string().unwrap().to_lowercase();
    let pattern = |b: u8| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-');
    if host.is_empty() || host.len() > 63 || !host.bytes().all(pattern) {
        eprintln!("skipped: the host name {host:?} is no node name");
        return;
    }
    let mut valkey = admin(&fleet.valkey);
    fleet.publish();
    let (node, _) = fleet.node("one");
    let companion = Companion::start(&fleet.dir, &node, &[]);
    let beat = wait_beat(&mut valkey, fleet.case, &host, |_| true);
    companion.stop();
    assert_eq!(beat["name"], host.as_str());
}

// Workspace versions on the machine (#302): `meada workspace pull` and the commands that read
// the pulled version.

/// The bootstrap manifest the store-source tests write beside the fleet's.
const BOOTSTRAP: &str = "ops/bootstrap.toml";

/// `ops/bootstrap.toml` in `dir`: the fleet manifest's sections, `[workspace] source = "store"`
/// and `index_dir`, emptied of what a previous run left.
fn store_bootstrap(dir: &Path, index_dir: &str) {
    let _ = fs::remove_dir_all(dir.join("ops").join(index_dir));
    let manifest = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let sections = &manifest[manifest.find("\n[snapshot]").unwrap()..];
    let bootstrap =
        format!("index_dir = \"{index_dir}\"\n{sections}\n[workspace]\nsource = \"store\"\n");
    fs::write(dir.join(BOOTSTRAP), bootstrap).unwrap();
}

/// `meada <args> --workspace ops/bootstrap.toml` in `dir`, in English.
fn on_bootstrap(dir: &Path, args: &[&str]) -> (String, String, Option<i32>) {
    let args = [args, &["--workspace", BOOTSTRAP]].concat();
    meada_in(dir, "en_US.UTF-8", &args)
}

/// Every file in `<dir>/indexes/workspace/` with its bytes, sorted by name; none when the
/// directory does not exist.
fn pulled_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(read) = fs::read_dir(dir.join("indexes/workspace")) else {
        return Vec::new();
    };
    let mut files: Vec<(String, Vec<u8>)> = read
        .map(|e| {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().into_owned();
            (name, fs::read(e.path()).unwrap())
        })
        .collect();
    files.sort();
    files
}

/// `meada workspace pull` refuses with no current version; then it writes the document as
/// stored and the current file, says a second run is current and changes nothing, swaps after
/// the pointer moves, keeps only the current version and the one it replaced, and refuses a
/// pointer whose fence is not above the current file's, naming both and changing nothing.
#[test]
fn workspace_pull_swaps_the_current_file_and_never_lowers_its_fence() {
    let Some(fleet) = Fleet::new("workspace_pull", 3) else {
        return;
    };
    let dir = &fleet.dir;
    let key = workspace_pointer(fleet.case);
    let (stdout, stderr, code) = workspace(dir, &["pull"]);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        format!(
            "meada: {key}: no workspace version is current: apply one with `meada workspace \
             apply`\n"
        )
    );
    assert!(pulled_files(dir).is_empty());

    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let document = |id: &str| {
        let stored = crate::snapshot::get(&s3, &version_key(id));
        (format!("{id}.json"), stored)
    };
    let current = |id: &str, fence: u64| {
        let line = format!("{id} {fence}\n").into_bytes();
        ("current".to_string(), line)
    };
    let expected = |mut files: Vec<(String, Vec<u8>)>| {
        files.push(("pull.lock".to_string(), Vec::new()));
        files.sort();
        files
    };
    let mut valkey = admin(&fleet.valkey);
    let (v1, _) = import(dir, MANIFEST, "one", "alice");
    send(&mut valkey, &["HSET", &key, "id", &v1, "fence", "1"]);
    let (stdout, stderr, code) = workspace(dir, &["pull"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout, format!("{v1}\n"));
    let one = expected(vec![document(&v1), current(&v1, 1)]);
    assert_eq!(pulled_files(dir), one);

    let (stdout, stderr, code) = workspace(dir, &["pull"]);
    assert_eq!((code, stdout), (Some(0), format!("{v1}\n")));
    assert_eq!(
        stderr,
        format!("meada: {v1}: already the current workspace version: nothing changed\n")
    );
    assert_eq!(pulled_files(dir), one);

    let fewer = "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n";
    fs::write(dir.join("ops/fewer.toml"), fewer).unwrap();
    let (v2, _) = import(dir, "ops/fewer.toml", "two", "bob");
    send(&mut valkey, &["HSET", &key, "id", &v2, "fence", "2"]);
    let (stdout, stderr, code) = workspace(dir, &["pull"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout, format!("{v2}\n"));
    let two = expected(vec![document(&v1), document(&v2), current(&v2, 2)]);
    assert_eq!(pulled_files(dir), two);

    let gateway = "[[repository]]\nrepo = \"example.com/chain/gateway\"\n\
                   path = \"../chain-gateway\"\n";
    fs::write(dir.join("ops/gateway.toml"), gateway).unwrap();
    let (v3, _) = import(dir, "ops/gateway.toml", "three", "bob");
    send(&mut valkey, &["HSET", &key, "id", &v3, "fence", "3"]);
    let (stdout, stderr, code) = workspace(dir, &["pull"]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout, format!("{v3}\n"));
    let three = expected(vec![document(&v2), document(&v3), current(&v3, 3)]);
    assert_eq!(pulled_files(dir), three);

    for fence in ["2", "3"] {
        send(&mut valkey, &["HSET", &key, "id", &v1, "fence", fence]);
        let (stdout, stderr, code) = workspace(dir, &["pull"]);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{stderr}");
        let file = "ops/../indexes/workspace/current";
        let fence_key = format!("{{{}}}:fence", prefix(fleet.case));
        assert_eq!(
            stderr,
            format!(
                "meada: {key}: workspace version {v1} has fence {fence}, not above fence 3 in \
                 {file}: not swapped; raise {fence_key} with meada snapshot fence recover \
                 --at-least 3 and apply again, or delete {file}\n"
            )
        );
        assert_eq!(pulled_files(dir), three);
    }
}

/// Everything `meada workspace pull` reads is hostile: a malformed pointer id or fence, a
/// document of another id, one whose id does not derive from its entries, an oversized one and
/// one with invalid entries are each refused naming the key, leaving `<index_dir>/workspace/`
/// as it was.
#[test]
fn workspace_pull_refuses_hostile_input_and_changes_nothing() {
    let Some(fleet) = Fleet::new("workspace_pull_hostile", 3) else {
        return;
    };
    let dir = &fleet.dir;
    let key = workspace_pointer(fleet.case);
    let mut valkey = admin(&fleet.valkey);
    let (v1, _) = import(dir, MANIFEST, "one", "alice");
    send(&mut valkey, &["HSET", &key, "id", &v1, "fence", "1"]);
    assert_eq!(workspace(dir, &["pull"]).2, Some(0));
    let before = pulled_files(dir);

    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let stored = crate::snapshot::get(&s3, &version_key(&v1));
    let document: serde_json::Value = serde_json::from_slice(&stored).unwrap();
    let with_id = |id: &str, repositories: serde_json::Value| {
        let mut forged = document.clone();
        forged["id"] = id.into();
        forged["repositories"] = repositories;
        serde_json::to_vec_pretty(&forged).unwrap()
    };
    let other = "000000000000000b";
    crate::snapshot::put_stored(&s3, &version_key(other), &stored);
    let underived = "000000000000000c";
    let entries = document["repositories"].clone();
    let underived_document = with_id(underived, entries);
    crate::snapshot::put_stored(&s3, &version_key(underived), &underived_document);
    let oversized = "000000000000000d";
    let spaces = vec![b' '; (4 << 20) + 1];
    crate::snapshot::put_stored(&s3, &version_key(oversized), &spaces);
    let invalid = "000000000000000e";
    let local = serde_json::json!([{"repo": "local/web", "path": "w"}]);
    crate::snapshot::put_stored(&s3, &version_key(invalid), &with_id(invalid, local));

    let bad_pointer = format!(
        "meada: {key}: not a hash of `id`, a workspace version id of 16 lowercase hex \
         characters, and `fence`, a decimal fence\n"
    );
    let not_version = |id: &str| {
        format!(
            "meada: {}: malformed: not the workspace version document of its id\n",
            version_key(id)
        )
    };
    let too_big = format!(
        "meada: {}: not stored with the size and hash it records\n",
        version_key(oversized)
    );
    let cases = [
        ("../escape", "5", bad_pointer.clone()),
        (v1.as_str(), "5x", bad_pointer),
        (other, "5", not_version(other)),
        (underived, "5", not_version(underived)),
        (oversized, "5", too_big),
        (invalid, "5", not_version(invalid)),
    ];
    for (id, fence, expected) in cases {
        send(&mut valkey, &["HSET", &key, "id", id, "fence", fence]);
        let (stdout, stderr, code) = workspace(dir, &["pull"]);
        assert_eq!(
            (stdout.as_str(), code),
            ("", Some(1)),
            "{id} {fence}: {stderr}"
        );
        assert_eq!(stderr, expected, "{id} {fence}");
        assert_eq!(pulled_files(dir), before, "{id} {fence}");
    }
}

/// The chain's entries, three of them owned, as a workspace version pulled into `dir`
/// (`<index_dir>/workspace/`), as `meada workspace pull` leaves it at fence 1.
fn write_pulled(dir: &Path) {
    let blocks = serde_json::json!({});
    let entries: Vec<(&str, String)> = (crate::manifest::CHAIN.iter())
        .map(|(repo, path)| (*repo, format!("../{path}")))
        .collect();
    // The owners the version holds: the pulled version is read with them and ignores them.
    let owners = [
        Some("team-web"),
        None,
        Some("team-catalog"),
        Some("team-worker"),
    ];
    let hashed: Vec<(&str, &str, &serde_json::Value, Option<&str>)> = (entries.iter().zip(owners))
        .map(|((repo, path), owner)| (*repo, path.as_str(), &blocks, owner))
        .collect();
    let id = meada_core::ids::workspace_version_id(&hashed);
    let repositories: Vec<serde_json::Value> = (entries.iter().zip(owners))
        .map(|((repo, path), owner)| match owner {
            Some(owner) => serde_json::json!({"repo": repo, "path": path, "owner": owner}),
            None => serde_json::json!({"repo": repo, "path": path}),
        })
        .collect();
    let document = serde_json::json!({
        "id": id, "author": "alice", "time": "2026-10-22T00:00:00Z", "message": "chain",
        "repositories": repositories,
    });
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(format!("{id}.json")), document.to_string()).unwrap();
    fs::write(dir.join("current"), format!("{id} 1\n")).unwrap();
}

/// `meada index --workspace` over a pulled version, its `[snapshot]` and `[valkey]` ports held
/// by this test, indexes the chain and opens no connection; `export` then gives the chain's
/// export goldens, still with no connection.
#[test]
fn index_over_a_pulled_version_opens_no_connection() {
    let store = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let valkey = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dir = chain_manifest("workspace_index_offline");
    fs::write(dir.join("ops/s3-secret"), "x\n").unwrap();
    let port = |l: &std::net::TcpListener| l.local_addr().unwrap().port();
    let bootstrap = format!(
        "index_dir = \"../store-indexes\"\n{}\n[valkey]\nurl = \"redis://127.0.0.1:{}\"\n\n\
         [generations]\nkeep = 3\n\n[workspace]\nsource = \"store\"\n",
        crate::snapshot::section(&format!("http://127.0.0.1:{}", port(&store)), "offline"),
        port(&valkey)
    );
    fs::write(dir.join(BOOTSTRAP), bootstrap).unwrap();
    let _ = fs::remove_dir_all(dir.join("store-indexes"));
    write_pulled(&dir.join("store-indexes/workspace"));
    let (stdout, stderr, code) = on_bootstrap(&dir, &["index"]);
    assert_eq!((stdout.as_str(), stderr.as_str(), code), ("", "", Some(0)));
    for (repo, path) in crate::manifest::CHAIN {
        let (export, stderr, code) = on_bootstrap(&dir, &["export", "--repo", repo]);
        assert_eq!(code, Some(0), "{stderr}");
        assert_golden(&format!("{path}.export.json"), &export);
    }
    for listener in [store, valkey] {
        listener.set_nonblocking(true).unwrap();
        let accepted = listener.accept().map(drop).map_err(|e| e.kind());
        assert_eq!(accepted, Err(std::io::ErrorKind::WouldBlock));
    }
}

/// The job's commands over a stored, applied and pulled version of the chain: `index` then
/// `export` give the export goldens and `status` the same report as the same entries in a file;
/// `publish` records the version in the generation document with the ids a file's publish
/// gives, a stored document is never rewritten (not by a file's publish, nor after another
/// version), a file's publish records none, and a node pulls and serves a generation that
/// records one but refuses one whose field is not a version id.
#[test]
fn the_job_s_commands_read_the_pulled_version_and_publish_records_it() {
    let Some(fleet) = Fleet::new("workspace_job", 3) else {
        return;
    };
    let dir = &fleet.dir;
    let key = workspace_pointer(fleet.case);
    let mut valkey = admin(&fleet.valkey);
    store_bootstrap(dir, "../store-indexes");
    // The version holds owners; the job's commands give what the same entries give without.
    let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let owners = [
        Some("team-web"),
        Some("team-gateway"),
        None,
        Some("team-worker"),
    ];
    fs::write(dir.join("ops/owned.toml"), owned(&text, &owners)).unwrap();
    let (v1, _) = import(dir, "ops/owned.toml", "chain", "alice");
    send(&mut valkey, &["HSET", &key, "id", &v1, "fence", "1"]);
    let (stdout, stderr, code) = on_bootstrap(dir, &["workspace", "pull"]);
    assert_eq!(
        (stdout, stderr.as_str(), code),
        (format!("{v1}\n"), "", Some(0))
    );
    let (stdout, stderr, code) = on_bootstrap(dir, &["index"]);
    assert_eq!((stdout.as_str(), stderr.as_str(), code), ("", "", Some(0)));
    for (repo, path) in crate::manifest::CHAIN {
        let (export, stderr, code) = on_bootstrap(dir, &["export", "--repo", repo]);
        assert_eq!(code, Some(0), "{stderr}");
        assert_golden(&format!("{path}.export.json"), &export);
    }
    let (from_store, stderr, code) = on_bootstrap(dir, &["status"]);
    assert_eq!(code, Some(0), "{stderr}");
    let status = ["status", "--workspace", MANIFEST];
    let (from_file, _, _) = meada_in(dir, "en_US.UTF-8", &status);
    assert_eq!(from_store, from_file);

    let s3 = bucket(&fleet.endpoint, &fleet.bucket);
    let generation_key = |id: &str| format!("prod/meada/generations/{id}.json");
    let document = |id: &str| crate::snapshot::get(&s3, &generation_key(id));
    let field = |bytes: &[u8]| {
        let document: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        document.get("workspace").cloned()
    };
    let (g1, stderr, code) = on_bootstrap(dir, &["snapshot", "publish"]);
    assert_eq!(code, Some(0), "{stderr}");
    let g1 = g1.trim().to_string();
    let first = document(&g1);
    assert_eq!(field(&first), Some(v1.clone().into()));

    // A node pulls and serves it; a fresh one refuses it once its field is no version id.
    let (node, _) = fleet.node("node");
    assert_eq!(fleet.pull(&node), g1);
    let (stdout, stderr, code) = probe(dir, &node);
    assert_eq!((code, stderr.as_str()), (Some(0), ""), "{stdout}");
    let mut forged: serde_json::Value = serde_json::from_slice(&first).unwrap();
    forged["workspace"] = "../x".into();
    let forged = serde_json::to_vec_pretty(&forged).unwrap();
    crate::snapshot::put_stored(&s3, &generation_key(&g1), &forged);
    let (other, _) = fleet.node("other");
    let (stdout, stderr, code) = pull(dir, &other, &[]);
    assert_eq!((stdout.as_str(), code), ("", Some(1)), "{stderr}");
    assert_eq!(
        stderr,
        format!(
            "meada: {}: `workspace` ../x: not a workspace version id of 16 lowercase hex \
             characters\n",
            generation_key(&g1)
        )
    );
    crate::snapshot::put_stored(&s3, &generation_key(&g1), &first);

    // The same bundles from the file: the same generation, its document as first stored.
    assert_eq!(fleet.publish(), g1);
    assert_eq!(document(&g1), first);
    // Another version of the same entries (reversed): the same generation, still unchanged.
    let reversed: String = (crate::manifest::CHAIN.iter().rev())
        .map(|(repo, path)| format!("[[repository]]\nrepo = \"{repo}\"\npath = \"../{path}\"\n"))
        .collect();
    fs::write(dir.join("ops/reversed.toml"), reversed).unwrap();
    let (v2, _) = import(dir, "ops/reversed.toml", "reversed", "bob");
    assert_ne!(v2, v1);
    send(&mut valkey, &["HSET", &key, "id", &v2, "fence", "2"]);
    assert_eq!(on_bootstrap(dir, &["workspace", "pull"]).2, Some(0));
    let (g, stderr, code) = on_bootstrap(dir, &["snapshot", "publish"]);
    assert_eq!((g.trim(), code), (g1.as_str(), Some(0)), "{stderr}");
    assert_eq!(document(&g1), first);
    // The same entries with owners changed, cleared and set: no new generation, the stored
    // document unchanged and still naming the version that first produced it.
    let other = [None, Some("team-b"), Some("team-c"), Some("team-web")];
    fs::write(dir.join("ops/reowned.toml"), owned(&text, &other)).unwrap();
    let (v3, _) = import(dir, "ops/reowned.toml", "reowned", "carol");
    assert_ne!(v3, v1);
    send(&mut valkey, &["HSET", &key, "id", &v3, "fence", "3"]);
    assert_eq!(on_bootstrap(dir, &["workspace", "pull"]).2, Some(0));
    let (g, stderr, code) = on_bootstrap(dir, &["snapshot", "publish"]);
    assert_eq!((g.trim(), code), (g1.as_str(), Some(0)), "{stderr}");
    assert_eq!(document(&g1), first);
    assert_eq!(field(&document(&g1)), Some(v1.clone().into()));
    // A file's new generation records no version.
    next_commit(dir, "chain-web", "file_only");
    let g2 = fleet.publish();
    assert_ne!(g2, g1);
    assert_eq!(field(&document(&g2)), None);
}

mod imports;
