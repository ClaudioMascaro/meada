//! GCP provisioners, subscriptions and Cloud Run workloads (#667, ADR 0026).
use crate::common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

fn workspace(case: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        fixture_repo(case, "gcp-infra", Some("https://example.com/gcp/infra.git")),
        fixture_repo(case, "gcp-app", Some("https://example.com/gcp/app.git")),
    )
}

fn channels(app: &std::path::Path, source: &str, target: &str) {
    std::fs::write(
        app.join("asyncapi.json"),
        serde_json::json!({
            "asyncapi": "2.6.0",
            "servers": {"gcp": {"url": "localhost", "protocol": "pubsub"}},
            "channels": {
                source: {"subscribe": {}},
                target: {"publish": {}}
            }
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn pubsub_subscription_targets_preserve_filters_and_disappear_when_removed() {
    use meada::engine::{Direction, Start};
    let (infra, app) = workspace("gcp_subscription");
    channels(&app, "orders", "subscriptions/orders");
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["links"].as_array().unwrap().len(), 1, "{text}");
    let link = &json["links"][0];
    assert_eq!(link["level"], "inferred", "{link}");
    assert!(
        link["reason"]
            .as_str()
            .unwrap()
            .contains("google_pubsub_subscription.orders")
            && link["reason"].as_str().unwrap().contains("filter"),
        "{link}"
    );
    let context = engine
        .context("channel::pubsub::subscriptions/orders", Some(16000))
        .unwrap();
    let up = engine
        .trace(
            Start::From("settings.yaml#exact.subscription"),
            Direction::Up,
            Some(16000),
        )
        .unwrap();
    let down = engine
        .trace(
            Start::From("google_pubsub_topic.orders"),
            Direction::Down,
            Some(16000),
        )
        .unwrap();
    assert!(
        context.contains("pubsub send orders") && context.contains("filter"),
        "{context}"
    );
    assert!(
        up.contains("google_pubsub_subscription.orders")
            && down.contains("pubsub receive subscriptions/orders"),
        "{up}\n{down}"
    );
    assert_golden("gcp-subscription.context.txt", &context);
    assert_golden("gcp-subscription.trace.txt", &(up + &down));
    let path = infra.join("main.tf");
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        original.replace("  filter = \"attributes.kind = \\\"order\\\"\"\n", ""),
    )
    .unwrap();
    let plain: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(plain["links"].as_array().unwrap().len(), 1, "{plain}");
    assert_eq!(plain["links"][0]["level"], "exact", "{plain}");
    let start = original
        .find("resource \"google_pubsub_subscription\"")
        .unwrap();
    let end = start + original[start..].find("\n}\n").unwrap() + 3;
    std::fs::write(&path, format!("{}{}", &original[..start], &original[end..])).unwrap();
    let removed: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(removed["links"], serde_json::json!([]), "{removed}");
    assert_eq!(
        removed["providers"].as_array().unwrap().len(),
        1,
        "{removed}"
    );
    assert_eq!(
        removed["consumers"].as_array().unwrap().len(),
        3,
        "{removed}"
    );
    assert_eq!(
        removed["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["contract"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "channel::pubsub::subscriptions/orders",
            "resource::cloudsql::audit-db/orders",
            "resource::cloudsql::orders-db/orders",
        ]),
        "{removed}"
    );
    std::fs::write(&path, original).unwrap();
    assert_eq!(engine.export().unwrap(), text);
}

#[test]
fn gcp_names_provision_distinct_topics_subscriptions_instances_and_databases() {
    let (infra, app) = workspace("gcp_kinds");
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let actual = json["provisioners"]
        .as_array()
        .expect("GCP provisioners")
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            "channel::pubsub::orders",
            "channel::pubsub::subscriptions/orders",
            "resource::cloudsql::audit-db",
            "resource::cloudsql::audit-db/orders",
            "resource::cloudsql::orders-db",
            "resource::cloudsql::orders-db/orders",
            "resource::gcpsecretmanager::api-key",
            "resource::gcs::assets",
        ],
        "{text}"
    );
    let app_text = Engine::open(&app).unwrap().export().unwrap();
    let app_json: Value = serde_json::from_str(&app_text).unwrap();
    let uses = app_json["uses"].as_array().unwrap();
    assert_eq!(uses.len(), 10, "{app_text}");
    assert!(
        uses.iter()
            .all(|u| !u["at"].as_str().unwrap().contains("controls")),
        "{app_text}"
    );
    let provisions = app_json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 12, "{app_text}");
    for p in provisions {
        let expected = match p["at"].as_str() {
            Some(at) if at.starts_with("settings.yaml#typed.") => "inferred",
            Some(_) => "exact",
            None => "declared",
        };
        assert_eq!(p["level"], expected, "{p}");
    }
    let mut export = meada_core::export::parse(&text).unwrap();
    export.commit = None;
    export.dirty = None;
    assert_golden("gcp-infra.export.json", &export.write().unwrap());
}

