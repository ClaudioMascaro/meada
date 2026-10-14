//! Open items (spec #18): the ambiguous links and unresolved consumers of a workspace, their
//! sorted candidates, and how `search` lists them.

mod common;

use std::fs;

use common::{age, assert_golden, chain, fixture_repo};
use meada::engine::Engine;

fn few(case: &str) -> Engine {
    let repo = fixture_repo(case, "open-few", Some("https://example.com/open/few.git"));
    Engine::open(&repo).unwrap()
}

fn generated(case: &str) -> Engine {
    let origin = Some("https://example.com/open/generated.git");
    Engine::open(&fixture_repo(case, "open-generated", origin)).unwrap()
}

#[test]
fn search_lists_the_ambiguous_links_of_the_chain_with_a_decide_hint() {
    let (web, _, _) = chain("items_ambiguous");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "chain.search-ambiguous.txt",
        &engine.search("", Some("ambiguous"), None).unwrap(),
    );
}

#[test]
fn search_lists_the_unresolved_consumers_of_the_chain() {
    let (web, _, _) = chain("items_unresolved");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "chain.search-unresolved.txt",
        &engine.search("", Some("unresolved"), None).unwrap(),
    );
}

#[test]
fn search_matches_the_enclosing_symbol_and_the_call_expression() {
    let (web, _, _) = chain("items_query");
    let engine = Engine::open(&web).unwrap();

    // The enclosing symbol is `track`; the call expression holds `event.url`.
    for query in ["track", "event.url"] {
        let answer = engine.search(query, Some("unresolved"), None).unwrap();
        assert!(answer.contains("src/status.ts:8"), "{query}: {answer}");
    }
    assert_eq!(
        engine.search("track", Some("ambiguous"), None).unwrap(),
        "no matches for `track`\n"
    );
}

#[test]
fn a_dynamic_url_lists_the_routes_its_method_and_fragments_leave() {
    let engine = few("items_few");

    let items = engine.open_items().unwrap();

    assert_eq!(items.len(), 1);
    let routes: Vec<String> = (items[0].candidates.iter())
        .map(|c| format!("{} {} {}", c.repository, c.method(), c.path()))
        .collect();
    // The DELETE and POST routes, `/api/users` and `/api/health` are filtered out.
    assert_eq!(
        routes,
        [
            "example.com/open/few GET /api/reports/summary",
            "example.com/open/few GET /api/reports/{}",
            "example.com/open/few GET /api/reports/{}/download",
        ]
    );
    assert_golden(
        "open-few.search-unresolved.txt",
        &engine.search("report", Some("unresolved"), None).unwrap(),
    );
}

#[test]
fn generated_routes_leave_a_dynamic_url_more_than_fifty_candidates() {
    let engine = generated("items_generated");

    let items = engine.open_items().unwrap();

    assert_eq!(items[0].candidates.len(), 55);
    assert_golden(
        "open-generated.search-unresolved.txt",
        &engine.search("", Some("unresolved"), None).unwrap(),
    );
}

#[test]
fn ambiguous_candidates_are_the_listed_providers_sorted_by_repository() {
    let (web, _, _) = chain("items_order");
    let items = Engine::open(&web).unwrap().open_items().unwrap();

    // Item ids are call-site identities, handled in id order.
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "example.com/chain/web::src/status.ts::health::fetch#0",
            "example.com/chain/web::src/status.ts::track::fetch#0",
        ]
    );
    let providers: Vec<&str> = (items[0].candidates.iter())
        .map(|c| c.site.as_str())
        .collect();
    assert_eq!(
        providers,
        [
            "example.com/chain/catalog::app/api/health/route.ts::GET",
            "example.com/chain/gateway::src/server.ts::GET /api/health",
        ]
    );
}

#[test]
fn a_refreshed_index_lists_the_open_items_a_cold_index_does() {
    // The method and fragments of an unresolved consumer are not in the export, so this is
    // where a refresh that loses them shows.
    let render = |engine: &Engine| {
        let mut out = engine.search("", Some("unresolved"), None).unwrap();
        for item in engine.open_items().unwrap() {
            out += &format!("{} {:?}\n", item.id, item.candidates);
        }
        out
    };
    let edit = |repo: &std::path::Path| {
        let latest = repo.join("app/api/reports/latest");
        fs::create_dir_all(&latest).unwrap();
        fs::write(
            latest.join("route.ts"),
            "export function GET() {\n  return new Response(\"latest\");\n}\n",
        )
        .unwrap();
        fs::write(repo.join("app/api/users/route.ts"), "export const x = 1;\n").unwrap();
    };
    let origin = Some("https://example.com/open/few.git");
    let repo = fixture_repo("items_fresh", "open-few", origin);
    age(&repo);
    let before = render(&Engine::open(&repo).unwrap());
    edit(&repo);

    let refreshed = render(&Engine::open(&repo).unwrap());

    let cold = fixture_repo("items_fresh_cold", "open-few", origin);
    edit(&cold);
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, render(&Engine::open(&cold).unwrap()));
}
