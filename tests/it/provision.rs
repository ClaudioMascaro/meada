//! Terraform provisioners across sibling infrastructure and application repositories (#656).

use std::{fs, path::PathBuf};

use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

use crate::common::{assert_golden, fixture_repo};

fn workspace(case: &str) -> (PathBuf, PathBuf) {
    let infra = fixture_repo(
        case,
        "provision-infra",
        Some("https://example.com/provision/infra.git"),
    );
    let app = fixture_repo(
        case,
        "provision-app",
        Some("https://example.com/provision/app.git"),
    );
    (infra, app)
}

#[test]
fn terraform_provisions_every_side_in_every_root_without_ambiguity() {
    let (infra, app) = workspace("provision_export");
    for (path, name) in [(&infra, "infra"), (&app, "app")] {
        let text = Engine::open(path).unwrap().export().unwrap();
        let mut export = meada_core::export::parse(&text).unwrap();
        export.commit = None;
        export.dirty = None;
        export.processes.clear();
        let json = serde_json::to_value(&export).unwrap();
        assert_eq!(json["format"], 1);
        let provisions = json["provisions"].as_array().unwrap();
        assert_eq!(
            provisions.len(),
            if name == "infra" { 18 } else { 16 },
            "{text}"
        );
        let config: Vec<_> = provisions
            .iter()
            .filter(|l| l.get("at").is_some())
            .collect();
        assert_eq!(config.len(), if name == "infra" { 2 } else { 0 }, "{text}");
        for link in &config {
            assert_eq!(link["side"], "example.com/provision/infra");
            assert_eq!(link["at"], "envs/prod/terraform.tfvars#queue");
            assert_eq!(link["contract"], "channel::sqs::orders");
            assert_eq!(link["level"], "inferred");
        }
        assert!(provisions.iter().all(|l| l["level"] != "ambiguous"));
        for l in provisions {
            if l.get("at").is_some() {
                continue;
            }
            assert!(
                l["contract"]
                    .as_str()
                    .unwrap()
                    .starts_with("channel::sqs::")
                    || l["contract"] == "resource::s3::archive",
                "{l}"
            );
            if l["contract"] == "channel::sqs::orders" {
                match l["root"].as_str().unwrap() {
                    "envs/dev" => assert_eq!(l["level"], "exact"),
                    "envs/prod" => {
                        assert_eq!(l["level"], "inferred");
                        assert!(l["reason"].as_str().unwrap().contains("var.queue (tfvars)"));
                    }
                    other => panic!("unexpected root {other}"),
                }
            }
            if l["contract"] == "resource::s3::archive" && l["root"] == "envs/dev" {
                assert_eq!(l["level"], "declared");
            }
        }
        assert_golden(
            &format!("provision-{name}.export.json"),
            &export.write().unwrap(),
        );
    }
}

