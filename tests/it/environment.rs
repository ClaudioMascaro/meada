//! Environment reads resolve only through the workloads running their code (#663).

use crate::common::{assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};

fn workspace(case: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        fixture_repo(
            case,
            "environment-app",
            Some("https://example.com/acme/app.git"),
        ),
        fixture_repo(
            case,
            "environment-deploy",
            Some("https://example.com/acme/deploy.git"),
        ),
    )
}

#[test]
fn a_workload_environment_joins_its_code_to_the_provisioned_queue() {
    let (app, deploy) = workspace("environment_queue");
    let engine = Engine::open(&app).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let side = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["site"].as_str().unwrap().contains("send.ts"))
        .unwrap();
    assert_eq!(side["contract"], "channel::sqs::orders", "{export}");
    let link = export["provisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["side"] == side["site"])
        .unwrap();
    assert_eq!(link["level"], "inferred");
    let reason = link["reason"].as_str().unwrap();
    assert!(
        reason.contains("Deployment.apps/shop/sender") && reason.contains("QUEUE_URL"),
        "{reason}"
    );
    let context = Engine::open(&deploy)
        .unwrap()
        .context("Deployment.apps/shop/sender", None)
        .unwrap();
    assert!(
        context.contains("QUEUE_URL")
            && context.contains("ConfigMap")
            && context.contains("orders"),
        "{context}"
    );
    let trace = engine
        .trace(Start::From("aws_sqs_queue.orders"), Direction::Down, None)
        .unwrap();
    assert!(trace.contains("send.ts"), "{trace}");
    assert_golden("environment.queue.export.json", &engine.export().unwrap());
    assert_golden("environment.queue.context.txt", &context);
}

