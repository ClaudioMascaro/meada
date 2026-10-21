//! Server mode's workspace manifest (spec #259, ADR 0011): `meada index --workspace`,
//! `meada status --workspace` and `meada export --workspace` over fixture mirrors listed in a
//! `meada.workspace.toml`, with every index under the manifest's `index_dir`, nothing written
//! into a mirror, and only `meada index` writing.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::freshness::edit_chain;
use common::without_processes;
use common::{age, assert_golden, chain, decisions, documented_chain, fixture_repo, git};
use meada::engine::Engine;

pub(crate) const CHAIN: [(&str, &str); 4] = [
    ("example.com/chain/web", "chain-web"),
    ("example.com/chain/gateway", "chain-gateway"),
    ("example.com/chain/catalog", "chain-catalog"),
    ("example.com/chain/worker", "chain-worker"),
];

/// The chain's four mirrors in `<case>/`, listed by `<case>/ops/meada.workspace.toml` with
/// paths relative to it and `index_dir = "../indexes"`. Returns the case directory.
pub(crate) fn chain_manifest(case: &str) -> PathBuf {
    let dir = mirrors(case);
    list(&dir, &CHAIN, false);
    dir
}

/// Writes `<dir>/ops/meada.workspace.toml` listing `repos` (identity, directory in `dir`) with
/// `index_dir = "../indexes"`, after removing what a previous run left. With `configs`, each
/// mirror's `meada.toml` moves into its entry: `[serve]`, `[[peer]]` (without `path`) and
/// `[[resource]]` become the entry's `[repository.serve]`, `[[repository.peer]]` and
/// `[[repository.resource]]`, and the mirror is left without one.
fn list(dir: &Path, repos: &[(&str, &str)], configs: bool) {
    for left in ["indexes", "ops"] {
        let _ = fs::remove_dir_all(dir.join(left));
    }
    let mut toml = String::from("index_dir = \"../indexes\"\n");
    for (repo, path) in repos {
        toml += &format!("\n[[repository]]\nrepo = \"{repo}\"\npath = \"../{path}\"\n");
        let file = dir.join(path).join("meada.toml");
        if !configs || !file.exists() {
            continue;
        }
        for line in fs::read_to_string(&file).unwrap().lines() {
            let line = match line {
                "[serve]" => "[repository.serve]",
                "[[peer]]" => "[[repository.peer]]",
                "[[resource]]" => "[[repository.resource]]",
                hint if hint.starts_with("path = ") => continue,
                line => line,
            };
            toml += &format!("{line}\n");
        }
        fs::remove_file(&file).unwrap();
    }
    write_manifest(dir, &toml);
}

/// The chain's four mirrors in `<case>/`, with nothing a previous run left next to them.
/// Returns the case directory.
fn mirrors(case: &str) -> PathBuf {
    let (web, _, _) = chain(case);
    let dir = web.parent().unwrap().to_path_buf();
    for left in ["indexes", "ops", "bare.git"] {
        let _ = fs::remove_dir_all(dir.join(left));
    }
    dir
}

/// Writes `<dir>/ops/meada.workspace.toml`.
fn write_manifest(dir: &Path, toml: &str) -> PathBuf {
    let ops = dir.join("ops");
    fs::create_dir_all(&ops).unwrap();
    let manifest = ops.join("meada.workspace.toml");
    fs::write(&manifest, toml).unwrap();
    manifest
}

/// `meada <command> --workspace ops/meada.workspace.toml`, run in `dir` in English: stdout and
/// stderr with `dir` replaced by `<repo>`, and whether it succeeded.
pub(crate) fn run(dir: &Path, command: &str) -> (String, String, bool) {
    let (stdout, stderr, code) = run_code(dir, command);
    (stdout, stderr, code == Some(0))
}

/// `run` with the exit code.
fn run_code(dir: &Path, command: &str) -> (String, String, Option<i32>) {
    run_args(dir, &[command, "--workspace", MANIFEST])
}

/// The manifest every test writes, relative to its case directory.
pub(crate) const MANIFEST: &str = "ops/meada.workspace.toml";

/// `meada export --workspace ops/meada.workspace.toml --repo <repo>`, as `run_code`.
fn export(dir: &Path, repo: &str) -> (String, String, Option<i32>) {
    run_args(dir, &["export", "--workspace", MANIFEST, "--repo", repo])
}

