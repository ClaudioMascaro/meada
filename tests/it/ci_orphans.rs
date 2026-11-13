//! Orphan providers and the absent mark of the CI contract (#827, ADR 0028; ADR 0026,
//! "Orphans"). A reusable pipeline or an action nothing calls is an orphan line of `meada
//! status`, only when the scope holds a provider and a consumer of the kind. A consumer whose
//! path names no provider in its repository of the workspace is marked absent at the indexed
//! commit, on its side: the file may exist at the Git ref.

use std::fs;
use std::path::{Path, PathBuf};

use meada::engine::Engine;

use crate::ci_calls::{exported, listed, reproduction};
use crate::ci_uses::{consumers, pair};
use crate::common::{assert_golden, fixture_repo, meada};

const WORKFLOWS: &str = "ci::github.com/acme/workflows";
const ABSENT: &str = ".github/workflows/absent.yml";
const MARK: &str = " (absent at the indexed commit)";
const COMPOSITE: &str =
    "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n";
/// Calls of `acme/app-a` to paths of `acme/workflows`: a pipeline file that is not there, one
/// that is there and is not reusable (`internal.yml`), a folder that is not there, and one
/// that holds only a `Dockerfile`.
const CALLS: &str = "\
on: push
jobs:
  missing:
    uses: acme/workflows/.github/workflows/missing.yml@main
  plain:
    uses: acme/workflows/.github/workflows/internal.yml@main
  steps:
    runs-on: ubuntu-latest
    steps:
      - uses: acme/workflows/tools/nowhere@v1
        with:
          mode: fast
      - uses: acme/workflows/tools/container@v1
";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The reproduction of #816 with what #827 names added: an action nothing calls and a folder
/// with only a `Dockerfile` in `acme/workflows` (whose `forms.yml` is a reusable pipeline
/// nothing calls), and the calls of `CALLS` in `acme/app-a`. Gives (`acme/workflows`,
/// `acme/app-a`).
fn workspace(case: &str) -> (PathBuf, PathBuf) {
    let [workflows, _, app_a, ..] = reproduction(case);
    write(&workflows, "tools/unused/action.yml", COMPOSITE);
    write(
        &workflows,
        "tools/container/Dockerfile",
        "FROM alpine:3.20\n",
    );
    write(&app_a, ABSENT, CALLS);
    (workflows, app_a)
}

/// The orphan lines of a `meada status` answer.
fn orphans(status: &str) -> Vec<&str> {
    let lines = status.lines().map(str::trim_start);
    lines.filter(|line| line.starts_with("orphan ")).collect()
}

/// `meada status` on the workspace of `root`, indexed first: the command reports and does not
/// index.
fn status(root: &Path) -> String {
    Engine::open(root).unwrap().refresh().unwrap();
    let (status, _, ok) = meada(root, &["status"], "", |_| {});
    assert!(ok, "{status}");
    status
}

#[test]
fn status_lists_the_providers_nothing_calls_and_nothing_else() {
    let (_, app_a) = workspace("ci_orphans_status");
    let status = status(&app_a);
    // The absent paths and the folder with only a `Dockerfile` are no orphan line.
    assert_eq!(
        orphans(&status),
        [
            format!(
                "orphan {WORKFLOWS}/.github/workflows/forms.yml: no consumer; .github/workflows/forms.yml"
            ),
            format!("orphan {WORKFLOWS}/tools/unused: no consumer; tools/unused/action.yml"),
        ],
        "{status}"
    );
    assert!(!status.contains("absent at"), "{status}");
    assert_golden("ci-orphans.status.txt", &status);
}

