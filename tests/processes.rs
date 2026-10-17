//! Entry points and their processes (spec #31): `meada export` carries each repository's
//! processes, and `trace` reads one from `process::<entry point identity>`, over the chain
//! fixture.

mod common;

use std::fs;

use common::{assert_golden, chain, fixture_repo};
use meada::engine::{Direction, Engine, Start};

const LIST_ORDERS: &str = "process::example.com/chain/gateway::src/orders.ts::listOrders\
                           ::http::GET::/api/orders";
const PRODUCT: &str = "process::example.com/chain/catalog::app/api/products/[id]/route.ts::GET\
                       ::http::GET::/api/products/{}";

fn processes(repo: &std::path::Path) -> String {
    let export = Engine::open(repo).unwrap().export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    serde_json::to_string_pretty(&export["processes"]).unwrap() + "\n"
}

fn trace(
    repo: &std::path::Path,
    from: &str,
    direction: Direction,
    budget: Option<usize>,
) -> String {
    let engine = Engine::open(repo).unwrap();
    engine.trace(Start::From(from), direction, budget).unwrap()
}

#[test]
fn each_repository_exports_the_process_of_every_entry_point() {
    let (web, gateway, catalog) = chain("processes_export");

    assert_golden("chain-web.processes.json", &processes(&web));
    assert_golden("chain-gateway.processes.json", &processes(&gateway));
    assert_golden("chain-catalog.processes.json", &processes(&catalog));
}

#[test]
fn a_route_handler_is_no_export_entry_point() {
    let (_, gateway, _) = chain("processes_route_not_export");

    let found = processes(&gateway);

    // `listOrders` is exported and nothing calls it: it is the entry point of its route only.
    assert!(found.contains("\"entry_point\": \"example.com/chain/gateway::src/orders.ts::listOrders::http::GET::/api/orders\""));
    assert!(
        !found
            .contains("\"entry_point\": \"example.com/chain/gateway::src/orders.ts::listOrders\"")
    );
}

#[test]
fn trace_from_a_route_process_crosses_into_a_peer() {
    let (_, gateway, _) = chain("processes_trace_route");

    let answer = trace(&gateway, LIST_ORDERS, Direction::Down, None);

    assert_golden("chain.trace-process-route.txt", &answer);
}

#[test]
fn a_process_stops_at_a_cycle_and_shows_a_repeated_symbol_as_a_reference() {
    let (_, _, catalog) = chain("processes_trace_cycle");

    let answer = trace(&catalog, PRODUCT, Direction::Down, None);

    assert_golden("chain.trace-process-cycle.txt", &answer);
}

#[test]
fn trace_from_an_export_process_follows_links_across_repositories() {
    let (web, _, _) = chain("processes_trace_export");

    let answer = trace(
        &web,
        "process::example.com/chain/web::src/dashboard.ts::dashboard",
        Direction::Down,
        None,
    );

    assert_golden("chain.trace-process-export.txt", &answer);
}

#[test]
fn an_ambiguous_call_is_one_step_listing_its_candidates() {
    let (_, _, catalog) = chain("processes_trace_ambiguous");
    fs::write(
        catalog.join("lib/ambiguous.ts"),
        "export class Cache {\n  get(key: string) {\n    return key;\n  }\n}\n\n\
         export class Store {\n  get(key: string) {\n    return key;\n  }\n}\n\n\
         export function lookup(source: { get(key: string): string }, key: string) {\n  \
           return source.get(key);\n}\n",
    )
    .unwrap();
    let from = "process::example.com/chain/catalog::lib/ambiguous.ts::lookup";

    let answer = trace(&catalog, from, Direction::Down, None);
    let found = processes(&catalog);

    assert_golden("chain.trace-process-ambiguous.txt", &answer);
    // The two methods have an ambiguous caller, so neither is an entry point.
    assert!(
        !found.contains(
            "\"entry_point\": \"example.com/chain/catalog::lib/ambiguous.ts::Cache.get\""
        )
    );
    assert!(
        found.contains("\"entry_point\": \"example.com/chain/catalog::lib/ambiguous.ts::lookup\"")
    );
}

#[test]
fn a_missing_peer_is_named_in_the_footer_of_a_process() {
    let case = "processes_trace_missing";
    let web = fixture_repo(case, "chain-web", Some("https://example.com/chain/web.git"));
    fixture_repo(
        case,
        "chain-gateway",
        Some("git@example.com:chain/gateway.git"),
    );

    let answer = trace(
        &web,
        "process::example.com/chain/web::src/dashboard.ts::dashboard",
        Direction::Down,
        None,
    );

    assert_golden("chain.trace-process-missing-peer.txt", &answer);
}

#[test]
fn a_process_is_cut_to_the_budget_with_the_footer_last() {
    let (_, gateway, _) = chain("processes_trace_cut");

    let answer = trace(&gateway, LIST_ORDERS, Direction::Down, Some(100));

    assert_golden("chain.trace-process-cut.txt", &answer);
}

#[test]
fn only_the_process_form_starts_a_process() {
    let (_, gateway, _) = chain("processes_trace_form");

    let none = trace(
        &gateway,
        "process::example.com/nothing",
        Direction::Down,
        None,
    );
    let up = trace(&gateway, LIST_ORDERS, Direction::Up, None);
    // The entry point's handler, named plainly, is an ordinary symbol start.
    let plain = trace(&gateway, "listOrders", Direction::Down, None);

    assert_golden("chain.trace-process-none.txt", &none);
    assert_golden("chain.trace-process-up.txt", &up);
    assert!(!plain.starts_with("process "));
}
