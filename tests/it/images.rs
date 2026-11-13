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
    assert_eq!(export["links"][0]["provider"], BUILD_STEP);
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
    // The repository builds the image in a CI step and, on weaker evidence, in its Compose
    // file: the workload links once, to the site at the best level.
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let links: Vec<_> = (export["links"].as_array().unwrap().iter())
        .map(|l| {
            (
                l["provider"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect();
    // The Deployment, then the Helm value, whose own evidence leaves its link `inferred`.
    assert_eq!(
        links,
        [(BUILD_STEP, "exact"), (BUILD_STEP, "inferred")],
        "{export}"
    );
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
    // Only `docker.io/library` runs: the builder of `ghcr.io/acme` keeps its mark.
    std::fs::write(
        repo.join("compose.yaml"),
        "services:\n  cache:\n    image: redis:7\n",
    )
    .unwrap();
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
        "services:\n  cache:\n    image: redis:7\n  worker:\n    image: ghcr.io/acme/worker:1\n",
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
    // Asked per registry and namespace: `ghcr.io/acme` builds and runs, `docker.io/library` runs.
    assert!(
        status.contains("orphan image::ghcr.io/acme/orders: no runner"),
        "{status}"
    );
    assert!(
        status.contains("orphan image::ghcr.io/acme/worker: no builder"),
        "{status}"
    );
    assert!(!status.contains("orphan image::docker.io/"), "{status}");
    std::fs::remove_file(repo.join(".github/workflows/build.yml")).unwrap();
    for image in [
        "image::docker.io/library/redis",
        "image::ghcr.io/acme/worker",
    ] {
        let context = engine.context(image, None).unwrap();
        assert!(context.contains("no builder in the workspace"), "{context}");
    }
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

/// The orphan scope of images is asked per registry and namespace (ADR 0026, "Orphan sides"),
/// so a public image no repository builds is no orphan because the workspace builds another.
#[test]
fn an_image_is_an_orphan_only_in_a_group_that_builds_and_runs() {
    let repo = fixture_repo(
        "images_orphan_groups",
        "orphan-image",
        Some("https://example.com/acme/o4.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let orphans = || -> Vec<String> {
        (engine.status_report().unwrap().1.into_iter().flatten())
            .filter(|l| l.starts_with("orphan "))
            .collect()
    };
    assert_eq!(orphans(), Vec::<String>::new());
    let postgres = engine
        .context("image::docker.io/library/postgres", None)
        .unwrap();
    assert!(
        postgres.contains("(no builder in the workspace)"),
        "{postgres}"
    );
    let compose = std::fs::read_to_string(repo.join("docker-compose.yaml")).unwrap();
    std::fs::write(
        repo.join("docker-compose.yaml"),
        format!("{compose}  worker:\n    image: ghcr.io/acme/worker\n"),
    )
    .unwrap();
    assert_eq!(
        orphans(),
        ["orphan image::ghcr.io/acme/worker: no builder; docker-compose.yaml::services.worker"]
    );
}

/// A builder whose name is a pattern (`ghcr.io/${{ github.repository }}` reads `ghcr.io/*`) is in
/// the groups of the names it can take, so a runner of such a name carries no `no builder` mark.
#[test]
fn a_pattern_builder_is_in_the_groups_of_the_images_it_can_name() {
    let repo = fixture_repo(
        "images_pattern_groups",
        "orphan-image",
        Some("https://example.com/acme/o4.git"),
    );
    std::fs::write(
        repo.join("docker-compose.yaml"),
        "services:\n  app:\n    image: ghcr.io/acme/shop\n  cache:\n    image: redis:7.2\n",
    )
    .unwrap();
    std::fs::create_dir_all(repo.join(".github/workflows")).unwrap();
    std::fs::write(
        repo.join(".github/workflows/build.yml"),
        "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          \
         tags: ghcr.io/${{ github.repository }}:latest\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let shop = engine.context("image::ghcr.io/acme/shop", None).unwrap();
    assert!(
        shop.contains("runner docker-compose.yaml:2 services.app")
            && !shop.contains("in the workspace"),
        "{shop}"
    );
    let search = engine.search("ghcr.io", Some("image"), None).unwrap();
    assert!(
        search.contains("image::ghcr.io/*") && !search.contains("in the workspace"),
        "{search}"
    );
    let redis = engine
        .context("image::docker.io/library/redis", None)
        .unwrap();
    assert!(redis.contains("(no builder in the workspace)"), "{redis}");
}

/// A pattern that holds a namespace (`registry.acme.io/team/*`) names no image of one segment, so
/// it is not in the registry's own group: a mirrored `registry.acme.io/nginx` beside it keeps its
/// mark, and neither is an orphan line.
#[test]
fn a_pattern_with_a_namespace_is_in_no_group_of_one_segment_names() {
    let repo = fixture_repo(
        "images_pattern_one_segment",
        "orphan-image",
        Some("https://example.com/acme/o4.git"),
    );
    std::fs::write(
        repo.join("docker-compose.yaml"),
        "services:\n  proxy:\n    image: registry.acme.io/nginx:1.27\n",
    )
    .unwrap();
    std::fs::create_dir_all(repo.join(".github/workflows")).unwrap();
    std::fs::write(
        repo.join(".github/workflows/build.yml"),
        "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          \
         tags: registry.acme.io/team/${{ github.event.repository.name }}:latest\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let orphans = (engine.status_report().unwrap().1.into_iter().flatten())
        .filter(|l| l.starts_with("orphan "))
        .collect::<Vec<_>>();
    assert_eq!(orphans, Vec::<String>::new());
    let nginx = engine
        .context("image::registry.acme.io/nginx", None)
        .unwrap();
    assert!(nginx.contains("(no builder in the workspace)"), "{nginx}");
    let team = engine
        .search("registry.acme.io/team", Some("image"), None)
        .unwrap();
    assert!(team.contains("(no runner in the workspace)"), "{team}");
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
    assert_eq!(links[0]["provider"], BUILD_STEP);
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
    assert_eq!(links[0]["provider"], BUILD_STEP);
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
        // The gate of the command table reads each command (#833): one inside `if`, one before
        // `;` and one on a line of several count, and text in `echo` or a string does not.
        (
            "if false; then\nko publish --bare ./cmd/orders\nfi\n",
            None,
            true,
        ),
        ("ko publish --bare ./cmd/orders", None, true),
        ("ko build --bare ./cmd/orders; echo done", None, true),
        ("set -e\nko build --bare ./cmd/orders\n", None, true),
        ("echo ko build --bare ./cmd/orders", None, false),
        ("NOTE='ko build --bare ./cmd/orders'", None, false),
        ("ko build --bare ./cmd/$APP", None, false),
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

/// The builder sides of an export as (contract, CI step, `at` inside the pipeline file).
fn built(export: &serde_json::Value, file: &str) -> Vec<(String, String, String)> {
    let text = |p: &serde_json::Value, key: &str| p[key].as_str().unwrap().to_string();
    (export["providers"].as_array().unwrap().iter())
        .map(|p| {
            let site = text(p, "site");
            let site = site.strip_prefix(&format!("example.com/acme/app::{file}::"));
            let at = text(p, "at");
            let at = at.strip_prefix(&format!("{file}#")).unwrap().to_string();
            (text(p, "contract"), site.unwrap().to_string(), at)
        })
        .collect()
}

#[test]
fn a_run_script_that_pushes_an_image_makes_its_ci_step_the_builder() {
    let file = ".github/workflows/publish.yml";
    let app = repo(
        "images_run_push",
        &[(
            file,
            r#"name: publish
on: push
env:
  IMAGE: ghcr.io/acme/orders
jobs:
  push:
    runs-on: ubuntu-latest
    steps:
      - run: |
          docker push ghcr.io/acme/pushed:v1
          docker push ghcr.io/acme/pushed:v2
      - run: |
          docker build -t "$IMAGE:${{ github.sha }}" .
          docker push "$IMAGE:${{ github.sha }}"
      - run: >-
          docker buildx build --push -t ghcr.io/acme/built:v1 --tag=ghcr.io/acme/built-too
          --build-arg BASE=ghcr.io/acme/base --label source=$UNKNOWN .
      - run: docker push ghcr.io/${{ github.repository_owner }}/part:latest
      - run: docker push ${{ inputs.image }}
  silent:
    runs-on: ubuntu-latest
    steps:
      - run: docker build -t ghcr.io/acme/silent-build .
      - run: docker buildx build -t ghcr.io/acme/silent-buildx .
      - run: echo docker push ghcr.io/acme/silent-echo
      - run: |
          MESSAGE="docker push ghcr.io/acme/silent-string"
          # docker push ghcr.io/acme/silent-comment
      - run: |
          docker push ghcr.io/acme/silent-heredoc
          cat <<EOF
          docker push ghcr.io/acme/silent-heredoc-text
          EOF
      - run: docker push ghcr.io/acme/silent-shell
        shell: pwsh
      - run: docker push --unknown ghcr.io/acme/silent-option
  boxed:
    runs-on: ubuntu-latest
    container: ghcr.io/acme/job-container:v1
    services:
      db:
        image: ghcr.io/acme/job-service:v1
    env:
      IMAGE: ghcr.io/acme/job-env:v1
    steps:
      - run: echo boxed
"#,
        )],
    );
    // A CI job is no workload: its container, its services and its `env` run no image.
    let read = export(&app);
    let runners = read["consumers"].as_array();
    assert!(runners.is_none_or(|sides| sides.is_empty()), "{read}");
    let side = |contract: &str, step: usize| {
        let step = format!("jobs.push.steps[{step}]");
        (
            format!("image::{contract}"),
            step.clone(),
            format!("{step}.run"),
        )
    };
    assert_eq!(
        built(&export(&app), file),
        [
            side("*", 4),
            side("ghcr.io/*/part", 3),
            side("ghcr.io/acme/built", 2),
            side("ghcr.io/acme/built-too", 2),
            side("ghcr.io/acme/orders", 1),
            side("ghcr.io/acme/pushed", 0),
        ]
    );
}

#[test]
fn a_workload_links_to_each_ci_step_of_the_repository_that_builds_its_image() {
    use meada::engine::{Direction, Start};
    let file = ".github/workflows/publish.yml";
    let app = repo(
        "images_run_link",
        &[
            (
                file,
                "name: publish\non: push\njobs:\n  push:\n    runs-on: ubuntu-latest\n    steps:\n      \
                 - uses: docker/metadata-action@v5\n        with:\n          images: ghcr.io/acme/orders\n      \
                 - run: docker push ghcr.io/acme/orders:v1\n",
            ),
            ("orders.py", "def orders():\n    pass\n"),
        ],
    );
    let deploy = fixture_repo(
        "images_run_link",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(deploy.join("values.yaml")).unwrap();
    std::fs::remove_file(deploy.join("overlays/prod/kustomization.yaml")).unwrap();
    let engine = Engine::open(&deploy).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    // Two CI steps of one repository are one builder repository: both links stay `exact`.
    let step = |n: usize| format!("example.com/acme/app::{file}::jobs.push.steps[{n}]");
    let links: Vec<_> = (export["links"].as_array().unwrap().iter())
        .map(|l| {
            (
                l["provider"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        links,
        [(&*step(0), "exact"), (&*step(1), "exact")],
        "{export}"
    );
    assert_eq!(
        built(&self::export(&app), file),
        [
            (
                "image::ghcr.io/acme/orders".into(),
                "jobs.push.steps[0]".into(),
                "jobs.push.steps[0].with.images".into()
            ),
            (
                "image::ghcr.io/acme/orders".into(),
                "jobs.push.steps[1]".into(),
                "jobs.push.steps[1].run".into()
            ),
        ] as [(String, String, String); 2]
    );
    let context = engine.context("Deployment.apps/shop/orders", None).unwrap();
    for n in 0..2 {
        let line = format!("  builder: {} [exact: image::ghcr.io/acme/orders]", step(n));
        assert!(context.contains(&line), "{context}");
    }
    // The workload runs the code of the builder's repository, once.
    let down = engine
        .trace(
            Start::From("Deployment.apps/shop/orders"),
            Direction::Down,
            None,
        )
        .unwrap();
    let runs = "runs code example.com/acme/app . [exact: image::ghcr.io/acme/orders]";
    assert_eq!(down.matches(runs).count(), 1, "{down}");
    assert!(down.contains("orders.py"), "{down}");
    // Upward from the CI step, the workload that runs what it builds.
    let up = engine
        .trace(Start::From("jobs.push.steps[1]"), Direction::Up, None)
        .unwrap();
    assert!(up.contains("Deployment.apps/shop/orders"), "{up}");
    // On the CI step, the image it builds and the workload that runs it.
    let context = engine.context(&step(1), None).unwrap();
    let side = "builds image::ghcr.io/acme/orders\n  runner: \
                example.com/acme/deploy::deployment.yaml::Deployment.apps/shop/orders (root .) \
                [exact: image::ghcr.io/acme/orders]\n";
    assert!(context.contains(side), "{context}");
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
    std::fs::write(repo.join("code/Dockerfile"), "FROM scratch\nCOPY . /app\n").unwrap();
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
            "site": "independent.invalid/azure/roots::.github/workflows/image.yml::jobs.build.steps[0]",
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
                "provider": "independent.invalid/azure/roots::.github/workflows/image.yml::jobs.build.steps[0]",
                "contract": "image::ghcr.io/independent/root-review",
                "level": "exact",
                "reason": "image::ghcr.io/independent/root-review",
                "root": "overlays/left"
            },
            {
                "consumer": "independent.invalid/azure/roots::base/pod.yaml::Pod/common",
                "provider": "independent.invalid/azure/roots::.github/workflows/image.yml::jobs.build.steps[0]",
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
            "provider": "independent.invalid/azure/roots::.github/workflows/image.yml::jobs.build.steps[0]",
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

/// A build context a deployer can override runs code `inferred`; a literal and a pattern one
/// builder repository publishes are one candidate; a Helm `image.registry` prefixes its
/// repository; and an unresolved image name is no orphan.
#[test]
fn image_levels_and_names_follow_defaults_registries_and_unresolved_names() {
    let app = fixture_repo(
        "images_levels",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    let deploy = fixture_repo(
        "images_levels",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    std::fs::remove_file(deploy.join("overlays/prod/kustomization.yaml")).unwrap();
    std::fs::write(
        deploy.join("values.yaml"),
        "image:\n  registry: ghcr.io\n  repository: acme/orders\n  tag: v3\n",
    )
    .unwrap();
    std::fs::create_dir(app.join("code")).unwrap();
    std::fs::write(app.join("code/api.py"), "def handle():\n    return 1\n").unwrap();
    std::fs::write(
        app.join("compose.yaml"),
        "services:\n  api:\n    build: ${CTX:-./code}\n",
    )
    .unwrap();
    std::fs::write(
        app.join(".github/workflows/build.yml"),
        "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/acme/orders:v1\n  \
         pattern:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/acme/${IMAGE}:v1\n  \
         meta:\n    steps:\n      - id: meta\n        uses: docker/metadata-action@v5\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ${{ steps.meta.outputs.tags }}\n",
    )
    .unwrap();
    let built: serde_json::Value =
        serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    let executions = built["executions"].as_array().unwrap();
    assert_eq!(executions.len(), 1, "{built}");
    assert_eq!(executions[0]["path"], "code");
    assert_eq!(executions[0]["level"], "inferred", "{built}");

    let engine = Engine::open(&deploy).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let consumers = export["consumers"].as_array().unwrap();
    assert!(
        consumers.iter().any(|c| c["site"]
            .as_str()
            .is_some_and(|s| s.contains("values.yaml"))
            && c["contract"] == "image::ghcr.io/acme/orders"),
        "{export}"
    );
    assert!(
        !consumers
            .iter()
            .any(|c| c["contract"] == "image::docker.io/acme/orders"),
        "{export}"
    );
    // A registry with a trailing slash, one a template sets, and a repository that already
    // names it.
    for (values, expected) in [
        (
            "registry: ghcr.io/\n  repository: acme/orders",
            "image::ghcr.io/acme/orders",
        ),
        (
            "registry: \"{{ .Values.r }}\"\n  repository: acme/orders",
            "image::*/acme/orders",
        ),
        (
            "registry: ghcr.io\n  repository: ghcr.io/acme/orders",
            "image::ghcr.io/acme/orders",
        ),
    ] {
        std::fs::write(deploy.join("values.yaml"), format!("image:\n  {values}\n")).unwrap();
        let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
        let helm = (export["consumers"].as_array().unwrap().iter())
            .find(|c| {
                c["site"]
                    .as_str()
                    .is_some_and(|s| s.contains("values.yaml"))
            })
            .map(|c| c["contract"].clone());
        assert_eq!(helm, Some(expected.into()), "{values}: {export}");
    }
    std::fs::write(
        deploy.join("values.yaml"),
        "image:\n  registry: ghcr.io\n  repository: acme/orders\n  tag: v3\n",
    )
    .unwrap();
    // One link per runner, the Helm one inferred from its value.
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{export}");
    for link in links {
        assert_eq!(link["provider"], BUILD_STEP, "{export}");
        let helm = link["consumer"].as_str().unwrap().contains("values.yaml");
        assert_eq!(
            link["level"],
            if helm { "inferred" } else { "exact" },
            "{export}"
        );
    }
    let status = Engine::open(&app)
        .unwrap()
        .status_report()
        .unwrap()
        .1
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!status.contains("orphan image::*"), "{status}");
    assert!(!status.contains("unresolved"), "{status}");
}

#[test]
fn a_helm_values_file_is_a_yaml_file_named_with_the_word_values() {
    let deploy = fixture_repo(
        "images_values_names",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    for (name, repository) in [
        ("prod-values.yaml", "acme/billing"),
        ("values.prod.yml", "acme/ledger"),
        ("valuesets.yaml", "acme/stock"),
    ] {
        std::fs::write(
            deploy.join(name),
            format!("image:\n  repository: {repository}\n"),
        )
        .unwrap();
    }
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&deploy).unwrap().export().unwrap()).unwrap();
    let helm: Vec<&str> = (export["consumers"].as_array().unwrap().iter())
        .filter(|c| c["site"].as_str().unwrap().ends_with("::image.repository"))
        .filter_map(|c| c["contract"].as_str())
        .collect();
    assert!(helm.contains(&"image::docker.io/acme/billing"), "{export}");
    assert!(helm.contains(&"image::docker.io/acme/ledger"), "{export}");
    assert!(!helm.contains(&"image::docker.io/acme/stock"), "{export}");
}

// A Compose build runs what its Dockerfile takes from the context (#744, ADR 0026).

const APP: Option<&str> = Some("https://example.com/acme/app.git");
/// The CI step of the fixture pipeline that builds the image: the builder's site (#833).
const BUILD_STEP: &str = "example.com/acme/app::.github/workflows/build.yml::jobs.image.steps[0]";

/// A repository of `image-app` with its Compose file, Dockerfile, workflow and code removed,
/// holding `files` (path, text) instead.
fn repo(case: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let repo = fixture_repo(case, "image-app", APP);
    for gone in ["compose.yaml", "Dockerfile", "orders.py", "meada.toml"] {
        std::fs::remove_file(repo.join(gone)).unwrap();
    }
    std::fs::remove_dir_all(repo.join(".github")).unwrap();
    for (path, text) in files {
        write(&repo, path, text);
    }
    repo
}

fn write(repo: &std::path::Path, path: &str, text: &str) {
    std::fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
    std::fs::write(repo.join(path), text).unwrap();
}

fn export(repo: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&Engine::open(repo).unwrap().export().unwrap()).unwrap()
}

/// The executions of an export as (workload address, path, level), in the export's order.
fn runs(export: &serde_json::Value) -> Vec<(String, String, String)> {
    let text = |x: &serde_json::Value, key: &str| x[key].as_str().unwrap().to_string();
    (export["executions"].as_array().into_iter().flatten())
        .map(|x| {
            let workload = text(x, "workload");
            let address = workload.rsplit("::").next().unwrap().to_string();
            (address, text(x, "path"), text(x, "level"))
        })
        .collect()
}

fn run(workload: &str, path: &str, level: &str) -> (String, String, String) {
    (workload.into(), path.into(), level.into())
}

/// The reasons of the executions of `workload`, each once.
fn reasons(export: &serde_json::Value, workload: &str) -> Vec<String> {
    let mut reasons: Vec<String> = (export["executions"].as_array().into_iter().flatten())
        .filter(|x| {
            x["workload"]
                .as_str()
                .unwrap()
                .ends_with(&format!("::{workload}"))
        })
        .map(|x| x["reason"].as_str().unwrap().to_string())
        .collect();
    reasons.dedup();
    reasons
}

#[test]
fn a_root_context_runs_only_the_paths_its_dockerfile_takes_in_code_and_environment() {
    use meada::engine::{Direction, Start};
    let repo = fixture_repo("images_dockerfile_root", "image-dockerfile", APP);
    let engine = Engine::open(&repo).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        runs(&parsed),
        [
            run("services.playwright", "bun.lock", "exact"),
            run("services.playwright", "frontend", "exact"),
            run("services.playwright", "frontend/package.json", "exact"),
            run("services.playwright", "package.json", "exact"),
        ]
    );
    assert_eq!(
        reasons(&parsed, "services.playwright"),
        [
            "Compose build compose.yaml#services.playwright.build; Dockerfile frontend/Dockerfile.playwright"
        ]
    );
    // Backend code is run by no workload: no line for the service, and its environment read
    // joins no entry of the service.
    let up = engine
        .trace(Start::From("get_password_hash"), Direction::Up, None)
        .unwrap();
    assert!(up.contains("create_user"), "{up}");
    assert!(
        !up.contains("playwright") && !up.contains("runs code"),
        "{up}"
    );
    let queues: Vec<&serde_json::Value> = (parsed["providers"].as_array().unwrap().iter())
        .filter(|p| p["contract"].as_str().unwrap().starts_with("channel::"))
        .collect();
    assert_eq!(queues.len(), 1, "{parsed}");
    assert_eq!(queues[0]["contract"], "channel::sqs", "{parsed}");
    let down = engine
        .trace(Start::From("services.playwright"), Direction::Down, None)
        .unwrap();
    assert!(down.contains("frontend/page.ts"), "{down}");
    assert!(!down.contains("backend/"), "{down}");

    // The service takes the backend from the next build: its code, its entries and its line.
    write(
        &repo,
        "frontend/Dockerfile.playwright",
        "FROM node\nCOPY frontend ./frontend\nCOPY backend ./backend\n",
    );
    let parsed: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        runs(&parsed),
        [
            run("services.playwright", "backend", "exact"),
            run("services.playwright", "frontend", "exact"),
        ]
    );
    let queue = (parsed["providers"].as_array().unwrap().iter())
        .find(|p| p["site"].as_str().unwrap().contains("queue.py"))
        .unwrap();
    assert_eq!(queue["contract"], "channel::sqs::jobs", "{parsed}");
    let up = engine
        .trace(Start::From("get_password_hash"), Direction::Up, None)
        .unwrap();
    assert_eq!(up.matches("runs code").count(), 1, "{up}");
    assert!(up.contains("services.playwright"), "{up}");
}

#[test]
fn a_context_with_copy_dot_runs_the_whole_repository_at_exact() {
    let repo = fixture_repo("images_dockerfile_dot", "image-app", APP);
    assert_eq!(runs(&export(&repo)), [run("services.orders", ".", "exact")]);
}

#[test]
fn a_dockerfile_the_reader_cannot_read_leaves_the_whole_context_inferred() {
    let repo = repo(
        "images_dockerfile_unread",
        &[
            (
                "compose.yaml",
                "services:\n  missing:\n    build: ./missing\n  variable:\n    build: ./variable\n  \
                 linked:\n    build: ./linked\n  taking:\n    build: ./taking\n",
            ),
            ("missing/a.py", "def a(): pass\n"),
            ("variable/Dockerfile", "FROM x\nARG SRC\nCOPY ${SRC} /app\n"),
            ("variable/a.py", "def b(): pass\n"),
            ("real/Dockerfile", "FROM x\nCOPY a.py /app\n"),
            ("linked/a.py", "def c(): pass\n"),
            ("taking/Dockerfile", "FROM x\nRUN make\n"),
            ("taking/a.py", "def d(): pass\n"),
        ],
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink("../real/Dockerfile", repo.join("linked/Dockerfile")).unwrap();
    let parsed = export(&repo);
    #[cfg(unix)]
    let linked = [run("services.linked", "linked", "inferred")];
    #[cfg(not(unix))]
    let linked = [];
    assert_eq!(
        runs(&parsed),
        [
            linked.as_slice(),
            &[
                run("services.missing", "missing", "inferred"),
                run("services.variable", "variable", "inferred"),
            ]
        ]
        .concat()
    );
    for (workload, why) in [
        ("services.missing", "missing"),
        ("services.variable", "a source with a variable"),
    ] {
        assert_eq!(reasons(&parsed, workload).len(), 1);
        assert!(
            reasons(&parsed, workload)[0].ends_with(&format!("not read: {why}")),
            "{parsed}"
        );
    }
    #[cfg(unix)]
    assert!(reasons(&parsed, "services.linked")[0].ends_with("not read: a symbolic link"));
    // A Dockerfile that takes nothing runs no code directly: the image side stays.
    assert!(!runs(&parsed).iter().any(|(w, ..)| w == "services.taking"));
}

#[test]
fn every_stage_and_every_bind_mount_with_no_from_is_run() {
    let repo = repo(
        "images_dockerfile_stages",
        &[
            (
                "compose.yaml",
                "services:\n  staged:\n    build: ./staged\n  mounted:\n    build: ./mounted\n  \
                 whole:\n    build: ./whole\n  cached:\n    build: ./cached\n",
            ),
            (
                "staged/Dockerfile",
                "FROM node AS build\nCOPY web /src/web\nRUN npm run build\nFROM nginx\n\
                 COPY --from=build /src/web/dist /html\nCOPY --from=other secret /s\n",
            ),
            ("staged/web/a.ts", "export const a = 1;\n"),
            ("staged/secret/b.ts", "export const b = 1;\n"),
            (
                "mounted/Dockerfile",
                "FROM x\nRUN --mount=type=bind,source=src,target=/s --mount=type=bind,from=base,\
                 source=other,target=/o make\n",
            ),
            ("mounted/src/a.py", "def a(): pass\n"),
            ("mounted/other/b.py", "def b(): pass\n"),
            (
                "whole/Dockerfile",
                "FROM x\nRUN --mount=type=bind,target=/s make\n",
            ),
            ("whole/a.py", "def c(): pass\n"),
            (
                "cached/Dockerfile",
                "FROM x\nRUN --mount=type=cache,target=/c --mount=type=secret,id=s make\n",
            ),
            ("cached/a.py", "def d(): pass\n"),
        ],
    );
    assert_eq!(
        runs(&export(&repo)),
        [
            run("services.mounted", "mounted/src", "exact"),
            run("services.staged", "staged/web", "exact"),
            run("services.whole", "whole", "exact"),
        ]
    );
}

#[test]
fn a_continued_dockerfile_line_runs_the_sources_docker_reads() {
    // Docker keeps the blanks of a continued line and ends a line only at a lone backslash.
    let repo = repo(
        "images_dockerfile_continuation",
        &[
            (
                "compose.yaml",
                "services:\n  split:\n    build: {context: ., dockerfile: docker/Dockerfile}\n  \
                 named:\n    build: {context: ., dockerfile: docker/Named}\n  \
                 doubled:\n    build: {context: ., dockerfile: docker/Doubled}\n",
            ),
            (
                "docker/Dockerfile",
                "FROM x\nCOPY backend\\\n     frontend ./\n",
            ),
            (
                "docker/Named",
                "FROM x\nCOPY package.json\\\n     bun.lock ./\n",
            ),
            (
                "docker/Doubled",
                "FROM x\nRUN echo a\\\\\nCOPY frontend ./f\n",
            ),
            ("backend/a.py", "def a(): pass\n"),
            ("frontend/b.ts", "export const b = 1;\n"),
            ("package.json", "{}\n"),
            ("bun.lock", "\n"),
        ],
    );
    assert_eq!(
        runs(&export(&repo)),
        [
            run("services.doubled", "frontend", "exact"),
            run("services.named", "bun.lock", "exact"),
            run("services.named", "package.json", "exact"),
            run("services.split", "backend", "exact"),
            run("services.split", "frontend", "exact"),
        ]
    );
}

#[test]
fn a_dockerfile_form_docker_reads_another_way_runs_no_fewer_paths() {
    // Each form below made the build run fewer paths than the image takes, at `exact`.
    let repo = repo(
        "images_dockerfile_docker_forms",
        &[
            (
                "compose.yaml",
                "services:\n  parents:\n    build: {context: p, dockerfile: Parents}\n  \
                 stars:\n    build: {context: p, dockerfile: Stars}\n  \
                 quoted:\n    build: q\n  \
                 mount:\n    build: m\n",
            ),
            ("p/Parents", "FROM x\nCOPY --parents ./src/**/*.py /app/\n"),
            ("p/Stars", "FROM x\nCOPY src/**/*.py /app/\n"),
            ("p/src/a.py", "def a(): pass\n"),
            ("p/src/x/b.py", "def b(): pass\n"),
            ("p/src/x/y/c.py", "def c(): pass\n"),
            (
                "q/Dockerfile",
                "FROM x\nRUN echo 'use <<EOF here'\nCOPY backend /app\nRUN cat <<EOF >/x\nEOF\n",
            ),
            ("q/backend/a.py", "def a(): pass\n"),
            (
                "m/Dockerfile",
                "FROM x\nRUN --mount=type=BIND,source=backend,target=/b make\n",
            ),
            ("m/backend/a.py", "def a(): pass\n"),
        ],
    );
    assert_eq!(
        runs(&export(&repo)),
        [
            run("services.mount", "m/backend", "exact"),
            // `**` crosses directories under `--parents`: the whole context, as for a Dockerfile
            // not read.
            run("services.parents", "p", "inferred"),
            run("services.quoted", "q/backend", "exact"),
            // Without `--parents`, Docker's `filepath.Match` reads `**` as one segment.
            run("services.stars", "p/src/x/b.py", "exact"),
        ]
    );
}

#[test]
fn a_wildcard_source_runs_exactly_the_files_it_matches() {
    let repo = repo(
        "images_dockerfile_wildcard",
        &[
            ("compose.yaml", "services:\n  api:\n    build: .\n"),
            (
                "Dockerfile",
                "FROM x\nCOPY app/*.py /srv/\nCOPY lib/v?/[a-b]*.py /lib/\n",
            ),
            ("app/a.py", "def a(): pass\n"),
            ("app/sub/b.py", "def b(): pass\n"),
            ("app/c.txt", "text\n"),
            ("lib/v1/a1.py", "def c(): pass\n"),
            ("lib/v2/b2.py", "def d(): pass\n"),
            ("lib/v2/c3.py", "def e(): pass\n"),
            ("lib/v10/a4.py", "def f(): pass\n"),
        ],
    );
    assert_eq!(
        runs(&export(&repo)),
        [
            run("services.api", "app/a.py", "exact"),
            run("services.api", "lib/v1/a1.py", "exact"),
            run("services.api", "lib/v2/b2.py", "exact"),
        ]
    );
}

#[test]
fn compose_builds_with_the_dockerfile_the_service_names_and_an_override_changes() {
    let repo = repo(
        "images_dockerfile_named",
        &[
            (
                "compose.yaml",
                "services:\n  relative:\n    build:\n      context: ./svc\n      dockerfile: docker/Dockerfile.prod\n  \
                 inline:\n    build:\n      context: ./svc\n      dockerfile_inline: |\n        FROM x\n        \
                 COPY src /app\n  changed:\n    build:\n      context: ./svc\n      dockerfile: Dockerfile.base\n  \
                 kept:\n    build:\n      context: ./svc\n      dockerfile: Dockerfile.base\n",
            ),
            (
                "compose.override.yaml",
                "services:\n  changed:\n    build:\n      dockerfile: Dockerfile.over\n  kept:\n    build: ./svc\n",
            ),
            ("svc/docker/Dockerfile.prod", "FROM x\nCOPY prod /p\n"),
            ("svc/Dockerfile.base", "FROM x\nCOPY base /b\n"),
            ("svc/Dockerfile.over", "FROM x\nCOPY over /o\n"),
            ("svc/Dockerfile", "FROM x\nCOPY wrong /w\n"),
            ("svc/prod/a.py", "def a(): pass\n"),
            ("svc/src/b.py", "def b(): pass\n"),
            ("svc/base/c.py", "def c(): pass\n"),
            ("svc/over/d.py", "def d(): pass\n"),
            ("svc/wrong/e.py", "def e(): pass\n"),
        ],
    );
    let parsed = export(&repo);
    assert_eq!(
        runs(&parsed),
        [
            run("services.changed", "svc/over", "exact"),
            run("services.inline", "svc/src", "exact"),
            run("services.kept", "svc/base", "exact"),
            run("services.relative", "svc/prod", "exact"),
        ]
    );
    assert!(
        reasons(&parsed, "services.relative")[0]
            .ends_with("; Dockerfile svc/docker/Dockerfile.prod")
    );
    assert!(reasons(&parsed, "services.inline")[0].ends_with("; Dockerfile dockerfile_inline"));
    assert!(reasons(&parsed, "services.changed")[0].ends_with("; Dockerfile svc/Dockerfile.over"));
}

#[test]
fn an_overridable_context_or_dockerfile_keeps_its_paths_inferred() {
    let repo = repo(
        "images_dockerfile_overridable",
        &[
            (
                "compose.yaml",
                "services:\n  context:\n    build: ${CTX:-./a}\n  file:\n    build:\n      context: ./b\n      \
                 dockerfile: ${DF:-Dockerfile.b}\n  plain:\n    build: ./c\n",
            ),
            ("a/Dockerfile", "FROM x\nCOPY src /s\n"),
            ("a/src/a.py", "def a(): pass\n"),
            ("b/Dockerfile.b", "FROM x\nCOPY src /s\n"),
            ("b/src/b.py", "def b(): pass\n"),
            ("c/Dockerfile", "FROM x\nCOPY src /s\n"),
            ("c/src/c.py", "def c(): pass\n"),
        ],
    );
    assert_eq!(
        runs(&export(&repo)),
        [
            run("services.context", "a/src", "inferred"),
            run("services.file", "b/src", "inferred"),
            run("services.plain", "c/src", "exact"),
        ]
    );
    let context = Engine::open(&repo)
        .unwrap()
        .context("services.context", None)
        .unwrap();
    assert!(
        context.contains("code: example.com/acme/app a/src [inferred]"),
        "{context}"
    );
}

/// A workload is written once per answer, under the first node it runs: `x` runs leaf and mid, `y`
/// runs leaf, mid and top, and neither line is written again under mid or top.
#[test]
fn an_upward_trace_writes_each_workload_once_under_the_first_node_it_runs() {
    use meada::engine::{Direction, Start};
    let repo = repo(
        "images_dockerfile_trace",
        &[
            (
                "compose.yaml",
                "services:\n  x:\n    build:\n      context: .\n      dockerfile: Dockerfile.x\n  y:\n    \
                 build:\n      context: .\n      dockerfile: Dockerfile.y\n",
            ),
            ("Dockerfile.x", "FROM x\nCOPY leaf.py mid.py /app/\n"),
            (
                "Dockerfile.y",
                "FROM x\nCOPY mid.py top.py /app/\nCOPY leaf.py /leaf.py\n",
            ),
            ("leaf.py", "def leaf():\n    return 1\n"),
            (
                "mid.py",
                "from leaf import leaf\n\n\ndef mid():\n    return leaf()\n",
            ),
            (
                "top.py",
                "from mid import mid\n\n\ndef top():\n    return mid()\n",
            ),
        ],
    );
    let up = Engine::open(&repo)
        .unwrap()
        .trace(Start::From("leaf"), Direction::Up, None)
        .unwrap();
    let lines: Vec<&str> = up.lines().collect();
    let at = |needle: &str| lines.iter().position(|l| l.contains(needle));
    assert_eq!(up.matches("runs code").count(), 2, "{up}");
    for service in ["services.x", "services.y"] {
        assert_eq!(up.matches(&format!("{service} (root")).count(), 1, "{up}");
    }
    // Both run leaf, the first node: both lines are written under it, before its callers.
    assert!(
        at("services.x") < at("mid.py:") && at("services.y") < at("mid.py:"),
        "{up}"
    );
}
