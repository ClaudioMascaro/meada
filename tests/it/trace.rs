//! `trace` over the chain fixture (spec #3): `chain-web` (A) consumes from `chain-gateway` (B),
//! an Express gateway that consumes from `chain-catalog` (C), a Next.js app. Between them sit a
//! client wrapper, imported path constants, a base URL variable, a mount prefix, an absolute
//! URL on a declared host, a path both B and C serve, an unresolved URL and a cycle in C.

use crate::common;

use std::fs;
use std::path::Path;

use common::{assert_golden, chain, fixture_repo, without_processes, worker};
use meada::engine::{Direction, Engine, Start};

#[test]
fn every_expected_link_of_the_chain_appears_at_its_level() {
    let (web, gateway, catalog) = chain("chain_export");
    let worker = worker(&web);

    for (repo, name) in [
        (web, "web"),
        (gateway, "gateway"),
        (catalog, "catalog"),
        (worker, "worker"),
    ] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(&format!("chain-{name}.export.json"), &export);
    }
}

#[test]
fn trace_down_follows_calls_and_links_across_three_repositories() {
    let (web, _, _) = chain("trace_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("dashboard"), Direction::Down, None)
        .unwrap();

    assert_golden("chain.trace-down.txt", &answer);
}

#[test]
fn trace_down_shows_every_consumer_a_wrapper_call_site_holds() {
    let (web, _, _) = chain("trace_wrapper_consumers");
    // One call of a wrapper with three parameter-dependent fetch calls: three consumers on
    // one site, one linked, one with no provider and one unresolved.
    fs::write(
        web.join("src/sync.ts"),
        "const BASE = process.env.GATEWAY_URL;\n\n\
         function both(path: string, url: string) {\n  \
           fetch(`${BASE}${path}`);\n  \
           fetch(`${BASE}${path}/archive`);\n  \
           return fetch(url);\n\
         }\n\n\
         export function sync() {\n  \
           return both(\"/api/orders\", location.href);\n\
         }\n",
    )
    .unwrap();

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("sync"), Direction::Down, None)
        .unwrap();

    assert_golden("chain.trace-wrapper.txt", &answer);
}

#[test]
fn trace_up_from_a_route_reaches_every_consumer_upstream() {
    let (_, _, catalog) = chain("trace_up");

    let answer = Engine::open(&catalog)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/products/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("chain.trace-up.txt", &answer);
}

/// The gateway's `cancelOrder` writes the `postgres` resource `orders`; the worker's
/// `settleOrders` reads it.
const WRITER: &str = "example.com/chain/gateway::src/orders.ts::cancelOrder";
const READER: &str = "example.com/chain/worker::src/jobs.ts::settleOrders";

#[test]
fn every_writer_links_to_every_reader_of_another_repository() {
    let (_, gateway, _) = chain("resource_sides");
    let worker = worker(&gateway);
    let append = |repo: &Path, toml: &str| {
        let path = repo.join("meada.toml");
        fs::write(&path, fs::read_to_string(&path).unwrap() + toml).unwrap();
    };
    let entry = |role: &str, via: &str| {
        format!(
            "\n[[resource]]\nkind = \"postgres\"\nid = \"orders\"\nrole = \"{role}\"\nvia = [{via}]\n"
        )
    };
    // A second writer in the gateway; in the worker, a writer of its own, which links to no
    // reader of the worker, and a `via` that names no symbol.
    append(&gateway, &entry("write", "\"src/orders.ts::listOrders\""));
    append(
        &worker,
        &entry(
            "read-write",
            "\"src/notify.ts::notify\", \"src/jobs.ts::gone\"",
        ),
    );

    let export = Engine::open(&worker).unwrap().export().unwrap();

    assert_golden(
        "chain-worker.resources.export.json",
        &without_processes(&export),
    );
}

#[test]
fn trace_down_from_a_writer_crosses_the_resource_into_its_reader() {
    let (web, _, _) = chain("trace_resource_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From(WRITER), Direction::Down, None)
        .unwrap();

    assert_golden("chain.trace-resource-down.txt", &answer);
}

#[test]
fn a_resource_both_sides_read_and_write_closes_a_cycle() {
    let (web, gateway, _) = chain("trace_resource_cycle");
    let append = |repo: &Path, via: &str| {
        let path = repo.join("meada.toml");
        let entry = format!(
            "\n[[resource]]\nkind = \"redis\"\nid = \"locks\"\nrole = \"read-write\"\nvia = [\"{via}\"]\n"
        );
        fs::write(&path, fs::read_to_string(&path).unwrap() + &entry).unwrap();
    };
    append(&gateway, "src/orders.ts::cancelOrder");
    append(&worker(&gateway), "src/jobs.ts::settleOrders");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From(WRITER), Direction::Down, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[declared: resource::redis::locks] example.com/chain/gateway src/orders.ts:10 \
             cancelOrder (cycle)"
        ),
        "{answer}"
    );
}

