//! Test harness: fixture repositories on disk and golden files.
#![allow(dead_code)] // each test binary uses a different part of this module

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Copies `tests/fixtures/<fixture>` to `<CARGO_TARGET_TMPDIR>/<case>/<fixture>` and makes the
/// copy a repository of its own: `git init`, `origin` set to `origin` when given, and one
/// commit of every file. Fixtures live inside Meada's repository, so they are never used in
/// place. `case` must be unique per test, since tests run in parallel.
pub fn fixture_repo(case: &str, fixture: &str, origin: Option<&str>) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(case)
        .join(fixture);
    let _ = fs::remove_dir_all(&dir);
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    copy_dir(&source, &dir);
    git(&dir, &["-c", "init.defaultBranch=main", "init", "-q"]);
    if let Some(url) = origin {
        git(&dir, &["remote", "add", "origin", url]);
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "fixture"]);
    dir
}

/// The chain fixture (spec #3) as siblings in one case directory: (web, gateway, catalog).
pub fn chain(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    (
        fixture_repo(case, "chain-web", Some("https://example.com/chain/web.git")),
        fixture_repo(
            case,
            "chain-gateway",
            Some("git@example.com:chain/gateway.git"),
        ),
        fixture_repo(
            case,
            "chain-catalog",
            Some("https://example.com/chain/catalog.git"),
        ),
    )
}

/// Runs `git` in `dir`, isolated from the user's and the system's git configuration, with a
/// fixed author and date so commits are the same on every machine.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Meada")
        .env("GIT_AUTHOR_EMAIL", "fixtures@example.com")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_NAME", "Meada")
        .env("GIT_COMMITTER_EMAIL", "fixtures@example.com")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("git output is UTF-8")
}

/// Compares `actual` with `tests/golden/<name>`. With `MEADA_BLESS=1` it writes `actual` to
/// the golden file instead; that is the only way a golden file changes.
pub fn assert_golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("MEADA_BLESS").is_some_and(|v| v == "1") {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden file {}; run with MEADA_BLESS=1 to create it",
            path.display()
        )
    });
    assert!(
        actual == expected,
        "{name} differs from its golden file; review the diff and rerun with MEADA_BLESS=1 to \
         accept it\n--- actual ---\n{actual}"
    );
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Sets the modification time of every file under `dir` but `.git/` to an instant long past,
/// so no file counts as just written and a later edit always changes it.
pub fn age(dir: &Path) {
    let past = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.ends_with(".git") {
            continue;
        }
        if path.is_dir() {
            age(&path);
        } else {
            set_mtime(&path, past);
        }
    }
}

pub fn set_mtime(path: &Path, time: std::time::SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}
