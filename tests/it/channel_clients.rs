//! A Client declaration (`[[channel_client]]`): a call that resolves to a declared function or
//! method makes a Kafka side of its role, named by the value at its topic position (front 6,
//! Task 5).

use std::path::{Path, PathBuf};

use meada::engine::{self, Engine};
use serde_json::Value;

use crate::common::fixture_repo;

const CONSUMER: &str = "[[channel_client]]\ncall = \"pkg/kafkaclient/client.go::Client.Consume\"\n\
                        role = \"consumer\"\nbroker = \"kafka\"\n";
const PRODUCER: &str = "[[channel_client]]\ncall = \"example.com/acme/flow.NewProducer\"\n\
                        role = \"producer\"\nbroker = \"kafka\"\n";

/// The `channel-clients` fixture, copied for `case`.
fn fixture(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "channel-clients",
        Some("https://example.com/acme/clients.git"),
    )
}

fn export_of(engine: &Engine) -> Value {
    serde_json::from_str(&engine.export().unwrap()).unwrap()
}

fn export(case: &str, change: impl FnOnce(&Path)) -> Value {
    let repo = fixture(case);
    change(&repo);
    export_of(&Engine::open(&repo).unwrap())
}

/// A `meada.toml` with the chart declaration and the two clients at `consumer` and `producer`.
fn toml(consumer: &str, producer: &str) -> String {
    format!(
        "[[helm_env]]\nkey = \"lib-chart.envs\"\n\n{CONSUMER}topic = \"{consumer}\"\n\n\
         {PRODUCER}topic = \"{producer}\"\n"
    )
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

/// The level of the link of `contract` whose consumer holds `consumer` and provider `provider`.
fn level(export: &Value, contract: &str, consumer: &str, provider: &str) -> String {
    let found = (export["links"].as_array().unwrap().iter()).find(|l| {
        l["contract"] == contract
            && l["consumer"].as_str().unwrap().contains(consumer)
            && l["provider"].as_str().unwrap().contains(provider)
    });
    let found = found.unwrap_or_else(|| panic!("no {contract} link: {export}"));
    found["level"].as_str().unwrap().to_string()
}

#[test]
fn a_declared_interface_method_consumes_the_topic_at_its_argument() {
    let export = export("clients_interface", |_| {});
    let run = "::internal/service/svc.go::Service.Run";
    assert_eq!(
        sides(&export, "consumers", run),
        ["channel::kafka::orders.created"]
    );
    let contract = "channel::kafka::orders.created";
    let level = level(&export, contract, run, "::tools/tools.go::Created");
    assert_eq!(level, "declared");
}

#[test]
fn a_declared_call_lifts_a_parameter_topic_through_exact_callers() {
    let export = export("clients_lift", |_| {});
    let svc = "::internal/service/svc.go::";
    let contract = "channel::kafka::orders.audit";
    assert!(sides(&export, "consumers", svc).contains(&contract.to_string()));
    let level = level(&export, contract, svc, "::tools/tools.go::Audit");
    assert_eq!(level, "exact");
}

#[test]
fn a_declared_external_constructor_produces_the_topic_in_its_config_field() {
    let export = export("clients_constructor", |_| {});
    let main = "::main.go::main";
    assert_eq!(
        sides(&export, "providers", main),
        ["channel::kafka::events.published"]
    );
    let contract = "channel::kafka::events.published";
    let level = level(&export, contract, "::tools/tools.go::Events", main);
    assert_eq!(level, "declared");
}

#[test]
fn a_declared_call_that_matches_nothing_is_a_status_line() {
    let repo = fixture("clients_status");
    let unmatched = "[[channel_client]]\ncall = \"example.com/acme/flow.NewConsumer\"\n\
                     role = \"consumer\"\nbroker = \"kafka\"\ntopic = \"0\"\n";
    let toml = format!("{}\n{unmatched}", toml("1", "0.TopicName"));
    std::fs::write(repo.join("meada.toml"), toml).unwrap();
    Engine::open(&repo).unwrap().export().unwrap();
    let status = engine::status(&repo).unwrap();
    let lines: Vec<&str> = (status.lines())
        .filter(|l| l.contains("channel_client"))
        .collect();
    assert_eq!(lines.len(), 1, "{status}");
    assert!(
        lines[0].contains("example.com/acme/flow.NewConsumer"),
        "{status}"
    );
}

#[test]
fn editing_channel_client_reextracts_on_a_warm_index() {
    let repo = fixture("clients_warm");
    let engine = Engine::open(&repo).unwrap();
    let before = export_of(&engine);
    let main = "::main.go::main";
    let published = ["channel::kafka::events.published"];
    assert_eq!(sides(&before, "providers", main), published);
    let without = format!("[[helm_env]]\nkey = \"lib-chart.envs\"\n\n{CONSUMER}topic = \"1\"\n");
    std::fs::write(repo.join("meada.toml"), without).unwrap();
    let after = export_of(&engine);
    assert!(sides(&after, "providers", main).is_empty(), "{after}");
}

#[test]
fn an_external_client_refuses_a_same_named_function_of_another_import_path() {
    let result = export("clients_external_path", |repo| {
        let manifest = toml("1", "0.TopicName").replace("acme/flow", "acme/kafka");
        std::fs::write(repo.join("meada.toml"), manifest).unwrap();
        let main = repo.join("main.go");
        let src = std::fs::read_to_string(&main).unwrap();
        std::fs::write(
            main,
            src.replace("example.com/acme/flow", "example.com/other/kafka")
                .replace("flow.", "kafka."),
        )
        .unwrap();
    });
    assert!(sides(&result, "providers", "::main.go::main").is_empty());
}

#[test]
fn a_call_by_name_alone_is_not_a_declared_client() {
    let export = export("clients_by_name", |_| {});
    assert!(sides(&export, "consumers", "::other/client.go::").is_empty());
}

#[test]
fn the_concrete_wrapper_keeps_its_topicless_side() {
    let export = export("clients_wrapper", |_| {});
    let wrapper = "::pkg/kafkaclient/client.go::";
    assert_eq!(sides(&export, "consumers", wrapper), ["channel::kafka"]);
}

#[test]
fn a_topic_position_the_call_lacks_leaves_the_side_unresolved() {
    let export = export("clients_missing", |repo| {
        std::fs::write(repo.join("meada.toml"), toml("5", "0.Missing")).unwrap();
    });
    let run = "::internal/service/svc.go::Service.Run";
    assert_eq!(sides(&export, "consumers", run), ["channel::kafka"]);
    assert_eq!(
        sides(&export, "providers", "::main.go::main"),
        ["channel::kafka"]
    );
}

fn ts_clients() -> &'static (Value, String) {
    static RESULT: std::sync::OnceLock<(Value, String)> = std::sync::OnceLock::new();
    RESULT.get_or_init(|| {
        let repo = fixture_repo("ts_channel_clients", "ts-channel-clients", None);
        let export = export_of(&Engine::open(&repo).unwrap());
        (export, engine::status(&repo).unwrap())
    })
}

