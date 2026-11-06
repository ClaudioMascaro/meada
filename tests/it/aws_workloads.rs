//! AWS function/task execution and scoped environment resolution (#664).

use crate::common::{assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
use serde_json::{Value, json};

fn workspace(case: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        fixture_repo(
            case,
            "aws-workload-app",
            Some("https://example.com/acme/app.git"),
        ),
        fixture_repo(
            case,
            "aws-workload-deploy",
            Some("https://example.com/acme/deploy.git"),
        ),
    )
}

#[test]
fn terraform_lambda_runs_its_image_and_resolves_only_its_code_environment() {
    let (app, deploy) = workspace("aws_workloads_lambda");
    let engine = Engine::open(&app).unwrap();
    let exported = engine.export().unwrap();
    let export: Value = serde_json::from_str(&exported).unwrap();
    let providers = export["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 2, "{exported}");
    assert!(
        providers
            .iter()
            .any(|p| p["contract"] == "channel::sqs::orders"),
        "{exported}"
    );
    let infra = Engine::open(&deploy).unwrap();
    let exported = infra.export().unwrap();
    let export: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        export["consumers"],
        json!([{
            "contract": "image::ghcr.io/acme/aws-worker",
            "site": "example.com/acme/deploy::main.tf::aws_lambda_function.worker",
            "via": null,
            "root": "."
        }]),
        "{exported}"
    );
    assert_eq!(export["links"][0]["level"], "exact", "{exported}");
    let context = infra.context("aws_lambda_function.worker", None).unwrap();
    assert!(
        context.contains("env worker QUEUE_NAME") && context.contains("orders"),
        "{context}"
    );
    let down = infra
        .trace(
            Start::From("aws_lambda_function.worker"),
            Direction::Down,
            None,
        )
        .unwrap();
    assert!(down.contains("send.ts"), "{down}");
    let up = engine
        .trace(Start::From("dispatch"), Direction::Up, None)
        .unwrap();
    assert!(up.contains("aws_lambda_function.worker"), "{up}");
    assert_golden("aws-workloads.lambda.export.json", &exported);
    assert_golden("aws-workloads.lambda.context.txt", &context);
    assert_golden("aws-workloads.lambda.trace.txt", &format!("{down}\n{up}"));
}

#[test]
fn sam_code_uri_and_merged_environment_join_the_local_handler_and_known_queue() {
    let repo = fixture_repo(
        "aws_workloads_sam",
        "aws-workload-sam",
        Some("https://example.com/acme/sam.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let exported = engine.export().unwrap();
    let export: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        export["executions"].as_array().map(Vec::len),
        Some(1),
        "{exported}"
    );
    assert_eq!(export["executions"][0]["path"], "worker");
    assert_eq!(export["executions"][0]["level"], "exact");
    assert_eq!(
        export["provisioners"].as_array().map(Vec::len),
        Some(1),
        "{exported}"
    );
    assert_eq!(
        export["provisioners"][0]["contract"],
        "channel::sqs::orders"
    );
    let providers = export["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 2, "{exported}");
    assert!(
        providers
            .iter()
            .any(|p| p["contract"] == "channel::sqs::orders"
                && p["site"].as_str().unwrap().contains("worker/send.ts")),
        "{exported}"
    );
    assert!(
        providers.iter().any(|p| p["contract"] == "channel::sqs"
            && p["site"].as_str().unwrap().contains("outside.ts")),
        "{exported}"
    );
    let context = engine.context("Worker", None).unwrap();
    assert!(
        context.contains("code: example.com/acme/sam worker [exact]")
            && context.contains("INHERITED")
            && context.contains("channel::sqs::orders"),
        "{context}"
    );
    assert!(
        !context.contains("must-stay-redacted") && !context.contains("wrong-global"),
        "{context}"
    );
    let down = engine
        .trace(Start::From("Worker"), Direction::Down, None)
        .unwrap();
    assert!(
        down.contains("worker/send.ts") && !down.contains("outside.ts"),
        "{down}"
    );
    let up = engine
        .trace(Start::From("dispatch"), Direction::Up, None)
        .unwrap();
    assert!(up.contains("Worker"), "{up}");
    let queue = engine
        .trace(Start::From("Orders"), Direction::Down, None)
        .unwrap();
    assert!(queue.contains("worker/send.ts"), "{queue}");
    assert_golden("aws-workloads.sam.export.json", &exported);
    assert_golden("aws-workloads.sam.context.txt", &context);
    assert_golden(
        "aws-workloads.sam.trace.txt",
        &format!("{down}\n{up}\n{queue}"),
    );
}

#[test]
fn terraform_lambda_code_uses_local_archives_and_explicit_archive_inputs_without_remote_guesses() {
    let repo = fixture_repo(
        "aws_workloads_lambda_code",
        "aws-workload-lambda",
        Some("https://example.com/acme/lambda.git"),
    );
    std::fs::write(
        repo.join("worker.zip"),
        "an opaque local deployment package",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let exported = engine.export().unwrap();
    let export: Value = serde_json::from_str(&exported).unwrap();
    let executions = export["executions"].as_array().unwrap();
    assert_eq!(executions.len(), 2, "{exported}");
    assert_eq!(
        executions
            .iter()
            .map(|x| x["path"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        ["worker", "worker.zip"].into_iter().collect()
    );
    assert_eq!(
        export["providers"][0]["contract"], "channel::sqs::jobs",
        "{exported}"
    );
    let context = engine.context("aws_lambda_function.local", None).unwrap();
    assert!(
        context.contains("channel::sqs::jobs")
            && context.contains("code: example.com/acme/lambda worker [exact]"),
        "{context}"
    );
    let trace = engine
        .trace(
            Start::From("aws_lambda_function.local"),
            Direction::Down,
            None,
        )
        .unwrap();
    assert!(trace.contains("worker/send.ts"), "{trace}");
    assert_golden("aws-workloads.lambda-code.export.json", &exported);
    assert_golden("aws-workloads.lambda-code.context.txt", &context);
}

#[test]
fn terraform_json_lambda_uses_the_shared_workload_inputs_and_environment() {
    let repo = fixture_repo(
        "aws_workloads_lambda_json",
        "aws-workload-lambda",
        Some("https://example.com/acme/lambda.git"),
    );
    std::fs::remove_file(repo.join("main.tf")).unwrap();
    std::fs::write(
        repo.join("main.tf.json"),
        r#"{
      "resource": {
        "aws_sqs_queue": {"jobs": {"name": "jobs"}},
        "aws_lambda_function": {"local": {
          "function_name": "worker", "filename": "worker", "handler": "send.dispatch",
          "environment": [{"variables": {"QUEUE_NAME": "${aws_sqs_queue.jobs.name}"}}]
        }}
      }
    }"#,
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let exported = engine.export().unwrap();
    let export: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        export["executions"].as_array().map(Vec::len),
        Some(1),
        "{exported}"
    );
    assert_eq!(export["executions"][0]["path"], "worker", "{exported}");
    assert_eq!(
        export["providers"][0]["contract"], "channel::sqs::jobs",
        "{exported}"
    );
    let context = engine.context("aws_lambda_function.local", None).unwrap();
    assert!(
        context.contains("channel::sqs::jobs")
            && context.contains("code: example.com/acme/lambda worker [exact]"),
        "{context}"
    );
}
