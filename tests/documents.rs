//! Provider documents (spec #21): OpenAPI routes of a repository with a `[serve]` table become
//! provider contracts, merged with the code routes they match, over the documented chain
//! (`common::documented_chain`).

mod common;

use common::{assert_golden, documented_chain};
use meada::engine::{self, Direction, Engine, Start};

#[test]
fn document_routes_are_providers_and_merge_with_code_routes() {
    let (web, gateway, _) = documented_chain("documents_export");

    for (repo, name) in [(gateway, "gateway"), (web, "web")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(&format!("chain-documents-{name}.export.json"), &export);
    }
}

#[test]
fn trace_from_web_stops_at_a_documented_only_route() {
    let (web, _, _) = documented_chain("documents_trace");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("shipment"), Direction::Down, None)
        .unwrap();

    assert_golden("chain-documents.trace-shipment.txt", &answer);
}

#[test]
fn search_names_the_document_of_a_documented_only_route() {
    let (web, _, _) = documented_chain("documents_search");

    let answer = Engine::open(&web)
        .unwrap()
        .search("shipments", Some("route"), None)
        .unwrap();

    assert_golden("chain-documents.search-shipments.txt", &answer);
}

#[test]
fn status_reports_the_documents_it_ignored() {
    let (_, gateway, _) = documented_chain("documents_status");
    Engine::open(&gateway).unwrap().refresh().unwrap();

    assert_golden(
        "chain-documents.status.txt",
        &engine::status(&gateway).unwrap(),
    );
}

/// The `(level, reason)` of the link from the web call in `function`, or `None` when it has
/// none.
fn link_of(web: &std::path::Path, function: &str) -> Option<(String, String)> {
    let export = Engine::open(web).unwrap().export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    let links = export["links"].as_array().unwrap();
    let link = links.iter().find(|l| {
        let consumer = l["consumer"].as_str().unwrap();
        consumer.contains(&format!("::{function}::"))
    })?;
    Some((
        link["level"].as_str().unwrap().into(),
        link["reason"].as_str().unwrap().into(),
    ))
}

#[test]
fn servers_urls_add_hosts_and_base_paths() {
    let (web, _, _) = documented_chain("documents_servers");
    let link = |function| link_of(&web, function);
    let inferred = |reason: &str| Some(("inferred".to_string(), reason.to_string()));

    // The public URL of the document: its host and its base path.
    assert_eq!(link("edgeShipment"), inferred("gateway prefix /gw"));
    // A server variable takes its default.
    assert_eq!(link("euShipment"), inferred("gateway prefix /v3"));
    // A relative URL gives only its path, whatever the host.
    assert_eq!(
        link("relativeShipment"),
        inferred("gateway prefix /relative/base")
    );
    // A variable without a default contributes nothing, not even its path.
    assert_eq!(link("unsetShipment"), None);
}

#[test]
fn listed_documents_are_read_even_when_ignored_or_oddly_named() {
    let (_, gateway, _) = documented_chain("documents_listed");

    let export = Engine::open(&gateway).unwrap().export().unwrap();

    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    let strings = |key: &str, field: &str| -> Vec<String> {
        let items = export[key].as_array().unwrap();
        items
            .iter()
            .map(|i| i[field].as_str().unwrap().to_string())
            .collect()
    };
    let documents = strings("documents", "path");
    // Listed and gitignored; listed under a name discovery does not know.
    assert!(documents.contains(&"generated/openapi.yaml".into()));
    assert!(documents.contains(&"docs/api-contract.yaml".into()));
    // Listed and missing: no document, no error.
    assert!(!documents.contains(&"docs/missing.yaml".into()));
    let providers = strings("providers", "contract");
    assert!(providers.contains(&"http::GET::/api/generated".into()));
    assert!(providers.contains(&"http::GET::/api/carriers".into()));
}

#[test]
fn search_finds_a_document_by_path_or_title() {
    let (web, gateway, _) = documented_chain("documents_find");
    let engine = Engine::open(&gateway).unwrap();

    let by = |query: &str| engine.search(query, Some("document"), None).unwrap();

    assert_golden(
        "chain-documents.search-document-path.txt",
        &by("orders.openapi"),
    );
    assert_golden("chain-documents.search-document-title.txt", &by("CARRIERS"));
    assert_eq!(by("nothing"), "no matches for `nothing`\n");
    // The workspace's documents, from any repository; the vendored copy of a repository
    // without `[serve]` is none.
    let answer = Engine::open(&web)
        .unwrap()
        .search("openapi", Some("document"), None)
        .unwrap();
    assert!(answer.contains("chain/gateway openapi.yaml"), "{answer}");
    assert!(!answer.contains("vendor"), "{answer}");
}
