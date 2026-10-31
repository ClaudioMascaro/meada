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
use meada::engine::{Direction, Engine, Start};

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
pub(crate) fn list(dir: &Path, repos: &[(&str, &str)], configs: bool) {
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
pub(crate) fn run_export(dir: &Path, repo: &str) -> (String, String, Option<i32>) {
    run_args(dir, &["export", "--workspace", MANIFEST, "--repo", repo])
}

/// `meada <args>` in `dir`, as `run_code`.
pub(crate) fn run_args(dir: &Path, args: &[&str]) -> (String, String, Option<i32>) {
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
/// `index.db`, SQLite's companions and the rebuild lock (`index.db.new.*` exists only during a
/// rebuild), and no mirror has a `.meada/`.
pub(crate) fn assert_nothing_else_written(dir: &Path, indexed: &[&str]) {
    let written = files(&dir.join("indexes"));
    for file in &written {
        let (repo, name) = file.rsplit_once('/').unwrap();
        assert!(indexed.contains(&repo), "{file}");
        assert!(
            [
                "index.db",
                "index.commit.json",
                "index.db.lock",
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
         meada: example.com/chain/other: the mirror's remote is `example.com/chain/gateway`, not `example.com/chain/other`\n\
         meada: index failed for 10 repositories\n"
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
         `ops/../chain-web/./x/../example.com/chain/catalog` would be inside the mirror `ops/../chain-web`\n\
         meada: index failed for 2 repositories\n"
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
            "meada: ops/meada.workspace.toml:8: `repository.serve.prefix`: must start with `/`\n",
        ),
        // An owner belongs to a workspace version (ADR 0015): a static manifest names the key
        // of the first entry that holds one, and `source = "store"`.
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\nrepo = \"example.com/chain/web\"\n\
             path = \"../chain-web\"\n\n[[repository]]\nrepo = \"example.com/chain/gateway\"\n\
             path = \"../chain-gateway\"\nowner = \"team-gateway\"\n",
            "meada: ops/meada.workspace.toml:10: `repository.owner`: an owner belongs to the \
             entries of a workspace version, which a static manifest has none of: use [workspace] \
             source = \"store\" and `meada workspace import`\n",
        ),
        // Likewise an `added_by` (#330); a repository's own `meada.toml` keeps it unknown.
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\nrepo = \"example.com/chain/web\"\n\
             path = \"../chain-web\"\nadded_by = \"ana\"\n",
            "meada: ops/meada.workspace.toml:6: `repository.added_by`: an `added_by` belongs to \
             the entries of a workspace version, which a static manifest has none of: use \
             [workspace] source = \"store\" and `meada workspace import`\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\nrepo = \"example.com/chain/web\"\n\
             path = \"../chain-web\"\nowner = \" team\"\n",
            "meada: ops/meada.workspace.toml:6: `repository.owner`: has whitespace at either end\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[[repository]]\nrepo = \"example.com/chain/web\"\n\
             path = \"../chain-web\"\nowner = [\"a\", \"b\"]\n",
            "meada: ops/meada.workspace.toml:6: `repository.owner`: must be one string, the name \
             of one group\n",
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

/// A section a module owns, in a build without the module's feature (ADR 0012): an error naming
/// the file, the line, the section and the feature. A section the table does not list stays an
/// unknown key.
#[cfg(not(feature = "valkey"))]
#[test]
fn a_module_section_without_its_feature_is_an_error_naming_the_feature() {
    let dir = mirrors("manifest_module_sections");
    let mut cases = vec![
        (
            "index_dir = \"../indexes\"\n\n[valkey]\nurl = \"redis://127.0.0.1\"\n",
            "meada: ops/meada.workspace.toml:3: [valkey] needs a Meada built with the valkey feature\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n\n[generations]\nkeep = 3\n",
            "meada: ops/meada.workspace.toml:4: [generations] needs a Meada built with the generation-pointer feature\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[refresh_lock]\nttl_seconds = 300\n",
            "meada: ops/meada.workspace.toml:3: [refresh_lock] needs a Meada built with the refresh-lock feature\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[cache]\nurl = \"redis://127.0.0.1\"\n",
            "meada: ops/meada.workspace.toml:3: `cache` is not a meada.workspace.toml key\n",
        ),
        // The session store was not built (#277): its section is an unknown key too.
        (
            "index_dir = \"../indexes\"\n\n[sessions]\nttl_seconds = 60\n",
            "meada: ops/meada.workspace.toml:3: `sessions` is not a meada.workspace.toml key\n",
        ),
    ];
    // `oidc` needs no `valkey`, so a build may have it here.
    if !cfg!(feature = "oidc") {
        cases.push((
            "index_dir = \"../indexes\"\n[oidc]\nissuer = \"https://id.example.com\"\n",
            "meada: ops/meada.workspace.toml:2: [oidc] needs a Meada built with the oidc feature\n",
        ));
    }
    // Nor does `postgres`.
    if !cfg!(feature = "postgres") {
        cases.push((
            "index_dir = \"../indexes\"\n\n[postgres]\nschema = \"meada\"\n",
            "meada: ops/meada.workspace.toml:3: [postgres] needs a Meada built with the postgres feature\n",
        ));
    }
    // Nor does `directory`.
    if !cfg!(feature = "directory") {
        cases.push((
            "index_dir = \"../indexes\"\n\n[directory]\nuser_claim = \"oid\"\n",
            "meada: ops/meada.workspace.toml:3: [directory] needs a Meada built with the directory \
             feature\n",
        ));
    }
    // Nor does `catalog` (#341).
    if !cfg!(feature = "workspaces") {
        cases.push((
            "index_dir = \"../indexes\"\n\n[catalog]\nallow = [\"github.com/acme/*\"]\n",
            "meada: ops/meada.workspace.toml:3: [catalog] needs a Meada built with the workspaces \
             feature\n",
        ));
    }
    // Nor does `proposals` (ADR 0022).
    if !cfg!(feature = "proposals") {
        cases.push((
            "index_dir = \"../indexes\"\n\n[proposals]\npublic_url = \"https://p.example\"\n",
            "meada: ops/meada.workspace.toml:3: [proposals] needs a Meada built with the \
             proposals feature\n",
        ));
    }
    // `ui` needs no `valkey` either.
    if !cfg!(feature = "ui") {
        cases.push((
            "index_dir = \"../indexes\"\n\n\n\n[ui]\ndir = \"ui\"\n",
            "meada: ops/meada.workspace.toml:5: [ui] needs a Meada built with the ui feature\n",
        ));
    }
    for (toml, expected) in cases {
        write_manifest(&dir, toml);
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {toml}");
            assert_eq!(stderr, expected, "{command}: {toml}");
        }
    }
}

/// `[snapshot]` in a build without the `snapshot` feature (ADR 0012): the error naming it.
#[cfg(not(feature = "snapshot"))]
#[test]
fn a_snapshot_section_without_its_feature_is_an_error_naming_the_feature() {
    let dir = mirrors("manifest_snapshot_section");
    write_manifest(
        &dir,
        "index_dir = \"../indexes\"\n\n[snapshot]\nendpoint = \"https://s3.example.com\"\n",
    );
    for command in ["index", "status"] {
        let (stdout, stderr, ok) = run(&dir, command);
        assert!(!ok && stdout.is_empty(), "{command}");
        assert_eq!(
            stderr,
            "meada: ops/meada.workspace.toml:3: [snapshot] needs a Meada built with the snapshot \
             feature\n",
            "{command}"
        );
    }
}

/// The 1-based line of the first `found` in `text`.
#[cfg(feature = "generation-pointer")]
pub(crate) fn line_of(text: &str, found: &str) -> usize {
    text[..text.find(found).unwrap()].matches('\n').count() + 1
}

/// `[workspace]` (ADR 0014) is read in every build: an unknown key and an unknown source name the
/// file, the line and the key; without `generation-pointer`, `source = "store"` names the
/// feature. `source = "file"` is the file as before.
#[test]
fn the_workspace_section_is_read_in_every_build() {
    let dir = mirrors("manifest_workspace_section");
    let mut cases = vec![
        (
            "index_dir = \"../indexes\"\n\n[workspace]\nsource = \"git\"\n",
            "meada: ops/meada.workspace.toml:4: `workspace.source`: is not one of the values it accepts \
             (`file`, `store`)\n",
        ),
        (
            "index_dir = \"../indexes\"\n\n[workspace]\nrepositories = \"store\"\n",
            "meada: ops/meada.workspace.toml:4: `workspace.repositories` is not a \
             meada.workspace.toml key\n",
        ),
    ];
    if !cfg!(feature = "generation-pointer") {
        cases.push((
            "index_dir = \"../indexes\"\n\n[workspace]\n\nsource = \"store\"\n",
            "meada: ops/meada.workspace.toml:5: `workspace.source`: \"store\" needs a Meada built \
             with the generation-pointer feature\n",
        ));
    }
    for (toml, expected) in cases {
        write_manifest(&dir, toml);
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {toml}");
            assert_eq!(stderr, expected, "{command}: {toml}");
        }
    }
    let dir = chain_manifest("manifest_workspace_file");
    let manifest = dir.join(MANIFEST);
    let toml = fs::read_to_string(&manifest).unwrap() + "\n[workspace]\nsource = \"file\"\n";
    fs::write(&manifest, toml).unwrap();
    let (_, stderr, ok) = run(&dir, "index");
    assert!(ok, "{stderr}");
    let (stdout, stderr, ok) = run(&dir, "status");
    assert!(ok, "{stderr}");
    for (repo, _) in CHAIN {
        assert!(stdout.contains(repo), "{stdout}");
    }
}

/// `source = "store"` with the feature: without `[snapshot]` or `[generations]` the error names
/// them, and beside a `[[repository]]` it names both.
#[cfg(feature = "generation-pointer")]
#[test]
fn a_store_source_needs_its_sections_and_no_repository_entry() {
    let dir = mirrors("manifest_workspace_store");
    let valkey = "\n[valkey]\nurl = \"redis://127.0.0.1:6390\"\n";
    let snapshot = crate::snapshot::section("http://127.0.0.1:9", "nowhere");
    let store = "\n[workspace]\nsource = \"store\"\n";
    let entry = "\n[[repository]]\nrepo = \"example.com/chain/web\"\npath = \"../chain-web\"\n";
    let head = "index_dir = \"../indexes\"\n";
    let sections = format!("{snapshot}{valkey}\n[generations]\nkeep = 3\n");
    let cases = [
        (
            format!("{head}{store}"),
            "[workspace]",
            "[workspace] needs [snapshot] and [generations]",
        ),
        (
            format!("{head}{snapshot}{store}"),
            "[workspace]",
            "[workspace] needs [generations]",
        ),
        (
            format!("{head}{sections}{store}{entry}"),
            "[[repository]]",
            "`repository`: [[repository]] entries beside [workspace] source = \"store\": the \
             repositories come from the current workspace version",
        ),
    ];
    for (toml, at, problem) in cases {
        write_manifest(&dir, &toml);
        let expected = format!(
            "meada: ops/meada.workspace.toml:{}: {problem}\n",
            line_of(&toml, at)
        );
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {toml}");
            assert_eq!(stderr, expected, "{command}: {toml}");
        }
    }
}

/// The `[valkey]` section of a build that has the module: its keys are checked, and each error
/// names the file, the line and the key without echoing the URL.
#[cfg(feature = "valkey")]
#[test]
fn valkey_section_errors_name_the_file_the_line_and_the_key() {
    let dir = mirrors("manifest_valkey_errors");
    let head = "index_dir = \"../indexes\"\n\n[valkey]\n";
    let at = "meada: ops/meada.workspace.toml";
    for (body, expected) in [
        (
            "url = \"redis://127.0.0.1\"\nttl = 3\n",
            format!("{at}:5: `valkey.ttl` is not a meada.workspace.toml key\n"),
        ),
        (
            "key_prefix = \"x\"\n",
            format!("{at}:3: `valkey.url` is missing\n"),
        ),
        (
            "url = \"rediss://meada:hunter2@cache.example.internal\"\n",
            format!(
                "{at}:4: `valkey.url`: carries a password: name it in password_env or password_file\n"
            ),
        ),
        (
            "url = \"redis://cache.example.internal\"\n",
            format!(
                "{at}:4: `valkey.url`: must be a rediss:// URL: TLS is required unless the host is loopback\n"
            ),
        ),
        (
            "url = \"redis://127.0.0.1\"\npassword_env = \"V\"\npassword_file = \"v\"\n",
            format!(
                "{at}:6: `valkey.password_file`: is set together with password_env: use one of the two\n"
            ),
        ),
        (
            "url = \"redis://127.0.0.1\"\ncluster = true\n",
            format!(
                "{at}:5: `valkey.cluster`: needs a rediss:// url: the nodes a cluster announces are reached over TLS\n"
            ),
        ),
        (
            "url = \"redis://127.0.0.1\"\nkey_prefix = \"a{b}\"\n",
            format!("{at}:5: `valkey.key_prefix`: must not contain `{{` or `}}`\n"),
        ),
    ] {
        write_manifest(&dir, &format!("{head}{body}"));
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {body}");
            assert_eq!(stderr, expected, "{command}: {body}");
        }
    }
}

