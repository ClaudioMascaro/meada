//! Open items (spec #18): the ambiguous links and unresolved consumers of a workspace, their
//! sorted candidates, and how `search` lists them.

use crate::common;

use std::fs;

use common::{age, assert_golden, chain, fixture_repo, meada};
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
        .map(|c| format!("{} {}", c.repository, c.label()))
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

/// The issue's repositories (#801): `api` serves `GET /api/orders`, `/orders`, `/v2/orders`,
/// `/items`, `/api/orders/items`, `/api/items/export` and `/api/lineitems`; `go` calls it with
/// bases read in place and with one a method returns. `client` is the client's fixture.
fn unread_base(case: &str, client: &str) -> std::path::PathBuf {
    fixture_repo(
        case,
        "unread-base-api",
        Some("https://example.com/acme/api.git"),
    );
    let origin = format!(
        "https://example.com/acme/{}.git",
        &client["unread-base-".len()..]
    );
    fixture_repo(case, client, Some(&origin))
}

/// Every link of `engine`'s export as `consumer -> contract [reason]`.
fn links_of(engine: &Engine) -> Vec<String> {
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    (export["links"].as_array().unwrap().iter())
        .map(|l| {
            let at = l["consumer"].as_str().unwrap();
            let call = at.split_once("::").map_or(at, |(_, call)| call);
            format!(
                "{call} -> {} [{}]",
                l["contract"].as_str().unwrap(),
                l["reason"].as_str().unwrap()
            )
        })
        .collect()
}

/// Each open item as its call, its limit and its candidates' labels.
fn items_of(engine: &Engine) -> Vec<String> {
    (engine.open_items().unwrap().iter())
        .map(|i| {
            let routes: Vec<String> = i.candidates.iter().map(|c| c.label()).collect();
            let call =
                i.id.split_once("::")
                    .map_or(i.id.as_str(), |(_, call)| call);
            format!("{call}: {}", routes.join(", "))
        })
        .collect()
}

/// A URL whose base is the return value of a call Meada does not evaluate (`c.baseV2()`, which
/// adds `/v2`) links to no route: it is an open item whose candidates are the routes that end
/// with its known part, and it names the limit and the callee. A base read in place still links
/// by its variable, whether a peer names it or not.
#[test]
fn a_base_a_call_returns_makes_an_open_item_that_names_the_callee() {
    let go = unread_base("items_unread_base_call", "unread-base-go");
    let engine = Engine::open(&go).unwrap();

    assert_eq!(
        items_of(&engine),
        [
            "client.go::Client.ViaMethodPrefix::http.Get#0: GET /api/orders, GET /orders, GET /v2/orders"
        ]
    );
    assert_eq!(
        links_of(&engine),
        [
            "client.go::Client.Inline::http.Get#0 -> http::GET::/api/orders [base URL from \
             API_URL, a base_url_env of peer example.com/acme/api]",
            "client.go::Client.InlineOther::http.Get#0 -> http::GET::/v2/orders [base URL from \
             OTHER_URL, no peer's base_url_env; the only repository serving it is \
             example.com/acme/api]",
        ]
    );
    let search = engine.search("", Some("unresolved"), None).unwrap();
    assert!(
        search.contains("candidates: 3; limit: base from the return value of c.baseV2;"),
        "{search}"
    );
    let (listed, _, ok) = meada(&go, &["decide", "--list"], "", |c| {
        c.env("LANG", "en_US.UTF-8");
    });
    assert!(ok);
    assert!(
        listed.contains(
            "client.go:16  http.Get(c.baseV2() + \"/orders\")\n  \
             limit: base from the return value of c.baseV2\n  1) "
        ),
        "{listed}"
    );
}

