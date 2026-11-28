use crate::common;

use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

use common::{
    HEALTH, TRACK, assert_golden, chain, fixture_repo, without_processes, written as file,
};
use meada::engine::Engine;

fn decide(repo: &Path, lang: &str, args: &[&str], stdin: &str) -> (String, String, bool) {
    let args = [&["decide"], args].concat();
    common::meada(repo, &args, stdin, |c| {
        c.env("LANG", lang);
    })
}

#[test]
fn picking_by_number_writes_the_canonical_file_and_the_next_query_replays_it() {
    let (web, _, _) = chain("decide_pick");

    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &[], "2\n1\n");

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decide.pick.en.txt", &stdout);
    let written = file(&web).expect("the file is written");
    assert_golden("chain-web.decisions.human.toml", &written);
    let (cold, _, _) = chain("decide_pick_cold");
    fs::write(cold.join("meada.decisions.toml"), &written).unwrap();
    let export = Engine::open(&cold).unwrap().export().unwrap();
    assert_golden(
        "chain-web.decided-human.export.json",
        &without_processes(&export),
    );
}

#[test]
fn skipping_and_end_of_input_leave_the_rest_undecided() {
    let (web, _, _) = chain("decide_skip");

    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &[], "s\n\n");
    assert!(ok, "{stderr}");
    assert!(stdout.ends_with("nothing decided\n"), "{stdout}");
    assert_eq!(file(&web), None);

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
    common::decisions(&web, "chain-web");

    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &["--item", HEALTH], "1\n");
    assert!(ok, "{stderr}");
    assert!(
        stdout.starts_with(&format!(
            "{HEALTH}\nsrc/status.ts:3  fetch(\"/api/health\")\n  kind = \"ambiguous-link\"\n\
             \x20 1) example.com/chain/catalog GET /api/health\n\
             \x20 2) example.com/chain/gateway GET /api/health\n"
        )),
        "{stdout}"
    );
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

    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &[], "");
    assert!(ok);
    assert_eq!(stdout, "no open items\n");
}

