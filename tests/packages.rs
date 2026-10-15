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
    // The calls through the renamed export and subpath no longer cross: no consumer, link or
    // import names them.
    let fixed = "example.com/pkg/web::src/app.ts::main::fixed#0";
    for (export, crossing) in [(&before[0], true), (&refreshed[0], false)] {
        let json: serde_json::Value = serde_json::from_str(export).unwrap();
        for site in [WELCOME_GREET, fixed] {
            let named = ["consumers", "links", "imports"]
                .iter()
                .any(|list| json[list].to_string().contains(&format!("\"{site}\"")));
            assert_eq!(named, crossing, "{site}");
        }
    }
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

#[test]
fn each_entry_field_resolves_and_only_the_nearest_manifest_counts() {
    let p = packages("packages_entries");
    let json: serde_json::Value = serde_json::from_str(&export(&p.web)).unwrap();
    let imports: Vec<(&str, &str)> = (json["imports"].as_array().unwrap().iter())
        .map(|i| (i["site"].as_str().unwrap(), i["how"].as_str().unwrap()))
        .collect();
    let at = |site: &str| format!("example.com/pkg/web::{site}");
    let how = |site: &str| {
        (imports.iter())
            .find(|(s, _)| *s == at(site))
            .map(|(_, h)| *h)
    };

    assert_eq!(how("src/entries.ts::entries::typed#0"), Some("types"));
    assert_eq!(how("src/entries.ts::entries::esm#0"), Some("module"));
    assert_eq!(how("src/entries.ts::entries::tool#0"), Some("path"));
    assert_eq!(how("src/entries.ts::entries::part#0"), Some("path"));
    // `e2e/package.json` declares the kit in `devDependencies`; `tools/package.json`, the
    // nearest manifest of `tools/run.ts`, does not declare it, though the root one does.
    assert_eq!(how("e2e/smoke.ts::smoke::greet#0"), Some("exports"));
    assert_eq!(how("tools/run.ts::run::greet#0"), None);
    assert!(!json["links"].to_string().contains(&at("tools/run.ts")));
}

#[test]
fn search_finds_a_package_by_name_with_its_consumers() {
    let p = packages("packages_search");
    let engine = Engine::open(&p.web).unwrap();

    let all = engine.search("@example", Some("package"), None).unwrap();
    let kit = engine.search("KIT", Some("package"), None).unwrap();

    assert_golden("pkg.search-package.txt", &all);
    assert_golden("pkg.search-package-kit.txt", &kit);
    assert_eq!(
        engine
            .search("nothing-like-it", Some("package"), None)
            .unwrap(),
        "no matches for `nothing-like-it`\n"
    );
}

#[test]
fn search_places_a_dependency_entry_at_its_manifest_and_field() {
    let p = packages("packages_search_contract");
    let engine = Engine::open(&p.web).unwrap();

    let found = engine.search("shared", Some("contract"), None).unwrap();

    assert_golden("pkg.search-contract-shared.txt", &found);
}

#[test]
fn context_on_an_exported_function_lists_its_consumers_across_repositories() {
    let p = packages("packages_context");
    let engine = Engine::open(&p.shared).unwrap();

    let found = engine.context(GREET, None).unwrap();

    assert_golden("pkg-shared.context-greet.txt", &found);
}

#[test]
fn a_consumer_carries_the_range_flag_of_its_dependency() {
    let p = packages("packages_context_flags");
    let replace = |repo: &Path, from: &str, to: &str| {
        let file = repo.join("package.json");
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains(from), "{file:?} lacks {from}");
        fs::write(&file, text.replace(from, to)).unwrap();
    };
    // `web` asks for a major `kit` that is not out; `api` names it by a tag.
    replace(
        &p.web,
        "\"@example/kit\": \"^2.0.0\"",
        "\"@example/kit\": \"^3.0.0\"",
    );
    replace(
        &p.api,
        "\"@example/kit\": \"2.x\"",
        "\"@example/kit\": \"latest\"",
    );

    let found = Engine::open(&p.shared)
        .unwrap()
        .context(GREET, None)
        .unwrap();

    assert_golden("pkg-shared.context-greet-flagged.txt", &found);
}

#[test]
fn context_on_a_package_lists_its_manifest_and_dependents() {
    let p = packages("packages_context_package");
    let engine = Engine::open(&p.web).unwrap();

    let found = engine.context("package::npm::@example/kit", None).unwrap();
    let traced = engine
        .trace(
            Start::From("package::npm::@example/kit"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("pkg.context-package-kit.txt", &found);
    assert_golden("pkg.trace-package-kit.txt", &traced);
}

#[test]
fn consumers_in_the_same_repository_show_through_workspace_ranges() {
    let p = packages("packages_context_mono");
    let engine = Engine::open(&p.mono).unwrap();

    let found = engine
        .context("example.com/pkg/mono::packages/a/src/index.js::add", None)
        .unwrap();

    // `b` names `a` through `workspace:*` (`range unchecked`, exact); `c` names it with
    // `^1.0.0`, which accepts 1.0.0, while `fork` also provides `@example/a` (`ambiguous`).
    assert_golden("pkg-mono.context-add.txt", &found);
}
