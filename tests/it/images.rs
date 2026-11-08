//! Image builders and workload runners (#662, ADR 0026).

use crate::common::{assert_golden, fixture_repo};
use meada::engine::Engine;

#[test]
fn workloads_link_to_the_repository_whose_workflow_builds_their_image() {
    let app = fixture_repo(
        "images_workflow",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let deploy = fixture_repo(
        "images_workflow",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(app.join("compose.yaml")).unwrap();
    std::fs::remove_file(deploy.join("values.yaml")).unwrap();
    std::fs::remove_file(deploy.join("overlays/prod/kustomization.yaml")).unwrap();
    let engine = Engine::open(&deploy).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(export["consumers"].as_array().unwrap().len(), 1, "{export}");
    assert_eq!(export["links"].as_array().unwrap().len(), 1, "{export}");
    assert_eq!(export["links"][0]["contract"], "image::ghcr.io/acme/orders");
    assert_eq!(export["links"][0]["provider"], "example.com/acme/app");
    assert_eq!(export["links"][0]["level"], "exact");
    let built: serde_json::Value =
        serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    assert_eq!(
        built["providers"][0]["at"],
        ".github/workflows/build.yml#jobs.image.steps[0].with.tags"
    );
    assert_golden("images.workflow.export.json", &engine.export().unwrap());
    assert_golden(
        "images.workflow.context.txt",
        &engine.context("Deployment.apps/shop/orders", None).unwrap(),
    );
}

#[test]
fn compose_and_helm_keep_evaluation_evidence_and_kustomize_runs_its_override() {
    let app = fixture_repo(
        "images_compose",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let deploy = fixture_repo(
        "images_compose",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    let engine = Engine::open(&deploy).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(export["consumers"].as_array().unwrap().len(), 2, "{export}");
    assert!(
        export["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["site"] == "example.com/acme/deploy::values.yaml::image.repository"),
        "{export}"
    );
    assert!(
        export["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["site"]
                == "example.com/acme/deploy::deployment.yaml::Deployment.apps/shop/prod-orders"),
        "{export}"
    );
    let helm = export["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["consumer"] == "example.com/acme/deploy::values.yaml::image.repository")
        .unwrap();
    assert_eq!(helm["level"], "inferred");
    assert!(
        helm["reason"]
            .as_str()
            .unwrap()
            .contains("values.yaml#image.repository"),
        "{helm}"
    );
    let app_engine = Engine::open(&app).unwrap();
    let context = app_engine.context("services.orders", None).unwrap();
    assert!(context.contains("image::ghcr.io/acme/orders"), "{context}");
    assert!(
        context.contains("code: example.com/acme/app . [exact]"),
        "{context}"
    );
    assert_golden("images.compose.context.txt", &context);
}

#[test]
fn trace_crosses_images_and_same_repository_code_paths_both_ways() {
    use meada::engine::{Direction, Start};
    fixture_repo(
        "images_trace",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let deploy = fixture_repo(
        "images_trace",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    let engine = Engine::open(&deploy).unwrap();
    let down = engine
        .trace(
            Start::From("Deployment.apps/shop/prod-orders"),
            Direction::Down,
            None,
        )
        .unwrap();
    assert!(down.contains("image::ghcr.io/acme/orders"), "{down}");
    assert!(down.contains("example.com/acme/app"), "{down}");
    assert!(down.contains("orders.py"), "{down}");
    let up = engine
        .trace(Start::From("orders"), Direction::Up, None)
        .unwrap();
    assert!(up.contains("Deployment.apps/shop/prod-orders"), "{up}");
    assert!(up.contains("services.orders"), "{up}");
    assert_golden("images.trace.txt", &format!("{down}\n{up}"));
}

#[test]
fn image_orphans_require_both_roles_in_the_visible_scope_and_refresh() {
    let repo = fixture_repo(
        "images_orphans",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    std::fs::remove_file(repo.join("compose.yaml")).unwrap();
    let engine = Engine::open(&repo).unwrap();
    let search = engine
        .search("image::ghcr.io/acme/orders", Some("image"), None)
        .unwrap();
    assert!(search.contains("no runner in the workspace"), "{search}");
    let status = engine
        .status_report()
        .unwrap()
        .1
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!status.contains("orphan image::"), "{status}");
    std::fs::write(
        repo.join("compose.yaml"),
        "services:\n  cache:\n    image: redis:7\n",
    )
    .unwrap();
    let status = engine
        .status_report()
        .unwrap()
        .1
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        status.contains("orphan image::ghcr.io/acme/orders: no runner"),
        "{status}"
    );
    assert!(
        status.contains("orphan image::docker.io/library/redis: no builder"),
        "{status}"
    );
    std::fs::remove_file(repo.join(".github/workflows/build.yml")).unwrap();
    let context = engine
        .context("image::docker.io/library/redis", None)
        .unwrap();
    assert!(context.contains("no builder in the workspace"), "{context}");
    let status = engine
        .status_report()
        .unwrap()
        .1
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!status.contains("orphan image::"), "{status}");
}

#[test]
fn build_configurations_publish_destinations_leave_base_images_out_and_keep_name_patterns() {
    let repo = fixture_repo(
        "images_builders",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    std::fs::remove_file(repo.join("compose.yaml")).unwrap();
    std::fs::write(
        repo.join(".github/workflows/build.yml"),
        r#"jobs:
  publish:
    env:
      KO_DOCKER_REPO: ghcr.io/acme/ko-orders
    steps:
      - uses: docker/metadata-action@v5
        with:
          images: |
            ghcr.io/acme/meta-orders
            name=ghcr.io/acme/disabled,enable=false
      - run: ko build --bare ./cmd/orders
      - uses: docker/build-push-action@v6
        with:
          tags: |
            ghcr.io/acme/dynamic-tag:${{ github.sha }}
            ghcr.io/${{ github.repository_owner }}/unknown:latest
"#,
    )
    .unwrap();
    std::fs::write(repo.join("skaffold.yaml"), "apiVersion: skaffold/v4beta11\nkind: Config\nbuild:\n  artifacts:\n    - image: ghcr.io/acme/skaffold-orders\n---\napiVersion: skaffold/v4beta11\nkind: Config\nbuild:\n  artifacts:\n    - image: ghcr.io/acme/skaffold-worker\n").unwrap();
    std::fs::write(repo.join("pom.xml"), r#"<project><build><plugins><plugin><groupId>com.google.cloud.tools</groupId><artifactId>jib-maven-plugin</artifactId><configuration><from><image>base-only</image></from><to><image>ghcr.io/acme/maven-orders:v1</image></to></configuration></plugin></plugins></build></project>"#).unwrap();
    std::fs::write(repo.join("build.gradle.kts"), "plugins { id(\"com.google.cloud.tools.jib\") }\njib { to { image = \"ghcr.io/acme/gradle-orders\" } }\n").unwrap();
    let engine = Engine::open(&repo).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let mut names = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [
            "image::ghcr.io/*/unknown",
            "image::ghcr.io/acme/dynamic-tag",
            "image::ghcr.io/acme/gradle-orders",
            "image::ghcr.io/acme/ko-orders",
            "image::ghcr.io/acme/maven-orders",
            "image::ghcr.io/acme/meta-orders",
            "image::ghcr.io/acme/skaffold-orders",
            "image::ghcr.io/acme/skaffold-worker"
        ]
    );
    assert!(
        export["providers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["at"].as_str().is_some_and(|a| a.contains('#'))),
        "{export}"
    );
    assert_golden("images.builders.export.json", &engine.export().unwrap());
}

#[test]
fn image_patterns_link_only_to_literals_at_segment_boundaries() {
    let app = fixture_repo(
        "images_patterns",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let deploy = fixture_repo(
        "images_patterns",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(app.join("compose.yaml")).unwrap();
    std::fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/acme/${IMAGE}:v1\n").unwrap();
    let engine = Engine::open(&deploy).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{export}");
    assert!(links.iter().all(|l| l["level"] == "inferred"), "{export}");
    assert!(
        links
            .iter()
            .all(|l| l["reason"].as_str().unwrap().contains("pattern")),
        "{export}"
    );
    std::fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/acme/ord${IMAGE}:v1\n").unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(export["links"], serde_json::json!([]), "{export}");
}

#[test]
fn unresolved_registry_segments_match_literal_images_in_both_directions() {
    let app = fixture_repo(
        "images_registry_pattern",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let deploy = fixture_repo(
        "images_registry_pattern",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(app.join("compose.yaml")).unwrap();
    std::fs::remove_file(deploy.join("values.yaml")).unwrap();
    std::fs::remove_file(deploy.join("overlays/prod/kustomization.yaml")).unwrap();
    let workflow = app.join(".github/workflows/build.yml");
    let original = std::fs::read_to_string(&workflow).unwrap();
    std::fs::write(&workflow, original.replace("ghcr.io", "${REGISTRY}")).unwrap();
    let engine = Engine::open(&deploy).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{export}");
    assert_eq!(links[0]["level"], "inferred");
    assert_eq!(links[0]["provider"], "example.com/acme/app");
    assert!(
        links[0]["reason"]
            .as_str()
            .unwrap()
            .contains("image::*/acme/orders")
    );

    std::fs::write(&workflow, original).unwrap();
    let manifest = deploy.join("deployment.yaml");
    let source = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, source.replace("ghcr.io", "${REGISTRY}")).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{export}");
    assert_eq!(links[0]["contract"], "image::*/acme/orders");
    assert_eq!(links[0]["level"], "inferred");
    assert_eq!(links[0]["provider"], "example.com/acme/app");
}

#[test]
fn ko_script_text_is_not_a_builder_and_plain_commands_refresh() {
    let app = fixture_repo(
        "images_ko_shell",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let deploy = fixture_repo(
        "images_ko_shell",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(app.join("compose.yaml")).unwrap();
    std::fs::remove_file(deploy.join("values.yaml")).unwrap();
    std::fs::remove_file(deploy.join("overlays/prod/kustomization.yaml")).unwrap();
    let workflow = app.join(".github/workflows/build.yml");
    let engine = Engine::open(&deploy).unwrap();
    for (command, shell, builds) in [
        (
            "cat <<'EOF'\nko build --bare ./cmd/orders\nEOF\n",
            None,
            false,
        ),
        ("ko build --bare ./cmd/orders", None, true),
        (
            "if false; then\nko publish --bare ./cmd/orders\nfi\n",
            None,
            false,
        ),
        ("ko publish --bare ./cmd/orders", None, true),
        ("ko build --bare ./cmd/orders; echo done", None, false),
        ("ko build --bare --help ./cmd/orders", None, false),
        ("ko publish --bare -h ./cmd/orders", None, false),
        ("ko build --bare --unknown ./cmd/orders", None, false),
        ("ko build --bare ./cmd/orders", Some("python"), false),
        ("ko build --bare ./cmd/orders", Some("bash"), true),
    ] {
        let mut step = serde_json::json!({
            "run": command,
            "env": {"KO_DOCKER_REPO": "ghcr.io/acme/orders"}
        });
        if let Some(shell) = shell {
            step["shell"] = shell.into();
        }
        let doc = serde_json::json!({"jobs": {"publish": {"steps": [step]}}});
        std::fs::write(&workflow, doc.to_string()).unwrap();
        let built: serde_json::Value =
            serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
        let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
        assert_eq!(
            built["providers"].as_array().unwrap().len(),
            usize::from(builds),
            "{command}: {built}"
        );
        assert_eq!(
            export["links"].as_array().unwrap().len(),
            usize::from(builds),
            "{command}: {export}"
        );
        if builds {
            assert_eq!(
                built["providers"][0]["contract"],
                "image::ghcr.io/acme/orders"
            );
            assert_eq!(export["links"][0]["level"], "exact");
        }
    }
}

#[test]
fn several_image_names_from_one_builder_remain_ambiguous_open_items() {
    let app = fixture_repo(
        "images_ambiguity",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let deploy = fixture_repo(
        "images_ambiguity",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(app.join("compose.yaml")).unwrap();
    std::fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/acme/orders-east:v1,ghcr.io/acme/orders-west:v2\n").unwrap();
    std::fs::remove_file(deploy.join("values.yaml")).unwrap();
    std::fs::remove_file(deploy.join("overlays/prod/kustomization.yaml")).unwrap();
    std::fs::write(deploy.join("deployment.yaml"), "apiVersion: v1\nkind: Pod\nmetadata:\n  name: orders\nspec:\n  containers:\n    - name: orders\n      image: ghcr.io/acme/orders-${REGION}:v1\n").unwrap();
    let engine = Engine::open(&deploy).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{export}");
    assert!(links.iter().all(|l| l["level"] == "ambiguous"), "{export}");
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .candidates
            .iter()
            .map(|c| c.contract.as_str())
            .collect::<Vec<_>>(),
        [
            "image::ghcr.io/acme/orders-east",
            "image::ghcr.io/acme/orders-west"
        ]
    );
}

#[cfg(unix)]
#[test]
fn direct_code_paths_need_no_image_and_never_follow_symlinks_on_refresh() {
    use meada::engine::{Direction, Start};
    let repo = fixture_repo(
        "images_paths",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    std::fs::remove_file(repo.join(".github/workflows/build.yml")).unwrap();
    std::fs::create_dir(repo.join("code")).unwrap();
    std::fs::write(repo.join("code/api.py"), "def handle():\n    return 1\n").unwrap();
    std::fs::write(
        repo.join("compose.yaml"),
        "services:\n  api:\n    build: ./code\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(export["providers"], serde_json::json!([]));
    assert_eq!(export["consumers"], serde_json::json!([]));
    assert_eq!(
        export["executions"].as_array().unwrap().len(),
        1,
        "{export}"
    );
    assert_eq!(export["executions"][0]["path"], "code");
    assert_eq!(export["executions"][0]["level"], "exact");
    assert_eq!(export["iac_nodes"].as_array().unwrap().len(), 1, "{export}");
    let trace = engine
        .trace(Start::From("services.api"), Direction::Down, None)
        .unwrap();
    assert!(trace.contains("code/api.py"), "{trace}");
    assert!(!trace.contains("orders.py"), "{trace}");
    std::fs::rename(repo.join("code"), repo.join("real")).unwrap();
    std::os::unix::fs::symlink("real", repo.join("code")).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(export.get("executions").is_none(), "{export}");
    std::fs::remove_file(repo.join("code")).unwrap();
    std::fs::rename(repo.join("real"), repo.join("code")).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        export["executions"].as_array().unwrap().len(),
        1,
        "{export}"
    );
}

/// A module run from a local archive in one root and from an image in another: the archive runs
/// code without an image in its root only, so the other root keeps its image runner.
#[test]
fn a_local_archive_in_one_root_leaves_the_image_another_root_runs() {
    let repo = fixture_repo(
        "images_archive_root",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    std::fs::remove_file(repo.join("compose.yaml")).unwrap();
    std::fs::create_dir_all(repo.join("modules/fn")).unwrap();
    std::fs::write(
        repo.join("modules/fn/main.tf"),
        "variable \"filename\" {\n  default = null\n}\nvariable \"image_uri\" {\n  default = null\n}\n\
         resource \"aws_lambda_function\" \"this\" {\n  function_name = \"orders\"\n  \
         filename = var.filename\n  image_uri = var.image_uri\n  handler = \"index.handler\"\n}\n",
    )
    .unwrap();
    for (env, input) in [
        ("dev", "filename = \"lambda.zip\""),
        ("prod", "image_uri = \"ghcr.io/acme/orders:v1\""),
    ] {
        std::fs::create_dir_all(repo.join("envs").join(env)).unwrap();
        std::fs::write(
            repo.join("envs").join(env).join("main.tf"),
            format!("module \"fn\" {{\n  source = \"../../modules/fn\"\n  {input}\n}}\n"),
        )
        .unwrap();
    }
    std::fs::write(repo.join("envs/dev/lambda.zip"), "an opaque package").unwrap();
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let links: Vec<_> = (export["links"].as_array().unwrap().iter())
        .map(|l| {
            (
                l["contract"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(links, [("image::ghcr.io/acme/orders", "exact")], "{export}");
    assert_eq!(export["executions"][0]["root"], "envs/dev", "{export}");
}

#[test]
fn review_helm_runner_is_a_context_and_trace_start() {
    use meada::engine::{Direction, Start};
    fixture_repo(
        "images_helm_start",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "images_helm_start",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let start = "example.com/acme/deploy::values.yaml::image.repository";
    let context = engine.context(start, None).unwrap();
    assert!(context.contains("image::ghcr.io/acme/orders"), "{context}");
    assert!(
        context.contains("builder: example.com/acme/app"),
        "{context}"
    );
    let trace = engine
        .trace(Start::From(start), Direction::Down, None)
        .unwrap();
    assert!(trace.contains("orders.py"), "{trace}");
}

#[test]
fn review_overlays_keep_their_own_image_in_context_and_execution() {
    use meada::engine::{Direction, Start};
    let app = fixture_repo(
        "images_overlay_roots",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "images_overlay_roots",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(app.join("compose.yaml")).unwrap();
    std::fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/acme/east,ghcr.io/acme/west\n").unwrap();
    std::fs::remove_dir_all(repo.join("overlays/prod")).unwrap();
    for region in ["east", "west"] {
        std::fs::create_dir_all(repo.join(format!("overlays/{region}"))).unwrap();
        std::fs::write(repo.join(format!("overlays/{region}/kustomization.yaml")), format!("resources:\n  - ../../deployment.yaml\nimages:\n  - name: ghcr.io/acme/orders\n    newName: ghcr.io/acme/{region}\n")).unwrap();
    }
    let engine = Engine::open(&repo).unwrap();
    let start = "Deployment.apps/shop/orders (root overlays/east)";
    let context = engine.context(start, None).unwrap();
    assert!(
        context.contains("image: image::ghcr.io/acme/east"),
        "{context}"
    );
    assert!(!context.contains("image::ghcr.io/acme/west"), "{context}");
    let trace = engine
        .trace(Start::From(start), Direction::Down, None)
        .unwrap();
    assert!(trace.contains("image::ghcr.io/acme/east"), "{trace}");
    assert!(!trace.contains("image::ghcr.io/acme/west"), "{trace}");
}

#[test]
fn overlays_of_one_workload_keep_both_runners_of_the_same_image() {
    let repo = fixture_repo(
        "images_identical_overlay_roots",
        "image-evaluation-roots",
        Some("https://independent.invalid/azure/roots.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let raw = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&raw).unwrap();
    // These complete rows come from the two overlay roots in the fixture and ADR 0026.
    assert_eq!(
        export["providers"],
        serde_json::json!([{
            "contract": "image::ghcr.io/independent/root-review",
            "site": "independent.invalid/azure/roots",
            "at": ".github/workflows/image.yml#jobs.build.steps[0].with.tags"
        }]),
        "{raw}"
    );
    assert_eq!(
        export["consumers"],
        serde_json::json!([
            {
                "contract": "image::ghcr.io/independent/root-review",
                "site": "independent.invalid/azure/roots::base/pod.yaml::Pod/common",
                "via": null,
                "root": "overlays/left"
            },
            {
                "contract": "image::ghcr.io/independent/root-review",
                "site": "independent.invalid/azure/roots::base/pod.yaml::Pod/common",
                "via": null,
                "root": "overlays/right"
            }
        ]),
        "{raw}"
    );
    assert_eq!(
        export["links"],
        serde_json::json!([
            {
                "consumer": "independent.invalid/azure/roots::base/pod.yaml::Pod/common",
                "provider": "independent.invalid/azure/roots",
                "contract": "image::ghcr.io/independent/root-review",
                "level": "exact",
                "reason": "image::ghcr.io/independent/root-review",
                "root": "overlays/left"
            },
            {
                "consumer": "independent.invalid/azure/roots::base/pod.yaml::Pod/common",
                "provider": "independent.invalid/azure/roots",
                "contract": "image::ghcr.io/independent/root-review",
                "level": "exact",
                "reason": "image::ghcr.io/independent/root-review",
                "root": "overlays/right"
            }
        ]),
        "{raw}"
    );
    assert_golden("images.identical-overlay-roots.export.json", &raw);

    // Repeated evidence in one root still describes one runner of the normalized image.
    std::fs::remove_dir_all(repo.join("overlays/right")).unwrap();
    let pod = repo.join("base/pod.yaml");
    let mut source = std::fs::read_to_string(&pod).unwrap();
    source.push_str("    - name: second\n      image: ghcr.io/independent/root-review:canary\n");
    std::fs::write(pod, source).unwrap();
    let raw = engine.export().unwrap();
    let control: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(control["providers"], export["providers"], "{raw}");
    assert_eq!(
        control["consumers"],
        serde_json::json!([{
            "contract": "image::ghcr.io/independent/root-review",
            "site": "independent.invalid/azure/roots::base/pod.yaml::Pod/common",
            "via": null,
            "root": "overlays/left"
        }]),
        "{raw}"
    );
    assert_eq!(
        control["links"],
        serde_json::json!([{
            "consumer": "independent.invalid/azure/roots::base/pod.yaml::Pod/common",
            "provider": "independent.invalid/azure/roots",
            "contract": "image::ghcr.io/independent/root-review",
            "level": "exact",
            "reason": "image::ghcr.io/independent/root-review",
            "root": "overlays/left"
        }]),
        "{raw}"
    );
    assert_golden("images.same-root-evidence.export.json", &raw);
}

#[test]
fn review_remote_compose_build_publishes_without_a_direct_code_path() {
    let repo = fixture_repo(
        "images_remote_build",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    std::fs::remove_file(repo.join(".github/workflows/build.yml")).unwrap();
    std::fs::write(repo.join("compose.yaml"), "services:\n  api:\n    build: https://github.com/acme/source.git\n    image: ghcr.io/acme/remote\n").unwrap();
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 1, "{export}");
    assert_eq!(
        export["providers"][0]["contract"],
        "image::ghcr.io/acme/remote"
    );
    assert!(export.get("executions").is_none(), "{export}");
}

#[test]
fn every_marked_kubernetes_workload_type_runs_its_container_images() {
    let repo = fixture_repo(
        "images_workload_types",
        "image-workloads",
        Some("https://example.com/acme/workloads.git"),
    );
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let consumers = export["consumers"].as_array().unwrap();
    assert_eq!(consumers.len(), 8, "{export}");
    assert!(
        consumers
            .iter()
            .all(|c| c["contract"] == "image::docker.io/library/nginx"),
        "{export}"
    );
    let mut types = export["iac_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["type"].as_str().unwrap())
        .collect::<Vec<_>>();
    types.sort();
    assert_eq!(
        types,
        [
            "CronJob",
            "DaemonSet",
            "Deployment",
            "Job",
            "Pod",
            "ReplicaSet",
            "ReplicationController",
            "StatefulSet"
        ]
    );
}

#[test]
fn two_image_ambiguities_can_be_decided_independently() {
    let app = fixture_repo(
        "images_independent_decisions",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "images_independent_decisions",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(app.join("compose.yaml")).unwrap();
    std::fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/acme/app-east,ghcr.io/acme/app-west,ghcr.io/acme/init-east,ghcr.io/acme/init-west\n").unwrap();
    std::fs::remove_file(repo.join("values.yaml")).unwrap();
    std::fs::remove_file(repo.join("overlays/prod/kustomization.yaml")).unwrap();
    std::fs::write(repo.join("deployment.yaml"), "apiVersion: v1\nkind: Pod\nmetadata:\n  name: orders\nspec:\n  containers:\n    - name: app\n      image: ghcr.io/acme/app-${REGION}\n  initContainers:\n    - name: init\n      image: ghcr.io/acme/init-${REGION}\n").unwrap();
    let engine = Engine::open(&repo).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 2);
    assert_ne!(items[0].id, items[1].id);
    assert!(items.iter().all(|i| i.candidates.len() == 2));
    let selected = items
        .iter()
        .find(|i| i.candidates[0].contract.contains("/app-"))
        .unwrap();
    let decisions = format!(
        "[[decision]]\nkind = \"ambiguous-link\"\ncall = {}\ntarget = \"example.com/acme/app::image::ghcr.io/acme/app-east\"\nby = \"human\"\n",
        serde_json::to_string(&selected.id).unwrap()
    );
    std::fs::write(repo.join("meada.decisions.toml"), decisions).unwrap();
    let engine = Engine::open(&repo).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        items[0]
            .candidates
            .iter()
            .all(|c| c.contract.contains("/init-"))
    );
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(export["links"].as_array().unwrap().len(), 3, "{export}");
    assert_eq!(
        export["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["level"] == "decided")
            .count(),
        1,
        "{export}"
    );
}

#[test]
fn image_queries_match_full_reads_of_the_same_workspace() {
    use meada::engine::{Direction, Start, with_every_row};
    fixture_repo(
        "images_partial",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "images_partial",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    for symbol in [
        "Deployment.apps/shop/prod-orders",
        "image::ghcr.io/acme/orders",
        "values.yaml::image.repository",
        "orders",
    ] {
        let query = || engine.context(symbol, Some(100_000)).unwrap();
        assert_eq!(query(), with_every_row(query), "context {symbol}");
    }
    for (start, direction) in [
        ("Deployment.apps/shop/prod-orders", Direction::Down),
        ("values.yaml::image.repository", Direction::Down),
        ("orders", Direction::Up),
    ] {
        let query = || {
            engine
                .trace(Start::From(start), direction, Some(100_000))
                .unwrap()
        };
        assert_eq!(query(), with_every_row(query), "trace {start}");
    }
    let query = || {
        engine
            .search("orders", Some("image"), Some(100_000))
            .unwrap()
    };
    assert_eq!(query(), with_every_row(query));
}
