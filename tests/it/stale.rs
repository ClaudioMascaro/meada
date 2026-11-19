//! Stale decisions (spec #18, ADR 0006): a decision that no longer fits is ignored by indexing
//! and reported, with its cause, by `meada status` and `meada export`; one that waits on a
//! missing peer is reported apart. Each variant edits the chain after it was indexed, so
//! `status` also shows that it refreshes before it reports. With an index missing, of another
//! version or failing to refresh, one line says the decisions were not evaluated (#934).

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{assert_golden, chain, decisions, without_processes};
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
    assert_golden(
        &format!("chain-web.stale-{name}.export.json"),
        &without_processes(&export),
    );
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

/// The chain indexed with a decision that is stale once evaluated: the web and catalog
/// repositories.
fn indexed_chain(case: &str) -> (PathBuf, PathBuf) {
    let (web, _, catalog) = chain(case);
    decisions(&web, "chain-web-not-candidate");
    Engine::open(&web).unwrap().refresh().unwrap();
    (web, catalog)
}

/// Edits a source of the catalog, so that its next refresh writes.
fn edit_catalog(catalog: &Path) {
    let route = catalog.join("app/api/events/route.ts");
    let src = fs::read_to_string(&route).unwrap();
    fs::write(route, format!("{src}// edited\n")).unwrap();
}

/// The repository lines of the chain's `status` with every index at the current schema, the
/// line that stands for the decisions after them when one fails to refresh.
const INDEXED: &str = "workspace: 4 repositories\n\
                       example.com/chain/web (.) indexed, 5 files\n\
                       example.com/chain/gateway (../chain-gateway) indexed, 8 files\n\
                       example.com/chain/catalog (../chain-catalog) indexed, 4 files\n\
                       example.com/chain/worker (../chain-worker) indexed, 3 files\n\
                       decisions and other workspace lines not evaluated: example.com/chain/catalog: ";

/// A missing index leaves the decisions unevaluated, and one line says so in their place;
/// `status` builds no index (#934).
#[test]
fn status_says_the_decisions_are_not_evaluated_while_a_peer_is_not_indexed() {
    let (web, catalog) = indexed_chain("stale_unindexed");
    fs::remove_dir_all(catalog.join(".meada")).unwrap();

    let status = engine::status(&web).unwrap();

    assert_golden("chain-web.stale-not-indexed.status.txt", &status);
    assert!(!catalog.join(".meada").exists());
}

/// An index of another schema leaves them unevaluated the same way, and `status` does not
/// rebuild it.
#[test]
fn status_says_the_decisions_are_not_evaluated_while_a_peer_is_of_another_version() {
    let (web, catalog) = indexed_chain("stale_other_version");
    let index = rusqlite::Connection::open(catalog.join(".meada/index.db")).unwrap();
    index.pragma_update(None, "user_version", 1).unwrap();

    let status = engine::status(&web).unwrap();

    assert_golden("chain-web.stale-other-version.status.txt", &status);
    let version = index.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0));
    assert_eq!(version.unwrap(), 1);
}

/// An index that fails to refresh (a table gone, a source changed) is named with its error in
/// place of the decisions, below states that all say `indexed`.
#[test]
fn status_names_the_peer_whose_index_fails_to_refresh() {
    let (web, catalog) = indexed_chain("stale_unrefreshed");
    let index = rusqlite::Connection::open(catalog.join(".meada/index.db")).unwrap();
    index.execute_batch("DROP TABLE symbols").unwrap();
    edit_catalog(&catalog);

    let status = engine::status(&web).unwrap();

    assert_eq!(status, format!("{INDEXED}no such table: symbols\n"));
}

/// A read-only index with a source changed, as #934 found it: the refresh cannot write, and the
/// line names the peer. Skipped where permissions do not bind (root).
#[cfg(unix)]
#[test]
fn status_names_the_peer_whose_read_only_index_cannot_be_refreshed() {
    let (web, catalog) = indexed_chain("stale_read_only");
    edit_catalog(&catalog);
    let Some(_locked) = crate::manifest::ReadOnly::lock(&catalog.join(".meada")) else {
        return;
    };

    let status = engine::status(&web).unwrap();

    assert!(status.starts_with(INDEXED), "{status}");
    assert_eq!(status.lines().count(), 6, "{status}");
}