/// A base that is a parameter no call site binds, or the return value of a method, makes an open
/// item whose candidates end with the known part at a `/`: `/items` takes `GET /items` and `GET
/// /api/orders/items`, not `GET /api/items/export` nor `GET /api/lineitems`. A URL written with
/// no base, and an absolute path resolved over an unknown base, keep the hostless link.
#[test]
fn a_base_a_parameter_holds_makes_an_open_item_and_a_certain_path_still_links() {
    let web = unread_base("items_unread_base_param", "unread-base-web");
    let engine = Engine::open(&web).unwrap();

    assert_eq!(
        items_of(&engine),
        [
            "src/client.ts::OrdersClient.list::fetch#0: GET /api/orders, GET /orders, GET /v2/orders",
            "src/client.ts::byParam::fetch#0: GET /api/orders/items, GET /items",
        ]
    );
    let search = engine.search("", Some("unresolved"), None).unwrap();
    for limit in [
        "limit: base from the parameter base;",
        "limit: base from the return value of this.base;",
    ] {
        assert!(search.contains(limit), "{limit}: {search}");
    }
    let hostless = "[host unresolved; the only repository serving it is example.com/acme/api]";
    assert_eq!(
        links_of(&engine),
        [
            format!("src/client.ts::plain::fetch#0 -> http::GET::/orders {hostless}"),
            format!("src/client.ts::viaUrl::fetch#0 -> http::GET::/orders {hostless}"),
        ]
    );
}

/// A `dynamic-url` decision that names one of the item's candidates applies on the next index.
#[test]
fn a_decision_for_a_candidate_of_an_unread_base_applies() {
    let go = unread_base("items_unread_base_decided", "unread-base-go");
    common::decisions(&go, "unread-base-go");
    let engine = Engine::open(&go).unwrap();

    assert_eq!(items_of(&engine), Vec::<String>::new());
    assert!(
        links_of(&engine).contains(
            &"client.go::Client.ViaMethodPrefix::http.Get#0 -> http::GET::/v2/orders [decided by human]"
                .to_string()
        ),
        "{:?}",
        links_of(&engine)
    );
}

/// Each open item as its call and the limit it names.
fn limits_of(engine: &Engine) -> Vec<String> {
    (engine.open_items().unwrap().iter())
        .map(|i| {
            let call =
                i.id.split_once("::")
                    .map_or(i.id.as_str(), |(_, call)| call);
            let limit = i.limit.as_ref().map(|l| l.describe());
            format!("{call}: {}", limit.as_deref().unwrap_or("none"))
        })
        .collect()
}

/// A Python `httpx` client built with a `base_url` Meada does not read (it comes from another
/// module), and a URL that starts with a call's return value, are open items that name the
/// client's base and the callee; neither links to a guessed route.
#[test]
fn a_python_base_meada_did_not_read_makes_open_items_that_name_it() {
    let python = unread_base("items_unread_base_python", "unread-base-python");
    let engine = Engine::open(&python).unwrap();

    assert_eq!(
        items_of(&engine),
        [
            "client.py::via_call::requests.get#0: GET /api/orders/items, GET /items",
            "client.py::via_client::client.get#0: GET /api/orders, GET /orders, GET /v2/orders",
        ]
    );
    assert_eq!(
        limits_of(&engine),
        [
            "client.py::via_call::requests.get#0: base from the return value of load_base",
            "client.py::via_client::client.get#0: base of a client Meada did not read",
        ]
    );
    assert_eq!(links_of(&engine), Vec::<String>::new());
}

/// Swift: a URL that starts with a call's return value, and a path component appended to a URL
/// Meada did not read, are open items that name the callee and the client's base.
#[test]
fn a_swift_base_meada_did_not_read_makes_open_items_that_name_it() {
    let swift = unread_base("items_unread_base_swift", "unread-base-swift");
    let engine = Engine::open(&swift).unwrap();

    let at = "Sources/Web/Client.swift";
    assert_eq!(
        items_of(&engine),
        [
            format!(
                "{at}::viaCall()::URLSession.shared.data#0: GET /api/orders, GET /orders, GET /v2/orders"
            ),
            format!(
                "{at}::viaComponent()::URLSession.shared.data#0: GET /api/orders/items, GET /items"
            ),
        ]
    );
    assert_eq!(
        limits_of(&engine),
        [
            format!(
                "{at}::viaCall()::URLSession.shared.data#0: base from the return value of loadBase"
            ),
            format!(
                "{at}::viaComponent()::URLSession.shared.data#0: base of a client Meada did not read"
            ),
        ]
    );
    assert_eq!(links_of(&engine), Vec::<String>::new());
}
