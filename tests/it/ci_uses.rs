//! The other forms of `uses` (#826, ADR 0028): an action of the workspace, a local call, `$/`,
//! a CI step's `./<folder>` and `docker://`, each read as GitHub resolves it or left unresolved.
//! The cases sit in `forms.yml` of `acme/app-a` (a pipeline that is not reusable) and of
//! `acme/workflows` (a reusable one), and in the actions of `acme/workflows`.

use std::fs;

use meada::engine::{Direction, Engine};
use serde_json::Value;

use crate::ci_calls::{chain, exported, indexed, listed, reproduction, traced};
use crate::common::{assert_golden, fixture_repo, git, meada};

const FORMS: &str = ".github/workflows/forms.yml";
const WORKFLOWS: &str = "github.com/acme/workflows";
const APP_A: &str = "github.com/acme/app-a";

/// The consumers of `export` in the file `path`: address and contract, `None` for unresolved.
fn consumers(export: &Value, path: &str) -> Vec<(String, Option<String>)> {
    let repository = export["repository"].as_str().unwrap();
    let prefix = format!("{repository}::{path}::");
    let rows = export["consumers"].as_array().unwrap().iter();
    rows.filter_map(|row| {
        let address = row["site"].as_str().unwrap().strip_prefix(&prefix)?;
        let contract = row["contract"].as_str().map(str::to_string);
        Some((address.to_string(), contract))
    })
    .collect()
}

/// The links of `export` that leave the file `path`: address, provider, level and reason.
fn links(export: &Value, path: &str) -> Vec<(String, String, String, String)> {
    let repository = export["repository"].as_str().unwrap();
    let prefix = format!("{repository}::{path}::");
    let text = |row: &Value, key: &str| row[key].as_str().unwrap().to_string();
    let rows = export["links"].as_array().unwrap().iter();
    rows.filter_map(|row| {
        let address = row["consumer"].as_str().unwrap().strip_prefix(&prefix)?;
        Some((
            address.to_string(),
            text(row, "provider"),
            text(row, "level"),
            text(row, "reason"),
        ))
    })
    .collect()
}

fn pair(address: &str, contract: Option<&str>) -> (String, Option<String>) {
    (address.to_string(), contract.map(str::to_string))
}

fn link(address: &str, provider: &str, reason: &str) -> (String, String, String, String) {
    (
        address.into(),
        provider.into(),
        "exact".into(),
        reason.into(),
    )
}

/// A pipeline that is not reusable, whose CI job checks its own repository out at the default
/// path: every form a CI step takes. `actions/checkout`, `docker://`, a path that leaves the
/// root and an action of `other-org` hold no side.
#[test]
fn a_ci_step_consumes_the_action_its_uses_names() {
    let [workflows, _, app_a, ..] = reproduction("ci_uses_steps");
    let export = exported(&Engine::open(&app_a).unwrap());
    let own = format!("ci::{APP_A}/tools/local");
    assert_eq!(
        consumers(&export, FORMS),
        [
            pair("jobs.actions.steps[1]", Some(&format!("ci::{WORKFLOWS}"))),
            pair(
                "jobs.actions.steps[2]",
                Some(&format!("ci::{WORKFLOWS}/tools/setup"))
            ),
            pair("jobs.actions.steps[3]", Some(&own)),
            pair("jobs.actions.steps[4]", Some(&own)),
            pair("jobs.elsewhere.steps[1]", None),
            pair("jobs[\"at-a-path\"].steps[2]", None),
            pair("jobs[\"no-checkout\"].steps[0]", None),
        ]
    );
    let at = indexed(&workflows);
    let local = format!("{APP_A}::tools/local/action.yml::.");
    assert_eq!(
        links(&export, FORMS),
        [
            link(
                "jobs.actions.steps[1]",
                &format!("{WORKFLOWS}::action.yml::."),
                &format!("@v1 (indexed {at})")
            ),
            link(
                "jobs.actions.steps[2]",
                &format!("{WORKFLOWS}::tools/setup/action.yml::."),
                &format!("@v2 (indexed {at})")
            ),
            // The own repository, with no ref to show or to compare.
            link("jobs.actions.steps[3]", &local, "own repository"),
            link("jobs.actions.steps[4]", &local, "own repository"),
        ]
    );
}

