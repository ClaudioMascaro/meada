use crate::common;

use std::path::PathBuf;

use common::{assert_golden, fixture_repo, without_processes};
use meada::engine::Engine;
use serde_json::Value;

fn channels(case: &str) -> PathBuf {
    for fixture in ["channels-worker", "channels-billing"] {
        let name = fixture.trim_start_matches("channels-");
        let origin = format!("https://example.com/channels/{name}.git");
        fixture_repo(case, fixture, Some(&origin));
    }
    fixture_repo(
        case,
        "channels-api",
        Some("https://example.com/channels/api.git"),
    )
}

const API: &str = "example.com/channels/api";
const WORKER: &str = "example.com/channels/worker";
const BILLING: &str = "example.com/channels/billing";

#[test]
fn every_consumer_links_to_every_producer_of_its_kafka_topic() {
    let api = channels("channels_kafka");

    let export = Engine::open(&api).unwrap().export().unwrap();

    let json: Value = serde_json::from_str(&export).unwrap();
    let links: Vec<(String, String, String, String)> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            let text = |k: &str| l[k].as_str().unwrap().to_string();
            (
                text("consumer"),
                text("provider"),
                text("level"),
                text("reason"),
            )
        })
        .filter(|(_, _, _, reason)| reason.starts_with("channel::kafka::"))
        .collect();
    let link = |consumer: String, provider: &str, channel: &str| {
        let provider = format!("{API}::src/{provider}");
        let reason = format!("channel::kafka::{channel}");
        (consumer, provider, "exact".to_string(), reason)
    };
    let created = "orders.ts::createOrder::producer.send#0";
    let billing_handler =
        format!("{BILLING}::src/listen.ts::listen.kafka orders.created billing.invoices");
    let mut expected = vec![
        link(
            format!("{API}::src/audit.ts::auditLog.kafka audit.log"),
            "orders.ts::audit::producer.send#0",
            "audit.log",
        ),
        link(
            billing_handler.clone(),
            "orders.ts::invoice::producer.send#0",
            "billing.invoices",
        ),
        link(billing_handler, created, "orders.created"),
        link(
            format!("{WORKER}::src/consumer.ts::handleOrder"),
            created,
            "orders.created",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
    let providers: Vec<&str> = (json["providers"].as_array().unwrap())
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .filter(|contract| contract.starts_with("channel::kafka"))
        .collect();
    assert_eq!(
        providers,
        [
            "channel::kafka",
            "channel::kafka",
            "channel::kafka::audit.log",
            "channel::kafka::billing.invoices",
            "channel::kafka::orders",
            "channel::kafka::orders.created",
            "channel::kafka::orders.shipped",
        ]
    );
    assert_golden("channels-api.export.json", &without_processes(&export));
}

fn queues(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "channels-queues-worker",
        Some("https://example.com/queues/worker.git"),
    );
    fixture_repo(
        case,
        "channels-queues-api",
        Some("https://example.com/queues/api.git"),
    )
}

