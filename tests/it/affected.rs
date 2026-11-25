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
        // The workspace root's manifest and lockfile affect every package (front 4).
        ("package.json", "all-packages", None),
        ("packages/a/README.md", "no-symbols", Some("packages/a")),
        ("packages/d/package.json", "manifest", Some("packages/d")),
        ("yarn.lock", "all-packages", None),
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
    // A lockfile beside a workspace root affects every package (front 4).
    assert_eq!(answer["open"][0]["reason"], "all-packages");
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
            "changed",
            "files",
            "iac",
            "lockfiles",
            "open",
            "packages",
            "reads",
            "sides"
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
    let reader = in_repo(&answer, "example.com/chain/worker", "src/jobs.ts").expect("the reader");
    // A declared link is certain.
    assert_eq!(
        (&reader["level"], &reader["tier"]),
        (&json!("declared"), &json!("certain"))
    );
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
            // A lockfile read meets a lockfile change by name (`moved`), not by path.
            .filter(|e| e.get("name").is_none())
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
        !at(a, "files", true).is_disjoint(&b_side) || !moved(a).is_disjoint(&lockfile_reads(b))
    };
    one_way(a, b) || one_way(b, a)
}

/// The lockfile reads of an answer: `(repo, lockfile, name)`.
fn lockfile_reads(x: &Value) -> BTreeSet<(String, String, String)> {
    (x["reads"].as_array().into_iter().flatten())
        .filter_map(|e| {
            let name = e.get("name")?.as_str()?.to_string();
            let (repo, path) = (e["repo"].as_str()?.into(), e["path"].as_str()?.into());
            Some((repo, path, name))
        })
        .collect()
}

/// What an answer's lockfiles move, `(repo, lockfile, name)`: the names of the entries that moved
/// and the dependencies that lead there.
fn moved(x: &Value) -> BTreeSet<(String, String, String)> {
    let mut out = BTreeSet::new();
    for l in x["lockfiles"].as_array().into_iter().flatten() {
        let (repo, path) = (l["repo"].as_str().unwrap(), l["path"].as_str().unwrap());
        let entries = (l["entries"].as_array().into_iter().flatten()).map(|e| {
            let d = e["descriptor"].as_str().unwrap();
            let at = d[1..].find('@').map_or(d.len(), |i| i + 1);
            d[..at].to_string()
        });
        let via = (l["packages"].as_array().into_iter().flatten())
            .flat_map(|p| p["via"].as_array().unwrap())
            .map(|n| n.as_str().unwrap().to_string());
        out.extend(
            entries
                .chain(via)
                .map(|n| (repo.to_string(), path.to_string(), n)),
        );
    }
    out
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

#[test]
fn a_manifest_others_depend_on_stays_open() {
    let repo = affected_repo("affected_depended_manifest");
    let answer = of_files(&repo, &["packages/a/package.json"]);
    let manifest = json!({"repo": "example.com/scope/mono", "path": "packages/a/package.json",
        "reason": "manifest", "package": "packages/a"});
    assert_eq!(answer["open"], json!([manifest]), "{answer:#}");
}

/// The worker's `settleOrders` reads `resource::postgres::orders` as a symbol, not a call.
#[test]
fn a_changed_reader_reads_the_writer_of_what_it_reads() {
    let (web, _, _) = common::chain("affected_reads_writer");
    let worker = common::worker(&web);
    let answer = of_files(&worker, &["src/jobs.ts"]);
    let reads = answer["reads"].as_array().unwrap();
    let writer =
        |r: &&Value| r["repo"] == "example.com/chain/gateway" && r["path"] == "src/orders.ts";
    assert!(reads.iter().any(|r| writer(&r)), "{answer:#}");
}

/// The web's `shipment` calls a route only the gateway's document lists.
#[test]
fn a_call_to_a_documented_route_reads_its_document() {
    let (web, _, _) = common::documented_chain("affected_reads_document");
    let answer = of_files(&web, &["src/shipments.ts"]);
    let reads = answer["reads"].as_array().unwrap();
    let document =
        |r: &&Value| r["repo"] == "example.com/chain/gateway" && r["path"] == "openapi.yaml";
    assert!(reads.iter().any(|r| document(&r)), "{answer:#}");
}

/// `packages/app` names `@scope/d` in its manifest and imports nothing; `@scope/d` does the
/// same with `@scope/a`.
#[test]
fn manifest_only_follows_dependents_past_one_step() {
    let repo = affected_repo("affected_manifest_two_steps");
    let answer = of_files(&repo, &["packages/a/src/f.ts"]);
    let manifest_only = packages(&answer, "manifest_only");
    for package in ["packages/app", "packages/d"] {
        assert!(
            manifest_only.contains(&package.to_string()),
            "{package}: {manifest_only:?}"
        );
    }
}

// Lockfiles (front 4): a Yarn Berry `yarn.lock` change roots the files that load what moved.

const WEB: &str = "example.com/scope/web";

fn lockfile_repo(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "affected-lockfile",
        Some("https://example.com/scope/web.git"),
    )
}

