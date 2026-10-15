//! BullMQ jobs and Redis streams as channels (spec #20): `add` on a `Queue` to the processor
//! of the `Worker` of the same name, `xadd` to the function that reads the stream with
//! `xreadgroup`, in the ioredis and node-redis styles.

mod common;

use std::path::PathBuf;

use common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

/// The jobs fixtures as siblings in one case directory; returns the producer side.
fn jobs(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "channels-jobs-worker",
        Some("https://example.com/jobs/worker.git"),
    );
    fixture_repo(
        case,
        "channels-jobs-api",
        Some("https://example.com/jobs/api.git"),
    )
}

#[test]
fn bullmq_jobs_and_redis_streams_link_by_name() {
    let api = jobs("channels_jobs");

    let export = Engine::open(&api).unwrap().export().unwrap();

    let json: Value = serde_json::from_str(&export).unwrap();
    let links: Vec<(String, String, String, String)> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            let text = |k: &str| {
                let id = l[k].as_str().unwrap();
                let id = id.strip_prefix("example.com/jobs/").unwrap_or(id);
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
    let link = |consumer: &str, provider: &str, reason: &str| {
        let (consumer, provider) = (consumer.to_string(), provider.to_string());
        (consumer, provider, "exact".to_string(), reason.to_string())
    };
    let mut expected = vec![
        // The queue name of `new Queue(QUEUE)` is a constant; the processor is a definition.
        link(
            "worker::src/bullmq.ts::sendEmail",
            "api::src/bullmq.ts::sendWelcome::emails.add#0",
            "channel::bullmq::emails",
        ),
        // A processor written in place.
        link(
            "worker::src/bullmq.ts::startReports.bullmq reports",
            "api::src/bullmq.ts::requestReport::reports.add#0",
            "channel::bullmq::reports",
        ),
        // ioredis: both streams named after STREAMS, not the ids that follow them.
        link(
            "worker::src/ioredis.ts::readOrders",
            "api::src/ioredis.ts::appendOrder::redis.xadd#0",
            "channel::redis::orders",
        ),
        link(
            "worker::src/ioredis.ts::readOrders",
            "api::src/ioredis.ts::appendRefund::redis.xadd#0",
            "channel::redis::refunds",
        ),
        // node-redis: one key, and a list of keys.
        link(
            "worker::src/noderedis.ts::readShipments",
            "api::src/noderedis.ts::appendShipment::client.xAdd#0",
            "channel::redis::shipments",
        ),
        link(
            "worker::src/noderedis.ts::readBoth",
            "api::src/ioredis.ts::appendOrder::redis.xadd#0",
            "channel::redis::orders",
        ),
        link(
            "worker::src/noderedis.ts::readBoth",
            "api::src/ioredis.ts::appendRefund::redis.xadd#0",
            "channel::redis::refunds",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
    // `Set.add` is no producer.
    assert_eq!(json["providers"].as_array().unwrap().len(), 5);
    assert_golden("channels-jobs-api.export.json", &export);
}