#[test]
fn a_call_to_a_path_with_no_provider_is_marked_absent_on_its_side() {
    let (_, app_a) = workspace("ci_orphans_absent");
    let engine = Engine::open(&app_a).unwrap();
    let context = |address: &str| {
        let symbol = format!("{ABSENT}::{address}");
        engine.context(&symbol, None).unwrap()
    };
    // On the consumer's node: the mark closes the line, and no provider follows.
    let missing = context("jobs.missing");
    assert_eq!(
        listed(&missing, "consumes "),
        [format!(
            "{WORKFLOWS}/.github/workflows/missing.yml @main{MARK}"
        )],
        "{missing}"
    );
    assert!(listed(&missing, "  provider: ").is_empty(), "{missing}");
    // A file that is there and holds no `on.workflow_call` is no provider either.
    let plain = context("jobs.plain");
    assert_eq!(
        listed(&plain, "consumes "),
        [format!(
            "{WORKFLOWS}/.github/workflows/internal.yml @main (absent at the indexed commit: the file is no reusable pipeline there)"
        )],
        "{plain}"
    );
    let step = context("jobs.steps.steps[0]");
    assert_eq!(
        listed(&step, "consumes "),
        [format!("{WORKFLOWS}/tools/nowhere @v1 with: mode{MARK}")],
        "{step}"
    );
    // On the contract identifier: its one consumer, marked.
    let contract = engine
        .context(&format!("{WORKFLOWS}/tools/nowhere"), None)
        .unwrap();
    let sides: Vec<&str> = (contract.lines())
        .filter(|line| line.contains(" consumer "))
        .collect();
    assert_eq!(sides.len(), 1, "{contract}");
    assert!(sides[0].ends_with(MARK), "{contract}");
    // `search` lists the side with the same mark.
    let found = engine.search("nowhere", Some("contract"), None).unwrap();
    assert!(found.contains(MARK), "{found}");
    // The consumer row of the export, and only where no provider is.
    let export = exported(&engine);
    let mut marked: Vec<&str> = (export["consumers"].as_array().unwrap().iter())
        .filter(|row| row["absent"] == true)
        .map(|row| row["site"].as_str().unwrap())
        .collect();
    marked.sort_unstable();
    let site = |address: &str| format!("github.com/acme/app-a::{ABSENT}::{address}");
    assert_eq!(
        marked,
        [
            site("jobs.missing"),
            site("jobs.plain"),
            site("jobs.steps.steps[0]"),
            // `ci.yml` of the fixture calls `internal.yml` too.
            "github.com/acme/app-a::.github/workflows/ci.yml::jobs.internal".to_string(),
        ],
        "{export:#}"
    );
    assert!(
        (export["consumers"].as_array().unwrap().iter())
            .all(|row| row.get("absent").is_none_or(|absent| absent == true)),
        "{export:#}"
    );
    // A folder with only a `Dockerfile` is an action no reader takes: the call holds no side,
    // so no mark.
    assert_eq!(
        consumers(&export, ABSENT),
        [
            pair(
                "jobs.missing",
                Some(&format!("{WORKFLOWS}/.github/workflows/missing.yml"))
            ),
            pair(
                "jobs.plain",
                Some(&format!("{WORKFLOWS}/.github/workflows/internal.yml"))
            ),
            pair(
                "jobs.steps.steps[0]",
                Some(&format!("{WORKFLOWS}/tools/nowhere"))
            ),
        ]
    );
    let container = context("jobs.steps.steps[1]");
    assert!(listed(&container, "consumes ").is_empty(), "{container}");
    // An absent path is no open item, and the mark never says broken.
    let open = engine.search("", Some("unresolved"), None).unwrap();
    assert!(!open.contains(ABSENT), "{open}");
    for answer in [&missing, &plain, &step, &contract, &found] {
        assert!(!answer.contains("broken"), "{answer}");
    }
    let shown = [missing, plain, step, container, contract].join("\n");
    assert_golden("ci-orphans.context.txt", &shown);
}

/// The scope rule (ADR 0026): a provider nothing calls is listed only when the scope holds a
/// consumer of the kind. An external `uses` holds no side and an unresolved consumer no
/// contract: neither opens the scope.
#[test]
fn a_scope_without_a_consumer_prints_no_orphan_line() {
    // Three actions and a pipeline, alone: the only side that calls is the `./tools/setup` of
    // the root action, unresolved in a composite action.
    let repo = fixture_repo(
        "ci_orphans_scope",
        "ci-nodes",
        Some("https://example.com/acme/app.git"),
    );
    assert_eq!(orphans(&status(&repo)), [""; 0]);
    // A reusable pipeline whose only call is to `actions/checkout`, outside the workspace.
    write(
        &repo,
        ".github/workflows/reuse.yml",
        "on: workflow_call\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n",
    );
    assert_eq!(orphans(&status(&repo)), [""; 0]);
    // One call inside the repository opens the scope: every provider nothing calls is listed,
    // and the pipeline it calls is not.
    write(
        &repo,
        ".github/workflows/call.yml",
        "on: push\njobs:\n  call:\n    uses: ./.github/workflows/reuse.yml\n",
    );
    assert_eq!(
        orphans(&status(&repo)),
        [
            "orphan ci::example.com/acme/app: no consumer; action.yml",
            "orphan ci::example.com/acme/app/tools/only: no consumer; tools/only/action.yaml",
            "orphan ci::example.com/acme/app/tools/setup: no consumer; tools/setup/action.yml",
        ]
    );
}

