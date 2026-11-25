//! `meada trace --affected` over the yarn workspace `trace-affected`: what a change may affect,
//! file by file, each file carrying the weakest link of its strongest chain, with what the walk
//! could not place listed in `open`.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::fixture_repo;
use meada::engine::{Changes, Engine};
use serde_json::Value;

fn affected_repo(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "trace-affected",
        Some("https://example.com/scope/mono.git"),
    )
}

/// The affected answer of `changes` in `repo`, as JSON.
fn affected(repo: &Path, changes: Changes) -> Value {
    let answer = Engine::open(repo).unwrap().affected(changes).unwrap();
    serde_json::to_value(answer).unwrap()
}

/// The answer of whole files `paths`.
fn of_files(repo: &Path, paths: &[&str]) -> Value {
    let files: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
    affected(repo, Changes::Files(&files))
}

/// The `path` of each entry of the list `key`.
fn paths(answer: &Value, key: &str) -> Vec<String> {
    (answer[key].as_array().into_iter().flatten())
        .map(|e| e["path"].as_str().unwrap().to_string())
        .collect()
}

/// The `files` entry of `path` in the repository where Meada runs.
fn file<'a>(answer: &'a Value, path: &str) -> Option<&'a Value> {
    in_repo(answer, "example.com/scope/mono", path)
}

/// The `files` entry of `path` in the repository `repo`.
fn in_repo<'a>(answer: &'a Value, repo: &str, path: &str) -> Option<&'a Value> {
    (answer["files"].as_array().into_iter().flatten())
        .find(|f| f["repo"] == repo && f["path"] == path)
}

/// The ids of the `iac` list.
fn iac(answer: &Value) -> Vec<String> {
    (answer["iac"].as_array().into_iter().flatten())
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect()
}

/// The answer of a commit that replaces `old` with `new` in `path`, as a range.
fn edited(repo: &Path, path: &str, old: &str, new: &str) -> Value {
    let file = repo.join(path);
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains(old), "{path}: {old}");
    fs::write(&file, text.replace(old, new)).unwrap();
    commit_all(repo, "edit");
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    affected(repo, range)
}

fn commit_all(repo: &Path, message: &str) {
    common::git(repo, &["add", "-A"]);
    common::git(repo, &["commit", "-q", "-m", message]);
}

const OPENED: [&str; 6] = [
    "packages/a/README.md",
    "yarn.lock",
    "package.json",
    "packages/d/package.json",
    ".github/workflows/ci.yml",
    "gone.ts",
];

#[test]
fn open_lists_what_no_symbol_holds() {
    let repo = affected_repo("affected_open");
    let answer = of_files(&repo, &OPENED);
    let open: Vec<(String, String, Option<String>)> = (answer["open"].as_array().unwrap())
        .iter()
        .map(|e| {
            assert_eq!(e["repo"], "example.com/scope/mono");
            let package = e.get("package").map(|p| p.as_str().unwrap().to_string());
            let reason = e["reason"].as_str().unwrap().to_string();
            (e["path"].as_str().unwrap().to_string(), reason, package)
        })
        .collect();
    let expected = [
        (".github/workflows/ci.yml", "iac", None),
        ("gone.ts", "deleted", None),
        ("package.json", "root", None),
        ("packages/a/README.md", "no-symbols", Some("packages/a")),
        ("packages/d/package.json", "manifest", Some("packages/d")),
        ("yarn.lock", "root", None),
    ]
    .map(|(p, r, k)| (p.to_string(), r.to_string(), k.map(str::to_string)));
    assert_eq!(open, expected);
    let changed: Vec<(&str, &str)> = (answer["changed"].as_array().unwrap())
        .iter()
        .map(|e| (e["path"].as_str().unwrap(), e["change"].as_str().unwrap()))
        .collect();
    assert_eq!(
        changed,
        [
            (".github/workflows/ci.yml", "whole"),
            ("gone.ts", "deleted"),
            ("package.json", "whole"),
            ("packages/a/README.md", "whole"),
            ("packages/d/package.json", "whole"),
            ("yarn.lock", "whole"),
        ]
    );
}

#[test]
fn touched_paths_are_affected() {
    let repo = affected_repo("affected_touched");
    let answer = of_files(&repo, &OPENED);
    let files = paths(&answer, "files");
    for path in paths(&answer, "changed")
        .iter()
        .chain(&paths(&answer, "open"))
    {
        assert!(files.contains(path), "{path} not in {files:?}");
    }
    for file in answer["files"].as_array().unwrap() {
        assert_eq!(file["tier"], "certain", "{file}");
    }
}