/// A reusable pipeline and two composite actions: a local call and `$/` link inside their
/// repository, and a CI step's `./<folder>` stays unresolved in both.
#[test]
fn a_local_call_and_a_rooted_call_link_inside_their_repository() {
    let [workflows, ..] = reproduction("ci_uses_local");
    let export = exported(&Engine::open(&workflows).unwrap());
    let contract = |path: &str| format!("ci::{WORKFLOWS}/{path}");
    assert_eq!(
        consumers(&export, FORMS),
        [
            pair("jobs.local", Some(&contract(".github/workflows/lint.yml"))),
            pair("jobs.rooted", Some(&contract(".github/workflows/scan.yml"))),
            pair("jobs.steps.steps[1]", None),
            pair("jobs.steps.steps[2]", Some(&contract("tools/setup"))),
        ]
    );
    assert_eq!(
        consumers(&export, "action.yml"),
        [
            pair("runs.steps[0]", Some(&contract("tools/setup"))),
            pair("runs.steps[1]", None),
        ]
    );
    let site = |path: &str| format!("{WORKFLOWS}::{path}::.");
    assert_eq!(
        links(&export, FORMS),
        [
            link(
                "jobs.local",
                &site(".github/workflows/lint.yml"),
                "own repository"
            ),
            link(
                "jobs.rooted",
                &site(".github/workflows/scan.yml"),
                "own repository"
            ),
            link(
                "jobs.steps.steps[2]",
                &site("tools/setup/action.yml"),
                "own repository"
            ),
        ]
    );
    // A call that names its own repository by owner and name writes a ref as any peer's does.
    assert_eq!(
        links(&export, "tools/setup/action.yml"),
        [link(
            "runs.steps[0]",
            &site("tools/login/action.yml"),
            &format!("@v1 (indexed {})", indexed(&workflows))
        )]
    );
}

/// An action at the root has the repository identity alone as its path, one in a folder the
/// folder, and a folder with both file names has one provider: `action.yml`.
#[test]
fn an_action_provides_the_contract_of_its_folder() {
    let [workflows, ..] = reproduction("ci_uses_providers");
    let export = exported(&Engine::open(&workflows).unwrap());
    let rows = export["providers"].as_array().unwrap().iter();
    let actions: Vec<(&str, &str)> = rows
        .map(|row| {
            (
                row["contract"].as_str().unwrap(),
                row["site"].as_str().unwrap(),
            )
        })
        .filter(|(_, site)| !site.contains(".github/workflows/"))
        .collect();
    assert_eq!(
        actions,
        [
            (
                "ci::github.com/acme/workflows",
                "github.com/acme/workflows::action.yml::."
            ),
            (
                "ci::github.com/acme/workflows/tools/login",
                "github.com/acme/workflows::tools/login/action.yml::."
            ),
            (
                "ci::github.com/acme/workflows/tools/setup",
                "github.com/acme/workflows::tools/setup/action.yml::."
            ),
        ]
    );
}

#[test]
fn context_on_an_action_lists_every_ci_step_that_calls_it() {
    let [workflows, ..] = reproduction("ci_uses_context");
    let engine = Engine::open(&workflows).unwrap();
    let answer = engine.context("tools/setup/action.yml", None).unwrap();
    assert_eq!(
        listed(&answer, "provides "),
        [format!("ci::{WORKFLOWS}/tools/setup")],
        "{answer}"
    );
    let at = indexed(&workflows);
    assert_eq!(
        listed(&answer, "  consumer: "),
        [
            format!(
                "{APP_A} {FORMS}:9 jobs.actions.steps[2] [exact: @v2 (indexed {at})] with: version"
            ),
            format!("{WORKFLOWS} {FORMS}:15 jobs.steps.steps[2] [exact: own repository]"),
            format!("{WORKFLOWS} action.yml:6 runs.steps[0] [exact: own repository]"),
        ],
        "{answer}"
    );
    // The calling side names the contract without a ref, then its provider.
    let step = engine
        .context(&format!("{WORKFLOWS}::action.yml::runs.steps[0]"), None)
        .unwrap();
    assert_eq!(
        listed(&step, "consumes "),
        [format!("ci::{WORKFLOWS}/tools/setup")],
        "{step}"
    );
    assert_eq!(
        listed(&step, "  provider: "),
        [format!(
            "{WORKFLOWS} tools/setup/action.yml:1 . [exact: own repository]"
        )],
        "{step}"
    );
}