#[test]
fn provision_context_and_trace_cross_from_terraform_to_code_and_back() {
    let (infra, _) = workspace("provision_queries");
    let engine = Engine::open(&infra).unwrap();
    let queries = [
        "aws_sqs_queue.orders (root envs/dev)",
        "aws_s3_bucket.archive (root envs/prod)",
        "enqueueOrder",
        "channel::sqs::orders",
        "resource::s3::archive",
        "module.shared.aws_sqs_queue.this",
    ];
    let contexts = queries
        .iter()
        .map(|q| engine.context(q, Some(12000)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(contexts.contains("producer src/producer.ts:"), "{contexts}");
    assert!(contexts.contains("consumer src/consumer.ts:"), "{contexts}");
    assert!(contexts.contains("reader (repository-level)"), "{contexts}");
    assert!(contexts.contains("provisioners:"), "{contexts}");
    assert_golden("provision.context.txt", &contexts);
    let mut traces = Vec::new();
    for (q, direction) in [
        ("aws_sqs_queue.orders (root envs/dev)", Direction::Down),
        ("aws_s3_bucket.archive (root envs/prod)", Direction::Down),
        ("enqueueOrder", Direction::Up),
        ("drain", Direction::Up),
        ("example.com/provision/app", Direction::Up),
        (
            "module.shared.aws_sqs_queue.this (root envs/dev)",
            Direction::Down,
        ),
    ] {
        let text = engine
            .trace(Start::From(q), direction, Some(12000))
            .unwrap();
        assert!(text.contains("provisions "), "{q}: {text}");
        traces.push(text);
    }
    assert_golden("provision.trace.txt", &traces.join("\n"));
}

#[test]
fn provision_partial_queries_equal_every_row() {
    let (infra, _) = workspace("provision_partial");
    let engine = Engine::open(&infra).unwrap();
    let asks = || {
        let mut text = String::new();
        for q in [
            "aws_sqs_queue.orders (root envs/dev)",
            "enqueueOrder",
            "drain",
            "aws_s3_bucket.archive (root envs/prod)",
            "module.shared.aws_sqs_queue.this (root envs/dev)",
            "module.shared.aws_sqs_queue.this (root envs/prod)",
            "resource::s3::archive",
            "example.com/provision/app",
        ] {
            text += &engine.context(q, Some(12000)).unwrap();
            for direction in [Direction::Down, Direction::Up] {
                text += &engine
                    .trace(Start::From(q), direction, Some(12000))
                    .unwrap();
            }
        }
        text
    };
    let partial = asks();
    let whole = meada::engine::with_every_row(asks);
    assert_eq!(partial, whole);
}

#[test]
fn edited_bucket_and_tfvars_refresh_provisioning_without_stale_links() {
    let (infra, _) = workspace("provision_refresh");
    let engine = Engine::open(&infra).unwrap();
    let before = engine.export().unwrap();
    assert!(before.contains("resource::s3::archive"));
    let file = infra.join("envs/dev/main.tf");
    fs::write(
        &file,
        fs::read_to_string(&file)
            .unwrap()
            .replace("bucket = \"archive\"", "bucket = \"changed\""),
    )
    .unwrap();
    fs::write(
        infra.join("envs/prod/terraform.tfvars"),
        "queue = \"changed\"\n",
    )
    .unwrap();
    let warm: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        warm["provisions"]
            .as_array()
            .unwrap()
            .iter()
            .all(
                |l| !(l["contract"] == "channel::sqs::orders" && l["root"] == "envs/prod")
                    && !(l["contract"] == "resource::s3::archive" && l["root"] == "envs/dev")
            )
    );
    drop(engine);
    fs::remove_dir_all(infra.join(".meada")).unwrap();
    let cold: Value =
        serde_json::from_str(&Engine::open(&infra).unwrap().export().unwrap()).unwrap();
    assert_eq!(warm, cold);
}

#[test]
fn patterns_link_only_literal_names_at_segment_boundaries() {
    let (infra, app) = workspace("provision_patterns");
    let mut declarations = fs::read_to_string(app.join("meada.toml")).unwrap();
    for name in [
        "archive-prod",
        "archive-stage",
        "abc-prod",
        "ab-prod",
        "a-prod",
        "archive",
    ] {
        declarations +=
            &format!("\n[[resource]]\nkind = \"s3\"\nid = \"{name}\"\nrole = \"read\"\n");
    }
    fs::write(app.join("meada.toml"), declarations).unwrap();
    let dir = infra.join("patterns");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("main.tf"),
        r#"
resource "aws_s3_bucket" "several" { bucket = "archive-${terraform.workspace}" }
resource "aws_s3_bucket" "minimum" { bucket = "abc-${terraform.workspace}" }
resource "aws_s3_bucket" "separator" { bucket = "ab-${terraform.workspace}" }
resource "aws_s3_bucket" "short" { bucket = "a-${terraform.workspace}" }
resource "aws_s3_bucket" "inside" { bucket = "arch${terraform.workspace}ive" }
"#,
    )
    .unwrap();
    let engine = Engine::open(&infra).unwrap();
    let data: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let links = data["provisions"].as_array().unwrap();
    let pattern_links: Vec<_> = links.iter().filter(|l| l["root"] == "patterns").collect();
    assert_eq!(pattern_links.len(), 3, "{data}");
    for l in pattern_links {
        match l["contract"].as_str().unwrap() {
            "resource::s3::archive-prod" | "resource::s3::archive-stage" => {
                assert_eq!(l["level"], "ambiguous");
                assert!(l["reason"].as_str().unwrap().contains("archive-*"));
            }
            // Three letters or digits are the minimum; a separator is no evidence.
            "resource::s3::abc-prod" => assert_eq!(l["level"], "inferred"),
            other => panic!("unexpected pattern target {other}"),
        }
    }
    let context = engine.context("aws_s3_bucket.several", Some(8000)).unwrap();
    assert!(context.contains("resource::s3::archive-prod"), "{context}");
    assert!(context.contains("resource::s3::archive-stage"), "{context}");
    assert!(context.contains("reader (repository-level)"), "{context}");
    let contract = engine
        .context("resource::s3::archive-prod", Some(8000))
        .unwrap();
    assert!(
        contract.contains("aws_s3_bucket.several (root patterns)"),
        "{contract}"
    );
    let trace = engine
        .trace(
            Start::From("aws_s3_bucket.minimum"),
            Direction::Down,
            Some(8000),
        )
        .unwrap();
    assert!(
        trace.contains("provisions resource::s3::abc-prod [inferred:"),
        "{trace}"
    );
}

