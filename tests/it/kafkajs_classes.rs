use std::path::{Path, PathBuf};

use meada::engine::Engine;
use serde_json::Value;

use crate::common::fixture_repo;

const APPROVED: &str = "channel::kafka::orders.approved";
const SHIPPED: &str = "channel::kafka::orders.shipped";
const APPROVALS: &str = "::plugins/approvals/src/KafkaClient.ts::";

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

const METHOD: &str =
    "::plugins/orders/src/KafkaClient.ts::KafkaClient.publishApproved::this.send#0";
const ROUTE: &str = "::plugins/orders/src/publish.ts::publishFromRoute::client.send#0";

fn levels(export: &Value, contract: &str, provider: &str) -> Vec<String> {
    let mut found: Vec<String> = (export["links"].as_array().unwrap().iter())
        .filter(|l| {
            l["contract"] == contract && l["provider"].as_str().unwrap().ends_with(provider)
        })
        .map(|l| l["level"].as_str().unwrap().to_string())
        .collect();
    found.sort();
    found
}

#[test]
fn a_kafkajs_client_in_a_class_field_makes_sides() {
    let export = export("kafkajs_sides", |_| {});
    for caller in [METHOD, ROUTE] {
        assert_eq!(sides(&export, "providers", caller), [APPROVED]);
    }
    assert_eq!(sides(&export, "consumers", APPROVALS), [APPROVED, SHIPPED]);
}

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
    let consumers = sides(&export, "consumers", APPROVALS);
    assert_eq!(consumers, [APPROVED, SHIPPED]);
}

#[test]
fn a_topic_from_app_config_is_exact() {
    let export = export("kafkajs_app_config", |_| {});
    for caller in [METHOD, ROUTE] {
        assert_eq!(levels(&export, APPROVED, caller), ["exact"]);
    }
}

#[test]
fn two_app_config_values_give_one_inferred_side_each() {
    let export = export("kafkajs_two_values", |repo| {
        let production = "acme:\n  orders:\n    topics:\n      approved: orders.v2\n";
        std::fs::write(repo.join("app-config.production.yaml"), production).unwrap();
    });
    let v2 = "channel::kafka::orders.v2";
    for caller in [METHOD, ROUTE] {
        assert_eq!(sides(&export, "providers", caller), [APPROVED, v2]);
        for topic in [APPROVED, v2] {
            assert_eq!(levels(&export, topic, caller), ["inferred"]);
        }
    }
    assert_eq!(
        sides(&export, "consumers", APPROVALS),
        [APPROVED, SHIPPED, v2]
    );
}

#[test]
fn a_topic_from_a_runtime_input_has_no_name() {
    let export = export("kafkajs_runtime_input", |_| {});
    let action = sides(&export, "providers", "::plugins/actions/");
    assert_eq!(action, ["channel::kafka"]);
}

#[test]
fn a_shared_name_wrapper_lifts_the_topic_from_its_typed_caller() {
    let export = export("kafkajs_shared_name", |_| {});
    let send = export["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"].as_str().unwrap().ends_with(ROUTE))
        .unwrap();
    assert_eq!(send["level"], "exact");
    assert!(
        send["target"]
            .as_str()
            .unwrap()
            .ends_with("::plugins/orders/src/KafkaClient.ts::KafkaClient.send")
    );
    assert_eq!(sides(&export, "providers", ROUTE), [APPROVED]);
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