/// The lines of one `yarn.lock` entry as the fixture writes them: version, resolution, checksum.
fn entry(name: &str, version: &str, checksum: &str) -> String {
    format!(
        "  version: {version}\n  resolution: \"{name}@npm:{version}\"\n  checksum: 10c0/{checksum}\n"
    )
}

/// The answer of a commit that bumps `name` in `web/yarn.lock` from one entry to another.
fn bumped(repo: &Path, name: &str, from: (&str, &str), to: (&str, &str)) -> Value {
    edited(
        repo,
        "web/yarn.lock",
        &entry(name, from.0, from.1),
        &entry(name, to.0, to.1),
    )
}

const LODASH: (&str, &str) = ("4.17.20", "lodash41720");
const LODASH_NEXT: (&str, &str) = ("4.17.21", "lodash41721");

/// The paths of `files` a lockfile rooted, each with its lockfile.
fn locked(answer: &Value) -> Vec<(String, String)> {
    (answer["files"].as_array().into_iter().flatten())
        .filter_map(|f| {
            let lockfile = f.get("lockfile")?.as_str()?.to_string();
            Some((f["path"].as_str().unwrap().to_string(), lockfile))
        })
        .collect()
}

fn locked_paths(answer: &Value) -> Vec<String> {
    locked(answer).into_iter().map(|(p, _)| p).collect()
}

/// The `open` entries as `(path, reason, package)`.
fn opened(answer: &Value) -> Vec<(String, String, Option<String>)> {
    (answer["open"].as_array().into_iter().flatten())
        .map(|e| {
            let package = e.get("package").map(|p| p.as_str().unwrap().to_string());
            let reason = e["reason"].as_str().unwrap().to_string();
            (e["path"].as_str().unwrap().to_string(), reason, package)
        })
        .collect()
}

#[test]
fn a_direct_bump_roots_only_the_importing_files() {
    let repo = lockfile_repo("lockfile_direct");
    let answer = bumped(&repo, "lodash", LODASH, LODASH_NEXT);
    let in_a: Vec<String> = (locked_paths(&answer).into_iter())
        .filter(|p| p.starts_with("web/packages/a/"))
        .collect();
    assert_eq!(
        in_a,
        ["web/packages/a/src/fp.ts", "web/packages/a/src/uses.ts"]
    );
    for path in ["web/packages/a/src/fp.ts", "web/packages/a/src/uses.ts"] {
        let f = in_repo(&answer, WEB, path).unwrap();
        assert_eq!(
            (f["tier"].as_str(), f["level"].as_str()),
            (Some("certain"), Some("exact"))
        );
        assert_eq!(f["lockfile"], "web/yarn.lock");
        // The link that reached a file keeps its own field.
        assert!(f.get("reason").is_none(), "{f}");
    }
    assert!(in_repo(&answer, WEB, "web/packages/a/src/plain.ts").is_none());
    let lockfile = &answer["lockfiles"][0];
    assert_eq!(lockfile["path"], "web/yarn.lock");
    assert_eq!(
        lockfile["packages"],
        json!([{"package": "web/packages/a", "via": ["lodash"], "whole": false}])
    );
    assert_eq!(
        lockfile["entries"],
        json!([{"descriptor": "lodash@npm:^4.17.0", "before": "4.17.20", "after": "4.17.21"}])
    );
    // Attributed, the lockfile is touched and roots, so it is not open.
    assert!(opened(&answer).is_empty(), "{:?}", opened(&answer));
    assert!(in_repo(&answer, WEB, "web/yarn.lock").is_some());
    // The dependent `b` is reached through the code graph, not through the lockfile.
    assert!(
        !locked_paths(&answer)
            .iter()
            .any(|p| p.starts_with("web/packages/b/"))
    );
}

