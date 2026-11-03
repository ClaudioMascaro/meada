//! Go packages (spec #25): a `go.mod` of a workspace repository provides
//! `package::go::<module path>`, its `require` entries and local `replace` directives consume
//! it, and an import of one of its packages crosses into the exported symbols there.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, fixture_repo, git, without_processes};
use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

/// The shared module and its consumer as siblings: `web` names `lib` as its peer.
fn pair(case: &str) -> (PathBuf, PathBuf) {
    let repo = |name: &str| {
        let origin = format!("https://example.com/gopkg/{name}.git");
        fixture_repo(case, &format!("gopkg-{name}"), Some(&origin))
    };
    (repo("lib"), repo("web"))
}

fn export(repo: &Path) -> String {
    Engine::open(repo).unwrap().export().unwrap()
}

const LIB: &str = "example.com/gopkg/lib::";
const WEB: &str = "example.com/gopkg/web::";
const GREET: &str = "example.com/gopkg/lib::text/text.go::Greet";
const WELCOME_GREET: &str = "example.com/gopkg/web::main.go::welcome::k.Greet#0";

/// The `(consumer, provider, level, reason)` of every link of `json` whose consumer starts
/// with `site`.
fn links<'j>(json: &'j Value, site: &str) -> Vec<(&'j str, &'j str, &'j str, &'j str)> {
    (json["links"].as_array().into_iter().flatten())
        .filter(|l| l["consumer"].as_str().unwrap().starts_with(site))
        .map(|l| {
            let at = |k: &str| l[k].as_str().unwrap();
            (at("consumer"), at("provider"), at("level"), at("reason"))
        })
        .collect()
}

/// The `(provider, level)` pairs of the links of `json` whose consumer starts with `site`.
fn ends<'j>(json: &'j Value, site: &str) -> Vec<(&'j str, &'j str)> {
    let found = links(json, site);
    found.into_iter().map(|(_, p, l, _)| (p, l)).collect()
}

#[test]
fn modules_and_their_imports_link_to_the_module_in_the_workspace() {
    let (lib, web) = pair("gopkg_links");

    assert_golden("gopkg-web.export.json", &without_processes(&export(&web)));
    assert_golden("gopkg-lib.export.json", &without_processes(&export(&lib)));
}

#[test]
fn a_require_and_a_local_replace_link_to_the_module_they_name() {
    let (_, web) = pair("gopkg_forms");
    let json: Value = serde_json::from_str(&export(&web)).unwrap();
    let at = |site: &str| links(&json, &format!("{WEB}{site}"));

    // A `require`: the version as the range, `(local none)` since a `go.mod` has no version.
    let kit = "package::go::example.com/shared/kit v1.2.0 (local none), range unchecked";
    assert_eq!(
        at("go.mod::require::example.com/shared/kit"),
        [(
            "example.com/gopkg/web::go.mod::require::example.com/shared/kit",
            "example.com/gopkg/lib::go.mod",
            "exact",
            kit
        )]
    );
    // A `require` with a `replace` to a directory of a sibling workspace repository.
    let extras = "package::go::example.com/shared/extras v0.0.0 (local none), range unchecked";
    assert_eq!(
        at("worker/go.mod::require::example.com/shared/extras"),
        [(
            "example.com/gopkg/web::worker/go.mod::require::example.com/shared/extras",
            "example.com/gopkg/lib::extras/go.mod",
            "exact",
            extras
        )]
    );
    // A module outside the workspace links to nothing: `go.sum` and the proxy are never read.
    assert!(at("go.mod::require::example.com/outside/lib").is_empty());
}

#[test]
fn an_import_reaches_the_exported_symbol_of_the_package_directory() {
    let (_, web) = pair("gopkg_imports");
    let json: Value = serde_json::from_str(&export(&web)).unwrap();
    let at = |caller: &str| links(&json, &format!("{WEB}{caller}"));
    let said = |range: &str| format!("package::go::{range}, range unchecked; import module path");

    let kit = said("example.com/shared/kit v1.2.0 (local none)");
    let extras = said("example.com/shared/extras v0.0.0 (local none)");
    for (caller, target, reason) in [
        // Under a local name.
        ("main.go::welcome::", "text/text.go::Greet", &kit),
        ("main.go::connect::", "api/client.go::NewClient", &kit),
        // Through a `replace` to a directory outside the repository.
        (
            "worker/worker.go::Run::",
            "extras/extras.go::Boost",
            &extras,
        ),
    ] {
        let found: Vec<_> = at(caller)
            .into_iter()
            .map(|(_, p, l, r)| (p, l, r))
            .collect();
        let target = format!("{LIB}{target}");
        assert_eq!(
            found,
            [(target.as_str(), "exact", reason.as_str())],
            "{caller}"
        );
    }
    // A directory the provider lacks, a module outside the workspace, a local that hides the
    // import, a name only a test file or a file built for another target defines, a directory of
    // a nested module of the provider and a name two files of the directory define add nothing.
    let none = [
        "peek", "encode", "shadowed", "testOnly", "windows", "nested", "dup",
    ];
    for caller in none {
        assert!(at(&format!("main.go::{caller}::")).is_empty(), "{caller}");
    }
}