/// The identity provider's `type` and its table (ADR 0016): each error names the file, the line
/// and the key, and none echoes a value, a token or a URL.
#[cfg(feature = "oidc")]
#[test]
fn oidc_type_errors_name_the_file_the_line_and_the_key() {
    let dir = mirrors("manifest_oidc_type_errors");
    let head = "index_dir = \"../indexes\"\n\n[oidc]\nissuer = \"https://id.example\"\npublic_url = \
                \"https://meada.example\"\nidentities_claim = \"r\"\n";
    let at = "meada: ops/meada.workspace.toml";
    let types = "must be one of `generic`, `entra`, `okta` or `keycloak`";
    let entra = "`oidc.entra`: needs `oidc.type = \"entra\"`";
    let overage = "`oidc.entra.overage`: must be one of `ignore` or `directory`";
    for (body, expected) in [
        (
            "type = \"azure\"\n",
            format!("{at}:7: `oidc.type`: {types}\n"),
        ),
        ("type = 1\n", format!("{at}:7: `oidc.type`: {types}\n")),
        ("[oidc.entra]\n", format!("{at}:7: {entra}\n")),
        (
            "type = \"generic\"\n[oidc.entra]\n",
            format!("{at}:8: {entra}\n"),
        ),
        (
            "type = \"okta\"\n[oidc.entra]\noverage = \"ignore\"\n",
            format!("{at}:8: {entra}\n"),
        ),
        (
            "type = \"entra\"\n[oidc.entra]\noverage = \"always\"\n",
            format!("{at}:9: {overage}\n"),
        ),
        (
            "type = \"entra\"\n[oidc.entra]\ntenant = \"https://login.example\"\n",
            format!("{at}:9: `oidc.entra.tenant` is not a meada.workspace.toml key\n"),
        ),
        (
            "[oidc.github]\nlogin_claim = \"login\"\nmax_age_seconds = 60\norg = \"acme\"\n",
            format!("{at}:10: `oidc.github.org` is not a meada.workspace.toml key\n"),
        ),
    ] {
        write_manifest(&dir, &format!("{head}{body}"));
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {body}");
            assert_eq!(stderr, expected, "{command}: {body}");
        }
    }
}

