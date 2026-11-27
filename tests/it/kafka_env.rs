//! A Kafka topic read from the environment carries its variable, so a workload that sets it
//! names the side (front 6, Task 3).

use meada::engine::Engine;
use serde_json::Value;

use crate::common::fixture_repo;

/// The export of the `kafka-env` fixture, copied for `case`, after `change` edits the copy.
fn export(case: &str, change: impl FnOnce(&std::path::Path)) -> Value {
    let repo = fixture_repo(
        case,
        "kafka-env",
        Some("https://example.com/acme/kafka-env.git"),
    );
    change(&repo);
    serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap()
}

/// The side of `role` (`providers` or `consumers`) whose site is in `file`.
fn side<'e>(export: &'e Value, role: &str, file: &str) -> &'e Value {
    let at = format!("::{file}::");
    (export[role].as_array().unwrap().iter())
        .find(|s| s["site"].as_str().unwrap().contains(&at))
        .unwrap_or_else(|| panic!("no {role} in {file}: {export}"))
}

/// The link between a provider in `from` and a consumer in `to`.
fn link<'e>(export: &'e Value, from: &str, to: &str) -> &'e Value {
    let (from, to) = (format!("::{from}::"), format!("::{to}::"));
    (export["links"].as_array().unwrap().iter())
        .find(|l| {
            l["provider"].as_str().unwrap().contains(&from)
                && l["consumer"].as_str().unwrap().contains(&to)
        })
        .unwrap_or_else(|| panic!("no link {from} -> {to}: {export}"))
}

#[test]
fn a_go_reader_topic_from_an_env_field_resolves_through_compose() {
    let export = export("kafka_env_go", |_| {});
    let reader = side(&export, "consumers", "go/consumer.go");
    assert_eq!(reader["contract"], "channel::kafka::orders.created");
    let found = link(&export, "tools/literal.ts", "go/consumer.go");
    assert_eq!(found["level"], "inferred", "{found}");
    assert!(
        found["reason"].as_str().unwrap().contains("workload:"),
        "{found}"
    );
}

#[test]
fn a_go_writer_topic_from_an_env_field_resolves_through_compose() {
    let export = export("kafka_env_writer", |_| {});
    let writer = side(&export, "providers", "go/consumer.go");
    assert_eq!(writer["contract"], "channel::kafka::orders.created");
    let found = link(&export, "go/consumer.go", "tools/literal.ts");
    assert_eq!(found["level"], "inferred", "{found}");
    assert!(
        found["reason"].as_str().unwrap().contains("workload:"),
        "{found}"
    );
}

#[test]
fn a_kafkajs_topic_from_process_env_resolves_through_compose() {
    let export = export("kafka_env_ts", |_| {});
    let producer = side(&export, "providers", "ts/producer.ts");
    assert_eq!(producer["contract"], "channel::kafka::orders.created");
    let found = link(&export, "ts/producer.ts", "tools/literal.ts");
    assert_eq!(found["level"], "inferred", "{found}");
    assert!(
        found["reason"].as_str().unwrap().contains("workload:"),
        "{found}"
    );
}

#[test]
fn without_a_workload_the_side_stays_unresolved() {
    let export = export("kafka_env_none", |repo| {
        std::fs::remove_file(repo.join("docker-compose.yml")).unwrap();
    });
    assert_eq!(
        side(&export, "consumers", "go/consumer.go")["contract"],
        "channel::kafka"
    );
    assert_eq!(
        side(&export, "providers", "go/consumer.go")["contract"],
        "channel::kafka"
    );
    assert_eq!(
        side(&export, "providers", "ts/producer.ts")["contract"],
        "channel::kafka"
    );
}

fn embedded(case: &str, config: &str) -> Value {
    let repo = fixture_repo(case, "go-embedded-config", None);
    let file = repo.join("consumer.go");
    let src = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        file,
        src.replace("*config.Config", &format!("*config.{config}")),
    )
    .unwrap();
    serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap()
}

fn assert_orders(export: &Value) {
    let consumer = side(export, "consumers", "consumer.go");
    assert_eq!(consumer["contract"], "channel::kafka::orders.created");
    assert!(
        consumer["via"]
            .as_str()
            .unwrap()
            .contains("env:ORDERS_TOPIC")
    );
    assert_eq!(
        link(export, "producer.go", "consumer.go")["level"],
        "declared"
    );
}

#[test]
fn an_embedded_config_field_names_the_consumers_variable() {
    assert_orders(&embedded("embedded_named", "Named"));
    assert_orders(&embedded("embedded_config", "Config"));
}

#[test]
fn an_embedded_pointer_or_qualified_type_names_the_field() {
    assert_orders(&embedded("embedded_pointer", "Pointer"));
    assert_orders(&embedded("embedded_qualified", "Qualified"));
}

#[test]
fn a_promoted_read_has_no_topic() {
    let export = embedded("embedded_promoted", "Config");
    assert_eq!(
        side(&export, "consumers", "promoted.go")["contract"],
        "channel::kafka"
    );
}
