//! SwiftPM packages link through literal `Package.swift` files, without a Swift toolchain or
//! `Package.resolved` (spec #29, "Packages").

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{age, assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

fn workspace(case: &str) -> [PathBuf; 3] {
    ["kit", "lib", "app"].map(|role| {
        fixture_repo(
            case,
            &format!("swift-pkg-{role}"),
            Some(&match role {
                "app" => "https://example.com/apps/shop.git".to_string(),
                role => format!("https://example.com/swift/{role}.git"),
            }),
        )
    })
}

fn export(repo: &Path) -> String {
    Engine::open(repo).unwrap().export().unwrap()
}

fn app_json(case: &str) -> Value {
    let [_, _, app] = workspace(case);
    serde_json::from_str(&export(&app)).unwrap()
}

#[test]
fn package_swift_gives_providers_and_links_literal_dependencies() {
    let [kit, lib, app] = workspace("swift_packages_literals");
    let kit_export = export(&kit);
    let json: Value = serde_json::from_str(&kit_export).unwrap();
    let names: Vec<_> = (json["packages"].as_array().unwrap().iter())
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Kit"]);
    // One contract per package, the library products and their targets recorded.
    assert_eq!(
        json["providers"].as_array().unwrap().len(),
        1,
        "{kit_export}"
    );
    assert_golden("swift-pkg-kit.export.json", &kit_export);
    assert_golden("swift-pkg-lib.export.json", &export(&lib));
    let engine = Engine::open(&app).unwrap();
    let app_export = engine.export().unwrap();
    let json: Value = serde_json::from_str(&app_export).unwrap();
    let links: Vec<_> = (json["links"].as_array().unwrap().iter())
        .filter(|l| {
            l["consumer"]
                .as_str()
                .unwrap()
                .contains("::Package.swift::")
        })
        .map(|l| {
            let at = |k: &str| l[k].as_str().unwrap();
            (at("consumer"), at("level"), at("reason"))
        })
        .collect();
    // A url and a path link exact; the computed url is ambiguous; vapor is outside the workspace.
    let level = |name: &str| links.iter().find(|l| l.0.ends_with(name)).map(|l| l.1);
    assert_eq!(level("::Kit"), Some("exact"), "{links:?}");
    assert_eq!(level("::Shared"), Some("exact"), "{links:?}");
    assert!(
        links.iter().any(|l| l.1 == "ambiguous"
            && l.2.contains("computed dependency")
            && l.2.contains("2 providers fit")),
        "{links:?}"
    );
    assert!(
        !app_export.contains("vapor"),
        "an external dependency is ignored"
    );
    assert!(!app_export.contains("Package.resolved"));
    assert_golden("swift-pkg-app.export.json", &app_export);
    assert_golden(
        "swift-packages.search.txt",
        &engine.search("", Some("package"), None).unwrap(),
    );
}

const APP: &str = "example.com/apps/shop::Sources/App/main.swift::";
const KIT: &str = "example.com/swift/kit::Sources/Kit/Greeter.swift::";

/// `(call, level, how, target)` of every import made from a call in `caller`.
fn calls(json: &Value, caller: &str) -> Vec<(String, String, String, String)> {
    let site = format!("{APP}{caller}::");
    (json["imports"].as_array().into_iter().flatten())
        .filter_map(|i| {
            let at = |k: &str| i[k].as_str().unwrap().to_string();
            let call = at("site").strip_prefix(&site)?.to_string();
            Some((call, at("level"), at("how"), at("target")))
        })
        .collect()
}

#[test]
fn an_import_of_a_module_reaches_its_exported_declarations_by_labels() {
    let json = app_json("swift_packages_imports");
    let target = |caller: &str, call: &str| {
        let found: Vec<_> = calls(&json, caller)
            .into_iter()
            .filter(|c| c.0 == call)
            .collect();
        assert_eq!(found.len(), 1, "{caller}: {found:?}");
        let (_, level, how, target) = found.into_iter().next().unwrap();
        assert_eq!(
            (level.as_str(), how.as_str()),
            ("exact", "module"),
            "{caller}"
        );
        target
    };
    // Argument labels narrow among the overloads of `greet`, through a bare call, a call
    // qualified by the module and an unlabelled one.
    assert_eq!(
        target("welcome(who:)", "greet#0"),
        format!("{KIT}greet(name:)")
    );
    assert_eq!(
        target("formal(who:)", "Kit.greet#0"),
        format!("{KIT}greet(name:greeting:)")
    );
    assert_eq!(target("plain(who:)", "greet#0"), format!("{KIT}greet(_:)"));
    // A constructor, and a method on a local made by one or on a typed parameter, with a
    // defaulted parameter left out.
    assert_eq!(
        target("hello(who:)", "Greeter#0"),
        format!("{KIT}Greeter.init(name:)")
    );
    assert_eq!(
        target("hello(who:)", "greeter.hello#0"),
        format!("{KIT}Greeter.hello(to:loudly:)")
    );
    assert_eq!(
        target("typed(greeter:)", "greeter.hello#0"),
        format!("{KIT}Greeter.hello(to:loudly:)")
    );
    // A module of another package.
    assert_eq!(
        target("loud(text:)", "shout#0"),
        "example.com/swift/lib::Sources/Shared/Shared.swift::shout(_:)"
    );
    // Two packages expose `KitMoney`: ambiguous, never exact.
    let money = calls(&json, "total()");
    assert!(!money.is_empty());
    assert!(money.iter().all(|m| m.1 == "ambiguous"), "{money:?}");
    let repos: Vec<_> = money
        .iter()
        .map(|m| m.3.split("::").next().unwrap())
        .collect();
    assert!(repos.contains(&"example.com/swift/kit") && repos.contains(&"example.com/swift/lib"));
    // Internal declarations and outside modules reach nothing.
    assert!(
        calls(&json, "local()").is_empty(),
        "{:?}",
        calls(&json, "local()")
    );
}

const GREET: &str = "example.com/swift/kit::Sources/Kit/Greeter.swift::greet(name:)";
const HELLO: &str = "example.com/swift/kit::Sources/Kit/Greeter.swift::Greeter.hello(to:loudly:)";

#[test]
fn context_on_a_public_declaration_lists_its_consumers_in_other_repositories() {
    let [kit, _, _] = workspace("swift_packages_context");
    let engine = Engine::open(&kit).unwrap();
    assert_golden(
        "swift-pkg-kit.context-greet.txt",
        &engine.context(GREET, None).unwrap(),
    );
    assert_golden(
        "swift-pkg-kit.context-hello.txt",
        &engine.context(HELLO, None).unwrap(),
    );
}

#[test]
fn trace_down_crosses_the_package_hop() {
    let [_, _, app] = workspace("swift_packages_trace_down");
    let from = format!("{APP}hello(who:)::greeter.hello#0");
    let down = Engine::open(&app)
        .unwrap()
        .trace(Start::From(&from), Direction::Down, None)
        .unwrap();
    assert_golden("swift-pkg-app.trace-down.txt", &down);
}

fn change_manifests(repos: &[PathBuf; 3]) {
    let [kit, lib, app] = repos;
    let replace = |path: PathBuf, old: &str, new: &str| {
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(old), "{} lacks {old}", path.display());
        fs::write(&path, text.replace(old, new)).unwrap();
    };
    replace(
        kit.join("Package.swift"),
        "\"KitMoney\"]",
        "\"KitMoney\", \"Hidden\"]",
    );
    replace(
        lib.join("Package.swift"),
        "name: \"Shared\"",
        "name: \"Common\"",
    );
    replace(
        app.join("Package.swift"),
        "from: \"1.2.0\"",
        "from: \"1.3.0\"",
    );
    replace(
        app.join("Package.swift"),
        "../swift-pkg-lib",
        "../swift-pkg-kit",
    );
}

#[test]
fn edited_package_swift_files_refresh_like_a_cold_workspace() {
    let warm = workspace("swift_packages_refresh");
    age(warm[0].parent().unwrap());
    let exports = |repos: &[PathBuf; 3]| repos.each_ref().map(|p| export(p));
    let before = exports(&warm);
    assert_eq!(before, exports(&warm), "byte-identical repeated exports");
    change_manifests(&warm);
    let refreshed = exports(&warm);
    assert_ne!(before, refreshed);
    let cold = workspace("swift_packages_cold");
    change_manifests(&cold);
    assert_eq!(refreshed, exports(&cold));
}
