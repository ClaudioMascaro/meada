//! Freshness (ADR 0004): every query first brings the index up to date with the files on
//! disk, and a refreshed index exports exactly what a cold index of the same files does.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{
    age, assert_golden, chain, documented_chain, drift_chain, fixture_repo, set_mtime, worker,
};
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

/// Renames the topic the gateway produces through its wrapper, in the imported constant, and
/// the topics the conformance fixture builds from a constant (spec #20).
fn edit_topics(gateway: &Path, conformance: &Path) {
    let replace = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    replace(
        gateway.join("src/topics.ts"),
        "orders.created",
        "orders.placed",
    );
    replace(
        conformance.join("src/messages/topics.ts"),
        "\"orders\"",
        "\"sales\"",
    );
}

#[test]
fn channels_follow_an_edited_topic_constant() {
    let topics = |case: &str| {
        let (_, gateway, _) = chain(case);
        let conformance = fixture_repo(&format!("{case}_conformance"), "conformance-ts-js", ORIGIN);
        [gateway.clone(), worker(&gateway), conformance]
    };
    let exports = |repos: &[PathBuf; 3]| -> Vec<String> {
        (repos.iter())
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let repos = topics("fresh_topics");
    repos.iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_topics(&repos[0], &repos[2]);

    let refreshed = exports(&repos);

    let cold = topics("fresh_topics_cold");
    edit_topics(&cold[0], &cold[2]);
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

/// Moves the documented-only route of the gateway's OpenAPI document, adds a JSON document with
/// a route of its own, and deletes the invalid one.
fn edit_documents(gateway: &Path) {
    let doc = fs::read_to_string(gateway.join("openapi.yaml")).unwrap();
    let doc = doc.replace("/api/shipments/{id}", "/api/parcels/{id}");
    fs::write(gateway.join("openapi.yaml"), doc).unwrap();
    fs::write(
        gateway.join("extra.openapi.json"),
        r#"{"openapi": "3.0.3", "paths": {"/api/returns": {"post": {}}}}"#,
    )
    .unwrap();
    fs::remove_file(gateway.join("broken.openapi.yaml")).unwrap();
}

/// Writes the listed path that did not exist, and takes the gitignored document and the
/// oddly named one off the list.
fn edit_listed_documents(gateway: &Path) {
    fs::write(
        gateway.join("docs/missing.yaml"),
        "openapi: 3.0.3\npaths:\n  /api/returns:\n    post: {}\n",
    )
    .unwrap();
    let toml = fs::read_to_string(gateway.join("meada.toml")).unwrap();
    let toml = toml.replace(
        "\"generated/openapi.yaml\", \"docs/api-contract.yaml\", ",
        "",
    );
    fs::write(gateway.join("meada.toml"), toml).unwrap();
}

/// Drops the gateway's `[serve]` table: its documents stop counting.
fn drop_serve(gateway: &Path) {
    let toml = fs::read_to_string(gateway.join("meada.toml")).unwrap();
    let serve = ["[serve]", "hosts = ", "documents = "];
    let kept = toml
        .lines()
        .filter(|l| !serve.iter().any(|k| l.starts_with(k)));
    let toml = kept.map(|l| format!("{l}\n")).collect::<String>();
    fs::write(gateway.join("meada.toml"), toml).unwrap();
}

/// The web's client wrapper returns the response instead of its parsed body: its caller, in an
/// unchanged file, reads no field any more.
fn return_the_response(web: &Path) {
    let path = web.join("src/api/body.ts");
    let src = fs::read_to_string(&path).unwrap();
    fs::write(&path, src.replace("return res.json();", "return res;")).unwrap();
}

#[test]
fn an_edited_client_wrapper_changes_its_callers_drift_like_a_cold_chain() {
    let (web, _, _) = drift_chain("fresh_drift");
    age(web.parent().unwrap());
    let before = Engine::open(&web).unwrap().export().unwrap();
    return_the_response(&web);

    let refreshed = Engine::open(&web).unwrap().export().unwrap();

    let (cold, _, _) = drift_chain("fresh_drift_cold");
    return_the_response(&cold);
    assert!(before.contains("\"[].customer\""));
    assert!(!refreshed.contains("\"[].customer\""));
    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
}

/// The web's shared `Entity` type renames `id`: the consumers that extend it, in another file
/// and unchanged, declare `key` instead.
fn rename_a_shared_field(web: &Path) {
    let path = web.join("src/api/types.ts");
    let src = fs::read_to_string(&path).unwrap();
    let edited = src.replace("  id: string;", "  key: string;");
    assert_ne!(src, edited);
    fs::write(&path, edited).unwrap();
}

#[test]
fn an_edited_shared_type_changes_the_drift_of_its_users_like_a_cold_chain() {
    let (web, _, _) = drift_chain("fresh_type");
    age(web.parent().unwrap());
    let before = Engine::open(&web).unwrap().export().unwrap();
    rename_a_shared_field(&web);

    let refreshed = Engine::open(&web).unwrap().export().unwrap();

    let (cold, _, _) = drift_chain("fresh_type_cold");
    rename_a_shared_field(&cold);
    assert!(!before.contains("\"items[].key\": \"exact\""));
    assert!(refreshed.contains("\"items[].key\": \"exact\""));
    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
}

/// The catalog's zod schema drops `title`, which a consumer in the web app reads.
fn drop_a_schema_field(catalog: &Path) {
    let path = catalog.join("lib/schemas.ts");
    let src = fs::read_to_string(&path).unwrap();
    let edited = src.replace("  title: z.string().min(1),\n", "");
    assert_ne!(src, edited);
    fs::write(&path, edited).unwrap();
}

#[test]
fn a_field_removed_from_a_zod_schema_adds_drift_like_a_cold_chain() {
    let exports = |repos: [&Path; 2]| -> Vec<String> {
        (repos.iter())
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let (web, _, catalog) = drift_chain("fresh_zod");
    age(web.parent().unwrap());
    let before = exports([&catalog, &web]);
    drop_a_schema_field(&catalog);

    let refreshed = exports([&catalog, &web]);

    let (cold_web, _, cold) = drift_chain("fresh_zod_cold");
    drop_a_schema_field(&cold);
    assert!(!before[1].contains("\"[].title\": \"inferred\""));
    assert!(refreshed[1].contains("\"[].title\": \"inferred\""));
    assert_eq!(refreshed, exports([&cold, &cold_web]));
}

/// The catalog's shared package drops `price` from its zod schema and `warehouse` from its
/// constant, which consumers in the web app read; the routes importing them are unchanged.
fn drop_shared_fields(catalog: &Path) {
    let path = catalog.join("packages/contracts/src/index.ts");
    let src = fs::read_to_string(&path).unwrap();
    let edited = (src.replace(", price: z.number()", ""))
        .replace("{ warehouse: \"w1\", reserved: 0 }", "{ reserved: 0 }");
    assert_ne!(src, edited);
    fs::write(&path, edited).unwrap();
}

#[test]
fn an_edited_shared_package_changes_its_importers_drift_like_a_cold_chain() {
    let exports = |repos: [&Path; 2]| -> Vec<String> {
        (repos.iter())
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let (web, gateway, catalog) = drift_chain("fresh_shared");
    age(web.parent().unwrap());
    let before = exports([&gateway, &web]);
    drop_shared_fields(&catalog);

    let refreshed = exports([&gateway, &web]);

    let (cold_web, cold_gateway, cold) = drift_chain("fresh_shared_cold");
    drop_shared_fields(&cold);
    for missing in ["\"price\": \"inferred\"", "\"warehouse\": \"inferred\""] {
        assert!(!before[1].contains(missing), "{missing}");
        assert!(refreshed[1].contains(missing), "{missing}");
    }
    assert_eq!(refreshed, exports([&cold_gateway, &cold_web]));
}

#[test]
fn a_package_that_gets_its_name_back_keeps_its_shapes_like_a_cold_chain() {
    let (web, _, catalog) = drift_chain("fresh_shared_name");
    let path = catalog.join("packages/contracts/package.json");
    let named = fs::read_to_string(&path).unwrap();
    let unnamed = named.replace("  \"name\": \"@chain/contracts\",\n", "");
    assert_ne!(named, unnamed);
    age(web.parent().unwrap());
    fs::write(&path, unnamed).unwrap();
    let without = Engine::open(&web).unwrap().export().unwrap();
    age(web.parent().unwrap());
    fs::write(&path, named).unwrap();

    let refreshed = Engine::open(&web).unwrap().export().unwrap();

    let (cold, _, _) = drift_chain("fresh_shared_name_cold");
    assert!(!without.contains("\"aisle\": \"inferred\""));
    assert!(refreshed.contains("\"aisle\": \"inferred\""));
    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
}

#[test]
fn edited_documents_relink_like_a_cold_chain() {
    let exports = |repos: [&Path; 2]| -> Vec<String> {
        repos
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    for (case, edit) in [
        ("fresh_documents", edit_documents as fn(&Path)),
        ("fresh_documents_listed", edit_listed_documents),
        ("fresh_documents_serve", drop_serve),
    ] {
        let (web, gateway, _) = documented_chain(case);
        age(gateway.parent().unwrap());
        let before = exports([&gateway, &web]);
        edit(&gateway);

        let refreshed = exports([&gateway, &web]);

        let (cold_web, cold, _) = documented_chain(&format!("{case}_cold"));
        edit(&cold);
        assert_ne!(refreshed[0], before[0], "{case}");
        assert_eq!(refreshed, exports([&cold, &cold_web]), "{case}");
    }
}
