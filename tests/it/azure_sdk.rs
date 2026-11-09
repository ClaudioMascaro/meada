//! Constructor-backed Azure SDK sides (#670), through the public Engine seam.
use crate::common::fixture_repo;
use meada::engine::Engine;
use serde_json::Value;

fn export(fixture: &str) -> (String, Value) {
    let repo = fixture_repo(fixture, fixture, None);
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let json = serde_json::from_str(&text).unwrap();
    (text, json)
}

fn assert_golden(name: &str, text: &str) {
    let mut export = meada_core::export::parse(text).unwrap();
    export.commit = None;
    export.dirty = None;
    export.processes.clear();
    crate::common::assert_golden(name, &export.write().unwrap());
}

fn inventory(text: &str, json: &Value, languages: &[&str]) {
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 3 * languages.len(), "{text}");
    assert!(links.iter().all(|link| link["level"] == "exact"), "{text}");
    for (role, expected) in [("providers", 3), ("consumers", 5)] {
        let sides = json[role].as_array().unwrap();
        assert_eq!(sides.len(), expected * languages.len(), "{text}");
        for language in languages {
            let contracts = if role == "providers" {
                vec![
                    format!("channel::servicebus::orders-{language}"),
                    format!("channel::eventhubs::events-{language}"),
                    format!("resource::azureblob::uploads-{language}"),
                ]
            } else {
                vec![
                    format!("channel::servicebus::orders-{language}"),
                    format!("channel::servicebus::subscription:orders-{language}/audit"),
                    format!("channel::eventhubs::events-{language}"),
                    format!("resource::azureblob::uploads-{language}"),
                    format!("resource::keyvault::token-{language}"),
                ]
            };
            for contract in contracts {
                assert!(
                    sides.iter().any(|side| side["contract"] == contract),
                    "{text}"
                );
            }
        }
    }
}

