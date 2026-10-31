//! `meada.decisions.toml` (spec #18, ADR 0006): a repository's decisions turn its ambiguous
//! links and unresolved consumers into links at the level `decided`. The chain fixture has two
//! open items, both in `chain-web`; `tests/fixtures/decisions/` holds the files that resolve them.

use crate::common;

use std::fs;

use common::{assert_golden, chain, decisions, fixture_repo, without_processes};
use meada::engine::{Direction, Engine, Start};

#[test]
fn decisions_link_the_ambiguous_path_and_the_dynamic_url_at_the_level_decided() {
    let (web, _, _) = chain("decided_export");
    decisions(&web, "chain-web");

    let export = Engine::open(&web).unwrap().export().unwrap();

    assert_golden("chain-web.decided.export.json", &without_processes(&export));
}

#[test]
fn a_decision_applies_from_a_peer_that_queries_the_workspace() {
    let (web, _, catalog) = chain("decided_peer");
    decisions(&web, "chain-web");

    let export = Engine::open(&catalog).unwrap().export().unwrap();

    assert_golden(
        "chain-catalog.decided.export.json",
        &without_processes(&export),
    );
}

#[test]
fn trace_crosses_decided_links_into_the_provider_and_the_footer_says_decided() {
    let (web, _, _) = chain("decided_trace");
    decisions(&web, "chain-web");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("dashboard"), Direction::Down, None)
        .unwrap();

    assert_golden("chain.decided.trace-down.txt", &answer);
}

#[test]
fn context_shows_a_decided_link_with_its_level_and_reason() {
    let (web, _, _) = chain("decided_context");
    decisions(&web, "chain-web");
    let engine = Engine::open(&web).unwrap();

    for name in ["health", "track"] {
        let answer = engine.context(name, None).unwrap();
        assert_golden(&format!("chain-web.decided.context-{name}.txt"), &answer);
    }
}

/// The error `Engine::open` gives for a repository whose `meada.decisions.toml` is `toml`, with
/// the repository's path replaced by `<repo>`.
fn decisions_error(case: &str, toml: &str) -> String {
    let repo = fixture_repo(case, "ts-basic", Some("https://example.com/acme/web.git"));
    fs::write(repo.join("meada.decisions.toml"), toml).unwrap();
    let error = Engine::open(&repo)
        .err()
        .expect("an invalid meada.decisions.toml fails")
        .to_string();
    error.replace(repo.to_str().unwrap(), "<repo>")
}

#[test]
fn invalid_decisions_errors_name_file_line_and_key() {
    let head = "[[decision]]\nkind = \"dynamic-url\"\ncall = \"a::b::c#0\"\n";
    let tail = "target = \"x/y::http::GET::/z\"\nby = \"human\"\n";
    for (case, toml, expected) in [
        (
            "decisions_unknown",
            format!("{head}{tail}branch = \"main\"\n"),
            "<repo>/meada.decisions.toml:6: `decision.branch` is not a meada.decisions.toml key",
        ),
        (
            "decisions_later",
            "resource = 1\n".to_string(),
            "<repo>/meada.decisions.toml:1: `resource` is not a meada.decisions.toml key",
        ),
        (
            "decisions_kind",
            format!("{}{tail}", head.replace("dynamic-url", "merge")),
            "<repo>/meada.decisions.toml:2: `decision.kind`: is not one of the values it accepts \
             (`ambiguous-link`, `dynamic-url`)",
        ),
        (
            "decisions_missing",
            format!("{head}by = \"human\"\n"),
            "<repo>/meada.decisions.toml:1: `decision.target` is missing",
        ),
        (
            "decisions_human_p",
            format!("{head}{tail}p = 0.5\n"),
            "<repo>/meada.decisions.toml:1: `decision`: `p` is for a model, not for `human`",
        ),
        (
            "decisions_model_without_p",
            format!("{head}{}", tail.replace("human", "some/model")),
            "<repo>/meada.decisions.toml:1: `decision`: `p` is missing for a model",
        ),
        (
            "decisions_p_range",
            format!("{head}{}p = 1.5\n", tail.replace("human", "some/model")),
            "<repo>/meada.decisions.toml:1: `decision`: `p` must be between 0 and 1",
        ),
        (
            "decisions_syntax",
            "[[decision]\nkind = 1\n".to_string(),
            "<repo>/meada.decisions.toml:1: ",
        ),
    ] {
        let error = decisions_error(case, &toml);
        assert!(error.starts_with(expected), "{case}: {error}");
    }
}

#[test]
fn a_decision_that_no_longer_fits_is_ignored() {
    let (web, _, _) = chain("decided_unfit");
    decisions(&web, "chain-web-unfit");

    let export = Engine::open(&web).unwrap().export().unwrap();

    // Every decision is stale, so the rest of the export is the core's.
    let (core, _) = export.split_once(",\n  \"stale_decisions\"").unwrap();
    assert_golden("chain-web.export.json", &format!("{core}\n}}\n"));
    assert_golden("chain-web.unfit.export.json", &without_processes(&export));
}

#[test]
fn a_decision_of_the_wrong_kind_for_its_call_is_ignored() {
    let (web, _, _) = chain("decided_unfit_kind");
    decisions(&web, "chain-web-unfit-kind");

    let export = Engine::open(&web).unwrap().export().unwrap();

    assert_golden(
        "chain-web.unfit-kind.export.json",
        &without_processes(&export),
    );
}

#[test]
fn inserting_a_line_above_a_decided_call_leaves_the_decision_applied() {
    let (web, _, _) = chain("decided_insert");
    decisions(&web, "chain-web");
    let status = web.join("src/status.ts");
    let src = fs::read_to_string(&status).unwrap();
    fs::write(&status, format!("// a new first line\n{src}")).unwrap();

    let export = Engine::open(&web).unwrap().export().unwrap();

    // The links alone: the processes show levels too.
    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    let levels: Vec<&str> = (export["links"].as_array().unwrap().iter())
        .map(|l| l["level"].as_str().unwrap())
        .collect();
    assert_eq!(levels.iter().filter(|l| **l == "decided").count(), 2);
    assert!(!levels.contains(&"ambiguous"));
}

#[test]
fn a_changed_decisions_file_is_picked_up_and_exports_what_a_cold_copy_does() {
    let (web, _, _) = chain("decided_edit");
    decisions(&web, "chain-web");
    let before = Engine::open(&web).unwrap().export().unwrap();
    decisions(&web, "chain-web-health");

    let refreshed = Engine::open(&web).unwrap().export().unwrap();

    let (cold, _, _) = chain("decided_edit_cold");
    decisions(&cold, "chain-web-health");
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
}
