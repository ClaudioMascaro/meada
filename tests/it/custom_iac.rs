//! Curated and repository-configured infrastructure abstractions (#671).

use meada::engine::{Direction, Engine, Start};
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
        ("[[iac.module]]\nsource = \"a\"\nentry = \"terraform:google_cloud_run_service\"\ninput = \"name\"\n".into(), "not supported for a module mapping"),
    ] {
        std::fs::write(root.join("meada.toml"), text).unwrap();
        let error = match Engine::open(&root) { Err(error) => error.to_string(), Ok(_) => panic!("invalid mapping was accepted") };
        assert!(error.contains(wanted), "{error}");
    }
}

#[test]
fn composition_namespace_patches_invalidate_only_implicit_reference_scopes() {
    let root = fixture_repo(
        "custom_namespace_patches",
        "custom-namespace-patches",
        Some("https://example.com/custom-671-namespace/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let value: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let mut actual = provisions(&value)
        .into_iter()
        .map(|(contract, node, root)| {
            assert_eq!(root, ".");
            (contract, node.rsplit_once("::").unwrap().1.to_string())
        })
        .collect::<Vec<_>>();
    actual.sort();
    assert_eq!(
        actual,
        [
            (
                "channel::pubsub::original-topic".into(),
                "PubSubTopic.pubsub.cnrm.cloud.google.com/oldscope/topic".into()
            ),
            (
                "channel::pubsub::replacement-topic".into(),
                "PubSubTopic.pubsub.cnrm.cloud.google.com/newscope/topic".into()
            ),
            (
                "channel::pubsub::subscriptions/audit-explicit".into(),
                "Composition.apiextensions.crossplane.io/scope/subscription-explicit".into()
            ),
            (
                "channel::pubsub::subscriptions/audit-literal".into(),
                "Composition.apiextensions.crossplane.io/scope/subscription-literal".into()
            ),
            (
                "channel::pubsub::subscriptions/audit-unchanged".into(),
                "Composition.apiextensions.crossplane.io/scope/subscription-unchanged".into()
            ),
            (
                "resource::cloudsql::original-instance".into(),
                "SQLInstance.sql.cnrm.cloud.google.com/oldscope/instance".into()
            ),
            (
                "resource::cloudsql::original-instance/orders-explicit".into(),
                "Composition.apiextensions.crossplane.io/scope/database-explicit".into()
            ),
            (
                "resource::cloudsql::original-instance/orders-literal".into(),
                "Composition.apiextensions.crossplane.io/scope/database-literal".into()
            ),
            (
                "resource::cloudsql::original-instance/orders-unchanged".into(),
                "Composition.apiextensions.crossplane.io/scope/database-unchanged".into()
            ),
            (
                "resource::cloudsql::replacement-instance".into(),
                "SQLInstance.sql.cnrm.cloud.google.com/newscope/instance".into()
            ),
        ],
        "{value}"
    );
    let mut subscriptions = value["subscriptions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["source"].as_str().unwrap(), s["target"].as_str().unwrap()))
        .collect::<Vec<_>>();
    subscriptions.sort();
    assert_eq!(
        subscriptions,
        [
            (
                "channel::pubsub::original-topic",
                "channel::pubsub::subscriptions/audit-explicit"
            ),
            (
                "channel::pubsub::original-topic",
                "channel::pubsub::subscriptions/audit-literal"
            ),
            (
                "channel::pubsub::original-topic",
                "channel::pubsub::subscriptions/audit-unchanged"
            ),
        ]
    );
    let context = engine
        .context(
            "Composition.apiextensions.crossplane.io/scope/database-implicit",
            Some(12000),
        )
        .unwrap();
    assert!(context.contains("name: \"orders\""), "{context}");
    assert!(
        !context.contains("resource::cloudsql::original-instance/orders"),
        "{context}"
    );
    let explicit = engine
        .context(
            "Composition.apiextensions.crossplane.io/scope/database-explicit",
            Some(12000),
        )
        .unwrap();
    assert!(
        explicit.contains("resource::cloudsql::original-instance/orders-explicit"),
        "{explicit}"
    );
}

#[test]
fn equivalent_mapping_key_paths_never_create_directionless_self_uses() {
    let root = fixture_repo(
        "custom_quoted_definition",
        "custom-quoted-definition",
        Some("https://example.com/custom-671-quoted/infra.git"),
    );
    // Keep repository provenance equal while changing only the mapping spelling.
    let path = root.join("main.tf.json");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, format!("{source}\n")).unwrap();
    let engine = Engine::open(&root).unwrap();
    let ask = || serde_json::from_str::<Value>(&engine.export().unwrap()).unwrap();
    let check = |value: &Value| {
        assert_eq!(
            provisions(value),
            [
                (
                    "resource::s3::custom-assets".into(),
                    "example.com/custom-671-quoted/infra::store.json::Store.review.invalid/store"
                        .into(),
                    ".".into()
                ),
                (
                    "resource::s3::module-assets".into(),
                    "example.com/custom-671-quoted/infra::main.tf.json::module.store".into(),
                    ".".into()
                ),
            ]
        );
        for field in ["uses", "provisions", "providers", "consumers", "links"] {
            assert!(
                value
                    .get(field)
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty),
                "unexpected {field}: {value}"
            );
        }
    };
    let quoted = ask();
    check(&quoted);
    let config = include_str!("../fixtures/custom-quoted-definition/meada.toml");
    std::fs::write(
        root.join("meada.toml"),
        config
            .replace("settings[\"bucket\"]", "settings.bucket")
            .replace("spec[\"bucket\"]", "spec.bucket"),
    )
    .unwrap();
    assert_eq!(ask(), quoted);
    std::fs::write(root.join("main.tf.json"), r#"{"module":{"store":{"source":"private/storage","settings":{"items":[{"bucket":"module-assets"}]}}}}"#).unwrap();
    std::fs::write(root.join("store.json"), r#"{"apiVersion":"review.invalid/v1","kind":"Store","metadata":{"name":"store"},"spec":{"items":[{"bucket":"custom-assets"}]}}"#).unwrap();
    let array = config
        .replace(
            "settings[\"bucket\"]",
            "settings[\"items\"][00][\"bucket\"]",
        )
        .replace("spec[\"bucket\"]", "spec[\"items\"][00][\"bucket\"]");
    std::fs::write(root.join("meada.toml"), &array).unwrap();
    let quoted_array = ask();
    check(&quoted_array);
    std::fs::write(
        root.join("meada.toml"),
        array
            .replace(
                "settings[\"items\"][00][\"bucket\"]",
                "settings.items[0].bucket",
            )
            .replace("spec[\"items\"][00][\"bucket\"]", "spec.items[0].bucket"),
    )
    .unwrap();
    assert_eq!(ask(), quoted_array);
    std::fs::write(root.join("main.tf.json"), r#"{"variable":{"unknown":{}},"module":{"store":{"source":"private/storage","settings":{"bucket":"module-assets","unread":"${var.unknown}"}}}}"#).unwrap();
    std::fs::write(
        root.join("store.json"),
        include_str!("../fixtures/custom-quoted-definition/store.json"),
    )
    .unwrap();
    std::fs::write(root.join("meada.toml"), config).unwrap();
    let unread = ask();
    assert_eq!(
        provisions(&unread),
        [(
            "resource::s3::custom-assets".into(),
            "example.com/custom-671-quoted/infra::store.json::Store.review.invalid/store".into(),
            ".".into()
        )]
    );
    for field in ["uses", "provisions", "providers", "consumers", "links"] {
        assert!(
            unread
                .get(field)
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty),
            "unexpected {field}: {unread}"
        );
    }
}

#[test]
fn registry_modules_create_the_named_resources_and_workloads_and_honor_disabled_inputs() {
    let root = fixture_repo(
        "custom_registry",
        "custom-registry",
        Some("https://example.com/custom-671-registry/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let value: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        provisions(&value),
        [
            (
                "channel::sns::registered-notices".into(),
                "example.com/custom-671-registry/infra::main.tf::module.topic".into(),
                ".".into()
            ),
            (
                "channel::sqs::registered-parcels".into(),
                "example.com/custom-671-registry/infra::main.tf::module.queue".into(),
                ".".into()
            ),
            (
                "resource::dynamodb::registered-ledger".into(),
                "example.com/custom-671-registry/infra::main.tf::module.table".into(),
                ".".into()
            ),
            // `putin_khuylo` gates the bucket upstream: `false` or unknown leaves none (#787).
            (
                "resource::s3::confirmed-assets".into(),
                "example.com/custom-671-registry/infra::main.tf::module.confirmed_bucket".into(),
                ".".into()
            ),
            (
                "resource::s3::registered-assets".into(),
                "example.com/custom-671-registry/infra::main.tf::module.bucket".into(),
                ".".into()
            ),
        ]
    );
    let consumers = value["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 2, "{value}");
    assert_eq!(
        value["consumers"],
        serde_json::json!([
            {"contract": "image::registry.example.com/registry/worker", "site": "example.com/custom-671-registry/infra::main.tf::module.function", "via": null, "root": "."},
            {"contract": "image::registry.example.com/registry/service", "site": "example.com/custom-671-registry/infra::main.tf::module.service", "via": null, "root": "."}
        ])
    );
    for (query, name) in [
        ("module.function", "registered-worker"),
        ("module.service", "registered-service"),
    ] {
        let context = engine.context(query, Some(12000)).unwrap();
        assert!(context.contains(name), "{context}");
        assert!(
            context.contains("image::registry.example.com/registry/"),
            "{context}"
        );
    }
}

#[test]
fn private_module_workloads_use_the_supported_lambda_and_ecs_input_seams() {
    let root = fixture_repo(
        "custom_private_workloads",
        "custom-private",
        Some("https://example.com/custom-671-private-workloads/infra.git"),
    );
    std::fs::write(root.join("meada.toml"), "[[iac.module]]\nsource = \"private/function\"\nentry = \"terraform:aws_lambda_function\"\ninput = \"workload_name\"\n[[iac.module]]\nsource = \"private/service\"\nentry = \"terraform:aws_ecs_task_definition\"\ninput = \"settings.name\"\n").unwrap();
    std::fs::write(
        root.join("main.tf"),
        r#"module "function" {
 source = "private/function"
 workload_name = "processor"
 package_type = "Image"
 image_uri = "registry.example.com/private/function:1"
}


module "service" {
 source = "private/service"
 settings = { name = "processor-service" }
 container_definitions = { worker = { image = "registry.example.com/private/service:1" } }
}
module "off" {
 source = "private/function"
 workload_name = "off"
 package_type = "Image"
 image_uri = "registry.example.com/private/off:1"
 create = false
}
"#,
    )
    .unwrap();
    let value: Value =
        serde_json::from_str(&Engine::open(&root).unwrap().export().unwrap()).unwrap();
    assert_eq!(
        value["consumers"],
        serde_json::json!([
            {"contract":"image::registry.example.com/private/function","site":"example.com/custom-671-private-workloads/infra::main.tf::module.function","via":null,"root":"."},
            {"contract":"image::registry.example.com/private/service","site":"example.com/custom-671-private-workloads/infra::main.tf::module.service","via":null,"root":"."}
        ])
    );
    assert!(provisions(&value).is_empty(), "{value}");
}

#[test]
fn structured_patterns_cannot_fall_back_to_the_wrong_mapped_kind() {
    let root = fixture_repo(
        "custom_structured_patterns",
        "custom-structured-patterns",
        Some("https://example.com/custom-671-patterns/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let value: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        provisions(&value),
        [
            (
                "channel::servicebus::batch-*".into(),
                "example.com/custom-671-patterns/infra::main.tf.json::module.azure".into(),
                ".".into()
            ),
            (
                "channel::sqs::batch-*".into(),
                "example.com/custom-671-patterns/infra::main.tf.json::module.known_arn".into(),
                ".".into()
            ),
            (
                "channel::sqs::batch-*".into(),
                "example.com/custom-671-patterns/infra::main.tf.json::module.plain".into(),
                ".".into()
            ),
            (
                "resource::gcs::batch-*".into(),
                "example.com/custom-671-patterns/infra::main.tf.json::module.gcp".into(),
                ".".into()
            ),
        ],
        "{value}"
    );
    for field in ["uses", "provisions", "providers", "consumers", "links"] {
        assert!(
            value
                .get(field)
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty),
            "unexpected {field}: {value}"
        );
    }
    let context = engine.context("module.arm", Some(12000)).unwrap();
    assert!(context.contains("/subscriptions/"), "{context}");
    assert!(
        !context.contains("channel::sqs::/subscriptions/"),
        "{context}"
    );
}

#[test]
fn registry_lambda_source_and_package_inputs_use_checked_paths_and_shared_environment_resolution() {
    let root = fixture_repo(
        "custom_module_code",
        "custom-module-code",
        Some("https://example.com/custom-671-code/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let asks = || serde_json::from_str::<Value>(&engine.export().unwrap()).unwrap();
    let executions = |value: &Value| {
        let mut rows = value["executions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                assert_eq!(row["level"], "exact", "{row}");
                (
                    row["workload"]
                        .as_str()
                        .unwrap()
                        .rsplit_once("::")
                        .unwrap()
                        .1
                        .to_string(),
                    row["path"].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>();
        rows.sort();
        rows
    };
    let value = asks();
    assert_eq!(
        executions(&value),
        [
            ("module.local_function".into(), "worker".into()),
            (
                "module.package_function".into(),
                "deploy/function.zip".into()
            )
        ]
    );
    let providers = value["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 1, "{value}");
    assert_eq!(providers[0]["contract"], "channel::sqs::module-tasks");
    let provisions = value["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 1, "{value}");
    assert_eq!(provisions[0]["level"], "inferred");
    let context = engine
        .context("module.local_function", Some(12000))
        .unwrap();
    assert!(context.contains("source_path"), "{context}");
    assert!(context.contains("worker"), "{context}");

    std::fs::remove_dir_all(root.join("worker")).unwrap();
    assert_eq!(
        executions(&asks()),
        [(
            "module.package_function".into(),
            "deploy/function.zip".into()
        )]
    );
    #[cfg(unix)]
    {
        let outside = root.parent().unwrap().join("outside-source");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("send.ts"),
            include_str!("../fixtures/custom-module-code/worker/send.ts"),
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, root.join("worker")).unwrap();
        assert_eq!(
            executions(&asks()),
            [(
                "module.package_function".into(),
                "deploy/function.zip".into()
            )]
        );
        std::fs::remove_file(root.join("worker")).unwrap();
    }
    std::fs::create_dir_all(root.join("worker")).unwrap();
    std::fs::write(
        root.join("worker/send.ts"),
        include_str!("../fixtures/custom-module-code/worker/send.ts"),
    )
    .unwrap();
    assert_eq!(executions(&asks()), executions(&value));
}

#[test]
fn registry_lambda_and_ecs_environments_follow_their_own_image_and_container() {
    let case = "custom_registry_environments";
    let root = fixture_repo(
        case,
        "custom-registry",
        Some("https://example.com/custom-671-env/infra.git"),
    );
    let function = fixture_repo(
        case,
        "custom-registry-function-app",
        Some("https://example.com/custom-671-env/function-app.git"),
    );
    let service = fixture_repo(
        case,
        "custom-registry-service-app",
        Some("https://example.com/custom-671-env/service-app.git"),
    );
    std::fs::write(root.join("meada.toml"), "[[peer]]\nrepo = \"example.com/custom-671-env/function-app\"\n[[peer]]\nrepo = \"example.com/custom-671-env/service-app\"\n").unwrap();
    let path = root.join("main.tf");
    let source = std::fs::read_to_string(&path).unwrap()
        .replace("  package_type = \"Image\"", "  environment_variables = { QUEUE_NAME = \"function-target\" }\n  package_type = \"Image\"")
        .replace("worker = { image = \"registry.example.com/registry/service:1\" }", "worker = { image = \"registry.example.com/registry/service:1\", environment = [{ name = \"QUEUE_NAME\", value = \"service-target\" }] }\n    disabled = { create = false, image = \"registry.example.com/registry/service:1\", environment = [{ name = \"QUEUE_NAME\", value = \"wrong-disabled\" }] }");
    std::fs::write(path, source).unwrap();
    for (repo, target) in [
        (&function, "channel::sqs::function-target"),
        (&service, "channel::sqs::service-target"),
    ] {
        let value: Value =
            serde_json::from_str(&Engine::open(repo).unwrap().export().unwrap()).unwrap();
        let providers = value["providers"].as_array().unwrap();
        assert_eq!(providers.len(), 2, "{value}");
        let channels = providers
            .iter()
            .filter(|side| side["contract"].as_str().unwrap().starts_with("channel::"))
            .collect::<Vec<_>>();
        assert_eq!(channels.len(), 1, "{value}");
        assert_eq!(channels[0]["contract"], target, "{value}");
        assert!(
            channels[0]["via"]
                .as_str()
                .is_some_and(|via| via.contains("QUEUE_NAME")),
            "{value}"
        );
    }
}

#[test]
fn rabbitmq_operator_bindings_route_by_exchange_type_without_inventing_exchange_channels() {
    let root = fixture_repo(
        "custom_rabbit",
        "custom-rabbit",
        Some("https://example.com/custom-671-rabbit/infra.git"),
    );
    let engine = Engine::open(&root).unwrap();
    let value: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let actual = provisions(&value)
        .into_iter()
        .map(|(contract, _, _)| contract)
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            "channel::amqp::broadcast",
            "channel::amqp::composed-inbox",
            "channel::amqp::direct-inbox",
            "channel::amqp::events",
            "channel::amqp::inbox"
        ]
    );
    let bindings = value["bindings"].as_array().unwrap();
    assert_eq!(bindings.len(), 4, "{value}");
    let mut rows = bindings
        .iter()
        .map(|b| {
            (
                b["contract"].as_str().unwrap(),
                b["exchange"].as_str().unwrap(),
                b["pattern"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    assert_eq!(
        rows,
        [
            ("channel::amqp::broadcast", "fan", "#"),
            (
                "channel::amqp::composed-inbox",
                "composed-exchange",
                "order.created"
            ),
            ("channel::amqp::direct-inbox", "direct", "order.created"),
            ("channel::amqp::inbox", "events", "order.*"),
        ]
    );
    let links = value["links"].as_array().unwrap();
    assert_eq!(links.len(), 4, "{value}");
    let mut deliveries = links
        .iter()
        .map(|l| {
            (
                l["contract"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    deliveries.sort();
    assert_eq!(
        deliveries,
        [
            ("channel::amqp::broadcast", "inferred"),
            ("channel::amqp::composed-inbox", "exact"),
            ("channel::amqp::direct-inbox", "exact"),
            ("channel::amqp::inbox", "inferred")
        ]
    );
    assert_eq!(value["provisions"].as_array().unwrap().len(), 9, "{value}");
    let binding = "Binding.rabbitmq.com/topic-bind";
    let context = engine.context(binding, Some(12000)).unwrap();
    assert!(
        context.contains("Exchange.rabbitmq.com/event-object"),
        "{context}"
    );
    assert!(
        context.contains("Queue.rabbitmq.com/inbox-object"),
        "{context}"
    );
    let down = engine
        .trace(Start::From(binding), Direction::Down, Some(12000))
        .unwrap();
    assert!(down.contains("onInbox"), "{down}");
    let up = engine
        .trace(Start::From("emitTopic"), Direction::Up, Some(12000))
        .unwrap();
    assert!(up.contains("Queue.rabbitmq.com/inbox-object"), "{up}");
    let exchange = engine
        .context("Exchange.rabbitmq.com/event-object", Some(12000))
        .unwrap();
    assert!(exchange.contains("name: \"events\""), "{exchange}");
}