/// `meada <args>` in `dir`, as `run_code`.
fn run_args(dir: &Path, args: &[&str]) -> (String, String, Option<i32>) {
    let out = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(dir)
        .args(args)
        .env("LANG", "en_US.UTF-8")
        .output()
        .expect("meada runs");
    let text =
        |bytes: &[u8]| String::from_utf8_lossy(bytes).replace(dir.to_str().unwrap(), "<repo>");
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

/// Every file under `dir`, relative to it, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in fs::read_dir(&next).into_iter().flatten() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(dir).unwrap();
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out
}

/// After a command: the only files under `<dir>/indexes` are each listed repository's
/// `index.db` and SQLite's companions, and no mirror has a `.meada/`.
pub(crate) fn assert_nothing_else_written(dir: &Path, indexed: &[&str]) {
    let written = files(&dir.join("indexes"));
    for file in &written {
        let (repo, name) = file.rsplit_once('/').unwrap();
        assert!(indexed.contains(&repo), "{file}");
        assert!(
            [
                "index.db",
                "index.db-journal",
                "index.db-wal",
                "index.db-shm"
            ]
            .contains(&name),
            "{file}"
        );
    }
    for repo in indexed {
        assert!(written.contains(&format!("{repo}/index.db")), "{repo}");
    }
    for (_, path) in CHAIN {
        assert!(!dir.join(path).join(".meada").exists(), "{path}");
    }
}

#[test]
fn index_writes_every_index_under_index_dir_and_nothing_into_the_mirrors() {
    let dir = chain_manifest("manifest_index");

    let (stdout, stderr, ok) = run(&dir, "index");

    assert!(ok, "{stderr}");
    assert_eq!((stdout.as_str(), stderr.as_str()), ("", ""));
    let all: Vec<&str> = CHAIN.iter().map(|(repo, _)| *repo).collect();
    assert_nothing_else_written(&dir, &all);
    // The manifest's workspace answers as the local one: the chain's own export goldens.
    let manifest = dir.join("ops/meada.workspace.toml");
    for (repo, path) in CHAIN {
        let export = Engine::open_manifest(&manifest, repo)
            .unwrap()
            .export()
            .unwrap();
        assert_golden(&format!("{path}.export.json"), &export);
    }
    assert_nothing_else_written(&dir, &all);
}

#[test]
fn status_reports_every_listed_repository_and_the_peers_the_manifest_leaves_out() {
    let dir = mirrors("manifest_status");
    // The catalog, which the gateway's `meada.toml` names, is not listed; the worker is not
    // listed either, and nothing looks for it as a sibling clone.
    write_manifest(
        &dir,
        "index_dir = \"../indexes\"\n\n\
         [[repository]]\nrepo = \"example.com/chain/gateway\"\npath = \"../chain-gateway\"\n\n\
         [[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n",
    );

    let (unindexed, stderr, ok) = run(&dir, "status");
    // Not servable (no index yet): the readiness probe fails, the text is the same.
    assert!(!ok && stderr.is_empty(), "{stderr}");
    assert_eq!(
        unindexed,
        "example.com/chain/gateway (../chain-gateway) not indexed\n\
         example.com/chain/web (../chain-web) not indexed\n\
         missing peer: example.com/chain/catalog, named by example.com/chain/gateway\n"
    );
    assert!(run(&dir, "index").2);
    let (indexed, stderr, ok) = run(&dir, "status");
    assert!(ok, "{stderr}");
    assert_golden("chain.workspace-status.txt", &indexed);

    // An index of another schema is reported, and rebuilt by the next `meada index`.
    let web_index = dir.join("indexes/example.com/chain/web/index.db");
    fs::write(&web_index, "not an index\n").unwrap();
    let (other, _, _) = run(&dir, "status");
    assert!(
        other.contains(
            "\nexample.com/chain/web (../chain-web) indexed by another version; \
             rebuilt by meada index --workspace\n"
        ),
        "{other}"
    );
    assert!(run(&dir, "index").2);
    assert_eq!(run(&dir, "status").0, indexed);
    assert_nothing_else_written(
        &dir,
        &["example.com/chain/gateway", "example.com/chain/web"],
    );
}