/// The five positions of a CI step's `./<folder>`: the one where the CI runner's workspace is
/// known to be the file's repository is covered above; the others are open items with no
/// candidate, also when the workspace holds a route any unresolved call could reach.
#[test]
fn a_steps_local_folder_is_an_open_item_where_the_workspace_is_not_known() {
    let [workflows, _, app_a, ..] = reproduction("ci_uses_unresolved");
    let route = app_a.join("app/api/health/route.ts");
    fs::create_dir_all(route.parent().unwrap()).unwrap();
    fs::write(
        &route,
        "export async function GET() {\n  return Response.json({});\n}\n",
    )
    .unwrap();
    git(&app_a, &["add", "-A"]);
    git(&app_a, &["commit", "-q", "-m", "a route"]);
    let engine = Engine::open(&app_a).unwrap();
    let routes = engine.search("health", Some("contract"), None).unwrap();
    assert!(routes.contains("http::GET::/api/health"), "{routes}");
    let item = |repository: &str, place: &str, address: &str, cause: &str| {
        let (path, _) = place.rsplit_once(':').unwrap();
        format!(
            "{repository} {place} (unresolved) candidates: 0; {cause} (`{repository}::{path}::{address}`)"
        )
    };
    let reusable = "a checkout in a reusable pipeline is of its caller's repository";
    let composite = "a composite action runs in the workspace of its caller";
    let no_checkout = "no checkout before the CI step";
    let named = "a checkout before the CI step names `repository` or `path`";
    let answer = engine.search("uses", Some("unresolved"), None).unwrap();
    assert_eq!(
        answer.lines().collect::<Vec<_>>(),
        [
            item(
                APP_A,
                &format!("{FORMS}:30"),
                "jobs.elsewhere.steps[1]",
                named
            ),
            item(
                APP_A,
                &format!("{FORMS}:38"),
                "jobs[\"at-a-path\"].steps[2]",
                named
            ),
            item(
                APP_A,
                &format!("{FORMS}:22"),
                "jobs[\"no-checkout\"].steps[0]",
                no_checkout
            ),
            item(
                WORKFLOWS,
                &format!("{FORMS}:14"),
                "jobs.steps.steps[1]",
                reusable
            ),
            item(WORKFLOWS, "action.yml:7", "runs.steps[1]", composite),
        ],
        "{answer}"
    );
    for (start, uses, cause) in [
        (
            format!("{APP_A}::{FORMS}::jobs[\"no-checkout\"].steps[0]"),
            "./tools/local",
            no_checkout,
        ),
        (
            format!("{WORKFLOWS}::action.yml::runs.steps[1]"),
            "./tools/setup",
            composite,
        ),
    ] {
        let answer = engine.context(&start, None).unwrap();
        assert_eq!(
            listed(&answer, "consumes "),
            [format!("(unresolved) {uses}: {cause}")],
            "{answer}"
        );
        assert!(listed(&answer, "  provider: ").is_empty(), "{answer}");
    }
    // `docker://` and a path that leaves the root are no open item either.
    for other in ["docker", "outside", "alpine"] {
        let answer = engine.search(other, Some("unresolved"), None).unwrap();
        assert_eq!(answer, format!("no matches for `{other}`\n"));
    }
    // An open item of the kind is no contract to list: the default search and the kind
    // `contract` answer as before it.
    for kind in [None, Some("contract")] {
        let answer = engine.search("steps", kind, None).unwrap();
        assert!(!answer.contains("(unresolved)"), "{answer}");
    }
    // `meada decide` shows the cause and offers nothing.
    let (list, _, ok) = meada(&app_a, &["decide", "--list"], "", |_| {});
    let shown = format!("{FORMS}:22  - uses: ./tools/local\n  {no_checkout}\n");
    assert!(ok && list.contains(&shown), "{list}");
    let _ = workflows;
}

/// GitHub reads an input's key in any case (`INPUT_REPOSITORY`): a checkout that writes
/// `REPOSITORY` or `Path` names what one in lower case does.
#[test]
fn a_checkout_names_its_inputs_in_any_case() {
    let [_, _, app_a, ..] = reproduction("ci_uses_key_case");
    for key in ["REPOSITORY: acme/workflows", "Path: sub"] {
        let pipeline = format!(
            "on: push\njobs:\n  cased:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n        with:\n          {key}\n      - uses: ./tools/local\n"
        );
        fs::write(app_a.join(".github/workflows/cased.yml"), pipeline).unwrap();
        let engine = Engine::open(&app_a).unwrap();
        let step = ".github/workflows/cased.yml::jobs.cased.steps[1]";
        let answer = engine.context(step, None).unwrap();
        assert_eq!(
            listed(&answer, "consumes "),
            [
                "(unresolved) ./tools/local: a checkout before the CI step names `repository` or `path`"
            ],
            "{key}: {answer}"
        );
    }
}

