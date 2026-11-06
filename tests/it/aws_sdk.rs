//! Constructor-backed AWS SDK use sides (#660).
use crate::common::{assert_golden, fixture_repo};
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
    assert_export_golden("aws-sdk-java.export.json", &text);
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
    assert_export_golden("aws-sdk-go.export.json", &text);
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
    assert_export_golden("aws-sdk-csharp.export.json", &text);
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
    assert_eq!(readers.len(), 6, "{text}");
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
    for contract in [
        "resource::dynamodb::orders",
        "resource::dynamodb::customers",
        "resource::ssm::/java/one",
    ] {
        assert!(readers.iter().any(|r| r["contract"] == contract), "{text}");
    }
    assert!(
        readers
            .iter()
            .any(|r| r["contract"] == "resource::ssm" && r["via"] == "env:JAVA_PARAMETER"),
        "{text}"
    );
    assert_export_golden("aws-sdk-batches.export.json", &text);
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
    assert_export_golden("aws-sdk-origins.export.json", &text);
}

#[test]
fn sdk_resource_links_are_exact_declarations_stay_declared_and_unknowns_never_link() {
    let app = fixture_repo(
        "sdk_resource_links",
        "aws-sdk-go",
        Some("https://example.com/sdk/go.git"),
    );
    let peer = fixture_repo(
        "sdk_resource_links",
        "aws-sdk-csharp",
        Some("https://example.com/sdk/dotnet.git"),
    );
    std::fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/sdk/dotnet\"\n",
    )
    .unwrap();
    std::fs::write(
        app.join("main.tf"),
        "resource \"aws_s3_bucket\" \"uploads\" { bucket = \"uploads\" }\n",
    )
    .unwrap();
    std::fs::write(
        peer.join("meada.toml"),
        "[[resource]]\nkind = \"s3\"\nid = \"uploads\"\nrole = \"read\"\n",
    )
    .unwrap();
    std::fs::remove_file(peer.join("Sends.cs")).unwrap();
    std::fs::write(peer.join("Data.cs"), r#"
using System;
using Amazon.S3;
using Amazon.S3.Model;
class Data {
    async Task Run() {
        var client = new AmazonS3Client();
        await client.PutObjectAsync(new PutObjectRequest { BucketName = Environment.GetEnvironmentVariable("BUCKET") });
        await client.GetObjectAsync(new GetObjectRequest { BucketName = "uploads" });
        await client.GetObjectAsync(new GetObjectRequest { BucketName = Environment.GetEnvironmentVariable("BUCKET") });
    }
}
"#).unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    assert!(
        links
            .iter()
            .all(|l| l["contract"] == "resource::s3::uploads"),
        "{text}"
    );
    assert_eq!(
        links.iter().filter(|l| l["level"] == "exact").count(),
        1,
        "{text}"
    );
    assert_eq!(
        links.iter().filter(|l| l["level"] == "declared").count(),
        1,
        "{text}"
    );
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 4, "{text}");
    assert_eq!(
        provisions.iter().filter(|l| l["level"] == "exact").count(),
        3,
        "{text}"
    );
    assert_eq!(
        provisions
            .iter()
            .filter(|l| l["level"] == "declared")
            .count(),
        1,
        "{text}"
    );
    assert!(
        engine
            .context("resource::s3", Some(8000))
            .unwrap()
            .contains("unresolved")
    );
}

#[test]
fn csharp_dictionary_batches_keep_every_table_key_and_environment_read() {
    let repo = fixture_repo("sdk_dictionary", "aws-sdk-csharp", None);
    std::fs::remove_file(repo.join("Sends.cs")).unwrap();
    std::fs::write(
        repo.join("Batch.cs"),
        r#"
using System;
using System.Collections.Generic;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;
class Batch {
    async Task Read() {
        var client = new AmazonDynamoDBClient();
        await client.BatchGetItemAsync(new BatchGetItemRequest {
            RequestItems = new Dictionary<string, KeysAndAttributes> {
                ["orders"] = new KeysAndAttributes(),
                [Environment.GetEnvironmentVariable("TABLE")] = new KeysAndAttributes(),
            }
        });
    }
}

"#,
    )
    .unwrap();
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(json["providers"].as_array().unwrap().is_empty(), "{text}");
    let readers = json["consumers"].as_array().unwrap();
    assert_eq!(readers.len(), 2, "{text}");
    assert!(
        readers
            .iter()
            .any(|r| r["contract"] == "resource::dynamodb::orders"),
        "{text}"
    );
    assert!(
        readers
            .iter()
            .any(|r| r["contract"] == "resource::dynamodb" && r["via"] == "env:TABLE"),
        "{text}"
    );
}