/// `overage = "directory"` (#336) needs `[oidc] groups_claim` and `[directory]`: each missing one
/// is the error naming the file, the line and the key or section (a build without the
/// `directory` feature names the feature instead), and under a `type` other than `entra` the
/// error of `[oidc.entra]` stands.
#[cfg(feature = "oidc")]
#[test]
fn oidc_overage_directory_names_what_it_needs() {
    let dir = mirrors("manifest_oidc_overage");
    let head = "index_dir = \"../indexes\"\n\n[oidc]\nissuer = \"https://id.example\"\npublic_url = \
                \"https://meada.example\"\nidentities_claim = \"r\"\n";
    let at = "meada: ops/meada.workspace.toml";
    let feature =
        "`oidc.entra.overage`: [oidc.entra] needs a Meada built with the directory feature";
    let claim = "`oidc.entra.overage`: `oidc.groups_claim` is missing";
    let section = "`oidc.entra.overage`: [oidc.entra] needs [directory]";
    let groups = "groups_claim = \"groups\"\n[oidc.groups]\n\"g\" = [\"a\"]\n";
    let directory = cfg!(feature = "directory");
    for (body, expected) in [
        (
            "type = \"entra\"\n[oidc.entra]\noverage = \"directory\"\n".to_string(),
            format!("{at}:9: {}\n", if directory { claim } else { feature }),
        ),
        (
            format!("type = \"entra\"\n{groups}[oidc.entra]\noverage = \"directory\"\n"),
            format!("{at}:12: {}\n", if directory { section } else { feature }),
        ),
        (
            "type = \"okta\"\n[oidc.entra]\noverage = \"directory\"\n".to_string(),
            format!("{at}:8: `oidc.entra`: needs `oidc.type = \"entra\"`\n"),
        ),
    ] {
        write_manifest(&dir, &format!("{head}{body}"));
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {body}");
            assert_eq!(stderr, expected, "{command}: {body}");
        }
    }
}

