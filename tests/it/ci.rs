//! CI nodes (#823, ADR 0028): a pipeline, an action, a CI job and a CI step are IaC nodes of the
//! GitHub Actions format, found by `search` with the kind `ci` and explained by `context`.

use std::fs;

use meada::engine::Engine;

use crate::common::{assert_golden, fixture_repo};

const ORIGIN: Option<&str> = Some("https://example.com/acme/app.git");
const REPO: &str = "example.com/acme/app";
const CI: &str = ".github/workflows/ci.yml";

/// The answers to `questions`, a blank line between two, as the output grammar's check reads a
/// golden of several answers.
fn answers(questions: &[&str], ask: impl Fn(&str) -> String) -> String {
    let answers: Vec<_> = questions.iter().map(|q| ask(q)).collect();
    answers.join("\n")
}

/// The lines of `answer` that are hits of the kind `ci`: every line, each of the shape
/// `<repository> <path>:<line> <address> (<type>...)`.
fn hits(answer: &str) -> Vec<&str> {
    let lines: Vec<&str> = answer.lines().collect();
    for line in &lines {
        assert!(line.starts_with(&format!("{REPO} ")), "{answer}");
    }
    lines
}

/// The fixture: a pipeline with three CI jobs (one with a matrix and three CI steps, one that
/// needs it, one that calls a reusable pipeline and needs both), a composite action at the
/// root, a folder with both action file names, one with `action.yaml` alone, a `dependabot.yml`
/// that is no pipeline, and a symbol named like a CI job.
#[test]
fn pipelines_actions_jobs_and_steps_are_nodes_found_by_the_kind_ci() {
    let repo = fixture_repo("ci_nodes", "ci-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let ci = |q: &str| engine.search(q, Some("ci"), None).unwrap();
    // A CI step by its `id`, its `name` and its `uses`: the same node, at its key path.
    let login = format!(
        "{REPO} {CI}:15 jobs.build.steps[1] (step, id \"login\", name \"Log in\", uses \"docker/login-action@v3\")"
    );
    for query in ["login", "Log in", "docker/login-action@v3"] {
        assert_eq!(hits(&ci(query)), [&login], "{query}");
    }
    // A CI job by its id: one node for the job with a matrix, and a quoted key where the id is
    // no plain identifier.
    let build = ci("build");
    let jobs: Vec<_> = (hits(&build).into_iter())
        .filter(|line| line.contains(" (job"))
        .collect();
    assert_eq!(
        jobs,
        [format!(
            "{REPO} {CI}:7 jobs.build (job, id \"build\", name \"Build\")"
        )]
    );
    assert_eq!(
        hits(&ci("publish-prd")),
        [format!(
            "{REPO} {CI}:33 jobs[\"publish-prd\"] (job, id \"publish-prd\", uses \"acme/workflows/.github/workflows/publish.yml@v1\")"
        )]
    );
    // Every node of a type, counted over the whole answer.
    let of = |answer: &str, ty: &str| {
        (hits(answer).iter())
            .filter(|line| line.contains(&format!(" ({ty}")))
            .count()
    };
    // A type word also matches the addresses under `jobs`: the CI jobs come first, by rank.
    let by_type = ci("job");
    assert_eq!(of(&by_type, "job"), 3, "{by_type}");
    assert!(
        hits(&by_type)[..3]
            .iter()
            .all(|line| line.contains(" (job")),
        "{by_type}"
    );
    assert_eq!(hits(&by_type).len(), 3 + 4, "{by_type}");
    // Three in `build`, one in `test`, two in the root action, one in `tools/setup/action.yml`:
    // the `action.yaml` beside it gives none, and a JavaScript action has no steps.
    assert_eq!(of(&ci("step"), "step"), 7, "{}", ci("step"));
    assert_eq!(hits(&ci("step")).len(), 7, "{}", ci("step"));
    let actions = ci("action");
    assert_eq!(of(&actions, "action"), 3, "{actions}");
    assert!(!actions.contains("tools/setup/action.yaml"), "{actions}");
    assert!(
        actions.contains(&format!("{REPO} tools/only/action.yaml:1 . (action")),
        "{actions}"
    );
    assert_eq!(of(&ci("workflow"), "workflow"), 1);
    let queries = [
        "workflow",
        "action",
        "job",
        "step",
        "build",
        "Log in",
        "nothing-here",
    ];
    assert_golden("ci-nodes.search.txt", &answers(&queries, ci));

    let context = |q: &str| engine.context(q, None).unwrap();
    let pipeline = context(CI);
    assert!(
        pipeline.starts_with(&format!(
            "{REPO} {CI}:1-37 .\ntype: workflow\nname: \"CI\"\n"
        )),
        "{pipeline}"
    );
    let listed = format!(
        "references:\n{REPO} {CI}:7 jobs.build\n{REPO} {CI}:28 jobs.test\n{REPO} {CI}:33 jobs[\"publish-prd\"]\nreferenced by: none\n"
    );
    assert!(pipeline.contains(&listed), "{pipeline}");
    // A CI job: its CI steps and the CI jobs it needs below, the pipeline and who needs it above.
    let test = context("jobs.test");
    let (down, up) = test.split_once("referenced by").unwrap();
    assert!(
        down.starts_with(&format!("{REPO} {CI}:28-32 jobs.test\n")),
        "{test}"
    );
    let needed = format!(
        "references:\n{REPO} {CI}:7 jobs.build via needs\n{REPO} {CI}:32 jobs.test.steps[0]\n"
    );
    assert!(down.ends_with(&needed), "{test}");
    let needing =
        format!(":\n{REPO} {CI}:1 .\n{REPO} {CI}:33 jobs[\"publish-prd\"] via needs\nvalues:\n");
    assert!(up.starts_with(&needing), "{test}");
    // A CI step, named by a selector: its lines and the values of its `with` and `env`.
    let step = context("jobs.build.steps[id=login]");
    assert!(
        step.starts_with(&format!(
            "{REPO} {CI}:15-22 jobs.build.steps[1]\ntype: step\n"
        )),
        "{step}"
    );
    for value in [
        "jobs.build.steps[1].with.registry",
        "ghcr.io",
        "jobs.build.steps[1].env.MODE",
        "release",
    ] {
        assert!(step.contains(value), "{step}");
    }
    assert!(!step.contains("make build"), "{step}");
    assert_eq!(
        step,
        context(&format!(
            "{CI}::jobs.build.steps[uses=docker/login-action@v3]"
        ))
    );
    assert_eq!(
        step,
        context(&format!("{REPO}::{CI}::jobs.build.steps[name=\"Log in\"]"))
    );
    // A composite action shows its CI steps; the `action.yaml` beside an `action.yml` is no node.
    let action = context("action.yml");
    assert!(
        action.starts_with(&format!(
            "{REPO} action.yml:1-9 .\ntype: action\nname: \"Root action\"\n"
        )),
        "{action}"
    );
    let steps = format!(
        "references:\n{REPO} action.yml:6 runs.steps[0]\n{REPO} action.yml:9 runs.steps[1]\n"
    );
    assert!(action.contains(&steps), "{action}");
    assert_eq!(
        context("tools/setup/action.yaml"),
        context("nothing-here").replace("nothing-here", "tools/setup/action.yaml")
    );
    let questions = [
        CI,
        "jobs.build",
        "jobs.test",
        "jobs[\"publish-prd\"]",
        "jobs.build.steps[id=login]",
        "action.yml",
        "tools/setup/action.yml",
        "tools/only/action.yaml",
        "runs.steps[0]",
    ];
    assert_golden("ci-nodes.context.txt", &answers(&questions, context));
}

/// The kind `iac`, the default search and the kind `value` answer as before, and the export
/// holds only the CI nodes that hold a side (#826): the three actions, and the CI step of a
/// composite action whose `./tools/setup` is unresolved.
#[test]
fn ci_nodes_stay_out_of_the_kind_iac_the_default_search_and_the_export() {
    let repo = fixture_repo("ci_nodes_others", "ci-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    for query in ["build", "job", "step", "workflow", "action", "login"] {
        let iac = engine.search(query, Some("iac"), None).unwrap();
        assert!(!iac.contains(REPO), "{iac}");
    }
    // A symbol named like the CI job: the default search shows it and no CI node.
    let default = engine.search("build", None, None).unwrap();
    assert!(default.contains("src/build.ts"), "{default}");
    assert!(
        !default.contains("jobs.build") && !default.contains(CI),
        "{default}"
    );
    // Every scalar of a pipeline stays a config value.
    let value = engine
        .search("docker/login-action@v3", Some("value"), None)
        .unwrap();
    assert!(value.contains("jobs.build.steps[1].uses"), "{value}");
    let others = [
        format!("# default build\n{default}"),
        format!("# value docker/login-action@v3\n{value}"),
        format!(
            "# value make test\n{}",
            engine.search("make test", Some("value"), None).unwrap()
        ),
    ];
    assert_golden("ci-nodes.other-kinds.txt", &others.join("\n"));
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let nodes = export["iac_nodes"].as_array().unwrap().iter();
    let nodes: Vec<&str> = nodes.map(|n| n["identity"].as_str().unwrap()).collect();
    assert_eq!(
        nodes,
        [
            "action.yml::.",
            "action.yml::runs.steps[1]",
            "tools/only/action.yaml::.",
            "tools/setup/action.yml::."
        ]
        .map(|node| format!("{REPO}::{node}"))
    );
    assert!(export["files"].is_array(), "{export:#}");
}

/// Editing a pipeline changes the answers at the next query, as a cold index gives them: a CI
/// step inserted above another renumbers it, a file that leaves takes its nodes, and with both
/// action file names in a folder the action follows `action.yml` as it leaves and returns.
#[test]
fn an_edited_pipeline_or_action_changes_its_nodes_as_a_cold_index_gives_them() {
    let repo = fixture_repo("ci_nodes_edit", "ci-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let cold = |q: &str, kind: &str| {
        Engine::open(&repo)
            .unwrap()
            .search(q, Some(kind), None)
            .unwrap()
    };
    let ci = |q: &str| engine.search(q, Some("ci"), None).unwrap();
    assert!(ci("login").contains("jobs.build.steps[1] "));
    let path = repo.join(CI);
    let text = fs::read_to_string(&path).unwrap();
    let inserted = text.replace(
        "      - id: login\n",
        "      - run: echo first\n      - id: login\n",
    );
    assert_ne!(text, inserted);
    fs::write(&path, inserted).unwrap();
    let warm = ci("login");
    assert!(
        warm.contains(&format!("{CI}:16 jobs.build.steps[2] (step, id \"login\"")),
        "{warm}"
    );
    assert_eq!(warm, cold("login", "ci"));
    assert_eq!(ci("step"), cold("step", "ci"));
    // Both action file names in `tools/setup`: the action is `action.yml` until it leaves.
    let yml = repo.join("tools/setup/action.yml");
    let setup = fs::read_to_string(&yml).unwrap();
    assert!(ci("Setup").contains("tools/setup/action.yml:1 . (action, name \"Setup\")"));
    assert!(!ci("Setup").contains("tools/setup/action.yaml"));
    fs::remove_file(&yml).unwrap();
    let left = ci("Setup");
    assert!(
        left.contains("tools/setup/action.yaml:1 . (action, name \"Setup shadowed\")"),
        "{left}"
    );
    assert!(!left.contains("tools/setup/action.yml"), "{left}");
    assert_eq!(left, cold("Setup", "ci"));
    assert_eq!(ci("step"), cold("step", "ci"));
    fs::write(&yml, setup).unwrap();
    let back = ci("Setup");
    assert!(
        back.contains("tools/setup/action.yml:1 . (action, name \"Setup\")"),
        "{back}"
    );
    assert!(!back.contains("tools/setup/action.yaml"), "{back}");
    assert_eq!(back, cold("Setup", "ci"));
    assert_eq!(ci("step"), cold("step", "ci"));
    // A file that is no pipeline any more, then gone.
    fs::write(&path, "on: push\n").unwrap();
    assert!(!ci("job").contains(CI), "{}", ci("job"));
    assert_eq!(ci("job"), cold("job", "ci"));
    fs::remove_file(repo.join("action.yml")).unwrap();
    let gone = ci("prepare");
    assert!(!gone.contains("action.yml"), "{gone}");
    assert_eq!(gone, cold("prepare", "ci"));
}
