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

#[test]
fn serverless_handlers_merge_provider_environment_and_keep_each_function_scope() {
    let repo = fixture_repo(
        "aws_workloads_serverless",
        "aws-workload-serverless",
        Some("https://example.com/acme/serverless.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let exported = engine.export().unwrap();
    let export: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        export["executions"].as_array().map(Vec::len),
        Some(2),
        "{exported}"
    );
    let providers = export["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 4, "{exported}");
    for (path, contract) in [
        ("worker/send.ts::dispatch", "channel::sqs::orders"),
        ("worker/send.ts::typedDispatch", "channel::sqs::orders"),
        ("other/send.ts::dispatch", "channel::sqs::refunds"),
        ("outside.ts::dispatch", "channel::sqs"),
    ] {
        assert!(
            providers.iter().any(|p| p["site"]
                .as_str()
                .unwrap()
                .ends_with(&format!("{path}::SendMessageCommand#0"))
                && p["contract"] == contract),
            "{exported}"
        );
    }
    let context = engine.context("functions.worker", None).unwrap();
    assert!(
        context.contains("INHERITED")
            && context.contains("channel::sqs::orders")
            && context.contains("worker/send.ts [exact]"),
        "{context}"
    );
    assert!(!context.contains("wrong-global"), "{context}");
    let down = engine
        .trace(Start::From("functions.worker"), Direction::Down, None)
        .unwrap();
    assert!(
        down.contains("worker/send.ts")
            && !down.contains("other/send.ts")
            && !down.contains("outside.ts"),
        "{down}"
    );
    let up = engine
        .trace(Start::From("typedDispatch"), Direction::Up, None)
        .unwrap();
    assert!(up.contains("functions.worker"), "{up}");
    assert_golden("aws-workloads.serverless.export.json", &exported);
    assert_golden("aws-workloads.serverless.context.txt", &context);
    assert_golden(
        "aws-workloads.serverless.trace.txt",
        &format!("{down}\n{up}"),
    );
}

#[test]
fn ecs_encoded_containers_run_images_independently_and_scope_environment_to_each_image() {
    let app = fixture_repo(
        "aws_workloads_ecs",
        "aws-workload-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "aws_workloads_ecs",
        "aws-workload-ecs",
        Some("https://example.com/acme/tasks.git"),
    );
    std::fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/acme/tasks\"\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let exported = engine.export().unwrap();
    let export: Value = serde_json::from_str(&exported).unwrap();
    let names = export["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["contract"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names,
        [
            "image::ghcr.io/acme/aws-worker",
            "image::ghcr.io/acme/aws-sidecar",
            "image::docker.io/library/redis"
        ]
        .into_iter()
        .collect(),
        "{exported}"
    );
    let app_export: Value =
        serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    assert_eq!(
        app_export["providers"].as_array().unwrap().len(),
        2,
        "{app_export}"
    );
    assert!(
        app_export["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "channel::sqs::orders"),
        "{app_export}"
    );
    assert!(
        !app_export["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "channel::sqs::sidecar"),
        "{app_export}"
    );
    let context = engine
        .context("aws_ecs_task_definition.worker", None)
        .unwrap();
    assert!(
        context.contains("env worker QUEUE_NAME")
            && context.contains("env sidecar QUEUE_NAME")
            && context.contains("sidecar"),
        "{context}"
    );
    let down = engine
        .trace(
            Start::From("aws_ecs_task_definition.worker"),
            Direction::Down,
            None,
        )
        .unwrap();
    let up = Engine::open(&app)
        .unwrap()
        .trace(Start::From("dispatch"), Direction::Up, None)
        .unwrap();
    assert!(
        down.contains("send.ts") && up.contains("aws_ecs_task_definition.worker"),
        "{down}\n{up}"
    );
    let status = engine
        .status_report()
        .unwrap()
        .1
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        status.contains("orphan image::docker.io/library/redis: no builder"),
        "{status}"
    );
    assert_golden("aws-workloads.ecs.export.json", &exported);
    assert_golden("aws-workloads.ecs.context.txt", &context);
    assert_golden("aws-workloads.ecs.trace.txt", &format!("{down}\n{up}"));
}

#[test]
fn ecs_computed_json_and_partial_fields_keep_typed_targets_and_secret_overrides() {
    let app = fixture_repo(
        "aws_workloads_ecs_values",
        "aws-workload-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "aws_workloads_ecs_values",
        "aws-workload-ecs",
        Some("https://example.com/acme/tasks.git"),
    );
    std::fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/acme/tasks\"\n",
    )
    .unwrap();
    let sender = std::fs::read_to_string(app.join("send.ts")).unwrap();
    std::fs::write(app.join("send.ts"),format!("{sender}\nexport function unknown() {{ return client.send(new SendMessageCommand({{QueueUrl: process.env.UNKNOWN_QUEUE, MessageBody: 'x'}})); }}\nexport function secret() {{ return client.send(new SendMessageCommand({{QueueUrl: process.env.SECRET_QUEUE, MessageBody: 'x'}})); }}\n")).unwrap();
    std::fs::write(repo.join("main.tf"), r#"
variable "unknown" {}
resource "aws_sqs_queue" "orders" { name = "orders" }
locals {
  encoded = jsonencode([{ name = "computed", image = "ghcr.io/acme/aws-worker:1", environment = [{name = "QUEUE_NAME", value = "computed"}] }])
}
resource "aws_ecs_task_definition" "computed" {
  family = "computed"
  container_definitions = local.encoded
}
resource "aws_ecs_task_definition" "partial" {
  family = "partial"
  container_definitions = jsonencode([{ name = "worker", image = "ghcr.io/acme/aws-worker:1", environment = [
    {name = "QUEUE_NAME", value = aws_sqs_queue.orders.name},
    {name = "UNKNOWN_QUEUE", value = var.unknown},
    {name = "SECRET_QUEUE", value = "must-be-overridden"}
  ], secrets = [{name = "SECRET_QUEUE", valueFrom = "arn:aws:ssm:us-east-1:111111111111:parameter/private"}] }])
}
"#).unwrap();
    let engine = Engine::open(&app).unwrap();
    let exported = engine.export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    let providers = data["providers"].as_array().unwrap();
    let names = providers
        .iter()
        .filter(|p| p["site"].as_str().unwrap().contains("::dispatch::"))
        .map(|p| p["contract"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names,
        ["channel::sqs::computed", "channel::sqs::orders"]
            .into_iter()
            .collect(),
        "{exported}"
    );
    for name in ["unknown", "secret"] {
        let names = providers
            .iter()
            .filter(|p| p["site"].as_str().unwrap().contains(&format!("::{name}::")))
            .map(|p| p["contract"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, ["channel::sqs"], "{exported}");
    }
    let context = Engine::open(&repo)
        .unwrap()
        .context("aws_ecs_task_definition.partial", None)
        .unwrap();
    assert!(
        context.contains("channel::sqs::orders")
            && context.contains("ECS secret reference (redacted): unresolved"),
        "{context}"
    );
}

#[test]
fn new_aws_paths_refuse_parent_absolute_remote_and_symlinks_and_refresh_from_dependencies() {
    let repo = fixture_repo(
        "aws_workloads_path_controls",
        "aws-workload-lambda",
        Some("https://example.com/acme/lambda.git"),
    );
    std::fs::write(repo.join("worker.zip"), "opaque archive").unwrap();
    std::fs::create_dir(repo.join("other")).unwrap();
    std::fs::copy(repo.join("worker/send.ts"), repo.join("other/send.ts")).unwrap();
    let original = std::fs::read_to_string(repo.join("main.tf")).unwrap();
    let source = format!(
        "variable \"code_dir\" {{ default = \"worker\" }}\n{}",
        original.replace("source_dir = \"worker\"", "source_dir = var.code_dir")
    );
    std::fs::write(repo.join("main.tf"), &source).unwrap();
    let engine = Engine::open(&repo).unwrap();
    let paths = |s: String| -> std::collections::BTreeMap<String, String> {
        let d: Value = serde_json::from_str(&s).unwrap();
        d["executions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| {
                (
                    v["workload"]
                        .as_str()
                        .unwrap()
                        .rsplit("::")
                        .next()
                        .unwrap()
                        .to_string(),
                    v["path"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    assert_eq!(
        paths(engine.export().unwrap())
            .get("aws_lambda_function.local")
            .map(String::as_str),
        Some("worker")
    );
    std::fs::write(repo.join("terraform.tfvars"), "code_dir = \"other\"\n").unwrap();
    assert_eq!(
        paths(engine.export().unwrap())
            .get("aws_lambda_function.local")
            .map(String::as_str),
        Some("other")
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("main.tf")).unwrap(),
        source
    );
    std::fs::remove_file(repo.join("terraform.tfvars")).unwrap();
    std::fs::create_dir(repo.join("module")).unwrap();
    std::fs::write(
        repo.join("module/outputs.tf"),
        "output \"path\" { value = \"worker\" }\n",
    )
    .unwrap();
    let module_source = format!(
        "{}\nmodule \"code\" {{ source = \"./module\" }}\n",
        source.replace("source_dir = var.code_dir", "source_dir = module.code.path")
    );
    std::fs::write(repo.join("main.tf"), &module_source).unwrap();
    assert_eq!(
        paths(engine.export().unwrap())
            .get("aws_lambda_function.local")
            .map(String::as_str),
        Some("worker")
    );
    std::fs::write(
        repo.join("module/outputs.tf"),
        "output \"path\" { value = \"other\" }\n",
    )
    .unwrap();
    assert_eq!(
        paths(engine.export().unwrap())
            .get("aws_lambda_function.local")
            .map(String::as_str),
        Some("other")
    );
    for refused in [
        "".to_string(),
        "worker/../worker".to_string(),
        "../worker".into(),
        "https://example.com/code.zip".into(),
        "~/worker".into(),
        repo.join("worker").to_string_lossy().to_string(),
    ] {
        std::fs::write(
            repo.join("module/outputs.tf"),
            format!(
                "output \"path\" {{ value = {} }}\n",
                serde_json::to_string(&refused).unwrap()
            ),
        )
        .unwrap();
        assert!(
            !paths(engine.export().unwrap()).contains_key("aws_lambda_function.local"),
            "{refused}"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("main.tf")).unwrap(),
            module_source
        );
    }
    std::fs::write(
        repo.join("module/outputs.tf"),
        "output \"path\" { value = \"worker\" }\n",
    )
    .unwrap();
    assert_eq!(
        paths(engine.export().unwrap())
            .get("aws_lambda_function.local")
            .map(String::as_str),
        Some("worker")
    );
    #[cfg(unix)]
    {
        std::fs::rename(repo.join("worker"), repo.join("worker-saved")).unwrap();
        std::os::unix::fs::symlink("worker-saved", repo.join("worker")).unwrap();
        assert!(!paths(engine.export().unwrap()).contains_key("aws_lambda_function.local"));
        std::fs::remove_file(repo.join("worker")).unwrap();
        std::fs::rename(repo.join("worker-saved"), repo.join("worker")).unwrap();
        assert_eq!(
            paths(engine.export().unwrap())
                .get("aws_lambda_function.local")
                .map(String::as_str),
            Some("worker")
        );
    }
    std::fs::remove_file(repo.join("worker.zip")).unwrap();
    assert!(!paths(engine.export().unwrap()).contains_key("aws_lambda_function.zip"));
    std::fs::write(repo.join("worker.zip"), "opaque archive restored").unwrap();
    assert!(paths(engine.export().unwrap()).contains_key("aws_lambda_function.zip"));
    let trace = engine
        .trace(
            Start::From("aws_lambda_function.zip"),
            Direction::Down,
            None,
        )
        .unwrap();
    assert!(!trace.contains("worker/send.ts"), "{trace}");
}

#[test]
fn serverless_ambiguous_or_missing_handler_never_supplies_an_environment() {
    let repo = fixture_repo(
        "aws_workloads_handler_controls",
        "aws-workload-serverless",
        Some("https://example.com/acme/serverless.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    std::fs::copy(repo.join("worker/send.ts"), repo.join("worker/send.js")).unwrap();
    let exported = engine.export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        data["executions"].as_array().map(Vec::len),
        Some(1),
        "{exported}"
    );
    assert!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["site"].as_str().unwrap().contains("::worker/"))
            .all(|p| p["contract"] == "channel::sqs"),
        "{exported}"
    );
    std::fs::remove_file(repo.join("worker/send.js")).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&engine.export().unwrap()).unwrap()["executions"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    std::fs::rename(repo.join("worker/send.ts"), repo.join("worker/saved.ts")).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&engine.export().unwrap()).unwrap()["executions"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn curated_cloudformation_aliases_provision_all_aws_names_without_defining_key_uses() {
    let repo = fixture_repo(
        "aws_workloads_aliases",
        "aws-workload-aliases",
        Some("https://example.com/acme/aliases.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let exported = engine.export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    let names = data["provisioners"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names,
        [
            "channel::sqs::orders",
            "resource::s3::assets",
            "channel::sns::events",
            "channel::eventbridge::bus",
            "channel::kinesis::stream",
            "resource::dynamodb::records",
            "resource::dynamodb::global-records",
            "resource::dynamodb::simple-records",
            "resource::rds::database",
            "resource::rds::cluster",
            "resource::elasticache::cache",
            "resource::elasticache::replicas",
            "resource::ssm::/config/endpoint",
            "resource::secretsmanager::app-token"
        ]
        .into_iter()
        .collect(),
        "{exported}"
    );
    assert!(
        data.get("uses")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty),
        "{exported}"
    );
}

#[test]
fn ambiguous_image_builders_never_supply_a_workload_environment() {
    let (app, deploy) = workspace("aws_workloads_builder_ambiguity");
    fixture_repo(
        "aws_workloads_builder_ambiguity",
        "aws-workload-app-other",
        Some("https://example.com/acme/other.git"),
    );
    std::fs::write(
        deploy.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/acme/app\"\n[[peer]]\nrepo = \"example.com/acme/other\"\n",
    )
    .unwrap();
    let exported = Engine::open(&deploy).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        data["links"].as_array().map(Vec::len),
        Some(2),
        "{exported}"
    );
    assert!(
        data["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["level"] == "ambiguous"),
        "{exported}"
    );
    let app_export = Engine::open(&app).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&app_export).unwrap();
    assert!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["contract"].as_str().unwrap().starts_with("channel::"))
            .all(|p| p["contract"] == "channel::sqs"),
        "{app_export}"
    );
}

#[test]
fn unknown_serverless_environment_maps_override_inherited_values_conservatively() {
    let repo = fixture_repo(
        "aws_workloads_unknown_override",
        "aws-workload-serverless",
        Some("https://example.com/acme/serverless.git"),
    );
    std::fs::write(repo.join("serverless.yml"),"service: worker\nprovider:\n  environment:\n    QUEUE_NAME: must-not-survive\nfunctions:\n  worker:\n    handler: worker/send.dispatch\n    environment: ${ssm:/unknown-map}\n").unwrap();
    let exported = Engine::open(&repo).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["contract"] == "channel::sqs"),
        "{exported}"
    );
}

#[test]
fn ambiguous_archive_inputs_never_choose_a_local_source() {
    let repo = fixture_repo(
        "aws_workloads_archive_ambiguity",
        "aws-workload-lambda",
        Some("https://example.com/acme/lambda.git"),
    );
    let source = std::fs::read_to_string(repo.join("main.tf"))
        .unwrap()
        .replace(
            "source_dir = \"worker\"",
            "source_dir = \"worker\"\n  source_file = \"worker/send.ts\"",
        );
    std::fs::write(repo.join("main.tf"), source).unwrap();
    let exported = Engine::open(&repo).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert!(
        !data
            .get("executions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|x| x["workload"]
                .as_str()
                .unwrap()
                .ends_with("aws_lambda_function.local")),
        "{exported}"
    );
}

#[test]
fn environment_fanout_discards_partial_names_but_keeps_lambda_and_ecs_images() {
    let app = fixture_repo(
        "aws_workloads_image_bounds",
        "aws-workload-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "aws_workloads_image_bounds",
        "aws-workload-ecs",
        Some("https://example.com/acme/tasks.git"),
    );
    std::fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/acme/tasks\"\n",
    )
    .unwrap();
    let image = format!("ghcr.io/acme/{}", "a".repeat(30_000));
    std::fs::write(app.join(".github/workflows/build.yml"),format!("jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: {image}:1\n")).unwrap();
    let lambda_env = std::iter::once("QUEUE_NAME = \"orders\"".to_string())
        .chain((0..40).map(|i| format!("EXTRA_{i} = \"orders\"")))
        .collect::<Vec<_>>()
        .join("\n");
    let ecs_env = std::iter::once("{name = \"QUEUE_NAME\", value = \"orders\"}".to_string())
        .chain((0..40).map(|i| format!("{{name = \"EXTRA_{i}\", value = \"orders\"}}")))
        .collect::<Vec<_>>()
        .join(",");
    std::fs::write(repo.join("main.tf"),format!(r#"
variable "image" {{ default = "{image}" }}
resource "aws_lambda_function" "worker" {{
  function_name = "worker"
  image_uri = var.image
  environment {{ variables = {{
{lambda_env}
  }} }}
}}
resource "aws_ecs_task_definition" "worker" {{
  family = "worker"
  container_definitions = jsonencode([{{ name = "worker", image = var.image, environment = [{ecs_env}] }}])
}}
"#)).unwrap();
    let exported = Engine::open(&repo).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(data["consumers"].as_array().map(Vec::len), Some(2));
    assert_eq!(data["links"].as_array().map(Vec::len), Some(2));
    let data: Value = serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    let sides = data["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["contract"].as_str().unwrap().starts_with("channel::"))
        .collect::<Vec<_>>();
    assert_eq!(sides.len(), 1);
    assert_eq!(sides[0]["contract"], "channel::sqs");
}

#[test]
fn parameter_and_self_expansion_bounds_keep_sam_and_serverless_code_without_partial_environment() {
    for (case, fixture, source) in [
        ("sam", "aws-workload-sam", "template.yaml"),
        ("serverless", "aws-workload-serverless", "serverless.yml"),
    ] {
        let repo = fixture_repo(
            &format!("aws_workloads_env_bounds_{case}"),
            fixture,
            Some("https://example.com/acme/bounds.git"),
        );
        let big = "v".repeat(1000);
        let env = (0..1200)
            .map(|i| {
                if case == "sam" {
                    format!("          ZZ_EXTRA_{i}: !Ref Big\n")
                } else {
                    format!("      ZZ_EXTRA_{i}: ${{self:custom.big}}\n")
                }
            })
            .collect::<String>();
        let text = if case == "sam" {
            format!(
                "Transform: AWS::Serverless-2016-10-31\nParameters:\n  Big:\n    Type: String\n    Default: {big}\nResources:\n  Worker:\n    Type: AWS::Serverless::Function\n    Properties:\n      CodeUri: worker\n      Handler: send.dispatch\n      Environment:\n        Variables:\n          QUEUE_NAME: orders\n{env}"
            )
        } else {
            format!(
                "service: worker\ncustom:\n  big: {big}\nfunctions:\n  worker:\n    handler: worker/send.dispatch\n    environment:\n      QUEUE_NAME: orders\n{env}"
            )
        };
        std::fs::write(repo.join(source), text).unwrap();
        let exported = Engine::open(&repo).unwrap().export().unwrap();
        let data: Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(
            data["executions"].as_array().map(Vec::len),
            Some(1),
            "{case}"
        );
        assert!(
            data["providers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["contract"] == "channel::sqs"),
            "{case}: {exported}"
        );
    }
}

#[test]
fn terraform_json_ecs_uses_the_same_encoder_and_container_inventory() {
    let app = fixture_repo(
        "aws_workloads_ecs_json",
        "aws-workload-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "aws_workloads_ecs_json",
        "aws-workload-ecs",
        Some("https://example.com/acme/tasks.git"),
    );
    std::fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/acme/tasks\"\n",
    )
    .unwrap();
    std::fs::remove_file(repo.join("main.tf")).unwrap();
    let source = json!({"resource":{"aws_ecs_task_definition":{"json":{"family":"json","container_definitions":"${jsonencode([{name = \"worker\", image = \"ghcr.io/acme/aws-worker:1\", environment = [{name = \"QUEUE_NAME\", value = \"json-queue\"}]}])}"}}}});
    std::fs::write(
        repo.join("main.tf.json"),
        serde_json::to_string(&source).unwrap(),
    )
    .unwrap();
    let exported = Engine::open(&repo).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        data["consumers"].as_array().map(Vec::len),
        Some(1),
        "{exported}"
    );
    let exported = Engine::open(&app).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "channel::sqs::json-queue"),
        "{exported}"
    );

    let source = json!({"variable":{"unknown":{}},"resource":{"aws_ecs_task_definition":{"json":{"family":"json","container_definitions":r#"${jsonencode([{name = "worker", image = "ghcr.io/acme/aws-worker:1", environment = var.unknown}])}"#}}}});
    std::fs::write(
        repo.join("main.tf.json"),
        serde_json::to_string(&source).unwrap(),
    )
    .unwrap();
    let infra: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    assert_eq!(infra["consumers"].as_array().map(Vec::len), Some(1));
    let app: Value = serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    assert!(
        app["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["contract"].as_str().unwrap().starts_with("channel::"))
            .all(|p| p["contract"] == "channel::sqs")
    );
}

#[test]
fn typed_environment_uses_physical_attributes_and_sqs_url_without_secret_contents() {
    let (app, deploy) = workspace("aws_workloads_identity_attributes");
    let sender = std::fs::read_to_string(app.join("send.ts")).unwrap();
    std::fs::write(app.join("send.ts"),format!("{sender}\nimport {{ S3Client, GetObjectCommand }} from '@aws-sdk/client-s3';\nconst s3 = new S3Client({{}});\nexport function bucket() {{ return s3.send(new GetObjectCommand({{Bucket: process.env.BUCKET, Key: 'x'}})); }}\nexport function secret() {{ return client.send(new SendMessageCommand({{QueueUrl: process.env.SECRET_QUEUE, MessageBody: 'x'}})); }}\n")).unwrap();
    std::fs::write(
        deploy.join("main.tf"),
        r#"
resource "aws_sqs_queue" "orders" { name = "orders" }
resource "aws_s3_bucket" "assets" { bucket = "assets" }
resource "aws_ssm_parameter" "hidden" {
  name = "/hidden"
  type = "SecureString"
  value = "aws-secret-must-stay-hidden"
}
resource "aws_lambda_function" "worker" {
  function_name = "worker"
  image_uri = "ghcr.io/acme/aws-worker:1"
  environment { variables = {
    QUEUE_NAME = aws_sqs_queue.orders.url
    BUCKET = aws_s3_bucket.assets.bucket
    SECRET_QUEUE = aws_ssm_parameter.hidden.value
  } }
}
"#,
    )
    .unwrap();
    let exported = Engine::open(&app).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "channel::sqs::orders"),
        "{exported}"
    );
    assert!(
        data["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "resource::s3::assets"),
        "{exported}"
    );
    assert!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["site"].as_str().unwrap().contains("::secret::"))
            .all(|p| p["contract"] == "channel::sqs"),
        "{exported}"
    );
    let context = Engine::open(&deploy)
        .unwrap()
        .context("aws_lambda_function.worker", None)
        .unwrap();
    assert!(
        !context.contains("resource::ssm") && !context.contains("aws-secret-must-stay-hidden"),
        "{context}"
    );
}

#[test]
fn unknown_sam_environment_maps_never_inherit_global_values() {
    let repo = fixture_repo(
        "aws_workloads_unknown_sam_override",
        "aws-workload-sam",
        Some("https://example.com/acme/sam.git"),
    );
    std::fs::write(repo.join("template.yaml"),"Transform: AWS::Serverless-2016-10-31\nParameters:\n  Stage:\n    Type: String\nConditions:\n  IsProd: !Equals [!Ref Stage, prod]\nGlobals:\n  Function:\n    Environment:\n      Variables:\n        QUEUE_NAME: must-not-survive\nResources:\n  Worker:\n    Type: AWS::Serverless::Function\n    Properties:\n      CodeUri: worker\n      Handler: send.dispatch\n      Environment:\n        Variables: !If [IsProd, {QUEUE_NAME: prod}, {QUEUE_NAME: dev}]\n").unwrap();
    let exported = Engine::open(&repo).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["contract"] == "channel::sqs"),
        "{exported}"
    );
}

#[test]
fn unread_sam_image_never_invents_a_registry_or_supplies_builder_environment() {
    let app = fixture_repo(
        "aws_workloads_unknown_sam_image",
        "aws-workload-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "aws_workloads_unknown_sam_image",
        "aws-workload-sam",
        Some("https://example.com/acme/sam.git"),
    );
    std::fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/acme/sam\"\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/acme/app\"\n",
    )
    .unwrap();
    std::fs::write(app.join(".github/workflows/build.yml"),"jobs:\n  build:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: busybox:1\n").unwrap();
    std::fs::write(repo.join("template.yaml"),"Transform: AWS::Serverless-2016-10-31\nParameters:\n  Image:\n    Type: String\n    NoEcho: true\n    Default: busybox:1\nResources:\n  Worker:\n    Type: AWS::Serverless::Function\n    Properties:\n      ImageUri: !Ref Image\n      Environment:\n        Variables:\n          QUEUE_NAME: orders\n").unwrap();
    let exported = Engine::open(&repo).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert!(
        data["consumers"].as_array().unwrap().is_empty(),
        "{exported}"
    );
    let exported = Engine::open(&app).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["contract"].as_str().unwrap().starts_with("channel::"))
            .all(|p| p["contract"] == "channel::sqs"),
        "{exported}"
    );
}
