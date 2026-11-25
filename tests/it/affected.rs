//! `meada trace --affected` over the yarn workspace `trace-affected`: what a change may affect,
//! file by file, each file carrying the weakest link of its strongest chain, with what the walk
//! could not place listed in `open`.

use crate::common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::fixture_repo;
use meada::engine::{Changes, Engine};
use serde_json::{Value, json};

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

#[test]
fn an_affected_answer_is_the_same_over_two_runs() {
    let repo = affected_repo("affected_twice");
    let files = [
        "packages/a/src/f.ts".to_string(),
        "packages/e/src/runners.ts".to_string(),
    ];
    let run = || {
        let answer = Engine::open(&repo)
            .unwrap()
            .affected(Changes::Files(&files))
            .unwrap();
        serde_json::to_string(&answer).unwrap()
    };
    assert_eq!(run(), run());
}

/// The directories a `packages` group lists.
fn packages(answer: &Value, group: &str) -> Vec<String> {
    (answer["packages"][group].as_array().into_iter().flatten())
        .map(|p| p.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn manifest_only_lists_a_declared_dependent_with_no_import() {
    let repo = affected_repo("affected_manifest_only");
    let answer = of_files(&repo, &["packages/a/src/f.ts"]);
    let certain = packages(&answer, "certain");
    assert!(certain.contains(&"packages/b".to_string()), "{certain:?}");
    assert!(!certain.contains(&"packages/d".to_string()), "{certain:?}");
    let manifest_only = packages(&answer, "manifest_only");
    assert!(
        manifest_only.contains(&"packages/d".to_string()),
        "{manifest_only:?}"
    );
    // A file's `manifest` says its package is in the closure.
    assert_eq!(
        file(&answer, "packages/b/src/g.ts").unwrap()["manifest"],
        true
    );
}

#[test]
fn a_test_file_reached_through_its_top_level_describe_is_a_test() {
    let repo = affected_repo("affected_test_file");
    let answer = of_files(&repo, &["packages/a/src/f.ts"]);
    let test = file(&answer, "packages/c/src/g.test.ts").expect("g.test.ts");
    assert_eq!(test["test"], true);
    assert_eq!(file(&answer, "packages/b/src/g.ts").unwrap()["test"], false);
}

#[test]
fn a_name_match_only_package_is_inferred() {
    let repo = affected_repo("affected_inferred_package");
    let answer = of_files(&repo, &["packages/a/src/named.ts"]);
    assert_eq!(packages(&answer, "inferred"), ["packages/e"]);
    assert_eq!(packages(&answer, "certain"), ["packages/a"]);
    let loose = file(&answer, "packages/e/src/loose.ts").expect("loose.ts");
    assert_eq!(
        (&loose["tier"], &loose["level"]),
        (&json!("inferred"), &json!("inferred"))
    );
}

/// The chain's catalog reads `resource::postgres::orders` with no `via`: the whole repository.
#[test]
fn a_repository_level_side_is_listed_in_sides() {
    let (_, gateway, _) = common::chain("affected_repository_side");
    let answer = of_files(&gateway, &["src/orders.ts"]);
    let side =
        json!({"id": "example.com/chain/catalog", "place": "repository", "level": "declared"});
    assert!(
        answer["sides"].as_array().unwrap().contains(&side),
        "{answer:#}"
    );
    let catalog = (answer["files"].as_array().unwrap().iter())
        .filter(|f| f["repo"] == "example.com/chain/catalog")
        .count();
    assert_eq!(catalog, 0, "a repository-level side has no file");
}

/// A config use is reached only through provisioning, which this walk does not follow: no
/// fixture form reaches one, so only the document side is listed here.
#[test]
fn a_document_side_is_listed_in_sides() {
    let (_, gateway, _) = common::documented_chain("affected_document_side");
    let answer = of_files(&gateway, &["openapi.yaml"]);
    let sides = answer["sides"].as_array().unwrap();
    let shipments = sides
        .iter()
        .find(|s| s["id"].as_str().unwrap().ends_with("GET /api/shipments/{}"));
    let shipments = shipments.unwrap_or_else(|| panic!("{answer:#}"));
    assert_eq!(
        (&shipments["place"], &shipments["level"]),
        (&json!("document"), &json!("exact"))
    );
}

/// The queue's criterion at the certain tier: A and B conflict when what A affects meets what
/// B touches or reads, or the other way round.
fn conflicts(a: &Value, b: &Value) -> bool {
    let at = |x: &Value, key: &str, certain: bool| -> BTreeSet<(String, String)> {
        (x[key].as_array().into_iter().flatten())
            .filter(|e| !certain || e["tier"] == "certain")
            .map(|e| {
                (
                    e["repo"].as_str().unwrap().into(),
                    e["path"].as_str().unwrap().into(),
                )
            })
            .collect()
    };
    let one_way = |a: &Value, b: &Value| {
        let mut b_side = at(b, "changed", false);
        b_side.extend(at(b, "open", false));
        b_side.extend(at(b, "reads", true));
        !at(a, "files", true).is_disjoint(&b_side)
    };
    one_way(a, b) || one_way(b, a)
}

/// Whether the certain `files` of `a` and `b` meet: the plain intersection.
fn files_meet(a: &Value, b: &Value) -> bool {
    let certain = |x: &Value| -> BTreeSet<String> {
        (x["files"].as_array().into_iter().flatten())
            .filter(|f| f["tier"] == "certain")
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect()
    };
    !certain(a).is_disjoint(&certain(b))
}

/// The answers of two changes made on one base, each its own commit checked out in turn and
/// answered as a range.
fn pair(case: &str, a: impl FnOnce(&Path), b: impl FnOnce(&Path)) -> (Value, Value) {
    let repo = affected_repo(case);
    (side(&repo, "a", a), side(&repo, "b", b))
}

/// The answer of `edit` committed on a branch `branch` from `main`, as a range.
fn side(repo: &Path, branch: &str, edit: impl FnOnce(&Path)) -> Value {
    common::git(repo, &["checkout", "-q", "main"]);
    common::git(repo, &["checkout", "-q", "-b", branch]);
    edit(repo);
    commit_all(repo, branch);
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    affected(repo, range)
}

fn replace(repo: &Path, path: &str, old: &str, new: &str) {
    let file = repo.join(path);
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains(old), "{path}: {old}");
    fs::write(&file, text.replace(old, new)).unwrap();
}

#[test]
fn a_new_call_into_a_changed_function_conflicts() {
    let (a, b) = pair(
        "affected_pair_new_call",
        |r| replace(r, "packages/a/src/f.ts", "return 1;", "return 2;"),
        |r| {
            let h = "import { f } from '@scope/a';\n\nexport function h(): number {\n  return f();\n}\n";
            fs::write(r.join("packages/e/src/h.ts"), h).unwrap();
        },
    );
    let reads = paths(&b, "reads");
    assert!(reads.contains(&"packages/a/src/f.ts".to_string()), "{b:#}");
    assert!(conflicts(&a, &b));
    assert!(!files_meet(&a, &b), "plain intersection sees no conflict");
}

#[test]
fn an_import_line_rebinding_an_unchanged_call_conflicts() {
    let (a, b) = pair(
        "affected_pair_rebind",
        |r| replace(r, "packages/b/src/fallback.ts", "return 0;", "return 9;"),
        |r| {
            replace(
                r,
                "packages/b/src/g.ts",
                "from '@scope/a'",
                "from './fallback'",
            )
        },
    );
    let reads = paths(&b, "reads");
    assert!(
        reads.contains(&"packages/b/src/fallback.ts".to_string()),
        "{b:#}"
    );
    assert!(conflicts(&a, &b));
    assert!(!files_meet(&a, &b), "plain intersection sees no conflict");
}

#[test]
fn two_edits_of_one_yaml_conflict() {
    let ci = ".github/workflows/ci.yml";
    let (a, b) = pair(
        "affected_pair_yaml",
        |r| replace(r, ci, "run: yarn test", "run: yarn test --ci"),
        |r| replace(r, ci, "runs-on: ubuntu-latest", "runs-on: ubuntu-24.04"),
    );
    assert!(conflicts(&a, &b));
}

#[test]
fn a_deletion_and_a_new_call_to_it_conflict() {
    let (a, b) = pair(
        "affected_pair_deletion",
        |r| {
            let thrice = "import { twice } from './twice';\n\nexport function thrice(): number {\n  return twice() + 1;\n}\n";
            fs::write(r.join("packages/a/src/thrice.ts"), thrice).unwrap();
        },
        |r| fs::remove_file(r.join("packages/a/src/twice.ts")).unwrap(),
    );
    assert!(
        paths(&a, "reads").contains(&"packages/a/src/twice.ts".to_string()),
        "{a:#}"
    );
    assert!(conflicts(&a, &b));
    assert!(!files_meet(&a, &b), "plain intersection sees no conflict");
}

#[test]
fn the_affected_answer_of_the_fixture_is_golden() {
    let repo = affected_repo("affected_golden");
    let answer = of_files(&repo, &["packages/a/src/f.ts"]);
    common::assert_golden(
        "trace-affected.json",
        &serde_json::to_string_pretty(&answer).unwrap(),
    );
}

#[test]
fn a_batch_answers_each_block_as_one_line() {
    let repo = affected_repo("affected_batch");
    let blocks = [
        ("one", vec!["packages/a/src/f.ts"]),
        (
            "two",
            vec![
                "packages/a/src/named.ts",
                "packages/e/src/runners.ts",
                "gone.ts",
            ],
        ),
    ];
    let mut text = String::new();
    for (label, paths) in &blocks {
        text.push_str(&format!("@@{label}\n{}\n", paths.join("\n")));
    }
    let batch = repo.join("../blocks.txt");
    fs::write(&batch, text).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(&repo)
        .args(["trace", "--affected", "--batch"])
        .arg(&batch)
        .env("LANG", "en_US.UTF-8")
        .output()
        .expect("meada runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<Value> = (String::from_utf8(out.stdout).unwrap().lines())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), blocks.len());
    for (line, (label, paths)) in lines.into_iter().zip(&blocks) {
        let mut single = of_files(&repo, paths);
        single["label"] = json!(label);
        assert_eq!(line, single, "{label}");
    }
    // Paths before any label form a block labelled "", with CRLF and blank lines read.
    fs::write(&batch, "packages/a/src/f.ts\r\n\r\n").unwrap();
    let run = |batch: &Path| {
        Command::new(env!("CARGO_BIN_EXE_meada"))
            .current_dir(&repo)
            .args(["trace", "--affected", "--batch"])
            .arg(batch)
            .output()
            .expect("meada runs")
    };
    let unlabelled = run(&batch);
    let mut single = of_files(&repo, &["packages/a/src/f.ts"]);
    single["label"] = json!("");
    assert_eq!(
        serde_json::from_slice::<Value>(&unlabelled.stdout).unwrap(),
        single
    );
    assert!(!run(&repo.join("../no-such-batch.txt")).status.success());
}

#[test]
fn a_changed_line_reads_the_candidates_of_its_call() {
    let repo = affected_repo("affected_reads_candidates");
    let answer = edited(
        &repo,
        "packages/e/src/loose.ts",
        "y.run();",
        "y.run(); // again",
    );
    let reads: Vec<&Value> = answer["reads"].as_array().unwrap().iter().collect();
    let runners = reads
        .iter()
        .find(|r| r["path"] == "packages/e/src/runners.ts");
    let runners = runners.unwrap_or_else(|| panic!("{answer:#}"));
    assert_eq!(
        (&runners["tier"], &runners["candidates"]),
        (&json!("ambiguous"), &json!(2))
    );
    // `x.uniqueName()` sits on a line the change left alone.
    assert!(
        !reads.iter().any(|r| r["path"] == "packages/a/src/named.ts"),
        "{answer:#}"
    );
}
