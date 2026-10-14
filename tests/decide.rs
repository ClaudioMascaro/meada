//! `meada decide` in human mode (spec #18, ADR 0006): the binary driven through scripted
//! standard input over the chain fixture's two open items, both in `chain-web`.

mod common;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use common::{assert_golden, chain};
use meada::engine::Engine;

/// Runs `meada decide <args>` in `repo` under `lang`, feeding it `stdin`. Returns stdout,
/// stderr, and whether it succeeded; the repository's path is replaced by `<repo>`.
fn decide(repo: &Path, lang: &str, args: &[&str], stdin: &str) -> (String, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(repo)
        .arg("decide")
        .args(args)
        .env("LANG", lang)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meada runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text =
        |bytes: &[u8]| String::from_utf8_lossy(bytes).replace(repo.to_str().unwrap(), "<repo>");
    (text(&out.stdout), text(&out.stderr), out.status.success())
}

fn file(repo: &Path) -> Option<String> {
    fs::read_to_string(repo.join("meada.decisions.toml")).ok()
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
    let export = Engine::open(&web).unwrap().export().unwrap();
    assert_golden("chain-web.decided-human.export.json", &export);
}

const HEALTH: &str = "example.com/chain/web::src/status.ts::health::fetch#0";

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
        stdout.ends_with("wrote 1 decisions to meada.decisions.toml\n"),
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
fn item_limits_the_run_and_an_unknown_id_is_an_error() {
    let (web, _, _) = chain("decide_item");

    let (stdout, _, ok) = decide(&web, "en_US.UTF-8", &["--item", HEALTH], "2\n");
    assert!(ok);
    assert!(!stdout.contains("track"), "{stdout}");

    let (_, stderr, ok) = decide(&web, "en_US.UTF-8", &["--item", "nope"], "");
    assert!(!ok);
    assert_eq!(
        stderr,
        "meada: `nope` is not an open item of this repository\n"
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
fn messages_follow_lang() {
    let (web, _, _) = chain("decide_pt");

    let (stdout, _, ok) = decide(&web, "pt_BR.UTF-8", &[], "2\n1\n");

    assert!(ok);
    assert_golden("chain-web.decide.pick.pt.txt", &stdout);
}