/// The GitHub grant provider's `[oidc.github]` (#331): each key error, the table without
/// `[postgres]` and, in a build without `postgres`, the table at all, name the file, the line
/// and the key or the sections, never a value; alone, without `identities_claim` or
/// `groups_claim`, it is accepted. `meada github-app roles` needs a `[[github_app]]` block for
/// its host.
#[cfg(feature = "oidc")]
#[test]
fn oidc_github_errors_name_the_file_the_line_and_the_key() {
    let dir = mirrors("manifest_oidc_github_errors");
    let oidc = "index_dir = \"../indexes\"\n\n[oidc]\nissuer = \"https://id.example\"\npublic_url = \
                \"https://meada.example\"\n";
    let at = "meada: ops/meada.workspace.toml";
    let table = "[oidc.github]\nlogin_claim = \"github_login\"\nmax_age_seconds = 5400\n";
    let refused = |toml: &str, expected: &str| {
        write_manifest(&dir, toml);
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {toml}");
            assert_eq!(stderr, expected, "{command}: {toml}");
        }
    };
    if !cfg!(feature = "postgres") {
        let feature = "[oidc.github] needs a Meada built with the postgres feature";
        refused(
            &format!("{oidc}{table}"),
            &format!("{at}:6: `oidc.github`: {feature}\n"),
        );
        return;
    }
    let postgres = "\n[postgres]\nschema = \"m\"\n";
    refused(
        &format!("{oidc}{table}"),
        &format!("{at}:6: `oidc.github`: [oidc.github] needs [postgres]\n"),
    );
    let roles = "must be a non-empty array of distinct role names of 1 to 100 characters with no \
                 control character";
    for (body, expected) in [
        (
            "[oidc.github]\nmax_age_seconds = 60\n",
            format!("{at}:6: `oidc.github`: `oidc.github.login_claim` is missing\n"),
        ),
        (
            "[oidc.github]\nlogin_claim = \"l\"\n",
            format!("{at}:6: `oidc.github`: `oidc.github.max_age_seconds` is missing\n"),
        ),
        (
            "[oidc.github]\nlogin_claim = \"\"\nmax_age_seconds = 60\n",
            format!("{at}:7: `oidc.github.login_claim`: must not be empty\n"),
        ),
        (
            "[oidc.github]\nlogin_claim = \"l\"\nmax_age_seconds = 59\n",
            format!("{at}:8: `oidc.github.max_age_seconds`: must be at least 60\n"),
        ),
        (
            "[oidc.github]\nhost = \"GitHub.com\"\nlogin_claim = \"l\"\nmax_age_seconds = 60\n",
            format!("{at}:7: `oidc.github.host`: must be a lowercase host name\n"),
        ),
        (
            "[oidc.github]\nlogin_claim = \"l\"\nroles = [\"read\", \"read\"]\nmax_age_seconds = 60\n",
            format!("{at}:8: `oidc.github.roles`: {roles}\n"),
        ),
        (
            "[oidc.github]\nlogin_claim = \"l\"\nroles = []\nmax_age_seconds = 60\n",
            format!("{at}:8: `oidc.github.roles`: {roles}\n"),
        ),
        (
            "[oidc.github]\nlogin_claim = \"l\"\nmax_age_seconds = 60\nlogin = \"x\"\n",
            format!("{at}:9: `oidc.github.login` is not a meada.workspace.toml key\n"),
        ),
    ] {
        refused(&format!("{oidc}{body}{postgres}"), &expected);
    }
    // Alone: no `identities_claim` or `groups_claim` is needed beside it.
    write_manifest(&dir, &format!("{oidc}{table}{postgres}"));
    let (_, stderr, ok) = run(&dir, "index");
    assert!(ok, "{stderr}");
    // The role check lists collaborators through the installations of its host.
    #[cfg(feature = "github-app")]
    for apps in [
        "",
        "\n[valkey]\nurl = \"redis://127.0.0.1:9\"\n\n[[github_app]]\nhost = \"ghe.example\"\n\
         api = \"https://ghe.example/api/v3\"\napp_id = 1\nprivate_key_env = \"K\"\n",
    ] {
        write_manifest(&dir, &format!("{oidc}{table}{postgres}{apps}"));
        let args = ["github-app", "roles", "--workspace", MANIFEST];
        let (stdout, stderr, code) = run_args(&dir, &args);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{apps}");
        let needed = "meada github-app roles needs a [[github_app]] block for host github.com";
        assert_eq!(
            stderr,
            format!("{at}:6: `oidc.github`: {needed}\n"),
            "{apps}"
        );
    }
}

/// The `[postgres]` section of a build that has the module (ADR 0016): each key error names the
/// file, the line and the key.
#[cfg(feature = "postgres")]
#[test]
fn postgres_section_errors_name_the_file_the_line_and_the_key() {
    let dir = mirrors("manifest_postgres_errors");
    let head = "index_dir = \"../indexes\"\n\n[postgres]\n";
    let at = "meada: ops/meada.workspace.toml";
    let schema = "must be 1 to 63 lowercase letters, digits or `_`, starting with a letter";
    let long = "a".repeat(64);
    for (body, expected) in [
        (
            "schema = \"m\"\nurl = \"postgres://localhost\"\n".to_string(),
            format!("{at}:5: `postgres.url` is not a meada.workspace.toml key\n"),
        ),
        (
            "url_env = \"P\"\nurl_file = \"p\"\n".into(),
            format!(
                "{at}:5: `postgres.url_file`: is set together with url_env: use one of the two\n"
            ),
        ),
        (
            "schema = \"Meada\"\n".into(),
            format!("{at}:4: `postgres.schema`: {schema}\n"),
        ),
        (
            "\nschema = \"1meada\"\n".into(),
            format!("{at}:5: `postgres.schema`: {schema}\n"),
        ),
        (
            format!("schema = \"{long}\"\n"),
            format!("{at}:4: `postgres.schema`: {schema}\n"),
        ),
        (
            "pool_size = 0\n".into(),
            format!("{at}:4: `postgres.pool_size`: must be at least 1\n"),
        ),
        (
            "connect_timeout_seconds = 0\n".into(),
            format!("{at}:4: `postgres.connect_timeout_seconds`: must be at least 1\n"),
        ),
        (
            "schema = \"m\"\nstatement_timeout_seconds = 0\n".into(),
            format!("{at}:5: `postgres.statement_timeout_seconds`: must be at least 1\n"),
        ),
    ] {
        write_manifest(&dir, &format!("{head}{body}"));
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {body}");
            assert_eq!(stderr, expected, "{command}: {body}");
        }
        let migrate = ["postgres", "migrate", "--workspace", MANIFEST];
        let (stdout, stderr, code) = run_args(&dir, &migrate);
        assert!(code == Some(1) && stdout.is_empty(), "migrate: {body}");
        assert_eq!(stderr, expected, "migrate: {body}");
    }
}