/// An `import` line rebinds names used in unchanged functions: the file and every symbol of it
/// are roots.
#[test]
fn a_line_in_no_symbol_roots_the_file_and_its_symbols() {
    let repo = affected_repo("affected_import_line");
    let g = repo.join("packages/b/src/g.ts");
    let text = fs::read_to_string(&g).unwrap();
    fs::write(&g, text.replace("import { f }", "import { f as f }")).unwrap();
    commit_all(&repo, "rebind");
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = affected(&repo, range);
    assert_eq!(answer["changed"][0]["path"], "packages/b/src/g.ts");
    assert_eq!(answer["changed"][0]["change"], "lines");
    assert!(paths(&answer, "files").contains(&"packages/b/src/g.ts".to_string()));
    assert_eq!(answer["open"], Value::Array(Vec::new()));
    // `g` is a root although its lines did not change: `routes` holds a use of it.
    assert!(
        file(&answer, "packages/b/src/routes.ts").is_some(),
        "{answer:#}"
    );
}

#[test]
fn a_workspaces_object_marks_the_root() {
    let repo = affected_repo("affected_workspaces_object");
    fs::write(
        repo.join("package.json"),
        "{\"private\": true, \"workspaces\": {\"packages\": [\"packages/*\"]}}\n",
    )
    .unwrap();
    let answer = of_files(&repo, &["yarn.lock"]);
    assert_eq!(answer["open"][0]["reason"], "root");
    assert!(answer["open"][0].get("package").is_none());
}

#[test]
fn the_cli_prints_the_affected_json() {
    let repo = affected_repo("affected_cli");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_meada"))
            .current_dir(&repo)
            .args(args)
            .env("LANG", "en_US.UTF-8")
            .output()
            .expect("meada runs")
    };
    let out = run(&["trace", "--files", "packages/a/src/f.ts", "--affected"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "changed", "files", "iac", "open", "packages", "reads", "sides"
        ]
    );
    let up = run(&[
        "trace",
        "--files",
        "packages/a/src/f.ts",
        "--affected",
        "--up",
    ]);
    assert_eq!(up.stdout, out.stdout);
    // The affected set needs a change start.
    let refused = run(&["trace", "f", "--affected"]);
    assert!(!refused.status.success());
}

#[test]
fn a_caller_is_affected() {
    let repo = affected_repo("affected_caller");
    let answer = of_files(&repo, &["packages/a/src/f.ts"]);
    let twice = file(&answer, "packages/a/src/twice.ts").expect("twice.ts");
    assert_eq!(twice["level"], "exact");
}

#[test]
fn a_call_on_a_bare_specifier_is_affected_through_its_package_link() {
    let repo = affected_repo("affected_package_link");
    let answer = of_files(&repo, &["packages/a/src/f.ts"]);
    let g = file(&answer, "packages/b/src/g.ts").expect("g.ts");
    assert_eq!(g["level"], "exact");
    assert!(
        g["reason"]
            .as_str()
            .unwrap()
            .starts_with("package::npm::@scope/a"),
        "{g}"
    );
}

#[test]
fn candidate_callers_are_ambiguous_with_their_count() {
    let repo = affected_repo("affected_candidates");
    let answer = of_files(&repo, &["packages/e/src/runners.ts"]);
    let loose = file(&answer, "packages/e/src/loose.ts").expect("loose.ts");
    assert_eq!(loose["level"], "ambiguous");
    assert_eq!(loose["candidates"], 2);
}

#[test]
fn a_decorated_definition_depends_on_its_decorator() {
    let repo = affected_repo("affected_decorator");
    let answer = edited(
        &repo,
        "packages/py/tracking.py",
        "return f\n",
        "return f  # kept\n",
    );
    assert!(file(&answer, "packages/py/app.py").is_some(), "{answer:#}");
}

#[test]
fn a_row_held_by_a_declaration_climbs_to_its_holder() {
    let repo = affected_repo("affected_holder");
    let answer = edited(&repo, "packages/b/src/g.ts", "f() + 1", "f() + 2");
    let routes = file(&answer, "packages/b/src/routes.ts").expect("routes.ts");
    assert_eq!(routes["level"], "exact");
    // The test file reaches `g` from its top level: the file is the caller.
    assert!(file(&answer, "packages/c/src/g.test.ts").is_some());
}

#[test]
fn a_declarative_client_method_is_affected_by_its_route() {
    let (web, _) = common::xlang_java("affected_declarative");
    let answer = of_files(&web, &["app/api/inventory/[sku]/route.ts"]);
    let client = "src/main/java/com/example/api/InventoryClient.java";
    let method = in_repo(&answer, "example.com/xlang-java/api", client);
    assert_eq!(method.expect("the client")["level"], "exact");
}