#[test]
fn cloud_run_services_and_jobs_scope_images_environment_and_secret_targets() {
    use meada::engine::{Direction, Start};
    let (infra, app) = workspace("gcp_workloads");
    std::fs::write(
        infra.join("run.tf"),
        include_str!("../fixtures/gcp-run/main.tf"),
    )
    .unwrap();
    std::fs::write(
        app.join("worker.ts"),
        include_str!("../fixtures/gcp-run/worker.ts"),
    )
    .unwrap();
    std::fs::create_dir_all(app.join(".github/workflows")).unwrap();
    std::fs::write(
        app.join(".github/workflows/build.yml"),
        include_str!("../fixtures/gcp-run/build.yml"),
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let providers = json["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 10, "{text}");
    for p in providers {
        let site = p["site"].as_str().unwrap();
        let expected = if site.contains("::legacy::") {
            "channel::sqs::tasks-v1"
        } else if site.contains("::current::") {
            "channel::sqs::tasks-v2"
        } else if site.contains("::batch::") {
            "channel::sqs::tasks-job"
        } else if site == "example.com/gcp/app::.github/workflows/build.yml::jobs.image.steps[0]" {
            "image::ghcr.io/acme/gcp-worker"
        } else {
            "channel::sqs"
        };
        assert_eq!(p["contract"], expected, "{p}");
        if expected.starts_with("channel::sqs::") {
            assert!(
                p["via"]
                    .as_str()
                    .unwrap()
                    .contains("workload:example.com/gcp/infra::run.tf::google_cloud_run"),
                "{p}"
            );
        }
    }
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 3, "{text}");
    assert!(
        links
            .iter()
            .all(|l| l["contract"] == "image::ghcr.io/acme/gcp-worker"),
        "{text}"
    );
    assert_eq!(
        links.iter().filter(|l| l["level"] == "inferred").count(),
        1,
        "{text}"
    );
    let deploy = Engine::open(&infra).unwrap();
    let context = [
        "google_cloud_run_service.v1",
        "google_cloud_run_v2_service.v2",
        "google_cloud_run_v2_job.batch",
    ]
    .map(|n| deploy.context(n, Some(20000)).unwrap())
    .join("\n");
    assert!(
        context.contains("SECRET_QUEUE")
            && context.contains("secret")
            && context.contains("V2_SECRET"),
        "{context}"
    );
    let down = deploy
        .trace(
            Start::From("google_cloud_run_v2_job.batch"),
            Direction::Down,
            Some(20000),
        )
        .unwrap();
    let up = engine
        .trace(Start::From("batch"), Direction::Up, Some(20000))
        .unwrap();
    assert!(
        down.contains("worker.ts") && up.contains("google_cloud_run_v2_job.batch"),
        "{down}\n{up}"
    );
    assert_golden("gcp-run.context.txt", &context);
    assert_golden("gcp-run.trace.txt", &(down + &up));
    let mut export = meada_core::export::parse(&text).unwrap();
    export.commit = None;
    export.dirty = None;
    assert_golden("gcp-run.export.json", &export.write().unwrap());

    // One known builder per container is required even when both builders are visible.
    std::fs::create_dir_all(infra.join(".github/workflows")).unwrap();
    std::fs::write(
        infra.join(".github/workflows/second.yml"),
        include_str!("../fixtures/gcp-run/build.yml"),
    )
    .unwrap();
    let ambiguous: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        ambiguous["providers"].as_array().unwrap().iter().all(|p| {
            !p["contract"].as_str().unwrap().starts_with("channel::")
                || p["contract"] == "channel::sqs"
        }),
        "{ambiguous}"
    );
    std::fs::remove_dir_all(infra.join(".github")).unwrap();
    assert_eq!(engine.export().unwrap(), text);
    std::fs::write(
        infra.join("terraform.tfvars"),
        "image = \"ghcr.io/acme/unbuilt:v1\"\n",
    )
    .unwrap();
    let changed: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        changed["providers"].as_array().unwrap().iter().all(|p| {
            !p["site"].as_str().unwrap().contains("::current::") || p["contract"] == "channel::sqs"
        }),
        "{changed}"
    );
    std::fs::remove_file(infra.join("terraform.tfvars")).unwrap();
    assert_eq!(engine.export().unwrap(), text);
}

