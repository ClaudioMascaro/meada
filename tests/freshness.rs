//! Freshness (ADR 0004): every query first brings the index up to date with the files on
//! disk, and a refreshed index exports exactly what a cold index of the same files does.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, fixture_repo, set_mtime};
use meada::engine::Engine;

const ORIGIN: Option<&str> = Some("https://example.com/acme/conformance.git");
const WEB: Option<&str> = Some("https://example.com/acme/web.git");

/// Edits a file whose exports others import, adds a file whose definition turns an inferred
/// call in an unrelated file into an ambiguous one, deletes a file, and drops the tsconfig
/// path alias.
fn edit(repo: &Path) {
    let math = fs::read_to_string(repo.join("src/math.ts")).unwrap();
    fs::write(
        repo.join("src/math.ts"),
        math.replace("export const square", "const square"),
    )
    .unwrap();
    fs::write(
        repo.join("src/extra.ts"),
        "export function legacy() {\n  return 2;\n}\n",
    )
    .unwrap();
    fs::remove_file(repo.join("src/widget.jsx")).unwrap();
    fs::write(repo.join("tsconfig.json"), "{}\n").unwrap();
}

fn cold_export(case: &str, fixture: &str, origin: Option<&str>, edit: impl Fn(&Path)) -> String {
    let repo = fixture_repo(case, fixture, origin);
    edit(&repo);
    Engine::open(&repo).unwrap().export().unwrap()
}

#[test]
fn refreshed_export_is_byte_identical_to_a_cold_export_of_the_same_files() {
    let repo = fixture_repo("fresh_incremental", "conformance-ts-js", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    edit(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    assert_ne!(refreshed, before);
    assert_eq!(
        refreshed,
        cold_export("fresh_incremental_cold", "conformance-ts-js", ORIGIN, edit)
    );
}

#[test]
fn an_open_engine_answers_after_files_change() {
    // No aging: every file was just written, so none can be trusted by mtime alone.
    let repo = fixture_repo("fresh_same_engine", "conformance-ts-js", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    engine.export().unwrap();
    edit(&repo);

    let refreshed = engine.export().unwrap();

    assert_eq!(
        refreshed,
        cold_export("fresh_same_engine_cold", "conformance-ts-js", ORIGIN, edit)
    );
}

#[test]
fn unchanged_files_are_not_read_again() {
    // An unreadable file whose mtime and size still match the index cannot be read, so a
    // refresh that succeeds took its facts from the index.
    let view = |repo: &Path| {
        fs::write(
            repo.join("src/view.tsx"),
            "function label() {\n  add();\n}\n",
        )
        .unwrap()
    };
    let repo = fixture_repo("fresh_unread", "ts-basic", WEB);
    age(&repo);
    let engine = Engine::open(&repo).unwrap();
    engine.export().unwrap();
    let math = repo.join("src/math.ts");
    fs::set_permissions(&math, fs::Permissions::from_mode(0o000)).unwrap();
    view(&repo);

    let refreshed = engine.export();

    fs::set_permissions(&math, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        refreshed.unwrap(),
        cold_export("fresh_unread_cold", "ts-basic", WEB, view)
    );
}

/// The two-repository fixture as siblings in one case directory.
fn web_and_api(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(case, "http-web", Some("git@example.com:acme/web.git")),
        fixture_repo(case, "http-api", Some("https://example.com/acme/api.git")),
    )
}

/// Changes links from both sides: the api drops one route handler, deletes a route and adds
/// one; the web consumes the new route.
fn edit_link_ends(web: &Path, api: &Path) {
    let users = api.join("app/api/users/route.ts");
    let route = fs::read_to_string(&users).unwrap();
    let route = route.split("export async function POST").next().unwrap();
    fs::write(&users, route).unwrap();
    fs::remove_file(api.join("app/api/health/route.js")).unwrap();
    fs::create_dir_all(api.join("app/api/teams")).unwrap();
    fs::write(
        api.join("app/api/teams/route.ts"),
        "export async function GET() {\n  return Response.json([]);\n}\n",
    )
    .unwrap();
    fs::write(
        web.join("src/teams.ts"),
        "export async function listTeams() {\n  return fetch(\"https://api.example.com/api/teams\");\n}\n",
    )
    .unwrap();
}

#[test]
fn links_that_touch_changed_files_follow_a_refresh_on_both_sides() {
    let (web, api) = web_and_api("fresh_links");
    age(&web);
    let engine = Engine::open(&web).unwrap();
    let before = engine.export().unwrap();
    // The api has its own index already, refreshed from its side.
    Engine::open(&api).unwrap().export().unwrap();
    edit_link_ends(&web, &api);

    let consumer = engine.export().unwrap();
    let provider = Engine::open(&api).unwrap().export().unwrap();

    let (cold_web, cold_api) = web_and_api("fresh_links_cold");
    edit_link_ends(&cold_web, &cold_api);
    assert_ne!(consumer, before);
    assert_eq!(consumer, Engine::open(&cold_web).unwrap().export().unwrap());
    let (cold_web, cold_api) = web_and_api("fresh_links_cold_api");
    edit_link_ends(&cold_web, &cold_api);
    assert_eq!(provider, Engine::open(&cold_api).unwrap().export().unwrap());
}

#[test]
fn context_refreshes_when_the_span_hash_no_longer_matches() {
    // Same size and the old mtime: only the span hash shows the edit.
    let repo = fixture_repo("fresh_context", "ts-basic", WEB);
    age(&repo);
    let engine = Engine::open(&repo).unwrap();
    engine.context("add", None).unwrap();
    let path = repo.join("src/math.ts");
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let math = fs::read_to_string(&path).unwrap();
    fs::write(&path, math.replace("return a + b;", "return a * b;")).unwrap();
    set_mtime(&path, mtime);

    assert_golden(
        "ts-basic.context-add-edited.txt",
        &engine.context("add", None).unwrap(),
    );
}