/// The `[directory]` section (#334): without `[postgres]`, without `[oidc]`, with both token
/// keys, with an unknown key and with a `user_attribute` it does not accept, each error names the
/// file, the line and the sections or the key, and never a value.
#[cfg(all(feature = "directory", feature = "oidc"))]
#[test]
fn directory_section_errors_name_the_file_the_line_and_the_keys() {
    let dir = mirrors("manifest_directory_errors");
    let at = "meada: ops/meada.workspace.toml";
    let head = "index_dir = \"../indexes\"\n";
    let postgres = "\n[postgres]\nschema = \"m\"\n";
    let oidc = "\n[oidc]\nissuer = \"https://id.example.com\"\npublic_url = \"https://m.example\"\n\
                identities_claim = \"r\"\n";
    let directory = "\n[directory]\nuser_claim = \"oid\"\nuser_attribute = \"userName\"\n";
    let full = format!("{head}{postgres}{oidc}{directory}");
    for (toml, expected) in [
        (
            format!("{head}{oidc}{directory}"),
            format!("{at}:8: [directory] needs [postgres]\n"),
        ),
        (
            format!("{head}{postgres}{directory}"),
            format!("{at}:6: [directory] needs [oidc]\n"),
        ),
        (
            format!("{head}{directory}"),
            format!("{at}:3: [directory] needs [postgres] and [oidc]\n"),
        ),
        (
            format!("{full}token_env = \"SCIM\"\ntoken_file = \"scim-token\"\n"),
            format!(
                "{at}:15: `directory.token_file`: is set together with token_env: use one of the \
                 two\n"
            ),
        ),
        (
            format!("{full}token = \"scim-secret-334\"\n"),
            format!("{at}:14: `directory.token` is not a meada.workspace.toml key\n"),
        ),
        (
            full.replace("\"userName\"", "\"email\""),
            format!(
                "{at}:13: `directory.user_attribute`: must be one of `userName` or `externalId`\n"
            ),
        ),
        (
            full.replace("user_claim = \"oid\"\n", ""),
            format!("{at}:11: `directory`: `directory.user_claim` is missing\n"),
        ),
    ] {
        write_manifest(&dir, &toml);
        let (stdout, stderr, ok) = run(&dir, "status");
        assert!(!ok && stdout.is_empty(), "{toml}");
        assert_eq!(stderr, expected, "{toml}");
        assert!(!stderr.contains("scim-secret-334"), "{stderr}");
    }
}

/// The `[catalog]` section (#341): without `[oidc.github]`, with an allow list that is not
/// patterns on its host, with a missing or out-of-range cap and with an unknown key, each error
/// names the file, the line and the key, and never a pattern.
#[cfg(feature = "workspaces")]
#[test]
fn catalog_section_errors_name_the_file_the_line_and_the_keys() {
    let dir = mirrors("manifest_catalog_errors");
    let at = "meada: ops/meada.workspace.toml";
    let head = "index_dir = \"../indexes\"\n";
    let oidc = "\n[oidc]\nissuer = \"https://id.example.com\"\npublic_url = \"https://m.example\"\n\
                identities_claim = \"r\"\n\n[oidc.github]\nhost = \"github.com\"\nlogin_claim = \"l\"\n\
                max_age_seconds = 600\n\n[postgres]\nschema = \"m\"\n";
    let catalog = "\n[catalog]\nallow = [\"github.com/secret-owner/*\"]\nmax_repositories = 50\n";
    let full = format!("{head}{oidc}{catalog}");
    let allow = "allow = [\"github.com/secret-owner/*\"]";
    let line = full.lines().position(|l| l == "[catalog]").unwrap() + 1;
    let keyed = |at_line: usize, key: &str, why: &str| format!("{at}:{at_line}: `{key}`: {why}\n");
    for (toml, expected) in [
        (
            format!("{head}{catalog}"),
            format!("{at}:3: [catalog] needs [oidc.github]\n"),
        ),
        (
            full.replace(allow, "allow = [\"gitlab.com/secret-owner/*\"]"),
            keyed(
                line + 1,
                "catalog.allow",
                "must be a non-empty array of distinct lowercase patterns, each \
                 github.com/<owner>/* or github.com/<owner>/<repo>",
            ),
        ),
        (
            full.replace(allow, "allow = []"),
            keyed(
                line + 1,
                "catalog.allow",
                "must be a non-empty array of distinct lowercase patterns, each \
                 github.com/<owner>/* or github.com/<owner>/<repo>",
            ),
        ),
        (
            full.replace("max_repositories = 50\n", ""),
            format!("{at}:{line}: `catalog`: `catalog.max_repositories` is missing\n"),
        ),
        (
            full.replace("= 50", "= 5001"),
            keyed(line + 2, "catalog.max_repositories", "must be at most 5000"),
        ),
        (
            full.replace("= 50", "= 0"),
            keyed(line + 2, "catalog.max_repositories", "must be at least 1"),
        ),
        (
            format!("{full}secret = \"x\"\n"),
            format!(
                "{at}:{}: `catalog.secret` is not a meada.workspace.toml key\n",
                line + 3
            ),
        ),
    ] {
        write_manifest(&dir, &toml);
        let (stdout, stderr, ok) = run(&dir, "status");
        assert!(!ok && stdout.is_empty(), "{toml}");
        assert_eq!(stderr, expected, "{toml}");
        assert!(!stderr.contains("secret-owner"), "{stderr}");
    }
}

/// The `[proposals]` section (ADR 0022): without `[workspaces]`, with a URL that is not `https`
/// off loopback or has a path, with an empty audience and with an unknown key, each error names
/// the file, the line and the key.
#[cfg(feature = "proposals")]
#[test]
fn proposals_section_errors_name_the_file_the_line_and_the_keys() {
    let dir = mirrors("manifest_proposals_errors");
    let at = "meada: ops/meada.workspace.toml";
    let head = "index_dir = \"../indexes\"\n\n[proposals]\n";
    let url = "must be an https:// URL with a host and nothing after it (http:// only on loopback)";
    for (toml, expected) in [
        (
            format!("{head}public_url = \"https://p.example\"\n"),
            format!("{at}:3: [proposals] needs [workspaces]\n"),
        ),
        (
            format!("{head}audience = \"agents\"\n"),
            format!("{at}:3: `proposals`: `proposals.public_url` is missing\n"),
        ),
        (
            format!("{head}public_url = \"http://p.example\"\n"),
            format!("{at}:4: `proposals.public_url`: {url}\n"),
        ),
        (
            format!("{head}public_url = \"https://p.example/mcp\"\n"),
            format!("{at}:4: `proposals.public_url`: {url}\n"),
        ),
        (
            format!("{head}public_url = \"https://p.example\"\naudience = \"\"\n"),
            format!("{at}:5: `proposals.audience`: must not be empty\n"),
        ),
        (
            format!("{head}public_url = \"https://p.example\"\ngrant = \"*\"\n"),
            format!("{at}:5: `proposals.grant` is not a meada.workspace.toml key\n"),
        ),
    ] {
        write_manifest(&dir, &toml);
        let (stdout, stderr, ok) = run(&dir, "status");
        assert!(!ok && stdout.is_empty(), "{toml}");
        assert_eq!(stderr, expected, "{toml}");
    }
}

