use crate::common;

use std::fs;
use std::path::Path;

use common::{age, assert_golden, fixture_repo, git, without_processes};
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

    assert_golden("ts-basic.export.json", &without_processes(&export));
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
fn python_exports_of_the_same_files_are_byte_identical() {
    let origin = Some("https://example.com/acme/conformance.git");
    let a = fixture_repo("python_byte_identical_a", "conformance-python", origin);
    let b = fixture_repo("python_byte_identical_b", "conformance-python", origin);

    let first = Engine::open(&a).unwrap().export().unwrap();
    let again = Engine::open(&a).unwrap().export().unwrap();
    let elsewhere = Engine::open(&b).unwrap().export().unwrap();

    assert_eq!(first, again);
    assert_eq!(first, elsewhere);
}

#[test]
fn go_exports_of_the_same_files_are_byte_identical() {
    let origin = Some("https://example.com/acme/conformance.git");
    let a = fixture_repo("go_byte_identical_a", "conformance-go", origin);
    let b = fixture_repo("go_byte_identical_b", "conformance-go", origin);

    let first = Engine::open(&a).unwrap().export().unwrap();
    let again = Engine::open(&a).unwrap().export().unwrap();
    let elsewhere = Engine::open(&b).unwrap().export().unwrap();

    assert_eq!(first, again);
    assert_eq!(first, elsewhere);
}

#[test]
fn java_exports_of_the_same_files_are_byte_identical() {
    let origin = Some("https://example.com/acme/conformance.git");
    let a = fixture_repo("java_byte_identical_a", "conformance-java", origin);
    let b = fixture_repo("java_byte_identical_b", "conformance-java", origin);
    let first = Engine::open(&a).unwrap().export().unwrap();
    assert_eq!(first, Engine::open(&a).unwrap().export().unwrap());
    assert_eq!(first, Engine::open(&b).unwrap().export().unwrap());
}

#[test]
fn rust_exports_of_the_same_files_are_byte_identical() {
    let origin = Some("https://example.com/acme/conformance.git");
    let a = fixture_repo("rust_byte_identical_a", "conformance-rust", origin);
    let b = fixture_repo("rust_byte_identical_b", "conformance-rust", origin);

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

#[test]
fn index_whose_stored_facts_do_not_parse_is_rebuilt() {
    let origin = Some("https://example.com/acme/web.git");
    let fresh = fixture_repo("facts_fresh", "ts-basic", origin);
    let broken = fixture_repo("facts_broken", "ts-basic", origin);
    Engine::open(&broken).unwrap().export().unwrap();
    let db = rusqlite::Connection::open(broken.join(".meada/index.db")).unwrap();
    db.execute("UPDATE files SET facts = '{'", []).unwrap();
    drop(db);
    // A changed file makes the refresh read the facts it keeps for the others.
    for repo in [&fresh, &broken] {
        age(repo);
        let math = repo.join("src/math.ts");
        let src = fs::read_to_string(&math).unwrap();
        fs::write(&math, format!("{src}\nexport const ZERO = 0;\n")).unwrap();
    }

    let rebuilt = Engine::open(&broken).unwrap().export().unwrap();

    assert_eq!(rebuilt, Engine::open(&fresh).unwrap().export().unwrap());
}

#[test]
fn a_root_tsconfig_that_is_invalid_or_linked_declares_no_paths() {
    use std::os::unix::fs::symlink;

    let export = |case: &str, config: &dyn Fn(&Path)| {
        let repo = fixture_repo(case, "conformance-ts-js", None);
        config(&repo.join("tsconfig.json"));
        Engine::open(&repo).unwrap().export().unwrap()
    };
    let none = export("tsconfig_none", &|path| fs::write(path, "{}\n").unwrap());
    // The fixture's `@/*` alias resolves calls: a config that declares nothing shows.
    assert_ne!(export("tsconfig_valid", &|_| {}), none);

    let invalid = export("tsconfig_invalid", &|path| {
        fs::write(path, "{ not").unwrap()
    });
    // Like the walk, the read goes through no symbolic link.
    let linked = export("tsconfig_linked", &|path| {
        let outside = path.parent().unwrap().with_extension("tsconfig.json");
        fs::rename(path, &outside).unwrap();
        symlink(&outside, path).unwrap();
    });

    assert_eq!(invalid, none);
    assert_eq!(linked, none);
}

#[test]
fn an_oversized_document_is_read_up_to_the_limit_and_refreshes_as_cold() {
    // One byte past `document::MAX_BYTES`, the size limit of a provider document.
    let read = (16 << 20) + 1;
    let document = |repo: &Path| {
        let valid = fs::read_to_string(repo.join("openapi.json")).unwrap();
        let oversized = format!("{valid}{}", " ".repeat(read));
        fs::write(repo.join("openapi.json"), &oversized).unwrap();
        oversized
    };
    let cold = fixture_repo("oversized_document_cold", "documents-shapes", None);
    let warm = fixture_repo("oversized_document_warm", "documents-shapes", None);
    let before = Engine::open(&warm).unwrap().export().unwrap();
    document(&cold);
    let oversized = document(&warm);

    let refreshed = Engine::open(&warm).unwrap().export().unwrap();

    assert_ne!(refreshed, before);
    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
    // The document row hashes what was read.
    let db = rusqlite::Connection::open(warm.join(".meada/index.db")).unwrap();
    let hash: String = db
        .query_row(
            "SELECT hash FROM documents WHERE path = 'openapi.json'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let prefix = blake3::hash(&oversized.as_bytes()[..read]);
    assert_eq!(hash, prefix.to_hex().to_string());
}

#[test]
fn an_oversized_manifest_is_read_up_to_the_limit_and_refreshes_as_cold() {
    // One byte past `lang::MAX_BYTES`, the size limit of every manifest and config.
    let read = (1 << 20) + 1;
    let config = |repo: &Path| {
        let valid = fs::read_to_string(repo.join("tsconfig.json")).unwrap();
        let oversized = format!("{valid}{}", " ".repeat(read));
        fs::write(repo.join("tsconfig.json"), &oversized).unwrap();
        oversized
    };
    let cold = fixture_repo("oversized_cold", "conformance-ts-js", None);
    let warm = fixture_repo("oversized_warm", "conformance-ts-js", None);
    Engine::open(&warm).unwrap().export().unwrap();
    config(&cold);
    let oversized = config(&warm);

    let refreshed = Engine::open(&warm).unwrap().export().unwrap();

    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
    // The manifest row hashes what was read.
    let db = rusqlite::Connection::open(warm.join(".meada/index.db")).unwrap();
    let hash: String = db
        .query_row(
            "SELECT hash FROM manifests WHERE path = 'tsconfig.json'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let prefix = blake3::hash(&oversized.as_bytes()[..read]);
    assert_eq!(hash, prefix.to_hex().to_string());
}