#[test]
fn a_subpath_import_loads_its_package() {
    let repo = lockfile_repo("lockfile_subpath");
    let answer = bumped(&repo, "lodash", LODASH, LODASH_NEXT);
    assert!(locked_paths(&answer).contains(&"web/packages/a/src/fp.ts".to_string()));
}

#[test]
fn a_transitive_bump_roots_the_importers_of_the_direct_dependency() {
    let repo = lockfile_repo("lockfile_transitive");
    let answer = bumped(
        &repo,
        "left-pad",
        ("1.3.0", "leftpad130"),
        ("1.3.1", "leftpad131"),
    );
    assert_eq!(locked_paths(&answer), ["web/packages/b/src/kit.ts"]);
    assert_eq!(
        answer["lockfiles"][0]["packages"],
        json!([{"package": "web/packages/b", "via": ["@scope/kit"], "whole": false}])
    );
}

#[test]
fn a_dependency_no_file_loads_roots_the_whole_package() {
    let repo = lockfile_repo("lockfile_whole");
    let answer = bumped(&repo, "typescript", ("5.4.5", "ts545"), ("5.4.6", "ts546"));
    assert_eq!(locked_paths(&answer), ["web/packages/c/src/index.ts"]);
    assert_eq!(
        answer["lockfiles"][0]["packages"],
        json!([{"package": "web/packages/c", "via": ["typescript"], "whole": true}])
    );
}

#[test]
fn a_bare_import_is_rooted_beside_a_named_one() {
    let repo = lockfile_repo("lockfile_bare");
    let manifest = repo.join("web/packages/b/package.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace("\"@scope/kit\"", "\"left-pad\": \"^1.3.0\", \"@scope/kit\""),
    )
    .unwrap();
    replace(
        &repo,
        "web/yarn.lock",
        "    \"@scope/kit\": \"npm:^2.0.0\"\n  languageName: unknown",
        "    \"@scope/kit\": \"npm:^2.0.0\"\n    left-pad: \"npm:^1.3.0\"\n  languageName: unknown",
    );
    fs::write(
        repo.join("web/packages/b/src/kit2.ts"),
        "import pad from 'left-pad';\n\nexport function padded() {\n  return pad('a', 2);\n}\n",
    )
    .unwrap();
    commit_all(&repo, "declare left-pad");
    let answer = bumped(
        &repo,
        "left-pad",
        ("1.3.0", "leftpad130"),
        ("1.3.1", "leftpad131"),
    );
    assert_eq!(
        locked_paths(&answer),
        [
            "web/packages/b/src/kit.ts",
            "web/packages/b/src/kit2.ts",
            "web/packages/b/src/side.ts"
        ]
    );
}

#[test]
fn a_nested_lockfile_roots_by_repository_path() {
    let repo = lockfile_repo("lockfile_nested");
    let answer = bumped(&repo, "lodash", LODASH, LODASH_NEXT);
    for key in ["changed", "files"] {
        for path in paths(&answer, key) {
            assert!(path.starts_with("web/"), "{key}: {path}");
        }
    }
    let packages = answer["lockfiles"][0]["packages"].as_array().unwrap();
    assert!(
        packages
            .iter()
            .all(|p| p["package"].as_str().unwrap().starts_with("web/"))
    );
}