#[test]
fn subscription_companions_stay_in_the_selected_container() {
    let (text, json) = export("azure-sdk-env");
    let mut contracts = json["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|side| side["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    contracts.sort();
    let mut expected = Vec::new();
    for _ in 0..6 {
        expected.extend([
            "channel::servicebus::orders/subscriptions/audit",
            "channel::servicebus::shipping/subscriptions/ops",
            "channel::servicebus::blocked/subscriptions/secret",
            "channel::servicebus::subscription:orders/audit",
            "channel::servicebus::subscription:shipping/ops",
        ]);
    }
    for _ in 0..5 {
        expected.extend([
            "channel::servicebus",
            "channel::servicebus::subscription:literal/audit",
            "channel::servicebus::subscription:literal/ops",
            "channel::servicebus::subscription:orders/fixed",
            "channel::servicebus::subscription:shipping/fixed",
            "channel::servicebus::subscription:blocked/fixed",
        ]);
    }
    for language in ["javascript", "typescript", "python", "java", "go", "csharp"] {
        expected.push(match language {
            "javascript" => "channel::servicebus::unaffected-javascript",
            "typescript" => "channel::servicebus::unaffected-typescript",
            "python" => "channel::servicebus::unaffected-python",
            "java" => "channel::servicebus::unaffected-java",
            "go" => "channel::servicebus::unaffected-go",
            _ => "channel::servicebus::unaffected-csharp",
        });
    }
    expected.push("channel::servicebus");
    expected.push("image::ghcr.io/probe/azure-sdk-worker");
    expected.push("channel::servicebus");
    expected.sort();
    assert_eq!(contracts, expected, "{text}");
    assert!(!text.contains("AZURE_SDK_CREDENTIAL_CANARY_670"));
    assert!(!text.contains("secret-subscription-canary-670"));
    assert!(
        ["providers", "consumers"]
            .iter()
            .flat_map(|role| json[role].as_array().unwrap())
            .all(|side| side.get("identity").is_none())
    );
    assert_golden("azure-sdk-env.export.json", &text);
}

#[test]
fn javascript_and_typescript_azure_operations_keep_receiver_names() {
    let (text, json) = export("azure-sdk-ts");
    inventory(&text, &json, &["javascript", "typescript"]);
    assert_golden("azure-sdk-ts.export.json", &text);
}

#[test]
fn python_azure_operations_require_constructed_clients() {
    let (text, json) = export("azure-sdk-python");
    inventory(&text, &json, &["python"]);
    assert_golden("azure-sdk-python.export.json", &text);
}

/// A Java builder held in a local that receives another call names no entity; one nothing
/// else calls keeps its name.
#[test]
fn a_java_azure_builder_changed_after_it_is_made_names_no_entity() {
    let repo = fixture_repo("azure_sdk_java_changed", "azure-sdk-java", None);
    std::fs::write(
        repo.join("Worker.java"),
        "import com.azure.messaging.servicebus.ServiceBusClientBuilder;\nimport com.azure.messaging.eventhubs.EventHubClientBuilder;\nclass Worker {\n\
         void topic() { var r = new ServiceBusClientBuilder().connectionString(connection).receiver().topicName(\"old-topic\").subscriptionName(\"s\"); r.topicName(\"new-topic\"); r.buildClient().receiveMessages(1); }\n\
         void chain() { var q = new ServiceBusClientBuilder().connectionString(connection).receiver().queueName(\"old-chain\"); q.receiveMode(mode).queueName(\"new-chain\"); q.buildClient().receiveMessages(1); }\n\
         void group() { var g = new EventHubClientBuilder().connectionString(connection, \"kept-hub\").consumerGroup(\"g\"); g.consumerGroup(\"g2\"); g.buildConsumerClient().receiveFromPartition(\"0\", 1, position); }\n\
         void rebuilt() { var b = new ServiceBusClientBuilder().connectionString(connection).sender().queueName(\"old\"); b.queueName(\"new\"); b.buildClient().sendMessage(message); }\n\
         void kept() { var k = new ServiceBusClientBuilder().connectionString(connection).sender().queueName(\"kept\"); k.buildClient().sendMessage(message); }\n}\n",
    )
    .unwrap();
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let contracts = (json["providers"].as_array().unwrap().iter())
        .map(|s| s["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(contracts.iter().all(|c| !c.contains("old")), "{text}");
    assert!(contracts.contains(&"channel::servicebus::kept"), "{text}");
    let consumers = (json["consumers"].as_array().unwrap().iter())
        .map(|s| (s["contract"].as_str().unwrap(), s["site"].as_str().unwrap()))
        .collect::<Vec<_>>();
    assert!(consumers.iter().all(|(c, _)| !c.contains("old")), "{text}");
    assert!(
        consumers
            .iter()
            .any(|&(c, s)| c == "channel::servicebus" && s.ends_with("Worker.topic()")),
        "{text}"
    );
    assert!(
        consumers
            .iter()
            .any(|&(c, _)| c == "channel::eventhubs::kept-hub"),
        "{text}"
    );
}

#[test]
fn java_azure_builders_retain_only_the_explicit_entity_names() {
    let (text, json) = export("azure-sdk-java");
    inventory(&text, &json, &["java"]);
    assert_golden("azure-sdk-java.export.json", &text);
}

#[test]
fn go_azure_operations_follow_exact_imported_factories() {
    let (text, json) = export("azure-sdk-go");
    inventory(&text, &json, &["go"]);
    assert_golden("azure-sdk-go.export.json", &text);
}

#[test]
fn csharp_new_azure_receivers_keep_legacy_servicebus_sends() {
    let (text, json) = export("azure-sdk-csharp");
    inventory(&text, &json, &["csharp"]);
    assert_golden("azure-sdk-csharp.export.json", &text);
}

#[test]
fn exact_factories_keep_sibling_names_and_reject_unproven_origins_in_every_group() {
    let (text, json) = export("azure-sdk-origins");
    assert_eq!(json["providers"].as_array().unwrap().len(), 15, "{text}");
    assert_eq!(json["consumers"].as_array().unwrap().len(), 13, "{text}");
    for forbidden in [
        "hidden-",
        "stale-",
        "unsafe-",
        "matched-",
        "unsupported-",
        "handle-only-",
    ] {
        for role in ["providers", "consumers"] {
            assert!(
                json[role]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|side| !side["contract"].as_str().unwrap().contains(forbidden)),
                "{text}"
            );
        }
    }
    for file in [
        "worker.js",
        "worker.ts",
        "worker.py",
        "worker.go",
        "Worker.java",
        "Worker.cs",
    ] {
        assert!(
            json["calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|call| call["span"]["path"] == file
                    && matches!(call["level"].as_str(), Some("inferred" | "ambiguous"))
                    && (call["id"].as_str().unwrap().contains("::unknown.make")
                        || call["id"].as_str().unwrap().contains("::foreign."))),
            "name-matched hop was not exercised for {file}: {text}"
        );
    }
    assert_golden("azure-sdk-origins.export.json", &text);
}

#[test]
fn typed_subscriptions_and_filtered_delivery_preserve_receiver_subtype() {
    let (text, json) = export("azure-sdk-typed-env");
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 6, "{text}");
    for (symbol, expected) in [
        ("receive", "channel::servicebus::subscription:orders/audit"),
        ("queue", "channel::servicebus::orders/subscriptions/audit"),
        ("wrongQueue", "channel::servicebus"),
        ("wrongSubscription", "channel::servicebus"),
        ("wrongService", "channel::servicebus"),
    ] {
        let side = consumers
            .iter()
            .find(|side| {
                side["site"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("::{symbol}"))
            })
            .unwrap();
        assert_eq!(side["contract"], expected, "{text}");
    }
    assert_golden("azure-sdk-typed-env.export.json", &text);
}

#[test]
fn stored_receiver_names_survive_source_removal_and_exclude_constructor_credentials() {
    let repo = fixture_repo(
        "azure_sdk_stored",
        "azure-sdk-env",
        Some("https://example.com/azure-sdk/env.git"),
    );
    let before = Engine::open(&repo).unwrap().export().unwrap();
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    let identities = db
        .prepare("SELECT identity FROM contracts WHERE identity IS NOT NULL")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!identities.is_empty());
    for identity in identities {
        assert!(!identity.contains("AZURE_SDK_CREDENTIAL_CANARY_670"));
        assert!(!identity.contains("Endpoint"));
        assert!(!identity.contains("SharedAccessKey"));
    }
    let manifest = repo.parent().unwrap().join("azure-stored.toml");
    std::fs::write(&manifest,format!("index_dir = \"indexes\"\n[[repository]]\nrepo = \"example.com/azure-sdk/env\"\npath = {:?}\n",repo.to_str().unwrap())).unwrap();
    meada::engine::index_workspace(&manifest).unwrap();
    for path in [
        "worker.ts",
        "worker.js",
        "worker.py",
        "worker.go",
        "Worker.java",
        "Worker.cs",
        "pods.yaml",
        ".github/workflows/build.yml",
    ] {
        std::fs::remove_file(repo.join(path)).unwrap();
    }
    let stored = Engine::open_manifest(&manifest, "example.com/azure-sdk/env")
        .unwrap()
        .export()
        .unwrap();
    let mut expected: Value = serde_json::from_str(&before).unwrap();
    expected.as_object_mut().unwrap().remove("commit");
    expected.as_object_mut().unwrap().remove("dirty");
    assert_eq!(serde_json::from_str::<Value>(&stored).unwrap(), expected);
}

