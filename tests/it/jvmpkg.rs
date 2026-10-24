//! JVM artifacts link through literal Maven and Gradle manifests, without invoking a build.

use std::fs;
use std::path::PathBuf;

use crate::common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

fn workspace(case: &str) -> [PathBuf; 4] {
    ["maven", "gradle", "client", "gradle-client"].map(|role| {
        fixture_repo(
            case,
            &format!("jvm-{role}"),
            Some(&format!("https://example.com/jvm/{role}.git")),
        )
    })
}

#[test]
fn literal_maven_and_gradle_artifacts_link_to_workspace_dependencies() {
    let [maven, gradle, client, gradle_client] = workspace("jvm_packages_literals");
    let engine = Engine::open(&client).unwrap();
    let export = engine.export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let links = json["links"].as_array().unwrap();
    assert!(
        links.iter().any(|l| {
            l["contract"] == "package::maven::example:models" && l["level"] == "exact"
        }),
        "{export}"
    );
    assert_eq!(links.len(), 2, "{export}");
    assert!(
        links
            .iter()
            .any(|l| l["contract"] == "package::maven::example:kit")
    );
    let gradle_export = Engine::open(&gradle).unwrap().export().unwrap();
    let gradle_json: Value = serde_json::from_str(&gradle_export).unwrap();
    let names: Vec<_> = gradle_json["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["example:kit", "example:nested", "example:tools"]);
    assert_golden("jvm-gradle.export.json", &gradle_export);
    assert_golden(
        "jvm-gradle-client.export.json",
        &Engine::open(&gradle_client).unwrap().export().unwrap(),
    );
    assert!(
        links
            .iter()
            .all(|l| l["reason"].as_str().unwrap().contains("range unchecked"))
    );
    assert_golden("jvm-client.export.json", &export);
    assert_golden(
        "jvm-maven.export.json",
        &Engine::open(&maven).unwrap().export().unwrap(),
    );
    assert_golden(
        "jvm-packages.search.txt",
        &engine.search("", Some("package"), None).unwrap(),
    );
}

#[test]
fn computed_coordinates_keep_literal_parts_and_stay_ambiguous_with_one_provider() {
    let [_, _, client, gradle_client] = workspace("jvm_packages_computed");
    fs::copy(client.join("computed.xml"), client.join("pom.xml")).unwrap();
    fs::copy(
        gradle_client.join("computed.gradle"),
        gradle_client.join("build.gradle"),
    )
    .unwrap();
    for (path, golden, version) in [
        (&client, "jvm-client.computed.json", "${version}"),
        (
            &gradle_client,
            "jvm-gradle-client.computed.json",
            "$version",
        ),
    ] {
        let export = Engine::open(path).unwrap().export().unwrap();
        let json: Value = serde_json::from_str(&export).unwrap();
        let links = json["links"].as_array().unwrap();
        assert!(!links.is_empty(), "{export}");
        // A computed version alone leaves a literal coordinate: exact, its range as written.
        let literal: Vec<_> = (links.iter())
            .filter(|l| l["contract"] == "package::maven::example:models")
            .collect();
        assert_eq!(literal.len(), 1, "{export}");
        assert_eq!(literal[0]["level"], "exact", "{export}");
        assert_eq!(
            literal[0]["reason"],
            format!("package::maven::example:models {version} (local 1.2.0), range unchecked")
        );
        let computed: Vec<_> = (links.iter())
            .filter(|l| l["contract"] != "package::maven::example:models")
            .collect();
        assert!(
            computed.iter().all(|l| l["level"] == "ambiguous"),
            "{export}"
        );
        for pattern in ["example:model*", "*:tools"] {
            let wanted = format!("package::maven::{pattern}");
            let matching: Vec<_> = links.iter().filter(|l| l["contract"] == wanted).collect();
            assert_eq!(matching.len(), 1, "{wanted}: {export}");
        }
        assert!(
            links
                .iter()
                .filter(|l| l["contract"] == "package::maven::example:*")
                .count()
                > 1,
            "{export}"
        );
        // A consumer's own manifest is never one of its candidates.
        for l in links {
            let consumer = l["consumer"].as_str().unwrap();
            let provider = l["provider"].as_str().unwrap();
            assert!(
                !consumer.starts_with(&format!("{provider}::")),
                "{consumer} -> {provider}"
            );
        }
        assert_golden(golden, &export);
    }
}

#[test]
fn comments_strings_and_unrelated_blocks_never_supply_gradle_coordinates() {
    let [_, gradle, _, gradle_client] = workspace("jvm_packages_scanner");
    let path = gradle_client.join("build.gradle");
    let mut source = fs::read_to_string(&path).unwrap();
    source.push_str(
        r#"
// group = "decoy"
/* group = "decoy"
dependencies { api("example:billing:99") }
*/
val quoted = """
group = "decoy"
dependencies { api("example:billing:99") }
"""
tasks.register("fake") {
    group = "decoy"
    dependencies { api("example:billing:99") }
}
"#,
    );
    fs::write(&path, source).unwrap();
    let export = Engine::open(&gradle_client).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    assert_eq!(json["packages"][0]["name"], "example:worker", "{export}");
    assert_eq!(json["links"].as_array().unwrap().len(), 2, "{export}");
    // Settings still name the projects when comments and literal strings mention fake ones.
    fs::write(gradle.join("settings.gradle.kts"),"rootProject.name = \"kit\"\n/* include(\":fake\") */\nval text = \"include(\\\":fake\\\")\"\ninclude(\n\":tools\",\n\":extras:nested\"\n)\n").unwrap();
    let export = Engine::open(&gradle).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    assert_eq!(json["packages"].as_array().unwrap().len(), 3, "{export}");
}

#[test]
fn a_colon_inside_a_computed_expression_does_not_discard_literal_coordinate_parts() {
    let [_, _, _, client] = workspace("jvm_packages_expression");
    fs::write(
        client.join("build.gradle"),
        r#"group = 'example'
dependencies { api("example:model${suffix ?: 's:v'}:1") }
"#,
    )
    .unwrap();
    let export = Engine::open(&client).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{export}");
    assert_eq!(links[0]["contract"], "package::maven::example:model*");
    assert_eq!(
        links[0]["provider"],
        "example.com/jvm/maven::models/pom.xml"
    );
    assert_eq!(links[0]["level"], "ambiguous");
}

#[test]
fn hostile_build_files_are_empty_or_unknown_and_indexing_continues() {
    let [_, _, client, gradle_client] = workspace("jvm_packages_hostile");
    let project = "<project><groupId>example</groupId><artifactId>web</artifactId>";
    let cases=[
        format!("{project}{}data{}</project>","<x>".repeat(70),"</x>".repeat(70)),
        format!("{project}{}</project>","<x/>".repeat(70_000)),
        format!("{project}</project>{}"," ".repeat(1_100_000)),
        "<!DOCTYPE project [<!ENTITY data SYSTEM 'file:///etc/passwd'>]><project><groupId>example</groupId><artifactId>&data;</artifactId></project>".into(),
        "<project xmlns='urn:unrelated'><groupId>example</groupId><artifactId>web</artifactId></project>".into(),
        "<project><groupId>example</groupId><artifactId>*</artifactId></project>".into(),
        "<project><groupId>example</groupId><artifactId>broken".into(),
        "<project><groupId>example</groupId><groupId>other</groupId><artifactId>web</artifactId></project>".into(),
        format!("<project><groupId>example</groupId><artifactId>{}</artifactId></project>","a".repeat(230)),
    ];
    for (n, source) in cases.iter().enumerate() {
        fs::write(client.join("pom.xml"), source).unwrap();
        let export = Engine::open(&client).unwrap().export().unwrap();
        let json: Value = serde_json::from_str(&export).unwrap();
        assert_eq!(
            json["providers"].as_array().unwrap().len(),
            0,
            "case {n}: {export}"
        );
    }
    // An unsupported escape cannot silently turn a different string into `models`.
    fs::write(
        gradle_client.join("build.gradle"),
        r#"group = 'example'
dependencies { implementation 'example:model\s:1' }
"#,
    )
    .unwrap();
    let export = Engine::open(&gradle_client).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    assert!(json["links"].as_array().unwrap().is_empty(), "{export}");
    fs::write(
        gradle_client.join("build.gradle"),
        format!("group = 'example'\n/*{}*/", "x".repeat(1_100_000)),
    )
    .unwrap();
    let export = Engine::open(&gradle_client).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    assert!(json["providers"].as_array().unwrap().is_empty(), "{export}");
}

fn change_manifests(repos: &[PathBuf; 4]) {
    let [maven, gradle, client, gradle_client] = repos;
    let replace = |path: PathBuf, old: &str, new: &str| {
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(old), "{} lacks {old}", path.display());
        fs::write(&path, text.replace(old, new)).unwrap();
    };
    replace(
        maven.join("models/pom.xml"),
        "<artifactId>models</artifactId>",
        "<artifactId>schemas</artifactId>",
    );
    replace(
        client.join("pom.xml"),
        "<artifactId>models</artifactId>",
        "<artifactId>schemas</artifactId>",
    );
    replace(gradle.join("settings.gradle.kts"), "\"kit\"", "\"toolkit\"");
    replace(
        client.join("pom.xml"),
        "<artifactId>kit</artifactId>",
        "<artifactId>toolkit</artifactId>",
    );
    replace(
        gradle_client.join("build.gradle"),
        "example:tools:2.0",
        "example:nested:3.0",
    );
    fs::remove_file(gradle.join("tools/build.gradle")).unwrap();
    fs::create_dir_all(maven.join("added")).unwrap();
    fs::write(
        maven.join("added/pom.xml"),
        "<project><groupId>example</groupId><artifactId>added</artifactId></project>",
    )
    .unwrap();
}