/// The local Valkey `MEADA_TEST_VALKEY_URL` names, or `None` with the reason printed;
/// `MEADA_TEST_SERVICES_REQUIRED=1` turns the missing variable into a failure.
#[cfg(feature = "valkey")]
fn valkey_service() -> Option<String> {
    let url = std::env::var("MEADA_TEST_VALKEY_URL").ok();
    if url.is_none() {
        let required = std::env::var("MEADA_TEST_SERVICES_REQUIRED").is_ok_and(|v| v == "1");
        assert!(!required, "MEADA_TEST_VALKEY_URL is not set");
        eprintln!("skipped: MEADA_TEST_VALKEY_URL is not set");
    }
    url
}

/// The chain's manifest with a `[valkey]` section made of `lines`, indexed.
#[cfg(feature = "valkey")]
fn valkey_manifest(case: &str, lines: &str) -> PathBuf {
    let dir = chain_manifest(case);
    assert!(run(&dir, "index").2);
    let ops = dir.join(MANIFEST);
    let toml = fs::read_to_string(&ops).unwrap();
    // Top-level tables go before the first `[[repository]]`.
    let (head, entries) = toml.split_once("\n[[repository]]").unwrap();
    fs::write(
        &ops,
        format!("{head}\n[valkey]\n{lines}\n[[repository]]{entries}"),
    )
    .unwrap();
    dir
}

/// `status` names Valkey on one line and its exit code stays what local state alone says: a
/// dead Valkey neither fails an indexed workspace nor stalls the probe, a live one does not
/// rescue an unindexed one.
#[cfg(feature = "valkey")]
#[test]
fn status_names_whether_valkey_is_reachable_and_its_exit_code_ignores_it() {
    // Nothing listens on port 1.
    let dir = valkey_manifest("manifest_valkey_dead", "url = \"redis://127.0.0.1:1\"\n");
    let start = std::time::Instant::now();
    let (status, stderr, code) = run_args(&dir, &["status", "--workspace", MANIFEST]);
    let took = start.elapsed();
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(status.contains("\nvalkey: unreachable ("), "{status}");
    assert!(took < std::time::Duration::from_millis(1500), "{took:?}");
    // Unindexed: still not ready, whatever Valkey says.
    fs::remove_file(dir.join("indexes/example.com/chain/web/index.db")).unwrap();
    let (status, _, code) = run_args(&dir, &["status", "--workspace", MANIFEST]);
    assert_eq!(code, Some(1));
    assert!(status.contains("valkey: unreachable ("), "{status}");

    let Some(url) = valkey_service() else { return };
    let dir = valkey_manifest("manifest_valkey_live", &format!("url = \"{url}\"\n"));
    let (status, stderr, code) = run_args(&dir, &["status", "--workspace", MANIFEST]);
    assert!(code == Some(0) && stderr.is_empty(), "{stderr}");
    assert!(status.ends_with("\nvalkey: reachable\n"), "{status}");
}

/// A `password_file` that is a FIFO nobody writes is refused at once: the probe neither hangs on
/// the open nor changes its exit code.
#[cfg(all(feature = "valkey", unix))]
#[test]
fn status_refuses_a_password_file_that_is_not_a_regular_file_without_stalling() {
    let lines = "url = \"redis://127.0.0.1:1\"\npassword_file = \"../pass-fifo\"\n";
    let dir = valkey_manifest("manifest_valkey_fifo", lines);
    let fifo = dir.join("pass-fifo");
    let _ = fs::remove_file(&fifo);
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let start = std::time::Instant::now();
    let (status, stderr, code) = run_args(&dir, &["status", "--workspace", MANIFEST]);
    assert!(start.elapsed() < std::time::Duration::from_millis(1500));
    assert!(code == Some(0) && stderr.is_empty(), "{stderr}");
    assert!(
        status.contains("pass-fifo: not a regular file)"),
        "{status}"
    );
}

