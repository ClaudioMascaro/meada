use std::path::{Path, PathBuf};

use meada::engine::Engine;
use serde_json::Value;

use crate::common::{commit_all, edited, fixture_repo, in_repo};

const PEER: &str = "example.com/acme/peer-worker";
const PEER_CONSUMER: &str = "src/consumer.ts";
const MAIN: (&str, &str) = ("main.go", "\tcfg, err := config.NewConfig()\n");

fn peer(case: &str) {
    let origin = Some("https://example.com/acme/peer-worker.git");
    fixture_repo(case, "kafka-peer-worker", origin);
}

fn clients(case: &str) -> PathBuf {
    peer(case);
    let origin = Some("https://example.com/acme/clients.git");
    fixture_repo(case, "channel-clients", origin)
}

fn plugins(case: &str) -> PathBuf {
    peer(case);
    let origin = Some("https://example.com/acme/plugins.git");
    fixture_repo(case, "kafkajs-plugins", origin)
}

fn main_edited(repo: &Path) -> Value {
    let (path, line) = MAIN;
    edited(repo, path, line, &format!("\t// edited\n{line}"))
}

fn tier(answer: &Value, repo: &str, path: &str) -> (String, String) {
    let file = in_repo(answer, repo, path);
    let file = file.unwrap_or_else(|| panic!("{repo} {path} is not affected: {answer}"));
    let text = |key: &str| file[key].as_str().unwrap().to_string();
    (text("tier"), text("level"))
}

fn level(export: &Value, contract: &str, provider: &str, consumer: &str) -> String {
    let found = (export["links"].as_array().unwrap().iter()).find(|l| {
        l["contract"] == contract
            && l["provider"].as_str().unwrap().contains(provider)
            && l["consumer"].as_str().unwrap().contains(consumer)
    });
    let found = found.unwrap_or_else(|| panic!("no {contract} link: {export}"));
    found["level"].as_str().unwrap().to_string()
}

fn export(repo: &Path) -> Value {
    serde_json::from_str(&Engine::open(repo).unwrap().export().unwrap()).unwrap()
}

#[test]
fn a_producer_change_reaches_a_peer_consumer_at_certain() {
    let repo = clients("lock_peer_consumer");
    let answer = main_edited(&repo);
    let found = tier(&answer, PEER, PEER_CONSUMER);
    assert_eq!(found, ("certain".into(), "declared".into()));
}

#[test]
fn a_peer_producer_links_to_both_monorepo_consumers() {
    let peer = format!("{PEER}::src/producer.ts::");
    let created = "channel::kafka::orders.created";
    let run = "::internal/service/svc.go::Service.Run";
    let level_of = level(&export(&clients("lock_links_clients")), created, &peer, run);
    assert_eq!(level_of, "declared");
    let shipped = "channel::kafka::orders.shipped";
    let approvals = "::plugins/approvals/src/KafkaClient.ts::";
    let level_of = level(
        &export(&plugins("lock_links_plugins")),
        shipped,
        &peer,
        approvals,
    );
    assert_eq!(level_of, "exact");
}

#[test]
fn a_plugin_publish_change_reaches_the_other_plugin_at_certain() {
    let repo = plugins("lock_plugins");
    let publish = "'acme.orders.topics.approved'), 'approved');";
    let path = "plugins/orders/src/KafkaClient.ts";
    let answer = edited(
        &repo,
        path,
        publish,
        &publish.replace("'approved')", "'sent')"),
    );
    let consumer = "plugins/approvals/src/KafkaClient.ts";
    let found = tier(&answer, "example.com/acme/plugins", consumer);
    assert_eq!(found, ("certain".into(), "exact".into()));
}

#[test]
fn an_inferred_topic_reaches_the_consumer_below_certain() {
    let repo = clients("lock_inferred");
    let values = repo.join("chart/svc/values-prd.yaml");
    let text = std::fs::read_to_string(&values).unwrap();
    let text = text.replace("EVENTS_TOPIC: events.published", "EVENTS_TOPIC: events.v2");
    std::fs::write(&values, text).unwrap();
    commit_all(&repo, "prd events");
    let answer = main_edited(&repo);
    let found = tier(&answer, PEER, PEER_CONSUMER);
    assert_eq!(found, ("inferred".into(), "inferred".into()));
}