#[test]
fn gcp_composite_targets_keep_instance_provenance_and_refuse_unknown_names() {
    let (infra, app) = workspace("gcp_composite");
    let path = infra.join("main.tf");
    let text = std::fs::read_to_string(&path).unwrap();
    let text = format!(
        "variable \"instance\" {{ default = \"default-db\" }}\nvariable \"unknown\" {{}}\n{text}"
    )
    .replace("name = \"orders-db\"", "name = var.instance");
    std::fs::write(&path, format!("{text}\nresource \"google_sql_database\" \"unknown\" {{\n name = \"orders\"\n instance = var.unknown\n}}\nresource \"google_pubsub_subscription\" \"unknown\" {{\n name = var.unknown\n topic = google_pubsub_topic.orders.id\n}}\n")).unwrap();
    let manifest = app.join("meada.toml");
    std::fs::write(
        &manifest,
        std::fs::read_to_string(&manifest)
            .unwrap()
            .replace("orders-db/orders", "default-db/orders"),
    )
    .unwrap();
    let engine = Engine::open(&infra).unwrap();
    let initial = engine.export().unwrap();
    let data: Value = serde_json::from_str(&initial).unwrap();
    let provisioners = data["provisioners"].as_array().unwrap();
    assert_eq!(provisioners.len(), 8, "{initial}");
    assert!(
        provisioners
            .iter()
            .all(|p| !p["node"].as_str().unwrap().ends_with(".unknown")),
        "{initial}"
    );
    let database = data["provisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["contract"] == "resource::cloudsql::default-db/orders")
        .unwrap();
    assert_eq!(database["level"], "inferred", "{database}");
    assert!(
        database["reason"]
            .as_str()
            .unwrap()
            .contains("var.instance (default)"),
        "{database}"
    );
    let context = engine
        .context("google_sql_database.orders", Some(12000))
        .unwrap();
    assert!(
        context.contains("name: \"orders\"")
            && context.contains("resource::cloudsql::default-db/orders"),
        "{context}"
    );
    std::fs::write(
        infra.join("terraform.tfvars"),
        "instance = \"changed-db\"\n",
    )
    .unwrap();
    let changed: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        changed["provisioners"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "resource::cloudsql::changed-db/orders"),
        "{changed}"
    );
    assert!(
        changed["provisioners"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["contract"] != "resource::cloudsql::default-db/orders"),
        "{changed}"
    );
    std::fs::remove_file(infra.join("terraform.tfvars")).unwrap();
    assert_eq!(engine.export().unwrap(), initial);
}

