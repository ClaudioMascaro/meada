//! Directionless config uses (#661), observed through the public engine.
use crate::common::{assert_golden, fixture_repo};
use meada::engine::Engine;

fn workspace(case: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let infra = fixture_repo(
        case,
        "config-use-infra",
        Some("https://example.com/config-use/infra.git"),
    );
    let app = fixture_repo(
        case,
        "config-use-app",
        Some("https://example.com/config-use/app.git"),
    );
    (infra, app)
}

#[test]
fn config_values_are_directionless_uses_at_their_keys() {
    let (infra, app) = workspace("config_use_export");
    for (path, name) in [(&infra, "infra"), (&app, "app")] {
        let text = Engine::open(path).unwrap().export().unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            json["provisions"].as_array().map(Vec::len),
            Some(7),
            "{text}"
        );
        assert_eq!(json["providers"], serde_json::json!([]));
        assert_eq!(json["consumers"], serde_json::json!([]));
        assert_eq!(json["links"], serde_json::json!([]));
        for link in json["provisions"].as_array().unwrap() {
            assert_eq!(link["side"], "example.com/config-use/app");
            let at = link["at"].as_str().unwrap();
            assert!(
                at.starts_with("application.yml#spring.") || at.starts_with("values.yaml#worker."),
                "{link}"
            );
            assert_eq!(
                link["level"],
                if at.starts_with("values.yaml") {
                    "inferred"
                } else {
                    "exact"
                }
            );
        }
        if name == "app" {
            assert_eq!(json["uses"].as_array().map(Vec::len), Some(7));
        } else {
            assert!(json.get("uses").is_none());
        }
        let mut export = meada_core::export::parse(&text).unwrap();
        export.commit = None;
        export.dirty = None;
        assert_golden(
            &format!("config-use-{name}.export.json"),
            &export.write().unwrap(),
        );
    }
}

#[test]
fn config_use_queries_show_keys_and_trace_only_provisioning() {
    use meada::engine::{Direction, Start};
    let (infra, _) = workspace("config_use_queries");
    let engine = Engine::open(&infra).unwrap();
    let context = engine.context("channel::sqs::orders", Some(12000)).unwrap();
    assert!(
        context.contains("use application.yml#spring.messaging.endpoint (line 4) [exact]"),
        "{context}"
    );
    assert!(
        context.contains("use values.yaml#worker.queue (line 2) [inferred]"),
        "{context}"
    );
    assert!(
        !context.contains("producer") && !context.contains("consumer"),
        "{context}"
    );
    let search = engine
        .search("orders", Some("channel"), Some(12000))
        .unwrap();
    assert!(
        search.contains("application.yml#spring.messaging.endpoint (line 4)"),
        "{search}"
    );
    let site = "example.com/config-use/app::values.yaml#worker.queue";
    let key_context = engine.context(site, Some(12000)).unwrap();
    assert!(key_context.contains("provisioners:"), "{key_context}");
    let up = engine
        .trace(Start::From(site), Direction::Up, Some(12000))
        .unwrap();
    assert!(
        up.contains("aws_sqs_queue.orders") && up.contains("inferred"),
        "{up}"
    );
    let down = engine
        .trace(
            Start::From("aws_sqs_queue.orders"),
            Direction::Down,
            Some(12000),
        )
        .unwrap();
    assert!(down.contains("values.yaml#worker.queue (line 2)"), "{down}");
    let aggregate = engine
        .trace(
            Start::From("example.com/config-use/app"),
            Direction::Up,
            Some(12000),
        )
        .unwrap();
    assert!(
        aggregate.contains("provisions channel::sqs::orders") && aggregate.contains("worker.queue"),
        "{aggregate}"
    );
    assert_golden("config-use.context.txt", &(context + &key_context));
    assert_golden("config-use.search.txt", &search);
    assert_golden("config-use.trace.txt", &(up + &down));
}