#[test]
fn the_documented_adjacent_operations_and_explicit_constructor_overloads_are_read() {
    let (text, json) = export("azure-sdk-variants");
    assert_eq!(json["providers"].as_array().unwrap().len(), 51, "{text}");
    assert_eq!(json["consumers"].as_array().unwrap().len(), 67, "{text}");
    let providers = json["providers"].as_array().unwrap();
    assert_eq!(
        providers
            .iter()
            .filter(|s| matches!(
                s["contract"].as_str(),
                Some("channel::servicebus" | "channel::eventhubs")
            ))
            .count(),
        4,
        "{text}"
    );
    for file in ["worker.ts", "worker.js"] {
        let reads = json["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|side| {
                side["site"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("::{file}::repeatReads::"))
            })
            .collect::<Vec<_>>();
        assert_eq!(reads.len(), 4, "{text}");
        assert_eq!(
            reads
                .iter()
                .map(|s| s["site"].as_str().unwrap())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4,
            "{text}"
        );
    }
    assert_golden("azure-sdk-variants.export.json", &text);
}

#[test]
fn without_a_workload_environment_reads_stay_unresolved_beside_literal_calls() {
    let repo = fixture_repo("azure_sdk_no_workload", "azure-sdk-env", None);
    std::fs::remove_file(repo.join("pods.yaml")).unwrap();
    std::fs::remove_file(repo.join(".github/workflows/build.yml")).unwrap();
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(json["providers"].as_array().unwrap().is_empty(), "{text}");
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 35, "{text}");
    assert_eq!(
        consumers
            .iter()
            .filter(|side| side["contract"] == "channel::servicebus")
            .count(),
        29,
        "{text}"
    );
    assert_eq!(
        consumers
            .iter()
            .filter(|side| side["via"]
                .as_str()
                .is_some_and(|via| via.starts_with("env:")))
            .count(),
        28,
        "{text}"
    );
    for language in ["javascript", "typescript", "python", "java", "go", "csharp"] {
        assert!(
            consumers
                .iter()
                .any(|side| side["contract"]
                    == format!("channel::servicebus::unaffected-{language}")),
            "{text}"
        );
    }
    assert_golden("azure-sdk-no-workload.export.json", &text);
}