/// The file that was missing arrives in the provider's repository: the mark leaves at the next
/// query, as from a cold index. A `Dockerfile` alone in the folder takes the side with it.
#[test]
fn adding_the_missing_file_removes_the_mark_as_a_cold_index_does() {
    let (workflows, app_a) = workspace("ci_orphans_fresh");
    let engine = Engine::open(&app_a).unwrap();
    let answers = |engine: &Engine| {
        ["jobs.missing", "jobs.steps.steps[0]"].map(|address| {
            let symbol = format!("{ABSENT}::{address}");
            engine.context(&symbol, None).unwrap()
        })
    };
    let cold = || {
        for root in [&workflows, &app_a] {
            fs::remove_dir_all(root.join(".meada")).unwrap();
        }
        answers(&Engine::open(&app_a).unwrap())
    };
    let before = answers(&engine);
    assert!(
        before.iter().all(|answer| answer.contains(MARK)),
        "{before:?}"
    );
    write(
        &workflows,
        ".github/workflows/missing.yml",
        "on: workflow_call\njobs: {}\n",
    );
    write(&workflows, "tools/nowhere/action.yml", COMPOSITE);
    let warm = answers(&engine);
    for answer in &warm {
        assert!(!answer.contains("absent at"), "{answer}");
        assert_eq!(listed(answer, "  provider: ").len(), 1, "{answer}");
    }
    assert_eq!(warm, cold());
    // The action file leaves and a `Dockerfile` stays: the CI step holds no side.
    let engine = Engine::open(&app_a).unwrap();
    fs::remove_file(workflows.join("tools/nowhere/action.yml")).unwrap();
    write(&workflows, "tools/nowhere/Dockerfile", "FROM alpine:3.20\n");
    let [_, step] = answers(&engine);
    assert!(listed(&step, "consumes ").is_empty(), "{step}");
    assert_eq!(answers(&engine), cold());
    // Without it the folder holds nothing GitHub runs: the mark is back.
    let engine = Engine::open(&app_a).unwrap();
    fs::remove_file(workflows.join("tools/nowhere/Dockerfile")).unwrap();
    let [_, step] = answers(&engine);
    assert!(step.contains(MARK), "{step}");
    assert_eq!(answers(&engine), cold());
}

/// What the verification of #827 found. The second reason is a pipeline's alone: a CI step
/// that names an action's file names a folder that is not there. A `./<folder>` read in a
/// workspace Meada does not know stays an open item, whatever the own repository holds in
/// that folder. And the runner builds an action from `dockerfile` as from `Dockerfile`.
#[test]
fn the_reason_the_open_item_and_the_lower_case_name_follow_the_runner() {
    let (workflows, app_a) = workspace("ci_orphans_verified");
    write(&workflows, "tools/lower/dockerfile", "FROM alpine:3.20\n");
    write(
        &workflows,
        "tools/wrap/action.yml",
        "runs:\n  using: composite\n  steps:\n    - uses: ./tools/container\n",
    );
    write(
        &app_a,
        ABSENT,
        "on: push\njobs:\n  steps:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: acme/workflows/tools/unused/action.yml@v1\n      - uses: acme/workflows/tools/lower@v1\n",
    );
    let engine = Engine::open(&app_a).unwrap();
    let file = engine
        .context(&format!("{ABSENT}::jobs.steps.steps[0]"), None)
        .unwrap();
    assert_eq!(
        listed(&file, "consumes "),
        [format!("{WORKFLOWS}/tools/unused/action.yml @v1{MARK}")],
        "{file}"
    );
    let export = exported(&engine);
    assert_eq!(
        consumers(&export, ABSENT),
        [pair(
            "jobs.steps.steps[0]",
            Some(&format!("{WORKFLOWS}/tools/unused/action.yml"))
        )]
    );
    let engine = Engine::open(&workflows).unwrap();
    assert_eq!(
        consumers(&exported(&engine), "tools/wrap/action.yml"),
        [pair("runs.steps[0]", None)]
    );
    let open = engine.search("", Some("unresolved"), None).unwrap();
    assert!(open.contains("tools/wrap/action.yml"), "{open}");
}
