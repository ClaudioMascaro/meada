//! `meada decide` in human mode (spec #18, ADR 0006): the binary driven through scripted
//! standard input over the chain fixture's two open items, both in `chain-web`.

mod common;

use std::fs;
use std::path::Path;

use common::{HEALTH, TRACK, assert_golden, chain, written as file};
use meada::engine::Engine;

/// Runs `meada decide <args>` in `repo` under `lang`, feeding it `stdin`. Returns stdout,
/// stderr, and whether it succeeded; the repository's path is replaced by `<repo>`.
fn decide(repo: &Path, lang: &str, args: &[&str], stdin: &str) -> (String, String, bool) {
    let args = [&["decide"], args].concat();
    common::meada(repo, &args, stdin, |c| {
        c.env("LANG", lang);
    })
}

#[test]
fn picking_by_number_writes_the_canonical_file_and_the_next_query_replays_it() {
    let (web, _, _) = chain("decide_pick");

    // The health check is the second candidate (gateway), the tracking URL the first (catalog).
    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &[], "2\n1\n");

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decide.pick.en.txt", &stdout);
    let written = file(&web).expect("the file is written");
    assert_golden("chain-web.decisions.human.toml", &written);
    // A cold index of the chain with the file gives the decided export.
    let (cold, _, _) = chain("decide_pick_cold");
    fs::write(cold.join("meada.decisions.toml"), &written).unwrap();
    let export = Engine::open(&cold).unwrap().export().unwrap();
    assert_golden("chain-web.decided-human.export.json", &export);
}

#[test]
fn skipping_and_end_of_input_leave_the_rest_undecided() {
    let (web, _, _) = chain("decide_skip");

    // `s` and an empty line skip: nothing is picked, so nothing is written.
    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &[], "s\n\n");
    assert!(ok, "{stderr}");
    assert!(stdout.ends_with("nothing decided\n"), "{stdout}");
    assert_eq!(file(&web), None);

    // Input that ends after the first answer ends the run normally, keeping that answer.
    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &[], "2\n");
    assert!(ok, "{stderr}");
    assert!(
        stdout.ends_with("wrote 1 decision to meada.decisions.toml\n"),
        "{stdout}"
    );
    let written = file(&web).unwrap();
    assert!(
        written.contains("health") && !written.contains("track"),
        "{written}"
    );
}

#[test]
fn an_answer_out_of_range_asks_again() {
    let (web, _, _) = chain("decide_retry");

    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &["--item", HEALTH], "9\nx\n1\n");

    assert!(ok);
    assert_eq!(
        stdout
            .matches("enter a number from 1 to 2, or s to skip:")
            .count(),
        2
    );
    assert!(
        file(&web)
            .unwrap()
            .contains("catalog::http::GET::/api/health")
    );
}

#[test]
fn a_new_decision_replaces_the_entry_of_its_call_stale_or_not() {
    let (web, _, _) = chain("decide_replace");
    // A stale decision for the health check (no `ghost` repository provides it), and a
    // decision for the tracking URL that stays as it is.
    let stale = "# a comment, not kept\n\
        [[decision]]\nkind = \"dynamic-url\"\n\
        call = \"example.com/chain/web::src/status.ts::track::fetch#0\"\n\
        target = \"example.com/chain/catalog::http::POST::/api/events\"\n\
        by = \"openrouter/jev\"\np = 0.62\n\n\
        [[decision]]\nkind = \"ambiguous-link\"\n\
        call = \"example.com/chain/web::src/status.ts::health::fetch#0\"\n\
        target = \"example.com/chain/ghost::http::GET::/api/health\"\nby = \"human\"\n";
    fs::write(web.join("meada.decisions.toml"), stale).unwrap();

    let (_, stderr, ok) = decide(&web, "en_US.UTF-8", &["--item", HEALTH], "2\n");

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decisions.replaced.toml", &file(&web).unwrap());
}