#[test]
fn config_use_refresh_and_secret_boundaries_preserve_complete_sets() {
    use std::fs;
    let (infra, app) = workspace("config_use_refresh");
    fs::write(app.join("controls.yaml"), "ancestor:\n  queue:\n    name: orders\nqueue_url: orders\nqueue: '${QUEUE}'\nbucket: false\ntable: 17\nitems:\n  - queue: orders\n\"dot.queue\": orders\nbad:\n  queue: arn:aws:sqs:eu-west-1::orders\n").unwrap();
    fs::write(app.join("secret.yaml"), "apiVersion: v1\nkind: Secret\nmetadata:\n  name: hidden\nstringData:\n  queue: orders\n  reference: arn:aws:sns:eu-west-1:123456789012:events\n").unwrap();
    fs::write(
        app.join("long.json"),
        serde_json::json!({"queue": "x".repeat(1025)}).to_string(),
    )
    .unwrap();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["provisions"].as_array().unwrap().len(), 8, "{text}");
    let app_json: serde_json::Value =
        serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    let uses = app_json["uses"].as_array().unwrap();
    assert_eq!(uses.len(), 8, "{app_json}");
    assert!(uses.iter().all(
        |u| u["at"].as_str().unwrap().starts_with("application.yml#")
            || u["at"].as_str().unwrap().starts_with("values.yaml#worker.")
            || u["at"] == "controls.yaml#items[0].queue"
    ));
    let values = app.join("values.yaml");
    let original = fs::read_to_string(&values).unwrap();
    fs::write(&values, original.replace("queue: orders", "queue: absent")).unwrap();
    let warm = engine.export().unwrap();

    let changed: serde_json::Value = serde_json::from_str(&warm).unwrap();
    assert_eq!(changed["provisions"].as_array().unwrap().len(), 7);
    assert!(
        changed["provisions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["at"] != "values.yaml#worker.queue")
    );
    // A key-name use is `inferred`: no orphan line, while `context` shows the refreshed side (#797).
    let (_, reports) = engine.status_report().unwrap();
    let reports = reports.into_iter().flatten().collect::<Vec<_>>().join("\n");
    assert!(!reports.contains("channel::sqs::absent"), "{reports}");
    let context = engine.context("channel::sqs::absent", None).unwrap();
    assert!(
        context.contains("use values.yaml#worker.queue") && context.contains("[inferred]"),
        "{context}"
    );
    fs::write(&values, original).unwrap();
    assert_eq!(engine.export().unwrap(), text);
    drop(engine);
    fs::remove_dir_all(infra.join(".meada")).unwrap();
    fs::remove_dir_all(app.join(".meada")).unwrap();
    assert_eq!(Engine::open(&infra).unwrap().export().unwrap(), text);
}

#[test]
fn config_use_classification_stays_bounded_on_a_full_file() {
    use std::{
        fs,
        time::{Duration, Instant},
    };
    let (infra, app) = workspace("config_use_hostile");
    // A Terraform var file keeps its values past `VALUES_MAX` (#817), so the whole file is still
    // classified; a generic YAML file of this size is data and gives no use at all.
    let mut input = String::from("{\n");
    let mut count = 0;
    while input.len() < (1 << 20) - 2048 {
        if count > 0 {
            input += ",\n";
        }
        input += &format!("\"v{count}\": \"arn:aws:sqs:eu-west-1:123456789012:orders\"");
        count += 1;
    }
    input += "\n}\n";
    fs::write(app.join("wide.tfvars.json"), input).unwrap();
    let start = Instant::now();
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let elapsed = start.elapsed();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["provisions"].as_array().unwrap().len(), count + 7);
    assert!(elapsed < Duration::from_secs(30), "{elapsed:?}");
    eprintln!("config use full-file probe: {elapsed:?}, {count} identifiers");
}

#[test]
fn s3_object_key_characters_leave_the_bucket_use_exact() {
    use std::fs;
    let (infra, app) = workspace("config_use_s3_suffix");
    fs::write(
        app.join("objects.yaml"),
        "one: s3://archive/reports/a b.json\ntwo: s3://archive/reports/version#1.json\n",
    )
    .unwrap();
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let links = json["provisions"].as_array().unwrap();
    assert_eq!(links.len(), 9, "{text}");
    for at in ["objects.yaml#one", "objects.yaml#two"] {
        assert!(links.iter().any(|l| l["at"] == at
            && l["contract"] == "resource::s3::archive"
            && l["level"] == "exact"));
    }
}

