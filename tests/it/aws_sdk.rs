//! Constructor-backed AWS SDK use sides (#660).
use crate::common::{assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
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
    assert_eq!(readers[0]["contract"], "resource::s3");
    assert_eq!(readers[0]["via"], "env:BUCKET");
    assert_eq!(readers[1]["contract"], "resource::s3::uploads");
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
fn sdk_clients_require_construction_or_a_declared_sqs_type_and_follow_only_exact_factory_hops() {
    let repo = fixture_repo("sdk_origins", "aws-sdk-origins", None);
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let writers = json["providers"].as_array().unwrap();
    let mut contracts = writers
        .iter()
        .map(|w| w["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    contracts.sort_unstable();
    // A declared SNS client stays without a side; SQS keeps its earlier injected-client forms.
    assert_eq!(
        contracts,
        [
            "channel::sns::positive",
            "channel::sns::positive",
            "channel::sns::positive",
            "channel::sqs::declared",
            "channel::sqs::declared",
            "channel::sqs::declared",
        ],
        "{text}"
    );
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
    assert_eq!(links.len(), 3, "{text}");
    assert!(
        links
            .iter()
            .all(|l| l["contract"] == "resource::s3::uploads"),
        "{text}"
    );
    assert_eq!(
        links.iter().filter(|l| l["level"] == "exact").count(),
        2,
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
    let sdk = links
        .iter()
        .find(|l| {
            l["level"] == "exact"
                && l["consumer"]
                    .as_str()
                    .unwrap()
                    .starts_with("example.com/sdk/dotnet::")
        })
        .unwrap();
    assert!(
        links
            .iter()
            .any(|l| l["level"] == "exact"
                && l["consumer"] == "example.com/sdk/go::main.go::useBucket"),
        "{text}"
    );
    let reader = sdk["consumer"].as_str().unwrap();
    let writer = sdk["provider"].as_str().unwrap();
    let up = engine
        .trace(Start::From(reader), Direction::Up, Some(8000))
        .unwrap();
    assert!(
        up.contains("example.com/sdk/go main.go:12 useBucket"),
        "{up}"
    );
    assert!(up.contains("[exact: resource::s3::uploads]"), "{up}");
    let down = engine
        .trace(Start::From(writer), Direction::Down, Some(8000))
        .unwrap();
    assert!(down.contains("example.com/sdk/dotnet Data.cs:"), "{down}");
    assert!(!down.contains("(top level)"), "{down}");
    let peer_export: Value =
        serde_json::from_str(&Engine::open(&peer).unwrap().export().unwrap()).unwrap();
    assert!(
        peer_export["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == reader),
        "resource readers are symbols: {reader}"
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

#[test]
fn typescript_aws_commands_require_the_sdk_client_origin() {
    let repo = fixture_repo(
        "aws_sdk_ts",
        "aws-sdk",
        Some("https://example.com/aws/app.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 2, "{export}");
    assert_eq!(
        export["providers"][1]["contract"],
        "resource::s3::orders-bucket"
    );
    assert_eq!(export["providers"][0]["contract"], "channel::sns::orders");
    assert_golden(
        "aws-sdk.context-sns.txt",
        &engine.context("channel::sns::orders", None).unwrap(),
    );
}

#[test]
fn typescript_aws_resources_keep_directions_and_environment_names() {
    let repo = fixture_repo(
        "aws_sdk_ts_resources",
        "aws-sdk",
        Some("https://example.com/aws/app.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let sides = export["consumers"].as_array().unwrap();
    assert_eq!(sides.len(), 2, "{export}");
    assert_eq!(sides[0]["contract"], "resource::s3::orders-bucket");
    assert_eq!(sides[1]["contract"], "resource::s3");
    assert_eq!(sides[1]["via"], "env:BUCKET");
    assert_golden(
        "aws-sdk.context-s3.txt",
        &engine.context("store", None).unwrap(),
    );
}

#[test]
fn python_aws_clients_keep_directions_and_environment_names() {
    let repo = fixture_repo(
        "aws_sdk_python",
        "aws-sdk-python",
        Some("https://example.com/aws/python.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 2, "{export}");
    assert_eq!(export["consumers"].as_array().unwrap().len(), 2, "{export}");
    assert_eq!(export["providers"][0]["contract"], "channel::sns::orders");
    assert_eq!(export["consumers"][1]["contract"], "resource::s3");
    assert_eq!(export["consumers"][1]["via"], "env:BUCKET");
    assert_golden(
        "aws-sdk-python.context-s3.txt",
        &engine.context("store", None).unwrap(),
    );
}

#[test]
fn aws_sdk_operation_matrix_has_only_the_requested_sides() {
    let repo = fixture_repo(
        "aws_sdk_matrix",
        "aws-sdk-matrix",
        Some("https://example.com/aws/matrix.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let providers = export["providers"].as_array().unwrap();
    let consumers = export["consumers"].as_array().unwrap();
    assert_eq!(providers.len(), 25, "{export}");
    assert_eq!(consumers.len(), 24, "{export}");
    let mut actual = Vec::new();
    for (role, sides) in [("writer", providers), ("reader", consumers)] {
        for side in sides {
            actual.push(format!(
                "{role} {} {} {}",
                side["site"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("example.com/aws/matrix::")
                    .unwrap(),
                side["contract"].as_str().unwrap(),
                side["via"].as_str().unwrap_or("")
            ));
        }
    }
    actual.sort();
    assert_golden("aws-sdk-matrix.sides.txt", &(actual.join("\n") + "\n"));
}

#[test]
fn hidden_aws_client_and_command_names_never_feed_contracts() {
    let typescript = [
        (
            "parameter",
            "function run(sns: unknown) { sns.send(new PublishCommand({TopicArn:'forbidden'})); }",
        ),
        (
            "catch",
            "try { fail(); } catch (sns) { sns.send(new PublishCommand({TopicArn:'forbidden'})); }",
        ),
        (
            "destructure",
            "function run() { const {sns} = other; sns.send(new PublishCommand({TopicArn:'forbidden'})); }",
        ),
        (
            "array",
            "function run() { const [sns] = other; sns.send(new PublishCommand({TopicArn:'forbidden'})); }",
        ),
        (
            "destructured_parameter",
            "function run({sns}) { sns.send(new PublishCommand({TopicArn:'forbidden'})); }",
        ),
        (
            "command_parameter",
            "function run(PublishCommand) { sns.send(new PublishCommand({TopicArn:'forbidden'})); }",
        ),
        (
            "name_hop",
            "function make() { return new SNSClient({}); } const unknown = object.make(); unknown.send(new PublishCommand({TopicArn:'forbidden'}));",
        ),
        (
            "unused_command",
            "new PublishCommand({TopicArn:'forbidden'});",
        ),
        (
            "wrong_client",
            "const wrong = new OtherClient({}); wrong.send(new PublishCommand({TopicArn:'forbidden'}));",
        ),
        (
            "wrong_command",
            "sns.send(new OtherCommand({TopicArn:'forbidden'}));",
        ),
    ];
    let python = [
        (
            "parameter",
            "def run(sns):\n    sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "with",
            "def run():\n    with other() as sns:\n        sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "except",
            "def run():\n    try: other()\n    except Exception as sns:\n        sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "walrus",
            "def run():\n    (sns := other())\n    sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "comprehension",
            "def run():\n    return [sns.publish(TopicArn='forbidden') for sns in other()]\n",
        ),
        (
            "lambda",
            "def run():\n    return lambda sns: sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "global",
            "def run():\n    global sns\n    sns = other()\n    sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "nonlocal",
            "def run():\n    sns = boto3.client('sns')\n    def nested():\n        nonlocal sns\n        sns = other()\n        sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "capture",
            "def run(value):\n    match value:\n        case sns:\n            sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "local_import",
            "def run():\n    import other as sns\n    sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "class_import",
            "class Run:\n    import other as sns\n    sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "unpack",
            "def run():\n    sns, unused = other()\n    sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "attribute_unpack",
            "class Run:\n    def __init__(self):\n        self.sns = boto3.client('sns')\n    def run(self):\n        self.sns, unused = other()\n        self.sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "closure",
            "def run(sns):\n    def nested():\n        sns.publish(TopicArn='forbidden')\n",
        ),
        (
            "name_hop",
            "class Maker:\n    def make(self):\n        return boto3.client('sns')\ndef run(value):\n    client = value.make()\n    client.publish(TopicArn='forbidden')\n",
        ),
        (
            "constructor",
            "def run(boto3):\n    client = boto3.client('sns')\n    client.publish(TopicArn='forbidden')\n",
        ),
    ];
    for (extension, prefix, cases) in [
        (
            "ts",
            "import {SNSClient, PublishCommand} from '@aws-sdk/client-sns';\nimport {OtherClient, OtherCommand} from 'other';\nconst sns = new SNSClient({});\n",
            typescript.as_slice(),
        ),
        (
            "py",
            "import boto3\nsns = boto3.client('sns')\n",
            python.as_slice(),
        ),
    ] {
        for resource in [false, true] {
            for (name, source) in cases {
                let repo = fixture_repo(
                    &format!("aws_sdk_hidden_{extension}_{name}_{resource}"),
                    "aws-sdk-controls",
                    None,
                );
                let source = format!("{prefix}{source}\n");
                let source = if resource {
                    source
                        .replace("SNSClient", "S3Client")
                        .replace("PublishCommand", "GetObjectCommand")
                        .replace("client-sns", "client-s3")
                        .replace(".publish(", ".get_object(")
                        .replace("TopicArn", "Bucket")
                        .replace("'sns'", "'s3'")
                } else {
                    source
                };
                std::fs::write(repo.join(format!("app.{extension}")), source).unwrap();
                let text = Engine::open(&repo).unwrap().export().unwrap();
                let export: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(
                    export["providers"],
                    serde_json::json!([]),
                    "{extension} {name}: {text}"
                );
                assert_eq!(
                    export["consumers"],
                    serde_json::json!([]),
                    "{extension} {name}: {text}"
                );
            }
        }
    }
}

#[test]
fn literal_sdk_resource_links_are_exact_and_declared_sides_stay_declared() {
    let app = fixture_repo(
        "aws_sdk_levels",
        "aws-sdk",
        Some("https://example.com/aws/app.git"),
    );
    let infra = fixture_repo(
        "aws_sdk_levels",
        "aws-sdk-controls",
        Some("https://example.com/aws/infra.git"),
    );
    std::fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/aws/infra\"\n",
    )
    .unwrap();
    std::fs::write(app.join("writer.py"), "import boto3\ns3 = boto3.client('s3')\ndef write():\n    s3.put_object(Bucket='orders-bucket')\n").unwrap();
    std::fs::write(infra.join("main.tf"), "resource \"aws_s3_bucket\" \"orders\" { bucket = \"orders-bucket\" }\nresource \"aws_sns_topic\" \"orders\" { name = \"orders\" }\n").unwrap();
    std::fs::write(infra.join("meada.toml"), "[[peer]]\nrepo = \"example.com/aws/app\"\n[[resource]]\nkind = \"s3\"\nid = \"orders-bucket\"\nrole = \"read-write\"\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let export: Value = serde_json::from_str(&text).unwrap();
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 5, "{text}");
    assert_eq!(
        links.iter().filter(|l| l["level"] == "exact").count(),
        2,
        "{text}"
    );
    assert_eq!(
        links.iter().filter(|l| l["level"] == "declared").count(),
        3,
        "{text}"
    );
    let provisions = export["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 4, "{text}");
    assert!(provisions.iter().all(|p| p["level"] == "exact"), "{text}");
    let infra_text = Engine::open(&infra).unwrap().export().unwrap();
    let infra_export: Value = serde_json::from_str(&infra_text).unwrap();
    let all = infra_export["provisions"].as_array().unwrap();
    assert_eq!(all.len(), 5, "{infra_text}");
    assert_eq!(
        all.iter().filter(|p| p["level"] == "declared").count(),
        1,
        "{infra_text}"
    );
    assert_golden(
        "aws-sdk.links-context.txt",
        &engine.context("resource::s3::orders-bucket", None).unwrap(),
    );
}

#[test]
fn aws_batch_keys_and_condition_checks_preserve_names_and_direction() {
    let repo = fixture_repo("aws_sdk_computed", "aws-sdk-controls", None);
    std::fs::write(repo.join("app.ts"), r#"import {DynamoDBClient, BatchWriteItemCommand, TransactWriteItemsCommand} from '@aws-sdk/client-dynamodb';
import {SSMClient, GetParametersByPathCommand} from '@aws-sdk/client-ssm';
const db = new DynamoDBClient({});
const ssm = new SSMClient({});
db.send(new BatchWriteItemCommand({RequestItems: {['orders']: []}}));
db.send(new BatchWriteItemCommand({RequestItems: {[process.env.TABLE]: []}}));
db.send(new TransactWriteItemsCommand({TransactItems: [{ConditionCheck: {TableName: 'checked'}}]}));
ssm.send(new GetParametersByPathCommand({Path: '/orders/'}));
"#).unwrap();
    std::fs::write(
        repo.join("app.py"),
        r#"import boto3
import os
db = boto3.client('dynamodb')
ssm = boto3.client('ssm')
db.batch_write_item(RequestItems={'orders': []})
db.batch_write_item(RequestItems={os.getenv('TABLE'): []})
db.transact_write_items(TransactItems=[{'ConditionCheck': {'TableName': 'checked'}}])
ssm.get_parameters_by_path(Path='/orders/')
"#,
    )
    .unwrap();
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let export: Value = serde_json::from_str(&text).unwrap();
    let mut providers: Vec<_> = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["contract"].clone(), p["via"].clone()))
        .collect();
    let mut consumers: Vec<_> = export["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["contract"].clone(), p["via"].clone()))
        .collect();
    providers.sort_by_key(|p| p.0.to_string());
    consumers.sort_by_key(|p| p.0.to_string());
    assert_eq!(
        providers,
        vec![
            (
                serde_json::json!("resource::dynamodb"),
                serde_json::json!("env:TABLE")
            ),
            (
                serde_json::json!("resource::dynamodb"),
                serde_json::json!("env:TABLE")
            ),
            (
                serde_json::json!("resource::dynamodb::orders"),
                serde_json::json!("sdk:aws")
            ),
            (
                serde_json::json!("resource::dynamodb::orders"),
                serde_json::json!("sdk:aws")
            ),
        ],
        "{text}"
    );
    assert_eq!(
        consumers,
        vec![
            (
                serde_json::json!("resource::dynamodb::checked"),
                serde_json::json!("sdk:aws")
            ),
            (
                serde_json::json!("resource::dynamodb::checked"),
                serde_json::json!("sdk:aws")
            ),
            (
                serde_json::json!("resource::ssm"),
                serde_json::json!("sdk:aws")
            ),
            (
                serde_json::json!("resource::ssm"),
                serde_json::json!("sdk:aws")
            ),
        ],
        "{text}"
    );
}

#[test]
fn aws_origin_chains_and_inline_factories_stay_bounded_at_the_file_size_ceiling() {
    for extension in ["ts", "py"] {
        let repo = fixture_repo(
            &format!("aws_sdk_bounded_{extension}"),
            "aws-sdk-controls",
            None,
        );
        let mut source = if extension == "ts" {
            "import {SNSClient, PublishCommand} from '@aws-sdk/client-sns';\nimport {S3Client, PutObjectCommand} from '@aws-sdk/client-s3';\nconst sns = new SNSClient({});\nconst s3 = new S3Client({});\nconst c0 = sns;\n".to_string()
        } else {
            "import boto3\nsns = boto3.client('sns')\nc0 = sns\n".into()
        };
        for i in 1..=300 {
            source += &if extension == "ts" {
                format!("const c{i} = c{};\n", i - 1)
            } else {
                format!("c{i} = c{}\n", i - 1)
            };
        }
        for _ in 0..500 {
            source += if extension == "ts" {
                "sns.send(new PublishCommand({TopicArn:'alerts'}));\ns3.send(new PutObjectCommand({Bucket:'bucket'}));\n"
            } else {
                "boto3.client('sns').publish(TopicArn='alerts')\nboto3.client('s3').put_object(Bucket='bucket')\n"
            };
        }
        source += if extension == "ts" {
            "c64.send(new PublishCommand({TopicArn:'short-chain'}));\nc300.send(new PublishCommand({TopicArn:'too-deep'}));\n//"
        } else {
            "c64.publish(TopicArn='short-chain')\nc300.publish(TopicArn='too-deep')\n#"
        };
        let missing = (1 << 20) - source.len();
        source += &"x".repeat(missing);
        std::fs::write(repo.join(format!("app.{extension}")), source).unwrap();
        let started = std::time::Instant::now();
        let text = Engine::open(&repo).unwrap().export().unwrap();
        let elapsed = started.elapsed();
        eprintln!("AWS SDK {extension} 1MiB chain/factory probe: {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_secs(60),
            "{extension}: {elapsed:?}"
        );
        let export: Value = serde_json::from_str(&text).unwrap();
        let providers = export["providers"].as_array().unwrap();
        assert_eq!(
            providers.len(),
            1001,
            "{extension}: {} providers",
            providers.len()
        );
        assert!(
            providers.iter().all(|p| matches!(
                p["contract"].as_str(),
                Some("channel::sns::alerts" | "channel::sns::short-chain" | "resource::s3::bucket")
            )),
            "{text}"
        );
        assert_eq!(export["consumers"], serde_json::json!([]));
    }
}

#[test]
fn sdk_environment_notes_stay_with_their_contract_kind() {
    let repo = fixture_repo(
        "aws_sdk_environment_notes",
        "aws-sdk-controls",
        Some("https://example.com/aws/notes.git"),
    );
    std::fs::write(
        repo.join("app.ts"),
        "import {SNSClient, PublishCommand} from '@aws-sdk/client-sns';\n\
         import {S3Client, PutObjectCommand} from '@aws-sdk/client-s3';\n\
         const s3 = new S3Client({});\n\
         const sns = new SNSClient({});\n\
         export function store() { s3.send(new PutObjectCommand({Bucket: process.env.BUCKET, Key: 'x'})); }\n\
         export function publish() { sns.send(new PublishCommand({TopicArn: process.env.TOPIC, Message: 'x'})); }\n\
         export function request() { fetch(process.env.API_URL + '/orders'); }\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 2, "{export}");
    assert_eq!(export["consumers"].as_array().unwrap().len(), 1, "{export}");
    assert_eq!(export["consumers"][0]["via"], "env:API_URL", "{export}");
    for (symbol, contract, variable, line) in [
        ("store", "resource::s3", "BUCKET", 5),
        ("publish", "channel::sns", "TOPIC", 6),
    ] {
        let context = engine.context(symbol, None).unwrap();
        let (_, contracts) = context.split_once("\ncontracts:\n").unwrap();
        assert_eq!(
            contracts.lines().collect::<Vec<_>>(),
            [
                format!("example.com/aws/notes app.ts:{line} produces {contract} (unresolved)"),
                format!("example.com/aws/notes app.ts:{line} unresolved environment {variable}"),
            ],
            "{context}"
        );
    }
    let context = engine.context("request", None).unwrap();
    let notes: Vec<_> = context
        .lines()
        .filter(|line| line.contains("unresolved environment"))
        .collect();
    assert_eq!(notes, Vec::<&str>::new(), "{context}");
}

#[test]
fn python_sdk_factories_require_every_return_to_construct_the_same_service() {
    let repo = fixture_repo("sdk_python_complete", "aws-sdk-controls", None);
    std::fs::write(
        repo.join("app.py"),
        r#"
import boto3

def mixed(flag):
    if flag:
        return boto3.client("sns")
    return None

def opaque(flag, other):
    if flag:
        return boto3.client("s3")
    return other

def mixed_queue(flag):
    if flag:
        return boto3.client("sqs")
    return None

def bare(flag):
    if flag:
        return boto3.client("sns")
    return

def fallthrough(flag):
    if flag:
        return boto3.client("sns")

def different(flag):
    if flag:
        return boto3.client("sns")
    return boto3.client("s3")

async def asynchronous():
    return boto3.client("sns")

async def awaited_use():
    client = await asynchronous()
    client.publish(TopicArn="certain-awaited")
    pending = asynchronous()
    alias = await pending
    alias.publish(TopicArn="certain-await-alias")
    twice = await alias
    twice.publish(TopicArn="forbidden-double-await")
    session = await boto3.Session()
    bad_session_client = session.client("sns")
    bad_session_client.publish(TopicArn="forbidden-awaited-session")
    other_session = await boto3.session.Session()
    other_client = other_session.client("sns")
    other_client.publish(TopicArn="forbidden-awaited-nested-session")

def session_choices(flag):
    if flag:
        return boto3.Session()
    return boto3.session.Session()

def generator():
    yield 1
    return boto3.client("sns")

def optional(client=None):
    client.publish(TopicArn="forbidden-default")

def known(flag):
    if flag:
        return boto3.client("sns")
    return boto3.Session().client(service_name="sns")

def session(flag):
    if flag:
        return boto3.Session()
    return None

def defaulted(client=boto3.client("sns")):
    return client

def positional(unused, client):
    return client

def identity(client):
    return client

def emit(client):
    client.publish(TopicArn="certain-argument")

class Sender:
    def __init__(self):
        self.client = boto3.client("sns")
    def send(self):
        self.client.publish(TopicArn="certain-field")

def run(flag, other):
    queue = mixed_queue(flag)
    queue.send_message(QueueUrl="https://sqs.us-east-1.amazonaws.com/123456789012/forbidden-sqs")
    queue.receive_message(QueueUrl="https://sqs.us-east-1.amazonaws.com/123456789012/forbidden-sqs")
    bad = mixed(flag)
    bad.publish(TopicArn="forbidden-none")
    reader = opaque(flag, other)
    reader.get_object(Bucket="forbidden-opaque")
    missing = bare(flag)
    missing.publish(TopicArn="forbidden-bare")
    fallen = fallthrough(flag)
    fallen.publish(TopicArn="forbidden-fallthrough")
    wrong = different(flag)
    wrong.publish(TopicArn="forbidden-service")
    parent = session(flag)
    child = parent.client("sns")
    child.publish(TopicArn="forbidden-session")
    coroutine = asynchronous()
    coroutine.publish(TopicArn="forbidden-coroutine")
    iterator = generator()
    iterator.publish(TopicArn="forbidden-generator")
    optional(boto3.client("sns"))
    optional()
    sessions = session_choices(flag)
    session_client = sessions.client("sns")
    session_client.publish(TopicArn="certain-sessions")
    fake_module = boto3().client("sns")
    fake_module.publish(TopicArn="forbidden-module-call")
    default = defaulted()
    default.publish(TopicArn="certain-default")
    packed = defaulted(**other)
    packed.publish(TopicArn="forbidden-kwargs")
    shifted = positional(*other, boto3.client("sns"))
    shifted.publish(TopicArn="forbidden-splat")
    good = known(flag)
    good.publish(TopicArn="certain")
    forwarded = identity(boto3.client("sns"))
    forwarded.publish(TopicArn="certain-forwarded")
    bucket = identity(boto3.client("s3"))
    bucket.get_object(Bucket="certain-bucket")
    emit(boto3.client("sns"))
"#,
    )
    .unwrap();
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let providers = json["providers"].as_array().unwrap();
    let actual: std::collections::BTreeSet<_> = providers
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .collect();
    assert_eq!(providers.len(), 8, "{text}");
    assert_eq!(
        actual,
        [
            "channel::sns::certain",
            "channel::sns::certain-argument",
            "channel::sns::certain-awaited",
            "channel::sns::certain-await-alias",
            "channel::sns::certain-default",
            "channel::sns::certain-field",
            "channel::sns::certain-forwarded",
            "channel::sns::certain-sessions"
        ]
        .into_iter()
        .collect(),
        "{text}"
    );
    let consumers = json["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 1, "{text}");
    assert_eq!(
        consumers[0]["contract"], "resource::s3::certain-bucket",
        "{text}"
    );
}

#[test]
fn python_sdk_return_lattices_remain_bounded_across_repeated_operations() {
    let repo = fixture_repo("sdk_python_lattice", "aws-sdk-controls", None);
    let mut source = String::from("import boto3\ndef f0(client, flag):\n    return client\n");
    for n in 1..=28 {
        source += &format!(
            "def f{n}(client, flag):\n    if flag:\n        return f{}(client, flag)\n    return f{}(client, flag)\n",
            n - 1,
            n - 1
        );
    }
    source += "def run(flag):\n    seed = boto3.client('sns')\n    client = f28(seed, flag)\n";
    for _ in 0..128 {
        source += "    client.publish(TopicArn='lattice')\n";
    }
    source += "    direct = boto3.client('sns')\n    direct.publish(TopicArn='after')\n#";
    source += &"x".repeat((1 << 20) - source.len());
    std::fs::write(repo.join("app.py"), source).unwrap();
    let started = std::time::Instant::now();
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let elapsed = started.elapsed();
    eprintln!("Python repeated SDK lattice at 1MiB: {elapsed:?}");
    assert!(elapsed < std::time::Duration::from_secs(30), "{elapsed:?}");
    let json: Value = serde_json::from_str(&text).unwrap();
    let providers = json["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 129, "{text}");
    assert!(
        providers
            .iter()
            .all(|p| p["contract"] == "channel::sns::lattice"
                || p["contract"] == "channel::sns::after"),
        "{text}"
    );
    assert_eq!(
        providers
            .iter()
            .filter(|p| p["contract"] == "channel::sns::after")
            .count(),
        1,
        "{text}"
    );
    assert!(json["consumers"].as_array().unwrap().is_empty(), "{text}");
}

#[test]
fn python_message_factories_preserve_awaited_aliases_without_guessing_coroutines() {
    let repo = fixture_repo("python_message_async", "aws-sdk-controls", None);
    std::fs::write(
        repo.join("app.py"),
        r#"
import nats
import pika

async def connect():
    return await nats.connect("nats://localhost:4222")

async def run():
    pending = connect()
    invalid = pending
    await invalid.publish("forbidden", b"x")
    client = await pending
    await client.publish("retained", b"x")
    connection = await pika.BlockingConnection()
    channel = connection.channel()
    channel.queue_bind(queue="forbidden", exchange="forbidden", routing_key="route")
    channel.basic_consume(queue="forbidden")
    channel.basic_publish(exchange="forbidden", routing_key="route", body=b"x")
"#,
    )
    .unwrap();
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let providers = json["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 1, "{text}");
    assert_eq!(
        providers[0]["contract"], "channel::nats::retained",
        "{text}"
    );
    assert!(json["consumers"].as_array().unwrap().is_empty(), "{text}");
}
