//! Cargo packages (spec #30): a named `Cargo.toml` of a workspace repository provides
//! `package::cargo::<name>`, the dependency entries of `Cargo.toml` consume it, and a `use` of
//! the crate crosses into its public items, through their re-exports.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, fixture_repo, without_processes};
use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

/// The shared crates and their consumer as siblings: `web` names `lib` as its peer.
fn pair(case: &str) -> (PathBuf, PathBuf) {
    let repo = |name: &str| {
        let origin = format!("https://example.com/cargo/{name}.git");
        fixture_repo(case, &format!("cargo-{name}"), Some(&origin))
    };
    (repo("lib"), repo("web"))
}

fn export(repo: &Path) -> String {
    Engine::open(repo).unwrap().export().unwrap()
}

const LIB: &str = "example.com/cargo/lib::";
const WEB: &str = "example.com/cargo/web::";
const GREET: &str = "example.com/cargo/lib::core/src/text.rs::greet";
const WELCOME_GREET: &str = "example.com/cargo/web::app/src/main.rs::welcome::greet#0";

#[test]
fn crates_and_their_use_link_to_the_crate_in_the_workspace() {
    let (lib, web) = pair("cargo_links");

    assert_golden("cargo-web.export.json", &without_processes(&export(&web)));
    assert_golden("cargo-lib.export.json", &without_processes(&export(&lib)));
}

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

#[test]
fn each_dependency_form_links_to_the_crate_it_names() {
    let (_, web) = pair("cargo_forms");
    let json: Value = serde_json::from_str(&export(&web)).unwrap();
    let at = |site: &str| links(&json, &format!("{WEB}{site}"));
    let core = format!("{LIB}core/Cargo.toml");
    let extras = format!("{LIB}extras/Cargo.toml");

    // By version, with the requirement as the range and the provider's own version.
    let version = "package::cargo::shared-core ^1.1 (local 1.2.0), range unchecked";
    assert_eq!(
        at("app/Cargo.toml::dependencies::shared-core"),
        [(
            "example.com/cargo/web::app/Cargo.toml::dependencies::shared-core",
            core.as_str(),
            "exact",
            version
        )]
    );
    // By git: the repository whose identity is the normalized URL, under the real crate name.
    let git = "package::cargo::shared-extras none (local 0.3.0), range unchecked";
    assert_eq!(
        at("app/Cargo.toml::dependencies::extras"),
        [(
            "example.com/cargo/web::app/Cargo.toml::dependencies::extras",
            extras.as_str(),
            "exact",
            git
        )]
    );
    // `workspace = true`: the requirement of `[workspace.dependencies]`; a renamed one by
    // version: the real name behind the key.
    let inherited = "package::cargo::shared-core 1.0 (local 1.2.0), range unchecked";
    let renamed = "package::cargo::shared-core 1 (local 1.2.0), range unchecked";
    let found: Vec<_> = at("worker/Cargo.toml")
        .into_iter()
        .map(|(c, p, l, r)| (c.strip_prefix(WEB).unwrap(), p, l, r))
        .collect();
    assert_eq!(
        found,
        [
            (
                "worker/Cargo.toml::dependencies::kit",
                core.as_str(),
                "exact",
                renamed
            ),
            (
                "worker/Cargo.toml::dependencies::shared-core",
                core.as_str(),
                "exact",
                inherited
            ),
        ]
    );
    // A path that lands in a sibling workspace repository links to the crate at that path.
    assert_eq!(
        at("app/Cargo.toml::dependencies::away"),
        [(
            "example.com/cargo/web::app/Cargo.toml::dependencies::away",
            extras.as_str(),
            "exact",
            git
        )]
    );
    // A path that lands in no workspace repository, and a crate outside the workspace, link to
    // nothing: `Cargo.lock` and crates.io are never read.
    let manifest = at("app/Cargo.toml");
    assert_eq!(manifest.len(), 3, "{manifest:?}");
}

