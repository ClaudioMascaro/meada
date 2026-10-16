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

#[test]
fn trace_ends_the_links_of_consumers_reading_past_a_zod_shape() {
    let (web, _, _) = drift_chain("drift_zod_trace");
    let engine = Engine::open(&web).unwrap();

    let mut answers = Vec::new();
    for route in [
        "GET /api/comments",
        "POST /api/comments",
        "GET /api/comments/summary",
    ] {
        let start = Start::From(route);
        answers.push(engine.trace(start, Direction::Up, None).unwrap());
    }

    assert_golden("chain-drift.trace-zod.txt", &answers.join("\n"));
}

#[test]
fn trace_ends_the_links_of_consumers_destructuring_and_iterating_the_response() {
    let (web, _, _) = drift_chain("drift_reads_trace");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("GET /api/batches"), Direction::Up, None)
        .unwrap();

    assert_golden("chain-drift.trace-batches.txt", &answer);
}

#[test]
fn trace_ends_the_links_of_consumers_reading_past_a_declared_type() {
    let (web, _, _) = drift_chain("drift_declared_trace");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("GET /api/profiles"), Direction::Up, None)
        .unwrap();

    assert_golden("chain-drift.trace-declared.txt", &answer);
}

#[test]
fn trace_marks_reads_under_an_index_signature_of_the_return_type_not_checked() {
    let (web, _, _) = drift_chain("drift_labels_trace");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("GET /api/labels"), Direction::Up, None)
        .unwrap();

    assert_golden("chain-drift.trace-labels.txt", &answer);
}

#[test]
fn trace_ends_the_links_of_consumers_of_shapes_shared_through_a_package() {
    let (web, _, _) = drift_chain("drift_shared_trace");
    let engine = Engine::open(&web).unwrap();

    let mut answers = Vec::new();
    for route in [
        "GET /api/quotes",
        "GET /api/shelves/{}",
        "GET /api/shelves",
        "GET /api/receipts",
    ] {
        let start = Start::From(route);
        answers.push(engine.trace(start, Direction::Up, None).unwrap());
    }

    assert_golden("chain-drift.trace-shared.txt", &answers.join("\n"));
}

/// The gateway's `listOrders` sends a body parsed with a zod schema.
fn parse_the_orders(gateway: &std::path::Path) {
    let path = gateway.join("src/orders.ts");
    let src = std::fs::read_to_string(&path).unwrap();
    let schema = "import { z } from \"zod\";\nconst Orders = z.array(z.object({ id: z.string(), note: z.string() }));\n";
    let edited = src.replace(
        "res.json([{ id: \"o1\", product }]);",
        "res.json(Orders.parse([{ id: \"o1\", product }]));",
    );
    assert_ne!(src, edited);
    std::fs::write(path, format!("{schema}{edited}")).unwrap();
}

#[test]
fn a_document_wins_over_a_zod_schema() {
    let route = "\"contract\": \"http::GET::/api/orders\",\n      \"site\": \"example.com/chain/gateway::src/orders.ts::listOrders\",\n      \"shape\": {\n        \"source\": {\n";
    let (_, gateway, _) = drift_chain("drift_zod_order");
    parse_the_orders(&gateway);
    let documented = Engine::open(&gateway).unwrap().export().unwrap();
    assert!(documented.contains(&format!("{route}          \"document\": \"openapi.yaml\"")));

    // Without the documents, the schema is the source.
    let (_, gateway, _) = drift_chain("drift_zod_order_no_documents");
    parse_the_orders(&gateway);
    for document in [
        "openapi.yaml",
        "legacy.openapi.yaml",
        "broken.openapi.yaml",
        "orders.openapi.json",
    ] {
        std::fs::remove_file(gateway.join(document)).unwrap();
    }
    std::fs::remove_dir_all(gateway.join("docs")).unwrap();
    let coded = Engine::open(&gateway).unwrap().export().unwrap();
    assert!(coded.contains(&format!("{route}          \"zod\": \"src/orders.ts\"")));
}