#[test]
fn status_lists_orphans_only_where_both_sides_of_the_group_exist() {
    let app = fixture_repo(
        "orphan_status",
        "orphan-app",
        Some("https://example.com/orphan/app.git"),
    );
    let infra = fixture_repo(
        "orphan_status",
        "orphan-infra",
        Some("https://example.com/orphan/infra.git"),
    );
    Engine::open(&app).unwrap().export().unwrap();
    let status = meada::engine::status(&app).unwrap();
    assert_eq!(
        status,
        "example.com/orphan/app (.) indexed, 1 files
  resource::s3::archive is unlinked: no writer and reader in different repositories
  resource::s3::unmanaged is unlinked: no writer and reader in different repositories
  orphan channel::sqs::unmanaged: no provisioner; app.ts::sendUnmanaged::SendMessageCommand#0 [exact]
  orphan resource::s3::unmanaged: no provisioner; (repository-level) [declared]
example.com/orphan/infra (../orphan-infra) indexed, 0 files
  orphan channel::sqs::unused: no use; main.tf:2 aws_sqs_queue.unused (root .)
  orphan resource::s3::unused: no use; main.tf:4 aws_s3_bucket.unused (root .)
  config files: 1 read, 0 skipped (lockfiles: 0, unreadable: 0); values: 4, redacted: 0
"
    );
    assert_golden("orphan.status.txt", &status);
    // The same scope works when the infrastructure repository is the entry point.
    let status = meada::engine::status(&infra).unwrap();
    assert_golden("orphan-infra.status.txt", &status);
}