#[test]
fn each_unservable_entry_is_refused_and_the_others_are_still_indexed() {
    let case = "manifest_refusals";
    let dir = mirrors(case);
    git(&dir, &["clone", "--bare", "-q", "chain-web", "bare.git"]);
    let nomote = fixture_repo("manifest_refusals_nomote", "chain-catalog", None);
    let entry = |repo: &str, path: &str| {
        format!("\n[[repository]]\nrepo = \"{repo}\"\npath = \"{path}\"\n")
    };
    let toml = [
        "index_dir = \"../indexes\"\n".to_string(),
        entry("example.com/chain/web", "../chain-web"),
        entry("example.com/chain/../web", "../chain-web"),
        entry("example.com//web", "../chain-web"),
        entry(
            "local/chain-catalog",
            "../../manifest_refusals_nomote/chain-catalog",
        ),
        entry(
            "example.com/chain/nomote",
            "../../manifest_refusals_nomote/chain-catalog",
        ),
        entry("example.com/chain/web", "../chain-gateway"),
        entry("example.com/chain/gone", "../gone"),
        entry("example.com/chain/bare", "../bare.git"),
        entry("example.com/chain/ops", "."),
        entry("example.com/chain/src", "../chain-web/src"),
        entry("example.com/chain/other", "../chain-gateway"),
        entry("example.com/chain/catalog", "../chain-catalog"),
    ]
    .concat();
    write_manifest(&dir, &toml);

    let (stdout, stderr, code) = run_code(&dir, "index");

    assert_eq!(code, Some(1));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "meada: example.com/chain/../web: not a repository identity: a segment is empty, `.` or `..`\n\
         meada: example.com//web: not a repository identity: a segment is empty, `.` or `..`\n\
         meada: local/chain-catalog: a local identity: server mode serves repositories by their remote\n\
         meada: example.com/chain/nomote: the mirror's remote is `local/chain-catalog`, not `example.com/chain/nomote`\n\
         meada: example.com/chain/web: listed twice\n\
         meada: example.com/chain/gone: `ops/../gone` does not exist\n\
         meada: example.com/chain/bare: `ops/../bare.git` is not the root of a checkout with a working tree\n\
         meada: example.com/chain/ops: `ops/.` is not the root of a checkout with a working tree\n\
         meada: example.com/chain/src: `ops/../chain-web/src` is not the root of a checkout with a working tree\n\
         meada: example.com/chain/other: the mirror's remote is `example.com/chain/gateway`, not `example.com/chain/other`\n"
    );
    let served = ["example.com/chain/web", "example.com/chain/catalog"];
    assert_nothing_else_written(&dir, &served);
    assert!(!nomote.join(".meada").exists());
    assert!(!dir.join("bare.git/.meada").exists());

    let (status, _, ok) = run(&dir, "status");
    // A listed repository that is not an identity cannot be served: exit 1.
    assert!(!ok);
    assert!(
        status.starts_with(
            "example.com/chain/web (../chain-web) indexed, 5 files\n\
             example.com/chain/../web (../chain-web) not a repository identity"
        ),
        "{status}"
    );
    assert!(
        status.contains("\nexample.com/chain/web (../chain-gateway) listed twice\n"),
        "{status}"
    );
    assert_nothing_else_written(&dir, &served);

    // An `index_dir` inside a mirror would put every index there.
    let toml = [
        "index_dir = \"../chain-web/./x/..\"\n".to_string(),
        entry("example.com/chain/web", "../chain-web"),
        // Overridden by the web mirror's own `meada.toml`, but the web is not served at all.
        "\n[repository.serve]\nprefix = \"/edge\"\n".to_string(),
        entry("example.com/chain/catalog", "../chain-catalog"),
    ]
    .concat();
    write_manifest(&dir, &toml);
    let (status, _, _) = run(&dir, "status");
    assert!(!status.contains("manifest block ignored"), "{status}");
    let (_, stderr, code) = run_code(&dir, "index");
    assert_eq!(code, Some(1));
    assert_eq!(
        stderr,
        "meada: example.com/chain/web: its index directory \
         `ops/../chain-web/./x/../example.com/chain/web` would be inside the mirror `ops/../chain-web`\n\
         meada: example.com/chain/catalog: its index directory \
         `ops/../chain-web/./x/../example.com/chain/catalog` would be inside the mirror `ops/../chain-web`\n"
    );
    assert!(!dir.join("chain-web/example.com").exists());
    assert!(!dir.join("chain-web/x").exists());
}

