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