#[test]
fn sqs_queues_link_by_name_and_nats_subjects_through_wildcards() {
    let api = queues("channels_queues");

    let export = Engine::open(&api).unwrap().export().unwrap();

    let json: Value = serde_json::from_str(&export).unwrap();
    let links: Vec<(String, String, String, String)> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            let text = |k: &str| {
                let id = l[k].as_str().unwrap();
                let id = id.strip_prefix("example.com/queues/").unwrap_or(id);
                id.to_string()
            };
            (
                text("consumer"),
                text("provider"),
                text("level"),
                text("reason"),
            )
        })
        .collect();
    let link = |consumer: &str, provider: &str, level: &str, reason: &str| {
        let (consumer, provider) = (consumer.to_string(), provider.to_string());
        (consumer, provider, level.to_string(), reason.to_string())
    };
    let nats_api = "api::src/nats.ts::";
    let nats_worker = "worker::src/nats.ts::";
    let created = format!("{nats_api}orderCreated::nc.publish#0");
    let created_eu = format!("{nats_api}orderCreated::nc.publish#1");
    let mut expected = vec![
        link(
            "worker::src/sqs.ts::drain",
            "api::src/sqs.ts::enqueueOrder::SendMessageCommand#0",
            "exact",
            "channel::sqs::orders",
        ),
        link(
            "worker::src/sqs.ts::handleRefund",
            "api::src/sqs.ts::enqueueRefund::enqueue#0",
            "exact",
            "channel::sqs::refunds",
        ),
        link(
            &format!("{nats_worker}oneToken"),
            &created,
            "inferred",
            "channel::nats::orders.created wildcard orders.*",
        ),
        link(
            &format!("{nats_worker}onOrder"),
            &created,
            "inferred",
            "channel::nats::orders.created wildcard orders.>",
        ),
        link(
            &format!("{nats_worker}onOrder"),
            &created_eu,
            "inferred",
            "channel::nats::orders.eu.created wildcard orders.>",
        ),
        link(
            &format!("{nats_worker}middle.nats orders.*.created"),
            &created_eu,
            "inferred",
            "channel::nats::orders.eu.created wildcard orders.*.created",
        ),
        link(
            &format!("{nats_worker}literal"),
            &format!("{nats_api}invoicePaid::nc.publish#0"),
            "exact",
            "channel::nats::billing.invoice.paid",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
    assert_golden(
        "channels-queues-api.export.json",
        &without_processes(&export),
    );
}

#[test]
fn context_on_a_subject_lists_the_wildcard_sides_linked_to_it() {
    let engine = Engine::open(&queues("channels_wildcard_context")).unwrap();

    let subject = engine
        .context("channel::nats::orders.created", None)
        .unwrap();
    assert_golden("channels-queues.context-subject.txt", &subject);
    let pattern = engine.context("channel::nats::orders.>", None).unwrap();
    assert_golden("channels-queues.context-pattern.txt", &pattern);
}

#[test]
fn amqp_publishes_reach_the_queues_bound_to_their_exchange() {
    let api = channels("channels_amqp");

    let export = Engine::open(&api).unwrap().export().unwrap();

    let json: Value = serde_json::from_str(&export).unwrap();
    let rel = |id: &str| {
        (id.split_once("::"))
            .map_or(id, |(_, rest)| rest)
            .to_string()
    };
    let mut links: Vec<(String, String, String, String)> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            let text = |k: &str| l[k].as_str().unwrap().to_string();
            (
                rel(&text("consumer")),
                rel(&text("provider")),
                text("level"),
                text("reason"),
            )
        })
        .filter(|(_, _, _, reason)| reason.starts_with("channel::amqp::"))
        .collect();
    links.sort();
    let (api_file, worker_file) = ("src/amqp.ts::", "src/queues.ts::");
    let link = |handler: &str, producer: &str, level: &str, reason: &str| {
        let consumer = format!("{worker_file}{handler}");
        let provider = format!("{api_file}{producer}");
        (consumer, provider, level.to_string(), reason.to_string())
    };
    let mut expected = vec![
        link(
            "onOrder",
            "queueOrder::channel.sendToQueue#0",
            "exact",
            "channel::amqp::orders",
        ),
        link(
            "onJob",
            "dispatch::channel.publish#0",
            "exact",
            "channel::amqp::jobs",
        ),
        link(
            "onAudit",
            "announce::channel.publish#0",
            "exact",
            "channel::amqp::audit binding events order.created",
        ),
        link(
            "onShipment",
            "announce::channel.publish#1",
            "inferred",
            "channel::amqp::shipments binding events *.shipped",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
    let publishes: Vec<String> = (json["publishes"].as_array().unwrap().iter())
        .map(|p| {
            format!(
                "{} {} {}",
                p["exchange"],
                p["key"],
                rel(p["site"].as_str().unwrap())
            )
        })
        .collect();
    let publish = |key: &str, n: u8| {
        format!("\"events\" \"{key}\" src/amqp.ts::announce::channel.publish#{n}")
    };
    let expected = [
        publish("order.created", 0),
        publish("order.refunded", 2),
        publish("order.shipped", 1),
    ];
    assert_eq!(publishes, expected);
    let sides = ["providers", "consumers", "bindings", "links"].map(|k| &json[k]);
    let channels = (sides.iter().filter_map(|s| s.as_array()).flatten())
        .filter_map(|side| side["contract"].as_str())
        .filter(|c| c.starts_with("channel::"));
    for contract in channels {
        let broker = contract.split("::").nth(1).unwrap();
        let v1 = ["kafka", "amqp", "sqs", "nats", "bullmq", "redis"];
        assert!(v1.contains(&broker), "{contract}");
    }
}
