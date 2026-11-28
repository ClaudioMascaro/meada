use crate::common;

use common::{assert_golden, fixture_repo};
use meada::engine::Engine;

fn web(case: &str) -> Engine {
    let repo = fixture_repo(case, "ts-basic", Some("https://example.com/acme/web.git"));
    Engine::open(&repo).unwrap()
}

#[test]
fn search_ranks_exact_then_prefix_then_substring() {
    let answer = web("search_ranking").search("A", None, None).unwrap();

    assert_golden("ts-basic.search-a.txt", &answer);
}

#[test]
fn search_matches_the_last_segment_of_a_nested_name() {
    let answer = web("search_nested").search("inner", None, None).unwrap();

    assert_golden("ts-basic.search-inner.txt", &answer);
}

#[test]
fn search_filters_by_kind() {
    let engine = web("search_kind");

    assert_eq!(
        engine.search("add", Some("route"), None).unwrap(),
        "no matches for `add`\n"
    );
    assert!(
        engine
            .search("add", Some("symbol"), None)
            .unwrap()
            .contains("add")
    );
}

#[test]
fn context_lists_span_callers_and_callees() {
    let engine = web("context_add");

    assert_golden(
        "ts-basic.context-add.txt",
        &engine.context("add", None).unwrap(),
    );
}

#[test]
fn context_shows_unresolved_callees_and_top_level_callers() {
    let engine = web("context_unresolved");

    assert_golden(
        "ts-basic.context-label.txt",
        &engine.context("src/view.tsx::label", None).unwrap(),
    );
    assert_golden(
        "ts-basic.context-outer.txt",
        &engine.context("outer", None).unwrap(),
    );
}

#[test]
fn context_lists_candidates_instead_of_guessing() {
    let repo = fixture_repo(
        "context_ambiguous",
        "ts-basic",
        Some("https://example.com/acme/web.git"),
    );
    std::fs::write(repo.join("src/other.ts"), "function add() {}\n").unwrap();
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "ts-basic.context-ambiguous.txt",
        &engine.context("add", None).unwrap(),
    );
    assert_golden(
        "ts-basic.context-none.txt",
        &engine.context("nothing", None).unwrap(),
    );
}

#[test]
fn context_lists_every_candidate_of_an_ambiguous_call() {
    let repo = fixture_repo(
        "context_ambiguous_call",
        "conformance-ts-js",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "conformance-ts-js.context-main.txt",
        &engine.context("main", None).unwrap(),
    );
    assert_golden(
        "conformance-ts-js.context-render.txt",
        &engine.context("src/widget.jsx::render", None).unwrap(),
    );
}

#[test]
fn python_functions_are_found_by_name_and_list_callers_and_callees() {
    let repo = fixture_repo(
        "context_python",
        "conformance-python",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "conformance-python.search-scale.txt",
        &engine.search("scale", None, None).unwrap(),
    );
    assert_golden(
        "conformance-python.context-scale.txt",
        &engine.context("scale", None).unwrap(),
    );
    assert_golden(
        "conformance-python.context-render.txt",
        &engine.context("Panel.render", None).unwrap(),
    );
}

#[test]
fn rust_items_are_found_by_name_and_list_callers_and_callees() {
    let repo = fixture_repo(
        "context_rust",
        "conformance-rust",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "conformance-rust.search-invoice.txt",
        &engine.search("Invoice", None, None).unwrap(),
    );
    assert_golden(
        "conformance-rust.context-serve.txt",
        &engine.context("serve", None).unwrap(),
    );
    assert_golden(
        "conformance-rust.context-bump.txt",
        &engine.context("Counter.bump", None).unwrap(),
    );
    assert_golden(
        "conformance-rust.context-reset.txt",
        &engine.context("Counter.reset", None).unwrap(),
    );
    assert_golden(
        "conformance-rust.context-unresolved.txt",
        &engine.context("unresolved", None).unwrap(),
    );
}

#[test]
fn go_functions_are_found_by_name_and_list_callers_and_callees() {
    let repo = fixture_repo(
        "context_go",
        "conformance-go",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "conformance-go.search-new.txt",
        &engine.search("New", None, None).unwrap(),
    );
    assert_golden(
        "conformance-go.context-new.txt",
        &engine.context("store/store.go::New", None).unwrap(),
    );
    assert_golden(
        "conformance-go.context-bump.txt",
        &engine
            .context("methods/types.go::Counter.Bump", None)
            .unwrap(),
    );
    assert_golden(
        "conformance-go.context-unresolved.txt",
        &engine.context("client/net.go::Unresolved", None).unwrap(),
    );
}

#[test]
fn rust_programs_are_found_as_processes() {
    let repo = fixture_repo(
        "search_rust_programs",
        "conformance-rust",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "conformance-rust.search-process-main.txt",
        &engine.search("main", Some("process"), None).unwrap(),
    );
}

#[test]
fn java_programs_are_found_as_processes() {
    let repo = fixture_repo(
        "search_java_programs",
        "conformance-java",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "conformance-java.search-process-main.txt",
        &engine.search("main", Some("process"), None).unwrap(),
    );
}

#[test]
fn kotlin_programs_are_found_as_processes() {
    let repo = fixture_repo(
        "search_kotlin_programs",
        "conformance-kotlin",
        Some("https://example.com/acme/conformance.git"),
    );
    assert_golden(
        "conformance-kotlin.search-process-main.txt",
        &Engine::open(&repo)
            .unwrap()
            .search("main", Some("process"), None)
            .unwrap(),
    );
}

#[test]
fn go_programs_are_found_as_processes() {
    let repo = fixture_repo(
        "context_go_programs",
        "conformance-go",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "conformance-go.search-process-main.txt",
        &engine.search("main", Some("process"), None).unwrap(),
    );
}

#[test]
fn python_programs_are_found_as_processes_and_shown_with_their_range() {
    let repo = fixture_repo(
        "context_python_programs",
        "conformance-python",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    assert_golden(
        "conformance-python.search-process-main.txt",
        &engine.search("__main__", Some("process"), None).unwrap(),
    );
    assert_golden(
        "conformance-python.search-process-script.txt",
        &engine.search("build", Some("process"), None).unwrap(),
    );
    assert_golden(
        "conformance-python.context-main.txt",
        &engine.context("jobs/sync.py::__main__", None).unwrap(),
    );
    assert_golden(
        "conformance-python.context-main-single.txt",
        &engine.context("jobs/single.py::__main__", None).unwrap(),
    );
    assert_golden(
        "conformance-python.context-decorated.txt",
        &engine.context("web/api.py::health", None).unwrap(),
    );
}

#[test]
fn answers_over_budget_say_where_they_stopped() {
    let engine = web("budget_cut");

    assert_golden(
        "ts-basic.search-cut.txt",
        &engine.search("a", None, Some(30)).unwrap(),
    );
}
