//! AWS SDK use sides, observed through the query engine (#659).
use crate::common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

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
