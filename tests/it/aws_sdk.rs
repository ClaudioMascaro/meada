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

/// A request changed after it was made names no resource: a TypeScript input object whose field
/// is assigned again, and a Java builder held in a local that receives another call. A field
/// left alone, and a builder nothing else calls, keep their names.
#[test]
fn sdk_requests_changed_after_they_are_made_name_no_resource() {
    let repo = fixture_repo(
        "aws_sdk_changed_requests",
        "aws-sdk",
        Some("https://example.com/aws/app.git"),
    );
    std::fs::write(
        repo.join("changed.ts"),
        "import {S3Client, PutObjectCommand} from '@aws-sdk/client-s3';\n\
         import {SNSClient, PublishCommand} from '@aws-sdk/client-sns';\n\
         const s3 = new S3Client({});\nconst sns = new SNSClient({});\n\
         export function store() { const input = {Bucket: 'old', Key: 'k'}; input.Bucket = 'new'; s3.send(new PutObjectCommand(input)); }\n\
         export function publish() { const req = {TopicArn: 'old-topic', Message: 'm'}; req.TopicArn = 'new-topic'; sns.send(new PublishCommand(req)); }\n\
         export function kept() { const input = {Bucket: 'kept', Key: 'k'}; input.Key = 'other'; s3.send(new PutObjectCommand(input)); }\n\
         import {SQSClient, SendMessageCommand} from '@aws-sdk/client-sqs';\nconst sqs = new SQSClient({});\n\
         export function queue() { const i = {QueueUrl: 'https://sqs.us-east-1.amazonaws.com/1/old-queue', MessageBody: 'm'}; i.QueueUrl = 'https://sqs.us-east-1.amazonaws.com/1/new-queue'; sqs.send(new SendMessageCommand(i)); }\n\
         export function queued() { const i = {QueueUrl: 'https://sqs.us-east-1.amazonaws.com/1/kept-queue', MessageBody: 'm'}; i.MessageBody = 'n'; sqs.send(new SendMessageCommand(i)); }\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("Builder.java"),
        "import software.amazon.awssdk.services.sns.SnsClient;\n\
         import software.amazon.awssdk.services.sns.model.PublishRequest;\n\
         class Builder {\n\
         void send() { var sns = SnsClient.builder().build(); var b = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:old\"); b.topicArn(\"arn:aws:sns:us-east-1:123:new\"); sns.publish(b.message(\"m\").build()); }\n\
         void kept() { var sns = SnsClient.builder().build(); var k = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:kept\"); sns.publish(k.build()); }\n\
         void retarget(PublishRequest.Builder x) { x.topicArn(\"arn:aws:sns:us-east-1:123:other\"); }\n\
         void helped() { var sns = SnsClient.create(); var h = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:old-helped\"); retarget(h); sns.publish(h.build()); }\n\
         private final PublishRequest.Builder field = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:old-field\");\n\
         void moved() { field.topicArn(\"arn:aws:sns:us-east-1:123:moved\"); }\n\
         void chained() { var sns = SnsClient.create(); var c = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:old-chained\"); c.message(\"m\").topicArn(\"arn:aws:sns:us-east-1:123:x\"); sns.publish(c.build()); }\n\
         void derived() { var sns = SnsClient.create(); var d = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:old-derived\"); var e = d.message(\"m\"); e.topicArn(\"arn:aws:sns:us-east-1:123:x\"); sns.publish(d.build()); }\n\
         void aliased() { var sns = SnsClient.create(); var a = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:old-aliased\"); var o = a; o.topicArn(\"arn:aws:sns:us-east-1:123:x\"); sns.publish(a.build()); }\n\
         void passed() { var sns = SnsClient.create(); var p = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:old-passed\"); retarget(p.message(\"m\")); sns.publish(p.build()); }\n\
         void fielded() { var sns = SnsClient.create(); sns.publish(field.build()); }\n\
         void logged() { var sns = SnsClient.create(); var l = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:kept-logged\"); System.out.println(l.toString()); log(l.hashCode()); sns.publish(l.build()); }\n\
         void log(int h) {}\n\
         private final PublishRequest.Builder base = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:kept-base\");\n\
         void one() { var sns = SnsClient.create(); sns.publish(base.message(\"a\").build()); }\n\
         void two() { var sns = SnsClient.create(); sns.publish(base.message(\"b\").build()); var t = PublishRequest.builder().topicArn(\"arn:aws:sns:us-east-1:123:kept-twice\"); sns.publish(t.build()); sns.publish(t.message(\"c\").build()); }\n}\n",
    )
    .unwrap();
    // A named entry an inline request holds changes the same way.
    std::fs::write(
        repo.join("Entries.java"),
        "import software.amazon.awssdk.services.eventbridge.EventBridgeClient;\n\
         import software.amazon.awssdk.services.eventbridge.model.PutEventsRequest;\n\
         import software.amazon.awssdk.services.eventbridge.model.PutEventsRequestEntry;\n\
         class Entries {\n\
         void send(boolean prod) { var events = EventBridgeClient.create(); var e = PutEventsRequestEntry.builder().eventBusName(\"old-bus-java\"); if (prod) e.eventBusName(\"new-bus-java\"); events.putEvents(PutEventsRequest.builder().entries(e.build()).build()); }\n\
         void kept() { var events = EventBridgeClient.create(); var k = PutEventsRequestEntry.builder().eventBusName(\"kept-bus-java\"); events.putEvents(PutEventsRequest.builder().entries(k.build()).build()); }\n}\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("Entries.cs"),
        "using System.Collections.Generic;\nusing Amazon.EventBridge;\nusing Amazon.EventBridge.Model;\n\
         class Entries {\n\
         async System.Threading.Tasks.Task Send() { var events = new AmazonEventBridgeClient(); var entry = new PutEventsRequestEntry { EventBusName = \"old-bus-cs\", Source = \"app\" }; entry.EventBusName = \"new-bus-cs\"; await events.PutEventsAsync(new PutEventsRequest { Entries = new List<PutEventsRequestEntry> { entry } }); }\n\
         async System.Threading.Tasks.Task Kept() { var events = new AmazonEventBridgeClient(); await events.PutEventsAsync(new PutEventsRequest { Entries = new List<PutEventsRequestEntry> { new PutEventsRequestEntry { EventBusName = \"kept-bus-cs\" } } }); }\n}\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("go.mod"),
        "module example.com/entries\n\ngo 1.22\n\nrequire github.com/aws/aws-sdk-go-v2 v1.30.0\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("entries.go"),
        "package main\n\nimport (\n\t\"context\"\n\n\t\"github.com/aws/aws-sdk-go-v2/aws\"\n\t\"github.com/aws/aws-sdk-go-v2/config\"\n\t\"github.com/aws/aws-sdk-go-v2/service/eventbridge\"\n\t\"github.com/aws/aws-sdk-go-v2/service/eventbridge/types\"\n\t\"github.com/aws/aws-sdk-go-v2/service/sqs\"\n)\n\n\
         func send(ctx context.Context) {\n\tcfg, _ := config.LoadDefaultConfig(ctx)\n\tclient := eventbridge.NewFromConfig(cfg)\n\tentry := types.PutEventsRequestEntry{EventBusName: aws.String(\"old-bus-go\")}\n\tentry.EventBusName = aws.String(\"new-bus-go\")\n\tclient.PutEvents(ctx, &eventbridge.PutEventsInput{Entries: []types.PutEventsRequestEntry{entry}})\n}\n\n\
         func kept(ctx context.Context) {\n\tcfg, _ := config.LoadDefaultConfig(ctx)\n\tclient := eventbridge.NewFromConfig(cfg)\n\tclient.PutEvents(ctx, &eventbridge.PutEventsInput{Entries: []types.PutEventsRequestEntry{types.PutEventsRequestEntry{EventBusName: aws.String(\"kept-bus-go\")}}})\n}\n\n\
         func replaced(ctx context.Context) {\n\tcfg, _ := config.LoadDefaultConfig(ctx)\n\tclient := sqs.NewFromConfig(cfg)\n\tinput := &sqs.SendMessageInput{QueueUrl: aws.String(\"https://sqs.us-east-1.amazonaws.com/1/old-ptr\")}\n\t*input = sqs.SendMessageInput{QueueUrl: aws.String(\"https://sqs.us-east-1.amazonaws.com/1/new-ptr\")}\n\tclient.SendMessage(ctx, input)\n}\n\n\
         func pointed(ctx context.Context) {\n\tcfg, _ := config.LoadDefaultConfig(ctx)\n\tclient := sqs.NewFromConfig(cfg)\n\tinput := &sqs.SendMessageInput{QueueUrl: aws.String(\"https://sqs.us-east-1.amazonaws.com/1/kept-ptr\")}\n\tclient.SendMessage(ctx, input)\n}\n",
    )
    .unwrap();
    let export: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let contracts = ["providers", "consumers"]
        .iter()
        .flat_map(|side| export[side].as_array().unwrap())
        .map(|s| s["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(
        contracts.iter().all(|c| !c.contains("old")),
        "{contracts:?}"
    );
    for kept in [
        "resource::s3::kept",
        "channel::sns::kept",
        "channel::eventbridge::kept-bus-java",
        "channel::eventbridge::kept-bus-cs",
        "channel::eventbridge::kept-bus-go",
        "channel::sqs::kept-ptr",
        "channel::sqs::kept-queue",
        "channel::sns::kept-base",
        "channel::sns::kept-twice",
        "channel::sns::kept-logged",
    ] {
        assert!(contracts.contains(&kept), "{kept}: {contracts:?}");
    }
}

/// ADR 0026: an EventBridge entry without `EventBusName` sends to `default`, and an SNS phone
/// publish names no topic, in Java, Go and C# as in TypeScript and Python. A request whose
/// fields Meada cannot all see (a `toBuilder()`, a helper's result, a factory of the
/// repository's own named `builder`) still publishes somewhere, so it keeps an unresolved side.
#[test]
fn sdk_omitted_bus_is_default_and_phone_publishes_name_no_topic() {
    let repo = fixture_repo("aws_sdk_defaults", "aws-sdk-defaults", None);
    let export: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let mut sides = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["contract"].as_str().unwrap().starts_with("channel::"))
        .map(|s| {
            let site = s["site"].as_str().unwrap();
            let file = ["Defaults.java", "Defaults.cs", "defaults.go"]
                .into_iter()
                .find(|f| site.contains(f))
                .unwrap_or(site);
            (s["contract"].as_str().unwrap(), file)
        })
        .collect::<Vec<_>>();
    sides.sort();
    assert_eq!(
        sides,
        [
            ("channel::eventbridge::default", "Defaults.cs"),
            ("channel::eventbridge::default", "Defaults.java"),
            ("channel::eventbridge::default", "defaults.go"),
            ("channel::eventbridge::default", "defaults.go"),
            ("channel::sns", "Defaults.java"),
            ("channel::sns", "Defaults.java"),
            ("channel::sns", "defaults.go"),
            ("channel::sns", "defaults.go"),
            ("channel::sns::alerts-cs", "Defaults.cs"),
            ("channel::sns::alerts-go", "defaults.go"),
            ("channel::sns::alerts-java", "Defaults.java"),
        ],
        "{export}"
    );
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
    assert_eq!(providers.len(), 43, "{export}");
    assert_eq!(consumers.len(), 30, "{export}");
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

/// The Java 2.x, Go v2 and .NET operation tables (#719): one call per operation in its own
/// method, so a reader keeps its own site, and the same sides in all three languages.
#[test]
fn aws_sdk_operation_matrix_covers_java_go_and_dotnet() {
    let repo = fixture_repo(
        "aws_sdk_matrix_jvm_go_dotnet",
        "aws-sdk-matrix-jvm-go-dotnet",
        Some("https://example.com/aws/jvm.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let providers = export["providers"].as_array().unwrap();
    let consumers = export["consumers"].as_array().unwrap();
    // Per language: 22 writers and 16 readers, and .NET's blocking `GetObjectMetadata` reader.
    assert_eq!(providers.len(), 66, "{export}");
    assert_eq!(consumers.len(), 49, "{export}");
    let mut actual = Vec::new();
    let mut by_language: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        Default::default();
    for (role, sides) in [("writer", providers), ("reader", consumers)] {
        for side in sides {
            let site = side["site"]
                .as_str()
                .unwrap()
                .strip_prefix("example.com/aws/jvm::")
                .unwrap();
            let contract = side["contract"].as_str().unwrap();
            let via = side["via"].as_str().unwrap_or("");
            actual.push(format!("{role} {site} {contract} {via}"));
            let (file, _) = site.split_once("::").unwrap();
            by_language
                .entry(file.rsplit('.').next().unwrap().into())
                .or_default()
                .insert(format!("{role} {contract} {via}"));
        }
    }
    actual.sort();
    assert_golden(
        "aws-sdk-matrix-jvm-go-dotnet.sides.txt",
        &(actual.join("\n") + "\n"),
    );
    let sets: Vec<_> = by_language.iter().collect();
    assert_eq!(
        sets.iter()
            .map(|(language, _)| language.as_str())
            .collect::<Vec<_>>(),
        ["cs", "go", "java"],
        "{export}"
    );
    assert_eq!(sets[0].1, sets[1].1, "C# and Go name different sides");
    assert_eq!(sets[1].1, sets[2].1, "Go and Java name different sides");
    // A hierarchy path is no parameter name: the reader stays unresolved.
    let by_path: Vec<_> = consumers
        .iter()
        .filter(|s| {
            s["site"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("getparametersbypath")
        })
        .map(|s| s["contract"].as_str().unwrap())
        .collect();
    assert_eq!(by_path, ["resource::ssm"; 3], "{export}");
}

/// A DynamoDB transaction in Java, Go and .NET (#719): each `Put`, `Update` and `Delete` entry
/// writes its own table, each `ConditionCheck` and `Get` reads its own, and a name that is not a
/// literal follows the other name arguments: an environment read keeps its variable, a computed
/// name stays unresolved.
#[test]
fn dynamodb_transactions_keep_every_entry_table_and_direction_in_java_go_and_dotnet() {
    let repo = fixture_repo("aws_sdk_transactions", "aws-sdk-transactions", None);
    let export: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let mut by_language: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (role, sides) in [("writer", "providers"), ("reader", "consumers")] {
        for side in export[sides].as_array().unwrap() {
            let site = side["site"].as_str().unwrap();
            let mut parts = site.split("::").skip(1);
            let file = parts.next().unwrap();
            // The enclosing method, spelled `Transactions.write()`, `write` or `Transactions.Write()`.
            let method = parts.next().unwrap().trim_end_matches("()");
            let method = method.rsplit('.').next().unwrap().to_lowercase();
            by_language
                .entry(file.rsplit('.').next().unwrap().into())
                .or_default()
                .push(format!(
                    "{method} {role} {} {}",
                    side["contract"].as_str().unwrap(),
                    side["via"].as_str().unwrap_or("")
                ));
        }
    }
    let expected = [
        "computed reader resource::dynamodb ",
        "computed writer resource::dynamodb ",
        "fromenvironment reader resource::dynamodb env:CHECKED",
        "fromenvironment writer resource::dynamodb env:TABLE",
        "read reader resource::dynamodb::customers ",
        "read reader resource::dynamodb::orders ",
        "write reader resource::dynamodb::customers ",
        "write writer resource::dynamodb::archive ",
        "write writer resource::dynamodb::orders ",
        "write writer resource::dynamodb::totals ",
    ];
    assert_eq!(by_language.len(), 3, "{export}");
    for (language, mut sides) in by_language {
        sides.sort();
        assert_eq!(sides, expected, "{language}: {export}");
    }
}

/// A transaction entry held in a named value and changed after it is made names no table, as
/// the other named values on a name's path do (a Java builder is read per builder). The same
/// entries left alone keep their tables, so the change is what unnamed them.
#[test]
fn dynamodb_transaction_entries_changed_after_they_are_made_name_no_table() {
    let sides = |changed: bool| {
        let repo = fixture_repo("aws_sdk_transactions_changed", "aws-sdk-controls", None);
        let (java, go, cs) = if changed {
            (
                " if (prod) p.tableName(\"new\");",
                " p.TableName = aws.String(\"new\")\n",
                " put.TableName = \"new\";",
            )
        } else {
            ("", "", "")
        };
        let java_held = if changed {
            " if (prod) b.put(Put.builder().tableName(\"other\").build());"
        } else {
            ""
        };
        let (java_check, go_check, cs_check) = if changed {
            (
                " if (prod) c.tableName(\"new\");",
                " c.TableName = aws.String(\"new\")\n",
                " check.TableName = \"new\";",
            )
        } else {
            ("", "", "")
        };
        std::fs::write(
            repo.join("Changed.java"),
            format!("import software.amazon.awssdk.services.dynamodb.DynamoDbClient;\n\
             import software.amazon.awssdk.services.dynamodb.model.ConditionCheck;\n\
             import software.amazon.awssdk.services.dynamodb.model.Put;\n\
             import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;\n\
             import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;\n\
             class Changed {{\n\
             void put(boolean prod) {{ var db = DynamoDbClient.create(); var p = Put.builder().tableName(\"put-java\");{java} db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p.build()).build()).build()); }}\n\
             void check(boolean prod) {{ var db = DynamoDbClient.create(); var c = ConditionCheck.builder().tableName(\"check-java\");{java_check} db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().conditionCheck(c.build()).build()).build()); }}\n\
             void held(boolean prod) {{ var db = DynamoDbClient.create(); var b = TransactWriteItem.builder().put(Put.builder().tableName(\"held-java\").build());{java_held} db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(b.build()).build()); }}\n}}\n"),
        )
        .unwrap();
        std::fs::write(
            repo.join("changed.go"),
            format!("package application\n\nimport (\n\t\"context\"\n\n\t\"github.com/aws/aws-sdk-go-v2/aws\"\n\t\"github.com/aws/aws-sdk-go-v2/service/dynamodb\"\n\t\"github.com/aws/aws-sdk-go-v2/service/dynamodb/types\"\n)\n\n\
             func put(ctx context.Context) {{\n\tclient := dynamodb.NewFromConfig(cfg)\n\tp := &types.Put{{TableName: aws.String(\"put-go\")}}\n\t{go}client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{{TransactItems: []types.TransactWriteItem{{{{Put: p}}}}}})\n}}\n\n\
             func check(ctx context.Context) {{\n\tclient := dynamodb.NewFromConfig(cfg)\n\tc := &types.ConditionCheck{{TableName: aws.String(\"check-go\")}}\n\t{go_check}client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{{TransactItems: []types.TransactWriteItem{{{{ConditionCheck: c}}}}}})\n}}\n"),
        )
        .unwrap();
        std::fs::write(
            repo.join("Changed.cs"),
            format!("using System.Collections.Generic;\nusing Amazon.DynamoDBv2;\nusing Amazon.DynamoDBv2.Model;\n\
             class Changed {{\n\
             async System.Threading.Tasks.Task Put() {{ var db = new AmazonDynamoDBClient(); var put = new Put {{ TableName = \"put-cs\" }};{cs} await db.TransactWriteItemsAsync(new TransactWriteItemsRequest {{ TransactItems = new List<TransactWriteItem> {{ new TransactWriteItem {{ Put = put }} }} }}); }}\n\
             async System.Threading.Tasks.Task Check() {{ var db = new AmazonDynamoDBClient(); var check = new ConditionCheck {{ TableName = \"check-cs\" }};{cs_check} await db.TransactWriteItemsAsync(new TransactWriteItemsRequest {{ TransactItems = new List<TransactWriteItem> {{ new TransactWriteItem {{ ConditionCheck = check }} }} }}); }}\n}}\n"),
        )
        .unwrap();
        let export: Value =
            serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
        let mut sides: Vec<_> = [("writer", "providers"), ("reader", "consumers")]
            .iter()
            .flat_map(|(role, sides)| {
                export[sides]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(move |s| format!("{role} {}", s["contract"].as_str().unwrap()))
            })
            .collect();
        sides.sort();
        sides
    };
    assert_eq!(
        sides(false),
        [
            "reader resource::dynamodb::check-cs",
            "reader resource::dynamodb::check-go",
            "reader resource::dynamodb::check-java",
            "writer resource::dynamodb::held-java",
            "writer resource::dynamodb::put-cs",
            "writer resource::dynamodb::put-go",
            "writer resource::dynamodb::put-java",
        ]
    );
    assert_eq!(
        sides(true),
        [
            "reader resource::dynamodb",
            "reader resource::dynamodb",
            "reader resource::dynamodb",
            "writer resource::dynamodb",
            "writer resource::dynamodb",
            "writer resource::dynamodb",
            "writer resource::dynamodb",
        ]
    );
}

/// A Java builder set again before its `.build()` names no table when the built value is held in
/// a named local first (F1 of #719; `PutItemRequest` named the old table before): the check
/// judges the builder by what happened to it up to the `.build()`. A value built before the
/// builder is set again is an immutable copy and keeps its table.
#[test]
fn java_builders_set_again_before_a_named_build_name_no_table() {
    let repo = fixture_repo("aws_sdk_named_builds", "aws-sdk-controls", None);
    std::fs::write(
        repo.join("Named.java"),
        r#"import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.ConditionCheck;
import software.amazon.awssdk.services.dynamodb.model.Get;
import software.amazon.awssdk.services.dynamodb.model.Put;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactGetItem;
import software.amazon.awssdk.services.dynamodb.model.TransactGetItemsRequest;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItem;
import software.amazon.awssdk.services.dynamodb.model.TransactWriteItemsRequest;

class Named {
    void put() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("put-old"); pb.tableName("put-new");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }
    void check() {
        var db = DynamoDbClient.create();
        var cb = ConditionCheck.builder().tableName("check-old"); cb.tableName("check-new");
        ConditionCheck cc = cb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().conditionCheck(cc).build()).build());
    }
    void get() {
        var db = DynamoDbClient.create();
        var gb = Get.builder().tableName("get-old"); gb.tableName("get-new");
        Get g = gb.build();
        db.transactGetItems(TransactGetItemsRequest.builder().transactItems(TransactGetItem.builder().get(g).build()).build());
    }
    void item() {
        var db = DynamoDbClient.create();
        var ib = TransactWriteItem.builder().put(Put.builder().tableName("item-old").build()); ib.put(Put.builder().tableName("item-new").build());
        var item = ib.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(item).build());
    }
    void request() {
        var db = DynamoDbClient.create();
        var rb = TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(Put.builder().tableName("request-old").build()).build());
        rb.transactItems(TransactWriteItem.builder().put(Put.builder().tableName("request-new").build()).build());
        var req = rb.build();
        db.transactWriteItems(req);
    }
    void conditional(boolean prod) {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("conditional-old"); if (prod) pb.tableName("conditional-new");
        Put p = pb.build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }
    void plain() {
        var db = DynamoDbClient.create();
        var pb = PutItemRequest.builder().tableName("plain-old"); pb.tableName("plain-new");
        PutItemRequest req = pb.build();
        db.putItem(req);
    }
    void copy() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("copy");
        Put p = pb.build();
        pb.tableName("copy-new");
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(p).build()).build());
    }
    void copyPlain() {
        var db = DynamoDbClient.create();
        var pb = PutItemRequest.builder().tableName("copy-plain");
        PutItemRequest req = pb.build();
        pb.tableName("copy-plain-new");
        db.putItem(req);
    }
    void nested() {
        var db = DynamoDbClient.create();
        var ib = TransactWriteItem.builder().put(Put.builder().tableName("nested-old").build()); ib.put(Put.builder().tableName("nested-new").build());
        var item = ib.build();
        var rb = TransactWriteItemsRequest.builder().transactItems(item);
        db.transactWriteItems(rb.build());
    }
    void alias() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("alias-old"); pb.tableName("alias-new");
        Put p = pb.build();
        Put q = p;
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(TransactWriteItem.builder().put(q).build()).build());
    }
    void clean() {
        var db = DynamoDbClient.create();
        var pb = Put.builder().tableName("clean");
        Put p = pb.build();
        var item = TransactWriteItem.builder().put(p).build();
        db.transactWriteItems(TransactWriteItemsRequest.builder().transactItems(item).build());
    }
}
"#,
    )
    .unwrap();
    let export: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let mut sides: Vec<_> = [("writer", "providers"), ("reader", "consumers")]
        .iter()
        .flat_map(|(role, sides)| {
            export[sides].as_array().unwrap().iter().map(move |s| {
                // `Named.conditional(boolean)::db.transactWriteItems#0` or the reader's method.
                let scope = s["site"].as_str().unwrap().split("::").nth(2).unwrap();
                let method = scope.split('(').next().unwrap().rsplit('.').next().unwrap();
                format!("{method} {role} {}", s["contract"].as_str().unwrap())
            })
        })
        .collect();
    sides.sort();
    assert_eq!(
        sides,
        [
            "alias writer resource::dynamodb",
            "check reader resource::dynamodb",
            "clean writer resource::dynamodb::clean",
            "conditional writer resource::dynamodb",
            "copy writer resource::dynamodb::copy",
            "copyPlain writer resource::dynamodb::copy-plain",
            "get reader resource::dynamodb",
            "item writer resource::dynamodb",
            "nested writer resource::dynamodb",
            "plain writer resource::dynamodb",
            "put writer resource::dynamodb",
            "request writer resource::dynamodb",
        ],
        "{export}"
    );
}