#[test]
fn gcp_config_refresh_keeps_privacy_cut_values_and_complete_orphan_scope() {
    let (infra, app) = workspace("gcp_config_refresh");
    std::fs::write(app.join("secret.yaml"), "apiVersion: v1\nkind: Secret\nmetadata:\n  name: private\nstringData:\n  endpoint: projects/p/topics/orders\n  bucket: gs://assets\n").unwrap();
    std::fs::write(
        app.join("cut.json"),
        serde_json::json!({"gcs_bucket": format!("gs://assets/{}", "x".repeat(2048))}).to_string(),
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    let initial = engine.export().unwrap();
    let data: Value = serde_json::from_str(&initial).unwrap();
    assert_eq!(data["uses"].as_array().unwrap().len(), 10, "{initial}");
    assert!(
        data["uses"]
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u["at"].as_str().unwrap().starts_with("settings.yaml#")),
        "{initial}"
    );
    let settings = app.join("settings.yaml");
    let original = std::fs::read_to_string(&settings).unwrap();
    std::fs::write(
        &settings,
        original
            .replace("gs://assets/", "gs://absent/")
            .replace("gcs_bucket: assets", "gcs_bucket: absent"),
    )
    .unwrap();
    let (_, reports) = Engine::open(&infra).unwrap().status_report().unwrap();
    let reports = reports.into_iter().flatten().collect::<Vec<_>>().join("\n");
    let orphan_rows = reports
        .lines()
        .filter(|l| l.starts_with("orphan "))
        .collect::<Vec<_>>();
    // `gcs_bucket: absent` is `inferred` and no line; `gs://absent/` is `exact` (#797).
    assert_eq!(orphan_rows.len(), 3, "{reports}");
    let orphans = orphan_rows
        .iter()
        .map(|l| l.split(';').next().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        orphans,
        std::collections::BTreeSet::from([
            "orphan resource::cloudsql::audit-db: no use",
            "orphan resource::gcs::absent: no provisioner",
            "orphan resource::gcs::assets: no use",
        ]),
        "{reports}"
    );
    assert_golden("gcp-orphans.status.txt", &reports);
    std::fs::write(&settings, original).unwrap();
    assert_eq!(engine.export().unwrap(), initial);
}

