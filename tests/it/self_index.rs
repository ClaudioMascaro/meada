//! Invariants over Meada's own index: a copy of the repository's tracked files, indexed like any
//! other repository (indexing writes `.meada/`, so never in place).

use crate::common::git;

use std::fs;
use std::path::Path;

use meada::engine::Engine;
use serde_json::Value;

const ID: &str = "github.com/claudiomascaro/meada";

#[test]
fn meada_own_index_keeps_its_invariants() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = Path::new(env!("CARGO_TARGET_TMPDIR")).join("self_index/meada");
    let _ = fs::remove_dir_all(&repo);
    // The working tree's tracked files, so an uncommitted change is tested too.
    for path in git(source, &["ls-files", "-z"]).split_terminator('\0') {
        let from = source.join(path);
        // Symbolic links (`.claude/skills`) and files deleted but not staged are left out.
        if !fs::symlink_metadata(&from).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let to = repo.join(path);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(&from, &to).unwrap();
    }
    git(&repo, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let origin = "https://github.com/ClaudioMascaro/meada.git";
    git(&repo, &["remote", "add", "origin", origin]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "copy"]);

    let export = Engine::open(&repo).unwrap().export().unwrap();

    let export: Value = serde_json::from_str(&export).unwrap();
    let calls = export["calls"].as_array().unwrap();
    let fixtures = format!("{ID}::tests/fixtures/");
    let leaks: Vec<&Value> = calls
        .iter()
        .filter(|c| {
            c["caller"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{ID}::src/"))
        })
        .filter(|c| {
            let candidates = c["candidates"].as_array().into_iter().flatten();
            c["target"]
                .as_str()
                .into_iter()
                .chain(candidates.filter_map(Value::as_str))
                .any(|t| t.starts_with(&fixtures))
        })
        .collect();
    assert!(
        leaks.is_empty(),
        "calls from src/ into test data: {leaks:#?}"
    );
    let call = format!("{ID}::src/resolve.rs::Resolver.call");
    assert!(
        calls
            .iter()
            .any(|c| c["target"] == call.as_str() && c["level"] == "exact"),
        "no exact caller of {call}"
    );
}
