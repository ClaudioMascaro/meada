//! What each workspace member's dependencies resolve to, read from a Yarn Berry `yarn.lock`
//! (`resolved` in `meada export`, the `lockfiles` table, `context` on a package).

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{assert_golden, fixture_repo, without_processes};
use meada::engine::Engine;
use serde_json::Value;

fn berry(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "lockfile-berry",
        Some("https://example.com/scope/berry.git"),
    )
}

fn export(repo: &Path) -> Value {
    serde_json::from_str(&export_text(repo)).unwrap()
}

/// The export as `meada export` writes it, the form an export golden holds.
fn export_text(repo: &Path) -> String {
    Engine::open(repo).unwrap().export().unwrap()
}

/// The `resolved` rows as `(manifest, name, range, origin, version)`.
fn resolved(export: &Value) -> Vec<(String, String, Option<String>, String, String)> {
    let text = |v: &Value| v.as_str().unwrap().to_string();
    (export["resolved"].as_array().into_iter().flatten())
        .map(|r| {
            let range = r.get("range").map(text);
            (
                text(&r["manifest"]),
                text(&r["name"]),
                range,
                text(&r["origin"]),
                text(&r["version"]),
            )
        })
        .collect()
}

fn row(
    m: &str,
    n: &str,
    r: &str,
    o: &str,
    v: &str,
) -> (String, String, Option<String>, String, String) {
    (m.into(), n.into(), Some(r.into()), o.into(), v.into())
}

#[test]
fn the_index_records_each_direct_dependency_resolution() {
    let repo = berry("lockfile_resolved");
    let text = export_text(&repo);
    let export: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        resolved(&export),
        [
            row(
                "packages/a/package.json",
                "@scope/kit",
                "^2.0.0",
                "npm",
                "2.0.0"
            ),
            row(
                "packages/a/package.json",
                "lodash",
                "^4.17.0",
                "npm",
                "4.17.21"
            ),
            row(
                "packages/b/package.json",
                "@scope/a",
                "workspace:^",
                "workspace",
                "0.0.0-use.local"
            ),
        ]
    );
    assert_golden("lockfile-berry.export.json", &without_processes(&text));
}

#[test]
fn a_lockfile_edit_refreshes_resolved_rows() {
    let repo = berry("lockfile_refresh");
    let before = export(&repo);
    let lock = repo.join("yarn.lock");
    let text = fs::read_to_string(&lock).unwrap();
    let text = text
        .replace("version: 4.17.21", "version: 4.17.22")
        .replace("lodash@npm:4.17.21", "lodash@npm:4.17.22");
    fs::write(&lock, text).unwrap();
    let after = export(&repo);
    let lodash = |e: &Value| {
        (resolved(e).into_iter())
            .find(|r| r.1 == "lodash")
            .map(|r| r.4)
            .unwrap()
    };
    assert_eq!(lodash(&before), "4.17.21");
    assert_eq!(lodash(&after), "4.17.22");
    // No source file was read again.
    assert_eq!(before["files"], after["files"]);
}

#[test]
fn a_yarn_1_lockfile_records_why_and_no_rows() {
    let repo = berry("lockfile_yarn_1");
    fs::write(
        repo.join("yarn.lock"),
        "# yarn lockfile v1\n\n\nlodash@^4.17.0:\n  version \"4.17.21\"\n",
    )
    .unwrap();
    let export = export(&repo);
    assert!(
        export.get("resolved").is_none(),
        "{:?}",
        export.get("resolved")
    );
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    let state: String = db
        .query_row(
            "SELECT state FROM lockfiles WHERE path = 'yarn.lock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "format");
    let rows: i64 = db
        .query_row("SELECT count(*) FROM resolved", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn a_removed_lockfile_takes_its_rows_along() {
    let repo = berry("lockfile_removed");
    assert!(!resolved(&export(&repo)).is_empty());
    fs::remove_file(repo.join("yarn.lock")).unwrap();
    assert!(export(&repo).get("resolved").is_none());
}

#[test]
fn context_on_a_package_lists_resolved_versions() {
    let repo = berry("lockfile_context");
    let found = Engine::open(&repo)
        .unwrap()
        .context("package::npm::@scope/a", None)
        .unwrap();
    assert!(
        found
            .lines()
            .any(|l| l == "  lodash resolves ^4.17.0 to 4.17.21 (npm)"),
        "{found}"
    );
    assert!(!found.contains("@scope/a resolves"), "{found}");
}

#[test]
fn an_export_without_a_lockfile_has_no_resolved_key() {
    let repo = fixture_repo(
        "lockfile_none",
        "pkg-shared",
        Some("https://example.com/pkg/shared.git"),
    );
    assert!(export(&repo).get("resolved").is_none());
}

#[test]
fn a_sibling_folder_sharing_a_prefix_is_not_a_member() {
    let repo = berry("lockfile_prefix_sibling");
    let folder = repo.join("apps/a");
    let sibling = repo.join("apps/ab/x/package.json");
    fs::create_dir_all(folder.join("b/x")).unwrap();
    fs::create_dir_all(sibling.parent().unwrap()).unwrap();
    let manifest = r#"{"name":"@acme/a","dependencies":{"some-lib":"1.0.0"}}"#;
    fs::write(folder.join("b/x/package.json"), manifest).unwrap();
    fs::write(&sibling, manifest.replace("@acme/a", "@acme/b")).unwrap();
    fs::write(
        folder.join("package.json"),
        r#"{"name":"@acme/root","workspaces":["b/*"]}"#,
    )
    .unwrap();
    let lock = r#"__metadata:
  version: 8

"@acme/a@workspace:b/x":
  version: 0.0.0-use.local
  resolution: "@acme/a@workspace:b/x"
  dependencies:
    some-lib: "npm:1.0.0"

"some-lib@npm:1.0.0":
  version: 1.0.0
  resolution: "some-lib@npm:1.0.0"
"#;
    fs::write(folder.join("yarn.lock"), lock).unwrap();
    common::commit_all(&repo, "create nested workspace and sibling");

    fs::write(
        &sibling,
        fs::read_to_string(&sibling)
            .unwrap()
            .replace("1.0.0", "2.0.0"),
    )
    .unwrap();
    let answer = common::edited(&repo, "apps/a/yarn.lock", "1.0.0", "2.0.0");
    let lock = &answer["lockfiles"][0];
    assert_eq!(lock["path"], "apps/a/yarn.lock");
    assert!(
        lock["packages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["package"] == "apps/a/b/x")
    );
    assert!(
        answer["open"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"] == "apps/ab/x/package.json" && e["reason"] == "manifest"),
        "{}",
        answer["open"]
    );
}
