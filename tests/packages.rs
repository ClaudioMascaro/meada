//! Packages (spec #22): every named `package.json` of a workspace repository provides
//! `package::npm::<name>`, and every dependency entry naming such a package links to it.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};

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

/// Bumps the shared version, narrows a range, renames what the kit exports, and adds a manifest
/// that takes a second provider of `@example/a` into the monorepo.
fn edit(p: &Packages) {
    let replace = |repo: &Path, file: &str, from: &str, to: &str| {
        let text = fs::read_to_string(repo.join(file)).unwrap();
        assert!(text.contains(from), "{file} lacks {from}");
        fs::write(repo.join(file), text.replace(from, to)).unwrap();
    };
    replace(&p.shared, "package.json", "1.2.0", "1.3.0");
    replace(&p.web, "package.json", "^1.0.0", "^2.0.0");
    // The kit renames an export and a subpath: the calls through them stop crossing.
    let renamed = "export { greet as hello } from './greet';";
    replace(
        &p.shared,
        "kit/src/index.ts",
        "export { greet } from './greet';",
        renamed,
    );
    replace(&p.shared, "kit/package.json", "\"./format\"", "\"./fmt\"");
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
    // Bumping `shared` to 1.3.0 flips `web`'s `~1.2.0`, and the edit narrowing `^1.0.0` to
    // `^2.0.0` flips the other: both entries now say `range mismatch`.
    assert!(!before[0].contains("range mismatch"));
    assert_eq!(refreshed[0].matches("range mismatch").count(), 2);
}

const WELCOME_GREET: &str = "example.com/pkg/web::src/app.ts::welcome::greet#0";
const GREET: &str = "example.com/pkg/shared::kit/src/greet.ts::greet";

#[test]
fn trace_down_from_a_call_crosses_into_the_package_function_and_onward() {
    let p = packages("packages_trace_down");
    let engine = Engine::open(&p.web).unwrap();

    let down = engine
        .trace(Start::From(WELCOME_GREET), Direction::Down, None)
        .unwrap();

    assert_golden("pkg-web.trace-down.txt", &down);
}

#[test]
fn trace_up_from_a_package_function_reaches_its_callers_in_other_repositories() {
    let p = packages("packages_trace_up");
    let engine = Engine::open(&p.shared).unwrap();

    let up = engine
        .trace(Start::From(GREET), Direction::Up, None)
        .unwrap();

    assert_golden("pkg-shared.trace-up.txt", &up);
}

#[test]
fn a_local_binding_of_the_imported_name_does_not_cross() {
    let p = packages("packages_hidden");
    let json: serde_json::Value = serde_json::from_str(&export(&p.web)).unwrap();

    let sites = |list: &str, field: &str| -> Vec<String> {
        (json[list].as_array().into_iter().flatten())
            .map(|v| v[field].as_str().unwrap().to_string())
            .collect()
    };
    let mut crossing = [sites("links", "consumer"), sites("imports", "site")].concat();
    crossing.retain(|c| c.contains("::src/app.ts::"));
    // `welcome` crosses, through the link and its import resolution.
    let welcome = crossing.iter().filter(|c| *c == WELCOME_GREET).count();
    assert_eq!(welcome, 2, "{crossing:?}");
    for hidden in ["hidden", "caught", "destructured", "shadowed"] {
        let site = format!("example.com/pkg/web::src/app.ts::{hidden}::");
        assert!(
            !crossing.iter().any(|c| c.starts_with(&site)),
            "{hidden}: {crossing:?}"
        );
    }
    // A package outside the workspace has no consumer, link, import or dependency entry.
    for list in ["consumers", "links", "imports", "dependencies"] {
        assert!(!json[list].to_string().contains("express"), "{list}");
    }
}
