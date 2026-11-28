use std::fs;

use meada::engine::{Direction, Engine, Start};

use crate::common::{assert_golden, fixture_repo};

const ORIGIN: Option<&str> = Some("https://example.com/acme/platform.git");

fn answers(questions: &[&str], ask: impl Fn(&str) -> String) -> String {
    (questions.iter())
        .map(|q| ask(q))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn search_with_the_kind_iac_finds_crossplane_objects_and_composed_resources() {
    let repo = fixture_repo("xp_search", "crossplane-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let queries = [
        "Composition",
        "PostgreSQLInstance",
        "orders",
        "Bucket",
        "logs",
    ];
    let search = |q: &str| engine.search(q, Some("iac"), None).unwrap();
    assert_golden("crossplane-nodes.search.txt", &answers(&queries, search));
}

#[test]
fn context_on_a_crossplane_object_shows_its_role_name_and_references() {
    let repo = fixture_repo("xp_context", "crossplane-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let questions = [
        "CompositeResourceDefinition.apiextensions.crossplane.io/xpostgresqlinstances.database.acme.io",
        "Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws",
        "Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws/subnets",
        "Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws/instance",
        "Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws/backups",
        "Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws/network",
        "Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws-pipeline",
        "PostgreSQLInstance.database.acme.io/team-a/orders-db",
        "PostgreSQLInstance.database.acme.io/team-b/reports-db",
        "XNetwork.network.acme.io/shared",
        "Bucket.s3.aws.upbound.io/shared-assets",
    ];
    let context = |q: &str| engine.context(q, None).unwrap();
    assert_golden(
        "crossplane-nodes.context.txt",
        &answers(&questions, context),
    );
}

#[test]
fn trace_walks_from_a_claim_to_the_resources_its_composition_creates() {
    let repo = fixture_repo("xp_trace", "crossplane-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let down = |q: &str| engine.trace(Start::From(q), Direction::Down, None).unwrap();
    let up = |q: &str| engine.trace(Start::From(q), Direction::Up, None).unwrap();
    let text = [
        answers(
            &["PostgreSQLInstance.database.acme.io/team-a/orders-db"],
            down,
        ),
        answers(
            &["Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws/backups"],
            up,
        ),
        answers(&["Bucket.s3.aws.upbound.io/shared-assets"], down),
    ];
    assert_golden("crossplane-nodes.trace.txt", &text.join("\n"));
}

#[test]
fn a_changed_composition_or_definition_changes_the_nodes_at_the_next_query() {
    let repo = fixture_repo("xp_fresh", "crossplane-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let subnets = "Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws/subnets";
    let before = engine.context(subnets, None).unwrap();
    assert!(before.contains("name: \"orders-db-subnets\"\n"), "{before}");
    let path = repo.join("platform/composition.yaml");
    let text = fs::read_to_string(&path).unwrap().replace(
        "            region: us-east-1\n    - name: instance",
        "            region: us-east-1\n      patches:\n        - type: PatchSet\n          \
         patchSetName: named\n    - name: instance",
    );
    fs::write(&path, text).unwrap();
    let after = engine.context(subnets, None).unwrap();
    assert!(after.contains("name: \"*\" (pattern)\n"), "{after}");

    let claim = "PostgreSQLInstance.database.acme.io/team-b/reports-db";
    assert!(
        engine
            .context(claim, None)
            .unwrap()
            .contains("role: crossplane claim\n")
    );
    fs::remove_file(repo.join("platform/definition.yaml")).unwrap();
    let gone = engine.context(claim, None).unwrap();
    assert!(!gone.contains("role:"), "{gone}");
}

#[test]
fn only_named_curated_crossplane_resources_enter_the_export() {
    let repo = fixture_repo("xp_export", "crossplane-nodes", ORIGIN);
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value.as_object_mut().unwrap().remove("commit");
    value.as_object_mut().unwrap().remove("dirty");
    assert_eq!(
        value,
        serde_json::json!({
            "format": 1,
            "repository": "example.com/acme/platform",
            "files": [], "symbols": [], "calls": [], "providers": [], "consumers": [],
            "iac_nodes": [
                {
                    "identity": "example.com/acme/platform::platform/bucket.yaml::Bucket.s3.aws.upbound.io/shared-assets",
                    "address": "Bucket.s3.aws.upbound.io/shared-assets", "type": "Bucket",
                    "root": "platform", "path": "platform/bucket.yaml", "line": 1, "end_line": 11
                },
                {
                    "identity": "example.com/acme/platform::platform/pipeline.yaml::Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws-pipeline/events",
                    "address": "Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws-pipeline/events", "type": "Queue",
                    "root": "platform", "path": "platform/pipeline.yaml", "line": 18, "end_line": 27
                }
            ],
            "provisioners": [
                { "contract": "channel::sqs::orders-events", "node": "example.com/acme/platform::platform/pipeline.yaml::Composition.apiextensions.crossplane.io/xpostgresqlinstances.aws-pipeline/events", "root": "platform" },
                { "contract": "resource::s3::acme-shared-assets", "node": "example.com/acme/platform::platform/bucket.yaml::Bucket.s3.aws.upbound.io/shared-assets", "root": "platform" }
            ],
            "links": []
        })
    );
}