#[test]
fn edited_added_and_deleted_jvm_manifests_refresh_like_a_cold_workspace() {
    let warm = workspace("jvm_packages_refresh");
    crate::common::age(warm[0].parent().unwrap());
    let exports = |repos: &[PathBuf; 4]| {
        repos
            .each_ref()
            .map(|p| Engine::open(p).unwrap().export().unwrap())
    };
    let before = exports(&warm);
    assert_eq!(before, exports(&warm), "byte-identical repeated exports");
    change_manifests(&warm);
    let refreshed = exports(&warm);
    assert_ne!(before, refreshed);
    let cold = workspace("jvm_packages_cold");
    change_manifests(&cold);
    assert_eq!(refreshed, exports(&cold));
}

#[test]
fn a_computed_or_conditional_reassignment_never_leaves_a_literal_gradle_provider() {
    let [_, _, _, client] = workspace("jvm_packages_rebound");
    for assignment in [
        "group += suffix",
        "if (enabled) { group = suppliedGroup }",
        "if (enabled) group = suppliedGroup",
    ] {
        fs::write(
            client.join("build.gradle"),
            format!("group = 'example'\n{assignment}\n"),
        )
        .unwrap();
        let export = Engine::open(&client).unwrap().export().unwrap();
        let json: Value = serde_json::from_str(&export).unwrap();
        assert!(
            json["packages"].as_array().is_none_or(|p| p.is_empty()),
            "{assignment}: {export}"
        );
    }
    fs::write(client.join("build.gradle"), "group = 'example'\n").unwrap();
    fs::write(
        client.join("settings.gradle"),
        "rootProject.name = 'worker'\nif (enabled) { rootProject.name = computedName }\n",
    )
    .unwrap();
    let export = Engine::open(&client).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    assert!(
        json["packages"].as_array().is_none_or(|p| p.is_empty()),
        "{export}"
    );
}

