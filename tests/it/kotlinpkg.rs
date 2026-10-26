//! Kotlin DSL builds give package contracts and links like the Groovy ones (spec #27).

use std::fs;
use std::path::PathBuf;

use crate::common::{age, assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

fn workspace(case: &str) -> [PathBuf; 2] {
    // The kit is a peer of the app too: it declares the package of a second provider.
    fixture_repo(
        case,
        "kotlin-pkg-kit",
        Some("https://example.com/kotlin/kit.git"),
    );
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
    // The links of the build files: a call through an import is another test's.
    let links: Vec<_> = (json["links"].as_array().unwrap().iter())
        .filter(|l| !l["consumer"].as_str().unwrap().contains(".kt::"))
        .collect();
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
        "implementation package::maven::dev.kit:kit exact build.gradle.kts | package::maven::dev.kit:kit 1.0 (local 1.0), range unchecked".to_string(),
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

const APP: &str = "example.com/kotlin/app::src/main/kotlin/dev/shop/";
const KOTLIN: &str = "example.com/kotlin/";
const LIB: &str = "example.com/kotlin/lib::";
const CORE: &str = "example.com/kotlin/lib::core/src/main/kotlin/dev/kt/core/";

/// The `(call, specifier, name, level, how, target)` of every import made from a call site that
/// starts with `site`, the site without it.
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

/// The imports of the calls in `caller` of `file` in the app, as `(call, specifier, name,
/// level, target)`.
fn calls(json: &Value, file: &str, caller: &str) -> Vec<(String, String, String, String, String)> {
    (imports(json, &format!("{APP}{file}::dev.shop.{caller}::")).into_iter())
        .inspect(|i| assert_eq!(i.4, "package"))
        .map(|(c, s, n, l, _, t)| (c.into(), s.into(), n.into(), l.into(), t.into()))
        .collect()
}

fn app_export(case: &str) -> Value {
    let [_, app] = workspace(case);
    serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap()
}

#[test]
fn an_import_reaches_the_declaration_in_the_provider_sources() {
    let json = app_export("kotlin_imports");
    let at = |file: &str, caller: &str| calls(&json, file, caller);
    let row = |call: &str, spec: &str, name: &str, level: &str, target: String| {
        (call.into(), spec.into(), name.into(), level.into(), target)
    };
    let money = |member: &str| format!("{CORE}Money.kt::dev.kt.core.Money{member}");
    // A constructor is the class, a companion call its member, an extension and a method by the
    // type of the receiver.
    assert_eq!(
        at("Shop.kt", "pay(Int)"),
        [
            row("Money#0", "dev.kt.core", "Money(1)", "exact", money("")),
            row(
                "Money.of#0",
                "dev.kt.core.Money",
                "Companion.of(1)",
                "exact",
                money(".Companion.of(Int)")
            ),
            // An extension crosses by the import of its name, never as a member of the class:
            // `exact` on a receiver of the provider's class, whose members are all known.
            row(
                "doubled.format#0",
                "dev.kt.core",
                ".format(0)@dev.kt.core.Money",
                "exact",
                money(".format()")
            ),
            row(
                "order.plus#0",
                "dev.kt.core.Money",
                "plus(1)",
                "exact",
                money(".plus(Money)")
            ),
            row(
                "order.total#0",
                "dev.kt.core.Money",
                "total(0)",
                "exact",
                money(".total()")
            ),
        ]
    );
    // A top-level function, overloads narrowed by the argument count.
    let greet = |sig: &str| format!("{CORE}Core.kt::dev.kt.core.greet({sig})");
    assert_eq!(
        at("Shop.kt", "hello()"),
        [
            row(
                "greet#0",
                "dev.kt.core",
                "greet(1)",
                "exact",
                greet("String")
            ),
            row(
                "greet#1",
                "dev.kt.core",
                "greet(2)",
                "exact",
                greet("String,Int")
            ),
        ]
    );
    // An extension function is imported by its name: a name match, as a member of the receiver's
    // type, which is not read, would win.
    assert_eq!(
        at("Shop.kt", "loud(String)"),
        [row(
            "name.shout#0",
            "dev.kt.core",
            ".shout(0)",
            "inferred",
            format!("{CORE}Core.kt::dev.kt.core.String.shout()")
        )]
    );
    // An alias names the class, a star import the package.
    assert_eq!(
        at("Shop.kt", "aliased()"),
        [row(
            "Cash.of#0",
            "dev.kt.core.Money",
            "Companion.of(1)",
            "exact",
            money(".Companion.of(Int)")
        )]
    );
    assert_eq!(
        at("Bulk.kt", "bulk()"),
        [
            row(
                "Money.of#0",
                "dev.kt.core.Money",
                "Companion.of(1)",
                "exact",
                money(".Companion.of(Int)")
            ),
            row(
                "greet#0",
                "dev.kt.core",
                "greet(1)",
                "exact",
                greet("String")
            ),
        ]
    );
    // A top-level function of a main source set nested in a test root; the test sources, a
    // literal test directory, an internal function and a type outside the workspace: no import.
    assert_eq!(
        at("Shop.kt", "reached()"),
        [row(
            "extras#0",
            "dev.kt.core",
            "extras(0)",
            "exact",
            format!("{LIB}core/src/test/kotlin/shared/dev/kt/core/Extras.kt::dev.kt.core.extras()")
        )]
    );
    // A file outside every root of its module resolves by its package declaration.
    assert_eq!(
        at("Shop.kt", "loosely()"),
        [row(
            "loose#0",
            "dev.kt.core",
            "loose(0)",
            "exact",
            format!("{LIB}core/loose/dev/kt/core/Loose.kt::dev.kt.core.loose()")
        )]
    );
    assert!(at("Shop.kt", "unreached(String)").is_empty(), "{json}");
    let probes = format!("{APP}probes/Probes.kt::dev.shop.probes.");
    assert!(imports(&json, &probes).is_empty(), "{json}");
}

fn rows(json: &Value, caller: &str) -> Vec<(String, String, String, String)> {
    (calls(json, "Ext.kt", caller).into_iter())
        .map(|(c, s, n, l, t)| (c, format!("{s} {n} {l}"), t, String::new()))
        .collect()
}

#[test]
fn a_call_that_may_leave_arguments_out_reaches_every_overload_it_could() {
    let json = app_export("kotlin_imports_defaults");
    let core = |sig: &str| format!("{CORE}Core.kt::dev.kt.core.{sig}");
    let row = |call: &str, spec: &str, level: &str, target: String| {
        (
            call.into(),
            format!("dev.kt.core {spec} {level}"),
            target,
            String::new(),
        )
    };
    // `pick(x, y = 0)` takes the call that leaves `y` out, and so does `pick(text)`.
    assert_eq!(
        rows(&json, "defaults()"),
        [
            row("pick#0", "pick(1)", "ambiguous", core("pick(Int,Int)")),
            row("pick#0", "pick(1)", "ambiguous", core("pick(String)")),
        ]
    );
    assert_eq!(
        rows(&json, "every()"),
        [row("pick#0", "pick(2)", "exact", core("pick(Int,Int)"))]
    );
    // A vararg takes any number of arguments.
    assert_eq!(
        rows(&json, "varargs()"),
        [row(
            "many#0",
            "many(2)",
            "exact",
            core("many(vararg String)")
        )]
    );
}

#[test]
fn an_extension_crosses_by_the_import_of_its_name_and_a_member_wins() {
    let json = app_export("kotlin_imports_extensions");
    let ext = |call: &str, receiver: &str, level: &str, target: &str| {
        (
            call.to_string(),
            format!(
                "dev.kt.core .{call_name}(0)@{receiver} {level}",
                call_name = call.split(['.', '#']).nth(1).unwrap()
            ),
            format!("{CORE}{target}"),
            String::new(),
        )
    };
    // Typed receivers, in a file of neither class: a class of the provider, whose members are
    // known (`exact`), and an outside type, whose members may hide the extension.
    assert_eq!(
        rows(&json, "labelled(Money)"),
        [ext(
            "money.label#0",
            "dev.kt.core.Money",
            "exact",
            "MoneyExt.kt::dev.kt.core.Money.label()"
        )]
    );
    assert_eq!(
        rows(&json, "drawn(Thing)"),
        [ext(
            "thing.render#0",
            "outside.absent.Thing",
            "inferred",
            "MoneyExt.kt::dev.kt.core.Thing.render()"
        )]
    );
    // The member of the receiver's type, not the extension of the same name.
    assert_eq!(
        rows(&json, "counted(Money)"),
        [(
            "money.total#0".to_string(),
            "dev.kt.core.Money total(0) exact".to_string(),
            format!("{CORE}Money.kt::dev.kt.core.Money.total()"),
            String::new()
        )]
    );
    // The explicit import beats the star import of the same name.
    assert_eq!(
        rows(&json, "shouted(String)"),
        [(
            "text.shout#0".to_string(),
            "dev.kt.core .shout(0) inferred".to_string(),
            format!("{CORE}Core.kt::dev.kt.core.String.shout()"),
            String::new()
        )]
    );
    // The app's own extension of the same class and name, with the provider's never imported.
    let own = format!("{APP}own/Own.kt::");
    assert!(imports(&json, &own).is_empty(), "{json}");
}

#[test]
fn a_call_on_the_class_name_is_the_companion_s_and_one_on_an_instance_the_member_s() {
    let json = app_export("kotlin_imports_companion");
    let boxed = |member: &str| format!("{CORE}Box.kt::dev.kt.core.Box{member}");
    assert_eq!(
        calls(&json, "Ext.kt", "viaClass()"),
        [(
            "Box.of#0".into(),
            "dev.kt.core.Box".into(),
            "Companion.of(1)".into(),
            "exact".into(),
            boxed(".Companion.of(Int)")
        )]
    );
    assert_eq!(
        calls(&json, "Ext.kt", "viaInstance(Box)"),
        [(
            "box.of#0".into(),
            "dev.kt.core.Box".into(),
            "of(1)".into(),
            "exact".into(),
            boxed(".of(Int)")
        )]
    );
}

#[test]
fn a_name_the_code_hides_or_a_class_from_outside_reaches_no_provider() {
    let json = app_export("kotlin_imports_probes");
    // Hidden names: a local function, a parameter, a local value and a member of the enclosing
    // class, each named like the imported function; the one call that reaches it is the control.
    let hidden = imports(&json, &format!("{APP}probes/Hidden.kt::"));
    assert_eq!(hidden.len(), 1, "{hidden:?}");
    assert_eq!(hidden[0].0, "dev.shop.probes.real()::greet#0");
    // Opaque outside: a class from a library named like the package's, imported by name over the
    // package's star import, has members unknown to the workspace.
    assert!(
        imports(&json, &format!("{APP}probes/Opaque.kt::")).is_empty(),
        "{json}"
    );
}

#[test]
fn several_providers_declaring_the_package_and_declaration_make_the_import_ambiguous() {
    let json = app_export("kotlin_imports_ambiguous");
    let at = |caller: &str| calls(&json, "Shop.kt", caller);
    let seen = |caller: &str| -> Vec<_> {
        at(caller)
            .into_iter()
            .map(|(c, s, n, l, t)| (c, s, n, l, t.strip_prefix(KOTLIN).unwrap().to_string()))
            .collect()
    };
    let row = |c: &str, s: &str, n: &str, t: &str| {
        (c.into(), s.into(), n.into(), "ambiguous".into(), t.into())
    };
    // A Kotlin object of one provider and a Java class of another.
    assert_eq!(
        seen("stamp()"),
        [
            row(
                "Stamp.now#0",
                "dev.kt.shared.Stamp",
                "Companion.now(0)",
                "kit::src/main/java/dev/kt/shared/Stamp.java::dev.kt.shared.Stamp.now()"
            ),
            row(
                "Stamp.now#0",
                "dev.kt.shared.Stamp",
                "Companion.now(0)",
                "lib::src/main/kotlin/dev/kt/shared/Shared.kt::dev.kt.shared.Stamp.now()"
            ),
            row(
                "stamped#0",
                "dev.kt.shared",
                "stamped(0)",
                "kit::src/main/kotlin/dev/kt/shared/Marks.kt::dev.kt.shared.stamped()"
            ),
            row(
                "stamped#0",
                "dev.kt.shared",
                "stamped(0)",
                "lib::src/main/kotlin/dev/kt/shared/Shared.kt::dev.kt.shared.stamped()"
            ),
        ]
    );
    // The link's reason says the symbol hop.
    let reasons: Vec<_> = (json["links"].as_array().unwrap().iter())
        .filter(|l| {
            l["consumer"]
                .as_str()
                .unwrap()
                .ends_with("hello()::greet#0")
        })
        .map(|l| (l["level"].as_str().unwrap(), l["reason"].as_str().unwrap()))
        .collect();
    assert_eq!(
        reasons,
        [(
            "exact",
            "package::maven::dev.kt.core:core 3.1 (local 3.1), range unchecked; import package"
        )]
    );
}

const GREET: &str =
    "example.com/kotlin/lib::core/src/main/kotlin/dev/kt/core/Core.kt::dev.kt.core.greet(String)";
const MONEY: &str =
    "example.com/kotlin/lib::core/src/main/kotlin/dev/kt/core/Money.kt::dev.kt.core.Money";

#[test]
fn context_on_a_public_declaration_lists_its_consumers_in_other_repositories() {
    let [lib, _] = workspace("kotlin_context");
    let engine = Engine::open(&lib).unwrap();
    assert_golden(
        "kotlin-pkg-lib.context-greet.txt",
        &engine.context(GREET, None).unwrap(),
    );
    assert_golden(
        "kotlin-pkg-lib.context-money.txt",
        &engine.context(MONEY, None).unwrap(),
    );
}

#[test]
fn trace_down_crosses_the_package_hop_and_onward() {
    let [_, app] = workspace("kotlin_trace_down");
    let from = format!("{APP}Shop.kt::dev.shop.pay(Int)::doubled.format#0");
    let down = Engine::open(&app)
        .unwrap()
        .trace(Start::From(&from), Direction::Down, None)
        .unwrap();
    assert_golden("kotlin-pkg-app.trace-down.txt", &down);
}

/// Moves the `test` source set of `core` onto another directory, so `src/check/kotlin` is no
/// source root: `Checks` is then a file outside any root, which resolves by its package.
fn move_test_set(lib: &std::path::Path) {
    let file = lib.join("core/build.gradle.kts");
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("src/check/kotlin"));
    fs::write(file, text.replace("src/check/kotlin", "src/other/kotlin")).unwrap();
}

#[test]
fn an_edited_source_set_relinks_like_a_cold_workspace() {
    let warm = workspace("kotlin_source_sets_fresh");
    age(warm[0].parent().unwrap());
    let exports = |repos: &[PathBuf; 2]| {
        repos
            .each_ref()
            .map(|p| Engine::open(p).unwrap().export().unwrap())
    };
    let before = exports(&warm);
    move_test_set(&warm[0]);
    let refreshed = exports(&warm);
    let cold = workspace("kotlin_source_sets_cold");
    move_test_set(&cold[0]);
    assert_ne!(before, refreshed);
    assert_eq!(refreshed, exports(&cold));
    let json: Value = serde_json::from_str(&refreshed[1]).unwrap();
    assert_eq!(calls(&json, "Shop.kt", "unreached(String)").len(), 1);
}
