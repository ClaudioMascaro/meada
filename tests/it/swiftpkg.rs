//! SwiftPM packages link through literal `Package.swift` files, without a Swift toolchain or
//! `Package.resolved` (spec #29, "Packages").

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{age, assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

fn workspace(case: &str) -> [PathBuf; 4] {
    ["kit", "lib", "dyn", "app"].map(|role| {
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
    let [.., app] = workspace(case);
    serde_json::from_str(&export(&app)).unwrap()
}

#[test]
fn package_swift_gives_providers_and_links_literal_dependencies() {
    let [kit, lib, _, app] = workspace("swift_packages_literals");
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
            && l.2.contains("3 providers fit")),
        "{links:?}"
    );
    assert!(
        !app_export.contains("vapor"),
        "an external dependency is ignored"
    );
    // A computed part that folding takes out of a URL (a userinfo) still makes it uncertain, and
    // so does a package whose name is computed.
    assert!(
        links
            .iter()
            .any(|l| l.0.ends_with("::example.com/swift/lib*")
                && l.1 == "ambiguous"
                && l.2.starts_with("computed dependency;")),
        "{links:?}"
    );
    assert!(
        links.iter().any(|l| l.0.ends_with("::dependencies::*")
            && l.1 == "ambiguous"
            && l.2.contains("computed dependency")),
        "{links:?}"
    );
    // `Package.resolved` is never read: the fixture pins a version no manifest writes.
    assert!(!app_export.contains("9.9.9"), "Package.resolved was read");
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
    // A method of a type only one package declares, when two expose the module: still ambiguous.
    let both = calls(&json, "both(m:)");
    assert!(!both.is_empty());
    assert!(both.iter().all(|m| m.1 == "ambiguous"), "{both:?}");
    // A variadic with a label of its own takes the unlabelled arguments after its first.
    assert_eq!(
        target("variadic()", "record#0"),
        format!("{KIT}record(level:items:)")
    );
    // A package whose name is computed is an uncertain provider.
    let dynamic: Vec<_> = (json["links"].as_array().unwrap().iter())
        .filter(|l| {
            l["consumer"]
                .as_str()
                .unwrap()
                .contains("viaDyn()::dynamic#0")
        })
        .filter(|l| l["contract"] == "package::swiftpm::*")
        .collect();
    assert_eq!(dynamic.len(), 1, "{dynamic:?}");
    assert_eq!(dynamic[0]["level"], "ambiguous");
    assert_eq!(
        dynamic[0]["provider"],
        "example.com/swift/dyn::Sources/Dyn/Dyn.swift::dynamic(name:)"
    );
    // Internal declarations and outside modules reach nothing.
    assert!(
        calls(&json, "local()").is_empty(),
        "{:?}",
        calls(&json, "local()")
    );
}

/// What a name the code declares itself (a parameter, a local, a closure parameter, a value
/// that is not only a construction) hides: no import of a call that `call` names in `caller`.
#[test]
fn a_name_the_code_declares_hides_the_one_an_import_brings() {
    let json = app_json("swift_packages_hidden");
    for (caller, call) in [
        ("hidesGreet(greet:)", "greet#0"),
        ("hidesGreetByLocal()", "greet#0"),
        ("hidesGreeterByParameter(greeter:)", "greeter.hello#0"),
        ("hidesGreeterByLocal()", "greeter.hello#0"),
        ("hidesTheTypeByParameter(Greeter:)", "Greeter.hello#0"),
        // Through values: two origins, and a construction a method was called on.
        ("twoOrigins(flag:)", "held.hello#0"),
        ("onAResult()", "result.hello#0"),
    ] {
        let found = calls(&json, caller);
        assert!(found.iter().all(|c| c.0 != call), "{caller}: {found:?}");
    }
    assert!(calls(&json, "hidesGreetByClosure()").is_empty());
    // The same shapes without the hiding do reach the package.
    assert!(!calls(&json, "typed(greeter:)").is_empty());
}

#[test]
fn a_path_to_the_repository_s_own_root_links_the_package() {
    let [_, lib, ..] = workspace("swift_packages_own_root");
    let json: Value = serde_json::from_str(&export(&lib)).unwrap();
    let link = (json["links"].as_array().unwrap().iter())
        .find(|l| {
            l["consumer"] == "example.com/swift/lib::Examples/Package.swift::dependencies::Shared"
        })
        .unwrap_or_else(|| panic!("{json}"));
    assert_eq!(link["provider"], "example.com/swift/lib::Package.swift");
    assert_eq!(link["level"], "exact");
}

const GREET: &str = "example.com/swift/kit::Sources/Kit/Greeter.swift::greet(name:)";
const HELLO: &str = "example.com/swift/kit::Sources/Kit/Greeter.swift::Greeter.hello(to:loudly:)";

#[test]
fn context_on_a_public_declaration_lists_its_consumers_in_other_repositories() {
    let [kit, ..] = workspace("swift_packages_context");
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
    let [.., app] = workspace("swift_packages_trace_down");
    let from = format!("{APP}typed(greeter:)::greeter.hello#0");
    let down = Engine::open(&app)
        .unwrap()
        .trace(Start::From(&from), Direction::Down, None)
        .unwrap();
    assert_golden("swift-pkg-app.trace-down.txt", &down);
}

fn change_manifests(repos: &[PathBuf; 4]) {
    let [kit, lib, _, app] = repos;
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
    let exports = |repos: &[PathBuf; 4]| repos.each_ref().map(|p| export(p));
    let before = exports(&warm);
    assert_eq!(before, exports(&warm), "byte-identical repeated exports");
    change_manifests(&warm);
    let refreshed = exports(&warm);
    assert_ne!(before, refreshed);
    let cold = workspace("swift_packages_cold");
    change_manifests(&cold);
    assert_eq!(refreshed, exports(&cold));
}
