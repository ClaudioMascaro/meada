//! Declared permissions on a CI call and `permissions mismatch` on its link (#834, ADR 0028):
//! the block that governs a CI job is its own, else its pipeline's, and the link says when a
//! CI job of the called pipeline declares a permission above it. Declared, never granted.

use std::fs;

use meada::engine::Engine;

use crate::ci_calls::{exported, indexed, listed, reproduction};
use crate::common::assert_golden;

const PUBLISH: &str = ".github/workflows/publish.yml";
const CI: &str = ".github/workflows/ci.yml";
const RELEASE: &str = ".github/workflows/release.yml";

#[test]
fn a_reusable_pipeline_shows_each_callers_declared_permissions_and_flags_the_one_below_it() {
    let [workflows, wrapper, _, app_b, app_c] = reproduction("ci_permissions_table");
    let engine = Engine::open(&workflows).unwrap();
    let answer = engine.context(PUBLISH, None).unwrap();
    let at = indexed(&workflows);
    let both = "permissions: contents: read, id-token: write";
    // #820's table: only the CI job its pipeline's `contents: read` governs is below what the
    // called pipeline declares.
    assert_eq!(
        listed(&answer, "  consumer: "),
        [
            format!(
                "github.com/acme/app-a {CI}:12 jobs.publish [exact: @main (indexed {at})] with: package, registry; secrets: inherit; {both}"
            ),
            format!(
                "github.com/acme/app-b {CI}:11 jobs[\"publish-prd\"] [exact: @v1 (indexed {at}), permissions mismatch: id-token write against none] with: package; secrets: token; permissions: contents: read"
            ),
            format!(
                "github.com/acme/app-b {CI}:18 jobs[\"publish-stg\"] [exact: @main (indexed {at})] with: package; {both}"
            ),
            format!(
                "github.com/acme/wrapper {RELEASE}:5 jobs.release [exact: @main (indexed {at})] with: package; secrets: inherit; {both}"
            ),
        ],
        "{answer}"
    );
    // What the CI jobs of the pipeline declare together: its block governs its one CI job.
    assert_eq!(
        listed(&answer, "  permissions: "),
        ["contents: read, id-token: write"],
        "{answer}"
    );
    // A CI job with no block of its own or of its pipeline is shown so and never compared.
    let release = Engine::open(&wrapper).unwrap();
    let release =
        (release.context(&format!("github.com/acme/wrapper::{RELEASE}::."), None)).unwrap();
    let at = indexed(&wrapper);
    assert_eq!(
        listed(&release, "  consumer: "),
        [format!(
            "github.com/acme/app-c {RELEASE}:6 jobs.release [exact: @main (indexed {at})] permissions: not declared"
        )],
        "{release}"
    );
    // The export: the block on the consumer row, left out where none is declared.
    let rows = |repo: &std::path::Path| {
        let export = exported(&Engine::open(repo).unwrap());
        let rows = export["consumers"].as_array().unwrap().clone();
        let publish = |row: &serde_json::Value| {
            let contract = row["contract"].as_str().unwrap_or_default();
            contract.ends_with("/publish.yml") || contract.ends_with("/release.yml")
        };
        let block = |row: serde_json::Value| (row["site"].clone(), row.get("permissions").cloned());
        rows.into_iter()
            .filter(publish)
            .map(block)
            .collect::<Vec<_>>()
    };
    let site = |job: &str| serde_json::json!(format!("github.com/acme/app-b::{CI}::{job}"));
    assert_eq!(
        rows(&app_b),
        [
            (
                site("jobs[\"publish-prd\"]"),
                Some(serde_json::json!({"contents": "read"}))
            ),
            (
                site("jobs[\"publish-stg\"]"),
                Some(serde_json::json!({"contents": "read", "id-token": "write"}))
            ),
        ]
    );
    let none = rows(&app_c);
    assert_eq!(none.len(), 1, "{none:?}");
    assert_eq!(none[0].1, None, "{none:?}");
}

