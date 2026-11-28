pub mod stub;

pub const SCHEMA_VERSION: u32 = 10;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use meada::engine::{Changes, Engine};
use serde_json::Value;

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

pub fn chain(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    fixture_repo(
        case,
        "chain-worker",
        Some("https://example.com/chain/worker.git"),
    );
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

pub fn xlang(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(
            case,
            "xlang-storefront",
            Some("https://example.com/xlang/storefront.git"),
        ),
        fixture_repo(
            case,
            "xlang-inventory",
            Some("git@example.com:xlang/inventory.git"),
        ),
    )
}

pub fn xlang_rust(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(
            case,
            "xlang-rust-web",
            Some("https://example.com/xlang-rust/web.git"),
        ),
        fixture_repo(
            case,
            "xlang-rust-api",
            Some("git@example.com:xlang-rust/api.git"),
        ),
    )
}

pub fn xlang_go(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(
            case,
            "xlang-go-web",
            Some("https://example.com/xlang-go/web.git"),
        ),
        fixture_repo(
            case,
            "xlang-go-api",
            Some("git@example.com:xlang-go/api.git"),
        ),
    )
}

pub fn xlang_java(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(
            case,
            "xlang-java-web",
            Some("https://example.com/xlang-java/web.git"),
        ),
        fixture_repo(
            case,
            "xlang-java-api",
            Some("git@example.com:xlang-java/api.git"),
        ),
    )
}

pub fn xlang_kotlin(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(
            case,
            "xlang-kotlin-web",
            Some("https://example.com/xlang-kotlin/web.git"),
        ),
        fixture_repo(
            case,
            "xlang-kotlin-api",
            Some("git@example.com:xlang-kotlin/api.git"),
        ),
    )
}

pub fn xlang_csharp(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(
            case,
            "xlang-csharp-web",
            Some("https://example.com/xlang-csharp/web.git"),
        ),
        fixture_repo(
            case,
            "xlang-csharp-api",
            Some("git@example.com:xlang-csharp/api.git"),
        ),
    )
}

pub fn xlang_swift(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(
            case,
            "xlang-swift-web",
            Some("https://example.com/xlang-swift/web.git"),
        ),
        fixture_repo(
            case,
            "xlang-swift-api",
            Some("git@example.com:xlang-swift/api.git"),
        ),
    )
}

pub fn xlang_java_mirror(case: &str, routes: &[&str]) -> (PathBuf, PathBuf) {
    let (web, api) = xlang_java(case);
    let mirror = web.parent().unwrap().join("xlang-java-mirror");
    let _ = fs::remove_dir_all(&mirror);
    fs::create_dir_all(&mirror).unwrap();
    fs::write(
        mirror.join("meada.toml"),
        "[serve]\nhosts = [\"web.example.com\"]\n",
    )
    .unwrap();
    for route in routes {
        let to = mirror.join(route);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(web.join(route), to).unwrap();
    }
    git(&mirror, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let origin = "https://example.com/xlang-java/mirror.git";
    git(&mirror, &["remote", "add", "origin", origin]);
    git(&mirror, &["add", "-A"]);
    git(&mirror, &["commit", "-q", "-m", "fixture"]);
    let toml = api.join("meada.toml");
    let peer = "\n[[peer]]\nrepo = \"example.com/xlang-java/mirror\"\n";
    fs::write(&toml, fs::read_to_string(&toml).unwrap() + peer).unwrap();
    (web, api)
}

pub fn loose_chain(case: &str) -> PathBuf {
    let (_, gateway, catalog) = chain(case);
    let toml = catalog.join("meada.toml");
    let loose = "\n[[resource]]\nkind = \"s3-bucket\"\nid = \"invoices\"\nrole = \"write\"\n\n\
                 [[resource]]\nkind = \"postgres\"\nid = \"orders\"\nrole = \"write\"\n\
                 via = [\"lib/gone.ts::sync\"]\n";
    fs::write(&toml, fs::read_to_string(&toml).unwrap() + loose).unwrap();
    gateway
}

pub fn documented_chain(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (web, gateway, catalog) = chain(case);
    let overlay = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/chain-documents");
    copy_dir(&overlay.join("gateway"), &gateway);
    copy_dir(&overlay.join("web"), &web);
    let toml = gateway.join("meada.toml");
    let hosts = "hosts = [\"gateway.example.com\"]\n";
    let listed = "documents = [\"generated/openapi.yaml\", \"docs/api-contract.yaml\", \
                  \"docs/missing.yaml\"]\n";
    let src = fs::read_to_string(&toml).unwrap();
    fs::write(&toml, src.replace(hosts, &format!("{hosts}{listed}"))).unwrap();
    fs::create_dir_all(gateway.join("generated")).unwrap();
    fs::copy(
        overlay.join("generated-openapi.yaml"),
        gateway.join("generated/openapi.yaml"),
    )
    .unwrap();
    for repo in [&web, &gateway] {
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", "documents"]);
    }
    (web, gateway, catalog)
}

pub fn drift_chain(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (web, gateway, catalog) = documented_chain(case);
    let overlay = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/chain-drift");
    for (repo, dir) in [(&web, "web"), (&gateway, "gateway"), (&catalog, "catalog")] {
        copy_dir(&overlay.join(dir), repo);
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", "drift"]);
    }
    (web, gateway, catalog)
}

pub fn worker(sibling: &Path) -> PathBuf {
    sibling.with_file_name("chain-worker")
}

pub fn decisions(repo: &Path, name: &str) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/decisions")
        .join(format!("{name}.toml"));
    fs::copy(source, repo.join("meada.decisions.toml")).unwrap();
}