#[test]
fn manifest_errors_name_the_file_the_line_and_the_key() {
    let dir = mirrors("manifest_errors");
    for (toml, expected) in [
        (
            "index_dir = \"../indexes\"\nindex = \"x\"\n",
            "meada: ops/meada.workspace.toml:2: `index` is not a meada.workspace.toml key\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\nrepo = \"example.com/chain/web\"\n\
             path = \"../chain-web\"\nbranch = \"main\"\n",
            "meada: ops/meada.workspace.toml:6: `repository.branch` is not a meada.workspace.toml key\n",
        ),
        (
            "[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n",
            "meada: ops/meada.workspace.toml: `index_dir` is missing\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\npath = \"../chain-web\"\n",
            "meada: ops/meada.workspace.toml:3: `repository.repo` is missing\n",
        ),
        // The manifest locates every repository: a peer has no `path`.
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\nrepo = \"example.com/chain/web\"\n\
             path = \"../chain-web\"\n\n[[repository.peer]]\nrepo = \"example.com/chain/gateway\"\n\
             path = \"../chain-gateway\"\n",
            "meada: ops/meada.workspace.toml:9: `repository.peer.path` is not a meada.workspace.toml key\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\nrepo = \"example.com/chain/web\"\n\
             path = \"../chain-web\"\n\n[repository.serve]\nprefix = \"edge\"\n",
            "meada: ops/meada.workspace.toml:8: `repository.serve.prefix`: `edge` must start with `/`\n",
        ),
    ] {
        write_manifest(&dir, toml);
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {toml}");
            assert_eq!(stderr, expected, "{command}: {toml}");
        }
    }
    // Read only from the path given: no manifest there is an error, not an empty workspace.
    fs::remove_file(dir.join("ops/meada.workspace.toml")).unwrap();
    let (_, stderr, ok) = run(&dir, "index");
    assert!(!ok);
    assert!(
        stderr.starts_with("meada: ops/meada.workspace.toml: "),
        "{stderr}"
    );
    assert!(!dir.join("indexes").exists());
}

#[test]
fn a_meada_directory_inside_a_mirror_is_ignored() {
    let dir = chain_manifest("manifest_stray_meada");
    let stray = dir.join("chain-web/.meada");
    fs::create_dir_all(&stray).unwrap();
    fs::write(stray.join("index.db"), "not an index\n").unwrap();
    fs::write(
        stray.join("leak.ts"),
        "export function leak() {\n  return 1;\n}\n",
    )
    .unwrap();

    assert!(run(&dir, "index").2);

    let manifest = dir.join("ops/meada.workspace.toml");
    let export = Engine::open_manifest(&manifest, "example.com/chain/web")
        .unwrap()
        .export()
        .unwrap();
    assert_golden("chain-web.export.json", &export);
    assert_eq!(
        fs::read_to_string(stray.join("index.db")).unwrap(),
        "not an index\n"
    );
    let mut left: Vec<_> = (fs::read_dir(&stray).unwrap())
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(left, ["index.db", "leak.ts"]);
}

#[test]
fn a_chain_refreshed_by_index_workspace_exports_what_a_cold_one_does() {
    let mirrors = |dir: &Path| {
        let at = |name: &str| dir.join(name);
        (at("chain-web"), at("chain-gateway"), at("chain-catalog"))
    };
    let exports = |dir: &Path| -> Vec<String> {
        let manifest = dir.join("ops/meada.workspace.toml");
        (CHAIN.iter())
            .map(|(repo, _)| {
                let engine = Engine::open_manifest(&manifest, repo).unwrap();
                engine.export().unwrap()
            })
            .collect()
    };
    let dir = chain_manifest("manifest_fresh");
    CHAIN.iter().for_each(|(_, path)| age(&dir.join(path)));
    assert!(run(&dir, "index").2);
    let before = exports(&dir);
    edit_chain(&mirrors(&dir));
    // Only `meada index` refreshes: until it runs, the export is the index's, not the files'.
    assert_eq!(exports(&dir), before);

    assert!(run(&dir, "index").2);
    let refreshed = exports(&dir);

    let cold = chain_manifest("manifest_fresh_cold");
    edit_chain(&mirrors(&cold));
    assert!(run(&cold, "index").2);
    for (n, ((refreshed, before), cold)) in (refreshed.iter().zip(&before))
        .zip(exports(&cold))
        .enumerate()
    {
        // The worker is not edited.
        if n < 3 {
            assert_ne!(refreshed, before, "repository {n}");
        }
        assert_eq!(*refreshed, cold, "repository {n}");
    }
    let all: Vec<&str> = CHAIN.iter().map(|(repo, _)| *repo).collect();
    assert_nothing_else_written(&dir, &all);
}

