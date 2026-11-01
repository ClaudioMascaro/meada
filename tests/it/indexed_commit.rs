//! The export format and the indexed commit (ADR 0004 "Canonical export", #512): every export
//! names its `format`; a
//! local export names the indexed commit and whether the checkout differed from it, read around
//! the refresh; a server-mode export names the commit indexing recorded, never the mirror's
//! live `HEAD`.

use crate::common;

use std::fs;
use std::path::Path;

use common::{fixture_repo, git};
use meada::engine::Engine;
use serde_json::{Value, json};

use crate::manifest::{chain_manifest, run, run_export};

const ORIGIN: Option<&str> = Some("https://example.com/acme/conformance.git");

/// The local export of the repository at `repo`, parsed.
fn export(repo: &Path) -> Value {
    serde_json::from_str(&written(repo)).unwrap()
}

/// The local export of the repository at `repo`, as `meada export` prints it.
fn written(repo: &Path) -> String {
    Engine::open(repo).unwrap().export().unwrap()
}

/// `format`, `commit` and `dirty` of `export`, `Null` for a field left out.
fn fields(export: &Value) -> (Value, Value, Value) {
    let field = |name: &str| export.get(name).cloned().unwrap_or(Value::Null);
    (field("format"), field("commit"), field("dirty"))
}

fn head(repo: &Path) -> Value {
    json!(git(repo, &["rev-parse", "HEAD"]).trim())
}

#[test]
fn a_clean_checkout_names_its_commit_and_is_not_dirty() {
    let repo = fixture_repo("indexed_commit_clean", "conformance-ts-js", ORIGIN);
    let text = written(&repo);
    let exported: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(fields(&exported), (json!(1), head(&repo), json!(false)));
    // The three fields come first, around `repository`, before the lists.
    let start = format!(
        "{{\n  \"format\": 1,\n  \"repository\": \"example.com/acme/conformance\",\n  \
         \"commit\": {},\n  \"dirty\": false,\n  \"files\": [",
        head(&repo)
    );
    assert!(text.starts_with(&start), "{text}");
}

#[test]
fn an_edited_tracked_file_or_an_untracked_one_makes_the_checkout_dirty() {
    let repo = fixture_repo("indexed_commit_edited", "conformance-ts-js", ORIGIN);
    let commit = head(&repo);
    let file = repo.join("tsconfig.json");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{text}\n")).unwrap();
    assert_eq!(fields(&export(&repo)), (json!(1), commit, json!(true)));

    let repo = fixture_repo("indexed_commit_untracked", "conformance-ts-js", ORIGIN);
    let commit = head(&repo);
    fs::write(repo.join("notes.txt"), "draft\n").unwrap();
    assert_eq!(fields(&export(&repo)), (json!(1), commit, json!(true)));
}

/// Ignored files never count: not the index's own `.meada/`, not a path the checkout ignores.
#[test]
fn an_ignored_path_and_the_index_leave_the_checkout_clean() {
    let repo = fixture_repo("indexed_commit_ignored", "conformance-ts-js", ORIGIN);
    let commit = head(&repo);
    fs::write(repo.join(".git/info/exclude"), "build/\n").unwrap();
    fs::create_dir_all(repo.join("build")).unwrap();
    fs::write(repo.join("build/out.js"), "export const built = 1;\n").unwrap();
    assert_eq!(fields(&export(&repo)), (json!(1), commit, json!(false)));
    assert!(repo.join(".meada/index.db").exists());
}

/// Outside git and before the first commit there is no commit to name: `format` alone.
#[test]
fn a_directory_outside_git_or_with_no_commit_names_only_its_format() {
    let dir = std::env::temp_dir().join(format!("meada-indexed-commit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.ts"), "export function a() {}\n").unwrap();
    assert_eq!(fields(&export(&dir)), (json!(1), Value::Null, Value::Null));
    git(&dir, &["-c", "init.defaultBranch=main", "init", "-q"]);
    git(&dir, &["add", "-A"]);
    assert_eq!(fields(&export(&dir)), (json!(1), Value::Null, Value::Null));
    fs::remove_dir_all(&dir).unwrap();
}

/// Server mode: the commit `meada index --workspace` recorded for a clean mirror, kept after the
/// mirror's `HEAD` moves without reindexing; with none recorded, neither field.
#[test]
fn a_workspace_export_names_the_recorded_commit_not_the_mirror_head() {
    let dir = chain_manifest("indexed_commit_workspace");
    let web = dir.join("chain-web");
    let indexed = head(&web);
    assert!(run(&dir, "index").2);
    let exported = |dir: &Path| -> Value {
        let (stdout, stderr, code) = run_export(dir, "example.com/chain/web");
        assert_eq!(code, Some(0), "{stderr}");
        serde_json::from_str(&stdout).unwrap()
    };
    let recorded = (json!(1), indexed.clone(), json!(false));
    assert_eq!(fields(&exported(&dir)), recorded);

    fs::write(web.join("src/later.ts"), "export const later = 1;\n").unwrap();
    git(&web, &["add", "-A"]);
    git(&web, &["commit", "-q", "-m", "later"]);
    assert_ne!(head(&web), indexed);
    assert_eq!(fields(&exported(&dir)), recorded);

    let sidecar = dir.join("indexes/example.com/chain/web/index.commit.json");
    fs::remove_file(sidecar).unwrap();
    assert_eq!(
        fields(&exported(&dir)),
        (json!(1), Value::Null, Value::Null)
    );
}