/// A refused AUTH and a failed TLS handshake leave the password out of stdout and stderr.
#[cfg(feature = "valkey")]
#[test]
fn status_never_prints_the_valkey_password() {
    let Some(url) = valkey_service() else { return };
    let secret = "known-secret-7";
    let tls = url.replacen("redis://", "rediss://", 1);
    // The default user has no such password: AUTH is refused. A TLS handshake with a plain
    // port fails.
    for (case, url) in [("auth", url), ("tls", tls)] {
        let lines = format!("url = \"{url}\"\npassword_file = \"../valkey-pass\"\n");
        let dir = valkey_manifest(&format!("manifest_valkey_secret_{case}"), &lines);
        fs::write(dir.join("valkey-pass"), format!("{secret}\n")).unwrap();
        let (status, stderr, code) = run_args(&dir, &["status", "--workspace", MANIFEST]);
        assert_eq!(code, Some(0), "{case}: {stderr}");
        assert!(status.contains("valkey: unreachable ("), "{case}: {status}");
        for text in [&status, &stderr] {
            assert!(!text.contains(secret), "{case}: {text}");
        }
    }
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

/// "Build once, distribute" (spec #259): an index built elsewhere for the same mirror commits,
/// renamed over `<index_dir>/<identity>/index.db`, answers as the one it replaces, and an engine
/// opened before the rename keeps answering from the old one.
#[test]
fn an_index_built_elsewhere_and_renamed_in_answers_as_the_one_it_replaces() {
    let dir = chain_manifest("manifest_renamed_in");
    assert!(run(&dir, "index").2);
    let manifest = dir.join(MANIFEST);
    let web = "example.com/chain/web";
    let trace =
        |engine: &Engine| (engine.trace(Start::From("dashboard"), Direction::Down, None)).unwrap();
    let before = Engine::open_manifest(&manifest, web).unwrap();
    let served = trace(&before);
    let built = "ops/built.workspace.toml";
    let toml = fs::read_to_string(&manifest).unwrap();
    fs::write(dir.join(built), toml.replace("../indexes", "../built")).unwrap();
    assert_eq!(run_args(&dir, &["index", "--workspace", built]).2, Some(0));

    let index = |under: &str| dir.join(under).join(web).join("index.db");
    fs::rename(index("built"), index("indexes")).unwrap();

    assert_eq!(trace(&before), served);
    drop(before);
    let after = Engine::open_manifest(&manifest, web).unwrap();
    assert_eq!(trace(&after), served);
    assert_golden("chain.trace-down.txt", &served);
    assert_golden("chain-web.export.json", &after.export().unwrap());
    let all: Vec<&str> = CHAIN.iter().map(|(repo, _)| *repo).collect();
    assert_nothing_else_written(&dir, &all);
}

/// The bytes of every file under `dir`, by path relative to it.
pub(crate) fn contents(dir: &Path) -> Vec<(String, Vec<u8>)> {
    (files(dir).into_iter())
        .map(|f| (f.clone(), fs::read(dir.join(&f)).unwrap()))
        .collect()
}

/// A directory tree made read-only (directories `0555`, files `0444`) until dropped, when the
/// permissions come back so the temp dir can be cleaned.
#[cfg(unix)]
pub(crate) struct ReadOnly(Vec<(PathBuf, fs::Permissions)>);

#[cfg(unix)]
impl ReadOnly {
    /// Locks `dir` down, or `None` with a printed reason when permissions do not bind this user
    /// (root).
    pub(crate) fn lock(dir: &Path) -> Option<Self> {
        use std::os::unix::fs::PermissionsExt;
        let mut all = vec![dir.to_path_buf()];
        let mut at = 0;
        while at < all.len() {
            if all[at].is_dir() && !all[at].is_symlink() {
                all.extend(fs::read_dir(&all[at]).unwrap().map(|e| e.unwrap().path()));
            }
            at += 1;
        }
        let saved: Vec<_> = (all.into_iter().filter(|p| !p.is_symlink()))
            .map(|p| {
                let was = fs::metadata(&p).unwrap().permissions();
                (p, was)
            })
            .collect();
        let locked = Self(saved);
        for (path, _) in &locked.0 {
            let mode = if path.is_dir() { 0o555 } else { 0o444 };
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        }
        if fs::File::create(dir.join("probe")).is_ok() {
            let _ = fs::remove_file(dir.join("probe"));
            eprintln!("skipped: permissions do not bind this user (root)");
            return None;
        }
        Some(locked)
    }
}

#[cfg(unix)]
impl Drop for ReadOnly {
    fn drop(&mut self) {
        // Directories first, so what is inside them can be reached again.
        for (path, was) in &self.0 {
            let _ = fs::set_permissions(path, was.clone());
        }
    }
}

/// A mirror owned by another user (`GIT_TEST_ASSUME_DIFFERENT_OWNER`): in server mode Meada
/// passes `-c safe.directory=<mirror>` for the manifest's mirrors, so the start checks pass;
/// `git` itself still refuses the mirror, and a local run in it gets no such option.
#[test]
fn a_mirror_owned_by_another_user_is_trusted_in_server_mode() {
    let dir = chain_manifest("manifest_other_owner");
    assert!(run(&dir, "index").2);
    let owned = |cwd: &Path, args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_meada"))
            .current_dir(cwd)
            .args(args)
            .env("LANG", "en_US.UTF-8")
            .env("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1")
            // A runner's own `safe.directory` (even `*`) must not trust the mirror.
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("meada runs");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let mirror = dir.join("chain-web");

    // Without the option, git refuses; a git that does not (older than 2.38, or one that
    // ignores the variable) cannot be tested here.
    let refused = Command::new("git")
        .arg("-C")
        .arg(&mirror)
        .args(["rev-parse", "--show-toplevel"])
        .env("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    if refused.status.success() {
        eprintln!("skipped: this git does not refuse a mirror owned by another user");
        return;
    }

    let (code, stderr) = owned(&dir, &["status", "--workspace", MANIFEST]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    let (code, stderr) = owned(&dir, &["export", "--workspace", MANIFEST, "--repo", WEB]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));

    // The start checks of `mcp --listen` pass too: it reaches its listening line.
    let mut server = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(&dir)
        .args(["mcp", "--listen", "127.0.0.1:0", "--workspace", MANIFEST])
        .env("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("meada runs");
    let mut line = String::new();
    let stderr = server.stderr.take().unwrap();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(stderr), &mut line).unwrap();
    let _ = server.kill();
    let _ = server.wait();
    assert!(line.starts_with("listening on "), "{line:?}");

    // Locally nothing changes: in the same mirror, git's refusal stands, so the checkout is
    // taken for a directory outside git and Meada names it `local/<directory>`.
    let local = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(&mirror)
        .arg("status")
        .env("LANG", "en_US.UTF-8")
        .env("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("meada runs");
    let shown = String::from_utf8_lossy(&local.stdout);
    assert!(shown.contains("local/chain-web"), "{shown}");
    assert!(!shown.contains(WEB), "{shown}");
}

const WEB: &str = "example.com/chain/web";

#[test]
fn export_prints_one_repository_over_the_manifest_and_only_index_writes() {
    let dir = chain_manifest("manifest_export");
    let web = "example.com/chain/web";

    // Before `meada index`, nothing is created or built: the error names the fix.
    let (stdout, stderr, code) = run_export(&dir, web);
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
        let (stdout, stderr, code) = run_export(&dir, repo);
        assert_eq!(code, Some(0), "{stderr}");
        assert_golden(&format!("{path}.export.json"), &stdout);
    }
    assert!(run(&dir, "status").2);
    assert!(contents(&indexes) == indexed, "a read wrote an index");

    // An identity the manifest does not list names the ones it serves.
    let (_, stderr, code) = run_export(&dir, "example.com/chain/nope");
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
    let (stdout, stderr, code) = run_export(&dir, web);
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
        let (stdout, stderr, _) = run_export(&dir, repo);
        assert_golden(&format!("{path}.export.json"), &stdout);
        assert_eq!(stderr, "");
    }

    // Provider documents under `serve`.
    let (web, ..) = documented_chain("manifest_blocks_documents");
    let dir = web.parent().unwrap();
    list(dir, &CHAIN, true);
    assert!(run(dir, "index").2);
    for (repo, name) in [CHAIN[0], CHAIN[1]] {
        let export = run_export(dir, repo).0;
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
        let export = run_export(&dir, repo).0;
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

    let (stdout, _, _) = run_export(&dir, "example.com/chain/gateway");
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

    let (stdout, stderr, _) = run_export(&dir, "example.com/chain/web");

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

/// A commit in progress on one index does not stall the probe, nor fail it: the index is there
/// and the server answers once the lock clears. The text still names the locked state.
#[test]
fn status_probe_answers_fast_and_ready_while_a_writer_holds_an_index() {
    let dir = chain_manifest("manifest_status_locked");
    assert!(run(&dir, "index").2);
    let writer =
        rusqlite::Connection::open(dir.join("indexes/example.com/chain/web/index.db")).unwrap();
    writer
        .execute_batch("BEGIN EXCLUSIVE; INSERT INTO communities VALUES ('c', 'm');")
        .unwrap();
    let start = std::time::Instant::now();
    let (status, stderr, ok) = run(&dir, "status");
    let took = start.elapsed();
    eprintln!("probe took {took:?}");
    assert!(ok && stderr.is_empty(), "{stderr}");
    assert!(took < std::time::Duration::from_millis(700), "{took:?}");
    assert!(
        status.contains("example.com/chain/web (../chain-web) locked by a commit in progress"),
        "{status}"
    );
    writer.execute_batch("ROLLBACK").unwrap();
}

/// `[snapshot] node_name` (#298): a configured name is checked against the node name pattern, and
/// each failure names the file, the line and the key.
#[cfg(feature = "snapshot")]
#[test]
fn a_bad_node_name_names_the_file_the_line_and_the_key() {
    let dir = mirrors("manifest_node_name");
    let head = format!(
        "index_dir = \"../indexes\"\n{}",
        crate::snapshot::section("http://127.0.0.1:9", "b-1")
    );
    for name in ["Upper", "a_b", "", &"a".repeat(64)] {
        let toml = format!("{head}node_name = \"{name}\"\n");
        write_manifest(&dir, &toml);
        let line = toml[..toml.find("node_name").unwrap()]
            .matches('\n')
            .count()
            + 1;
        let expected = format!(
            "meada: ops/meada.workspace.toml:{line}: `snapshot.node_name`: must be 1 to 63 \
             lowercase letters, digits, `.` and `-`\n"
        );
        for command in ["index", "status"] {
            let (stdout, stderr, ok) = run(&dir, command);
            assert!(!ok && stdout.is_empty(), "{command}: {name:?}");
            assert_eq!(stderr, expected, "{command}: {name:?}");
        }
    }
}

/// `[workspaces]` (#342) needs the sections it rides on, each error naming both, and its keys
/// are checked: the file, the line and the key are named, and the mirrors root is a path, not a
/// value anyone may read back from an answer.
#[cfg(feature = "workspaces")]
#[test]
fn workspaces_section_errors_name_the_file_the_line_and_the_sections() {
    let dir = mirrors("manifest_workspaces_errors");
    let at = "meada: ops/meada.workspace.toml";
    let head = "index_dir = \"../indexes\"\n";
    let section = "\n[workspaces]\nmirrors = \"/srv/mirrors\"\n";
    let oidc = "\n[oidc]\nissuer = \"https://id.example.com\"\npublic_url = \"https://m.example\"\n\
                identities_claim = \"r\"\n\n[oidc.github]\nhost = \"github.com\"\nlogin_claim = \"l\"\n\
                max_age_seconds = 600\n\n[postgres]\nschema = \"m\"\n\n\
                [catalog]\nallow = [\"github.com/a/*\"]\nmax_repositories = 5\n";
    let ui = "\n[ui]\ndir = \"u\"\npublic_url = \"https://c.example\"\nclient_id = \"c\"\n\
              operations = true\noperator_claim = \"op\"\n";
    let full = format!("{head}{section}{oidc}{ui}");
    let line = full.lines().position(|l| l == "[workspaces]").unwrap() + 1;
    for (toml, expected) in [
        (
            format!("{head}{section}"),
            format!("{at}:3: [workspaces] needs [ui] and [oidc.github] and [catalog]\n"),
        ),
        (
            full.replace("operations = true", "operations = false"),
            format!("{at}:{line}: `workspaces`: [workspaces] needs [ui] operations = true\n"),
        ),
        (
            full.clone(),
            format!(
                "{at}:{line}: `workspaces`: [workspaces] needs [workspace] source = \"store\"\n"
            ),
        ),
        (
            full.replace("/srv/mirrors", "relative/mirrors"),
            format!(
                "{at}:{}: `workspaces.mirrors`: must be an absolute path without `..`, the root \
                 of the mirrors named workspaces use\n",
                line + 1
            ),
        ),
        (
            full.replace("mirrors = \"/srv/mirrors\"", "max_entries = 501"),
            format!("{at}:{line}: `workspaces`: `workspaces.mirrors` is missing\n"),
        ),
        (
            full.replace(
                "mirrors = \"/srv/mirrors\"\n",
                "mirrors = \"/m\"\nmax_workspaces = 2001\n",
            ),
            format!(
                "{at}:{}: `workspaces.max_workspaces`: must be at most 2000\n",
                line + 2
            ),
        ),
    ] {
        write_manifest(&dir, &toml);
        let (stdout, stderr, ok) = run(&dir, "status");
        assert!(!ok && stdout.is_empty(), "{toml}");
        assert_eq!(stderr, expected, "{toml}");
    }
}