/// The bytes of every file under `dir`, by path relative to it.
pub(crate) fn contents(dir: &Path) -> Vec<(String, Vec<u8>)> {
    (files(dir).into_iter())
        .map(|f| (f.clone(), fs::read(dir.join(&f)).unwrap()))
        .collect()
}

#[test]
fn export_prints_one_repository_over_the_manifest_and_only_index_writes() {
    let dir = chain_manifest("manifest_export");
    let web = "example.com/chain/web";

    // Before `meada index`, nothing is created or built: the error names the fix.
    let (stdout, stderr, code) = export(&dir, web);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        "meada: example.com/chain/web: not indexed; run meada index --workspace \
         ops/meada.workspace.toml\n"
    );
    assert!(!dir.join("indexes").exists());

    assert!(run(&dir, "index").2);
    let indexes = dir.join("indexes");
    let indexed = contents(&indexes);
    for (repo, path) in CHAIN {
        let (stdout, stderr, code) = export(&dir, repo);
        assert_eq!(code, Some(0), "{stderr}");
        assert_golden(&format!("{path}.export.json"), &stdout);
    }
    assert!(run(&dir, "status").2);
    assert!(contents(&indexes) == indexed, "a read wrote an index");

    // An identity the manifest does not list names the ones it serves.
    let (_, stderr, code) = export(&dir, "example.com/chain/nope");
    assert_eq!(code, Some(1));
    assert_eq!(
        stderr,
        "meada: `example.com/chain/nope` is not listed in the workspace manifest; it serves \
         example.com/chain/web, example.com/chain/gateway, example.com/chain/catalog, \
         example.com/chain/worker\n"
    );
    // A peer's index of another schema is not rebuilt by a read.
    let catalog = indexes.join("example.com/chain/catalog/index.db");
    fs::write(&catalog, "not an index\n").unwrap();
    let (stdout, stderr, code) = export(&dir, web);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        "meada: example.com/chain/catalog: indexed by another version; run meada index \
         --workspace ops/meada.workspace.toml\n"
    );
    assert_eq!(fs::read_to_string(&catalog).unwrap(), "not an index\n");

    // `--repo` and `--workspace` go together.
    let (_, stderr, code) = run_args(&dir, &["export", "--workspace", MANIFEST]);
    assert!(code == Some(2) && stderr.contains("--repo"), "{stderr}");
    let (_, stderr, code) = run_args(&dir, &["export", "--repo", web]);
    assert!(
        code == Some(2) && stderr.contains("--workspace"),
        "{stderr}"
    );
    let all: Vec<&str> = CHAIN.iter().map(|(repo, _)| *repo).collect();
    assert_nothing_else_written(&dir, &all);
}