#[test]
fn code_only_scope_marks_uses_in_search_and_context_without_orphan_status() {
    let app = fixture_repo(
        "orphan_code_only",
        "orphan-app",
        Some("https://example.com/orphan/app.git"),
    );
    let config = fs::read_to_string(app.join("meada.toml")).unwrap();
    fs::write(
        app.join("meada.toml"),
        config
            .split_once("[[resource]]")
            .map(|(_, rest)| format!("[[resource]]{rest}"))
            .unwrap(),
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    let search = engine.search("unmanaged", Some("channel"), None).unwrap();
    assert_eq!(
        search,
        "example.com/orphan/app app.ts:10 channel::sqs::unmanaged (no provisioner in the workspace)\n"
    );
    let context = engine.context("channel::sqs::unmanaged", None).unwrap();
    assert_eq!(
        context,
        "channel::sqs::unmanaged\n  example.com/orphan/app producer app.ts:10 sendUnmanaged (no provisioner in the workspace)\n"
    );
    let symbol = engine.context("sendUnmanaged", None).unwrap();
    assert!(
        symbol.contains("produces channel::sqs::unmanaged (no provisioner in the workspace)"),
        "{symbol}"
    );
    assert_eq!(
        meada::engine::status(&app).unwrap(),
        "example.com/orphan/app (.) indexed, 1 files\n  resource::s3::archive is unlinked: no writer and reader in different repositories\n  resource::s3::unmanaged is unlinked: no writer and reader in different repositories\n"
    );
    assert_golden("orphan-code.context.txt", &(context + "\n" + &symbol));
}

#[test]
fn iac_only_scope_marks_provisioners_without_orphan_status() {
    let infra = fixture_repo(
        "orphan_iac_only",
        "orphan-infra",
        Some("https://example.com/orphan/infra.git"),
    );
    fs::write(infra.join("meada.toml"), "").unwrap();
    let engine = Engine::open(&infra).unwrap();
    let search = engine
        .search("aws_sqs_queue.orders", Some("iac"), None)
        .unwrap();
    assert_eq!(
        search,
        "example.com/orphan/infra main.tf:1 aws_sqs_queue.orders (aws_sqs_queue, name \"orders\", root .) (no use in the workspace)\n"
    );
    let context = engine.context("channel::sqs::orders", None).unwrap();
    assert_eq!(
        context,
        "channel::sqs::orders\n  example.com/orphan/infra provisioner main.tf:1 aws_sqs_queue.orders (root .) (no use in the workspace)\n"
    );
    let node = engine.context("aws_sqs_queue.orders", None).unwrap();
    assert!(
        node.contains("aws_sqs_queue.orders (root .) (no use in the workspace)"),
        "{node}"
    );
    assert_eq!(
        meada::engine::status(&infra).unwrap(),
        "example.com/orphan/infra (.) indexed, 0 files\n  config files: 1 read, 0 skipped (lockfiles: 0, unreadable: 0); values: 4, redacted: 0\n"
    );
    assert_golden("orphan-iac.context.txt", &(context + "\n" + &node));
}

#[test]
fn orphan_groups_do_not_borrow_another_broker_or_resource_kind() {
    let app = fixture_repo(
        "orphan_groups",
        "orphan-app",
        Some("https://example.com/orphan/app.git"),
    );
    let infra = fixture_repo(
        "orphan_groups",
        "orphan-infra",
        Some("https://example.com/orphan/infra.git"),
    );
    fs::write(app.join("meada.toml"), "[[peer]]\nrepo = \"example.com/orphan/infra\"\n[[resource]]\nkind = \"postgres\"\nid = \"archive\"\nrole = \"read\"\n").unwrap();
    fs::write(
        app.join("app.ts"),
        r#"import { Kafka } from "kafkajs";
import { SQSClient, SendMessageCommand } from "@aws-sdk/client-sqs";
const kafka = new Kafka({ brokers: ["kafka:9092"] });
const producer = kafka.producer();
const client = new SQSClient({});
export async function send() {
  await producer.send({ topic: "orders", messages: [{ value: "x" }] });
  await client.send(new SendMessageCommand({ QueueUrl: process.env.QUEUE_URL, MessageBody: "x" }));
}
"#,
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    let status = meada::engine::status(&app).unwrap();
    assert_eq!(
        status,
        "example.com/orphan/app (.) indexed, 1 files\n  resource::postgres::archive is unlinked: no writer and reader in different repositories\nexample.com/orphan/infra (../orphan-infra) indexed, 0 files\n  config files: 1 read, 0 skipped (lockfiles: 0, unreadable: 0); values: 4, redacted: 0\n"
    );
    assert_eq!(
        engine.search("orders", Some("channel"), None).unwrap(),
        "example.com/orphan/app app.ts:7 channel::kafka::orders (no provisioner in the workspace)\nexample.com/orphan/infra main.tf:1 aws_sqs_queue.orders (root .) provisioner channel::sqs::orders (no use in the workspace)\n"
    );
    assert_eq!(
        engine
            .search("channel::sqs", Some("channel"), None)
            .unwrap(),
        "example.com/orphan/app app.ts:8 channel::sqs (unresolved)\nexample.com/orphan/infra main.tf:1 aws_sqs_queue.orders (root .) provisioner channel::sqs::orders (no use in the workspace)\nexample.com/orphan/infra main.tf:2 aws_sqs_queue.unused (root .) provisioner channel::sqs::unused (no use in the workspace)\n"
    );
    assert!(
        engine
            .context("channel::sqs::orders", None)
            .unwrap()
            .contains("(no use in the workspace)")
    );
    assert!(
        engine
            .context("resource::s3::archive", None)
            .unwrap()
            .contains("(no use in the workspace)")
    );
    assert!(
        engine
            .context("resource::postgres::archive", None)
            .unwrap()
            .contains("(no provisioner in the workspace)")
    );
    assert_golden("orphan-groups.status.txt", &status);
    // A use added inside the provisioner's repository enables just its group on refresh.
    fs::write(infra.join("meada.toml"), "[[peer]]\nrepo = \"example.com/orphan/app\"\n[[resource]]\nkind = \"s3\"\nid = \"archive\"\nrole = \"read\"\n").unwrap();
    let status = meada::engine::status(&app).unwrap();
    assert_golden("orphan-local-use.status.txt", &status);
    let engine = Engine::open(&app).unwrap();
    assert_eq!(
        engine.context("resource::s3::archive", None).unwrap(),
        "resource::s3::archive\n  example.com/orphan/infra reader (repository-level)\n  example.com/orphan/infra provisioner main.tf:3 aws_s3_bucket.archive (root .)\n"
    );
}

#[test]
fn orphan_matching_retains_patterns_and_independent_roots_on_refresh() {
    let app = fixture_repo(
        "orphan_patterns",
        "orphan-app",
        Some("https://example.com/orphan/app.git"),
    );
    let infra = fixture_repo(
        "orphan_patterns",
        "orphan-infra",
        Some("https://example.com/orphan/infra.git"),
    );
    fs::create_dir(infra.join("prod")).unwrap();
    fs::write(infra.join("prod/main.tf"), "resource \"aws_s3_bucket\" \"pattern\" { bucket = \"unmanaged-${terraform.workspace}\" }\nresource \"aws_s3_bucket\" \"archive\" { bucket = \"archive\" }\n").unwrap();
    let config = fs::read_to_string(app.join("meada.toml")).unwrap();
    fs::write(
        app.join("meada.toml"),
        config.replace("id = \"unmanaged\"", "id = \"unmanaged-prod\""),
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    engine.export().unwrap();
    let before = meada::engine::status(&app).unwrap();
    assert_golden("orphan-patterns.status.txt", &before);
    fs::write(infra.join("prod/main.tf"), "resource \"aws_s3_bucket\" \"pattern\" { bucket = \"different-${terraform.workspace}\" }\nresource \"aws_s3_bucket\" \"archive\" { bucket = \"archive\" }\n").unwrap();
    let after = meada::engine::status(&app).unwrap();
    assert_golden("orphan-patterns-edited.status.txt", &after);
    assert!(
        after.contains("orphan resource::s3::unmanaged-prod: no provisioner"),
        "{after}"
    );
    assert!(
        after.contains("orphan resource::s3::different-*: no use"),
        "{after}"
    );
    assert!(
        !before.contains("orphan resource::s3::unmanaged-prod"),
        "{before}"
    );
}

#[test]
fn search_finds_provisioner_sides_as_contracts_without_uses() {
    let infra = fixture_repo(
        "orphan_search_provisioner",
        "orphan-infra",
        Some("https://example.com/orphan/infra.git"),
    );
    fs::write(infra.join("meada.toml"), "").unwrap();
    let engine = Engine::open(&infra).unwrap();
    let mut searches = String::new();
    for kind in [None, Some("contract"), Some("channel")] {
        searches += &engine.search("orders", kind, None).unwrap();
        assert_eq!(
            engine.search("orders", kind, None).unwrap(),
            "example.com/orphan/infra main.tf:1 aws_sqs_queue.orders (root .) provisioner channel::sqs::orders (no use in the workspace)\n",
            "{kind:?}"
        );
    }
    assert_golden("orphan-iac.search.txt", &searches);
    assert_eq!(
        engine.search("archive", Some("resource"), None).unwrap(),
        "example.com/orphan/infra main.tf:3 aws_s3_bucket.archive (root .) provisioner resource::s3::archive (no use in the workspace)\n"
    );
    assert_eq!(
        engine.search("orders", Some("resource"), None).unwrap(),
        "no matches for `orders`\n"
    );
}

/// A use side `inferred` from a key name opens no orphan scope and is never an orphan line, while
/// one Meada is certain of is, naming its level; `context` still shows the inferred side and the
/// export's `uses` rows carry the level (ADR 0026, "Orphan sides", #797).
#[test]
fn a_use_inferred_from_a_key_name_opens_no_orphan_scope() {
    let repo = fixture_repo(
        "orphan_level",
        "orphan-level",
        Some("https://example.com/acme/o2.git"),
    );
    let orphans = || {
        let (_, reports) = Engine::open(&repo).unwrap().status_report().unwrap();
        (reports.into_iter().flatten())
            .filter(|l| l.starts_with("orphan "))
            .collect::<Vec<_>>()
    };
    assert_eq!(orphans(), Vec::<String>::new());
    let engine = Engine::open(&repo).unwrap();
    let context = engine.context("channel::sqs::jobs", None).unwrap();
    assert!(
        context.contains(
            "example.com/acme/o2 use config/labels.yaml#routing.queue (line 2) [inferred]"
        ),
        "{context}"
    );
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let levels: Vec<_> = (export["uses"].as_array().unwrap().iter())
        .map(|u| (u["contract"].as_str(), u["level"].as_str()))
        .collect();
    assert_eq!(
        levels,
        [
            (Some("channel::sqs::jobs"), Some("inferred")),
            (Some("resource::dynamodb::sales.orders"), Some("inferred")),
        ]
    );
    // A certain use of a queue nothing provisions opens the scope of SQS queues, and of them
    // alone: the inferred `jobs` and the DynamoDB tables stay out.
    fs::write(
        repo.join("config/arn.yaml"),
        "target: arn:aws:sqs:us-east-1:123456789012:ghost\n",
    )
    .unwrap();
    assert_eq!(
        orphans(),
        [
            "orphan channel::sqs::acme-jobs: no use; infra/main.tf:1 aws_sqs_queue.jobs (root infra)",
            "orphan channel::sqs::ghost: no provisioner; config/arn.yaml#target [exact]",
        ]
    );
    // A provisioned queue whose only use is an inferred one of its own name is not unused.
    fs::write(
        repo.join("config/labels.yaml"),
        "routing:\n  queue: acme-jobs\n",
    )
    .unwrap();
    assert_eq!(
        orphans(),
        ["orphan channel::sqs::ghost: no provisioner; config/arn.yaml#target [exact]"]
    );
}

/// A structured identifier in the resource list of an IAM policy statement is a policy grant, no
/// use side: it links to nothing, opens no orphan scope and has an export array of its own; a
/// value under a keyword of a JSON Schema document gives no side and stays a value (ADR 0026,
/// "Orphan sides", #798).
#[test]
fn a_policy_grant_and_a_schema_example_are_no_use_side() {
    let repo = fixture_repo(
        "orphan_grant",
        "orphan-grant",
        Some("https://example.com/acme/o3.git"),
    );
    let orphans = || {
        let (_, reports) = Engine::open(&repo).unwrap().status_report().unwrap();
        (reports.into_iter().flatten())
            .filter(|l| l.starts_with("orphan "))
            .collect::<Vec<_>>()
    };
    assert_eq!(orphans(), Vec::<String>::new());
    let engine = Engine::open(&repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let rows = |array: &str| -> Vec<(String, String)> {
        (export[array].as_array().into_iter().flatten())
            .map(|r| (r["contract"].to_string(), r["at"].to_string()))
            .collect()
    };
    let row = |contract: &str, at: &str| (format!("\"{contract}\""), format!("\"{at}\""));
    let writer = "infra/writer.tf#data.aws_iam_policy_document.writer.statement";
    let reader = "infra/main.tf#data.aws_iam_policy_document.reader.statement.resources";
    let template = "infra/template.yaml#Resources.ReadPolicy.Properties.PolicyDocument.Statement";
    assert_eq!(
        rows("policy_grants"),
        [
            row(
                "channel::sns::acme-events",
                &format!("{writer}[1].resources[0]")
            ),
            row(
                "channel::sqs::acme-dead",
                &format!("{writer}[0].not_resources[0]")
            ),
            row("resource::dynamodb::acme-sessions", &format!("{reader}[1]")),
            row(
                "resource::dynamodb::acme-sessions",
                &format!("{template}[0].Resource[0]")
            ),
            row(
                "resource::s3::acme-exports",
                "infra/policy.json#Statement[0].Resource[0]"
            ),
            row("resource::s3::acme-reports", &format!("{reader}[0]")),
            row(
                "resource::s3::acme-vault",
                "infra/deny.json#Statement.NotResource"
            ),
        ]
    );
    // A property named `default` holds a subschema, not the keyword's value, and the subschema
    // of a property named `properties` holds the keyword.
    assert_eq!(
        rows("uses"),
        [
            row(
                "channel::sqs::acme-dependent",
                "schemas/Subscription.v1.schema.json#dependencies.default.description"
            ),
            row(
                "channel::sqs::acme-named",
                "schemas/Topic.schema.json#properties.default.description"
            ),
        ]
    );
    assert!(export.get("provisions").is_none(), "{export}");
    assert_golden(
        "orphan-grant.context.txt",
        &engine
            .context("resource::dynamodb::acme-sessions", None)
            .unwrap(),
    );
    assert_golden(
        "orphan-grant.search.txt",
        &engine
            .search("acme-exports", Some("resource"), None)
            .unwrap(),
    );
    let context = engine.context("channel::sns::orders-events", None).unwrap();
    assert!(!context.contains("schema.json"), "{context}");
    let value = engine.search("orders-events", Some("value"), None).unwrap();
    assert!(value.contains("examples[0].spec.topicArn"), "{value}");
    // A use of a table nothing provisions opens the scope of DynamoDB tables, and the policy
    // grants of `acme-sessions` hide no `no use` line.
    fs::create_dir_all(repo.join("config")).unwrap();
    fs::write(
        repo.join("config/app.yaml"),
        "carts:\n  tableArn: arn:aws:dynamodb:us-east-1:123456789012:table/acme-carts\n",
    )
    .unwrap();
    assert_eq!(
        orphans(),
        [
            "orphan resource::dynamodb::acme-carts: no provisioner; config/app.yaml#carts.tableArn [exact]",
            "orphan resource::dynamodb::acme-sessions: no use; infra/main.tf:5 aws_dynamodb_table.sessions (root infra)",
        ]
    );
    // `examples` in a document with no `$schema` at its root is any key.
    fs::write(
        repo.join("config/examples.json"),
        r#"{ "examples": [ { "topicArn": "arn:aws:sns:us-east-1:123456789012:orders-events" } ] }"#,
    )
    .unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        (export["uses"].as_array().unwrap().iter())
            .any(|u| u["contract"] == "channel::sns::orders-events"
                && u["at"] == "config/examples.json#examples[0].topicArn"
                && u["level"] == "exact"),
        "{export}"
    );
}

/// `context` finds a policy grant on its resource and at its place, and a grant carries no
/// orphan note: nothing provisions a queue in the fixture, yet `acme-dead`'s grant is no orphan.
#[test]
fn a_policy_grant_is_found_at_its_place_and_carries_no_orphan_note() {
    let repo = fixture_repo(
        "orphan_grant_place",
        "orphan-grant",
        Some("https://example.com/acme/o3.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let grant =
        "example.com/acme/o3 policy grant infra/policy.json#Statement[0].Resource[0] (line 2)";
    let resource = engine.context("resource::s3::acme-exports", None).unwrap();
    assert!(
        resource.contains(grant) && !resource.contains(" use "),
        "{resource}"
    );
    assert_golden(
        "orphan-grant.context-place.txt",
        &(engine.context("infra/policy.json#Statement[0].Resource[0]", None)).unwrap(),
    );
    let named = engine.context("channel::sqs::acme-named", None).unwrap();
    assert!(
        named.contains("(no provisioner in the workspace)"),
        "{named}"
    );
    for answer in [
        engine.context("channel::sqs::acme-dead", None).unwrap(),
        engine.search("acme-dead", Some("channel"), None).unwrap(),
    ] {
        assert!(
            answer.contains("policy grant infra/writer.tf#") && !answer.contains("(no provisioner"),
            "{answer}"
        );
    }
}

/// A document whose root holds `$schema` is a schema: one document of a YAML stream, but not
/// an item of a JSON file's root array, which is one document.
#[test]
fn only_a_document_whose_root_holds_a_schema_key_is_a_schema() {
    let repo = fixture_repo(
        "orphan_grant_schema",
        "orphan-grant",
        Some("https://example.com/acme/o3.git"),
    );
    fs::create_dir_all(repo.join("config")).unwrap();
    fs::write(
        repo.join("config/stream.yaml"),
        "$schema: https://json-schema.org/draft/2020-12/schema\nexamples:\n  \
         - arn:aws:sns:us-east-1:123456789012:acme-schema\n---\nexamples:\n  \
         - arn:aws:sns:us-east-1:123456789012:acme-stream\n",
    )
    .unwrap();
    fs::write(
        repo.join("config/list.json"),
        r#"[ { "$schema": "https://json-schema.org/draft/2020-12/schema",
               "examples": [ "arn:aws:sns:us-east-1:123456789012:acme-list" ] } ]"#,
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let config: Vec<_> = (export["uses"].as_array().unwrap().iter())
        .filter_map(|u| u["at"].as_str().filter(|at| at.starts_with("config/")))
        .collect();
    assert_eq!(
        config,
        [
            "config/list.json#[0].examples[0]",
            "config/stream.yaml#[1].examples[0]"
        ]
    );
}