/// The gateway's `createOrder` produces the Kafka topic `orders.created` through its
/// `publish` wrapper; the worker's `onOrderCreated` handles it (spec #20).
const PRODUCER: &str = "example.com/chain/gateway::src/checkout.ts::createOrder";
const HANDLER: &str = "example.com/chain/worker::src/consumer.ts::onOrderCreated";

#[test]
fn trace_down_from_a_producer_crosses_the_channel_into_its_handler() {
    let (web, _, _) = chain("trace_channel_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From(PRODUCER), Direction::Down, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[exact: channel::kafka::orders.created] example.com/chain/worker \
             src/consumer.ts:13 onOrderCreated"
        ),
        "{answer}"
    );
    assert_golden("chain.trace-channel-down.txt", &answer);
}

#[test]
fn trace_up_from_a_handler_crosses_the_channel_then_http_into_the_web_app() {
    let (web, _, _) = chain("trace_channel_up");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From(HANDLER), Direction::Up, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[exact: channel::kafka::orders.created] example.com/chain/gateway \
             src/checkout.ts:11 createOrder"
        ),
        "{answer}"
    );
    assert_golden("chain.trace-channel-up.txt", &answer);
}

#[test]
fn a_trace_from_a_channel_points_to_context() {
    let (web, _, _) = chain("trace_channel_start");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("channel::kafka::orders.created"),
            Direction::Down,
            None,
        )
        .unwrap();

    assert!(answer.contains("is a channel"), "{answer}");
}

#[test]
fn trace_up_from_a_reader_crosses_the_resource_into_its_writer_and_upstream() {
    let (web, _, _) = chain("trace_resource_up");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From(READER), Direction::Up, None)
        .unwrap();

    assert_golden("chain.trace-resource-up.txt", &answer);
}

#[test]
fn trace_from_uncommitted_changes_starts_at_edited_and_untracked_symbols() {
    let (_, _, catalog) = chain("trace_uncommitted");
    let products = catalog.join("lib/products.ts");
    let edited = fs::read_to_string(&products)
        .unwrap()
        .replace("return { id };", "return { id, found: true };");
    fs::write(&products, edited).unwrap();
    fs::write(
        catalog.join("lib/cache.ts"),
        "export function warm() {\n  return new Map();\n}\n",
    )
    .unwrap();

    let answer = Engine::open(&catalog)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("chain.trace-uncommitted.txt", &answer);
}

#[test]
fn a_trace_over_budget_says_where_it_stopped_and_keeps_the_footer() {
    let (web, _, _) = chain("trace_cut");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("dashboard"), Direction::Down, Some(120))
        .unwrap();

    assert_golden("chain.trace-cut.txt", &answer);
    let tokens = answer.len().div_ceil(4);
    assert!(tokens <= 120, "{tokens} tokens");
}

#[test]
fn a_missing_peer_closes_the_footer_and_the_rest_of_the_chain_answers() {
    let case = "trace_missing";
    let web = fixture_repo(case, "chain-web", Some("https://example.com/chain/web.git"));
    fixture_repo(
        case,
        "chain-gateway",
        Some("git@example.com:chain/gateway.git"),
    );

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("dashboard"), Direction::Down, None)
        .unwrap();

    assert_golden("chain.trace-missing-peer.txt", &answer);
}

#[test]
fn uncommitted_changes_include_a_file_whose_name_has_a_space() {
    let (_, _, catalog) = chain("trace_uncommitted_space");
    let stock = catalog.join("lib/old stock.ts");
    fs::write(&stock, "export function stock() {\n  return 1;\n}\n").unwrap();
    common::git(&catalog, &["add", "-A"]);
    common::git(&catalog, &["commit", "-q", "-m", "stock"]);
    fs::write(&stock, "export function stock() {\n  return 2;\n}\n").unwrap();

    let answer = Engine::open(&catalog)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert!(
        answer.starts_with("example.com/chain/catalog lib/old stock.ts:1 stock\nchain: none"),
        "{answer}"
    );
}

