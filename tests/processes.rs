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
fn a_process_stops_at_a_cycle() {
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

#[test]
fn a_symbol_reached_from_two_branches_is_expanded_once() {
    let (_, _, catalog) = chain("processes_trace_seen");
    fs::write(
        catalog.join("lib/diamond.ts"),
        "export function root() {\n  return left() + right();\n}\n\n\
         function left() {\n  return shared();\n}\n\n\
         function right() {\n  return shared();\n}\n\n\
         function shared() {\n  return 1;\n}\n",
    )
    .unwrap();

    let answer = trace(
        &catalog,
        "process::example.com/chain/catalog::lib/diamond.ts::root",
        Direction::Down,
        None,
    );

    assert_golden("chain.trace-process-seen.txt", &answer);
    assert!(answer.contains("(see above)"));
}

#[test]
fn an_ambiguous_link_is_one_step_listing_its_providers() {
    let (web, _, _) = chain("processes_ambiguous_link");

    // `dashboard` calls `/api/health`, which two gateway handlers serve: neither is followed.
    let found: serde_json::Value = serde_json::from_str(&processes(&web)).unwrap();
    let steps = found
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["steps"].as_array().unwrap())
        .filter(|s| s["mark"] == "ambiguous" && s["level"] == "ambiguous")
        .collect::<Vec<_>>();

    assert!(!steps.is_empty());
    assert!(
        steps
            .iter()
            .all(|s| s["candidates"].as_array().unwrap().len() > 1)
    );
}

#[test]
fn a_route_served_by_a_file_starts_from_the_files_calls() {
    let api = fixture_repo(
        "processes_file_route",
        "http-api",
        Some("https://example.com/acme/api.git"),
    );

    let found = processes(&api);

    let id = "\"entry_point\": \"example.com/acme/api::app/api/health/route.js::http::GET::/api/health\"";
    assert!(found.contains(id), "{found}");
}

#[test]
fn a_recursive_export_nothing_else_calls_is_an_entry_point() {
    let (_, _, catalog) = chain("processes_self_call");
    fs::write(
        catalog.join("lib/recur.ts"),
        "export function countdown(n: number): number {\n  return n <= 0 ? 0 : countdown(n - 1);\n}\n",
    )
    .unwrap();

    let found = processes(&catalog);

    let id = "\"entry_point\": \"example.com/chain/catalog::lib/recur.ts::countdown\"";
    assert!(found.contains(id));
}

#[test]
fn a_handler_registered_twice_on_a_path_is_one_entry_point() {
    let (_, gateway, _) = chain("processes_dedupe");
    fs::write(
        gateway.join("src/twice.ts"),
        "import express from \"express\";\n\nconst app = express();\n\n\
         export function ping(_req: unknown, res: { send(body: string): void }) {\n  \
           res.send(\"pong\");\n}\n\n\
         app.get(\"/api/ping\", ping);\napp.get(\"/api/ping\", ping);\n",
    )
    .unwrap();

    let found = processes(&gateway);

    let id =
        "\"entry_point\": \"example.com/chain/gateway::src/twice.ts::ping::http::GET::/api/ping\"";
    assert_eq!(found.matches(id).count(), 1, "{found}");
}

#[test]
fn a_process_stops_at_the_depth_limit_and_says_so() {
    let (_, _, catalog) = chain("processes_depth");
    let mut code = String::from("export function step0() {\n  return step1();\n}\n");
    for n in 1..300 {
        code += &format!("function step{n}() {{\n  return step{}();\n}}\n", n + 1);
    }
    code += "function step300() {\n  return 1;\n}\n";
    fs::write(catalog.join("lib/deep.ts"), code).unwrap();

    let found: serde_json::Value = serde_json::from_str(&processes(&catalog)).unwrap();
    let deep = (found.as_array().unwrap().iter())
        .find(|p| {
            let id = p["entry_point"].as_str().unwrap();
            id.ends_with("lib/deep.ts::step0")
        })
        .unwrap();
    let steps = deep["steps"].as_array().unwrap();

    let last = steps.last().unwrap();
    assert_eq!(last["mark"], "depth-limit");
    assert_eq!(last["depth"], 256);
    assert_eq!(steps.len(), 257);
}

const ORDER_CREATED: &str = "process::example.com/chain/worker::src/consumer.ts::onOrderCreated\
                             ::channel::kafka::orders.created";

#[test]
fn a_channel_handler_is_a_channel_entry_point_and_no_export() {
    let (_, _, catalog) = chain("processes_channel_export");
    let worker = common::worker(&catalog);

    let found = processes(&worker);

    assert_golden("chain-worker.processes.json", &found);
    assert!(found.contains("\"kind\": \"channel\""));
    assert!(found.contains("\"label\": \"channel::kafka::orders.created\""));
    assert!(!found.contains("src/consumer.ts::onOrderCreated\","));
}

#[test]
fn trace_from_a_channel_process_shows_its_tree_and_the_links_it_reaches() {
    let (_, _, catalog) = chain("processes_channel_trace");
    let worker = common::worker(&catalog);

    let answer = trace(&worker, ORDER_CREATED, Direction::Down, None);

    assert_golden("chain.trace-process-channel.txt", &answer);
}