/// The chain's gateway writes `resource::postgres::orders` and the worker reads it.
#[test]
fn the_reader_of_what_a_writer_writes_is_affected() {
    let (_, gateway, _) = common::chain("affected_writer");
    let answer = of_files(&gateway, &["src/orders.ts"]);
    let reader = in_repo(&answer, "example.com/chain/worker", "src/jobs.ts");
    assert_eq!(reader.expect("the reader")["level"], "declared");
}

#[test]
fn the_writer_of_what_a_reader_reads_is_not() {
    let (web, _, _) = common::chain("affected_reader");
    let worker = common::worker(&web);
    let answer = of_files(&worker, &["src/jobs.ts"]);
    assert!(in_repo(&answer, "example.com/chain/worker", "src/jobs.ts").is_some());
    let gateway = (answer["files"].as_array().unwrap().iter())
        .filter(|f| f["repo"] == "example.com/chain/gateway")
        .count();
    assert_eq!(gateway, 0, "{answer:#}");
}

#[test]
fn a_workload_running_the_code_is_affected_and_ends() {
    let app = fixture_repo(
        "affected_workload",
        "image-app",
        Some("https://example.com/acme/app.git"),
    );
    fixture_repo(
        "affected_workload",
        "image-deploy",
        Some("https://example.com/acme/deploy.git"),
    );
    let answer = of_files(&app, &["orders.py"]);
    let reached = iac(&answer);
    for workload in [
        "example.com/acme/app::compose.yaml::services.orders",
        "example.com/acme/deploy::deployment.yaml::Deployment.apps/shop/prod-orders",
    ] {
        assert!(
            reached.iter().any(|id| id == workload),
            "{workload}: {reached:?}"
        );
    }
}

#[test]
fn a_ci_step_running_the_program_is_affected_with_its_job() {
    let repos = [
        ("core-api", "core-api"),
        ("core-sdk", "core-sdk"),
        ("core-workflows", "workflows"),
    ]
    .map(|(fixture, name)| {
        let origin = format!("https://github.com/acme/{name}.git");
        fixture_repo(
            "affected_ci_program",
            &format!("ci-{fixture}"),
            Some(&origin),
        )
    });
    let answer = of_files(&repos[1], &["core_sdk/cli.py"]);
    let reached = iac(&answer);
    let publish = "github.com/acme/workflows::.github/workflows/publish.yml::";
    for node in ["jobs.publish.steps[2]", "jobs.publish", "."] {
        let id = format!("{publish}{node}");
        assert!(reached.contains(&id), "{id}: {reached:?}");
    }
}

#[test]
fn a_provisioner_of_what_the_code_uses_is_not_affected() {
    let case = "affected_provisioner";
    fixture_repo(
        case,
        "provision-infra",
        Some("https://example.com/provision/infra.git"),
    );
    let app = fixture_repo(
        case,
        "provision-app",
        Some("https://example.com/provision/app.git"),
    );
    let answer = of_files(&app, &["src/producer.ts"]);
    let infra = |e: &&Value| e.to_string().contains("example.com/provision/infra");
    let files = answer["files"].as_array().unwrap();
    assert!(!files.is_empty());
    assert_eq!(files.iter().filter(infra).count(), 0, "{answer:#}");
    assert_eq!(
        answer["iac"]
            .as_array()
            .unwrap()
            .iter()
            .filter(infra)
            .count(),
        0
    );
}

/// A declared key read by an IaC node (no fixture has code reading a config key): the key's
/// line roots its config site, and the node that reads it is affected.
#[test]
fn a_changed_config_key_affects_what_reads_it() {
    let case = "affected_config_key";
    fixture_repo(
        case,
        "resource-via-infra",
        Some("https://example.com/resource-via/infra.git"),
    );
    let app = fixture_repo(
        case,
        "resource-via-app",
        Some("https://example.com/resource-via/app.git"),
    );
    let values = fs::read_to_string(app.join("values.yaml")).unwrap();
    let line = values.lines().nth(10).unwrap().to_string();
    let answer = edited(&app, "values.yaml", &line, &format!("{line} # edited"));
    // The reading node's file; `iac` lists only the workloads and CI nodes reached.
    let reader = in_repo(&answer, "example.com/resource-via/infra", "main.tf");
    assert_eq!(reader.expect("the reading node")["level"], "declared");
    assert_eq!(answer["iac"], Value::Array(Vec::new()));
}

#[test]
fn a_changed_document_affects_the_consumers_of_its_sides() {
    let (_, gateway, _) = common::documented_chain("affected_document");
    let answer = of_files(&gateway, &["openapi.yaml"]);
    let consumer = in_repo(&answer, "example.com/chain/web", "src/shipments.ts");
    assert!(consumer.is_some(), "{answer:#}");
    assert_eq!(answer["open"], Value::Array(Vec::new()));
}