#[test]
fn an_aliased_dependency_roots_files_importing_the_alias() {
    let repo = lockfile_repo("lockfile_alias");
    let manifest = repo.join("web/packages/a/package.json");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace(
            "\"lodash\": \"^4.17.0\"",
            "\"lodash\": \"^4.17.0\", \"util\": \"npm:lodash@^4\"",
        ),
    )
    .unwrap();
    replace(
        &repo,
        "web/yarn.lock",
        "    lodash: \"npm:^4.17.0\"\n  languageName: unknown",
        "    lodash: \"npm:^4.17.0\"\n    util: \"npm:lodash@^4\"\n  languageName: unknown",
    );
    replace(
        &repo,
        "web/yarn.lock",
        "\"lodash@npm:^4.17.0\":",
        "\"lodash@npm:^4.17.0, util@npm:lodash@^4\":",
    );
    fs::write(
        repo.join("web/packages/a/src/alias.ts"),
        "import u from 'util';\n\nexport function aliased() {\n  return u.identity(1);\n}\n",
    )
    .unwrap();
    commit_all(&repo, "alias lodash");
    let answer = bumped(&repo, "lodash", LODASH, LODASH_NEXT);
    assert!(locked_paths(&answer).contains(&"web/packages/a/src/alias.ts".to_string()));
    assert_eq!(
        answer["lockfiles"][0]["packages"][0]["via"],
        json!(["lodash", "util"])
    );
}

/// The one `lockfiles` entry's `unread`, and the `open` entry of `web/yarn.lock`.
fn unread(answer: &Value) -> (String, Option<(String, String, Option<String>)>) {
    let lockfiles = answer["lockfiles"].as_array().unwrap();
    assert_eq!(lockfiles.len(), 1, "{lockfiles:?}");
    assert!(lockfiles[0].get("packages").is_none());
    let open = opened(answer)
        .into_iter()
        .find(|(p, _, _)| p == "web/yarn.lock");
    (lockfiles[0]["unread"].as_str().unwrap().to_string(), open)
}

fn all_packages() -> Option<(String, String, Option<String>)> {
    Some(("web/yarn.lock".into(), "all-packages".into(), None))
}

#[test]
fn a_lockfile_added_in_the_change_is_all_packages() {
    let repo = lockfile_repo("lockfile_added");
    let lock = repo.join("web/yarn.lock");
    let text = fs::read(&lock).unwrap();
    fs::remove_file(&lock).unwrap();
    commit_all(&repo, "no lockfile");
    fs::write(&lock, text).unwrap();
    commit_all(&repo, "lockfile");
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = affected(&repo, range);
    assert_eq!(unread(&answer), ("no-base".to_string(), all_packages()));
    assert!(locked(&answer).is_empty());
}

#[test]
fn files_give_no_base() {
    let repo = lockfile_repo("lockfile_files");
    let answer = of_files(&repo, &["web/yarn.lock"]);
    assert_eq!(unread(&answer), ("no-base".to_string(), all_packages()));
}

#[test]
fn a_deleted_lockfile_is_all_packages() {
    let repo = lockfile_repo("lockfile_deleted");
    fs::remove_file(repo.join("web/yarn.lock")).unwrap();
    commit_all(&repo, "drop the lockfile");
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = affected(&repo, range);
    assert_eq!(unread(&answer), ("deleted".to_string(), all_packages()));
}

#[test]
fn a_yarn_1_lockfile_is_all_packages() {
    let repo = lockfile_repo("lockfile_yarn_1");
    fs::write(
        repo.join("web/yarn.lock"),
        "# yarn lockfile v1\n\n\nlodash@^4.17.0:\n  version \"4.17.21\"\n",
    )
    .unwrap();
    commit_all(&repo, "yarn 1");
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = affected(&repo, range);
    assert_eq!(unread(&answer), ("format".to_string(), all_packages()));
}

