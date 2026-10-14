mod common;

use std::fs;

use common::{assert_golden, fixture_repo, git};
use meada::engine::Engine;

#[test]
fn export_matches_golden() {
    let repo = fixture_repo(
        "export_matches_golden",
        "ts-basic",
        Some("git@Example.com:Acme/Web.git"),
    );
    // An untracked file is indexed; a gitignored one is not.
    fs::write(
        repo.join("src/draft.ts"),
        "function draft() {\n  draft();\n}\n",
    )
    .unwrap();
    fs::create_dir(repo.join("generated")).unwrap();
    fs::write(repo.join("generated/out.ts"), "function out() {}\n").unwrap();

    let export = Engine::open(&repo).unwrap().export().unwrap();

    assert_golden("ts-basic.export.json", &export);
}

#[test]
fn exports_of_the_same_files_are_byte_identical() {
    let origin = Some("https://example.com/acme/web.git");
    let a = fixture_repo("byte_identical_a", "ts-basic", origin);
    let b = fixture_repo("byte_identical_b", "ts-basic", origin);

    let first = Engine::open(&a).unwrap().export().unwrap();
    let again = Engine::open(&a).unwrap().export().unwrap();
    let elsewhere = Engine::open(&b).unwrap().export().unwrap();

    assert_eq!(first, again);
    assert_eq!(first, elsewhere);
}

#[test]
fn index_writes_only_inside_meada() {
    let repo = fixture_repo("writes_only_inside_meada", "ts-basic", None);

    Engine::open(&repo).unwrap();

    assert_eq!(
        fs::read_to_string(repo.join(".meada/.gitignore")).unwrap(),
        "*\n"
    );
    assert!(repo.join(".meada/index.db").is_file());
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}

#[test]
fn repository_without_remote_is_local() {
    let repo = fixture_repo("without_remote", "ts-basic", None);

    let export = Engine::open(&repo).unwrap().export().unwrap();

    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    assert_eq!(export["repository"], "local/ts-basic");
    assert_eq!(
        export["symbols"][0]["id"],
        "local/ts-basic::src/math.ts::add"
    );
}

#[test]
fn index_from_another_schema_version_is_rebuilt() {
    let origin = Some("https://example.com/acme/web.git");
    let fresh = fixture_repo("schema_fresh", "ts-basic", origin);
    let stale = fixture_repo("schema_stale", "ts-basic", origin);
    fs::create_dir(stale.join(".meada")).unwrap();
    let old = rusqlite::Connection::open(stale.join(".meada/index.db")).unwrap();
    old.execute_batch("CREATE TABLE files (legacy TEXT); PRAGMA user_version = 999;")
        .unwrap();
    drop(old);

    let rebuilt = Engine::open(&stale).unwrap().export().unwrap();

    assert_eq!(rebuilt, Engine::open(&fresh).unwrap().export().unwrap());
}
