//! A CI step that runs a console script of the workspace (#832, ADR 0028): the step is a
//! consumer of the script's package and links to the program entry point the script names, at
//! the level its evidence gives. `trace` crosses the link both ways.

use std::fs;
use std::path::PathBuf;

use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

use crate::ci_calls::{exported, indexed};
use crate::common::{assert_golden, fixture_repo};

const SDK: &str = "github.com/acme/core-sdk";
const WORKFLOWS: &str = "github.com/acme/workflows";
const PROGRAMS: &str = ".github/workflows/programs.yml";
const PUBLISH: &str = ".github/workflows/publish.yml";
const CLI: &str = "core_sdk/cli.py";
const TOOLS_CLI: &str = "tools/core_tools/cli.py";
const INSTALLED: &str = "its package installed earlier in the CI job";
const NOT_INSTALLED: &str = "no install of its package earlier in the CI job";
/// The SDK's call to the API, a link Meada found before #832: the base URL is read from the
/// environment, whatever the pipeline sets it to.
const HTTP: &str =
    "inferred: base URL from CORE_API_URL, a base_url_env of peer github.com/acme/core-api";
/// The link from the CI step of `publish.yml` that runs `core-publish` to its program.
const RUNS: &str = "exact: package::pypi::core-sdk none (local 1.0.0), range unchecked; console \
                    script core-publish, its package installed earlier in the CI job";

/// The chain of F2 as siblings in one case directory: the API, the SDK whose console scripts
/// call it, the reusable pipeline that runs them, and the callers of #816's reproduction
/// (`acme/app-a`, and `acme/app-c` through `acme/wrapper`).
fn chain(case: &str) -> [PathBuf; 6] {
    [
        ("core-api", "core-api"),
        ("core-sdk", "core-sdk"),
        ("core-workflows", "workflows"),
        ("wrapper", "wrapper"),
        ("app-a", "app-a"),
        ("app-c", "app-c"),
    ]
    .map(|(fixture, name)| {
        let origin = format!("https://github.com/acme/{name}.git");
        fixture_repo(case, &format!("ci-{fixture}"), Some(&origin))
    })
}

fn program(path: &str, function: &str) -> String {
    format!("{SDK}::{path}::{function}")
}

/// The `(consumer address, provider, level)` of every link that leaves a CI node of `file`.
fn links(export: &Value, file: &str) -> Vec<(String, String, String)> {
    let own = format!("{WORKFLOWS}::{file}::");
    let rows = export["links"].as_array().into_iter().flatten();
    rows.filter_map(|l| {
        let at = |k: &str| l[k].as_str().unwrap().to_string();
        let address = l["consumer"].as_str()?.strip_prefix(&own)?;
        Some((address.to_string(), at("provider"), at("level")))
    })
    .collect()
}

/// The reason of the link from the CI node `address` of `file` to `provider`.
fn reason(export: &Value, file: &str, address: &str, provider: &str) -> String {
    let consumer = format!("{WORKFLOWS}::{file}::{address}");
    let rows = export["links"].as_array().unwrap().iter();
    let found: Vec<_> = rows
        .filter(|l| l["consumer"] == *consumer && l["provider"] == *provider)
        .collect();
    assert_eq!(found.len(), 1, "{consumer} -> {provider}: {found:#?}");
    found[0]["reason"].as_str().unwrap().to_string()
}

/// The `(address, contract, via)` of the consumer rows at the CI nodes of `file`.
fn sides(export: &Value, file: &str) -> Vec<(String, String, Option<String>)> {
    let own = format!("{WORKFLOWS}::{file}::");
    let rows = export["consumers"].as_array().into_iter().flatten();
    rows.filter_map(|row| {
        let address = row["site"].as_str()?.strip_prefix(&own)?;
        let contract = row["contract"].as_str()?.strip_prefix("package::pypi::")?;
        let via = row["via"].as_str().map(str::to_string);
        Some((address.to_string(), contract.to_string(), via))
    })
    .collect()
}

fn traced(engine: &Engine, from: &str, direction: Direction) -> String {
    engine.trace(Start::From(from), direction, None).unwrap()
}

