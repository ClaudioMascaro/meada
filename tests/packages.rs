//! Packages (spec #22): every named `package.json` of a workspace repository provides
//! `package::npm::<name>`, and every dependency entry naming such a package links to it.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, fixture_repo};
use meada::engine::Engine;

/// The package fixtures as siblings in one case directory. `web`, `api` and `mono` name
/// `shared` as their peer, `fork` names `mono`, so a query from any of them sees all five.
struct Packages {
    shared: PathBuf,
    web: PathBuf,
    api: PathBuf,
    mono: PathBuf,
    fork: PathBuf,
}

fn packages(case: &str) -> Packages {
    let repo = |name: &str| {
        let origin = format!("https://example.com/pkg/{name}.git");
        fixture_repo(case, &format!("pkg-{name}"), Some(&origin))
    };
    Packages {
        shared: repo("shared"),
        web: repo("web"),
        api: repo("api"),
        mono: repo("mono"),
        fork: repo("fork"),
    }
}

fn export(repo: &Path) -> String {
    Engine::open(repo).unwrap().export().unwrap()
}

#[test]
fn a_dependency_on_a_workspace_package_links_to_its_manifest() {
    let p = packages("packages_links");

    assert_golden("pkg-web.export.json", &export(&p.web));
    assert_golden("pkg-api.export.json", &export(&p.api));
    assert_golden("pkg-shared.export.json", &export(&p.shared));
}

#[test]
fn workspace_ranges_stay_in_their_repository_and_the_same_name_elsewhere_is_ambiguous() {
    let p = packages("packages_mono");

    // `b` depends on `a` through `workspace:*`: exact, though `fork` also provides `@example/a`.
    // `c` names `a` with a plain range: both providers fit. `ignored/` is in `.gitignore`.
    assert_golden("pkg-mono.export.json", &export(&p.mono));
    assert_golden("pkg-fork.export.json", &export(&p.fork));
}

#[test]
fn ambiguous_package_links_are_not_open_items() {
    let p = packages("packages_items");
    let engine = Engine::open(&p.mono).unwrap();

    assert!(engine.open_items().unwrap().is_empty());
    assert_eq!(
        engine.search("", Some("ambiguous"), None).unwrap(),
        "no matches for ``\n"
    );
}

/// Bumps the shared version, narrows a range, and adds a manifest that takes a second
/// provider of `@example/a` into the monorepo.
fn edit(p: &Packages) {
    let replace = |repo: &Path, file: &str, from: &str, to: &str| {
        let text = fs::read_to_string(repo.join(file)).unwrap();
        assert!(text.contains(from), "{file} lacks {from}");
        fs::write(repo.join(file), text.replace(from, to)).unwrap();
    };
    replace(&p.shared, "package.json", "1.2.0", "1.3.0");
    replace(&p.web, "package.json", "^1.0.0", "^2.0.0");
    fs::create_dir_all(p.mono.join("packages/d")).unwrap();
    fs::write(
        p.mono.join("packages/d/package.json"),
        r#"{ "name": "@example/a", "version": "1.1.0" }"#,
    )
    .unwrap();
}

#[test]
fn an_edited_package_json_relinks_like_a_cold_export() {
    let warm = packages("packages_fresh");
    age(warm.shared.parent().unwrap());
    let repos = |p: &Packages| [&p.web, &p.shared, &p.mono, &p.fork].map(|r| export(r));
    let before = repos(&warm);
    edit(&warm);

    let refreshed = repos(&warm);

    let cold = packages("packages_fresh_cold");
    edit(&cold);
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, repos(&cold));
}