#[test]
fn root_files_are_all_packages_and_a_readme_stays_root() {
    let repo = lockfile_repo("lockfile_root_files");
    replace(&repo, "web/.yarnrc.yml", "node-modules", "pnp");
    replace(&repo, "web/backstage.json", "1.30.0", "1.31.0");
    replace(&repo, "web/README.md", "The web", "Our web");
    commit_all(&repo, "root files");
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = affected(&repo, range);
    let expected = [
        ("web/.yarnrc.yml", "all-packages"),
        ("web/README.md", "root"),
        ("web/backstage.json", "all-packages"),
    ]
    .map(|(p, r)| (p.to_string(), r.to_string(), None));
    assert_eq!(opened(&answer), expected);
    assert_eq!(answer["lockfiles"], json!([]));
}

#[test]
fn a_deleted_root_file_is_all_packages() {
    let repo = lockfile_repo("lockfile_root_deleted");
    fs::remove_file(repo.join("web/backstage.json")).unwrap();
    commit_all(&repo, "drop the version file");
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = affected(&repo, range);
    assert_eq!(
        opened(&answer),
        [(
            "web/backstage.json".to_string(),
            "all-packages".to_string(),
            None
        )]
    );
}

#[test]
fn a_single_package_lockfile_roots_its_importers() {
    let repo = fixture_repo(
        "lockfile_single",
        "affected-lockfile-single",
        Some("https://example.com/scope/solo.git"),
    );
    let answer = edited(
        &repo,
        "yarn.lock",
        &entry("lodash", LODASH.0, LODASH.1),
        &entry("lodash", LODASH_NEXT.0, LODASH_NEXT.1),
    );
    assert_eq!(locked_paths(&answer), ["src/uses.ts"]);
    assert_eq!(
        answer["lockfiles"][0]["packages"],
        json!([{"package": ".", "via": ["lodash"], "whole": false}])
    );
    assert!(opened(&answer).is_empty(), "{:?}", opened(&answer));
}

#[test]
fn a_root_only_tooling_bump_is_all_packages() {
    let repo = lockfile_repo("lockfile_root_tooling");
    let answer = bumped(
        &repo,
        "eslint",
        ("9.0.0", "eslint900"),
        ("9.0.1", "eslint901"),
    );
    assert_eq!(
        opened(&answer),
        [(
            "web/yarn.lock".to_string(),
            "all-packages".to_string(),
            None
        )]
    );
    // Still attributed.
    assert_eq!(
        answer["lockfiles"][0]["packages"],
        json!([{"package": "web", "via": ["eslint"], "whole": true}])
    );
}

#[test]
fn a_hoisted_import_of_a_reached_name_is_rooted() {
    let repo = lockfile_repo("lockfile_hoisted");
    let answer = bumped(&repo, "lodash", LODASH, LODASH_NEXT);
    assert!(
        locked(&answer).contains(&(
            "web/packages/d/src/hoisted.ts".into(),
            "web/yarn.lock".into()
        )),
        "{:?}",
        locked(&answer)
    );
    // `b` does not load `lodash`: nothing of it is rooted.
    assert!(
        !locked_paths(&answer)
            .iter()
            .any(|p| p.starts_with("web/packages/b/"))
    );
}

/// A commit that bumps `lodash` to `^4.17.21` in `a`'s manifest and the lockfile, with
/// `also` replacing more of the manifest.
fn range_bump(repo: &Path, also: Option<(&str, &str)>) -> Value {
    let manifest = "web/packages/a/package.json";
    replace(
        repo,
        manifest,
        "\"lodash\": \"^4.17.0\"",
        "\"lodash\": \"^4.17.21\"",
    );
    if let Some((old, new)) = also {
        replace(repo, manifest, old, new);
    }
    let lock = "web/yarn.lock";
    replace(
        repo,
        lock,
        "    lodash: \"npm:^4.17.0\"",
        "    lodash: \"npm:^4.17.21\"",
    );
    replace(
        repo,
        lock,
        "\"lodash@npm:^4.17.0\":",
        "\"lodash@npm:^4.17.21\":",
    );
    replace(
        repo,
        lock,
        &entry("lodash", LODASH.0, LODASH.1),
        &entry("lodash", LODASH_NEXT.0, LODASH_NEXT.1),
    );
    commit_all(repo, "bump lodash");
    affected(
        repo,
        Changes::Range {
            base: "HEAD~1",
            head: "HEAD",
        },
    )
}

