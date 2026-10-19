//! Go packages (spec #25): a `go.mod` of a workspace repository provides
//! `package::go::<module path>`, its `require` entries and local `replace` directives consume
//! it, and an import of one of its packages crosses into the exported symbols there.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, fixture_repo, without_processes};
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
