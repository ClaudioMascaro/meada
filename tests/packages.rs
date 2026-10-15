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
    // `^2.0.0` flips the other: both entries now say `range mismatch`, and so do the links of
    // the two calls into `shared` (`clean` and `squash`).
    assert!(!before[0].contains("range mismatch"));
    assert_eq!(refreshed[0].matches("range mismatch").count(), 4);
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

const CLEAN: &str = "example.com/pkg/web::src/text.ts::clean::normalize#0";
const NORMALIZE: &str = "example.com/pkg/shared::src/text.ts::normalize";

#[test]
fn trace_down_through_build_output_reaches_the_source_and_onward() {
    let p = packages("packages_outdir_down");
    let engine = Engine::open(&p.web).unwrap();

    let down = engine
        .trace(Start::From(CLEAN), Direction::Down, None)
        .unwrap();

    assert_golden("pkg-web.trace-down-outdir.txt", &down);
}

#[test]
fn trace_up_from_a_function_behind_build_output_reaches_both_callers() {
    let p = packages("packages_outdir_up");
    let engine = Engine::open(&p.shared).unwrap();

    let up = engine
        .trace(Start::From(NORMALIZE), Direction::Up, None)
        .unwrap();

    assert_golden("pkg-shared.trace-up-outdir.txt", &up);
}

/// The `(level, how, target)` of every import resolution of `site` in `repo`'s export.
fn imports_of(repo: &Path, site: &str) -> Vec<(String, String, String)> {
    let json: serde_json::Value = serde_json::from_str(&export(repo)).unwrap();
    let text = |i: &serde_json::Value, k: &str| i[k].as_str().unwrap().to_string();
    (json["imports"].as_array().into_iter().flatten())
        .filter(|i| i["site"] == site)
        .map(|i| (text(i, "level"), text(i, "how"), text(i, "target")))
        .collect()
}

#[test]
fn build_output_is_inferred_and_an_unmappable_entry_lists_its_candidates() {
    let p = packages("packages_outdir_levels");
    let at = |caller: &str, callee: &str| {
        format!("example.com/pkg/web::src/text.ts::{caller}::{callee}#0")
    };
    let shared = |file: &str, name: &str| format!("example.com/pkg/shared::{file}::{name}");
    let inferred = |target: String| vec![("inferred".to_string(), "outDir".to_string(), target)];

    // `types` and a pattern subpath point into `dist`, which `rootDir` maps to `src`.
    assert_eq!(
        imports_of(&p.web, &at("clean", "normalize")),
        inferred(shared("src/text.ts", "normalize"))
    );
    assert_eq!(
        imports_of(&p.web, &at("squash", "collapse")),
        inferred(shared("src/utils/collapse.ts", "collapse"))
    );
    let label = "example.com/pkg/api::src/label.js::label::normalize#0";
    assert_eq!(
        imports_of(&p.api, label),
        inferred(shared("src/text.ts", "normalize"))
    );
    // `sealed` has an `outDir` and no `rootDir`: `dist/index.js` would be `index.ts` at the
    // package directory, which is not there. Both files exporting `seal` are listed.
    let sealed = |file: &str| {
        (
            "ambiguous".to_string(),
            "main".to_string(),
            shared(file, "seal"),
        )
    };
    assert_eq!(
        imports_of(&p.web, &at("lock", "seal")),
        vec![sealed("sealed/src/one.ts"), sealed("sealed/src/two.ts")]
    );
    // An ambiguous import is no open item for `meada decide`.
    let items = Engine::open(&p.web).unwrap().open_items().unwrap();
    assert!(items.iter().all(|i| i.path != "src/text.ts"));
}

#[test]
fn a_nested_tsconfig_that_is_invalid_or_leaves_the_repository_maps_nothing() {
    let p = packages("packages_tsconfig_hostile");
    let nested = p.shared.join("sealed/tsconfig.json");
    let lock = "example.com/pkg/web::src/text.ts::lock::seal#0";
    for text in [
        "{ not json",
        &"[".repeat(100_000),
        r#"{"compilerOptions": {"outDir": "../sealed/dist", "rootDir": "../sealed/src"}}"#,
        r#"{"compilerOptions": {"outDir": "/dist", "rootDir": "/sealed/src"}}"#,
    ] {
        fs::write(&nested, text).unwrap();
        let levels: Vec<_> = (imports_of(&p.web, lock).into_iter())
            .map(|(level, _, _)| level)
            .collect();
        assert_eq!(levels, ["ambiguous", "ambiguous"], "{text}");
    }
}

#[test]
fn editing_the_providers_tsconfig_relinks_like_a_cold_export() {
    let edit = |p: &Packages| {
        let file = p.shared.join("tsconfig.json");
        let text = fs::read_to_string(&file).unwrap();
        fs::write(&file, text.replace("\"src\"", "\"lib\"")).unwrap();
    };
    let warm = packages("packages_tsconfig_fresh");
    age(warm.shared.parent().unwrap());
    let before = export(&warm.web);
    assert_eq!(imports_of(&warm.web, CLEAN)[0].0, "inferred");
    edit(&warm);

    let refreshed = export(&warm.web);

    let cold = packages("packages_tsconfig_fresh_cold");
    edit(&cold);
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, export(&cold.web));
    // `dist/index.d.ts` no longer maps to a file: the import lists what exports the name.
    assert_eq!(imports_of(&warm.web, CLEAN)[0].0, "ambiguous");
}