#[test]
fn env_from_and_explicit_entries_keep_container_namespace_and_secret_scope() {
    let (app, deploy) = workspace("environment_sources");
    let source = std::fs::read_to_string(app.join("send.ts")).unwrap();
    for variable in [
        "FROM_QUEUE",
        "SIDE_QUEUE",
        "SECRET_QUEUE",
        "MISSING_QUEUE",
        "WRONG_NAMESPACE",
        "OTHER_QUEUE",
    ] {
        std::fs::write(
            app.join(format!("{variable}.ts")),
            source.replace("QUEUE_URL", variable),
        )
        .unwrap();
    }
    std::fs::write(
        deploy.join("deployment.yaml"),
        r#"apiVersion: v1
kind: ConfigMap
metadata: {name: queues, namespace: shop}
data: {QUEUE: orders}
---
apiVersion: v1
kind: ConfigMap
metadata: {name: wrong, namespace: elsewhere}
data: {queue: orders}
---
apiVersion: v1
kind: Secret
metadata: {name: secret, namespace: shop}
stringData: {SECRET_QUEUE: orders}
---
apiVersion: apps/v1
kind: Deployment
metadata: {name: sender, namespace: shop}
spec:
  template:
    spec:
      containers:
        - name: sender
          image: ghcr.io/acme/environment-app:v1
          envFrom:
            - {prefix: FROM_, configMapRef: {name: queues}}
            - {secretRef: {name: secret}}
          env:
            - {name: QUEUE_URL, value: orders}
            - {name: MISSING_QUEUE, valueFrom: {configMapKeyRef: {name: absent, key: queue}}}
            - {name: WRONG_NAMESPACE, valueFrom: {configMapKeyRef: {name: wrong, key: queue}}}
        - name: sidecar
          image: ghcr.io/acme/other:v1
          env: [{name: SIDE_QUEUE, value: orders}]
---
apiVersion: v1
kind: Pod
metadata: {name: unrelated-one, namespace: shop}
spec:
  containers:
    - name: other
      image: ghcr.io/acme/other:v1
      env: [{name: OTHER_QUEUE, value: orders}]
---
apiVersion: v1
kind: Pod
metadata: {name: unrelated-two, namespace: shop}
spec:
  containers:
    - name: other
      image: ghcr.io/acme/other:v1
      env: [{name: OTHER_QUEUE, value: orders}]
"#,
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let providers = export["providers"].as_array().unwrap();
    for (file, expected) in [
        ("send.ts", "channel::sqs::orders"),
        ("FROM_QUEUE.ts", "channel::sqs::orders"),
        ("SIDE_QUEUE.ts", "channel::sqs"),
        ("SECRET_QUEUE.ts", "channel::sqs"),
        ("MISSING_QUEUE.ts", "channel::sqs"),
        ("WRONG_NAMESPACE.ts", "channel::sqs"),
        ("OTHER_QUEUE.ts", "channel::sqs"),
    ] {
        let p = providers
            .iter()
            .find(|p| p["site"].as_str().unwrap().contains(&format!("::{file}::")))
            .unwrap();
        assert_eq!(p["contract"], expected, "{file}: {export}");
    }
    assert!(engine.open_items().unwrap().is_empty());
    let context = Engine::open(&deploy)
        .unwrap()
        .context("Deployment.apps/shop/sender", None)
        .unwrap();
    assert!(
        context.contains("FROM_QUEUE")
            && context.contains("envFrom")
            && context.contains("SECRET_QUEUE")
            && context.contains("redacted"),
        "{context}"
    );
}

#[test]
fn compose_defaults_and_overrides_resolve_only_code_in_the_build_context() {
    let (app, deploy) = workspace("environment_compose");
    std::fs::remove_dir_all(app.join(".github")).unwrap();
    std::fs::create_dir(app.join("code")).unwrap();
    std::fs::copy(app.join("send.ts"), app.join("code/send.ts")).unwrap();
    std::fs::write(app.join("code/Dockerfile"), "FROM scratch\nCOPY . .\n").unwrap();
    std::fs::write(app.join("code/queue.py"), "import os\nimport boto3\nqueue = boto3.client('sqs')\ndef publish():\n    queue.send_message(QueueUrl=os.getenv('QUEUE_URL'), MessageBody='x')\ndef receive():\n    return queue.receive_message(QueueUrl=os.getenv('QUEUE_URL'))\n").unwrap();
    std::fs::write(app.join("code/parameter.ts"), "import { SSMClient, GetParameterCommand } from '@aws-sdk/client-ssm';\nconst client = new SSMClient({});\nexport function parameter() { return client.send(new GetParameterCommand({ Name: process.env.PARAMETER })); }\n").unwrap();
    std::fs::write(app.join("compose.yaml"), "services:\n  worker:\n    build: ./code\n    environment:\n      QUEUE_URL: ${QUEUE_URL:-https://sqs.us-east-1.amazonaws.com/123456789012/orders}\n      PARAMETER: /app/key\n").unwrap();
    std::fs::write(app.join("compose.override.yaml"), "services:\n  worker:\n    environment:\n      - QUEUE_URL=${OVERRIDE:-https://sqs.us-east-1.amazonaws.com/123456789012/jobs}\n").unwrap();
    std::fs::write(deploy.join("queue.tf"), "resource \"aws_sqs_queue\" \"jobs\" { name = \"jobs\" }\nresource \"aws_ssm_parameter\" \"key\" {\n  name = \"/app/key\"\n  type = \"String\"\n}\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    for (file, expected) in [
        ("code/send.ts", "channel::sqs::jobs"),
        ("code/queue.py", "channel::sqs::jobs"),
        ("send.ts", "channel::sqs"),
    ] {
        let p = export["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["site"].as_str().unwrap().contains(&format!("::{file}::")))
            .unwrap();
        assert_eq!(p["contract"], expected, "{file}: {export}");
    }
    let consumers = export["consumers"].as_array().unwrap();
    assert!(
        consumers
            .iter()
            .any(|c| c["contract"] == "channel::sqs::jobs"),
        "{export}"
    );
    assert!(
        consumers
            .iter()
            .any(|c| c["contract"] == "resource::ssm::/app/key"),
        "{export}"
    );
    assert_eq!(
        export["provisions"].as_array().unwrap().len(),
        4,
        "{export}"
    );
    assert!(
        export["provisions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["level"] == "inferred"
                && p["reason"].as_str().unwrap().contains("services.worker"))
    );
    assert!(
        export["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["level"] == "inferred")
    );
    let context = engine.context("services.worker", None).unwrap();
    assert!(
        context.contains("Compose default")
            && context.contains("QUEUE_URL")
            && context.contains("/app/key"),
        "{context}"
    );
    assert_golden("environment.compose.export.json", &engine.export().unwrap());
    std::fs::write(
        app.join("compose.override.yaml"),
        "services:\n  worker:\n    environment:\n      - QUEUE_URL\n",
    )
    .unwrap();
    let changed: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        changed["providers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["contract"] == "channel::sqs"),
        "{changed}"
    );
    assert!(engine.open_items().unwrap().is_empty());
    std::fs::write(app.join("compose.override.yaml"), "services:\n  worker:\n    environment:\n      QUEUE_URL: https://sqs.us-east-1.amazonaws.com/123456789012/${UNREAD_QUEUE}\n").unwrap();
    let unresolved: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        unresolved["providers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["contract"] == "channel::sqs"),
        "{unresolved}"
    );
    assert!(
        engine
            .context("services.worker", None)
            .unwrap()
            .contains("QUEUE_URL -> Compose environment (unresolved): unresolved")
    );
}

#[test]
fn overlays_preserve_each_workload_reason_without_duplicating_the_code_side() {
    let (app, deploy) = workspace("environment_overlays");
    std::fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/acme/environment-east:v1,ghcr.io/acme/environment-west:v1\n").unwrap();
    for region in ["east", "west"] {
        std::fs::create_dir_all(deploy.join(format!("overlays/{region}"))).unwrap();
        std::fs::write(deploy.join(format!("overlays/{region}/kustomization.yaml")), format!("resources:\n  - ../../deployment.yaml\nnamePrefix: {region}-\nnamespace: {region}\nimages:\n  - name: ghcr.io/acme/environment-app\n    newName: ghcr.io/acme/environment-{region}\n")).unwrap();
    }
    let engine = Engine::open(&app).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let sides = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["contract"].as_str().unwrap().starts_with("channel::"))
        .collect::<Vec<_>>();
    assert_eq!(export["providers"].as_array().unwrap().len(), 3, "{export}");
    assert_eq!(sides.len(), 1, "{export}");
    assert_eq!(sides[0]["contract"], "channel::sqs::orders");
    let links = export["provisions"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{export}");
    let reason = links[0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("Deployment.apps/east/east-sender")
            && reason.contains("Deployment.apps/west/west-sender")
            && reason.contains("QUEUE_URL"),
        "{reason}"
    );
    let context = Engine::open(&deploy)
        .unwrap()
        .context("Deployment.apps/east/east-sender", None)
        .unwrap();
    assert!(
        context.contains("env sender QUEUE_URL -> ConfigMap/east/east-queues key queue")
            && context.contains("image::ghcr.io/acme/environment-east"),
        "{context}"
    );
    assert!(
        !context.contains("environment-west") && !context.contains("west-queues"),
        "{context}"
    );
    let path = deploy.join("deployment.yaml");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        source.replace("123456789012/orders", "123456789012/jobs"),
    )
    .unwrap();
    let changed: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        changed["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "channel::sqs::jobs"),
        "{changed}"
    );
    assert!(changed.get("provisions").is_none(), "{changed}");
}

#[test]
fn nested_overlays_and_unknown_env_from_preserve_reference_and_override_scope() {
    let (app, deploy) = workspace("environment_nested");
    let source = std::fs::read_to_string(app.join("send.ts")).unwrap();
    std::fs::write(
        app.join("explicit.ts"),
        source.replace("QUEUE_URL", "QUEUE_EXPLICIT"),
    )
    .unwrap();
    std::fs::write(
        app.join("prefix.ts"),
        source.replace("QUEUE_URL", "UNRELATED_QUEUE"),
    )
    .unwrap();
    std::fs::remove_file(deploy.join("deployment.yaml")).unwrap();
    std::fs::write(
        deploy.join("kustomization.yaml"),
        "resources: [east, west]\n",
    )
    .unwrap();
    for (region, queue) in [("east", "orders"), ("west", "jobs")] {
        std::fs::create_dir_all(deploy.join(region)).unwrap();
        std::fs::write(
            deploy.join(format!("{region}/kustomization.yaml")),
            format!("resources: [objects.yaml]\nnamePrefix: {region}-\nnamespace: shop\n"),
        )
        .unwrap();
        std::fs::write(
            deploy.join(format!("{region}/objects.yaml")),
            format!(
                r#"apiVersion: v1
kind: ConfigMap
metadata: {{name: queues}}
data: {{QUEUE_URL: {queue}, QUEUE_EXPLICIT: wrong, UNRELATED_QUEUE: {queue}}}
---
apiVersion: apps/v1
kind: Deployment
metadata: {{name: sender}}
spec:
  template:
    spec:
      containers:
        - name: sender
          image: ghcr.io/acme/environment-app:v1
          envFrom:
            - configMapRef: {{name: queues}}
            - prefix: QUEUE_
              secretRef: {{name: external}}
          env:
            - name: QUEUE_EXPLICIT
              valueFrom:
                configMapKeyRef: {{name: queues, key: QUEUE_URL}}
"#
            ),
        )
        .unwrap();
    }
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 6, "{export}");
    let mut sides = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["contract"].as_str().unwrap().starts_with("channel::"))
        .map(|p| {
            (
                p["site"]
                    .as_str()
                    .unwrap()
                    .split("::")
                    .nth(1)
                    .unwrap()
                    .to_string(),
                p["contract"].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    sides.sort();
    assert_eq!(
        sides,
        [
            ("explicit.ts", "channel::sqs::jobs"),
            ("explicit.ts", "channel::sqs::orders"),
            ("prefix.ts", "channel::sqs::jobs"),
            ("prefix.ts", "channel::sqs::orders"),
            ("send.ts", "channel::sqs")
        ]
        .map(|(p, c)| (p.to_string(), c.to_string())),
        "{export}"
    );
    for p in export["providers"].as_array().unwrap().iter().filter(|p| {
        p["contract"]
            .as_str()
            .unwrap()
            .starts_with("channel::sqs::")
    }) {
        let (own, other) = if p["contract"] == "channel::sqs::orders" {
            ("east-sender", "west-sender")
        } else {
            ("west-sender", "east-sender")
        };
        let reason = p["via"].as_str().unwrap();
        assert!(reason.contains(own) && !reason.contains(other), "{p}");
    }
}

#[test]
fn many_workloads_match_only_the_variable_and_code_path_they_run() {
    use std::time::{Duration, Instant};
    let (app, deploy) = workspace("environment_growth");
    let source = std::fs::read_to_string(app.join("send.ts")).unwrap();
    std::fs::remove_file(app.join("send.ts")).unwrap();
    std::fs::remove_file(deploy.join("deployment.yaml")).unwrap();
    let mut images = String::new();
    let mut pods = String::new();
    for group in 0..2 {
        let mut compose = String::from("services:\n");
        for item in 0..256 {
            let id = group * 256 + item;
            let dir = app.join(format!("p{group}/code/s{item}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("send.ts"), &source).unwrap();
            compose += &format!(
                "  s{item}:\n    build: ./code/s{item}\n    environment:\n      QUEUE_URL: orders\n"
            );
            for var in 0..127 {
                compose += &format!("      NOISE_{var}: x\n");
            }
            images += &format!("            ghcr.io/acme/growth-{id}:v1\n");
            pods += &format!(
                "---\napiVersion: v1\nkind: Pod\nmetadata: {{name: p{id}}}\nspec:\n  containers:\n    - name: app\n      image: ghcr.io/acme/growth-{id}:v1\n      env:\n"
            );
            for var in 0..32 {
                pods += &format!("        - {{name: IMAGE_{id}_{var}, value: x}}\n");
            }
        }
        std::fs::write(app.join(format!("p{group}/compose.yaml")), compose).unwrap();
    }
    std::fs::write(app.join(".github/workflows/build.yml"), format!("jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: |\n{images}")).unwrap();
    // Several files keep each YAML stream below the shared document ceiling.
    let docs = pods
        .split("---\n")
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    for (part, docs) in docs.chunks(128).enumerate() {
        std::fs::write(deploy.join(format!("pods-{part}.yaml")), docs.join("---\n")).unwrap();
    }
    let started = Instant::now();
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    let elapsed = started.elapsed();
    eprintln!("512 direct workloads, 512 image workloads, 81920 environment entries: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(180), "{elapsed:?}");
    let sides = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["contract"].as_str().unwrap().starts_with("channel::"))
        .collect::<Vec<_>>();
    assert_eq!(export["providers"].as_array().unwrap().len(), 1024);
    assert_eq!(sides.len(), 512);
    assert!(
        sides
            .iter()
            .all(|p| p["contract"] == "channel::sqs::orders")
    );
    assert_eq!(export["provisions"].as_array().unwrap().len(), 512);
    assert_eq!(export["links"].as_array().unwrap().len(), 512);
    assert_eq!(export["executions"].as_array().unwrap().len(), 512);
}
