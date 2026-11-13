//! Routes a test registers (#620): a test's router or fake upstream is no provider of its
//! repository, `test_routes = true` in `meada.toml` keeps them, and the test code stays indexed.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::fixture_repo;
use meada::engine::Engine;
use serde_json::Value;

/// The client `web` and its peer `api`, siblings: both register routes in test files.
fn pair(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(
            case,
            "test-routes-web",
            Some("https://example.com/test-routes/web.git"),
        ),
        fixture_repo(
            case,
            "test-routes-api",
            Some("https://example.com/test-routes/api.git"),
        ),
    )
}

fn export(repo: &Path) -> Value {
    serde_json::from_str(&Engine::open(repo).unwrap().export().unwrap()).unwrap()
}

/// The `site`s of the HTTP providers of an export.
fn routes(export: &Value) -> Vec<&str> {
    let providers = export["providers"].as_array().unwrap();
    let http = providers
        .iter()
        .filter(|p| p["contract"].as_str().unwrap().starts_with("http::"));
    http.map(|p| p["site"].as_str().unwrap()).collect()
}

#[test]
fn routes_a_test_registers_are_no_providers_and_the_calls_reach_only_the_real_server() {
    let (web, api) = pair("test_routes_default");

    let (web_export, api_export) = (export(&web), export(&api));

    assert_eq!(routes(&web_export), Vec::<&str>::new());
    assert_eq!(
        routes(&api_export),
        ["example.com/test-routes/api::main.go::main.GET /orders"]
    );
    let links = web_export["links"].as_array().unwrap();
    assert_eq!(links.len(), 1);
    for link in links {
        assert_eq!(
            link["provider"],
            "example.com/test-routes/api::main.go::main.GET /orders"
        );
        assert_eq!(link["level"], "inferred");
        assert!(
            link["reason"]
                .as_str()
                .unwrap()
                .contains("the only repository serving it")
        );
    }
    // The Go client joins a base only its test sets (`c.base`) with the path: an open item whose
    // one candidate is the real server, never the test's fake upstream (#801).
    let items = Engine::open(&web).unwrap().open_items().unwrap();
    let candidates: Vec<&str> = (items.iter())
        .flat_map(|i| i.candidates.iter().map(|c| c.site.as_str()))
        .collect();
    assert_eq!(
        candidates,
        ["example.com/test-routes/api::main.go::main.GET /orders"]
    );
    // No entry point or process of a route a test registers either.
    let entries = |e: &Value| -> Vec<String> {
        let processes = e["processes"].as_array().into_iter().flatten();
        processes
            .filter(|p| p["kind"] == "route")
            .map(|p| p["entry_point"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(entries(&web_export), Vec::<String>::new());
    assert_eq!(
        entries(&api_export),
        ["example.com/test-routes/api::main.go::main.GET /orders::http::GET::/orders"]
    );
}

#[test]
fn test_code_is_still_indexed() {
    let (web, _) = pair("test_routes_indexed");

    let found = Engine::open(&web)
        .unwrap()
        .search("stubUpstream", None, None)
        .unwrap();

    assert!(found.contains("client/client_test.go"), "{found}");
    assert!(found.contains("stubUpstream"), "{found}");
}

#[test]
fn test_routes_keeps_them_as_providers() {
    let (web, api) = pair("test_routes_kept");
    fs::write(api.join("meada.toml"), "test_routes = true\n").unwrap();
    fs::write(
        web.join("meada.toml"),
        "test_routes = true\n\n[[peer]]\nrepo = \"example.com/test-routes/api\"\n",
    )
    .unwrap();

    let (web_export, api_export) = (export(&web), export(&api));

    assert_eq!(
        routes(&web_export),
        [
            "example.com/test-routes/web::client/client_test.go::stubUpstream.GET /orders",
            "example.com/test-routes/web::src/orders.test.ts::buildApp.GET /orders",
        ]
    );
    assert_eq!(
        routes(&api_export),
        [
            "example.com/test-routes/api::main.go::main.GET /orders",
            "example.com/test-routes/api::main_test.go::TestMain.GET /test",
        ]
    );
}
