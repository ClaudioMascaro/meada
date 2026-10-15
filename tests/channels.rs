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
    assert_golden("channels-api.export.json", &export);
}

/// RabbitMQ (spec #20): `sendToQueue` links to `consume` by the queue name; a publish reaches
/// the queues bound to its exchange, the binding declared in another repository, at `exact`
/// when the pattern is the routing key and `inferred` for a wildcard; the empty exchange is
/// the queue the key names. The Kafka topic `orders` beside the queue `orders` never links.
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
    // `order.refunded` matches no binding: its publish stays exported, with no link.
    assert_eq!(links, expected);
}