#[test]
fn messages_follow_lang() {
    let (web, _, _) = chain("decide_pt");

    for lang in ["en_US.UTF-8", "pt_BR.UTF-8", "es_ES.UTF-8"] {
        let (stdout, stderr, ok) = decide(&web, lang, &["--list"], "");
        assert!(ok && stderr.is_empty(), "{stderr}");
        assert!(stdout.contains("kind = \"ambiguous-link\""), "{stdout}");
        assert!(stdout.contains("kind = \"dynamic-url\""), "{stdout}");
    }
    let (stdout, _, ok) = decide(&web, "pt_BR.UTF-8", &[], "2\n1\n");

    assert!(ok);
    assert_golden("chain-web.decide.pick.pt.txt", &stdout);
}

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
fn list_prints_every_cause_of_a_stale_decision() {
    let (web, _, _) = chain("decide_list_unfit");
    common::decisions(&web, "chain-web-unfit");

    let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", &["--list"], "");

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decide.list-unfit.en.txt", &stdout);
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

#[cfg(unix)]
const LINKED: &str = "<repo>/meada.decisions.toml: a symbolic link; \
    Meada writes this file in the repository root only";

#[cfg(unix)]
fn link_out(web: &Path) -> (PathBuf, PathBuf) {
    let link = web.join("meada.decisions.toml");
    let target = web.with_file_name("shared.decisions.toml");
    fs::rename(&link, &target).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    (link, target)
}

#[cfg(unix)]
fn untouched(web: &Path, (link, target): &(PathBuf, PathBuf), before: Option<&[u8]>) {
    let meta = fs::symlink_metadata(link).unwrap();
    assert!(meta.file_type().is_symlink(), "{}", link.display());
    assert_eq!(fs::read(target).ok().as_deref(), before);
    assert!(!web.join("meada.decisions.toml.tmp").exists());
}

#[cfg(unix)]
#[test]
fn every_mode_that_can_save_refuses_a_linked_decisions_file_and_leaves_it_alone() {
    let (web, _, _) = chain("decide_linked");
    common::decisions(&web, "chain-web-stale-and-waiting");
    add_missing_peer(&web);
    let linked = link_out(&web);
    let before = fs::read(&linked.1).unwrap();

    for (args, stdin) in [
        (&[][..], "1\n1\n"),
        (&["--item", HEALTH], "1\n"),
        (&["--prune"], ""),
    ] {
        let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", args, stdin);
        assert!(!ok, "{args:?}");
        assert_eq!(
            (stdout.as_str(), stderr),
            ("", format!("meada: {LINKED}\n"))
        );
        untouched(&web, &linked, Some(&before));
    }

    let (_, stderr, ok) = decide(&web, "pt_BR.UTF-8", &[], "1\n1\n");
    assert!(!ok);
    assert_eq!(
        stderr,
        "meada: <repo>/meada.decisions.toml: um link simbólico; o Meada grava este arquivo só \
         na raiz do repositório\n"
    );
    untouched(&web, &linked, Some(&before));
}

#[cfg(unix)]
#[test]
fn the_refusal_of_a_linked_decisions_file_does_not_depend_on_what_the_run_would_save() {
    let (web, _, _) = chain("decide_linked_nothing");
    common::decisions(&web, "chain-web");
    let linked = link_out(&web);
    let before = fs::read(&linked.1).unwrap();
    let refused = (String::new(), format!("meada: {LINKED}\n"), false);

    for args in [&[][..], &["--prune"]] {
        assert_eq!(
            decide(&web, "en_US.UTF-8", args, "1\n"),
            refused,
            "{args:?}"
        );
        untouched(&web, &linked, Some(&before));
    }

    fs::remove_file(&linked.1).unwrap();
    for args in [&[][..], &["--prune"]] {
        assert_eq!(
            decide(&web, "en_US.UTF-8", args, "1\n1\n"),
            refused,
            "{args:?}"
        );
        untouched(&web, &linked, None);
    }
}

#[cfg(unix)]
#[test]
fn saving_modes_check_the_link_before_loading_its_malformed_target() {
    let (web, _, _) = chain("decide_linked_malformed");
    let bytes = b"decision = [\n";
    fs::write(web.join("meada.decisions.toml"), bytes).unwrap();
    let linked = link_out(&web);
    let destination = fs::read_link(&linked.0).unwrap();
    for args in [&[][..], &["--item", HEALTH], &["--prune"], &["--decider"]] {
        assert_eq!(
            decide(&web, "en_US.UTF-8", args, "1\n"),
            (String::new(), format!("meada: {LINKED}\n"), false),
            "{args:?}"
        );
        untouched(&web, &linked, Some(bytes));
        assert_eq!(fs::read_link(&linked.0).unwrap(), destination);
    }
    for args in [
        &["--list"][..],
        &["--dry-run"],
        &["--decider", "--show-request"],
    ] {
        let (stdout, stderr, ok) = decide(&web, "en_US.UTF-8", args, "");
        assert!(!ok && stdout.is_empty(), "{args:?}: {stdout}");
        assert!(stderr.contains("unclosed array"), "{args:?}: {stderr}");
        untouched(&web, &linked, Some(bytes));
    }
}

#[cfg(unix)]
#[test]
fn what_never_saves_reads_a_linked_decisions_file_as_a_regular_one() {
    let (web, _, _) = chain("decide_linked_reads");
    common::decisions(&web, "chain-web-stale-and-waiting");
    add_missing_peer(&web);
    let runs: [(&[&str], &str); 4] = [
        (&["decide", "--list"], ""),
        (&["decide", "--dry-run"], "1\n"),
        (&["index"], ""),
        (&["status"], ""),
    ];
    let outputs = || {
        runs.map(|(args, stdin)| {
            let lang = |c: &mut std::process::Command| {
                c.env("LANG", "en_US.UTF-8");
            };
            let (stdout, stderr, ok) = common::meada(&web, args, stdin, lang);
            assert!(ok, "{args:?}: {stderr}");
            (stdout, stderr)
        })
    };

    let regular = outputs();
    let linked = link_out(&web);
    let before = fs::read(&linked.1).unwrap();
    let through = outputs();

    assert_golden("chain-web.decide.list-stale.en.txt", &through[0].0);
    assert_eq!(through, regular);
    untouched(&web, &linked, Some(&before));
}

#[test]
fn the_overview_ranks_and_shortens_candidates_without_changing_picks() {
    let repo = fixture_repo(
        "decide_overview_paths",
        "open-few",
        Some("https://example.com/open/few.git"),
    );
    fs::write(
        repo.join("src/rank.ts"),
        r#"
declare function tenant(): string;
export function Pick(suffix: string) {
    return fetch(`https://${tenant()}.example.com/api/orders${suffix}`);
}
export function Count(url: string) {
    return fetch(url);
}
"#,
    )
    .unwrap();
    let base = repo.join("app/api/orders");
    for path in ["", "[id]", "export", "inventory", "history", "downloads"] {
        let dir = base.join(path);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("route.ts"),
            "export function GET() { return new Response('ok'); }",
        )
        .unwrap();
    }
    let call = "example.com/open/few::src/rank.ts::Pick::fetch#0";
    let counted = "example.com/open/few::src/rank.ts::Count::fetch#0";
    let read = |args: &[&str]| decide(&repo, "en_US.UTF-8", args, "2\n");
    let block = |text: String, id: &str| {
        text.split("\n\n")
            .find(|block| block.starts_with(id))
            .unwrap()
            .to_owned()
    };
    let choices = |shown: String| {
        shown
            .lines()
            .filter(|line| line.starts_with("  ") && line.contains(") example.com"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let (overview, err, ok) = read(&["--list"]);
    assert!(ok, "{err}");
    let limited = block(overview.clone(), call);
    assert_eq!(choices(limited.clone()).len(), 5);
    assert!(
        limited.contains(&format!("1 more. Run meada decide --list --item {call}")),
        "{limited}"
    );
    let count = block(overview, counted);
    assert!(count.contains("candidates matching the method"), "{count}");
    assert!(choices(count).is_empty());
    let (counted_full, error, ok) = read(&["--list", "--item", counted]);
    assert!(
        ok && !counted_full.contains("candidates matching the method"),
        "{error}"
    );
    assert!(choices(counted_full).len() >= 6);

    let (full, _, ok) = read(&["--list", "--item", call]);
    assert!(ok);
    let full = choices(full);
    assert_eq!(full.len(), 6);
    assert_eq!(
        &full[..3],
        [
            "  1) example.com/open/few GET /api/orders",
            "  2) example.com/open/few GET /api/orders/{}",
            "  3) example.com/open/few GET /api/orders/export",
        ]
    );
    let (prompt, err, ok) = read(&["--item", call]);
    assert!(ok, "{err}");
    assert_eq!(choices(prompt), full);
    let stored = common::written(&repo).unwrap();
    assert!(
        stored.contains("example.com/open/few::http::GET::/api/orders/{}"),
        "{stored}"
    );

    fs::remove_file(repo.join("meada.decisions.toml")).unwrap();
    fs::remove_file(base.join("downloads/route.ts")).unwrap();
    let exactly_five = block(read(&["--list"]).0, call);
    assert_eq!(choices(exactly_five.clone()).len(), 5);
    assert!(!exactly_five.contains(" more."), "{exactly_five}");
}
