//! Resource queries (spec #19) over the chain fixture: the gateway writes the `postgres`
//! resource `orders` through `cancelOrder`, the worker reads it through `settleOrders` and the
//! catalog reads it as a whole repository.

use crate::common;

use common::{assert_golden, chain, loose_chain};
use meada::engine::{self, Direction, Engine, Start};

#[test]
fn search_kind_resource_finds_a_resource_by_its_id_and_by_its_kind() {
    let (web, _, _) = chain("resource_search");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "chain.search-resource.txt",
        &engine.search("orders", Some("resource"), None).unwrap(),
    );
    // The kind matches too, and `route` never lists a resource.
    let by_kind = engine.search("postgres", Some("resource"), None).unwrap();
    assert_eq!(by_kind.lines().count(), 3, "{by_kind}");
    let routes = engine.search("orders", Some("route"), None).unwrap();
    assert!(!routes.contains("resource::"), "{routes}");
    let all = engine.search("orders", Some("contract"), None).unwrap();
    assert!(all.contains("resource::postgres::orders"), "{all}");
}

#[test]
fn context_on_a_resource_lists_its_writers_and_readers() {
    let (web, _, _) = chain("resource_context");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "chain.context-resource.txt",
        &engine.context("resource::postgres::orders", None).unwrap(),
    );
    assert_eq!(
        engine.context("resource::postgres::gone", None).unwrap(),
        "no resource matches `resource::postgres::gone`\n"
    );
}

#[test]
fn context_on_a_reader_symbol_shows_the_resource_like_a_writer_does() {
    let (web, _, _) = chain("resource_context_reader");
    let engine = Engine::open(&web).unwrap();

    let reader = engine.context("settleOrders", None).unwrap();
    let writer = engine
        .context(
            "example.com/chain/gateway::src/orders.ts::cancelOrder",
            None,
        )
        .unwrap();

    assert!(
        reader.contains("consumes resource::postgres::orders"),
        "{reader}"
    );
    assert!(
        writer.contains("provides resource::postgres::orders"),
        "{writer}"
    );
}

#[test]
fn trace_does_not_start_from_a_resource_and_points_to_context() {
    let (web, _, _) = chain("resource_trace_start");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("resource::postgres::orders"),
            Direction::Down,
            None,
        )
        .unwrap();

    assert_golden("chain.trace-resource-start.txt", &answer);
}

#[test]
fn status_reports_a_resource_with_one_side_and_a_via_that_names_no_symbol() {
    let gateway = loose_chain("resource_status");
    Engine::open(&gateway).unwrap().refresh().unwrap();

    assert_golden("chain-loose.status.txt", &engine::status(&gateway).unwrap());
}