#[test]
fn sdk_constructor_lattices_visit_shared_returns_once_without_starving_later_calls() {
    for (language, fixture, source, constructor, signature, call, final_methods) in [
        (
            "java",
            "aws-sdk-java",
            "Lattice.java",
            "SnsClient.create()",
            "SnsClient make",
            "make",
            r#"
void lattice() { var client = make40(true); client.publish(PublishRequest.builder().topicArn("lattice").build()); }
void after() { var client = SnsClient.create(); client.publish(PublishRequest.builder().topicArn("after").build()); }
"#,
        ),
        (
            "go",
            "aws-sdk-go",
            "lattice.go",
            "sns.NewFromConfig(cfg)",
            "func make",
            "make",
            r#"
func lattice() { client := make40(true); client.Publish(context.TODO(), &sns.PublishInput{TopicArn: aws.String("lattice")}) }
func after() { client := sns.NewFromConfig(cfg); client.Publish(context.TODO(), &sns.PublishInput{TopicArn: aws.String("after")}) }
"#,
        ),
        (
            "csharp",
            "aws-sdk-csharp",
            "Lattice.cs",
            "new AmazonSimpleNotificationServiceClient()",
            "AmazonSimpleNotificationServiceClient Make",
            "Make",
            r#"
async Task Lattice() { var client = Make40(true); await client.PublishAsync(new PublishRequest { TopicArn = "lattice" }); }
async Task After() { var client = new AmazonSimpleNotificationServiceClient(); await client.PublishAsync(new PublishRequest { TopicArn = "after" }); }
"#,
        ),
    ] {
        let case = format!("sdk_lattice_{language}");
        let repo = fixture_repo(&case, fixture, None);
        for entry in std::fs::read_dir(&repo).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                std::fs::remove_file(path).unwrap();
            }
        }
        let mut text = match language {
            "java" => "import software.amazon.awssdk.services.sns.SnsClient;\nimport software.amazon.awssdk.services.sns.model.PublishRequest;\nclass Lattice {\n".to_string(),
            "go" => "package application\nimport (\"context\"; \"github.com/aws/aws-sdk-go-v2/aws\"; \"github.com/aws/aws-sdk-go-v2/service/sns\")\n".to_string(),
            _ => "using Amazon.SimpleNotificationService;\nusing Amazon.SimpleNotificationService.Model;\nclass Lattice {\n".to_string(),
        };
        let arguments = if language == "go" {
            "choose bool) *sns.Client"
        } else {
            "bool choose)"
        };
        let arguments = if language == "java" {
            "boolean choose)"
        } else {
            arguments
        };
        text += &format!("{signature}0({arguments} {{ return {constructor}; }}\n");
        for n in 1..=40 {
            let previous = n - 1;
            text += &format!(
                "{signature}{n}({arguments} {{ if (choose) {{ return {call}{previous}(choose); }} return {call}{previous}(choose); }}\n"
            );
        }
        text += final_methods;
        if language != "go" {
            text += "}\n";
        }
        let padding = (1 << 20) - text.len() - 4;
        text += "/*";
        text += &"x".repeat(padding);
        text += "*/";
        assert_eq!(text.len(), (1 << 20));
        std::fs::write(repo.join(source), text).unwrap();
        let started = std::time::Instant::now();
        let export = Engine::open(&repo).unwrap().export().unwrap();
        let elapsed = started.elapsed();
        eprintln!("{language} SDK lattice at the file ceiling: {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_secs(30),
            "{language}: {elapsed:?}"
        );
        let json: Value = serde_json::from_str(&export).unwrap();
        let writers = json["providers"].as_array().unwrap();
        assert_eq!(writers.len(), 2, "{language}: {export}");
        assert!(
            writers
                .iter()
                .any(|p| p["contract"] == "channel::sns::lattice"),
            "{language}: {export}"
        );
        assert!(
            writers
                .iter()
                .any(|p| p["contract"] == "channel::sns::after"),
            "{language}: {export}"
        );
        assert!(
            json["consumers"].as_array().unwrap().is_empty(),
            "{language}: {export}"
        );
    }
}

fn assert_export_golden(name: &str, text: &str) {
    let mut export = meada_core::export::parse(text).unwrap();
    export.commit = None;
    export.dirty = None;
    export.processes.clear();
    assert_golden(name, &export.write().unwrap());
}

#[test]
fn sdk_environment_aliases_keep_provenance_and_hidden_java_collections_stay_unknown() {
    let repo = fixture_repo("sdk_alias_environment", "aws-sdk-alias-env", None);
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let writers = json["providers"].as_array().unwrap();
    let readers = json["consumers"].as_array().unwrap();
    assert_eq!(writers.len(), 2, "{text}");
    assert_eq!(readers.len(), 7, "{text}");
    assert!(
        writers.iter().all(|w| w["contract"] == "resource::s3"),
        "{text}"
    );
    for variable in ["env:GO_BUCKET", "env:CS_BUCKET"] {
        assert!(writers.iter().any(|w| w["via"] == variable), "{text}");
        assert!(
            readers
                .iter()
                .any(|r| r["contract"] == "resource::s3" && r["via"] == variable),
            "{text}"
        );
    }
    assert_eq!(
        readers
            .iter()
            .filter(|r| r["contract"] == "resource::dynamodb" && r["via"].is_null())
            .count(),
        3,
        "{text}"
    );
    assert!(json["links"].as_array().unwrap().is_empty(), "{text}");
    assert_eq!(
        readers
            .iter()
            .filter(|r| r["contract"] == "resource::s3" && r["via"].is_null())
            .count(),
        2,
        "{text}"
    );
    assert_export_golden("aws-sdk-alias-env.export.json", &text);
}
