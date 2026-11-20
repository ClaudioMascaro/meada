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

#[test]
fn replay_reports_kind_identity_and_candidate_failures_separately() {
    let (web, _, _) = chain("stale_distinct_causes");
    use common::{HEALTH, TRACK};
    common::age(&web);
    fs::write(
        web.join("src/diagnose.ts"),
        r#"
export function WrongTarget(url: string) { return fetch(url); }
export function NoCandidates(url: string) { return fetch(url, { method: "TRACE" }); }
export function Unknown(url: string) { return fetch(url); }
export function Malformed(url: string) { return fetch(url); }
export function Scheme(url: string) { return fetch(url); }
export function Suffix(url: string) { return fetch(url); }
export function Gone(url: string) { return fetch(url); }
export function Waiting(url: string) { return fetch(url); }
"#,
    )
    .unwrap();
    let config = web.join("meada.toml");
    let peers =
        fs::read_to_string(&config).unwrap() + "\n[[peer]]\nrepo = \"example.com/chain/ghost\"\n";
    fs::write(config, peers).unwrap();
    let diagnose = |name| format!("example.com/chain/web::src/diagnose.ts::{name}::fetch#0");
    let router = "example.com/chain/catalog";
    let route = "http::GET::/api/health";
    let cases = [
        (
            HEALTH.to_owned(),
            "dynamic-url",
            format!("{router}::{route}"),
            "wrong-kind",
        ),
        (
            TRACK.to_owned(),
            "ambiguous-link",
            format!("{router}::{route}"),
            "wrong-kind",
        ),
        (
            diagnose("WrongTarget"),
            "dynamic-url",
            "example.com/chain/gateway::http::POST::/api/events".into(),
            "not-candidate",
        ),
        (
            diagnose("NoCandidates"),
            "dynamic-url",
            format!("{router}::{route}"),
            "no-candidates",
        ),
        (
            diagnose("Unknown"),
            "dynamic-url",
            format!("short::{route}"),
            "unknown-repository",
        ),
        (
            diagnose("Malformed"),
            "dynamic-url",
            "short".into(),
            "unknown-repository",
        ),
        (
            diagnose("Scheme"),
            "dynamic-url",
            format!("https://{router}::{route}"),
            "unknown-repository",
        ),
        (
            diagnose("Suffix"),
            "dynamic-url",
            format!("{router}.git::{route}"),
            "unknown-repository",
        ),
        (
            diagnose("Gone"),
            "dynamic-url",
            format!("{router}::http::GET::/gone"),
            "target-gone",
        ),
        (
            diagnose("Waiting"),
            "dynamic-url",
            format!("example.com/chain/ghost::{route}"),
            "",
        ),
    ];
    let decisions = (cases.iter())
        .map(|(call, kind, target, _)| format!("[[decision]]\nkind = {kind:?}\ncall = {call:?}\ntarget = {target:?}\nby = \"human\"\n\n"))
        .collect::<String>();
    let file = web.join("meada.decisions.toml");
    fs::write(&file, decisions).unwrap();
    let engine = Engine::open(&web).unwrap();
    let json: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let rows = json["stale_decisions"].as_array().unwrap();
    assert_eq!(rows.len(), cases.len() - 1, "{rows:?}");
    for (row, (call, _, target, cause)) in rows.iter().zip(&cases) {
        assert_eq!(row["call"], *call);
        assert_eq!(row["target"], *target);
        assert_eq!(row["cause"], *cause, "{row:?}");
    }
    let run = |args: &[&str]| {
        let (out, error, ok) = common::meada(&web, args, "", |c| {
            c.env("LANG", "en_US.UTF-8");
        });
        assert!(ok && error.is_empty(), "{error}");
        out
    };
    for out in [run(&["status"]), run(&["decide", "--list"])] {
        for diagnostic in [
            "target no longer a candidate",
            "item has no candidates for a decision",
            "unknown repository identity `short`",
            "target gone",
            "Use kind = \"ambiguous-link\"",
            "Use kind = \"dynamic-url\"",
            "decision waiting on missing peer example.com/chain/ghost",
        ] {
            assert!(out.contains(diagnostic), "{diagnostic}: {out}");
        }
    }
    assert!(
        json["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|link| link["level"] != "decided")
    );
    assert_eq!(
        run(&["decide", "--prune"]),
        "pruned 2 stale decisions from meada.decisions.toml\n"
    );
    let kept = fs::read_to_string(&file).unwrap();
    for (call, _, _, cause) in &cases {
        let preserve = !matches!(*cause, "target-gone" | "not-candidate");
        assert_eq!(kept.contains(call), preserve, "{call}: {kept}");
    }
}
