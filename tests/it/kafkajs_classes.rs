//! kafkajs clients held in private class fields, and topics read from app-config (front 6,
//! Task 6). Each topic reaches the kafkajs call through calls that resolve `exact` (a method of
//! the class called on `this`); a call of a method several classes define is `ambiguous`, and
//! its topic does not lift (`a_shared_name_wrapper_call_is_an_expected_miss`).

use std::path::{Path, PathBuf};

use meada::engine::Engine;
use serde_json::Value;

use crate::common::fixture_repo;

const APPROVED: &str = "channel::kafka::orders.approved";

/// The `kafkajs-plugins` fixture, copied for `case`.
fn fixture(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "kafkajs-plugins",
        Some("https://example.com/acme/plugins.git"),
    )
}

fn export(case: &str, change: impl FnOnce(&Path)) -> Value {
    let repo = fixture(case);
    change(&repo);
    serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap()
}

/// The channel contracts of the `role` (`providers` or `consumers`) sides whose site holds `at`.
fn sides(export: &Value, role: &str, at: &str) -> Vec<String> {
    let mut found: Vec<String> = (export[role].as_array().unwrap().iter())
        .filter(|s| s["site"].as_str().unwrap().contains(at))
        .filter_map(|s| s["contract"].as_str())
        .filter(|c| c.starts_with("channel::"))
        .map(String::from)
        .collect();
    found.sort();
    found
}

/// The levels of the links of `contract`, sorted.
fn levels(export: &Value, contract: &str) -> Vec<String> {
    let mut found: Vec<String> = (export["links"].as_array().unwrap().iter())
        .filter(|l| l["contract"] == contract)
        .map(|l| l["level"].as_str().unwrap().to_string())
        .collect();
    found.sort();
    found
}

#[test]
fn a_kafkajs_client_in_a_class_field_makes_sides() {
    let export = export("kafkajs_sides", |_| {});
    assert_eq!(sides(&export, "providers", "::plugins/orders/"), [APPROVED]);
    assert_eq!(
        sides(&export, "consumers", "::plugins/approvals/"),
        [APPROVED]
    );
}

/// Orders' consumer is set in `connect`, not in the constructor; no call gives its `subscribe` a
/// topic, so its side has none.
#[test]
fn a_consumer_field_set_in_a_method_is_a_client() {
    let export = export("kafkajs_method_field", |_| {});
    let consumers = sides(
        &export,
        "consumers",
        "::plugins/orders/src/KafkaClient.ts::",
    );
    assert_eq!(consumers, ["channel::kafka"]);
}

#[test]
fn a_hash_private_field_is_a_client() {
    let export = export("kafkajs_hash_field", |_| {});
    let consumers = sides(
        &export,
        "consumers",
        "::plugins/approvals/src/KafkaClient.ts::",
    );
    assert_eq!(consumers, [APPROVED]);
}

#[test]
fn a_topic_from_app_config_is_exact() {
    let export = export("kafkajs_app_config", |_| {});
    assert_eq!(levels(&export, APPROVED), ["exact"]);
}

#[test]
fn two_app_config_values_give_one_inferred_side_each() {
    let export = export("kafkajs_two_values", |repo| {
        let production = "acme:\n  orders:\n    topics:\n      approved: orders.v2\n";
        std::fs::write(repo.join("app-config.production.yaml"), production).unwrap();
    });
    let v2 = "channel::kafka::orders.v2";
    assert_eq!(
        sides(&export, "providers", "::plugins/orders/"),
        [APPROVED, v2]
    );
    assert_eq!(
        sides(&export, "consumers", "::plugins/approvals/"),
        [APPROVED, v2]
    );
    assert_eq!(levels(&export, APPROVED), ["inferred"]);
    assert_eq!(levels(&export, v2), ["inferred"]);
}

#[test]
fn a_topic_from_a_runtime_input_has_no_name() {
    let export = export("kafkajs_runtime_input", |_| {});
    let action = sides(&export, "providers", "::plugins/actions/");
    assert_eq!(action, ["channel::kafka"]);
}

/// `publishFromRoute` calls `client.send(config.getString(..))` on a `KafkaClient`, but three
/// classes define `send`: the call is `ambiguous`, so `expand` (exact callers only) does not lift
/// the topic to it, and the route has no side. Receiver-type method resolution is a front of its
/// own.
#[test]
fn a_shared_name_wrapper_call_is_an_expected_miss() {
    let export = export("kafkajs_shared_name", |_| {});
    let route = "::plugins/orders/src/publish.ts::publishFromRoute";
    let send = (export["calls"].as_array().unwrap().iter())
        .find(|c| c["caller"].as_str().unwrap().ends_with(route) && c["level"] == "ambiguous");
    assert!(send.is_some(), "no ambiguous call from {route}");
    assert!(sides(&export, "providers", "::plugins/orders/src/publish.ts::").is_empty());
}

#[test]
fn a_public_field_is_not_a_client() {
    let export = export("kafkajs_public_field", |_| {});
    assert!(!export.to_string().contains("orders.public"));
}

#[test]
fn a_field_assigned_from_something_else_is_not_a_client() {
    let export = export("kafkajs_mixed_field", |_| {});
    assert!(!export.to_string().contains("orders.mixed"));
}

#[test]
fn an_app_config_value_that_is_no_channel_name_names_nothing() {
    let export = export("kafkajs_bad_name", |repo| {
        let partial = "topics:\n  approved: \"orders approved\"\n";
        std::fs::write(repo.join("partials/orders.yaml"), partial).unwrap();
    });
    assert!(
        !export
            .to_string()
            .contains("channel::kafka::orders approved")
    );
    assert!(!export.to_string().contains(APPROVED));
}