pub const HEALTH: &str = "example.com/chain/web::src/status.ts::health::fetch#0";
pub const TRACK: &str = "example.com/chain/web::src/status.ts::track::fetch#0";

pub fn meada(
    repo: &Path,
    args: &[&str],
    stdin: &str,
    setup: impl FnOnce(&mut Command),
) -> (String, String, bool) {
    use std::io::Write;
    use std::process::Stdio;
    let mut command = Command::new(env!("CARGO_BIN_EXE_meada"));
    command
        .current_dir(repo)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    setup(&mut command);
    let mut child = command.spawn().expect("meada runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text =
        |bytes: &[u8]| String::from_utf8_lossy(bytes).replace(repo.to_str().unwrap(), "<repo>");
    (text(&out.stdout), text(&out.stderr), out.status.success())
}

pub fn written(repo: &Path) -> Option<String> {
    fs::read_to_string(repo.join("meada.decisions.toml")).ok()
}

pub fn crypto_provider() {
    #[cfg(feature = "snapshot")]
    let _ = rustls::crypto::ring::default_provider().install_default();
}

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

pub fn assert_golden_but_indexed_commit(name: &str, actual: &str) {
    if std::env::var_os("MEADA_BLESS").is_some_and(|v| v == "1") {
        return;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    let expected = fs::read_to_string(&path).unwrap();
    let without = |export: &str| -> String {
        (export.split_inclusive('\n'))
            .filter(|l| !l.starts_with("  \"commit\": ") && !l.starts_with("  \"dirty\": "))
            .collect()
    };
    assert!(
        without(actual) == without(&expected),
        "{name} differs from its golden file but for commit and dirty\n--- actual ---\n{actual}"
    );
}

pub fn without_processes(export: &str) -> String {
    let Some(start) = export.find(",\n  \"processes\": [") else {
        assert!(
            !export.contains("\"processes\""),
            "processes left in:\n{export}"
        );
        return export.to_string();
    };
    let end = start + export[start..].find("\n  ]").unwrap() + "\n  ]".len();
    format!("{}{}", &export[..start], &export[end..])
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

pub fn affected(repo: &Path, changes: Changes) -> Value {
    let answer = Engine::open(repo).unwrap().affected(changes).unwrap();
    serde_json::to_value(answer).unwrap()
}

pub fn of_files(repo: &Path, paths: &[&str]) -> Value {
    let files: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
    affected(repo, Changes::Files(&files))
}

pub fn in_repo<'a>(answer: &'a Value, repo: &str, path: &str) -> Option<&'a Value> {
    (answer["files"].as_array().into_iter().flatten())
        .find(|f| f["repo"] == repo && f["path"] == path)
}

pub fn edited(repo: &Path, path: &str, old: &str, new: &str) -> Value {
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

pub fn commit_all(repo: &Path, message: &str) {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
}