#[test]
fn entry_blocks_configure_a_mirror_with_no_meada_toml_as_the_file_would() {
    // The chain: base URL variables, hosts and resources.
    let dir = mirrors("manifest_blocks");
    list(&dir, &CHAIN, true);
    assert!(run(&dir, "index").2);
    for (repo, path) in CHAIN {
        assert!(!dir.join(path).join("meada.toml").exists());
        let (stdout, stderr, _) = export(&dir, repo);
        assert_golden(&format!("{path}.export.json"), &stdout);
        assert_eq!(stderr, "");
    }

    // Provider documents under `serve`.
    let (web, ..) = documented_chain("manifest_blocks_documents");
    let dir = web.parent().unwrap();
    list(dir, &CHAIN, true);
    assert!(run(dir, "index").2);
    for (repo, name) in [CHAIN[0], CHAIN[1]] {
        let export = export(dir, repo).0;
        let golden = name.replace("chain-", "chain-documents-");
        assert_golden(
            &format!("{golden}.export.json"),
            &without_processes(&export),
        );
    }

    // Gateway prefixes and strips.
    let case = "manifest_blocks_gateway";
    let acme = ["ledger", "client", "orders", "billing"].map(|name| {
        let origin = format!("https://example.com/acme/{name}.git");
        fixture_repo(case, &format!("gateway-{name}"), Some(&origin));
        (
            format!("example.com/acme/{name}"),
            format!("gateway-{name}"),
        )
    });
    let repos: Vec<(&str, &str)> = (acme.iter())
        .map(|(repo, path)| (repo.as_str(), path.as_str()))
        .collect();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(case);
    list(&dir, &repos, true);
    assert!(run(&dir, "index").2);
    for (repo, name) in [repos[1], repos[3]] {
        let export = export(&dir, repo).0;
        assert_golden(&format!("{name}.export.json"), &without_processes(&export));
    }
}

#[test]
fn a_mirror_with_a_meada_toml_keeps_it_whole_and_status_names_the_ignored_blocks() {
    let dir = mirrors("manifest_ignored");
    list(&dir, &CHAIN, false);
    let manifest = dir.join(MANIFEST);
    let gateway = "path = \"../chain-gateway\"\n";
    let blocks = "\n[repository.serve]\nprefix = \"/edge\"\n\n\
                  [[repository.resource]]\nkind = \"s3-bucket\"\nid = \"invoices\"\n\
                  role = \"write\"\n\n\
                  [[repository.peer]]\nrepo = \"example.com/chain/worker\"\n\
                  base_url_env = [\"WORKER_URL\"]\n";
    let toml = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        toml.replace(gateway, &format!("{gateway}{blocks}")),
    )
    .unwrap();

    assert!(run(&dir, "index").2);

    let (stdout, _, _) = export(&dir, "example.com/chain/gateway");
    assert_golden("chain-gateway.export.json", &stdout);
    let (status, stderr, ok) = run(&dir, "status");
    assert!(ok, "{stderr}");
    assert!(
        status.ends_with(
            "\nmanifest block ignored: [repository.serve] of example.com/chain/gateway, \
             whose mirror has its own meada.toml\n\
             manifest block ignored: [[repository.resource]] s3-bucket invoices of \
             example.com/chain/gateway, whose mirror has its own meada.toml\n\
             manifest block ignored: [[repository.peer]] example.com/chain/worker of \
             example.com/chain/gateway, whose mirror has its own meada.toml\n"
        ),
        "{status}"
    );
}

#[test]
fn each_mirrors_decisions_are_replayed_and_the_stale_ones_reported() {
    let dir = chain_manifest("manifest_decisions");
    let web = dir.join("chain-web");
    decisions(&web, "chain-web");
    assert!(run(&dir, "index").2);

    let (stdout, stderr, _) = export(&dir, "example.com/chain/web");

    assert_golden("chain-web.decided.export.json", &without_processes(&stdout));
    assert_eq!(stderr, "");
    decisions(&web, "chain-web-unfit");
    let (status, _, _) = run(&dir, "status");
    assert!(status.contains("\n  stale decision: "), "{status}");
}

/// `status --workspace` as a readiness probe: 0 when every repository is servable, 1 when one
/// is not (here the gateway's index is missing), whatever the text says. Without `--workspace`
/// the exit code is what it was.
#[test]
fn status_exits_zero_only_when_every_repository_can_be_served() {
    let dir = chain_manifest("manifest_status_probe");
    assert!(run(&dir, "index").2);
    let (all, _, ok) = run(&dir, "status");
    assert!(ok);
    let gateway = dir.join("indexes/example.com/chain/gateway/index.db");
    fs::remove_file(&gateway).unwrap();
    let (some, stderr, ok) = run(&dir, "status");
    assert!(!ok && stderr.is_empty(), "{stderr}");
    assert_ne!(some, all);
    assert!(some.contains("example.com/chain/gateway (../chain-gateway) not indexed\n"));
    assert!(some.contains("example.com/chain/web (../chain-web) indexed, "));
}