#[test]
fn in_a_repository_with_no_commits_every_file_is_uncommitted() {
    let repo = Path::new(env!("CARGO_TARGET_TMPDIR")).join("trace_unborn/web");
    let _ = fs::remove_dir_all(&repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    common::git(&repo, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let a = "export function a() {\n  return 1;\n}\n";
    fs::write(repo.join("src/a.ts"), a).unwrap();
    fs::write(repo.join("src/b.ts"), a.replace('a', "b")).unwrap();
    // One file staged, one not: neither is in a commit.
    common::git(&repo, &["add", "src/a.ts"]);

    let answer = Engine::open(&repo)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Down, None)
        .unwrap();

    assert!(
        answer.starts_with("local/web src/a.ts:1 a\nlocal/web src/b.ts:1 b\nchain: none"),
        "{answer}"
    );
}

#[test]
fn a_trace_with_no_single_start_says_why() {
    let (web, _, _) = chain("trace_no_start");
    let engine = Engine::open(&web).unwrap();
    let trace = |from| {
        engine
            .trace(Start::From(from), Direction::Down, None)
            .unwrap()
    };

    assert_golden("chain.trace-none.txt", &trace("nothing"));
    // Two route files of the catalog export a `GET`.
    assert_golden("chain.trace-fits-symbols.txt", &trace("GET"));
    // Four functions of the workspace each make a first `fetch`.
    assert_golden("chain.trace-fits-calls.txt", &trace("fetch#0"));
    let clean = engine.trace(Start::Uncommitted, Direction::Down, None);
    assert_golden("chain.trace-clean.txt", &clean.unwrap());
}

#[test]
fn sdk_reader_calls_keep_their_identity_and_follow_data_in_both_directions() {
    for extension in ["js", "ts", "py"] {
        let group = format!("trace_sdk_calls_{extension}");
        let writer_id = format!("example.com/sdk-call-flow/writer-{extension}");
        let reader_id = format!("example.com/sdk-call-flow/reader-{extension}");
        let writer = fixture_repo(
            &group,
            "aws-sdk-controls",
            Some(&format!("https://{writer_id}.git")),
        );
        let writer_moved = writer.with_file_name("writer");
        let _ = fs::remove_dir_all(&writer_moved);
        fs::rename(&writer, &writer_moved).unwrap();
        let writer = writer_moved;
        let reader = fixture_repo(
            &group,
            "aws-sdk-controls",
            Some(&format!("https://{reader_id}.git")),
        );
        fs::write(
            writer.join("meada.toml"),
            format!("[[peer]]\nrepo = \"{reader_id}\"\n"),
        )
        .unwrap();
        fs::write(
            reader.join("meada.toml"),
            format!("[[peer]]\nrepo = \"{writer_id}\"\n"),
        )
        .unwrap();
        let (write, read) = if extension == "py" {
            (
                r#"import boto3
from fastapi import FastAPI
app = FastAPI()
client = boto3.client('s3')
@app.post('/publish')
def write():
    client.put_object(Bucket='shared')
"#,
                r#"import boto3
client = boto3.client('s3')
def after_read():
    pass
def after_unlinked():
    pass
def unlinked():
    client.get_object(Bucket='unmanaged')
    after_unlinked()
def read():
    client.get_object(Bucket='shared')
    client.get_object(Bucket='shared')
    after_read()
client.get_object(Bucket='shared')
"#,
            )
        } else {
            (
                r#"import express from 'express';
import {S3Client, PutObjectCommand} from '@aws-sdk/client-s3';
const app = express();
const client = new S3Client({});
function write() { client.send(new PutObjectCommand({Bucket:'shared'})); }
app.post('/publish', write);
"#,
                r#"import {S3Client, GetObjectCommand} from '@aws-sdk/client-s3';
const client = new S3Client({});
function after_read() {}
function after_unlinked() {}
function unlinked() { client.send(new GetObjectCommand({Bucket:'unmanaged'})); after_unlinked(); }
function read() {
    client.send(new GetObjectCommand({Bucket:'shared'}));
    client.send(new GetObjectCommand({Bucket:'shared'}));
    after_read();
}
client.send(new GetObjectCommand({Bucket:'shared'}));
"#,
            )
        };
        let file = format!("app.{extension}");
        fs::write(writer.join(&file), write).unwrap();
        fs::write(reader.join(&file), read).unwrap();
        let writers = Engine::open(&writer).unwrap();
        let readers = Engine::open(&reader).unwrap();
        let export: serde_json::Value = serde_json::from_str(&readers.export().unwrap()).unwrap();
        let consumers = export["consumers"].as_array().unwrap();
        assert_eq!(consumers.len(), 4, "{extension}: {export}");
        assert!(
            consumers.iter().all(|c| matches!(
                c["contract"].as_str(),
                Some("resource::s3::shared" | "resource::s3::unmanaged")
            ) && c["site"].as_str().unwrap().contains('#')),
            "{extension}: {export}"
        );
        let ids: std::collections::BTreeSet<_> = consumers
            .iter()
            .map(|c| c["site"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 4, "{extension}: {export}");
        let links = export["links"].as_array().unwrap();
        assert_eq!(links.len(), 3, "{extension}: {export}");
        assert!(
            links
                .iter()
                .all(|l| l["contract"] == "resource::s3::shared" && l["level"] == "exact"),
            "{extension}: {export}"
        );
        let linked: std::collections::BTreeSet<_> = links
            .iter()
            .map(|l| l["consumer"].as_str().unwrap())
            .collect();
        assert_eq!(linked.len(), 3, "{extension}: {export}");
        assert!(
            consumers
                .iter()
                .all(|c| linked.contains(c["site"].as_str().unwrap())
                    == (c["contract"] == "resource::s3::shared")),
            "{extension}: {export}"
        );
        let producer = links[0]["provider"].as_str().unwrap();
        for id in &ids {
            let up = readers
                .trace(Start::From(id), Direction::Up, Some(8000))
                .unwrap();
            if linked.contains(id) {
                assert!(
                    up.contains(&writer_id) && up.contains("[exact: resource::s3::shared]"),
                    "{extension} {id}: {up}"
                );
            } else {
                assert!(!up.contains(&writer_id), "{extension} {id}: {up}");
            }
            let down = readers
                .trace(Start::From(id), Direction::Down, Some(8000))
                .unwrap();
            assert!(!down.contains(&writer_id), "{extension} {id}: {down}");
            if id.contains("::read::") {
                assert!(down.contains("after_read"), "{extension} {id}: {down}");
            }
            if id.contains("::unlinked::") {
                assert!(down.contains("after_unlinked"), "{extension} {id}: {down}");
            }
        }
        let read_symbol = export["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|s| s["id"].as_str().filter(|id| id.ends_with("::read")))
            .unwrap();
        let up = readers
            .trace(Start::From(read_symbol), Direction::Up, Some(8000))
            .unwrap();
        assert!(
            up.contains(&writer_id) && up.contains("[exact: resource::s3::shared]"),
            "{extension}: {up}"
        );
        let read_down = readers
            .trace(Start::From(read_symbol), Direction::Down, Some(8000))
            .unwrap();
        assert!(!read_down.contains(&writer_id), "{extension}: {read_down}");
        let context = readers.context(read_symbol, Some(8000)).unwrap();
        assert!(context.contains("POST /publish"), "{extension}: {context}");
        let down = writers
            .trace(Start::From(producer), Direction::Down, Some(8000))
            .unwrap();
        assert!(
            down.lines()
                .any(|l| l.contains(&reader_id) && l.contains(&file) && l.ends_with(" read")),
            "{extension}: {down}"
        );
        assert!(down.contains("after_read"), "{extension}: {down}");
        assert!(
            down.lines()
                .any(|l| l.contains(&reader_id) && l.contains(&file) && l.ends_with("(top level)")),
            "{extension}: {down}"
        );
        let writer_export: serde_json::Value =
            serde_json::from_str(&writers.export().unwrap()).unwrap();
        let nodes: std::collections::BTreeSet<_> = writer_export["processes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|p| p["steps"].as_array().unwrap())
            .filter_map(|step| step["node"].as_str())
            .collect();
        assert!(
            ids.iter()
                .all(|id| nodes.contains(id) == linked.contains(id)),
            "{extension}: {writer_export}"
        );
        assert!(
            writer_export["processes"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|p| p["steps"].as_array().unwrap())
                .all(|step| step["mark"] != "cycle"),
            "{extension}: {writer_export}"
        );
        assert!(
            nodes.iter().any(|id| id.ends_with("::after_read")),
            "{extension}: {writer_export}"
        );
    }
}