/// The level table of #832, one CI job of `programs.yml` per row.
#[test]
fn a_program_link_has_the_level_its_evidence_gives() {
    let [_, _, workflows, ..] = chain("ci_program_levels");
    let export = exported(&Engine::open(&workflows).unwrap());
    let row = |address: &str, provider: String, level: &str| {
        (address.to_string(), provider, level.to_string())
    };
    let (sdk, tools) = (
        format!("{SDK}::pyproject.toml"),
        format!("{SDK}::tools/pyproject.toml"),
    );
    // Every link that leaves the file: an install links to its package's manifest, a console
    // script to its program, and `jobs.outside` (a command no package declares) to nothing.
    let mut expected = vec![
        row("jobs.earlier_step.steps[0]", tools.clone(), "exact"),
        row(
            "jobs.earlier_step.steps[1]",
            program(TOOLS_CLI, "lint_main"),
            "exact",
        ),
        row("jobs.earlier_line.steps[0]", sdk.clone(), "exact"),
        row(
            "jobs.earlier_line.steps[0]",
            program(CLI, "health_main"),
            "exact",
        ),
        row("jobs.by_package.steps[0]", program(CLI, "main"), "exact"),
        row("jobs.by_package.steps[1]", program(CLI, "main"), "exact"),
        // `uvx core-tools`: the package declares no script of its own name.
        row("jobs.no_script.steps[0]", tools.clone(), "exact"),
        row(
            "jobs.no_install.steps[0]",
            program(TOOLS_CLI, "lint_main"),
            "inferred",
        ),
        row(
            "jobs.declared_twice.steps[0]",
            program(CLI, "health_main"),
            "ambiguous",
        ),
        row(
            "jobs.declared_twice.steps[0]",
            program(TOOLS_CLI, "health_main"),
            "ambiguous",
        ),
        row("jobs.picked.steps[0]", tools.clone(), "exact"),
        row(
            "jobs.picked.steps[0]",
            program(TOOLS_CLI, "health_main"),
            "exact",
        ),
        row(
            "jobs.later.steps[0]",
            program(CLI, "publish_main"),
            "inferred",
        ),
        row("jobs.later.steps[1]", sdk.clone(), "exact"),
        row(
            "jobs.elsewhere.steps[1]",
            program(TOOLS_CLI, "lint_main"),
            "inferred",
        ),
    ];
    expected.sort();
    let mut found = links(&export, PROGRAMS);
    found.sort();
    assert_eq!(found, expected);
    // The reason names the script and the evidence, after the package's own reason.
    let said = |address: &str, provider: &str| reason(&export, PROGRAMS, address, provider);
    assert_eq!(
        said(
            "jobs.earlier_step.steps[1]",
            &program(TOOLS_CLI, "lint_main")
        ),
        format!(
            "package::pypi::core-tools none (local 0.2.0), range unchecked; console script core-lint, {INSTALLED}"
        )
    );
    // The range is the install's.
    assert_eq!(
        said("jobs.earlier_line.steps[0]", &program(CLI, "health_main")),
        format!(
            "package::pypi::core-sdk >=1.0 (local 1.0.0); console script core-health, {INSTALLED}"
        )
    );
    assert_eq!(
        said("jobs.by_package.steps[0]", &program(CLI, "main")),
        "package::pypi::core-sdk none (local 1.0.0), range unchecked; console script core-sdk, \
         run by its package's name"
    );
    assert_eq!(
        said("jobs.no_install.steps[0]", &program(TOOLS_CLI, "lint_main")),
        format!(
            "package::pypi::core-tools none (local 0.2.0), range unchecked; console script core-lint, {NOT_INSTALLED}"
        )
    );
    assert_eq!(
        said("jobs.declared_twice.steps[0]", &program(CLI, "health_main")),
        "package::pypi::core-sdk none (local 1.0.0), range unchecked; console script \
         core-health, 2 packages declare it"
    );
}