#[test]
fn config_use_partial_queries_equal_whole_exports() {
    use meada::engine::{Direction, Start};
    let (infra, _) = workspace("config_use_partial");
    let engine = Engine::open(&infra).unwrap();
    let ask = || {
        let mut text = engine
            .search("orders", Some("contract"), Some(12000))
            .unwrap();
        for name in [
            "channel::sqs::orders",
            "aws_sqs_queue.orders",
            "values.yaml#worker.queue",
        ] {
            text += &engine.context(name, Some(12000)).unwrap();
            for direction in [Direction::Up, Direction::Down] {
                text += &engine
                    .trace(Start::From(name), direction, Some(12000))
                    .unwrap();
            }
        }
        text
    };
    assert_eq!(ask(), meada::engine::with_every_row(ask));
}

#[test]
fn config_uses_and_workload_images_keep_their_own_edges_on_refresh() {
    use meada::engine::{Direction, Start};
    use std::fs;

    let (infra, app) = workspace("config_use_images");
    fs::write(
        app.join("orders.py"),
        "def orders():\n    return 'orders'\n",
    )
    .unwrap();
    fs::write(
        app.join("compose.yaml"),
        "services:\n  orders:\n    build: .\n    image: ghcr.io/acme/orders:local\n",
    )
    .unwrap();
    fs::write(app.join("Dockerfile"), "FROM scratch\nCOPY . .\n").unwrap();
    fs::write(infra.join("deployment.yaml"), "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: orders\n  namespace: shop\nspec:\n  template:\n    spec:\n      containers:\n        - name: orders\n          image: ghcr.io/acme/orders:v1\n").unwrap();

    let engine = Engine::open(&app).unwrap();
    let original = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&original).unwrap();
    assert_eq!(export["uses"].as_array().unwrap().len(), 7);
    assert_eq!(export["provisions"].as_array().unwrap().len(), 7);
    for section in ["providers", "consumers", "links"] {
        let rows = export[section].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{section}: {rows:?}");
        assert_eq!(rows[0]["contract"], "image::ghcr.io/acme/orders");
    }
    let execution = &export["executions"].as_array().unwrap()[..];
    assert_eq!(execution.len(), 1);
    assert_eq!(execution[0]["path"], ".");
    assert_eq!(execution[0]["level"], "exact");
    assert_eq!(
        execution[0]["workload"],
        "example.com/config-use/app::compose.yaml::services.orders"
    );
    let queue = engine.context("channel::sqs::orders", None).unwrap();
    assert!(queue.contains("use values.yaml#worker.queue"), "{queue}");
    assert!(
        !queue.contains("producer") && !queue.contains("consumer"),
        "{queue}"
    );
    let provisioned = engine
        .trace(Start::From("aws_sqs_queue.orders"), Direction::Down, None)
        .unwrap();
    assert!(
        provisioned.contains("values.yaml#worker.queue"),
        "{provisioned}"
    );
    assert!(!provisioned.contains("runs code"), "{provisioned}");
    let runs = engine
        .trace(
            Start::From("Deployment.apps/shop/orders"),
            Direction::Down,
            None,
        )
        .unwrap();
    assert!(runs.contains("image::ghcr.io/acme/orders"), "{runs}");
    assert!(runs.contains("orders.py"), "{runs}");
    assert!(!runs.contains("worker.queue"), "{runs}");
    let up = engine
        .trace(Start::From("orders"), Direction::Up, None)
        .unwrap();
    assert!(up.contains("Deployment.apps/shop/orders"), "{up}");
    assert!(up.contains("services.orders"), "{up}");

    let values = app.join("values.yaml");
    let before = fs::read_to_string(&values).unwrap();
    fs::write(&values, before.replace("queue: orders", "queue: absent")).unwrap();
    let changed: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(changed["uses"].as_array().unwrap().len(), 7);
    assert_eq!(changed["provisions"].as_array().unwrap().len(), 6);
    for section in ["providers", "consumers", "links", "executions", "iac_nodes"] {
        assert_eq!(changed[section], export[section], "{section}");
    }
    fs::write(values, before).unwrap();
    assert_eq!(engine.export().unwrap(), original);
}

/// The `at` of every config use of the repository at `repo`.
fn uses_at(repo: &std::path::Path) -> Vec<String> {
    let text = Engine::open(repo).unwrap().export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    (json["uses"].as_array().into_iter().flatten())
        .map(|u| u["at"].as_str().unwrap().to_string())
        .collect()
}

