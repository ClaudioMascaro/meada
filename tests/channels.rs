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
    let providers: Vec<&str> = (json["providers"].as_array().unwrap())
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .collect();
    assert_eq!(
        providers,
        [
            "channel::kafka::audit.log",
            "channel::kafka::billing.invoices",
            "channel::kafka::orders.created",
            "channel::kafka::orders.shipped",
        ]
    );
    assert_golden("channels-api.export.json", &export);
}
