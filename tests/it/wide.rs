//! A workspace of any number of repositories: twelve small ones chained through `[[peer]]`
//! (past the ten databases SQLite attaches to one connection) open, refresh and answer locally,
//! from a manifest, from a held generation and under a grant.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::git;
use meada::engine::{Direction, Engine, Start};

/// Twelve repositories: one more than the eleven a single connection reads.
pub(crate) const REPOS: usize = 12;

pub(crate) fn identity(n: usize) -> String {
    format!("example.com/wide/r{n}")
}

/// `REPOS` repositories `r0..` as siblings in `<CARGO_TARGET_TMPDIR>/<case>`, each a git
/// repository of its own: `r<n>` serves `GET /api/r<n>` from `hop<n>` and `hop<n>` calls the
/// route of `r<n+1>`, which `r<n>` names as its only peer. Returns the case directory and the
/// manifest entries (identity, directory) in order.
pub(crate) fn repos(case: &str) -> (PathBuf, Vec<(String, String)>) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(case);
    let _ = fs::remove_dir_all(&dir);
    let mut listed = Vec::new();
    for n in 0..REPOS {
        let name = format!("r{n}");
        let repo = dir.join(&name);
        fs::create_dir_all(repo.join("app/api").join(&name)).unwrap();
        fs::create_dir_all(repo.join("src")).unwrap();
        let mut config = format!("[serve]\nhosts = [\"r{n}.example.com\"]\n");
        let mut hop = format!("export async function hop{n}() {{\n  return \"end\";\n}}\n");
        if n + 1 < REPOS {
            config += &format!("\n[[peer]]\nrepo = \"{}\"\n", identity(n + 1));
            hop = format!(
                "export async function hop{n}() {{\n  return fetch(\"https://r{next}.example.com/api/r{next}\");\n}}\n",
                next = n + 1
            );
        }
        fs::write(repo.join("meada.toml"), config).unwrap();
        fs::write(repo.join("src/hop.ts"), hop).unwrap();
        fs::write(
            repo.join("app/api").join(&name).join("route.ts"),
            format!("import {{ hop{n} }} from \"../../../src/hop\";\nexport async function GET() {{\n  return hop{n}();\n}}\n"),
        )
        .unwrap();
        git(&repo, &["-c", "init.defaultBranch=main", "init", "-q"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                &format!("https://{}.git", identity(n)),
            ],
        );
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "wide"]);
        listed.push((identity(n), name));
    }
    (dir, listed)
}

/// What a workspace of `REPOS` repositories must answer from `r0`: the chain's trace reaches
/// the last repository and the first link's provider, `search` finds the symbol of the last.
fn answers_across(engine: &Engine) {
    let trace = (engine.trace(Start::From("hop0"), Direction::Down, None)).unwrap();
    let last = identity(REPOS - 1);
    assert!(trace.contains(&last), "{trace}");
    let found = engine.search(&format!("hop{}", REPOS - 1), Some("symbol"), None);
    assert!(found.unwrap().contains(&last));
    let (_, lines) = engine.status_report().unwrap();
    assert_eq!(lines.len(), REPOS);
}

#[test]
fn a_workspace_past_eleven_repositories_opens_and_links_across_them() {
    let (dir, _) = repos("wide_local");
    let engine = Engine::open(&dir.join("r0")).unwrap();

    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();

    let links = export["links"].as_array().unwrap();
    assert!(
        links
            .iter()
            .any(|l| l["provider"].as_str().unwrap().starts_with(&identity(1))),
        "{links:?}"
    );
    answers_across(&engine);
}

#[test]
fn a_manifest_of_twelve_repositories_answers_read_only() {
    let (dir, listed) = repos("wide_manifest");
    let entries: Vec<_> = listed
        .iter()
        .map(|(i, p)| (i.as_str(), p.as_str()))
        .collect();
    crate::manifest::list(&dir, &entries, false);
    assert!(crate::manifest::run(&dir, "index").2);

    let engine = Engine::open_manifest(&dir.join(crate::manifest::MANIFEST), &identity(0)).unwrap();

    answers_across(&engine);
}

#[cfg(feature = "snapshot")]
#[test]
fn a_held_generation_of_twelve_repositories_answers() {
    let (dir, listed) = repos("wide_generation");
    let entries: Vec<_> = listed
        .iter()
        .map(|(i, p)| (i.as_str(), p.as_str()))
        .collect();
    crate::manifest::list(&dir, &entries, false);
    let (manifest, _) = crate::serving::hand_node_of(&dir, &entries);

    let engine = Engine::open_node(&dir.join(manifest), &identity(0)).unwrap();

    answers_across(&engine);
}