/// A name that more packages declare than the limit (`MAX_DECLARING`, 8) tells nothing: the CI
/// step holds no side, where eight packages give it an `ambiguous` link each.
#[test]
fn a_script_name_too_many_packages_declare_gives_no_side() {
    let file = ".github/workflows/many.yml";
    let declaring = |packages: usize| {
        let [_, sdk, workflows, ..] = chain(&format!("ci_program_many_{packages}"));
        let write = |root: &PathBuf, path: String, text: String| {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        for n in 0..packages {
            let manifest = format!(
                "[project]\nname = \"dup-{n}\"\nversion = \"1.0.0\"\n\n[project.scripts]\n\
                 dup-tool = \"dup_{n}.cli:main\"\n"
            );
            write(&sdk, format!("dup{n}/pyproject.toml"), manifest);
            let source = "def main():\n    pass\n".to_string();
            write(&sdk, format!("dup{n}/dup_{n}/cli.py"), source);
        }
        let pipeline = "name: many\non: push\njobs:\n  run:\n    runs-on: ubuntu-latest\n    \
                        steps:\n      - run: dup-tool\n";
        write(&workflows, file.to_string(), pipeline.to_string());
        exported(&Engine::open(&workflows).unwrap())
    };
    let export = declaring(8);
    let found = links(&export, file);
    assert_eq!(found.len(), 8, "{found:?}");
    assert!(found.iter().all(|(_, _, level)| level == "ambiguous"));
    let export = declaring(9);
    assert_eq!(links(&export, file), []);
    assert_eq!(sides(&export, file), []);
}

/// The program link is never stronger than the package link it continues: a package of the CI
/// step's own repository is `inferred` (the package kind's rule for an entry with no source),
/// install or not.
#[test]
fn a_program_of_the_ci_steps_own_repository_links_inferred() {
    let [_, sdk, ..] = chain("ci_program_own");
    let file = ".github/workflows/self.yml";
    let pipeline = "name: self\non: push\njobs:\n  smoke:\n    runs-on: ubuntu-latest\n    \
                    steps:\n      - run: pip install core-sdk && core-publish x\n";
    fs::create_dir_all(sdk.join(".github/workflows")).unwrap();
    fs::write(sdk.join(file), pipeline).unwrap();
    let export = exported(&Engine::open(&sdk).unwrap());
    let step = format!("{SDK}::{file}::jobs.smoke.steps[0]");
    let rows = export["links"].as_array().unwrap().iter();
    let mut found: Vec<_> = rows
        .filter(|l| l["consumer"] == *step)
        .map(|l| {
            let at = |k: &str| l[k].as_str().unwrap().to_string();
            (at("provider"), at("level"), at("reason"))
        })
        .collect();
    found.sort();
    // The package kind's own reason for an install with no source in the package's repository.
    let package = "no uv source names the local package, the registry package is the likely \
                   provider; package::pypi::core-sdk none (local 1.0.0), range unchecked";
    assert_eq!(
        found,
        [
            (
                program(CLI, "publish_main"),
                "inferred".to_string(),
                format!("{package}; console script core-publish, {INSTALLED}")
            ),
            (
                format!("{SDK}::pyproject.toml"),
                "inferred".to_string(),
                package.to_string()
            ),
        ]
    );
}

/// The command is recorded as written on the consumer row, after `run`.
#[test]
fn a_program_consumer_records_its_command_as_text() {
    let [_, _, workflows, ..] = chain("ci_program_sides");
    let export = exported(&Engine::open(&workflows).unwrap());
    let side = |address: &str, package: &str, via: Option<&str>| {
        (
            address.to_string(),
            package.to_string(),
            via.map(str::to_string),
        )
    };
    assert_eq!(
        sides(&export, PUBLISH),
        [
            side("jobs.publish.steps[1]", "core-sdk", None),
            side(
                "jobs.publish.steps[2]",
                "core-sdk",
                Some("run core-publish ${{ inputs.package }}")
            ),
        ]
    );
    let found = sides(&export, PROGRAMS);
    let at = |address: &str| -> Vec<_> {
        (found.iter().filter(|s| s.0 == address))
            .map(|s| (s.1.as_str(), s.2.as_deref()))
            .collect()
    };
    assert_eq!(
        at("jobs.earlier_line.steps[0]"),
        [
            ("core-sdk", None),
            ("core-sdk", Some("run core-health --wait 5"))
        ]
    );
    assert_eq!(
        at("jobs.by_package.steps[1]"),
        [("core-sdk", Some("run pipx run core-sdk status"))]
    );
    // A package run by its name with no script of that name: a consumer, as an install is.
    assert_eq!(at("jobs.no_script.steps[0]"), [("core-tools", None)]);
    // A script two packages declare: a consumer of each.
    assert_eq!(
        at("jobs.declared_twice.steps[0]"),
        [
            ("core-sdk", Some("run core-health")),
            ("core-tools", Some("run core-health"))
        ]
    );
    assert_eq!(at("jobs.outside.steps[0]"), []);
    // No open item: a command no package declares is silent.
    let unresolved = export["consumers"].as_array().unwrap().iter();
    assert_eq!(unresolved.filter(|c| c["contract"].is_null()).count(), 0);
    assert_golden(
        "ci-programs.workflows.export.json",
        &Engine::open(&workflows).unwrap().export().unwrap(),
    );
}

/// `context` shows the link from both ends of the package contract: the CI step names the
/// program it starts, and the package lists a CI step that installs it and runs its script once.
#[test]
fn context_shows_the_program_on_the_ci_step_and_the_ci_step_once_on_the_package() {
    let [_, _, workflows, ..] = chain("ci_program_context");
    let engine = Engine::open(&workflows).unwrap();
    let step = format!("{WORKFLOWS}::{PUBLISH}::jobs.publish.steps[2]");
    let answer = engine.context(&step, None).unwrap();
    let expected = format!(
        "consumes package::pypi::core-sdk\n  provider: {SDK} {CLI}::publish_main [{RUNS}]\n"
    );
    assert!(answer.contains(&expected), "{answer}");
    let answer = engine.context("package::pypi::core-sdk", None).unwrap();
    let dependent = format!("  {WORKFLOWS} dependent {PROGRAMS}:15 jobs.earlier_line.steps[0]");
    let listed = answer.lines().filter(|line| *line == dependent).count();
    assert_eq!(listed, 1, "{answer}");
}

/// The acceptance of F2: upward from the API's route, through the SDK's program and the
/// reusable pipeline that runs it, to the CI jobs that call the pipeline.
#[test]
fn trace_up_from_a_route_reaches_the_ci_jobs_that_run_its_caller() {
    let [api, _, workflows, wrapper, ..] = chain("ci_program_trace_up");
    let engine = Engine::open(&api).unwrap();
    let answer = traced(&engine, "POST /v1/packages", Direction::Up);
    let (at, through) = (indexed(&workflows), indexed(&wrapper));
    let release = ".github/workflows/release.yml";
    let ci = ".github/workflows/ci.yml";
    let lines: Vec<&str> = answer.lines().collect();
    let chain = &lines[..lines.len() - 1];
    assert_eq!(
        chain,
        [
            "github.com/acme/core-api app/main.py:6 register_package".to_string(),
            format!("  →[{HTTP}] {SDK} {CLI}:10 publish_main"),
            // The job of the level table that installs the package after it runs the script.
            format!(
                "    →[inferred: package::pypi::core-sdk none (local 1.0.0), range unchecked; console script core-publish, {NOT_INSTALLED}] {WORKFLOWS} {PROGRAMS}:48 jobs.later.steps[0]"
            ),
            format!("      {WORKFLOWS} {PROGRAMS}:45 jobs.later"),
            format!("        {WORKFLOWS} {PROGRAMS}:1 ."),
            format!("    →[{RUNS}] {WORKFLOWS} {PUBLISH}:20 jobs.publish.steps[2]"),
            format!("      {WORKFLOWS} {PUBLISH}:12 jobs.publish"),
            format!("        {WORKFLOWS} {PUBLISH}:1 ."),
            format!(
                "          →[exact: @main (indexed {at})] github.com/acme/app-a {ci}:12 jobs.publish"
            ),
            format!("            github.com/acme/app-a {ci}:1 ."),
            format!(
                "          →[exact: @main (indexed {at})] github.com/acme/wrapper {release}:5 jobs.release"
            ),
            format!("            github.com/acme/wrapper {release}:1 ."),
            format!(
                "              →[exact: @main (indexed {through})] github.com/acme/app-c {release}:6 jobs.release"
            ),
            format!("                github.com/acme/app-c {release}:1 ."),
        ],
        "{answer}"
    );
    // The weakest level of the chain is the HTTP link's, as before #832.
    let footer = answer.lines().last().unwrap();
    assert!(footer.starts_with("chain: inferred · "), "{answer}");
}

/// Downward from a CI job that calls the pipeline: into the program's process, and on to the
/// route it calls.
#[test]
fn trace_down_from_a_ci_job_enters_the_program_its_pipeline_runs() {
    let [_, _, workflows, wrapper, _, app_c] = chain("ci_program_trace_down");
    let engine = Engine::open(&app_c).unwrap();
    let release = ".github/workflows/release.yml";
    let start = format!("github.com/acme/app-c::{release}::jobs.release");
    let answer = traced(&engine, &start, Direction::Down);
    let (at, through) = (indexed(&workflows), indexed(&wrapper));
    let lines: Vec<&str> = answer.lines().collect();
    assert_eq!(
        &lines[..lines.len() - 1],
        [
            format!("github.com/acme/app-c {release}:6 jobs.release"),
            format!("  →[exact: @main (indexed {through})] github.com/acme/wrapper {release}:1 ."),
            format!("    github.com/acme/wrapper {release}:5 jobs.release"),
            format!("      →[exact: @main (indexed {at})] {WORKFLOWS} {PUBLISH}:1 ."),
            format!("        {WORKFLOWS} {PUBLISH}:12 jobs.publish"),
            format!("          {WORKFLOWS} {PUBLISH}:18 jobs.publish.steps[0]"),
            format!("          {WORKFLOWS} {PUBLISH}:19 jobs.publish.steps[1]"),
            format!("          {WORKFLOWS} {PUBLISH}:20 jobs.publish.steps[2]"),
            format!("            →[{RUNS}] {SDK} {CLI}:9 publish_main"),
            format!(
                "              →[{HTTP}] github.com/acme/core-api app/main.py:6 register_package"
            ),
        ],
        "{answer}"
    );
    assert_golden(
        "ci-programs.trace.txt",
        &format!(
            "{}\n{answer}",
            traced(
                &Engine::open(&workflows).unwrap(),
                "POST /v1/packages",
                Direction::Up
            )
        ),
    );
}