/// One caller and one called pipeline per row: the block of the caller's pipeline and of its
/// CI job, the block of the called pipeline and of its CI jobs, then what the caller shows and
/// the flag its link carries. The expected values follow GitHub's documented rules: a block is
/// closed, a CI job's own block replaces its pipeline's, `read-all` and `write-all` give every
/// permission that level, and the levels order as `none`, `read`, `write`.
#[test]
fn a_block_is_closed_a_job_replaces_its_pipeline_and_the_words_stand_for_every_permission() {
    let [workflows, _, _, _, app_c] = reproduction("ci_permissions_rules");
    // (caller pipeline, caller job, called pipeline, called jobs, shown, flag)
    type Case = (
        &'static str,
        &'static str,
        &'static str,
        &'static [&'static str],
        &'static str,
        Option<&'static str>,
    );
    let cases: [Case; 15] = [
        ("", "read-all", "", &["contents: read"], "read-all", None),
        (
            "",
            "read-all",
            "",
            &["id-token: write"],
            "read-all",
            Some("id-token write against read"),
        ),
        ("", "write-all", "write-all", &[""], "write-all", None),
        (
            "",
            "{}",
            "",
            &["contents: read"],
            "{}",
            Some("contents read against none"),
        ),
        (
            "",
            "contents: write",
            "read-all",
            &[""],
            "contents: write",
            Some("every other permission read against none"),
        ),
        (
            "",
            "contents: read",
            "",
            &["write-all"],
            "contents: read",
            Some("contents write against read, every other permission write against none"),
        ),
        // Not declared, on either side: shown so, never compared.
        ("", "", "write-all", &[""], "not declared", None),
        ("", "contents: read", "", &["", ""], "contents: read", None),
        // The CI job's own block replaces its pipeline's, also to hold less.
        (
            "id-token: write",
            "contents: read",
            "",
            &["id-token: write"],
            "contents: read",
            Some("id-token write against none"),
        ),
        (
            "contents: read",
            "id-token: write",
            "",
            &["id-token: write"],
            "id-token: write",
            None,
        ),
        // Any CI job of the called pipeline: the highest level one of them declares.
        (
            "",
            "contents: write",
            "",
            &["contents: write", "id-token: write", ""],
            "contents: write",
            Some("id-token write against none"),
        ),
        // A permission named at `none` is declared, and holds nothing.
        (
            "",
            "contents: read, id-token: none",
            "",
            &["id-token: read"],
            "contents: read, id-token: none",
            Some("id-token read against none"),
        ),
        // A block Meada does not read is as one not declared: no level is guessed.
        ("", "admin", "", &["write-all"], "not declared", None),
        (
            "",
            "contents: maybe",
            "",
            &["write-all"],
            "not declared",
            None,
        ),
        // A name that could write a level of its own in an answer is no name.
        (
            "",
            "\"id-token: write\": read",
            "",
            &["write-all"],
            "not declared",
            None,
        ),
    ];
    // A block as YAML at `indent`: a word, `{}` or a mapping of `key: level` pairs.
    let block = |text: &str, indent: &str| match text {
        "" => String::new(),
        word if !word.contains(':') => format!("{indent}permissions: {word}\n"),
        named => {
            let lines = named.split(", ").map(|pair| format!("{indent}  {pair}\n"));
            format!("{indent}permissions:\n{}", lines.collect::<String>())
        }
    };
    for (n, (pipeline, job, called, jobs, ..)) in cases.iter().enumerate() {
        let jobs = jobs.iter().enumerate().map(|(m, own)| {
            format!(
                "  job{m}:\n{}    runs-on: ubuntu-latest\n    steps:\n      - run: echo {m}\n",
                block(own, "    ")
            )
        });
        fs::write(
            workflows.join(format!(".github/workflows/p{n}.yml")),
            format!(
                "on: workflow_call\n{}jobs:\n{}",
                block(called, ""),
                jobs.collect::<String>()
            ),
        )
        .unwrap();
        fs::write(
            app_c.join(format!(".github/workflows/c{n}.yml")),
            format!(
                "on: push\n{}jobs:\n  call:\n{}    uses: acme/workflows/.github/workflows/p{n}.yml@main\n",
                block(pipeline, ""),
                block(job, "    ")
            ),
        )
        .unwrap();
    }
    let engine = Engine::open(&workflows).unwrap();
    let at = indexed(&workflows);
    for (n, (pipeline, _, _, _, shown, flag)) in cases.iter().enumerate() {
        let answer = engine
            .context(&format!(".github/workflows/p{n}.yml"), None)
            .unwrap();
        let flag = flag.map_or(String::new(), |above| {
            format!(", permissions mismatch: {above}")
        });
        // The CI job sits below `on`, the pipeline's block and `jobs`.
        let line = 3 + block(pipeline, "").lines().count();
        assert_eq!(
            listed(&answer, "  consumer: "),
            [format!(
                "github.com/acme/app-c .github/workflows/c{n}.yml:{line} jobs.call [exact: @main (indexed {at}){flag}] permissions: {shown}"
            )],
            "case {n}: {answer}"
        );
    }
    // What the called pipeline's CI jobs declare together, and `not declared` when none does.
    let together = |n: usize| {
        let answer = engine
            .context(&format!(".github/workflows/p{n}.yml"), None)
            .unwrap();
        listed(&answer, "  permissions: ")
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(together(2), ["write-all"]);
    assert_eq!(together(4), ["read-all"]);
    assert_eq!(together(7), ["not declared"]);
    assert_eq!(together(10), ["contents: write, id-token: write"]);
}

