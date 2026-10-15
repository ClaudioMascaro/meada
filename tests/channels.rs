//! Channels (spec #20): producers and consumers of a broker's topics and queues, recognized
//! only on clients that trace back to their library, linked from every consumer to every
//! producer of the same channel, in the same repository too.

mod common;

use std::path::PathBuf;

use common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

/// The channels fixture as siblings in one case directory; returns the producer side, `api`,
/// whose workspace holds all three.
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
        // Same repository, through an object-literal method handler.
        link(
            format!("{API}::src/audit.ts::auditLog.kafka audit.log"),
            "orders.ts::audit::producer.send#0",
            "audit.log",
        ),
        // A template literal over an imported constant.
        link(
            billing_handler.clone(),
            "orders.ts::invoice::producer.send#0",
            "billing.invoices",
        ),
        // Fan-out: both consumers of `orders.created`, each at `exact`.
        link(billing_handler, created, "orders.created"),
        link(
            format!("{WORKER}::src/consumer.ts::handleOrder"),
            created,
            "orders.created",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
    // `mailer.send` is no producer, and `orders.shipped` has no consumer yet stays exported.
    // The two producers whose topic Meada cannot evaluate are `channel::kafka`, never linked.
    let providers: Vec<&str> = (json["providers"].as_array().unwrap())
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .collect();
    assert_eq!(
        providers,
        [
            "channel::kafka",
            "channel::kafka",
            "channel::kafka::audit.log",
            "channel::kafka::billing.invoices",
            "channel::kafka::orders.created",
            "channel::kafka::orders.shipped",
        ]
    );
    assert_golden("channels-api.export.json", &export);
}

/// The queue and subject fixtures as siblings in one case directory; returns the producer side.
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

/// SQS queues link by the queue name at the end of the URL, whatever its prefix, with the AWS
/// SDK v3 and `sqs-consumer`; NATS subjects link exactly, and through `*` and `>` at
/// `inferred`, with the pattern in the reason.
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
        // The queue name is the last segment of a URL whose prefix is dynamic.
        link(
            "worker::src/sqs.ts::drain",
            "api::src/sqs.ts::enqueueOrder::SendMessageCommand#0",
            "exact",
            "channel::sqs::orders",
        ),
        // `sqs-consumer`'s `handleMessage`, a producer reached through a client wrapper.
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
    assert_golden("channels-queues-api.export.json", &export);
}
