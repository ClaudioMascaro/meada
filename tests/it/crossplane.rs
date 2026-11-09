//! Crossplane objects among the Kubernetes nodes (#655, ADR 0025): composite resource
//! definitions, compositions, claims and managed resources are recognized, each resource a
//! composition creates from a literal base is a node under the composition, and the references
//! run from a claim to its composite kind, from a composition to the type it composes and to the
//! resources it creates, and from those to the definition of their kind.

use std::fs;

use meada::engine::{Direction, Engine, Start};

use crate::common::{assert_golden, fixture_repo};

const ORIGIN: Option<&str> = Some("https://example.com/acme/platform.git");

/// Each answer of `ask` over `questions`, in order, a blank line between two.
fn answers(questions: &[&str], ask: impl Fn(&str) -> String) -> String {
    (questions.iter())
        .map(|q| ask(q))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fixture: in `platform`, an XRD with a claim kind and a default composition, a second XRD
/// with a composite resource of its kind, a composition of four resources (an external name, a
/// name a patch set sets, a literal name only an outward patch reads, a nested composite whose
/// name a patch sets), a pipeline composition (one patch-and-transform resource, one templated
/// step that creates no node), a claim naming the pipeline composition, and a managed resource
/// beside the definition of its kind; in `examples`, a claim away from its XRD.
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

/// `context` names each object's Crossplane role, the physical name of a composed resource (a
/// pattern when a patch sets it), and the references both ways.
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

/// `trace` walks from a claim down to its composite kind, the compositions and the resources
/// they create, and up from a composed resource to the claim.
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

/// A patch that starts setting a resource's name turns its name into a pattern at the next
/// query, and an object of a kind no XRD defines any more loses its role.
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

/// Only curated resources with complete identities enter the contract graph (#671).
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
