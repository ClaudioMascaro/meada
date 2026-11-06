//! Constructor-backed GCP SDK use sides (#668).
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

#[test]
fn javascript_and_typescript_gcp_operations_keep_topic_subscription_bucket_and_secret() {
    let (text, json) = export("gcp-sdk-ts");
    let providers = json["providers"].as_array().unwrap();
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(providers.len(), 4, "{text}");
    assert_eq!(consumers.len(), 6, "{text}");
    for suffix in ["js", "ts"] {
        for (role, contracts) in [
            (
                providers,
                vec![
                    format!("channel::pubsub::orders-{suffix}"),
                    format!("resource::gcs::uploads-{suffix}"),
                ],
            ),
            (
                consumers,
                vec![
                    format!("channel::pubsub::subscriptions/orders-{suffix}"),
                    format!("resource::gcs::uploads-{suffix}"),
                    format!("resource::gcpsecretmanager::token-{suffix}"),
                ],
            ),
        ] {
            for contract in contracts {
                assert!(role.iter().any(|s| s["contract"] == contract), "{text}");
            }
        }
    }
    assert_golden("gcp-sdk-ts.export.json", &text);
}

#[test]
fn python_gcp_operations_require_complete_client_origins() {
    let (text, json) = export("gcp-sdk-python");
    let providers = json["providers"].as_array().unwrap();
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(providers.len(), 2, "{text}");
    assert_eq!(consumers.len(), 3, "{text}");
    for (sides, contracts) in [
        (
            providers,
            vec![
                "channel::pubsub::orders-python",
                "resource::gcs::uploads-python",
            ],
        ),
        (
            consumers,
            vec![
                "channel::pubsub::subscriptions/orders-python",
                "resource::gcs::uploads-python",
                "resource::gcpsecretmanager::token-python",
            ],
        ),
    ] {
        for contract in contracts {
            assert!(sides.iter().any(|s| s["contract"] == contract), "{text}");
        }
    }
    assert_golden("gcp-sdk-python.export.json", &text);
}

#[test]
fn java_gcp_operations_verify_builders_and_typed_name_helpers() {
    let (text, json) = export("gcp-sdk-java");
    assert_inventory(&text, &json, "java", 2, 3);
    assert_golden("gcp-sdk-java.export.json", &text);
}