#[test]
fn a_stale_entry_for_another_call_survives_a_rewrite() {
    let (web, _, _) = chain("decide_survive");
    let gone = "example.com/chain/web::src/gone.ts::run::fetch#0";
    let stale = format!(
        "[[decision]]\nkind = \"dynamic-url\"\ncall = \"{gone}\"\n\
         target = \"example.com/chain/ghost::http::GET::/gone\"\nby = \"human\"\n"
    );
    fs::write(web.join("meada.decisions.toml"), stale).unwrap();

    let (_, stderr, ok) = decide(&web, "en_US.UTF-8", &["--item", HEALTH], "2\n");

    assert!(ok, "{stderr}");
    let written = file(&web).unwrap();
    assert!(
        written.contains(gone) && written.contains(HEALTH),
        "{written}"
    );
}

#[test]
fn a_repeated_call_in_the_file_is_a_parse_error_naming_the_repeat() {
    let (web, _, _) = chain("decide_repeat");
    let entry = |target: &str| {
        format!(
            "[[decision]]\nkind = \"dynamic-url\"\ncall = \"{HEALTH}\"\n\
             target = \"{target}\"\nby = \"human\"\n\n"
        )
    };
    let text = format!("{}{}", entry("a::http::GET::/a"), entry("b::http::GET::/b"));
    fs::write(web.join("meada.decisions.toml"), text).unwrap();

    let (_, stderr, ok) = decide(&web, "en_US.UTF-8", &["--list"], "");

    assert!(!ok);
    assert_eq!(
        stderr,
        format!(
            "meada: <repo>/meada.decisions.toml:9: `call`: `{HEALTH}` is decided twice, \
             one entry per call\n"
        )
    );
}

#[test]
fn item_limits_the_run_and_an_unknown_id_is_an_error() {
    let (web, _, _) = chain("decide_item");

    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &["--item", HEALTH], "2\n");
    assert!(ok);
    assert!(!stdout.contains("track"), "{stdout}");

    let (_, stderr, ok) = decide(&web, "en_US.UTF-8", &["--item", "nope"], "");
    assert!(!ok);
    assert_eq!(
        stderr,
        "meada: `nope` is not an open or decided item of this repository\n"
    );
}

#[test]
fn dry_run_prints_the_file_and_writes_nothing() {
    let (web, _, _) = chain("decide_dry");

    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &["--dry-run"], "2\n1\n");

    assert!(ok);
    assert!(
        stdout.contains("dry run, meada.decisions.toml would be:\n[[decision]]\n"),
        "{stdout}"
    );
    assert!(stdout.ends_with("by = \"human\"\n"), "{stdout}");
    assert_eq!(file(&web), None);
}

#[test]
fn list_prints_the_open_items_and_neither_asks_nor_writes() {
    let (web, _, _) = chain("decide_list");

    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &["--list"], "2\n1\n");

    assert!(ok);
    assert_golden("chain-web.decide.list.en.txt", &stdout);
    assert_eq!(file(&web), None);
}

#[test]
fn decided_items_are_not_open_and_a_run_without_open_items_writes_nothing() {
    let (web, _, _) = chain("decide_none");
    common::decisions(&web, "chain-web");
    let before = file(&web);

    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &[], "1\n");

    assert!(ok);
    assert_eq!(stdout, "no open items\n");
    assert_eq!(file(&web), before);
    let engine = Engine::open(&web).unwrap();
    for kind in ["ambiguous", "unresolved"] {
        let answer = engine.search("", Some(kind), None).unwrap();
        assert_eq!(answer, "no matches for ``\n", "{kind}");
    }
}

