//! Shape drift (spec #23): the field paths a consumer reads from the parsed response, against
//! the shape of the route it links to, over the drift chain (`common::drift_chain`).

mod common;

use common::{assert_golden, decisions, drift_chain, without_processes};
use meada::engine::{self, Direction, Engine, Start};

#[test]
fn export_carries_consumer_shapes_and_drift() {
    let (web, _, _) = drift_chain("drift_export");

    let export = Engine::open(&web).unwrap().export().unwrap();

    assert_golden("chain-drift-web.export.json", &without_processes(&export));
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
        assert_golden(golden, &without_processes(&export));
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
        "GET /api/inventory/{}/price",
        "GET /api/reviews",
    ] {
        let start = Start::From(route);
        answers.push(engine.trace(start, Direction::Up, None).unwrap());
    }

    assert_golden("chain-drift.trace-literals.txt", &answers.join("\n"));
}

#[test]
fn context_on_a_route_names_the_shape_source_and_the_drift_of_its_consumers() {
    let (web, _, _) = drift_chain("drift_context");
    let engine = Engine::open(&web).unwrap();

    // A document (its consumers with drift, with reads not checked, and with neither, which are
    // listed too), a zod schema and returned literals; and a route whose consumer only has reads
    // not checked.
    let routes = [
        ("orders", "http::GET::/api/orders"),
        ("comments", "http::GET::/api/comments"),
        ("inventory", "http::GET::/api/inventory/{}"),
        ("cached", "http::GET::/api/cached"),
    ];
    for (name, route) in routes {
        let answer = engine.context(route, None).unwrap();
        assert_golden(&format!("chain-drift.context-route-{name}.txt"), &answer);
    }
}

#[test]
fn search_drift_lists_each_missing_path_with_its_consumer_and_level() {
    let (web, _, _) = drift_chain("drift_search");
    let engine = Engine::open(&web).unwrap();

    // The query matches the route path or the field path.
    let mut answers = Vec::new();
    for query in ["/api/stock", "total", "nickname", "nothing-like-this"] {
        answers.push(engine.search(query, Some("drift"), None).unwrap());
    }

    assert_golden("chain-drift.search-drift.txt", &answers.join("\n"));
}

#[test]
fn status_prints_the_drift_count_of_each_repository() {
    let (web, _, _) = drift_chain("drift_status");
    Engine::open(&web).unwrap().refresh().unwrap();

    assert_golden("chain-drift.status.txt", &engine::status(&web).unwrap());
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
        "GET /api/tickets",
        "GET /api/offers",
        "GET /api/tags",
    ] {
        let start = Start::From(route);
        answers.push(engine.trace(start, Direction::Up, None).unwrap());
    }

    assert_golden("chain-drift.trace-shared.txt", &answers.join("\n"));
}

#[test]
fn a_decided_link_has_drift_and_an_ambiguous_one_none() {
    let (web, _, _) = drift_chain("drift_decided_trace");
    decisions(&web, "chain-drift-web");
    let engine = Engine::open(&web).unwrap();

    // `rate` reaches `GET /api/reviews` through a decision; `ping` fits two `GET /api/ping`.
    let mut answers = Vec::new();
    for route in ["GET /api/reviews", "GET /api/ping"] {
        let start = Start::From(route);
        answers.push(engine.trace(start, Direction::Up, None).unwrap());
    }

    assert_golden("chain-drift.trace-decided.txt", &answers.join("\n"));
}

#[test]
fn trace_down_from_a_consumer_ends_its_link_line_with_the_missing_paths() {
    let (web, _, _) = drift_chain("drift_trace_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("src/stock.ts::price"), Direction::Down, None)
        .unwrap();

    assert_golden("chain-drift.trace-down.txt", &answer);
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