/// A crafted chain of named values stays bounded: past the shared evaluation depth it is not
/// followed, and the table stays unresolved rather than naming a literal it did not check.
#[test]
fn java_named_builds_chained_past_the_evaluation_depth_stay_unresolved() {
    let repo = fixture_repo("aws_sdk_named_chain", "aws-sdk-controls", None);
    let mut body = String::from(
        "import software.amazon.awssdk.services.dynamodb.DynamoDbClient;\n\
         import software.amazon.awssdk.services.dynamodb.model.Put;\n\
         import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;\n\
         class Chain {\n void send(DynamoDbClient db) {\n\
         var pb = PutItemRequest.builder().tableName(\"chained\");\n\
         PutItemRequest v0 = pb.build();\n",
    );
    for n in 1..=20_000 {
        body += &format!(" PutItemRequest v{n} = v{};\n", n - 1);
    }
    body += " db.putItem(v20000);\n }\n}\n";
    std::fs::write(repo.join("Chain.java"), body).unwrap();
    let export: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let contracts: Vec<_> = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["contract"].as_str().unwrap())
        .collect();
    assert!(
        contracts
            .iter()
            .all(|c| *c != "resource::dynamodb::chained"),
        "{export}"
    );
}

/// A name is exact only when every mention of its builder, by any name, is a form the reader
/// follows (R3 of #719): a method that runs caller code, a consumer copy, an alias at any depth
/// (reassigned or not), a field builder reached through an alias. Each other mention is a change
/// the reader cannot follow, and the side is unresolved. The builders left alone keep their table.
#[test]
fn java_builders_with_a_mention_the_reader_cannot_follow_name_no_table() {
    let repo = fixture_repo("aws_sdk_builder_forms", "aws-sdk-builders", None);
    let export: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let mut sides: Vec<_> = [("writer", "providers"), ("reader", "consumers")]
        .iter()
        .flat_map(|(role, sides)| {
            export[sides].as_array().unwrap().iter().map(move |s| {
                let scope = s["site"].as_str().unwrap().split("::").nth(2).unwrap();
                let method = scope.split('(').next().unwrap().rsplit('.').next().unwrap();
                format!("{method} {role} {}", s["contract"].as_str().unwrap())
            })
        })
        .collect();
    sides.sort();
    assert_eq!(
        sides,
        [
            "afterLoop writer resource::dynamodb::after-loop",
            "apply writer resource::dynamodb",
            "arrayAlias writer resource::dynamodb",
            "castAlias writer resource::dynamodb",
            "checkApply reader resource::dynamodb",
            "copyInline writer resource::dynamodb",
            "copyNamed writer resource::dynamodb",
            "deep writer resource::dynamodb",
            "fieldAlias writer resource::dynamodb",
            "fromAlias writer resource::dynamodb",
            "lambdaLater writer resource::dynamodb",
            "loopFresh writer resource::dynamodb::loop-fresh",
            "loopLater writer resource::dynamodb",
            "otherMember writer resource::dynamodb::other-member",
            "plainDeep writer resource::dynamodb",
            "reassigned writer resource::dynamodb",
            "requestReassigned writer resource::dynamodb",
            "restarted writer resource::dynamodb::restarted",
            "supplierAlias writer resource::dynamodb",
            "ternaryAlias writer resource::dynamodb",
        ],
        "{export}"
    );
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

/// The contracts of the providers of the repository at `repo`, its export timed against `bound`.
fn timed_providers(repo: &std::path::Path, label: &str, bound: u64) -> Vec<String> {
    let started = std::time::Instant::now();
    let text = Engine::open(repo).unwrap().export().unwrap();
    let elapsed = started.elapsed();
    eprintln!("{label}: {elapsed:?}");
    assert!(
        elapsed < std::time::Duration::from_secs(bound),
        "{elapsed:?}"
    );
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(json["consumers"].as_array().unwrap().is_empty());
    (json["providers"].as_array().unwrap().iter())
        .map(|p| p["contract"].as_str().unwrap().to_string())
        .collect()
}

/// Thousands of operations on a client a factory of thousands of returns gives, and thousands on
/// a client thousands of call sites pass a parameter: the file keeps the answers no limit cut, so
/// each operation after the first reads the returns and the call sites in a step, where reading
/// them again took minutes and would spend the file's budget (`FACTORY_FILE_BUDGET`).
#[test]
fn python_sdk_factory_answers_are_read_once_per_file() {
    let repo = fixture_repo("sdk_python_factory_answers", "aws-sdk-controls", None);
    let n = 2000;
    let mut source = String::from("import boto3\n\n\ndef make(c):\n");
    for i in 0..n {
        source += &format!("    if c == {i}: return boto3.client('sns')\n");
    }
    source += "    return boto3.client('sns')\n\n\ndef run():\n";
    for i in 0..n {
        source += &format!("    make(1).publish(TopicArn='r{}')\n", i % 10);
    }
    source += "\n\ndef use(client):\n";
    for i in 0..n {
        source += &format!("    client.publish(TopicArn='p{}')\n", i % 10);
    }
    source += "\n\n";
    source += &"use(boto3.client('sns'))\n".repeat(n);
    std::fs::write(repo.join("app.py"), source).unwrap();
    let label = format!("Python factory of {n} returns and parameter of {n} callers");
    let providers = timed_providers(&repo, &label, 60);
    assert_eq!(providers.len(), 2 * n);
    let returned = providers
        .iter()
        .filter(|c| c.starts_with("channel::sns::r"));
    assert_eq!(returned.count(), n);
    let passed = providers
        .iter()
        .filter(|c| c.starts_with("channel::sns::p"));
    assert_eq!(passed.count(), n);
}

/// One factory question may spend all its file has left of `FACTORY_FILE_BUDGET`: five factories
/// of 2,000 returns, each return calling the factory below it, resolve on the file's only
/// operation, where a question held to `EVAL_BUDGET` stopped short and only a second operation,
/// reading what the first one kept, resolved.
#[test]
fn python_sdk_factory_question_may_spend_what_its_file_has_left() {
    let repo = fixture_repo("sdk_python_factory_lattice", "aws-sdk-controls", None);
    let n = 2000;
    let mut source = String::from("import boto3\n\n\ndef f0(c):\n");
    for i in 0..n {
        source += &format!("    if c == {i}: return boto3.client('sns')\n");
    }
    source += "    return boto3.client('sns')\n\n\n";
    for k in 1..5 {
        source += &format!("def f{k}(c, a=None):\n");
        for i in 0..n {
            source += &format!(
                "    if c == {i}: return f{}({i}, boto3.client('sns'))\n",
                k - 1
            );
        }
        source += &format!("    return f{}(0)\n\n\n", k - 1);
    }
    source += "f4(1).publish(TopicArn='lattice')\n";
    std::fs::write(repo.join("app.py"), source).unwrap();
    let label = format!("Python lattice of five factories of {n} returns");
    assert_eq!(
        timed_providers(&repo, &label, 60),
        ["channel::sns::lattice"]
    );
}

/// Factory questions share a budget per file (`FACTORY_FILE_BUDGET`), every visit of a value a
/// step: thousands of operations, each walking thousands of call sites whose answer a cycle keeps
/// from being kept, spend it, whether each call site is read anew or is the same default visited
/// again. A client 22 steps from its operation resolves before them and not after them, and one 4
/// steps away still does (`FACTORY_QUESTION_FLOOR`).
#[test]
fn python_sdk_factory_questions_share_a_budget_per_file() {
    let chain = |name: &str| {
        let mut s = format!("def {name}():\n    c0 = boto3.client('sns')\n");
        for i in 1..=20 {
            s += &format!("    c{i} = c{}\n", i - 1);
        }
        s + &format!("    c20.publish(TopicArn='{name}')\n\n\n")
    };
    let operations = "    x.publish(TopicArn='cycle')\n".repeat(2000) + "    return x\n\n\n";
    // Call sites each passing a client, the last of them in a cycle; then call sites leaving out
    // an argument whose default a cycle reaches, the last passing another client.
    let read_anew = format!(
        "def g(x):\n{operations}{}\n\ndef h():\n    return g(h())\n\n\n",
        "g(boto3.client('sns'))\n".repeat(5000)
    );
    let visited_again = format!(
        "def mk(y):\n    return boto3.client('sns')\n\n\ndef h():\n    return mk(h())\n\n\n\
         def g(x=h()):\n{operations}{}g(boto3.client('sqs'))\n\n\n",
        "g()\n".repeat(5000)
    );
    for (case, walked) in [("read_anew", read_anew), ("visited_again", visited_again)] {
        let repo = fixture_repo(
            &format!("sdk_python_factory_budget_{case}"),
            "aws-sdk-controls",
            None,
        );
        let after = "def after():\n    direct = boto3.client('sns')\n    direct.publish(TopicArn='after')\n\n\n";
        let source = format!(
            "import boto3\n\n\n{}{walked}{after}{}",
            chain("first"),
            chain("far")
        );
        std::fs::write(repo.join("app.py"), source).unwrap();
        let label = format!("Python factory operations whose call sites are {case}");
        let mut providers = timed_providers(&repo, &label, 60);
        providers.sort();
        assert_eq!(
            providers,
            ["channel::sns::after", "channel::sns::first"],
            "{case}"
        );
    }
}

/// A factory answer kept for the file's later questions is read again only where its walk still
/// fits `EVAL_DEPTH`. A chain of 300 names stays unresolved whether it is asked before or after
/// the chains of 100 and 200 names that reach the same client; so does a chain of 150 names to a
/// factory returning a chain of 120, after the factory and the chain's first name were asked.
#[test]
fn python_sdk_factory_answers_kept_for_later_questions_keep_the_depth_ceiling() {
    let chain = |name: &str, n: usize| {
        (1..=n)
            .map(|i| format!("{name}{i} = {name}{}\n", i - 1))
            .collect::<String>()
    };
    let names = format!("c0 = sns\n{}", chain("c", 300));
    let names = (
        names,
        "c100.publish(TopicArn='near')\nc200.publish(TopicArn='middle')\n",
        "c300.publish(TopicArn='far')\n",
    );
    let returns = format!(
        "c0 = sns\n{}def make():\n    return c120\n\n\nm0 = make()\n{}",
        chain("c", 120),
        chain("m", 150)
    );
    let returns = (
        returns,
        "make().publish(TopicArn='near')\nm0.publish(TopicArn='middle')\n",
        "m150.publish(TopicArn='far')\n",
    );
    for (layout, (chains, near, far)) in [("names", names), ("returns", returns)] {
        for (order, operations) in [("far_last", [near, far]), ("far_first", [far, near])] {
            let case = format!("{layout}_{order}");
            let repo = fixture_repo(
                &format!("sdk_python_factory_depth_{case}"),
                "aws-sdk-controls",
                None,
            );
            let source = format!(
                "import boto3\nsns = boto3.client('sns')\n{chains}{}",
                operations.concat()
            );
            std::fs::write(repo.join("app.py"), source).unwrap();
            let label = format!("Python factory chains, {case}");
            let mut providers = timed_providers(&repo, &label, 60);
            providers.sort();
            assert_eq!(
                providers,
                ["channel::sns::middle", "channel::sns::near"],
                "{case}"
            );
        }
    }
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