/// A document the reader of a contract kind recognizes means what that reader says (ADR 0026,
/// "Orphan sides", #800): a Prometheus rule's `queue` label is an alert label, no use of a
/// queue, while a queue ARN in the same rule is still a use at `exact`.
#[test]
fn a_rule_file_the_alert_reader_takes_feeds_no_key_name_rule() {
    let repo = fixture_repo(
        "config_use_alert_rule",
        "orphan-alert-rule",
        Some("https://example.com/acme/o5.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let (_, reports) = engine.status_report().unwrap();
    let orphans = (reports.into_iter().flatten())
        .filter(|l| l.starts_with("orphan "))
        .collect::<Vec<_>>();
    assert_eq!(orphans, Vec::<String>::new());
    let jobs = engine.context("channel::sqs::jobs", None).unwrap();
    assert!(!jobs.contains("alerts/app.rules.yaml"), "{jobs}");
    let alerts = engine.search("JobFailures", Some("alert"), None).unwrap();
    assert!(
        alerts.contains(
            "example.com/acme/o5 alerts/app.rules.yaml#groups[0].rules[0] \
             alert::alertname::JobFailures"
        ),
        "{alerts}"
    );
    let values = engine.search("jobs", Some("value"), None).unwrap();
    assert!(
        values.contains("o5 alerts/app.rules.yaml:8 groups[0].rules[0].labels.queue = \"jobs\""),
        "{values}"
    );
    // Named `jobs`, the provisioned queue keeps its provisioner alone.
    std::fs::write(
        repo.join("infra/main.tf"),
        "resource \"aws_sqs_queue\" \"jobs\" {\n  name = \"jobs\"\n}\n",
    )
    .unwrap();
    let jobs = Engine::open(&repo)
        .unwrap()
        .context("channel::sqs::jobs", None)
        .unwrap();
    let lines = jobs.lines().collect::<Vec<_>>();
    assert!(
        lines.len() == 2
            && lines[1].starts_with(
                "  example.com/acme/o5 provisioner infra/main.tf:1 aws_sqs_queue.jobs (root infra)"
            ),
        "{jobs}"
    );
    // A queue ARN in the rule is a use at `exact`; the same label in a file no reader recognizes
    // keeps its `inferred` use.
    let rule = std::fs::read_to_string(repo.join("alerts/app.rules.yaml")).unwrap();
    std::fs::write(
        repo.join("alerts/app.rules.yaml"),
        format!("{rule}      queue_arn: arn:aws:sqs:us-east-1:123456789012:jobs\n"),
    )
    .unwrap();
    std::fs::create_dir_all(repo.join("config")).unwrap();
    std::fs::write(
        repo.join("config/app.yaml"),
        "labels:\n  severity: error\n  queue: jobs\n",
    )
    .unwrap();
    assert_eq!(
        uses_at(&repo),
        [
            "alerts/app.rules.yaml#groups[0].rules[0].labels.queue_arn",
            "config/app.yaml#labels.queue"
        ]
    );
    let jobs = Engine::open(&repo)
        .unwrap()
        .context("channel::sqs::jobs", None)
        .unwrap();
    assert!(
        jobs.contains(
            "use alerts/app.rules.yaml#groups[0].rules[0].labels.queue_arn (line 9) [exact]"
        ) && jobs.contains("use config/app.yaml#labels.queue (line 3) [inferred]"),
        "{jobs}"
    );
}

/// In a Kubernetes stream only the document the alert reader takes a rule from is its own: the
/// key-name rule still reads the stream's other documents (#800).
#[test]
fn only_the_rule_document_of_a_stream_is_kept_from_the_key_name_rule() {
    let repo = fixture_repo(
        "config_use_rule_stream",
        "orphan-alert-rule",
        Some("https://example.com/acme/o5.git"),
    );
    std::fs::remove_file(repo.join("alerts/app.rules.yaml")).unwrap();
    std::fs::create_dir_all(repo.join("k8s")).unwrap();
    std::fs::write(
        repo.join("k8s/app.yaml"),
        "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: worker\ndata:\n  queue: jobs\n---\n\
         apiVersion: monitoring.coreos.com/v1\nkind: PrometheusRule\nmetadata:\n  name: app\n\
         spec:\n  groups:\n  - name: app\n    rules:\n    - alert: JobFailures\n      \
         expr: up == 0\n      labels:\n        queue: jobs\n",
    )
    .unwrap();
    assert_eq!(uses_at(&repo), ["k8s/app.yaml#[0].data.queue"]);
}

/// A provider document (an OpenAPI file of a repository with a `[serve]` table) is the HTTP
/// kind's own; without the table the same file is configuration the key-name rule reads (#800).
#[test]
fn a_provider_document_feeds_no_key_name_rule() {
    let repo = fixture_repo(
        "config_use_provider_document",
        "orphan-alert-rule",
        Some("https://example.com/acme/o5.git"),
    );
    std::fs::remove_file(repo.join("alerts/app.rules.yaml")).unwrap();
    std::fs::write(
        repo.join("openapi.yaml"),
        "openapi: 3.0.3\ninfo:\n  title: Jobs\n  version: 1.0.0\npaths: {}\nx-messaging:\n  \
         queue: jobs\n",
    )
    .unwrap();
    std::fs::write(repo.join("meada.toml"), "[serve]\n").unwrap();
    assert_eq!(uses_at(&repo), Vec::<String>::new());
    std::fs::remove_file(repo.join("meada.toml")).unwrap();
    assert_eq!(uses_at(&repo), ["openapi.yaml#[\"x-messaging\"].queue"]);
}

/// Each document the alert reader takes something from (a Loki rule file, a Grafana alert rule
/// file, an Alertmanager configuration, an `AlertmanagerConfig` object of a stream) feeds no
/// key-name rule, while a Terraform `grafana_rule_group` (an IaC source) and a promtool test file
/// (which no reader takes) are still read.
#[test]
fn each_document_the_alert_reader_takes_feeds_no_key_name_rule() {
    let repo = fixture_repo(
        "config_use_alert_documents",
        "orphan-alert-rule",
        Some("https://example.com/acme/o5.git"),
    );
    let files = [
        (
            "loki/rules.yaml",
            "groups:\n- name: logs\n  rules:\n  - alert: JobErrors\n    \
             expr: 'sum(rate({app=\"worker\"} |= \"error\" [5m])) > 0'\n    \
             labels:\n      queue: jobs\n",
        ),
        (
            "grafana/alerts.yaml",
            "apiVersion: 1\ngroups:\n- orgId: 1\n  name: jobs\n  folder: alerts\n  interval: 1m\n  \
             rules:\n  - uid: job-failures\n    title: JobFailures\n    condition: A\n    \
             data:\n    - refId: A\n      datasourceUid: prom\n      model:\n        \
             expr: job_failures_total > 0\n    labels:\n      queue: jobs\n",
        ),
        (
            "alertmanager/alertmanager.yml",
            "route:\n  receiver: team\n  routes:\n  - receiver: jobs\n    match:\n      \
             queue: jobs\nreceivers:\n- name: team\n- name: jobs\n",
        ),
        (
            "k8s/alerting.yaml",
            "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: worker\ndata:\n  queue: jobs\n---\n\
             apiVersion: monitoring.coreos.com/v1alpha1\nkind: AlertmanagerConfig\nmetadata:\n  \
             name: jobs\n  labels:\n    queue: jobs\nspec:\n  route:\n    receiver: jobs\n  \
             receivers:\n  - name: jobs\n",
        ),
        (
            "infra/alerts.tf",
            "resource \"grafana_rule_group\" \"jobs\" {\n  name = \"jobs\"\n  rule {\n    \
             name   = \"JobFailures\"\n    labels = { queue = \"jobs\" }\n  }\n}\n",
        ),
        (
            "alerts/app_test.yaml",
            "rule_files:\n- app.rules.yaml\ntests:\n- interval: 1m\n  alert_rule_test:\n  \
             - eval_time: 5m\n    alertname: JobFailures\n    exp_alerts:\n    - exp_labels:\n        \
             queue: jobs\n",
        ),
    ];
    for (path, text) in files {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    assert_eq!(
        uses_at(&repo),
        [
            "alerts/app_test.yaml#tests[0].alert_rule_test[0].exp_alerts[0].exp_labels.queue",
            "infra/alerts.tf#resource.grafana_rule_group.jobs.rule.labels.queue",
            "k8s/alerting.yaml#[0].data.queue",
        ]
    );
}