#[test]
fn a_manifest_whose_only_change_is_the_bumped_range_is_covered() {
    let repo = lockfile_repo("lockfile_manifest_covered");
    let answer = range_bump(&repo, None);
    assert_eq!(
        answer["lockfiles"][0]["manifests"],
        json!(["web/packages/a/package.json"])
    );
    assert!(opened(&answer).is_empty(), "{:?}", opened(&answer));
    assert!(in_repo(&answer, WEB, "web/packages/a/src/plain.ts").is_none());
    assert!(locked_paths(&answer).contains(&"web/packages/a/src/uses.ts".to_string()));
}

#[test]
fn a_manifest_that_also_changes_main_stays_manifest() {
    let repo = lockfile_repo("lockfile_manifest_main");
    let answer = range_bump(&repo, Some(("src/uses.ts", "src/fp.ts")));
    assert_eq!(answer["lockfiles"][0]["manifests"], json!([]));
    assert_eq!(
        opened(&answer),
        [(
            "web/packages/a/package.json".to_string(),
            "manifest".to_string(),
            Some("web/packages/a".to_string())
        )]
    );
}

#[test]
fn lockfile_roots_seed_the_manifest_tier() {
    let repo = lockfile_repo("lockfile_manifest_tier");
    let answer = bumped(&repo, "lodash", LODASH, LODASH_NEXT);
    assert!(
        packages(&answer, "manifest_only").contains(&"web/packages/e".to_string()),
        "{}",
        answer["packages"]
    );
}

/// The answers of two changes on one base of the lockfile fixture.
fn lockfile_pair(case: &str, a: impl FnOnce(&Path), b: impl FnOnce(&Path)) -> (Value, Value) {
    let repo = lockfile_repo(case);
    (side(&repo, "a", a), side(&repo, "b", b))
}

fn bump_left_pad(repo: &Path) {
    replace(
        repo,
        "web/yarn.lock",
        &entry("left-pad", "1.3.0", "leftpad130"),
        &entry("left-pad", "1.3.1", "leftpad131"),
    );
}

#[test]
fn a_first_import_of_x_conflicts_with_a_bump_of_x() {
    let (a, b) = lockfile_pair(
        "lockfile_pair_first_import",
        |repo| {
            replace(
                repo,
                "web/packages/a/src/plain.ts",
                "export function plain",
                "import { pad } from 'left-pad';\n\nexport function plain",
            )
        },
        bump_left_pad,
    );
    assert!(
        lockfile_reads(&a).contains(&(WEB.into(), "web/yarn.lock".into(), "left-pad".into())),
        "{}",
        a["reads"]
    );
    let read = (a["reads"].as_array().unwrap().iter())
        .find(|r| r.get("name").is_some())
        .unwrap();
    assert_eq!(
        (read["tier"].as_str(), read["level"].as_str()),
        (Some("certain"), Some("exact"))
    );
    assert!(!files_meet(&a, &b));
    assert!(conflicts(&a, &b));
}

#[test]
fn an_import_of_another_name_does_not_conflict() {
    let (a, b) = lockfile_pair(
        "lockfile_pair_other_name",
        |repo| {
            replace(
                repo,
                "web/packages/a/src/plain.ts",
                "export function plain",
                "import 'react';\n\nexport function plain",
            )
        },
        bump_left_pad,
    );
    assert!(lockfile_reads(&a).contains(&(WEB.into(), "web/yarn.lock".into(), "react".into())));
    assert!(!conflicts(&a, &b));
}

#[test]
fn an_unchanged_line_reads_no_lockfile_entry() {
    let repo = lockfile_repo("lockfile_unchanged_import");
    let answer = edited(&repo, "web/packages/a/src/uses.ts", "x * 2", "x * 3");
    assert!(lockfile_reads(&answer).is_empty(), "{}", answer["reads"]);
    // A workspace package is read through the code graph, never through the lockfile.
    let answer = edited(
        &repo,
        "web/packages/a/src/plain.ts",
        "export function plain",
        "import { useKit } from '@scope/b';\n\nexport function plain",
    );
    assert!(lockfile_reads(&answer).is_empty(), "{}", answer["reads"]);
}