#[test]
fn context_on_an_exported_function_lists_its_consumers_in_another_repository() {
    let (lib, _) = pair("gopkg_context");

    let found = Engine::open(&lib).unwrap().context(GREET, None).unwrap();

    assert_golden("gopkg-lib.context-greet.txt", &found);
}

#[test]
fn trace_down_crosses_the_package_hop_and_onward() {
    let (_, web) = pair("gopkg_trace_down");
    let engine = Engine::open(&web).unwrap();

    let down = engine
        .trace(Start::From(WELCOME_GREET), Direction::Down, None)
        .unwrap();

    assert_golden("gopkg-web.trace-down.txt", &down);
}

/// Bumps the required version of the kit and renames the required module of `worker`.
fn edit(web: &Path) {
    let replace = |file: &str, from: &str, to: &str| {
        let text = fs::read_to_string(web.join(file)).unwrap();
        assert!(text.contains(from), "{file} lacks {from}");
        fs::write(web.join(file), text.replace(from, to)).unwrap();
    };
    replace("go.mod", "kit v1.2.0", "kit v1.3.0");
    replace("worker/go.mod", "extras v0.0.0", "extras v0.1.0");
    replace(
        "worker/go.mod",
        "=> ../../gopkg-lib/extras",
        "=> ./vendored",
    );
}

#[test]
fn an_edited_go_mod_relinks_like_a_cold_export() {
    let (lib, web) = pair("gopkg_fresh");
    age(lib.parent().unwrap());
    let both = |lib: &Path, web: &Path| [export(web), export(lib)];
    let before = both(&lib, &web);
    edit(&web);

    let refreshed = both(&lib, &web);

    let (cold_lib, cold_web) = pair("gopkg_fresh_cold");
    edit(&cold_web);
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, both(&cold_lib, &cold_web));
    // The replace now names a directory of its own repository: the call crosses no more.
    let json: Value = serde_json::from_str(&refreshed[0]).unwrap();
    assert!(links(&json, &format!("{WEB}worker/worker.go::Run::")).is_empty());
}

#[test]
fn a_require_on_a_module_two_repositories_provide_is_ambiguous() {
    let (_, web) = pair("gopkg_ambiguous");
    fixture_repo(
        "gopkg_ambiguous",
        "gopkg-fork",
        Some("https://example.com/gopkg/fork.git"),
    );
    let peers = fs::read_to_string(web.join("meada.toml")).unwrap();
    let peer = "\n[[peer]]\nrepo = \"example.com/gopkg/fork\"\n";
    fs::write(web.join("meada.toml"), peers + peer).unwrap();

    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    let both = |from: &str| ends(&json, &format!("{WEB}{from}"));
    assert_eq!(
        both("go.mod::require::example.com/shared/kit"),
        [
            ("example.com/gopkg/fork::go.mod", "ambiguous"),
            ("example.com/gopkg/lib::go.mod", "ambiguous")
        ]
    );
    assert_eq!(
        both("main.go::welcome::"),
        [
            ("example.com/gopkg/fork::text/text.go::Greet", "ambiguous"),
            ("example.com/gopkg/lib::text/text.go::Greet", "ambiguous"),
        ]
    );
}

/// Replaces `from` by `to` in `file` of `repo`, which must hold it.
fn replace(repo: &Path, file: &str, from: &str, to: &str) {
    let text = fs::read_to_string(repo.join(file)).unwrap();
    assert!(text.contains(from), "{file} lacks {from}");
    fs::write(repo.join(file), text.replace(from, to)).unwrap();
}

#[test]
fn a_replace_by_another_module_leaves_the_module_out_of_the_workspace() {
    let (_, web) = pair("gopkg_replaced");
    let go_mod = fs::read_to_string(web.join("go.mod")).unwrap();
    let fork = "\nreplace example.com/shared/kit => example.com/fork/kit v1.0.0\n";
    fs::write(web.join("go.mod"), go_mod + fork).unwrap();

    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    assert!(links(&json, &format!("{WEB}go.mod")).is_empty());
    assert!(links(&json, &format!("{WEB}main.go")).is_empty());
    // The module `worker` requires is still the sibling's.
    assert_eq!(links(&json, &format!("{WEB}worker/")).len(), 2);
}

