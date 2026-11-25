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