#[test]
fn go_gcp_operations_keep_v2_and_v1_subscription_targets_distinct_from_topics() {
    let (text, json) = export("gcp-sdk-go");
    assert_inventory(&text, &json, "go", 3, 4);
    assert!(
        json["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["contract"] == "channel::pubsub::orders-legacy"),
        "{text}"
    );
    assert!(
        json["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["contract"] == "channel::pubsub::subscriptions/orders-legacy"),
        "{text}"
    );
    assert_golden("gcp-sdk-go.export.json", &text);
}

#[test]
fn csharp_gcp_operations_keep_sync_async_and_named_arguments() {
    let (text, json) = export("gcp-sdk-csharp");
    assert_inventory(&text, &json, "csharp", 3, 5);
    assert_golden("gcp-sdk-csharp.export.json", &text);
}

#[test]
fn workload_environment_preserves_pubsub_operation_subtype_and_typed_targets() {
    let (text, json) = export("gcp-sdk-env");
    let providers = json["providers"].as_array().unwrap();
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(providers.len(), 10, "{text}");
    assert_eq!(consumers.len(), 13, "{text}");
    for language in ["Js", "Python", "Java", "Go", "Csharp"] {
        for (sides, operation, contract) in [
            (providers, "publish", "channel::pubsub::orders-env"),
            (
                consumers,
                "receive",
                "channel::pubsub::subscriptions/orders-env",
            ),
        ] {
            let symbol = format!("{operation}{language}");
            let side = sides
                .iter()
                .find(|s| s["site"].as_str().unwrap().contains(&symbol))
                .unwrap();
            assert_eq!(side["contract"], contract, "{text}");
            assert!(
                side["via"].as_str().unwrap().contains("workload:"),
                "{text}"
            );
        }
    }
    for (sides, symbol, contract) in [
        (providers, "publishPlain", "channel::pubsub::orders-env"),
        (
            providers,
            "publishStructured",
            "channel::pubsub::orders-path",
        ),
        (providers, "publishTyped", "channel::pubsub::orders-env"),
        (
            consumers,
            "receivePlain",
            "channel::pubsub::subscriptions/orders-env",
        ),
        (
            consumers,
            "receiveStructured",
            "channel::pubsub::subscriptions/orders-path",
        ),
        (
            consumers,
            "receiveTyped",
            "channel::pubsub::subscriptions/orders-env",
        ),
        (consumers, "readBucket", "resource::gcs::uploads-env"),
        (
            consumers,
            "readSecret",
            "resource::gcpsecretmanager::token-env",
        ),
    ] {
        let side = sides
            .iter()
            .find(|s| s["site"].as_str().unwrap().contains(&format!("::{symbol}")))
            .unwrap();
        assert_eq!(side["contract"], contract, "{text}");
        assert!(
            side["via"].as_str().unwrap().contains("workload:"),
            "{text}"
        );
    }
    for (sides, symbol, variable) in [
        (providers, "wrongTopic", "TYPED_SUB"),
        (consumers, "wrongSubscription", "TYPED_TOPIC"),
        (consumers, "unknown", "NO_SCOPE"),
    ] {
        let side = sides
            .iter()
            .find(|s| s["site"].as_str().unwrap().contains(&format!("::{symbol}")))
            .unwrap();
        assert_eq!(side["contract"], "channel::pubsub", "{text}");
        assert_eq!(side["via"], format!("env:{variable}"), "{text}");
    }
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 51, "{text}");
    let filtered: Vec<_> = links
        .iter()
        .filter(|l| l["contract"] == "channel::pubsub::subscriptions/orders-env")
        .collect();
    assert_eq!(filtered.len(), 49, "{text}");
    assert!(
        filtered.iter().all(|l| l["level"] == "inferred"
            && l["reason"]
                .as_str()
                .unwrap()
                .contains("subscription google_pubsub_subscription.orders")
            && l["reason"].as_str().unwrap().contains("; filter")),
        "{text}"
    );
    assert!(!text.contains("pending-workload-env:"));
    assert_golden("gcp-sdk-env.export.json", &text);
}

#[test]
fn gcp_sdk_origin_controls_and_factory_call_frames_hold_in_every_group() {
    let (text, json) = export("gcp-sdk-controls");
    let providers = json["providers"].as_array().unwrap();
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(providers.len(), 24, "{text}");
    assert_eq!(consumers.len(), 18, "{text}");
    for language in ["js", "ts"] {
        for suffix in ["a", "b"] {
            let contract = format!("channel::pubsub::structured-{language}-{suffix}");
            assert!(
                providers.iter().any(|p| p["contract"] == contract),
                "{text}"
            );
        }
    }
    for language in ["js", "ts", "python", "java", "go", "csharp"] {
        for suffix in ["a", "b"] {
            let prefix = if language == "python" {
                "resource::gcs"
            } else {
                "channel::pubsub"
            };
            let expected = format!("{prefix}::factory-{language}-{suffix}");
            assert!(
                providers.iter().any(|p| p["contract"] == expected),
                "{text}"
            );
        }
        let name = language.to_uppercase();
        assert!(
            providers
                .iter()
                .any(|p| p["contract"] == "channel::pubsub"
                    && p["via"] == format!("env:TOPIC_{name}")),
            "{text}"
        );
        for (contract, variable) in [
            ("channel::pubsub", "SUB"),
            ("resource::gcs", "BUCKET"),
            ("resource::gcpsecretmanager", "SECRET"),
        ] {
            assert!(
                consumers
                    .iter()
                    .any(|c| c["contract"] == contract
                        && c["via"] == format!("env:{variable}_{name}")),
                "{text}"
            );
        }
    }
    for side in providers.iter().chain(consumers) {
        assert!(
            !side["contract"].as_str().unwrap().contains("forbidden"),
            "{text}"
        );
    }
    for file in [
        "control.js",
        "control.ts",
        "control.py",
        "Control.java",
        "control.go",
        "Control.cs",
    ] {
        assert!(
            json["calls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["span"]["path"] == file
                    && c["id"].as_str().unwrap().contains("::unknown.make")
                    && matches!(c["level"].as_str(), Some("inferred" | "ambiguous")))
                || json["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["span"]["path"] == file
                        && c["id"].as_str().unwrap().contains("::unknown.Make")
                        && matches!(c["level"].as_str(), Some("inferred" | "ambiguous"))),
            "name-matched control was not exercised for {file}: {text}"
        );
    }
    assert_golden("gcp-sdk-controls.export.json", &text);
}

#[test]
fn named_gcp_factory_lattices_share_equivalent_frames_without_losing_later_operations() {
    for (
        language,
        fixture,
        file,
        header,
        signature,
        parameters,
        constructor,
        call,
        operation,
        end,
    ) in [
        (
            "ts",
            "gcp-sdk-ts",
            "Lattice.ts",
            "import {PubSub as Bus} from '@google-cloud/pubsub';\n",
            "function make",
            "flag, name",
            "new Bus().topic(name)",
            "make",
            "conduit.publishMessage({data});",
            "",
        ),
        (
            "python",
            "gcp-sdk-python",
            "lattice.py",
            "from google.cloud import storage as store\n",
            "def make",
            "flag, name",
            "store.Client().bucket(name).blob(unknown)",
            "make",
            "conduit.upload_from_string(data)",
            "",
        ),
        (
            "java",
            "gcp-sdk-java",
            "Lattice.java",
            "import com.google.cloud.pubsub.v1.Publisher;\nclass Lattice {\n",
            "Publisher make",
            "boolean flag, String name",
            "Publisher.newBuilder(name).build()",
            "make",
            "conduit.publish(data);",
            "}\n",
        ),
        (
            "go",
            "gcp-sdk-go",
            "lattice.go",
            "package neutral\nimport bus \"cloud.google.com/go/pubsub/v2\"\n",
            "func make",
            "flag bool, name string",
            "root.Publisher(name)",
            "make",
            "conduit.Publish(ctx, data)",
            "",
        ),
        (
            "csharp",
            "gcp-sdk-csharp",
            "Lattice.cs",
            "using Google.Cloud.PubSub.V1;\nclass Lattice {\n",
            "async Task<PublisherClient> Make",
            "bool flag, string name",
            "await PublisherClient.CreateAsync(name)",
            "await Make",
            "await conduit.PublishAsync(data);",
            "}\n",
        ),
    ] {
        let repo = fixture_repo(&format!("gcp_lattice_{language}"), fixture, None);
        for entry in std::fs::read_dir(&repo).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                std::fs::remove_file(path).unwrap();
            }
        }
        let mut source = header.to_string();
        let result = if language == "go" {
            " *bus.Publisher"
        } else {
            ""
        };
        if language == "python" {
            source += &format!("{signature}0({parameters}):\n    return {constructor}\n");
        } else {
            let setup = if language == "go" {
                "root, err := bus.NewClient(ctx, \"p\"); "
            } else {
                ""
            };
            source +=
                &format!("{signature}0({parameters}){result} {{ {setup}return {constructor}; }}\n");
        }
        for n in 1..=28 {
            let previous = n - 1;
            if language == "python" {
                source += &format!(
                    "{signature}{n}({parameters}):\n    if flag:\n        return {call}{previous}(flag, name)\n    return {call}{previous}(flag, name)\n"
                );
            } else {
                source += &format!(
                    "{signature}{n}({parameters}){result} {{ if (flag) {{ return {call}{previous}(flag, name); }} return {call}{previous}(flag, name); }}\n"
                );
            }
        }
        let binding = match language {
            "ts" => "const conduit =",
            "python" => "conduit =",
            "go" => "conduit :=",
            _ => "var conduit =",
        };
        let truth = if language == "python" { "True" } else { "true" };
        let body = format!(
            "{binding} {call}28({truth}, \"lattice\");\n{}",
            (operation.to_string() + "\n").repeat(128)
        );
        let after = format!("{binding} {call}0({truth}, \"after\");\n{operation}\n");
        if language == "python" {
            source += &format!(
                "def drive():\n{}\ndef after():\n{}\n",
                body.lines()
                    .map(|l| format!("    {l}\n"))
                    .collect::<String>(),
                after
                    .lines()
                    .map(|l| format!("    {l}\n"))
                    .collect::<String>()
            );
            source += &"#".repeat((1 << 20) - source.len() - 1);
            source += "\n";
        } else {
            let method = match language {
                "ts" => "export function",
                "go" => "func",
                "csharp" => "async Task",
                _ => "void",
            };
            source +=
                &format!("{method} drive() {{ {body} }}\n{method} after() {{ {after} }}\n{end}");
            let padding = (1 << 20) - source.len() - 4;
            source += "/*";
            source += &"x".repeat(padding);
            source += "*/";
        }
        assert_eq!(source.len(), 1 << 20);
        std::fs::write(repo.join(file), source).unwrap();
        let started = std::time::Instant::now();
        let text = Engine::open(&repo).unwrap().export().unwrap();
        let elapsed = started.elapsed();
        eprintln!("{language} named GCP SDK lattice at 1MiB: {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_secs(30),
            "{language}: {elapsed:?}"
        );
        let json: Value = serde_json::from_str(&text).unwrap();
        let providers = json["providers"].as_array().unwrap();
        let prefix = if language == "python" {
            "resource::gcs"
        } else {
            "channel::pubsub"
        };
        assert_eq!(providers.len(), 129, "{language}: {text}");
        assert_eq!(
            providers
                .iter()
                .filter(|p| p["contract"] == format!("{prefix}::lattice"))
                .count(),
            128,
            "{language}: {text}"
        );
        assert_eq!(
            providers
                .iter()
                .filter(|p| p["contract"] == format!("{prefix}::after"))
                .count(),
            1,
            "{language}: {text}"
        );
        assert!(
            json["consumers"].as_array().unwrap().is_empty(),
            "{language}: {text}"
        );
    }
}

fn assert_inventory(text: &str, json: &Value, language: &str, writers: usize, readers: usize) {
    let providers = json["providers"].as_array().unwrap();
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(providers.len(), writers, "{text}");
    assert_eq!(consumers.len(), readers, "{text}");
    for (sides, contracts) in [
        (
            providers,
            vec![
                format!("channel::pubsub::orders-{language}"),
                format!("resource::gcs::uploads-{language}"),
            ],
        ),
        (
            consumers,
            vec![
                format!("channel::pubsub::subscriptions/orders-{language}"),
                format!("resource::gcs::uploads-{language}"),
                format!("resource::gcpsecretmanager::token-{language}"),
            ],
        ),
    ] {
        for contract in contracts {
            assert!(sides.iter().any(|s| s["contract"] == contract), "{text}");
        }
    }
}
