//! Stale decisions (spec #18, ADR 0006): a decision that no longer fits is ignored by indexing
//! and reported, with its cause, by `meada status` and `meada export`; one that waits on a
//! missing peer is reported apart. Each variant edits the chain after it was indexed, so
//! `status` also shows that it refreshes before it reports.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{assert_golden, chain, decisions};
use meada::engine::{self, Engine};

type Chain = (PathBuf, PathBuf, PathBuf);

/// The chain with the decisions `variant`, indexed, then edited by `edit`: the golden `status`
/// and `export` (of the web repository) `name` names.
fn stale(case: &str, variant: &str, name: &str, edit: impl Fn(&Chain)) {
    let repos = chain(case);
    decisions(&repos.0, variant);
    Engine::open(&repos.0).unwrap().refresh().unwrap();
    edit(&repos);

    let status = engine::status(&repos.0).unwrap();
    let export = Engine::open(&repos.0).unwrap().export().unwrap();

    assert_golden(&format!("chain-web.stale-{name}.status.txt"), &status);
    assert_golden(&format!("chain-web.stale-{name}.export.json"), &export);
}

#[test]
fn a_decision_whose_call_site_was_deleted_is_stale() {
    stale("stale_call", "chain-web", "call-gone", |(web, _, _)| {
        fs::remove_file(web.join("src/status.ts")).unwrap();
    });
}

#[test]
fn a_decision_whose_target_route_was_removed_is_stale() {
    stale(
        "stale_target",
        "chain-web",
        "target-gone",
        |(_, _, catalog)| {
            fs::remove_file(catalog.join("app/api/events/route.ts")).unwrap();
        },
    );
}

#[test]
fn a_decision_whose_target_is_no_longer_a_candidate_is_stale() {
    stale(
        "stale_candidate",
        "chain-web-not-candidate",
        "not-candidate",
        |_| {},
    );
}

#[test]
fn a_decision_for_a_call_the_core_now_resolves_is_stale_and_the_core_link_stands() {
    stale(
        "stale_resolved",
        "chain-web",
        "resolved",
        |(_, _, catalog)| {
            fs::remove_file(catalog.join("app/api/health/route.ts")).unwrap();
        },
    );
}

#[test]
fn a_decision_on_a_missing_peer_waits_and_is_neither_applied_nor_stale() {
    stale(
        "stale_peer",
        "chain-web-missing-peer",
        "missing-peer",
        |(web, _, _)| {
            let toml = web.join("meada.toml");
            let mut src = fs::read_to_string(&toml).unwrap();
            src.push_str("\n[[peer]]\nrepo = \"example.com/chain/billing\"\n");
            fs::write(toml, src).unwrap();
        },
    );
}

#[test]
fn status_reports_no_stale_decision_while_an_index_is_missing() {
    let (web, _, _) = chain("stale_unindexed");
    decisions(&web, "chain-web-not-candidate");

    assert_golden(
        "chain-web.stale-not-indexed.status.txt",
        &engine::status(&web).unwrap(),
    );
}
