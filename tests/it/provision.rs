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
        assert_eq!(provisions.len(), 16, "{text}");
        assert!(provisions.iter().all(|l| l["level"] != "ambiguous"));
        for l in provisions {
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
resource "aws_s3_bucket" "minimum" { bucket = "ab-${terraform.workspace}" }
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
            "resource::s3::ab-prod" => assert_eq!(l["level"], "inferred"),
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
        trace.contains("provisions resource::s3::ab-prod [inferred:"),
        "{trace}"
    );
}