#[test]
fn a_foreign_xml_dependency_is_not_a_maven_dependency() {
    let [_, _, client, _] = workspace("jvm_packages_namespace");
    fs::write(client.join("pom.xml"),r#"<project xmlns="http://maven.apache.org/POM/4.0.0" xmlns:x="urn:unrelated">
<groupId>example</groupId><artifactId>web</artifactId>
<dependencies>
  <dependency><groupId>example</groupId><artifactId>models</artifactId><version>1</version></dependency>
  <x:dependency><x:groupId>example</x:groupId><x:artifactId>billing</x:artifactId><x:version>1</x:version></x:dependency>
</dependencies></project>"#).unwrap();
    let export = Engine::open(&client).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{export}");
    assert_eq!(links[0]["contract"], "package::maven::example:models");
}

#[test]
fn only_standalone_literal_include_arguments_name_gradle_artifacts() {
    let [_, gradle, _, _] = workspace("jvm_packages_includes");
    for (include, wanted) in [
        (
            "include(\":tools\" + suffix, \":extras:nested\")",
            vec!["example:kit", "example:nested"],
        ),
        (
            "include(projectName(\":tools\"), \":extras:nested\")",
            vec!["example:kit", "example:nested"],
        ),
        (
            "include(\":tools\", projectName(\":extras:nested\"))",
            vec!["example:kit", "example:tools"],
        ),
        (
            "include(\":tools\", \":extras:nested\")",
            vec!["example:kit", "example:nested", "example:tools"],
        ),
        ("include(\"::tools\")", vec!["example:kit"]),
    ] {
        fs::write(
            gradle.join("settings.gradle.kts"),
            format!("rootProject.name = \"kit\"\n{include}\n"),
        )
        .unwrap();
        let export = Engine::open(&gradle).unwrap().export().unwrap();
        let json: Value = serde_json::from_str(&export).unwrap();
        let actual: Vec<_> = json["packages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(actual, wanted, "{include}: {export}");
    }
}

#[test]
fn a_gradle_declaration_without_a_coordinate_string_is_no_consumer() {
    let [_, _, _, client] = workspace("jvm_packages_not_coordinates");
    for declaration in [
        "implementation(project(':tools'))",
        "implementation project(':tools')",
        "implementation(libs.models)",
        "testImplementation files('libs/models.jar')",
        "api(platform('example:kit:1'))",
    ] {
        fs::write(
            client.join("build.gradle"),
            format!("group = 'example'\ndependencies {{\n    {declaration}\n}}\n"),
        )
        .unwrap();
        let export = Engine::open(&client).unwrap().export().unwrap();
        let json: Value = serde_json::from_str(&export).unwrap();
        assert!(
            json["links"].as_array().unwrap().is_empty(),
            "{declaration}: {export}"
        );
    }
}

#[test]
fn map_notation_and_configured_declarations_read_their_coordinate() {
    let [_, _, _, client] = workspace("jvm_packages_declarations");
    let exact = Some(("package::maven::example:models", "exact"));
    for (declaration, wanted) in [
        (
            "implementation group: 'example', name: 'models', version: '1.2.0'",
            exact,
        ),
        (
            "implementation(group = \"example\", name = \"models\", version = \"1.2.0\")",
            exact,
        ),
        (
            "implementation group: 'outside', name: 'absent', version: '1'",
            None,
        ),
        ("implementation group: 'example', version: '1'", None),
        (
            "implementation(\"example:models:1.2.0\") { exclude(group = \"x\") }",
            exact,
        ),
        (
            "implementation('example:models:1.2.0') {\n        transitive = false\n    }",
            exact,
        ),
        // No literal fragment at all is no coordinate, as `libs.models` is none.
        ("implementation \"$coordinates\"", None),
        ("implementation(\"${group}:${artifact}:1\")", None),
    ] {
        fs::write(
            client.join("build.gradle"),
            format!("group = 'example'\ndependencies {{\n    {declaration}\n}}\n"),
        )
        .unwrap();
        let export = Engine::open(&client).unwrap().export().unwrap();
        let json: Value = serde_json::from_str(&export).unwrap();
        let links: Vec<_> = (json["links"].as_array().unwrap().iter())
            .map(|l| {
                (
                    l["contract"].as_str().unwrap(),
                    l["level"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            links,
            wanted.into_iter().collect::<Vec<_>>(),
            "{declaration}: {export}"
        );
    }
}
