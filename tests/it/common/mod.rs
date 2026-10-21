//! Test harness: fixture repositories on disk and golden files.

pub mod stub;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Copies `tests/fixtures/<fixture>` to `<CARGO_TARGET_TMPDIR>/<case>/<fixture>` and makes the
/// copy a repository of its own: `git init`, `origin` set to `origin` when given, and one
/// commit of every file. Fixtures live inside Meada's repository, so they are never used in
/// place. `case` must be unique per test of every module, since they all run in parallel in one
/// process.
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
/// The worker (spec #19), which reads a resource the gateway writes, is their sibling too, at
/// [`worker`].
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

/// The cross-language fixture (spec #24) as siblings in one case directory: the TypeScript
/// storefront and the Python inventory service, which call each other.
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

/// The cross-language fixture of the Rust pack (spec #30) as siblings in one case directory:
/// the TypeScript web app and the Rust api service (axum and reqwest), which call each other.
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

/// The cross-language fixture of the Go pack (spec #25) as siblings in one case directory: the
/// TypeScript web app and the Go api service (net/http and resty), which call each other. A
/// repository of another language that calls or serves the Go api is added the same way: one
/// more `fixture_repo` of a fixture named `xlang-go-<role>`, with the `meada.toml` keys of the
/// others.
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

/// The chain with two loose ends added to the catalog's `meada.toml`: a bucket only the
/// catalog declares, and a write of `orders` through a symbol it does not have. Returns the
/// gateway, whose workspace holds all four repositories.
pub fn loose_chain(case: &str) -> PathBuf {
    let (_, gateway, catalog) = chain(case);
    let toml = catalog.join("meada.toml");
    let loose = "\n[[resource]]\nkind = \"s3-bucket\"\nid = \"invoices\"\nrole = \"write\"\n\n\
                 [[resource]]\nkind = \"postgres\"\nid = \"orders\"\nrole = \"write\"\n\
                 via = [\"lib/gone.ts::sync\"]\n";
    fs::write(&toml, fs::read_to_string(&toml).unwrap() + loose).unwrap();
    gateway
}

/// The chain with the OpenAPI documents of spec #21 laid over it (`tests/fixtures/
/// chain-documents/`): the gateway gains `openapi.yaml` (3.1, one route its code also serves,
/// one only the document lists), a Swagger 2.0 file, an invalid YAML file and, under its
/// gitignored `generated/`, a document never read; the web app gains a call to the
/// documented-only route and a vendored `vendor/openapi.yaml`, ignored since it has no
/// `[serve]`. Each overlaid repository gets a second commit. Returns (web, gateway, catalog).
pub fn documented_chain(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (web, gateway, catalog) = chain(case);
    let overlay = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/chain-documents");
    copy_dir(&overlay.join("gateway"), &gateway);
    copy_dir(&overlay.join("web"), &web);
    // The gateway lists the gitignored document, one with a name discovery does not know, and
    // a path that does not exist.
    let toml = gateway.join("meada.toml");
    let hosts = "hosts = [\"gateway.example.com\"]\n";
    let listed = "documents = [\"generated/openapi.yaml\", \"docs/api-contract.yaml\", \
                  \"docs/missing.yaml\"]\n";
    let src = fs::read_to_string(&toml).unwrap();
    fs::write(&toml, src.replace(hosts, &format!("{hosts}{listed}"))).unwrap();
    // The overlay cannot hold the gitignored document: Meada's own git would ignore it there.
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

/// The documented chain with the shape drift cases of spec #23 laid over it
/// (`tests/fixtures/chain-drift/`): the web app gains consumers of the gateway's documented
/// `GET /api/orders` reading a field the document lacks, a present field, a field under its
/// open part, a field through a client wrapper that returns the parsed body, and a name the
/// function rebinds. For the provider shapes the object literals of a handler give, the gateway
/// gains Express routes returning literals (an imported constant spread, an error branch with
/// a literal status, a spread of an unknown value) and the catalog Next.js routes doing the
/// same through `NextResponse.json` and `Response.json`; the web app reaches both through
/// `GATEWAY_URL` and `CATALOG_URL`. For shapes through package imports (issue #175), the catalog
/// holds the package `@chain/contracts`, which its own routes, the gateway and the web app import.
/// Each overlaid repository gets a third commit. Returns
/// (web, gateway, catalog).
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

/// The chain's worker, next to any repository of the chain.
pub fn worker(sibling: &Path) -> PathBuf {
    sibling.with_file_name("chain-worker")
}

/// Copies `tests/fixtures/decisions/<name>.toml` to `<repo>/meada.decisions.toml` (spec #18).
/// The base chain fixture has no decisions file, so each test picks the variant it needs.
pub fn decisions(repo: &Path, name: &str) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/decisions")
        .join(format!("{name}.toml"));
    fs::copy(source, repo.join("meada.decisions.toml")).unwrap();
}

/// The chain's two open items, both in `chain-web`: the ambiguous health check and the
/// unresolved tracking call.
pub const HEALTH: &str = "example.com/chain/web::src/status.ts::health::fetch#0";
pub const TRACK: &str = "example.com/chain/web::src/status.ts::track::fetch#0";

/// Runs `meada <args>` in `repo`, with `setup` adjusting the command (language, environment)
/// and `stdin` as input. Returns stdout, stderr and success, with the repository's path
/// replaced by `<repo>`.
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

/// `<repo>/meada.decisions.toml`, when there is one.
pub fn written(repo: &Path) -> Option<String> {
    fs::read_to_string(repo.join("meada.decisions.toml")).ok()
}

/// The process's rustls provider for the HTTP clients the tests build (rmcp's, object_store's).
/// With the `snapshot` feature reqwest carries rustls with no provider of its own, since Meada
/// hands its store client ring itself (`Cargo.toml`); a client built elsewhere reads the
/// process default, so the tests install the same one. Without the feature, nothing.
pub fn crypto_provider() {
    #[cfg(feature = "snapshot")]
    let _ = rustls::crypto::ring::default_provider().install_default();
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

/// `export` without its `processes`, for an export golden whose test is about something else:
/// the steps repeat the calls and links, so each change to the walk or to what it crosses would
/// re-bless every export golden. The chain's own exports (spec #31), the conformance export and
/// the `*.processes.json` goldens keep them.
pub fn without_processes(export: &str) -> String {
    let Some(start) = export.find(",\n  \"processes\": [") else {
        // A change to the export's layout must not let the processes back into the goldens.
        assert!(
            !export.contains("\"processes\""),
            "processes left in:\n{export}"
        );
        return export.to_string();
    };
    // The array ends at the first line that is `  ]`: the arrays inside it are indented deeper.
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