#[test]
fn item_redoes_a_decided_call_from_the_core_view_and_the_new_pick_replaces_its_entry() {
    let (web, _, _) = chain("decide_redo");
    // Health decided for the gateway by a person, tracking for the catalog by a model.
    common::decisions(&web, "chain-web");

    // The decided health check is offered with its candidates as if undecided.
    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &["--item", HEALTH], "1\n");
    assert!(ok, "{stderr}");
    assert!(
        stdout.starts_with(&format!(
            "{HEALTH}\nsrc/status.ts:3  fetch(\"/api/health\")\n\
             \x20 1) example.com/chain/catalog GET /api/health\n\
             \x20 2) example.com/chain/gateway GET /api/health\n"
        )),
        "{stdout}"
    );
    // The decided tracking URL too; the person's pick replaces the model's entry.
    let (_, stderr, ok) = decide(&web, "en_US.UTF-8", &["--item", TRACK], "2\n");
    assert!(ok, "{stderr}");
    assert_eq!(
        file(&web).unwrap(),
        format!(
            "[[decision]]\nkind = \"ambiguous-link\"\ncall = \"{HEALTH}\"\n\
             target = \"example.com/chain/catalog::http::GET::/api/health\"\nby = \"human\"\n\n\
             [[decision]]\nkind = \"dynamic-url\"\ncall = \"{TRACK}\"\n\
             target = \"example.com/chain/gateway::http::POST::/api/events\"\nby = \"human\"\n"
        )
    );

    // Without `--item`, decided calls stay closed (`search` too, see the test below).
    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &[], "");
    assert!(ok);
    assert_eq!(stdout, "no open items\n");
}

#[test]
fn messages_follow_lang() {
    let (web, _, _) = chain("decide_pt");

    let (stdout, _, ok) = decide(&web, "pt_BR.UTF-8", &[], "2\n1\n");

    assert!(ok);
    assert_golden("chain-web.decide.pick.pt.txt", &stdout);
}

/// Names `example.com/chain/billing`, which the chain does not have, as a peer of `web`.
fn add_missing_peer(web: &Path) {
    let toml = web.join("meada.toml");
    let mut src = fs::read_to_string(&toml).unwrap();
    src.push_str("\n[[peer]]\nrepo = \"example.com/chain/billing\"\n");
    fs::write(toml, src).unwrap();
}

#[test]
fn list_prints_the_stale_and_waiting_decisions_after_the_open_items() {
    let (web, _, _) = chain("decide_list_stale");
    common::decisions(&web, "chain-web-stale-and-waiting");
    add_missing_peer(&web);

    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &["--list"], "");

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decide.list-stale.en.txt", &stdout);
}

#[test]
fn list_prints_the_stale_decisions_even_when_no_item_is_open() {
    let (web, _, catalog) = chain("decide_list_resolved");
    common::decisions(&web, "chain-web");
    fs::remove_file(catalog.join("app/api/health/route.ts")).unwrap();

    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &["--list"], "");

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decide.list-resolved.en.txt", &stdout);
}

#[test]
fn prune_removes_exactly_the_stale_entries_and_keeps_those_waiting() {
    let (web, _, _) = chain("decide_prune");
    common::decisions(&web, "chain-web-stale-and-waiting");
    add_missing_peer(&web);

    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &["--prune"], "");

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decide.prune.en.txt", &stdout);
    assert_golden("chain-web.decisions.pruned.toml", &file(&web).unwrap());

    // Nothing is stale any more: a second run leaves the file as it is.
    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &["--prune"], "");
    assert!(ok);
    assert_eq!(stdout, "nothing to prune\n");
    assert_golden("chain-web.decisions.pruned.toml", &file(&web).unwrap());
}

#[test]
fn prune_reports_in_portuguese_how_many_entries_it_removed() {
    let (web, _, _) = chain("decide_prune_pt");
    common::decisions(&web, "chain-web-stale-and-waiting");
    add_missing_peer(&web);

    let (stdout, _, ok) = decide(&web, "pt_BR.UTF-8", &["--prune"], "");

    assert!(ok);
    assert_eq!(
        stdout,
        "1 decisão obsoleta removida de meada.decisions.toml\n"
    );
}

#[test]
fn prune_with_no_file_writes_none() {
    let (web, _, _) = chain("decide_prune_none");

    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &["--prune"], "");

    assert!(ok);
    assert_eq!(stdout, "nothing to prune\n");
    assert_eq!(file(&web), None);
}

#[test]
fn prune_does_not_combine_with_the_other_flags() {
    let (web, _, _) = chain("decide_prune_flags");
    for other in ["--list", "--dry-run", "--item=x"] {
        let (_, stderr, ok) = decide(&web, "en_US.UTF-8", &["--prune", other], "");
        assert!(!ok && stderr.contains("cannot be used"), "{stderr}");
    }
}
