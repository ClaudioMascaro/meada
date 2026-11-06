//! Constructor-backed AWS SDK use sides (#660).
use crate::common::fixture_repo;
use meada::engine::Engine;
use serde_json::Value;

#[test]
fn java_sdk_constructor_publishes_to_the_sns_topic() {
    let repo = fixture_repo("java_sdk_sns", "aws-sdk-java", None);
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["providers"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(json["providers"][0]["contract"], "channel::sns::alerts");
    assert!(json["consumers"].as_array().unwrap().is_empty(), "{text}");
}

#[test]
fn go_sdk_bucket_writes_reads_and_environment_provenance() {
    let repo = fixture_repo("go_sdk_bucket", "aws-sdk-go", None);
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let writers = json["providers"].as_array().unwrap();
    let readers = json["consumers"].as_array().unwrap();
    assert_eq!(writers.len(), 1, "{text}");
    assert_eq!(writers[0]["contract"], "resource::s3::uploads");
    assert_eq!(readers.len(), 2, "{text}");
    assert_eq!(readers[0]["contract"], "resource::s3::uploads");
    assert_eq!(readers[1]["contract"], "resource::s3");
    assert_eq!(readers[1]["via"], "env:BUCKET");
}

#[test]
fn csharp_sdk_secret_arn_and_producer_environment_provenance() {
    let repo = fixture_repo("csharp_sdk_secret", "aws-sdk-csharp", None);
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let writers = json["providers"].as_array().unwrap();
    let readers = json["consumers"].as_array().unwrap();
    assert_eq!(writers.len(), 2, "{text}");
    assert!(
        writers.iter().all(|w| w["contract"] == "channel::sns"),
        "{text}"
    );
    assert!(writers.iter().any(|w| w["via"] == "env:TOPIC"), "{text}");
    assert!(
        writers.iter().any(|w| w["via"] == "env:NAMED_TOPIC"),
        "{text}"
    );
    assert_eq!(readers.len(), 1, "{text}");
    assert_eq!(
        readers[0]["contract"],
        "resource::secretsmanager::orders/token"
    );
}

#[test]
fn sdk_batches_keep_every_bus_table_and_parameter() {
    let repo = fixture_repo("sdk_batches", "aws-sdk-batches", None);
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let mut writers: Vec<_> = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["contract"].as_str().unwrap(), s["via"].as_str()))
        .collect();
    writers.sort();
    assert_eq!(
        writers,
        [
            ("channel::eventbridge", Some("env:BUS")),
            ("channel::eventbridge::orders", None),
            ("resource::dynamodb::customers", None),
            ("resource::dynamodb::orders", None),
        ],
        "{text}"
    );
    let readers = json["consumers"].as_array().unwrap();
    assert_eq!(readers.len(), 2, "{text}");
    assert!(
        readers
            .iter()
            .any(|r| r["contract"] == "resource::ssm::/orders/key"),
        "{text}"
    );
    assert!(
        readers
            .iter()
            .any(|r| r["contract"] == "resource::ssm" && r["via"] == "env:PARAM"),
        "{text}"
    );
}

#[test]
fn sdk_clients_require_construction_and_follow_only_exact_factory_hops() {
    let repo = fixture_repo("sdk_origins", "aws-sdk-origins", None);
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let writers = json["providers"].as_array().unwrap();
    assert_eq!(writers.len(), 3, "{text}");
    for side in writers {
        assert_eq!(side["contract"], "channel::sns::positive", "{text}");
    }
    assert!(json["consumers"].as_array().unwrap().is_empty(), "{text}");
}