#[test]
fn a_dependency_newly_added_to_a_member_reaches_it() {
    let repo = lockfile_repo("lockfile_added_dependency");
    replace(
        &repo,
        "web/packages/c/package.json",
        "{\"typescript\": \"^5.4.0\"}",
        "{\"lodash\": \"^4.17.0\", \"typescript\": \"^5.4.0\"}",
    );
    let answer = edited(
        &repo,
        "web/yarn.lock",
        "  dependencies:\n    typescript: \"npm:^5.4.0\"\n",
        "  dependencies:\n    lodash: \"npm:^4.17.0\"\n    typescript: \"npm:^5.4.0\"\n",
    );
    let lockfile = &answer["lockfiles"][0];
    assert_eq!(
        lockfile["packages"],
        json!([{"package": "web/packages/c", "via": ["lodash"], "whole": true}])
    );
    assert_eq!(
        lockfile["manifests"],
        json!(["web/packages/c/package.json"])
    );
    assert_eq!(locked_paths(&answer), ["web/packages/c/src/index.ts"]);
    assert!(opened(&answer).is_empty(), "{:?}", opened(&answer));
}

#[test]
fn a_patch_rehash_roots_the_importers() {
    let repo = lockfile_repo("lockfile_patch");
    let patched = "patch:resolve@npm%3A^1.22.0#optional!builtin<compat/resolve>";
    replace(
        &repo,
        "web/packages/a/package.json",
        "\"lodash\": \"^4.17.0\"",
        "\"lodash\": \"^4.17.0\", \"resolve\": \"^1.22.0\"",
    );
    replace(
        &repo,
        "web/yarn.lock",
        "    lodash: \"npm:^4.17.0\"\n  languageName: unknown",
        &format!(
            "    lodash: \"npm:^4.17.0\"\n    resolve: \"{patched}\"\n  languageName: unknown"
        ),
    );
    let lock = repo.join("web/yarn.lock");
    let mut text = fs::read_to_string(&lock).unwrap();
    text.push_str(&format!(
        "\n\"resolve@npm:^1.22.0\":\n{}  languageName: node\n  linkType: hard\n\n\
         \"resolve@{patched}\":\n  version: 1.22.8\n  resolution: \
         \"resolve@patch:resolve@npm%3A1.22.8#optional!builtin<compat/resolve>::version=1.22.8&hash=c3c19d\"\n  \
         checksum: 10c0/resolvepatch1\n  languageName: node\n  linkType: hard\n",
        entry("resolve", "1.22.8", "resolve1228")
    ));
    fs::write(&lock, text).unwrap();
    fs::write(
        repo.join("web/packages/a/src/res.ts"),
        "import resolve from 'resolve';\n\nexport function found(x: string) {\n  return resolve.sync(x);\n}\n",
    )
    .unwrap();
    commit_all(&repo, "patched resolve");
    replace(&repo, "web/yarn.lock", "hash=c3c19d", "hash=c3c19e");
    let answer = edited(&repo, "web/yarn.lock", "resolvepatch1", "resolvepatch2");
    assert_eq!(locked_paths(&answer), ["web/packages/a/src/res.ts"]);
    assert_eq!(
        answer["lockfiles"][0]["packages"],
        json!([{"package": "web/packages/a", "via": ["resolve"], "whole": false}])
    );
}

#[test]
fn an_oversized_lockfile_is_too_large() {
    let repo = lockfile_repo("lockfile_too_large");
    let mut text = String::from("__metadata:\n  version: 8\n");
    while text.len() <= 16 << 20 {
        text.push_str("# padding padding padding padding padding padding padding padding\n");
    }
    fs::write(repo.join("web/yarn.lock"), text).unwrap();
    commit_all(&repo, "oversized");
    let range = Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = affected(&repo, range);
    assert_eq!(unread(&answer), ("too-large".to_string(), all_packages()));
}