#[test]
fn a_use_of_the_crate_reaches_the_item_through_its_re_exports() {
    let (_, web) = pair("cargo_use");
    let json: Value = serde_json::from_str(&export(&web)).unwrap();
    let at = |caller: &str| links(&json, &format!("{WEB}{caller}"));
    let exact =
        |range: &str| format!("package::cargo::{range}, range unchecked; import crate name");

    // A `pub use text::{greet, shout}` of a private module, under the key and under a rename,
    // and a name reached through a `pub mod` and an `impl`.
    let shared = exact("shared-core ^1.1 (local 1.2.0)");
    for (caller, target, reason) in [
        (
            "app/src/main.rs::welcome::",
            "core/src/text.rs::greet",
            &shared,
        ),
        (
            "app/src/main.rs::connect::",
            "core/src/api.rs::Client.create",
            &shared,
        ),
        (
            "app/src/main.rs::lift::",
            "extras/src/lib.rs::boost",
            &exact("shared-extras none (local 0.3.0)"),
        ),
        (
            "app/src/main.rs::faraway::",
            "extras/src/lib.rs::boost",
            &exact("shared-extras none (local 0.3.0)"),
        ),
        (
            "worker/src/lib.rs::run::",
            "core/src/text.rs::greet",
            &exact("shared-core 1.0 (local 1.2.0)"),
        ),
        (
            "worker/src/lib.rs::loud::",
            "core/src/text.rs::shout",
            &exact("shared-core 1 (local 1.2.0)"),
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
    // A path that lands in no workspace repository, a crate outside the workspace, and an item
    // behind a private module add nothing.
    for caller in ["haunt", "encode", "peek"] {
        assert!(
            at(&format!("app/src/main.rs::{caller}::")).is_empty(),
            "{caller}"
        );
    }
}

#[test]
fn a_path_dependency_inside_the_repository_stays_inside_it() {
    let (lib, _) = pair("cargo_inner");
    let json: Value = serde_json::from_str(&export(&lib)).unwrap();

    let found = links(&json, &format!("{LIB}app/Cargo.toml"));
    let ends: Vec<_> = found.iter().map(|(c, p, l, _)| (*c, *p, *l)).collect();
    assert_eq!(
        ends,
        [
            (
                "example.com/cargo/lib::app/Cargo.toml::dependencies::shared-core",
                "example.com/cargo/lib::core/Cargo.toml",
                "exact"
            ),
            (
                "example.com/cargo/lib::app/Cargo.toml::dependencies::shared-extras",
                "example.com/cargo/lib::extras/Cargo.toml",
                "exact"
            ),
        ]
    );
}

#[test]
fn context_on_a_public_item_lists_its_consumers_in_another_repository() {
    let (lib, _) = pair("cargo_context");

    let found = Engine::open(&lib).unwrap().context(GREET, None).unwrap();

    assert_golden("cargo-lib.context-greet.txt", &found);
}

#[test]
fn trace_down_crosses_the_package_hop_and_onward() {
    let (_, web) = pair("cargo_trace_down");
    let engine = Engine::open(&web).unwrap();

    let down = engine
        .trace(Start::From(WELCOME_GREET), Direction::Down, None)
        .unwrap();

    assert_golden("cargo-web.trace-down.txt", &down);
}

/// Bumps the library, narrows the requirement of `app`, moves the workspace requirement of
/// `worker` and renames the git dependency's key.
fn edit(lib: &Path, web: &Path) {
    let replace = |repo: &Path, file: &str, from: &str, to: &str| {
        let text = fs::read_to_string(repo.join(file)).unwrap();
        assert!(text.contains(from), "{file} lacks {from}");
        fs::write(repo.join(file), text.replace(from, to)).unwrap();
    };
    replace(lib, "core/Cargo.toml", "1.2.0", "1.3.0");
    replace(web, "app/Cargo.toml", "\"^1.1\"", "\"~1.3\"");
    replace(
        web,
        "Cargo.toml",
        "shared-core = \"1.0\"",
        "shared-core = \"1.3\"",
    );
    replace(web, "app/Cargo.toml", "extras = {", "boosted = {");
}

#[test]
fn a_remote_dependency_links_only_to_the_repository_of_its_url() {
    let (_, web) = pair("cargo_remote_elsewhere");
    let manifest = web.join("app/Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    // The same crate name, from a remote that is not in the workspace.
    fs::write(
        &manifest,
        text.replace("cargo/lib.git", "cargo/elsewhere.git"),
    )
    .unwrap();

    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    assert!(links(&json, &format!("{WEB}app/Cargo.toml::dependencies::extras")).is_empty());
    assert!(links(&json, &format!("{WEB}app/src/main.rs::lift::")).is_empty());
}

#[test]
fn an_edited_cargo_manifest_relinks_like_a_cold_export() {
    let (lib, web) = pair("cargo_fresh");
    age(lib.parent().unwrap());
    let both = |lib: &Path, web: &Path| [export(web), export(lib)];
    let before = both(&lib, &web);
    edit(&lib, &web);

    let refreshed = both(&lib, &web);

    let (cold_lib, cold_web) = pair("cargo_fresh_cold");
    edit(&cold_lib, &cold_web);
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, both(&cold_lib, &cold_web));
    // The renamed key no longer matches the `use`: that call crosses no more.
    let json: Value = serde_json::from_str(&refreshed[0]).unwrap();
    assert!(links(&json, &format!("{WEB}app/src/main.rs::lift::")).is_empty());
}

/// Replaces `from` by `to` in `file` of `repo`, which must hold it.
fn replace(repo: &Path, file: &str, from: &str, to: &str) {
    let text = fs::read_to_string(repo.join(file)).unwrap();
    assert!(text.contains(from), "{file} lacks {from}");
    fs::write(repo.join(file), text.replace(from, to)).unwrap();
}

/// The `(provider, level)` pairs of the links of `json` whose consumer starts with `site`.
fn ends<'j>(json: &'j Value, site: &str) -> Vec<(&'j str, &'j str)> {
    let found = links(json, site);
    found.into_iter().map(|(_, p, l, _)| (p, l)).collect()
}

#[test]
fn a_version_dependency_on_a_crate_two_repositories_provide_is_ambiguous() {
    let (_, web) = pair("cargo_ambiguous");
    fixture_repo(
        "cargo_ambiguous",
        "cargo-fork",
        Some("https://example.com/cargo/fork.git"),
    );
    let peers = fs::read_to_string(web.join("meada.toml")).unwrap();
    let peer = "\n[[peer]]\nrepo = \"example.com/cargo/fork\"\n";
    fs::write(web.join("meada.toml"), peers + peer).unwrap();

    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    let both = |from: &str| ends(&json, &format!("{WEB}{from}"));
    let fork = "example.com/cargo/fork::Cargo.toml";
    let core = format!("{LIB}core/Cargo.toml");
    assert_eq!(
        both("app/Cargo.toml::dependencies::shared-core"),
        [(fork, "ambiguous"), (core.as_str(), "ambiguous")]
    );
    assert_eq!(
        both("app/src/main.rs::welcome::"),
        [
            ("example.com/cargo/fork::src/lib.rs::greet", "ambiguous"),
            (
                "example.com/cargo/lib::core/src/text.rs::greet",
                "ambiguous"
            ),
        ]
    );
    let said = links(
        &json,
        &format!("{WEB}app/Cargo.toml::dependencies::shared-core"),
    );
    assert!(
        said.iter()
            .all(|(.., r)| r.starts_with("2 providers fit; "))
    );
    // The git dependency still names one repository.
    assert_eq!(
        both("app/Cargo.toml::dependencies::extras"),
        [("example.com/cargo/lib::extras/Cargo.toml", "exact")]
    );
}

#[test]
fn an_item_behind_a_private_module_is_reached_once_the_module_is_public() {
    let (lib, web) = pair("cargo_private");
    let peek = format!("{WEB}app/src/main.rs::peek::");
    let before: Value = serde_json::from_str(&export(&web)).unwrap();
    assert!(links(&before, &peek).is_empty());

    replace(&lib, "core/src/lib.rs", "mod text;", "pub mod text;");
    let after: Value = serde_json::from_str(&export(&web)).unwrap();

    assert_eq!(
        ends(&after, &peek),
        [("example.com/cargo/lib::core/src/text.rs::greet", "exact")]
    );
}

#[test]
fn a_library_name_is_what_a_use_writes_unless_the_key_renames_the_crate() {
    let (lib, web) = pair("cargo_lib_name");
    replace(
        &lib,
        "core/Cargo.toml",
        "edition",
        "[lib]\nname = \"core_lib\"\n#",
    );
    replace(&web, "app/src/main.rs", "shared_core::", "core_lib::");

    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    let at = |caller: &str| ends(&json, &format!("{WEB}{caller}"));
    let greet = ("example.com/cargo/lib::core/src/text.rs::greet", "exact");
    assert_eq!(at("app/src/main.rs::welcome::"), [greet]);
    // `use shared_core` in `worker` no longer names the library; its renamed key still does.
    assert!(at("worker/src/lib.rs::run::").is_empty());
    let shout = ("example.com/cargo/lib::core/src/text.rs::shout", "exact");
    assert_eq!(at("worker/src/lib.rs::loud::"), [shout]);
}

#[test]
fn a_version_inherited_from_the_workspace_is_the_local_version() {
    let (lib, web) = pair("cargo_version_workspace");
    replace(
        &lib,
        "Cargo.toml",
        "\"app\"]",
        "\"app\"]\n\n[workspace.package]\nversion = \"1.7.0\"",
    );
    replace(
        &lib,
        "core/Cargo.toml",
        "version = \"1.2.0\"",
        "version.workspace = true",
    );

    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    let site = format!("{WEB}app/Cargo.toml::dependencies::shared-core");
    let reason = "package::cargo::shared-core ^1.1 (local 1.7.0), range unchecked";
    let found: Vec<_> = links(&json, &site).iter().map(|l| l.3).collect();
    assert_eq!(found, [reason]);
}
