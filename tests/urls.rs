//! URLs as real code builds them (spec #3, HTTP contract kind): paths in imported constants,
//! client wrappers around `fetch`, Express routers under mount prefixes, inline route
//! handlers, and links Meada can only infer, leave ambiguous or leave out.

mod common;

use std::path::PathBuf;

use common::{assert_golden, fixture_repo};
use meada::engine::Engine;

/// The Next.js provider of `tests/fixtures/http-api`, in the case directory.
fn api(case: &str) -> PathBuf {
    fixture_repo(case, "http-api", Some("https://example.com/acme/api.git"))
}

#[test]
fn constants_fold_into_urls_across_files() {
    let case = "urls_constants";
    let paths = fixture_repo(
        case,
        "http-paths",
        Some("https://example.com/acme/paths.git"),
    );
    api(case);

    let export = Engine::open(&paths).unwrap().export().unwrap();

    assert_golden("http-paths.export.json", &export);
}

/// The Express provider of `tests/fixtures/http-express`, in the case directory.
fn backend(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "http-express",
        Some("https://example.com/acme/backend.git"),
    )
}

/// The client wrapper consumer of `tests/fixtures/http-wrappers`, in the case directory.
fn wrappers(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "http-wrappers",
        Some("https://example.com/acme/admin.git"),
    )
}

#[test]
fn wrapper_call_sites_are_consumers_through_two_levels_and_a_cycle() {
    let case = "urls_wrappers";
    let admin = wrappers(case);
    backend(case);

    let export = Engine::open(&admin).unwrap().export().unwrap();

    assert_golden("http-wrappers.export.json", &export);
}

#[test]
fn express_routes_take_mount_prefixes_through_imports_at_two_depths() {
    let case = "urls_express";
    let backend = backend(case);
    wrappers(case);

    let export = Engine::open(&backend).unwrap().export().unwrap();

    assert_golden("http-express.export.json", &export);
}

#[test]
fn inline_handlers_are_definitions_named_by_method_and_full_path() {
    let engine = Engine::open(&backend("urls_inline")).unwrap();

    assert_golden(
        "http-express.search-inline.txt",
        &engine.search("GET /api/users", None, None).unwrap(),
    );
    assert_golden(
        "http-express.context-inline.txt",
        &engine
            .context("register.GET /api/users/{}#1", None)
            .unwrap(),
    );
}

/// The consumer of `tests/fixtures/http-client`, with both providers next to it.
fn client(case: &str) -> Engine {
    let client = fixture_repo(
        case,
        "http-client",
        Some("https://example.com/acme/client.git"),
    );
    api(case);
    backend(case);
    Engine::open(&client).unwrap()
}

#[test]
fn hostless_urls_link_inferred_to_the_one_repository_serving_them_else_ambiguous() {
    let export = client("urls_hostless").export().unwrap();

    assert_golden("http-client.export.json", &export);
}

#[test]
fn an_unresolved_consumer_shows_in_search_and_context_but_never_links() {
    let engine = client("urls_unresolved");

    assert_golden(
        "http-client.search-unresolved.txt",
        &engine.search("unresolved", Some("contract"), None).unwrap(),
    );
    assert_golden(
        "http-client.context-dynamic.txt",
        &engine.context("dynamic", None).unwrap(),
    );
}

#[test]
fn destructured_parameters_fold_like_positional_ones() {
    let case = "urls_destructured";
    let reports = fixture_repo(
        case,
        "http-destructured",
        Some("https://example.com/acme/portal.git"),
    );

    let export = Engine::open(&reports).unwrap().export().unwrap();

    assert_golden("http-destructured.export.json", &export);
}

#[test]
fn a_parameter_defaulting_to_fetch_is_a_fetch_call() {
    let case = "urls_fetch_param";
    let client = fixture_repo(
        case,
        "http-fetch-param",
        Some("https://example.com/acme/portal.git"),
    );

    let export = Engine::open(&client).unwrap().export().unwrap();

    assert_golden("http-fetch-param.export.json", &export);
}
