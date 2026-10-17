//! Communities and processes in `search` and `context` (spec #31), over the chain fixture.

mod common;

use common::{assert_golden, chain};
use meada::engine::Engine;

#[test]
fn search_finds_a_community_by_name() {
    let (_, gateway, _) = chain("community_queries_search_community");
    let engine = Engine::open(&gateway).unwrap();

    let answer = engine.search("lib", Some("community"), None).unwrap();

    assert_golden("chain.search-community.txt", &answer);
}

#[test]
fn search_finds_a_process_by_the_label_of_its_entry_point() {
    let (_, gateway, _) = chain("community_queries_search_process");
    let engine = Engine::open(&gateway).unwrap();

    let route = engine.search("/api/orders", Some("process"), None).unwrap();
    let export = engine.search("dashboard", Some("process"), None).unwrap();

    assert_golden("chain.search-process-route.txt", &route);
    assert_golden("chain.search-process-export.txt", &export);
}