/// A CI step calls an action: its link and its line carry no permissions text. A link with a
/// commit id other than the indexed one and a permission below carries both flags, the ref
/// first.
#[test]
fn only_a_ci_job_is_compared_and_both_flags_keep_one_order() {
    let [workflows, _, app_a, _, app_c] = reproduction("ci_permissions_flags");
    let setup = Engine::open(&workflows).unwrap();
    let setup = setup.context("tools/setup/action.yml", None).unwrap();
    assert!(!setup.contains("permissions"), "{setup}");
    let forms = Engine::open(&app_a).unwrap();
    let forms = forms
        .context(".github/workflows/forms.yml::jobs.actions.steps[2]", None)
        .unwrap();
    assert!(!forms.contains("permissions"), "{forms}");
    let commit = "0123456789abcdef0123456789abcdef01234567";
    fs::write(
        app_c.join(".github/workflows/pinned.yml"),
        format!(
            "on: push\npermissions:\n  contents: read\njobs:\n  pinned:\n    uses: acme/workflows/{PUBLISH}@{commit}\n"
        ),
    )
    .unwrap();
    let engine = Engine::open(&app_c).unwrap();
    let answer = engine
        .context(".github/workflows/pinned.yml::jobs.pinned", None)
        .unwrap();
    let at = indexed(&workflows);
    assert_eq!(
        listed(&answer, "  provider: "),
        [format!(
            "github.com/acme/workflows {PUBLISH}:1 . [exact: @{commit} (indexed {at}), ref mismatch, permissions mismatch: id-token write against none]"
        )],
        "{answer}"
    );
    let sides = (answer.lines())
        .filter(|line| line.starts_with("consumes ") || line.starts_with("  provider: "))
        .map(|line| line.replace(&at, "<indexed>"))
        .collect::<Vec<_>>();
    assert_golden("ci-permissions.context.txt", &(sides.join("\n") + "\n"));
}

/// Editing only the called pipeline's block, or only the caller's, changes the flag on the
/// next query, as a cold index gives it.
#[test]
fn an_edited_block_on_either_side_changes_the_flag_as_a_cold_index_gives_it() {
    let [workflows, _, _, app_b, _] = reproduction("ci_permissions_edit");
    let engine = Engine::open(&workflows).unwrap();
    let flagged = |answer: &str| {
        (listed(answer, "  consumer: ").iter())
            .filter(|line| line.contains("permissions mismatch"))
            .count()
    };
    let cold = || {
        Engine::open(&workflows)
            .unwrap()
            .context(PUBLISH, None)
            .unwrap()
    };
    assert_eq!(flagged(&engine.context(PUBLISH, None).unwrap()), 1);
    // The called pipeline stops declaring `id-token`: nothing is above any caller.
    let publish = workflows.join(PUBLISH);
    let text = fs::read_to_string(&publish).unwrap();
    fs::write(&publish, text.replace("  id-token: write\n", "")).unwrap();
    let warm = engine.context(PUBLISH, None).unwrap();
    assert_eq!(flagged(&warm), 0, "{warm}");
    assert_eq!(warm, cold());
    fs::write(&publish, &text).unwrap();
    assert_eq!(flagged(&engine.context(PUBLISH, None).unwrap()), 1);
    // The caller's pipeline declares it: its CI job with no block of its own holds it now.
    let ci = app_b.join(CI);
    let caller = fs::read_to_string(&ci).unwrap();
    let declared = caller.replace(
        "permissions:\n  contents: read\n",
        "permissions:\n  contents: read\n  id-token: write\n",
    );
    assert_ne!(caller, declared);
    fs::write(&ci, declared).unwrap();
    let warm = engine.context(PUBLISH, None).unwrap();
    assert_eq!(flagged(&warm), 0, "{warm}");
    assert_eq!(warm, cold());
}