/// A repository without `origin` names no host: its local calls link, its calls by owner and
/// name hold no side.
#[test]
fn a_repository_without_origin_links_its_local_calls() {
    let root = fixture_repo("ci_uses_no_origin", "ci-workflows", None);
    let export = exported(&Engine::open(&root).unwrap());
    let identity = export["repository"].as_str().unwrap();
    assert!(identity.starts_with("local/"), "{identity}");
    let site = |path: &str| format!("{identity}::{path}::.");
    assert_eq!(
        links(&export, FORMS),
        [
            link(
                "jobs.local",
                &site(".github/workflows/lint.yml"),
                "own repository"
            ),
            link(
                "jobs.rooted",
                &site(".github/workflows/scan.yml"),
                "own repository"
            ),
            link(
                "jobs.steps.steps[2]",
                &site("tools/setup/action.yml"),
                "own repository"
            ),
        ]
    );
    assert!(consumers(&export, "tools/setup/action.yml").is_empty());
}

/// `acme/app-a` calls the action `tools/setup`, whose CI step calls the action `tools/login`:
/// the chain crosses both, as it crosses reusable pipelines.
#[test]
fn trace_follows_a_chain_through_composite_actions() {
    let [workflows, _, app_a, ..] = reproduction("ci_uses_trace");
    let engine = Engine::open(&workflows).unwrap();
    let at = indexed(&workflows);
    let up = traced(&engine, "tools/login/action.yml", Direction::Up);
    let setup = "tools/setup/action.yml";
    assert_eq!(
        chain(&up),
        [
            format!("{WORKFLOWS} tools/login/action.yml:1 ."),
            format!("  →[exact: @v1 (indexed {at})] {WORKFLOWS} {setup}:6 runs.steps[0]"),
            format!("    {WORKFLOWS} {setup}:1 ."),
            format!("      →[exact: @v2 (indexed {at})] {APP_A} {FORMS}:9 jobs.actions.steps[2]"),
            format!("        {APP_A} {FORMS}:4 jobs.actions"),
            format!("          {APP_A} {FORMS}:1 ."),
            format!("      →[exact: own repository] {WORKFLOWS} {FORMS}:15 jobs.steps.steps[2]"),
            format!("        {WORKFLOWS} {FORMS}:10 jobs.steps"),
            format!("          {WORKFLOWS} {FORMS}:1 ."),
            format!("      →[exact: own repository] {WORKFLOWS} action.yml:6 runs.steps[0]"),
            format!("        {WORKFLOWS} action.yml:1 ."),
            format!(
                "          →[exact: @v1 (indexed {at})] {APP_A} {FORMS}:8 jobs.actions.steps[1]"
            ),
            format!("            {APP_A} {FORMS}:4 jobs.actions (see above)"),
        ],
        "{up}"
    );
    let down = traced(
        &Engine::open(&app_a).unwrap(),
        &format!("{APP_A}::{FORMS}::jobs.actions.steps[2]"),
        Direction::Down,
    );
    assert_eq!(
        chain(&down),
        [
            format!("{APP_A} {FORMS}:9 jobs.actions.steps[2]"),
            format!("  →[exact: @v2 (indexed {at})] {WORKFLOWS} {setup}:1 ."),
            format!("    {WORKFLOWS} {setup}:6 runs.steps[0]"),
            format!("      →[exact: @v1 (indexed {at})] {WORKFLOWS} tools/login/action.yml:1 ."),
            format!("    {WORKFLOWS} {setup}:9 runs.steps[1]"),
        ],
        "{down}"
    );
}

/// The answers on the forms, whole: what the output grammar documents of them.
#[test]
fn the_forms_answer_as_their_goldens() {
    let [workflows, _, app_a, ..] = reproduction("ci_uses_goldens");
    let engine = Engine::open(&app_a).unwrap();
    let questions = [
        format!("{WORKFLOWS}::tools/setup/action.yml"),
        format!("{APP_A}::{FORMS}::jobs.actions.steps[3]"),
        format!("{APP_A}::{FORMS}::jobs[\"no-checkout\"].steps[0]"),
        format!("{WORKFLOWS}::{FORMS}::jobs.local"),
    ];
    let answers = questions.map(|q| engine.context(&q, None).unwrap());
    assert_golden("ci-uses.context.txt", &answers.join("\n"));
    let open = engine.search("uses", Some("unresolved"), None).unwrap();
    assert_golden("ci-uses.search-unresolved.txt", &open);
    let up = traced(&engine, "tools/login/action.yml", Direction::Up);
    let start = format!("{APP_A}::{FORMS}::jobs.actions");
    let down = traced(&engine, &start, Direction::Down);
    assert_golden("ci-uses.trace.txt", &format!("{up}\n{down}"));
    let _ = workflows;
}
