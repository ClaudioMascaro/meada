//! Server mode's workspace manifest (spec #259, ADR 0011): `meada index --workspace` and
//! `meada status --workspace` over fixture mirrors listed in a `meada.workspace.toml`, with every
//! index under the manifest's `index_dir` and nothing written into a mirror.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use crate::freshness::edit_chain;
use common::{age, assert_golden, chain, fixture_repo, git, meada};
use meada::engine::Engine;

const CHAIN: [(&str, &str); 4] = [
    ("example.com/chain/web", "chain-web"),
    ("example.com/chain/gateway", "chain-gateway"),
    ("example.com/chain/catalog", "chain-catalog"),
    ("example.com/chain/worker", "chain-worker"),
];

/// The chain's four mirrors in `<case>/`, listed by `<case>/ops/meada.workspace.toml` with
/// paths relative to it and `index_dir = "../indexes"`. Returns the case directory.
fn chain_manifest(case: &str) -> PathBuf {
    let dir = mirrors(case);
    let entries: String = CHAIN
        .iter()
        .map(|(repo, path)| format!("\n[[repository]]\nrepo = \"{repo}\"\npath = \"../{path}\"\n"))
        .collect();
    write_manifest(&dir, &format!("index_dir = \"../indexes\"\n{entries}"));
    dir
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

/// `meada <command> --workspace ops/meada.workspace.toml`, run in `dir`.
fn run(dir: &Path, command: &str) -> (String, String, bool) {
    meada(
        dir,
        &[command, "--workspace", "ops/meada.workspace.toml"],
        "",
        |c| {
            c.env("LANG", "en_US.UTF-8");
        },
    )
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
fn assert_nothing_else_written(dir: &Path, indexed: &[&str]) {
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
    assert!(ok, "{stderr}");
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

    let (stdout, stderr, ok) = run(&dir, "index");

    assert!(!ok);
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
    assert!(ok);
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
            "meada: ops/meada.workspace.toml:1: `index_dir` is missing\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\npath = \"../chain-web\"\n",
            "meada: ops/meada.workspace.toml:3: `repository.repo` is missing\n",
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
