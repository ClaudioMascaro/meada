//! Kotlin DSL builds give package contracts and links like the Groovy ones (spec #27).

use std::fs;
use std::path::PathBuf;

use crate::common::{age, assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

fn workspace(case: &str) -> [PathBuf; 2] {
    ["lib", "app"].map(|role| {
        fixture_repo(
            case,
            &format!("kotlin-pkg-{role}"),
            Some(&format!("https://example.com/kotlin/{role}.git")),
        )
    })
}

#[test]
fn kotlin_dsl_builds_export_packages_and_link_literal_dependencies() {
    let [lib, app] = workspace("kotlin_packages_literals");
    let lib_engine = Engine::open(&lib).unwrap();
    let lib_export = lib_engine.export().unwrap();
    let names: Vec<_> = serde_json::from_str::<Value>(&lib_export).unwrap()["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "dev.kt.core:core",
            "dev.kt.http:client",
            "dev.kt:toolbox",
            "dev.kt:util"
        ]
    );
    assert_golden("kotlin-pkg-lib.export.json", &lib_export);
    let engine = Engine::open(&app).unwrap();
    let export = engine.export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let links = json["links"].as_array().unwrap();
    // Each declaration, by its configuration: a contract with several sources loses none of them.
    let declared = |l: &Value| {
        let site = l["consumer"].as_str().unwrap();
        let mut parts = site.split("::").skip(2);
        let field = parts.next().unwrap().to_string();
        (field, l["contract"].as_str().unwrap().to_string())
    };
    let mut found: Vec<_> = (links.iter())
        .map(|l| {
            let (field, contract) = declared(l);
            let provider = l["provider"].as_str().unwrap().split_once("::").unwrap().1;
            let level = l["level"].as_str().unwrap();
            let reason = l["reason"].as_str().unwrap();
            format!("{field} {contract} {level} {provider} | {reason}")
        })
        .collect();
    found.sort();
    let exact = |field: &str, contract: &str, provider: &str, range: &str| {
        let reason = format!("package::maven::{contract} {range} (local 3.1), range unchecked");
        format!("{field} package::maven::{contract} exact {provider} | {reason}")
    };
    let guessed = |provider: &str| {
        let reason = "2 providers fit; computed coordinate; package::maven::dev.kt:* 3.1 (local 3.1), range unchecked";
        format!("implementation package::maven::dev.kt:* ambiguous {provider} | {reason}")
    };
    let mut expected = vec![
        exact("api", "dev.kt.core:core", "core/build.gradle.kts", "3.1"),
        exact("api", "dev.kt:toolbox", "build.gradle.kts", "3.1"),
        exact(
            "compileOnly",
            "dev.kt.http:client",
            "http/client/build.gradle.kts",
            "3.1",
        ),
        exact(
            "implementation",
            "dev.kt:toolbox",
            "build.gradle.kts",
            "3.1",
        ),
        exact(
            "runtimeOnly",
            "dev.kt:util",
            "util/build.gradle.kts",
            "none",
        ),
        exact(
            "testCompileOnly",
            "dev.kt.core:core",
            "core/build.gradle.kts",
            "3.1",
        ),
        exact(
            "testImplementation",
            "dev.kt:toolbox",
            "build.gradle.kts",
            "3.1",
        ),
        exact(
            "testRuntimeOnly",
            "dev.kt.http:client",
            "http/client/build.gradle.kts",
            "3.1",
        ),
        // Every provider whose literal parts fit, and no other.
        guessed("build.gradle.kts"),
        guessed("util/build.gradle.kts"),
    ];
    expected.sort();
    // project(":web") and okhttp declare no coordinate; nor does a Kotlin comment, which nests.
    assert_eq!(found, expected, "{export}");
    assert!(!export.contains("9.9"), "{export}");
    // `api(project(":util"))` in core stays inside the repository although `:util` is a provider.
    let lib_json: Value = serde_json::from_str(&lib_export).unwrap();
    assert!(
        (lib_json["consumers"].as_array().unwrap().iter()).all(|c| !c["site"]
            .as_str()
            .unwrap()
            .contains("core/build.gradle.kts")),
        "{lib_export}"
    );
    assert!(
        (lib_json["links"].as_array().unwrap().iter()).all(|l| l["consumer"]
            .as_str()
            .unwrap()
            .starts_with("example.com/kotlin/app::")),
        "{lib_export}"
    );
    assert_golden("kotlin-pkg-app.export.json", &export);
    assert_golden(
        "kotlin-packages.search.txt",
        &engine.search("", Some("package"), None).unwrap(),
    );
}

fn change_manifests(repos: &[PathBuf; 2]) {
    let [lib, app] = repos;
    let replace = |path: PathBuf, old: &str, new: &str| {
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(old), "{} lacks {old}", path.display());
        fs::write(&path, text.replace(old, new)).unwrap();
    };
    replace(lib.join("settings.gradle.kts"), "\":util\"", "\":utils\"");
    replace(lib.join("build.gradle.kts"), "\"3.1\"", "\"3.2\"");
    replace(
        app.join("build.gradle.kts"),
        "\"dev.kt:util\"",
        "\"dev.kt:utils\"",
    );
    replace(
        app.join("settings.gradle.kts"),
        "rootProject.name = \"app\"",
        "rootProject.name = \"shop\"",
    );
}

#[test]
fn edited_kotlin_dsl_manifests_refresh_like_a_cold_workspace() {
    let warm = workspace("kotlin_packages_refresh");
    age(warm[0].parent().unwrap());
    let exports = |repos: &[PathBuf; 2]| {
        repos
            .each_ref()
            .map(|p| Engine::open(p).unwrap().export().unwrap())
    };
    let before = exports(&warm);
    assert_eq!(before, exports(&warm), "byte-identical repeated exports");
    change_manifests(&warm);
    let refreshed = exports(&warm);
    assert_ne!(before, refreshed);
    let cold = workspace("kotlin_packages_cold");
    change_manifests(&cold);
    assert_eq!(refreshed, exports(&cold));
}