#[test]
fn a_declared_ts_method_makes_sides_through_typed_receivers() {
    let (export, status) = ts_clients();
    let contract = "channel::kafka::orders.created";
    let provider = "::plugins/a/src/client.ts::publish";
    assert_eq!(sides(export, "providers", provider), [contract]);
    for consumer in [
        "::plugins/a/src/client.ts::subscribe",
        "::plugins/a/src/client.ts::Service.listen",
    ] {
        assert_eq!(sides(export, "consumers", consumer), [contract]);
        assert_eq!(level(export, contract, consumer, provider), "declared");
    }
    for method in ["KafkaClient.sendMessage", "KafkaClient.createConsumer"] {
        assert!(
            !status
                .lines()
                .any(|line| line.contains("channel_client") && line.contains(method)),
            "{status}"
        );
    }
}

#[test]
fn a_same_named_method_of_another_class_makes_no_side() {
    let (export, _) = ts_clients();
    assert!(sides(export, "providers", "::plugins/b/src/client.ts::publish").is_empty());
}

#[test]
fn the_status_line_names_an_unreached_ts_declaration_once() {
    let (_, status) = ts_clients();
    assert_eq!(
        status
            .lines()
            .filter(|line| line.contains("channel_client")
                && line.contains("plugins/a/src/client.ts::KafkaClient.missing"))
            .count(),
        1,
        "{status}"
    );
}

#[test]
fn an_external_entry_is_counted_unreached_once() {
    let (_, status) = ts_clients();
    assert_eq!(
        status
            .lines()
            .filter(|line| line.contains("channel_client")
                && line.contains("example.com/acme/flow.NewConsumer"))
            .count(),
        1,
        "{status}"
    );
}

#[test]
fn a_declared_ts_client_reads_the_configured_argument_field() {
    let repo = fixture_repo("ts_client_topic_field", "ts-channel-clients", None);
    let manifest = repo.join("meada.toml");
    let config = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        manifest,
        config.replacen("topic = \"0\"", "topic = \"1.topic\"", 1),
    )
    .unwrap();
    let file = repo.join("plugins/a/src/client.ts");
    let source = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        file,
        source
            .replace(
                "sendMessage(process.env.ORDERS_TOPIC, \"message\")",
                "sendMessage('message', { topic: 'orders.shipped' })",
            )
            .replace("message: string", "message: { topic: string }"),
    )
    .unwrap();
    let export = export_of(&Engine::open(&repo).unwrap());
    assert_eq!(
        sides(&export, "providers", "::plugins/a/src/client.ts::publish"),
        ["channel::kafka::orders.shipped"]
    );
}
