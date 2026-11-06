//! Curated and repository-configured infrastructure abstractions (#671).

use meada::engine::Engine;
use serde_json::Value;

use crate::common::fixture_repo;

fn provisions(value: &Value) -> Vec<(String, String, String)> {
    let mut rows = value["provisioners"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            (
                row["contract"].as_str().unwrap().into(),
                row["node"].as_str().unwrap().into(),
                row["root"].as_str().unwrap().into(),
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

#[test]
fn strimzi_provisions_the_topic_name_instead_of_the_kubernetes_object_name() {
    let root = fixture_repo(
        "custom_strimzi",
        "custom-strimzi",
        Some("https://example.com/custom-671/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        export["provisioners"],
        serde_json::json!([{
            "contract": "channel::kafka::parcel-events",
            "node": "example.com/custom-671/infra::topics.yaml::KafkaTopic.kafka.strimzi.io/transport-object",
            "root": "."
        }])
    );
    let context = engine
        .context("KafkaTopic.kafka.strimzi.io/transport-object", Some(12000))
        .unwrap();
    assert!(context.contains("name: \"parcel-events\""), "{context}");
    assert!(
        context.contains("channel::kafka::parcel-events"),
        "{context}"
    );
}

#[test]
fn private_modules_and_custom_resources_follow_exact_sources_prefix_boundaries_and_key_paths() {
    let root = fixture_repo(
        "custom_private",
        "custom-private",
        Some("https://example.com/custom-671-private/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let exported: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        provisions(&exported),
        [
            ("channel::sns::private-priority".into(), "example.com/custom-671-private/infra::main.tf::module.exact".into(), ".".into()),
            ("channel::sqs::private-custom".into(), "example.com/custom-671-private/infra::queue.yaml::MessageQueue.platform.example.com/custom-object".into(), ".".into()),
            ("channel::sqs::private-production".into(), "example.com/custom-671-private/infra::main.tf::module.private".into(), ".".into()),
            ("resource::s3::private-assets".into(), "example.com/custom-671-private/infra::main.tf::module.longest".into(), ".".into()),
        ]
    );
    let context = engine.context("module.private", Some(12000)).unwrap();
    assert!(context.contains("private-production"), "{context}");
    assert!(context.contains("var.stage"), "{context}");
    let unknown = engine
        .search("searchable-unknown", Some("iac"), Some(12000))
        .unwrap();
    assert!(
        unknown.contains("MessageQueue.neighbor.example.com"),
        "{unknown}"
    );

    std::fs::write(root.join("meada.toml"), "").unwrap();
    let removed: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(provisions(&removed).is_empty(), "{removed}");
    std::fs::write(
        root.join("meada.toml"),
        include_str!("../fixtures/custom-private/meada.toml"),
    )
    .unwrap();
    let restored: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(provisions(&restored), provisions(&exported));
}

#[test]
fn an_unknown_private_mapping_entry_is_an_error_naming_the_entry() {
    let root = fixture_repo("custom_bad_entry", "custom-private", None);
    std::fs::write(root.join("meada.toml"), "[[iac.module]]\nsource = \"private/queue\"\nentry = \"terraform:missing_queue\"\ninput = \"queue_name\"\n").unwrap();
    let error = match Engine::open(&root) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("unknown entry was accepted"),
    };
    assert!(error.contains("terraform:missing_queue"), "{error}");
    assert!(error.contains("meada.toml"), "{error}");
}

#[test]
fn every_curated_operator_entry_provisions_its_official_physical_identity() {
    let root = fixture_repo(
        "custom_inventory",
        "custom-inventory",
        Some("https://example.com/custom-671-inventory/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let expected: Vec<Value> =
        serde_json::from_str(include_str!("../fixtures/custom-inventory-oracle.json")).unwrap();
    let mut wanted = expected
        .iter()
        .map(|row| {
            (
                row["contract"].as_str().unwrap().to_string(),
                format!(
                    "example.com/custom-671-inventory/infra::{}::{}",
                    row["path"].as_str().unwrap(),
                    row["address"].as_str().unwrap()
                ),
                ".".to_string(),
            )
        })
        .collect::<Vec<_>>();
    wanted.sort();
    let value: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(provisions(&value), wanted, "{value}");
    for row in expected {
        let context = engine
            .context(row["address"].as_str().unwrap(), Some(12000))
            .unwrap();
        assert!(
            context.contains(&format!("name: {}", row["name"])),
            "{context}"
        );
        assert!(
            context.contains(row["contract"].as_str().unwrap()),
            "{context}"
        );
    }
}

#[test]
fn custom_names_preserve_composites_references_and_unknown_patch_boundaries() {
    let root = fixture_repo(
        "custom_controls",
        "custom-controls",
        Some("https://example.com/custom-671-controls/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let value: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let mut actual = provisions(&value)
        .into_iter()
        .map(|(contract, node, root)| {
            assert_eq!(root, ".");
            assert!(node.starts_with("example.com/custom-671-controls/infra::resources.yaml::"));
            (contract, node.rsplit_once("::").unwrap().1.to_string())
        })
        .collect::<Vec<_>>();
    actual.sort();
    assert_eq!(
        actual,
        [
            (
                "channel::kafka::fallback-topic".into(),
                "KafkaTopic.kafka.strimzi.io/fallback-topic".into()
            ),
            (
                "channel::pubsub::same-channel".into(),
                "PubSubTopic.pubsub.cnrm.cloud.google.com/topic-object".into()
            ),
            (
                "channel::pubsub::subscriptions/same-channel".into(),
                "PubSubSubscription.pubsub.cnrm.cloud.google.com/subscription-object".into()
            ),
            (
                "channel::sqs::composed-inbox".into(),
                "Composition.apiextensions.crossplane.io/inventory/known".into()
            ),
            (
                "resource::cloudsql::composite-db/ledger".into(),
                "Composition.apiextensions.crossplane.io/inventory/database".into()
            ),
            (
                "resource::cloudsql::primary-db".into(),
                "SQLInstance.sql.cnrm.cloud.google.com/instance-object".into()
            ),
            (
                "resource::cloudsql::primary-db/ledger".into(),
                "SQLDatabase.sql.cnrm.cloud.google.com/database-object".into()
            ),
        ]
    );
    let context = engine
        .context(
            "SQLDatabase.sql.cnrm.cloud.google.com/database-object",
            Some(12000),
        )
        .unwrap();
    assert!(
        context.contains("SQLInstance.sql.cnrm.cloud.google.com/instance-object"),
        "{context}"
    );
    let subscriptions = value["subscriptions"].as_array().unwrap();
    assert_eq!(subscriptions.len(), 1, "{value}");
    assert_eq!(subscriptions[0]["source"], "channel::pubsub::same-channel");
    assert_eq!(
        subscriptions[0]["target"],
        "channel::pubsub::subscriptions/same-channel"
    );
}

#[test]
fn private_mapping_inventory_and_key_paths_are_bounded_and_validated() {
    let root = fixture_repo("custom_mapping_limits", "custom-private", None);
    let oversized = (0..257).map(|n| format!("[[iac.module]]\nsource = \"private/{n}\"\nentry = \"terraform:aws_sqs_queue\"\ninput = \"name\"\n")).collect::<String>();
    for (text, wanted) in [
        (oversized, "256 entries"),
        ("[[iac.module]]\nsource = \"a\"\nsource_prefix = \"a\"\nentry = \"terraform:aws_sqs_queue\"\ninput = \"name\"\n".into(), "one of source or source_prefix"),
        ("[[iac.custom_resource]]\ngroup = \"a\"\nkind = \"Queue\"\nentry = \"terraform:aws_sqs_queue\"\nkey_path = \"spec.[broken\"\n".into(), "key path"),
    ] {
        std::fs::write(root.join("meada.toml"), text).unwrap();
        let error = match Engine::open(&root) { Err(error) => error.to_string(), Ok(_) => panic!("invalid mapping was accepted") };
        assert!(error.contains(wanted), "{error}");
    }
}