#[test]
fn pubsub_sources_follow_modules_aliases_and_selected_instances_without_guessing() {
    let (infra, app) = workspace("gcp_endpoints");
    std::fs::create_dir_all(infra.join("modules/feed")).unwrap();
    std::fs::write(infra.join("modules/feed/main.tf"), "variable \"id\" {}\nresource \"google_pubsub_topic\" \"this\" { name = var.id }\noutput \"topic\" { value = google_pubsub_topic.this.id }\n").unwrap();
    std::fs::write(
        infra.join("endpoints.tf"),
        r#"
module "feed" {
  source = "./modules/feed"
  id = "module-topic"
}

variable "selected" { default = "chosen" }
resource "google_pubsub_topic" "indexed" {
  for_each = { chosen = "index-topic" }
  name = each.value
}
locals {
  source = module.feed.topic
  selected = google_pubsub_topic.indexed[var.selected].id
}
resource "google_pubsub_subscription" "module" {
  name = "module-reader"
  topic = local.source
}
resource "google_pubsub_subscription" "indexed" {
  name = "index-reader"
  topic = local.selected
}
resource "google_pubsub_subscription" "wrong_kind" {
  name = "wrong-reader"
  topic = google_storage_bucket.assets.name
}
resource "google_sql_database" "wrong_kind" {
  name = "orders"
  instance = google_storage_bucket.assets.name
}
"#,
    )
    .unwrap();
    std::fs::write(app.join("asyncapi.json"), serde_json::json!({
        "asyncapi": "2.6.0", "servers": {"gcp": {"url": "localhost", "protocol": "pubsub"}},
        "channels": {
            "module-topic": {"subscribe": {}}, "index-topic": {"subscribe": {}},
            "subscriptions/module-reader": {"publish": {}}, "subscriptions/index-reader": {"publish": {}}
        }
    }).to_string()).unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let links = data["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    for link in links {
        let source = link["provider"].as_str().unwrap();
        let expected = if source.ends_with("module-topic") {
            "exact"
        } else {
            "inferred"
        };
        assert_eq!(link["level"], expected, "{link}");
    }
    let infra_data: Value =
        serde_json::from_str(&Engine::open(&infra).unwrap().export().unwrap()).unwrap();
    assert!(
        infra_data["provisioners"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| !p["node"]
                .as_str()
                .unwrap()
                .ends_with("google_sql_database.wrong_kind")),
        "{infra_data}"
    );
    assert!(
        infra_data["subscriptions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| !s["node"].as_str().unwrap().ends_with(".wrong_kind")),
        "{infra_data}"
    );
}

#[test]
fn cloud_run_unknown_environment_fanout_stays_bounded() {
    let (infra, _) = workspace("gcp_hostile_environment");
    let env = "env {\n name = var.unknown\n value = \"must-not-resolve\"\n}\n".repeat(25);
    std::fs::write(
        infra.join("run.tf"),
        format!(
            r#"
variable "unknown" {{}}
resource "google_cloud_run_v2_job" "fanout" {{
  count = 1000
  name = "worker-${{count.index}}"
  template {{
    template {{
      containers {{
        image = "ghcr.io/acme/gcp-worker:v1"
        {env}
      }}
    }}
  }}
}}
"#
        ),
    )
    .unwrap();
    let started = std::time::Instant::now();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let elapsed = started.elapsed();
    let data: Value = serde_json::from_str(&text).unwrap();
    let consumers = data["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 1000, "{} runners", consumers.len());
    assert!(
        consumers
            .iter()
            .all(|c| c["contract"] == "image::ghcr.io/acme/gcp-worker")
    );
    assert_eq!(data["iac_nodes"].as_array().unwrap().len(), 1008);
    assert!(text.len() < 8 * 1024 * 1024);
    assert!(elapsed < std::time::Duration::from_secs(30), "{elapsed:?}");
    eprintln!(
        "Cloud Run unknown environment fanout: {elapsed:?}, {} exported bytes",
        text.len()
    );
}

#[test]
fn terraform_json_keeps_gcp_composite_targets_and_nested_workloads() {
    let (infra, _) = workspace("gcp_json");
    std::fs::remove_file(infra.join("main.tf")).unwrap();
    std::fs::write(
        infra.join("main.tf.json"),
        serde_json::json!({
            "resource": {
                "google_pubsub_topic": {"feed": {"name": "json-topic"}},
                "google_pubsub_subscription": {"feed": {
                    "name": "json-reader", "topic": "${google_pubsub_topic.feed.id}"
                }},
                "google_sql_database_instance": {"primary": {"name": "json-db"}},
                "google_sql_database": {"orders": {
                    "name": "orders", "instance": "${google_sql_database_instance.primary.name}"
                }},
                "google_cloud_run_v2_job": {"worker": {
                    "name": "json-worker",
                    "template": [{"template": [{"containers": [{
                        "name": "worker", "image": "ghcr.io/acme/json-worker:v1",
                        "env": [{"name": "TOPIC", "value": "${google_pubsub_topic.feed.id}"}]
                    }]}]}]
                }}
            }
        })
        .to_string(),
    )
    .unwrap();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let provisioners = data["provisioners"].as_array().unwrap();
    assert_eq!(
        provisioners
            .iter()
            .map(|p| p["contract"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "channel::pubsub::json-topic",
            "channel::pubsub::subscriptions/json-reader",
            "resource::cloudsql::json-db",
            "resource::cloudsql::json-db/orders",
        ],
        "{text}"
    );
    assert_eq!(data["iac_nodes"].as_array().unwrap().len(), 5, "{text}");
    let subscriptions = data["subscriptions"].as_array().unwrap();
    assert_eq!(subscriptions.len(), 1, "{text}");
    assert_eq!(subscriptions[0]["source"], "channel::pubsub::json-topic");
    assert_eq!(
        subscriptions[0]["target"],
        "channel::pubsub::subscriptions/json-reader"
    );
    let consumers = data["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 1, "{text}");
    assert_eq!(consumers[0]["contract"], "image::ghcr.io/acme/json-worker");
    let context = engine
        .context("google_cloud_run_v2_job.worker", Some(12000))
        .unwrap();
    assert!(
        context.contains("TOPIC") && context.contains("Terraform google_pubsub_topic json-topic"),
        "{context}"
    );
}

#[test]
fn gcp_projected_patterns_keep_raw_anchor_and_instance_evidence() {
    let (infra, app) = workspace("gcp_patterns");
    std::fs::write(
        infra.join("patterns.tf"),
        r#"
variable "unknown" {}
resource "google_pubsub_subscription" "anchored" {
  name = "${var.unknown}-reader"
  topic = google_pubsub_topic.orders.id
}
resource "google_pubsub_subscription" "too_short" {
  name = "${var.unknown}-a"
  topic = google_pubsub_topic.orders.id
}
resource "google_sql_database" "anchored" {
  name = "orders-${var.unknown}"
  instance = google_sql_database_instance.primary.name
}
resource "google_sql_database" "too_short" {
  name = "${var.unknown}"
  instance = google_sql_database_instance.primary.name
}
"#,
    )
    .unwrap();
    std::fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/gcp/infra\"\n[[resource]]\nkind = \"cloudsql\"\nid = \"orders-db/orders-prod\"\nrole = \"read\"\n",
    )
    .unwrap();
    std::fs::write(
        app.join("settings.yaml"),
        "reader: projects/p/subscriptions/eu-reader\n",
    )
    .unwrap();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let provisioners = data["provisioners"].as_array().unwrap();
    assert_eq!(provisioners.len(), 10, "{text}");
    assert!(
        provisioners
            .iter()
            .all(|p| !p["node"].as_str().unwrap().ends_with(".too_short")),
        "{text}"
    );
    let provisions = data["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 2, "{text}");
    assert_eq!(
        provisions
            .iter()
            .map(|p| p["contract"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "channel::pubsub::subscriptions/eu-reader",
            "resource::cloudsql::orders-db/orders-prod",
        ]),
        "{text}"
    );
    assert!(
        provisions
            .iter()
            .all(|p| p["level"] == "inferred" && p["reason"].as_str().unwrap().contains("pattern")),
        "{text}"
    );
}

#[test]
fn cloud_run_environment_budget_discards_entries_without_losing_its_image() {
    let (infra, _) = workspace("gcp_environment_budget");
    // Repeating a retained image across entries crosses the real 1 MiB environment ceiling,
    // while the source itself stays small. No huge resource fleet is needed for this guard.
    let image = format!("registry.example.com/{}:v1", "a".repeat(16384));
    let env = (0..128)
        .map(|i| format!("env {{\n name = \"V{i}\"\n value = \"known\"\n}}\n"))
        .collect::<String>();
    assert!(128 * image.len() > 1024 * 1024);
    let source = format!(
        "resource \"google_cloud_run_v2_job\" \"bounded\" {{\n template {{\n template {{\n containers {{\n image = \"{image}\"\n{env}\n}}\n containers {{\n image = \"ghcr.io/acme/budget-sidecar:v1\"\n env {{\n name = \"SIDE\"\n value = \"discarded\"\n}}\n}}\n}}\n}}\n}}\n"
    );
    assert!(source.len() < 1024 * 1024);
    std::fs::write(infra.join("run.tf"), source).unwrap();
    let started = std::time::Instant::now();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let consumers = data["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 2, "{} image runners", consumers.len());
    assert_eq!(
        consumers
            .iter()
            .map(|c| c["contract"].as_str().unwrap().to_string())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            format!("image::{}", image.strip_suffix(":v1").unwrap()),
            "image::ghcr.io/acme/budget-sidecar".into(),
        ])
    );
    let context = engine
        .context("google_cloud_run_v2_job.bounded", Some(64000))
        .unwrap();
    assert!(
        context.lines().all(|line| !line.starts_with("env ")),
        "environment entries survive their budget cut"
    );
    assert!(text.len() < 64 * 1024);
    let elapsed = started.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(30), "{elapsed:?}");
    eprintln!(
        "Cloud Run environment byte ceiling: {elapsed:?}, {} exported bytes",
        text.len()
    );
}

#[test]
fn cloud_run_empty_blocks_spend_work_before_expanding_a_full_file() {
    let (infra, _) = workspace("gcp_empty_blocks");
    let body = format!(
        "resource \"google_cloud_run_v2_job\" \"empty\" {{\n count = 1000\n name = \"worker-${{count.index}}\"\n template {{\n template {{\n containers {{\n image = \"ghcr.io/acme/empty-worker:v1\"\n{}\n}}\n}}\n}}\n}}\n",
        "env {}\n".repeat(10000)
    );
    let source = format!("{body}#{}\n", "p".repeat(1024 * 1024 - body.len() - 2));
    assert_eq!(source.len(), 1024 * 1024);
    std::fs::write(infra.join("run.tf"), source).unwrap();
    let started = std::time::Instant::now();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let consumers = data["consumers"].as_array().unwrap();
    assert!(
        !consumers.is_empty() && consumers.len() < 1000,
        "{} runners, structural work must exhaust before all instances",
        consumers.len()
    );
    assert!(
        consumers
            .iter()
            .all(|c| c["contract"] == "image::ghcr.io/acme/empty-worker")
    );
    assert!(text.len() < 8 * 1024 * 1024);
    let elapsed = started.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(30), "{elapsed:?}");
    eprintln!(
        "Cloud Run full-file structural work: {elapsed:?}, {} runners, {} exported bytes",
        consumers.len(),
        text.len()
    );
}
