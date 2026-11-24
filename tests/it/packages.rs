//! Packages (spec #22): every named `package.json` of a workspace repository provides
//! `package::npm::<name>`, and every dependency entry naming such a package links to it.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, fixture_repo, without_processes};
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

    assert_golden("pkg-web.export.json", &without_processes(&export(&p.web)));
    assert_golden("pkg-api.export.json", &without_processes(&export(&p.api)));
    assert_golden(
        "pkg-shared.export.json",
        &without_processes(&export(&p.shared)),
    );
}

#[test]
fn workspace_ranges_stay_in_their_repository_and_the_same_name_elsewhere_is_ambiguous() {
    let p = packages("packages_mono");

    // `b` depends on `a` through `workspace:*`: exact, though `fork` also provides `@example/a`.
    // `c` names `a` with a plain range: both providers fit. `ignored/` is in `.gitignore`.
    assert_golden("pkg-mono.export.json", &without_processes(&export(&p.mono)));
    assert_golden("pkg-fork.export.json", &without_processes(&export(&p.fork)));
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
    // The processes repeat the reasons of the links they cross: count the entries and links.
    let mismatches = |export: &str| {
        let mut json: serde_json::Value = serde_json::from_str(export).unwrap();
        json.as_object_mut().unwrap().remove("processes");
        json.to_string().matches("range mismatch").count()
    };
    assert_eq!(mismatches(&before[0]), 0);
    assert_eq!(mismatches(&refreshed[0]), 4);
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
fn a_field_that_leads_to_no_source_does_not_stop_the_search_and_no_field_reads_as_index() {
    let p = packages("packages_entry_fallback");
    let at = |callee: &str| format!("example.com/pkg/web::src/entries.ts::entries::{callee}#0");
    let shared = |file: &str, name: &str| format!("example.com/pkg/shared::{file}::{name}");
    let imports = |callee: &str| imports_of(&p.web, &at(callee));
    let row =
        |level: &str, how: &str, target: String| vec![(level.to_string(), how.to_string(), target)];

    // `types` names a declaration file that is no source: `main` is tried next.
    assert_eq!(
        imports("dts"),
        row("exact", "main", shared("dts/index.js", "dts"))
    );
    // None of `exports`, `types`, `module` and `main`: Node's default, `./index`.
    assert_eq!(
        imports("plain"),
        row("exact", "path", shared("plain/index.ts", "plain"))
    );
    // No `index` file either: the files of the package exporting the name are listed.
    assert_eq!(
        imports("thing"),
        row("ambiguous", "path", shared("bare/src/thing.ts", "thing"))
    );
}

#[test]
fn the_first_field_that_leads_to_a_file_wins_and_a_nested_package_is_not_the_outer_ones() {
    let p = packages("packages_entry_owner");
    let at = |callee: &str| format!("example.com/pkg/web::src/nest.ts::nest::{callee}#0");
    let shared = |file: &str, name: &str| format!("example.com/pkg/shared::{file}::{name}");

    // `types` maps through the `outDir` (inferred) to `index.ts`; the later `main`, a source
    // that would be exact, is not tried.
    assert_eq!(
        imports_of(&p.web, &at("prec")),
        [(
            "inferred".to_string(),
            "outDir".to_string(),
            shared("prec/index.ts", "prec")
        )]
    );
    // With no entry field Node reads `index.<ext>`, never `index/index.ts`; and that file, like
    // a subpath file, belongs to a nested package, not to `@example/nest`.
    assert!(imports_of(&p.web, &at("inner")).is_empty());
    assert!(imports_of(&p.web, &at("deep")).is_empty());
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

/// The rows a query asks of the index of `repo`, one line each with its columns spaced.
fn rows_of(repo: &Path, sql: &str) -> Vec<String> {
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    let mut rows = db.prepare(sql).unwrap();
    let count = rows.column_count();
    rows.query_map([], |row| {
        let cols = (0..count).map(|n| {
            row.get::<_, Option<String>>(n)
                .map(|v| v.unwrap_or("-".into()))
        });
        cols.collect::<Result<Vec<_>, _>>().map(|c| c.join(" "))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[test]
fn an_exported_constant_or_default_expression_is_an_import_target() {
    let repo = fixture_repo(
        "packages_exported",
        "pkg-exported",
        Some("https://example.com/pkg/exported.git"),
    );
    let at = |callee: &str| {
        format!("example.com/pkg/exported::packages/b/src/use.ts::useAll::{callee}#0")
    };
    let sym = |file: &str, name: &str| format!("example.com/pkg/exported::packages/{file}::{name}");
    let exact = |target: String| vec![("exact".to_string(), "main".to_string(), target)];

    let plugin = exact(sym("a/src/plugin.ts", "aPlugin"));
    assert_eq!(imports_of(&repo, &at("a")), plugin);
    assert_eq!(imports_of(&repo, &at("aPlugin")), plugin);
    assert_eq!(
        imports_of(&repo, &at("handler")),
        exact(sym("c/src/index.ts", "default"))
    );

    let json: serde_json::Value = serde_json::from_str(&export(&repo)).unwrap();
    let ids: Vec<&str> = (json["symbols"].as_array().unwrap().iter())
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    for (file, name) in [
        ("a/src/plugin.ts", "aPlugin"),
        ("a/src/plugin.ts", "APlugin"),
        ("a/src/status.ts", "S"),
        ("a/src/status.ts", "EXTRA"),
        ("c/src/index.ts", "default"),
    ] {
        assert!(ids.contains(&sym(file, name).as_str()), "{file} {name}");
    }
    let exports = rows_of(
        &repo,
        "SELECT path, name, file, target FROM exports ORDER BY path, name",
    );
    for row in [
        "packages/a/src/index.ts S packages/a/src/status.ts S",
        "packages/c/src/index.ts default packages/c/src/index.ts default",
    ] {
        assert!(exports.iter().any(|r| r == row), "{row} in {exports:#?}");
    }
    // A URL built from an enum member reads as before: no contract.
    let fetches: Vec<&serde_json::Value> = (json["consumers"].as_array().unwrap().iter())
        .filter(|c| (c["site"].as_str()).is_some_and(|s| s.contains("use.ts::useAll::fetch")))
        .collect();
    assert_eq!(fetches.len(), 1, "{fetches:#?}");
    assert!(fetches[0]["contract"].is_null(), "{fetches:#?}");
    // The `S` row alone does not bring `status.ts` into the package's surface.
    let shapes = rows_of(
        &repo,
        "SELECT path, method FROM contracts WHERE role = 'shape' ORDER BY path, method",
    );
    assert!(
        shapes
            .iter()
            .all(|s| !s.starts_with("packages/a/src/status.ts")),
        "{shapes:#?}"
    );

    // A refresh resolves `plugin.ts` from its stored facts and writes the same rows.
    let symbols = rows_of(
        &repo,
        "SELECT id, start || ' ' || end, hash FROM symbols ORDER BY id",
    );
    let imports = (
        json["imports"].as_array().unwrap().clone(),
        json["symbols"].clone(),
    );
    let use_ts = repo.join("packages/b/src/use.ts");
    let mut text = fs::read_to_string(&use_ts).unwrap();
    text.push('\n');
    fs::write(&use_ts, text).unwrap();
    Engine::open(&repo).unwrap().refresh().unwrap();
    assert_eq!(
        rows_of(
            &repo,
            "SELECT id, start || ' ' || end, hash FROM symbols ORDER BY id"
        ),
        symbols
    );
    let again: serde_json::Value = serde_json::from_str(&export(&repo)).unwrap();
    assert_eq!(
        (
            again["imports"].as_array().unwrap().clone(),
            again["symbols"].clone()
        ),
        imports
    );
}

/// The export's `imports` rows at `file` that name `specifier`, whole and without their `site`,
/// sorted by their JSON text.
fn rows_at(repo: &Path, file: &str, specifier: &str) -> Vec<serde_json::Value> {
    let json: serde_json::Value = serde_json::from_str(&export(repo)).unwrap();
    let mut rows: Vec<serde_json::Value> = (json["imports"].as_array().into_iter().flatten())
        .filter(|i| i["site"].as_str().and_then(|s| s.split("::").nth(1)) == Some(file))
        .filter(|i| i["specifier"] == specifier)
        .cloned()
        .map(|mut i| {
            i.as_object_mut().unwrap().remove("site");
            i
        })
        .collect();
    rows.sort_by_key(|r| r.to_string());
    rows
}

#[test]
fn a_backstage_backend_add_of_a_dynamic_import_links_its_default() {
    let repo = fixture_repo(
        "packages_backend_add",
        "pkg-backend-add",
        Some("https://example.com/pkg/backend-add.git"),
    );
    let sym =
        |file: &str, name: &str| format!("example.com/pkg/backend-add::plugins/{file}::{name}");
    let dynamic = |s: &str| rows_at(&repo, "packages/backend/src/index.ts", s);
    let fixed = |s: &str| rows_at(&repo, "packages/static/src/index.ts", s);

    for s in [
        "@scope/plugin-a",
        "@scope/plugin-b",
        "@scope/plugin-c",
        "@scope/plugin-d",
        "@scope/plugin-e",
        "@scope/nodefault",
        "@backstage/plugin-external",
    ] {
        assert_eq!(dynamic(s), fixed(s), "{s}");
        assert!(dynamic(s).iter().all(|r| r["name"] == "default"), "{s}");
    }
    // The `dist/` entry maps to no source: the candidates the static twin gets.
    assert!(!dynamic("@scope/plugin-e").is_empty());

    let what = |s: &str| -> Vec<(String, String, String)> {
        let text = |r: &serde_json::Value, k: &str| r[k].as_str().unwrap().to_string();
        (dynamic(s).iter())
            .map(|r| (text(r, "level"), text(r, "how"), text(r, "target")))
            .collect()
    };
    let exact = |how: &str, target: String| vec![("exact".to_string(), how.to_string(), target)];
    assert_eq!(
        what("@scope/plugin-a"),
        exact("main", sym("plugin-a/src/plugin.ts", "aPlugin"))
    );
    assert_eq!(
        what("@scope/plugin-b"),
        exact("types", sym("plugin-b/src/plugin.ts", "bPlugin"))
    );
    assert_eq!(
        what("@scope/plugin-c"),
        exact("main", sym("plugin-c/src/index.ts", "default"))
    );
    assert_eq!(
        what("@scope/plugin-d"),
        exact("main", sym("plugin-d/src/index.ts", "root"))
    );
    // A relative module is no cross-package edge, and an external one has no `imports` row.
    assert!(dynamic("./local").is_empty());
    assert!(dynamic("@backstage/plugin-external").is_empty());
}

#[test]
fn an_add_of_a_dynamic_import_on_another_receiver_links_nothing() {
    let repo = fixture_repo(
        "packages_backend_add_no_link",
        "pkg-backend-add",
        Some("https://example.com/pkg/backend-add.git"),
    );
    let json: serde_json::Value = serde_json::from_str(&export(&repo)).unwrap();
    let sites: Vec<&str> = (json["imports"].as_array().unwrap().iter())
        .map(|i| i["site"].as_str().unwrap())
        .collect();
    // The one backend that does link is in the same package: the case is not vacuous.
    assert!(
        sites
            .iter()
            .any(|s| s.contains("packages/backend/src/index.ts::"))
    );
    let mut leaks = Vec::new();
    for file in [
        "wired.ts",
        "other.ts",
        "reassigned.ts",
        "chained.ts",
        "aliased.ts",
        "template.ts",
        "then.ts",
        "set.ts",
        "wrapped.ts",
        "not-one-import.ts",
    ] {
        let prefix = format!("example.com/pkg/backend-add::packages/backend/src/{file}::");
        leaks.extend((sites.iter().filter(|s| s.starts_with(&prefix))).map(|s| s.to_string()));
    }
    assert!(leaks.is_empty(), "{leaks:#?}");
}

#[test]
fn a_reexported_create_backend_links_like_the_original() {
    let repo = fixture_repo(
        "packages_backend_add_reexport",
        "pkg-backend-add",
        Some("https://example.com/pkg/backend-add.git"),
    );
    let at = |file: &str| {
        rows_at(
            &repo,
            &format!("packages/backend/src/{file}"),
            "@scope/plugin-a",
        )
    };
    assert_eq!(at("reexported.ts"), at("index.ts"));
    assert_eq!(at("index.ts").len(), 1);
}

#[test]
fn a_warm_index_keeps_the_rows_of_an_unchanged_backend() {
    let url = Some("https://example.com/pkg/backend-add.git");
    let imports = |repo: &Path| -> Vec<serde_json::Value> {
        let json: serde_json::Value = serde_json::from_str(&export(repo)).unwrap();
        json["imports"].as_array().unwrap().clone()
    };
    // Another file of the backend's package changes, so the package is extracted again while
    // `index.ts` keeps its stored facts, which hold no argument.
    let touch = |repo: &Path| {
        let set = repo.join("packages/backend/src/set.ts");
        let text = fs::read_to_string(&set).unwrap();
        fs::write(&set, format!("{text}\n")).unwrap();
    };
    let warm = fixture_repo("packages_backend_add_warm", "pkg-backend-add", url);
    age(&warm);
    let before = imports(&warm);
    touch(&warm);
    let after = imports(&warm);

    let cold = fixture_repo("packages_backend_add_cold", "pkg-backend-add", url);
    touch(&cold);

    assert!((after.iter()).any(|i| {
        i["site"]
            .as_str()
            .unwrap()
            .contains("backend/src/index.ts::")
            && i["specifier"] == "@scope/plugin-a"
    }));
    assert_eq!(after, before);
    assert_eq!(after, imports(&cold));
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
    // The nested `@example/sealed` exports a `normalize` too: it is not a candidate.
    assert_eq!(
        imports_of(&warm.web, CLEAN),
        [(
            "ambiguous".to_string(),
            "exports".to_string(),
            NORMALIZE.to_string()
        )]
    );
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

#[test]
fn exported_declarations_are_searchable_and_have_context() {
    let repo = fixture_repo(
        "packages_exported_search",
        "pkg-exported",
        Some("https://example.com/pkg/exported.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    let found = engine.search("aPlugin", Some("symbol"), None).unwrap();
    assert!(
        (found.lines()).any(|l| l == "example.com/pkg/exported packages/a/src/plugin.ts:1 aPlugin"),
        "{found}"
    );
    let context = engine
        .context(
            "example.com/pkg/exported::packages/a/src/plugin.ts::aPlugin",
            None,
        )
        .unwrap();
    let consumers = context.split_once("consumers:\n").map(|(_, c)| c);
    assert!(
        consumers.is_some_and(|c| (c.lines())
            .any(|l| l.starts_with("example.com/pkg/exported packages/b/src/use.ts:3 "))),
        "{context}"
    );
    assert!(context.lines().any(|l| l == "callees: none"), "{context}");
    // The same constant in two packages: a bare name fits both, and they are two copies.
    let limit = engine.context("LIMIT", None).unwrap();
    assert!(
        limit.starts_with("`LIMIT` fits 2 symbols; pass one of:\n"),
        "{limit}"
    );
    let copies = engine.search("LIMIT", Some("duplicate"), None).unwrap();
    assert!(copies.starts_with("2 copies\n"), "{copies}");
}

#[test]
fn declaration_files_give_rows_and_a_committed_copy_is_a_duplicate() {
    let repo = fixture_repo(
        "packages_exported_dts",
        "pkg-exported",
        Some("https://example.com/pkg/exported.git"),
    );
    let engine = Engine::open(&repo).unwrap();

    let json: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let ids: Vec<&str> = (json["symbols"].as_array().unwrap().iter())
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    let shape = "example.com/pkg/exported::packages/a/dist/shape.d.ts::Shape";
    assert!(ids.contains(&shape), "{ids:#?}");
    // `dist/shape.d.ts` holds the same interface as `src/shape.ts`: one group of two.
    let copies = engine.search("Shape", Some("duplicate"), None).unwrap();
    assert!(copies.starts_with("2 copies\n"), "{copies}");
    for path in ["packages/a/dist/shape.d.ts", "packages/a/src/shape.ts"] {
        assert!(copies.contains(path), "{path} in {copies}");
    }
}

#[test]
fn an_exported_value_and_type_of_one_name_keep_their_zod_shapes() {
    let repo = fixture_repo(
        "packages_exported_zod",
        "pkg-exported",
        Some("https://example.com/pkg/exported.git"),
    );
    Engine::open(&repo).unwrap().export().unwrap();

    // The type comes first in `schema.ts`; the one row is the value's.
    let symbols = rows_of(
        &repo,
        "SELECT id, start || ' ' || end FROM symbols WHERE path = 'packages/a/src/schema.ts'",
    );
    let src = fs::read_to_string(repo.join("packages/a/src/schema.ts")).unwrap();
    let at = src.find("Foo = z.object").unwrap();
    let end = src.rfind(')').unwrap() + 1;
    assert_eq!(
        symbols,
        [format!(
            "example.com/pkg/exported::packages/a/src/schema.ts::Foo {at} {end}"
        )]
    );
    // What meada 1.5.0 keeps for this fixture: the `Foo` row reaches `schema.ts` as before.
    let shapes = rows_of(
        &repo,
        "SELECT method, via, shape FROM contracts WHERE role = 'shape' \
         AND path = 'packages/a/src/schema.ts' ORDER BY method, via",
    );
    assert_eq!(
        shapes,
        [
            r#"Foo infer {"source":{"zod":"packages/a/src/schema.ts"},"fields":["id","size"],"declared":["id","size"]}"#,
            r#"Foo type {"source":{"reads":"packages/a/src/schema.ts"},"fields":["id","size"],"declared":["id","size"]}"#,
            r#"Foo zod {"source":{"zod":"packages/a/src/schema.ts"},"fields":["id","size"],"scalars":["id","size"]}"#,
        ]
    );
}
