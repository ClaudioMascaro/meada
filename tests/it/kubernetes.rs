use std::fs;

use meada::engine::{Direction, Engine, Start};

use crate::common::{assert_golden, fixture_repo};

const ORIGIN: Option<&str> = Some("https://example.com/acme/deploy.git");

fn answers(questions: &[&str], ask: impl Fn(&str) -> String) -> String {
    (questions.iter())
        .map(|q| ask(q))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn search_with_the_kind_iac_finds_kubernetes_objects() {
    let repo = fixture_repo("kube_search", "kubernetes-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let queries = [
        "Deployment",
        "dev-api",
        "api-settings",
        "Service/shop/api-v2",
        "Namespace",
        "tools",
        "Release",
    ];
    let search = |q: &str| engine.search(q, Some("iac"), None).unwrap();
    assert_golden("kubernetes-nodes.search.txt", &answers(&queries, search));
}

#[test]
fn context_on_an_object_shows_its_overlay_images_references_and_values() {
    let repo = fixture_repo("kube_context", "kubernetes-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let questions = [
        "Deployment.apps/shop-dev/dev-api",
        "Deployment.apps/shop/api-v2",
        "Service/shop/api-v2",
        "Secret/shop-dev/dev-db-credentials",
        "ServiceAccount/shop/runner-v2 (root overlays/prod)",
        "Namespace/tools",
        "Pod/tools/runner",
        "Job.batch/tools/migrate",
        "base/deployment.yaml::Deployment.apps/shop-dev/dev-api",
    ];
    let context = |q: &str| engine.context(q, None).unwrap();
    assert_golden(
        "kubernetes-nodes.context.txt",
        &answers(&questions, context),
    );
}

#[test]
fn trace_walks_the_references_of_an_object_both_ways() {
    let repo = fixture_repo("kube_trace", "kubernetes-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let down = |q: &str| engine.trace(Start::From(q), Direction::Down, None).unwrap();
    let up = |q: &str| engine.trace(Start::From(q), Direction::Up, None).unwrap();
    let text = [
        answers(&["Ingress.networking.k8s.io/shop-dev/dev-edge"], down),
        answers(&["Service/shop/api-v2"], down),
        answers(&["ConfigMap/shop/api-settings-v2"], up),
        answers(&["Service/tools/runner"], down),
    ];
    assert_golden("kubernetes-nodes.trace.txt", &text.join("\n"));
}

#[test]
fn status_counts_the_entries_a_kustomization_did_not_follow() {
    let repo = fixture_repo("kube_status", "kubernetes-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let status = |engine: &Engine| engine.status_report().unwrap().1.concat().join("\n");
    let text = status(&engine);
    assert!(
        text.contains("kustomize entries not followed: 1 (remote, never fetched: 1)"),
        "{text}"
    );
    #[cfg(unix)]
    {
        fs::create_dir_all(repo.join("overlays/stage")).unwrap();
        std::os::unix::fs::symlink("../../base", repo.join("overlays/linked")).unwrap();
        fs::write(
            repo.join("overlays/stage/kustomization.yaml"),
            "resources:\n  - ../linked\n  - ../../../outside\n  - ../../base/deployment.yaml\n",
        )
        .unwrap();
        let text = status(&engine);
        assert!(
            text.contains("kustomize entries not followed: 3 (remote, never fetched: 1)"),
            "{text}"
        );
        let found = engine.search("Deployment", Some("iac"), None).unwrap();
        let stage: Vec<&str> = found
            .lines()
            .filter(|l| l.contains("overlays/stage"))
            .collect();
        assert_eq!(stage.len(), 1, "{found}");
        assert!(
            stage[0]
                .contains("Deployment.apps/api (Deployment, name \"api\", root overlays/stage)")
        );
    }
}

#[test]
fn only_kubernetes_objects_with_image_sides_enter_the_export() {
    let repo = fixture_repo("kube_export", "kubernetes-nodes", ORIGIN);
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(export["providers"], serde_json::json!([]));
    assert_eq!(export["links"], serde_json::json!([]));
    assert_eq!(export["consumers"].as_array().unwrap().len(), 4, "{text}");
    assert_eq!(export["iac_nodes"].as_array().unwrap().len(), 4, "{text}");
    assert!(
        export["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["contract"]
                .as_str()
                .is_some_and(|c| c.starts_with("image::"))),
        "{text}"
    );
    assert!(!text.contains("api-settings"), "{text}");
}

#[test]
fn the_default_search_leaves_kubernetes_objects_out() {
    let repo = fixture_repo("kube_default", "kubernetes-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let found = engine.search("db-credentials", None, None).unwrap();
    assert!(!found.contains("root overlays"), "{found}");
}

#[test]
fn a_changed_manifest_changes_the_nodes_at_the_next_query() {
    let repo = fixture_repo("kube_fresh", "kubernetes-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let ask = |engine: &Engine| {
        let mut text = engine.search("Service", Some("iac"), None).unwrap();
        text += &engine.search("ConfigMap", Some("iac"), None).unwrap();
        text += &engine
            .trace(Start::From("Pod/tools/runner"), Direction::Up, None)
            .unwrap();
        text
    };
    let before = ask(&engine);
    fs::write(
        repo.join("overlays/dev/kustomization.yaml"),
        "resources:\n  - ../../base\nnamespace: shop-dev\nnamePrefix: dev2-\n",
    )
    .unwrap();
    fs::write(
        repo.join("raw/extra.yaml"),
        "apiVersion: v1\nkind: Service\nmetadata:\n  name: extra\n",
    )
    .unwrap();
    fs::write(
        repo.join("base/kustomization.yaml"),
        "resources:\n  - deployment.yaml\n  - service.yaml\n  - config.yaml\n  - ingress.yaml\n",
    )
    .unwrap();
    let after = ask(&engine);
    assert_ne!(before, after);
    assert!(after.contains("Service/shop-dev/dev2-api"), "{after}");
    assert!(after.contains("Service/extra"), "{after}");
    assert!(!after.contains("dev-api"), "{after}");
    assert!(!after.contains("ServiceAccount/shop-dev"), "{after}");
    fs::remove_file(repo.join("raw/extra.yaml")).unwrap();
    let deleted = ask(&engine);
    assert!(!deleted.contains("Service/extra"), "{deleted}");
    fs::remove_dir_all(repo.join(".meada")).unwrap();
    assert_eq!(deleted, ask(&Engine::open(&repo).unwrap()));
}

#[test]
fn a_refresh_with_no_manifest_changed_keeps_the_nodes() {
    let repo = fixture_repo("kube_kept", "kubernetes-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    engine.refresh().unwrap();
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    db.execute(
        "UPDATE iac_nodes SET type = 'kept' WHERE address = 'Pod/tools/runner'",
        [],
    )
    .unwrap();
    fs::write(repo.join("README.md"), "# deploy\n").unwrap();
    fs::write(repo.join("package.json"), "{\"name\": \"x\"}\n").unwrap();
    let kept = engine
        .search("Pod/tools/runner", Some("iac"), None)
        .unwrap();
    assert!(kept.contains("(kept, "), "{kept}");
    let stack = repo.join("raw/stack.yaml");
    let text = fs::read_to_string(&stack).unwrap();
    fs::write(&stack, format!("{text}\n")).unwrap();
    let fresh = engine
        .search("Pod/tools/runner", Some("iac"), None)
        .unwrap();
    assert!(fresh.contains("(Pod, "), "{fresh}");
}

#[test]
fn a_listed_manifest_past_the_size_ceiling_is_not_an_unresolved_entry() {
    let repo = fixture_repo("kube_big", "kubernetes-nodes", ORIGIN);
    let big = format!("# {}\n", "x".repeat(1 << 21));
    fs::write(repo.join("overlays/dev/big.yaml"), big).unwrap();
    fs::write(
        repo.join("overlays/dev/kustomization.yaml"),
        "resources:\n  - ../../base\n  - big.yaml\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let text = engine.status_report().unwrap().1.concat().join("\n");
    assert!(
        text.contains("kustomize entries not followed: 1 (remote, never fetched: 1)"),
        "{text}"
    );
}

#[test]
fn a_terraform_root_and_a_kubernetes_unit_of_one_directory_keep_each_others_rows() {
    let repo = fixture_repo("kube_shared", "kubernetes-nodes", ORIGIN);
    fs::write(
        repo.join("raw/main.tf"),
        "resource \"aws_sqs_queue\" \"jobs\" {\n  name = \"jobs\"\n}\n\
         resource \"aws_sns_topic\" \"events\" {\n  name = \"events\"\n  queue = aws_sqs_queue.jobs.arn\n}\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let ask = |engine: &Engine| {
        let mut text = engine.search("aws_", Some("iac"), None).unwrap();
        text += &engine.search("runner", Some("iac"), None).unwrap();
        text += &engine
            .trace(Start::From("aws_sns_topic.events"), Direction::Down, None)
            .unwrap();
        text += &engine
            .trace(Start::From("Service/tools/runner"), Direction::Down, None)
            .unwrap();
        text
    };
    let before = ask(&engine);
    assert!(before.contains("aws_sqs_queue.jobs"), "{before}");
    assert!(before.contains("Pod/tools/runner"), "{before}");
    let stack = repo.join("raw/stack.yaml");
    let text = fs::read_to_string(&stack).unwrap();
    fs::write(&stack, format!("{text}\n")).unwrap();
    assert_eq!(before, ask(&engine));
    let main = repo.join("raw/main.tf");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, format!("{text}\n")).unwrap();
    assert_eq!(before, ask(&engine));
    fs::remove_dir_all(repo.join(".meada")).unwrap();
    assert_eq!(before, ask(&Engine::open(&repo).unwrap()));
}
