//! Communities and processes in `search` and `context` (spec #31), over the chain fixture.

mod common;

use std::fs;

use common::{assert_golden, chain, decisions};
use meada::engine::Engine;

#[test]
fn search_finds_a_community_by_name() {
    let (_, gateway, _) = chain("community_queries_search_community");
    let engine = Engine::open(&gateway).unwrap();

    let answer = engine.search("lib", Some("community"), None).unwrap();

    assert_golden("chain.search-community.txt", &answer);
}

#[test]
fn context_names_the_community_and_the_processes_across_repositories() {
    let (_, _, catalog) = chain("community_queries_context");
    let engine = Engine::open(&catalog).unwrap();

    let answer = engine.context("lib/products.ts::find", None).unwrap();

    assert!(answer.contains("community: lib\n"), "{answer}");
    assert_golden("chain.context-processes.txt", &answer);
}

const SPAN_CUT_BUDGET: usize = 150;

const HEALTH: &str = "src/server.ts::GET /api/health";

#[test]
fn a_symbol_with_no_community_says_none() {
    let (_, gateway, _) = chain("community_queries_none");
    let engine = Engine::open(&gateway).unwrap();

    let answer = engine.context(HEALTH, None).unwrap();

    assert!(answer.contains("\ncommunity: none\n"), "{answer}");
    assert!(
        answer.contains(
            "\nprocesses: 1 (1 route; 1 repository): GET /api/health (example.com/chain/gateway)\n"
        ),
        "{answer}"
    );
}

#[test]
fn a_decided_link_counts_and_an_ambiguous_one_does_not() {
    let (web, _, _) = chain("community_queries_decided");
    let health = |engine: &Engine| engine.context(HEALTH, None).unwrap();
    let before = health(&Engine::open(&web).unwrap());

    decisions(&web, "chain-web");
    let after = health(&Engine::open(&web).unwrap());

    // `dashboard` reaches the route through an ambiguous link until a person decides it.
    assert!(!before.contains("dashboard"), "{before}");
    assert!(
        after.contains(
            "\nprocesses: 2 (1 route, 1 export; 2 repositories): \
             GET /api/health (example.com/chain/gateway), dashboard (example.com/chain/web)\n"
        ),
        "{after}"
    );
}

#[test]
fn an_ambiguous_call_is_no_membership() {
    let (_, _, catalog) = chain("community_queries_ambiguous");
    fs::write(
        catalog.join("lib/ambiguous.ts"),
        "export class Cache {\n  get(key: string) {\n    return key;\n  }\n}\n\n\
         export class Store {\n  get(key: string) {\n    return key;\n  }\n}\n\n\
         export function lookup(source: { get(key: string): string }, key: string) {\n  \
           return source.get(key);\n}\n",
    )
    .unwrap();
    let engine = Engine::open(&catalog).unwrap();

    let cache = engine.context("lib/ambiguous.ts::Cache.get", None).unwrap();
    let lookup = engine.context("lib/ambiguous.ts::lookup", None).unwrap();

    assert!(cache.contains("\nprocesses: none\n"), "{cache}");
    assert!(
        lookup.contains(
            "\nprocesses: 1 (1 export; 1 repository): lookup (example.com/chain/catalog)\n"
        ),
        "{lookup}"
    );
}

#[test]
fn the_processes_line_is_cut_under_the_budget_with_a_count_of_the_rest() {
    let (_, _, catalog) = chain("community_queries_cut");
    let engine = Engine::open(&catalog).unwrap();

    let answer = engine.context("lib/products.ts::find", Some(90)).unwrap();

    assert_golden("chain.context-processes-cut.txt", &answer);
}

#[test]
fn the_processes_line_is_the_summary_alone_when_no_entry_fits() {
    let (_, _, catalog) = chain("community_queries_none_fit");
    let engine = Engine::open(&catalog).unwrap();

    let answer = engine.context("lib/products.ts::find", Some(60)).unwrap();

    assert!(
        answer.contains("\nprocesses: 5 (3 route, 2 export; 3 repositories)\n"),
        "{answer}"
    );
}

#[test]
fn the_answer_stays_within_the_budget_and_keeps_the_summary() {
    let (_, _, catalog) = chain("community_queries_sweep");
    let engine = Engine::open(&catalog).unwrap();

    for budget in [None, Some(90), Some(60), Some(46)] {
        let answer = engine.context("lib/products.ts::find", budget).unwrap();

        let limit = budget.unwrap_or(2_000);
        assert!(answer.len().div_ceil(4) <= limit, "{budget:?}: {answer}");
        let third = answer.lines().nth(2).unwrap_or_default();
        assert!(
            third.starts_with("processes: 5 (3 route, 2 export; 3 repositories)"),
            "{budget:?}: {answer}"
        );
    }
}

#[test]
fn a_long_body_is_cut_before_the_processes_and_the_callers() {
    let (_, _, catalog) = chain("community_queries_span_cut");
    let body: String = (1..=9)
        .map(|n| format!("  const step{n} = find(id + {n});\n"))
        .collect();
    fs::write(
        catalog.join("lib/long.ts"),
        format!(
            "import {{ find }} from \"./products\";\n\n\
             export function long(id: string) {{\n{body}  return find(id);\n}}\n\n\
             export function first(id: string) {{\n  return long(id);\n}}\n\n\
             export function second(id: string) {{\n  return long(id);\n}}\n"
        ),
    )
    .unwrap();
    let engine = Engine::open(&catalog).unwrap();

    let answer = engine
        .context("lib/long.ts::long", Some(SPAN_CUT_BUDGET))
        .unwrap();

    assert_golden("chain.context-processes-span-cut.txt", &answer);
}

#[test]
fn two_processes_of_one_repository_with_the_same_label_show_their_identity() {
    let (_, _, catalog) = chain("community_queries_same_label");
    for file in ["a", "b"] {
        fs::write(
            catalog.join(format!("lib/{file}.ts")),
            "import { shared } from \"./shared\";\n\nexport function handler() {\n  \
               return shared();\n}\n",
        )
        .unwrap();
    }
    fs::write(
        catalog.join("lib/shared.ts"),
        "export function shared() {\n  return 1;\n}\n",
    )
    .unwrap();
    let engine = Engine::open(&catalog).unwrap();

    let answer = engine.context("lib/shared.ts::shared", None).unwrap();

    assert!(
        answer.contains(
            "\nprocesses: 2 (2 export; 1 repository): \
             process::example.com/chain/catalog::lib/a.ts::handler, \
             process::example.com/chain/catalog::lib/b.ts::handler\n"
        ),
        "{answer}"
    );
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
