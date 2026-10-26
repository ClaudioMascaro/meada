//! JVM artifacts link through literal Maven and Gradle manifests, without invoking a build.

use std::fs;
use std::path::PathBuf;

use crate::common::{age, assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
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

/// The links of `json` a build file declares: a dependency entry, not a call through an import.
fn declared(json: &Value) -> Vec<&Value> {
    (json["links"].as_array().into_iter().flatten())
        .filter(|l| !l["consumer"].as_str().unwrap().contains(".java::"))
        .collect()
}

#[test]
fn literal_maven_and_gradle_artifacts_link_to_workspace_dependencies() {
    let [maven, gradle, client, gradle_client] = workspace("jvm_packages_literals");
    let engine = Engine::open(&client).unwrap();
    let export = engine.export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let links = declared(&json);
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
fn a_csharp_file_under_a_pom_is_no_class_an_import_brings() {
    let [maven, _, client, _] = workspace("jvm_packages_csharp_under_pom");
    let gadget = "namespace example.models;\n\npublic class Gadget\n{\n    public static int make() => 1;\n}\n";
    let dir = maven.join("models/src/main/java/example/models");
    fs::write(dir.join("Gadget.cs"), gadget).unwrap();
    let user = "package example.web;\n\nimport example.models.Gadget;\n\npublic class Uses {\n    public int run() {\n        return Gadget.make();\n    }\n}\n";
    fs::write(client.join("src/main/java/example/web/Uses.java"), user).unwrap();
    let export = Engine::open(&client).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let uses = |l: &&Value| l["consumer"].as_str().unwrap().contains("Uses.java");
    assert_eq!(
        (json["links"].as_array().unwrap().iter())
            .filter(uses)
            .count(),
        0,
        "{export}"
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
        let links = declared(&json);
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
    assert_eq!(declared(&json).len(), 2, "{export}");
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
    let links = declared(&json);
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
    assert!(declared(&json).is_empty(), "{export}");
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
    let links = declared(&json);
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
        assert!(declared(&json).is_empty(), "{declaration}: {export}");
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
        let links: Vec<_> = (declared(&json).iter())
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

const CLIENT: &str = "example.com/jvm/client::src/main/java/example/web/";
const MODELS: &str = "example.com/jvm/maven::models/src/main/java/example/";
const KIT: &str = "example.com/jvm/gradle::";

/// The `(call, specifier, name, level, how, target)` of every import of `json` made from a
/// call site that starts with `site`, the site without it.
fn imports<'j>(
    json: &'j Value,
    site: &str,
) -> Vec<(&'j str, &'j str, &'j str, &'j str, &'j str, &'j str)> {
    (json["imports"].as_array().into_iter().flatten())
        .filter_map(|i| {
            let at = |k: &str| i[k].as_str().unwrap();
            Some((
                at("site").strip_prefix(site)?,
                at("specifier"),
                at("name"),
                at("level"),
                at("how"),
                at("target"),
            ))
        })
        .collect()
}

#[test]
fn an_import_reaches_the_method_of_the_class_in_the_provider() {
    let [_, _, client, _] = workspace("jvm_imports");
    let json: Value =
        serde_json::from_str(&Engine::open(&client).unwrap().export().unwrap()).unwrap();
    let at = |caller: &str| imports(&json, &format!("{CLIENT}{caller}"));
    let order = |member: &str| format!("{MODELS}models/Order.java::example.models.Order.{member}");
    let model = |call: &str, name: &'static str, level: &'static str, member: &str| {
        (
            call.to_string(),
            "example.models.Order",
            name,
            level,
            "package",
            order(member),
        )
    };
    let seen = |caller: &str| -> Vec<_> {
        at(caller)
            .into_iter()
            .map(|(c, s, n, l, h, t)| (c.to_string(), s, n, l, h, t.to_string()))
            .collect()
    };
    let pay = "Checkout.java::example.web.Checkout.pay(int)::";
    // Static and typed-local calls; overloads narrowed by the argument count.
    assert_eq!(
        seen(pay),
        [
            model("Order.of#0", "of(1)", "exact", "of(int)"),
            model("order.place#0", "place(2)", "exact", "place(int,int)"),
            model("order.place#1", "place(1)", "ambiguous", "place(String)"),
            model("order.place#1", "place(1)", "ambiguous", "place(int)"),
            model("order.total#0", "total(0)", "exact", "total()"),
        ]
    );
    // A constructor is a method named after its class.
    assert_eq!(
        seen("Checkout.java::example.web.Checkout.create()::"),
        [model("Order#0", "Order(1)", "exact", "Order(int)")]
    );
    // Two providers the client depends on declare the class: ambiguous in each.
    let stamp = at("Checkout.java::example.web.Checkout.stamp()::");
    assert_eq!(
        stamp.iter().map(|i| (i.1, i.2, i.3)).collect::<Vec<_>>(),
        [
            ("example.shared.Stamp", "now(0)", "ambiguous"),
            ("example.shared.Stamp", "now(0)", "ambiguous")
        ]
    );
    let mut targets: Vec<_> = stamp.iter().map(|i| i.5).collect();
    targets.sort();
    assert_eq!(
        targets,
        [
            format!("{KIT}src/main/java/example/shared/Stamp.java::example.shared.Stamp.now()"),
            format!("{MODELS}shared/Stamp.java::example.shared.Stamp.now()"),
        ]
    );
    // A class of a test source root, a type outside the workspace: no import.
    for caller in ["sample", "external"] {
        assert!(at(&format!("Checkout.java::example.web.Checkout.{caller}()::")).is_empty());
    }
    // An on-demand import brings the type, a static import the method.
    for (caller, call) in [("total(int)", "Order.of#0"), ("prepared()", "of#0")] {
        assert_eq!(
            at(&format!("Bulk.java::example.web.Bulk.{caller}::"))
                .iter()
                .map(|i| (i.0, i.1, i.2, i.3))
                .collect::<Vec<_>>(),
            [(call, "example.models.Order", "of(1)", "exact")]
        );
    }
    // The link's reason says the symbol hop.
    let reasons: Vec<_> = (json["links"].as_array().unwrap().iter())
        .filter(|l| l["consumer"].as_str().unwrap().ends_with("order.total#0"))
        .map(|l| (l["level"].as_str().unwrap(), l["reason"].as_str().unwrap()))
        .collect();
    assert_eq!(
        reasons,
        [(
            "exact",
            "package::maven::example:models 1.2.0 (local 1.2.0), range unchecked; import package"
        )]
    );
}

#[test]
fn on_demand_candidates_and_shadowing_names_never_give_a_wrong_exact() {
    let [_, _, client, _] = workspace("jvm_imports_on_demand");
    let json: Value =
        serde_json::from_str(&Engine::open(&client).unwrap().export().unwrap()).unwrap();
    // Two on-demand imports whose packages both declare `Ledger`: neither enters, even where
    // only one of them declares the method.
    let wild = "example.com/jvm/client::src/main/java/example/batch/Wild.java::";
    assert!(
        imports(&json, wild).is_empty(),
        "{:?}",
        imports(&json, wild)
    );
    // A repository class of the file's package shadows `import example.models.*`; a parameter
    // and a local named like the imported class hide it.
    let probes = imports(&json, &format!("{CLIENT}Probes.java::"));
    assert!(probes.is_empty(), "{probes:?}");
    // Two providers declare `example.shared.Stamp`, though only one declares `later()`.
    let later = imports(
        &json,
        &format!("{CLIENT}Checkout.java::example.web.Checkout.later()::"),
    );
    assert_eq!(
        later.iter().map(|i| (i.2, i.3, i.5)).collect::<Vec<_>>(),
        [(
            "later(0)",
            "ambiguous",
            &*format!("{MODELS}shared/Stamp.java::example.shared.Stamp.later()")
        )]
    );
}

#[test]
fn an_import_merges_the_overloads_of_the_provider_class_family() {
    let [_, _, client, _] = workspace("jvm_imports_family");
    let json: Value =
        serde_json::from_str(&Engine::open(&client).unwrap().export().unwrap()).unwrap();
    let at = |caller: &str| -> Vec<(String, String, String, String)> {
        imports(
            &json,
            &format!("{CLIENT}Family.java::example.web.Family.{caller}::"),
        )
        .into_iter()
        .map(|(c, _, n, l, _, t)| {
            let t = t.strip_prefix(&format!("{MODELS}models/")).unwrap_or(t);
            (c.into(), n.into(), l.into(), t.into())
        })
        .collect()
    };
    let row = |c: &str, n: &str, l: &str, t: &str| (c.into(), n.into(), l.into(), t.into());
    // An inherited overload of the count stays beside the class's own.
    assert_eq!(
        at("find(Repo)"),
        [
            row(
                "repo.find#0",
                "find(1)",
                "ambiguous",
                "Base.java::example.models.Base.find(long)"
            ),
            row(
                "repo.find#0",
                "find(1)",
                "ambiguous",
                "Repo.java::example.models.Repo.find(String)"
            ),
        ]
    );
    assert_eq!(
        at("made()"),
        [
            row(
                "Repo.of#0",
                "of(1)",
                "ambiguous",
                "Base.java::example.models.Base.of(int)"
            ),
            row(
                "Repo.of#0",
                "of(1)",
                "ambiguous",
                "Repo.java::example.models.Repo.of(String)"
            ),
        ]
    );
    // An override hides the base's signature; a method only the base declares is inherited.
    assert_eq!(
        at("named(Repo)"),
        [row(
            "repo.name#0",
            "name(0)",
            "exact",
            "Repo.java::example.models.Repo.name()"
        )]
    );
    assert_eq!(
        at("sized(Repo)"),
        [row(
            "repo.size#0",
            "size(0)",
            "exact",
            "Base.java::example.models.Base.size()"
        )]
    );
    // A base outside the provider's repository may declare an overload: no `exact`.
    assert_eq!(
        at("coded(Event)"),
        [row(
            "event.code#0",
            "code(0)",
            "inferred",
            "Event.java::example.models.Event.code()"
        )]
    );
    // A method named after its class is no constructor.
    assert!(at("noted()").is_empty(), "{:?}", at("noted()"));
}

#[test]
fn a_gradle_source_set_directory_decides_which_classes_a_dependent_reaches() {
    let [_, _, _, worker] = workspace("jvm_source_sets");
    let json: Value =
        serde_json::from_str(&Engine::open(&worker).unwrap().export().unwrap()).unwrap();
    let job =
        "example.com/jvm/gradle-client::src/main/java/example/worker/Job.java::example.worker.Job.";
    let tools = "example.com/jvm/gradle::tools/src/core/java/example/tools/Slug.java::example.tools.Slug.of(String)";
    // The `main` literal directory holds `Slug`; the `test` one holds `Checks`.
    let slug: Vec<_> = imports(&json, &format!("{job}run(String)::"))
        .into_iter()
        .map(|i| (i.1, i.2, i.3, i.5))
        .collect();
    assert_eq!(slug, [("example.tools.Slug", "of(1)", "exact", tools)]);
    assert!(imports(&json, &format!("{job}check()::")).is_empty());
    // `Order` reaches through the `testImplementation` dependency, as any declared field.
    assert_eq!(imports(&json, &format!("{job}count(Order)::")).len(), 1);
}

/// Moves the `test` source set of `tools` onto another directory, so `src/check/java` is no
/// source root: `Checks` is then a class outside any root, which resolves by its package.
fn move_test_set(gradle: &std::path::Path) {
    let file = gradle.join("tools/build.gradle");
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("src/check/java"));
    fs::write(file, text.replace("src/check/java", "src/other/java")).unwrap();
}

#[test]
fn an_edited_source_set_relinks_like_a_cold_workspace() {
    let warm = workspace("jvm_source_sets_fresh");
    age(warm[0].parent().unwrap());
    let exports = |repos: &[PathBuf; 4]| {
        repos
            .each_ref()
            .map(|p| Engine::open(p).unwrap().export().unwrap())
    };
    let before = exports(&warm);
    move_test_set(&warm[1]);
    let refreshed = exports(&warm);
    let cold = workspace("jvm_source_sets_cold");
    move_test_set(&cold[1]);
    assert_ne!(before, refreshed);
    assert_eq!(refreshed, exports(&cold));
    let json: Value = serde_json::from_str(&refreshed[3]).unwrap();
    let job = "example.com/jvm/gradle-client::src/main/java/example/worker/Job.java::example.worker.Job.check()::";
    assert_eq!(imports(&json, job).len(), 1);
}

const CHECKOUT_TOTAL: &str = "example.com/jvm/client::src/main/java/example/web/Checkout.java::example.web.Checkout.pay(int)::order.total#0";
const ORDER_TOTAL: &str = "example.com/jvm/maven::models/src/main/java/example/models/Order.java::example.models.Order.total()";

#[test]
fn context_on_a_public_class_method_lists_its_consumers_in_other_repositories() {
    let [maven, ..] = workspace("jvm_context");
    let found = Engine::open(&maven)
        .unwrap()
        .context(ORDER_TOTAL, None)
        .unwrap();
    assert_golden("jvm-maven.context-total.txt", &found);
}

/// A public class lists as consumers the calls in other repositories that reach it through its
/// import (spec #26, user story 10): those that reach its constructors and methods, one line per
/// call, at its link's level.
#[test]
fn context_on_a_public_class_lists_the_calls_that_reach_it_in_other_repositories() {
    let [maven, ..] = workspace("jvm_context_class");
    let order = "example.com/jvm/maven::models/src/main/java/example/models/Order.java::example.models.Order";

    let found = Engine::open(&maven).unwrap().context(order, None).unwrap();

    assert!(
        found.contains("example.com/jvm/client src/main/java/example/web/Checkout.java:10 example.web.Checkout.pay(int)"),
        "{found}"
    );
    assert_golden("jvm-maven.context-order.txt", &found);
}

#[test]
fn trace_down_crosses_the_package_hop_and_onward() {
    let [_, _, client, _] = workspace("jvm_trace_down");
    let down = Engine::open(&client)
        .unwrap()
        .trace(Start::From(CHECKOUT_TOTAL), Direction::Down, None)
        .unwrap();
    assert_golden("jvm-client.trace-down.txt", &down);
}
