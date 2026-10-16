//! Shape drift (spec #23): the field paths a consumer reads from the parsed response, against
//! the shape of the route it links to, over the drift chain (`common::drift_chain`).

mod common;

use common::{assert_golden, drift_chain};
use meada::engine::{Direction, Engine, Start};

#[test]
fn export_carries_consumer_shapes_and_drift() {
    let (web, _, _) = drift_chain("drift_export");

    let export = Engine::open(&web).unwrap().export().unwrap();

    assert_golden("chain-drift-web.export.json", &export);
}

#[test]
fn trace_ends_the_link_lines_of_drifting_consumers_with_the_missing_paths() {
    let (web, _, _) = drift_chain("drift_trace");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("GET /api/orders"), Direction::Up, None)
        .unwrap();

    assert_golden("chain-drift.trace-orders.txt", &answer);
}

#[test]
fn export_carries_the_shapes_of_returned_object_literals() {
    let (_, gateway, catalog) = drift_chain("drift_literals_export");

    for (repo, golden) in [
        (&gateway, "chain-drift-gateway.export.json"),
        (&catalog, "chain-drift-catalog.export.json"),
    ] {
        let export = Engine::open(repo).unwrap().export().unwrap();
        assert_golden(golden, &export);
    }
}

#[test]
fn trace_ends_the_links_of_consumers_reading_past_a_literal_shape() {
    let (web, _, _) = drift_chain("drift_literals_trace");
    let engine = Engine::open(&web).unwrap();

    let mut answers = Vec::new();
    for route in [
        "GET /api/inventory/{}",
        "POST /api/inventory",
        "GET /api/stock",
        "POST /api/stock",
        "GET /api/stock/{}",
        "GET /api/cached",
        "GET /api/inventory/{}/image",
    ] {
        let start = Start::From(route);
        answers.push(engine.trace(start, Direction::Up, None).unwrap());
    }

    assert_golden("chain-drift.trace-literals.txt", &answers.join("\n"));
}

#[test]
fn context_on_a_route_names_the_literals_its_shape_comes_from() {
    let (_, gateway, _) = drift_chain("drift_literals_context");
    let engine = Engine::open(&gateway).unwrap();

    let answer = engine
        .context("http::GET::/api/inventory/{}", None)
        .unwrap();

    assert_golden("chain-drift.context-route-inventory.txt", &answer);
}