#[test]
fn mutable_factory_arguments_and_memo_keys_share_exact_call_time_evidence() {
    let (text, json) = export("azure-sdk-mutations");
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 6, "{text}");
    let mut contracts = consumers
        .iter()
        .map(|side| side["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    contracts.sort();
    assert_eq!(
        contracts,
        [
            "channel::servicebus",
            "channel::servicebus",
            "channel::servicebus",
            "channel::servicebus::subscription:memo/audit",
            "channel::servicebus::subscription:sibling/audit",
            "channel::servicebus::subscription:snapshot/audit"
        ],
        "{text}"
    );
    assert_golden("azure-sdk-mutations.export.json", &text);
}

#[test]
fn every_service_retains_unscoped_environment_selectors_in_each_source_language() {
    for (fixture, file, language, quote) in [
        ("azure-sdk-ts", "worker.js", "javascript", "'"),
        ("azure-sdk-ts", "worker.ts", "typescript", "'"),
        ("azure-sdk-python", "worker.py", "python", "'"),
        ("azure-sdk-java", "Worker.java", "java", "\""),
        ("azure-sdk-go", "worker.go", "go", "\""),
        ("azure-sdk-csharp", "Worker.cs", "csharp", "\""),
    ] {
        let repo = fixture_repo(&format!("azure_sdk_env_all_{language}"), fixture, None);
        if fixture == "azure-sdk-ts" {
            std::fs::remove_file(repo.join(if file == "worker.js" {
                "worker.ts"
            } else {
                "worker.js"
            }))
            .unwrap();
        }
        let path = repo.join(file);
        let mut source = std::fs::read_to_string(&path).unwrap();
        if language == "python" {
            source = format!("import os\n{source}");
        }
        if language == "go" {
            source = source.replacen("import (", "import \"os\"\nimport (", 1);
        }
        for (name, variable) in [
            ("orders", "BUS"),
            ("events", "HUB"),
            ("uploads", "CONTAINER"),
            ("token", "SECRET"),
        ] {
            let environment = match language {
                "javascript" | "typescript" => format!("process.env.{variable}"),
                "python" => format!("os.getenv('{variable}')"),
                "java" => format!("System.getenv(\"{variable}\")"),
                "go" => format!("os.Getenv(\"{variable}\")"),
                _ => format!("System.Environment.GetEnvironmentVariable(\"{variable}\")"),
            };
            source = source.replace(&format!("{quote}{name}-{language}{quote}"), &environment);
        }
        let positive = match language {
            "javascript" | "typescript" => {
                "export function positive() { bus.createReceiver('positive').receiveMessages(1); }"
            }
            "python" => {
                "def positive():\n    bus.get_queue_receiver('positive').receive_messages(1)\n"
            }
            "java" => {
                "void positive() { new ServiceBusClientBuilder().connectionString(connection).receiver().queueName(\"positive\").buildClient().receiveMessages(1); }"
            }
            "go" => {
                "func positive() { client, _ := bus.NewClient(namespace,credential,nil); receiver, _ := client.NewReceiverForQueue(\"positive\",nil); receiver.ReceiveMessages(ctx,1,nil) }"
            }
            _ => {
                "async Task Positive() { await new ServiceBusClient(connection).CreateReceiver(\"positive\").ReceiveMessageAsync(); }"
            }
        };
        if matches!(language, "java" | "csharp") {
            let at = source.rfind('}').unwrap();
            source.insert_str(at, positive);
        } else {
            source.push('\n');
            source.push_str(positive);
        }
        std::fs::write(path, source).unwrap();
        let text = Engine::open(&repo).unwrap().export().unwrap();
        let json: Value = serde_json::from_str(&text).unwrap();
        for (role, expected) in [
            (
                "providers",
                vec![
                    "channel::servicebus",
                    "channel::eventhubs",
                    "resource::azureblob",
                ],
            ),
            (
                "consumers",
                vec![
                    "channel::servicebus",
                    "channel::servicebus",
                    "channel::eventhubs",
                    "resource::azureblob",
                    "resource::keyvault",
                    "channel::servicebus::positive",
                ],
            ),
        ] {
            let sides = json[role].as_array().unwrap();
            let mut actual = sides
                .iter()
                .map(|side| side["contract"].as_str().unwrap())
                .collect::<Vec<_>>();
            actual.sort();
            let mut expected = expected;
            expected.sort();
            assert_eq!(actual, expected, "{language}: {text}");
            for side in sides {
                if side["contract"] == "channel::servicebus::positive"
                    || (language == "csharp"
                        && role == "providers"
                        && side["contract"] == "channel::servicebus")
                {
                    continue;
                }
                assert!(
                    side["via"]
                        .as_str()
                        .is_some_and(|via| via.starts_with("env:")),
                    "{language}: {text}"
                );
            }
        }
    }
}

#[test]
fn eventhub_blob_and_secret_workload_values_share_literal_identifier_policy() {
    let (text, json) = export("azure-sdk-values-env");
    let mut providers = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|side| side["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    providers.sort();
    assert_eq!(
        providers,
        [
            "channel::eventhubs",
            "channel::eventhubs::events-env",
            "image::ghcr.io/probe/azure-sdk-worker",
            "resource::azureblob::uploads-env"
        ],
        "{text}"
    );
    let mut consumers = json["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|side| side["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    consumers.sort();
    assert_eq!(
        consumers,
        [
            "channel::eventhubs::events-env",
            "image::ghcr.io/probe/azure-sdk-worker",
            "resource::azureblob",
            "resource::azureblob::uploads-env",
            "resource::keyvault",
            "resource::keyvault",
            "resource::keyvault",
            "resource::keyvault::token-env"
        ],
        "{text}"
    );
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 3, "{text}");
    assert!(
        links
            .iter()
            .filter(|link| link["contract"] != "image::ghcr.io/probe/azure-sdk-worker")
            .all(|link| link["level"] == "inferred"
                && link["reason"].as_str().unwrap().contains("workload:")),
        "{text}"
    );
    assert!(!text.contains("private-selector-canary-670"));
    assert_golden("azure-sdk-values-env.export.json", &text);
}

#[test]
fn queue_and_subscription_in_one_handler_survive_private_identity_deduplication() {
    let (text, json) = export("azure-sdk-same-handler");
    let mut consumers = json["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|side| side["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    consumers.sort();
    assert_eq!(
        consumers,
        [
            "channel::servicebus::orders",
            "channel::servicebus::subscription:orders/audit",
            "image::ghcr.io/probe/azure-sdk-worker"
        ],
        "{text}"
    );
    assert_golden("azure-sdk-same-handler.export.json", &text);
}

#[test]
fn receiver_options_preserve_queue_overloads_without_promoting_unknown_second_arguments() {
    let (text, json) = export("azure-sdk-overloads");
    assert!(json["providers"].as_array().unwrap().is_empty(), "{text}");
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 33, "{text}");
    for language in ["javascript", "typescript", "csharp"] {
        for suffix in ["inline", "named", "fields", "factory", "unaffected"] {
            let contract = format!("channel::servicebus::{language}-{suffix}");
            assert_eq!(
                consumers
                    .iter()
                    .filter(|side| side["contract"] == contract)
                    .count(),
                1,
                "{text}"
            );
        }
        let optional = if language == "csharp" {
            "null-named"
        } else {
            "undefined"
        };
        assert!(
            consumers
                .iter()
                .any(|side| side["contract"]
                    == format!("channel::servicebus::{language}-{optional}")),
            "{text}"
        );
        assert!(
            consumers.iter().any(|side| side["contract"]
                == format!("channel::servicebus::subscription:{language}-topic/audit")),
            "{text}"
        );
    }
    assert_eq!(
        consumers
            .iter()
            .filter(|side| side["contract"] == "channel::servicebus")
            .count(),
        12,
        "{text}"
    );
    assert!(
        consumers
            .iter()
            .all(|side| !side["contract"].as_str().unwrap().contains("unproven")),
        "{text}"
    );
    assert_golden("azure-sdk-overloads.export.json", &text);
}

#[test]
fn arrow_parameters_named_undefined_never_make_a_queue_delivery() {
    let (text, json) = export("azure-sdk-arrow-bindings");
    assert_eq!(json["providers"].as_array().unwrap().len(), 2, "{text}");
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 12, "{text}");
    for ext in ["js", "ts"] {
        for name in ["bare", "commented", "parenthesized", "asynchronous"] {
            let site = format!("::worker.{ext}::{name}");
            let side = consumers
                .iter()
                .find(|side| side["site"].as_str().unwrap().ends_with(&site))
                .unwrap_or_else(|| panic!("missing {site}: {text}"));
            assert_eq!(side["contract"], "channel::servicebus", "{text}");
        }
        for (name, entity) in [("unaffected", "shadowed-route"), ("builtin", "builtin")] {
            let site = format!("::worker.{ext}::{name}");
            assert!(
                consumers
                    .iter()
                    .any(|side| side["site"].as_str().unwrap().ends_with(&site)
                        && side["contract"] == format!("channel::servicebus::{entity}-{ext}")),
                "{text}"
            );
        }
    }
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    assert!(
        links.iter().all(|link| link["level"] == "exact"
            && link["consumer"].as_str().unwrap().ends_with("::unaffected")),
        "{text}"
    );
    assert_golden("azure-sdk-arrow-bindings.export.json", &text);
}
