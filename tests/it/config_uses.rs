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
    let (_, reports) = engine.status_report().unwrap();
    let reports = reports.into_iter().flatten().collect::<Vec<_>>().join("\n");
    assert!(
        reports.contains("channel::sqs::absent") && reports.contains("worker.queue"),
        "{reports}"
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
