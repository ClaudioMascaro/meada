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
    let exact: Vec<_> = (links.iter())
        .filter(|l| l["level"] == "exact")
        .map(|l| l["contract"].as_str().unwrap())
        .collect();
    for wanted in [
        "package::maven::dev.kt:toolbox",
        "package::maven::dev.kt.core:core",
        "package::maven::dev.kt.http:client",
        "package::maven::dev.kt:util",
    ] {
        assert!(exact.contains(&wanted), "{wanted}: {export}");
    }
    // project(":web") stays inside the repository; okhttp is external.
    assert!(
        !export.contains("okhttp") && !export.contains("project"),
        "{export}"
    );
    assert!(
        (links.iter().filter(|l| l["level"] == "exact"))
            .all(|l| l["reason"].as_str().unwrap().contains("range unchecked")),
        "{export}"
    );
    let computed: Vec<_> = (links.iter())
        .filter(|l| l["contract"] == "package::maven::dev.kt:*")
        .collect();
    // Every provider whose literal parts fit, and no other.
    let candidates: Vec<_> = (computed.iter())
        .map(|l| l["provider"].as_str().unwrap().split_once("::").unwrap().1)
        .collect();
    assert_eq!(
        candidates,
        ["build.gradle.kts", "util/build.gradle.kts"],
        "{export}"
    );
    assert!(
        computed.iter().all(|l| l["level"] == "ambiguous"),
        "{export}"
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
        "include(\":web\")",
        "include(\":web\", \":api\")",
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
