//! Freshness (ADR 0004): every query first brings the index up to date with the files on
//! disk, and a refreshed index exports exactly what a cold index of the same files does.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, chain, fixture_repo, set_mtime, worker};
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

/// Renames a function others import, adds a module whose function turns an inferred call
/// into an ambiguous one, deletes a module star-imported elsewhere, and declares a source
/// root, which drops the implicit `src` one.
fn edit_python(repo: &Path) {
    let units = fs::read_to_string(repo.join("geometry/units.py")).unwrap();
    fs::write(
        repo.join("geometry/units.py"),
        units.replace("def to_cm", "def to_mm"),
    )
    .unwrap();
    fs::write(repo.join("stars/extra.py"), "def legacy():\n    return 2\n").unwrap();
    fs::remove_file(repo.join("helpers.py")).unwrap();
    fs::write(
        repo.join("pyproject.toml"),
        "[tool.setuptools.packages.find]\nwhere = [\"lib\"]\n",
    )
    .unwrap();
}

#[test]
fn python_refreshed_export_is_byte_identical_to_a_cold_export_of_the_same_files() {
    let repo = fixture_repo("fresh_python", "conformance-python", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    edit_python(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    assert_ne!(refreshed, before);
    assert_eq!(
        refreshed,
        cold_export(
            "fresh_python_cold",
            "conformance-python",
            ORIGIN,
            edit_python
        )
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

/// Repositories whose contracts read values from other files: path constants (`http-paths`,
/// with its provider `http-api`), client wrappers (`http-wrappers`) and Express mounts
/// (`http-express`, whose inline handlers are named by the mounts above them).
fn values(case: &str) -> [PathBuf; 3] {
    fixture_repo(case, "http-api", Some("https://example.com/acme/api.git"));
    [
        fixture_repo(
            case,
            "http-paths",
            Some("https://example.com/acme/paths.git"),
        ),
        fixture_repo(
            case,
            "http-wrappers",
            Some("https://example.com/acme/admin.git"),
        ),
        fixture_repo(
            case,
            "http-express",
            Some("https://example.com/acme/backend.git"),
        ),
    ]
}

/// Edits only the files that hold values: a constant other files import, a wrapper's default
/// method, and the mount prefix that names the inline handlers of another file.
fn edit_values([paths, admin, backend]: &[PathBuf; 3]) {
    let replace = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    replace(
        paths.join("src/routes.ts"),
        "\"/api/users\"",
        "\"/api/v2/users\"",
    );
    replace(
        admin.join("src/http.ts"),
        "method = \"GET\"",
        "method = \"POST\"",
    );
    replace(backend.join("src/server.ts"), "\"/api\"", "\"/v1\"");
}

#[test]
fn contracts_follow_values_edited_in_other_files() {
    let repos = values("fresh_values");
    let exports = |repos: &[PathBuf; 3]| -> Vec<String> {
        repos
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    repos.iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_values(&repos);

    let refreshed = exports(&repos);

    let cold = values("fresh_values_cold");
    edit_values(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

type Chain = (PathBuf, PathBuf, PathBuf);

/// Edits, adds and deletes files in each repository of the chain: the web adds a consumer and
/// deletes the file of two others, the gateway moves a route to another method, the catalog
/// deletes a route its peers link to and edits the end of its cycle.
fn edit_chain((web, gateway, catalog): &Chain) {
    let replace = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    fs::write(
        web.join("src/archive.ts"),
        "import { request } from \"./api/client\";\n\n\
         export function archive(id: string) {\n  \
           return request(`/api/orders/${id}`, \"PATCH\");\n}\n",
    )
    .unwrap();
    fs::remove_file(web.join("src/status.ts")).unwrap();
    replace(gateway.join("src/routes.ts"), "api.delete(", "api.patch(");
    fs::remove_file(catalog.join("app/api/health/route.ts")).unwrap();
    replace(
        catalog.join("lib/products.ts"),
        "return { id };",
        "return { id, found: true };",
    );
}

#[test]
fn a_refreshed_chain_exports_what_a_cold_chain_does() {
    let exports = |(web, gateway, catalog): &Chain| -> Vec<String> {
        [web, gateway, catalog]
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let repos = chain("fresh_chain");
    [&repos.0, &repos.1, &repos.2].iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_chain(&repos);

    let refreshed = exports(&repos);

    let cold = chain("fresh_chain_cold");
    edit_chain(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

/// Moves the gateway's write of `orders` to `listOrders` and makes the worker a writer too.
fn edit_resources(gateway: &Path) {
    let edit = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    edit(
        gateway.join("meada.toml"),
        "src/orders.ts::cancelOrder",
        "src/orders.ts::listOrders",
    );
    edit(
        worker(gateway).join("meada.toml"),
        "role = \"read\"",
        "role = \"read-write\"",
    );
}

/// Edits, adds and deletes `.py` files of the inventory and deletes a route of the storefront:
/// a route path changes, a module with a consumer on the declared host appears, the jobs
/// module goes.
fn edit_xlang((storefront, inventory): &(PathBuf, PathBuf)) {
    let main = inventory.join("inventory/main.py");
    let src = fs::read_to_string(&main).unwrap();
    assert!(src.contains("\"/stock/{sku}\""));
    fs::write(
        &main,
        src.replace("\"/stock/{sku}\"", "\"/stock/{sku}/level\""),
    )
    .unwrap();
    fs::write(
        inventory.join("inventory/report.py"),
        "import requests\n\n\ndef report():\n    return requests.post(\"https://storefront.example.com/api/audit\")\n",
    )
    .unwrap();
    fs::remove_file(inventory.join("inventory/jobs.py")).unwrap();
    fs::remove_file(storefront.join("app/api/promotions/route.ts")).unwrap();
}

#[test]
fn a_refreshed_cross_language_pair_exports_what_a_cold_pair_does() {
    let exports = |(storefront, inventory): &(PathBuf, PathBuf)| -> Vec<String> {
        [storefront, inventory]
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let repos = common::xlang("fresh_xlang");
    [&repos.0, &repos.1].iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_xlang(&repos);

    let refreshed = exports(&repos);

    let cold = common::xlang("fresh_xlang_cold");
    edit_xlang(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

#[test]
fn an_edited_meada_toml_relinks_resources_like_a_cold_chain() {
    let exports = |gateway: &Path| -> Vec<String> {
        [gateway.to_path_buf(), worker(gateway)]
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let (_, gateway, _) = chain("fresh_resources");
    let before = exports(&gateway);
    edit_resources(&gateway);

    let refreshed = exports(&gateway);

    let (_, cold, _) = chain("fresh_resources_cold");
    edit_resources(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}
