//! `trace` over the chain fixture (spec #3): `chain-web` (A) consumes from `chain-gateway` (B),
//! an Express gateway that consumes from `chain-catalog` (C), a Next.js app. Between them sit a
//! client wrapper, imported path constants, a base URL variable, a mount prefix, an absolute
//! URL on a declared host, a path both B and C serve, an unresolved URL and a cycle in C.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};

/// The three repositories as siblings in one case directory: (web, gateway, catalog).
fn chain(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    (
        fixture_repo(case, "chain-web", Some("https://example.com/chain/web.git")),
        fixture_repo(
            case,
            "chain-gateway",
            Some("git@example.com:chain/gateway.git"),
        ),
        fixture_repo(
            case,
            "chain-catalog",
            Some("https://example.com/chain/catalog.git"),
        ),
    )
}

#[test]
fn trace_down_follows_calls_and_links_across_three_repositories() {
    let (web, _, _) = chain("trace_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("dashboard"), Direction::Down, None)
        .unwrap();

    assert_golden("chain.trace-down.txt", &answer);
}

#[test]
fn trace_up_from_a_route_reaches_every_consumer_upstream() {
    let (_, _, catalog) = chain("trace_up");

    let answer = Engine::open(&catalog)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/products/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("chain.trace-up.txt", &answer);
}

#[test]
fn trace_from_uncommitted_changes_starts_at_edited_and_untracked_symbols() {
    let (_, _, catalog) = chain("trace_uncommitted");
    let products = catalog.join("lib/products.ts");
    let edited = fs::read_to_string(&products)
        .unwrap()
        .replace("return { id };", "return { id, found: true };");
    fs::write(&products, edited).unwrap();
    fs::write(
        catalog.join("lib/cache.ts"),
        "export function warm() {\n  return new Map();\n}\n",
    )
    .unwrap();

    let answer = Engine::open(&catalog)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("chain.trace-uncommitted.txt", &answer);
}

#[test]
fn a_trace_over_budget_says_where_it_stopped_and_keeps_the_footer() {
    let (web, _, _) = chain("trace_cut");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("dashboard"), Direction::Down, Some(120))
        .unwrap();

    assert_golden("chain.trace-cut.txt", &answer);
    let tokens = answer.len().div_ceil(4);
    assert!(tokens <= 120, "{tokens} tokens");
}

#[test]
fn a_missing_peer_closes_the_footer_and_the_rest_of_the_chain_answers() {
    let case = "trace_missing";
    let web = fixture_repo(case, "chain-web", Some("https://example.com/chain/web.git"));
    fixture_repo(
        case,
        "chain-gateway",
        Some("git@example.com:chain/gateway.git"),
    );

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("dashboard"), Direction::Down, None)
        .unwrap();

    assert_golden("chain.trace-missing-peer.txt", &answer);
}

#[test]
fn uncommitted_changes_include_a_file_whose_name_has_a_space() {
    let (_, _, catalog) = chain("trace_uncommitted_space");
    let stock = catalog.join("lib/old stock.ts");
    fs::write(&stock, "export function stock() {\n  return 1;\n}\n").unwrap();
    common::git(&catalog, &["add", "-A"]);
    common::git(&catalog, &["commit", "-q", "-m", "stock"]);
    fs::write(&stock, "export function stock() {\n  return 2;\n}\n").unwrap();

    let answer = Engine::open(&catalog)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert!(
        answer.starts_with("example.com/chain/catalog lib/old stock.ts:1 stock\nchain: none"),
        "{answer}"
    );
}

#[test]
fn in_a_repository_with_no_commits_every_file_is_uncommitted() {
    let repo = Path::new(env!("CARGO_TARGET_TMPDIR")).join("trace_unborn/web");
    let _ = fs::remove_dir_all(&repo);
    fs::create_dir_all(repo.join("src")).unwrap();
    common::git(&repo, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let a = "export function a() {\n  return 1;\n}\n";
    fs::write(repo.join("src/a.ts"), a).unwrap();
    fs::write(repo.join("src/b.ts"), a.replace('a', "b")).unwrap();
    // One file staged, one not: neither is in a commit.
    common::git(&repo, &["add", "src/a.ts"]);

    let answer = Engine::open(&repo)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Down, None)
        .unwrap();

    assert!(
        answer.starts_with("local/web src/a.ts:1 a\nlocal/web src/b.ts:1 b\nchain: none"),
        "{answer}"
    );
}