#[test]
fn a_module_path_without_a_dot_links_through_its_local_replace() {
    let (lib, web) = pair("gopkg_dotless");
    replace(&lib, "go.mod", "example.com/shared/kit", "kit");
    replace(
        &web,
        "go.mod",
        "example.com/shared/kit v1.2.0",
        "kit v1.2.0",
    );
    replace(&web, "main.go", "example.com/shared/kit/", "kit/");
    let go_mod = fs::read_to_string(web.join("go.mod")).unwrap();
    fs::write(
        web.join("go.mod"),
        go_mod + "\nreplace kit => ../gopkg-lib\n",
    )
    .unwrap();

    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    let at = |site: &str| links(&json, &format!("{WEB}{site}"));
    let reason = "package::go::kit v1.2.0 (local none), range unchecked";
    assert_eq!(
        at("go.mod::require::kit"),
        [(
            "example.com/gopkg/web::go.mod::require::kit",
            "example.com/gopkg/lib::go.mod",
            "exact",
            reason
        )]
    );
    let found: Vec<_> = (at("main.go::welcome::").into_iter())
        .map(|(_, p, l, r)| (p, l, r.to_string()))
        .collect();
    assert_eq!(
        found,
        [(
            "example.com/gopkg/lib::text/text.go::Greet",
            "exact",
            format!("{reason}; import module path")
        )]
    );
}

/// The consumer that replaces `example.com/orig/lib` by the fork, and the two repositories that
/// declare it, as siblings; the consumer (`example.com/gopkg/app`) is the one returned.
fn replaced(case: &str) -> PathBuf {
    let repo = |fixture: &str, origin: &str| fixture_repo(case, fixture, Some(origin));
    repo("gopkg-replace-orig", "https://example.com/orig/lib.git");
    repo("gopkg-replace-fork", "https://example.com/fork/lib.git");
    repo("gopkg-replace", "https://example.com/gopkg/app.git")
}

const APP: &str = "example.com/gopkg/app::";

#[test]
fn a_replace_by_a_module_the_workspace_holds_keeps_only_that_provider() {
    let app = replaced("gopkg_replace_held");

    let json: Value = serde_json::from_str(&export(&app)).unwrap();

    assert_eq!(
        ends(&json, &format!("{APP}go.mod::require::")),
        [("example.com/fork/lib::go.mod", "exact")]
    );
    assert_eq!(
        ends(&json, &format!("{APP}main.go::main::")),
        [("example.com/fork/lib::text/text.go::Greet", "exact")]
    );
    assert_golden(
        "gopkg-replace.export.json",
        &without_processes(&export(&app)),
    );
}

#[test]
fn without_the_replace_both_providers_stay_ambiguous() {
    let app = replaced("gopkg_replace_absent");
    replace(
        &app,
        "go.mod",
        "\nreplace example.com/orig/lib => example.com/fork/lib v1.0.0-fork.1\n",
        "",
    );

    let json: Value = serde_json::from_str(&export(&app)).unwrap();

    assert_eq!(
        ends(&json, &format!("{APP}go.mod::require::")),
        [
            ("example.com/fork/lib::go.mod", "ambiguous"),
            ("example.com/orig/lib::go.mod", "ambiguous")
        ]
    );
    assert_eq!(
        ends(&json, &format!("{APP}main.go::main::")),
        [
            ("example.com/fork/lib::text/text.go::Greet", "ambiguous"),
            ("example.com/orig/lib::text/text.go::Greet", "ambiguous"),
        ]
    );
}

#[test]
fn a_replace_whose_repository_declares_another_module_path_links_to_nothing() {
    let app = replaced("gopkg_replace_other_path");
    let fork = app.parent().unwrap().join("gopkg-replace-fork");
    replace(
        &fork,
        "go.mod",
        "example.com/orig/lib",
        "example.com/fork/lib",
    );
    git(&fork, &["commit", "-q", "-am", "own path"]);

    let json: Value = serde_json::from_str(&export(&app)).unwrap();

    assert!(links(&json, &format!("{APP}go.mod")).is_empty());
    assert!(links(&json, &format!("{APP}main.go")).is_empty());
}

#[test]
fn a_replace_by_a_module_no_repository_holds_keeps_the_dependency_out() {
    let app = replaced("gopkg_replace_unheld");
    replace(
        &app,
        "go.mod",
        "example.com/fork/lib",
        "example.com/elsewhere/lib",
    );

    let json: Value = serde_json::from_str(&export(&app)).unwrap();

    assert!(links(&json, &format!("{APP}go.mod")).is_empty());
    assert!(links(&json, &format!("{APP}main.go")).is_empty());
}
