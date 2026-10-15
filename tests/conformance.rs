//! The language conformance suite (ADR 0007): a language pack is done when the export of its
//! conformance fixture holds exactly the expected definitions, every expected call at its
//! expected level, and matches its golden file. A new pack adds a fixture under
//! `tests/fixtures/conformance-<language>/` and one test calling `conform`.

mod common;

use common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

const ORIGIN: &str = "https://example.com/acme/conformance.git";
const REPO: &str = "example.com/acme/conformance";

/// An expected call site: its id and level, and its targets (one for `exact` and `inferred`,
/// every candidate for `ambiguous`, none when the call stays unresolved). Ids are relative to
/// the repository.
type ExpectedCall<'a> = (&'a str, Option<&'a str>, &'a [&'a str]);

const EXACT: Option<&str> = Some("exact");
const INFERRED: Option<&str> = Some("inferred");
const AMBIGUOUS: Option<&str> = Some("ambiguous");

fn conform(fixture: &str, symbols: &[&str], calls: &[ExpectedCall]) {
    let repo = fixture_repo(&format!("conformance_{fixture}"), fixture, Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let id = |rel: &str| format!("{REPO}::{rel}");

    let actual: Vec<&str> = json["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    let mut expected: Vec<String> = symbols.iter().map(|s| id(s)).collect();
    expected.sort();
    assert_eq!(actual, expected, "definitions of {fixture}");

    for (call, level, targets) in calls {
        let site = json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id(call))
            .unwrap_or_else(|| panic!("no call {call} in {fixture}"));
        let mut found: Vec<&str> = site["target"].as_str().into_iter().collect();
        if let Some(candidates) = site["candidates"].as_array() {
            found.extend(candidates.iter().map(|c| c.as_str().unwrap()));
        }
        let wanted: Vec<String> = targets.iter().map(|t| id(t)).collect();
        assert_eq!(
            (site["level"].as_str(), found),
            (*level, wanted.iter().map(String::as_str).collect()),
            "call {call} in {fixture}"
        );
    }

    assert_golden(&format!("{fixture}.export.json"), &export);
}

#[test]
fn typescript_and_javascript() {
    conform(
        "conformance-ts-js",
        &[
            "src/app.ts::main",
            "src/legacy.cjs::legacy",
            "src/math.ts::add",
            "src/math.ts::double",
            "src/math.ts::scale",
            "src/math.ts::scale.times",
            "src/math.ts::square",
            "src/messages/consume.ts::consumeOrders",
            "src/messages/consume.ts::consumeOrders.kafka orders.created orders.shipped",
            "src/messages/consume.ts::consumePayments",
            "src/messages/consume.ts::consumeRefunds",
            "src/messages/consume.ts::consumeRefunds.kafka refunds",
            "src/messages/consume.ts::listen",
            "src/messages/consume.ts::listenRetries",
            "src/messages/consume.ts::record",
            "src/messages/exchanges.ts::announce",
            "src/messages/exchanges.ts::bind",
            "src/messages/exchanges.ts::listen",
            "src/messages/exchanges.ts::onAlert",
            "src/messages/exchanges.ts::onAudit",
            "src/messages/exchanges.ts::onShipment",
            "src/messages/handlers.ts::onPayment",
            "src/messages/produce.ts::deliver",
            "src/messages/produce.ts::emit",
            "src/messages/produce.ts::emitRefund",
            "src/messages/produce.ts::notProduced",
            "src/messages/produce.ts::produceAll",
            "src/messages/produce.ts::relay",
            "src/messages/produce.ts::retry",
            "src/messages/queues.ts::enqueue",
            "src/messages/queues.ts::enqueueReports",
            "src/messages/queues.ts::queue",
            "src/messages/queues.ts::record",
            "src/messages/queues.ts::work",
            "src/messages/queues.ts::work.amqp tasks",
            "src/messages/queues.ts::workReports",
            "src/shapes/circle.ts::Circle",
            "src/shapes/circle.ts::Circle.area",
            "src/shapes/circle.ts::Circle.constructor",
            "src/shapes/shape.ts::Shape",
            "src/shapes/shape.ts::Shape.area",
            "src/shapes/shape.ts::Shape.describe",
            "src/shapes/shape.ts::Shape.unit",
            "src/util/format.mjs::format",
            "src/util/log.js::default",
            "src/view.tsx::render",
            "src/widget.jsx::render",
        ],
        &[
            // In-file scope: top level, nested function, `this.method` inside a class.
            ("src/math.ts::double::add#0", EXACT, &["src/math.ts::add"]),
            (
                "src/math.ts::scale::double#0",
                EXACT,
                &["src/math.ts::double"],
            ),
            (
                "src/math.ts::scale::times#0",
                EXACT,
                &["src/math.ts::scale.times"],
            ),
            (
                "src/shapes/shape.ts::Shape.describe::this.area#0",
                EXACT,
                &["src/shapes/shape.ts::Shape.area"],
            ),
            // Imports: named through `../` without extension, named with alias, default.
            (
                "src/shapes/circle.ts::Circle.area::square#0",
                EXACT,
                &["src/math.ts::square"],
            ),
            ("src/app.ts::main::plus#0", EXACT, &["src/math.ts::add"]),
            (
                "src/app.ts::main::double#0",
                EXACT,
                &["src/math.ts::double"],
            ),
            ("src/app.ts::main::scale#0", EXACT, &["src/math.ts::scale"]),
            // A namespace import whose `.js` extension names the `.ts` file.
            (
                "src/app.ts::main::numbers.square#0",
                EXACT,
                &["src/math.ts::square"],
            ),
            // Re-exports through `index` resolution: a namespace, a named one with its
            // extension, a default renamed.
            ("src/app.ts::main::math.add#0", EXACT, &["src/math.ts::add"]),
            (
                "src/app.ts::main::util.format#0",
                EXACT,
                &["src/util/format.mjs::format"],
            ),
            (
                "src/app.ts::main::log#0",
                EXACT,
                &["src/util/log.js::default"],
            ),
            // The `@/*` alias of `tsconfig.json`, to an `index` file re-exporting a class, then
            // its static method; and from a TSX file.
            (
                "src/app.ts::main::Shape.unit#0",
                EXACT,
                &["src/shapes/shape.ts::Shape.unit"],
            ),
            (
                "src/view.tsx::render::format#0",
                EXACT,
                &["src/util/format.mjs::format"],
            ),
            // A package outside the repository stays unresolved.
            ("src/app.ts::main::useState#0", None, &[]),
            // Only a unique name match finds these: an inherited static method, a method on
            // a parameter, a function no import names.
            (
                "src/app.ts::main::Circle.unit#0",
                INFERRED,
                &["src/shapes/shape.ts::Shape.unit"],
            ),
            (
                "src/app.ts::main::shape.describe#0",
                INFERRED,
                &["src/shapes/shape.ts::Shape.describe"],
            ),
            (
                "src/app.ts::main::legacy#0",
                INFERRED,
                &["src/legacy.cjs::legacy"],
            ),
            // Several definitions fit: all are listed.
            (
                "src/app.ts::main::shape.area#0",
                AMBIGUOUS,
                &[
                    "src/shapes/circle.ts::Circle.area",
                    "src/shapes/shape.ts::Shape.area",
                ],
            ),
            (
                "src/app.ts::main::render#0",
                AMBIGUOUS,
                &["src/view.tsx::render", "src/widget.jsx::render"],
            ),
            // No definition has the name.
            ("src/util/log.js::default::console.log#0", None, &[]),
        ],
    );
}

/// The kafkajs forms of the TS/JS pack (spec #20): every producer and consumer of the
/// fixture, as `(consumer, producer, channel)`, all `exact`. `transport.send` and a client of
/// another package produce nothing.
#[test]
fn typescript_and_javascript_kafkajs() {
    let repo = fixture_repo("conformance_kafkajs", "conformance-ts-js", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let rel = |id: &str| id.strip_prefix(&format!("{REPO}::")).unwrap().to_string();

    let links: Vec<(String, String, String, &str)> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            (
                rel(l["consumer"].as_str().unwrap()),
                rel(l["provider"].as_str().unwrap()),
                l["reason"].as_str().unwrap().to_string(),
                l["level"].as_str().unwrap(),
            )
        })
        .filter(|(_, _, reason, _)| reason.starts_with("channel::kafka::"))
        .collect();

    let consume = "src/messages/consume.ts::";
    let produce = "src/messages/produce.ts::";
    let link = |consumer: &str, producer: &str, channel: &str| {
        (
            consumer.to_string(),
            format!("{produce}{producer}"),
            format!("channel::kafka::{channel}"),
            "exact",
        )
    };
    let orders = format!("{consume}consumeOrders.kafka orders.created orders.shipped");
    let mut expected = vec![
        link(&orders, "produceAll::producer.send#1", "orders.created"),
        link(&orders, "produceAll::producer.send#2", "orders.shipped"),
        link(
            &format!("{consume}consumeRefunds.kafka refunds"),
            "emitRefund::emit#0",
            "refunds",
        ),
        link(
            &format!("{consume}listenRetries"),
            "retry::relay#0",
            "retries",
        ),
        link(
            "src/messages/handlers.ts::onPayment",
            "produceAll::producer.send#0",
            "payments",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
}

/// The amqplib forms of the TS/JS pack (spec #20): `sendToQueue`, `consume`, `bindQueue` and
/// `publish` on a channel that traces back to `amqplib` (a confirm channel too), as
/// `(consumer, producer, reason, level)`.
#[test]
fn typescript_and_javascript_amqplib() {
    let repo = fixture_repo("conformance_amqplib", "conformance-ts-js", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let rel = |id: &str| id.strip_prefix(&format!("{REPO}::")).unwrap().to_string();

    let links: Vec<(String, String, String, &str)> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            (
                rel(l["consumer"].as_str().unwrap()),
                rel(l["provider"].as_str().unwrap()),
                l["reason"].as_str().unwrap().to_string(),
                l["level"].as_str().unwrap(),
            )
        })
        .filter(|(_, _, reason, _)| reason.starts_with("channel::amqp::"))
        .collect();

    let queues = "src/messages/queues.ts::";
    let exchanges = "src/messages/exchanges.ts::";
    let link = |consumer: &str, producer: &str, queue: &str| {
        (
            consumer.to_string(),
            format!("{queues}{producer}"),
            format!("channel::amqp::{queue}"),
            "exact",
        )
    };
    // A publish to the exchange `events` reaches the queues whose binding matches its key.
    let published = |handler: &str, queue: &str, call: usize, binding: &str, level| {
        (
            format!("{exchanges}{handler}"),
            format!("{exchanges}announce::channel.publish#{call}"),
            format!("channel::amqp::{queue} binding events {binding}"),
            level,
        )
    };
    let mut expected = vec![
        link(
            &format!("{queues}work.amqp tasks"),
            "enqueue::channel.sendToQueue#0",
            "tasks",
        ),
        link(
            "src/messages/handlers.ts::onPayment",
            "enqueue::confirming.sendToQueue#0",
            "invoices",
        ),
        link(
            &format!("{queues}workReports"),
            "enqueueReports::queue#0",
            "reports",
        ),
        published("onAudit", "audit", 0, "order.created", "exact"),
        published("onAlert", "alerts", 0, "order.#", "inferred"),
        published("onAlert", "alerts", 1, "order.#", "inferred"),
        published("onShipment", "shipments", 1, "*.shipped", "inferred"),
        // The empty exchange is the queue `tasks`.
        (
            format!("{queues}work.amqp tasks"),
            format!("{exchanges}announce::channel.publish#2"),
            "channel::amqp::tasks".to_string(),
            "exact",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
}
