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
